# 20: Noncoding artifact-manifest pipeline

**Parent:** [Command-center decision map](../map.md).
**What to build:** Run a deterministic local artifact-manifest job in an authorized plain-directory project using existing task/run and worker supervision.
**Blocked by:** [Project team profiles](14-project-teams.md), [Honest tool authority](19-honest-tool-authority.md).
**Status:** bounded slice implemented; live cancellation demonstration remains open.
**Execution:** package approved; verify named blockers before implementation.
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

- [x] An authorized plain-directory project starts the local worker and shows task/run/artifact status in Work.
- [x] Identical input bytes yield identical sorted manifest content, with separate per-run provenance.
- [x] Cancellation leaves partial staged output clearly incomplete; it never appears as a promoted final artifact.
- [x] An input request outside authorized project roots is refused before launch; the allowed fixture remains unmodified.
- [x] Disabling or failing this integration leaves unrelated projects operable, and failures remain inspectable through existing record reads.

**Exclusions:** Network access, paid APIs, Git dependency, live publishing/trading, terminal requirements, and broad domain plugin scaffolding.
**Test seam/demo:** Run twice and compare manifests, cancel a larger fixture mid-run, and inspect its outcome and unpromoted output.

## Evidence

`POST /v1/artifacts/manifests` validates both the authorized project and input directory, creates the ordinary conversation/task/run rows, and launches a bounded local worker.
The worker sorts relative files, records byte sizes and SHA-256 digests, writes `manifest.json.partial`, and renames it to `manifest.json` only after completion.
Cancellation is an atomic flag observed during traversal and writing; the artifact row and task transition become cancelled while the partial file remains visibly incomplete.
`GET /v1/tasks/{task_id}` and the Work widget expose artifact status and failure text.
The runtime admits at most two manifest workers at once and refuses trees above 100,000 file entries or 8 MiB of path metadata before unbounded memory growth.
The public HTTP route now has a regression test that waits for completion, opens task detail, and verifies the final manifest bytes.
The remaining live demonstration is cancellation of a larger fixture through the public HTTP route; this slice owns no child process, so cancellation is worker cancellation rather than process reaping.
