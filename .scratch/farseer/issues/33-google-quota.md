# 33 google quota

**Status:** **reopened and answered 2026-08-28.** The negative answer below was correct about `agy` and wrong about the world - see the reversal at the foot of this ticket.
**Researched:** 2026-08-27, against `agy 1.1.13` on this machine, plus the operator's prior art in `D:\Dev\baby-menu-win`.

## The question

`27 quota accounting` runs on a runner reporting its subscription window. `codex-app-server` is the only runner that does. The operator has a Google subscription and `agy` on the machine, so: can farseer read a Google quota the same way?

## The answer

**No.** Not from anything farseer can reach without becoming a different kind of program.

The quota exists and is not small print - Antigravity meters hard, with a two-window shape farseer would recognise immediately: a ~250-unit sprint refreshing every 5 hours and a ~2,800-unit weekly baseline, and exhausting the weekly one locks the account out for the rest of the week. Google has moved these repeatedly through 2026. It is exactly the shape `27` was built for. It is simply not exposed anywhere a runner adapter can read.

Four routes were checked and all four are closed:

| route | result |
|---|---|
| the `stream-json` event stream | `init` carries `model`, `cwd`, `tools`, `permission_mode`; `result` carries token counts. **No quota field, no cost field.** |
| `agy` subcommands | `agent`, `agents`, `changelog`, `help`, `install`, `models`, `plugin`, `update`. No usage or credits command. |
| `/usage` and `/credits` | Real, and **TUI-only**. Sent through `-p`, agy treats the text as a prompt: the model started probing the filesystem to guess what was meant. Not a data path. |
| the IDE's local state | The operator's own prior art already settled this. See below. |

## The prior art already said so, and it was right

`D:\Dev\baby-menu-win/extensions/recipes/antigravity-quota.html` is a recipe for a quota widget that deliberately renders **unavailable**, written after inspecting the Antigravity IDE's own state. Its finding, which this ticket confirms rather than repeats:

- `antigravityUnifiedStateSync.modelCredits` holds sentinel entries, not a balance - the values do not move with usage and carry no total, percentage or reset window.
- `antigravityUnifiedStateSync.userStatus` is an undocumented nested protobuf with no quota-labelled field. Parsing it by field number is guesswork against a wire format Google renumbers at will.
- `antigravityUnifiedStateSync.oauthToken` exists and the recipe says not to touch it, because there is no documented endpoint it would authenticate against - extracting a credential for no purpose.

That recipe states the rule this project would have arrived at independently: **a wrong number here is worse than no number, because the operator would plan real work around it.** It is `27 quota accounting`'s refusal of a derived percentage and `10 runner inventory`'s observed-never-advertised rule, reached from a different direction by the same person.

One correction to it: the recipe predates the CLI growing `/usage` and `/credits`. Those exist now. They are still not readable headlessly, so the conclusion stands - but the reason has changed from *nothing exists* to *what exists is behind an interactive surface*.

## What farseer does

Nothing new. `control_of("agy").quota` is already `false`, and the settings menu already tells the operator "reports no quota, so exhaustion arrives as a failure" before they pick it. That was the honest answer before this research and it is the honest answer after.

The `runners.toml` entry gives agy its own `account = "google"`, which is not a quota but is the right bookkeeping: `27` keys windows by declared account, and agy is not spending the ChatGPT subscription.

## Not done, and why

**Community tooling exists** - at least one CLI claims to query an Antigravity quota API, and there is an open issue on the CLI asking for credits per request. Shelling out to a third-party binary to obtain a number farseer then presents as fact would put an unreviewed dependency inside `12 autonomy and deny list`'s boundary and inside the record. If that endpoint becomes documented, farseer should call it directly or not at all.

**Reading the IDE's SQLite state** would mean farseer parsing another vendor's undocumented protobuf and presenting the result as a window. That is the derived number `27` refuses, with extra steps.

## Re-check when

- Google documents an Antigravity usage endpoint, or
- `agy` grows a non-interactive usage output - `agy usage --json`, or `/usage` becoming expandable under `-p`, which is the smallest change that would flip this.

Verify live on the machine before writing a parser: read the real source, print the real response. Same rule the recipe states, and the same rule `10 runner inventory` states.

