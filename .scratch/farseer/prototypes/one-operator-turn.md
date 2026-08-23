# Prototype: one operator turn, end to end

Written 2026-08-23 for `15 Prototype: one operator turn, end to end`.
On paper. No code. Every noun is from `14`'s glossary.

**Note on the premise.** The ticket says "the A2A call to the social media cell". That is stale: `06` decided local cells take the **in-process path** and never traverse the A2A endpoint, which is off by default. This is written as a **cell call** over the native path. The A2A variant is section 11.

---

## 0. The setup

- **cell #0** - the default cell. Manager only, no roster of its own beyond the ability to call other cells.
- **social cell** - a farseer cell. Manager, plus two worker kinds: `post-writer` and `video-editor`. Tools: `image-gen` (`reversible`), `post-to-x` (`undoable`), `schedule-post` (`undoable`).

Both are local. Both are cell definitions in git.

---

## 1. What the operator types

```
> post about the Job Object findings on social media
```

That is the whole input. No flags, no cell name, no budget.

**This creates a `task`** in cell #0. The task is the operator's whole request and it will span several runs.

---

## 2. What the manager says back

```
Cell #0

I'll hand this to the social cell. It has a post-writer and a video-editor.

Before I do - two things I'd guess at, tell me if I'm wrong:
  - audience: developer, technical
  - the interesting bit: killing the parent leaves 5 of 6 processes alive

Posting is gated, so nothing goes out without you approving the text.
```

### What it must not say

Per `BRIEF.md`, the manager surfaces intent and never mechanics. So none of this:

- ~~"Creating cell call `c_01H...` to `social`"~~
- ~~"Spawning worker with runner `acp:claude`, contract id `r_88...`"~~
- ~~"Job object created, 3 processes assigned"~~
- ~~"Autonomy ceiling clamped to `undoable`"~~

The operator can see all of that if they ask. It is in the record and on the fleet view. It is not conversation.

**The line: the manager reports what it is going to do and what will interrupt them. Never how.**

---

## 3. The cell call cell #0 writes

```
call_id            c_7f3a
from_cell          cell-0
to_cell            social
goal               "Write and publish a technical social post about the
                    farseer Job Object spike finding: killing a parent
                    process on Windows leaves 5 of 6 descendants alive.
                    Audience is developers. Include the measured numbers."
autonomy_ceiling   undoable
budget             { wall_clock: 20m, cost_usd: 2.00 }
definition_of_done "A post the operator has approved, published, with the
                    permalink returned."
deadline           2026-08-23T15:40:00Z
```

Note what is **not** here, per `06`: no workspace, no runner, no tool grants. The social cell owns those. **The caller states what it wants and what it will pay. The callee decides how.**

`autonomy_ceiling` is `undoable` rather than `irreversible`, so the social cell may draft and may post-with-approval, and may never do anything unrecoverable. Per `12`, ceilings only narrow, so nothing the social cell calls can exceed this.

**This cell call is a `run` in cell #0** - one record entry, its own `last_activity_at`, cancellable, attachable. Per `06`, failure ownership is cell #0's.

---

## 4. What the social cell's manager does

Receives the cell call. Writes two **worker contracts**.

```
run r_a1  (worker: post-writer)
  goal               "Draft a technical post from the spike finding below.
                      3 variants, differing in angle."
  workspace          D:\fw\wt014        (plain directory - no git, per 08)
  runner             acp:claude
  tool grants        [ read-file, write-file ]
  autonomy level     reversible
  budget             { wall_clock: 8m, cost_usd: 0.60 }
  definition_of_done "3 drafts in drafts/, each under 280 chars."
```

```
run r_a2  (worker: video-editor)   -- queued, not started
  goal               "Render a 20s clip from the terminal capture."
  workspace          D:\fw\wt015
  runner             proc:ffmpeg
  tool grants        [ ffmpeg ]
  autonomy level     reversible
  budget             { wall_clock: 10m }
  definition_of_done "out.mp4 exists, 20s +/- 2s, 1080p."
```

