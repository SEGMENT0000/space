# Product

<!-- impeccable:product-schema 1 -->

## Platform

Desktop app for Windows on Tauri v2: a Rust shell (drive queries, scanner, previews, opener bridge) embedding a React 19 + TypeScript + Vite front end (`space/src/`, `space/src-tauri/`). The window opens 1320×860 (min 1024×680, centered, drag-drop enabled, dark by default — `tauri.conf.json`). Build chain: `npm run build` is `tsc && vite build` → `dist/`, which the shell embeds via `frontendDist: "../dist"`. **The release binary must be built through the Tauri CLI (`npx tauri build --no-bundle`) — verified 16:13 after a live defect:** a manual `cargo build --release` keeps `build.devUrl` active and embeds nothing; the exe silently serves a leftover `vite` dev server if one is alive (unminified modules, stale HMR state — it shipped a `TABLE_ROW_CAP is not defined` toast from an edit race). Verify the real embed by launching with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` and reading `127.0.0.1:9222/json/list`: the page URL must be `http://tauri.localhost/`, never `localhost:1420`. Version 0.1.0 in both `package.json` and `tauri.conf.json`. Measured bundle on disk: `dist/assets/index-DnrLeGbX.js` 389,364 B (≈389 KB; 125,265 B gzipped ≈126 KB), one 37,647 B CSS output, plus the two font files. Styles are handwritten CSS (`src/styles/` tokens + primitives + per-screen modules, imported through `src/index.css`); no CSS framework ships.

Fonts: Geist + Geist Mono, variable, self-hosted from `src/fonts/` as `Geist-Variable.woff2` (69,492 B) and `GeistMono-Variable.woff2` (71,236 B). One family pair for everything: Geist for labels and body text, Geist Mono for tabular figures, paths, and keycaps. The banned families (Inter, Roboto, Arial, Open Sans, Helvetica) appear nowhere.

## Users

The author, a technical desktop user who judges interface craft at pixel level and drives tools keyboard-first. They intend to ship `space` later, so the bar is product-grade, not a personal prototype. No external audience has been confirmed yet.

## Product Purpose

`space` shows what is eating disk space on a Windows machine: it scans a volume, a folder dropped onto the window, or a volume passed as `?scan=<path>`, and renders the result as an interactive two-level treemap you drill into. Success means the user identifies the files consuming space within seconds, reads the proportions correctly at a glance, and never has to trust the app with their data: storage is shown as an instrument, not a checklist to clean.

## Positioning

A read-only, local-first disk analyzer whose treemap *is* the product: a WebGL-rendered, zoom-navigable two-level layout with category semantics, no cloud, no telemetry, and no delete path anywhere in the codebase. The neighbouring tool could not truthfully copy the claim "read-only by construction — it is structurally incapable of modifying what you point it at."

## Operating Context

A desktop window containing the web UI. Three screens route through `App.tsx` (drives → scan → explorer) after a rendered boot state (lowercase wordmark + one honest line, no spinner); the window resizes freely and the layout holds at every size:

