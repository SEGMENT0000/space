// Treemap layout: entries -> world-space tiles. The world is built at the
// container's exact aspect ratio, and the fit transform is a COVER (scale+centre)
// so the fill invariant holds structurally: at matching aspect cover == contain
// == an exact edge-to-edge fill; on any aspect drift it overfills and crops
// rather than letterboxing, because background is the one thing that is
// forbidden. Colors are resolved here (theme-aware) and shipped with the
// geometry; dimming/highlight is a renderer concern.
//
// The partition itself is implemented in-module (Bruls squarified: grow a row
// along the rect's shortest side while the row's worst aspect ratio improves,
// then dice it). WHY NOT d3: the contract below needs guarantees d3 does not
// give — an EXACT fp partition (d3's round(true) perturbed every coordinate),
// no NaN on all-zero frames (d3 divides by a zero total), a minimum-tile floor
// that MERGES sub-pixel slivers instead of emitting them, and no per-build
// object churn (d3 allocates a HierarchyNode per entry). See "LAYOUT INVARIANTS".
//
// LAYOUT INVARIANTS (what consumers may rely on):
//   1. tiles/refs are pushed in lockstep, 1:1; every ancestor is pushed before
//      its descendants, so a reverse hit-test walk meets the deepest tile
//      first. Depth 0 = a child of the current folder; depth N = a child
//      inside N nested interiors (see MAX_INTERIOR_DEPTH). Interiors are laid
//      out level by level, so siblings are not necessarily contiguous — only
//      the ancestor-before-descendant order is load-bearing.
//   2. Depth-0 cells tile the world EXACTLY: sum(cell) == world area in fp,
//      shared boundaries are bit-identical, so there are no overlaps and no
//      area loss. Area conservation for the level is exact.
//   3. Interiors are allocated BREADTH-FIRST and area-prioritised: the work
//      list for a child depth is sorted by tile area desc (insertion order
//      breaks ties), so every large folder at that level gets its first
//      interior before any folder goes a level deeper. Each interior is also
//      capped by area (CHILD_CAP_MIN..MAX) so one 900-child folder cannot
//      turn into confetti. An interior lies inside its folder's TILE rect
//      (never its cell), inset by a size-aware gutter — children never leak
//      into the gutter that separates the folder from its siblings, and
//      siblings cannot overlap because each interior is partitioned from its
//      parent's disjoint rect. Interiors stop at MAX_INTERIOR_DEPTH /
//      MAX_FRAME_TILES; interior conservation is exact minus that gutter,
//      except where a cap truncates the tail.
//   4. Every emitted tile is >= MIN_TILE_WORLD on both sides; smaller cells
//      merge into an adjacent same-row cell (sliver merge) and, only where
//      geometry forbids a merge, are dropped — bounded, sub-pixel, documented.
//   5. Deterministic: size-descending order (caller order kept when already
//      size-desc, otherwise sorted by size desc / entry index asc), no
//      randomness, no rounding. Same input -> same output, bit for bit.
//   6. All loops are bounded by the entry count, MAX_INTERIOR_DEPTH and
//      MAX_FRAME_TILES; weights are sanitized (non-finite/negative -> 0)
//      before they touch any division.

import { categoryStyle, inkFor, tintFill } from "../colors";
import { formatCount } from "../format";
import type { ChildEntry } from "../types";
import type { TreemapFrameData, TileRef, RenderTile, Viewport, WorldRect } from "./types";

/** Collapse a contentless interior into a listing card.
 *
 *  A treemap reproduces structure, but some interiors are structure without
 *  content: a system folder whose every visible child is a sub-gate file or a
 *  folder too small to card. Painting that as a field of hatching and micro
 *  labels is honest but useless. This pass marks the HIGHEST such folder on a
 *  branch as card-eligible and removes its descendants from the frame; the
 *  canvas already knows how to plate a leaf folder, so the tile becomes the
 *  folder's real top-entry listing.
 *
 *  "Content" is a tile that can carry a full card: a leaf folder or a file at
 *  the 180×120 LIST floor. A smaller file would only earn a compact metadata
 *  card and should not veto the folder listing that would show it by name. A marked
 *  folder counts as content for its parent, which keeps the most specific
 *  branch: Windows cards only when System32/WinSxS have no cards of their own,
 *  and AppData keeps its map when Chrome is card-worthy. The HUGE-card cap
 *  keeps a sparse slab from replacing a genuinely useful map.
 *
 *  Pure and deterministic; runs once per frame rebuild (not per draw). */
export interface CollapseContentlessOptions {
  /** CSS px per world px at the fitted view. */
  scale: number;
  /** Map viewport in CSS px, for the sparse-card cap. */
  viewportW: number;
  viewportH: number;
}

const CONTENT_CARD_MIN_W = 180;
const CONTENT_CARD_MIN_H = 120;
// A file below the full card floor is not treated as content: it would only
// earn a small metadata card, and letting it veto the folder card hides the
// larger, more useful listing. At/above the floor the file is a real plate
// and the interior is the better view.
const CONTENT_FILE_MIN_W = 180;
const CONTENT_FILE_MIN_H = 120;
const CONTENT_CARD_MAX_W = 560;
const CONTENT_CARD_MAX_H = 440;
const CONTENT_CARD_MAX_AREA_FRACTION = 0.18;

