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
- A recognized file extension wins over folder context, including `target`,
  `node_modules`, `.cargo`, Temp and installed-app folders. `.rmeta`, `.o`,
  `.obj` and `.lib` are Code; `.dll` is Apps except below a drive/share-root
  `Windows` directory, where it is System. A project folder named `drivers`,
  `System32` or `Windows` does not make its DLLs System.
- Missing and unknown extensions reach known-filename rules, then strong and
  soft directory context. Directories themselves use context, not extensions;
  finalized directory colors still follow their largest descendant category.
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
  read-only with `SQLITE_OPEN_READ_ONLY`; MEDIA returns tags and embedded cover
  art, never playback. The inspector service does not call Windows thumbnail
  handlers; the opt-in rich tile path is separate and described below.
- At most four heavy previews run concurrently; the LRU holds 256 entries.
- No execution, process spawn, extraction to disk or write path. Symlinks and
  unsupported node types degrade to `Unavailable` or META with a reason.
- Malformed input never panics.

Tests: `tests/preview.rs` including every family, cache bounds/reuse across
clones, malformed archives/images/databases, symlink refusal and read caps.

## Tile art - `src/preview/tiles.rs` + `src/ui/tile_art.rs`

```rust
pub enum TileKind { Image, Text, SvgText, Folder, List, Db, Media, Executable, Meta, Hex }
pub enum FactKind { List, Db, Media, Executable, Meta, Hex }
pub fn tile_kind_for_name(name: &str) -> Option<TileKind>;
impl TileService {
    pub fn art(&self, path: &Path, kind: TileKind, edge: u32, rich: bool)
        -> Option<Arc<TileArt>>;
}
```

`TileArt` is `Image { width, height, rgba }`, `Text { lines }`,
`Facts { kind, lines }`, `Folder { files, items, categories }` or
`Unavailable`. It feeds the on-tile content in the canvas overlay (thumbnails,
snippets, factual rows, folder summaries, icon fallbacks); the inspector is not
involved. Every file name maps to a family: images, text/SVG source,
archives/Office/disk images (LIST), SQLite (DB), audio/video (MEDIA), Windows
executables/DLLs (executable version resource), and PDF/font/unknown/other
binaries (META or HEX). A failed decode falls back to the bundled icon.

Bounds and safety:

- Requests are issued for visible non-aggregate file tiles (and childless
  folders) in the Large tier, or at least 90x90 logical pixels in the compact
  tier, from the same scene tile list the GPU fills and hit testing use.
  Containers and aggregates never request file art. At most 8 new jobs per
  update and three concurrent disk jobs; results carry a generation that is
  bumped on navigation, filter, mode and rich-toggle changes, and stale results
  are dropped.
- `rich` is part of the cache key. It is set only by the opt-in setting labelled
  exactly **"Rich previews (uses Windows)"**, OFF by default. No shell call is
  made while it is off. `SPACE_RICH_PREVIEWS=1` is the review-harness override;
  otherwise the shipping default is off.
- When on, the shell is asked only for an image via
  `IShellItemImageFactory`
  (`SIIGBF_BIGGERSIZEOK | SIIGBF_RESIZETOFIT | SIIGBF_THUMBNAILONLY`) for
  families whose in-process decoder cannot render real content: PDF, video,
  Office documents, fonts, audio without embedded art, ebooks/AI/EPS and
  unsupported image codecs. Thumbnail-only means a provider that has no real
  image, or would return only a generic file-type icon, fails the call instead
  of being painted as content. The call runs on a dedicated worker thread with
  an STA COM apartment, rejects files above 2 GiB before any shell call, and
  waits at most 1.8 s; timeout or failure returns `None` and the in-process
  preview/metadata path is used. No shell verb, file execution, process spawn,
  write or extraction is performed.
