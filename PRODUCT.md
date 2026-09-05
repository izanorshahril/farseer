# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Farseer is for a local operator coordinating several AI managers and workers from one Windows desktop application.
The operator needs to understand what is running, what needs attention, where work is happening, and what the fleet has said or spent.

## Product Purpose

Farseer turns a configured fleet of cells and runners into one controllable operator surface.
The top manager receives every request and decides whether to handle it, delegate to a worker, or call another cell.
Success means the operator can start work, follow it, intervene when needed, and recover the resulting record without reconstructing state from runner terminals.

## Positioning

Farseer is a fleet control plane rather than a single-agent chat client.
Its differentiator is one top manager routing work across policy-bound cells while preserving run lifecycle, control, liveness, quota, memory, and evidence in one local record.

## Operating Context

The product runs as a Tauri desktop shell backed by Rust and a React client.
It works with local repositories and authorized project roots, creates isolated workspaces, supervises native runner processes, and records their observable output.
The operator moves between projects, conversations, active runs, delegated work, quota windows, and intervention controls during long-running desktop sessions.

## Capabilities and Constraints

The Rust runtime remains headless and exposes `/v1`; the UI is a replaceable client and never holds the operator token.
Every AI request goes to the top manager, while lifecycle and control verbs act directly on a selected run.
A widget displays a cell and never addresses one.
The runtime stores UI arrangement as an opaque blob and never parses frontend layout.
The canvas remains the product home and any other product surface remains a widget rather than a second runtime or plugin ABI.
The accepted home-screen prototype is now the production direction implemented in `ui/`.
Chat remains deferred to a separate design pass.

## Brand Commitments

The product name is Farseer.
The production home is a close structural and visual homage to Berd's current home screen while translating single-agent concepts into Farseer's fleet model.
The local clock is an optional home widget that can be arranged or hidden like other canvas objects.
The later chat experience may draw from Claude Code desktop, but that reference does not govern the home canvas.
Product language uses the locked nouns cell, runner, worker contract, and cell call.

## Evidence on Hand

The implemented product behavior and decision record live in `AGENTS.md` and `.scratch/farseer/issues/`.
The current operator surface lives in `ui/` and is the production implementation of the approved home direction.
Earlier Farseer artboards live in `.scratch/design/` and are anti-reference for this replacement direction.
Berd's public repository, product record, and design system at `block/berd` are the approved external reference.
No customer testimonials, commercial claims, or usage benchmarks are available and none may be invented.

## Product Principles

- Preserve fleet truth when borrowing a single-agent form.
- Keep routing with the top manager and direct verbs with runs.
- Prefer calm operational clarity over dashboard density.
- Make current state recognizable before exposing controls.
- Keep the runtime headless and the client replaceable.

## Accessibility & Inclusion

The operator surface must remain fully keyboard operable, expose semantic landmarks and live status, and preserve visible focus.
It must meet WCAG AA contrast and work at desktop and narrow web widths.
