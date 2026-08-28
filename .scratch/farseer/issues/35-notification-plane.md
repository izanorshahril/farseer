# 35 notification plane

**Status:** open.
**Raised:** 2026-08-28, from a gap review of the architecture. Four of its five findings were already built or misdiagnosed; this one was real.

## The question

A run finishes, or goes `LikelyHung`, or exhausts a budget, and **nobody is told**.

Every surface farseer has assumes the dashboard is open and a person is looking at it.
`16 local api surface` gives one SSE endpoint, `07 attach semantics` makes live and replay the same call with a different cursor, and `28 operator surface` puts the canvas on top of both.
All three are pull.
The operator is not at the machine at 3am, and farseer's whole point is that a run outlives the operator's attention.

So: **what pushes, to where, on which events, and what must it never become?**

## Why this is not the gate bar

The review that raised this also proposed wiring a gate bar to `policy.rs` so an operator could approve destructive shell commands mid-run.
That is refused, and `12 autonomy and deny list` already refused it: the deny list is not a security boundary, `deny read .env` is defeated by `cat .env`, and autonomy is decided **before** the run through tool grants.
A mid-run approval prompt would restate exactly the false assurance that ticket exists to deny.

This ticket is the other thing - **telling a person something happened**, which is not the same as asking one for permission.
Keeping them apart is the point.
A notifier that can be answered is a gate wearing a different hat.

## The shape, to be argued rather than assumed

**One trait, and the record is the trigger.**
Every candidate event already exists and is already appended: `run_finished`, `status_changed` carrying `LikelyHung`, and whatever `27 quota accounting` writes when a window closes.
So a notifier is a **subscriber to the log**, not a new call site threaded through the manager - which also means it can never invent a notification for something the record does not hold.

**Webhook first, and OS toast second.**
A WinRT toast needs a packaged app identity; a webhook needs a URL.
The cheap one proves the trait, and if the trait is right the toast is an afternoon.

**Never in the run's path.**
A notifier that can fail a run is worse than no notifier.
Delivery is best-effort, its failures go to the record, and nothing waits on it.

## What this must decide

1. **Which events.** Terminal outcomes are obvious. `LikelyHung` is the one that earns the feature - `05 run state model` is careful that mechanical silence is a hang and a thinking model is not, and a notifier that cries hang on a slow run gets muted, after which it is worse than absent.
2. **Where the config lives.** `runners.toml` is the operator's file for machine facts; a webhook URL is a **secret**, and `31 manager delegation reach` already settled that a credential belongs in the environment rather than anywhere a model can read it.
3. **Whether a notification is an event.** Recording that a person was told is cheap and makes "why did nobody know" answerable. Recording it in the same log the notifier reads is a loop that must be cut deliberately.
4. **One-way, and stated as such.** See above. If a later ticket wants an answer to come back, that is a new grilling, not an extension of this one.

## Not decided here

Mobile push, and anything with an account.
`00`'s constraint holds: user-space, portable, offline-friendly, no privileged installer and no avoidable online service.
A webhook the operator points at their own bridge satisfies that; a farseer account does not.

## Loose end found while raising this

`crates/farseer-manager/src/lib.rs:2443` cites `34 record mojibake`, and **no such ticket exists**.
The assertion it guards is real and passing. The citation is dangling.
