# QA — remaining physical probes (run when the space window is foreground and mine to drive)

Preconditions
- Dev app: `space/src-tauri/target/debug/space.exe` (loads vite :1420 — dev server must be up).
- Alt-activate once: ALT down/up + SetForegroundWindow (works; did at 13:51).
- Drivers: `.impeccable/click.ps1`, `hover.ps1`, `shot2.ps1` (topmost self-pin was REVERTED — app window is normal z-order; only drive when it IS the foreground window).
- Never left-click while javaw is foreground — verify with WindowFromPoint + GetForegroundWindow probe first.

Probes (window coords, capture space = 1336×899)
1. CLEAN STATE — fresh launch → drives screen; expect: no chip, no selection, no hover residue. (Previously verified 13-focused.png after fresh launch ✓.)
2. D: TIMING — click (1152,338) Scan; shots at +0.25/+0.95: explorer fully live ≤0.8s warm (verified ✓ twice). Log a cold-number if machine idle.
3. HIDE MEDIA (big-area dim sight-test at 0.16) — rail row bands (from 4x rail-crop): Media ≈ y 262–290, Documents ≈ 288–312, Code ≈ 312–336, Caches ≈ 336–358, Apps ≈ 362–384, Archives ≈ 388–408. Click (1080,277) = Media hidden → expect: big blue tiles at ~#1B2637 "legible-silent" (not void), chip "1 category hidden", struck Media row. THEN click the chip (403,873) — NEW BUTTON = unhide-all → tiles back bright. This is the deferred evidence for the 0.16 change + chip affordance.
4. DRILL-IN + PLATES MID-ZOOM — dbl-click GITHUB BACKUP tile (≈835,480): capture at +0.35s (mid-animation: previews must appear WHILE zooming, not only settled) and +1.4s (settled: plates for leaf tiles). Then topbar Back (29,53) → root.
5. SEARCH-FILTER DIM — click search bar (1050,54), SendKeys "mp4": non-matching tiles dim (same dimmed() predicate, 0.16). Esc clears.
6. C: COLD TIMING — Drives (1240,877) → click C: Scan (1152,220): shots at +0.3/+1.0/+1.7/+2.4/+3.1; record the click→explorer delta. (C: has more real content; the honest indeterminate meter should show briefly.)
7. LIGHT THEME PASS — statusbar "Light" (≈1300,877): verify palette swap (no dark residue, chips read, dim tiles legible on light field), then back to Dark.
8. FOREGROUND GUARD before EVERY click: GetForegroundWindow=space AND WindowFromPoint(cursor)=space's hwnd/webview; if javaw — STOP, close probes.

