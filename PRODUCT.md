# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

This build ships as a native Windows desktop application in pure Rust. Iced 0.14
owns the application shell, input, focus and accessibility tree; the treemap
fills are drawn through Iced's WGPU renderer from live filesystem data. There is
no web runtime, no Tauri, no React and no IPC boundary. One UI binary,
`space.exe`, built by `cargo build --release`; `space-bench` is a separate
headless diagnostic binary.

Source layout:

- `src/engine/` — read-only walker, classification, volume listing.
- `src/preview/` — bounded previews; text, hex, image, list, meta, SQLite, media.
- `src/treemap/` — pure squarify, camera, hit-test and label geometry.
- `src/render/` — instanced WGPU fill pipeline.
- `src/ui/` — shell, screens, inspector, overlays and design tokens.
- `DESIGN.md` — visual specification and theme tokens.
- `docs/` — architecture, contracts, QA and measured benchmarks.

The window opens at 1320×860 logical pixels with a minimum of 1024×680 and is
freely resizable. Windows 10/11 is the verified target. The portable walker and
platform-specific paths exist for macOS/Linux, but those platforms have not been
run and are not claimed as verified.

Fonts: Geist and Geist Mono static TTFs are bundled in `assets/fonts/` with
their OFL license. Geist carries labels and body text; Geist Mono carries
tabular figures, paths and keycaps. The banned families (Inter, Roboto, Arial,
Open Sans, Helvetica) are neither bundled nor referenced.

Read-only is structural. The scanner only enumerates directories and reads
metadata; previews only open files for bounded reads. No write, rename, delete,
cleanup or move API is linked into the production engine. Opening or revealing
a path is an explicit user action handed to the OS shell, never a preview.

## Users

A technical desktop user who judges interface craft at pixel level and works
keyboard-first. The app is intended to ship, so the bar is product-grade rather
than a prototype. No external audience has been confirmed.

## Product Purpose

`space` shows what is consuming storage on a Windows machine. It scans a volume,
a folder chosen in the OS picker, a folder dropped on the window, a CLI path, or
`SPACE_SCAN=<path>`, then renders the result as a progressive hierarchical
treemap. Success means the user finds the large consumers within seconds, reads
proportions correctly at a glance, and never has to trust the app with their
data: storage is presented as an instrument, not a cleanup checklist.

## Positioning

A read-only, local-first disk analyzer whose treemap is the product: native
WGPU rendering, recursive drill-down over real hierarchy, fixed category
semantics, no cloud, no telemetry, and no delete path anywhere in the codebase.
The claim is structural rather than a promise in marketing copy.

## Operating Context

A native desktop window with three screens and a persistent application bar:

- **Drives** — mounted volumes with capacity, used, free and utilization; a
  measured utilization ribbon; one quiet Scan action per volume; a folder picker;
  and a plain read-only statement.
- **Scan** — a live instrument for a real traversal: files, folders, bytes,
  skipped entries, observed throughput bars, elapsed time, the directory being
  read and a working Cancel action. A cancelled scan returns to the volume list
  with a toast. A failed scan shows the engine's message verbatim (or a generic
  failure explanation) with recovery actions.
- **Explorer** — a top bar with navigation history and breadcrumbs, a recursive
  filter field, a Map/Composition segmented view, sort control, inspector rail
  toggle and command palette. The treemap is the main surface; a five-figure
  summary strip sits above it. The inspector rail holds selection identity,
  category, size/share, path, previews, exact composition and largest files.
  The status bar reports items or matches, files, bytes, zoom and elapsed time.
- **Overlays** — a command palette for commands and scanned files, a context
  menu (Open, Show in parent folder, Copy path), drag-hover target, and short
  toasts. Each is dismissed by Escape or a click outside where appropriate.

The map has two modes. **Map** shows the current folder as a nested treemap;
directories can be drilled by double-click or Enter, the wheel zooms around the
pointer, left-drag pans, and Backspace goes to the parent. **Composition** shows
a ranked category breakdown for the scanned root with exact byte totals and a
proportional bar. Filtering searches recursively within the current subtree and
rebuilds the map around real matching ancestors.

## Capabilities and Constraints

Confirmed scope — every capability is read-only:

- Enumerate mounted volumes with capacity, used, free and utilization.
- Scan a volume, a picked folder, a dropped folder, a CLI path or an environment
  path with live progress, truthful counters, throughput samples, error counts
  and cancellation.
- Render a progressive hierarchical treemap colored by eight fixed categories:
  Media, Documents, Code, Caches, Apps, Archives, System, Other.
- Recursive drill-down with accurate hit testing, wheel zoom, left-drag pan,
  refit, history and breadcrumbs. Large folders render up to 63 real children
  plus one hatched aggregate tail; every tail label reports the real remainder.
- Adaptive, clipped, non-overlapping labels. Labels disappear before they can
  overflow a tile; folder interiors win over header text when space is tight.
- Category legend with hide/show toggles. Hidden categories leave the map and
  status counts; they never render as fake ghost tiles.
- Composition mode with exact category byte totals, percentages and file counts.
- Sort the inspector's largest files by size or name.
- Inspector previews: TEXT, HEX, IMAGE, LIST (folder/archive/table), META, DB
  (read-only SQLite) and MEDIA metadata. Previews are bounded and never execute
  or play content; media uses metadata plus an optional OS thumbnail.
