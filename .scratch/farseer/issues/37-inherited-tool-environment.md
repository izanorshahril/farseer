# 37 inherited tool environment

**Status:** resolved 2026-08-29. The decision was already made and unstated; this ticket found that out, stated it, and applied it where it had been missed.
**Found:** 2026-08-29, probing what `36 tool grant enforcement` left open - what it would mean to enforce a `kind = "tool"` roster entry farseer cannot itself serve.

## The finding

**Every runner farseer spawns loads the operator's own tool servers, and farseer's grants are additive rather than exclusive.**

A `goose acp` session given `mcpServers: []` - farseer asking for nothing at all - reports:

```json
"extensionResults":[
  {"name":"developer","success":true},{"name":"skills","success":true},
  {"name":"scheduler","success":true},{"name":"summon","success":true},
  {"name":"Extension Manager","success":true}]
```

`codex app-server` does the same in its own vocabulary, and `30 codex app server` recorded it at the time without drawing the conclusion:

```
"method":"mcpServer/startupStatus/updated","params":{"name":"node_repl","status":"ready"}
"method":"mcpServer/startupStatus/updated","params":{"name":"codex_apps","status":"ready"}
```

Both started **before farseer named a single server**, and both kept running when farseer added one.

This is `32 harness capability floor`'s skills finding in a second place.
That ticket found every harness discovering skills from the operator's home directory, so a run reached whatever happened to be installed and the cell had no say; farseer now denies that discovery and passes only what a cell names.
Nobody has done the same for tool servers, and `12 autonomy and deny list`'s whole premise - **the tool grant is the only isolation v1 has** - is stated against a set farseer does not control.

## What is built

**Visibility, and only that.** Every tool server a session loads - farseer's and the operator's alike - is recorded as a `status_changed` event at the start of the run.

`21 a2a conformance`'s rule about a run's inputs and `02 record scope`'s about the record both want this, and neither is served by a list of failures: before now farseer recorded only the servers it offered that did not start, so five extensions that loaded perfectly were invisible.

## What is not decided

### 1. Should farseer deny the inherited set at all?

There is a real argument for **no**. The operator installed those extensions on purpose, a coding cell without them may be worse at its job, and `12` already conceded that a cell granting shell has granted everything - most cells do grant shell.

There is a better argument for **yes**, and it is `32`'s: a run that reaches whatever happened to be installed is not reproducible, and the record cannot explain a difference between two runs of the same contract. `05 run state model` made the contract immutable so that "what was this worker allowed to do" has one answer. It has one answer and the answer is incomplete.

### 2. Farseer cannot deny it uniformly, and that is the hard part

Probed 2026-08-29:

| runner | deny inherited tool servers |
| --- | --- |
| opencode-acp | **`--pure`**, "run without external plugins" |
| goose-acp | **no flag** - `--with-builtin` only *adds* |
| codex-app-server | **no** - `config.mcp_servers` merges; the operator's still started |
| pi, omp | `--tools` allowlists what the model may call, which is the adjacent thing and not this one |

So a uniform "deny discovery" is not available, and `32`'s answer - deny everywhere, pass only what the cell names - **cannot be repeated here**.

That leaves the shape this project has used three times for exactly this: **refuse where a person can see it**. A cell that asks for an isolated tool environment gets it on the runners that can, and a refusal naming the runner on the ones that cannot.

### 3. What a `kind = "tool"` roster entry becomes

`36` established these name cell-level capabilities - `post`, `draft-file`, `cargo-test` - on an axis no runner shares, and left them recorded and unenforced.

The plausible answer is now visible: **a tool entry names an MCP server declared in runner config**, machine-wide beside `account` and `usd_micros_per_mtok`, and farseer forwards exactly the granted ones. Every transport built in `31 manager delegation reach` can already carry a server list, so the mechanism exists.

That makes "the roster is the grant" true rather than aspirational - but only as far as question 2 allows, because a granted set that arrives *alongside* the operator's own is a grant list that adds and never subtracts.

## Not decided here

Whether the deny list gets a second look. `12` settled that it prevents mistakes rather than attacks, and nothing found here changes that.

---

## Resolution, 2026-08-29: there was no decision to make

Question 1 asked whether farseer should deny the inherited set at all.
Reading the code answered it: **farseer already does, everywhere a flag exists.**

| runner | denial | since |
| --- | --- | --- |
| claude-code | `--strict-mcp-config` | `10 runner inventory` |
| pi, omp | `--no-extensions`, `--no-skills` | `32 harness capability floor` |
| opencode-acp | `--pure` | **this ticket** |
| goose-acp | none available - `--with-builtin` only adds | - |
| codex-app-server | none - `config.mcp_servers` merges | - |

Three runners were already isolated and nobody had written down that this was the rule.
The two ACP runners and the app-server were simply newer than the habit, and `opencode --pure` was a flag nobody had looked for.

**So this was an inconsistency, not a design question**, and the ticket's framing was wrong: it asked whether to adopt a policy that had been in force since `10`.
That is the second time on this map that a rule the code followed was never stated - `36 tool grant enforcement` found the same with the tool level's default.

Proven live: an `opencode-acp` manager launched with `--pure` answered `JUNIPER`, so denial costs nothing on the runner that offers it.

### What stays open, and it is now precise

**`goose-acp` and `codex-app-server` inherit, and farseer cannot stop them.**

That is not a gap to close by trying harder; it is a property of those two binaries, probed rather than assumed:

- `goose acp --help` offers `--with-builtin` and nothing that subtracts.
- `codex app-server` starts `node_repl` and `codex_apps` from the operator's config before farseer names a server, and keeps them when farseer adds one.

What farseer does instead is what it does everywhere else it cannot guarantee something: **record it.** Every tool server a session loads is in the record, farseer's and the operator's alike, so a run on those two says what it reached even though it could not choose it.

The remaining choice - refuse a cell that asks for isolation on a runner that cannot give it - is deliberately **not** taken. There is no field asking for it yet, and inventing one to refuse would be a policy nobody requested. When a cell needs guaranteed isolation, `36`'s pattern is sitting there: a declared field, a per-runner table, and a refusal naming the runner.

### Correction to this ticket's own framing

The finding as written said "farseer's grants are additive rather than exclusive" as though it were true everywhere.
It is true of `goose-acp` and `codex-app-server`, and false of the other four.