export function collapseContentlessInteriors(
  frame: TreemapFrameData,
  opts: CollapseContentlessOptions,
): TreemapFrameData {
  const { tiles, refs } = frame;
  const n = refs.length;
  const { scale, viewportW, viewportH } = opts;
  if (n === 0 || !(scale > 0) || !(viewportW > 0) || !(viewportH > 0)) return frame;
  const maxArea = viewportW * viewportH * CONTENT_CARD_MAX_AREA_FRACTION;

  const children = new Map<number, number[]>();
  const indexById = new Map<number, number>();
  for (let i = 0; i < n; i++) indexById.set(refs[i].entryId, i);
  for (let i = 0; i < n; i++) {
    const parent = refs[i].parentEntryId;
    if (parent == null) continue;
    let list = children.get(parent);
    if (!list) {
      list = [];
      children.set(parent, list);
    }
    list.push(i);
  }

  const marked = new Uint8Array(n);
  const visit = (i: number): boolean => {
    const ref = refs[i];
    const tile = tiles[i];
    const w = tile.w * scale;
    const h = tile.h * scale;
    if (!(w > 0) || !(h > 0)) return false;
    if (!ref.isDir) {
      // Aggregates have no filesystem identity and are never content.
      return ref.entryId >= 0 && w >= CONTENT_FILE_MIN_W && h >= CONTENT_FILE_MIN_H;
    }
    const kids = children.get(ref.entryId);
    if (!kids || kids.length === 0) {
      // A leaf folder is already a card candidate at this exact floor.
      return w >= CONTENT_CARD_MIN_W && h >= CONTENT_CARD_MIN_H;
    }
    let childContent = false;
    for (let k = 0; k < kids.length; k++) {
      if (visit(kids[k])) childContent = true;
    }
    if (childContent) return true;
    if (
      w >= CONTENT_CARD_MIN_W &&
      h >= CONTENT_CARD_MIN_H &&
      w <= CONTENT_CARD_MAX_W &&
      h <= CONTENT_CARD_MAX_H &&
      w * h <= maxArea
    ) {
      // Contentless and card-sized: this folder becomes the content edge.
      // Its marked flag is content for the ancestors, so cards never nest.
      marked[i] = 1;
      return true;
    }
    return false;
  };

  for (let i = 0; i < n; i++) {
    if (refs[i].parentEntryId == null) visit(i);
  }

  let anyMarked = false;
  for (let i = 0; i < n; i++) {
    if (marked[i]) {
      anyMarked = true;
      break;
    }
  }
  if (!anyMarked) return frame;

  // Remove every ref under a marked folder. The marked folder's own tile stays
  // (now childless) so the canvas can request and plate its listing.
  const kept = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    let parent = refs[i].parentEntryId;
    let underCard = false;
    while (parent != null) {
      const pi = indexById.get(parent);
      if (pi === undefined) break;
      if (marked[pi]) {
        underCard = true;
        break;
      }
      parent = refs[pi].parentEntryId;
    }
    if (!underCard) kept[i] = 1;
  }

  const nextTiles: RenderTile[] = [];
  const nextRefs: TileRef[] = [];
  for (let i = 0; i < n; i++) {
    if (!kept[i]) continue;
    nextTiles.push(tiles[i]);
    nextRefs.push(refs[i]);
  }
  return { world: frame.world, tiles: nextTiles, refs: nextRefs };
}

export const WORLD_W = 1000;

// ------------------------------------------------------------------ LOD contract
//
// Projected (SCREEN) px thresholds shared by the renderer and the preview
// layer. The layout itself is world-space; at the fitted view world units and
// CSS px coincide to within the window-width / WORLD_W ratio (a 1000px-wide
// window maps 1 world unit to ~1 CSS px), which is how the nesting gate below
// is calibrated. Other modules import these — the numbers are part of the
// contract, not tuning knobs.

/** Preview trigger: a FILE tile whose projected area is below this never asks
 *  the parent for a payload (the request is what resolves the path and reads
 *  bytes — firing it for sub-9000px² tiles sprays the backend LRU with tiles
 *  no plate could ever show). Pairs with PREVIEW_MIN_SIDE_PX: area alone would
 *  qualify a 9000×1 sliver, which no family can render. */
export const PREVIEW_MIN_AREA_PX = 9000;

/** Sliver guard for the preview trigger: both projected sides must clear this.
 *  Below it the tile keeps its floating label even when the area qualifies. */
export const PREVIEW_MIN_SIDE_PX = 72;

/** True when a tile of projected size wPx×hPx is big enough to REQUEST (and
 *  later show) a preview plate. Allocation-free, NaN-safe (NaN fails every
 *  >= check). The renderer still applies the per-family render minimum
 *  (PLATE_MIN) on top: this gate only decides when to ask. */
export function previewQualified(wPx: number, hPx: number): boolean {
  return (
    wPx >= PREVIEW_MIN_SIDE_PX &&
    hPx >= PREVIEW_MIN_SIDE_PX &&
    wPx * hPx >= PREVIEW_MIN_AREA_PX
  );
}

/** Tile-label LOD tiers by projected area. The renderer maps tiers to type:
 *  `none` = no floating label at all (the tile is the readout),
 *  `micro` = 10.5px name only, ellipsized hard,
 *  `small` = 10.5px name, comfortable measure,
 *  `large` = the big voice (11.5/600) + category swatch + the size line —
 *  exactly the tier DESIGN.md calls "big tile labels". */
export type LabelTier = "none" | "micro" | "small" | "large";

/** Projected px² floors for each LabelTier, ascending. Documented shapes:
 *  1200 = 40×30 (a name line that can actually be read; below this a micro
 *  label is visual noise, user-verified 2026-10-20), 1600 = 40×40 (a whole
 *  name line with air), 3240 = 90×36 (the depth-0 "big" gate: h≥36 && w≥90,
 *  which is where the size line starts to fit beside the name). */
export const LABEL_TIER_MIN_PX = {
  none: 0,
  micro: 1200,
  small: 1600,
  large: 3240,
} as const;

/** Label tier for a tile's projected area (w × h × scale², see
 *  projectedAreaPx). NaN resolves to `none` — a degenerate tile must never
 *  earn ink. O(1), allocation-free: safe to call per tile per frame. */
export function labelTierFor(areaPx: number): LabelTier {
  if (!(areaPx >= LABEL_TIER_MIN_PX.micro)) return "none";
  if (areaPx < LABEL_TIER_MIN_PX.small) return "micro";
  if (areaPx < LABEL_TIER_MIN_PX.large) return "small";
  return "large";
}

/** Projected screen area of a world rect at a viewport scale — the quantity
 *  every LOD gate above consumes. */
export function projectedAreaPx(wWorld: number, hWorld: number, scale: number): number {
  const w = wWorld * scale;
  const h = hWorld * scale;
  const a = w * h;
  return a > 0 && Number.isFinite(a) ? a : 0;
}

/** Nested-interior gate, projected px²: a folder tile below this shows as a
 *  solid tinted cell (its children would each be a few px of mush) and, on
 *  the canvas, qualifies as a leaf folder for a content card. Above it, the
 *  interior is the folder's preview surface. The floor is deliberately far
 *  above the old 2400 px²: subdividing a 50×50 folder produced 20×20 tiles
 *  that could not carry a plate or a readable label, which is exactly the
 *  confetti the map is trying to avoid. 110×110 or a 105×150 folder still
 *  subdivides; a 100×100 folder reads as one labelled cell. */
export const NESTED_MIN_AREA_PX = 12000;

/** Nested-interior gate, projected px on the short side: area alone would
 *  subdivide a 1200×2 sliver whose children can only be slivers too. */
export const NESTED_MIN_SIDE_PX = 90;

