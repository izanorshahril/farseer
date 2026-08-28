# 35 notification plane

**Status:** resolved and built 2026-08-28, proven end-to-end against a real listener.
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

## Resolution

Built as `crates/farseer-api/src/notify.rs`.
Off unless `FARSEER_NOTIFY_URL` is set, which is the whole configuration surface.

### The backend is a URL, and ntfy is the documented default

Anything that accepts an HTTP POST is a backend - ntfy, Slack, Discord, an operator's own bridge - so there is one code path and a different address, and no Rust trait was needed to get a seam.
ntfy earns the default on the constraint `00` set rather than on features: **no account, no token, no SDK**, self-hostable, phone app, `Title` and `Priority` are plain headers.

The URL lives in the environment because ntfy's own documentation says a topic *is* the password.
That makes it a credential, and `31 manager delegation reach` already settled where those live.

### Three triggers, and the third was found by running it

**`run_finished`, root run only.** A manager delegating six workers finishes seven runs; a notifier that reports all seven is one the operator mutes, after which it is worse than absent. `Store::first_run_of_task` is the scoping, and a run farseer cannot resolve is treated as a root - what this guards against is noise, and the worse failure is silence.

**`likely-hung`.** This ticket originally called this the risky trigger. **It is not, and this ticket was wrong about why.** `18 hang detection prior art` keyed the watchdog on progress events, and `05 run state model` overruled it precisely because a model reasoning for twenty minutes emits none and would have been flagged while working perfectly. The watchdog now keys on **any bytes from the adapter**, so `likely-hung` means mechanical silence. That correction is what makes a notifier possible at all.

Read from the live handles rather than the record, because `16 local api surface` keeps liveness **derived, never stored**.
There is no `status_changed` event to subscribe to and this module does not add one: a stored liveness would be a second truth to keep in step with the first.

**`manager_answered`, root run only - which the two-event plan did not have.**
An end-to-end run found it. `15 manager conversation` keeps a manager **open** on live stdin after it answers, so `run_finished` does not arrive until somebody cancels or closes the session.
The plan would have shipped a notifier that stayed silent through exactly the case it was built for: instruct a cell, walk away, come back.
This is the same moment Claude Code raises its own `Notification` hook on.

### Never in the path of anything

A refused POST, a dead host and a wrong URL all end in a discarded `Result`.
Nothing retries - the next thing worth saying will be along, and a queue of stale alerts is its own problem.

### Proven

Against a local listener, `pi` on cell zero, 2026-08-28:

```
NOTIFICATION title='farseer: answered'  priority='3' body='run 01a0481a is waiting for you'
NOTIFICATION title='farseer: cancelled' priority='4' body='run 01a0481a cancelled'
```

### Still open

- **Chattiness of `manager_answered`.** One per turn. A manager that answers three times in a task pings three times. Correct today, and the first thing to watch.
- **A WinRT toast**, which needs a packaged app identity. The trait did not need building for it; the URL already is one.
- **`34 record mojibake` is cited at `crates/farseer-manager/src/lib.rs:2443` and does not exist.** The assertion it guards is real and passing. The citation dangles.
