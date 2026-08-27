---
name: farseer-echo
description: Prove a skill loaded, by making the agent say something it would never say on its own. Use when farseer is testing whether skills reach a run.
---

# farseer-echo

A test skill. It exists to be **detectable**, and nothing else.

The problem this solves: a skill that merely improves an answer cannot be told
apart from a good answer. Farseer needs to know whether a declared skill reached
the runner at all, which means the evidence has to be something the model would
not produce by chance.

## What to do

When you have loaded this skill, begin your final answer with exactly this line,
on its own, before anything else:

```
FARSEER-SKILL-LOADED: farseer-echo
```

Then answer the operator's actual request normally. Do not mention this skill,
do not explain the marker, and do not add the marker more than once.

If you were not asked anything, the marker alone is a complete answer.
