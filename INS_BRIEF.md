# INS/ Design Brief — Treemap Interior & In-Tile Preview Plates

Binding visual contract for the preview/interior wave. Distilled from the `ins/` set +
DESIGN.md + PRODUCT.md by the visual-research agent. Implementation agents: this file
+ FLEET_SPEC.md are your specs.

## INTEGRATOR NOTES (amendments, binding)

1. **Plates are `pointer-events: none`.** Map gestures (left-drag pan = primary
   gesture) always win. `.tm-list__row:hover` styles stay in CSS but are inert in v1.
2. **SVG files preview as TEXT** (sanitized source lines) this wave — no vector
   rasterization in the webview.
3. **Uppercase rule:** DESIGN.md:177 wins over the ins/ look — sentence-case Geist for
   all labels; uppercase mono ONLY for inherently-upper data (hex bytes, signatures
   like `ELF`/`MZ`, extensions, units, mounts).
4. **Nesting channels apply to the existing 2-level frames only** (depth 0/1); the
   data contract is `getChildren(depth=2)` → `entries[].sub[]`. No deeper emission
   this wave.

---

## 0. Source inventory

| File | What it contributes |
|---|---|
| `ins/HUy1RLCaAAELx_R.jpg` | Dashed annotation chips with leader dashes at the annotated feature; `[1][2][3]` ghost numerals; hatched region + label; hairline cross-grid; tiny kicker over display type; violet only on CTAs |
| `ins/HUy1RHNbUAExsji.jpg` | KPI cells sharing one panel row divided by vertical hairlines (not cards): quiet label → huge tabular figure → footer delta. Dot-matrix beds; hairline callout chip with 3 real numbers; row with dot-fill bar + right %; hairline-outline pills |
| `ins/HUy1RHNbUAAaq4z.jpg` | Closest to target (LIGHT). Instrument row split by 1px hairlines; title strips with right-aligned meta; `TARGET INSPECTOR` key/value readout; `TRANSIT QUEUE` big tabular figures with tiny unit-caps above; compass `011°T` on a leader tick; `+` registration crosses; one row tinted accent for selection |
| `ins/HUn1VnhboAA6n2f.jpg` | Left name / center cell-strip / right result column; `reset` as 1px tick inside strip with mono label; hatched = locked; dotted = no forecast; 3-up stat readouts (key above, mono value below); bars with values printed ON bars; bottom keyed legend; muted 11px method note |
| `ins/HUs4ltwbQAA4ym7.jpg` | Cell-grid density; 4-step flat grey ramp keyed `under 25, 50, 75, 100%` (step ramp, never gradient); 45° red hatch = none free, always keyed; dashed = no reading; annotation text inside hatch; vertical hairline splits; selected row = one fill step |
| `ins/HUy1RHNboAAt3VM.jpg` | Registration `+` with bracket codes; stacked spec block; rotated edge-margin annotation; circular dial with circumference text; bracketed dimension drawing; red on exactly two elements |

## 1. Visual rules

- **R1 — One panel, carved by hairlines. Never separate boxes.** Regions butt
  edge-to-edge, zero gutter; `1px var(--hairline)` region boundary, `0.5px` intra-
  region. Square corners.
- **R2 — Line-weight vocabulary (4 only):** `1px --hairline` region/row/tile edge;
  `0.5px --hairline` intra divider/chip; `1px --hairline-strong` crop marks/focused
  boundary; `0.5px dashed --hairline-strong` = unavailable/projected ONLY.
- **R3 — Bimodal type: 10.5–11.5px quiet keys OR 20–30px tabular figures. Nothing
  between.** Big number: quiet key 3px above, optional 10.5px muted sub-line; units
  smaller/lighter than the figure.
- **R4 — Annotations attach, never float.** Legal positions: (a) above value, 3px;
  (b) left key / right value, 8px gap; (c) inside region 6–8px from top-left;
  (d) leader tick — 1px line + 4px stub terminating at the feature, then text.
- **R5 — The only codes are real.** Visual treatment of reference codes allowed; the
  payload MUST be live data (mount, depth, counts, offsets, timestamps) or cut.
- **R6 — Textures semantic + keyed.** 45° hatch = filtered/unavailable/locked;
  dot-matrix = bed/coverage/density; dotted outline = not-yet-measured. Always with a
  text label or swatch+word in view (Two-Channel). Utilities: `.tex-hatch`, `.tex-dots`.
