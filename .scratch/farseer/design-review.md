# Design review, 2026-08-26

Written at the end of the day `29 harness protocol` and `30 codex app server` closed, from what building them actually hit.

Not decisions. Candidates for tickets, ranked by how much each one will cost if left, with the evidence that suggested it.

---

## Architecture

### 1. Runner knowledge is spread across four tables in three crates

The strongest structural finding, and the newest - it only became visible when the sixth runner arrived.

Four separate places now have to agree about what a runner is:

| where | what it knows |
|---|---|
| `start_worker`'s match | how to spawn it, which parser, which `Channel` |
| `ACP_RUNNERS` | which name means which executable plus subcommand |
| `control_of` | what farseer can do with it |
| the shell's `KNOWN` | what to call it in the menu |

Adding `codex-app-server` meant touching all four, and the settings menu already grew a test that exists **only** to catch two of them drifting apart.

Worth noting what is *right* about the current state: `29` deliberately made the menu read `ACP_RUNNERS` rather than repeat it, and that instinct is the answer generalised - **one table, everything else derived**.

The shape to consider: a `RunnerKind` record carrying name, executable, argv, parser, channel and control, with `start_worker` and the menu both reading it. A test that the table is the only place a runner name appears in a `match`.

### 2. `RunnerSignal` lives inside `claude_code.rs`

Eight modules do `use crate::claude_code::{ParseError, RunnerSignal}` - including `jsonrpc.rs`, which has nothing to do with Claude Code.

The shared vocabulary of every runner is defined inside one runner's module because that runner happened to be first. It reads as though ACP and Codex are speaking Claude Code's dialect, which is exactly backwards.

Cheap to fix (`signal.rs`, re-export for a release), and it gets cheaper the sooner it happens.

Same defect one level down: `UsageInfo` lives in `acp.rs` and `codex_app_server.rs` emits it.

### 3. `RunReport` is becoming a bag, and clippy keeps saying so

Three separate size fixes now: `window` boxed, `session` boxed, `windows` a `Vec` added beside `window`. Each was correct locally; together they are a shape asking to change.

The root is `ManagerError::Cancelled(RunReport)` - a whole report inside an error variant, so every field added to the report widens every `Result` the crate returns.

Two candidates: box the variant (`Cancelled(Box<RunReport>)`), or separate *what the run produced* from *what was observed about the machine while it ran*. The second is the one `27 quota accounting` would recognise: a window observation is not a property of a run.

### 4. The conversational channels have parallel bootstraps

`Channel::Acp` and `Channel::CodexAppServer` do the same four things in the same order - handshake, remember the session, send the goal, record what the runner said about itself - in two hand-written blocks inside `StartedWorker::bootstrap`.

`jsonrpc.rs` was extracted on the second protocol needing the request loop, per `08 generalization test`. This is the same argument one level up, and it now has two implementations rather than one.

### 5. `#[ignore]`d tests rot silently, and they are the only ones touching real runners

Proven today: the ACP live test asserted `RunnerSignal::Output`, which was true when written and false hours later. Nothing failed, because nothing ran it.

They cannot go in a default `cargo test` - they spend a real subscription. But "run the ignored sweep when a signal's shape changes" is currently a habit written in a ticket, which is the weakest possible enforcement.

Worth considering: a cheap fake ACP agent (a fixture process replaying a captured transcript) so the *wiring* is tested without spending anything, leaving the ignored tests to prove only what a fixture cannot - that the real binary still speaks what farseer parses.

---

## Surface

### 6. The conversation meta strip is out of room

It now carries cell, runner, model, provider, configured effort, session, context, tokens, cost and last run - **ten fields in one flex row**, inside a widget that also holds a thread and a composer.

`28 operator surface` added them one at a time and each addition was justified. Together they are a wall, and the two that matter during a run - context pressure and outcome - have the same visual weight as the session id.

Worth a pass: what does an operator need *while watching*, versus *when something looks wrong*. The second set could fold behind the first.

### 7. Quota's own rule is now harder to see than it was

The widget deliberately shows no progress bar, and its doc comment explains why at length. It now also shows a provider percentage, correctly - but a reader glancing at it sees a percentage and no bar, which looks like an omission rather than a decision.

The distinction is real and worth keeping. It may need to be visible in the surface rather than only in the comment.

### 8. The record has no view of a runner's caveats

`control_of` tells the operator what farseer cannot do with a runner **before** they pick it. Nothing says so afterwards: a run on `goose-acp` has no quota, and the fleet view shows the same empty column as a run on a runner that simply had nothing to report.

Absent-because-unreportable and absent-because-nothing-happened look identical, which is the exact confusion `10 runner inventory`'s observed-never-advertised rule exists to prevent.

---

## Already written down elsewhere, not repeated here

- `turn/steer` with `expectedTurnId`, which is a correction to `20 worker control channel` before it is a feature - on `30`.
- Whether `codex exec` stays now that the app-server runner exists - on `30`.
- Whether `12 autonomy and deny list` should have an opinion about a runner loading the operator's own hooks, two of which failed during the probe - on `30`.
- `26 routing policy` needs re-reading before it is wired, because the asymmetry it was designed around shrank - on `26`.
