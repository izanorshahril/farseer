# Farseer Berd Home Design Record

## Status and provenance

- Seed: `b452fefe`.
- Boundary: `.scratch/farseer/prototypes/berd-home/`.
- Status: accepted static interactive home-screen prototype.
- Source of truth: the shipped `index.html`, `style.css`, and `app.js`, reviewed with the desktop and mobile rasters.
- The prototype remains the approval reference; its accepted direction is now implemented in the production `ui/` client.
- The production runtime remains outside this prototype boundary.
- Preview content is synthetic and is marked in the top bar as `Preview data`.

## Product intent

- Farseer is a local operator surface for coordinating several AI managers and workers from one Windows desktop application.
- The home surface reorients an operator after an interruption by exposing attention, active work, project context, and fleet state in one viewport.
- The composition is a close structural and visual homage to Berd's calm home canvas, translated from single-agent pins into fleet objects.
- The top manager remains the route for every AI request, while direct run actions remain run-level controls.
- The home composer is the primary action and is explicitly labeled `Prototype only`.
- Chat history and the full conversation screen are deferred until the home screen is approved.

## Visual direction

- The direction is calm operational clarity rather than a metric dashboard or terminal wall.
- The desktop frame uses a persistent sidebar, compact top bar, pale dotted freeform canvas, warm neutral panels, quiet hairlines, pill controls, and restrained state color.
- The first viewport anchors on an optional local-time pin surrounded by four other operational pins.
- The composer floats at bottom center instead of becoming a full chat surface.
- Neutral surfaces carry hierarchy, while blue, violet, coral, mint, and amber communicate project or state meaning.
- Pure black and pure white are avoided in favor of tinted ink and panel values.

## Palette

### Light theme

- `--bg` is `#f4f4f2` for the outer shell background.
- `--canvas` is `#f8f8f6` for the workspace and dotted home canvas.
- `--panel` is `#ffffff` for cards, sidebar, composer, and controls.
- `--panel-2` is `#f5f5f3` for secondary surfaces and hover fills.
- `--ink` is `#252525` for primary text, active navigation, dark marks, and send controls.
- `--muted` is `#70706d` for secondary text and quiet controls.
- `--faint` is `#92928e` for tertiary text, labels, and disabled-looking metadata.
- `--line` is `#e5e5e1` for standard borders and dividers.
- `--line-strong` is `#d4d4cf` for emphasized borders and scrollbar thumbs.
- `--blue` is `#4d72ff` and `--blue-soft` is `#e9edff` for project and focus meaning.
- `--violet` is `#7d64d9` and `--violet-soft` is `#eeeafc` for the Berd study project marker.
- `--coral` is `#d95f4b` and `--coral-soft` is `#ffebe7` for attention and warning meaning.
- `--mint` is `#31846f` and `--mint-soft` is `#e2f2ed` for healthy or running meaning.
- `--amber` is `#b06d20` for the orca project marker.
- Healthy status dots use `#38a275` with a three-pixel translucent ring.

### Dark theme

- `--bg` is `#161615` for the outer shell background.
- `--canvas` is `#1b1b1a` for the workspace and dotted home canvas.
- `--panel` is `#242423` for cards, sidebar, composer, and controls.
- `--panel-2` is `#2b2b29` for secondary surfaces and hover fills.
- `--ink` is `#f1f1ed` for primary text, active navigation, marks, and send controls.
- `--muted` is `#b4b4ae` for secondary text and quiet controls.
- `--faint` is `#8a8a84` for tertiary text and metadata.
- `--line` is `#373735` for standard borders and dividers.
- `--line-strong` is `#4a4a46` for emphasized borders and scrollbar thumbs.
- `--blue` is `#8298ff` and `--blue-soft` is `#2d3454` for project and focus meaning.
- `--violet` is `#ad9ee7` and `--violet-soft` is `#39334b` for project meaning.
- `--coral` is `#ee8979` and `--coral-soft` is `#4b302d` for attention and warning meaning.
- `--mint` is `#74bda9` and `--mint-soft` is `#293e38` for healthy or running meaning.
- `--amber` is `#dba45f` for the orca project marker.
- Healthy status dots retain `#38a275` and the shadows switch to black alpha values.

## Typography

- Body text uses `Inter`, then `Segoe UI Variable Text`, then `Segoe UI`, then the sans-serif generic.
- The default body size is `14px` with antialiasing enabled.
- The brand is `18px`, weight `680`, and letter spacing `-0.035em`.
- Primary navigation is `13px`, with active and emphasized labels using weight `620`.
- The clock widget time is `46px`, weight `540`, line height `1`, letter spacing `-0.05em`, and uses tabular numerals.
- The clock greeting is `12px` with letter spacing `-0.01em`.
- Pin titles are `14px`, weight `680`, and letter spacing `-0.015em`.
- Pin subtitles are `11px` in the muted color.
- Run and attention copy uses `12px` to `13px`, with supporting copy at `10px` to `11px`.
- Uppercase section labels use `11px`, weight `650`, and letter spacing `.07em`.
- Keyboard labels and project paths use `Geist Mono`, then `Cascadia Code`, then the monospace generic.
- Keyboard labels are `9px`, project paths are `9px`, and time metadata uses tabular numerals.

