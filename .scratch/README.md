# Workspace documents

## Start here

Use [the command-center map](farseer-command-center/map.md) for approved work and [its ticket index](farseer-command-center/tickets.md) to select the next slice.
Use [the project critique](../PROJECT_REVIEW_2026-09-06.md) for rationale and recommendations; it is a dated review, not a live completion ledger.
Use [the original decision map](farseer/map.md) for established contracts and corrections.
Read individual tickets on demand; completed implementation does not retire its design contract.

## Retention

Keep the two maps, their decisions and tickets, referenced research, and cited prototype/spike evidence.
Keep runtime implementation detail in [the optional reference](farseer/runtime-reference.md), outside automatically loaded AGENTS.md.
Keep .git, .github, .gitignore, .claude/launch.json, and CLAUDE.md: they support versioning, CI, generated-file exclusions, local UI launch, and instruction discovery.
Keep source, lockfiles, runner configuration, and current PRODUCT.md, DESIGN.md, and HARNESS.md.
Generated `target/`, `ui/node_modules/`, and `ui/dist/` are disposable and ignored; keep them only while actively iterating, and delete them after stopping local processes when disk space matters.
Rebuild `ui/dist/` before running the packaged shell after deleting it.
feedback.txt remains private user source material, not an implementation backlog.

## Git-only history

Cleanup on 2026-09-06 removed superseded BRIEF.md, ARCHITECTURE.md, and root REVIEW.md; unused .scratch/design exports; and stale .impeccable metadata, critiques, and UI captures.
The version-six design capture is historical; current UI source and active tickets govern future changes.
Historical mentions of these names in decision records refer to their Git versions, not current contracts.
All removed files are recoverable from commit df90320b0e7215d2ec82df5d285e5d731c8537ed without checking out the old project.

For example, read the old architecture draft:

```powershell
git show df90320b0e7215d2ec82df5d285e5d731c8537ed:ARCHITECTURE.md
```

Keep future temporary exports, generated spike targets, PID captures, and tool metadata out of the workspace.
Remove superseded snapshots after their useful findings are recorded in tickets; use Git for recovery instead of maintaining another archive tree.
The superseded `ui-command-center` and `one-operator-turn` prototypes were removed on 2026-09-06; `prototypes/berd-home` remains the accepted static design reference.

## Load policy

Start with the active command-center map and open only the selected ticket plus its named blockers.
Use the original Farseer map and a specific issue only when the implementation touches an existing runtime contract.
Treat research, prototypes, spikes, and completed tickets as evidence to open on demand, not as a prompt bundle.
Completed does not mean obsolete when source comments or API documentation still link the decision.
Delete generated output under ignored paths when it is no longer useful; do not delete a retained document solely because its ticket is closed.
