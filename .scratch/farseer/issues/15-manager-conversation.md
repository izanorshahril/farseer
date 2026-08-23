# Prototype: one operator turn, end to end

Type: prototype
Status: closed
Blocked by: none

## Question

Raise the fidelity of the discussion with a cheap artifact: a written transcript of one complete operator interaction, on paper, no code.

Take the operator's own example: "post about X on social media".

Write out every step in full:

- What the operator types.
- What the manager says back, and what it must not say (mechanics, per the brief).
- The contract the manager writes, field by field.
- The A2A call to the social media cell, as an actual cell call.
- What comes back, and what the manager surfaces versus swallows.
- Where an escalation would interrupt, and what that looks like.
- What the operator sees if they attach mid-run.

React to it. The point is to find which fields, states and messages are missing before anything is built.

## Resolution

Resolved 2026-08-23.
Artifact: [one-operator-turn.md](../prototypes/one-operator-turn.md).

### The premise was stale

This ticket asked for "the A2A call to the social media cell".
`06` later decided local cells take the **in-process path** and never traverse the A2A endpoint, which is off by default.
So the transcript is written as a **cell call** over the native path, and the A2A variant is kept as a contrast in section 11 rather than as the main line.

### Five gaps found

The ticket said the point was to find what is missing before anything is built. It did.

**Graduated to `22 Which cells may a manager call, and does an instruction route or delegate?`:**

1. **How does a manager know which cells it may call?** Cell #0 says "I'll hand this to the social cell" and nothing on the map justifies that sentence. `01` gave a cell a roster of workers and tools and never put callable cells in it.
2. **Does an operator instruction route or delegate?** The prototype assumed delegate, following `06`. Routing is coherent and nothing rules it out, and the two put `11`'s metrics in different places.

**Graduated to `23 Three loose ends the prototype exposed`:**

3. **Budget does not compose the way a ceiling does.** Two nested calls at $2 each can spend $4 unless a budget also narrows.
4. **`edit` has no home**, and looks identical to `07`'s takeover.
5. **A queued run the manager abandons has no terminal state.** `cancelled` means a human chose; `failed` means something broke. Neither fits.

### Three things it confirmed

- **`06`'s caller/callee split is legible on paper.** The cell call reads as a brief rather than a configuration, and the absence of workspace and runner fields is a feature you can feel.
- **`08`'s artifact concept survives a real non-coding example.** The post is a text artifact with no base; the video would have been binary; neither needed a review mode.
- **A gated action is a state, not an error.** `21` found A2A models this as `INPUT_REQUIRED`, interrupted rather than terminal, and writing the escalation out confirms that is the right shape.

### One thing worth showing the operator

Section 11 replays the same turn with the social harness as a **foreign** orchestrator rather than a farseer cell.
It loses attach, loses cursored replay, and **silently loses every bound the caller tried to set**, since `21` found four of eight cell-call fields have no native A2A home.

That contrast is the strongest available argument for keeping the social harness a farseer cell rather than a foreign one, and it is more persuasive as a worked example than as a principle.

### What it did not need

No new field. No new state on any axis except the one flagged as gap 5.
The transcript is written entirely in `14`'s glossary, which per `13` is the test a build kit must also pass.