`r_a2` stays `queued` until the manager knows whether a video is wanted at all. Per `01`, zero workers at init is legal and workers spawn on demand.

---

## 5. What runs, and what the record gets

`r_a1` starts. Lifecycle `running`, control `autonomous`, liveness `live`.

Progress events, per `02`:

```
seq  kind             actor    payload
812  status_change    system   run r_a1 -> running
813  memory_consulted worker   m_44 "keep technical posts under 3 sentences"
814  tool_call        worker   read-file spike-notes.md
815  tool_result      worker   ok, 4.1 KB
816  tool_call        worker   write-file drafts/1.txt
817  tool_result      worker   ok
...
824  status_change    system   run r_a1 -> finished(ok)
```

Activity - token deltas from the runner - is **not** in this list. It only bumps `last_activity_at`. Per `05`.

---

## 6. The escalation

The manager picks a draft and wants to publish. `post-to-x` is declared `undoable`, so per `12` it is **gated by default**.

The run does not fail and does not continue. Per `21`, this is the shape A2A calls `INPUT_REQUIRED` and ACP calls a permission request: an **interrupted, not terminal** state.

What the operator sees:

```
Cell #0

The social cell wants to publish this:

  "Killed a parent process on Windows. 5 of its 6 descendants kept
   running - every node process in the tree, plus the shell. None of
   them noticed. Job Objects reap all 6 in 340 microseconds.
   This is why orphan storms happen."

  278 chars. No video - the terminal capture was too short to be worth it.

  [approve]  [edit]  [reject]
```

**What the manager swallowed**: the two rejected drafts, the memory it consulted, the `ffmpeg` run it decided not to start, every tool call, the budget spend so far.

**What it surfaced**: the artifact, one judgement call it made unprompted ("no video, and why"), and the decision the operator has to make.

That second one matters. **A manager that silently drops a planned worker is a manager the operator stops trusting.** Surfacing the *decision* without the *mechanics* is the whole skill.

---

## 7. If the operator attaches mid-run

`farseer attach r_a1`, or clicking the run in a fleet view. Per `07` and `16`, this is a stream over the run with a cursor, and it replays before going live.

```
run r_a1 - post-writer - social
lifecycle running · control observed · liveness live · 4m12s

  14:02:31  consulted memory m_44
  14:02:33  read  spike-notes.md
  14:02:41  wrote drafts/1.txt
  14:02:58  wrote drafts/2.txt
  14:03:20  wrote drafts/3.txt
  14:03:22  ...

  [take over]  [cancel]
```

Read-only by default, per `07`. Control shows `observed`, and the liveness clock **keeps running** because the agent is still driving. Take over and it becomes `taken over`, the clock pauses, and releasing appends `operator_intervened` with `operator_touched` on the result.

Replay of a finished run is the identical view with a different cursor. Per `16`, same call.

---

## 8. What comes back

```
cell call c_7f3a -> finished(ok)
  permalink        https://x.com/.../1234567890
  operator_touched true          (approval counts, per 07)
  cost_usd         1.14
  runs             2  (r_a1 ok, r_a2 never started)
```

Cell #0's manager to the operator:

```
Cell #0

Posted. https://x.com/.../1234567890
$1.14, about 6 minutes.
```

---

## 9. Gaps this prototype found

Writing it out surfaced five things no closed ticket decides.

### 9.1 How does a manager know which cells it may call?

**The largest gap.**

Cell #0 said "I'll hand this to the social cell". How does it know the social cell exists, that it is appropriate, and that it is allowed to call it?

`01` gave a cell a **roster** of workers and tools. Nothing put *callable cells* in it.

Three candidates, none decided:

1. A **callable-cells list** in the cell definition, alongside the roster. Explicit, but every new cell means editing cell #0.
2. Any cell in the workspace is callable, and **policy narrows it**. Convenient, and it means adding a cell silently widens cell #0's reach.
3. Cells are **tools**. A cell call becomes a tool grant, which reuses `12`'s allowlist and gets irreversibility levels for free.