## Geometry and spacing

- The shell fills the viewport, uses an eight-pixel outer inset, and has an eight-pixel column gap.
- The desktop shell is a two-column grid with a `248px` sidebar and a remaining workspace column.
- The sidebar and workspace each have a one-pixel standard border and an `18px` radius.
- The sidebar uses `12px 10px` padding and a `40px` brand row.
- The fleet switcher is `52px` high, has `10px` top and `12px` bottom margins, `6px 8px` padding, and a `13px` radius.
- The fleet mark is `32px` square with a `9px` radius.
- Navigation rows are `36px` high with a `9px` radius, two-pixel row gaps, and ten-pixel horizontal padding.
- The project section begins after `24px` of top spacing.
- Project rows are `34px` high with an `8px` radius.
- The sidebar footer is separated by a one-pixel line and sits at the bottom with automatic top margin.
- The workspace top bar is `52px` high with `18px` left and `12px` right padding.
- The dotted canvas uses a radial dot of `.8px`, a transparent cutoff at `.9px`, a `20px` background size, and a `10px 10px` background position.
- Canvas pins use absolute percentage positions and unequal widths of `320px`, `286px`, `320px`, `310px`, and `294px`.
- Pin widths are capped by `min(variable width, calc(100% - 28px))`.
- Pins use `15px` padding, an `18px` radius, a one-pixel line, and a `34px` icon tile with a `10px` radius.
- The clock pin uses `14px 16px 18px` padding, a divided header, and a centered time face.
- Pin headers use a `10px` gap, and pin body sections begin after `12px` to `14px` of spacing.
- The attention row uses `10px` padding, an `11px` radius, and a three-column layout for status, copy, and disclosure.
- The active run footer is separated by a one-pixel line with `11px` top padding.
- The composer is `580px` wide at most, `112px` minimum high, and uses `10px 11px 9px` padding with a `22px` radius.
- The composer width becomes `min(520px, calc(100% - 130px))` at widths up to `1100px`.
- Composer controls use `25px` context chips, a `38px` textarea baseline, and a `30px` send button.
- Canvas controls sit `14px` from the right and bottom edges inside a three-pixel padded panel.
- Toasts are centered and sit `154px` from the bottom on desktop.

## Desktop composition

- The sidebar is persistent at desktop widths and frames the workspace without competing with the canvas.
- The top bar identifies `Home`, marks the content as `Preview data`, reports `3 cells`, and exposes search, clock visibility, theme, and profile controls.
- The optional clock pin starts at `38%` from the left and `33%` from the top.
- The `Needs you` pin starts at `7%` from the left and `10%` from the top.
- The `Active run` pin starts at `68%` from the left and `13%` from the top.
- The `Projects` pin starts at `12%` from the left and `58%` from the top.
- The `Cells` pin starts at `70%` from the left and `60%` from the top.
- The home composer is centered horizontally and sits `24px` above the canvas bottom.
- Canvas recenter and arrangement controls sit together at the lower right.

## Responsive composition

- At widths up to `1100px`, the run pin moves to `62%` left and the cells pin moves to `64%` left.
- At widths up to `820px`, the shell becomes a single full-width document with no outer inset.
- The workspace loses its border and radius, remains at least `100vh` tall, and keeps a `52px` top row.
- The sidebar becomes a fixed drawer inset `8px` from the left, top, and bottom, with width `min(280px, calc(100% - 32px))`.
- The mobile drawer starts translated left by `calc(-100% - 16px)` and opens through the `sidebar-open` class.
- The mobile top bar is sticky, has `10px` left padding, and keeps the menu, `Home`, clock visibility, and theme affordances while hiding fleet state.
- The mobile canvas has a minimum height of `calc(100vh - 52px)`, `14px 14px 170px` padding, and a twelve-pixel vertical gap.
- Mobile pins become normal-flow cards with full available width, a `540px` maximum, and centered margins.
- Mobile pin order is clock, attention, active run, projects, composer, then cells.
- The mobile composer is sticky, uses `calc(100% - 20px)` width, `105px` minimum height, an `18px` radius, and a ten-pixel bottom offset.
- Canvas controls are hidden on mobile and the toast moves to `128px` from the bottom.
- At widths up to `480px`, search and profile affordances are hidden, the preview badge is hidden, composer helper text is hidden, and the textarea uses `16px` text.

## Depth and surface treatment

