# Hang detection, observation and takeover: prior art for farseer

Date: 2026-08-19

Resolves ticket 18.
Informs the numbers and mechanism left open by ticket 07 (`D:\Dev\farseer\.scratch\farseer\issues\07-attach-semantics.md`).

Method note: combines direct `gh`/WebFetch/WebSearch lookups with three background research agents covering orca+traycer, buzz+herdr+firstmate, and multica+cline+ACP+Temporal+Kubernetes+tmux/screen+Jupyter.
Every claim below carries a URL where one was found.
Claims sourced from a subagent's own tool calls (not independently re-verified by this session) are marked accordingly.
Where no source was found, the text says "not found" rather than guessing.

## Bottom line

The operator's hypothesis is confirmed in shape, killed in blame assignment.
Windows genuinely lacks POSIX process groups and signals, so a tool that assumes `kill -0` or SIGTERM-based supervision will break there, and that gap is real and inherent.
But every concrete herdr and firstmate Windows failure found in this research has a stated root cause that is an implementation choice, not a platform wall: unqualified `Start-Process` spawn resolution (herdr #2685), a NUL-terminator bug in the cwd passed to `CreateProcessW` (herdr #2904), a POSIX-first raw-byte attach path that a maintainer explicitly called "feature work rather than a broken established behavior" (herdr #2726), and a `kill -0` liveness check with no Windows fallback despite the correct fallback already existing elsewhere in the same codebase (firstmate #1508).
Buzz shows the same pattern on a different codebase: a missing `CREATE_NO_WINDOW` flag (buzz #2685) and an admitted, not-yet-built Job Object story (buzz #6047, open as of research).
None of herdr, firstmate, or buzz was found to use a Win32 Job Object anywhere.
Orca and Traycer, the two most mature orchestration platforms surveyed, both have real hang-watchdog code, but it watches their own Electron host process, not the agent worker; both have a documented, named gap between "we detect our own infra hanging" and "we detect the agent hanging."
Farseer should close exactly that gap: build the Windows-native replacements herdr/firstmate/buzz skipped, and build the worker-level hang detector that orca/traycer never built either.

Recommended mechanism: no-progress-event watchdog, fixed 120 second stalled-flag threshold and fixed 600 second likely-hung threshold, both configurable, calibrated against Kubernetes's `periodSeconds`/`failureThreshold` defaults and multica's per-runtime idle watchdogs (10 minutes for Codex/OpenCode).
Orphan reaping via a Win32 Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` per worker process tree, a mechanism named in BRIEF.md and confirmed absent from every Windows agent tool surveyed.
Detail and reasoning below.

## Comparison table

| Tool | Hang detection mechanism | Timeout numbers | Orphan handling | Observation surface | Replay of dead sessions | Takeover | Source |
|---|---|---|---|---|---|---|---|
| stablyai/orca | main-thread hang watchdog (own Electron process only, darwin-only); worker liveness is a connectivity verdict (`'live'\|'unverifiable'`), not a stdout/token timeout | heartbeat interval 2000ms, timeout 45000ms, check interval 5000ms, all fixed, overridable by env var; no timeout found for agent-worker hangs specifically | daemon persists independent of GUI client; explicit stale-daemon kill logic (3000ms wait, 1000ms SIGKILL confirm) but multiple open issues report leaked orphaned daemons, relays and PTYs on Windows | xterm-based PTY mirror, chunked disk-backed history | yes (inferred from file/component names: replay paints into the same terminal-pane component as live view; not confirmed by reading full component bodies) | yes, `orchestration.workerTerminalUserInput` fired only from real keystrokes, recorded durably server-side, debounced 30000ms | https://github.com/stablyai/orca/blob/main/src/main/hang-watchdog/hang-watchdog-worker-protocol.ts |
| traycerai/traycer | host-process health monitor (own Electron backend only), process-existence + WebSocket reachability probe; no agent-turn hang detector found | poll interval 15000ms, 2 consecutive failures to confirm down, 600000ms (10min) unreachable-warn threshold (diagnostic only, does not trigger restart), 120000ms re-probe throttle, all fixed | explicit lease/linger model for terminal sessions: lifecycle states `creating\|running\|exited\|lost\|reaped`; "lost" allows recovery inside a linger window, "reaped" is host-confirmed dead; numeric linger duration not found; known gap, no way to close/archive finished child agents (#818, #1061) | chat/turn-transcript stream for GUI agents, raw terminal mirror only in "Terminal Agent" mode | not confirmed either way | not found; closest is agent-to-agent handoff, not human takeover; users report being unable to tell if a closed tab even stops the process (#779) | https://github.com/traycerai/traycer/blob/main/clients/desktop/src/electron-main/host/host-health-monitor.ts |
| block/buzz | fixed hard ceiling per turn plus a separate idle timeout for new sessions; no progress-based liveness check (open feature request) | `max_turn` 7200s (2h), `DEFAULT_IDLE_TIMEOUT_SECS` 900 (15min), generic RPC `REQUEST_TIMEOUT` 60s, all fixed | pooled ACP subprocess workers by design, but confirmed leaks: unreaped subprocesses after turn completion (#4577), unbounded per-channel session growth causing Windows OOM-kill of the app's own WebView2 host (#2961), leaked process trees after cancel-drain timeout (#5849); Job Object adoption is an open, unresolved design issue (#6047) | channel/message-based (chat platform), not a terminal mirror | not found | not found; buzz's model is multi-agent chat, not single-operator handoff | https://github.com/block/buzz/issues/6066 |
| herdrdev/herdr | none found; no heartbeat, no idle-timeout, no watchdog; status is screen-scrape/regex per agent, known to misclassify legitimate waits as idle | `agent wait --timeout` exists as a caller-supplied CLI polling primitive, ignored `--timeout` entirely on Windows until fixed in 0.7.5 (#1505); no independent hang-detector timeout found | no lease/keepalive found; confirmed zombie-reaping bugs, mostly on Linux/headless, fixed by adding ordinary `wait()`/`waitpid()` calls the code was simply missing (#1360, historic) | terminal panes, own tmux-like multiplexer; scrollback broken on native Windows, shows only current viewport (#1948) | not found | foreground pane takeover exists but is fragile: a brief takeover by another agent permanently strips hook authority from the original agent with no recovery verb short of killing the process (#1033, #2703); no evidence intervention is logged as a discrete event | https://github.com/herdrdev/herdr |
| kunchenguid/firstmate | none found; liveness is PID-based (`fm_pid_alive`), not heartbeat/progress-based | `fm_lock_acquire_wait()` retries every 0.1s with no max retry count or timeout (#1508) | leaks confirmed: fleet-lock deadlock into zombie bash processes on Windows (#1508), orphan tmux window on refused worktree validation (#1913), failed bootstrap leaves an unsupervised pane (#1336) | delegates to tmux or herdr as backend; no independent observation surface | not found | not found | https://github.com/kunchenguid/firstmate |
| multica-ai/multica | daemon heartbeat to server plus per-runtime idle watchdogs, not a single global timeout | daemon heartbeat 15s, poll interval 3s, `MULTICA_AGENT_TIMEOUT` default 0 (uncapped, delegated to watchdogs), Codex semantic-inactivity watchdog 10min, OpenCode idle watchdog 10min, OpenClaw CLI prep call 30s, generic HTTP API 30s; server-side threshold for declaring a daemon dead after missed heartbeats not found | GC subsystem with per-artifact-class TTLs: orphaned task dirs 72h, done/cancelled issues 24h, completed-task artifacts 12h, repo cache 720h, Hermes memory 2160h, per-conversation transcripts 336h; explicit statement "a store a running task holds is never reclaimed" | not found (no dedicated UI/frontend doc reached) | not found | not found | https://raw.githubusercontent.com/multica-ai/multica/main/CLI_AND_DAEMON.md |
| cline/cline | no global hang heartbeat found; hardcoded per-call timeouts observed in issue reports, not a published spec | `execute_command` background/`run_commands` reported hardcoded at 30s (#13246, #8154, #11159, user complaints that it is too tight with no setting to adjust); source-confirmed constants `DEFAULT_TIMEOUT_MS` 30000 (account service), `OLLAMA_DEFAULT_TIMEOUT_MS` 300000 (5min); MCP server init timeout 1500ms (#12044) | `gh search issues` for "reconnect", "orphan", "crashed process" against cline/cline returned zero results; not found | not found | not found | https://github.com/cline/cline/issues/13246 |
| Zed Agent Client Protocol | none; only explicit cancellation, no heartbeat/keepalive/ping/lease concept anywhere in the fetched schema | n/a | n/a | `session/update` streaming notification during a prompt turn | not applicable, protocol-level only; no session-store/replay concept in ACP itself | `session/cancel` stops in-flight work and returns `StopReason::Cancelled`; `$/cancel_request` is the protocol-level equivalent; neither is human-takes-control-mid-turn | https://agentclientprotocol.com/protocol/schema |
| Temporal | activity heartbeat, checked against an explicit Heartbeat Timeout | Heartbeat Timeout has **no default, must be set explicitly**; Schedule-To-Start and Schedule-To-Close default to infinity; Start-To-Close defaults to the Schedule-To-Close value; on missed heartbeat, "the Activity Task fails and a retry occurs if a Retry Policy dictates it" | task-queue retry model, not a process-supervision model; not applicable | not applicable | not applicable | not applicable | https://docs.temporal.io/encyclopedia/detecting-activity-failures |
| Kubernetes | liveness/readiness/startup HTTP or exec probes, fixed polling interval | `initialDelaySeconds` 0, `periodSeconds` 10, `timeoutSeconds` 1, `successThreshold` 1, `failureThreshold` 3, all fixed defaults, all independently configurable per probe | liveness failure restarts the container per restart policy; readiness failure removes the pod from service endpoints without killing it | not applicable | not applicable | not applicable | https://kubernetes.io/docs/concepts/configuration/liveness-readiness-startup-probes/ |
| tmux / screen | none; event-driven, immune to SIGHUP by design, no timeout model at all | none | server process (not the terminal) owns sessions and their process trees, so closing a terminal does not orphan children the way a bare shell would; `destroy-unattached` (off by default) and `remain-on-exit` control cleanup, no numeric auto-kill timeout exists | raw terminal mirror inside the multiplexer pane | no; scrollback is raw terminal bytes in a buffer tied to the live server, not a structured, independently replayable record | yes, any attaching client gets full interactive control immediately, no observed/read-only mode exists | https://man.openbsd.org/tmux |
| Jupyter | ZMQ REQ/REP heartbeat channel, ping/pong, dedicated socket | no numeric interval or timeout value specified anywhere in the messaging spec, confirmed explicitly absent | on detected death via heartbeat inactivity, the client "simply send[s] a forceful process termination signal" | not applicable to farseer's worker model | not applicable | not applicable | https://jupyter-client.readthedocs.io/en/latest/messaging.html |

## 1. Hang detection

No tool surveyed has a working, documented detector of a hung *agent*, as opposed to a hung host process.

Orca and Traycer make this gap explicit and named.
Orca's only fixed-timeout watchdog (2000ms heartbeat, 45000ms timeout, 5000ms check interval, overridable via `ORCA_HANG_WATCHDOG_TIMEOUT_MS`) is scoped to its own Electron main thread and is darwin-only by an explicit `if (process.platform !== 'darwin') return null` guard.
Source: https://github.com/stablyai/orca/blob/main/src/main/hang-watchdog/hang-watchdog-worker-protocol.ts
Its agent-worker liveness signal instead is `getTerminalLivenessVerdict`, returning `'live' | 'unverifiable'`, a PTY-connectivity check, not a progress signal.
This gap surfaced in production: "Orca workers sometimes don't send worker_done, causing coordinator to hang forever," where the only timeout in play was a caller-supplied `--timeout-ms 300000` on the coordinator's own wait call, not an independent hang detector.
Source: https://github.com/stablyai/orca/issues/10673

Traycer's health monitor (15000ms poll, 2 consecutive failures to confirm down, 600000ms unreachable-warn threshold that is diagnostics-only and does not trigger a restart, 120000ms re-probe throttle) also watches its own host backend process via process-existence plus WebSocket reachability, not the agent turn itself.
Source: https://github.com/traycerai/traycer/blob/main/clients/desktop/src/electron-main/host/host-health-monitor.ts
A prior version of this exact monitor caused a false-positive incident, killing healthy hosts in a loop by treating a busy-but-alive endpoint as dead; fixed by adding a process-existence check before declaring death.
Source: https://github.com/traycerai/traycer/issues/740
This is a useful negative lesson for farseer: naive endpoint-unreachable-equals-dead logic produces false hangs under load.

herdr issue #2545 is the closest direct evidence of the exact failure mode farseer must avoid: an agent CLI running in a herdr pane on Windows shows zero progress for over an hour while the window is backgrounded, with the CLI's own elapsed-time counter still ticking, and no alert anywhere in herdr.
Source: https://github.com/herdrdev/herdr/issues/2545
Maintainer response asked for repro details rather than pointing at an existing watchdog, itself evidence herdr has none.

Two tools have real, working per-runtime idle watchdogs worth copying the shape of, not the exact numbers.
Multica delegates hang detection to per-agent-runtime watchdogs rather than one global timeout: `MULTICA_AGENT_TIMEOUT` defaults to 0 (uncapped), with Codex getting a 10-minute semantic-inactivity watchdog and OpenCode a 10-minute idle watchdog.
Source: https://raw.githubusercontent.com/multica-ai/multica/main/CLI_AND_DAEMON.md
Cline has an issue-confirmed, source-confirmed 30-second hardcoded timeout on background command execution, widely complained about as too tight with no way to adjust it.
Source: https://github.com/cline/cline/issues/13246, https://github.com/cline/cline/issues/11159
This is the negative lesson: a single fixed, non-configurable, too-short timeout applied uniformly to all tool calls generates constant false hangs.

Zed's Agent Client Protocol defines no heartbeat or liveness primitive at all.
It defines `session/update` (agent pushes progress notifications) and `session/cancel` / `$/cancel_request` (client asks agent to stop).
Neither is a hang detector; a hung agent that stops sending `session/update` is indistinguishable, at the protocol level, from one still silently computing.
Source: https://agentclientprotocol.com/protocol/schema

Temporal is the calibration point with a real, named mechanism and an explicit statement of no default: Heartbeat Timeout "must be explicitly configured," and on a missed heartbeat "the Activity Task fails and a retry occurs if a Retry Policy dictates it."
Source: https://docs.temporal.io/encyclopedia/detecting-activity-failures
This is fixed-and-explicit, not adaptive: one number, one clock, set by the author, no learning.

Kubernetes is the second calibration point and ships real defaults: `initialDelaySeconds` 0, `periodSeconds` 10, `timeoutSeconds` 1, `successThreshold` 1, `failureThreshold` 3, for liveness, readiness, and startup probes alike, all fixed and independently configurable.
Source: https://kubernetes.io/docs/concepts/configuration/liveness-readiness-startup-probes/
Kubernetes distinguishes liveness (probe failure kills and restarts the container) from readiness (probe failure only pulls the pod from the load balancer, no kill), a distinction with a direct farseer analogue: a stalled worker should first be flagged and demoted from "actively trusted," only killed if a human or the manager decides to, since cancel is a separate verb per ticket 07.

Jupyter's ZMQ heartbeat channel is architecturally the closest analogue to an agent-emitted heartbeat, but its own messaging spec documents no numeric interval or timeout, leaving detection thresholds implementation-defined per frontend.
Source: https://jupyter-client.readthedocs.io/en/latest/messaging.html
On detected death, the client "simply send[s] a forceful process termination signal."
Same source.

## 2. Auto-release and orphan reaping

No lease, keepalive, or ownership-timeout mechanism was found for the agent-worker layer specifically in herdr, firstmate, or buzz.

firstmate #1508 is the clearest root-caused orphan bug found this pass: on Windows under Git Bash (MSYS), `fm_pid_alive()` calls POSIX `kill -0`, which under MSYS "falsely reports any Windows PID as alive - even PIDs from dead or unrelated processes."
The reporter's own analysis states firstmate already has Windows-native PID checking elsewhere in the codebase (`_fm_is_windows`, wmic-based checks in `fm-session-lock-lib.sh`) but the liveness function used for the fleet lock does not call it.
`fm_lock_acquire_wait()` then retries every 0.1 seconds with no maximum retry count, timeout, or backoff, so a stale lock spins forever and each new session start adds another spinning zombie bash process.
Source: https://github.com/kunchenguid/firstmate/issues/1508
This is an implementation gap, since the correct Windows-aware check already existed in the same codebase, not a Windows platform limitation.

herdr's historical zombie-reaping bugs (#1360, cited by a research subagent, not independently re-verified this session) were fixed by adding ordinary `wait()`/`waitpid()` calls the code was simply missing, mostly on Linux/headless deployments, not confirmed on Windows specifically.
Buzz's orphan bugs are more numerous and better documented: unreaped ACP bridge subprocesses after turn completion (#4577), and a confirmed Windows-specific case where unbounded per-channel session growth (not a leaked-process-tree bug, the reporter explicitly ruled that out) consumed enough commit charge to trigger Windows to kill the app's own WebView2 host with no logged error.
Source: https://github.com/block/buzz/issues/2961
Buzz has an open, unresolved design issue for adopting Win32 Job Objects for its managed-agent process trees, meaning the maintainers themselves have identified the gap and not yet closed it.
Source: https://github.com/block/buzz/issues/6047

Orca is the most mature orphan story surveyed, and still leaks.
It has explicit stale-daemon kill logic (3000ms wait, 1000ms SIGKILL confirm) and an internally-computed `orphaned` flag on PTY sessions, but that flag was found to never surface in the CLI's human-readable output, making orphans invisible even though the runtime already tracks them.
Source: https://github.com/stablyai/orca/issues/15313
Separate open issues report orphaned daemon processes accumulating silently across app restarts/crashes, orphaned detached relays never being reaped after a failed `--connect`, and an SSH relay leaking orphaned PTY sessions until hitting a hardcoded cap of 50.
Sources: https://github.com/stablyai/orca/issues/12243, https://github.com/stablyai/orca/issues/8585, https://github.com/stablyai/orca/issues/9819

Traycer's terminal-session lifecycle model (`creating|running|exited|lost|reaped`) is the one genuine lease-like design found in this survey: "lost" means the stream dropped but the session may still be alive inside a linger window and the client should attempt recovery, "reaped" means the host confirmed via a `TERMINAL_NOT_FOUND` signal that the linger expired and the session is dead server-side, a definitive dead-end with no retry.
Source: https://github.com/traycerai/traycer/blob/main/clients/gui-app/src/stores/terminals/terminal-session-store.ts (cited by a research subagent; numeric linger duration was not found)
Even with this model, Traycer has an open gap: no way to close or archive a finished child agent, so completed sessions accumulate with no cleanup verb.
Sources: https://github.com/traycerai/traycer/issues/818, https://github.com/traycerai/traycer/issues/1061

Multica's GC subsystem is the most fully specified orphan-and-retention story found, with per-artifact-class TTLs (orphaned task directories 72h, done/cancelled issues 24h, completed-task artifacts 12h, repo cache 720h, Hermes memory 2160h, per-conversation transcripts 336h) and an explicit guarantee that "a store a running task holds is never reclaimed."
Source: https://raw.githubusercontent.com/multica-ai/multica/main/CLI_AND_DAEMON.md
This is retention/GC of finished-task artifacts, not live-process orphan reaping, but the pattern of named, per-class TTLs enforced by a background sweep rather than by the owning process promising to clean up is directly reusable.

Kubernetes and Temporal both encode auto-release/reaping as server-side state enforced by the supervisor, not by the disconnecting/failing party cleaning up after itself: Kubernetes restarts the container on liveness failure per the pod's restart policy, Temporal fails and retries the activity task per its retry policy.
This is the abstraction farseer's auto-release-on-disconnect requirement from ticket 07 should follow: release enforced by farseer's runtime on a timer, never by the disconnecting client sending a goodbye it might never get to send.

## 3. Observation surfaces

herdr, firstmate (via tmux/herdr backend), and Traycer's Terminal Agent mode all use a raw terminal mirror, the exact PTY-based approach ticket 07 decision 5 already rejected, and the direct cause of BRIEF.md's ConPTY-lifecycle problems.
Traycer's default GUI-agent surface is instead a chat/turn-transcript stream reconciled against queued/optimistic actions, closer to what farseer wants, but whether Traycer replays a finished session through the identical live-view component was not confirmed from the source reached.
Buzz's surface is channel/message-based (a chat platform), architecturally different from a terminal-supervision model and not directly comparable on replay semantics; no evidence of dead-session replay through the live UI was found.

Orca is the one tool this survey found credible file-level evidence for the exact replay-through-the-same-surface property ticket 07 requires: chunked, disk-backed terminal history (`terminal-history-manager-options.ts`, `terminal-history-seed-chunks.ts`, `terminal-history-session-tombstone.ts`) is painted into the same xterm pane component used for live panes (`terminal-snapshot-replay-paint.ts`, `replay-guard.ts`), per file and function naming.
Source: https://github.com/stablyai/orca/tree/main/src/renderer/src/components/terminal-pane
This is inferred from names and directory structure by the research subagent, not confirmed by reading full component bodies, so treat it as strong circumstantial evidence, not a verified fact.
It is nonetheless the strongest signal in this survey that "replay through the live surface" is achievable and has been attempted by at least one comparable tool, validating ticket 07's decision rather than inventing it from nothing.

tmux and screen, the classic detach/reattach baseline, do not replay dead sessions through the live surface at all: scrollback is raw terminal bytes in a buffer tied to one running server process, gone once that server exits, with no structured record and no replay path independent of the server.
Source: https://man.openbsd.org/tmux

Zed ACP's `session/update` stream is structured (message chunks, tool calls, execution plan updates), closer to farseer's rendered-event-stream model than any terminal mirror, but ACP is a live-session protocol between one client and one agent process; it has no session-store or replay concept of its own.
Replay would be a property of whatever stores the `session/update` stream, not of the protocol.
Source: https://agentclientprotocol.com/protocol/schema

## 4. Takeover

tmux and screen allow any attaching client full interactive control immediately, no observed/read-only mode exists, confirming ticket 07's rejection of "immediate interactive control" as the default is a deliberate improvement, not a reinvention.

Orca has the clearest first-class, event-recorded takeover mechanism found in this survey.
`worker-terminal-takeover-report.ts` fires an `orchestration.workerTerminalUserInput` RPC "only from the real-user-input signal (never xterm auto-replies, programmatic prompt delivery, resize, or output)," with a code comment stating the takeover "is recorded durably in the orchestration DB," flipping the resource to a `user_owned` state, debounced to one report per 30000ms so continued typing keeps re-affirming the takeover state rather than latching once.
Source: https://github.com/stablyai/orca/blob/main/src/renderer/src/lib/worker-terminal-takeover-report.ts
What this does to the coordinating agent's plan afterward was not found in the files the research subagent reached; would need deeper inspection of orca's orchestration DB layer to confirm.

herdr has takeover, but it is fragile and undirected rather than a clean, recorded event: a brief foreground takeover by another agent permanently strips "hook authority" from the still-running original agent, with no CLI recovery verb; only killing the process recovers, confirmed duplicated in production.
Sources: https://github.com/herdrdev/herdr/issues/1033, https://github.com/herdrdev/herdr/issues/2703
No evidence herdr logs intervention as a discrete, first-class event was found.

Zed ACP's `session/cancel` lets a client stop an agent's in-flight work, but this is cancellation, not takeover: the agent does not hand control to a human mid-turn, it aborts and returns `StopReason::Cancelled`.
No ACP concept of a human injecting input into a live turn, or of a supervising agent being notified of an intervening event, was found.
Source: https://agentclientprotocol.com/protocol/schema

Traycer and firstmate show no evidence of human takeover at all; Traycer users report being unable to tell whether closing a tab even stops the underlying process, let alone seize control of it.
Source: https://github.com/traycerai/traycer/issues/779
Buzz's chat-channel model has no dedicated takeover mechanism either; its closest analogue is agent-to-agent handoff, not human-to-agent.

No tool surveyed except orca records an intervention as a first-class event the way ticket 07 specifies (`operator_intervened`, `operator_touched`), and even orca's recorded event does not confirm what happens to the coordinator's plan afterward.
This is a genuine gap farseer would be filling almost entirely from scratch, not something to copy wholesale.

## 5. Windows specifics

Verdict: the operator's hypothesis is confirmed for blame, killed for framing.

The Windows platform gap is real: no POSIX process groups, no POSIX signals, `kill -0` does not mean what it means on Linux even under MSYS/Git Bash, and BRIEF.md's own catalog (`D:\Dev\farseer\BRIEF.md` section 12) already documents this correctly: "No POSIX signals. `pty.kill(signal)` throws on Windows in node-pty." and "Killing a parent leaves orphans. Correct pattern is a Win32 Job Object with kill-on-job-close."

But every concrete Windows failure found across herdr, firstmate, and buzz this pass has a stated, specific, non-platform root cause.

- herdr #2685 (`agent start --kind opencode` fails on Windows): the reporter states plainly "This is not specific to how herdr manages child processes... The bug is that herdr passes an unqualified name to `Start-Process` at all, instead of first resolving it to a concrete, natively-executable file," landing on a non-executable npm shim instead of `opencode.cmd`, and `CreateProcess` rejects it with `ERROR_BAD_EXE_FORMAT`.
  A second commenter confirms the identical failure mode for Codex.
  Source: https://github.com/herdrdev/herdr/issues/2685
  Matches BRIEF.md's own documented pattern: "Node CLIs resolve as `.cmd` shims... which need `shell: true`, `cmd /c`, cross-spawn, or explicit resolution."

- herdr #2904 (infinite spawn-retry loop, UI freeze): a trailing NUL byte gets appended to the cwd string passed to `CreateProcessW`, so every spawn fails with `ERROR_PATH_NOT_FOUND` and herdr retries in a tight loop instead of failing cleanly.
  A commenter clarifies the NUL is a UTF-16-terminator artifact of `portable-pty`'s error formatter leaking into the displayed path, a string-handling bug in herdr's pty layer, not a Windows kernel behavior.
  Source: https://github.com/herdrdev/herdr/issues/2904

- herdr #1445 (`shell_mode = "login"` silently launches `cmd.exe` instead of the configured shell, no error at all): Windows has no POSIX login-shell concept, and herdr's fallback path silently drops the user's config instead of surfacing an error.
  Implementation oversight in the fallback path, not a platform wall, since erroring loudly was always an option.

- herdr #1685 (`herdr not launchable over SSH on Windows`, `ERROR_UNTRUSTED_MOUNT_POINT`): herdr's install directory is a Windows junction/reparse point, and OpenSSH on Windows refuses to traverse untrusted mount points from a network-logon session, reproduced independently by a second reporter who confirmed the junction via `Get-Item`.
  Source: https://github.com/herdrdev/herdr/issues/1685
  Mixed case: the network-logon-token restriction on junction traversal is a genuine Windows/OpenSSH platform interaction, but it was triggered by herdr's own installer choosing a junction, and the community workaround (a non-reparse shim directory) shows it was avoidable.

- herdr #2726: a maintainer states directly that Windows attach is "explicitly unsupported in the Windows beta: the current attach path uses Unix raw-byte terminal input, while Windows needs a semantic input/event path... feature work rather than a broken established behavior."
  Cited by a research subagent, URL not independently re-fetched this session: https://github.com/herdrdev/herdr/issues/2726
  This is the single clearest maintainer statement in the whole survey that a Windows gap is an unbuilt feature, not a platform impossibility.

- firstmate #1508 (fleet lock deadlock, zombie bash processes): as detailed in section 2, a POSIX `kill -0` liveness check with no Windows-native fallback, despite the codebase already having the correct Windows-native check available elsewhere.
  Source: https://github.com/kunchenguid/firstmate/issues/1508

- firstmate #433 (WSL2 host-sleep false-negative watcher-dead declarations): `fm_pid_identity` fingerprints processes via `ps -o lstart=` (wall-clock), which drifts after the Windows host sleeps and resumes; the proposed fix is fingerprinting via `/proc/<pid>/stat` field 22 (boot-clock ticks) instead.
  Cited by a research subagent: https://github.com/kunchenguid/firstmate/issues/433
  Implementation bug, wrong clock source chosen, not a WSL2/Windows-inherent limitation, since boot-tick data was available the whole time.

- buzz #2685 (Windows-specific): discovery/auth probes and login-shell resolution spawn visible consoles because the spawn call omits `CREATE_NO_WINDOW`, a well-known Win32 process-creation flag.
  Cited by a research subagent: https://github.com/block/buzz/issues/2685
  Implementation oversight, not a platform limitation.

- buzz #6047 (open): "Security decision: platform FFI for managed-agent runtime (process identity, Job Objects)," an explicit maintainer acknowledgment that Job Objects are not yet adopted for buzz's managed-agent process trees.
  Cited by a research subagent: https://github.com/block/buzz/issues/6047
  This is the clearest direct confirmation in the survey that a mature, well-funded tool (28,393 stars) still has not built the Windows-native reaping primitive BRIEF.md already names as correct.

No use of Win32 Job Objects was confirmed anywhere in herdr, firstmate, buzz, or orca's reachable source or issues; buzz's own open design issue is the only direct textual confirmation that a Job Object story is missing, the rest is inferred from bug shape (manual retry loops, PID-polling, or leaked process trees instead of OS-enforced tree-kill), and that inference is marked as such, not stated as sourced fact.

Reading across every tool: the inherent Windows gap (no POSIX signals/process groups) is real, well-documented, and solvable with the right Win32 API, and not one bug found this pass traces to an unsolvable platform wall.
They trace to: unqualified process-name resolution, a string-formatting bug, a silently-dropped config fallback, an install-path choice colliding with OpenSSH's trust model, a POSIX-first input path a maintainer admits was never ported, a liveness check that ignored the codebase's own Windows-aware alternative, a wrong clock source, and a missing well-known process-creation flag.
Farseer avoids all of these by construction if it resolves executables explicitly before spawn, tests cwd/argument marshalling to `CreateProcessW` directly rather than through a leaky pty-abstraction library, installs to a plain non-junction path, and builds liveness/reaping around Win32 primitives (Job Objects, native process enumeration, boot-tick-based PID identity) from day one instead of porting POSIX assumptions.

## Recommended for Farseer

**Hang detection: no-progress-event watchdog, not a PTY-idle check, not an agent-emitted heartbeat.**
Track the timestamp of the last structured event a worker's adapter emits (tool call start, tool call result, or a partial-token/status event, whichever the seat's ACP adapter or terminal-adapter surfaces).
Compare against two fixed thresholds, both configurable:

- **120 seconds with no event: mark the run "stalled" (a new, visible state) and surface it to the operator.**
  Below Kubernetes's `periodSeconds` x `failureThreshold` window scaled up for the fact that LLM tool calls can legitimately run tens of seconds with no intermediate event; catches herdr-#2545-class silent stalls (which ran over an hour undetected) with wide margin while tolerating normal long tool calls.
- **600 seconds with no event: flag as likely-hung, do not auto-kill.**
  Matches the order of magnitude of multica's real, working 10-minute per-runtime idle watchdogs for Codex and OpenCode, the only working precedent for a per-turn agent-idle timeout found in this survey.
  Cancel stays a separate verb per ticket 07 section 7; the watchdog only changes visible state and can prompt the manager's next wake, it never kills a worker on its own.

Reject a pure wall-clock-since-start timeout (matches nothing surveyed and nothing in ticket 07), reject a single short fixed timeout applied uniformly to every tool call (cline's 30-second `execute_command` timeout is the negative example, widely and correctly complained about as too tight), and reject requiring the agent to emit an explicit heartbeat call (Temporal's model): farseer's workers are third-party CLIs it does not control the internals of, so an enforceable heartbeat contract is not available the way it is for code the workflow author wrote.

**Auto-release on detach-without-release: fixed 300 second grace period, enforced by the supervisor, not the client.**
Ticket 07 section 7 already decided this must exist.
300 seconds sits between herdr's effectively-infinite window (no timeout found anywhere in this survey) and the 600-second likely-hung threshold above, giving a human time to notice a dropped connection and reattach before losing control while bounding the worst case to five minutes of a worker sitting in "taken over" unattended.
Enforce it server-side, following Kubernetes's and Temporal's supervisor-enforced pattern rather than client-promised cleanup, exactly the model firstmate's unbounded lock-wait bug (#1508) and buzz's still-open Job Object gap (#6047) show the cost of getting wrong.

**Orphan reaping on Windows: one Win32 Job Object per worker process tree, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigned at spawn time.**
Matches BRIEF.md section 12's own conclusion and is the one mechanism this research found zero evidence of any surveyed Windows agent tool actually using; buzz's maintainers have an open issue admitting the same gap.
Pair it with explicit executable resolution before spawn (resolve `.cmd`/`.exe` to a concrete path, never hand a bare command name to `CreateProcess`) to close the herdr #2685/#2904 class of bug at the source, and a `taskkill /pid <pid> /T /F` fallback only for the case where farseer's own process dies before it could register the Job Object.

**Observation and replay: adopt orca's apparent pattern of one component rendering both live and historical data, not a separate replay viewer.**
This is the strongest external validation found for ticket 07 decision 1; treat orca's implementation as circumstantial evidence the approach is buildable, not as a design to copy line for line, since it was not independently verified beyond file/function naming.

## Deliberately reject

- **PTY/terminal-mirror as the live observation surface.**
  tmux, screen, herdr, firstmate, and Traycer's Terminal Agent mode all use it; exactly the surface ticket 07 decision 5 already rejected, and the direct cause of BRIEF.md's ConPTY-lifecycle and ANSI-reconstruction problems.
- **POSIX `kill -0` or any POSIX-signal-shaped liveness check, even under an MSYS/Git-Bash shim.**
  firstmate #1508 is a direct demonstration of this failing silently and dangerously on Windows.
- **Watching your own supervisor process instead of the worker.**
  Orca's and Traycer's only working watchdogs guard their own Electron host, not the agent; both have an open, named gap where a hung agent is invisible to the watchdog that exists.
  Farseer's watchdog must be scoped to the worker's event stream, not to the runtime process's own liveness.
- **Unbounded retry loops with no timeout or backoff as a stand-in for a lease.**
  firstmate's `fm_lock_acquire_wait()` (no max retries, no timeout, 0.1s spin) is the negative example; any lock/lease primitive in farseer must have a hard ceiling, full stop.
- **A single short fixed timeout applied uniformly to every tool call.**
  Cline's hardcoded 30-second `execute_command` timeout generates the exact false-hang complaints farseer exists to eliminate; per-class or two-tier (stalled/likely-hung) thresholds, as recommended above, avoid this.
- **Bare/unqualified process names passed to the OS spawn call.**
  herdr #2685 and #2904 both trace here in different ways; farseer must resolve to a concrete executable path before calling `CreateProcess`, never delegate resolution to `Start-Process`-style implicit lookup.
- **Silently dropping a configuration option instead of erroring.**
  herdr #1445's silent `shell_mode = "login"` fallback to `cmd.exe` is the negative example; any unsupported config on a given platform should fail loudly, not substitute silently.
- **Client-promised cleanup on disconnect, no server-side enforcement.**
  Nothing surveyed that relies on the disconnecting party to clean up survived contact with reality; Kubernetes and Temporal both put enforcement on the supervisor side, and ticket 07's auto-release-after-timeout decision already agrees.
- **Adaptive/learned hang thresholds for v1.**
  Not found in use anywhere in this survey; Kubernetes, Temporal, and Jupyter's heartbeat are all fixed-and-configurable, not adaptive, and there is no baseline usage data yet to adapt from.
- **Endpoint-unreachable-equals-dead without a process-existence check.**
  Traycer's own history: an earlier version of its health monitor killed healthy hosts in a loop by treating a busy endpoint as a dead one, fixed by adding a process-existence check first.
  Source: https://github.com/traycerai/traycer/issues/740

## Gaps in this research

- Every claim attributed to a background research agent was accepted as reported and cross-checked only where this session independently re-fetched the same URL; herdr #2726, firstmate #433, buzz #2685, buzz #6047, and orca's replay-component file structure were not independently re-verified by this session's own tool calls, and are flagged inline as subagent-sourced.
- Multica's server-side threshold for declaring a daemon dead after missed heartbeats (as opposed to the 15s send interval, which is documented) was not found.
- The numeric linger-window duration for Traycer's "lost" terminal-session state was not found, only the internal reference name ("T13").
- ACP's docs site was checked only via the schema page and home page, not exhaustively crawled; absence of a replay or takeover concept is "not found in pages checked," not proven absent from the whole site.
- Whether herdr, firstmate, buzz, Traycer, or cline can replay a dead session through the same UI as live viewing was mostly not confirmed either way; flagged unknown or not found rather than guessed, except for orca where circumstantial file-level evidence exists.
