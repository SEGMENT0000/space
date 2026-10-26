# space — native design specification

Status: implementation spec for the Iced desktop application.
Scope: `src/ui/theme.rs` and the visual language applied by the native shell.

## 1. Intent

`space` should read as a precision instrument, not a dashboard template. The
treemap is the protagonist; chrome is quiet, flat, and structurally honest.
Surfaces feel like paper, rules are hairlines, measurements are mono, and a
single sage accent marks interaction. Color is information: it belongs to file
categories and state, not to decoration.

Three rules govern every screen:

1. **The data is the ornament.** No background gradients, glows, glass, or
   decorative chrome compete with the map.
2. **Structure is drawn, not implied.** Separation comes from 1 px rules,
   spacing, and tonal steps — not from a field of floating cards.
3. **One accent, real weights, exact figures.** Interaction is sage; type is
   Geist; numbers and paths are Geist Mono.

The authoritative composition reference is
`inspiration_images_that_how_should_the_ui_look_like/this_is_main_this_is_how_it_should_look_like.jpg`.
It defines the airy treemap, quiet top bar, narrow inspector rail, and compact
status line. The tokens below define exact values.

## 2. Reference application

- **Top bar — 52 px.** `surface`, one 1 px `hairline` beneath. Wordmark at
  17 px SemiBold; commands, volume context, and theme control at 12 px. No
  shadow, no gradient, no faux window chrome.
- **Main region.** A full-bleed treemap on `bg` with a 318 px inspector rail
  separated by a 1 px rule. The rail is `surface`; it is not a floating card.
- **Treemap.** Off-white bed, 1 px gutters between leaves, 3 px group insets,
  2 px tile radius. Nested pastel category fills carry the only large color
  field. Labels are category ink. No outer card, no inner glow.
- **Summary strip.** Four or five compact figures separated by vertical
  hairlines. Figures at 22 px Geist Mono SemiBold; captions at 12 px.
- **Status line — 28 px.** 11–12 px `ink_faint` metadata, 1 px `hairline` above.
- **Category chips.** A small solid 8–10 px swatch, 4 px gap, then the category
  name in `ink` or `ink_muted`. Never a colored pill, never an orange “s” badge.
- **Overlays.** Command palette, context menu, and toasts use `surface_raised`,
  a 1 px `hairline_strong` edge, 8 px radius, and the single overlay shadow.
- **Window.** Native decoration. Content fills the client area; no fake title
  bar, no rounded outer frame.

## 3. Color

### 3.1 Neutral and interaction tokens

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg` | `#F5F6F5` | `#121514` | App bed, treemap bed |
| `surface` | `#FBFCFB` | `#181C1A` | Chrome, panels, rails |
| `surface_raised` | `#FFFFFF` | `#202522` | Menus, palette, sheets |
| `surface_sunken` | `#EDF0EE` | `#0D100F` | Inputs, wells, tracks |
| `surface_hover` | `#F0F2F1` | `#202622` | Hover wash on neutral rows |
| `surface_pressed` | `#E5E8E6` | `#29302B` | Pressed controls |
| `ink` | `#1B201D` | `#EDF1EF` | Primary text |
| `ink_muted` | `#525A56` | `#AAB1AD` | Secondary text |
| `ink_faint` | `#666E6A` | `#89918D` | Metadata at ≥11 px |
| `hairline` | `#E2E5E3` | `#282E2A` | Decorative 1 px rules |
| `hairline_strong` | `#87908A` | `#667069` | Overlay edges, control boundaries |
| `accent` | `#256B5A` | `#7CC4AE` | Single interaction accent |
| `accent_ink` | `#F7FBF9` | `#0B241E` | Text/icons on accent |
| `accent_soft` | `#DDEDE7` | `#173A31` | Selected rows, active command row |
| `selection` | `#D3E7DE` | `#23453B` | Text-selection wash |
| `danger` | `#B24A35` | `#E2846D` | Destructive text/icons |
| `danger_soft` | `#FAEDE9` | `#3A241F` | Destructive well |
| `warning` | `#8A621C` | `#D6A85C` | Caution |
| `success` | `#3F704D` | `#7DBB8C` | Completion, positive status |
| `scrim` | `rgba(19,24,21,.24)` | `rgba(0,0,0,.58)` | Modal scrim only |
| `shadow` | `rgba(19,24,21,.16)` | `rgba(0,0,0,.55)` | Floating overlays only |

Rules:

- `accent` is the only interactive hue. Do not add a second brand color,
  gradient stop, or “info blue.”
- Semantic colors are not decoration. A green dot means success, not “this
  looks nice.”
- `hairline` is for structure; `hairline_strong` is for boundaries that must
  survive on a surface. Neither is a text color.
- `selection` is a wash behind selected text. It is deliberately distinct from
  `accent_soft`, which marks a selected row or command.
