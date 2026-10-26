# space

A read-only disk space analyzer for Windows. Native Rust, Iced 0.14 and WGPU;
no web runtime, no telemetry, no cleanup actions.

## Run

```powershell
cargo run --release
```

Open a specific folder or volume directly:

```powershell
cargo run --release -- "C:\"
$env:SPACE_SCAN = "D:\Projects"; cargo run --release
```

Review/deep-link equivalents (no URL handler is required):

- `SPACE_SCAN=<path>` opens straight into a scan.
- `SPACE_THEME=light|dark` forces an appearance.
- `SPACE_SCREEN=drives|scan|explorer` selects the starting screen; `scan` shows
  the idle scan instrument when no target is supplied.
- `SPACE_SHOT_DIR=<folder>` enables the review harness: `F12` writes a PNG of
  the client area, including the WGPU treemap.

## Keys

| Key | Action |
| --- | --- |
| `Backspace` | parent directory |
| `Alt+Left` / `Alt+Right` | history |
| `+` / `-` | zoom treemap |
| `Ctrl+0` | refit |
| `Ctrl+F` or `/` | filter |
| `Ctrl+K` | command palette |
| `Enter` | open file / enter folder |
| `Ctrl+C` | copy path |
| `Esc` | close overlay / clear filter / clear selection |

## Build and test

```powershell
cargo build --release
cargo test
cargo run --release --bin space-bench -- "C:\" --json
```

The benchmark harness records real elapsed time, files/s, peak working set,
exact category totals, category-composition and root-layout timings, and a cold
and cached preview. See `docs/BENCHMARKS.md` for machine details and measured
results. `docs/QA.md` records the reviewed states and screenshots.

## Guarantees

`space` never deletes, moves, renames, cleans or writes user files. The engine
only enumerates directories and reads metadata; previews only read bounded
ranges of file bytes and never execute them. There is no cleanup button and no
write path linked into the production engine.

## Layout

- `src/engine/` — read-only walker, classification, volume listing.
- `src/preview/` — bounded safe previews (text, hex, image, list, meta, db, media).
- `src/treemap/` — pure squarify/camera/hit-test/label geometry.
- `src/render/` — WGPU draw path.
- `src/ui/` — native shell, screens, inspector, overlays.
- `DESIGN.md` — visual specification and theme tokens.
- `PRODUCT.md` — product intent, scope and constraints.
- `docs/` — architecture, contracts, QA and benchmark records.

Licensed MIT. Geist and Geist Mono are licensed under the SIL Open Font
License; see `assets/fonts/OFL.txt`.
