# Prototype: the operator surface

Throwaway. Answers one question, then most of it gets deleted.

> Three variants of farseer's operator surface, switchable via `?variant=`, in one static file.

Open [`index.html`](index.html) in a browser.
Cycle with the bar at the bottom or the left and right arrow keys.
Every verb button is a stub that says which `POST` it would have been.
Data is fixtures at the top of the file: three channels, nine runs, three quota windows.

## The question

The map keeps UI shape as fog: "manager chat, fleet view, board, graph explorer ... what remains is layout and the surfaces themselves".
Two things have to come out of looking at these:

1. **What the operator looks at first**, given a channel may be project status, content creation, or trading monitoring - the same primitive with a different roster.
2. **Which of `05 run state model`'s verbs are reachable from the surface, and from where.**
   `steer`, `re-scope`, `cancel`, `re-run` are the manager's four; `observe`, `take over`, `release` are the operator's control axis from `07 attach semantics`.

## The three

| | A - channel rail | B - canvas | C - blotter |
| --- | --- | --- | --- |
| First thing you see | one channel's conversation | every channel at once, as tiles | every **run** at once, as rows |
| Navigation | pick a channel, then a run | none - arrange tiles instead | none - sort a column instead |
| Kanban | a tab inside the channel | a tile that expands to an overlay | a mode over the same rows |
| Verbs live | on the selected run, in the right inspector; `steer` doubles as the composer | inline on each run line, two at a time, plus a fleet-wide "needs you" tile | in the right drawer, all verbs for the selected run |
| Scales badly when | many channels - the rail becomes a list you scroll | many channels - the canvas needs paging | few runs - a nine-row table is a waste of a screen |
| Steals from | Slack, Cline | [block/berd](https://github.com/block/berd)'s canvas, which the map already flagged | a trading blotter, Datadog's monitor list |

All three derive liveness rather than reading it, gate the verb list on lifecycle and control so no surface offers a verb the runtime would refuse, and show quota as `allowed` / `exhausted_until` / `unknown` with farseer's own spend, never a percentage.

## What to feed back

The useful answer is usually a mix - "B's needs-you strip on top of A's thread".
Say which one is the **home screen**, and whether the board is a place you go or a lens you switch on.

## Then

The winner gets folded into a real UI-surface ticket and the rest is deleted, per the prototype skill.
Nothing here is production code: no tests, no error handling, no API.