/** The nesting gate itself: does a tile this size subdivide? Exported so the
 *  renderer's hover/drill copy can promise exactly what the layout does.
 *  World units at build time (see the header note on world≈px at fit). */
export function nestedInteriorApplies(wWorld: number, hWorld: number): boolean {
  if (!(wWorld >= NESTED_MIN_SIDE_PX) || !(hWorld >= NESTED_MIN_SIDE_PX)) return false;
  return wWorld * hWorld >= NESTED_MIN_AREA_PX;
}

// ---------------------------------------------------------------- layout constants

/** Minimum emitted tile size, world units on each side. Below this a tile is
 *  sub-pixel at the fitted view: it can never be hit, labelled, or read, so
 *  its cell MERGES into an adjacent same-row cell instead of being emitted
 *  (the sliver floor). Exported because "no tile smaller than this" is part
 *  of the frame contract. */
export const MIN_TILE_WORLD = 1.2;

/** Deepest interior the breadth-first passes may emit. `depth` is the depth OF
 *  THE CHILDREN: the first interior pass emits depth 1, so depth 5 is the last
 *  level that may receive children. Depth-5 tiles are leaves even when they
 *  name a directory — below this the next gutter would consume more than the
 *  tile and tintFill has already flattened the fill to its final step. */
export const MAX_INTERIOR_DEPTH = 5;

/** Hard ceiling on frame size once interiors are allocated. The depth-0 level
 *  is laid out first and reserves its tile count; interiors may spend the
 *  remainder, and `emitInterior` stops BEFORE pushing the next child when it
 *  is gone. A lazily expanded tree can grow while the user zooms, and a
 *  truncated map is the honest, bounded outcome; without it a pathological
 *  folder would stall the GPU instance upload and the label sort. The level
 *  itself keeps its own TILE_BUDGET and is never truncated by this cap, so an
 *  unexpanded frame keeps the exact old output — a level larger than this
 *  simply leaves no room for interiors. */
export const MAX_FRAME_TILES = 4000;

/** Gutter between interior siblings AND the ring inset from the folder tile's
 *  own edge, in world units. Scaled per interior (see interiorGutter) so a
 *  40px folder doesn't lose a third of its area to chrome while a big folder
 *  keeps a real channel. Depth 0 has no gutter: sibling separation there is
 *  the shader's fixed ~1px hairline seam, which is what keeps level area
 *  conservation EXACT (invariant 2). */
export const GUTTER_INTERIOR = 2;

/** Absolute cap on tiles emitted per layout level. A level with more entries
 *  than this truncates the sub-floor tail (their area redistributes through
 *  the partition's normalization); without it a million-node folder would
 *  hand the renderer a million instances and a million-index label sort. */
const TILE_BUDGET = 131072;

// One interior's child budget is about LEGIBILITY, not memory. A 700×540
// interior with 900 children is confetti; at the old 96-child cap each child
// averaged roughly 100 CSS px at fit, and four levels of that read as noise
// (user-verified 2026-10-20: "too conjusted"). The budget now targets a
// handful of large, nameable children per level — 4 at the nesting floor,
// 16 in a big interior — so the map stays a map and depth lives in the
// folder cards / drill / zoom rather than in a wall of slivers. This cap
// applies to interiors only: the depth-0 level must stay byte-identical to
// the old signature, so it keeps its TILE_BUDGET partition untouched.
const CHILD_AREA = 10000; // world² per child (~100×100 world at the cap)
const CHILD_CAP_MIN = 4;
const CHILD_CAP_MAX = 12;

/** A hidden-byte remainder below this fraction of its folder is not worth a
 *  dedicated cell: the visible children fill the parent instead, and the
 *  resulting inflation is under 1% — below anything a label can report. Above
 *  it, the omitted tail (cap, `sub` sample, or unreadable bytes) keeps its
 *  true area as one labelled “N more” tile, so a 26 GB folder can never draw
 *  at a 90 GB share. */
const REMAINDER_MIN_FRACTION = 0.01;

/** When the omitted tail is most of a folder, the interior is a roll-up, not
 *  a map: the visible children cannot carry the tile. If the tile is also big
 *  enough for a list card (the same 180×120 floor the canvas uses), emit NO
 *  children and let the canvas plate the folder's real top entries. This is
 *  what turns a wall of 4,805 tiny WinSxS cells into a readable listing while
 *  keeping a genuinely mixed folder (AppData, Program Files) as a map. */
const AGGREGATE_CARD_SHARE = 0.55;
const AGGREGATE_CARD_MIN_W = 180;
const AGGREGATE_CARD_MIN_H = 120;

/** Fp slack for the sliver merge (fp addition groups differently when a cell
 *  grows, so "exactly equal" can drift ~1 ULP). Real gaps are
 *  >= MIN_TILE_WORLD, so this can never bridge distinct cells. */
const MERGE_EPS = 1e-6;

// ------------------------------------------------------------------- scratch
// One layout pass must not allocate (it runs on rebuilds, which ride the same
// rAF loop as everything else). These grow on demand and never shrink; they
// are module-level because the layout is synchronous and non-reentrant.
// cells0/slots0 and cells1/slots1 are SEPARATE so a folder interior built
// mid-iteration can never clobber the level cells being iterated.

let orderBuf = new Int32Array(0); // entry indices, size-desc
let weightBuf = new Float64Array(0); // sanitized weights, slot-aligned
let cells0 = new Float64Array(0); // depth-0 cells: x,y,w,h per cell
let slots0 = new Int32Array(0); // cell -> entry index
let cells1 = new Float64Array(0); // interior cells
let slots1 = new Int32Array(0);

function ensureOrder(n: number): void {
  if (orderBuf.length < n) {
    orderBuf = new Int32Array(n);
    weightBuf = new Float64Array(n);
  }
}

function ensureCells0(n: number): void {
  if (cells0.length < n * 4) {
    cells0 = new Float64Array(n * 4);
    slots0 = new Int32Array(n);
  }
}

function ensureCells1(n: number): void {
  if (cells1.length < n * 4) {
    cells1 = new Float64Array(n * 4);
    slots1 = new Int32Array(n);
  }
}

/** Bytes -> layout weight. Non-finite or negative sizes are not data: they
 *  would poison every sum and division downstream (NaN rects), so they count
 *  as 0. */
function sanitizeSize(size: number): number {
  return Number.isFinite(size) && size > 0 ? size : 0;
}

