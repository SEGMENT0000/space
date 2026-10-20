// The explorer surface: a transparent instrument topbar (history, crumbs,
// filter, sort, palette, rail toggle) over the ground; the map region as a
// rounded .panel with the treemap filling it edge to edge; the 300px
// inspector rail as its own .panel; and a transparent mono status strip.
// Navigation is client-side history over node ids; every level swap
// animates through the treemap's shared-element transition.
//
// Presentation contract (the instrument-panel world + app-brief):
//   · the topbar and statusbar are transparent strips on the ground (--bg);
//     the map and the rail are .panel surfaces — chrome never boxes the data
//   · every live figure is Geist Mono tabular (.num); labels are
//     sentence-case Geist; nothing here shows a native title tooltip
//   · the hover readout splits IDENTITY from POSITION: the hovered
//     entryId is React state (re-rendering only the card content), while
//     pointer movement between identity changes is written straight to the
//     card element through a ref — no setState on move. The topbar, rail,
//     statusbar and map subtrees are memoized behind stable useCallback
//     props, so a tile-to-tile move re-renders nothing and a same-tile
//     move re-renders nothing at all. Nothing resolves hover by index: the
//     renderer's payload already names the item under the pointer; every
//     extra (path, file count) resolves from that entryId via findEntry.
//   · map overlays: one once-per-user gesture chip (localStorage), the
//     live filter readout, action toasts, and the honest empty/loading/
//     error bed with the real reason — no standing instructions
// All logic (history, load, selection, menu, keys, control surface) lives
// here; presentation styles are explorer.css.

import {
  ChevronLeft,
  ChevronRight,
  PanelRight,
  Search,
  X,
} from "lucide-react";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RefObject } from "react";
import { CATEGORY_LABELS, categoryStyle } from "../lib/colors";
import { extOf } from "../lib/ext";
import { formatBytes, formatCount, formatPercent } from "../lib/format";
import {
  getCategoryTotals,
  getChildren,
  getLargestFiles,
  openWithSystem,
  revealInExplorer,
} from "../lib/ipc";
import { invalidatePreviews, requestPreview, setPreviewGeneration } from "../lib/preview";
import type { HoverInfo, TreemapHandle } from "../lib/treemap/types";
import type {
  ChildEntry,
  ChildrenResponse,
  LargestFile,
  PreviewPayload,
} from "../lib/types";
import { ExplorerRail, type RailDetail, type RailSelection } from "./ExplorerRail";
import { TreemapCanvas } from "./treemap/TreemapCanvas";
import { HoverCard, IconButton, KeyHint, SegmentedControl, useShell } from "./ui";
import type { HoverCardData } from "./ui";

export type SortMode = "size" | "name" | "type";

/** Folder cards draw from the LIVE SCAN INDEX, not a fresh disk read: the
 *  scanner already knows every child's subtree size, so a folder card can
 *  answer the only question that matters on a huge leaf folder ("where are
 *  the bytes?") with real sizes on both file and directory rows. The preview
 *  backend's folder listing is the fallback for paths outside the scan; it
 *  can only report immediate names and no directory sizes.
 *
 *  Single-child descent: a leaf folder that wraps exactly one directory
 *  (Chrome → User Data) would otherwise spend a 600px card on one row. We
 *  descend up to FOLDER_CARD_CHAIN wrapper levels and render the first level
 *  with more than one child, noting the chain in the head meta ("via User
 *  Data"). Bounded: at most four in-memory child queries, all cached by
 *  getChildren. */
const FOLDER_CARD_ROWS = 32;
const FOLDER_CARD_CHAIN = 4;

async function loadFolderCard(entryId: number): Promise<PreviewPayload | null> {
  try {
    let nodeId = entryId;
    const via: string[] = [];
    for (let step = 0; ; step++) {
      const res = await getChildren(nodeId, 1, "size", FOLDER_CARD_ROWS);
      const entries = res.entries;
      // A wrapper, not content: exactly one child, a directory, and the
      // parent's own count confirms nothing else was hidden.
      if (
        step < FOLDER_CARD_CHAIN &&
        entries.length === 1 &&
        entries[0].isDir &&
        res.totalEntries === 1
      ) {
        via.push(entries[0].name);
        nodeId = entries[0].id;
        continue;
      }
      return {
        family: "list",
        list: {
          kind: "folder",
          entries: entries.map((e) => ({
            name: e.name,
            size: e.size,
            kind: e.isDir ? "dir" : "file",
          })),
          truncated: res.totalEntries > entries.length,
          total: res.totalEntries,
          ...(via.length > 0 ? { via } : {}),
        },
      };
    }
  } catch {
    return null; // honest fallback card; the scan may be torn down
  }
}

export interface FileApi {
  reveal(path: string): Promise<void>;
  copyPath(path: string): Promise<void>;
  openPath(path: string): Promise<void>;
}

export interface ExplorerControl {
  focusSearch(): void;
  toggleRail(): void;
  resetView(): void;
  zoomBy(factor: number): void;
  setSort(s: SortMode): void;
  searchEntries(q: string): { id: number; name: string; size: number; isDir: boolean }[];
  jumpTo(id: number): void;
  back(): void;
  forward(): void;
}

// The once-ever gesture chip teaches pan/drill/select: after 6 seconds on
// screen, or the first pointer interaction with the map, it fades and stays
// gone — across sessions (localStorage), not just this one. The module
// flag keeps a later remount from re-teaching what is already recorded.
const HINT_KEY = "space.hint.shown";
let hintRecorded = false;

// Preview/view epochs are module-global: preview.ts keys staleness by
// generation equality, so a remounting view must never re-use a number an
// earlier instance already stamped on an in-flight fetch — otherwise that
// stale response would look current again and cache under a dead dataset.
let viewEpoch = 0;

type HintPhase = "shown" | "fading" | "gone";

// Memo'd map + rail: property-identical wrappers. Hover/selection state
// changes re-render ExplorerView; these bail the heavy subtrees out.
const MemoMap = memo(TreemapCanvas);
const MemoRail = memo(ExplorerRail);

