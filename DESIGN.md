# Design System: space

## Overview

**Thesis — the instrument panel.** `space` is a read-only disk instrument: numbers are primary, the treemap owns every saturated pixel, and the chrome is floating panels on near-black that never compete with the data. The world is dark-first (`[data-theme]` flips both, `styles/tokens.css`); light exists as a full parity block resolved from the same token names. Warm signal orange (`#e8654a` dark / `#c9462e` light) is spent only on the primary action, selection, and real warnings — one accent, rationed.

Every visual constant below lives in `space/src/styles/{tokens,primitives}.css` unless another path is cited. Nothing aspirational survives: if a rule is named here, the code enforces it.

## The world tokens (`styles/tokens.css`)

- **Ground** `--bg`: `#0b0b0d` dark (default) / `#f1f1f3` light. Floating `.panel` surfaces sit on the ground via `--bg-elevated`: `#17171b` dark / `#ffffff` light, radius `--r-panel: 14px`, edge = `--shadow-panel` (0.5px hairline + 1px soft rest) plus `--panel-edge` (inset 1px top highlight: `rgba(255,255,255,.045)` dark / `.5` light). Floating layers instead use `.float`: `--r-menu: 12px` + `--shadow-menu` (heavy drop + 0.5px hairline) — menus, palette, hover cards only.
- **Fills and lines**: `--fill-quiet/hover/active` steps (dark `rgba(255,255,255,.045/.075/.11)`, light on-black `.035/.055/.085`); hairlines `--hairline` (`.075` dark) and `--hairline-strong` (`.15`). Ink ladder `--ink / --ink-2 / --ink-3` (`#f4f4f6` at 100/62/40% dark, `#1c1c1f` at 100/66/42% light). Sunken control beds `--bg-sunken #0d0d0f` dark, `#ebebed` light.
- **Signals**: accent + `--accent-soft` (selection/focus/marquee), `--accent-ink` (white on light, `#201009` on the dark orange for contrast), `--danger` `#e5544b`/`#c7372f`, `--warn` `#d9a23f`/`#a06a15`, `--focus` = accent at 50–55% alpha, `--scrim`.
- **Radii ladder**: 14 panel · 12 menu · 8 control/plate · 7 row · 6 chip (`--r-chip`). Keycaps 5px (`.kbd`). Treemap tiles are square-cornered — data has no radius.
- **The two curves** (`--ease-out 0.22,0.9,0.3,1` for state ≈120ms; `--ease-spring 0.32,0.72,0,1` for navigation at 420ms, `ZOOM_MS` in `treemap/TreemapCanvas.tsx`). `prefers-reduced-motion` clamps every animation/transition to 0.01ms globally (`primitives.css`) and the scan sweep parks mid-bed (`scan.css`).