/** Fill orderBuf[0..n) with entry indices in layout order and weightBuf with
 *  the matching sanitized weights; returns the ACTIVE count (entries with
 *  weight > 0 — 0-byte files take no area and get no tile, which is the
 *  honest rendering of 0 bytes).
 *
 *  Order: caller order when it is already size-descending (the common case —
 *  ExplorerView's default sort), otherwise a deterministic sort by size desc
 *  / entry index asc. Squarified packing needs descending input for its
 *  aspect guarantee; trusting the caller's order would make quality depend on
 *  which sort tab the user picked. The caller's array is never mutated. */
function prepareLevel(list: ChildEntry[]): number {
  const n = list.length;
  if (n === 0) return 0;
  ensureOrder(n);
  const order = orderBuf;
  for (let i = 0; i < n; i++) order[i] = i;

  let total = 0;
  let sorted = true;
  let prev = Number.POSITIVE_INFINITY;
  for (let i = 0; i < n; i++) {
    const s = sanitizeSize(list[i].size);
    total += s;
    if (s > prev) sorted = false;
    prev = s;
  }
  if (!sorted) {
    const sub = order.subarray(0, n);
    sub.sort((a, b) => sanitizeSize(list[b].size) - sanitizeSize(list[a].size) || a - b);
  }

  const weights = weightBuf;
  if (total > 0) {
    let count = 0;
    for (let k = 0; k < n; k++) {
      const w = sanitizeSize(list[order[k]].size);
      weights[k] = w;
      if (w > 0) count = k + 1;
    }
    return count;
  }
  // Every entry is 0 bytes: there is no proportion to honor. An equal split
  // renders an honest grid of "0 B" cells instead of an empty field — and,
  // unlike a normalized-by-zero path, it cannot produce NaN geometry.
  for (let k = 0; k < n; k++) weights[k] = 1;
  return n;
}

// ----------------------------------------------------------------- squarify

/** Squarified partition (Bruls et al.): grow a row along the rect's shortest
 *  side while the row's worst aspect ratio improves, dice it, recurse into
 *  the remainder. `weights[0..count)` must be finite, > 0 and non-increasing;
 *  cells/slots are written in layout order (cell k's slot is slots[k]).
 *
 *  Worst-aspect closed form: for a row of sum R in a rect with remaining
 *  weight T, short side S and long side L, the strip thickness is R/T·L (it
 *  falls on the long side) and each item's extent along the short side is
 *  (w_i/R)·S, so
 *      worst = max( maxW·S·T / (R²·L),  R²·L / (minW·S·T) ).
 *  Sorted input makes maxW = weights[from] and minW = weights[to-1], so the
 *  candidate test is O(1) and the whole pass is O(n).
 *
 *  FP EXACTNESS: the row's last cell is snapped to the rect's far edge and
 *  the final row's thickness is snapped to the remaining long side, so
 *  cells share bit-identical boundaries — the partition sums to the rect with
 *  no overlaps and no rounding drift (invariant 2). */
function squarify(
  rx: number,
  ry: number,
  rw: number,
  rh: number,
  count: number,
  weights: Float64Array,
  cells: Float64Array,
  slots: Int32Array,
): number {
  if (count <= 0 || !(rw > 0) || !(rh > 0)) return 0;
  let rem = 0;
  for (let k = 0; k < count; k++) rem += weights[k];
  if (!(rem > 0)) return 0;

  let x = rx;
  let y = ry;
  let w = rw;
  let h = rh;
  let i = 0; // next slot to consider
  let out = 0; // cells written
  let rowStart = 0;
  let rowSum = 0;
  let rowWorst = Number.POSITIVE_INFINITY;

  // Dice row slots [from, to) (sum weights, all of the remaining rect's
  // weight share) into the current rect and advance it. Declared as a local
  // function so it closes over the mutable rect state; called at most n+1
  // times, each call consuming >= 1 slot.
  //
  // ORIENTATION (Bruls): the row is laid against the rect's SHORTEST side —
  // its items stack ALONG that short side, and the row's thickness falls on
  // the LONG side. Laying against the long side is what turns the final row
  // into a comb of full-width slivers.
  function diceRow(from: number, to: number, sum: number): void {
    const shortIsW = w <= h;
    const longSide = shortIsW ? h : w;
    // Last row: it owns all remaining weight, so it must fill the remainder
    // exactly — snap instead of trusting sum/rem to be 1.0 in fp.
    const thickness = to === count ? longSide : (sum / rem) * longSide;
    if (shortIsW) {
      // Short side is w: items stack right along x, thickness falls on y.
      let cx = x;
      for (let k = from; k < to; k++) {
        // Last item snaps to the strip's far edge (exact coverage).
        const len = k === to - 1 ? x + w - cx : (weights[k] / sum) * w;
        const o = out * 4;
        cells[o] = cx;
        cells[o + 1] = y;
        cells[o + 2] = len;
        cells[o + 3] = thickness;
        slots[out] = k;
        out++;
        cx += len;
      }
      y += thickness;
      h -= thickness;
    } else {
      // Short side is h: items stack down y, thickness falls on x.
      let cy = y;
      for (let k = from; k < to; k++) {
        const len = k === to - 1 ? y + h - cy : (weights[k] / sum) * h;
        const o = out * 4;
        cells[o] = x;
        cells[o + 1] = cy;
        cells[o + 2] = thickness;
        cells[o + 3] = len;
        slots[out] = k;
        out++;
        cy += len;
      }
      x += thickness;
      w -= thickness;
    }
    rem -= sum;
  }

  while (i < count) {
    const shortIsW = w <= h;
    const shortSide = shortIsW ? w : h;
    const longSide = shortIsW ? h : w;
    // Rect exhausted by fp dust: whatever weight is unplaced wants ~0 area
    // (rows take their weight-proportional share of the rect, so a collapsed
    // remainder implies an exhausted remainder). Stop; never spin.
    if (!(shortSide > 0)) break;

    const wi = weights[i];
    const candSum = rowSum + wi;
    // scale = S·T/L so that weights[rowStart]·scale/R² = maxW·S·T/(R²·L) and
    // R²/(wi·scale) = R²·L/(minW·S·T) — the two terms of the worst-aspect
    // closed form above. Short side S here, long side L.
    const scale = (rem * shortSide) / longSide;
    const r2 = candSum * candSum;
    const candWorst = Math.max((weights[rowStart] * scale) / r2, r2 / (wi * scale));

    if (rowStart < i && candWorst >= rowWorst) {
      // Adding slot i would worsen the row: dice what we have and re-consider
      // i against the fresh rect. An empty row never closes (rowStart < i),
      // so every iteration either consumes a slot or advances rowStart —
      // termination is structural.
      diceRow(rowStart, i, rowSum);
      rowStart = i;
      rowSum = 0;
      rowWorst = Number.POSITIVE_INFINITY;
      continue;
    }
    rowSum = candSum;
    rowWorst = candWorst;
    i++;
    if (i === count) {
      diceRow(rowStart, i, rowSum);
      rowStart = i;
    }
  }
  return out;
}

