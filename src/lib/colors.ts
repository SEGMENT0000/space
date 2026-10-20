// Category palette — the single source of truth for tile colors, the GL
// clear/stroke tokens and the DOM legend. Eight hues, ONE per category,
// harmonized for light and dark; every category is also distinguishable by
// its text label and the caches hatch, so color is never the only channel.
// This is the treemap's entire saturation budget — chrome stays grey.
//
// Load-bearing exports (signatures frozen; treemap/layout.ts and
// TreemapCanvas.tsx depend on them): CATEGORY_LABELS, CatStyle, ThemeTokens,
// inkFor, tintFill, categoryStyle, themeTokens.
//
// Additive exports (palette-plate/token agents mirror these; no consumer
// breaks): CATEGORY_HEX (the raw per-theme hex table), CatStyle.surface
// (quiet hue wash on the theme panel) and CatStyle.ink (the hue tuned to be
// TEXT on the theme panel, ≥4.5:1), entryTint / entryTintCss (stable,
// deterministic, LOW-chroma per-entry variation so tiles can carry subtle
// identity without turning the field into confetti).

import { CAT_COUNT } from "./types";

export const CATEGORY_LABELS = [
  "Media",
  "Documents",
  "Code",
  "Caches",
  "Apps",
  "Archives",
  "System",
  "Other",
] as const;

export interface CatStyle {
  /** fill as rgb floats 0..1 (GL) */
  fill: [number, number, number];
  /** label ink ON THE FILL as rgb floats 0..1 (black/white by luminance) */
  text: [number, number, number];
  hatch: boolean;
  /** css color for the DOM legend — the solid hue */
  css: string;
  /** The hue as a quiet SURFACE WASH over this theme's panel (hex): a flat
   *  ~14–18% mix, no alpha, so chips/rows/plate beds tint predictably on
   *  both appearances without the caller composing over an unknown ground. */
  surface: string;
  /** The hue as TEXT on this theme's panel (hex), walked to clear 4.5:1 on
   *  --bg-elevated. Distinct from `text`: `text` is ink drawn ON the fill;
   *  `ink` is the fill hue drawn ON the surface. */
  ink: string;
}

export interface ThemeTokens {
  stroke: [number, number, number];
  canvasBg: [number, number, number];
}

function hexRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

// prettier-ignore
const LIGHT = [
  "#3d7fdd", // media    — system blue
  "#2f9b8e", // documents— teal
  "#5d9c46", // code     — leaf
  "#d2952c", // caches   — amber (hatched)
  "#d1663c", // apps     — vermilion
  "#c2547a", // archives — rose
  "#8a6fc0", // system   — violet
  "#7c828b", // other    — slate
];

// prettier-ignore
const DARK = [
  "#4e8ef0",
  "#3cb3a4",
  "#74b457",
  "#e0a53f",
  "#e07b50",
  "#d76a90",
  "#a188d9",
  "#8d949e",
];

/** The raw table, mirrored by token/plate work so CSS and GL can never drift. */
export const CATEGORY_HEX = { light: LIGHT, dark: DARK } as const;

const CACHES_INDEX = 3;

