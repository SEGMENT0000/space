# space - status

Updated: 2026-10-31 against the working tree. Windows 10 Pro N 19045,
i5-12400F, 24 GB RAM, rustc/cargo 1.99.1. Scan, interaction and screenshot
figures below are from this working tree unless marked historical.

## Implemented

- Native root Cargo project: library `space`, UI binary `space`, benchmark
  binary `space-bench`. No Tauri, React, npm or web runtime remains.
- Iced 0.14 shell with shader/WGPU treemap fills, Canvas labels and hit-testing.
- Read-only Windows walker with cancellation, progress, error counts, exact
  totals, largest files and eight-category classification.
- Drives, live Scan and Explorer screens; map/composition modes; summary strip;
  inspector with identity, category, size/share, path, exact composition and
  largest files; status bar; light and dark themes.
- Recursive bounded filter, 63+tail aggregate, camera zoom/pan/refit, history,
  breadcrumbs, command palette, context menu, toasts, drag-drop target, deep
  links and keyboard shortcuts.
- Explorer toolbar Back/Forward/Up controls with availability states, clickable
  breadcrumbs with a collapsed-path ellipsis, a Volumes affordance, and the
  inspector collapsed by default.
- Camera-driven map: wheel zoom anchored at the pointer, drag pan, `Fit` and
  `Ctrl+0` refit. `build_scene`/`build_filtered_scene` open a folder only when
  its transformed tile is at least 156x108 screen px and its depth is below
  `1 + floor(ln(scale)/ln(1.25))`; fit shows one inner level and each 1.25x of
  zoom admits one more, with no absolute depth cap. Only viewport-intersecting
  branches are visited, scene builds stop at 9000 tiles, and header strips,
  gutters and insets stay constant in screen pixels. A long single-child chain
  therefore no longer opens to its files at 100%.
- Rich tiles: every sufficiently large file tile shows real content - image
  thumbnails, Geist Mono text/code/JSON/CSV snippets, sanitized SVG source,
  audio tags and embedded cover art, archive/Office entry listings, SQLite
  table names, executable icon plus version-resource lines, and metadata or
  bounded hex for everything else. Folder tiles carry file/item counts with a
  stacked category bar. Medium, small and tiny tiers degrade cleanly. Art loads
  on bounded background workers with count/byte LRUs; the safety contract is in
  docs/CONTRACTS.md. Opt-in Windows shell previews request real thumbnails only,
  so a provider that returns just a generic file-type icon is rejected in favor
  of the in-process facts.
- Settings overlay in the top bar. "Rich previews (uses Windows)" is OFF by
  default and controls opt-in Windows shell thumbnails for PDF, video, Office,
  fonts, audio without embedded art and other formats without an in-process
  decoder; icon-only shell results are rejected. The switch description states
  the 2 GiB gate, 1.8 s timeout, no-execution rule and in-process fallback. `F12`
  remains the review-only capture path.
- Extension-first file classification, including inside build/dependency and
  cache folders. Rust metadata and native object/library files are Code;
  DLLs are Apps unless they are inside the drive/share-root Windows tree.
- Tiny siblings join one proportional, non-selectable remainder even below the
  63-real-tile cap. Squarify compares each row against its latest accepted
  aspect ratio, keeping equal file sizes in compact cells instead of long
  strips.
- Distinct category hues in both themes, clipped Caches/remainder hatching,
  quieter outlines and shared 11/12/13 px labels with 15/22/28 px mono figures.
  Compact thumbnails keep separate name/size rows; snippets use a solid
  continuation mark. Partial filter containers never show full-folder
  composition beside a matching-bytes figure.
- Bounded previews: TEXT, HEX, IMAGE, LIST, META, DB and MEDIA. No execution,
  playback, writes or shell calls from previews; the opt-in rich tile path is
  the only shell call and never opens a file for execution.
- Internal F12 screenshot harness and a headless benchmark harness.
- Repository cleanup removed 16 unreferenced Lucide SVGs; the 30 assets mapped
  by `src/ui/icon.rs` remain, and every icon variant is used.

## Verification

### This continuation - 2026-10-31

- `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings`:
  passed.
- `cargo build --release`: passed; `target/release/space.exe` was rebuilt.
- `cargo test --all-targets`: passed, 146 tests (library 72, engine 16,
  preview 28, render 9, treemap 21; the two binaries have no tests). This
  includes the new fit-gating, zoom-to-depth, no-absolute-depth-cap and
  pre-shell rich-request guard regressions.