- Standard panels use `0 12px 36px rgba(29,29,27,.07)` plus `0 2px 5px rgba(29,29,27,.04)`.
- Lifted panels use `0 24px 64px rgba(29,29,27,.14)` plus `0 4px 12px rgba(29,29,27,.08)`.
- The sidebar gets a quiet `0 1px 2px rgba(0,0,0,.02)` edge lift.
- The composer uses `0 18px 55px rgba(29,29,27,.13)` plus `0 2px 5px rgba(29,29,27,.05)`.
- Dark theme replaces these with `rgba(0,0,0,.24)` and `rgba(0,0,0,.16)` for standard depth, and `.42` plus `.24` for lifted depth.
- Hover raises pins by `2px` and changes the border to `--line-strong`.
- Dragging adds lifted depth, scales to `1.018`, rotates by `.35deg`, raises stacking order, and uses a grabbing cursor.
- The canvas changes its dot spacing from `20px` to `16px` while dragging to make movement legible.

## Motion and reduced motion

- The shared easing curve is `cubic-bezier(.16,1,.3,1)`.
- Sidebar grid collapse takes `.38s` on the shared easing curve.
- Icon hover and active feedback use `.18s`, with active controls scaling to `.94`.
- Pin border feedback uses `.2s`, while pin elevation and movement use `.28s` on the shared easing curve.
- Composer focus feedback uses `.2s`, send hover uses `.18s`, and toast opacity uses `.2s` with `.3s` positional easing.
- Pin and composer entry use `@starting-style` with reduced opacity, blur or downward offset, and scale settling over `.48s`.
- The running badge pulses every `2s` with `ease-in-out`.
- Reduced-motion mode clamps animation and transition durations to `.01ms`, disables smooth scrolling, and removes the pin entry blur.

## Interaction rules

- The collapse control adds `collapsed` to the shell and reduces the desktop grid sidebar to `72px` while hiding text labels and secondary content.
- The mobile menu adds `sidebar-open`, and Escape removes that class.
- Search opens a modal command palette, focuses its search field on the next animation frame, and closes through the backdrop or Escape.
- Closing the palette returns focus to the search button.
- Ctrl-K or Meta-K opens the command palette, and Ctrl-Comma or Meta-Comma displays the prototype settings message.
- Theme toggling switches the root `data-theme` between `light` and `dark` and reports the new appearance in a toast.
- Clock visibility starts enabled, the pin's hide control removes it, and the persistent top-bar toggle restores or hides it while keeping `aria-pressed`, label, title, focus, and toast feedback synchronized.
- Non-home navigation and the Windows health row remain home entry points and report that their destination stays in the prototype.
- Selecting a project marks every matching project control selected, updates the composer context, and reports the selected project.
- Pin action buttons report that detail views come after home approval.
- Arrangement starts enabled with `aria-pressed=true`; toggling it locks or unlocks pin movement and reports the current state.
- Pointer dragging requires the primary button, ignores pointer events that begin on a button, captures the pointer, and clamps the pin to a twelve-pixel canvas inset.
- Focused pins move by eight pixels per arrow key or twenty-four pixels with Shift and remain clamped to the canvas.
- Recenter resets every pin to its original CSS variable position and reports that the home pins were recentered.
- The composer grows from `38px` up to `84px` based on content height.
- Empty submission keeps focus in the prompt and reports that an instruction is required.
- Non-empty submission clears the prompt and reports that no request was sent because this is a prototype.
- Toasts are live status feedback, last `2600ms`, and do not intercept pointer input.
- The clock uses the browser's local locale, refreshes every `15000ms`, and changes the greeting by local hour.

## Fleet semantics and preview content

- The fleet switcher names the scope `Local fleet` and reports `3 cells`.
- The top bar repeats the fleet liveness summary as `3 cells` with a healthy status dot.
- The sidebar projects are `farseer` with `2 live`, `berd study` with `quiet`, and `orca` with `quiet`.
- The attention pin says `Needs you`, `One run is waiting`, and identifies `Review workspace choice` from `top manager` at `8m`.
- The active run pin identifies `farseer · top manager`, the task `Build a second operator surface`, and a `running` state.
- The active run copy says `Reading Berd's home canvas and preserving Farseer's fleet model.`
- The projects pin identifies authorized roots for `farseer` at `D:\Dev\farseer` and `berd study` at `D:\Dev\berd`.
- The cells pin lists `zero` as `pi · manager` and ready, `social` as `goose · manager` and ready, and `research` as `codex · manager` and idle.
- A cell widget represents a cell and does not address an individual runner.
- The composer defaults to `farseer` context and routes to `top manager`.
- State color is reserved for truth such as attention, running, ready, idle, and project identity.

## Scope limits

- This prototype remains unchanged as the accepted visual reference for the production port in `ui/`.
- The prototype composer itself does not send an AI request and does not implement chat, chat history, or a conversation screen.
- Navigation, pin actions, search results, settings, project selection, and theme changes are demo feedback rather than production routing.
- Pin movement is transient DOM state and is not persisted to the runtime arrangement blob.
- Preview projects, runs, cells, paths, times, and statuses are synthetic display data.
- Backend APIs, runner processes, worker contracts, quota accounting, memory records, lifecycle control, and evidence records are outside this prototype.
- The later chat composition remains a separate design decision and may use Claude Code desktop as a reference.