// -------------------------------------------------------- gutter + sliver floor

/** Interior gutter for a folder tile of this size: proportional (so small
 *  folders keep their area) with a floor that reads as a hairline and a cap
 *  that never turns into a channel. Pure function of the tile — deterministic
 *  across rebuilds. */
function interiorGutter(minSide: number): number {
  const g = minSide * 0.04;
  return g < 1 ? 1 : g > GUTTER_INTERIOR ? GUTTER_INTERIOR : g;
}

/** Inset a cell into module scratch `probe` (x, y, w, h). Depth-0 cells are
 *  emitted as-is (separation is the shader hairline — which is what keeps
 *  level area conservation EXACT, invariant 2); interior children are inset
 *  by half the interior gutter on every side, so sibling gaps read as a full
 *  gutter and the folder's own fill rings its children by half of one.
 *  Returns false when the result would be below the minimum-tile floor (or
 *  non-finite) — the caller merges instead of emitting. */
const probe = new Float64Array(4);

function probeTile(cx: number, cy: number, cw: number, ch: number, gutter: number): boolean {
  const ix = gutter * 0.5;
  const tw = cw - ix - ix;
  const th = ch - ix - ix;
  // !(x >= floor) rejects NaN as well as undersize — NaN must never emit.
  if (!(tw >= MIN_TILE_WORLD) || !(th >= MIN_TILE_WORLD)) return false;
  probe[0] = cx + ix;
  probe[1] = cy + ix;
  probe[2] = tw;
  probe[3] = th;
  return true;
}

/** Do two rects share a full edge (same-row/column adjacency)? Rect-safe
 *  merging — growing one to cover the other — is only possible in that case,
 *  which is why the sliver floor merges within a row and nowhere else. */
function mergeable(
  ax: number,
  ay: number,
  aw: number,
  ah: number,
  bx: number,
  by: number,
  bw: number,
  bh: number,
): boolean {
  const vertical =
    Math.abs(ax - bx) < MERGE_EPS &&
    Math.abs(aw - bw) < MERGE_EPS &&
    Math.abs(ay + ah - by) < MERGE_EPS;
  const horizontal =
    Math.abs(ay - by) < MERGE_EPS &&
    Math.abs(ah - bh) < MERGE_EPS &&
    Math.abs(ax + aw - bx) < MERGE_EPS;
  return vertical || horizontal;
}

/** True when the two rects are same-COLUMN neighbours (stacked down y) as
 *  opposed to same-ROW (stacked right) — decides which axis grows. */
function sameColumn(
  ax: number,
  aw: number,
  bx: number,
  bw: number,
): boolean {
  return Math.abs(ax - bx) < MERGE_EPS && Math.abs(aw - bw) < MERGE_EPS;
}

/** Sliver floor pass: compact `cells`/`slots` in place so that no surviving
 *  cell probes below MIN_TILE_WORLD on either side. A sub-minimum cell merges
 *  into the same-row neighbour it touches (previous first, else next — both
 *  rect-safe); only a cell with no mergeable neighbour is dropped, leaving at
 *  most a sub-pixel hole. Returns the new cell count. O(n), no allocation
 *  beyond two module-independent locals-per-call closures (one call per
 *  level, never per tile). */
function mergeSlivers(
  cells: Float64Array,
  slots: Int32Array,
  count: number,
  gutter: number,
): number {
  if (count <= 0) return 0;
  if (count === 1) {
    return probeTile(cells[0], cells[1], cells[2], cells[3], gutter) ? 1 : 0;
  }

  let out = 0; // cells committed to positions [0, out)
  let hasPend = false;
  let pendOk = false;
  let hx = 0;
  let hy = 0;
  let hw = 0;
  let hh = 0;
  let hs = 0;

  function write(idx: number, x: number, y: number, w: number, h: number, slot: number): void {
    const o = idx * 4;
    cells[o] = x;
    cells[o + 1] = y;
    cells[o + 2] = w;
    cells[o + 3] = h;
    slots[idx] = slot;
  }

  /** Commit the pending cell: emit it, absorb it into the last committed
   *  cell when they share an edge, or drop it (a bounded, sub-pixel hole). */
  function flushPend(): void {
    if (pendOk) {
      write(out, hx, hy, hw, hh, hs);
      out++;
      return;
    }
    if (out > 0) {
      const o = (out - 1) * 4;
      const lx = cells[o];
      const ly = cells[o + 1];
      const lw = cells[o + 2];
      const lh = cells[o + 3];
      if (mergeable(lx, ly, lw, lh, hx, hy, hw, hh)) {
        if (sameColumn(lx, lw, hx, hw)) cells[o + 3] = lh + hh; // grow down
        else cells[o + 2] = lw + hw; // grow right
        return;
      }
    }
    // No mergeable neighbour: drop. Area lost is below one floor cell.
  }

  for (let k = 0; k < count; k++) {
    const o = k * 4;
    let cx = cells[o];
    let cy = cells[o + 1];
    let cw = cells[o + 2];
    let ch = cells[o + 3];
    const cs = slots[k];
    const ok = probeTile(cx, cy, cw, ch, gutter);

    if (hasPend) {
      const mg = mergeable(hx, hy, hw, hh, cx, cy, cw, ch);
      if (mg && !ok) {
        // k is a sliver: absorb it into the pending cell, then re-probe —
        // two merged slivers can together clear the floor.
        if (sameColumn(hx, hw, cx, cw)) hh += ch;
        else hw += cw;
        pendOk = probeTile(hx, hy, hw, hh, gutter);
        continue;
      }
      if (mg && ok && !pendOk) {
        // The pending cell is the sliver: fold it backward into k (k extends
        // up/left over it — same row or column, so the union is a rect).
        if (sameColumn(hx, hw, cx, cw)) {
          cy = hy;
          ch = cells[o + 1] + cells[o + 3] - hy;
        } else {
          cx = hx;
          cw = cells[o] + cells[o + 2] - hx;
        }
        write(k, cx, cy, cw, ch, cs);
        hx = cx;
        hy = cy;
        hw = cw;
        hh = ch;
        hs = cs;
        pendOk = true;
        continue;
      }
      // Every remaining case commits the pending cell (emit it, absorb it
      // into the last committed neighbour, or drop it) and takes k as the
      // new pending cell.
      flushPend();
    }

    hasPend = true;
    pendOk = ok;
    hx = cx;
    hy = cy;
    hw = cw;
    hh = ch;
    hs = cs;
  }
  if (hasPend) flushPend();
  return out;
}