## Sources

- [Plans | Google Antigravity Docs](https://antigravity.google/docs/plans/)
- [Model Quotas (/usage) | Google Antigravity Docs](https://antigravity.google/docs/cli/commands/usage/)
- [AI Credits Command (/credits) | Google Antigravity Docs](https://antigravity.google/docs/cli/commands/credits/)
- [Usage and Quota - antigravity-cli issue #46](https://github.com/google-antigravity/antigravity-cli/issues/46)
- [Report credits used per request - antigravity-cli issue #332](https://github.com/google-antigravity/antigravity-cli/issues/332)
- [Google has tripled Gemini usage limits for Antigravity, twice](https://9to5google.com/2026/05/21/google-has-tripled-gemini-usage-limits-for-antigravity-twice/)
- `D:\Dev\baby-menu-win/extensions/recipes/antigravity-quota.html` (local prior art)

---

## Reversed 2026-08-28: the answer came from a different binary

`omp usage --json` reports the Antigravity quota, live, on this machine:

```
google-antigravity:google:default:daily     | Usage (Google)    | 0/100 percent | resets in 4h59m
google-antigravity:anthropic:default:daily  | Usage (Anthropic) | 0/100 percent | resets in 4h59m
google-antigravity:openai:default:daily     | Usage (OpenAI)    | 0/100 percent | resets in 4h59m
```

It reports Codex's two windows beside them, with `metadata.email` naming the account on both.

### The re-check condition named the wrong binary

This ticket said to re-check when Google documents an endpoint, **or when `agy` grows `agy usage --json`**.
Neither happened.
The condition assumed the answer would come from the harness that *owns* the account, and it came from a harness that merely **shares the login** - omp is authenticated to Antigravity and reads the same quota through the same credential.

That is the transferable part.
A negative answer about a capability was scoped to one binary, and the capability was never a property of the binary.
**A re-check condition should name the capability, not the tool** - "when anything on this machine can read a Google quota non-interactively", not "when agy can".

### What was kept from the negative answer

Everything else in it stands, and one line especially:

> a wrong number here is worse than no number, because the operator would plan real work around it.

So farseer passes omp's numbers through and computes none of its own, which is `27 quota accounting`'s standing rule for `used_percent` rather than a new one.

### Built

[`farseer_runner::omp_usage`](../../../crates/farseer-runner/src/omp_usage.rs), surfaced on `GET /v1/quota` with `source: "omp usage"` beside the recorded windows.

**A read, never an append.** `27` made the record's windows a log of transitions *observed by a run*; this snapshot belongs to no run and no cell, and writing it would need a cell id the record has no honest value for. `27` also made current state derived, and a live poll is the most derived thing there is.

Two details the fixture exists to hold:

- **Three Antigravity windows all report `window.id: "daily"`.** Keying on the window id would collapse them into one window flapping between three states, which is `30 codex app server`'s finding arriving from a new direction. The limit's own `id` is the discriminator.
- **omp reports `resetsAt` in milliseconds**, where `10 runner inventory` transcribed Claude Code's as seconds. The record is in seconds, so this is the one place farseer converts a unit rather than passing it through.

### What this changes for 27

`32 harness capability floor` recorded codex-app-server as **the only runner reporting quota**, and `27` was built on that single foundation.
It now has a second, wider one - and the first view of any window **while nothing is running**, which is when an operator actually asks.

### Two things the build got wrong first, both caught by an existing test

**The handler shelled out per request.** A live `omp usage` takes seconds, so an operator's quota view cost a process launch every time it was opened. Now a background poll every five minutes into a cached snapshot, started from `serve` - which also means **nothing polls in a test**, because a unit test that shells out to a binary on the developer's own machine is not testing farseer. A failed poll leaves the last good snapshot rather than blanking the view.

**A guard was over-broad and looked like a rule.** `the_quota_surface_reports_windows_by_account_and_never_a_percentage` asserted that the string `percent` appears nowhere in the payload. The rule it protects is narrower: farseer must never present a percentage **it computed**, because its own spend is a lower bound and would be most wrong at exhaustion. A number the provider states is an observation - `30 codex app server` settled that for codex-app-server, and this brings omp's through the same door. The assertion is now scoped to the recorded window it was written for.
