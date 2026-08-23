# Which cells may a manager call, and does an instruction route or delegate?

Type: grilling
Status: closed
Blocked by: none

## Question

Surfaced by [one-operator-turn.md](../prototypes/one-operator-turn.md) on 2026-08-23.
The prototype found this by writing a single sentence that nothing on the map justifies.

Cell #0 says **"I'll hand this to the social cell."**
Nothing decides how it knows the social cell exists, that it is appropriate, or that it is allowed to call it.

### 1. Where do callable cells live?

`01` gave a cell a **roster** of workers and tools.
It never put **callable cells** in it.

Three candidates, none obviously right:

1. **A callable-cells list in the cell definition**, alongside the roster.
   Explicit and auditable. But every new cell means editing cell #0's definition, and the operator will forget.
2. **Every cell in the workspace is callable, and policy narrows it.**
   Convenient. But adding a cell then *silently widens* cell #0's reach, which is the failure mode `12` spent a whole ticket avoiding.
3. **A cell call is a tool grant.**
   Reuses `12`'s allowlist, which `12` found is the only real isolation v1 has, and inherits irreversibility levels for free.
   `21` already concluded a **foreign** peer cell is an `irreversible` tool, so making a **local** cell call a tool grant would be consistent.
   But it collides with `01`, which separated a **tool** (a call that returns or errors) from a **cell** (a manager that delegates). A cell call is supervised, has a run, and is cancellable - which is `01`'s definition of a worker, not a tool.

Option 3 is the tempting one and the collision is real. Resolve it rather than picking by feel.

### 2. Does an operator instruction route, or delegate?

The operator typed a social media request, with no cell named, into cell #0.

`01` made cell #0 the **default address, not the sole one**. It did not say what cell #0 does with work that belongs elsewhere.

Two different things:

- **Route** - the task moves to the social cell and cell #0 drops out. One task, one owner, ownership transfers.
- **Delegate** - the task stays in cell #0 and a cell call happens as a sub-run. One task, one owner, a nested run.

The prototype assumed **delegate**, because `06` made a cell call a run in the calling cell and `06`'s failure ownership follows from that.
But routing is a coherent alternative and nothing rules it out.

The distinction has teeth: under routing, `11`'s cost and intervention metrics attach to the social cell; under delegation they attach to cell #0 and the social cell's numbers are nested inside.

### 3. What this must not break

- `01` - a worker may never spawn. Whatever is decided, only a manager may call a cell.
- `06` - a cell call is a run in the calling cell, with the calling manager owning retry, timeout and escalation.
- `12` - autonomy ceilings and deny lists only ever narrow down a chain.
- `08` - **adding a field only some cells need reopens the falsification test.** If callable cells become a new top-level definition field, check that a cell with none is still coherent.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. A callable cell is a third roster entry kind

The collision this ticket recorded was real but resolvable, because it **conflated the grant mechanism with the supervision classification**.

- `12` made the **allowlist** the grant mechanism.
- `01` made **supervision** the classifier that separates a worker from a tool.

Those are independent. Something can be granted through the allowlist without being a tool.

**So a callable cell is a third entry kind in the roster, alongside workers and tools.**

That takes what is good from all three candidates and the cost of none:

- **No new field.** `roster` already exists, so `08`'s falsification test survives untouched. A cell whose roster contains no cell entries is perfectly coherent, which is the check `08` demands.
- **Explicit and auditable**, which was candidate 1's virtue.
- **Uses `12`'s allowlist**, which was candidate 3's virtue.
- **Without candidate 3's category error.** A cell stays a cell. It is supervised, it has a run, it is cancellable, and `01`'s discriminator still classifies it correctly.

Candidate 1's stated cost - every new cell means editing cell #0's definition, and the operator will forget - **is nil in v1.**
`01` deferred autonomous cell generation and v1 hand-writes the second definition, so the author is already editing files by hand when a cell appears.

Candidate 2 - everything in the workspace is callable, policy narrows - is rejected outright.
Adding a cell would then silently widen cell #0's reach, which is precisely the failure mode `12` spent a whole ticket avoiding.

