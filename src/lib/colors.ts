// Category palette — the single source of truth for tile colors, the GL
// clear/stroke tokens and the DOM legend. Eight hues, harmonized for
// light and dark; every category is also distinguishable by its text label
// and the caches hatch, so color is never the only channel.
//
// Load-bearing exports (signatures frozen; treemap/layout.ts and
// TreemapCanvas.tsx depend on them): CATEGORY_LABELS, CatStyle, ThemeTokens,
// inkFor, tintFill, categoryStyle, themeTokens.

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
  /** label ink as rgb floats 0..1 */
  text: [number, number, number];
  hatch: boolean;
  /** css color for the DOM legend */
  css: string;
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

function styles(hexes: string[]): CatStyle[] {
  return hexes.map((hex, i) => {
    const fill = hexRgb(hex);
    return {
      fill,
      text: inkFor(fill),
      hatch: i === CACHES_INDEX,
      css: hex,
    };
  });
}

const LIGHT_CATS = styles(LIGHT);
const DARK_CATS = styles(DARK);

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
