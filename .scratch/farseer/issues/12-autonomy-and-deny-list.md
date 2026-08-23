# Autonomy grants, the deny list, and the delivery gate

Type: grilling
Status: closed
Blocked by: none

## Question

There is no OS sandbox on native Windows, so policy is the only isolation v1 has.
That makes this load-bearing rather than administrative.

- Default autonomy: may a worker commit and push to a branch without asking, or does every write stop at a diff?
- Who merges: never farseer, farseer per project policy, or farseer after CI is green?
- Draft the deny list of things that must never happen unattended. Starting set: force push, history rewrite, secret files, package publish, anything touching `.env`, migrations against a real database.
- Note that without a sandbox, deny rules only bind built-in tools. A shell command routes around a file-read deny rule.
- Is the existing `no-mistakes` skill the delivery gate, or does farseer define its own?
- Does a non-coding cell posting publicly need a stricter default than a coding cell writing a diff?

## Carried from 06

`06 Cell-to-cell transport` introduced **`autonomy_ceiling`** and the rule that goes with it:

**A caller may cap a callee, never raise it above the callee's own policy.**

A ceiling composes safely under nesting; an absolute value does not.
Cell A calls cell B calls cell C, and each hop can only narrow.
So the effective autonomy at any depth is the minimum of every ceiling on the path and the local policy.

This ticket owns what happens at the edges of that rule.
Specifically: does a deny list compose the same way (union of denials down the chain), and can a callee refuse a call whose ceiling is below what the work needs, rather than accepting it and failing later?

## Carried from 02

The **deny list** governs what a worker may do.
**Scrubbing** governs what the record may keep.

Related, but not the same list, and they must not be merged.
A worker may legitimately read a secret it is allowed to use; the record must still never store it.

## Carried from 08

**Irreversibility is a policy dimension this ticket does not currently have.**

Every authority farseer has granted so far is over local files, which is reversible.
A published post is not.
That asymmetry is what a social media cell introduces, and it is a policy value rather than a field.

Proposed shape: **tools declare whether they are irreversible, autonomy policy gates on that, default gated, overridable per cell.**
The machinery already exists - `01` has gated actions and `16` mapped them onto ACP's permission flow - so this is a new input to existing machinery rather than new machinery.

Open here: is irreversibility binary, or does it need degrees (a post can be deleted, a payment cannot)?

## Carried from 21

**A foreign A2A callee is unbounded by construction.**

`21` mapped `06`'s cell call onto A2A field by field and found **four of eight fields have no native A2A home**: `autonomy_ceiling`, `budget`, `definition_of_done` and `deadline`.
They can ride as structured data in a message part, but **a non-farseer A2A agent will silently ignore all four.**

So enforcement cannot live in the callee. It must live in the **calling cell's run**, which `06` already established is where a cell call lives.

That makes this a policy question this ticket owns, not an implementation detail:

- Is calling a foreign peer cell itself a **gated action**, given nothing about the call can be enforced remotely?
- Does an unbounded callee need a **wall-clock deadline enforced locally by cancelling the call**, since the callee will not observe the one it was sent?
- Does the **irreversibility dimension** from `08` compose across a cell boundary, when the callee's own tool grants are invisible to the caller?

## Carried from 17

**Purge is the only irreversible verb farseer owns.**

`08` made **irreversibility** a policy dimension for tools: tools declare it, autonomy policy gates on it, default gated.
Purge is that same dimension pointed **inward**, at farseer's own record.

So it should be gated with at least the force of an irreversible tool, and this ticket owns saying how.

Open here: is purge operator-only, or may a manager ever call it?
A cell that can purge its own record can destroy the evidence of what it did, which is the one thing `02` built the record to prevent when it refused agents a raw event-append path.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. The deny list prevents mistakes, not attacks

The ticket half-stated this and it turns out to be the whole answer: without a sandbox, a shell command routes around a file-read deny rule. `deny read .env` is defeated by `cat .env`.

**So the deny list is not a security boundary, and no document may imply that it is.**

It stops a worker that did not intend harm. It does nothing against one that is compromised or adversarial.
Writing that down is load-bearing, because a policy mechanism silently trusted as isolation is worse than no mechanism at all.

**Which relocates the real decision: without a sandbox, grant lists beat deny lists.**

The primary control is **what tools a worker is granted**, which `05` already put on the worker contract as `tool grants`.
The deny list is a second, weaker layer that catches accidents cheaply.

And the rule that follows:

**If shell is granted, everything is granted.**

A cell whose roster includes a shell has explicitly accepted that its deny list is advisory.
That is a legitimate choice - a coding cell without a shell is not much of a coding cell - but it must be a stated choice rather than an assumed one.

This also settles the ticket's framing that "policy is the only isolation v1 has".
It is more precise to say **the tool grant is the only isolation v1 has**, and policy is what shapes the grant.

### 2. The workspace boundary is the autonomy boundary

**A worker may write and commit freely inside its own workspace. It may never push, merge, or touch the operator's branches.**

This reuses `04`'s isolation as the policy line rather than inventing one.
Writes inside an isolated worktree are fully reversible: delete the workspace, per `04`'s measured teardown.
Push is the first act that escapes it.

No new concept, and the line falls exactly where reversibility ends.

**Who merges: never farseer in v1.**

Merging is the act that makes agent work real.
`11` chose **intervention rate** as a headline metric, and that metric only means something while the human is still the gate - an automated merge would make the number look excellent while measuring nothing.

Relaxable per project later, once there are numbers to relax it against.

### 3. Irreversibility has three levels, and the top one is not negotiable

`08` asked whether irreversibility is binary or needs degrees.

**Three levels. Not two, not a spectrum.**

