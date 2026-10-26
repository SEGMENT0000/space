# Module contracts

`space` is one crate with a library target and two binaries. These are the
interfaces the modules rely on; tests and `space-bench` link the same public
API as the UI. Changing a contract means updating its consumers and the tests in
the same change.

## Model - `src/model.rs`

- `NodeId` is `u32`; `NO_NODE` is the sentinel.
- `Tree` is an immutable arena: `node(id)`, `name(id)`, `path(id)`,
  `children(id)`, `lineage(id)`, `largest_files(limit)`.
- `Tree::category_totals(id)` returns exact bytes across the eight categories
  for a subtree, with a bounded 48-entry cache shared by clones.
- `ScanOutput { tree, summary }`; `ScanSummary` carries the scanned root,
  counters, elapsed milliseconds, exact root composition and the volume total.
- `Category` order is fixed: Media, Documents, Code, Caches, Apps, Archives,
  System, Other. `CategoryTotals` is a plain `[u64; 8]`.
- `ScanEvent::{Progress, Done, Failed}`. Exactly one terminal event follows any
  progress events. `ScanProgress` is indeterminate: there is no final total and
  no percent.

## Engine - `src/engine/`

```rust
pub fn list_drives() -> Vec<DriveInfo>;
pub fn classify_file(name: &str, parent: &Path) -> Category;
pub fn classify_entry(name: &str, parent: &Path, is_dir: bool) -> Category;
pub fn spawn_scan(root: PathBuf, tx: Sender<ScanEvent>,
                  cancel: Arc<AtomicBool>) -> io::Result<ScanHandle>;
pub fn scan_blocking(root: PathBuf, tx: Option<Sender<ScanEvent>>,
                     cancel: Arc<AtomicBool>) -> Result<ScanOutput, String>;
pub struct ScanHandle { pub cancel: Arc<AtomicBool>, pub join: JoinHandle<()> }
```

Requirements:

- Validate the root synchronously. An invalid, non-directory or unreadable root
  returns `Err` and starts no thread.
- Count files, directories, bytes and skipped errors exactly; every file byte is
  counted once across category buckets.
- Check cancellation before every directory and every entry. A cancelled scan
  returns `Err("Scan cancelled")`.
- Never follow symlinks, junctions or reparse points.
- Skip unreadable entries and count them; a permission error is never fatal.
- Hold names as UTF-8 (lossy only when the OS name is not valid Unicode) and
  never panic on unusual names.
- Windows fast path: `FindFirstFileExW` with `FindExInfoBasic` and
  `FIND_FIRST_FILE_EX_LARGE_FETCH`, distributed over worker threads. A portable
  walker exists for other targets and is not verified.
- No write, rename, delete or move API is linked into the engine.

Tests: `tests/engine.rs`, including totals, classification context, cancellation,
permission errors, empty directories, unusual names and terminal-event rules.

## Preview - `src/preview/`

```rust
pub struct Service;             // cheap Arc clone; cache and permit gate shared
pub fn new() -> Service;
pub fn preview(&self, path: &Path) -> Preview;
pub fn cache_len(&self) -> usize;
```

`Preview` is a plain enum: `Text`, `Hex`, `Image`, `List`, `Meta`, `Db`, `Media`
or `Unavailable { reason }`. Families are routed by extension with a content
sniff fallback.

Bounds and safety:

- TEXT at most 64 KiB / 120 lines; HEX at most 4 KiB; IMAGE decoded to a
  thumbnail with byte/pixel/allocation caps; LIST entries capped; DB opened
  read-only with `SQLITE_OPEN_READ_ONLY`; MEDIA returns metadata plus an
  optional OS thumbnail, never playback.
- At most four heavy previews run concurrently; the LRU holds 256 entries.
- No execution, process spawn, extraction to disk or write path. Symlinks and
  unsupported node types degrade to `Unavailable` or META with a reason.
- Malformed input never panics.

Tests: `tests/preview.rs` including every family, cache bounds/reuse across
clones, malformed archives/images/databases, symlink refusal and read caps.

## Treemap - `src/treemap/`