Option 3 is tempting because `12` already made tool grants the only real isolation, and `21` already concluded a foreign peer cell is an `irreversible` tool. Making a *local* cell call a tool grant too would be consistent. But it collides with `01`, which separated tools (a call that returns) from cells (a manager that delegates).

**This needs a ticket.**

### 9.2 Which cell does an operator instruction go to?

The operator typed a social media request with no cell named, into cell #0.

`01` said cell #0 is the default, not the sole address. But nothing says whether cell #0 **routes** an instruction to another cell, or **calls** another cell as part of doing the work itself.

They are different: routing means the task moves and cell #0 drops out; calling means the task stays in cell #0 and a sub-run happens. This prototype assumed calling. Nothing decided it.

### 9.3 Where does a budget come from when the operator does not give one?

The cell call above has `budget: { wall_clock: 20m, cost_usd: 2.00 }`. The operator typed nothing about money.

So it came from a default - but from **which** default? Cell #0's definition, the social cell's definition, or a global one? `12` settled policy composition for autonomy and deny lists and said nothing about budget.

Note budget does **not** compose like a ceiling: two nested calls each with a $2 budget can spend $4, unless a budget is also a narrowing quantity. **Probably it should be, and nothing says so.**

### 9.4 Approve, edit, reject - `edit` has no home

`16` listed "approve or reject a gated action" as an API operation. The mock above offers three buttons.

**`edit` is a third thing.** It is not approval and not rejection: it is the operator modifying the artifact and then approving their own version.

That is very close to `07`'s **takeover**, and possibly identical - the operator intervening in a run, producing `operator_intervened` and `operator_touched`. If so, `edit` is not a new verb and the API needs no third operation.

Worth confirming rather than assuming, because if `edit` is takeover then the gated-action prompt and the attach surface are the same surface wearing two hats.

### 9.5 A queued run that never starts has no outcome

`r_a2` was `queued`, and the manager decided against a video. What is its lifecycle now?

`05` gave `queued -> running -> finished(ok / failed / cancelled)`. There is no transition from `queued` to anything except `running`.

`cancelled` is wrong - `05` said `cancelled` means a human decided not to, and here the manager decided.
`failed` is wrong - nothing failed.

Either `05` needs a fourth terminal outcome such as `abandoned`, or a queued run the manager drops is simply **deleted rather than finished** - which contradicts `02`'s append-only record if it was ever written.

Small, and exactly the kind of thing this prototype exists to find.

---

## 10. What the prototype confirmed

Not everything it found was a gap. Three things worked exactly as designed:

- **The caller/callee split from `06` is legible on paper.** The cell call reads as a brief, not as a configuration, and the absence of workspace and runner fields is a feature you can feel.
- **`08`'s artifact concept survives a real non-coding example.** The post is a text artifact with no base, the video would have been a binary artifact, and neither needed a review mode.
- **The gated action is a state, not an error.** `21` found A2A's `INPUT_REQUIRED` is interrupted rather than terminal, and writing the escalation out confirms that is the right shape - the run is not failed, it is waiting.

---

## 11. The A2A variant, for contrast

If the social harness were a **foreign orchestrator** rather than a farseer cell, `06` makes it a **peer cell** over A2A, and everything changes:

- `21` found four of the eight cell-call fields have no native A2A home. `autonomy_ceiling`, `budget`, `definition_of_done` and `deadline` would be **silently ignored**.
- So per `12`, the call is an **`irreversible` tool**: gated by default, never lowerable, deadline enforced locally by cancelling.
- Per `07`, the peer cell is **observable but not attachable**. Section 7's attach view would not exist.
- Per `21`, no cursored replay either - a snapshot plus live, and nothing recoverable from while disconnected.

The same operator turn, one protocol boundary away, loses attach, loses replay, and loses every bound the caller tried to set.

**Worth showing the operator that contrast, because it is the strongest argument for keeping the social cell a farseer cell rather than a foreign harness.**
