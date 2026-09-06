# 20: Noncoding artifact-manifest pipeline

**Parent:** [Command-center decision map](../map.md).
**What to build:** Run a deterministic local artifact-manifest job in an authorized plain-directory project using existing task/run and worker supervision.
**Blocked by:** [Project team profiles](14-project-teams.md), [Honest tool authority](19-honest-tool-authority.md).
**Status:** proposed-awaiting-review.
**Execution:** blocked until the command-center package receives its single approval.
**Review refs:** R12; R13 applies as targeted cleanup.
**Decision:** [Self-maintenance and domain integration](../decisions/05-maintenance.md).

## Contract choices

The worker reads an explicitly authorized input directory and stages a manifest of relative file names, byte sizes, and SHA-256 digests.
Sort entries deterministically.
Keep producer run identity/timestamps in artifact provenance outside the deterministic manifest content.
The worker accepts cancellation and reports progress/outcome through the runner seam; no special domain execution engine is added.
Inputs remain unchanged and partial outputs are never promoted as complete.
The operator promotes a completed staged artifact through the existing review/action pattern.
Terminal presentation and maintenance are not prerequisites.
A declared input-path check is not a claim to sandbox every action of a shell-capable process.

## Acceptance criteria

- [ ] A project without Git starts the worker through the ordinary task route and shows task/run/artifact status in Work.
- [ ] Identical input bytes yield identical sorted manifest content, with separate per-run provenance.
- [ ] Cancellation reaps owned processes and leaves partial staged output clearly incomplete; it never appears as a promoted final artifact.
- [ ] An input request outside authorized project roots is refused before launch; the allowed fixture remains unmodified.
- [ ] Disabling this runner/integration leaves unrelated projects operable, and failures remain inspectable through existing record reads.

**Exclusions:** Network access, paid APIs, Git dependency, live publishing/trading, terminal requirements, and broad domain plugin scaffolding.
**Test seam/demo:** Run twice and compare manifests, cancel a larger fixture mid-run, and inspect its outcome and unpromoted output.
