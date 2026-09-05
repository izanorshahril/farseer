# Farseer Production Home Design Record

## Status and provenance

- Seed: `b452fefe`.
- Status: production `ui/` home surface is authoritative.
- The accepted static prototype at `.scratch/farseer/prototypes/berd-home/` is the approval reference for this production translation.
- This record describes the shipped production files; the source is authoritative over stale review captures after layout changes.
- Sources are `ui/index.html`, `ui/src/App.tsx`, `ui/src/style.css`, `ui/src/widgets/work.tsx`, and the other built widgets under `ui/src/widgets/`.
- `.impeccable/review/production-desktop.png` and `.impeccable/review/production-mobile.png` preserve the approved version-six composition rather than the configurable version-eight arrangement.
- The production client keeps the canvas as the only home surface, with every detailed surface represented as a widget on that canvas.

## Product intent

- Farseer is a calm local fleet workbench for an operator coordinating cells, runners, managers, workers, runs, projects, quota windows, memory, and evidence.
- The home surface exposes current work and fleet state before the operator asks the top manager what happens next.
- The surface is a close Berd-inspired canvas translation that preserves Farseer's fleet semantics and runtime controls.
- The top manager receives every AI request, while run verbs remain direct controls on a selected run.
- The full chat composition is deferred as a separate design decision, while the shipped surface provides a compact top-manager composer and a conversation widget.

## Palette and color behavior

- The production theme is light-only and declares `color-scheme: light`.
- `--bg` is `#f4f4f2` for the application surround and input backgrounds.
- `--canvas` is `#f8f8f6` for the workspace, canvas, and loading state.
- `--panel` is `#ffffff` for the sidebar, widgets, composer, and controls.
- `--panel-2` is `#f5f5f3` for secondary fills, chips, and hover states.
- `--line` is `#e5e5e1` for borders and dividers.
- `--line-strong` is `#d4d4cf` for emphasized borders and scrollbar thumbs.
- `--ink` is `#252525` for primary text and dark controls.
- `--dim` is `#666662` for secondary text.
- `--faint` is `#73736e` for tertiary text and metadata.
- `--accent` is `#4d72ff` and `--accent-soft` is `#e9edff` for focus, selected projects, and authored-widget status.
- `--ok` is `#23805f` for healthy, live, and allowed states.
- `--warn` is `#98601b` for context pressure and caveats.
- `--bad` is `#c14539` for errors, failed states, and destructive affordances.
- Inline state colors include `#ffebe7` for failed or run-alert avatars, `#e2f2ed` for cell and runner avatars, `#a371f7` for operator thread edges, `#58a6ff` for gate links, `#3fb950` for provider meters, `#d29922` for warning meters, and `#f85149` for bad meters.
- Selection uses the accent background with `#ffffff` text.
- Healthy status dots use an eight-pixel `--ok` circle with a three-pixel `rgba(35, 128, 95, 0.12)` ring.
- No production dark-theme token set is shipped in this stylesheet.

## Typography

- Body text uses `Inter`, then `Segoe UI Variable Text`, then `Segoe UI`, then the sans-serif generic.
- The default body is `13px/1.5` with antialiasing enabled.
- The mono stack is `ui-monospace`, then `Cascadia Mono`, then `Consolas`, then the monospace generic.
- The brand is `18px`, weight `680`, and letter spacing `-0.035em`.
- Widget headings are `13px`, weight `680`.
- The clock face uses `clamp(38px, 4vw, 58px)`, weight `540`, line height `1`, letter spacing `-0.05em`, and tabular numerals.
- Clock supporting text is `12px` with an `8px` top margin.
- Navigation and widget-toggle labels use `12px` to `13px`, with emphasized labels at weights `620` to `650`.
- Group labels use `10px`, weight `700`, uppercase transformation, and letter spacing `.07em`.
- Widget subtitles and compact metadata use `10px` to `12px` in dim or faint colors.
- Badges use `11px/1` in the mono stack, with size badges reduced to `9px`.
- Provider and run detail labels use mono uppercase text where the record needs a scannable technical key.

