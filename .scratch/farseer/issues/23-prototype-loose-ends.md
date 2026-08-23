# Three loose ends the prototype exposed

Type: grilling
Status: closed
Blocked by: none

## Question

Surfaced by [one-operator-turn.md](../prototypes/one-operator-turn.md) on 2026-08-23.

Three small, sharp questions that no closed ticket answers. Grouped because each is a paragraph, not a session.
The larger gap the same prototype found went to `22 Which cells may a manager call, and does an instruction route or delegate?`.

### 1. Does a budget narrow the way a ceiling does?

The prototype's cell call carried `budget: { wall_clock: 20m, cost_usd: 2.00 }`, and the operator typed nothing about money.

Two questions fall out.

**Where does a default budget come from** when the operator gives none - cell #0's definition, the callee's definition, or a global default?
`12` settled composition for autonomy ceilings and deny lists and said nothing about budget.

**And budget does not compose the way a ceiling does.**
`12` made a ceiling take the **minimum** down a chain, so nesting can only narrow.
A budget is a **quantity**, so two nested calls each carrying $2 can spend $4 unless a budget is also treated as narrowing.

That is almost certainly wrong, and the fix is probably that a callee's budget is capped by whatever remains of the caller's.
But "almost certainly" is how a runaway spend gets shipped, so decide it explicitly.

Note this interacts with `11`: **cost per successful run** is a headline metric, and if nested budgets can exceed their parent then the number the operator sees is not the number they authorised.

### 2. Is `edit` the same thing as takeover?

`16` gave the API **approve** and **reject** for a gated action.

The prototype's escalation offers the operator three choices - approve, **edit**, reject - and `edit` has no home.

It is neither approval nor rejection: it is the operator modifying the artifact and then approving their own version.

**That is very close to `07`'s takeover**, possibly identical: the operator intervenes in a run, `operator_intervened` is appended, and the result carries `operator_touched`.

If they are the same thing, two useful consequences:

- The API needs **no third verb**.
- The gated-action prompt and the **attach surface are the same surface** wearing two hats, which is either an elegant unification or a UI mistake waiting to happen.

If they are different, say how, because the difference will be invisible in the record otherwise.

### 3. A queued run that never starts has no outcome

In the prototype, `r_a2` sat in lifecycle `queued`, and the manager then decided a video was not worth making.

`05` gave the lifecycle axis `queued` -> `running` -> `finished(ok / failed / cancelled)`.
**There is no transition out of `queued` except into `running`.**

Neither terminal state fits:

- `cancelled` is wrong. `05` was explicit that `cancelled` means **a human decided not to**, and here the manager decided.
- `failed` is wrong. Nothing failed, and `05` said `failed` invites a retry.

Two candidate fixes:

1. **A fourth terminal outcome** - `abandoned`, meaning the manager decided it was unnecessary before it started.
2. **Delete the queued run** rather than finishing it - but that contradicts `02`'s append-only record if the run was ever written to it, and it hides a planning decision the operator might want to see.

Option 1 is more honest and costs one enum value. Option 2 is cheaper and loses information.

Note the prototype also observed that **a manager silently dropping a planned worker is a manager the operator stops trusting**, which argues for whichever option keeps the decision visible.

## Carried from 22

**Budget composition is now load-bearing.**

`22` refuses a cell call that re-enters a cell already on the call path, and section 5 of that resolution says why: until budgets are settled, **a cycle is not a design possibility, it is a spend bomb** - each hop re-entering with a fresh budget.

So question 1 below is not a tidiness question. It is what decides whether `22`'s cycle refusal is a **safety measure** or merely a **convenience**.

If a budget turns out to be capped by whatever remains of the caller's, then a cycle is bounded and the refusal could be revisited later.
If a budget does not narrow, the refusal must stay, and nesting depth itself becomes a spend risk worth a limit.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. A budget narrows, but it is drawn down rather than compared

**A ceiling is compared. A budget is drawn down.** That distinction is the answer to this question.

`12` made an autonomy ceiling a **level**: compute `min(entry_max, requested, callee_policy)` once at call time and the job is done, because a level is not consumed by being used.

