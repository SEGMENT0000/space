---
version: 1
slug: "space-src-app-tsx"
primary_target: "space/src/App.tsx"
related_targets: []
---

# Surface brief — space (whole app, explorer-first)

## Direction contract

THESIS: `space` turns a disk scan into one honest proportional picture — the treemap IS the product; everything around it is a quiet instrument panel in the pinned Headroom language: floating rounded panels on near-black, warm signal accents spent only where they mean something. Refuses both the boxed card grid and the decorative one-number dashboard.

OWN-WORLD: window ground #0b0b0d; floating panels #17171b, radius 14px, hairline edge rgba(255,255,255,.07), one soft edge highlight; interiors divided by 1px hairlines, never nested cards; Geist + self-hosted Geist Mono, tabular figures everywhere; dot-matrix beds under progress and usage rails; legend chips (swatch + word + mono value); the 8-category data palette owns the treemap and nothing else; warm accent #e8654a spent only on the primary action, selection, and real warnings; sentence-case labels; mono for figures, paths, keycaps.

STORY: scan any volume or drop a folder, read what eats the space within seconds, drill into folders by zoom, and hover anything — tile or rail row — to get its complete, correct readout with useful actions. Read-only by construction; the interface says so plainly.

FIRST VIEWPORT (explorer): transparent topbar on the ground (back/forward, breadcrumb path, search field, sort control, rail toggle) over the content row: the map panel — rounded, hairline-edged, filling all remaining width, treemap edge-to-edge inside it — and the 300px inspector rail panel (hero folder figure, composition legend with toggles, largest-files list). Statusbar strip below the panels, transparent on the ground, mono readouts left, quiet actions right.

FORM: user-pinned direction (the ins/ Headroom instrument-panel set + the user's love of the current rail). concept-seed roll skipped: a pinned direction beats the roll. seed: none (unavailable).

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.

## Scope & constraints (standing)

- Mode: Operate. Code-led build (no image generation in this harness).
- Keep PRODUCT.md truth byte-for-byte: read-only, no delete affordances, banned fonts, banned decoration, left-button primary drag, treemap fills its container exactly, no label overlap, synthetic data fully fictional.
- Both appearances must survive (light + dark); dark becomes the default.
- Every live figure rides Geist Mono tabular-nums.
- Hover truth: a hovering pointer must resolve the item under it — never a stale or first-row payload.