- Dark mode is a tonal translation, not an inversion and not pure black.

### 3.2 Category palette

Order is the `Category` contract: Media, Documents, Code, Caches, Apps,
Archives, System, Other.

| Category | Light fill | Light ink | Dark fill | Dark ink |
|---|---|---|---|---|
| Media | `#F3C8B5` | `#5B392B` | `#5D3B34` | `#F1D5C9` |
| Documents | `#EFE1B4` | `#594924` | `#534A2D` | `#EFE2BD` |
| Code | `#CFE0C1` | `#36533A` | `#384127` | `#D6E8D3` |
| Caches | `#B9E4E9` | `#2E4C55` | `#21474F` | `#CFE9EC` |
| Apps | `#D6D6F0` | `#453F5F` | `#3D3E59` | `#DFDCF4` |
| Archives | `#F2CEDC` | `#5D3A47` | `#513849` | `#F2D6E1` |
| System | `#BBC6CE` | `#3C474F` | `#33393E` | `#DBE1E4` |
| Other | `#DAD1C6` | `#4F493D` | `#4A4238` | `#E8E2D4` |

Each category also has a `category_soft` wash for rows and chips. Fills are
muted pastels in light mode and desaturated deep pastels in dark mode; hue
identity is preserved across appearances. Apps is a flat lilac — never a
purple-blue gradient.

## 4. Typography

Only two families ship: **Geist** (`Geist-Regular`, `-Medium`, `-SemiBold`,
`-Bold`) and **Geist Mono** (`GeistMono-Regular`, `-Medium`, `-SemiBold`).
Inter, Roboto, Arial, Helvetica, Open Sans, and system fallback faces are not
part of the design. Weights must come from the loaded files; never synthesize
bold.

| Role | Size / line | Family / weight | Use |
|---|---|---|---|
| Micro | 11 / 14 | Geist Medium | Chips, keycaps, dense metadata |
| Caption | 12 / 16 | Geist Regular | Secondary labels, paths, hints |
| Body | 13 / 18 | Geist Regular | Controls, rows, prose |
| Label | 15 / 20 | Geist Medium | Section labels, emphasized values |
| Heading | 17 / 22 | Geist SemiBold | Panel and screen headings |
| Title | 22 / 28 | Geist SemiBold | Screen titles, large figures |
| Display | 28 / 34 | Geist SemiBold | Empty states, hero figures |
| Figure SM | 12 / 16 | Geist Mono Medium | Table figures |
| Figure MD | 15 / 20 | Geist Mono Medium | Inspector values |
| Figure LG | 22 / 26 | Geist Mono SemiBold | Summary strip |
| Figure XL | 28 / 32 | Geist Mono SemiBold | Hero measurement |
| Path | 12 / 16 | Geist Mono Regular | File paths, breadcrumb tails |
| Keycap | 11 / 14 | Geist Mono Medium | Keyboard hints |

- UI text uses 11 px or larger for primary labels, controls, prose and figures.
  The inspector's densest secondary metadata (preview footers, schema columns,
  aggregate captions) uses 9.5-10.5 px by explicit exception; it never carries
  the only copy of an action or state. No product text is smaller than 9.5 px.
- Use Geist Mono for figures, sizes, paths, IDs, and keyboard hints. Do not set
  prose in mono.
- Alignment beats decoration: right-align numeric columns and use tabular
  figures for comparisons.
- Letter spacing stays at the font default. No all-caps tracking tricks except
  short 11 px metadata labels, and even those remain sentence case where space
  allows.

## 5. Spacing, shape, and structure

**Spacing** follows one four-pixel grid: `4, 8, 12, 16, 24, 32, 48`.
Use 4 inside compact marks, 8 between related controls, 12 within a section,
16 for panel padding, 24 between sections, 32 for page margins, 48 for
empty-state breathing room. Do not introduce 20 or 28 px spacing.

**Radii:** 2 for swatches, treemap tiles, and tiny marks; 4 for buttons, inputs,
and segmented controls; 6 for panels; 8 for floating overlays; 10 for modal
sheets. Large cards are not 16–24 px rounded. Pills are reserved for status
badges, not ordinary controls.

**Controls:** 24 px compact, 28 px default, 32 px prominent. Minimum hit area
is 28 × 28 px even when the visible glyph is smaller.

**Layout:** top bar 52, status line 30, inspector rail 318, tree/list row 28.

**Hairlines:** 1 px for rules, panel edges, and overlay edges; 1.5 px for
selected boundaries, category parent outlines, and strong dividers; 2 px focus
ring with a 1 px offset. Never use 0.5 px rules for structure or 3 px borders
for emphasis.

**Icons:** 14, 16, or 20 px with a 1.5 px stroke. No filled icon sets mixed with
line icons. No emoji as product iconography.