Module map: `src/index.css` is the import manifest and the cascade — tokens → primitives (resets, `.num/.micro/.kbd`, `.panel/.float/.chip`, textures, controls, `.stat`, `.hovercard`, app shell) → per-surface modules (`shell/drives/scan/explorer/rail/map`, each owning exactly one screen's classes). `preview.css` is imported by `previewPlate.ts` so it ships with the renderer's use of plates, not in the manifest.

**Named rule — Flat resting.** `--shadow-panel` carries one 1px rest tone and a soft edge; only floating layers get `--shadow-menu`. A shadow on a resting in-flow surface is a defect.

## Colors

Chrome is grey. The eight-category data palette (`lib/colors.ts`, the single source for GL fills, label ink and the DOM legend) is the only saturated field:

| Category | Light | Dark | | Category | Light | Dark |
|---|---|---|---|---|---|---|
| Media | `#3d7fdd` | `#4e8ef0` | | Apps | `#d1663c` | `#e07b50` |
| Documents | `#2f9b8e` | `#3cb3a4` | | Archives | `#c2547a` | `#d76a90` |
| Code | `#5d9c46` | `#74b457` | | System | `#8a6fc0` | `#a188d9` |
| Caches | `#d2952c` | `#e0a53f` | | Other | `#7c828b` | `#8d949e` |

These eight are fixed; Caches always carries the fragment-level hatch (9px diagonal, `TILE_FS` in `treemap/gl.ts`) and rail swatch overlay. **Two-Channel Rule:** every category also wears a text label — depth-0 big tiles even carry a 3.5px swatch before the name; chips are always dot+word (`.chip`).

Label ink is derived, not stored: `inkFor()` picks white / black / `#1c1c1f` by the *drawn* fill's relative luminance so every tile label clears 4.5:1 (`lib/colors.ts`); depth-1/2 fills flatten toward `canvasBg` 100/88/76% (`tintFill()`). `canvasBg` = `#0b0b0d` / `#f1f1f3` — same as `--bg` — and the GL clear color is re-resolved at runtime as the first opaque ancestor behind the canvas (`resolveOpaqueField`, `TreemapCanvas.tsx`), so ground and field can never drift.

**Theming flow.** Dark is the default (no stored preference → dark, `App.tsx`); the choice persists in `localStorage["space.theme"]`, applies via `document.documentElement.dataset.theme`, and is overridable by the `?theme=light|dark` deep-link for review harnesses. The renderer re-reads `--accent`, `--accent-soft` and the field through `getComputedStyle`, re-armed by the `theme` prop and a `MutationObserver` on `[data-theme]`; hardcoded accent hexes exist only as unparsed-CSS fallbacks (mirroring the tokens), never as primary sources.

## Typography

Geist + Geist Mono, variable, self-hosted via `@font-face` from `src/fonts/` (Geist-Variable.woff2 ≈68 KB, GeistMono-Variable.woff2 ≈70 KB, `font-display: swap`; the label draw path also pre-loads the four faces it measures with and drops its measure cache on `fonts.ready`). No second family anywhere.

Ladder (all verified in CSS):

| Step | Value | Where |
|---|---|---|
| Label voice | Geist 11.5/500 ink-2, sentence case | `.micro`, section titles |
| Figures | Geist Mono tabular (`.num` = `tabular-nums`) | every readout |
| Stat block | mono 21px/550, `.is-hero` 30px/600, `.is-sm` 15 | `.stat`; rail hero overrides to **34px** (`rail.css`); drives hero stays 30 |
| Chip | 11px/500 | `.chip` |
| Body | 13px base; rows/menu 12.5; crumbs 13.5/500; drive names 13/500 | screen modules |
| Micro figures | 10.5–11px mono | keycaps, status keys, plate values |
| Tile labels | names 600/11.5 big, 500/10.5 small; selected rides **650**; size line Geist Mono 10.5 at the quietest 4.5:1-safe alpha (≥62%) | `TreemapCanvas.drawLabels` |
| Boot wordmark | Geist 26/600, sentence "space" | `.boot-mark` (`shell.css`) |

**Named rule — Label voice.** Uppercase tracked mono labels are banned; mono is reserved for figures, paths, and keycaps (`.kbd`). Sentence case everywhere.

## Layout system

App shell `.app` (column, `100vh`); screens fade in 260ms. Three screens route through `App.tsx`: drives → scan → explorer, with a rendered boot state (wordmark + one honest line, no spinner).

- **Topbar** (explorer): transparent 46px strip on the ground — back/forward icon pair, breadcrumb chain (crumb cells ellipsize at 220px, `…` collapse when the chain exceeds 5), filter field 200→240px on focus (2.5px focus ring), sort segmented control (Size/Name/Type), `Ctrl K` hint chip, rail toggle. Memoized; never re-renders for hover state.
- **Drives**: one ledger `.panel`; on the ground above it, the lowercase wordmark and a real figure hero — Σ(total − free) across the volumes the system reported, `n volumes · capacity` beneath, an em dash while the query flies (never a guess). Rows read left → right: mount-letter chip · name over mount/fs/kind meta · 8px dot-matrix usage bed whose fill warns past 80% and goes danger past 92% (the pct prints beside it, so the bed is `aria-hidden`) · fixed total/free figure block · one quiet **Scan** action — transparent at rest, one fill step on row hover/focus, never accent-filled (One Accent per Field: the warm spend on this screen belongs to the low/critical tier words) — carrying the whole row's math as its `aria-label` (the keyboard/AT door for the row); the row click is a mouse convenience. Rows carry no rules — the ledger divides by rhythm and the hover fill; one hairline separates it from the keyed legend. Footer: "Read-only · nothing on disk is ever modified."
- **Scan**: one centered 620px instrument panel — head (root leaf + mount/elapsed), the honest **indeterminate** accent sweep on a 10px dot bed (the progress payload carries no total, so a percentage would be fabrication; the % slot prints a dash), a four-counter band (bytes/files/dirs/skipped) divided by hairlines, a 90-bar rate strip of raw observed deltas quantized to 1/14 height steps, and the walking path at the foot. Failure swaps the meter for a danger band with the engine's message verbatim and selectable.
- **Content row**: gap 8, padding 0 8 8. The map is one `.panel` that flexes; the **inspector rail is 300px fixed** (`.rail-panel`, `rail.css`) — a second floating panel of hairline-stacked 14px/16px sections: selection/current-folder readout, composition (Categories), and Largest files. The list is the rail's **only** scroll region; ≤900px the rail overlays the map (absolute, default closed) with `-12px 0 32px` depth shadow.
- **Statusbar**: transparent 30px strip. Left: `Items · Files · Size · Of volume` in `.num` mono, plus, when filters hide categories, a hatch-swatched chip — "`N` categories hidden" — which is a `<button>`: clicking it unhides every category at once (`ExplorerView.tsx` `unhideAll`). Right: quiet `Rescan / Drives / Light|Dark` text buttons.
- Responsive: ≤560px the sort control hides (the palette retains it), the search narrows progressively, and the Files readout drops; drives rows fold to two lines at ≤720px; scan bands tighten at ≤640/≤460px.

**Fill invariant (`treemap/TreemapCanvas.tsx`).** The renderer owns the frame pipeline: ResizeObserver → relayout at the container's exact aspect → rebuild when fitted-cover would drift > 0.5px. The world is laid out at the canvas aspect, so fit IS an edge-to-edge edge fill, and fit's scale is the zoom **floor**: every viewport path (wheel, `zoomBy`, `zoomToTile`, nav targets, resize) clamps through `enforceFill` (floor lift + world-covers-canvas pin). Letterboxing is structurally impossible — if background shows, a clamp is broken. Ceiling is `14×fit`, raised per-gesture by `zoomCeilingAt()` so the tile under the cursor can always reach the preview request gate (BUG 2).

## Interaction states

- **Hover.** Map tiles lift one fill step toward white (`HOVER_LIFT 0.12`) plus full-ink label — no ring; selection stays distinct via the warm-accent ring. Rows (drives, rail, palette, menu) step `--fill-hover`/`-active` over 120ms. The hover card is the app's **one readout**: floating `.float` card (name / mono size + share / category chip / count / tail-true path / real actions), flip-and-clamped to its host. Identity (which item) is React state; position is written imperatively via `positionCard` between identity changes — same-tile moves re-render nothing (`ExplorerView.tsx`). Rail rows get the interactive variant with a 150ms leave-grace (`LEAVE_GRACE_MS`), focus parity, and scroll-closes.
- **Hidden categories.** A toggled-off category's tiles hold their place at alpha **0.16** (legible-silent — "a switched-off cell, not a void"); no hover lift, no label. The legend row wears a struck swatch (hatch overlay + ink-3 label), and the statusbar chip spells the count.
- **Selection.** 1px warm-accent inner ring drawn inside the tile + `--accent-soft` fill lift scaled per-instance (multi-select works). Rail row tint = `--accent-soft`. Escape clears; Ctrl C copies; Enter drills or opens.
- **Drill law.** A single click on a directory tile drills ONE level (files still select); a double-click is the same gesture, not a second one — a 500ms drill cooldown (armed on every directory drill) makes `onDown` and the synthesized `dblclick` swallow the tail. Without it, one physical double-click teleported three levels deep the instant deeper layouts loaded from memory. Double-click still opens file previews.
- **Filter law.** Escape inside the filter box clears the query and blurs; blur alone left the 0.16-filtered map stranded behind a focused-looking field.
- **Marquee.** shift-drag draws an accent-soft fill + 0.8-stroke rect on the 2D layer; selects depth-0 tiles it overlaps, adds to the selection at release.
- **Focus.** `:focus-visible` = 2px `--focus` ring, 1px offset (global). Every icon-only button carries `title` **and** `aria-label` (`ui.tsx` `IconBtn`); rail's scanning button announces the full row math.
- **Zoom instruments** (`map.css`): two 9px crop-mark L-brackets inset 8px bracket the data field; the mono `ZOOM N%` chip at bottom-right only speaks when the view leaves 100% (opacity toggled by the draw loop, never React).

## In-tile previews (`treemap/previewPlate.ts`, `styles/preview.css`)

One pooled DOM layer (`.map-previews`, `pointer-events:none`, clipped at the map edge) over the GL field; the draw loop owns position/content/visibility imperatively — React never re-renders a plate. Families render real, read-only content: **TEXT** (mono bed with line-gutter, greyscale syntax — ink steps and weight only; SVG arrives as scrubbed source), **HEX** (4 KB head, 8 B/row, offset/bytes/ascii; confirmed signature bytes get the only accent underline + a key chip), **IMAGE** (decoded thumbnail, 1px frame, crop marks + dimension tab at ≥160px tile width), **LIST** (archives, sheets, sqlite tables as 22px rows with real columns), **META** (key/value pairs, optional rotated edge rail at ≥360×240, signature fallback when nothing parses), **MEDIA** (inert: video first frame, audio readout; duration stamp no transport controls).

**Named rule — A directory with an interior never plates; a leaf folder does.** An interior folder's preview IS the nested treemap (its children drawn beneath it, subdivided lazily to files), so a plate may never cover those children or hijack hover identity for what the pointer sees under it (the "every element reads as the parent" failure, user-verified 2026-10-19). But a directory with **no interior to render** — at the depth cap, in a cap-truncated tail, or with an empty/unreadable expansion — would otherwise be a dead grey fill. Those leaf folders request the backend's real folder listing at the LIST gate and plate it with the same table geometry as an archive: files size-sorted, directories name-sorted, honest `folder · N entries` footer. The renderer therefore requests and qualifies an `isDir` tile only when the current frame has no child rect for it, so a folder card can never cover an interior.

Mechanics worth copying:

- **Pool 24 (> cap 8)**, recycled forever — no DOM churn. Only `≤8` plates visible at once, ranked by visible canvas area with a **1.25× incumbent bias** so continuity survives panning.
- **Request gate 260×110** fires once per entry per layout generation — intentionally smaller than any render gate, so content is warm before it qualifies. **Deviation, documented:** HEX renders from 330×140 because one offset+bytes+ascii row is meaningless narrower; below a family's render minimum (`PLATE_MIN`) the tile keeps its floating label.
- **XL listing cap — a plate is a preview instrument, not a map replacement.** A file LIST/DB plate (archive, sheet, sqlite table) offering ≥20 rows on a tile covering ≥72% of the canvas in BOTH axes (a giant archive seen at shallow zoom) is bounded to ≤50% canvas width × ≤42% canvas height, anchored top-left; the map around it stays visible. Table plates render at most 20 rows (`TABLE_ROW_CAP`); the honest `… N more entries` footer carries the remainder. Modest listings (the 13-row class) keep their full-tile preview.
- **Plate-aware hit-testing — identity matches what the pointer sees.** `hitTest` scans claimed+visible plate slots first: a pointer inside a painted plate box resolves to the plate's owner tile, never to a child tile hidden beneath the paper. Applies to hover, drag, context menu, and the double-click/open paths alike.
- Every plate paints at its canvas∩tile box (deep zoom can't strand a plate's head off-screen); an unanswered request that survives 30 drawn frames becomes the honest fallback plate ("no preview"), and the tile's floating label is suppressed only while a plate is actually shown.

## Renderer performance contract

- All work rides one rAF loop (`tick()`); idle = zero frames. The loop is panic-latched: one `console.error`, one `space:error` event, then it stops — never spam, never a silent death.
- **Hover is rAF-aligned and allocation-free**: pointermove only mutates a pending point; one scheduled pass per frame resolves. Crossing tiles patches exactly two 4-byte `aHover` floats in place (`bufferSubData`) and re-issues the instanced draw + label redraw — never a re-upload (identity-checked against `instFrame`). The parent gets `onHoverChange` only when the tile changes; ExplorerView re-renders only the card, the topbar/rail/map subtrees stay memoized.
- **Crash-class guards**: raw frame indices are bounds-checked before every `refs`/buffer dereference (`hover.current < refs.length`, `drag.idx = -1` on rebuild), and rebuilds reset hover/arm state to never point at stale data — the `dimmed()/refs[i]` TypeError class is closed by construction.
- Label engine keeps state in preallocated typed arrays (collision quads, order); `measureText` and `rgb()` strings are cached. Dragging, zooming, hovering allocate nothing per frame; DPR is capped at 2.
- Treemap geometry: WebGL2 instanced tiles (12 floats each) + a 2D label layer; WebGL2 mount failure degrades the app to the honest map-state bed (`getChildren` failures print the real reason on a dots bed).

## A11y & keyboard

Tab reaches every control; rails/toggles are real `<button>`s (`aria-pressed` on category toggles, segmentation via `role="tablist"`). Explorer keys: Backspace up a level, Alt+←/→ history, `+/−` zoom, `Ctrl 0` refit, `Ctrl F` focus filter, `Ctrl K` palette, Enter / Ctrl C act on selection (the context menu promises those keycaps — they are wired). The palette (`Ctrl/Meta+K`) filters commands and, with a query, jumps to files/folders with real sizes; ↑/↓/Enter/Escape. Hover cards are `role="status"` with **`aria-live="off"` on purpose**: pointer-driven readouts must not flood the accessibility tree — keyboard parity comes from focus-opening the same card.

## Do's and Don'ts

- **Do** treat `.panel`/`.float` as the whole surface vocabulary; interiors are hairlines, never nested cards.
- **Do** keep every live figure `.num` (tabular mono) and every label sentence-case Geist.
- **Do** gate new saturated color: the treemap palette, accent, warn and danger are the entire budget.
- **Don't** use gradients, glass/blur, glowing ambient, or blinking dots (the scan meter sweeps cleanly instead).
- **Don't** letterbox the treemap (impossible) or overlap tile labels (largest-first ladder + focus priority forbids it).
- **Don't** add a delete/move/rename affordance — read-only is structural (`capabilities/default.json` allows only reveal-in-dir and open-path).
- **Don't** reintroduce a logo tile: the wordmark is lowercase Geist type, small, like this build.

Last verified: 2026-10-19 · `space` 0.1.0 · checked against the working tree (no git history in this folder).
