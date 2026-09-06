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
Generated target, node_modules, and UI build output are ignored; retain them to avoid unnecessary rebuilds or downloads.
feedback.txt and the operator's reference list remain user source material, not an implementation backlog.

## Git-only history

Cleanup on 2026-09-06 removed superseded BRIEF.md, ARCHITECTURE.md, and root REVIEW.md; unused .scratch/design exports; and stale .impeccable metadata, critiques, and UI captures.
The version-six design capture is historical; current UI source and active tickets govern future changes.
Historical mentions of these names in decision records refer to their Git versions, not current contracts.
All removed files are recoverable from commit df90320b0e7215d2ec82df5d285e5d731c8537ed without checking out the old project.

For example, read the old architecture draft:

```powershell
git show df90320b0e7215d2ec82df5d285e5d731c8537ed:ARCHITECTURE.md
```

Keep future temporary exports and captures out of the tracked workspace unless a ticket needs them as evidence.
Remove superseded snapshots after their useful findings are recorded in tickets; use Git for recovery instead of maintaining another archive tree.
