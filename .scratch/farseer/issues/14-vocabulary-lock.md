# Vocabulary and naming lock

Type: grilling
Status: closed
Blocked by: none

## Question

Cheap now, expensive once code exists. Lock the words.

- Is "cell" the right word? Alternatives: crew, team, harness, division, department. The corporate metaphor may argue for one of the latter.
- Manager, CEO, or captain? `BRIEF.md` inherited firstmate's captain-and-first-mate framing, which now collides with the owner-and-CEO framing.
- Chronicler and Librarian: keep, or plainer names?
- Worker, crewmate, or staff?
- Run versus session versus task versus contract. Four different things that must never be used interchangeably.
- Confirm the product name "farseer" and the CLI verb: `farseer`, `fsr`, or something else. `fs` collides with too much.

Produce a glossary every later document uses.

## Carried from earlier tickets

Terms already fixed by resolved tickets. This ticket confirms the final words, but these distinctions are decided, not open.

From `01 Is the cell the right primitive?`:

- **cell definition** versus **running cell**. Two nouns, different lifecycles. Not interchangeable.
- **manager**, **worker**, **tool**, **runner**, **peer cell**, **roster**, **record scope**.
- Worker versus tool is decided by supervision, not by whether an LLM is involved.

From `07 Attach: to a worker or to a run, and what does intervention do?`:

- **run** is one worker contract's execution, and exactly one record entry.
- **task** is the operator's whole request, spanning many runs. It needs a word distinct from run, and "task" is provisional.
- **attach** targets a run, never a process.
- Run control states: **autonomous**, **observed**, **taken over**.
- Record events: **operator_intervened**, and the result flag **operator_touched**.

That leaves the map's original four-way ambiguity partly resolved: run and task are now distinct and defined, contract is defined by `05 worker control contract`, and **session** is still unclaimed.
Decide whether session survives at all, or whether "the manager's long-lived agent session" is the only legitimate use of the word.

## Carried from 05

Terms fixed by `05 Run state model and control semantics`:

- **activity** vs **progress** - two different signals, deliberately not synonyms. Activity drives the watchdog, progress drives the record.
- **contract** and **envelope** - the contract is the envelope (goal, workspace, runner, tool grants, autonomy level, budget, definition-of-done), immutable for the life of the run.
- **steer** vs **re-scope** - steering moves within the envelope and keeps the run; re-scoping changes the envelope and starts a new one.
- **orphaned** - a workspace that outlived its run and could not be deleted.
- Three axis names: **lifecycle**, **control**, **liveness**.

## Carried from 08

**`runner` is the one word known to be wrong.**

`01` introduced runners as foreign agents over ACP. `08` then established that a six-minute `ffmpeg` render is a **worker**, and its contract still needs a `runner`.
So a runner is **anything that satisfies the worker control channel contract** - emit activity, emit the three progress kinds, accept cancellation - and an ACP agent is only one thing that can.

The word carries "an agent sits here", and that connotation will mislead every future reader into thinking a non-agent worker needs some other slot.
Decide: redefine loudly, or rename.

Also fixed by `08` and needing a home here: **artifact**, **irreversible tool**, **trigger** (time-triggered cron and event-triggered hook, both API clients).

## Resolution

Resolved 2026-08-23 by grilling.

**This glossary is authoritative. Every later document uses these words.**

### The glossary

#### Structure

| Term | Meaning |
| --- | --- |
| **farseer** | the product. Lowercase. |
| **cell** | the unit of addressing plus policy plus record scope. Cells contain cells. |
| **cell definition** | data in git. What a cell *is*. |
| **running cell** | a live manager plus in-flight workers. What a cell is *doing*. Never interchangeable with the definition. |
| **manager** | the one mandatory inhabitant of a cell. Exactly one, at every depth. |
| **worker** | a supervised unit of work. Needs a contract, a budget and a record entry. May be cancelled, retried, escalated, attached to. |
| **tool** | a call that returns or errors. Not supervised. |
| **runner** | anything that satisfies the worker control channel contract. **Renamed from `seat`.** |
| **peer cell** | a foreign orchestrator, reached over A2A. |
| **roster** | the workers and tools a cell may use. |

Worker versus tool is decided by **supervision**, never by whether an LLM is involved.

#### Work

| Term | Meaning |
| --- | --- |
| **task** | the operator's whole request. Spans many runs. No longer provisional. |
| **run** | one worker contract's execution. Exactly one record entry. |
| **worker contract** | goal, workspace, runner, tool grants, autonomy level, budget, definition-of-done. Immutable for the life of the run. |
| **cell call** | `call_id`, `from_cell`, `to_cell`, `goal`, `autonomy_ceiling`, `budget`, `definition_of_done`, `deadline`. Never names workspace, runner or tool grants - the callee owns those. |
| **session** | a runner's own conversation with its model. **Owned by the harness, never coined by farseer.** |
| **artifact** | the general unit of reviewable change. A diff is how a text artifact is presented. |

#### Verbs

| Term | Meaning |
| --- | --- |
| **steer** | same run, same contract, new instruction. Delivered at the next **turn boundary**, per `20`. |
| **re-scope** | a contract field changed, so a new run against the same task. |
| **cancel** | end the run. Never the same as takeover. |
| **re-run** | new run, fresh workspace. |
| **attach** | targets a **run**, never a process. |

#### States

Three axes, never one enum.

| Axis | Values |
| --- | --- |
| **lifecycle** | `queued`, `running`, `finished(ok / failed / cancelled)` |
| **control** | `autonomous`, `observed`, `taken over` |
| **liveness** | `live`, `stalled`, `likely-hung`. **Derived, never stored.** |

Plus **orphaned**, which is a workspace that outlived its run and could not be deleted. Not a run state.