## Shell geometry

- The application is a full-height two-column grid with `248px` sidebar, remaining workspace, an `8px` gap, and `8px` outer padding.
- The sidebar and workspace use a one-pixel `--line` border and an `18px` radius.
- The sidebar uses `12px 10px` padding, a `40px` brand row, and overflow hidden.
- The fleet switcher is at least `52px` high, has `10px 0 12px` margin, `6px 8px` padding, a `13px` radius, and a `32px` fleet mark.
- The fleet switcher uses a `32px minmax(0, 1fr)` grid with a `9px` gap.
- The widget navigation scrolls independently and groups built widgets under `Home widgets` and discovered widgets under `Authored`.
- The sidebar footer is a two-column status row with an `8px` status dot, `10px` gap, `12px 9px 2px` padding, and a top divider.
- The workspace is a flex column with hidden overflow, `--canvas` background, and the same border and radius as the sidebar.
- The top bar is `52px` high, flexes to the full workspace width, and uses `0 12px 0 18px` padding with a bottom divider.
- Top actions use a four-pixel gap, while saved-state text has an eight-pixel right margin.
- The operator profile is `30px` square with a four-pixel left margin and a circular shape.

## Canvas and grid

- The canvas is a flowing CSS grid whose columns use the operator-configured widget-unit width, whose auto rows use the configured widget-unit height, and whose row and column gaps are twelve pixels.
- The canvas scrolls independently and uses `14px 14px 154px` padding to keep the bottom composer clear.
- The canvas background repeats a `.8px` radial dot with a `.9px` transparent cutoff, `20px` size, and `10px 10px` position.
- The grid is the sole rendered interpretation of layout spans, and widgets flow rather than defend freeform coordinates.
- Widget spans are clamped to one or two units on each axis and snap to integer values.
- The default layout version is `8`.
- The default mounted order is `conversation`, `work`, `fleet`, `capacity`.
- Every default widget starts at `1x1`; the default unit is `300px` wide by `220px` high.
- The first desktop viewport therefore places Conversation, Work, Fleet, and Capacity across the canvas, with the composer floating over the lower canvas.
- Each widget is a vertical card with a one-pixel line, an `18px` radius, panel fill, standard shadow, and a flexing body.
- Widget headers are at least `48px` high with `9px 12px` padding, an eight-pixel gap, and a bottom divider.
- Widget bodies use `11px 13px` padding and scroll their own content when needed.
- The grip is a focusable `28px` target, offset by `-3px 0 -3px -5px`, and the resize handle is a `24px` target in the lower-right corner.
- A size badge exposes the current snapped span as `w x h`.

## Widget registry and fleet composition

- The built registry mounts Conversation, Work, Fleet, Capacity, Clock, Delegation, Activity, Runners, Runs, Run, and Projects.
- Registry entries expose an id, a title, a subtitle, and a render function.
- The sidebar presents each registry entry as a toggle with avatar, title, subtitle, mounted state, and `aria-pressed` state.
- Additional authored widgets are discovered from `/__widgets`, labeled `authored`, and rendered through the sandbox gate.
- Conversation is titled `Conversation` with subtitle `you and the top manager`.
- Work is titled `Work` with subtitle `board, conversations and graph`.
- Fleet is titled `Fleet` with subtitle `cell definitions`.
- Capacity is titled `Capacity` with subtitle `provider accounts`.
- Clock is an optional widget titled `Clock` with subtitle `local time` and can be mounted or hidden from the sidebar or top-bar clock button.
- Delegation is titled `Delegation` with subtitle `manager and workers`.
- Activity is titled `Activity` with subtitle `record, live`.
- Runners is titled `Runners` with subtitle `active processes`.
- Runs is titled `Runs` with subtitle `recent work`.
- Run is titled `Run` with subtitle `selected run`.
- Projects is titled `Projects` with subtitle `authorized folders`.
- Settings is a top-bar popover rather than a canvas widget.
- The fleet switcher says `Local fleet` and `layout saved to farseer`.
- The sidebar footer says `Windows native` and `local operator surface`.
- Fleet represents cell definitions, while Runners represent active native processes.
- Work projects the durable task rows as a board, conversation list, causal graph, and completed-work face.
- Conversation renders the selected durable conversation across every associated run and harness session.
- Projects present authorized roots, project chips, and separate authorization controls.
- Capacity presents provider accounts and provider-stated windows, while Farseer's own spend remains a lower-bound figure rather than a provider meter.

