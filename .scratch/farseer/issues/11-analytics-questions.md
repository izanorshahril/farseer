# Which analytics questions must the record answer?

Type: grilling
Status: closed
Blocked by: none

## Question

The graph schema should be driven by real questions, not by what happens to be graphable.

Get three or four concrete questions the operator genuinely wants answered, phrased as things they would actually ask on a Tuesday morning.
Candidates to react to, not to accept:

- Which files or areas repeatedly break, and under which harness or model?
- Which lessons actually reduced failure rate after adoption?
- Cost and token spend per project, per task shape, per runner, over time.
- Which contracts were ambiguous, measured by rework rate?
- What dead ends have already been explored, so a worker is warned before repeating one?

For each accepted question, name the entities and edges it requires.
Anything in the proposed schema that no question needs is cut.

## Carried from 02

**The record outlives deleted cells**, so historical questions can span cells that no longer exist.
A `cell_id` in the record may not resolve to a live definition, and any analytics surface must handle that rather than assume a join succeeds.

Also relevant: the record distinguishes **events** (written by the runtime, from what it observed) from **memory** (written by agents, as claims).
An analytics question that mixes the two is asking a different question than it looks like.

## Resolution

Resolved 2026-08-23.
The operator had no prior view on this and asked for research before deciding, so the candidate list was checked against practitioner consensus rather than accepted as written.

### The four questions

Everything the schema carries must serve one of these. Anything else is cut.

**1. What is a successful run costing me, by cell, runner and model, over time?**

Entities: `run`. Fields: `cost`, `tokens`, `runner`, `model`, `cell_id`, `outcome`, `ts`.
No new edges.

**2. How often did I have to step in?**

Entities: `run`. Events: `operator_intervened`, `operator_touched`, and operator-initiated `re-scope` / `re-run` from `16`.
No new edges.

**3. How much rework is a task taking?**

Entities: `run`, `task`. Edge: `run -> re-scoped-from -> run`, and the same for `re-run`.
The events already exist from `05` and `16`.

**4. Which lessons actually reduced failure rate after adoption?**

Entities: `run`, `memory`. Edge: **`run -> consulted -> memory`**.
This is the only edge on this ticket that does not already exist.
It is cheap because `02` routes all memory reads through the MCP face, so the read is already observed.

### Cut

- **Which files or areas repeatedly break, and under which harness or model.**
  Requires a **file entity** plus path extraction from every tool call. It is the only expensive item on the list, and `08` established that **artifact** is the general concept - a social media cell has no files that "break", so half of farseer could never answer it. Cut, and the file entity is never built.
- **What dead ends have already been explored.**
  Not an analytics question. It is a memory write plus a memory read, and `02` already provides both tiers. Cut from here; it belongs to whatever ships the memory conventions.

### What the research changed

Two of the four above are not what the ticket proposed, and both changes came from checking practitioner consensus.

**Cost per *successful* run, not cost per run.**
Agentic coding averages roughly 1 to 3.5 million tokens per task **including retries and self-correction loops**, so a raw cost figure cannot distinguish paying for work from paying for thrashing.
The fix costs nothing: `05` already gives every run an outcome and a budget, so the metric is a join that was already available and simply was not specified.

**Intervention rate was missing from the candidate list entirely.**
It appears in every practitioner metric set found, phrased as how often a human has to step in.
Farseer already emits `operator_intervened` and `operator_touched` from `05` and `07`, so it is **free**.

It is also the metric that most directly measures whether farseer is working at all.
**A fleet you have to babysit is not a fleet.**
Adding it costs one query and nothing in the schema.

### What was deliberately not adopted

Latency p50 and p95, CSAT, escalation rate, ROI, automation rate.

These recur across the same sources but are **team and enterprise metrics**.
Farseer is one operator on one machine, per `01`.
Percentile latency across a fleet of one tells the operator nothing they will not already have noticed, and the rest measure an organisation rather than a tool.

Recording this explicitly so a later reader does not mistake their absence for an oversight.

### Schema consequence

The full set of entities analytics requires:

- `run` - with `cost`, `tokens`, `runner`, `model`, `cell_id`, `outcome`, `ts`
- `task` - to group runs
- `memory` - with the `consulted` edge back to `run`

**Three entities and two edge kinds.**
No file entity. No artifact entity for analytics purposes. No separate cost table.

From `02`: the record outlives deleted cells, so a `cell_id` may not resolve to a live definition and every one of these queries must tolerate that rather than assume the join succeeds.
Also from `02`: **events** are runtime observations and **memory** is agent claims.
Question 4 deliberately spans both, and that is the one place it is correct to do so - it correlates a claim with an observed outcome, which is the entire point of asking it.

### Sources

- [AI Agent Evaluation Metrics: A 2026 Guide](https://aiagentsquare.com/blog/ai-agent-evaluation-metrics)
- [AI Agent Monitoring: Track Token Usage, Costs, and Performance](https://www.mintmcp.com/blog/ai-agent-monitoring)
- [AI Agent Development Cost: Real Cost per Successful Task](https://www.codebridge.tech/articles/ai-agent-development-cost-real-cost-per-successful-task)
- [AI Coding Costs 2026](https://www.morphllm.com/ai-coding-costs)

### Tickets this informs

- `09 store decision` - now unblocked. The analytical workload is **three entities, two edge kinds, four queries**. That is small, and it is a strong argument against reaching for a graph database. The heavy workload remains `02`'s sequential append and `seq >` range scans.
- `02 record scope` - the `run -> consulted -> memory` edge must be emitted when a worker reads memory through the MCP face. That is a new event kind rather than a new category.

## Amended 2026-08-23: compaction segments cost queries

Findings: [context-compaction.md](../research/context-compaction.md).

Question 1 above is **cost per successful run**. Compaction complicates it.

After a compaction the conversation is re-rendered, so cumulative input tokens either reset or double-count depending on the harness.
A cost query that sums naively across a compaction boundary is **wrong, and wrong in the direction of overstating spend**.

So `context_compacted` is not only a diagnostic event, it is a **segmentation boundary for cost queries**.
Any cost aggregate must either segment on it or state that it does not.

## Amended by 23

Two changes.

**`abandoned` runs are excluded from the cost-per-successful-run denominator**, because they cost nothing and including them would flatter the metric.
They are **surfaced as a planning signal** instead: a manager that abandons half of what it queues is planning badly, which is arguably more worth seeing than a run that merely failed.

**Nested cost now equals authorised cost.** `23` made budgets draw down rather than be compared, so question 1 above reports what the operator actually authorised rather than a figure nesting can inflate invisibly.

## Amended by 24

**UI state is excluded from every query here.**

It is view preference rather than evidence, and it carries no `seq`, so it cannot appear in a cursor scan.
