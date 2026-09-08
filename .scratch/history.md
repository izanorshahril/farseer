# Farseer historical record

Purpose: preserve compact history of removed planning and UI documents without making them part of normal agent context.

## Foundation

Farseer selected the cell as the unit of address, policy, roster, budget, workspace, and record scope.
Cell definitions are hand-written data in Git; running cells are live managers plus supervised workers.
Workers have immutable contracts, explicit authority, bounded budgets, and parent-linked cancellation.
Foreign agents are runners at an adapter boundary; foreign orchestrators are peer cells.
SQLite is canonical storage with one runtime-owned append-only record and rebuildable projections.
The local API is authenticated loopback HTTP plus SSE.
Windows Job Objects, PATHEXT resolution, process creation identity, and supervised teardown are core safety rules.
Capabilities and usage are observed from runner output and never inferred when absent.
Provider percentages and costs retain source, units, period, and confidence.
Skills and tools are declared per cell or roster entry and are not silently inherited.
Manager delegation uses the same authority checks across MCP, ACP, app-server, extension, and JSON transports.

## Command-center work

The command-center work added bounded task boards, session and transcript projections, scoped graphs, project profiles, terminal profiles, maintenance proposals, artifact manifests, optional resource samples, privacy presentation, and replaceable widgets.
These remain runtime concerns even though the old UI documents are removed.
Optional browser and live-runner demonstrations remain evidence gaps rather than core contract changes.

## Rework trigger

The shipped UI work exposed an application-boundary problem.
The instruction body mixes semantic routing with UI anchor data.
Runner, model, reasoning, cost, and capability selection are not represented by one sealed route plan.
Several ingress paths assemble tasks, runs, and events independently.
The desktop surface owns too much context state for a replaceable client.
CORE.md records the selected rework rather than preserving the old command-center ticket tree.

## Cleanup

Superseded `.scratch` maps, issues, research, spikes, prototypes, and the document-routing file were removed from the working tree.
Old exact files remain recoverable from Git history.
Cosmetic UI design guidance was removed from the durable contract.
CLAUDE.md and the repository `.claude` launch scaffolding were removed.
Global agent instructions are no longer duplicated in the repository AGENTS.md.
Private feedback remains outside Git.