**Elevation:** exactly one shadow recipe for floating overlays — `shadow`,
8 px downward offset, 24 px blur, paired with a 1 px `hairline_strong` edge.
Panels and cards have no shadow. There is no “hover lift.”

## 6. Treemap semantics

1. **Bed.** The treemap sits directly on `bg`. The light bed is a neutral
   off-white (`#F5F6F5`), not warm cream or sepia; the dark bed is `#121514`.
2. **Leaves.** Fill from `category_fill`. A 1 px gutter exposes the bed and
   keeps the map airy. Tiles use a 2 px radius, never pill shapes.
3. **Nesting.** A group insets its children by 3 px. Depth is a subtle tint of
   the category fill: 8% per level toward `surface_raised` in light mode or
   toward `bg` in dark mode, capped at 24%. Do not jump hue families with depth.
4. **Parent outlines.** Draw a 1.5 px `category_line` outline, derived by
   interpolating category ink toward the fill (42% in light, 30% in dark).
   This is containment, not a heavy border.
5. **Hidden categories.** Blend 84% toward the bed. The tile should recede but
   remain discoverable; pair the treatment with the “hidden” label in the
   legend.
6. **Hover.** A 1 px `category_line` outline or a 4% ink wash. No scale, glow,
   or shadow.
7. **Selection.** A 1.5 px `accent` outline plus a 1 px inner offset. Text stays
   `category_ink`; do not invert the tile to accent.
8. **Labels.** Group headers use 11 px Geist Medium; leaf names use 12 px Geist
   Medium when they fit. Sizes use Geist Mono. Clip with an ellipsis rather than
   overflowing. If a label cannot fit, omit it — never draw 8 px type.
9. **Chips and legends.** Solid swatch + label. The swatch carries the hue; the
   label stays neutral ink. Color is never the only encoding: the inspector,
   legend, and labels always name the category.
10. **Figures.** File sizes and percentages are mono. Percentage columns are
    right-aligned. Avoid repeating a percentage in both label and tooltip.

## 7. States, focus, and selection

- **Hover (neutral):** `surface_hover`. On treemap tiles, use the outline
  treatment above rather than a filled wash.
- **Pressed:** `surface_pressed`; no translation or bounce.
- **Selected row/command:** `accent_soft` fill, `ink` text, and a 2 px `accent`
  leading rule. The accent rule is the non-color cue.
- **Keyboard focus:** 2 px `accent` ring with a 1 px offset. It must remain
  visible on `bg`, `surface`, `surface_raised`, and `accent_soft`.
- **Text selection:** `selection` behind selected glyphs; keep `ink` as the
  value color.
- **Disabled:** `surface_sunken` fill, `ink_faint` text, `hairline` edge, no
  hover response. Do not apply blanket opacity below 0.55.
- **Error:** `danger` icon/text, optional `danger_soft` well. Never rely on red
  alone; pair with a direct sentence.
- **Warning:** `warning` icon/text with the same pairing rule.
- **Success:** `success` text or a static 6 px dot. No pulsing indicator.
- **Loading:** a 1.5 px accent segment on a `surface_sunken` track, using the
  standard motion durations. No shimmer, orbiting dots, or gradient sweep.
- **Empty:** Display-size sentence, one muted explanatory line, one primary or
  outline action. No illustration unless it carries real meaning.

## 8. Motion

One curve: `cubic-bezier(0.16, 1.0, 0.3, 1.0)`.

| Motion | Duration |
|---|---|
| Hover / wash | 120 ms |
| Control state | 160 ms |
| Overlay enter/exit | 200 ms |
| Camera / treemap refit | 220 ms |
| Hard ceiling | 240 ms |

No springs, elastic overshoot, parallax, animated gradients, or looping
decorative motion. Reduced-motion mode snaps to the final state; the interface
must remain fully legible without interpolation.

## 9. Light and dark

- **Light:** neutral off-white bed, near-white chrome, ink text. Hairlines are
  visible but quiet. Category fills are airy pastels; the map should feel like
  colored paper, not candy.
- **Dark:** deep green-black bed (`#121514`), slightly lifted chrome, off-white
  ink. Category fills become deep muted tints with light category ink. Avoid
  pure black fields and avoid simply inverting the light palette.
- Hue identity and category order are identical in both themes. Semantic roles
  never swap.
- Dark surfaces step by small tonal increments (`bg → surface → raised`), not by
  borders alone. Light mode uses hairlines more; dark mode uses tonal steps plus
  hairlines.
- Scrim and shadow are the only translucent tokens. No translucent panels.

## 10. Contrast

AA targets are verified against the hex tokens. Ratios below are computed, not
sampled from a screenshot.

