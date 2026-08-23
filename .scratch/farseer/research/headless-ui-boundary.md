# Headless core / UI boundary: prior art for Farseer

Date: 2026-08-19

## Bottom line

Yes, the headless-from-v1 split is confirmed by prior art.
Every actively multi-client tool surveyed here (OpenCode, Jupyter, Docker, Syncthing, Home Assistant, Transmission, Temporal) puts the durable process behind a documented local API and treats every UI, including its own default UI, as a client of that API.
The convergent boundary for local-first tools with a possible browser UI is: a background server process with a stable local API (HTTP/REST for control, plus a push channel for live output), and a thin CLI/TUI/GUI that only renders and forwards.
For streaming to several simultaneous viewers of one live job, the convergent mechanism is an event bus inside the server fanned out over Server-Sent Events (SSE) or WebSocket, not raw log-tailing or client-side polling.
Farseer should adopt this: a Rust core process exposing one local HTTP API with SSE (or WebSocket) for live worker output, CLI and GUI as equal clients, reachable over localhost TCP by default so a browser-based UI stays possible, deliberately giving up the marginal IPC latency and permission-model precision that a named pipe or Unix domain socket would offer.

## Comparison table

| Tool | Core/UI boundary | Transport | Multi-client on one live session | Streaming mechanism | Source |
|---|---|---|---|---|---|
| OpenCode | `opencode serve` server; TUI/web/IDE/desktop are clients | Local HTTP (default port 4096), OpenAPI 3.1 spec | Yes | SSE (`/event`, `/global/event`) | https://opencode.ai/docs/server/, https://github.com/anomalyco/opencode/issues/11616 |
| Claude Code (CLI + Agent SDK) | SDK spawns the `claude` CLI binary as a subprocess | stdio, not a network API | No (SDK drives its own subprocess; not a shared live session for multiple independent clients) | Async stream of typed JSON messages over stdio | https://code.claude.com/docs/en/headless |
| Zed Agent Client Protocol (ACP) | Editor spawns agent as subprocess; protocol is the boundary | JSON-RPC 2.0 over stdio (local); HTTP/WebSocket for remote is WIP | Not documented in the spec (out of scope of the intro material) | Not documented in the spec | https://agentclientprotocol.com/overview/introduction |
| Language Server Protocol (LSP) | Language server process, separate from editor | JSON-RPC 2.0; transport unspecified by the protocol (stdio, sockets, or in-process calls all conform) | Design intent is "one server implementation, many editor clients," but each server process is normally paired 1:1 with one client connection, not fan-out to concurrent viewers of one live session | N/A - request/response plus server-to-client notifications | https://en.wikipedia.org/wiki/Language_Server_Protocol |
| Debug Adapter Protocol (DAP) | Debug adapter process, separate from editor UI | JSON-RPC-like custom framing, over stdio (single-session mode) or TCP (multi-session mode) | Multi-session mode lets a tool open several connections to one running adapter over a listening port; single-session mode is 1:1 | Event messages pushed over the same JSON channel | https://microsoft.github.io/debug-adapter-protocol/overview |
| Docker | `dockerd` daemon; `docker` CLI and other UIs are clients | Unix socket (Linux default), Windows named pipe `\\.\pipe\docker_engine` (default), optional TCP :2375/:2376 | Yes, any number of clients can open the socket/pipe/TCP port concurrently | Long-lived HTTP streaming responses (e.g. `docker logs -f`, `events`) chunked over the same connection type | https://docs.docker.com/engine/reference/commandline/dockerd/, https://github.com/docker/for-win/issues/10701 |
| Syncthing | Core sync engine; web GUI and any external tool are REST clients | Local HTTP, default `127.0.0.1:8384` | Yes, multiple REST/GUI clients can hit the same instance concurrently | REST is poll style; GUI uses a `/rest/events` long-poll endpoint for updates (not classic SSE) | https://docs.syncthing.net/dev/rest.html, https://docs.syncthing.net/users/guilisten.html |
| Jupyter | Kernel process; notebook/console/qtconsole are frontends | ZeroMQ + JSON messaging protocol, multiple sockets (shell, iopub, stdin, control, heartbeat) | Yes, explicitly: "A kernel process can be connected to more than one frontend simultaneously" | ZeroMQ IOPub pub/sub socket broadcasts kernel output to every connected frontend | https://jupyter-client.readthedocs.io/en/stable/messaging.html |
| Transmission | `transmission-daemon`; CLI/Web/GTK/Qt are RPC clients | HTTP + JSON-RPC 2.0, default `localhost:9091` | Yes, any number of RPC clients can query the same daemon | No push channel found; clients poll the RPC endpoint | https://github.com/transmission/transmission/blob/main/docs/rpc-spec.md |
| qBittorrent | Single process; WebUI is an in-process component, not a separate daemon | In-process HTTP server (WebUI/API) | Multiple browser sessions can connect, but there is no separate daemon to attach other UIs to independently of the app | No push channel found; WebUI polls the API | https://deepwiki.com/qbittorrent/qBittorrent/4-web-user-interface (secondary; official docs did not describe streaming) |
| Home Assistant | Core; frontend, apps, integrations are API clients | Local HTTP REST + WebSocket, default port 8123 | Yes, many WebSocket clients can subscribe concurrently | WebSocket API: subscribe to state-change events, pushed to every connected client | https://developers.home-assistant.io/docs/frontend/architecture/ |
| Ollama | `ollama serve` background HTTP server; CLI and any app are clients | Local HTTP, default `127.0.0.1:11434`; no named-pipe transport found | Yes, any number of HTTP clients can call the same server | HTTP chunked streaming response per request (not a shared broadcast bus for one job) | (multiple secondary sources; no single Ollama primary architecture doc found - flagged as not-found for primary source) |
| Temporal | Server (Frontend/History/Matching/Worker services); CLI and Web UI are gRPC/HTTP clients of the Frontend service | gRPC (port 7233), HTTP for Web UI | Yes, CLI and Web UI and SDK workers all connect to the same Frontend service concurrently | gRPC streaming / long poll for task and workflow updates | https://docs.temporal.io/temporal-service/temporal-server, https://docs.temporal.io/self-hosted-guide/server-frontend-api-reference |
| aider | No server at all; CLI only | N/A - deliberately no daemon | N/A | N/A (recommends tmux/screen for shared remote access) | https://github.com/Aider-AI/aider |