// ------------------------------------------------------------------- build

/** Tiles emitted for one level never exceed what the area can hold one floor
 *  cell each — beyond that the sub-floor tail truncates and its area
 *  redistributes through the partition's normalization (see TILE_BUDGET). */
function tileBudget(area: number): number {
  const byFloor = area / (MIN_TILE_WORLD * MIN_TILE_WORLD);
  if (!(byFloor >= 1)) return 1;
  return byFloor < TILE_BUDGET ? Math.floor(byFloor) : TILE_BUDGET;
}

/** How many children ONE interior may emit: its area divided into
 *  CHILD_AREA-sized cells, clamped for legibility. NaN-safe (a degenerate
 *  area gets the floor). The depth-0 level never calls this. */
function childBudget(area: number): number {
  const byArea = Math.floor(area / CHILD_AREA);
  if (!(byArea >= CHILD_CAP_MIN)) return CHILD_CAP_MIN;
  return byArea < CHILD_CAP_MAX ? byArea : CHILD_CAP_MAX;
}

/** One directory tile waiting for its interior pass. The rect is the TILE
 *  rect (already gutter-inset by its parent's partition), copied at push time
 *  because module scratch is reused by every following interior. */
interface PendingInterior {
  entry: ChildEntry;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Monotone insertion order: deterministic tie-break for equal tile areas,
   *  independent of the engine's sort stability. */
  seq: number;
}

/** Build the frame in TWO layout passes (the wrong-proportions fix).
 *
 *  PASS 1 lays out the level by TRUE entry sizes: a directory participates as
 *  a LEAF valued by `entry.size`, so the cell it wins is exactly the share
 *  its label reports. (The old single hierarchy valued a directory by the sum
 *  of its `sub` sample — but the backend caps that sample at the top-8
 *  (engine.rs), so a 749 MB folder collapsed to the area of its 8 largest
 *  files while its label read "749 MB · 98%".)
 *
 *  PASS 2 lays out folder interiors BREADTH-FIRST, one child depth at a time:
 *  after PASS 1, every depth-0 directory becomes a work item. For childDepth
 *  1..MAX_INTERIOR_DEPTH the work list is sorted by TILE AREA descending and
 *  every eligible folder gets its interior before any of its children are
 *  promoted to the next depth. That allocation is the point: depth-first
 *  recursion let one 60%-of-the-map branch consume the entire frame budget,
 *  leaving equally large siblings as flat cells. Each interior is squarified
 *  to fill the folder's TILE rect exactly (translated by the tile origin),
 *  but ONLY above the nesting gate (nestedInteriorApplies): a folder too small
 *  to subdivide shows as one tinted cell, because children a few px wide are
 *  mush, not structure. Its child count is also capped by area (childBudget)
 *  so a 900-child folder cannot become confetti. Children come from `expanded`
 *  when it carries a real list, else from the entry's one-level `sub` sample.
 *  No interior ever touches the gutter that separates its folder from
 *  siblings, so a parent boundary can never be overdrawn by a child.
 *
 *  The depth-0 partition is byte-identical to the old signature for the same
 *  entries: the optional `expanded` map only feeds interiors.
 *
 *  `parentSize` is the frame total the renderer diffs for rebuild identity;
 *  it never enters the geometry (proportions come from the entries
 *  themselves, so filtered or inconsistent totals still lay out honestly).
 *
 *  Contract (invariants at the top of this file): tiles/refs 1:1, ancestors
 *  pushed before descendants, parentEntryId linkage at every level, children
 *  inside their parent's rect, every emitted tile >= MIN_TILE_WORLD per side. */
