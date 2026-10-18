// Treemap layout: entries -> world-space tiles. The world is built at the
// container's exact aspect ratio, and the fit transform is a COVER (scale+centre)
// so the fill invariant holds structurally: at matching aspect cover == contain
// == an exact edge-to-edge fill; on any aspect drift it overfills and crops
// rather than letterboxing, because background is the one thing that is
// forbidden. Colors are resolved here (theme-aware) and shipped with the
// geometry; dimming/highlight is a renderer concern.

import {
  hierarchy,
  treemap,
  treemapSquarify,
  type HierarchyRectangularNode,
} from "d3-hierarchy";
import { categoryStyle, inkFor, tintFill } from "../colors";
import type { ChildEntry } from "../types";
import type { TreemapFrameData, TileRef, RenderTile, Viewport, WorldRect } from "./types";

export const WORLD_W = 1000;

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

/** Build the frame in TWO layout passes (the wrong-proportions fix).
 *
 *  PASS 1 lays out the level by TRUE entry sizes: a directory participates as
 *  a LEAF valued by `entry.size`, so the cell it wins is exactly the share
 *  its label reports. The old single hierarchy valued a directory by the sum
 *  of its `sub` sample — but the backend caps that sample at the top-8
 *  (engine.rs), so a 749 MB folder collapsed to the area of its 8 largest
 *  files while its label read "749 MB · 98%".
 *
 *  PASS 2 lays out each directory's interior: its sub sample squarified to
 *  FILL that cell exactly (d3 normalizes the sample to the cell, so the
 *  interior never under-fills), translated into world space by the cell
 *  origin. Relative proportions inside the sample are preserved, and the
 *  contract is untouched: tiles/refs 1:1, depth 0|1, depth-0 pushed before
 *  its depth-1 children (hit-test walks in reverse), parentEntryId linkage,
 *  paddingInner(3) at both levels, round(true), sub-tile guard.
 *
 *  `parentSize` anchors the level root's entry — its own size never enters
 *  the geometry (a root with children contributes 0), and it is what the
 *  renderer's rebuild check diffs on. */
export function buildFrame(
  entries: ChildEntry[],
  parentSize: number,
  aspect: number,
  theme: "light" | "dark",
): TreemapFrameData {
  const worldW = WORLD_W;
  // Degenerate-measure guard: a NaN/0/negative aspect falls back to the
  // steady 0.62 aspect; an INFINITE aspect would drive worldH to 0 — the
  // frame would build empty (tiles all under the 0.6 guard) and draw bare
  // ground, so it is caught here alongside the NaN case.
  const worldH =
    aspect > 0 && isFinite(aspect) ? Math.max(WORLD_W / aspect, 1) : WORLD_W * 0.62;
  const world = { w: worldW, h: worldH };

  /** A flat level node: children exist only on a level root, which zeroes
   *  its own value — every real value on the level comes from an entry. */
  type Flat = { entry: ChildEntry; children?: Flat[] };
  const level = (list: ChildEntry[]): Flat[] => list.map((e) => ({ entry: e }));
  const ownSize = (d: Flat): number => (d.children ? 0 : d.entry.size);
  const rootData: Flat = {
    entry: {
      id: -1,
      name: "",
      size: parentSize,
      isDir: true,
      fileCount: 0,
      cat: -1,
      kidCount: 0,
    },
    children: level(entries),
  };

  const tiles: RenderTile[] = [];
  const refs: TileRef[] = [];

  const push = (
    rect: { x: number; y: number; w: number; h: number },
    depth: 0 | 1,
    data: Flat,
    parentId: number | null,
  ) => {
    const { x, y, w, h } = rect;
    if (w < 0.6 || h < 0.6) return;
    const style = categoryStyle(data.entry.cat, theme);
    // Depth ladder (INS_BRIEF §3), color-only: children resolve their fill
    // through tintFill() — one flat step toward the canvas background per
    // level, uniform across the level, never a spatial gradient — and the
    // label ink is re-derived from the TINTED fill via inkFor(), because the
    // 4.5:1 contrast floor must hold on the color actually drawn, not on the
    // base category hue. Geometry, paddingInner and the fill invariant are
    // untouched by this ladder.
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
      entryId: data.entry.id,
      parentEntryId: parentId,
      name: data.entry.name,
      size: data.entry.size,
      cat: data.entry.cat,
      isDir: data.entry.isDir,
      depth,
    });
  };

  // PASS 1 — the level itself, partitioned by true entry sizes.
  const rootNode = treemap<Flat>()
    .size([worldW, worldH])
    .tile(treemapSquarify)
    .paddingInner(3)
    .round(true)(hierarchy<Flat>(rootData).sum(ownSize));

  for (const n of (rootNode.children ?? []) as HierarchyRectangularNode<Flat>[]) {
    const rect = { x: n.x0, y: n.y0, w: n.x1 - n.x0, h: n.y1 - n.y0 };
    // A skipped cell's sub-tiles all lie inside it, so they cannot clear the
    // guard either — skipping here is exactly what the old per-tile guard did.
    if (rect.w < 0.6 || rect.h < 0.6) continue;
    push(rect, 0, n.data, null);
    const subs = n.data.entry.sub;
    if (!subs?.length) continue;
    // PASS 2 — this directory's interior, filling its cell edge to edge.
    const subRoot = treemap<Flat>()
      .size([rect.w, rect.h])
      .tile(treemapSquarify)
      .paddingInner(3)
      .round(true)(hierarchy<Flat>({ entry: n.data.entry, children: level(subs) }).sum(ownSize));
    for (const c of (subRoot.children ?? []) as HierarchyRectangularNode<Flat>[]) {
      push(
        { x: rect.x + c.x0, y: rect.y + c.y0, w: c.x1 - c.x0, h: c.y1 - c.y0 },
        1,
        c.data,
        n.data.entry.id,
      );
    }
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
