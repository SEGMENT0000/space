# QA

Final acceptance record for `space` 0.2.0, reviewed on 2026-10-26 against the
working tree and the release binaries built from it. Every screenshot and
benchmark below came from a real process reading real files on the machine named
here. No mock UI, synthetic scan result or placeholder content was used.

## Environment

| | |
| --- | --- |
| CPU | 12th Gen Intel Core i5-12400F |
| RAM | 24 GiB |
| OS | Windows 10 Pro N, 19045 |
| Display | 100% scale, hardware-accelerated WGPU surface |
| Toolchain | rustc/cargo 1.99.1, edition 2024 |
| Build | `cargo build --release`; opt-level 3, thin LTO, 1 codegen unit, stripped |
| Primary binary | `target\release\space.exe` |
| Diagnostic binary | `target\release\space-bench.exe` |
| Input method | Real keyboard events via `keybd_event`, real client-space mouse events; F12 screenshot harness |

## Release gates

All commands were run at the repository root on the final working tree.

| Gate | Command | Result |
| --- | --- | --- |
| Formatting | `cargo fmt --check` | Pass, no diff |
| Lints | `cargo clippy --all-targets -- -D warnings` | Pass, 0 warnings |
| Tests | `cargo test --all-targets` | 94 passed, 0 failed, 0 ignored |
| Release build | `cargo build --release` | Pass, no warnings |
| Design scan | `/impeccable detect --json src/ui` | `[]`, exit 0 |

