# Native architecture

`space` is a pure-Rust, Windows-first desktop application. Iced 0.14 owns the
window, input, focus and accessibility tree; a custom shader widget draws the
treemap through Iced's WGPU renderer. There is no web runtime, no Tauri, no
React and no IPC boundary. All content comes from a live read-only filesystem
walk.

## Binaries and crates

- `src/lib.rs` - library root, exposes every module for tests and tools.
- `src/main.rs` - thin entry point for the `space` desktop binary.
- `src/bin/space-bench.rs` - headless benchmark harness linked against the same
  engine, category and layout code as the UI.
- Dependencies are pinned in `Cargo.toml` and `Cargo.lock`; Geist and Geist Mono
  ship in `assets/fonts/` with their OFL license.

## Module contracts

- `src/model.rs` - frozen arena tree, category enum, scan summary/progress and
  drive types. Node ids are `u32`; `NO_NODE` is the sentinel. The tree is
  immutable after scanning and shared as `Arc<ScanOutput>`.
- `src/engine/` - the only filesystem traversal code. Classification lives in
  `classify.rs`; enumeration and cancellation in `scan.rs`; volume listing in
  `mod.rs`. No UI or WGPU types are allowed here.
- `src/preview/` - bounded, read-only preview builders for text, hex, image,
  list/archive/office, metadata, SQLite and media. No execution, no shell, no
  writes.
- `src/treemap/` - pure geometry: squarified layout, camera transform, hit
  testing and label fitting. No Iced or WGPU imports, so tests run headless.
- `src/render/` - instanced WGPU fill pipeline exposed as an Iced shader widget.
  It receives a plain draw list and emits no messages.
- `src/ui/` - the only state owner: app shell, screens, inspector, overlays,
  palette, filter view, formatting and design tokens.

## Data flow

```text
sysinfo disks -> Drives screen -> engine::spawn_scan(path, sender, cancel)
                                          |
                             ScanEvent progress / Done / Failed
                                          |
                                    UI tick poll
                                          |
                         Arc<ScanOutput> = Tree + ScanSummary
                                          |
             +----------------------------+----------------------------+
             |                            |                            |
      treemap::squarify            preview::Service              category_totals
             |                            |                            |
      render::TreemapProgram       bounded image/text          inspector composition
      + Canvas labels / hit test
```

The UI never blocks on disk. Scans run on a coordinator plus a bounded pool of
walker workers; previews run on Iced's thread pool behind four heavy-work
permits; exact category composition runs on a worker and is cached per node.
The draw scene, label pass, hover/selection and hit testing all use the same
`SceneTile` list.

## Windows walker

The fast path enumerates with `FindFirstFileExW` using `FindExInfoBasic` and
`FIND_FIRST_FILE_EX_LARGE_FETCH`, then fans directories out across workers. A
portable `std::fs`/`jwalk` path is compiled on non-Windows targets but has not
been run or verified. Symbolic links, junctions and other reparse points are
not followed. Unreadable directories increment the error count and are skipped,
never treated as fatal.

Cancellation is an `AtomicBool` checked before every directory and every entry.
The worker emits one terminal `Done` or `Failed` event after any progress
events; a cancelled scan returns `Failed("Scan cancelled")`, which the UI turns
into a toast after returning to the volume list.

## Read-only guarantees

- The engine calls directory enumeration and metadata reads only.
- Previews open files for bounded reads, and SQLite is opened with
  `SQLITE_OPEN_READ_ONLY` plus defensive flags.
- No create, write, rename, delete, move or cleanup API is linked into the
  production engine or preview code.
- "Open" and "Reveal" hand an explicit path to the OS shell only after a user
  action; a preview never executes or plays a scanned file.
- The app has no URL scheme handler and no network client.

## State model

`ui::App` owns: screen, appearance, drive list, active scan and progress
samples, immutable output, current node, selection, hover, camera, history,
filter state and bounded filter view, sort mode, map/composition mode, preview
cache state, palette state, context menu, drag state, toast and focus target.

History is a node-id vector with a cursor, not a second tree. Navigation
drilling and breadcrumbs mutate only this state. The file field is an
indeterminate stream; the UI never invents a percentage or ETA.

## Performance rules

- Node names live in one UTF-8 arena; child links are intrusive `u32` ranges.
- Per-node category arrays are not stored. Exact totals are computed on demand
  and bounded-cached; each tile is colored by its dominant direct branch.
- Layout is recomputed only when the viewport, current node, filter or camera
  changes; GPU instances are rebuilt only when the scene changes.
- The recursive filter is capped at 120,000 visits and 2,000 matches.
- Treemap folders render at most 63 real children plus one non-selectable,
  hatched aggregate tail.
- Previews are LRU-bounded (256 entries) and read only capped byte ranges.

## Platform isolation

Windows-specific code is behind `#[cfg(windows)]`: the fast walker, drive
metadata, shell open/reveal and the optional media thumbnail. Other platforms
compile against the portable walker but are not claimed as verified. Only
Windows 10/11 has been built, run, scanned and screenshotted.