- Real `C:\` scan in the UI: 991,828 files, 182,594 folders, 188 GB, 308
  skipped, about 3.9 s; 66.2 GB free of 270 GB. Fit showed the leading folders
  plus one inner level. Wheel zoom at 194% and 376% opened deeper branches and
  exposed real META/HEX facts on large files (`.wim`, `pagefile.sys`, `.pdb`
  blobs). Caches/aggregate hatching stayed clipped to its tiles.
- Real `src` folder: 39 files / 6 indexed folders / 677 KB, 1-2 ms. Every
  large Rust tile rendered real Geist Mono source lines at 100% in both
  themes; compact tiles showed fewer lines without overlap.
- Reference-image folder: 11 files / 2.62 MB, 1 ms. Real PNG/JPEG thumbnails
  rendered in both themes at 100% and 139%.
- Settings panel verified from the release binary: "Rich previews (uses
  Windows)" read Off on startup, switched to On, and its description names the
  2 GiB gate, 1.8 s timeout, no-execution rule and in-process fallback. With it
  on, real Windows thumbnails rendered for an MP4 (video frame) and a TTF (font
  specimen). The installed PDF provider returned only a generic PDF icon;
  thumbnail-only requests reject it, so the tile shows PDF META facts (format,
  pages, size). With it off, the same folder showed MEDIA/META facts and no
  shell thumbnails. No Office provider was available to test.
- Posted Win32 mouse input exercised double-click drill from `src` into `bin`,
  Back, Forward, breadcrumb return to `src`, drag pan at 270%, Fit back to
  100%, wheel zoom and the settings controls; every action produced the expected
  view change.
- Selected captures are in ignored `target/review/final/`, including the earlier
  `light-fit.png`, `light-zoom-164.png`, `light-zoom-319.png`, `dark-fit.png`,
  `dark-zoom-194.png`, `dark-zoom-376.png`, `src-light-100.png`,
  `src-light-164.png`, `src-dark-100.png`, `img-light-100.png`,
  `img-light-139.png`, `img-dark-139.png`, `settings-off.png` and
  `settings-on.png`, plus `rich-on-light.png`, `rich-off-light.png`,
  `rich-pdf-fallback.png`, `gesture-drill-bin.png`,
  `gesture-src-return.png`, `drag-zoom-270.png`, `drag-panned-270.png` and
  `fit-restored.png`.
- The window was driven with posted Win32 mouse and keyboard messages rather
  than physical hardware. Fit, zoom, drag pan, double-click, Back/Forward,
  breadcrumb, F12 capture, fonts, previews, themes and the settings toggle were
  exercised. `docs/QA.md` and `docs/qa/*.png` were left untouched.
  Nothing was committed or pushed.

### Prior recorded checks - not rerun or re-inspected here

- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean, 0 warnings.
- `cargo test --all-targets` (2026-10-29): 110 tests passed - library 40,
  engine 13, preview 28, render 9, treemap 20; 0 failed. This includes the new
  tile-tier, content-band, planner-gating, preview byte/size-limit, cache
  eviction and decode-failure-fallback regressions.
- `cargo build --release`: clean, no warnings.
- `/impeccable detect --json src/ui`: `[]` (no findings).
- Real `C:\` scan in the UI: 945,064 files, 179,321 directories, 178 GB, 301
  skipped, results in 3.6 s. Bench cold/warm: 945,180 files, 179,335 dirs,
  178.44 GiB, 2,986 / 2,825 ms, 94.9 / 95.8 MiB peak working set.
- Real repo scan: 13,094 files, 1,807 directories, 10.9 GB in the UI; 13,101
  files, 1,807 directories, 10.95 GiB in the final bench pass; 0 errors, 28 / 26
  ms.
- Screenshots inspected at 1024x680, 1320x860 and 1728x1080 in both themes for
  drives, scan, explorer, composition, all seven preview families, cancellation,
  permission error, deep links, palette, context menu, filter, sort, zoom/pan,
  legend category hide/show and history. Interaction evidence includes filtered
  typing, clipboard contents, a palette jump that selects/previews without
  opening the file, legend hide/show with `1 category hidden`, selection kept
  across tab switches, inspector wheel scrolling at 100% map zoom, and map wheel
  zoom still working.
- Step 2 F12 captures reviewed on the final release binary in both themes:
  image-thumbnail tiles (light and dark), text-snippet tiles, icon tiles with
  extension badges, nested-folder scenes, and a folder summary tile whose
  stacked category bar shows distinct segments. Medium tiles show icon + name
  + size; small tiles show name only. Mouse Back/Forward and disabled-button
  states are left for manual review.
- Read-only audit: no write/mutation API in engine, preview, model, treemap or
  render; the only `create_dir_all` is the opt-in F12 screenshot saver.

See `docs/BENCHMARKS.md` for the measured numbers and `docs/QA.md` for the
reviewed states, test coverage, defect log and remaining gaps.

## Open QA

- Office content thumbnails and real PDF page thumbnails depend on installed
  shell providers. This machine had no Office provider and its PDF provider
  returned only a generic icon, so those remain unverified; the app rejects
  icon-only results and uses in-process META/LIST facts instead.
- Pointer gestures were exercised with posted Win32 messages, not physical
  hardware or real focus transitions. Category hiding, sorting, filter typing
  and inspector scroll are also outside the new capture evidence.
- Drag-hover overlay and a real OS file drop have not been captured from a
  physical drag yet; the winit event path is wired and reviewed.
- macOS/Linux behavior, fractional DPI and screen-reader accessibility remain
  unverified.
- Benchmarks are single-workstation measurements with an unpurged OS cache.

## Constraints locked

- Read-only by construction; no cleanup action exists.
- Eight categories only; exact composition on demand with a bounded cache.
- Windows verified only. macOS/Linux fallbacks compile but have not been run.
- Geist/Geist Mono only; no banned families, gradients, glass, glow or decorative
  animation.
- The name is lowercase `space`; the retired orange badge does not exist.
