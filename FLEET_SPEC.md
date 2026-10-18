# space — Fleet Implementation Spec (v1)

Coordination document for the multi-agent build wave. Recon sources: backend report
(scanner.rs/map), frontend report (ipc.ts/ExplorerView map), classification report
(classify.rs), plus treemap + visual reports pending (sections E/F marked TBD).

## 0. Mission (user order, verbatim intent)

1. **Kill every runtime error** — the `dimmed()` TypeError overlay must never appear.
2. **Scan faster** — "scanning takes too long"; native-side optimization, no JS scan work.
3. **Redesign the map interior** — deep zoom must always show real structure (today a
   single-child folder renders as one flat gray tile — "so messy"). Interior reads
   organized at every zoom level.
4. **File previews at deep zoom** — zooming into a file shows its CONTENT inside the
   tile: text/code as formatted text, images as preview, binary as hexdump, archives as
   listings, etc., for ALL31 file types (mapping in §A).
5. **Performance target** — big codebases must load and pan at interactive frame rates;
   preview loading must be lazy, capped, cached, cancellable. Renderer stays allocation-
   free per frame.
6. Keep: right sidebar rail (user's favorite), drives screen (liked), sentence-case
   label voice, fill invariant, contrast floor ≥4.5:1, read-only-by-construction,
   8-category palette (do NOT expand categories — preview families are orthogonal).

## Ground rules (every agent)

- **File ownership is strict**: edit ONLY your listed files. Never touch frozen files.
- **No builds** unless your brief says "integrator". Plain CSS modules (one class in
  exactly one file). lucide-react + existing deps only unless your brief adds crates.
- Read DESIGN.md + PRODUCT.md first. Banned: gradients, glassmorphism, glows, blink
  dots, card grids, Inter/Roboto/Arial/Open Sans/Helvetica, uppercase-mono on labels
  (mono is for figures/paths/keycaps only), letterboxed treemap, delete affordances.
- WHY comments on every non-obvious decision. Keep JSX prop contracts byte-stable
  unless your brief rewrites that contract.
- Verification = re-read files + targeted greps + contrast math. The integrator builds
  (`npm run build` = tsc && vite) and runs `impeccable detect`.

---

## §A. Preview families + type mapping (LOCKED — recon report E4)

Families: **TEXT** (monospace content, cap ~200 lines / 64 KB), **IMAGE** (raster
thumbnail ≤256px longest edge, decode cap 8 MP), **SVG** (sanitized vector, cap 64 KB),
**HEX** (hexdump of first 4 KB), **LIST** (archive/TOC/DB-table listing, cap 200 rows),
**META** (structured key/value readout: signature hex, codec, timestamps, counts),
**DB** (sqlite: table list + schema + row sample), **NONE** (META only).

| Type | Extensions (examples) | Cat (8) | Family |
|---|---|---|---|
| Text Files | txt md rtf log(out) | Documents/Other | TEXT |
| Source Code | rs ts tsx js py java c cpp go sh ps1 bat sql vue… | Code | TEXT |
| Binary | bin dat dmp pak (no-ext) | Other | HEX |
| Image | jpg jpeg png gif webp bmp tiff ico heic | Media | IMAGE |
| Audio | mp3 wav flac aac ogg m4a | Media | META (no decode) |
| Video | mp4 mkv avi mov webm m4v | Media | META (no decode) |
| Database | db sqlite sqlite3 → DB; mdb accdb → HEX | Other | DB / HEX |
| Structured Data | json yaml yml toml xml → Code; csv → Documents | Code/Docs | TEXT |
| Compressed | gz tgz bz2 xz zst br lz4 | Archives | META (method+usize) / LIST if multi-member |
| Archive | zip rar 7z tar cab | Archives | LIST (central dir only) |
| Document | pdf mobi → META; docx odt → TEXT (zip/xml); epub → LIST | Documents | TEXT/LIST/META |
| Presentation | pptx odp → LIST/META (slide count) | Documents | META/LIST |
| Spreadsheet | xlsx ods → LIST (sheet names); csv → TEXT rows | Documents | LIST/TEXT |
| Executable | exe dll msi so dylib deb rpm apk jar | Apps | META (PE/ELF: arch, sections) + HEX head |
| System | sys drv + `C:\Windows` rule | System | HEX / META |
| Config | ini cfg conf env properties reg plist | Code (add exts) | TEXT |
| Log | log tlg | Other | TEXT (head) |
| Disk Image | iso img vhd vhdx wim → LIST(ISO PVD)/META; dmg → move to Archives | Archives | LIST/META |
| Firmware | fw rom efi fd | System | HEX+META |
| Font | ttf otf woff woff2 ttc | Other | META (family/tables) |
| 3D Model | obj stl ply gltf glb (ascii) → TEXT; fbx blend → HEX | Other | TEXT/HEX |
| CAD | dxf step iges (ascii) → TEXT; dwg → HEX+META | Other | TEXT/HEX+META |
| Vector | svg → SVG family (SANITIZE); eps → TEXT; ai → META | Media | SVG/TEXT/META |
| Scientific | h5 hdf5 nc fits dcm npy parquet mat | Other | META only |
| Backup | bak backup bkp → META; tar.* → LIST | Archives/Other | LIST/META |
| Temporary | tmp temp part crdownload → **new rule → Caches cat** | Caches | HEX |
| Encrypted | enc age gpg pgp vault | Other | NONE (META) |
| Cert/Key | pem crt cer asc key pub → TEXT; der pfx p12 jks → META | Other | TEXT/META |
| Email | eml mbox → TEXT; msg pst ost → META | Documents | TEXT/META |
| Subtitle | srt ass ssa vtt → TEXT; idx sub → HEX | Documents | TEXT/HEX |

**Classify-table changes allowed** (still 8 cats): add `.log .ttf .otf .eml .srt .vtt
.ini .cfg .db .sqlite .fw .rom .tmp …` to the right buckets; move `.sys .drv` from Apps
→ System, `.dmg` from Apps → Archives. Own file: `classify.rs` (Rust agent only).

**Renderer contract note**: the treemap is path-dumb — previews are keyed by
**entryId**, never path (paths resolve only in ExplorerView).

## §B. Rust preview commands (LOCKED — backend report)

- New module `src-tauri/src/preview.rs` + `mod preview;` in `lib.rs:6-9` +
  registration in `generate_handler!` `lib.rs:19-27`.
- ONE command: `get_preview(path: String) -> Result<PreviewPayload, String>` — sniff
  extension inside, return tagged union. DTOs in `model.rs` (frozen contract, mirror in
  `src/lib/types.ts`).
- `PreviewPayload` shape (serde, camelCase):
  `{ family, text?: {lines: string[], truncated: bool, totalLines?: number},
     image?: {dataUrl, w, h, mime}, hex?: {rows: {off, bytes, ascii}[],
     base: number, total: u64}, list?: {entries: {name, size?, kind}[], truncated},
     meta?: {pairs: {k, v}[]}, db?: {tables: {name, cols, rows?}[], truncated} }`
- **Async commands** (`async fn` on tauri's runtime) + a semaphore (≤4 concurrent) so
  heavy reads never block IPC. Size caps enforced BEFORE decode. LRU cache keyed
  `(path, mtime, len)` managed via `.manage()`; hand-rolled LRU or `mru` crate ok.
- New crates allowed: `image` (decode+resize), `zip` (central dir), `rusqlite`
  (bundled, `mode=ro` URI only), `base64`. Path safety: canonicalize, reject
  non-regular files, cap `file len` before opening (e.g. TEXT head 64 KB read, never
  whole file).
- Demo fallback: `DemoState` in `src/lib/demo.ts` synthesizes plausible per-family
  payloads (fake code/text lines, META pairs) so the browser preview shows the system.

## §C. Scan performance work (LOCKED — backend report, ranked)

1. **Immutable-snapshot queries**: after publish, `ScanData` is immutable — switch
   `Engine.data` to `RwLock<Arc<ScanData>>`-style snapshot so `get_*` hold a read
   guard only to clone the Arc (today every query holds the global mutex through its
   whole sort — `scanner.rs:249,346,378`).
2. **classify.rs**: compute per-DIRECTORY flags (in-cache-dir? under-C:\Windows?)
   once per directory in `walk_callback`, not per file (`classify.rs:70,104` do full-
   path lowercase per file).
3. **Sort without allocations**: extract lowercase sort keys ONCE per name (or store
   `name_lower` on Node / sort index pairs) — `compare_by_name` `scanner.rs:970-973`
   allocates 2 Strings per comparison today.
4. **Cache depth≥2 sub-entries**: `top_children` `scanner.rs:986-995` re-clones +
   re-sorts every dir's children on EVERY `get_children(depth≥2)` call → extend the
   sorted LRU (key: nodeId+sort).
5. **finish_scan**: replace `HashMap<u32,[u64;8]>` with a direct `Vec<[u64;8]>` indexed
   by node id; **persist per-dir `[u64;8]` into ScanData** so `get_category_totals` is
   O(1) (today it DFS-walks whole subtrees on cache miss `scanner.rs:904-920`).
6. **Reliability**: release profile is `panic="abort"` (`Cargo.toml:37`) which defeats
   `catch_unwind` (`scanner.rs:209`) — decision: switch release to unwind (keep the
   guard) OR remove the guard + make walker panic-free; recommend unwind.
7. Micro: `SeqCst`→`Relaxed` atomics; `current` path string update only for
   depth≤3 dirs; verify on Windows whether `entry.metadata()` is a cached
   FindFirstFile result (likely cheap) before optimizing stats.
- **Do NOT**: introduce SQLite persistence, rewrite the walker, or change progress
  cadence (120 ms/~100 B is fine).

## §D. Frontend preview integration (LOCKED — frontend report)

- `ipc.ts`: add `getPreview(path)` following the `getChildren` template (demo branch
  first). ALL IPC touchpoints stay in `ipc.ts`.
- New `src/lib/preview.ts`: module-level LRU `Map<entryId, PreviewPayload>` + cap
  (~300 entries, mcache pattern) + in-flight dedupe set. NO react-query.
- **Owner: ExplorerView** — renderer stays data-dumb. New optional prop on
  TreemapCanvas: `previews: ReadonlyMap<number, PreviewPayload>` +
  `onNeedPreview?: (entryId: number) => void` (renderer calls it when a FILE tile
  crosses the zoom threshold; ExplorerView resolves path via `findEntry`, fetches,
  stores by entryId). Contract change must be coordinated: TreemapCanvas agent +
  ExplorerView agent BOTH receive this exact prop shape.
- **Cancellation**: navToken/`requestGen` ref-epoch — bump on every `load()`; results
  from an older gen are dropped (there is no abort in Tauri invoke). Mirror App.tsx's
  `let dead=false` cleanup pattern.
- `load()` must also gain the token + a `.catch` (today: bare `.then`, no catch, no
  token — `ExplorerView.tsx:125-150`).
- Add caching for `getChildren` keyed by nodeId (module-level Map, capped) so
  back/forward/breadcrumbs don't refire the wire (report: zero cache exists today).
- Error UX: no raw error overlay ever reaches the user — App-level React
  ErrorBoundary + a global `window.onerror` guard that surfaces a quiet statusbar
  toast instead of a crash (dev overlay may still show for us, but the app must keep
  rendering).

## §E. Map interior redesign + LOD nesting + crash fix — TBD (awaiting E3)

Placeholder decisions to be finalized from the treemap recon:
- Fix `dimmed()` `refs[i]` TypeError at root cause (degenerate frames: single-child
  folders, 129-byte drives) + guards everywhere `refs[i]`/`tiles[i]` are assumed aligned.
- Nested interior: subdivide folder tiles whose screen size ≥ threshold into their
  children (progressive detail) so no view ever shows one flat tile; stroke/tint
  hierarchy for depth-2-in-tile; labels adapt (depth-aware priority passes).
- In-tile preview compositor seam (2D overlay vs GL texture upload) + per-tile screen
  rect availability per frame.
- Preview draw must be CACHED (offscreen canvas / texture per entryId, invalidated on
  viewport settle) — never re-measure text per frame.

## §F. Visual language for previews + interior — TBD (awaiting E5)

Placeholder: ins/-derived specs for TEXT/HEX, IMAGE, LIST, META tile interiors;
nested-tile differentiation rules; hard DON'Ts.

## §G. Wave-1 decisions (from treemap recon E3 — supersedes placeholders)

**Crash (landed by integrator, do not re-fix):** stale raw-index state after frame
rebuild — `hover.current` survives `ensureFrame` rebuilds, `drawLabels:504` calls
`drawOne(hover.current)` unbounded → `dimmed` → `refs[i]` undefined → TypeError.
Fix = bounds-check at the call site + guard in `dimmed` + reset hover/drag-idx on
rebuild. Root cause is the class of RAW FRAME INDICES (`hover`, `drag.idx`) — any
emission change makes them fragile; validate per frame.

**Interior/redesign answer:** richer INTERIOR = previews, not deeper d3 emission.
The data is 2-level (`getChildren(depth=2)` → `entries[].sub[]`), so full-depth LOD
emission is a no-op today → **LOD agent #5 dropped this wave**. The single-flat-tile
views (E: lone folder, W: steamapps) are solved by the preview system:
- **Folder tiles at deep zoom → LIST preview** (its contents as an instrument list).
  Served by `get_preview(path)` (Rust does read_dir head, size-sorted, capped) — same
  family shape as archive listings. One pipeline covers both.
- **File tiles at deep zoom → family preview** per §A.

**Preview compositor (locked — see `INS_BRIEF.md` for the full visual spec):**
DOM **plates**, not canvas painting. A pooled layer of ~24 `.tm-prev` nodes sits as a
`pointer-events:none` sibling of `.map-labels`; the draw loop positions them
imperatively (transform writes — the `updateInstruments` pattern), fills them via the
new module `src/components/treemap/previewPlate.ts`, and suppresses the floating map
label for any plated tile. Styles live in **new** `styles/preview.css`, imported from
`previewPlate.ts` (index.css untouched). Qualification thresholds, per-family layout,
and the depth tint/stroke/label ladder are specified in `INS_BRIEF.md` (§2, §3) —
that file is binding for agents #6/#7/#8.

**Cross-file prop contract (renderer ⇄ explorer):**
`TreemapCanvasProps += { previews?: ReadonlyMap<number, PreviewPayload>;
onNeedPreview?: (entryId: number) => void }`. Renderer decides *when* (tile screen
size ≥ ~360×200 px and zoom > fit); ExplorerView resolves path (`findEntry`), loads
via `preview.ts`, stores by entryId, guards with nav-generation token. Extra props
passed before the renderer knows them are runtime-safe (React ignores unknown props);
`tsc` completes only at integration.

**`src/lib/preview.ts` API (owner: preview-cache agent), exact:**
`loadPreview(entryId: number, path: string): Promise<PreviewPayload | null>` — ONE
entry point for files AND folders: it calls `ipc.getPreview(path)` (Rust sniffs
dir-vs-file and the family itself), caches by entryId, dedupes in-flight requests,
resolves `null` on any error (never throws). Plus `peekPreview(entryId)` and
`clearPreviews()`. Internal: LRU cap 300.

## Phase plan / roster

- **Recon (5, ALL DONE)**: backend, frontend, classification, treemap, visual →
  `FLEET_SPEC.md` + `INS_BRIEF.md`.
- **Wave 0: crash fix — LANDED by integrator** (call-site bounds check, `dimmed`
  guard, hover/drag-index invalidation on rebuild).
- **Wave 1 (8, parallel, disjoint files):**
  1. Rust scan-perf (`scanner.rs`, `classify.rs` ONLY — no Cargo.toml/lib.rs/model.rs) — §C
  2. Rust preview (`preview.rs` new, `lib.rs`, `model.rs`, `Cargo.toml`) — §B
  3. TS contract (`ipc.ts`, `types.ts`, `demo.ts`) — §B/§D
  4. TS preview cache (`lib/preview.ts` new) — API above
  6. Renderer (`TreemapCanvas.tsx`, `gl.ts`, `layout.ts`) — §G + `INS_BRIEF.md` (launches LAST)
  7. Preview plates (`components/treemap/previewPlate.ts` new + `styles/preview.css` new) — `INS_BRIEF.md` §2
  8. Explorer integration (`ExplorerView.tsx`, `explorer.css`) — §D
  9. Error resilience (`App.tsx`, `components/ErrorBoundary.tsx` new, `styles/errors.css` new) — §D
  - (old #5 LOD dropped — no deeper data; old #10 dropped — no readout changes)
- **Wave 2 (~4-5)**: verification (build + detector + headless ?theme/?screen shots +
  contrast sweep + perf smoke on a real big drive), DESIGN.md/PRODUCT.md update,
  polish pass from findings.
- **Integrator (lead)**: owns `FLEET_SPEC.md`, `INS_BRIEF.md`, `DESIGN.md`,
  `PRODUCT.md`, `colors.ts`, builds, screenshots, wave sequencing, cross-agent fixes.

## §H. Integration checklist (lead — waves 1+2 landed; final state)

- [x] **§C6 panic profile**: release `panic = "abort"` → `"unwind"` in
  `Cargo.toml` (keeps `scanner.rs` catch_unwind/fatal() meaningful in release).
  VERIFIED: `Cargo.toml` release profile has `panic = "unwind"`; cargo check green.
- [ ] **`list.kind`**: optional `list.kind: "dir"|"archive"|"generic"` in
  `model.rs` + mirror in `types.ts` — still not landed and NOT blocking:
  plates treat an absent kind as the generic preset (`1fr 7ch`), which is the
  correct fallback.
- [x] Verify `mod preview;` + `get_preview` in `generate_handler!` (lib.rs).
  VERIFIED: `mod preview;` present, handler entry + command wired; cargo check green.
- [x] Stale-cache race (query racing `start_scan` cache reset): **fixed at the
  root** — the real hazard was wider than noted: `finish_scan` swapped `data`
  WITHOUT clearing `sorted_cache`, so old-tree entries survived a publish and
  were served as valid (node ids are dense and reused across scans, so staleness
  is indistinguishable from validity; out-of-range stale ids could also reach
  `build_entry`'s unchecked index). Fix in `scanner.rs`: `generation: AtomicU64`
  bumped by `invalidate_sorted_cache()` (bump, then clear under the cache lock)
  at all three identity changes — scan start, cancel `data = None`, publish —
  and `snapshot()` reads the generation (Acquire) BEFORE the snapshot while
  `cached_sorted` re-checks it INSIDE the cache lock (`fresh` gates both the
  read and the insert; !fresh still answers that one request from the caller's
  own consistent snapshot). cargo check green.
- [ ] jwalk finding: `entry.metadata()` = real `symlink_metadata` syscall per entry
  (not served from cached find-data) — phase-2 research only, not blocking.
- [x] **Launch #8 (explorer)** + **#6 (renderer)**: both landed. #6's constraints
  preserved and grep-verified: the 4 crash-fix markers (`if (!ref) return false;`,
  `hover.current < f.refs.length`, `drag.current.idx = -1`,
  `d.idx < f.refs.length`); rAF try/catch → `space:error` → quiet toast (no
  overlay spam, loop stops on error); preview qualification per `INS_BRIEF.md`
  §2 thresholds + `.map-previews` layer (now `overflow:hidden`-clipped to the
  map so plates can never paint over rail/statusbar) + plate pool + label
  suppression + fallback plate; depth tint via `tintFill`/`inkFor` in
  `colors.ts`; gl.ts `aDepthF` stroke channel; `previewPlate.ts` imported.
- [x] Wave 2 verification: `npm run build` (tsc+vite) green; `impeccable detect`
  `[]` on all 18 files; headless shots (dark/light, drives, hex/list plates)
  verified; contrast sweep on plate families done. Vite served-content checks
  must use transform-proof markers (type annotations/`//` comments/CSS import
  paths do not survive transform — caused a false stale diagnosis once).
- [x] **Landed-payload reconciliation**: applied at integration —
  `text.content` (one string, split at render) + `encoding`; `hex.rows[] =
  { off, bytes }` only (ascii derived in the plate; 8→16 regroup is display-
  only); `db.tables[].cols: string[]`; `list.entries[].size` non-optional +
  per-entry `kind`, top-level `list.kind` optional → absent = generic preset.
  Demo payloads match the landed shape.
- [x] Rust changes take effect via tauri dev auto-rebuild on src-tauri writes —
  observed working (mid-wave rebuilds happened as expected).

## §I. Queued agent prompts (launch verbatim on dependency completion)

### #8 explorer — launch when contract+cache (ses_eb5f2f16e…) completes

> You are the ExplorerView integration agent for `space` (Tauri v2 + React 19 disk
> analyzer). Dir: C:\Users\SEGMENT\Downloads\new.
> READ FIRST: `FLEET_SPEC.md` §D + §G (prop contract, preview flow) + Ground Rules;
> `INS_BRIEF.md` §2.0; `space/src/lib/preview.ts` (exports: loadPreview/peekPreview/
> clearPreviews — exists by launch); `space/src/lib/types.ts` (PreviewPayload);
> `space/src/lib/ipc.ts` (getChildren now caches — call sites unchanged).
> YOU EXACTLY OWN: `space/src/components/ExplorerView.tsx`, `space/src/styles/explorer.css`
> (only if genuinely needed — likely minimal). NOT yours: TreemapCanvas/gl/layout,
> ipc/types/demo/preview, rail/statusbar, App.tsx.
> TASKS: (1) Preview state keyed entryId → PreviewPayload (ref-backed Map + version
> for reactivity); pass `previews={…}` and `onNeedPreview={…}` to <TreemapCanvas> —
> those props land with the renderer agent in this same wave (React ignores unknown
> props at runtime; tsc completes at integration). (2) onNeedPreview(entryId): guard
> with the nav generation token, resolve path via existing findEntry logic, await
> loadPreview(entryId, path), drop result if generation moved on, else set into Map.
> Null → nothing (renderer shows its fallback). (3) Nav generation token covering ALL
> async paths: bump at every load()/nav (double-click, back/forward, crumb, search);
> current `load()` is bare `.then` with no catch/no token (~lines 125-150) — add gen
> capture + stale-drop + .catch (no silent unhandled rejections). (4) clearPreviews()
> on unmount / new scan data (cleanup effect). (5) Keep every existing behavior
> byte-stable: history, crumbs, search, category toggles, rail props, statusbar,
> selection emit/receive, tooltip, demo branches — grep-prove nothing removed.
> VERIFY: full re-read; greps for old + new symbols; `npx tsc --noEmit` ignoring
> other files' errors (yours must be zero). Report: preview wiring, generation-token
> coverage list, preserved-behavior evidence.

### #6 renderer — launch when plates (ses_eb5f2f163…) completes

> You are the treemap renderer agent for `space` (WebGL2 treemap + 2D label overlay).
> Dir: C:\Users\SEGMENT\Downloads\new.
> READ FIRST: `FLEET_SPEC.md` §G + §H (your backbone), `INS_BRIEF.md` §2.0/§2.5/§3,
> `space/src/components/treemap/previewPlate.ts` (exports you consume — exists by
> launch), `DESIGN.md`, `PRODUCT.md` (fill invariant, no label overlap).
> YOU EXACTLY OWN: `TreemapCanvas.tsx`, `gl.ts`, `lib/treemap/layout.ts`. Nothing
> else. NOT colors.ts (`tintFill`, `inkFor` exports ready — import them), NOT
> previewPlate.ts/preview.css (consume only), NOT treemap/types.ts unless strictly
> required (report if so).
> TASKS:
> 1. PRESERVE the 4 crash-fix markers (grep before finishing): `if (!ref) return
>    false;`, `hover.current < f.refs.length`, `drag.current.idx = -1`,
>    `d.idx < f.refs.length`. Never deref raw indices unbounded.
> 2. PREVIEW INTEGRATION (INS_BRIEF §2): static `.map-previews` layer div (class from
>    preview.css) as sibling of the label canvas, pointer-events:none; pool ~24 plates
>    via createPlate(), managed imperatively (React never renders plate content);
>    qualify per frame from screenRectOf against the §2.0 threshold table; payload
>    absent + qualifying → `onNeedPreview(entryId)` (once per entryId per nav gen,
>    tracked Set cleared on rebuild); props += exactly
>    `{ previews?: ReadonlyMap<number, PreviewPayload>; onNeedPreview?: (entryId: number) => void }`
>    (both optional); position/hide plates each frame (4px inset per spec, CSS vars
>    --tile-w/--tile-h); fillPlate once per (entryId, family, truncated) change (key
>    via data attribute); null payload at/above readable threshold →
>    fillFallbackPlate(reason); ACTIVE-PLATED tiles are removed from the label
>    `order` (the plate head is the label) — collision/ellipses rules unchanged for
>    the rest.
> 3. DEPTH LADDER (INS_BRIEF §3, depth 0/1 only): layout.ts resolves colors through
>    `tintFill(fill, depth, theme)` then `inkFor(tinted)` for label ink (never base
>    fill); geometry/paddingInner/fill-invariant bodies UNCHANGED (color-only).
>    gl.ts: activate the unused `aDepthF` slot for the stroke ladder (depth 0 keeps
>    today's uStroke; depth 1 ~0.6× alpha) — nothing heavier than existing.
> 4. ROBUSTNESS: try/catch around the rAF tick/draw body → console.error once +
>    `window.dispatchEvent(new CustomEvent("space:error", { detail: msg }))` (App
>    bridges to the global toast at integration) + stop the loop. Never overlay spam,
>    never silent loop death.
> 5. PERF: per-frame cost = O(active plates) style writes only; no per-frame node
>    creation; Map.get for payloads; no new allocations in hot loops.
> VERIFY: full re-read of 3 files; grep the 4 markers + every import resolves to a
> real export; confirm files-untouched list. No builds. Report: preview lifecycle,
> threshold implementation, ladder changes, deferred items with reasons.
