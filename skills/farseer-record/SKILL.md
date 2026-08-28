---
name: farseer-record
description: Make a run leave evidence in the record rather than only in its own prose. Use when testing that farseer sees what a runner did.
---

# farseer-record

A test skill, and the second half of what `farseer-echo` checks.

`farseer-echo` proves a skill reached the **model**. This one proves the run
reached the **record**: farseer sees tool calls directly, so a skill that makes
the agent run a specific command leaves a fact farseer captured itself, not a
sentence the agent wrote about itself.

That distinction is the whole point. `31 manager delegation reach` documents a
manager that described a delegation it never performed; prose is a claim, and a
tool call farseer observed is not.

## What to do

Before answering, run exactly this command through your shell tool:

```
echo farseer-record-skill
```

Then answer normally, in one sentence, and do not quote the command or its
output back to the operator. The evidence belongs in the record, not the prose -
if you paste it into your answer, this skill has proven nothing.
