# Farseer project rules

Global agent instructions apply above this file.
Keep this file limited to Farseer-specific contracts and commands.

## Source of truth

- [README.md](README.md) contains operational entry points and repository shape.
- [CORE.md](CORE.md) contains the durable runtime contract and rework sequence.
- `.scratch/history.md` is historical context only.
- Source and tests are authoritative for current behavior.

## Core rules

- Keep `farseer-core` pure and free of clocks, filesystem, process, network, and persistence.
- Keep the runtime headless and clients replaceable.
- Keep UI or widget concepts out of core contracts.
- Route operator, manager, MCP, A2A, artifact, and maintenance work through shared application services.
- Seal immutable run contracts before spawn and record route and capability snapshots.
- Treat missing, unsupported, unavailable, and unreported values as distinct.
- Keep events runtime-owned, append-only, versioned, and redacted.
- Keep projections bounded, rebuildable, and source-labelled.
- Put external protocols and terminals behind adapters.
- Optional features must degrade without disabling core startup or core commands.
- Do not add a dynamic plugin ABI, learned router, or second canonical store without a concrete integration and an acceptance test.

## Windows and process rules

Windows is the primary target.
Identify processes by PID plus creation time or Job Object membership.
Resolve commands through PATHEXT before spawning.
Use Job Objects for supervised process trees.
Closed stdin is required for one-shot runners; conversational runners finish turns without EOF.
Worktrees start from committed state and are retained until output commits are safe.

## Validation

Use `cargo` for Rust and `bun` for UI.
Run `cargo fmt --all -- --check`.
Run `cargo clippy --workspace --all-targets -- -D warnings`.
Run `cargo test --workspace`.
Run `bun run --cwd ui check` and `bun run --cwd ui test` for client changes.
Run `bun run --cwd ui build` before packaged-shell checks.
Live runner tests are ignored unless explicitly authorized.

## Documentation

Update README architecture only when a core boundary or command changes.
Put durable design changes in CORE.md.
Do not add UI layout or cosmetic rules to the core contract.
Keep private `feedback.txt` untracked.
Use imperative commit subjects under 50 characters.
Do not push unless the user explicitly authorizes the destination.