- Images are rejected from length alone above 32 MiB before any read and
  decoded to at most 384 px (`TILE_IMAGE_EDGE_MAX`). Text reads a bounded
  16 KiB head at most 40 lines (`TILE_TEXT_HEAD`, `TILE_TEXT_LINES`); a 60 GB
  file is never read beyond that head.
- LIST reads bounded central directories / package indexes; DB uses the same
  read-only bounded parser as the inspector; MEDIA parses tags and embedded
  cover art; META and HEX are bounded rows. Folder summaries are computed in
  memory from `Tree::category_totals` and never touch disk. A partial filter
  container uses a folder icon and matching byte figure instead of showing
  full-folder composition beside a partial size. `art()` refuses symlinks,
  non-files and empty paths.
- Decoded art lands in count- and byte-bounded LRUs (128 entries / 48 MiB) and
  is cached at thumbnail size, never at full resolution.
- Icons come only from the bundled Lucide SVG bytes rasterized in-process with
  resvg/usvg; usvg drops scripts and external references. There is no
  `SHGetFileInfo` icon path, no video frame extraction, and no write path.
- The UI thread never reads or decodes: workers send results over a channel and
  `TilePlanner` drains them on its regular tick.

Tradeoff: Windows thumbnail handlers are third-party in-process code. They can
be slow, install shell extensions, or block on unavailable providers, which is
why the feature is opt-in, size-gated, timeout-bounded, off the UI thread, and
always falls back. The setting is a deliberate trade of stronger preview
coverage against handing a file path to shell code; users who never enable it
get the fully in-process path.

Tests: `preview::tiles::tests` (bounded thumbnail, byte-cap rejection without
reading, bounded text head, every file family, archive/database/media/executable
facts, rich key separation, decode-failure fallback, folder kinds never read,
count/byte eviction), `preview::winthumb::tests` (invalid edge/size requests are
refused before a shell call) and `ui::tile_art::tests` (request gating, dedupe,
per-update limit and edge buckets).

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
- Each candidate row is compared with the last accepted aspect ratio. Equal
  file sizes form compact cells instead of accumulating into thin strips.
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
- Map and filtered scenes are produced only by `build_scene` /
  `build_filtered_scene`; `scene_tiles` exposes the same list to the tile
  planner, so geometry has one source. A folder nests only when its
  camera-transformed tile is at least `NEST_MIN_W` x `NEST_MIN_H`
  (156 x 108 screen px), it has children, and its depth is below
  `nest_depth_budget(scale) = 1 + floor(ln(scale) / ln(1.25))`: one level at
  fit, one more per 1.25x zoom. There is no absolute depth cap; the camera's
  `MAX_SCALE` bounds the budget instead. Only branches whose transformed rect
  intersects the viewport are visited, and one scene build stops at
  `MAX_TILES = 9000`. Header strips, gutters and insets are constant screen
  pixels and converted through the camera scale. Pass-through folder chains
  still collapse without changing the scanned tree.
- Wheel zoom is anchored at the pointer, drag pans, `Fit`/`Ctrl+0` restores
  the identity camera. As zoom grows, deeper folders open lazily; at fit a long
  single-child chain no longer opens down to its files.
- Each sibling group renders at most 63 real children plus one non-selectable
  hatched aggregate (`TAIL_ID` in layout, `NO_NODE` in the scene). Positive-size
  tiles below the 60x36 Small tier also join this remainder, even below the
  count cap. Its `N smaller items` label and byte total are exact for the
  represented siblings; its area is proportional and never inflated for text.
  Single-item remainders use `1 smaller item`.
- The readability cutoff uses the same transformed geometry for fills,
  labels, art planning and hit tests. Each reduction removes at least one of
  the 63 candidates, limiting consolidation to 64 layouts per sibling group;
  the full tail is summed once. Zoom can expose previously aggregated items.
- Caches and remainder tiles use parallel 45-degree hatching, clipped to the
  tile and visible viewport. Container hatching stays in the header, so it
  cannot cover a child's thumbnail or text. Hatch work scales with visible
  pixels, including at maximum zoom.