export function buildFrame(
  entries: ChildEntry[],
  parentSize: number,
  aspect: number,
  theme: "light" | "dark",
  expanded?: ReadonlyMap<number, ChildEntry[]>,
): TreemapFrameData {
  // Read only through `void` below: the value is part of the public contract
  // (rebuild identity lives in the renderer), not of the geometry.
  void parentSize;
  const worldW = WORLD_W;
  // Degenerate-measure guard: a NaN/0/negative aspect falls back to the
  // steady 0.62 aspect; an INFINITE aspect would drive worldH to 0 — the
  // frame would build empty and draw bare ground, so it is caught here
  // alongside the NaN case. The floor is MIN_TILE_WORLD (not 1): a world
  // shorter than the per-tile minimum would fail the sliver floor on EVERY
  // cell and emit nothing — the one outcome the fill invariant forbids.
  const worldH =
    aspect > 0 && isFinite(aspect)
      ? Math.max(WORLD_W / aspect, MIN_TILE_WORLD)
      : WORLD_W * 0.62;
  const world = { w: worldW, h: worldH };

  const tiles: RenderTile[] = [];
  const refs: TileRef[] = [];

  // Recursive-expansion allowance. The depth-0 level is emitted first and
  // keeps its own TILE_BUDGET, so interiors must reserve room for the whole
  // level to keep MAX_FRAME_TILES an actual frame ceiling. Both are assigned
  // before the first interior call below; the defaults only keep push total.
  let interiorCap = MAX_FRAME_TILES;
  let interiorTiles = 0;

  const n = entries.length;
  if (n === 0) return { world, tiles, refs };
  ensureCells0(n);

  /** Emit one tile. The final guard is defense-in-depth: mergeSlivers has
   *  already merged every sub-floor cell, and `!(w >= floor)` also rejects
   *  NaN so a degenerate rect can never reach the renderer. The frame cap is
   *  repeated here so no caller can push an interior child past the ceiling;
   *  depth-0 keeps its own TILE_BUDGET (an unexpanded level is never
   *  truncated by this cap). */
  const push = (
    x: number,
    y: number,
    w: number,
    h: number,
    depth: number,
    entry: ChildEntry,
    parentId: number | null,
  ): void => {
    if (depth > 0 && interiorTiles >= interiorCap) return;
    if (!(w >= MIN_TILE_WORLD) || !(h >= MIN_TILE_WORLD)) return;
    // Depth ladder (INS_BRIEF §3), color-only: children resolve their fill
    // through tintFill() — one flat step toward the canvas background per
    // level, uniform across the level, never a spatial gradient — and label
    // ink is re-derived from the TINTED fill via inkFor(), because the 4.5:1
    // contrast floor must hold on the color actually drawn.
    const style = categoryStyle(entry.cat, theme);
    const fill = tintFill(style.fill, depth, theme);
    tiles.push({
      x,
      y,
      w,
      h,
      fill,
      text: inkFor(fill),
      hatch: style.hatch,
      depth,
    });
    refs.push({
      entryId: entry.id,
      parentEntryId: parentId,
      name: entry.name,
      size: entry.size,
      cat: entry.cat,
      isDir: entry.isDir,
      depth,
    });
    if (depth > 0) interiorTiles++;
  };

  let pending: PendingInterior[] = [];
  let queueSeq = 0;

  /** Lay out ONE folder's interior at `depth` (the depth OF ITS CHILDREN) and
   *  append its directory children to `next`. The caller runs a whole child
   *  depth before advancing, which is what guarantees every large folder gets
   *  its first interior before any branch goes deeper.
   *
   *  Scratch safety: this call completes its prepareLevel -> squarify ->
   *  mergeSlivers -> push loop before the next call. Queue items carry their
   *  own rects, so there is no mid-loop recursion and no parallel `emitted`
   *  array: orderBuf, weightBuf, cells1, slots1 and probe are free to be
   *  reused by the next interior. Bounded by the nesting gate, childBudget,
   *  MAX_INTERIOR_DEPTH and the frame allowance. */
  function emitInterior(
    entry: ChildEntry,
    x: number,
    y: number,
    w: number,
    h: number,
    depth: number,
    next: PendingInterior[],
  ): void {
    if (!entry.isDir) return;
    if (depth > MAX_INTERIOR_DEPTH) return;
    if (interiorTiles >= interiorCap) return;

    // `expanded` wins when it carries a real child list; the one-level `sub`
    // sample remains the fallback, so an unexpanded map behaves exactly like
    // the old signature. An EMPTY expansion falls back too: the parent stores
    // `[]` both for a genuinely empty folder and for a failed fetch, and a
    // transient IPC failure must never erase an interior the payload already
    // described. When both are empty the tile is honestly a leaf.
    const exp = expanded?.get(entry.id);
    const kids = exp && exp.length > 0 ? exp : entry.sub;
    if (!kids || kids.length === 0) return;
    if (!nestedInteriorApplies(w, h)) return;

    const parentBytes = sanitizeSize(entry.size);

    // A file-heavy folder is a list, not a map. If no child directory is
    // large enough to clear the 180×120 folder-card floor on its own, every
    // visible child would be a small file or a label that cannot carry
    // content — exactly the flat wall this map is trying to replace. When
    // the folder's own tile clears that floor, emit no children so the canvas
    // plates its actual top entries. A genuinely mixed folder with one or more
    // large child directories still maps normally; those children then get
    // their own cards.
    if (parentBytes > 0 && w >= AGGREGATE_CARD_MIN_W && h >= AGGREGATE_CARD_MIN_H) {
      const cardShare = (AGGREGATE_CARD_MIN_W * AGGREGATE_CARD_MIN_H) / (w * h);
      let maxChildDir = 0;
      for (let k = 0; k < kids.length; k++) {
        const kid = kids[k];
        if (kid.isDir && kid.size > maxChildDir) maxChildDir = kid.size;
      }
      if (maxChildDir < parentBytes * cardShare) return;
    }

    // Room for one aggregate slot on top of the real children. Must happen
    // BEFORE prepareLevel fills the sorted buffers: ensureOrder reallocates
    // and wipes, so a later grow would lose the sorted weights.
    ensureOrder(kids.length + 2);
    let ci = prepareLevel(kids);
    if (ci <= 0) return;
    // Two caps, two jobs: childBudget keeps ONE interior legible (no
    // 900-child confetti); tileBudget is the structural area floor shared by
    // every level. The depth-0 level never passes through here.
    ci = Math.min(ci, childBudget(w * h), tileBudget(w * h));

    // ---- honest remainder ------------------------------------------------
    // Squarify fills the parent with the visible top-ci children, which
    // silently RENORMALIZES them. When the parent's scanned bytes exceed
    // their sum (the child cap dropped a tail, the API handed us a `sub`
    // sample, or the walk could not read part of the subtree), that
    // normalization inflates every visible child: the 26 GB Chrome folder
    // used to draw with a 90 GB share. Keep the omitted bytes' real share as
    // one labelled remainder tile (“17 more”) instead. It is layout-only —
    // negative id, not a scan node, never previewed or plated; clicking it
    // drills into its parent folder, where the depth-0 level has no cap.
    let agg: ChildEntry | null = null;
    let active = ci;
    if (parentBytes > 0) {
      let visibleBytes = 0;
      for (let k = 0; k < ci; k++) visibleBytes += weightBuf[k];
      const remainder = parentBytes - visibleBytes;
      if (remainder > parentBytes * REMAINDER_MIN_FRACTION) {
        const hiddenCount = Math.max(0, Math.round(entry.kidCount) - ci);
        agg = {
          id: -entry.id - 1, // unique per parent; real ids are >= 0
          name: hiddenCount > 0 ? `${formatCount(hiddenCount)} more` : "other",
          size: remainder,
          isDir: false,
          fileCount: 0,
          cat: entry.cat,
          kidCount: 0,
        };
        // Insert at its descending-weight position: squarify's aspect
        // guarantee assumes non-increasing input, and a large tail belongs
        // among the visible weights rather than pinned to the end.
        let at = 0;
        while (at < ci && weightBuf[at] >= remainder) at++;
        for (let k = ci; k > at; k--) {
          weightBuf[k] = weightBuf[k - 1];
          orderBuf[k] = orderBuf[k - 1];
        }
        weightBuf[at] = remainder;
        orderBuf[at] = -1; // sentinel: aggregate, not kids[-1]
        active = ci + 1;
      }
    }

    // A roll-up that would dominate the folder is better served by a real
    // listing than by a hatched remainder cell. Emitting no children leaves
    // this folder tile as a leaf; the canvas then requests a folder card and
    // fills the tile with its true top entries. Only large-enough tiles take
    // this path (the card needs room for legible columns); smaller folders
    // keep the labelled remainder so the count/size is never lost.
    if (
      agg &&
      parentBytes > 0 &&
      agg.size > parentBytes * AGGREGATE_CARD_SHARE &&
      w >= AGGREGATE_CARD_MIN_W &&
      h >= AGGREGATE_CARD_MIN_H
    ) {
      return;
    }

    ensureCells1(active);
    let mi = squarify(x, y, w, h, active, weightBuf, cells1, slots1);
    // Slots -> entry indices NOW: the next interior will reuse orderBuf.
    for (let j = 0; j < mi; j++) slots1[j] = orderBuf[slots1[j]];
    const gutter = interiorGutter(w < h ? w : h);
    mi = mergeSlivers(cells1, slots1, mi, gutter);

    for (let j = 0; j < mi; j++) {
      // Cap BEFORE pushing: a partially laid out tail is preferable to an
      // unbounded frame, and the break keeps the 1:1 push order intact.
      if (interiorTiles >= interiorCap) break;
      const p = j * 4;
      if (!probeTile(cells1[p], cells1[p + 1], cells1[p + 2], cells1[p + 3], gutter)) continue;
      const si = slots1[j];
      const child = si >= 0 ? kids[si] : agg;
      if (!child) continue;
      const cx = probe[0];
      const cy = probe[1];
      const cw = probe[2];
      const ch = probe[3];
      const before = tiles.length;
      push(cx, cy, cw, ch, depth, child, entry.id);
      // Queue only AFTER the parent tile exists, so ancestors always precede
      // descendants. Depth-5 children are leaves even if they are directories:
      // their own children would be below the tile floor.
      if (tiles.length > before && depth < MAX_INTERIOR_DEPTH && child.isDir) {
        next.push({ entry: child, x: cx, y: cy, w: cw, h: ch, seq: queueSeq++ });
      }
    }
  }

  // ---- PASS 1: the level itself, partitioned by true entry sizes.
  let count = prepareLevel(entries);
  if (count > 0) {
    const budget = tileBudget(worldW * worldH);
    if (count > budget) count = budget;
    let m = squarify(0, 0, worldW, worldH, count, weightBuf, cells0, slots0);
    // Slots -> entry indices NOW, before any interior reuses orderBuf.
    for (let k = 0; k < m; k++) slots0[k] = orderBuf[slots0[k]];
    m = mergeSlivers(cells0, slots0, m, 0);

    // Every surviving depth-0 cell passes probeTile, so `m` is exactly the
    // number of level tiles the loop below will push. Reserve all of them:
    // MAX_FRAME_TILES is then a true ceiling, while the level itself remains
    // byte-identical to the old signature (it is never truncated by this cap).
    interiorCap = Math.max(0, MAX_FRAME_TILES - m);

    for (let k = 0; k < m; k++) {
      const o = k * 4;
      if (!probeTile(cells0[o], cells0[o + 1], cells0[o + 2], cells0[o + 3], 0)) continue;
      const tx = probe[0];
      const ty = probe[1];
      const tw = probe[2];
      const th = probe[3];
      const entry = entries[slots0[k]];
      if (!entry) continue;
      push(tx, ty, tw, th, 0, entry, null);
      // Capture the tile rect now: probe is scratch and the interior pass
      // below overwrites it on every cell.
      if (entry.isDir) pending.push({ entry, x: tx, y: ty, w: tw, h: th, seq: queueSeq++ });
    }
  }

  // ---- PASS 2: breadth-first, area-prioritised interior passes.
  // One child depth at a time. Sorting the work list by TILE area means the
  // dominant folders get their interiors first, but no branch may advance to
  // the next depth until every folder at this depth has had its pass — the
  // allocation a depth-first walk could not promise. seq is the tie-break for
  // equal areas, so the result is deterministic without relying on sort
  // stability.
  for (let childDepth = 1; childDepth <= MAX_INTERIOR_DEPTH; childDepth++) {
    if (pending.length === 0 || interiorTiles >= interiorCap) break;
    pending.sort((a, b) => b.w * b.h - a.w * a.h || a.seq - b.seq);
    const next: PendingInterior[] = [];
    for (let q = 0; q < pending.length; q++) {
      if (interiorTiles >= interiorCap) break;
      const item = pending[q];
      emitInterior(item.entry, item.x, item.y, item.w, item.h, childDepth, next);
    }
    pending = next;
  }

  return { world, tiles, refs };
}

