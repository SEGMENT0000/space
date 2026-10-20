// Contract between the layout module (pure TS — it owns the squarified
// partition itself, no d3) and the WebGL renderer (TreemapCanvas). The
// renderer consumes ONLY these shapes.
//
// Frame invariants (layout.ts, "LAYOUT INVARIANTS"): tiles/refs are 1:1 and
// pushed in lockstep; every ancestor comes before its descendants (the hit
// test walks in reverse); a tile always lies inside its parentEntryId tile's
// rect; every emitted tile is >= MIN_TILE_WORLD per side. `depth` is a number
// because interiors recurse: 0 is a direct child of the current folder, N is
// inside N nested interiors. The type stays unbounded so an LOD change needs
// no type change; layout.ts caps the real value at MAX_INTERIOR_DEPTH.
// Level-only LOD thresholds (preview gate, label tiers, nesting gate) live in
// layout.ts and are exported from there.

import type { ChildEntry, PreviewPayload } from "../types";

/** One drawable tile, in world coordinates (0..world.w, 0..world.h). */
export interface RenderTile {
  x: number;
  y: number;
  w: number;
  h: number;
  /** rgb floats 0..1 — resolved by colors.ts for the active theme */
  fill: [number, number, number];
  /** label color, rgb floats 0..1 */
  text: [number, number, number];
  /** diagonal hatch overlay (used for caches) */
  hatch: boolean;
  /** 0 = direct child of the current folder (a directory participates as a
   *  leaf valued by its own size); N = a child inside N nested interiors.
   *  Only directory tiles above the nesting gate get an interior (layout.ts,
   *  nestedInteriorApplies). Ancestors always appear earlier in `tiles`/`refs`
   *  and children are drawn over their parent's cell, inset by the interior
   *  gutter. The recursion is capped at MAX_INTERIOR_DEPTH (layout.ts); the
   *  field itself is a number so that cap is not frozen into the type. */
  depth: number;
}

/** Metadata aligned 1:1 with `tiles` by index. */
export interface TileRef {
  entryId: number;
  /** parent entry id, null for depth-0 tiles */
  parentEntryId: number | null;
  name: string;
  /** bytes */
  size: number;
  cat: number;
  isDir: boolean;
  /** 0 = direct child of the current folder; N = inside N nested interiors.
   *  Always equal to the aligned RenderTile.depth; ancestors come earlier. */
  depth: number;
}

export interface TreemapFrameData {
  world: { w: number; h: number };
  tiles: RenderTile[];
  refs: TileRef[];
}

export interface Viewport {
  /** translation in screen px */
  tx: number;
  ty: number;
  /** scale factor */
  scale: number;
}

/** Rect in world coordinates. */
export interface WorldRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface HoverInfo {
  entryId: number;
  cat: number;
  name: string;
  size: number;
  isDir: boolean;
  /** cursor position, CSS px relative to the canvas */
  x: number;
  y: number;
}

export interface TreemapHandle {
  /** Arm a navigation transition, consumed by the next frame rebuild:
   *  "in" zooms from `rect` (a tile rect of the current frame), "out"
   *  pulls back from the current world. */
  beginNav(dir: "in" | "out", rect?: WorldRect): void;
  /** Animate the viewport so the tile with this entryId fills the canvas. */
  zoomToTile(entryId: number): void;
  /** Animate back to the full frame view. */
  resetView(): void;
  /** Keyboard zoom about the canvas centre. */
  zoomBy(factor: number): void;
  clearSelection(): void;
  setSelection(entryIds: number[]): void;
  /** Tile rect of the current frame, in world coordinates. */
  getWorldRect(entryId: number): WorldRect | null;
}

export interface TreemapCanvasProps {
  /** Entries of the current node, already sorted by the parent. */
  entries: ChildEntry[];
  /** Byte size of the current node (frame total). */
  parentSize: number;
  theme: "light" | "dark";
  /**
   * Categories to dim (legend filter). Tiles whose ref.cat is in this set
   * render at low alpha; logic lives in the renderer so filtering animates.
   */
  hiddenCats: ReadonlySet<number>;
  /**
   * When non-null (search active), tiles outside the set dim the same way.
   */
  highlightIds: ReadonlySet<number> | null;
  /** A file tile was double-clicked — open with the system handler. */
  onOpenFile: (entryId: number) => void;
  /** A directory tile was double-clicked — navigate in. */
  onZoomCommit: (entryId: number, tileRect: WorldRect) => void;
  /** Single click on empty canvas. */
  onPick: (entryId: number | null) => void;
  /** User-driven selection changes (click / shift-click / marquee). */
  onSelectionChange: (entryIds: number[], totalBytes: number) => void;
  /** Called when the hovered tile changes. */
  onHoverChange: (hover: HoverInfo | null) => void;
  /** Right-click on a tile (null when on empty canvas). */
  onContextMenu: (entryId: number | null, x: number, y: number) => void;
  /**
   * Preview payloads by entryId (FLEET §D/G cross-file contract, owner:
   * ExplorerView). The renderer reads ONLY `p.family` for plate
   * qualification and passes the payload through to fillPlate unchanged.
   *
   * `null` is a DELIBERATE value, not a missing key: the pipeline stores
   * `null` for a preview that failed to resolve, and the renderer paints a
   * fallback META card for it instead of re-requesting (no spin). A tile
   * simply not in the map is still "not yet requested".
   */
  previews?: ReadonlyMap<number, PreviewPayload | null>;
  /**
   * Fired when a tile crosses the preview request gate and no payload is in
   * `previews` yet — once per entryId per nav generation. The renderer is
   * path-dumb: the parent resolves the path and stores the result by entryId.
   *
   * `path` is the best-known path from the renderer's hit ref and MAY be `""`
   * when it is unresolved — the parent (ExplorerView) resolves the real path
   * before requesting, so the renderer may pass it through unchanged.
   */
  onNeedPreview?: (entryId: number, path: string) => void;
  /** Children of directories beyond the API's one-level `sub` sample, keyed
   *  by directory entryId. Lazy: the renderer requests a directory's children
   *  via onNeedExpand; the parent fetches and stores them here. */
  expanded?: ReadonlyMap<number, ChildEntry[]>;
  /** Request a directory's children. Fired only for directories whose tile is
   *  large enough on screen to subdivide, once per entryId until it appears
   *  in `expanded`. */
  onNeedExpand?: (entryId: number) => void;
}
