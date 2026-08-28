# 33 google quota

**Status:** closed with a negative answer, and a re-check condition.
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