A budget is a **quantity**. Checking it once is not enough: three sequential $2 calls under a $2 parent would still spend $6 if each is merely compared against the parent's original figure.

So:

**A cell call's effective budget is `min(requested, caller_remaining)`, and the callee's spend decrements the caller's remaining pool.**

The budget is a hierarchy of pools, not a hierarchy of limits.

#### Where a default comes from

Three layers, all narrowing:

1. **The definition of the cell where the task started.** It is that cell's money, and the task is that cell's to own per `22`'s delegate-never-route decision.
2. **The roster entry**, which may cap what any one callee gets - exactly parallel to the maximum `autonomy_ceiling` that `22` put on a callable-cell entry.
3. **The caller's remaining budget**, which is the hard limit and cannot be exceeded by any combination of the above.

#### Two consequences

**`11`'s headline metric becomes honest.** Cost per successful run now equals what the operator actually authorised, rather than a figure that nesting can inflate invisibly.

**`22`'s cycle refusal is downgraded from a safety measure to a convenience.** With budgets drawn down, a cycle is bounded - it exhausts the pool and stops rather than compounding.

**The refusal stays in v1 anyway.** A bounded infinite loop is still an infinite loop that burns a real budget to no purpose, and `22`'s check costs nothing since the call path is already in the chain. Revisit when recursive delegation has a use case, not because it became survivable.

### 2. `edit` is takeover. There is no third verb.

**Identical, and the API needs only `approve` and `reject`.**

Editing an artifact before approving it **is** the operator intervening in a run, which is `07`'s definition word for word.
Same events, unchanged: `operator_intervened` appended to the run, `operator_touched` on the result.

"Edit" decomposes into what already exists: take over, modify, release, approve.

#### The consequence

**The gated-action prompt and the attach surface are the same surface.**

One place where a human touches a run, one set of events, one record shape.

#### The risk, which is a UI risk rather than a design one

**If the interface presents them as two different things, the record will not distinguish them.**

Someone will later ask "was this edited, or was it taken over?" and there will be no answer, because both produced the same two events.

That is fine as long as they are never presented as different things.
It becomes a defect the moment a UI implies a distinction the record cannot support.

Recorded here so whoever builds the surface knows the constraint is on **presentation**, not on storage.

### 3. `abandoned` is a fourth terminal outcome

`05` gave the lifecycle axis `queued` -> `running` -> `finished(ok / failed / cancelled)`, with no exit from `queued` except into `running`.

**Add `abandoned`: the manager decided the run was unnecessary before it started.**

Each outcome carries an implication the operator reads directly:

| Outcome | Means |
| --- | --- |
| `ok` | it worked |
| `failed` | something broke, and a retry is appropriate |
| `cancelled` | **a human** decided not to |
| `abandoned` | **the manager** decided it was not needed |

`cancelled` was wrong because `05` was explicit that it means a human chose.
`failed` was wrong because nothing broke and it invites a retry that should not happen.

Cost: one enum value.

#### Why not delete the queued run

Deleting is cheaper and loses exactly the thing this ticket's own prototype flagged:

**A manager that silently drops a planned worker is a manager the operator stops trusting.**

It also fights `02`'s append-only record if the run was ever written to it, and it hides a planning decision that is genuinely interesting.

#### Consequence for `11`

`abandoned` runs are **excluded from the cost-per-successful-run denominator**, because they cost nothing and including them would flatter the metric.

But they are **visible as a planning signal**. A manager that abandons half of what it queues is planning badly, and that is worth seeing - arguably more worth seeing than a run that merely failed.

### Tickets this informs

- `05 run state model` - the lifecycle axis gains a fourth terminal outcome, **`abandoned`**.
- `11 analytics questions` - `abandoned` is excluded from the cost denominator and surfaced as a planning signal. Budgets now draw down, so nested cost equals authorised cost.
- `16 local API surface` - **no third verb.** `approve` and `reject` are sufficient, because `edit` is takeover, which `07` already exposes. The gated-action prompt and the attach surface are one surface, and must be presented as one.
- `22 cell addressing` - the cycle refusal is now a convenience rather than a safety measure. **It stays in v1 regardless**, because a bounded infinite loop still burns a real budget.