- **Drives ledger** — one panel listing volumes as a narrative instrument: a statement claim above the panel ("Every volume has headroom" / "C: is 88% used" / "N volumes are running low"), Σ context lines, and per-row two-line readouts (fig1 "X used", fig2 "Y free · Z total"). The 10px tick-gridded ribbon warns past 80% (`low`, amber word printed inline) and goes danger past 92% (`critical`, diagonal hatch on the fill); a legend decodes the ribbon at the panel foot. One quiet Scan action per row — transparent at rest, revealed on row hover/focus, never accent-filled — carries the row's math as its `aria-label`; the row click is a mouse convenience. Rows carry no rules; tiny used shares print honestly ("<0.1%"). The wordmark sits above the panel and the footer repeats: "Read-only · nothing on disk is ever modified."
- **Scan meter** — a centered 620px instrument that is deliberately honest: an indeterminate accent sweep (no percentage — the scanner reports no total), a four-counter band (bytes / files / dirs / skipped), a 90-bar rate strip of observed deltas, and the walking path at the foot. Failure swaps the meter for a danger band with the engine's message verbatim and selectable, and Cancel becomes "Back to volumes."
- **Explorer** — topbar (back/forward, breadcrumbs that ellipsize at 220px and collapse past 5, a 200→240px filter field, Size/Name/Type sort, a `Ctrl K` hint, rail toggle) over a full-bleed treemap panel, a fixed 300px inspector rail (selection readout, category composition with on/off legend, Largest files), and a statusbar (Items / Files / Size / Of-volume figures; when filters hide categories, a hatched chip names the count and clicking it unhides all at once). Directory tiles never show listing plates — a folder's interior is its nested treemap, drilled and zoomed, and it subdivides lazily on demand inside its own tile (large folders become their real contents instead of flat tints); preview plates (text/hex/image/meta, archive & table listings) belong to file tiles at any depth. Hover identity always matches the pixels under the pointer — including under plates. A click on a directory tile drills exactly one level; Escape clears the filter box.
- **Hover card** — the app's one readout for tiles and rows: name, mono size + share, category chip, count, full path, and the real actions; follows the pointer between identity changes without re-rendering React.
- **Context menu** — Open file/folder · Reveal in Explorer · Copy path, with the keycap hints the key map actually honors (Ctrl C copies, Enter acts).
- **Command palette** — `Ctrl/Meta+K` filters commands and, with a query, jumps to files/folders by name with real sizes.
- **Deep links** — `?scan=<path>` boots straight into a scan, gated to `/^[A-Za-z]:\\/`; `?theme=light|dark`, `?screen=`, and `?zoom=` serve the review harness.

## Capabilities and Constraints

Confirmed scope — every capability is read-only; nothing deletes, moves, or writes files:

- List mounted volumes with capacity, used, free, and utilization.
- Scan a volume or dropped folder with live counters, a skipped ledger, cancel, and verbatim error reporting.
- Treemap colored by the eight fixed categories, subdividing recursively on demand (lazily, capped at 5 interior levels); the legend toggles categories, and hidden tiles hold their place at 0.16 alpha.
- In-tile content plates on large tiles: files (text/code, greyscale and scrubbed; a 4 KB hex head with signature highlight; decoded images; archive/table rows; metadata) and **leaf folders** (the real folder listing: size-sorted files, name-sorted directories, `folder · N entries`). Lazy, pooled 24 with a hard 24-plate visible ceiling, backend LRU 256 with 4 worker permits. A directory that renders an interior never plates, because its nested treemap is the preview; a directory with no interior (depth cap, cap-truncated tail, empty or unreadable expansion) gets a folder card instead of a dead fill.
- Inspector rail: Contents list and Largest files; multi-select with byte totals; Copy path rides the clipboard (`navigator.clipboard.writeText`, demo-aware).
- Light and dark appearance, persisted in `localStorage["space.theme"]`, overridable by deep link.

Binding constraints from the user:

- Banned type: Inter, Roboto, Arial, Open Sans, Helvetica. Real font files ship with the project and load locally.
- Banned decoration: purple-to-blue or mesh gradients, glassmorphism, floating ambient background lights, uniform default box-shadows, card-grid-everywhere layouts, lorem-style filler, redundant subheadings that restate a section title, blinking or pulsing status dots, and any element whose only job is to look designed.
- Interaction: the left mouse button must carry the primary drag gesture; middle-button-only drag is explicitly rejected.
- The treemap must fill its container at the correct aspect ratio at every window size, and tile labels must never overlap or bleed outside their tile.
- Synthetic data must be fully fictional (pinned by the user, 2026-10-17): demo content ships no real usernames, folder names, people, locations, timezones, email addresses, or phone numbers — ever. Personal data appears in the app only as live display of what the user's own scan returns, never baked into code. A purge-and-audit pass enforced this; regressions are release blockers.
- Open decisions (owned by the design flow, not this file): ship name/licensing, and any first-run onboarding.