## Composer and clock

- The home composer is an absolutely positioned workspace child at desktop widths, centered horizontally, `600px` wide at most, `112px` high at minimum, `18px` above the bottom, and `10px 11px 9px` padded.
- The composer uses a `22px` radius, line-strong border, panel fill, and elevated shadow.
- The composer context row shows `to top manager` and an `about {anchor}` chip that resets to `about canvas`.
- The textarea has a `40px` minimum, `78px` maximum, no border, transparent fill, and `9px 4px 4px` padding.
- The composer status line says `Point to a widget to change context`, reports errors as alerts, or reports the accepted run id prefix.
- The send control is `30px` square with a `10px` radius and is disabled with `.5` opacity while a request is in flight.
- Focus within the composer uses accent border and a three-pixel translucent accent ring.
- The anchor is derived from the widget title under focus, pointer, or mouse hover and is host-controlled.
- The clock is an ordinary optional widget whose local value is formatted by `Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit" })`.
- The clock refreshes every `15000ms`, stores an ISO datetime, and chooses Good morning before noon, Good afternoon before 18:00, and Good evening otherwise.
- Clock mounting and hiding are layout changes, not a separate page or mode.

## Responsive composition

- At widths up to `900px`, the app becomes a single block with no outer padding, the workspace loses its border and radius, and the sidebar becomes a drawer.
- The mobile sidebar is fixed `8px` from the top, bottom, and left, uses `min(280px, calc(100% - 32px))` width, and enters from `translateX(calc(-100% - 16px))`.
- Opening the drawer shows a full-screen backdrop beneath the sidebar and exposes the top-bar menu control.
- The mobile top bar keeps `52px` height and `10px` left padding.
- The mobile canvas becomes a one-column grid with `14px` padding, a bottom divider, and each widget spanning the full column.
- The mobile Clock widget is ordered first with `order: -1`.
- The mobile resize handle is hidden because grid cards are bounded to one column.
- The mobile composer leaves the canvas grid and becomes a normal-flow workspace child with `calc(100% - 20px)` width, `8px 10px 10px` margin, no absolute offset, no transform, and an `18px` radius.
- This bounded canvas and separate composer prevent the composer from obscuring the scrollable widget stack on narrow screens.
- At widths up to `560px`, saved-state text, profile, and the live-canvas crumb are hidden.
- At widths up to `560px`, widget header subtitles and size badges are hidden and composer textarea text becomes `16px`.

## Depth and motion

- Standard widgets use `0 12px 36px rgba(29, 29, 27, 0.07)` plus `0 2px 5px rgba(29, 29, 27, 0.04)`.
- The composer uses `0 18px 55px rgba(29, 29, 27, 0.13)` plus `0 2px 5px rgba(29, 29, 27, 0.05)`.
- Widget hover changes the border to line-strong and translates the card upward by `2px`.
- The shared movement easing is `cubic-bezier(0.16, 1, 0.3, 1)`.
- The app grid transition is `.38s`, the mobile drawer transition is `.32s`, widget border feedback is `.2s`, and widget elevation and movement are `.28s`.
- Provider meter transforms transition over `240ms ease`.
- Reduced-motion mode disables smooth scrolling, clamps transition and animation durations to `.01ms`, and runs animations at most once.
- The widget drag state lowers opacity to `.45` and a drop target receives a two-pixel dashed accent outline with a two-pixel offset.

## Arrangement and persistence rules