#### Signals

| Term | Meaning |
| --- | --- |
| **activity** | any bytes from the runner. Drives the **watchdog**. Never recorded. |
| **progress** | tool call start, tool result, status change. Drives the **record**. |
| **event** | written by the runtime, from what it observed. |
| **memory** | written by agents, as claims. Three tiers: global, cell-local, run-local. |
| **attachment** | out of band, unscrubbed, referenced by pointer only. |
| **trigger** | an API client that starts work. Time-triggered (cron) or event-triggered (hook). Never a subsystem. |
| **irreversible tool** | a tool whose effect cannot be undone. Drives autonomy gating. |

Record flags: `operator_intervened`, `operator_touched`, `manager_steered`, `context_compacted`, `memory_consulted`.

#### Product

- Canonical CLI: **`farseer`**
- Alias: **`fsr`**

Not `fs`, which collides with filesystem tooling on every platform.

### The decisions, and why

#### 1. `seat` is renamed to `runner`

`08` found the word wrong: a six-minute `ffmpeg` render is a worker, its contract needs a `seat`, and nothing sits there.

**`runner`** means the thing that runs a contract. An ACP agent runs one, an `ffmpeg` adapter runs one.
It is already the industry word - CI runners, Actions runners - so it imports the right intuition: **a slot that executes work, agnostic to what is inside.**

Renaming cost: a find-and-replace across planning documents, and **zero in code, because there is no code**.
This was the cheapest moment the rename would ever have.

#### 2. `cell` stays

Three reasons, the first decisive:

- **`harness` is already taken** by the thing farseer orchestrates. Using it for farseer's own unit would be a direct collision.
- `team` and `department` carry org-chart baggage implying fixed hierarchy. **Cells contain cells**, and the recursion is the point.
- It is load-bearing across fifteen closed tickets.

#### 3. `manager` stays, and the reason generalises

**The corporate metaphor is a good mental model and a bad vocabulary.**

"CEO" only makes sense for cell #0; a social media cell's manager is not a CEO.
"Captain" collides with firstmate, the thing being replaced.
"Manager" is boring, accurate and **scale-free**: every cell has exactly one, at every depth, with no implied seniority between cells.

The metaphor helps the operator think. Encoding it in the nouns would import a hierarchy the recursive model does not have.

#### 4. Chronicler and Librarian are cut

**They are not runtime concepts.**

`02` settled how the record works: the runtime writes **events**, agents write **memory**, and the MCP face exposes query plus memory-write.
There is no chronicler process and no librarian process.
Naming them implies daemons that do not exist and that `02` deliberately did not create.

If the operator wants them, they are **workers in cell #0 with a goal** - ordinary workers, not vocabulary.

#### 5. `session` survives, narrowed to one meaning

**A session is a runner's own conversation with its model, owned by the harness.**

The word is already taken by every dependency: ACP has `session/new` and `session/prompt`, Claude Code has `session_id`, Codex has threads and sessions.
`02` already used it this way in "the log is not session history".

Farseer coining a second meaning for a word its own dependencies use would be the most expensive naming mistake available.

#### 6. `envelope` is retired as a noun

Caught while assembling this glossary, and it is exactly the ambiguity this ticket exists to prevent, arriving late.

- `05` said "a contract is an immutable **envelope**" - goal, workspace, runner, tool grants, autonomy, budget, definition-of-done.
- `06` said "a typed internal **envelope**" - `call_id`, `from_cell`, `to_cell`, `goal`, `autonomy_ceiling`, `budget`, `definition_of_done`, `deadline`.

Overlapping fields, different scopes, one word.

**Two precise words replace it: `worker contract` and `cell call`.**

`06`'s own distinction survives and reads better: a worker contract names the workspace, runner and tool grants; a cell call must not, because the callee owns those.

`envelope` was doing metaphor duty rather than naming duty, so nothing is lost by dropping it.

### How the rename was applied, and why the two differ

**`seat` to `runner` was applied globally** across every ticket and the map. It is a one-to-one token substitution with no semantic risk.

**`envelope` was not.** The same token maps to two different targets depending on context, and three occurrences are quoting `ARCHITECTURE.md`'s **rejected** proposal, where the old wording is correct as a quotation.

So instead: open tickets were fixed in place, and `05` and `06` carry a **"Renamed by 14"** note pointing at the final words.
Their resolution prose is left as written, because rewriting a decided resolution risks changing what it decided.

**This glossary is authoritative where it disagrees with older prose.**

Research notes under `research/` written before 2026-08-23 and the spike code under `spikes/` are dated artefacts and were not rewritten. `hang-detection-and-attach.md` and the `storebench` schema still say `seat`.

### Tickets this informs

- `10 runner inventory` - retitled by this ticket. The question is no longer "which agents do we support" but "what does it take to be a runner, and which things already are".
- `13 harness build kit` - the kit's output is described in these words, and a kit that introduces a new noun has reopened this ticket.
- Every open ticket - this glossary is the vocabulary. A new word needs a reason.

## Widened by 22

`22 Which cells may a manager call, and does an instruction route or delegate?` widened one glossary entry.

**`roster`** - from "the workers and tools a cell may use" to **"the workers, tools and callable cells a cell may use"**.

A callable cell is a **third entry kind** in the roster, not a tool.
`22` found the apparent collision - a cell call is supervised, so by `01`'s discriminator it is worker-shaped rather than tool-shaped - dissolves once the **grant mechanism** and the **supervision classification** are treated as independent.

Not a new noun, so this ticket's rule that a new noun needs a reason is not triggered.
A callable-cell entry carries a maximum `autonomy_ceiling`, reusing `06`'s ceiling rather than adding a policy dimension.
