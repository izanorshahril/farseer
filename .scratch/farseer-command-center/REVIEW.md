# Review once: Farseer's next build

**Read this page, then reply `Approve plan` or name the decisions you want changed.**
Estimated reading time: 4 minutes.
Branch: `codex/command-center-plan`.
State: planning complete; implementation has not started.

## 1. My recommendation

**Keep Farseer's existing runtime and cell model.**
Build reliability first, then the command-center experience, then project teams and bounded self-maintenance.
The full plan has one specification and 21 independently verifiable implementation tickets.
You do not need to read all 21 to review the direction.

## 2. Decisions, nominees, and reasons

| Decision | Selected recommendation | Other nominees | Why I selected it |
| --- | --- | --- | --- |
| [Runtime and extensions](decisions/01-runtime.md) | Independent user-space daemon; optional features use existing interfaces | Shell-owned daemon; privileged service; native plugin ABI | Closing a window must not end work; existing deployment stays simple |
| [Data and analytics](decisions/02-data.md) | Bounded SQLite queries; optional analysis outside the writer lock | Fetch all history; new graph/vector database; remote embeddings by default | Addresses the confirmed growth problem before adding infrastructure |
| [UI and terminal](decisions/03-workspace.md) | Flow canvas plus shared focused panes; explicit context; optional supervised terminal | Cards only; arbitrary docking; free-position canvas | Supports overview and sustained work without a second execution model |
| [Projects and routing](decisions/04-routing.md) | File-based profiles referencing cells; multiple harnesses; deterministic eligible routing | New team engine; direct project-manager ingress; learned router/Switchyard now | Reuses ownership and grants, makes routing explainable, avoids extra classifier cost |
| [Maintenance and integrations](decisions/05-maintenance.md) | Isolated source proposals; separate staged promotion/rollback; local manifest pilot | Live self-patching; permanent LLM managers; general tool gateway | Improvement cannot make the running supervisor its own only recovery path |

Each linked decision records its contract, rejected alternatives, justification, and condition for reconsideration.
These are my selected recommendations, not claims that you already approved them.

## 3. Tradeoffs you are approving

1. **Work survives UI closure.** Explicit shutdown drains; after 30 seconds it reports pending work and stays alive, rather than cancelling automatically.
2. **The top manager remains the entry point.** Projects can use multiple harnesses, but coordination still consumes tokens; direct project ingress is deferred.
3. **Cost routing is measured, not guaranteed optimal.** Explicit pins are honored or refused; unknown spend stays unknown; no learned router is required.
4. **Customization is bounded initially.** Existing card sizes, persisted rail controls, focused navigation/main/inspector panes, and one comparison split; arbitrary docking comes later only if needed.
5. **Maintenance can prepare changes, not silently install them.** Promotion uses matched binary/data recovery; shell grants remain real authority checks rather than decorative tool approvals.

## 4. What gets built first

| Order | Usable result |
| --- | --- |
| Start | Verified backend startup and UI-independent runtime lifecycle |
| Reliability | Paged boards/graphs, bounded indexing, widget recovery, explicit composer context |
| Daily use | Responsive canvas, privacy mode, focused workspace, sessions, attributed usage |
| Orchestration | Explainable routing, project team profiles, optional terminal/resource views |
| Maintenance | Recoverable storage, source proposals, staged promotion, non-coding artifact pilot |

This is priority order, not one long dependency chain.
Independent UI fixes and storage recovery can proceed without waiting for unrelated features.
The [ticket index](tickets.md) contains the actual blockers and review-to-ticket coverage.

The chosen non-coding pilot reads an authorized plain directory and stages a deterministic manifest of file names, sizes, and hashes.
It exercises supervision, artifacts, and cancellation without Git, paid services, or external effects.
Trading, publishing, social collection, remote hosts, learned routing, semantic embeddings, and a marketplace are later projects, not hidden commitments in this build.

## 5. Scope of this one approval

Approval adopts the five decisions, [specification](spec.md), test seams, and ticket granularity as the implementation plan.
It releases eligible tickets for implementation in dependency order; no second product interview is planned.
Routine implementation details are selected within these contracts.
A material change to authority, external effects, or the agreed scope must be surfaced rather than silently added.
Future real runtime promotion and external actions retain their product authorization rules.

The original decision records and your inherited working-tree changes are preserved.
Corrections to old decisions are appended by the relevant implementation ticket after approval.
Documentation checks validate local links, ticket dependencies, and acceptance coverage; no product code or new live harness probes were needed in this planning turn.

**Done:** branch, decisions, spec, and 21 draft tickets saved.
**Next:** reply `Approve plan` to adopt this package and proceed with Verified startup, or name one decision to change.