| Pair | Light | Dark |
|---|---|---|
| `ink` on `bg` | 15.25:1 | 16.12:1 |
| `ink_muted` on `bg` | 6.56:1 | 8.40:1 |
| `ink_faint` on `bg` | 4.84:1 | 5.68:1 |
| `accent` on `bg` | 5.82:1 | 9.07:1 |
| `danger` on `bg` | 4.94:1 | 6.78:1 |
| `warning` on `bg` | 5.04:1 | 8.41:1 |
| `success` on `bg` | 5.33:1 | 8.19:1 |
| `accent_ink` on `accent` | 6.04:1 | 8.06:1 |
| `ink` on `selection` | 12.79:1 | 9.28:1 |
| `danger` on `danger_soft` | 4.68:1 | 5.34:1 |
| Category ink on fill (minimum) | 5.48:1 | 6.83:1 |
| Category ink on soft (minimum) | 7.39:1 | 8.80:1 |
| `hairline_strong` on `bg` (non-text) | 3.04:1 | 3.58:1 |

- All primary, muted, faint, accent, semantic, and category text pairs meet
  WCAG AA (4.5:1) on `bg`, `surface`, and `surface_sunken`.
- `hairline_strong` is a non-text boundary token and meets 3:1 on `bg`,
  `surface`, and `surface_raised` in both themes. On the light sunken well it is
  just below 3:1, so use `accent` or a focus ring for critical control
  boundaries there. `hairline` is decorative structure and is not used as the
  sole boundary for a critical control.
- Do not place `ink_faint` on category fills. Use `category_ink` there.
- Disabled controls are exempt from AA but must remain perceivable through
  position, label, and structure.

## 11. Anti-patterns

Do not introduce:

- purple-blue gradients, mesh gradients, aurora backgrounds, or gradient text;
- glassmorphism, backdrop blur, frosted panels, or translucent cards;
- neon glow, colored outer shadows, or “selected” glow;
- uniform card shadows or a card for every section;
- pulsing dots, shimmer skeletons, orbiting spinners, or decorative animation;
- fake window controls, scanlines, grain overlays, or ornamental chrome;
- the orange “s” badge or any category-as-pill treatment;
- multiple accent hues or semantic colors used decoratively;
- saturated treemap fills, rainbow palettes, or hue jumps by depth;
- text below 11 px, faux bold, or more than four weights in one screen;
- fully rounded (999 px) ordinary controls, inputs, or panels;
- color as the only signal for hidden, selected, error, or success states;
- springy, elastic, or overshooting motion;
- pure-black dark surfaces or a warm/sepia light bed.

## 12. Implementation contract

`src/ui/theme.rs` is the single source of truth.

```rust
let t = Theme::for_dark(app.dark);

container(content).style(move |_| container::Style {
    background: Some(Background::Color(t.bg)),
    text_color: Some(t.ink),
    ..Default::default()
});
```

Canonical API:

- `Theme::light()`, `Theme::dark()`, `Theme::for_dark(bool)`, `to_iced()`
- `t.is_dark` (canonical), `t.dark` (compatibility alias; both are set)
- neutral tokens, semantic tokens, `scrim`, `shadow`
- `t.category(cat)`, `t.category_soft(cat)`, `t.category_ink(cat)`,
  `t.on_category(cat)`, `t.category_line(cat)`
- category arrays for canvas palettes: `category_fill`, `category_soft`,
  `category_ink`
- `ui_font(weight)`, `mono_font(weight)`, weight helpers
- `mix`, `ease`, `truncated`
- spacing, radii, control, layout, icon, treemap, and motion constants

`to_iced()` maps `bg → background`, `ink → text`, `accent → primary`, and the
semantic colors to their Iced counterparts. Stock Iced widgets should be styled
from the same tokens rather than relying on defaults.

Fonts are registered once in `src/ui/mod.rs`. `FONT_UI` is `"Geist"` and
`FONT_MONO` is `"Geist Mono"`; all text constructors go through
`ui_font` / `mono_font`.

### Implementation notes

- The treemap bed is `t.bg`; the Canvas and shader use the same palette.
- Nested fills move 8% per depth toward the raised surface, capped at 24%.
- Parent outlines use `t.category_line(cat)` at 1 px; selection uses a 1.5 px
  accent outline with a 1 px offset.
- Neutral hover uses `surface_hover`; pressed uses `surface_pressed`.
- Category hatching is information: Caches and the aggregate tail carry a
  diagonal pattern, so category identity is never color-only.
- The low-level `t.dark` alias remains only where older call sites need it; new
  code reads `t.is_dark`.

### Verification

`cargo test --all-targets` is the gate. Unit tests in `theme.rs` assert AA
contrast for text and categories, that both dark flags agree, and that the
motion curve is bounded and monotonic. Contrast figures above are calculated
from the tokens, not sampled from a screenshot. Screenshot review is recorded in
`docs/QA.md`.