Fixed-since-shots (verify live)
- Dim alpha 0.05 → 0.16 (TreemapCanvas.writeInstances ~line 810).
- Statusbar hidden chip → button (unhide-all) + hover step (explorer.css button.sb-hid).
- Rail hover card clamp `.rail .hovercard { max-width: calc(100% - 16px); }` (name/path ellipsize in-rail).
- Finish-review wave (all PASS-with-notes fixes): keycaps now print Ctrl (not ⌘) everywhere; palette/keyboard zoom steps unified at 1.4×; index.html resolves data-theme pre-paint from `space.theme` (dark default, no foreign-neutral flash); rail hover card survives Tab into its action buttons (blurKeepOrClose); format.ts loops collapsed (formatBytes → splitBytes, redundant formatPercent branch); extOf deduped into src/lib/ext.ts.
- Release exe re-embeds the current dist on every rebuild — **OLD DOCTRINE DISPROVEN 2026-10-18 15:4x**: manual `cargo build --release` bakes `build.devUrl` (http://localhost:1420) into the context and embeds NOTHING; the exe serves the vite dev server if one is alive (unminified modules, HMR state). The `lib.rs` mtime ritual "worked" only while the 13:02 vite server was serving it. Correct procedure: `npx tauri build --no-bundle` (runs beforeBuildCommand + materializes the release context that embeds ../dist). Verify: the exe_WINDOW URL must not be localhost:1420 (check with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 + GET 127.0.0.1:9222/json/list, or simply confirm no respond to :1420).

## Round-9 fixes (user's Users-screen complaint — verify live)

Reported: at C:\Users the map read as a 64-row vertical file list (one mega-tile + listing plate), hover named the wrong item ("hovering .rustup, card says AppData"), rail Largest-files hover unresponsive, zoom "dead".

- XL listing plate cap — TreemapCanvas `updatePreviews` pass 3: a LIST/DB plate offering ≥20 rows on a tile covering ≥72% of canvas width AND height is bounded to ≤50% canvas width × ≤42% canvas height, anchored top-left. The tile's real nested treemap stays visible around it. Modest tiles (offline-vid class, 13 rows) keep their full-tile preview.
- Row cap — previewPlate `renderTable`: max 20 rendered rows per table plate (`TABLE_ROW_CAP`); the honest `… N more entries` footer carries the remainder.
- Plate-aware hitTest — `hitTest` now first scans claimed+visible plate slots and returns the plate OWNER tile index while the pointer is inside the painted plate box (slot.lx/ly + lw−8/lh−8 = the pw×ph painted box). Hover identity now matches what the pointer sees under plates — previously the pick resolved depth-1 tiles hidden beneath the listing paper.
- Rail hover card row-anchored — ExplorerRail `enterRow` anchors the card at the ROW rect (left+8, bottom+4), matching `focusRow`; the card no longer chases the pointer or straddles its own hit area.
- Drives screen rebuilt (narrative instrument): claim statement headline ("C: is 88% used" / "Every volume has headroom"), Σ context sub-line, tier words low≥80% critical≥92% printed inline + color-matched diagonal hatch on the critical fill, 10px tick-gridded ribbon, two-line per-row readout (fact + consequence), panel legend decoding the ribbon. DrivesView.tsx + drives.css rewritten whole. ⚠ Old probe coordinates for the drives screen are STALE — re-derive row/Scan positions from a fresh capture (D: row was ≈(1152,338), C: ≈(1152,220) pre-redesign).

Re-probe set at the failing state (C:\Users level):
- R1 plate bounded: giant listing ≤ ~half-map instrument, treemap visible around it. **VERIFIED 15:55** (shot 34: 12 rows + "… 59 more entries", children visible around the plate).
- R2 hover the plate rows → card names the OWNER (the folder tile), never a child hidden under the paper. **VERIFIED 15:56** (shot 35: card = SEGMENT · 148 GB · 100% of Users).
- R3 hover the visible side tiles → correct child identities. **VERIFIED** (shot 36: Downloads · 10.5 GB · 7.1%).
- R4 rail rows → card opens just below the row and HOLDS STILL; row hover fill present. **VERIFIED** (shot 37: hiberfil.sys card below its row).
- R5 wheel over map center → visible zoom; plate visibly shrinks/leaves the view. **VERIFIED** (shot 38: ZOOM 223% pill, map magnified, plate clamped at its cap; hover identity stayed honest).
- R6 dbl-click the big tile → drill plays (pick is honest through the plate mask). **DEFECT FOUND 16:00 → FIXED**: one physical double-click drilled FOUR levels (Users→SEGMENT→AppData→Local→NVIDIA, shots 40/41). Root cause: single-click already drills directories (TreemapCanvas onUp 1746) + the 2nd physical click lands after the instant in-memory rebuild and drills again + the synthesized dblclick event drills a third time (onDbl 1794). Fix: drill cooldown (500ms) armed on every directory drill; onDown + onDbl swallow the gesture tail; file dblclick-to-open unaffected. Re-verify after rebuild.
- Hide/unhide cycle (rail row click → struck row + "N category hidden" chip → chip click unhides): **VERIFIED** (shots 43/44).
- Search filter: type "mp4" → honest "0 of 21 entries match" pill + full-map 0.16 dim. **VERIFIED** (shot 45). **DEFECT FOUND → FIXED**: Esc only blurred the field, leaving the query + dim stranded; now Esc in the filter clears the query and blurs (ExplorerView onKey).
- Light theme: full pass, pastel tiles, light panels, "Dark" toggle offered. **VERIFIED** (shot 47).