The release build produced both binaries in `target\release\`. A final rebuild is
required after any source change; the screenshots in `docs/qa/` are from that
release build, not from `cargo run` debug output.

## Automated coverage

`cargo test --all-targets` executes 94 tests:

| Suite | Tests | Coverage |
| --- | ---: | --- |
| `tests/engine.rs` | 13 | Exact totals and reconciliation, top-1000 largest files, context-aware classification, empty roots, invalid roots, read-only scan, junction avoidance, long paths, pre-start and in-flight cancellation, terminal-event contract, disappearing entries, ACL-denied entries |
| `tests/preview.rs` | 28 | TEXT/HEX/IMAGE/LIST/META/DB/MEDIA families, text and byte caps, UTF-16, script-scrubbed SVG, image pixel caps, ZIP/TAR/ISO/spreadsheet/Word/ODT/ODS listings, bounded read-only SQLite, malformed and empty files, PE/ELF/PDF/font metadata, symlink refusal, unreadable files, cache bounds and clone sharing, no-write audit |
| `tests/treemap.rs` | 20 | Randomized containment/overlap/proportion checks, u64 overflow, gutters, degenerate and pathological aspect ratios, nested layout bounds, 10,000-item smoke test, camera identity/inversion/clamping/non-finite input, hit testing, label bounds, grapheme clusters, control scrubbing and hidden tiny tiles |
| `tests/render.rs` | 9 | Draw-list bounds, culling, ordering, sRGB-to-linear conversion, alpha and color packing, zero-sized surfaces, shader construction and draw-list swap |
| Library unit tests | 24 | Model category-cache insertion/eviction, recursive filter bounds and cases, preview-cache eviction, render unit conversions, treemap label maths, canvas pointer-event bounds gating, and light/dark contrast guards |

The read-only guarantees are asserted, not assumed:

- `scan_is_read_only` snapshots file bytes, mtimes and directory-entry counts
  before and after a scan, then compares them.
- `previewing_does_not_write_files_or_add_directory_entries` previews text,
  image, ZIP and SQLite fixtures, then checks that no bytes or entries changed.
- `sqlite_preview_is_read_only_and_bounded` compares the database byte-for-byte
  after preview and checks the row/cell caps. The implementation opens with
  `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` and also sets
  `PRAGMA query_only=ON`.
- `junctions_are_not_traversed` creates a real Windows junction and proves its
  payload is not counted.
- `denied_entries_are_counted_and_never_fatal` creates a real ACL-denied
  directory with `icacls`, scans the parent, and asserts readable totals plus
  `errors >= 1`; the ACL is restored before assertions.
- Cancellation tests assert the terminal contract and a prompt stop; the
  permission/error tests prove unreadable entries are skipped and counted.

## Read-only audit

A source scan for mutation APIs is part of the final review:

```powershell
rg -n "fs::write|write_all|File::create|create_dir|remove_file|remove_dir|rename\(|copy\(|set_permissions|create_new|OpenOptions" src
```

- No mutation API appears in `src/engine`, `src/preview`, `src/model.rs`,
  `src/treemap` or `src/render`.
- The only `create_dir_all` is the F12 screenshot saver in `src/ui/app.rs`; it is
  enabled only when `SPACE_SHOT_DIR` is set and is not part of normal use.
- Opening and revealing are explicit user actions. They pass a path to the OS
  shell in `src/ui/platform.rs`; previews never call them.
- Previews never execute a file, start playback, extract an archive or write a
  thumbnail. Media uses embedded cover art or metadata and states when a video
  thumbnail is unavailable.
- There is no network client, telemetry endpoint, cleanup action or deletion path
  in the application.

## Visual matrix

The curated captures are in [`docs/qa/`](qa/). All were taken from the release
binary through the internal F12 harness at the stated client size. Both themes
and all three supported size classes were exercised; the matrix is not a claim
that every state was re-captured at every size × theme permutation.

| State | Reviewed coverage | Curated evidence |
| --- | --- | --- |
| Drives | 1320x860 light and dark, 1024x680 dark, 1728x1080 light | [light 1320](qa/01-drives-light-1320.png) |
| Explorer map | 1320x860 light and dark, 1024x680 dark, 1728x1080 light | [light 1320](qa/02-explorer-light-1320.png), [dark 1320](qa/03-explorer-dark-1320.png), [dark 1024](qa/04-explorer-dark-1024.png), [light 1728](qa/05-explorer-light-1728.png) |
| Composition | 1320x860 light and dark, 1728x1080 light and dark | [dark 1728](qa/06-composition-dark-1728.png) |
| Live scan | 1320x860 light; real traversal and live counters | [scan](qa/07-scan-live-1320.png) |
| Cancelled scan | 1320x860 light; cancel action and return toast | [cancel](qa/08-cancel-toast-1320.png) |
| Permission error | 1320x860 light; ACL-denied root and recovery actions | [error](qa/09-permission-error-1320.png) |
| Command palette | 1320x860 light; open, query, keyboard selection and Escape | [palette](qa/10-command-palette-1320.png) |
| Context menu | 1320x860 light; real node menu, Escape dismissal | [menu](qa/11-context-menu-1320.png) |
| Preview families | Light and dark 1320: TEXT, HEX, IMAGE, LIST, META, DB, MEDIA; dark 1728: DB | [TEXT](qa/12-preview-text-1320.png), [IMAGE](qa/13-preview-image-1320.png), [TEXT dark](qa/19-preview-text-dark-1320.png), [DB dark 1728](qa/14-preview-db-dark-1728.png) |
| Zoom and pan | 1320x860 light, 1024x680 dark; content clipped to the map at both sizes | [zoom 1024](qa/15-zoom-pan-1024.png) |
| Category visibility | 1320x860 light; legend hides one category and restores it; map and status agree while exact composition still reports every byte | [hidden](qa/16-category-hidden-1320.png) |
| Sort | 1320x860 light; `Size` and `Name` orders both reorder the largest-files list | [name](qa/17-sort-name-1320.png) |
| Filter | 1320x860 light; recursive match keeps the real ancestor chain and reports the bounded count | [pdb](qa/18-filter-pdb-1320.png) |
| Pointer routing | 1320x860 light; off-canvas clicks preserve selection, inspector wheel scrolls, map wheel zooms | [wheel](qa/20-wheel-inspector-1320.png), [selection](qa/21-selection-persists-1320.png) |
| Deep links | `SPACE_SCREEN=drives`, `scan`, `explorer`; invalid path and invalid screen fallback; `SPACE_THEME` light/dark | Captures reviewed; not all duplicated above |
| CLI path | 1320x860 dark; directory argument opens a real scan | [CLI](qa/22-cli-argument-dark-1320.png) |
| Hover, history, copy, theme | 1320x860 light/dark; scripted input and screenshots | Captures reviewed |

Visual checks included: no label collisions or clipping inside tiles, no content
escaping the map during zoom/pan, no overflow in the inspector or summary strip,
legible contrast in both themes, consistent category hues, and no banned
gradients, glass, glow, placeholder text or decorative animation.

## Interaction evidence

These paths were driven against the release binary with real input events and
reviewed from captures:

| Interaction | Expected | Observed |
| --- | --- | --- |
| `Ctrl+K`, Escape | Palette closes, Explorer returns to its prior state | Pass; pre/post captures match |
| `Ctrl+F`, type `pdb`, `T`, Escape | Filter accepts text; `T` types rather than toggling theme; Escape clears and restores the unfiltered map | Pass |
| `T` outside a text field | Theme toggles | Pass |
| Palette jump to `space.pdb`, then `Ctrl+C` | File is selected and previewed; the absolute path is copied; no file is opened as a side effect | Pass; clipboard contained `C:\Users\SEGMENT\Downloads\space\target\debug\space.pdb` |
| Palette Enter | Accepts the highlighted result exactly once | Pass after the trailing-Enter guard described below |
| `Alt+Left` / `Alt+Right` | History travels and repaints the map and breadcrumbs | Pass |
| `Backspace` | Moves to the parent folder | Pass |
| `+`, `+`, `Ctrl+0` | Camera zooms around the center, then refits | Pass |
| Filtered map | Matching ancestors remain; counts and tiles share one bounded view | Pass |
| Hide/show categories | Legend toggles remove and restore real tiles and totals | Pass; Code hides from the legend, status reads `1 category hidden`, and the second click restores it |
| Sort control | `Size` and `Name` orders both reorder the largest-files list | Pass; name order captured |
| Wheel over inspector | Inspector scrolls; map zoom is unchanged | Pass; inspector reached its largest-files section at 100% map zoom |
| Click outside the map | Selection, preview and camera are not stolen by the canvas | Pass; tab and panel clicks preserve the preview; a real map gutter click still clears |
| Right-click, Escape | Node menu opens at the pointer and closes | Pass; light-theme capture |
| Palette row click | Activating a row selects/navigates its real node | Pass; `photo.jpg` selected with IMAGE preview |
| Theme switch | Both themes remain readable; no token falls back to stock Iced colors | Pass |
| Scan, Cancel | Live counters continue, Cancel returns to Drives with a toast, no result tree is installed | Pass |
| Invalid root | Error state reports the engine message with retry/pick actions | Pass |

The palette Enter guard exists because Iced's text input owns Enter through
`on_submit` while raw event delivery can observe the same physical press after
the overlay has closed. Without the guard, a palette jump selected/previewed the
file and then immediately triggered the global Enter action, opening it
externally. The fix consumes only the trailing Enter for 350 ms after palette
acceptance; normal Enter behavior outside that window is unchanged.

## Real-data smoke

| Run | Result |
| --- | --- |
| Full `C:\` scan, UI | 945,064 files, 179,321 directories, 178 GB, 301 skipped, results in 3.6 s |
| Full `C:\` scan, bench cold / warm | 945,180 files, 179,335 directories, 178.44 GiB, 2,986 ms / 2,825 ms, 316.5k / 334.6k files/s, 94.92 / 95.76 MiB peak working set |
| Repository scan, UI | 13,094 files, 1,807 directories, 10.9 GB, 0 errors, 49-75 ms |
| Repository scan, bench cold / warm | 13,101 files, 1,807 directories, 10.95 GiB, 0 errors, 28 ms / 26 ms |
| Cancellation | 25 ms request to terminal in 1.54 ms; 250 ms request to terminal in 7.20 ms |
| Sustained wheel input | 120 frames, median 15.56 ms, p95 16.14 ms, max 19.91 ms |

The two full-volume file counts differ because the runs were not identical
moments on a live workstation. The UI repository count was captured before the
later benchmark pass created a few more build artefacts; the byte counts also
drift as the working tree is rebuilt. Machine, cache conditions and caveats are
recorded in [`BENCHMARKS.md`](BENCHMARKS.md). The benchmark harness does not
purge the Windows file cache, so absolute times are workstation measurements,
not a portable guarantee.

## Defects found and resolved during final QA

1. Category cache could return totals under the wrong cache key; fixed with a
   regression test.
2. Filter and palette text fields swallowed Escape; raw keyboard delivery now
   routes global Escape while preserving widget-owned editing keys.
3. Plain letters typed into the filter could toggle the theme; text-focus handling
   was corrected and verified.
4. Zoom/pan content could escape the map at compact sizes; the map container now
   clips and was recaptured at 1024x680 and 1320x860.
5. The largest-files sort label did not visibly change with the order; the label
   now reports `Size ↓` or `Name ↑` and matches the reordered rows.
6. A palette Enter press could fire twice: accept the result, then open the file
   externally. The trailing-Enter guard was added and verified with selection and
   preview captures.
7. The Canvas program received pointer events outside its layout bounds. Clicks
   on the legend were ignored, unrelated clicks could pan the map and clear the
   selection, and a wheel over the inspector zoomed the map. Presses, releases
   and wheels are now gated on the pointer being inside the canvas, while an
   active drag may continue outside. Regression test:
   `ui::canvas::tests::pointer_events_outside_canvas_bounds_are_ignored`.
   Verified with the legend hide/show path, a tab-switch selection persistence
   check, inspector scrolling at 100% map zoom, and a real gutter click that
   still clears the selection.

## Known gaps

- A physical OS drag-and-drop path has not been captured from an external drag.
  The winit file-hover/drop path is wired and code-reviewed, but the synthetic
  screenshot harness cannot reliably capture the host drag overlay.
- macOS and Linux fallbacks compile but have not been run; no behavior on those
  platforms is claimed.
- Only 100% display scale was reviewed. Fractional DPI remains unverified.
- No screen-reader or full accessibility-tree audit was performed. Native Iced
  widgets provide the baseline focus and labels.
- Benchmarks come from one workstation with a warm, unpurged file cache; they are
  not an isolated lab measurement.
- The screenshot harness itself creates a folder/PNG when `SPACE_SHOT_DIR` is set.
  It is a review tool, not shipping behavior.

## Reproduce

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
target\release\space-bench.exe "C:\" --runs 5
$env:SPACE_SHOT_DIR = "C:\path\to\captures"
$env:SPACE_SCAN = "C:\"
target\release\space.exe
# Press F12 in the app for each reviewed state.
```

Benchmark commands, machine conditions and measured values are in
[`BENCHMARKS.md`](BENCHMARKS.md). Module contracts and the read-only boundary are
in [`CONTRACTS.md`](CONTRACTS.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md).
