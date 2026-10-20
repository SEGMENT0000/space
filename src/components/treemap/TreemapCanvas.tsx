// Treemap renderer: WebGL2 instanced tiles + a 2D label overlay with real
// collision resolution. The renderer OWNS the frame pipeline: it measures its
// own container (ResizeObserver) and lays out the world at the container's
// exact aspect, so the view always fills the canvas — no letterboxing and no
// stale-frame window.
//
// FILL INVARIANT (the product requirement): the world always fills the
// viewport; zoom is in-only. The world is laid out at the canvas's own
// aspect, so fitViewport() — a COVER at matching aspect — IS an exact
// edge-to-edge fill, and its scale is therefore not a default but the FLOOR:
// no viewport may ever scale below it (MIN_ZOOM is gone; 100% = the floor).
// On any aspect drift fitViewport() overfills and crops rather than
// letterboxes (layout.ts), and ensureFrame()'s drift check rebuilds at the
// fresh aspect, so background is unreachable even on a stale frame. Every
// path that sets scale (wheel, zoomBy, zoomToTile, nav transitions, animation
// targets, resize refits) is clamped through enforceFill(), and pan is pinned
// so the world's bounds always cover the canvas. Letterboxing is structurally
// impossible: if you can see background, a clamp is broken.
//
// Left-drag pans (the primary gesture), shift-drag marquees, click selects,
// double-click drills in. One spring curve drives every viewport animation.
//
// Chrome around the tiles stays instrument-quiet: an overlay renders crop
// marks framing the data field and a mono zoom readout that only speaks when
// the view leaves the fitted state. The readout is written via textContent
// from the draw loop — never through React state, so panning stays free.
//
// ---------------------------------------------------------------- hover pipeline
// Hover is rAF-aligned: pointermove only leaves coordinates behind
// (hoverPending, mutated in place); ONE scheduled pass per frame resolves,
// repaints, and notifies the parent. The repaint is NOT a frame rebuild — a
// tile crossing changes exactly two aHover floats in the instance buffer,
// which are patched in place with two 4-byte bufferSubData writes followed by
// a re-issue of the instanced draw plus a 2D label redraw. Why partial-patch
// instead of a label-layer-only redraw: the hover statement includes a GL
// fill lift (one step lighter), so a 2D-only path would leave the field
// stale; patching 8 bytes + one drawArraysInstanced costs µs and keeps the
// GL field and the label layer in lockstep. A rebuild (identity checked via
// instFrame) forces a full upload, so the patch can never desync.
//
// ---------------------------------------------------------------- theme palette
// The GL field/selection colors are read from CSS once per theme
// (getComputedStyle) and cached in `palette`: --accent (the warm selection
// ring + marquee), --accent-soft (the selected fill-lift strength), and the
// field = the first opaque ancestor background behind the canvas — so the
// clear color always matches whatever surface the map panel actually paints,
// in both themes. Reloaded by the theme prop effect and a MutationObserver
// on the document's data-theme attribute; tiles' own fills/inks still come
// from colors.ts (tintFill/inkFor via layout.ts) — palette only covers what
// CSS owns.
//
// Selection is signalled in the shader as a 1px warm-accent inner ring plus
// an accent-soft fill lift (see gl.ts), which scales to multi-select; hover
// is one fill step lighter and the hovered/selected label draws full-ink.
// The label overlay resolves collisions largest-first, and depth-0 labels
// carry a category swatch so hue is never the only channel. The BIG label
// tier stays depth-0-only; deeper labels all share the quiet 10.5px tier.
//
// Directories subdivide LAZILY to MAX_INTERIOR_DEPTH: the view feeds
// already-fetched interiors through `expanded`, and the draw loop asks for
// the ones that can visibly subdivide (requestInteriors). An interior
// folder's preview is its interior, so it never plates — unless the interior
// has no plate-worthy content at all, in which case collapseContentlessInteriors
// removes its descendants and turns it into a leaf folder first. Preview
// plates belong to FILE tiles at any depth AND to LEAF FOLDERS (directories
// with no child rect this frame): their real folder listing is strictly better
// than a dead grey fill, and since they have no children, no plate can ever
// stack on another.

import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { themeTokens } from "../../lib/colors";
import { formatBytes } from "../../lib/format";
import {
  buildFrame,
  collapseContentlessInteriors,
  fitViewport,
  findWorldRect,
  enterFromForZoomOut,
  zoomOutEnterFrom,
  labelTierFor,
  MAX_INTERIOR_DEPTH,
  projectedAreaPx,
} from "../../lib/treemap/layout";
import type {
  TreemapCanvasProps,
  TreemapHandle,
  TreemapFrameData,
  TileRef,
  Viewport,
  WorldRect,
} from "../../lib/treemap/types";
import type { ChildEntry, PreviewPayload } from "../../lib/types";
import { createTileProgram, INSTANCE_STRIDE, HOVER_LIFT, type TileProgram } from "./gl";
import { createPlate, fillFallbackPlate, fillPlate, type PlateMeta } from "./previewPlate";

/** Nav/zoom animation duration — the FALLBACK for tokens.css `--dur-nav`
 *  (420ms), which loadMotion() reads at runtime (CSS owns the number; this
 *  literal only covers an unparseable token). Clamped to ~0 outright under
 *  prefers-reduced-motion (see reducedMotion + tick/animateTo). */
const ZOOM_MS = 420;
/** Wheel-lerp smoothing factor — 1 (an outright snap) under reduced motion. */
const WHEEL_LERP = 0.18;
/** Duration applied when prefers-reduced-motion is on: one millisecond of
 *  easing is a cut for every practical purpose, while keeping the same code
 *  path (no branch that could skip a viewport invariant check). */
const REDUCED_MS = 0.01;
// Zoom ceiling, relative to the fitted scale. The floor is NOT a constant:
// it is fit().scale itself (see the fill invariant in the header) — zooming
// out past the fitted view would expose background, so it is not allowed.
// MAX_ZOOM is only the ORDINARY ceiling: zoomCeilingAt() raises it (never
// lowers it) for the tile under the gesture point so deep zoom can always
// reach that tile's plate-readable box — see BUG 2.
const MAX_ZOOM = 14;

/** Fallback accent (rgb 0..1) mirroring tokens.css — used only when the CSS
 *  --accent reads unparseable (never for stock tokens). Kept in sync with
 *  the pinned instrument-panel world: warm signal accent, light + dark. */
const ACCENT_FALLBACK: Record<"light" | "dark", [number, number, number]> = {
  light: [0xc9 / 255, 0x46 / 255, 0x2e / 255], // #c9462e
  dark: [0xe8 / 255, 0x65 / 255, 0x4a / 255], // #e8654a
};

/** Fallback --accent-soft alphas (tokens.css): the selected fill-lift step. */
const ACCENT_SOFT_FALLBACK: Record<"light" | "dark", number> = {
  light: 0.12,
  dark: 0.16,
};

/** Fallback stroke-ladder tokens (tokens.css), mirrored only for the case
 *  where getComputedStyle hands back an unparseable declaration (never for
 *  stock tokens): `--hairline` composites over the depth-1/2 stroke band,
 *  `--hairline-strong` over the depth-0 band. rgb + alpha, because the
 *  ladder must composite EXACTLY like CSS over the drawn tile fill. */
const HAIR_FALLBACK: Record<"light" | "dark", { hair: [number, number, number, number]; strong: [number, number, number, number] }> = {
  dark: { hair: [1, 1, 1, 0.075], strong: [1, 1, 1, 0.15] },
  light: { hair: [0, 0, 0, 0.08], strong: [0, 0, 0, 0.16] },
};

/** The ink-2 step: the quietest target for a label's size line (the same
 *  62% alpha tokens.css builds --ink-2 with). Where a tile's fill is bright
 *  the 4.5:1 floor clamps the step back up — see quietAlphaFor(). */
const QUIET_MIN = 0.62;

// Inline category swatch on depth-0 tile labels (two-channel rule).
const SWATCH = 3.5;
const SWATCH_GAP = 4;
const SWATCH_BLOCK = SWATCH + SWATCH_GAP;

// ------------------------------------------------------------- label tiers
// TILE SPEC: depth-0 BIG tile (w ≥ 90 AND h ≥ 36) = Geist 600/11.5 name +
// 3.5px category swatch (dropped when the name stops fitting beside it) +
// mono 500/10.5 size·share line (dropped when it doesn't fit). Every other
// tile = 500/10.5 name only. SELECTED rides 650; HOVER changes only the ink
// alpha (0.95 → 1) — never weight, never a ring beyond the GL fill step.
// The BIG tier is reserved for depth 0: nested interiors can now recurse to
// MAX_INTERIOR_DEPTH, and promoting deep tiles would turn the map into a
// wall of type. The deepest legible tier in the system is 9.5px and there is
// NO tier below 10.5px here: a tile that cannot host the tier at these fixed
// metrics drops its label — type never shrinks to fit, and a label never
// bleeds outside its tile.
const FONT_BIG = "600 11.5px Geist, sans-serif";
const FONT_BIG_SEL = "650 11.5px Geist, sans-serif";
const FONT_SMALL = "500 10.5px Geist, sans-serif";
const FONT_SMALL_SEL = "650 10.5px Geist, sans-serif";
const FONT_META = "500 10.5px 'Geist Mono', monospace";
const BIG_W = 90;
const BIG_H = 36;
/** Label box geometry: 6px inset (LABEL_PAD), one 13px name line, an
 *  optional 12px size·share band. The min-height gate below is exactly
 *  pad + line, so a drawn box always ends inside the tile. */
const LABEL_PAD = 6;
const LABEL_LINE_H = 13;
const LABEL_META_H = 12;

// --------------------------------------------------------- preview plates
// INS_BRIEF §2.0: a pooled layer of DOM plates sits over the GL canvas as a
// pointer-events:none sibling of the label canvas. React renders only the
// empty layer; the draw loop owns everything else (pool, position, content,
// visibility) via imperative writes — the updateInstruments pattern.

/** Pool size (§2.0 "~24"): plates are recycled, never created per frame.
 *  The pool bounds WAITING/claimed continuity; the VISIBILITY of plates is
 *  separately capped by VISIBLE_CAP so the map never runs a plate parade. */
const POOL = 24;

/** Max plates visible on screen simultaneously. Beyond the cap tiles keep
 *  their plain floating label. Priority = the tile's visible screen area,
 *  largest first, with a hysteresis bias for incumbents (INCUMBENT_BIAS) so
 *  a plate is not displaced by a newcomer of nearly equal area from one
 *  frame to the next. The cap rides the POOL exactly: with a pooled node
 *  per candidate there is no reason to withhold content from a tile the
 *  ranking already prefers — the old 8-plate cap is what left a screen of
 *  flat colour boxes carrying three previews. */
const VISIBLE_CAP = POOL;
const INCUMBENT_BIAS = 1.25;

/** Per-family minimum tile size for a plate (§2.0 table). HEX is gated at
 *  the 8-bytes-per-row density: an offset+bytes+ascii row measures ~330px,
 *  so a narrower tile would compress the dump into mush — below a family's
 *  minimum the tile falls back to its floating label only (no plate). DB is
 *  gated with LIST: it renders through the same 22px table geometry. */
const PLATE_MIN: Record<string, readonly [number, number]> = {
  text: [160, 118],
  image: [120, 92],
  list: [180, 120],
  db: [180, 120],
  meta: [120, 92],
  hex: [330, 140],
  media: [240, 120],
};

/** Preview REQUEST floor (FLEET §D/G) — DECOUPLED from plate qualification.
 *  A depth-0 FILE tile asks for its payload as soon as it is on-screen and at
 *  least REQ_MIN_PX on both sides, far below every plate gate: the request is
 *  what resolves the path and reads the bytes, so it must fire while the tile
 *  is still small — otherwise a tile that plates after one more zoom (or on
 *  hover) has no payload ready and shows nothing. Requests are therefore
 *  cap-independent: they fire at fit view as well as deep zoom, and NEVER for
 *  a directory tile (a folder's preview is its nested interior; see pass 1)
 *  nor for a dimmed/filtered tile. Depth ≥ 1 has a larger floor — see
 *  REQ_DEEP_MIN_W/H — because recursive interiors multiply file tiles. Once
 *  per (entryId, nav generation) as the first attempt, then retried on the
 *  backoff below — see reqFirst/reqLast. */
const REQ_MIN_PX = 8;

/** Deep-tile request floor (depth ≥ 1): the smallest plate gate
 *  (image/meta = 120×92). Lazily expanded interiors mean ONE map can hold
 *  thousands of file tiles; asking for every 8px leaf would flood the
 *  backend while the plate pool is capped at POOL = 24. A deep tile too
 *  small to ever plate has no reason to read its bytes. Depth-0 tiles keep
 *  the tiny floor above so a tile growing into a plate after one more zoom
 *  already has its payload warm. */
const REQ_DEEP_MIN_W = 120;
const REQ_DEEP_MIN_H = 92;

/** Interior-expansion request gates. A directory asks the view for its
 *  children only when its PROJECTED tile can visibly subdivide (at least
 *  EXPAND_MIN_AREA_PX CSS px², on-screen and undimmed) and it is shallower
 *  than MAX_INTERIOR_DEPTH. EXPAND_BATCH caps requests per drawn frame so a
 *  first paint cannot fan out into hundreds of IPC round-trips; the dedupe
 *  Set is the progress ledger across frames. A failed fetch is memoised by
 *  the view as an EMPTY array, which `.has()` sees as answered — the loop
 *  stops naturally. */
const EXPAND_MIN_AREA_PX = 7000;
const EXPAND_BATCH = 6;

/** Unanswered-request backoff (pass 1) + in-flight window (pass 3). The
 *  backend queues heavy previews behind a small permit semaphore, so >0.5s
 *  is routine and one silent attempt is not evidence of failure:
 *  - REQ_RETRY_TICKS: drawn frames between attempts (~1.5s at 60fps);
 *  - REQ_MAX_ATTEMPTS: attempts per (entryId, generation) — after this the
 *    tile is honestly unavailable until the next generation;
 *  - REQ_AWAIT_TICKS: while an unanswered request is inside this window it
 *    counts as IN FLIGHT — pass 3 re-arms the fallback clock every frame,
 *    so "no preview" can never be painted over a payload a moment away. */
const REQ_RETRY_TICKS = 90;
const REQ_MAX_ATTEMPTS = 3;
const REQ_AWAIT_TICKS = REQ_RETRY_TICKS * REQ_MAX_ATTEMPTS + 60; // 330 (~5.5s)

