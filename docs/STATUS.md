# space - status

Updated: 2026-10-26 against the working tree. Windows 10 Pro N 19045,
i5-12400F, 24 GB RAM, rustc/cargo 1.99.1.

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
- Bounded previews: TEXT, HEX, IMAGE, LIST, META, DB and MEDIA. No execution,
  playback, writes or shell calls from previews.
- Internal F12 screenshot harness and a headless benchmark harness.

## Verification

- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean, 0 warnings.
- `cargo test --all-targets`: 94 tests passed (engine, preview, render, treemap,
  model cache, filter, canvas pointer gating, theme contrast); 0 failed.
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
- Read-only audit: no write/mutation API in engine, preview, model, treemap or
  render; the only `create_dir_all` is the opt-in F12 screenshot saver.

See `docs/BENCHMARKS.md` for the measured numbers and `docs/QA.md` for the
reviewed states, test coverage, defect log and remaining gaps.

## Open QA

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