## Guarantees

- **Read-only is structural.** The Tauri capability file grants exactly two opener permissions beyond defaults — `opener:allow-reveal-item-in-dir` and `opener:allow-open-path` (`src-tauri/capabilities/default.json`); there is no write, delete, or rename permission to grant, and previews read bytes without executing them.
- **Keyboard parity.** Backspace up a level, Alt+←/→ history, `+`/`−` zoom, `Ctrl 0` refit, `Ctrl F` filter, `Ctrl K` palette, Enter/Ctrl C on selection; every icon-only button carries `title` + `aria-label`; pointer-only readouts have a focus path.
- **Honest numbers.** No fabricated progress percentages and no placeholder figures: unknown values print dashes, warnings are real thresholds, and failure text is the engine's own message.
- **Walker.** Windows enumerates via a direct FindFirstFileExW fast walker (UTF-16 name arena, zero-alloc staging, batched notify-gated queue, below-normal-priority thread, catch-unwind isolating bad paths); other platforms fall back to a parallel jwalk walk. Directory totals update O(1) against a 256-entry sorted cache.
- **Speed.** From D: at rest, clicking Scan reaches a fully live explorer in ≤ 0.8 s warm (verified twice, `.impeccable/qa-next.md`). A cold C: scan (1.3M files) completes in ≈2 s (verified 15:54, embedded build). An earlier logged figure of 416 files / 8.32 GB carries `>?` — it cannot be reproduced from the repo today.

## Evidence on Hand

- The built implementation is the evidence: `space/src/` (React 19 front end, WebGL2 treemap) and `space/src-tauri/` (Rust scanner and previews) compile today, type-checking enforced by `npm run build`; DESIGN.md and this file cite the code rather than argue.
- Measurement convention: any figure above traces to a file on disk or a dated QA note; unverifiable claims carry `>?` instead of applause.
- No stock photography, testimonials, customer names, or third-party proof exist. Future work must not fabricate any.

## Brand Commitments

Name: `space`, set lowercase. The former orange "s" letter-tile badge is retired by user decision and must not return in any form; a mark, if any, must be meaningful to the subject (volume, blocks, proportion) or be absent entirely. Voice: plain and technical; no marketing language, no hype copy.

**Standing aesthetic preference (pinned by the user, 2026-10-16):** the craft bar is Apple's system apps — Finder, Disk Utility, Photos. Every surface must read as pixel-perfect and smooth at that level: neutral slightly-cool surfaces in light and dark, one type family at real weight steps, hairline separators instead of cards or shadows, an 8pt spacing rhythm, tabular figures for all data, and a single unified motion curve for every transition. The user rejected skeuomorphic/costume visual worlds and rated generic "AI slop" UI 2/100. Borrow the craft discipline only — never Apple's chrome, icons, or copy. This is a standing preference for every future round.

## Product Principles

1. Read-only is structural, and the interface says so plainly.
2. Proportion is the message: area encodes size, and chrome never competes with the data.
3. Speed is a feature: instant feedback, keyboard-first operation, motion that never blocks.
4. Nothing decorative without meaning: if an element carries no information or affordance, it is cut.
5. Pixel-level craft is the acceptance bar; vague "looks modern" is not a pass.

## Accessibility & Inclusion

No separate standard was set; the built baseline holds: visible focus (`:focus-visible` 2px `--focus` ring, 1px offset), keyboard operability for every action, 4.5:1 label contrast computed from the drawn fill (`inkFor()`), category information carried as label plus pattern (the Caches hatch, swatch dots) never color alone, real `<button>`s with `aria-pressed`, segmentation via `role="tablist"`, and `role="status"` hover cards kept intentionally flood-free (`aria-live="off"`, keyboard parity via focus). Both appearances resolve from the same tokens.

Last verified: 2026-10-19 · `space` 0.1.0 · checked against the working tree (no git history in this folder).