## 1. Prior art: how the boundary is actually drawn

OpenCode is the closest architectural analog to what Farseer is proposing.
It runs `opencode serve`, a local Hono HTTP server (default port 4096) that owns sessions, tool execution, and state; the TUI, web UI, VS Code extension, JetBrains plugin, and desktop app are all clients that "speak HTTP" to it, using a generated SDK built from the server's OpenAPI 3.1 spec.
Source: https://opencode.ai/docs/server/ and the architecture issue at https://github.com/anomalyco/opencode/issues/11616, which documents a "Local Server + Remote UI Proxy" model.

Claude Code's Agent SDK is not this pattern.
The SDK spawns the `claude` CLI binary as a managed subprocess and talks to it over stdio, streaming typed JSON messages back to the calling program; this is a library wrapping a process, not a persistent server multiple independent clients attach to.
Source: https://code.claude.com/docs/en/headless.

Zed's Agent Client Protocol (ACP) standardizes the editor-to-agent boundary as JSON-RPC 2.0 over stdio for local agents (the editor spawns the agent as a subprocess), explicitly modeled on LSP's "stop writing one integration per editor per agent" motivation; remote (HTTP/WebSocket) transport is called out as work in progress.
Source: https://agentclientprotocol.com/overview/introduction.

LSP and DAP are the canonical "one core, many frontends" precedents, but with an important nuance: LSP standardizes the protocol so any editor can talk to any conforming server, but a live server process is still normally attached to one editor client at a time - the "many clients" property is about protocol reuse across editors, not concurrent viewers of one running server instance.
Source: https://en.wikipedia.org/wiki/Language_Server_Protocol.
DAP's "multi-session mode" is the more relevant precedent for Farseer: the adapter listens on a port instead of being spawned over stdio, and a tool can open a fresh connection into the already-running adapter for each debug session.
Source: https://microsoft.github.io/debug-adapter-protocol/overview.