export function findWorldRect(frame: TreemapFrameData, entryId: number): WorldRect | null {
  const i = frame.refs.findIndex((r) => r.entryId === entryId);
  if (i < 0) return null;
  const t = frame.tiles[i];
  // Defense-in-depth on the crash class (FLEET §G): tiles/refs are pushed in
  // lockstep by construction, but a raw index must never dereference an
  // unaligned slot blindly.
  if (!t) return null;
  return { x: t.x, y: t.y, w: t.w, h: t.h };
}

/** Fit transform for a world of `world` inside a cssW×cssH canvas.
 *
 *  COVER, not contain. The renderer lays the frame out at the canvas's own
 *  aspect, where cover and contain coincide to within fp (both scale factors
 *  equal cssW/world.w) — so the steady-state behaviour is unchanged. But the
 *  fill invariant is "you can NEVER see background", and contain violates it
 *  the instant the frame's aspect and the canvas's disagree (a frame measured
 *  a resize ago, a nav transition landing on a rebuild, a transient layout
 *  measure): it letterboxes the world into a full-width thin band with dead
 *  void above/below — the "world did not refit" screenshot. Cover overfills
 *  and crops instead: every tile keeps its true proportion, the viewport is
 *  always fully covered, and the renderer's drift check rebuilds at the fresh
 *  aspect on the very next draw so the crop lasts at most one frame. */
export function fitViewport(
  world: { w: number; h: number },
  cssW: number,
  cssH: number,
): Viewport {
  const scale = Math.max(cssW / world.w, cssH / world.h);
  return {
    scale,
    tx: (cssW - world.w * scale) / 2,
    ty: (cssH - world.h * scale) / 2,
  };
}

/** Viewport the zoom-out departs from: the tile's rect fills the canvas. */
export function enterFromForZoomOut(
  rect: WorldRect,
  cssW: number,
  cssH: number,
): Viewport {
  const scale = Math.min(cssW / Math.max(rect.w, 1), cssH / Math.max(rect.h, 1));
  return {
    scale,
    tx: cssW / 2 - (rect.x + rect.w / 2) * scale,
    ty: cssH / 2 - (rect.y + rect.h / 2) * scale,
  };
}

/** Viewport a zoom-out (up-level / back) departs from: the current world,
 *  zoomed in about its centre. */
export function zoomOutEnterFrom(
  world: { w: number; h: number },
  cssW: number,
  cssH: number,
): Viewport {
  const fit = fitViewport(world, cssW, cssH);
  const scale = fit.scale * 2.4;
  return {
    scale,
    tx: cssW / 2 - (world.w / 2) * scale,
    ty: cssH / 2 - (world.h / 2) * scale,
  };
}