/** Plate-READABLE box, in CSS px: the size at which a plate stops being a
 *  cramped stub and starts being an instrument. Deep-zoom headroom
 *  (zoomCeilingAt) opens exactly as far as it takes the tile under the
 *  pointer to reach it — this is a ceiling target, NOT a request gate
 *  (requests ride the depth-sensitive floors above: REQ_MIN_PX at depth 0,
 *  REQ_DEEP_MIN_* deeper). */
const PLATE_TARGET_W = 260;
const PLATE_TARGET_H = 110;

/** A tile still awaiting its payload after this many IN-ACTIVE-WAITING drawn
 *  frames (~0.5s at 60fps) is treated as never-returned and gets the honest
 *  fallback plate. The clock is RE-ARMED every frame while the request is
 *  genuinely in flight (pass 3, reqFirst/reqLast + REQ_AWAIT_TICKS), so a
 *  slow backend queues its permits without this painting "no preview" over
 *  a payload that lands moments later. The grace exists so a fast payload
 *  can never flash "unavailable" first. */
const FALLBACK_GRACE = 30;

/** Plated tiles are PAINTED at their canvas-∩-tile box, not their full
 *  rect: at deep zoom a tile dwarfs the viewport, and a plate sized to the
 *  full tile puts its head/keys/rows off-screen while the viewport stares
 *  at an empty body (the "plates corrupt at extreme zoom" bug class).
 *  Below this visible sliver the plate is not worth a node — the floating
 *  label carries the tile (when it is on-screen at all). */
const PLATE_VISIBLE_MIN = 48;

/** One pooled plate's full imperative state. Fill fields mirror the last
 *  fillPlate/fillFallbackPlate key so content is painted once per material
 *  change — never per frame (FLEET §E: preview content is cached and
 *  invalidated on viewport settle, never rebuilt inside an animation). */
interface PlateSlot {
  el: HTMLElement;
  /** frame entryId bound to this plate; -1 = free in the pool */
  entryId: number;
  /** last-filled key components */
  family: string;
  truncated: boolean;
  totalLines: number;
  fallback: boolean;
  reason: string;
  theme: "light" | "dark";
  /** tile size at fill time — plate content is laid out in pixels (§2.0) */
  fillW: number;
  fillH: number;
  /** last written style values (compare-before-write: idle frames are free) */
  lx: number;
  ly: number;
  lw: number;
  lh: number;
  /** tick when this entry began waiting for its payload (fallback grace) */
  reqTick: number;
  /** claimed during the current updatePreviews pass */
  claimed: boolean;
  /** tiles-array index of the tile this plate belongs to (hit-test owner);
      -1 whenever the plate is not part of the live frame */
  tileIdx: number;
}

// ---------------------------------------------------------- theme palette

interface ThemePalette {
  /** --accent, rgb 0..1 */
  accent: [number, number, number];
  /** --accent-soft alpha: the selected fill-lift strength */
  accentLift: number;
  /** the actual ground visible behind the GL canvas, rgb 0..1 */
  field: [number, number, number];
  /** --hairline (rgb 0..1, alpha): the depth-1 stroke-ladder step */
  hair: [number, number, number, number];
  /** --hairline-strong (rgb 0..1, alpha): the depth-0 stroke step */
  hairStrong: [number, number, number, number];
  /** premade 2D-canvas color strings for the marquee (theme change only) */
  marqueeFill: string;
  marqueeStroke: string;
}

/** Parse a CSS color ("#rrggbb" or rgb/rgba) to rgb 0..1 + alpha, or null.
 *  Custom properties come back as the DECLARED token stream, so the hex and
 *  functional forms tokens.css uses are the only shapes handled; anything
 *  else (color-mix, …) fails the parse and the caller falls back. */
function parseCssColor(v: string): { rgb: [number, number, number]; a: number } | null {
  const s = v.trim();
  if (s.charCodeAt(0) === 0x23 /* '#' */) {
    const hex = s.length === 4 ? s[1] + s[1] + s[2] + s[2] + s[3] + s[3] : s.slice(1);
    if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;
    const n = parseInt(hex, 16);
    return {
      rgb: [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255],
      a: 1,
    };
  }
  const m = /^rgba?\(([^)]+)\)$/i.exec(s);
  if (!m) return null;
  const parts = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
  if (parts.length < 3 || parts.some((n) => Number.isNaN(n))) return null;
  return {
    rgb: [parts[0] / 255, parts[1] / 255, parts[2] / 255],
    a: parts.length >= 4 ? Math.min(1, Math.max(0, parts[3])) : 1,
  };
}

/** The first OPAQUE background behind `start`'s ancestor chain — exactly the
 *  color the clear must match, whatever surface the map panel paints (the
 *  panel owner may move --bg/--bg-elevated; this follows at runtime). */
function resolveOpaqueField(start: HTMLElement | null): [number, number, number] | null {
  for (let n: HTMLElement | null = start; n; n = n.parentElement) {
    const bg = parseCssColor(getComputedStyle(n).backgroundColor);
    if (bg && bg.a >= 0.999) return bg.rgb;
  }
  return null;
}

/** Relative luminance (same coefficients as colors.ts's relLum, which is not
 *  exported — the two implementations must agree by construction). Scalar
 *  form so label drawing stays allocation-free. */
