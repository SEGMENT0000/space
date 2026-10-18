// Contract between the layout module (pure TS, owns d3-hierarchy) and the
// WebGL renderer (TreemapCanvas). The renderer consumes ONLY these shapes.

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
  /** depth 0 = direct child of current node, 1 = sub-tile */
  depth: 0 | 1;
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
  depth: 0 | 1;
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
   */
  previews?: ReadonlyMap<number, PreviewPayload>;
  /**
   * Fired when a tile crosses the preview request gate and no payload is in
   * `previews` yet — once per entryId per nav generation. The renderer is
   * path-dumb: the parent resolves the path and stores the result by entryId.
   */
  onNeedPreview?: (entryId: number) => void;
}
