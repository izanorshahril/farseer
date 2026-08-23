# jobspike

Throwaway spike for `03 Spike: Win32 Job Object kill-on-close with a real harness child`.

Builds a five-deep process tree rooted at a real `.cmd` shim, then kills it three ways and counts what survives.

The tree is `cmd.exe` -> `node.exe` (npm-cli) -> `cmd.exe` (npm script shell) -> `node.exe` (`tree/spawner.js`) -> `node.exe` (`tree/grandchild.js`).
That is the shape a real runner has: a shim, a runtime, a shell, an agent, and a tool child.

## Run

```
cargo run -- job      # job object, kill-on-close
cargo run -- naive    # no job, TerminateProcess on the root only
cargo run -- conpty   # job object, tree hosted behind a pseudoconsole
```

`naive` deliberately leaves orphans alive and prints a `Stop-Process` line to clean them up.
It never terminates anything itself, on purpose - see the comment on `descendants`.

## What each mode is for

| Mode | Question |
| --- | --- |
| `job` | Does closing the job handle reap the whole tree? |
| `naive` | Does killing the root alone leave orphans? The negative case that justifies the effort. |
| `conpty` | Does the answer hold when a terminal-only runner adapter owns a PTY, and does ConPTY's console host leak? |

Results are recorded on the ticket, not here.