function relLum3(r: number, g: number, b: number): number {
  const f = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** Secondary (size) line step: ink composited over the tile fill at the
 *  QUIETEST alpha that still clears the 4.5:1 floor — the ink-2 step
 *  (QUIET_MIN) where the floor allows, clamped up where the fill is bright
 *  (contrast wins: a quieter color on a saturated fill is a defect, not a
 *  style). Bisect on alpha; is monotonic for both ink polarities. */
function quietAlphaFor(
  fill: [number, number, number],
  ink: [number, number, number],
): number {
  const lFill = relLum3(fill[0], fill[1], fill[2]);
  let lo = 0.3;
  let hi = 1;
  for (let i = 0; i < 10; i++) {
    const a = (lo + hi) * 0.5;
    const l = relLum3(
      ink[0] * a + fill[0] * (1 - a),
      ink[1] * a + fill[1] * (1 - a),
      ink[2] * a + fill[2] * (1 - a),
    );
    const lighter = l > lFill ? l : lFill;
    const darker = l > lFill ? lFill : l;
    if ((lighter + 0.05) / (darker + 0.05) >= 4.5) hi = a;
    else lo = a;
  }
  return Math.max(QUIET_MIN, hi);
}

/** cubic-bezier(0.32, 0.72, 0, 1) — the one curve that moves here. */
function makeEase(x1: number, y1: number, x2: number, y2: number) {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const fx = (t: number) => ((ax * t + bx) * t + cx) * t;
  const dfx = (t: number) => (3 * ax * t + 2 * bx) * t + cx;
  return (x: number) => {
    let t = x;
    for (let i = 0; i < 5; i++) {
      const e = fx(t) - x;
      if (Math.abs(e) < 1e-4) break;
      const d = dfx(t);
      if (Math.abs(d) < 1e-6) break;
      t -= e / d;
    }
    return ((ay * t + by) * t + cy) * t;
  };
}
const ease = makeEase(0.32, 0.72, 0, 1);

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** Scratch buffer for the hover patch (single aHover float, module-level:
 *  written and consumed synchronously setGLHover → bufferSubData). */
const hoverPatch = new Float32Array(1);

/** OPTIONAL props this renderer accepts on top of the shared cross-file
 *  contract (existing props are never renamed or removed):
 *
 *  `pathFor` — an absolute-path resolver for preview requests. The treemap
 *  is path-dumb (ChildEntry/TileRef carry no path), so when the view does
 *  not wire a resolver the request rides "" and the view resolves the real
 *  path itself (its findEntry) — an empty path must therefore be treated as
 *  "unresolved", never fetched verbatim. */
export interface TreemapCanvasExtras {
  pathFor?: (entryId: number) => string | undefined;
}

/** The published request handler shape: TWO arguments, fixed (FLEET §D/G).
 *  Read through this alias so the call compiles whether types.ts currently
 *  declares the 1-arg or the 2-arg form (the fleet is landing the 2-arg
 *  contract); the renderer itself is path-dumb and always passes the path it
 *  resolved through `pathFor`, "" meaning "unresolved — resolve it yourself"
 *  (the view's findEntry does). */
type NeedPreviewFn = (entryId: number, path: string) => void;

/** Plate metadata for a tile (final plate contract §2.4): everything the
 *  renderer knows about a tile WITHOUT a payload, so a card can always be
 *  built — name, type (the plate parses the extension from `name`),
 *  category, hero size and the share-of-parent denominator. Built only at
 *  FILL time (never per frame), and passed to every fill so a null payload
 *  degrades to a real META card instead of an empty sheet. */
function plateMetaOf(ref: TileRef, parentSize: number): PlateMeta {
  return {
    name: ref.name,
    size: ref.size,
    isDir: ref.isDir,
    cat: ref.cat,
    parentSize,
  };
}

export const TreemapCanvas = forwardRef<TreemapHandle, TreemapCanvasProps & TreemapCanvasExtras>(
  function TreemapCanvas(props, ref) {
    const wrapRef = useRef<HTMLDivElement>(null);
    const glCanvasRef = useRef<HTMLCanvasElement>(null);
    const labelRef = useRef<HTMLCanvasElement>(null);
    // preview plate layer: static div rendered empty by React; plates are
    // pooled/appended imperatively in the mount effect
    const plateLayerRef = useRef<HTMLDivElement>(null);
    // instruments overlay: written imperatively from the draw loop
    const zoomRef = useRef<HTMLDivElement>(null);
    const zoomText = useRef("");
    const zoomHidden = useRef(true);

    // latest props for the imperative draw loop
    const p = useRef(props);
    p.current = props;

    const cssW = useRef(0);
    const cssH = useRef(0);
    const dpr = useRef(1);

    const prog = useRef<TileProgram | null>(null);
    const glCtx = useRef<WebGL2RenderingContext | null>(null);
    const instances = useRef<Float32Array>(new Float32Array(0));
    /** instance count + frame identity of the data currently IN the GL
     *  buffer — the hover patch path validates against these so it can
     *  never write into a stale or differently-laid-out buffer. */
    const instCount = useRef(0);
    const instFrame = useRef<ReturnType<typeof buildFrame> | null>(null);

    // CSS-read theme palette (see header) — initialized light so the very
    // first draw never sees an empty palette; loadPalette() rewrites it.
    const palette = useRef<ThemePalette>({
      accent: ACCENT_FALLBACK.light,
      accentLift: ACCENT_SOFT_FALLBACK.light,
      field: themeTokens("light").canvasBg,
      hair: HAIR_FALLBACK.light.hair,
      hairStrong: HAIR_FALLBACK.light.strong,
      marqueeFill: "rgba(201,70,46,0.12)",
      marqueeStroke: "rgba(201,70,46,0.8)",
    });

    // the frame is owned here: rebuilt whenever the measured aspect or the
    // entries change, always at draw-time-fresh geometry
    const frame = useRef<ReturnType<typeof buildFrame> | null>(null);
    const builtFor = useRef<{
      entries: ChildEntry[];
      parentSize: number;
      theme: string;
      aspect: number;
      expanded: ReadonlyMap<number, ChildEntry[]> | undefined;
    } | null>(null);
    const pendingNav = useRef<{ dir: "in" | "out"; rect?: WorldRect } | null>(null);

    const vp = useRef<Viewport>({ tx: 0, ty: 0, scale: 1 });
    const anim = useRef<{ from: Viewport; to: Viewport; t0: number } | null>(null);
    /** Smooth wheel-zoom target: each wheel event nudges this; the rAF loop
     *  lerps vp.current toward it (continuous, fractional, cursor-anchored). */
    const wheelTarget = useRef<Viewport | null>(null);
    const raf = useRef(0);

    const sel = useRef<Set<number>>(new Set());
    const marqueeSel = useRef<Set<number>>(new Set());
    const hover = useRef(-1);
    /** Last (x, y, idx) handed to the parent — dedupes every emission path so
     *  an unchanged hover re-renders the tooltip for nothing. Mutated in
     *  place (no per-press allocation); NaN/−2 never matches a real resolve. */
    const hoverLast = useRef({ x: NaN, y: NaN, idx: -2 });
    /** Pointer position awaiting rAF resolution — mutated in place by
     *  pointermove so a 120Hz+ mouse allocates nothing between frames. */
    const hoverPending = useRef<{ x: number; y: number } | null>(null);
    const hoverRaf = useRef(0);

    const drag = useRef<{
      mode: "pending" | "pan" | "marquee";
      sx: number;
      sy: number;
      px: number;
      py: number;
      idx: number;
      shift: boolean;
      rect: Rect | null;
    } | null>(null);

    /** measureText cache, keyed font → text (two levels, so a HIT allocates
     *  nothing: no `${font}|${text}` key string is ever built per frame —
     *  the renderer contract forbids per-frame allocation). */
    const mcache = useRef<Map<string, Map<string, number>>>(new Map());
    /** fitText result cache, keyed font → text → floor(available width).
     *  The floor keeps a cached truncation VALID at every larger width it
     *  could later be used at (measured ≤ bucket ≤ actual), so the cache can
     *  never overshoot the tile; sizes are CSS px, so the bucket is exact. */
    const fitCache = useRef<Map<string, Map<string, Map<number, string>>>>(new Map());
    /** quantized-rgb → "rgb(...)" string cache: fillStyle assignments must
     *  be strings, but building them per label per frame allocates — keyed
     *  by the packed 8-bit triplet instead. */
    const rgbStrCache = useRef<Map<number, string>>(new Map());
    /** largest-first label order + one world-area score per tile (parallel
     *  scratch, grown on rebuild only); the comparator reads these, so the
     *  sort itself is allocation-free every frame. */
    const orderBuf = useRef<Int32Array>(new Int32Array(0));
    const scoreBuf = useRef<Float32Array>(new Float32Array(0));
    /** depth-0 size·share strings keyed by entryId, invalidated by frame
     *  identity — formatTileMeta() allocates, steady-state frames don't. */
    const metaCache = useRef<Map<number, string>>(new Map());
    const metaCacheFrame = useRef<unknown>(null);

    // --------------------------------------------------- preview plate state
    const slots = useRef<PlateSlot[]>([]);
    /** entryId → its plate slot, for continuity across frames (Map.get only
     *  in the hot path; maintained on claim/unclaim) */
    const slotOf = useRef<Map<number, PlateSlot>>(new Map());
    /** entryIds of DIRECTORY tiles that actually render a child interior in
     *  the current frame. Those folders keep the "interior is the preview"
     *  law (no plate may cover their children). A directory absent from this
     *  set has no interior to show — depth cap, cap-truncated tail, empty or
     *  unreadable expansion — so it is a LEAF FOLDER and may earn a folder
     *  content card instead of staying a dead grey box. Reused Set, filled
     *  per preview pass; no per-frame allocation. */
    const interiorParents = useRef<Set<number>>(new Set());
    /** per frame-index flag: 1 = a VISIBLE plate covers this tile, so its
     *  floating map label is suppressed (the plate's head IS the label) */
    const plated = useRef<Uint8Array>(new Uint8Array(0));
    /** per frame-index flag: 1 = this tile holds one of the VISIBLE_CAP slots
     *  this frame (consumed by updatePreviews' claim pass only) */
    const candVis = useRef<Uint8Array>(new Uint8Array(0));
    /** qualifying-candidate store, preallocated on rebuild (typed arrays:
     *  tile index, ranking score, and a sortable order view) */
    const candTile = useRef<Int32Array>(new Int32Array(0));
    const candScore = useRef<Float64Array>(new Float64Array(0));
    const candOrd = useRef<Int32Array>(new Int32Array(0));
    /** Preview request bookkeeping, per nav generation (cleared on frame
     *  rebuild — the generation boundary). reqFirst: the tick of an entry's
     *  FIRST attempt (also the "already asked" guard, so the once-per-
     *  entryId-per-generation law holds); reqLast: the tick of the most
     *  recent attempt, driving the retry backoff. A payload ARRIVING is the
     *  only success signal the renderer sees — absence past REQ_AWAIT_TICKS
     *  is what makes a tile honestly unavailable. */
    const reqFirst = useRef<Map<number, number>>(new Map());
    const reqLast = useRef<Map<number, number>>(new Map());
    /** entryIds whose INTERIOR EXPANSION was requested for the current
     *  `entries` identity (navigation). Deliberately NOT cleared by an
     *  `expanded` arrival: the answer (children or the parent's memoised
     *  empty array) is here, and re-asking would loop. Cleared by the
     *  entries effect below. */
    const expandAsked = useRef<Set<number>>(new Set());
    /** monotonic drawn-frame counter: the fallback grace clock */
    const ticks = useRef(0);
    /** prefers-reduced-motion, kept live by a MediaQueryList listener at
     *  mount — never queried per frame. The CSS layer clamps its own
     *  transitions globally; this clamps the JS viewport animations (drill/
     *  zoom 420ms → 0.01ms, wheel lerp → snap) to the same law. */
    const reducedMotion = useRef(false);
    /** --dur-nav in ms, read once per theme load (loadMotion). CSS owns the
     *  number; ZOOM_MS only covers an unparseable token. */
    const durNav = useRef(ZOOM_MS);
    /** one-shot latch: a draw-loop exception is reported once, then the loop
     *  stops — no overlay spam, no silent loop death */
    const loopDead = useRef(false);

    // label collision store, preallocated on rebuild: placed boxes as
    // (x, y, w, h) quads + a live count, replacing per-frame Rect churn
    const placedBuf = useRef<Float32Array>(new Float32Array(0));
    const placedN = useRef(0);

    const fit = () => {
      const f = frame.current;
      if (!f) return { tx: 0, ty: 0, scale: 1 };
      return fitViewport(f.world, cssW.current, cssH.current);
    };

    // ------------------------------------------------- fill invariant
    //
    // THE INVARIANT: the world always fills the viewport; zoom is in-only.
    // fit().scale is the floor, and these helpers are the only thing
    // allowed to move a viewport that has slipped under it.

    /** Raise `v` to the fit floor, keeping the world point that was under
     *  the canvas centre under the canvas centre — a lift reads as a zoom,
     *  not a jump, so nav-transition focus survives it. */
    const liftToFloor = (v: Viewport): Viewport => {
      if (!frame.current) return v;
      const floor = fit().scale;
      if (!(v.scale > 0)) return fit(); // degenerate: snap to the fill view
      if (v.scale >= floor) return v;
      const k = floor / v.scale;
      return {
        scale: floor,
        tx: cssW.current / 2 - (cssW.current / 2 - v.tx) * k,
        ty: cssH.current / 2 - (cssH.current / 2 - v.ty) * k,
      };
    };

    /** Pin the world so it covers the canvas on both axes — the clamp that
     *  makes panning at fit a literal no-op (world width == canvas width =>
     *  the only legal tx is 0). Where the world is at least canvas-sized the
     *  translation is clamped into the range that keeps both edges past the
     *  canvas edges; where it is marginally thinner (the sub-pixel aspect
     *  drift the rebuild guard tolerates) it is centred — the same tx
     *  fitViewport() picks, so this never disagrees with the fitted view.
     *
     *  This is deliberately STRICTER than the "keep 120px of world on each
     *  axis" suggestion: any clamp short of full coverage still permits up to
     *  (dimension − 120px) of background, and the requirement is that the
     *  user can NEVER see background. Coverage is also the tightest clamp
     *  that never fights the user — at fit it is a no-op, zoomed in it only
     *  bites at the exact frame where the world's edge reaches the canvas
     *  edge. */
    const coverToCanvas = (v: Viewport): Viewport => {
      const f = frame.current;
      if (!f || !(v.scale > 0)) return v;
      const w = f.world.w * v.scale;
      const h = f.world.h * v.scale;
      const cx = cssW.current;
      const cy = cssH.current;
      const tx = w >= cx ? Math.min(0, Math.max(cx - w, v.tx)) : (cx - w) / 2;
      const ty = h >= cy ? Math.min(0, Math.max(cy - h, v.ty)) : (cy - h) / 2;
      return tx === v.tx && ty === v.ty ? v : { scale: v.scale, tx, ty };
    };

    /** The invariant, applied: floor first, coverage second (coverage wins
     *  any conflict, because a gap is the one thing that is forbidden).
     *  Idempotent, so it is safe to run on every path that touches vp. */
    const enforceFill = (v: Viewport): Viewport => coverToCanvas(liftToFloor(v));

    /** Enforce on the live viewport AND a live animation's endpoints — a
     *  resize can move the floor out from under an in-flight animation, and
     *  an interpolated frame between two legal endpoints is legal only if
     *  both endpoints were re-checked against the current floor. */
    const enforceLive = () => {
      vp.current = enforceFill(vp.current);
      const a = anim.current;
      if (a) {
        a.from = enforceFill(a.from);
        a.to = enforceFill(a.to);
      }
    };

    /** Zoom ceiling for a gesture about canvas point (x, y) — BUG 2 (deep-zoom
     *  headroom). Normally MAX_ZOOM above the fit floor, but RAISED to
     *  whatever it takes for the tile under the point to both frame the view
     *  and reach the PLATE-READABLE box (PLATE_TARGET_W×H): 14×fit cannot get
     *  a tile under ~2% of the view to a comfortable plate, so it was
     *  unreachable by wheel/keyboard — while "Jump to" (zoomToTile) could
     *  already take any tile to its fill scale. The ceiling follows the
     *  point, so it climbs as the cursor moves from a directory to its
     *  sub-tiles, and it is never below the live scale: a gesture can sit AT
     *  the ceiling but can never be snapped down to it. (Requests themselves
     *  need only their request floor — REQ_MIN_PX at depth 0, the smallest
     *  plate gate deeper — so this is about reaching a plate that READS, not
     *  about reaching the request gate.) */
    const zoomCeilingAt = (x: number, y: number): number => {
      const base = fit().scale;
      let ceil = base * MAX_ZOOM;
      const f = frame.current;
      if (f) {
        const i = hitTest(x, y);
        if (i >= 0) {
          const t = f.tiles[i];
          if (t.w > 0 && t.h > 0) {
            // tile frames the canvas (same quantity zoomToTile aims at)
            const fillScale = Math.min(cssW.current / t.w, cssH.current / t.h);
            // smallest scale at which the tile's PLATE BOX reaches the
            // plate-readable target (the box is the tile inset 4px per side,
            // so the full tile must clear the target + the 8px inset; the
            // canvas clamp caps it at the canvas dimension — a tile covering
            // the canvas can't qualify for more than the canvas offers)
            const gateScale = Math.max(
              Math.min(PLATE_TARGET_W + 8, cssW.current) / t.w,
              Math.min(PLATE_TARGET_H + 8, cssH.current) / t.h,
            );
            if (fillScale > ceil) ceil = fillScale;
            if (gateScale > ceil) ceil = gateScale;
          }
        }
      }
      return Math.max(ceil, vp.current.scale);
    };

    /** Tile under a canvas point, topmost first (reverse draw order). Fully
     *  scalar: the draw loop and every pointer event ride this, so it must
     *  not allocate. */
    const hitTest = (x: number, y: number): number => {
      const f = frame.current;
      if (!f) return -1;
      // Preview plates paint OVER the tile plane, so a pointer inside a
      // plate's painted box belongs to that plate's OWNER tile — never to
      // another tile hidden underneath it. Without this mask the listing's
      // rows read as one item while the hover card names a different tile
      // that the paper actually covers (identity must match what the pointer
      // sees, not what the layout knows).
      const pool = slots.current;
      for (let s = 0; s < pool.length; s++) {
        const slot = pool[s];
        if (!slot.claimed || slot.tileIdx < 0 || slot.el.hidden) continue;
        if (x >= slot.lx && x < slot.lx + slot.lw - 8 && y >= slot.ly && y < slot.ly + slot.lh - 8) {
          return slot.tileIdx;
        }
      }
      const sc = vp.current.scale;
      const tx0 = vp.current.tx;
      const ty0 = vp.current.ty;
      for (let i = f.tiles.length - 1; i >= 0; i--) {
        const t = f.tiles[i];
        const rx = tx0 + t.x * sc;
        const ry = ty0 + t.y * sc;
        if (x >= rx && x < rx + t.w * sc && y >= ry && y < ry + t.h * sc) return i;
      }
      return -1;
    };

    // ---------------------------------------------------------- palette

    /** Re-read the CSS-owned colors (once per theme change, never per
     *  frame) and scheduling a repaint so the GL uniforms pick them up. */
    const loadPalette = () => {
      const theme = p.current.theme;
      const cs = getComputedStyle(document.documentElement);
      const accent =
        parseCssColor(cs.getPropertyValue("--accent"))?.rgb ?? ACCENT_FALLBACK[theme];
      const accentLift =
        parseCssColor(cs.getPropertyValue("--accent-soft"))?.a ?? ACCENT_SOFT_FALLBACK[theme];
      const field =
        resolveOpaqueField(wrapRef.current) ?? themeTokens(theme).canvasBg;
      // Stroke ladder (gl.ts): both hairline tokens, rgb + alpha, composited
      // by the shader exactly as CSS would over each drawn fill.
      const hairCss = parseCssColor(cs.getPropertyValue("--hairline"));
      const strongCss = parseCssColor(cs.getPropertyValue("--hairline-strong"));
      const fb = HAIR_FALLBACK[theme];
      const hair: [number, number, number, number] = hairCss
        ? [hairCss.rgb[0], hairCss.rgb[1], hairCss.rgb[2], hairCss.a]
        : fb.hair;
      const hairStrong: [number, number, number, number] = strongCss
        ? [strongCss.rgb[0], strongCss.rgb[1], strongCss.rgb[2], strongCss.a]
        : fb.strong;
      const r = (accent[0] * 255) | 0;
      const g = (accent[1] * 255) | 0;
      const b = (accent[2] * 255) | 0;
      // Motion token (tokens.css --dur-nav): CSS owns the nav duration; the
      // JS viewport animations read the same number so a token change moves
      // both. A value that doesn't parse (missing token, rem, calc) keeps
      // the ZOOM_MS fallback — never a NaN duration.
      const durRaw = cs.getPropertyValue("--dur-nav").trim();
      const durMs = durRaw.endsWith("ms") ? parseFloat(durRaw) : parseFloat(durRaw) * 1000;
      if (Number.isFinite(durMs) && durMs > 0) durNav.current = durMs;
      palette.current = {
        accent,
        accentLift,
        field,
        hair,
        hairStrong,
        // marquee: accent-soft fill + a strong stroke, premade for the 2D canvas
        marqueeFill: theme === "dark" ? `rgba(${r},${g},${b},0.16)` : `rgba(${r},${g},${b},0.12)`,
        marqueeStroke: `rgba(${r},${g},${b},0.8)`,
      };
      kick();
    };

    // ------------------------------------------------------------ hover

    /** Drop a pointer position waiting for rAF resolution — used wherever a
     *  synchronous hover clear happens (leave, gesture commit, rebuild), so
     *  a stale pending point can never re-resolve against cleared state. */
    const cancelPendingHover = () => {
      if (hoverRaf.current) {
        cancelAnimationFrame(hoverRaf.current);
        hoverRaf.current = 0;
      }
      hoverPending.current = null;
    };

    /** Resolve a pending pointer position inside rAF (processHover). */
    const processHover = () => {
      hoverRaf.current = 0;
      const pt = hoverPending.current;
      hoverPending.current = null;
      if (loopDead.current || !pt) return;
      resolveHover(pt.x, pt.y, true);
    };

    /** Pointer positions enter here (one per pointermove max, mutated in
     *  place); resolution happens at most once per frame. */
    const scheduleHover = (x: number, y: number) => {
      const pt = hoverPending.current;
      if (pt) {
        pt.x = x;
        pt.y = y;
      } else {
        hoverPending.current = { x, y };
      }
      if (!hoverRaf.current) hoverRaf.current = requestAnimationFrame(processHover);
    };

    /** Patch one tile's aHover float IN PLACE in the instance buffer (4
     *  bytes) — the cheap partial GL update behind the hover pipeline. The
     *  patch is valid only while the buffer holds the live frame
     *  (instFrame) and the index is inside what was actually uploaded. */
    const setGLHover = (i: number, on: boolean) => {
      const gl = glCtx.current;
      const program = prog.current;
      const f = frame.current;
      if (!gl || !program || !f) return;
      // Crash-class guard: a raw frame index never reaches the buffer out
      // of range (FLEET §G discipline — hover is a raw frame index).
      if (i < 0 || i >= f.refs.length || i >= instCount.current) return;
      if (instFrame.current !== f) return; // impossible here, but never patch stale data
      // Same predicate writeInstances uses: a dimmed (filtered) tile has
      // no hover lift.
      hoverPatch[0] = on && !dimmed(i) ? 1 : 0;
      gl.bindBuffer(gl.ARRAY_BUFFER, program.buffer);
      gl.bufferSubData(gl.ARRAY_BUFFER, i * INSTANCE_STRIDE + 11 * 4, hoverPatch);
    };

    /** Resolve the hover at a canvas point and tell the parent — the ONE
     *  hover write path (idle move via scheduleHover, wheel, gesture end,
     *  animation end). A hover is only ever valid for the point it was
     *  resolved at, so every gesture that moves content under the cursor
     *  (pan, wheel) or moves the cursor under content (gesture end) must
     *  come through here: otherwise the tooltip keeps naming a tile that
     *  has since left the cursor and floats over bare canvas (BUG 3).
     *  Empty point → null; an identical repeat → the parent is not called
     *  at all.
     *
     *  `paintLight` is true when no full draw follows this call (the rAF
     *  scheduler path): the repaint is the PARTIAL path — two 4-byte aHover
     *  patches + a re-issued instanced draw + a 2D label redraw, and never
     *  a re-instance of GL buffers. Call sites that are followed by a full
     *  draw in the same tick (animation end inside the rAF tick) pass
     *  false and let the normal pipeline pick up hover.current. */
    const resolveHover = (x: number, y: number, paintLight: boolean) => {
      const idx = hitTest(x, y);
      if (idx !== hover.current) {
        const prev = hover.current;
        hover.current = idx;
        if (paintLight) {
          setGLHover(prev, false);
          setGLHover(idx, true);
          drawGL(false);
          drawLabels();
        }
      }
      const last = hoverLast.current;
      if (last.idx === idx && last.x === x && last.y === y) return;
      last.x = x;
      last.y = y;
      last.idx = idx;
      const f = frame.current;
      const ref = idx >= 0 && f ? f.refs[idx] : null;
      if (ref) {
        p.current.onHoverChange({
          entryId: ref.entryId,
          cat: ref.cat,
          name: ref.name,
          size: ref.size,
          isDir: ref.isDir,
          x,
          y,
        });
      } else {
        p.current.onHoverChange(null);
      }
    };

    const dimmed = (i: number): boolean => {
      const f = frame.current;
      if (!f) return false;
      const ref = f.refs[i];
      // Defense-in-depth: a raw frame index can outlive a rebuild by one
      // frame (the call sites are bounds-checked too — see drawLabels).
      if (!ref) return false;
      if (p.current.hiddenCats.has(ref.cat)) return true;
      if (p.current.highlightIds && !p.current.highlightIds.has(ref.entryId)) return true;
      return false;
    };

    // ------------------------------------------------------------ frame

    const ensureFrame = () => {
      const { entries, parentSize, theme, expanded } = p.current;
      if (cssW.current === 0 || cssH.current === 0 || entries.length === 0) {
        frame.current = null;
        builtFor.current = null;
        return;
      }
      const aspect = cssW.current / cssH.current;
      const prev = builtFor.current;
      // Rebuild when the frame's identity changed OR when the fitted view
      // would drift from the canvas: `drift` is exactly how many px the
      // COVERED world overflows the canvas on either axis — 0 at matching
      // aspect (where cover is an exact edge-to-edge fill), > 0 the moment
      // the aspect drifts. Rebuilding at the container's fresh aspect is
      // what keeps the layout true to the viewport; tolerating half a pixel
      // keeps tiles from reflowing on sub-pixel jitter (below that it is
      // invisible fp noise). Watch the sign: under cover the old contain-
      // style gap (css − world*scale) is always ≤ 0, so it could never fire
      // and a stale-aspect frame would survive every draw.
      const pf = frame.current;
      let drift = Number.POSITIVE_INFINITY;
      if (pf) {
        const s = fitViewport(pf.world, cssW.current, cssH.current).scale;
        drift = Math.max(
          pf.world.w * s - cssW.current,
          pf.world.h * s - cssH.current,
        );
      }
      if (
        prev &&
        prev.entries === entries &&
        prev.parentSize === parentSize &&
        prev.theme === theme &&
        prev.expanded === expanded &&
        drift <= 0.5
      ) {
        // Frame survived the (sub-threshold) resize — but the VIEWPORT did
        // not: vp still carries tx/ty/scale from the previous canvas metrics,
        // which can sit below the new floor (a stale fitted view from before
        // an aspect-preserving resize floats inset with dead margins).
        enforceLive();
        return;
      }
      // "Same level" = every identity term except `expanded` still matches
      // and the canvas did not resize: this rebuild is an interior ARRIVAL,
      // not a navigation. Its geometry lands inside the SAME world (world
      // dims derive from aspect only), so the live viewport stays valid and
      // must be preserved — refitting on every lazy expansion would snap a
      // zoomed-in user back to 100% several times a second.
      const sameLevel =
        prev !== null &&
        prev.entries === entries &&
        prev.parentSize === parentSize &&
        prev.theme === theme &&
        drift <= 0.5;
      const contentChanged = !prev || prev.entries !== entries;
      const old = frame.current;
      const raw = buildFrame(entries, parentSize, aspect, theme, expanded);
      // Contentless system folders (Windows, System32, ...) otherwise paint a
      // field of hatching once their subfolders are too small to card. Collapse
      // only the highest branch with no plate-worthy tile below it; a branch
      // that owns a real card (Chrome, an app folder, a large file) stays a map.
      frame.current = collapseContentlessInteriors(raw, {
        scale: fitViewport(raw.world, cssW.current, cssH.current).scale,
        viewportW: cssW.current,
        viewportH: cssH.current,
      });
      builtFor.current = {
        entries,
        parentSize,
        theme,
        aspect,
        expanded,
      };
      // A rebuild re-baselines every raw frame index: hover and the armed
      // drag index still point into the OLD refs — leaving them live is
      // exactly how dimmed()/refs[i] threw (stale index, one frame late).
      // Mirror pointerleave: clear the highlight AND tell the parent.
      if (hover.current >= 0) {
        hover.current = -1;
        const hl = hoverLast.current;
        hl.x = NaN;
        hl.y = NaN;
        hl.idx = -2;
        p.current.onHoverChange(null);
      }
      if (drag.current) drag.current.idx = -1;
      // A schedule already queued for the pre-rebuild point would resolve
      // against the NEW frame one frame from now — drop it with the rest
      // of the hover state.
      cancelPendingHover();
      // A rebuild re-baselines the preview request generation too: the parent
      // guards its fetches with a nav token, and the renderer re-asks for
      // whatever qualifies on the new frame (once per entry per generation,
      // retried on the backoff — see REQ_RETRY_TICKS). Exception: an
      // INTERIOR ARRIVAL stays inside the SAME navigation generation, so the
      // attempt clocks must survive it — clearing them on every lazy
      // expansion would re-ask every queued-out preview in the map. New
      // child entryIds are absent from the maps anyway and ask normally.
      if (!sameLevel) {
        reqFirst.current.clear();
        reqLast.current.clear();
      }
      // …and it re-baselines the plate → tile bindings that hitTest trusts.
      // Until updatePreviews re-claims them (same tick on the draw path, one
      // frame later on a resize measure), a pointer event must resolve
      // against the NEW frame's tiles — never a plate slot still naming an
      // index of the frame that was just replaced (the "hover resolves to
      // the wrong owner" bug class). The plates stay PAINTED; only the
      // identity bridge is dropped.
      for (const s of slots.current) {
        s.claimed = false;
        s.tileIdx = -1;
      }
      const nav = pendingNav.current;
      // Only a CONTENT SWAP consumes an armed nav transition: a resize/theme
      // rebuild landing between beginNav() and the new entries arriving must
      // leave it armed for the entries that are actually coming — otherwise
      // the drill-in would play against a rect of the wrong frame and the
      // real swap would land as a bare cut.
      if (nav && contentChanged) {
        pendingNav.current = null;
        vp.current =
          nav.dir === "in" && nav.rect && old
            ? enterFromForZoomOut(nav.rect, cssW.current, cssH.current)
            : zoomOutEnterFrom(old?.world ?? frame.current.world, cssW.current, cssH.current);
        // Lift the departure viewport to the floor BEFORE animateTo
        // snapshots it as `from` (a drill-in from a large tile can compute
        // under the floor when the window resized under the old frame).
        vp.current = enforceFill(vp.current);
        animateTo(fit());
      } else if (sameLevel) {
        // Interior arrival inside the same level: keep the user's viewport
        // (and any in-flight animation) — only re-check it against the
        // fill invariant, whose world is unchanged.
        enforceLive();
      } else {
        anim.current = null;
        wheelTarget.current = null;
        vp.current = fit();
        enforceLive();
      }
    };

    // -------------------------------------------------------- GL draw

    const writeInstances = () => {
      const f = frame.current;
      if (!f) return 0;
      const n = f.tiles.length;
      if (instances.current.length < n * (INSTANCE_STRIDE / 4)) {
        instances.current = new Float32Array(n * (INSTANCE_STRIDE / 4) + 256);
      }
      const buf = instances.current;
      let o = 0;
      for (let i = 0; i < n; i++) {
        const t = f.tiles[i];
        const r = f.refs[i];
        buf[o++] = t.x + t.w / 2;
        buf[o++] = t.y + t.h / 2;
        buf[o++] = t.w;
        buf[o++] = t.h;
        buf[o++] = t.fill[0];
        buf[o++] = t.fill[1];
        buf[o++] = t.fill[2];
        // Hidden (filtered) tiles hold their place at a legible-silent
        // alpha: 0.05 on the dark field read as a dead hole — the eye saw
        // nothing where the layout said "data lives here". 0.16 keeps the
        // cat tint faintly readable (a switched-off cell, not a void) and
        // survives the light theme's sunken panel the same way.
        buf[o++] = dimmed(i) ? 0.16 : 1;
        buf[o++] = sel.current.has(r.entryId) || marqueeSel.current.has(r.entryId) ? 1 : 0;
        buf[o++] = t.hatch ? 1 : 0;
        buf[o++] = t.depth;
        buf[o++] = !dimmed(i) && hover.current === i ? 1 : 0;
      }
      return n;
    };

    /** Draw the GL field. `forceFull` re-uploads every instance (the normal
     *  per-frame path); false rides the hover light path and re-issues the
     *  draw against the already-uploaded buffer — no re-instance, no upload.
     *  A frame-identity mismatch (rebuild/resize/first draw) escalates to a
     *  full upload automatically, so the patch can never desync. */
    const drawGL = (forceFull: boolean) => {
      const gl = glCtx.current;
      const program = prog.current;
      if (!gl || !program) return;
      const pal = palette.current;
      gl.viewport(0, 0, gl.canvas.width, gl.canvas.height);
      // Field = the panel ground behind the canvas (CSS-resolved) — the
      // first thing the fill invariant shows through any gap.
      gl.clearColor(pal.field[0], pal.field[1], pal.field[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      const f = frame.current;
      if (forceFull || instFrame.current !== f) {
        instFrame.current = f;
        const count = writeInstances();
        instCount.current = count;
        gl.bindBuffer(gl.ARRAY_BUFFER, program.buffer);
        gl.bufferData(
          gl.ARRAY_BUFFER,
          instances.current.subarray(0, count * (INSTANCE_STRIDE / 4)),
          gl.DYNAMIC_DRAW,
        );
      }
      if (instCount.current === 0 || !f) return;
      gl.useProgram(program.program);
      gl.bindVertexArray(program.vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, program.buffer); // patch writes rebind it
      const u = program.uniforms;
      gl.uniform1f(u.uTotalScale, vp.current.scale);
      gl.uniform1f(u.uTx, vp.current.tx);
      gl.uniform1f(u.uTy, vp.current.ty);
      gl.uniform1f(u.uDpr, dpr.current);
      gl.uniform1f(u.uCssW, cssW.current);
      gl.uniform1f(u.uCssH, cssH.current);
      // Stroke ladder tokens (gl.ts): composited by the shader per depth.
      gl.uniform4f(u.uHair, pal.hair[0], pal.hair[1], pal.hair[2], pal.hair[3]);
      gl.uniform4f(u.uHairStrong, pal.hairStrong[0], pal.hairStrong[1], pal.hairStrong[2], pal.hairStrong[3]);
      gl.uniform3f(u.uAccent, pal.accent[0], pal.accent[1], pal.accent[2]);
      gl.uniform1f(u.uAccentLift, pal.accentLift);
      gl.uniform1f(u.uHoverLift, HOVER_LIFT);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instCount.current);
    };

    // -------------------------------------------------------- labels

    const inkString = (c: [number, number, number]): string => {
      const key = (((c[0] * 255) | 0) << 16) | (((c[1] * 255) | 0) << 8) | ((c[2] * 255) | 0);
      let s = rgbStrCache.current.get(key);
      if (s === undefined) {
        s = `rgb(${(c[0] * 255) | 0},${(c[1] * 255) | 0},${(c[2] * 255) | 0})`;
        if (rgbStrCache.current.size > 512) rgbStrCache.current.clear();
        rgbStrCache.current.set(key, s);
      }
      return s;
    };

    const measure = (ctx: CanvasRenderingContext2D, text: string, font: string): number => {
      // Two levels (font → text): a HIT builds no key string, so measuring
      // a stable label set allocates nothing per frame.
      let fm = mcache.current.get(font);
      if (fm === undefined) {
        fm = new Map();
        mcache.current.set(font, fm);
      }
      const hit = fm.get(text);
      if (hit !== undefined) return hit;
      ctx.font = font;
      const w = ctx.measureText(text).width;
      if (fm.size > 4000) fm.clear();
      fm.set(text, w);
      return w;
    };

    const fitText = (
      ctx: CanvasRenderingContext2D,
      text: string,
      font: string,
      maxW: number,
    ): string => {
      if (measure(ctx, text, font) <= maxW) return text;
      // Truncate against floor(maxW): a cached result measured at the floor
      // stays valid for every width ≥ it (measured ≤ bucket ≤ actual), so
      // the steady-state frame hits the cache and allocates nothing — the
      // bisect below (and its slice/ellipsis strings) runs once per
      // (font, text, bucket) ever.
      const bucket = Math.max(1, Math.floor(maxW));
      let fm = fitCache.current.get(font);
      if (fm === undefined) {
        fm = new Map();
        fitCache.current.set(font, fm);
      }
      let tm = fm.get(text);
      if (tm === undefined) {
        tm = new Map();
        fm.set(text, tm);
      }
      const hit = tm.get(bucket);
      if (hit !== undefined) return hit;
      let lo = 1;
      let hi = text.length;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        const cand = `${text.slice(0, mid)}…`;
        if (measure(ctx, cand, font) <= bucket) lo = mid;
        else hi = mid - 1;
      }
      const out = lo <= 1 ? "" : `${text.slice(0, lo)}…`;
      if (tm.size > 64) tm.clear();
      tm.set(bucket, out);
      return out;
    };

    /** depth-0 size·share line, memoized per frame: formatTileMeta() builds
     *  fresh strings, and the draw path is not allowed to. The cache is
     *  keyed by entryId and dropped whenever the frame identity changes
     *  (the share denominator moves with it). */
    const tileMeta = (
      f: TreemapFrameData,
      ref: TileRef,
      total: number,
    ): string => {
      if (metaCacheFrame.current !== f) {
        metaCacheFrame.current = f;
        metaCache.current.clear();
      }
      const hit = metaCache.current.get(ref.entryId);
      if (hit !== undefined) return hit;
      const s = formatTileMeta(ref.size, total);
      if (metaCache.current.size > 2000) metaCache.current.clear();
      metaCache.current.set(ref.entryId, s);
      return s;
    };

    /** Overlap against the preallocated placed-label store — absolute rule:
     *  no label is ever drawn over another. Component scope (not the frame
     *  closure): the per-frame draw path must not allocate. */
    const placedOverlaps = (x: number, y: number, w: number, h: number): boolean => {
      const buf = placedBuf.current;
      for (let k = 0, n = placedN.current; k < n; k++) {
        const o = k * 4;
        if (
          x < buf[o] + buf[o + 2] &&
          buf[o] < x + w &&
          y < buf[o + 1] + buf[o + 3] &&
          buf[o + 1] < y + h
        )
          return true;
      }
      return false;
    };

    /** Record a placed label box. Grow-guarded: a rebuild resizes the
     *  buffer, the frame loop only ever writes inside it. */
    const pushPlaced = (x: number, y: number, w: number, h: number): void => {
      const buf = placedBuf.current;
      const o = placedN.current * 4;
      if (o + 4 > buf.length) return;
      buf[o] = x;
      buf[o + 1] = y;
      buf[o + 2] = w;
      buf[o + 3] = h;
      placedN.current++;
    };

    /** Area-descending comparator over the scratch score buffer — hoisted so
     *  the per-frame sort allocates no closure. Ineligible tiles carry a
     *  negative score and sort to the tail, where the passes skip them. */
    const cmpLabelArea = (a: number, b: number): number =>
      scoreBuf.current[b] - scoreBuf.current[a];

    /** The same trick for the plate ranking: comparator over candScore (the
     *  candidate tail is scored -1, so the whole scratch array sorts in
     *  place and no subarray view is allocated per frame). */
    const cmpPlateArea = (a: number, b: number): number =>
      candScore.current[b] - candScore.current[a];

    /** Focused = hovered or selected. The priority passes claim space for
     *  these first, so a focus label can never stack over another label. */
    const isFocused = (refs: TileRef[], i: number): boolean =>
      hover.current === i || sel.current.has(refs[i].entryId);

    /** One tile's floating label — TILE SPEC tiers (see FONT_* above):
     *  depth-0 BIG (w ≥ 90 AND h ≥ 36) leads with the category swatch +
     *  600/11.5 name + the mono size·share band; every other tile is a
     *  500/10.5 name only. Layout aggregates ("17 more", negative id) also
     *  take the BIG tier at any depth, with a plain byte count instead of a
     *  share: a roll-up without its byte total is just a colored box.
     *  SELECTED rides 650; HOVER moves only the ink alpha (0.95 → 1) — never
     *  weight, never size. Type NEVER shrinks below its tier (10.5 is above
     *  the 9.5 floor): a tile that cannot host the tier drops its label, and
     *  a box that would bleed outside its tile is never drawn. Plated and
     *  dimmed tiles never draw one. */
    const drawOne = (
      ctx: CanvasRenderingContext2D,
      f: TreemapFrameData,
      total: number,
      i: number,
    ): void => {
      if (dimmed(i)) return;
      // A tile with a VISIBLE plate never draws a floating label — the
      // plate's header strip is its label (INS_BRIEF §2.0). Guards the
      // hover pass too, which calls drawOne outside `order`.
      const plv = plated.current;
      if (i < plv.length && plv[i]) return;
      const sc = vp.current.scale;
      const tile = f.tiles[i];
      const ref = f.refs[i];
      // screenRectOf inlined (allocation-free draw loop)
      const x0 = vp.current.tx + tile.x * sc;
      const y0 = vp.current.ty + tile.y * sc;
      const w = tile.w * sc;
      const h = tile.h * sc;
      if (x0 + w < 0 || y0 + h < 0 || x0 > cssW.current || y0 > cssH.current) return;
      // shared LOD floor (layout.ts): 468px² = the label layer's minimum box
      if (labelTierFor(projectedAreaPx(tile.w, tile.h, sc)) === "none") return;
      const availW = w - LABEL_PAD * 2;
      if (availW < 26 || h < LABEL_PAD + LABEL_LINE_H) return;

      const isSel = sel.current.has(ref.entryId);
      const isFocus = hover.current === i || isSel;
      const aggregate = ref.entryId < 0;
      const big = (ref.depth === 0 || aggregate) && w >= BIG_W && h >= BIG_H;
      const nameFont = big
        ? isSel
          ? FONT_BIG_SEL
          : FONT_BIG
        : isSel
          ? FONT_SMALL_SEL
          : FONT_SMALL;

      // Two-channel rule: depth-0 labels with room lead with a category
      // swatch before the name. Reserve its width up front so the name
      // fits beside it; if nothing survives the narrower measure, drop
      // the swatch and fall back to a plain name at full width.
      let useSwatch = false;
      let name = "";
      if (big) {
        name = fitText(ctx, ref.name, nameFont, availW - SWATCH_BLOCK);
        if (name && measure(ctx, name, nameFont) + SWATCH_BLOCK <= availW) useSwatch = true;
      }
      if (!useSwatch) name = fitText(ctx, ref.name, nameFont, availW);
      if (!name) return;
      const nameW = measure(ctx, name, nameFont) + (useSwatch ? SWATCH_BLOCK : 0);
      let meta = "";
      let metaW = 0;
      if (big) {
        // An aggregate's byte total is its whole story; a share-of-volume
        // line would make it read like a single file. The count is already
        // in the name ("17 more"), so the band is just the real bytes.
        meta = aggregate ? formatBytes(ref.size) : tileMeta(f, ref, total);
        metaW = measure(ctx, meta, FONT_META);
        // The size·share band DROPS — never shrinks, never overlaps — when
        // it lacks room in the measure or in the tile's remaining height.
        if (metaW > availW || LABEL_PAD + LABEL_LINE_H + LABEL_META_H > h) {
          meta = "";
          metaW = 0;
        }
      }

      const boxH = LABEL_LINE_H + (meta ? LABEL_META_H : 0);
      const bx = x0 + LABEL_PAD;
      const by = y0 + LABEL_PAD;
      const bw = Math.max(nameW, metaW);
      // final bleed guard: a box that cannot end inside its tile drops
      if (by + boxH > y0 + h) return;
      if (placedOverlaps(bx, by, bw, boxH)) return;
      pushPlaced(bx, by, bw, boxH);

      const ink = inkString(tile.text);
      if (useSwatch) {
        ctx.fillStyle = inkString(tile.fill);
        const sy = by + (LABEL_LINE_H - SWATCH) / 2;
        if (typeof ctx.roundRect === "function") {
          ctx.beginPath();
          ctx.roundRect(bx, sy, SWATCH, SWATCH, 1);
          ctx.fill();
        } else {
          ctx.fillRect(bx, sy, SWATCH, SWATCH);
        }
      }
      ctx.font = nameFont;
      ctx.fillStyle = ink;
      ctx.globalAlpha = isFocus ? 1 : 0.95;
      ctx.textBaseline = "top";
      ctx.fillText(name, bx + (useSwatch ? SWATCH_BLOCK : 0), by);
      if (meta) {
        ctx.font = FONT_META;
        // Size line rides the quiet step: the tile's own (contrast-proven)
        // ink composited at the quietest still-4.5:1 alpha — never fail
        // the floor for the sake of quiet (see quietAlphaFor). Focus
        // labels are full-ink (the hovered/selected readout).
        ctx.globalAlpha = isFocus ? 1 : quietAlphaFor(tile.fill, tile.text);
        ctx.fillText(meta, bx, by + LABEL_LINE_H + 1);
      }
      ctx.globalAlpha = 1;
    };

    const drawLabels = () => {
      const ctx = labelRef.current?.getContext("2d");
      const f = frame.current;
      if (!ctx || !f) return;
      ctx.setTransform(dpr.current, 0, 0, dpr.current, 0, 0);
      ctx.clearRect(0, 0, cssW.current, cssH.current);
      const refs = f.refs;
      // The scratch buffers GROW only, so a shrink (fewer tiles after a
      // rebuild) must re-fit them too: a larger buffer would carry stale
      // scores past refs.length, and those sort into the read range. Still
      // rebuild-only allocation — never per frame.
      if (
        placedBuf.current.length < refs.length * 4 ||
        orderBuf.current.length !== refs.length
      ) {
        placedBuf.current = new Float32Array(refs.length * 4);
        orderBuf.current = new Int32Array(refs.length);
        scoreBuf.current = new Float32Array(refs.length);
      }
      placedN.current = 0;
      let total = 0;
      for (let i = 0; i < refs.length; i++) if (refs[i].depth === 0) total += refs[i].size;
      total = total || 1;

      const order = orderBuf.current;
      const score = scoreBuf.current;
      const pl = plated.current;
      // Every tile takes a slot in the sort; ineligible ones (plated — their
      // label lives in the plate head) carry a negative score and fall to
      // the tail, where the passes skip them. Sorting the WHOLE scratch
      // array in place avoids the subarray view a partial sort allocates.
      for (let i = 0; i < refs.length; i++) {
        order[i] = i;
        score[i] = i < pl.length && pl[i] ? -1 : f.tiles[i].w * f.tiles[i].h;
      }
      order.sort(cmpLabelArea);

      // Priority passes: focused labels claim their space first — the
      // selection, then the hovered tile on top of them — so a focus label
      // can never stack over another label. Everything else yields: a label
      // that would collide with a focused one is the one dropped.
      for (let k = 0; k < refs.length; k++) {
        const i = order[k];
        if (score[i] < 0) continue;
        if (isFocused(refs, i) && hover.current !== i) drawOne(ctx, f, total, i);
      }
      // Bounds check: hover is a raw frame index that can outlive a rebuild
      // by one frame (root cause of the dimmed()/refs[i] TypeError) — a stale
      // index must never reach drawOne.
      if (hover.current >= 0 && hover.current < refs.length) drawOne(ctx, f, total, hover.current);
      for (let k = 0; k < refs.length; k++) {
        const i = order[k];
        if (score[i] < 0) continue;
        if (!isFocused(refs, i)) drawOne(ctx, f, total, i);
      }

      const d = drag.current;
      if (d?.mode === "marquee" && d.rect) {
        const pal = palette.current;
        ctx.fillStyle = pal.marqueeFill;
        ctx.strokeStyle = pal.marqueeStroke;
        ctx.lineWidth = 1;
        ctx.fillRect(d.rect.x, d.rect.y, d.rect.w, d.rect.h);
        ctx.strokeRect(d.rect.x + 0.5, d.rect.y + 0.5, d.rect.w - 1, d.rect.h - 1);
      }
    };

    // -------------------------------------------------------- previews

    // The plate pipeline (INS_BRIEF §2): qualify → rank → cap → claim →
    // position → fill → suppress labels. Runs once per drawn frame —
    // INCLUDING the animated frames of the shared-element zoom — BEFORE
    // drawLabels so the suppression flags are fresh. Per-frame cost is
    // O(tiles) scalar math + sort over qualifying candidates + one Map.get
    // per candidate + O(visible) style writes — no node creation, nothing
    // allocated in steady state.
    //
    // The VISIBLE CAP: at most VISIBLE_CAP plates show simultaneously,
    // ranked by the tile's visible screen area (largest first; incumbents
    // carry INCUMBENT_BIAS so the ranking hysteresis keeps boundary tiles
    // stable). Tiles ranked out keep their plain floating label; their
    // preview requests still fired (pass 1), so when one grows into a cap
    // slot its content is already warm.
    const updatePreviews = () => {
      const f = frame.current;
      const refs = f?.refs;
      if (!f || !refs) return;
      // Directory ids that ACTUALLY own a child tile this frame. A folder in
      // this set is previewed by its interior and must never take a plate; a
      // folder outside it has no interior to show (depth cap, truncated tail,
      // empty/unreadable expansion) and is a leaf folder — the folder-card
      // candidate pass 1 below may request and plate.
      const withInterior = interiorParents.current;
      withInterior.clear();
      for (let i = 0; i < refs.length; i++) {
        const parent = refs[i].parentEntryId;
        if (parent != null) withInterior.add(parent);
      }
      if (plated.current.length !== refs.length) {
        // allocation on REBUILD only, never per frame
        plated.current = new Uint8Array(refs.length);
        candVis.current = new Uint8Array(refs.length);
        candTile.current = new Int32Array(refs.length);
        candScore.current = new Float64Array(refs.length);
        candOrd.current = new Int32Array(refs.length);
      }
      const flags = plated.current;
      flags.fill(0);
      const vis = candVis.current;
      vis.fill(0);
      const pool = slots.current;
      if (pool.length === 0) return;
      ticks.current++;
      for (const s of pool) {
        s.claimed = false;
        s.tileIdx = -1;
      }
      const previews = p.current.previews;
      // Published handler shape is (entryId, path) — read through the alias
      // so either published arity of types.ts compiles (see NeedPreviewFn).
      // The renderer is path-dumb: `pathFor` (optional) resolves a path when
      // the view supplies one, "" otherwise — the view's findEntry does.
      const request = p.current.onNeedPreview as NeedPreviewFn | undefined;
      const pathFor = p.current.pathFor;
      const theme = p.current.theme;
      const vpt = vp.current;
      const sTile = candTile.current;
      const sScore = candScore.current;
      const sOrd = candOrd.current;
      const now = ticks.current;
      const first = reqFirst.current;
      const last = reqLast.current;

      // Share-of-parent denominator for plate metadata — one O(depth-0)
      // scalar pass, no allocation (plates are the only consumers).
      let totalBytes = 0;
      for (let i = 0; i < refs.length; i++) if (refs[i].depth === 0) totalBytes += refs[i].size;

      // --- pass 1: REQUEST (decoupled from plating) → then qualify.
      //
      // REQUEST AND PLATE QUALIFICATION ARE DECOUPLED. A tile asks for its
      // payload as soon as it is on-screen and clears its REQUEST FLOOR —
      // long before any plate gate — because the request is what resolves
      // the path and reads the bytes: a tile that is small now but plates
      // after one more zoom (or on hover) must already have content warm,
      // and "no preview" must only ever mean the backend had nothing to
      // give. Requests are slot-, cap- and family-independent; they fire at
      // fit view as well as deep zoom. The floor is depth-sensitive:
      // depth 0 keeps the tiny REQ_MIN_PX warm-up gate; depth ≥ 1 requires
      // the smallest PLATE gate (REQ_DEEP_MIN_W×H) because lazy recursion
      // can multiply one map into thousands of file tiles while the plate
      // pool stays capped at POOL = 24 — tiles too small to ever plate must
      // not flood the backend with byte reads.
      //
      // A DIRECTORY with a rendered interior never asks: its interior IS the
      // nested treemap, and expand/drill/zoom is its preview (locked law).
      // A LEAF FOLDER — a directory with no child tiles this frame (depth
      // cap, cap-truncated tail, empty or unreadable expansion) — has no
      // interior to show, so it asks for its folder listing and may plate a
      // real content card. That is the only way a large grey box can become
      // content; it can never cover a child because there are none. FILE
      // tiles ask at ANY depth. Plates still cannot stack: an interior folder
      // is skipped, a leaf folder has no children, and a file has none.
      let nc = 0;
      for (let i = 0; i < refs.length; i++) {
        const ref = refs[i];
        // Layout-only aggregates ("17 more") have no filesystem identity and
        // must never reach pathFor/onNeedPreview. They are summaries, not
        // previewable entries.
        if (ref.entryId < 0) continue;
        if (ref.isDir && withInterior.has(ref.entryId)) continue;
        const leafFolder = ref.isDir;
        const t = f.tiles[i];
        const w = t.w * vpt.scale;
        const h = t.h * vpt.scale;
        // File floors are depth-sensitive (see above). A leaf-folder listing
        // costs a read_dir + metadata head, and below the LIST plate gate the
        // card could not render anyway — so the folder request floor IS the
        // LIST gate, one request per qualifying folder per generation.
        const floorW = leafFolder
          ? PLATE_MIN.list[0]
          : ref.depth === 0
            ? REQ_MIN_PX
            : REQ_DEEP_MIN_W;
        const floorH = leafFolder
          ? PLATE_MIN.list[1]
          : ref.depth === 0
            ? REQ_MIN_PX
            : REQ_DEEP_MIN_H;
        if (w < floorW || h < floorH) continue;
        const x = vpt.tx + t.x * vpt.scale;
        const y = vpt.ty + t.y * vpt.scale;
        if (x > cssW.current || y > cssH.current || x + w < 0 || y + h < 0) continue;
        // A dimmed tile is a filtered tile: no request, no content plate.
        if (dimmed(i)) continue;

        const payload: PreviewPayload | null | undefined = previews?.get(ref.entryId);

        // --- REQUEST (above every plate gate, hoisted before them): fires
        // for EVERY on-screen, non-dimmed FILE tile at any depth that clears
        // its depth-sensitive request floor, and for a LEAF FOLDER at the
        // LIST gate, independent of any plate/cap/family gate. The once-per-entryId-per-generation guard is reqFirst;
        // a silent attempt retries on the backoff (REQ_RETRY_TICKS ×
        // REQ_MAX_ATTEMPTS) so ONE queued-out attempt can never poison a tile
        // as "no preview" for the rest of the generation. The payload
        // arriving is the only success signal the renderer sees.
        //
        // `undefined` = NO ANSWER YET → ask. `null` = the pipeline already
        // answered with an honest failure (preview.ts memoises failures per
        // generation) → never re-ask: pass 3's fallback card is the correct
        // response, and repeating the IPC would only re-deliver the same
        // null. A present payload → nothing to ask.
        if (payload === undefined && request) {
          const f0 = first.get(ref.entryId);
          if (f0 === undefined) {
            first.set(ref.entryId, now);
            last.set(ref.entryId, now);
            request(ref.entryId, pathFor?.(ref.entryId) ?? "");
          } else {
            const l0 = last.get(ref.entryId) ?? f0;
            if (now - l0 >= REQ_RETRY_TICKS && now - f0 < REQ_RETRY_TICKS * REQ_MAX_ATTEMPTS) {
              last.set(ref.entryId, now);
              request(ref.entryId, pathFor?.(ref.entryId) ?? "");
            }
          }
        }

        // --- PLATE QUALIFICATION (independent of the request above).
        // Visible box: the plate is clamped to the canvas∩tile rect.
        // The 4px inset rides only the tile sides that are actually on
        // canvas; the clipped sides bleed to the canvas edge exactly like
        // the tile itself does (the canvas clips tiles at its own bounds).
        // At ANY zoom the painted plate therefore lives inside the viewport
        // with its head/keys/rows anchored top-left — a content island in
        // an empty plate sea is structurally impossible, and every column
        // (names included) is always on screen.
        const pleft = Math.max(x + 4, 0);
        const ptop = Math.max(y + 4, 0);
        const pw = Math.min(x + w - 4, cssW.current) - pleft;
        const ph = Math.min(y + h - 4, cssH.current) - ptop;
        if (pw < PLATE_VISIBLE_MIN || ph < PLATE_VISIBLE_MIN) continue; // sliver → label only

        if (payload) {
          // Family from the payload discriminant — the renderer never sniffs
          // content. §Integrator-note-2/§A: svg previews as TEXT, NONE reads
          // as a META plate; both map to their render twin's threshold. The
          // `string` widen also keeps runtime payloads outside the TS union
          // (svg/none exist in §A) from tripping a comparison error here.
          const famRaw: string = payload.family;
          const famKey = famRaw === "svg" ? "text" : famRaw === "none" ? "meta" : famRaw;
          const min = PLATE_MIN[famKey];
          if (!min || w < min[0] || h < min[1]) continue; // → floating label only
        } else {
          // PAYLOAD NOT IN HAND — two distinct answers, both eligible:
          //  • `undefined` (absent): still in flight, asked above;
          //  • `null`: the pipeline already ANSWERED with an honest failure
          //    (preview.ts — "null means UNREADABLE … callers must render
          //    their honest fallback card"), memoised for this generation.
          // Either way the tile must still claim a slot, because pass 3 is
          // the ONLY place a plate is ever shown — a tile skipped here can
          // never paint its fallback META card, and the map degenerates to
          // tint + label for every file whose payload is missing or failed
          // (this was the regression: no plate anywhere despite every gate
          // clearing). Two conditions, both pre-rewrite law:
          //  1. a handler exists that could ever fill the slot;
          //  2. the PAINTED box can host a card at all — the fallback ladder
          //     paints its META card at exactly PLATE_MIN.meta (previewPlate
          //     refuses below it), so that is the honest claim floor; below
          //     it the floating label is all there is room for.
          if (!request) continue;
          if (pw < PLATE_MIN.meta[0] || ph < PLATE_MIN.meta[1]) continue;
          // answered-failure (null) is NOT re-asked: preview.ts memoises
          // failures per generation, so re-asking only repeats a known
          // answer — pass 3's fallback card is the correct response.
          if (payload === null) {
            // (no request; candidacy continues below)
          } else if (
            // in-flight: first attempt once per entryId per generation,
            // then a backoff retry (REQ_RETRY_TICKS × REQ_MAX_ATTEMPTS) so
            // one queued-out attempt can never poison the tile for the
            // whole generation. The payload arriving is the renderer's only
            // success signal.
            first.get(ref.entryId) !== undefined
          ) {
            const f0 = first.get(ref.entryId) as number;
            const l0 = last.get(ref.entryId) ?? f0;
            if (now - l0 >= REQ_RETRY_TICKS && now - f0 < REQ_RETRY_TICKS * REQ_MAX_ATTEMPTS) {
              last.set(ref.entryId, now);
              request(ref.entryId, pathFor?.(ref.entryId) ?? "");
            }
          } else {
            first.set(ref.entryId, now);
            last.set(ref.entryId, now);
            request(ref.entryId, pathFor?.(ref.entryId) ?? "");
          }
        }

        // A candidate for a plate this frame. Incumbents carry the
        // hysteresis bias so continuity survives ranking.
        sTile[nc] = i;
        sScore[nc] = pw * ph * (slotOf.current.has(ref.entryId) ? INCUMBENT_BIAS : 1);
        sOrd[nc] = nc;
        nc++;
      }
      // Candidate tail scored -1 so the WHOLE scratch array sorts in place
      // (a subarray view per frame is an allocation).
      for (let k = nc; k < refs.length; k++) {
        sScore[k] = -1;
        sOrd[k] = k;
      }

      // --- pass 2: rank largest-first and mark the visibility cap slots.
      sOrd.sort(cmpPlateArea);
      const shown = Math.min(nc, VISIBLE_CAP);
      for (let k = 0; k < shown; k++) vis[sTile[sOrd[k]]] = 1;

      // --- pass 3: claim → position → fill → show, in RANK order, so the
      // top-ranked tiles claim the pool's slots first and a cap-honored
      // plate is never crowded out by a lower-ranked one.
      for (let k = 0; k < nc; k++) {
        if (k >= shown) break; // ranked out: no claim, floating label carries the tile
        const ci = sOrd[k];
        const i = sTile[ci];
        const ref = refs[i];
        const t = f.tiles[i];
        const w = t.w * vpt.scale;
        const h = t.h * vpt.scale;
        const x = vpt.tx + t.x * vpt.scale;
        const y = vpt.ty + t.y * vpt.scale;
        const pleft = Math.max(x + 4, 0);
        const ptop = Math.max(y + 4, 0);
        let pw = Math.min(x + w - 4, cssW.current) - pleft;
        let ph = Math.min(y + h - 4, cssH.current) - ptop;

        const payload: PreviewPayload | null | undefined = previews?.get(ref.entryId);
        const fam = payload ? (payload.family as string) : "";

        // --tile-* units: the CSS calc subtracts the 8px inset back out, so
        // the painted plate is exactly pw×ph at (pleft, ptop).
        // ================================================== XL listing cap
        // A plate is a preview instrument, not a replacement for the map. A
        // FILE tile that covers most of the canvas (a giant archive at
        // shallow zoom) lists 60+ rows and visually REPLACES the world: the
        // map reads as a file list, zoom reads as dead, and every picking
        // path hides under the paper. Long archive/table listings in such
        // tiles are therefore capped to a bounded top-left panel — the
        // tile's neighbourhood stays visible around it, so hover and drill
        // land on what the eye can see. Modest tiles keep their full-tile
        // preview: only family LIST/DB payloads with 20+ offering rows and
        // a near-full-canvas tile are capped, nothing else ever is.
        const famU = fam.toUpperCase();
        const listCap =
          (famU === "LIST" || famU === "DB") &&
          ((payload?.list?.entries?.length ?? 0) >= 20);
        if (listCap && pw >= cssW.current * 0.72 && ph >= cssH.current * 0.72) {
          pw = Math.min(pw, Math.max(Math.round(cssW.current * 0.5), 360));
          ph = Math.min(ph, Math.max(Math.round(cssH.current * 0.42), 260));
        }
        const tw = pw + 8;
        const th = ph + 8;

        // --- claim a slot: continuity first (an entry keeps its node across
        // frames, so pooled content survives pan/zoom), then a free slot,
        // then steal an unclaimed one; pool exhausted → this tile keeps its
        // floating label.
        let slot = slotOf.current.get(ref.entryId);
        if (!slot || slot.claimed) {
          slot = undefined;
          for (const s of pool)
            if (!s.claimed && s.entryId < 0) {
              slot = s;
              break;
            }
          if (!slot)
            for (const s of pool)
              if (!s.claimed) {
                slot = s;
                break;
              }
          if (!slot) continue;
          if (slot.entryId >= 0) {
            if (slotOf.current.get(slot.entryId) === slot) slotOf.current.delete(slot.entryId);
            slot.entryId = -1; // re-keyed below
          }
          slotOf.current.set(ref.entryId, slot);
        }
        slot.claimed = true;
        slot.tileIdx = i; // hit-testing: the plate paints for exactly this tile

        // --- position: left/top are the clamped box origin; width/height
        // ride --tile-w/--tile-h (the CSS calc subtracts the 8px inset
        // back out). Compare-before-write: a settled view costs zero style
        // traffic per frame. This runs on EVERY drawn frame — the 420ms
        // zoom animation included — so plates track their tiles in flight.
        if (slot.lx !== pleft || slot.ly !== ptop) {
          slot.el.style.left = `${pleft}px`;
          slot.el.style.top = `${ptop}px`;
          slot.lx = pleft;
          slot.ly = ptop;
        }
        if (slot.lw !== tw || slot.lh !== th) {
          slot.el.style.setProperty("--tile-w", `${tw}px`);
          slot.el.style.setProperty("--tile-h", `${th}px`);
          slot.lw = tw;
          slot.lh = th;
        }

        if (payload) {
          // --- fill once per (entryId, family, truncated, totalLines)
          // change. Size drift refills too, but ONLY on a settled viewport —
          // content is cached per entry; an animation never rebuilds DOM.
          const tr =
            payload.text?.truncated ?? payload.list?.truncated ?? payload.db?.truncated ?? false;
          const tl = payload.text?.totalLines ?? -1;
          const settled = !anim.current;
          // Drift is measured on the PAINTED box (canvas-clamped, in the
          // same --tile-* units the CSS paints): row caps, column tracks
          // and the head tier all derive from it at fill time, so any
          // material change refills once the viewport settles. The tier
          // term is its own predicate: a pure ratio test can straddle the
          // 320px lg-head boundary while staying inside the 12.5% band.
          const drifted =
            slot.fillW > 0 &&
            ((Math.abs(tw - slot.fillW) * 8 > slot.fillW) ||
              (Math.abs(th - slot.fillH) * 8 > slot.fillH) ||
              (slot.fillW >= 320) !== (tw >= 320) ||
              (slot.fillW >= 360) !== (tw >= 360));
          if (
            slot.entryId !== ref.entryId ||
            slot.fallback ||
            slot.family !== fam ||
            slot.truncated !== tr ||
            slot.totalLines !== tl ||
            slot.theme !== theme ||
            (settled && drifted)
          ) {
            // Tile metadata rides EVERY fill (final plate contract): a null
            // payload then degrades to a real META card (name / type /
            // category / share) instead of an empty sheet. entryId-stable,
            // so the meta term never forces a refill on its own.
            fillPlate(slot.el, payload, {
              fileName: ref.name,
              theme,
              tile: { w: tw, h: th },
              meta: plateMetaOf(ref, totalBytes),
              parentSize: totalBytes,
            });
            slot.entryId = ref.entryId;
            slot.fallback = false;
            slot.reason = "";
            slot.family = fam;
            slot.truncated = tr;
            slot.totalLines = tl;
            slot.theme = theme;
            slot.fillW = tw;
            slot.fillH = th;
            // last-filled key, inspectable in devtools (written on fills only)
            slot.el.dataset.fill = `${ref.entryId}|${fam}|${tr ? 1 : 0}|${tl}`;
          }
          if (slot.el.hidden) slot.el.hidden = false;
          flags[i] = 1; // visible plate → the floating label is suppressed
        } else {
          if (slot.entryId !== ref.entryId) {
            slot.entryId = ref.entryId;
            slot.fallback = false;
            slot.reason = "";
            slot.family = "";
            slot.fillW = 0;
            slot.reqTick = ticks.current;
          }
          // The request may still be genuinely IN FLIGHT — the backend
          // queues heavy previews behind a permit semaphore, so >0.5s is
          // routine. While it is, re-arm the clock EVERY frame so the grace
          // can never paint "no preview" over a payload a moment away.
          // "In flight" = asked this generation and inside REQ_AWAIT_TICKS
          // (past it the attempts are exhausted and the tile is honestly
          // unavailable — see the retry gate in pass 1).
          const f0 = reqFirst.current.get(ref.entryId);
          if (f0 !== undefined && ticks.current - f0 < REQ_AWAIT_TICKS) slot.reqTick = ticks.current;
          // Requested but nothing returned: after the await window and the
          // grace, the honest unavailable state — "no preview" means we
          // asked (up to REQ_MAX_ATTEMPTS) and got nothing back (§3 leaf
          // interior, §4.8 no guessing). Tile metadata rides the fallback
          // too, so a tile that still has facts paints the real META card
          // rather than a bare sheet.
          if (ticks.current - slot.reqTick >= FALLBACK_GRACE) {
            if (!slot.fallback || slot.reason !== "no preview" || slot.theme !== theme) {
              slot.el.dataset.theme = theme;
              fillFallbackPlate(slot.el, "no preview", plateMetaOf(ref, totalBytes));
              slot.fallback = true;
              slot.reason = "no preview";
              slot.theme = theme;
              slot.el.dataset.fill = `${ref.entryId}|fb|no preview`;
            }
            if (slot.el.hidden) slot.el.hidden = false;
            flags[i] = 1;
          }
          // within the grace: stay hidden; the floating label carries the tile
        }
      }

      // --- unclaimed slots return to the pool: hidden, binding dropped
      for (const s of pool) {
        if (s.claimed || s.entryId < 0) continue;
        if (slotOf.current.get(s.entryId) === s) slotOf.current.delete(s.entryId);
        s.entryId = -1;
        s.fallback = false;
        s.family = "";
        s.fillW = 0;
        s.lx = NaN; // sentinel: a future claim must rewrite position/size
        s.ly = NaN;
        s.lw = NaN;
        s.lh = NaN;
        if (!s.el.hidden) s.el.hidden = true;
      }
    };

    // ---------------------------------------------------- instruments

    // Zoom readout: 100% = the fitted view, which under the fill invariant is
    // also the floor — z is clamped to >= 1, so the chip's ordinary range is
    // (100%, MAX_ZOOM], and beyond it only where zoomCeilingAt() opened
    // deep-zoom headroom for a small tile (BUG 2). The readout is written via
    // textContent from the draw loop (no React state, so panning stays free)
    // and hidden AT the floor: the instrument only speaks when there is
    // something above 100% to say. The 1e-6 is fp slack, not a perceptual
    // band — a viewport snapped to fit compares exactly.
    const updateInstruments = () => {
      const node = zoomRef.current;
      if (!node) return;
      const base = fit().scale;
      const z = base > 0 ? vp.current.scale / base : 1;
      const text = `ZOOM ${Math.round(z * 100)}%`;
      if (text !== zoomText.current) {
        zoomText.current = text;
        node.textContent = text;
      }
      const atFit = z <= 1 + 1e-6;
      if (atFit !== zoomHidden.current) {
        zoomHidden.current = atFit;
        node.style.opacity = atFit ? "0" : "1";
      }
    };

    // ------------------------------------------------- interior expansion
    //
    // The map recurses LAZILY to MAX_INTERIOR_DEPTH: the view supplies the
    // interiors it already fetched through `expanded`, and this pass asks for
    // the ones that can show real structure NOW. A folder is worth
    // subdividing only when its projected tile clears EXPAND_MIN_AREA_PX (a
    // smaller box would subdivide into slivers, not structure), it is
    // on-screen, undimmed, shallower than the cap, and has no interior in
    // `expanded` yet. Requests are capped at EXPAND_BATCH per DRAWN frame so
    // a first paint cannot fan out into hundreds of IPC round-trips; the
    // dedupe Set is the progress ledger — a folder is asked at most once per
    // entries generation, whether the answer is children or the parent's
    // memoised empty array (failure). An `expanded` arrival rebuilds the
    // frame (ensureFrame identity) and the next draws ask for the newly
    // revealed, still-large folders. Raw frame indices + scalar math + Set/
    // Map lookups only: this pass allocates nothing in steady state.
    const requestInteriors = () => {
      const f = frame.current;
      if (!f) return;
      const need = p.current.onNeedExpand;
      if (!need) return;
      const asked = expandAsked.current;
      const expanded = p.current.expanded;
      const sc = vp.current.scale;
      const tx0 = vp.current.tx;
      const ty0 = vp.current.ty;
      let issued = 0;
      for (let i = 0; i < f.refs.length && issued < EXPAND_BATCH; i++) {
        const ref = f.refs[i];
        if (!ref.isDir || ref.depth >= MAX_INTERIOR_DEPTH) continue;
        if (asked.has(ref.entryId) || expanded?.has(ref.entryId)) continue;
        const t = f.tiles[i];
        const w = t.w * sc;
        const h = t.h * sc;
        const x = tx0 + t.x * sc;
        const y = ty0 + t.y * sc;
        if (x > cssW.current || y > cssH.current || x + w < 0 || y + h < 0) continue;
        if (dimmed(i)) continue;
        if (w * h < EXPAND_MIN_AREA_PX) continue;
        asked.add(ref.entryId);
        need(ref.entryId);
        issued++;
      }
    };

    const draw = () => {
      ensureFrame();
      requestInteriors(); // right after the frame: the new frame's folders ask for interiors
      drawGL(true);
      updatePreviews(); // before drawLabels: suppression flags must be fresh
      drawLabels();
      updateInstruments();
    };

    // -------------------------------------------------------- animation

    const tick = () => {
      raf.current = 0;
      if (loopDead.current) return; // already reported; kick() won't re-arm
      try {
        const a = anim.current;
        if (a) {
          // prefers-reduced-motion collapses the duration to a cut — the
          // same code path, so the invariants (enforceFill, hover re-resolve)
          // still run, they just land on the target frame.
          const dur = reducedMotion.current ? REDUCED_MS : durNav.current;
          const t = Math.min(1, (performance.now() - a.t0) / dur);
          const k = ease(t);
          vp.current = {
            tx: a.from.tx + (a.to.tx - a.from.tx) * k,
            ty: a.from.ty + (a.to.ty - a.from.ty) * k,
            scale: a.from.scale + (a.to.scale - a.from.scale) * k,
          };
          if (t >= 1) {
            vp.current = a.to;
            anim.current = null;
            // The world finished moving under a cursor that may not have:
            // re-resolve at the last known point so the tooltip names what is
            // actually there now (keyboard/command zoom, jump-to — BUG 3's
            // class; dedupe makes it free when nothing changed, and a pointer
            // that has left the canvas has NaN here, so nothing resolves).
            // paint=false: the draw() right below picks up hover.current in
            // its normal full pass — no partial repaint needed.
            const h = hoverLast.current;
            if (!Number.isNaN(h.x)) resolveHover(h.x, h.y, false);
          }
        } else if (wheelTarget.current) {
          const target = wheelTarget.current;
          // reduced motion: no smoothing — snap to the accumulated target
          const alpha = reducedMotion.current ? 1 : WHEEL_LERP;
          vp.current = enforceFill({
            scale: vp.current.scale + (target.scale - vp.current.scale) * alpha,
            tx: vp.current.tx + (target.tx - vp.current.tx) * alpha,
            ty: vp.current.ty + (target.ty - vp.current.ty) * alpha,
          });
          const scaleDone =
            Math.abs(vp.current.scale - target.scale) <=
            Math.max(1e-4, target.scale * 1e-4);
          const txDone = Math.abs(vp.current.tx - target.tx) <= 0.5;
          const tyDone = Math.abs(vp.current.ty - target.ty) <= 0.5;
          if (scaleDone && txDone && tyDone) {
            vp.current = target;
            wheelTarget.current = null;
          }
        }
        draw();
        if (anim.current || wheelTarget.current) raf.current = requestAnimationFrame(tick);
      } catch (err) {
        // ROBUSTNESS: exactly one console.error, one global toast event, and
        // the loop stops — no overlay spam (App bridges space:error to a
        // quiet statusbar toast) and never a silently dead loop. The latch
        // keeps later kick()es from re-arming a loop that can only throw the
        // same way again.
        loopDead.current = true;
        const msg = err instanceof Error ? err.message : String(err);
        console.error("treemap draw loop stopped:", err);
        window.dispatchEvent(new CustomEvent("space:error", { detail: `map render: ${msg}` }));
      }
    };

    const kick = () => {
      if (loopDead.current) return;
      if (!raf.current) raf.current = requestAnimationFrame(tick);
    };

    // Every animation goes through here, so clamping here covers ALL
    // targets: resetView (fit = the floor), zoomBy, zoomToTile, and the
    // nav transitions (enterFromForZoomOut / zoomOutEnterFrom, the latter
    // intentionally starting zoomed-in for the pull-back — legal, it is
    // above the floor; only its TARGET, fit, matters and fit == floor).
    const animateTo = (to: Viewport) => {
      vp.current = enforceFill(vp.current);
      const dest = enforceFill(to);
      if (reducedMotion.current) {
        // prefers-reduced-motion: the animation IS the destination. Same
        // end state as the eased path (enforced viewport, hover re-resolved
        // at the last known point so the tooltip names what is actually
        // there), minus the 420ms of travel.
        vp.current = dest;
        wheelTarget.current = null;
        anim.current = null;
        const h = hoverLast.current;
        if (!Number.isNaN(h.x)) resolveHover(h.x, h.y, false);
        kick();
        return;
      }
      anim.current = { from: { ...vp.current }, to: dest, t0: performance.now() };
      kick();
    };

    // -------------------------------------------------------- events

    const emitSelection = () => {
      const f = frame.current;
      if (!f) return;
      const ids = [...sel.current].sort((a, b) => a - b);
      let total = 0;
      for (const id of ids) {
        const i = f.refs.findIndex((r) => r.entryId === id);
        if (i >= 0) total += f.refs[i].size;
      }
      p.current.onSelectionChange(ids, total);
    };

    const localPos = (e: PointerEvent | WheelEvent) => {
      const rect = wrapRef.current!.getBoundingClientRect();
      return { x: e.clientX - rect.left, y: e.clientY - rect.top };
    };

    useEffect(() => {
      const wrap = wrapRef.current;
      const glc = glCanvasRef.current;
      const lbc = labelRef.current;
      if (!wrap || !glc || !lbc) return;

      const gl = glc.getContext("webgl2", { antialias: true, alpha: false });
      if (!gl) return;

      // CSS-owned colors first (after the context check so a failed WebGL2
      // mount can't leave a live observer behind): the initial palette
      // before the first draw.
      loadPalette();
      // Re-read whenever the document theme attribute flips — covers theme
      // changes that land on the DOM before (or without) a props.theme pass.
      const themeMO = new MutationObserver(() => loadPalette());
      themeMO.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-theme"],
      });
      glCtx.current = gl;
      prog.current = createTileProgram(gl); // enables aA blending (see gl.ts)

      // Preview plate pool (POOL nodes = VISIBLE_CAP, so every ranked-in
      // candidate can carry content): appended to the static layer. React
      // renders the layer empty and never touches a plate again — every
      // later write (position/visibility/content) is imperative from the
      // draw loop (updatePreviews). Plates leave the DOM with the layer.
      const layer = plateLayerRef.current;
      if (layer) {
        const pool: PlateSlot[] = [];
        for (let i = 0; i < POOL; i++) {
          const el = createPlate();
          layer.appendChild(el);
          pool.push({
            el,
            entryId: -1,
            family: "",
            truncated: false,
            totalLines: -1,
            fallback: false,
            reason: "",
            theme: "light",
            fillW: 0,
            fillH: 0,
            lx: NaN,
            ly: NaN,
            lw: NaN,
            lh: NaN,
            reqTick: 0,
            claimed: false,
            tileIdx: -1,
          });
        }
        slots.current = pool;
      }

      // Font-metric fix: measureText caches widths per font string, so a
      // measure taken before Geist loads would poison the cache with
      // fallback metrics for the session. Warm the label faces now and,
      // once the font set settles, drop the cache and repaint once — any
      // measures that happened in the meantime are simply discarded.
      const fonts = (document as Partial<Document>).fonts;
      if (fonts) {
        for (const face of [
          "650 11.5px Geist",
          "600 11.5px Geist",
          "500 10.5px Geist",
          "500 10.5px 'Geist Mono'",
        ]) {
          fonts.load(face).catch(() => {});
        }
        fonts.ready
          .then(() => {
            mcache.current.clear();
            kick();
          })
          .catch(() => {});
      }

      const measureNow = (isInitial: boolean) => {
        const rect = wrap.getBoundingClientRect();
        const w = Math.max(1, Math.round(rect.width));
        const h = Math.max(1, Math.round(rect.height));
        if (!isInitial && w === cssW.current && h === cssH.current) return;
        cssW.current = w;
        cssH.current = h;
        dpr.current = Math.min(window.devicePixelRatio || 1, 2);
        for (const c of [glc, lbc]) {
          c.width = Math.round(w * dpr.current);
          c.height = Math.round(h * dpr.current);
        }
        ensureFrame();
        kick();
      };

      measureNow(true);
      const ro = new ResizeObserver(() => measureNow(false));
      ro.observe(wrap);

      const onDown = (e: PointerEvent) => {
        if (e.button !== 0) return;
        // Drill cooldown: a directory drill just committed (<500ms ago). The
        // 2nd physical click of a double-click lands after the instant
        // in-memory rebuild and would drill AGAIN into whatever now sits
        // under the same pointer — a double-click teleported 2-3 levels
        // deep. Swallow the whole tail of the gesture here.
        if (performance.now() < drillLockUntil) return;
        cancelPendingHover();
        anim.current = null;
        wheelTarget.current = null;
        const { x, y } = localPos(e);
        const idx = hitTest(x, y);
        drag.current = {
          mode: "pending",
          sx: x,
          sy: y,
          px: x,
          py: y,
          idx,
          shift: e.shiftKey,
          rect: null,
        };
        wrap.setPointerCapture(e.pointerId);
      };

      const onMove = (e: PointerEvent) => {
        const { x, y } = localPos(e);
        const d = drag.current;
        if (d) {
          const dx = x - d.px;
          const dy = y - d.py;
          if (d.mode === "pending") {
            const dist = Math.hypot(x - d.sx, y - d.sy);
            if (dist > 3) {
              if (d.shift) {
                d.mode = "marquee";
                d.rect = { x: d.sx, y: d.sy, w: 0, h: 0 };
              } else {
                d.mode = "pan";
                wrap.style.cursor = "grabbing";
              }
              // BUG 3: the gesture is about to move content (or the cursor)
              // under a point that isn't being re-resolved — the hover would
              // name a tile that no longer sits under the cursor and its
              // tooltip would float over bare canvas. Drop it the moment the
              // gesture commits; onUp re-resolves at the release point.
              if (hover.current !== -1) {
                hover.current = -1;
                const hl = hoverLast.current;
                hl.x = NaN;
                hl.y = NaN;
                hl.idx = -2;
                p.current.onHoverChange(null);
                kick();
              }
              cancelPendingHover();
            }
          }
          if (d.mode === "pan") {
            vp.current.tx += dx;
            vp.current.ty += dy;
            // Coverage clamp (see coverToCanvas): at fit this is a no-op —
            // the world is exactly canvas-sized, so the only legal tx/ty is
            // 0 and the drag cannot reveal background; zoomed in it only
            // bites where the world's edge would leave the canvas.
            vp.current = coverToCanvas(vp.current);
            anim.current = null;
            wheelTarget.current = null;
            kick();
          } else if (d.mode === "marquee") {
            d.rect = {
              x: Math.min(d.sx, x),
              y: Math.min(d.sy, y),
              w: Math.abs(x - d.sx),
              h: Math.abs(y - d.sy),
            };
            const f = frame.current;
            marqueeSel.current.clear();
            if (f) {
              // screenRectOf inlined (marquee pass, allocation-free)
              const sc = vp.current.scale;
              const tx0 = vp.current.tx;
              const ty0 = vp.current.ty;
              for (let i = 0; i < f.refs.length; i++) {
                if (f.refs[i].depth !== 0 || dimmed(i)) continue;
                const t = f.tiles[i];
                const rect: Rect = {
                  x: tx0 + t.x * sc,
                  y: ty0 + t.y * sc,
                  w: t.w * sc,
                  h: t.h * sc,
                };
                if (overlaps(rect, d.rect!)) marqueeSel.current.add(f.refs[i].entryId);
              }
            }
            kick();
          }
          d.px = x;
          d.py = y;
          return;
        }
        // idle hover — pointer events only LEAVE a point behind; resolution
        // is rAF-aligned (scheduleHover): at most one emission per frame,
        // and a tile crossing patches the GL buffer in place instead of
        // re-instancing it (see the hover pipeline in the header).
        scheduleHover(x, y);
      };

      const onUp = (e: PointerEvent) => {
        const d = drag.current;
        drag.current = null;
        wrap.style.cursor = "";
        try {
          wrap.releasePointerCapture(e.pointerId);
        } catch {
          /* already released */
        }
        if (!d) return;
        const f = frame.current;
        // Raw drag index can outlive a rebuild too — same class as the
        // hover crash: cap it before it dereferences refs.
        if (d.mode === "pending" && f && d.idx >= 0 && d.idx < f.refs.length) {
          const ref = f.refs[d.idx];
          const id = ref.entryId;
          if (id < 0) {
            // Layout aggregate ("17 more"): open the folder whose tail it
            // summarizes. Drilling promotes that folder to depth 0, where its
            // full child list lays out without the interior child cap.
            const pid = ref.parentEntryId;
            if (pid != null) {
              const rect = findWorldRect(f, pid);
              if (rect) p.current.onZoomCommit(pid, rect);
            }
            kick();
            return;
          }
          if (ref.isDir && !d.shift) {
            // Single click on a directory tile navigates into it (files still
            // select). Mirror of the double-click drill-in path.
            const rect = findWorldRect(f, id);
            if (rect) {
              p.current.onZoomCommit(id, rect);
              // The single-click drill IS the whole double-click gesture;
              // arm the cooldown so the 2nd physical click and the
              // synthesized dblclick cannot drill again (see onDown).
              drillLockUntil = performance.now() + 500;
            }
            kick();
          } else if (d.shift) {
            if (sel.current.has(id)) sel.current.delete(id);
            else sel.current.add(id);
            emitSelection();
            kick();
          } else {
            sel.current = new Set([id]);
            p.current.onPick(id);
            emitSelection();
            kick();
          }
        } else if (d.mode === "pending" && d.idx < 0) {
          if (sel.current.size) {
            sel.current.clear();
            emitSelection();
          }
          p.current.onPick(null);
          kick();
        } else if (d.mode === "marquee") {
          if (!d.shift) sel.current = new Set();
          for (const id of marqueeSel.current) sel.current.add(id);
          marqueeSel.current.clear();
          emitSelection();
          kick();
        }
        // Gesture end (BUG 3): content moved under a cursor that never moved
        // (pan), or the cursor moved while re-resolution was suspended
        // (marquee) — re-resolve the hover AT the release point so the
        // tooltip names what is really there, or null over bare canvas.
        // rAF-aligned like every other hover emission.
        if (d.mode === "pan" || d.mode === "marquee") {
          const pos = localPos(e);
          scheduleHover(pos.x, pos.y);
        }
      };

      let drillLockUntil = 0; // armed by a directory drill; swallows the rest of that double-click gesture

      const onDbl = (e: MouseEvent) => {
        // Directory drills already happened via the single-click path; the
        // synthesized dblclick must never drill again (it fires inside the
        // drill cooldown). Files keep their double-click-to-open.
        if (performance.now() < drillLockUntil) return;
        const { x, y } = localPos(e as unknown as WheelEvent);
        const f = frame.current;
        const idx = hitTest(x, y);
        if (!f || idx < 0) return;
        const ref = f.refs[idx];
        if (ref.entryId < 0) return; // aggregate: nothing to open
        if (ref.isDir) {
          const rect = findWorldRect(f, ref.entryId);
          if (rect) p.current.onZoomCommit(ref.entryId, rect);
        } else {
          p.current.onOpenFile(ref.entryId);
        }
      };

      const onWheelN = (e: WheelEvent) => {
        e.preventDefault();
        const { x, y } = localPos(e);
        const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
        const dy = e.deltaY * unit;
        if (e.shiftKey) {
          vp.current.tx -= dy;
          vp.current = coverToCanvas(vp.current); // same clamp as drag-pan
          anim.current = null;
          wheelTarget.current = null;
          kick();
          scheduleHover(x, y); // content slid under a still cursor (BUG 3)
          return;
        }
        const base = fit().scale;
        const factor = Math.exp(-dy * 0.0016);
        // Continuous/smooth wheel zoom: accumulate a TARGET viewport from the
        // (fractional) wheel delta and let the rAF loop lerp toward it, so a
        // stream of wheel events reads as one fluid motion instead of discrete
        // jumps. Cursor-anchored: the anchor math is applied to the current
        // target (or the live viewport when no target is in flight), so
        // repeated events compound instead of fighting the lerp. FILL FLOOR:
        // zoom-out bottoms out at the fitted scale. CEILING: raised for the
        // tile under the cursor by zoomCeilingAt() (BUG 2).
        anim.current = null;
        const cur = wheelTarget.current ?? vp.current;
        const ceil = zoomCeilingAt(x, y);
        const nextScale = Math.min(ceil, Math.max(base, cur.scale * factor));
        const k = nextScale / cur.scale;
        const nextVp: Viewport = enforceFill({
          scale: nextScale,
          tx: x - (x - cur.tx) * k,
          ty: y - (y - cur.ty) * k,
        });
        wheelTarget.current = nextVp;
        kick();
        // The zoom re-pinned the world under the cursor (clamp/floor shifts
        // can move it even about a fixed point) — re-resolve (BUG 3).
        scheduleHover(x, y);
      };

      const onCtx = (e: MouseEvent) => {
        e.preventDefault();
        const { x, y } = localPos(e as unknown as WheelEvent);
        const idx = hitTest(x, y);
        const f = frame.current;
        // Negative ids are layout aggregates, not entries: no context menu.
        const id = idx >= 0 && f ? f.refs[idx].entryId : null;
        p.current.onContextMenu(id !== null && id >= 0 ? id : null, e.clientX, e.clientY);
      };

      const onLeave = () => {
        cancelPendingHover();
        if (hover.current !== -1) {
          hover.current = -1;
          const hl = hoverLast.current;
          hl.x = NaN;
          hl.y = NaN;
          hl.idx = -2;
          p.current.onHoverChange(null);
          kick();
        }
      };

      wrap.addEventListener("pointerdown", onDown);
      wrap.addEventListener("pointermove", onMove);
      wrap.addEventListener("pointerup", onUp);
      wrap.addEventListener("pointercancel", onUp);
      wrap.addEventListener("dblclick", onDbl);
      wrap.addEventListener("wheel", onWheelN, { passive: false });
      wrap.addEventListener("contextmenu", onCtx);
      wrap.addEventListener("pointerleave", onLeave);

      return () => {
        themeMO.disconnect();
        ro.disconnect();
        wrap.removeEventListener("pointerdown", onDown);
        wrap.removeEventListener("pointermove", onMove);
        wrap.removeEventListener("pointerup", onUp);
        wrap.removeEventListener("pointercancel", onUp);
        wrap.removeEventListener("dblclick", onDbl);
        wrap.removeEventListener("wheel", onWheelN);
        wrap.removeEventListener("contextmenu", onCtx);
        wrap.removeEventListener("pointerleave", onLeave);
        cancelPendingHover();
        if (raf.current) cancelAnimationFrame(raf.current);
        raf.current = 0;
        const g = glCtx.current;
        if (g && prog.current) {
          g.deleteBuffer(prog.current.buffer);
          g.deleteVertexArray(prog.current.vao);
          g.deleteProgram(prog.current.program);
        }
        prog.current = null;
        glCtx.current = null;
        instCount.current = 0;
        instFrame.current = null;
        // plate pool: the nodes die with the layer (React unmounts it); just
        // drop the bookkeeping so a remount starts clean
        slots.current = [];
        slotOf.current.clear();
        reqFirst.current.clear();
        reqLast.current.clear();
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // prefers-reduced-motion, live: the same law the CSS layer enforces on
    // its transitions (primitives.css clamps every duration to 0.01ms) for
    // the JS viewport animations — the nav/zoom animation collapses to a
    // cut and the wheel lerp snaps. One listener at mount, never a per-frame
    // matchMedia query.
    useEffect(() => {
      const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
      const apply = () => {
        reducedMotion.current = mq.matches;
      };
      apply();
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }, []);

    // entries / theme / previews / expanded arrive as props; rebuild lazily
    // (the draw pipeline calls ensureFrame, and this kick guarantees a draw
    // is scheduled). previews is in the list so a payload arrival re-runs
    // updatePreviews even while the user holds still; expanded is there for
    // the same reason — an interior arrival rebuilds the frame (ensureFrame
    // identity) and must schedule the draw that paints it, even at rest.
    // theme also reloads the CSS palette (GL uniforms + marquee strings).
    useEffect(() => {
      loadPalette();
    }, [props.theme]);

    // Interior-expansion bookkeeping belongs to a NAVIGATION, exactly like
    // the preview request generation: entryIds from the view we left must
    // not suppress requests in the view we entered. Deliberately keyed on
    // `entries` only — an `expanded` arrival is the ANSWER to a request, and
    // clearing the Set for it would re-ask the same folders forever.
    useEffect(() => {
      expandAsked.current.clear();
    }, [props.entries]);

    useEffect(() => {
      kick();
    }, [
      props.entries,
      props.parentSize,
      props.theme,
      props.hiddenCats,
      props.highlightIds,
      props.previews,
      props.expanded,
    ]);

    useImperativeHandle(ref, (): TreemapHandle => ({
      beginNav(dir, rect) {
        pendingNav.current = rect ? { dir, rect } : { dir };
      },
      zoomToTile(entryId) {
        const f = frame.current;
        const rect = f ? findWorldRect(f, entryId) : null;
        if (!rect) return;
        // "Jump to" aims the tile at 94% of the canvas, which is under the
        // floor when the tile is (nearly) the whole world — lift to the fit
        // floor first, then centre on the tile with the clamped scale so the
        // tile stays the focus. (animateTo re-checks; this keeps the intent
        // explicit at the source.)
        const scale = Math.max(
          fit().scale,
          Math.min(
            (cssW.current * 0.94) / rect.w,
            (cssH.current * 0.94) / rect.h,
          ),
        );
        animateTo({
          scale,
          tx: cssW.current / 2 - (rect.x + rect.w / 2) * scale,
          ty: cssH.current / 2 - (rect.y + rect.h / 2) * scale,
        });
      },
      resetView() {
        pendingNav.current = null;
        animateTo(fit());
      },
      zoomBy(factor) {
        const base = fit();
        const cx = cssW.current / 2;
        const cy = cssH.current / 2;
        // Keyboard/command zoom obeys the same floor as the wheel: below
        // fit is unreachable, so "zoom out" clamps to 100% and stays there.
        // The ceiling follows the tile at the canvas centre (zoomCeilingAt,
        // BUG 2) for the same reason the wheel's does: deep zoom must be able
        // to reach that tile's preview gate.
        const next = Math.min(
          zoomCeilingAt(cx, cy),
          Math.max(base.scale, vp.current.scale * factor),
        );
        const k = next / vp.current.scale;
        animateTo({
          scale: next,
          tx: cx - (cx - vp.current.tx) * k,
          ty: cy - (cy - vp.current.ty) * k,
        });
      },
      clearSelection() {
        sel.current.clear();
        kick();
      },
      setSelection(ids: number[]) {
        sel.current = new Set(ids);
        kick();
      },
      getWorldRect(entryId) {
        const f = frame.current;
        return f ? findWorldRect(f, entryId) : null;
      },
    }));

    return (
      <div ref={wrapRef} style={{ position: "absolute", inset: 0, touchAction: "none" }}>
        <canvas ref={glCanvasRef} className="map-gl" />
        <canvas ref={labelRef} className="map-labels" />
        {/* In-tile preview plates (INS_BRIEF §2): a static layer, empty in
            React's output — the draw loop pools/fills/positions the plates
            imperatively, and pointer-events:none (preview.css) keeps left-
            drag pan the primary gesture. */}
        <div ref={plateLayerRef} className="map-previews" aria-hidden="true" />
        {/* Instrument frame: crop marks bracket the data field; the zoom
            readout is updated imperatively from the draw loop. */}
        <div className="map-instruments" aria-hidden="true">
          <div className="map-crop crop" />
          <div ref={zoomRef} className="map-zoom" />
        </div>
      </div>
    );
  },
);

function formatTileMeta(size: number, total: number): string {
  const pct = (size / total) * 100;
  const p = pct >= 10 ? pct.toFixed(0) : pct >= 0.1 ? pct.toFixed(1) : "<0.1";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = size;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[u]} · ${p}%`;
}