- **R7 — Spacing:** 8pt base, 4px half-step. Dense row 16px, medium 18px, list row
  22px, header strip 22px (28px large tiles); key→value 8px; stat key→figure 3px;
  grouped readouts 14px; region padding 8/10/16px.
- **R8 — One accent per field**, spent only on selection / warning / action.
- **R9 — Legends:** one horizontal keyed row at the foot (`swatch + word`) + one muted
  method sentence. Never a card.
- **R10 — Selection = one fill step + hairline ring. Hover = one fill step, 120ms.**
  No glow/scale/shadow.
- **R11 — Light theme is whiter, not greyer:** plate `#ffffff`, grey line-work, one
  warm accent.

## 2. In-tile preview plates

### 2.0 Shared frame (all families)

DOM plate over the WebGL canvas in the `pointer-events:none` overlay layer family
(sibling of `.map-labels` / `.map-instruments`). **A tile with a plate never draws a
floating map label** (the plate's header strip IS the label).

```css
.tm-prev {
  position: absolute;               /* left/top/w/h written imperatively from tile rect */
  inset-block-start: 4px; inset-inline-start: 4px;
  width: calc(var(--tile-w) - 8px); height: calc(var(--tile-h) - 8px);
  display: flex; flex-direction: column;
  background: var(--bg-elevated);   /* L #ffffff · D #1f1f22 */
  border: 0.5px solid var(--hairline-strong);
  border-radius: 0; box-shadow: none; overflow: hidden;
}
.tm-prev__head {
  flex: none; height: 22px;         /* 28px when tile-w >= 320 */
  display: flex; align-items: center; gap: 8px; padding: 0 8px;
  border-bottom: 1px solid var(--hairline);
}
.tm-prev__name { font-family: var(--font-mono); font-size: 11px; font-weight: 500;
  color: var(--ink); white-space: nowrap; overflow: hidden;
  text-overflow: ellipsis; min-width: 0; }
.tm-prev__meta { margin-left: auto; flex: none; font-family: var(--font-mono);
  font-variant-numeric: tabular-nums; font-size: 10.5px; font-weight: 500;
  color: var(--ink-2); white-space: nowrap; }
.tm-prev__body { flex: 1; min-height: 0; position: relative; overflow: hidden; }
```

**Activation thresholds (renderer qualifies at/above):**

| Family | Min tile size | Below → |
|---|---|---|
| TEXT | 160 × 118 | META |
| HEX | 240 × 140 (8 B/row) · 420 × 180 (16 B/row) | TEXT if ≥160 else META |
| IMAGE | 120 × 92 | META |
| LIST | 180 × 120 | META |
| META | 120 × 92 | bare tile label only |

**Static, never scrollable.** Visible lines = `floor((bodyH - 16) / 16)` (TEXT/HEX),
`floor(bodyH / 22)` (LIST). A scrollbar inside a tile is a defect.
**Pool discipline:** ~24 pre-built plate nodes recycled from the draw loop with
imperative writes — plates never enter the React render path.

### 2.1 TEXT / HEX

TEXT body: grid `[gutter] auto [rule] 1px [code] 1fr`; mono 11px / 16px line.
Gutter: `--fill-quiet` bed, right-aligned 10.5px tabular `--ink-2`, width fixed once
(`max(3ch, digits of last visible line) + 14px`). Every line numbered — never skip
rows. **Greyscale syntax only:** comments/numeric literals `--ink-2`;
keywords/directives `--ink` weight 600; blank lines `--ink-3`. Truncation footer 16px
(`--fill-quiet`, top hairline): `… 1,204 more lines` — count in `.num`, right-aligned.
Header meta example: `UTF-8 · 4.2 KB · 128 L` — every field real or omitted.

HEX body: bed `--bg-sunken`; rows `00000000  4d 5a 90 00 …  |MZ..........|`;
8 bytes/row < 420px tile width else 16; non-printable → `·`; offset/bytes/ascii in
`--ink-2` mono 11/16 tabular; ONE `1px --hairline` vertical rule between hex and
ascii columns; extra 2ch space every 8 bytes. **Annotations only when real:**
recognized signature bytes get `--accent-soft` bg + `0.5px --accent` underline + right
margin mono 10.5 `--accent` note (e.g. `PK\x03\x04`) — max 2 notes; parsed field
spans get `0.5px --hairline-strong` rule with left mono 10px `--ink-2` key (`DOS
header`, `PE offset`). Nothing recognized → no annotations at all. Header meta:
`HEX · 0x00000000–0x000001FF`.

Light/dark: L plate `#ffffff`, hex bed `#ececef`, rules `rgba(0,0,0,.09)`, gutter
`rgba(0,0,0,.035)`. D plate `#1f1f22`, hex bed `#101012`, rules
`rgba(255,255,255,.09)`, gutter `rgba(255,255,255,.045)`. Dark body mono weight 400.
Readable ≤11px text uses `--ink-2` (≈4.6:1 L / ≈6.4:1 D); `--ink-3` only for ≥11.5px
secondary text or perfectly-repeating values (offset column).

### 2.2 IMAGE

Bed `--bg-sunken` (flat letterbox bed — permitted; the ban is on the treemap itself
letterboxing). `contain`, never crop. Frame: `1px solid --hairline-strong` around the
IMAGE element (not the bed). `.crop` marks on the frame: two 9px L-brackets
`--hairline-strong` top-left + bottom-right ONLY. Dimension callout `1920 × 1080`
inside the frame's bottom-right on opaque `--bg-elevated` tab with top+left hairlines.
Header meta: `PNG · 842 KB · sRGB`. Below 160px tile width: drop `.crop` + dim tab,
keep 1px frame. Failed decode: `.tex-hatch` bed + centred `.micro` `preview unavailable`
+ real reason in `.num`. **No scrim, gradient, radius, shadow, hover zoom.**

### 2.3 LIST (archives, directory listings, sqlite tables)

Rows 22px grid, columns via `--cols`, gap 8, padding 0 8, bottom `1px --hairline`:
name `11.5/500 --ink` (ellipsize) + value mono 10.5 `--ink-2` right-aligned tabular.
Presets: Archive `1fr 5ch 6ch` (name · method · unpacked) · Directory `1fr 7ch`
(name · size) · SQLite `1fr 7ch 7ch` (table · rows · bytes) · Generic `1fr 7ch`.
Key strip 16px `--fill-quiet`, sentence-case 10.5 `--ink-3`, only when ≥2 columns and
tile-h ≥160; numeric keys right-aligned over values. Rows never wrap; value always
fully visible. Honest truncation: last row clipped by overflow + 16px footer
`… 1,204 more entries`. Hover 120ms `--ease-out` (inert — pointer-events none);
selection `--accent-soft` + `inset 2px 0 0 --accent`. Dark: 1px rules only (never 2px
— striping); name weight 450–500.

### 2.4 META (properties readout — the TARGET INSPECTOR pattern)

Plate padding 8, sections gap 3 (`.stat` rhythm), `1px --hairline` + 8px pad between
sections. Section title `.micro` (Geist 11.5/500 `--ink-2`). kv rows: grid
`64px 1fr`, height 18, key Geist 10.5/500 `--ink-2`, value mono 11/500 tabular
`--ink` ellipsized; numbers/units right-align to a common axis, strings/paths
left-align (never right-align a path). Units in `.u` (10.5, `--ink-2`, 3px left
margin). **One hero figure per plate** via existing `.stat` (mono 550/21px; `.stat.is-hero`
30px only when tile-w ≥ 320). Flag values as boxed chips reusing `.kbd` geometry
(mono 10.5, `0.5px --hairline`, radius 5, pad 1.5/5). Unknown values: `tex-hatch`
12×8 swatch + the word `unknown` — never a bare em-dash. Warnings: value text
`--warn` only for real warnings; `--danger` reserved for scan errors; never tint a
row. Optional right-edge rail (tile-w ≥360 AND tile-h ≥240): 20px, left hairline,
rotated −90° mono 9.5 `--ink-2`, ONE real datum (`MTIME 2026-03-04 11:20`,
`4D 5A · PE32+`); cut the rail if the string doesn't fit exactly.
Dark: plate `#1f1f22`, rules `rgba(255,255,255,.09)`, keys `--ink-2` (not ink-3),
values `--ink`, hero `.stat` `--ink`. Chip borders `--hairline` (never strong).

### 2.5 Token summary table

| | Plate | Bed | Rule | Key colour | Value colour | Sizes |
|---|---|---|---|---|---|---|
| TEXT | `--bg-elevated` | elevated + `--fill-quiet` gutter | 1px hairline (gutter) | n/a | `--ink`/`--ink-2`/`--ink-3` | 11 mono/16, gutter 10.5 |
| HEX | elevated | `--bg-sunken` | 1px hairline (hex│ascii) | n/a | `--ink-2`, marks accent-on-`--accent-soft` | 11 mono/16, off 10.5 |
| IMAGE | elevated | sunken | 1px strong frame + `.crop` | n/a | dim tab `--ink-2` | 10 mono |
| LIST | elevated | elevated | 1px hairline/row | 10.5 `--ink-3` on `--fill-quiet` | name `--ink`, value `--ink-2` | 11.5/10.5, rows 22, keys 16 |
| META | elevated | elevated | 1px hairline/section | 10.5 `--ink-2` | `--ink` (+`--warn`, `.u` ink-2) | key 10.5, val 11, stat 21/30 |

Shared: radius 0, no shadow, `0.5px solid --hairline-strong`, inset 4px, head 22
(28 at w≥320), transitions `background 120ms var(--ease-out)` only.

## 3. Map interior structure (existing depth 0/1)

Goal: a parent at deep zoom reads as an organized container, never confetti.

- **Stroke ladder (primary):** focused parent boundary `1px --hairline-strong` ·
  L1 children `1px --hairline` (today's edge) · deeper `0.5px` 60% hairline · L3+
  no stroke (fill delta + label only). Stroke weight NEVER escalates to imply
  hierarchy (dashed/dotted vocab belongs to states).
- **Flat fill tint steps (secondary):** child fill =
  `color-mix(in srgb, <cat fill> n%, var(--bg))`, `n = 100 / 88 / 76` per level,
  clamped at 3 steps — a step per level, uniform across the level, NEVER a spatial
  gradient. Hatch keeps hatch at every level; hatch line colour steps with the fill.
  **Recompute `inkFor()` per TINTED fill** (contrast floor must hold on what's drawn).
- **Label hierarchy (floor at 9.5px):** focus parent `.micro` + figure right-aligned
  same baseline · L1 600/11.5 + size · L2 500/10.5 + size · L3 500/9.5 @0.85 name
  only · L4+ no label. Below 9.5px → drop the label, never shrink. Plate tiles draw
  no floating label.
- **Leaf interiors:** deep-zoom tile with no preview → `.tex-dots` bed over the
  category fill (6px grid) + centred `.micro` `no preview` + real reason in `.num`
  (`binary`, `0 bytes`, `decode error`).
- **No registration crosses** inside the map (DESIGN: crop marks scope = treemap
  viewport boundary only; image-frame `.crop` excepted above).

## 4. Hard DON'Ts (8)

1. No gradients/glows/glass/blur/shimmer — flat steps + hard-edged textures only.
2. No card-grid, no radius, no resting shadow on plates. Square, opaque, hairline-
   separated.
3. No decorative codes — every bracket/offset/tick carries live data or is cut.
4. No uppercase tracked micro labels (see Integrator Note 3).
5. No colour-only or texture-only encoding — every hatch/dot/tint/warn ships with
   text or a keyed swatch in view.
6. No rainbow syntax, no second accent: three ink steps + weight for text previews;
   accent only for selection + confirmed signature hit; the 8 category hues stay the
   map's only saturation.
7. No label overlap, no bleed outside the tile, no type below 9.5px, no plate gap
   against its tile. Compute rows from actual pixels; clip, never scroll.
8. No filler content, no destructive affordance. Real bytes or an honest
   `unavailable` + reason. No delete/edit/move anywhere.

## 5. File map

Plate styles → **new** `styles/preview.css` (imported from `previewPlate.ts` so
index.css stays untouched) · plate DOM build/fill → **new**
`components/treemap/previewPlate.ts` · pool/position/qualify/label-suppression →
`TreemapCanvas.tsx` · tint steps + inkFor recompute → `lib/treemap/layout.ts` using
helpers exported from `lib/colors.ts` · no new tokens (tokens.css covers all of it).
