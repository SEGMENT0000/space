# Benchmarks

All numbers below were measured on the real release binary and a real disk. No
value is simulated, smoothed or copied from an earlier session.

## Machine and conditions

| | |
| --- | --- |
| CPU | 12th Gen Intel Core i5-12400F |
| RAM | 24 GiB |
| OS | Windows 10 Pro N, 19045 |
| Toolchain | rustc/cargo 1.99.1, edition 2024 |
| Build | `cargo build --release`, opt-level 3, thin LTO, 1 codegen unit, stripped |
| Binaries | `target\release\space.exe`, `target\release\space-bench.exe` |
| Workspace | `C:\Users\SEGMENT\Downloads\space`, NTFS |
| Date | 2026-10-26 |

`cold` means the first run in the benchmark process. The harness does **not**
purge the Windows file cache or stop background applications, so absolute scan
times vary with cache state and disk activity. Category/preview/layout figures
are single-process medians or repeated samples as noted. The machine is the
developer workstation, not an isolated benchmark host.

## Full volume scan: `C:\`

Command:

```powershell
target\release\space-bench.exe "C:\" --runs 5
```

Latest five-pass session. The cold column is run 1; the warm column is run 2
from the same session. Files were present across passes:

| Metric | Cold | Warm |
| --- | ---: | ---: |
| Wall time | 2,986 ms | 2,825 ms |
| Engine index timer | 2,976 ms | 2,814 ms |
| Files | 945,180 | 945,180 |
| Directories | 179,335 | 179,335 |
| Bytes | 178.44 GiB | 178.44 GiB |
| Skipped errors | 301 | 301 |
| Throughput | 316.5k files/s | 334.6k files/s |
| Peak working set | 94.92 MiB | 95.76 MiB |
| Largest category | System, 24.8% | System, 24.8% |

Runs 2-5 measured 2,692-2,825 ms and 95.8-117.3 MiB peak working set, so later
warm runs were slightly faster but allocated more. An earlier pair in the same
review session measured 4,371 / 2,883 ms after a release rebuild, and older
passes ranged from 2.68 to 4.03 s. This is real workstation variance on a live
disk, not a guaranteed ceiling.

Category composition for the largest nested subtree, `C:\Users`, median of five
runs:

| | Cold | Cached repeat |
| --- | ---: | ---: |
| Eight exact category buckets | 5.91 ms | 1 µs |

The root composition is computed during the scan and primed into the cache; the
nested figure is the meaningful cold case.

## Project scan: repository

```powershell
target\release\space-bench.exe "C:\Users\SEGMENT\Downloads\space" --runs 5
```

| Metric | Cold | Warm |
| --- | ---: | ---: |
| Wall time | 28 ms | 26 ms |
| Files | 13,101 | 13,101 |
| Directories | 1,807 | 1,807 |
| Bytes | 10.95 GiB | 10.95 GiB |
| Errors | 0 | 0 |
| Throughput | 467.9k files/s | 514.0k files/s |
| Peak working set | 10.15 MiB | 10.15 MiB |
| Category composition, `target` (cold / cached, median) | 105 µs / 1 µs | 105 µs / 1 µs |
| Recursive scene layout, median | 94 µs | 94 µs |
| Tiles produced | 444 | 444 |

## Layout

The scene benchmark mirrors the visible-scene limits in `src/ui/canvas.rs`:
four levels, at most 64 children per folder, 2 px gutters, 4,000-tile ceiling,
1200x700 viewport. Values are medians of five runs in the final pass, with the
observed range.

| Data set | Tiles | Median layout |
| --- | ---: | ---: |
| `C:\` root | 3,645 | 4.44 ms (4.20-4.78 ms) |
| Repository root | 444 | 94 µs (93-96 µs) |

This is CPU squarify time only. It excludes label shaping, Canvas rasterization
and GPU submission, so it is a lower bound for a full repaint. Layout is only
rebuilt when the viewport, current node, filter or camera changes, not on every
event that leaves the scene untouched.

## Previews

Each file is a real fixture with real bytes. The preview service is fresh per
file, so the first call is cold and the warm column is the median of five cache
hits in the same service.

| Family | File | Cold | Cached |
| --- | --- | ---: | ---: |
| TEXT | `notes.md` (438 B) | 1 ms | 22 µs |
| HEX | `sample.bin` (4 KiB) | 1 ms | 49 µs |
| IMAGE | `photo.jpg` (78 KiB) | 13 ms | 23 µs |
| LIST | `bundle.zip` (765 B) | 1 ms | 22 µs |
| DB | `metrics.db` (8 KiB SQLite) | 1 ms | 22 µs |
| MEDIA | `tone.wav` (16 KiB) | 1 ms | 22 µs |
| META | `Geist-Regular.ttf` (126 KiB) | 1 ms | 24 µs |

On the full `C:\` scan the largest file (`pagefile.sys`, 8.50 GiB) degrades to
a META preview in 1 ms; the cached repeat is 46-64 µs. Preview work is bounded,
so file size does not translate into an unbounded read.

## Cancellation

```powershell
target\release\space-bench.exe --cancel "C:\" 25 --json
target\release\space-bench.exe --cancel "C:\" 250 --json
```

| Cancel request | Latency to terminal event | Files observed | Dirs observed |
| --- | ---: | ---: | ---: |
| 25 ms after start | 1.54 ms | 0 (progress interval is 125 ms) | 0 |
| 250 ms after start | 7.20 ms | 117,002 | 48,572 |

Both runs ended with `message: "Scan cancelled"`. The UI path was also driven
with the release binary: clicking **Cancel scan** during a live `C:\` traversal
returned to the volume list with a "Scan cancelled" toast.

## Frame pacing

Instrumented release binary with `SPACE_FRAME_LOG=1`. Samples are intervals
between view passes, which Iced emits per presented frame. Five 120-frame
windows were recorded. The first includes idle 220 ms subscription gaps; under
sustained wheel input the later clean windows measured:

| Window | Median | p95 | Max |
| --- | ---: | ---: | ---: |
| 120 frames under sustained wheel input | 15.56 ms | 16.14 ms | 19.91 ms |

Iced's shader widget does not expose a GPU timestamp query, so this is frame
pacing, not an isolated GPU kernel time. The 15.56 ms median is consistent with
a 60 Hz present interval.

## Navigation latency

Navigation is event-driven: a drill, history move, refit or zoom updates state
and schedules the next view; there is no blocking disk access in that path.
With the measured 4.44 ms root layout and 15.56 ms median frame pacing, a
navigation repaint is bounded by roughly one frame plus layout/scene work on
this machine. Preview latency is the cold/cached table above. These are derived
from measured components, not a separate stopwatch around a UI gesture.

## Reproduce

```powershell
cargo build --release
target\release\space-bench.exe "C:\" --runs 5
target\release\space-bench.exe "C:\Users\SEGMENT\Downloads\space" --runs 5
target\release\space-bench.exe --preview "C:\path\to\file" --json
target\release\space-bench.exe --cancel "C:\" 25 --json
$env:SPACE_FRAME_LOG = "1"; target\release\space.exe
```

The screenshot review is recorded in `docs/QA.md`.