// Label ink is chosen by the FILL's relative luminance, never by category:
// white clears 4.5:1 only at L <= 0.1833, #1c1c1f clears it only at
// L >= 0.2317, and pure black covers the band between (L >= 0.175). Three
// bands, so every tile label on both themes clears the contrast floor —
// the old white-on-bright-fill rule sat at 2.5–4.3:1 on most categories.
function relLum(c: [number, number, number]): number {
  const f = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  const [r, g, b] = c;
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** Ink for an arbitrary fill. Exported so layout can re-derive label ink AFTER
 *  applying a depth tint — the 4.5:1 floor must hold on the fill that is
 *  actually drawn, not the base category colour. */
export function inkFor(fill: [number, number, number]): [number, number, number] {
  const L = relLum(fill);
  if (L <= 0.1833) return [1, 1, 1]; // white: dark fills
  if (L < 0.2317) return hexRgb("#000000"); // mid band: only black reaches 4.5
  return hexRgb("#1c1c1f"); // the design's near-black ink
}

/** Depth tint: flatten a category fill toward the canvas background by one flat
 *  step per level (100 / 88 / 76%, clamped) — the interior-structure channel
 *  from INS_BRIEF §3. A step per level, uniform across the level; never a
 *  spatial gradient. Returns the tinted fill; call inkFor() on the RESULT. */
export function tintFill(
  fill: [number, number, number],
  depth: number,
  theme: "light" | "dark",
): [number, number, number] {
  const k = depth <= 0 ? 1 : depth === 1 ? 0.88 : 0.76;
  if (k === 1) return fill;
  const bg = themeTokens(theme).canvasBg;
  return [
    fill[0] * k + bg[0] * (1 - k),
    fill[1] * k + bg[1] * (1 - k),
    fill[2] * k + bg[2] * (1 - k),
  ];
}

// ---------------------------------------------------------------- hsl helpers
// Only the palette builder and entryTint use these; they run at module init or
// per layout rebuild (never per frame), so clarity beats micro-optimization.

/** h in [0,360), s/l in [0,1]. */
function rgbToHsl(c: [number, number, number]): [number, number, number] {
  const [r, g, b] = c;
  const mx = Math.max(r, g, b);
  const mn = Math.min(r, g, b);
  const l = (mx + mn) / 2;
  if (mx === mn) return [0, 0, l]; // grey: no hue to preserve
  const d = mx - mn;
  const s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
  let h: number;
  if (mx === r) h = ((g - b) / d + (g < b ? 6 : 0)) * 60;
  else if (mx === g) h = ((b - r) / d + 2) * 60;
  else h = ((r - g) / d + 4) * 60;
  return [h, s, l];
}

function hslRgb(h: number, s: number, l: number): [number, number, number] {
  if (s === 0) return [l, l, l];
  h = ((h % 360) + 360) % 360;
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hue = (t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [hue(h / 360 + 1 / 3), hue(h / 360), hue(h / 360 - 1 / 3)];
}

function hexOf(c: [number, number, number]): string {
  const b = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, "0");
  return `#${b(c[0])}${b(c[1])}${b(c[2])}`;
}

function contrast(a: number, b: number): number {
  return a > b ? (a + 0.05) / (b + 0.05) : (b + 0.05) / (a + 0.05);
}

// ------------------------------------------------------- surface + ink variants
// The panel each wash/ink is composed against (--bg-elevated in tokens.css).
const PANEL: Record<"light" | "dark", [number, number, number]> = {
  light: hexRgb("#ffffff"),
  dark: hexRgb("#17171b"),
};

// Surface wash strength: a flat mix of hue INTO the panel, chosen so dark
// reads as a felt tint (its ground is near-black) and light stays a whisper
// (its ground is already white). No alpha: the caller gets one hex that is
// correct on the panel it was built for.
const SURFACE_MIX: Record<"light" | "dark", number> = { light: 0.14, dark: 0.18 };

/** The hue walked toward readable TEXT contrast on the panel: darken on
 *  light, lift on dark, preserving hue (and most chroma — an ink that greys
 *  out stops carrying the category). Falls back to the theme ink if even the
 *  clamp can't reach 4.5:1 (never happens for these eight, but the floor is
 *  a floor). */
function panelInk(fill: [number, number, number], theme: "light" | "dark"): string {
  const [h, s] = rgbToHsl(fill);
  const surfL = relLum(PANEL[theme]);
  const base = theme === "dark" ? 0.58 : 0.42;
  const step = theme === "dark" ? 0.02 : -0.02;
  const clampTo = theme === "dark" ? 0.9 : 0.1;
  for (let i = 0, l = base; i <= 24; i++, l += step) {
    const c = hslRgb(h, Math.min(1, s * 1.08), Math.min(clampTo, Math.max(0.08, l)));
    if (contrast(relLum(c), surfL) >= 4.5) return hexOf(c);
  }
  return theme === "dark" ? "#f4f4f6" : "#1c1c1f";
}

function styles(hexes: string[], theme: "light" | "dark"): CatStyle[] {
  const mix = SURFACE_MIX[theme];
  const panel = PANEL[theme];
  return hexes.map((hex, i) => {
    const fill = hexRgb(hex);
    const surface: [number, number, number] = [
      fill[0] * mix + panel[0] * (1 - mix),
      fill[1] * mix + panel[1] * (1 - mix),
      fill[2] * mix + panel[2] * (1 - mix),
    ];
    return {
      fill,
      text: inkFor(fill),
      hatch: i === CACHES_INDEX,
      css: hex,
      surface: hexOf(surface),
      ink: panelInk(fill, theme),
    };
  });
}

const LIGHT_CATS = styles(LIGHT, "light");
const DARK_CATS = styles(DARK, "dark");

// GL tokens mirror styles/tokens.css so the WebGL canvas agrees with the DOM:
// canvasBg is the ground the canvas sits on (.map-wrap → var(--bg)) — it is
// the clear color AND the target the depth tint flattens toward, so it must
// never drift from the surface behind the canvas. stroke is the tile grout,
// kept equal to the theme ink.
const LIGHT_TOKENS: ThemeTokens = {
  stroke: hexRgb("#1c1c1f"),
  canvasBg: hexRgb("#f1f1f3"),
};
const DARK_TOKENS: ThemeTokens = {
  stroke: hexRgb("#f4f4f6"),
  canvasBg: hexRgb("#0b0b0d"),
};

export function categoryStyle(cat: number, theme: "light" | "dark"): CatStyle {
  const table = theme === "dark" ? DARK_CATS : LIGHT_CATS;
  return table[cat >= 0 && cat < table.length ? cat : CAT_COUNT - 1];
}

export function themeTokens(theme: "light" | "dark"): ThemeTokens {
  return theme === "dark" ? DARK_TOKENS : LIGHT_TOKENS;
}

// toGl removed: it was an identity function (fill floats already 0..1) with
// zero callers after the renderer started consuming CatStyle.fill directly —
// grep over src/ finds no import. Re-add only if a real transform appears.

// ------------------------------------------------------------- entry identity
// A stable, deterministic per-entry tint: same entry → same tint forever, on
// every machine, regardless of scan order. The variation is deliberately
// NARROW — hue ±12°, chroma pulled to 50–74% of the category's, lightness
// ±6% — so a tile reads as "a Media file, slightly its own" and never as a
// ninth category. This is the anti-confetti budget: restraint is the feature.
//
// No cache and no allocation beyond the returned tuple: the hash is FNV-1a
// (numbers fold through an integer mix first), the modulation is three
// arithmetic steps off the precomputed category HSL. Safe to call per tile
// per layout rebuild — never call it per frame.

function hashKey(key: number | string): number {
  let h = 0x811c9dc5;
  if (typeof key === "number") {
    // Integer mix (splitmix-ish): entry ids are dense and sequential, so a
    // raw FNV over the decimal digits would correlate neighbours. This
    // avalanche-mixes the id itself, no string ever built.
    h = Math.imul(key ^ 0x9e3779b9, 0x85ebca6b) >>> 0;
  } else {
    for (let i = 0; i < key.length; i++) {
      h ^= key.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
  }
  h ^= h >>> 15;
  h = Math.imul(h, 0x2545f491);
  h ^= h >>> 13;
  return h >>> 0;
}

/** Per-entry tint as rgb floats 0..1 (GL-ready). Deterministic from
 *  `key` (an entry id or name); low chroma by construction. Out-of-range
 *  cats fold to Other (same contract as categoryStyle). Call inkFor() on
 *  the result if you will draw a label on it; compose through tintFill()
 *  first if it is a depth->1 tile. */
export function entryTint(
  cat: number,
  key: number | string,
  theme: "light" | "dark",
): [number, number, number] {
  const style = categoryStyle(cat, theme);
  const [h, s, l] = rgbToHsl(style.fill);
  const k = hashKey(key);
  const hue = h + ((k % 25) - 12); // ±12°
  const sat = Math.min(1, s * (0.5 + ((k >>> 5) % 5) * 0.06)); // 0.50–0.74 × base
  const lit = Math.min(0.9, Math.max(0.16, l + (((k >>> 10) % 7) - 3) * 0.02)); // ±6%
  return hslRgb(hue, sat, lit);
}

/** Same tint as entryTint(), as a #rrggbb string for DOM use (legend chips,
 *  plate accents, row washes). */
export function entryTintCss(
  cat: number,
  key: number | string,
  theme: "light" | "dark",
): string {
  return hexOf(entryTint(cat, key, theme));
}