### 2. An operator instruction delegates, never routes

**One task, one owner.**

The task stays in the cell the operator addressed, and a cell call happens as a nested run.

Three reasons:

- `06` already made a cell call **a run in the calling cell**, with the calling manager owning retry, timeout and escalation. Routing would contradict a closed decision rather than extend it.
- **Under routing, who tells the operator it is done?** The callee's manager, which the operator never addressed. One request would give the operator two conversation partners, and neither would own the whole thing.
- `11`'s metrics nest cleanly under delegation: cell #0's cost includes the callee's, and the callee can still be queried alone. Under routing, cost attribution goes ambiguous - cell #0 would show near-zero spend on a task it supposedly owned.

The cost is real and accepted: **cell #0's manager sits in the middle of every exchange**, paying latency and tokens to relay.

That is the right trade. A manager that relays is what makes cell #0 an **orchestrator** rather than a switchboard, and `01` made it the operator's address precisely so there would be one place to talk to.

### 3. An ungranted cell stays ungranted, even if the operator names it

If the operator says "have the social cell do X" and `social` is not in cell #0's roster, **the manager refuses and says so plainly.**

Otherwise the roster is advisory, and `12` established that the grant is the only real isolation v1 has. A mechanism that yields to conversational pressure is not a mechanism.

The fix is cheap and better than an override: edit the definition and `reload`, both of which `16` provides.
That takes about ten seconds and **leaves a git commit**, which an in-conversation override would not.

### 4. A cell entry carries a maximum autonomy ceiling

`12` gave every tool an irreversibility level. Cell entries need the equivalent, and it already exists.

**A callable-cell entry carries the maximum `autonomy_ceiling` the caller may grant it.**

That is how cell #0 expresses "the social cell may post, but never pay".
It reuses `06`'s ceiling rather than inventing a second policy dimension, and composition still only ever narrows: the effective ceiling is the minimum of the roster entry's maximum, whatever the caller passes, and the callee's own policy.

Per `21`, **a foreign peer cell entry is pinned at `irreversible` and cannot be lowered**, because nothing about that call is enforceable remotely and four of the eight cell-call fields are silently ignored.

### 5. Cycles are refused by the runtime

**A cell call that would re-enter a cell already on the call path is refused.**

Not a policy question. A runtime invariant, and a cheap one: the call path is already in the chain, so the check costs nothing.

The reason it is not merely tidy: `23 Three loose ends the prototype exposed` found that **budgets do not narrow the way ceilings do**.
Until that is settled, a cycle is not a design possibility, it is a spend bomb - each hop re-entering with a fresh budget.

Recursive delegation may be legitimate later. Not in v1, and **not before budget composition is decided**.

### What this cost the primitive

Nothing structural. One glossary widening and one new entry kind inside an existing field.

`08`'s test was the thing to watch here, and it held: a cell definition with no callable cells is coherent, and the coding cell and the social cell still differ only in roster, tools and policy values.

This was the last question on the map that could plausibly have added a field. It did not.

### Tickets this informs

- `14 vocabulary lock` - **`roster` widens** from "the workers and tools a cell may use" to "the workers, tools and callable cells a cell may use". Not a new noun, so `14`'s rule that a new noun needs a reason is not triggered.
- `23 prototype loose ends` - **budget composition is now load-bearing.** Section 5 refuses cycles specifically because budgets do not narrow. If `23` decides a budget is capped by whatever remains of the caller's, the cycle refusal becomes a convenience rather than a safety measure, and could be revisited.
- `13 harness build kit` - a cell definition's roster must be able to hold three entry kinds, and a callable-cell entry carries a maximum `autonomy_ceiling`.

## Amended by 23

The cycle refusal in section 5 above is now a **convenience rather than a safety measure**.

`23` made budgets **draw down** rather than be compared, so a cycle is bounded: it exhausts the pool and stops rather than compounding.

**The refusal stays in v1 regardless.**
A bounded infinite loop is still an infinite loop burning a real budget to no purpose, and the check costs nothing since the call path is already in the chain.
Revisit when recursive delegation has a use case, not because it became survivable.