- Layout state loads from `/v1/ui-state/canvas` before the canvas renders and falls back to the version-eight default if missing or invalid.
- The runtime stores the layout as an opaque blob and does not parse frontend arrangement details.
- The client normalizes the stored version, removes duplicate mounts, repairs missing spans, and clamps spans and widget-unit dimensions.
- Mount toggles persist the mounted list and normalize the toggled widget span.
- Widget movement reorders the mounted list by moving the dragged widget into the drop target's original slot rather than swapping cards.
- Only the grip starts pointer movement, so selecting body text remains possible.
- Pointer movement uses capture and hit testing so it behaves consistently in Chromium and WebView2.
- Grip arrow keys move a widget one position at a time and retain focus after React reconciliation.
- Resize uses pointer capture, the rendered grid's measured unit and gutter steps, and a final snapped span rather than one save per pointer pixel.
- Resize handles accept arrow keys and change one unit on either axis within the span limits.
- A queued promise serializes layout PUTs so rapid edits cannot be overwritten by an older response.
- The top bar reports `arrangement saved` while the saved-state indicator is healthy.

## Bridge and security semantics

- The host creates one bridge and passes it to built widgets, so widgets do not hold the operator token or touch the file system.
- `bridge.read` is GET-only through `/v1`.
- `bridge.post` allows only named run verbs, quota refresh, project operations, conversation creation, validated task transitions, and transcript custody changes.
- `bridge.del` allows only project-root removal.
- Disallowed bridge paths fail with an explicit error rather than being silently ignored.
- `bridge.ask` always posts to `/v1/cells/zero/instruct`, so the top manager is the only AI request destination.
- Ask goals carry a host-stamped widget anchor plus the selected project, conversation, and manager candidate, and the accepted run and task become the shared subject.
- `loadState` and `saveState` use `/v1/ui-state/{key}` and authored widgets receive namespaced `widget.{id}.{key}` keys.
- Authored widgets run in an iframe with `sandbox="allow-scripts"` and no `allow-same-origin`, giving them an opaque origin without host DOM, cookies, local storage, session storage, or network access.
- The only authored-widget channel is a host-issued `MessagePort` that serves read, ask, loadState, and saveState calls.
- Authored widget reads cannot access `/ui-state`, and authored widgets cannot call direct run control verbs.
- The host stamps the anchor as the widget id, title, and optional cell subject, preventing an authored widget from claiming another widget's context.
- A widget displays a cell and never addresses an individual cell through an instruct-cell bridge method.
- Widget discovery and bundle loading occur through `/__widgets` endpoints, with the registry and sandbox gate remaining host-owned.
- GateBar exposes pending authored widgets and working-tree changes with explicit keep or undo choices, and undo requires confirmation.

## Accessibility and interaction

- The document uses a root application, an `aside` labeled `Widget navigation`, a navigation region labeled `Home widgets`, a workspace header, a main canvas, and a form composer.
- Every widget section receives an accessible labelled heading id.
- Focus-visible controls use a two-pixel accent outline with a two-pixel offset.
- Mount controls expose `aria-pressed` and titles that state whether the widget will be shown or hidden.
- The collapse control exposes Expand or Collapse sidebar labels based on state.
- The mobile backdrop has a Close widget navigation label.
- Grip and resize targets are focusable and expose keyboard shortcuts in their ARIA metadata.
- The composer textarea names the top-manager destination and current widget anchor.
- Asking disables the textarea and send button, exposes a Sending to top manager label, and reports errors as alerts.
- Empty and loading states use explicit text rather than silently disappearing content.
- Long feed, run, thread, account, and widget-body content scrolls inside bounded regions.
- Escape closes the mobile sidebar.

## Scope limits and deferred chat

- This record covers the shipped production `ui/` surface and its host bridge, not a static-only replacement.
- The static Berd-home prototype remains an approval reference and does not replace production `ui/`.
- The production home does not add a second layout, mode switch, runtime, or plugin ABI.
- Full durable conversations, task boards, harness-session identity, transcript custody, and causal topology are implemented by `40 work model and session explorer`.
- The composer remains a compact request entry point to the top manager, and Conversation is a widget on the same canvas rather than a second screen.
- Production runtime APIs, native runner supervision, authorized project enforcement, lifecycle state, quota truth, memory, and evidence remain separate runtime concerns rather than visual-shell policy.