| Level | Examples | Default |
| --- | --- | --- |
| `reversible` | file writes inside a workspace | ungated |
| `undoable` | post, pull request, comment | gated, a cell may lower it |
| `irreversible` | payment, email send, package publish, force push, **purge** | **gated, never lowerable** |

Two levels would collapse "embarrassing but fixable" with "the money is gone", and those deserve different defaults.
A spectrum would be unfalsifiable: nobody can defend 0.6 against 0.7.

**The rule that matters is that the top level is not overridable per cell.**
Otherwise unattended payments are one edit to a definition file away, and `01` made definitions plain files in git precisely so they would be easy to edit.

Tools declare their own level. Policy gates on it. The machinery already exists - `01` has gated actions, `16` mapped them onto ACP's permission flow, and `21` found A2A's `INPUT_REQUIRED` is the same shape - so this is a new input to existing machinery rather than new machinery.

### 4. Composition: everything only ever narrows

`06` introduced `autonomy_ceiling` and asked whether deny lists compose the same way.

**Deny lists union. Autonomy ceilings take the minimum. Both only ever narrow.**

Cell A calls cell B calls cell C, and the effective autonomy at any depth is the minimum of every ceiling on the path and the local policy.
The effective deny list is the union of every deny list on the path.

**A callee may refuse a call whose ceiling is below what the work needs**, returning a refusal rather than accepting and failing later.
That matches `06`'s fail-at-handshake instinct: fail at call time, not in the middle of an hour-long task.

#### A foreign peer cell is an `irreversible` tool

`21` found that a non-farseer A2A callee **silently ignores four of eight cell-call fields**: `autonomy_ceiling`, `budget`, `definition_of_done` and `deadline`.
So it cannot be bounded, and what it does cannot be undone.

That is the definition of the top level, so the three questions `21` handed this ticket collapse into one answer:

- **Is calling a foreign peer cell a gated action?** Yes, because `irreversible` is gated by default.
- **Does an unbounded callee need a locally enforced deadline?** Yes, and the caller enforces it by cancelling the call, since `06` made a cell call a run in the calling cell.
- **Does irreversibility compose across a cell boundary?** It does not need to. The **call itself** carries the top level, regardless of what the callee's own tool grants are, precisely because they are invisible.

Treating an unbounded external actor as the most dangerous kind of tool is both correct and the simplest thing that could work.

### 5. Purge is operator-only

`17` asked whether a manager may ever purge.

**No. Never a manager, never a worker. Operator only.**

Same reasoning `02` used when it refused agents a raw event-append path.
An agent that can destroy its own history makes the record worthless as evidence, and **forge and destroy are two halves of one threat**.

The real use case - retention - does not need an agent at all.
`08` made **triggers API clients rather than subsystems**, so retention purge is the operator's scheduled trigger calling the operator's API. No agent is in the path.

Purge is also `irreversible` by the table above, so it is gated and the gate is not lowerable.

### 6. The delivery gate is a tool, not a runtime concept

The ticket asked whether the existing `no-mistakes` skill is the delivery gate or farseer defines its own.

**Neither. Farseer defines the contract; the gate is a tool the cell may run.**

`05` already put `definition_of_done` on the worker contract. That is farseer's half.
Whether satisfying it means running `no-mistakes`, running `cargo test`, or a human watching a video is a **tool grant**.

Hardcoding a specific gate would add a field only coding cells need, which is exactly what `08`'s falsification test forbids.
A social media cell has no tests, no lint and no CI.

### 7. A non-coding cell does not need stricter defaults

The ticket asked whether posting publicly needs a stricter default than writing a diff.

**No, and the three levels are why the answer is no.**

A social media cell is not stricter *as a cell*. Its **tools** are more dangerous:

- "write a draft file" is `reversible`
- "post to X" is `undoable`
- "send payment" is `irreversible`

**Policy attaches to the tool. The cell inherits whatever its roster implies.**

The asymmetry the ticket sensed is real, and it lives entirely in the tool declarations.
Adding a `cell_kind` or a per-domain strictness setting would move it into the primitive instead, which is the field `08` spent a whole ticket proving unnecessary.

That is the second time this ticket's answer was determined by `08`'s test rather than by preference, which is a good sign the test is doing real work.

### Summary of what policy actually consists of

Four things, and only the first is a real boundary:

1. **Tool grants** - the allowlist. The only isolation v1 has.
2. **Irreversibility level** - declared by the tool, gated by policy, top level never lowerable.
3. **Autonomy ceiling** - composes by minimum, never raised by a caller.
4. **Deny list** - unions down the chain, catches accidents, advisory wherever a shell is granted.

### Tickets this informs

- `10 runner inventory` - a runner that provides shell access effectively grants every tool, so the inventory should record **what a runner can reach**, not only whether it satisfies the control contract.
- `13 harness build kit` - a cell definition must be able to declare **tool grants**, and a tool declaration must carry its **irreversibility level**. Those are the only two policy fields the kit needs; everything else here is runtime behaviour rather than definition content.

## Confirmed by measurement (10)

This ticket asserted there is no OS sandbox on native Windows, so the tool grant is the only isolation v1 has.
`10 Runner inventory` tested it rather than assuming it.

**Codex CLI exposes `--sandbox` with `read-only`, `workspace-write` and `danger-full-access`.**
On this machine, `codex exec --sandbox read-only` accepted the flag without warning, ran a shell command, and **created the file anyway**.

One observation rather than an audit, and it does not establish why - unimplemented on Windows, or silently downgraded.
The operational conclusion holds either way:

**A runner that advertises a sandbox is more dangerous than one that does not, if the sandbox does not enforce, because it invites confidence that is not earned.**

So: **reach is recorded as observed, never as advertised.**
This strengthens the decision above rather than changing it - the tool grant really is the only isolation, and that is now measured.
