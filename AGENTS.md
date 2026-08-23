# Working in this repository

Farseer is at **specification, not implementation**.
Twenty-seven decision tickets are closed and the runtime does not exist yet.
The only code is three throwaway spikes under `.scratch/farseer/spikes/`.

## Read the map before proposing architecture

[`.scratch/farseer/map.md`](.scratch/farseer/map.md) indexes every decision in one line each and links to the ticket holding the detail.
A decision lives in exactly one place: its ticket.

Before answering any question about how farseer should work, check whether a ticket already answers it.
Twenty-seven of them do, and they carry the reasoning, the rejected alternatives, and what the answer cost.

When a ticket turns out to be wrong or superseded, **append the correction to that ticket and to the map**, rather than editing the original text in place.
The map's own list of corrections is how a reader knows a resolution has moved.

## Use the locked glossary

[14 vocabulary lock](.scratch/farseer/issues/14-vocabulary-lock.md) is authoritative and every later document uses it.

Two words were retired and stay retired:

- A slot that executes work is a **runner**. Never a *seat*.
- The payload a manager gives a worker is a **worker contract**; the payload a manager sends another cell is a **cell call**. Never an *envelope*, which meant both.

A new noun needs a reason. Prefer widening an existing one.

## Writing conventions

- **Use a plain dash.** The em dash appears nowhere in this repository.
- **One sentence per line** in markdown, keeping normal markdown structure otherwise.
- Refer to a ticket by its **name**, not its number alone. `01, 07, 22` is illegible; names read at a glance.

## Platform

**Windows native first.** mac and Linux are a later milestone and are expected to be a subtraction of Windows workarounds, never a v1 constraint.

Toolchain is `x86_64-pc-windows-msvc` on rustup stable, chosen in [19 rust toolchain](.scratch/farseer/issues/19-rust-toolchain.md) because crates are tested on MSVC first.

Two platform facts the spikes established the hard way:

- **Process identity is `(pid, creation_time)` or job membership, never a pid alone.** Windows recycles pids aggressively, and parent-pid tree reconstruction terminated an unrelated application during `jobspike`.
- **Resolve a bare command against `PATHEXT` before accepting it.** An extension-less `npm` on PATH is a POSIX shell script, not the `npm.cmd` you wanted.

## Running a spike

Each spike has its own README with modes and expected output.

```bash
cargo run --release --manifest-path .scratch/farseer/spikes/jobspike/Cargo.toml -- job
```

Spikes exist to unblock decisions and are not the product. Treat them as evidence, not as a foundation to build on.

## Facts observed on this machine

Recorded in [10 runner inventory](.scratch/farseer/issues/10-runner-inventory.md), and each was measured rather than read from documentation:

- **Claude Code emits `rate_limit_event` on every successful headless run**, carrying `resetsAt` as unix epoch. The documented quota surface is a status line, which **does not fire in `-p` mode**.
- **Codex CLI and cursor-agent refuse a fresh directory**, needing `--skip-git-repo-check` and `--trust` respectively. Every run gets a fresh worktree, so an adapter omitting these fails every run while looking like a hang.
- **A runner inherits the operator's configuration directory** unless its adapter prevents it. A one-word prompt cost $0.32 loading plugins nobody granted it.

What a runner can reach is **observed, never advertised**. Codex CLI accepted `--sandbox read-only` and created the file anyway.