- Tile content is chosen from screen-space tiers (`tile_tier`) of the same
  scene rects; every label is fitted with the measured Geist width estimate and
  the content bands (`tile_content_layout`) are disjoint and inside the tile,
  so nothing overlaps or spills. The canvas publishes its actual draw bounds
  each frame so the tile planner only requests art for visible tiles.
- Tile type uses the shared Geist/Geist Mono scale: 11 px metadata and snippets,
  12 px compact names and group headers, 13 px large names, and 15/22/28 px size
  figures. Compact art keeps separate name and size rows; short aggregate
  tiles omit the size before it can overlap the name. Snippet continuation is
  a solid ellipsis, with no fade or gradient over the text.
- Dark categories retain separate terracotta, amber, green, cyan, lilac, rose,
  slate-blue and stone fills. Category ink is used for metadata on colored
  tiles; the palette's computed minimum dark ink/fill contrast is 5.69:1.
- Deep links: `SPACE_SCAN`, `SPACE_SCREEN=drives|scan|explorer`,
  `SPACE_THEME=light|dark`, `SPACE_RICH_PREVIEWS=0|1` (review override; the
  shipping default is off), plus a CLI directory argument. `SPACE_SHOT_DIR`
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

## Observed verification - 2026-10-31

`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and
`cargo build --release` pass. `cargo test --all-targets` passes 146 tests:
library 72, engine 16, preview 28, render 9, treemap 21; the two binaries
contain no tests. The canvas suite covers the one-level fit gate, multi-level
opening under zoom, viewport culling and the zoom-to-depth budget. The preview
suite covers every tile family, the rich/in-process cache split and the
pre-shell size/edge guard.

Real UI scans on the release binary:

- `C:\`: 991,828 files, 182,594 folders, 188 GB, 308 skipped, about 3.9 s.
  At 100% the scene showed the top-level tiles plus one inner level. Wheel
  zoom at 194% and 376% opened deeper branches and revealed META/HEX facts on
  large files (`.wim`, `pagefile.sys`), with light and dark renders matching.
- The `src` folder: 39 files, 6 folders, 677 KB in 1-2 ms. Every large Rust
  tile rendered real Geist Mono source lines in both themes; compact tiles
  degraded to fewer lines without overlap.
- The reference-image folder: 11 files, 2.62 MB in 1 ms. Real JPEG/PNG
  thumbnails were visible in both themes at 100% and 139%.

The settings gear opened the panel. "Rich previews (uses Windows)" read Off on
startup, and clicking the row switched it to On with a bounded-work toast. With
the setting on, the release build rendered real Windows thumbnails for an MP4
(a video frame) and a TTF (an "Abg" specimen sheet). The installed PDF provider
returned only a generic Chrome PDF icon; because the request now includes
`SIIGBF_THUMBNAILONLY`, that icon is rejected and the tile falls back to PDF
META facts (format, pages, size). With the setting off, the same folder showed
MEDIA/META rows and no shell thumbnails. Office content previews remain
unverified because no Office provider was available.

Posted Win32 mouse messages exercised double-click drill from `src` into
`bin`, Back, Forward, breadcrumb return to `src`, drag pan at 270%, Fit back to
100%, wheel zoom and the settings controls; each produced the expected state.
Physical hardware input and real focus transitions are still unverified.

Selected captures are in the ignored `target/review/final/`, including
`rich-on-light.png`, `rich-off-light.png`, `rich-pdf-fallback.png`,
`gesture-drill-bin.png`, `gesture-src-return.png`, `drag-zoom-270.png`,
`drag-panned-270.png` and `fit-restored.png` alongside the earlier C:\, source,
image and settings captures. `docs/QA.md` and `docs/qa/*.png` were left
untouched. Nothing was committed or pushed.
