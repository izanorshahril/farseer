# Working in this repository

## Read only what the task needs

Start with [README.md](README.md) for build, run, and repository layout.
For command-center implementation, select one ticket from [the active map](.scratch/farseer-command-center/map.md), then load only its dependencies and relevant spec sections.
The package is approved; unfinished tickets still require their blockers and acceptance evidence.
For architecture changes, consult the relevant decision in [the original map](.scratch/farseer/map.md).
Completed decisions remain authoritative unless a later correction supersedes them.
Append changed decisions to their ticket and map; preserve the original reasoning.
Read [runtime reference](.scratch/farseer/runtime-reference.md) only for runner semantics, runtime boundaries, or recorded machine probes; verify dated observations against current source.
Read [DESIGN.md](DESIGN.md) and [PRODUCT.md](PRODUCT.md) for UI work, and [HARNESS.md](HARNESS.md) for runner integration.
[Workspace document policy](.scratch/README.md) identifies retained evidence and Git-only history.
Do not bulk-load historical research, prototypes, spikes, or all tickets.

## Build and verify

Use cargo and bun; no external review service or push gate is a product dependency.
Run Rust tests with cargo test --workspace; bare cargo test covers only the default desktop-shell member.
Run cargo clippy --workspace --all-targets -- -D warnings and cargo fmt --all -- --check; apply cargo fmt --all before committing.
For UI changes, run bun run --cwd ui test and bun run --cwd ui check.
Live runner tests are ignored because they spend real time and provider usage; run only explicitly authorized tests by name.
Distinguish sandbox ACL failures from product defects using an owning-user reproduction; preserve current-user-only runtime credentials and data permissions.

## Design and vocabulary

Keep farseer-core pure: clocks, filesystem, and network enter through arguments or outer layers.
Keep the runtime headless; UI widgets are clients, compiled by the UI host, never loaded by the runtime.
Use [the locked glossary](.scratch/farseer/issues/14-vocabulary-lock.md): runner, worker contract, and cell call.
Cite the deciding ticket in non-obvious code comments; name tests after observable behavior.
Observe capabilities and usage from verified runner output; absent fields remain absent.
Keep external protocols at adapter boundaries and respect existing capability and budget checks.

## Windows process rules

Windows native first; detect the actual host and shell before issuing commands.
Identify a process by (pid, creation_time) or job membership, never PID alone.
Resolve bare commands through PATHEXT before spawn; an extensionless npm file may be a POSIX script.
Use Job Objects for supervised process trees.
One-shot runners need closed stdin; conversational runners finish a turn without exiting or reaching EOF.
Manager worktrees start from committed HEAD; required inputs must be committed and output commits need a retained branch before teardown.
Prior-art projects are evidence, not dependencies.

## Collaboration and documentation

Use GPT-5.6 Luna for delegated work, with reasoning no higher than xhigh.
Use plain dashes and one sentence per line in Markdown.
Refer to tickets by name, not number alone.
Keep feedback.txt private and untracked.
Update README layout and flow when structure or behavior changes.