- Selection, hover identity, context menu, copy path, open and reveal.
- Command palette with 11 commands and scanned-file search.
- Light and dark themes, both first-class; `T` toggles when text focus is free.
- Drag a folder onto the window to scan it.
- Keyboard: Backspace parent, Alt+Left/Right history, +/− zoom, Ctrl+0 refit,
  Ctrl+F or `/` filter, Ctrl+K palette, Enter open, Ctrl+C copy path, Escape
  close/clear. Shortcuts do not fire while a text input is being edited, except
  Escape, Ctrl+K, Ctrl+F and F12.
- Deep-link equivalents: `SPACE_SCAN`, `SPACE_SCREEN=drives|scan|explorer`,
  `SPACE_THEME=light|dark` and a CLI folder argument.
- Internal review harness: `SPACE_SHOT_DIR` plus F12 saves the rendered client
  area, including WGPU output. The shipping UI never takes screenshots on its
  own.

Binding constraints:

- Banned type: Inter, Roboto, Arial, Open Sans, Helvetica.
- Banned decoration: purple-to-blue or mesh gradients, glassmorphism, ambient
  glows, uniform card shadows, generic card grids, lorem or placeholder content,
  pulsing status dots, and decorative animation.
- The treemap must fill its container at the correct aspect ratio at every
  window size. Labels must never overlap, clip mid-word, or escape their tile.
- Content must not overflow the inspector rail, palette, context menu, summary
  strip or status bar at any tested size.
- The left mouse button carries pan; no middle-button-only gesture.
- Synthetic or demo scan data is not shipped. Personal paths appear only as the
  live result of the user's own scan.

## Guarantees

- **Read-only is structural.** The engine links only directory enumeration and
  metadata reads. Previews open files without write access and never execute
  them. There is no delete, rename, move, clean or write UI action to wire up.
- **Honest numbers.** Progress is indeterminate because the scanner cannot know
  the final total in advance. Counters, sizes, error counts, throughput and
  composition are measured; unknown values print as a dash. Failure text comes
  from the engine, not a fabricated friendly substitute.
- **Accurate geometry.** The same scene used for WGPU fills drives hit testing,
  hover, selection, labels and the tail aggregate. Treemap unit tests cover
  containment, overlap, camera inversion, zoom clamping and label safety.
- **Bounded work.** The recursive filter is capped at 120,000 visits and 2,000
  matches; treemap children are capped at 64 visible entries with the remainder
  expressed as one aggregate tile; previews read bounded byte ranges and use a
  bounded LRU cache with four heavy-work permits.
- **Keyboard parity.** Every primary action has a key; text inputs keep their
  own keys; the focus sink releases fields deterministically.
- **Measured performance.** Cold and warm scans, throughput, memory, geometry,
  navigation, previews and cancellation are recorded from real runs on a named
  machine in `docs/BENCHMARKS.md`.

## Evidence on Hand

- The built implementation is the evidence: `src/` compiles into a native
  `space.exe`; the headless `space-bench` links the same engine.
- The test suite covers engine totals, classification, cancellation, permission
  failures, unusual entries, empty directories, preview safety, cache bounds,
  treemap geometry and render draw lists. See `docs/QA.md`.
- Screenshots are captured from the running binary through the internal F12
  harness at 1024×680, 1320×860 and 1728×1080 in both themes. See `docs/QA.md`
  for the checked states.
- No stock photography, testimonials, customer names or third-party proof exist.
  Future work must not fabricate any.

## Brand Commitments

Name: `space`, set lowercase. The former orange "s" letter-tile badge is retired
and must not return in any form. A mark, if any, must be meaningful to the
subject (volume, blocks, proportion) or be absent.

Voice: plain and technical. No marketing language, no hype, no cute empty-state
copy. Numbers are exact, units are explicit, and failures say what happened.

**Standing aesthetic preference:** the craft bar is Apple's system utilities —
Finder, Disk Utility, Photos. Every surface must read as pixel-perfect and calm:
warm paper neutrals in light mode and deep neutral green-charcoal in dark mode,
one type family at real weight steps, hairline separators instead of cards and
shadows, an 8-point spacing rhythm, tabular figures for data, and restrained
motion. Borrow craft discipline only; never Apple chrome, icons or copy.

## Product Principles

1. Read-only is structural, and the interface says so plainly.
2. Proportion is the message: area encodes size and chrome never competes.
3. Speed is a feature: instant feedback, keyboard-first operation, motion that
   never blocks input.
4. Nothing decorative without meaning: if an element carries no information or
   affordance, cut it.
5. Pixel-level craft is the acceptance bar; "looks modern" is not a pass.

## Accessibility & Inclusion

The native baseline:

- Every interactive control is reachable by keyboard; text inputs are labelled
  by their surrounding context and never trap global Escape.
- Focus is visible through Iced's widget states and the sage accent.
- Category identity is carried by label and pattern as well as color: Caches
  and the tail aggregate use diagonal hatching; legends pair swatches with names.
- Label ink is chosen from the drawn fill and contrast-tested in `theme.rs`.
- Motion is short and non-blocking; there are no pulsing or blinking indicators.
- Both appearances use the same semantic tokens, so contrast does not depend on
  which theme happens to be active.

Last verified: 2026-10-26 — `space` 0.2.0 — against the working tree.