export function ExplorerView({
  rootId,
  rootPath,
  totalBytes,
  volumeTotal,
  theme,
  api,
  onToggleTheme,
  onRescan,
  onShowDrives,
  onOpenPalette,
  onReady,
}: {
  rootId: number;
  rootPath: string;
  totalBytes: number;
  volumeTotal: number;
  theme: "light" | "dark";
  api: FileApi;
  onToggleTheme: () => void;
  onRescan: () => void;
  onShowDrives: () => void;
  onOpenPalette: () => void;
  onReady: (ctl: ExplorerControl | null) => void;
}) {
  const mapRef = useRef<TreemapHandle>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  // Host of the hover card: the map panel's inner wrap (positioned, so it
  // is the card's offsetParent). Card x/y are canvas-relative, and the
  // canvases fill this wrap edge to edge, so its box is the clamp space
  // the card lives in.
  const mapWrapRef = useRef<HTMLDivElement | null>(null);
  const cardRef = useRef<HTMLDivElement | null>(null);

  const [resp, setResp] = useState<ChildrenResponse | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [currentName, setCurrentName] = useState(() => leafName(rootPath));
  const [sort, setSort] = useState<SortMode>("size");
  const [query, setQuery] = useState("");
  const [hiddenCats, setHiddenCats] = useState<ReadonlySet<number>>(new Set());
  const [hover, setHover] = useState<HoverInfo | null>(null);
  const [selIds, setSelIds] = useState<number[]>([]);
  const [selTotal, setSelTotal] = useState(0);
  const [menu, setMenu] = useState<{ id: number | null; x: number; y: number } | null>(null);
  const [railOpen, setRailOpen] = useState(() => typeof window === "undefined" || window.innerWidth > 900);
  const [categories, setCategories] = useState<number[]>([]);
  const [largest, setLargest] = useState<LargestFile[]>([]);
  const [toast, setToast] = useState<string | null>(null);
  const [hint, setHint] = useState<HintPhase>(() => {
    if (hintRecorded) return "gone";
    try {
      return localStorage.getItem(HINT_KEY) === "1" ? "gone" : "shown";
    } catch {
      return "gone";
    }
  });

  // View/preview generation (FLEET §D cancellation): there is no abort in
  // Tauri invoke, so every preview flow captures the epoch at dispatch and
  // drops its delivery when a newer one has moved on. Bumped on every
  // load()/drill and on every filter change, mirrored into the preview
  // pipeline through setPreviewGeneration (cache kept — entry ids are stable
  // within a scan), and always raised from the module counter above so the
  // number is unique across mounts.
  const genRef = useRef(0);

  // The load token is deliberately SEPARATE from the preview generation: a
  // filter keystroke bumps the preview epoch too, and that must never cancel
  // a folder read that is mid-flight. Bumped only at the top of load() and
  // on unmount; every getChildren/getCategoryTotals result checks it.
  const navGenRef = useRef(0);

  // entryId → payload for the plate layer (FLEET §D/G). React state
  // (not a bare ref) so each arrival re-renders TreemapCanvas with a fresh
  // ReadonlyMap — the renderer reads payloads by entryId and must see them
  // land. `null` is stored too: it is the pipeline's honest "unreadable",
  // and the plate layer draws its fallback card for it instead of leaving an
  // endless spinner. Entry ids are stable within a scan, so the map survives
  // nav (back/forward re-plates instantly from memory) and is rebuilt only
  // on a genuine reload (the mount below).
  const [previews, setPreviews] = useState<ReadonlyMap<number, PreviewPayload | null>>(new Map());

  // Lazy recursive interiors (entryId → that folder's children). The renderer
  // asks for a folder's next level through onNeedExpand; each arrival is
  // batched with its neighbours and published as ONE new Map (~48ms), so a
  // burst of expansions costs one render/rebuild, not one per folder.
  // Entry ids are stable within a scan and getChildren is LRU-cached, so this
  // survives drill navigation; a new scan remounts the view, starting empty.
  const [expanded, setExpanded] = useState<ReadonlyMap<number, ChildEntry[]>>(() => new Map());
  // Latest settled map, readable synchronously by the stable onNeedExpand
  // callback (depending on `expanded` itself would mint a new callback per
  // batch). Mirrors the render-assigned pathOfResp ref above.
  const expandedRef = useRef<ReadonlyMap<number, ChildEntry[]>>(expanded);
  expandedRef.current = expanded;
  // In-flight dedupe per folder, plus one shared accumulation batch and its
  // single flush timer.
  const expandPending = useRef<Set<number>>(new Set());
  const expandBatch = useRef<Map<number, ChildEntry[]>>(new Map());
  const expandTimer = useRef(0);

  // navigation history over node ids
  const historyRef = useRef<{ ids: number[]; names: string[]; idx: number }>({
    ids: [rootId],
    names: [leafName(rootPath)],
    idx: 0,
  });
  const [, forceHist] = useState(0);

  const pathOfResp = useRef(rootPath);
  pathOfResp.current = resp?.path ?? rootPath;

  // One bump for the view/preview epoch: raises the module counter (never
  // recycling an epoch an earlier mount already stamped), mirrors it into
  // the preview pipeline so queued/in-flight work for the old view resolves
  // null instead of landing late, and arms this view's delivery guard.
  const bumpViewGen = useCallback(() => {
    const gen = ++viewEpoch;
    genRef.current = gen;
    setPreviewGeneration(gen);
    return gen;
  }, []);

  // ------------------------------------------------------------ loading

  const load = useCallback(
    (nodeId: number, name: string, mode: "push" | "replace") => {
      // Generation capture: every load (double-click, goUp, back/forward,
      // breadcrumb, initial mount) invalidates all older in-flight loads
      // (nav token) and moves the preview pipeline to the new view (preview
      // epoch — cache stays warm, back/forward re-plates instantly).
      bumpViewGen();
      const nav = ++navGenRef.current;
      // Zero-stale hover begins here: the moment a navigation starts the
      // card folds (the renderer re-resolves on the next real move over the
      // NEW frame — never against the old one).
      hoverIdRef.current = null;
      setHover(null);
      void getChildren(nodeId, 2)
        .then((r) => {
          // Stale-drop: a newer navigation started while this request was in
          // flight — its result describes a view the user has already left.
          if (navGenRef.current !== nav) return;
          setResp(r);
          setLoadErr(null);
          setCurrentName(name);
          pathOfResp.current = r.path;
          setSelIds([]);
          setSelTotal(0);
          mapRef.current?.clearSelection();
          const h = historyRef.current;
          if (mode === "push") {
            h.ids = h.ids.slice(0, h.idx + 1);
            h.names = h.names.slice(0, h.idx + 1);
            h.ids.push(nodeId);
            h.names.push(name);
            h.idx = h.ids.length - 1;
          } else {
            h.ids[h.idx] = nodeId;
            h.names[h.idx] = name;
          }
          forceHist((v) => v + 1);
          // Same token guards the dependent fetch: category totals must
          // describe the node the user is actually looking at, and a
          // rejection here must never be a silent unhandled one.
          void getCategoryTotals(nodeId)
            .then((cats) => {
              if (navGenRef.current !== nav) return;
              setCategories(cats);
            })
            .catch((err) => {
              if (navGenRef.current !== nav) return;
              console.error("space: category totals failed", nodeId, err);
            });
        })
        .catch((err) => {
          // A failed read is a state of the panel, not a dead screen: show
          // the honest bed with the real reason (still logged for dev).
          if (navGenRef.current !== nav) return;
          console.error("space: getChildren failed", nodeId, err);
          setLoadErr(err instanceof Error ? err.message : String(err));
          setResp(null);
        });
    },
    [bumpViewGen],
  );
  // ------------------------------------------------------------ hover

  // Last hovered tile identity + freshest pointer point. Identity changes
  // are React state; point moves are imperative writes.
  const hoverIdRef = useRef<number | null>(null);
  const lastXY = useRef<{ x: number; y: number } | null>(null);

  // Position the card element directly (same math as HoverCard's own
  // effect, so identity changes and moves behave identically). Host = the
  // map wrap; flip/clamp keep the card inside the panel on every edge.
  const positionCard = useCallback((x: number, y: number) => {
    const el = cardRef.current;
    const host = mapWrapRef.current;
    if (!el || !host) return;
    const hw = host.clientWidth;
    const hh = host.clientHeight;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let left = x + 14;
    let top = y + 16;
    if (left + w > hw - 8) left = x - w - 14;
    if (top + h > hh - 8) top = y - h - 12;
    el.style.left = `${Math.round(Math.max(8, Math.min(left, hw - w - 8)))}px`;
    el.style.top = `${Math.round(Math.max(8, Math.min(top, hh - h - 8)))}px`;
  }, []);

  // Renderer → explorer folder-interior request. Reads exactly ONE level
  // (depth 1: the payload is just that folder's children, with no attached
  // sub-samples), dedupes in flight per entryId, and coalesces every settle
  // inside a ~48ms window into a single setExpanded — one render/rebuild per
  // batch. The view generation is captured BEFORE the async hop; a result
  // delivered after a drill/filter move is dropped. A failed read is stored
  // as [] (an honest "asked, no children to draw") so the renderer stops
  // asking instead of retrying forever.
  const onNeedExpand = useCallback((entryId: number) => {
    if (
      expandedRef.current.has(entryId) ||
      expandPending.current.has(entryId) ||
      expandBatch.current.has(entryId)
    ) {
      return;
    }
    expandPending.current.add(entryId);
    const gen = genRef.current;
    // Stage one settled folder and arm the shared flush on first arrival.
    const queue = (kids: ChildEntry[]) => {
      if (genRef.current !== gen) return;
      expandBatch.current.set(entryId, kids);
      if (expandTimer.current !== 0) return;
      expandTimer.current = window.setTimeout(() => {
        expandTimer.current = 0;
        const batch = expandBatch.current;
        expandBatch.current = new Map();
        if (batch.size === 0) return;
        setExpanded((prev) => {
          const next = new Map(prev);
          for (const [k, v] of batch) next.set(k, v);
          return next;
        });
      }, 48);
    };
    void getChildren(entryId, 1, "size", 900)
      .then((r) => queue(r.entries))
      .catch((err) => {
        // Unreadable folder: remember the empty answer, never throw.
        console.error("space: getChildren expand failed", entryId, err);
        queue([]);
      })
      .finally(() => {
        expandPending.current.delete(entryId);
      });
  }, []);

  // entryId → entry + path in the CURRENT view. The renderer is path-dumb:
  // the hover card, context menu, actions and preview requests all resolve
  // through here. Walks descendants at ANY depth (the payload's depth-1 `sub`
  // plus every lazily expanded folder), joining paths level by level, so deep
  // tiles resolve exactly like direct children. Lookups stay O(1); the depth
  // cap and visited guard make a malformed cycle terminate.
  const entryIndex = useMemo(() => {
    const m = new Map<number, { e: ChildEntry; path: string; depth: number }>();
    const base = pathOfResp.current;
    const walk = (list: readonly ChildEntry[], parentPath: string, depth: number) => {
      if (depth > 8) return;
      for (const e of list) {
        if (m.has(e.id)) continue;
        const path = joinPath(parentPath, e.name);
        m.set(e.id, { e, path, depth });
        const kids = expanded.get(e.id) ?? e.sub;
        if (kids && kids.length > 0) walk(kids, path, depth + 1);
      }
    };
    walk(resp?.entries ?? [], base, 0);
    return m;
  }, [resp, expanded]);

  const findEntry = useCallback(
    (id: number): { e: ChildEntry; path: string; depth: number } | null => entryIndex.get(id) ?? null,
    [entryIndex],
  );

  // Renderer → explorer preview request (FLEET §D/G), fired two ways: by the
  // treemap when a tile crosses its size gate, and by every hover identity
  // change below (the tile under the cursor always gets content, whatever
  // the zoom). The fixed signature is (entryId, path) — the path arrives
  // when the renderer carries one, and this view resolves it otherwise (it
  // is the only layer that knows paths). The view generation is captured
  // BEFORE the async hop: a delivery for a view the user has left is
  // dropped, never stored. `null` is stored as-is — the pipeline's honest
  // "unreadable" — so the plate layer draws its fallback card instead of
  // waiting forever.
  const onNeedPreview = useCallback(
    (entryId: number, path?: string) => {
      const gen = genRef.current;
      const info = findEntry(entryId);
      const resolved = path || info?.path;
      if (!resolved) return; // not in the current view — nothing honest to fetch
      // Leaf folder → a folder card from the scan index, with real subtree
      // sizes and a wrapper-chain descent. Interior folders never reach this
      // branch (the renderer only asks for directories with no child rect).
      if (info?.e.isDir) {
        void loadFolderCard(entryId).then((payload) => {
          if (genRef.current !== gen) return; // a navigation moved on
          setPreviews((prev) =>
            prev.get(entryId) === payload ? prev : new Map(prev).set(entryId, payload),
          );
        });
        return;
      }
      requestPreview(entryId, resolved, (id, payload) => {
        if (genRef.current !== gen) return; // a navigation moved on
        // Identity-guarded: a cached payload re-delivering itself must not
        // re-render the map. Otherwise a fresh Map instance → the
        // ReadonlyMap prop identity changes and TreemapCanvas re-renders
        // with the payload (or the honest null) present.
        setPreviews((prev) => (prev.get(id) === payload ? prev : new Map(prev).set(id, payload)));
      });
    },
    [findEntry],
  );

  // The ONE hover write path. Payload = always the item under the pointer.
  const onHoverChange = useCallback(
    (info: HoverInfo | null) => {
      if (!info) {
        if (hoverIdRef.current !== null) {
          hoverIdRef.current = null;
          setHover(null);
        }
        return;
      }
      lastXY.current = { x: info.x, y: info.y };
      if (hoverIdRef.current === info.entryId) {
        // Same tile: movement is imperative-only. No setState — no render.
        positionCard(info.x, info.y);
        return;
      }
      hoverIdRef.current = info.entryId;
      positionCard(info.x, info.y);
      setHover(info);
      // Warm the hovered tile's preview regardless of zoom. Directories never
      // plate (a folder's interior is the nested treemap), but file tiles at
      // ANY depth can now plate: lazy expansion puts deep folders on the map,
      // so resolve the path and ask for every non-directory the index knows.
      if (!info.isDir) {
        const f = findEntry(info.entryId);
        if (f) onNeedPreview(info.entryId, f.path);
      }
    },
    [positionCard, findEntry, onNeedPreview],
  );

  // After any render with a live hover, glue the card to the freshest
  // pointer point. Runs after HoverCard's own effect (parent effects follow
  // child effects), so it also corrects the identity-change position that
  // HoverCard derives from the slightly older payload point.
  useLayoutEffect(() => {
    if (hover && lastXY.current) positionCard(lastXY.current.x, lastXY.current.y);
  });

  useEffect(() => {
    // A genuine reload, and the ONLY place the preview pipeline is
    // invalidated: App renders ExplorerView per screen, so a mount is the
    // moment a new scan's dataset arrives (and a drives round-trip is at
    // worst a harmless refetch). The pipeline cache is keyed by entryId and
    // can outlive the data it describes — dropping it here means drills
    // never pay for it: they only move the generation and keep back/forward
    // instant.
    invalidatePreviews();
    setPreviews(new Map());
    load(rootId, leafName(rootPath), "replace");
    void getLargestFiles(30)
      .then(setLargest)
      .catch((err) => console.error("space: largest files failed", err));
    return () => {
      // Unmount: bump both tokens so any still-in-flight load/preview
      // delivery lands on a dead view and drops instead of calling setState
      // on an unmounted tree. The cache itself belongs to the NEXT mount's
      // reload above — no clearPreviews-on-unmount pattern here.
      genRef.current = ++viewEpoch;
      navGenRef.current += 1;
      window.clearTimeout(toastTimer.current);
      // Expansion pipeline dies with the view: no queued folder result or
      // pending batch may flush into an unmounted tree.
      expandPending.current.clear();
      expandBatch.current.clear();
      window.clearTimeout(expandTimer.current);
      expandTimer.current = 0;
      onReady(null);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A filter change reshapes what the renderer qualifies (dimmed tiles stop
  // asking, newly-matching ones start), so it moves the preview generation
  // exactly like a drill does: queued/in-flight results for the old shape
  // resolve null instead of landing late, and the pipeline's failed set gets
  // a fresh retry budget. It touches ONLY the preview epoch — the nav token
  // above keeps a keystroke from cancelling a folder read in flight.
  useEffect(() => {
    bumpViewGen();
  }, [bumpViewGen, query]);

  // ------------------------------------------------------------ derived

  const sorted = useMemo(() => {
    const entries = resp?.entries ?? [];
    const out = [...entries];
    const cmpName = (a: ChildEntry, b: ChildEntry) =>
      a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
    if (sort === "size") out.sort((a, b) => b.size - a.size || cmpName(a, b));
    else if (sort === "name") out.sort(cmpName);
    else
      out.sort((a, b) => {
        if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
        const ea = extOf(a.name);
        const eb = extOf(b.name);
        return ea.localeCompare(eb) || a.size - b.size || cmpName(a, b);
      });
    return out;
  }, [resp, sort]);

  const highlightIds = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return null;
    const set = new Set<number>();
    // Mirror entryIndex: match at every depth the map can draw, preferring a
    // lazily expanded folder's fetched children over the payload's `sub`.
    // Depth cap + visited guard keep a malformed cycle finite.
    const seen = new Set<number>();
    const walk = (list: readonly ChildEntry[], depth: number) => {
      if (depth > 8) return;
      for (const e of list) {
        if (seen.has(e.id)) continue;
        seen.add(e.id);
        if (e.name.toLowerCase().includes(q)) set.add(e.id);
        const kids = expanded.get(e.id) ?? e.sub;
        if (kids && kids.length > 0) walk(kids, depth + 1);
      }
    };
    walk(sorted, 0);
    return set;
  }, [query, sorted, expanded]);

  const matches = highlightIds?.size ?? 0;

  // selection -> rail detail
  const selection: RailSelection | null = useMemo(() => {
    if (selIds.length === 0) return null;
    // Deep tiles are selectable now that interiors expand recursively, so the
    // primary resolves through the same any-depth index as hover/actions.
    const primary = findEntry(selIds[selIds.length - 1]);
    let detail: RailDetail | null = null;
    if (primary) {
      detail = {
        id: primary.e.id,
        name: primary.e.name,
        size: primary.e.size,
        isDir: primary.e.isDir,
        cat: primary.e.cat,
        fileCount: primary.e.fileCount,
        kidCount: primary.e.kidCount,
        path: primary.path,
      };
    }
    return { ids: selIds, total: selTotal, detail };
  }, [selIds, selTotal, findEntry]);

  // Rail bounds the memo: rebuilt only when the folder or its size moves.
  const railCurrent = useMemo(
    () => ({
      name: currentName,
      size: resp?.parentSize ?? 0,
      items: resp?.totalEntries ?? 0,
      path: resp?.path ?? rootPath,
    }),
    [currentName, resp, rootPath],
  );
  // ------------------------------------------------------------ actions
  // (identity → entry/path resolves through findEntry above, in the hover
  // section: one index, shared by the card, the menu and every action.)

  // ------------------------------------------------------------ toasts

  const toastTimer = useRef(0);
  const showToast = useCallback((msg: string) => {
    window.clearTimeout(toastTimer.current);
    setToast(msg);
    toastTimer.current = window.setTimeout(() => setToast(null), 1400);
  }, []);

  // ------------------------------------------------------------ shell

  // Reveal / open ride the ipc exports (demo-aware): browsers and demo
  // report false and the toast says so honestly — no dead quiet action.
  const doReveal = useCallback(
    (id: number) => {
      const f = findEntry(id);
      if (!f) return;
      void revealInExplorer(f.path).then((ok) =>
        showToast(ok ? "Revealed in Explorer" : "Reveal unavailable in demo"),
      );
    },
    [findEntry, showToast],
  );

  const doOpen = useCallback(
    (id: number) => {
      const f = findEntry(id);
      if (!f) return;
      void openWithSystem(f.path).then((ok) => {
        if (!ok) showToast("Open unavailable in demo");
      });
    },
    [findEntry, showToast],
  );

  const doCopy = useCallback(
    (id: number) => {
      const f = findEntry(id);
      if (!f) return;
      void api.copyPath(f.path).then(() => showToast("Path copied"));
    },
    [api, findEntry, showToast],
  );

  const zoomIn = useCallback(
    (id: number, name: string) => {
      const rect = mapRef.current?.getWorldRect(id) ?? undefined;
      mapRef.current?.beginNav("in", rect);
      load(id, name, "push");
    },
    [load],
  );

  const onZoomCommit = useCallback(
    (id: number) => {
      const f = findEntry(id);
      zoomIn(id, f?.e.name ?? "Folder");
    },
    [findEntry, zoomIn],
  );

  const onPick = useCallback(() => undefined, []);

  const onSelectionChange = useCallback((ids: number[], total: number) => {
    setSelIds(ids);
    setSelTotal(total);
  }, []);

  const onContextMenu = useCallback(
    (id: number | null, x: number, y: number) => {
      // Right-click focuses: an unselected target replaces the selection.
      if (id !== null && !selIds.includes(id)) {
        setSelIds([id]);
        mapRef.current?.setSelection([id]);
      }
      setMenu({ id, x, y });
    },
    [selIds],
  );

  // ------------------------------------------------------------ hint

  // Why transient: an instruction the user has read is noise. Remembered
  // across sessions; fades on its own 6s clock or the first pointer touch.
  const dismissHint = useCallback(() => {
    hintRecorded = true;
    try {
      localStorage.setItem(HINT_KEY, "1");
    } catch {
      /* private mode: the module flag still keeps it gone this session */
    }
    setHint("fading");
    window.setTimeout(() => setHint("gone"), 450);
  }, []);

  useEffect(() => {
    if (hint !== "shown") return;
    const t = window.setTimeout(dismissHint, 6000);
    return () => window.clearTimeout(t);
  }, [hint, dismissHint]);

  // ------------------------------------------------------------ history

  const goUp = useCallback(() => {
    const h = historyRef.current;
    if (h.idx <= 0 || !resp) return;
    mapRef.current?.beginNav("out");
    load(h.ids[h.idx - 1], h.names[h.idx - 1], "push");
  }, [resp, load]);

  const goBack = useCallback(() => {
    const h = historyRef.current;
    if (h.idx <= 0) return;
    h.idx -= 1;
    mapRef.current?.beginNav("out");
    load(h.ids[h.idx], h.names[h.idx], "replace");
  }, [load]);

  const goForward = useCallback(() => {
    const h = historyRef.current;
    if (h.idx >= h.ids.length - 1) return;
    h.idx += 1;
    mapRef.current?.beginNav("out");
    load(h.ids[h.idx], h.names[h.idx], "replace");
  }, [load]);

  const goCrumb = useCallback(
    (c: { id: number; name: string }) => {
      if (c.id === resp?.parentId) {
        // Already at this level — just re-fit the view.
        mapRef.current?.resetView();
        return;
      }
      const h = historyRef.current;
      const idx = h.ids.indexOf(c.id);
      if (idx >= 0 && idx < h.idx) {
        h.idx = idx;
        mapRef.current?.beginNav("out");
        load(c.id, c.name, "replace");
      } else {
        mapRef.current?.beginNav("out");
        load(c.id, c.name, "push");
      }
    },
    [resp, load],
  );

  const toggleCat = useCallback((cat: number) => {
    setHiddenCats((prev) => {
      const next = new Set(prev);
      if (next.has(cat)) next.delete(cat);
      else next.add(cat);
      return next;
    });
  }, []);

  // One switch for the statusbar chip: bring every filtered category back.
  const unhideAll = useCallback(() => setHiddenCats(() => new Set()), []);

  const pickLargest = useCallback((id: number) => {
    setSelIds([id]);
    mapRef.current?.setSelection([id]);
    mapRef.current?.zoomToTile(id);
  }, []);
  // ------------------------------------------------------------ control surface

  useEffect(() => {
    onReady({
      focusSearch: () => searchRef.current?.focus(),
      toggleRail: () => setRailOpen((v) => !v),
      resetView: () => mapRef.current?.resetView(),
      zoomBy: (f) => mapRef.current?.zoomBy(f),
      setSort,
      searchEntries: (q) => {
        const needle = q.trim().toLowerCase();
        const out: { id: number; name: string; size: number; isDir: boolean }[] = [];
        for (const e of sorted) {
          if (!needle || e.name.toLowerCase().includes(needle)) out.push({ id: e.id, name: e.name, size: e.size, isDir: e.isDir });
          if (e.sub) for (const s of e.sub) if (!needle || s.name.toLowerCase().includes(needle)) out.push({ id: s.id, name: s.name, size: s.size, isDir: s.isDir });
        }
        return out.slice(0, 40);
      },
      jumpTo: (id) => {
        setSelIds([id]);
        mapRef.current?.setSelection([id]);
        mapRef.current?.zoomToTile(id);
      },
      back: goBack,
      forward: goForward,
    });
    return () => onReady(null);
  }, [onReady, sorted, goBack, goForward]);

  // ------------------------------------------------------------ keys

  // Enter / Ctrl C act on the selection — the context menu's keycap column
  // promises them, so they are wired here for real (no dead keycaps).
  const openSelection = useCallback(() => {
    const primary = selIds[selIds.length - 1];
    if (primary === undefined) return;
    const f = findEntry(primary);
    if (!f) return;
    if (f.e.isDir) zoomIn(primary, f.e.name);
    else {
      void openWithSystem(f.path).then((ok) => {
        if (!ok) showToast("Open unavailable in demo");
      });
    }
  }, [selIds, findEntry, zoomIn, showToast]);

  const copySelection = useCallback(() => {
    const primary = selIds[selIds.length - 1];
    if (primary !== undefined) doCopy(primary);
  }, [selIds, doCopy]);

  const clearQuery = useCallback(() => setQuery(""), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inInput = e.target instanceof HTMLInputElement;
      // Gate on THIS view's filter input identity — not merely "some input
      // is focused". The global palette input (owned by App) and the rail's
      // filter are other elements' business: Esc there must close/act on
      // their own terms and never wipe the explorer query.
      const inFilter = inInput && e.target === searchRef.current;
      if (e.key === "Escape") {
        setMenu(null);
        if (inFilter) {
          // The filter box owns Esc: clear the query and release focus —
          // blur alone left the dimmed filtered map stranded behind a
          // focused-looking field.
          clearQuery();
          searchRef.current?.blur();
        } else if (!inInput) {
          // Anywhere outside an input: Esc clears the selection.
          setSelIds([]);
          setSelTotal(0);
          mapRef.current?.clearSelection();
        }
        return;
      }
      if (inInput || menu) return;
      if (e.key === "Backspace") {
        e.preventDefault();
        goUp();
      } else if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        goBack();
      } else if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        goForward();
      } else if (e.key === "+" || e.key === "=") {
        mapRef.current?.zoomBy(1.4);
      } else if (e.key === "-" || e.key === "_") {
        mapRef.current?.zoomBy(1 / 1.4);
      } else if (e.key === "0" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        mapRef.current?.resetView();
      } else if (e.key === "Enter" && selIds.length > 0) {
        // the menu's ↵ keycap, made live: open the primary selection
        e.preventDefault();
        openSelection();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c" && selIds.length > 0) {
        // Ctrl C: copy the primary selection's path — same as the menu row
        e.preventDefault();
        copySelection();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [goUp, goBack, goForward, openSelection, copySelection, menu, selIds.length, clearQuery]);

  // close the context menu on the next outside click / scroll
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  // ------------------------------------------------------------ render

  const crumbs = useMemo(
    () => (resp ? withCurrent(resp.lineage, resp.parentId, currentName) : []),
    [resp, currentName],
  );
  const crumbsView = useMemo(
    () => (crumbs.length > 5 ? [crumbs[0], { ellipsis: true as const }, ...crumbs.slice(crumbs.length - 3)] : crumbs),
    [crumbs],
  );

  const hist = historyRef.current;
  const currentSize = resp?.parentSize ?? 0;
  // files under the current folder: sum of each direct child's file count
  const filesUnder = (resp?.entries ?? []).reduce((n, e) => n + (e.isDir ? e.fileCount : 1), 0);
  const shareBasis = volumeTotal || totalBytes;
  const volumeShare =
    currentSize > 0 && shareBasis > 0 ? `${((currentSize / shareBasis) * 100).toFixed(1)}%` : null;

  // Hover-card content rebuilds only when the hover identity (or the
  // context it reads) changes; pointer moves never touch this memo.
  const hoverCard = useMemo<HoverCardData | null>(() => {
    if (!hover) return null;
    const f = findEntry(hover.entryId);
    const parent = resp?.path ?? rootPath;
    const share =
      currentSize > 0
        ? `${formatPercent(hover.size / currentSize)} of ${leafName(parent)}`
        : undefined;
    // Dirs carry a real count line (files measured by the scan); the
    // payload itself has no count field — resolve honestly by entryId,
    // omit if the id is not in the current view.
    const count = hover.isDir && f?.e.isDir ? `${formatCount(f.e.fileCount)} files` : undefined;
    const cs = categoryStyle(hover.cat, theme);
    return {
      name: hover.name,
      size: formatBytes(hover.size),
      share,
      cat: { label: CATEGORY_LABELS[hover.cat] ?? CATEGORY_LABELS[CATEGORY_LABELS.length - 1], color: cs.css },
      count,
      path: f?.path,
    };
  }, [hover, currentSize, resp, rootPath, theme, findEntry]);

  const menuEntry = menu && menu.id !== null ? findEntry(menu.id) : null;

  const mapState =
    loadErr !== null
      ? "error"
      : resp === null
        ? "loading"
        : resp.entries.length === 0
          ? "empty"
          : null;

  const toggleRail = useCallback(() => setRailOpen((v) => !v), []);

  return (
    <div className="screen fade-in">
      <Topbar
        backDisabled={hist.idx <= 0}
        fwdDisabled={hist.idx >= hist.ids.length - 1}
        crumbs={crumbsView}
        query={query}
        sort={sort}
        railOpen={railOpen}
        searchRef={searchRef}
        onBack={goBack}
        onForward={goForward}
        onCrumb={goCrumb}
        onQuery={setQuery}
        onClearQuery={clearQuery}
        onSort={setSort}
        onPalette={onOpenPalette}
        onRail={toggleRail}
      />

      <div className="content">
        {/* The map panel: the treemap fills it edge to edge; every overlay
            below seats inside the wrap (the canvases' own coordinate space). */}
        <div className="panel map-panel">
          <div className="map-wrap" ref={mapWrapRef} onPointerDown={dismissHint} onWheel={dismissHint}>
            <MemoMap
              ref={mapRef}
              entries={sorted}
              parentSize={currentSize}
              theme={theme}
              hiddenCats={hiddenCats}
              highlightIds={highlightIds}
              // FLEET §G cross-file contract: payloads land in the pooled
              // plate layer by entryId. A stored `null` is the pipeline's
              // honest failure — the renderer shows its fallback card for
              // it. (The canvas's prop type still reads
              // ReadonlyMap<number, PreviewPayload> while the contract is
              // `payload | null`; the cast tracks the contract — drop it
              // when that type widens.)
              previews={previews as ReadonlyMap<number, PreviewPayload>}
              // Lazy recursive interiors: settled children by folder entryId,
              // and the stable request callback the renderer fires when a
              // folder's interior is large enough to draw but not yet known.
              expanded={expanded}
              onNeedExpand={onNeedExpand}
              onNeedPreview={onNeedPreview}
              onZoomCommit={onZoomCommit}
              onOpenFile={doOpen}
              onPick={onPick}
              onSelectionChange={onSelectionChange}
              onHoverChange={onHoverChange}
              onContextMenu={onContextMenu}
            />

            <HoverCard ref={cardRef} card={hoverCard} x={hover?.x ?? 0} y={hover?.y ?? 0} />

            {mapState && (
              <div className="map-state tex-dots">
                <div className="map-state-inner">
                  {mapState === "error" && (
                    <>
                      <p className="map-state-copy">Couldn’t read this folder</p>
                      <p className="num map-state-reason">{loadErr}</p>
                    </>
                  )}
                  {mapState === "loading" && <p className="map-state-copy">Reading folder…</p>}
                  {mapState === "empty" && <p className="map-state-copy">This folder has no entries to draw.</p>}
                </div>
              </div>
            )}

            {/* One ever-teaching gesture chip: sentence-case micro copy on a
                quiet chip, bottom centre. Gone across sessions once read. */}
            {hint !== "gone" && sorted.length > 0 && !query && (
              <div className={`map-chip map-first${hint === "fading" ? " is-gone" : ""}`}>
                <span className="micro">drag to pan · double-click to open · shift-drag to select</span>
              </div>
            )}

            {sorted.length > 0 && query && (
              <div className="map-chip">
                <span className="num">{formatCount(matches)}</span>
                <span>of</span>
                <span className="num">{formatCount(sorted.length)}</span>
                <span>entries match “{query.trim()}”</span>
              </div>
            )}

            {toast && (
              <div className="map-chip map-toast">
                <span className="micro">{toast}</span>
              </div>
            )}

            {menu && menuEntry && (
              <ContextMenu
                x={menu.x}
                y={menu.y}
                entry={menuEntry}
                onOpen={(id) => {
                  if (menuEntry.e.isDir) zoomIn(id, menuEntry.e.name);
                  else doOpen(id);
                }}
                onReveal={doReveal}
                onCopy={doCopy}
              />
            )}
          </div>
        </div>

        {/* The inspector rail panel: a .panel wrapping the rail component. */}
        {railOpen && (
          <div className="panel rail-panel">
            <MemoRail
              selection={selection}
              current={railCurrent}
              totalScan={shareBasis}
              categories={categories}
              hiddenCats={hiddenCats}
              onToggleCat={toggleCat}
              largest={largest}
              onPickLargest={pickLargest}
              onReveal={doReveal}
              onCopyPath={doCopy}
              onOpen={doOpen}
              selectedId={selIds.length === 1 ? selIds[0] : null}
              currentId={resp?.parentId ?? -1}
              theme={theme}
              // Rail's optional readout states, fed from the map panel's own
              // state: no response yet → dashed hero + skeletons; a failed
              // read → the engine's verbatim message in the readout cell.
              // Both optional upstream, so a frame where neither applies
              // renders the rail data-ready, never blank.
              loading={mapState === "loading"}
              error={mapState === "error" ? loadErr : null}
            />
          </div>
        )}
      </div>

      <Statusbar
        items={resp?.totalEntries ?? 0}
        files={filesUnder}
        size={currentSize}
        volumeShare={volumeShare}
        hidden={hiddenCats.size}
        theme={theme}
        onRescan={onRescan}
        onDrives={onShowDrives}
        onTheme={onToggleTheme}
        onUnhideAll={unhideAll}
      />
    </div>
  );
}

// ------------------------------------------------------------ topbar
// Memoized so pointer-level state (hover identity, selection) never
// re-renders the chrome; every callback below is stable across moves.

interface TopbarProps {
  backDisabled: boolean;
  fwdDisabled: boolean;
  crumbs: Crumb[];
  query: string;
  sort: SortMode;
  railOpen: boolean;
  searchRef: RefObject<HTMLInputElement | null>;
  onBack(): void;
  onForward(): void;
  onCrumb(c: { id: number; name: string }): void;
  onQuery(q: string): void;
  onClearQuery(): void;
  onSort(s: SortMode): void;
  onPalette(): void;
  onRail(): void;
}

const Topbar = memo(function Topbar({
  backDisabled,
  fwdDisabled,
  crumbs,
  query,
  sort,
  railOpen,
  searchRef,
  onBack,
  onForward,
  onCrumb,
  onQuery,
  onClearQuery,
  onSort,
  onPalette,
  onRail,
}: TopbarProps) {
  return (
    <header className="topbar">
      <div className="nav-pair">
        <IconButton label="Back" disabled={backDisabled} onClick={onBack}>
          <ChevronLeft size={16} />
        </IconButton>
        <IconButton label="Forward" disabled={fwdDisabled} onClick={onForward}>
          <ChevronRight size={16} />
        </IconButton>
      </div>

      <nav className="crumbs" aria-label="Folder path">
        {crumbs.map((c, i) =>
          "ellipsis" in c ? (
            <span key="e" className="crumb-ellipsis">
              …
            </span>
          ) : (
            <span key={c.id} className="crumb-slot">
              {i > 0 && (
                <span className="crumb-sep">
                  <ChevronRight size={12} />
                </span>
              )}
              <button type="button" className="crumb" onClick={() => onCrumb(c)}>
                {c.name}
              </button>
            </span>
          ),
        )}
      </nav>

      <div className="searchbox">
        <Search size={13} />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          placeholder="Filter files and folders"
          spellCheck={false}
        />
        {query && (
          <IconButton label="Clear" onClick={onClearQuery}>
            <X size={12} />
          </IconButton>
        )}
      </div>

      <SegmentedControl<SortMode>
        label="Sort by"
        options={[
          { id: "size", label: "Size" },
          { id: "name", label: "Name" },
          { id: "type", label: "Type" },
        ]}
        value={sort}
        onChange={onSort}
      />

      <button type="button" className="pal-hint" aria-label="Command palette (Ctrl K)" onClick={onPalette}>
        <KeyHint keys="Ctrl K" />
      </button>

      <IconButton label="Toggle inspector" active={railOpen} onClick={onRail}>
        <PanelRight size={15} />
      </IconButton>
    </header>
  );
});

// ------------------------------------------------------------ statusbar
// A transparent mono strip: readouts left, quiet actions right. Memoized
// behind primitives — it never re-renders for hover or pointer state.

interface StatusbarProps {
  items: number;
  files: number;
  size: number;
  volumeShare: string | null;
  hidden: number;
  theme: "light" | "dark";
  onRescan(): void;
  onDrives(): void;
  /** App's theme callback (the view's prop contract). The shell API below is
      the fallback when it is absent — they are one function in practice:
      App builds ShellContext from this very callback. */
  onTheme?: () => void;
  /** Chip action: the hidden-count readout is also the switch — clicking it
      unhides every filtered category at once. */
  onUnhideAll(): void;
}

const Statusbar = memo(function Statusbar({
  items,
  files,
  size,
  volumeShare,
  hidden,
  theme,
  onRescan,
  onDrives,
  onTheme,
  onUnhideAll,
}: StatusbarProps) {
  // The shell's theme switch (toggleTheme) — the fallback path above.
  const { toggleTheme } = useShell();
  return (
    <footer className="statusbar">
      <div className="sb-readouts">
        <span className="sb-cell">
          <span className="sb-key">Items</span>
          <span className="num">{formatCount(items)}</span>
        </span>
        <span className="sb-cell sb-opt">
          <span className="sb-key">Files</span>
          <span className="num">{formatCount(files)}</span>
        </span>
        <span className="sb-cell">
          <span className="sb-key">Size</span>
          <span className="num">{formatBytes(size)}</span>
        </span>
        {volumeShare !== null && (
          <span className="sb-cell">
            <span className="sb-key">Of volume</span>
            <span className="num">{volumeShare}</span>
          </span>
        )}
        {hidden > 0 && (
          <button type="button" className="chip sb-hid" onClick={onUnhideAll} title="Show all categories">
            <span className="chip-dot" aria-hidden="true" />
            {hidden} categor{hidden === 1 ? "y" : "ies"} hidden
          </button>
        )}
      </div>
      <div className="sb-actions">
        <button type="button" className="status-btn" onClick={onRescan}>
          Rescan
        </button>
        <button type="button" className="status-btn" onClick={onDrives}>
          Drives
        </button>
        <button type="button" className="status-btn" onClick={onTheme ?? toggleTheme}>
          {theme === "dark" ? "Light" : "Dark"}
        </button>
      </div>
    </footer>
  );
});

// ------------------------------------------------------------ context menu
// A .float surface: 24px sentence-case rows, mono keycaps that are all
// wired (↵ and Ctrl C live in the key handler above). Reveal/open ride the
// demo-aware ipc exports; copy rides the App-owned clipboard api.

function ContextMenu({
  x,
  y,
  entry,
  onOpen,
  onReveal,
  onCopy,
}: {
  x: number;
  y: number;
  entry: { e: ChildEntry; path: string };
  onOpen: (id: number) => void;
  onReveal: (id: number) => void;
  onCopy: (id: number) => void;
}) {
  const id = entry.e.id;
  const left = Math.max(8, Math.min(x, window.innerWidth - 224));
  const top = Math.max(8, Math.min(y, window.innerHeight - 168));
  return (
    <div className="menu float" style={{ left, top }} onPointerDown={(e) => e.stopPropagation()}>
      <button type="button" className="menu-item" onClick={() => onOpen(id)}>
        {entry.e.isDir ? "Open folder" : "Open file"}
        <span className="kbd">↵</span>
      </button>
      <div className="menu-sep" role="separator" />
      <button type="button" className="menu-item" onClick={() => onReveal(id)}>
        Reveal in Explorer
      </button>
      <button type="button" className="menu-item" onClick={() => onCopy(id)}>
        Copy path
        <span className="kbd">Ctrl C</span>
      </button>
    </div>
  );
}

// ------------------------------------------------------------ helpers

function leafName(p: string): string {
  const clean = p.replace(/[\\/]+$/, "");
  const i = Math.max(clean.lastIndexOf("\\"), clean.lastIndexOf("/"));
  return i >= 0 ? clean.slice(i + 1) : clean || p;
}

function joinPath(base: string, name: string): string {
  if (!base) return name;
  const sep = base.includes("\\") || /^[A-Za-z]:/.test(base) ? "\\" : "/";
  return `${base.replace(/[\\/]+$/, "")}${sep}${name}`;
}


type Crumb = { id: number; name: string } | { ellipsis: true };

function withCurrent(
  lineage: { id: number; name: string }[],
  parentId: number,
  currentName: string,
): Crumb[] {
  const list: Crumb[] = lineage.length ? [...lineage] : [{ id: parentId, name: currentName }];
  const last = list[list.length - 1] as { id?: number };
  if (last.id !== parentId) list.push({ id: parentId, name: currentName });
  else list[list.length - 1] = { id: parentId, name: currentName };
  return list;
}