Docker, Syncthing, Jupyter, Home Assistant, Transmission, and Temporal all separate a long-running core process from an interchangeable UI, and all document a stable local API as the seam:
Docker via the Engine API over a Unix socket/Windows named pipe/optional TCP (https://docs.docker.com/engine/reference/commandline/dockerd/);
Syncthing via local REST plus API-key auth (https://docs.syncthing.net/dev/rest.html);
Jupyter via a ZeroMQ messaging protocol between kernel and frontend (https://jupyter-client.readthedocs.io/en/stable/messaging.html);
Home Assistant via REST + WebSocket (https://developers.home-assistant.io/docs/frontend/architecture/);
Transmission via JSON-RPC over HTTP (https://github.com/transmission/transmission/blob/main/docs/rpc-spec.md);
Temporal via gRPC to a Frontend service that CLI, Web UI, and SDK workers all call (https://docs.temporal.io/temporal-service/temporal-server).

qBittorrent and aider are the two clear deliberate non-splits found.
qBittorrent runs its WebUI as an in-process HTTP component of the single qBittorrent binary rather than a separate daemon a client attaches to (secondary source: https://deepwiki.com/qbittorrent/qBittorrent/4-web-user-interface; no official qBittorrent architecture doc was found as a primary source, flagged as such).
aider has no server mode by design: "There is no separate 'server mode' daemon - Aider is just a CLI," and the maintainers point people at tmux/screen for shared remote access instead of building daemon infrastructure.
Source: https://github.com/Aider-AI/aider.

## 2. Multi-client on one live session (highest-value question)

Confirmed multi-client-on-one-session tools, with the concrete mechanism:

- **Jupyter**: explicit and primary-sourced. "A kernel process can be connected to more than one frontend simultaneously... the different frontends will have access to the same variables."
  Mechanism: the kernel exposes a ZeroMQ IOPub socket that is inherently pub/sub - every connected frontend subscribes and receives the same broadcast output stream.
  Source: https://jupyter-client.readthedocs.io/en/stable/messaging.html.
- **OpenCode**: "Multiple clients can attach to the same instance."
  Mechanism: SSE endpoints (`/event`, `/global/event`) broadcast an internal event bus to every subscribed HTTP client.
  Source: https://opencode.ai/docs/server/, https://github.com/anomalyco/opencode/issues/11616.
- **Docker**: any number of clients can open the socket/pipe/TCP transport concurrently; `docker logs -f` and `docker events` are long-lived HTTP responses that each client streams independently from the daemon's internal log/event source, so N attached clients get N independent chunked streams of the same underlying data rather than one shared multicast connection.
  Source: https://docs.docker.com/engine/reference/commandline/dockerd/.
- **Home Assistant**: WebSocket API lets many clients subscribe to state-change events; each gets pushed updates as they occur.
  Source: https://developers.home-assistant.io/docs/frontend/architecture/.
- **Temporal**: CLI, Web UI, and SDK workers all connect to the same Frontend gRPC service concurrently to observe/drive the same workflow.
  Source: https://docs.temporal.io/temporal-service/temporal-server.
- **Transmission**: multiple RPC clients can query the same daemon, but this is poll-based JSON-RPC over HTTP - no push/streaming mechanism was found in the RPC spec.
  Source: https://github.com/transmission/transmission/blob/main/docs/rpc-spec.md.
- **Syncthing**: GUI uses a long-poll `/rest/events` endpoint (client re-requests after each batch), not persistent push (SSE/WebSocket); this still delivers near-real-time updates to multiple GUI/API clients but the mechanism is long-polling, not fan-out pub/sub.
  Source: https://docs.syncthing.net/users/guilisten.html, https://docs.syncthing.net/dev/rest.html.

Tools that explicitly do NOT support multiple independent clients on one live session, and the resulting friction:

- **Claude Code Agent SDK**: the SDK owns a private subprocess per invocation; there is no documented mechanism for a second independent client to attach to an already-running session's live output.
  This is a 1:1 pattern, not fan-out.
  Source: https://code.claude.com/docs/en/headless.
- **DAP single-session mode**: the adapter is spawned as a private subprocess of one tool over stdio; only "multi-session mode" (listening on a port) allows more than one connection into a running adapter, and that is a distinct mode a tool must opt into.
  Source: https://microsoft.github.io/debug-adapter-protocol/overview.
- **aider**: has no concept of a live session an external second client could attach to at all; the maintainers' answer to shared/remote access is tmux/screen re-attachment at the terminal level, not an application-level API.
  Source: https://github.com/Aider-AI/aider.
- **qBittorrent**: multiple browser tabs can each open a WebUI session, but because there is no separate daemon process, "attaching a second UI" only ever means opening another HTTP session against the same in-process server - conceptually a single-process multi-client case, but not documented as a first-class supported pattern with push updates (no primary-sourced streaming mechanism found).

## 3. Costs of the split in practice

- **Docker: real security cost of exposing the API.** CVE-2025-9074 (patched in Docker Desktop 4.44.3, August 2025) allowed containers running locally to reach the Docker Engine API on the host over the exposed transport, enabling attackers to control other containers, create new ones, manage images, and in some cases break out of the container.
  This is a concrete, documented cost of putting a powerful control API behind a locally reachable transport.
  Source: https://dev.to/sharon_42e16b8da44dabde6d/cve-2025-9074-docker-desktop-engine-api-exposure-patch-now-4c47, https://github.com/docker/for-win/issues/10701.
- **Docker Desktop Windows named-pipe permission friction.** By default only members of the Administrators group can access the Docker Engine through the named pipe; users routinely hit `open \\.\pipe\docker_engine_windows: The system cannot find the file specified` and similar errors, and have to explicitly opt in to exposing TCP :2375 (unencrypted) to work around pipe permission or tooling limitations.
  Source: https://forums.docker.com/t/error-response-from-daemon-open-pipe-docker-engine-windows-the-system-cannot-find-the-file-specified/131750, https://github.com/docker/for-win/issues/10701.
- **Daemon pattern criticized for local AI specifically.** A commentary on local AI inference architecture argues the background-daemon pattern used by Ollama and LM Studio "carries risks that threat models rarely capture" in regulated environments: unauthenticated HTTP ports, unsigned launch agents, independent update mechanisms, and sandbox violations.
  This is opinion/secondary commentary, not a vendor admission, but it is a directly on-topic counter-argument to putting a local agent runtime behind an always-on HTTP daemon.
  Source: https://medium.com/@michael.hannecke/sovereign-ai-on-the-endpoint-where-the-daemon-pattern-breaks-down-in-regulated-environments-f421e5ac632b (secondary source, flagged as such).
- **aider's deliberate non-split.** The project explicitly avoids daemon/server complexity to stay "a deliberately small tool," offloading remote/shared access to tmux/screen rather than building and maintaining a server boundary.
  This is direct counter-evidence that a single-operator local tool does not strictly need a server split to be usable remotely - though it also means aider cannot support two independent UIs watching the same live session, which is exactly the capability Farseer wants.
  Source: https://github.com/Aider-AI/aider.
- **No direct "regret" testimony found** for OpenCode, Jupyter, Home Assistant, Syncthing, or Temporal splitting core from UI - i.e. no primary source stating the maintainers wish they had not separated core and UI.
  Stated as "not found" per instructions, rather than inferred.

## 4. Windows local transport

Real tools choose differently depending on whether a browser must reach the API:

- **Docker Desktop on Windows**: default is the Windows named pipe `\\.\pipe\docker_engine`, restricted by default to the Administrators group; TCP `localhost:2375` (unencrypted) is available as an explicit opt-in setting for tools that cannot speak named pipes, and Docker Engine 29.5+ additionally allows configuring `dockerd` on Windows to listen on a Unix domain socket.
  Sources: https://github.com/docker/for-win/issues/10701, https://learn.microsoft.com/en-us/virtualization/windowscontainers/manage-docker/configure-docker-daemon.
- **Syncthing**: localhost TCP HTTP (`127.0.0.1:8384`) on every platform including Windows, specifically because the GUI is a browser page; no named-pipe or Unix-socket option is documented.
  Source: https://docs.syncthing.net/users/guilisten.html.
- **Ollama**: localhost TCP HTTP (`127.0.0.1:11434`) on every platform including Windows; no Windows named-pipe transport was found in any source consulted.
  (Multiple secondary sources agree on this; no single Ollama primary architecture document was found, flagged as such.)
- **OpenCode**: localhost TCP HTTP (default port 4096) with an OpenAPI/SSE server, the same on Windows as elsewhere, because the web UI and IDE-extension clients need ordinary HTTP/fetch/EventSource reachability.
  Source: https://opencode.ai/docs/server/.

Tradeoffs found across sources:

- **Multi-client support**: TCP loopback and Unix/named-pipe sockets both support arbitrarily many concurrent client connections; this is not a differentiator.
  Source: https://digitalgarden.bhekani.com/tcp-vs-unix-sockets-vs-named-pipes-vs-shared-memory/.
- **Browser reachability**: a browser cannot open a named pipe or Unix domain socket directly - it can only reach `http(s)://` (or `ws(s)://`) endpoints, so any UI reachable from a normal browser tab (reason (c) in Farseer's brief - running old/new UI or UI variations concurrently) requires either TCP loopback HTTP, or a named-pipe/socket transport bridged through a local HTTP shim.
  This point is stated directly in the tradeoff discussion found: "If browsers are clients ... named pipes or Unix sockets with an HTTP bridge may be necessary."
  Source: https://linuxvox.com/blog/sockets-on-same-machine-for-windows-and-linux/ and https://digitalgarden.bhekani.com/tcp-vs-unix-sockets-vs-named-pipes-vs-shared-memory/ (synthesis of both, both secondary but converge with primary-sourced product choices above).
- **Performance**: Unix domain sockets and named pipes avoid the loopback TCP/IP stack and are measurably faster for local IPC (cited: "TCP loopback is approximately 3x slower than Unix sockets" for local IPC; named pipes about 30% faster than Unix sockets at small block sizes).
  These are microbenchmark claims from a single secondary source, not vendor-published numbers, so treat as directional only.
  Source: https://linuxvox.com/blog/sockets-on-same-machine-for-windows-and-linux/.
- **Permissions/ACL defaults**: named pipes and Unix sockets are isolated from the network by default and support OS-native ACLs (Docker's pipe defaults to Administrators-only); a TCP loopback port is reachable by any local process/user unless the application layers its own auth on top, which is why Syncthing requires an API key even though it binds to `127.0.0.1` (source: https://docs.syncthing.net/dev/rest.html) and why the Ollama/LM Studio daemon-pattern critique above specifically calls out "unauthenticated HTTP ports" as a risk.

## 5. Convergence

The tools that need a browser-reachable UI (OpenCode, Syncthing, Home Assistant, Ollama, Docker Desktop's own dashboard) converge on localhost HTTP, with a push mechanism layered on top for live data: SSE for OpenCode, WebSocket for Home Assistant, long-polling for Syncthing.
The tools that do not need browser reachability (Docker Engine CLI, Jupyter kernels, DAP single-session mode) are willing to use OS-native local IPC (named pipe, Unix socket, ZeroMQ, stdio) for the performance and ACL benefits.
This convergence is driven by browser reachability, not technical merit: every primary source that discusses the choice explicitly frames it as "the UI is a browser page, so we need HTTP" (Syncthing, OpenCode) or contrasts pipe/socket restriction against the need for a TCP fallback for less-capable clients (Docker on Windows).
No source claims TCP loopback HTTP is technically superior to a named pipe or Unix socket for a pure CLI-to-daemon connection; the technical tradeoff literature found (https://linuxvox.com/blog/sockets-on-same-machine-for-windows-and-linux/, https://digitalgarden.bhekani.com/tcp-vs-unix-sockets-vs-named-pipes-vs-shared-memory/) favors named pipes/Unix sockets on raw performance and isolation grounds.

## Recommendation for Farseer

- **Core**: single Rust binary running a local server process (the "runtime core"), exposing one HTTP + JSON API for control (start/stop/inspect cells and workers, send input) and one push channel for live output.
- **Transport**: localhost TCP HTTP, bound to `127.0.0.1` by default, not a Windows named pipe and not a Unix domain socket.
  This is chosen specifically because reason (c) in the brief - running old/new UI, or UI variations, concurrently, including a browser-based UI - requires ordinary browser reachability, which a named pipe cannot provide without an additional HTTP bridge process; that bridge would just re-add the TCP server this recommendation already proposes, at extra complexity.
- **Streaming mechanism**: Server-Sent Events (SSE), one stream per worker or one global event stream filterable by worker/cell ID, mirroring OpenCode's `/event` design (https://opencode.ai/docs/server/) and matching the fan-out pattern Jupyter proved with IOPub pub/sub (https://jupyter-client.readthedocs.io/en/stable/messaging.html).
  SSE over WebSocket because attach-to-watch is one-directional (server to operator) for the common case, SSE auto-reconnects natively, and it is simpler to implement and secure than a WebSocket upgrade; intervention/input from the operator can go over a plain HTTP POST on the same API, so a bidirectional transport is not required for the core loop.
- **Auth**: local API key or bound-to-loopback-plus-random-token-per-launch, following Syncthing's precedent (https://docs.syncthing.net/dev/rest.html), specifically because TCP loopback alone is not sufficient network isolation - CVE-2025-9074 (https://dev.to/sharon_42e16b8da44dabde6d/cve-2025-9074-docker-desktop-engine-api-exposure-patch-now-4c47) is direct evidence that "it's only on localhost" is not a safe assumption when other local processes or containers can reach the same port.
- **What this deliberately gives up**: the IPC performance and OS-native ACL isolation that a Windows named pipe or Unix domain socket would provide (per https://linuxvox.com/blog/sockets-on-same-machine-for-windows-and-linux/), and the simplicity of aider's no-server model (https://github.com/Aider-AI/aider).
  Farseer accepts a background server process as a permanent extra moving part - lifecycle (start/stop/crash-recovery), a listening port to secure, and version-skew risk between core and any out-of-tree UI - in exchange for the multi-client attach capability that is the operator's explicit, stated requirement.