```rust
pub struct Rect { x, y, w, h }                    // f32, top-left origin
pub struct Item { id, size: u64, cat, is_dir }
pub struct Tile { id, cat, is_dir, size, rect }
pub struct Camera { scale, offset_x, offset_y }
pub fn squarify(area: Rect, items: &[Item], gutter: f32) -> Vec<Tile>;
pub fn layout_children(parent: &Tile, children: &[Item], gutter: f32) -> Vec<Tile>;
pub fn hit_test(tiles: &[Tile], camera: &Camera, point: (f32, f32)) -> Option<usize>;
pub fn labels<F, S>(tiles: &[Tile], names: F) -> Vec<Label>
where F: Fn(NodeId) -> S, S: AsRef<str>;
```

- Squarify returns finite, in-bounds, non-overlapping rects in descending size
  order, omitting zero-size items and `NO_NODE`.
- Camera scale is at least 1, wheel zoom stays anchored at the pointer, pan is
  clamped so the background is never exposed, and refit is the identity.
- Hit testing inverts the camera exactly and returns the last (topmost) match.
- Labels are one per eligible tile, conservatively measured, truncated on
  grapheme boundaries and omitted when they cannot fit. `max_width` and
  `max_height` are never exceeded.
- No Iced or WGPU imports.

Tests: `tests/treemap.rs`, including randomized overlap/containment invariants,
degenerate areas, extreme sizes, camera inversion/clamping and label safety.

## Render - `src/render/`

```rust
pub struct DrawTile { rect, color, border, border_width, radius }
pub struct DrawList { size, tiles, ... }
pub struct TreemapProgram; // iced shader Program<Message = ()>
pub fn treemap<Message>(draw: Arc<DrawList>) -> TreemapShader<Message>;
```

- Instanced WGPU pipeline through Iced's shader widget; no CPU rasterization and
  no second window.
- Colors are converted from sRGB to linear before upload; alpha is preserved.
- Degenerate or off-canvas tiles are culled; an empty draw list renders nothing.
- `TreemapProgram` emits no messages. The UI owns all interaction and hit tests
  against the same scene tiles.

Tests: `tests/render.rs` (draw-list bounds, culling, color packing, program swap,
shader construction).

## UI - `src/ui/`

- Single owner of app state, input, focus, overlays and screens.
- `App::update` handles window/filesystem/keyboard messages; `view` builds the
  Iced tree; `canvas.rs` owns the overlay/label scene and hit testing; `gpu.rs`
  converts the same scene to a `DrawList`.
- Canvas pointer gestures are bounds-gated: presses, releases and wheel events
  outside the canvas fall through to the legend, toolbar, inspector and
  scrollables, while a drag that began inside may continue outside the map.
- Focus targets are stable widget ids: `FOCUS_SINK`, `FILTER_INPUT`,
  `PALETTE_INPUT`. Text inputs keep their own keys; global shortcuts are limited
  to `Ctrl+K`, `Ctrl+F`, `F12` and Escape handling.
- The filter view (`filter.rs`) is a bounded recursive view shared by the GPU
  fills, hit-test canvas and status counts. It caps visits at 120,000 and
  matches at 2,000.
- Folders with more than 64 visible children render 63 real children plus one
  non-selectable hatched aggregate tile (`TAIL_ID`).
- Deep links: `SPACE_SCAN`, `SPACE_SCREEN=drives|scan|explorer`,
  `SPACE_THEME=light|dark`, plus a CLI directory argument. `SPACE_SHOT_DIR`
  enables the F12 review capture.

## Bench - `src/bin/space-bench.rs`

Links only public library APIs. Reports wall time, counts, files/s, peak working
set, category composition cold/cached, recursive scene layout, a cold/cached
preview and cancellation latency. Diagnostic modes:

```text
space-bench [PATH] [--runs N] [--json]
space-bench --preview PATH [--json]
space-bench --cancel PATH [DELAY_MS] [--json]
```

`cold` means the first run in the process; the harness does not purge the OS
file cache. Values the platform does not provide print as `n/a`.
