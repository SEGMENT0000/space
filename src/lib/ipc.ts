// Typed wrappers around the Rust engine. Every IPC touchpoint lives here.
// When the app runs outside Tauri (browser dev server), `enableDemo()` swaps
// every call to the synthetic dataset in demo.ts.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { buildDemo, type DemoState } from "./demo";
import type {
  ChildrenResponse,
  DonePayload,
  DriveInfo,
  ErrorPayload,
  LargestFile,
  PreviewPayload,
  ProgressPayload,
  ScanStateDto,
} from "./types";

let demo: DemoState | null = null;

// getChildren response cache (FLEET §D): navigation refires the same node
// (back/forward, breadcrumbs, sort toggles) — memoize per query shape so those
// never re-cross the wire. Responses are treated as immutable (grep-verified:
// ExplorerView copies entries before sorting), so hits hand back the SAME
// object. Capped at 64; Map iterates in insertion order, so evicting the first
// key always drops the oldest entry.
const childrenCache = new Map<string, ChildrenResponse>();
const CHILDREN_CACHE_MAX = 64;

function cacheChildren(key: string, resp: ChildrenResponse): ChildrenResponse {
  childrenCache.set(key, resp);
  while (childrenCache.size > CHILDREN_CACHE_MAX) {
    const oldest = childrenCache.keys().next().value;
    if (oldest === undefined) break; // unreachable guard; keeps the loop total
    childrenCache.delete(oldest);
  }
  return resp;
}

export function enableDemo(): DemoState {
  childrenCache.clear(); // new dataset — cached listings describe the old one
  demo = buildDemo();
  return demo;
}

export function demoState(): DemoState | null {
  return demo;
}

export function listDrives(): Promise<DriveInfo[]> {
  if (demo) return Promise.resolve(demo.drives);
  return invoke("list_drives");
}

export function startScan(root: string): Promise<void> {
  childrenCache.clear(); // a fresh scan invalidates every cached node listing
  if (demo) return Promise.resolve();
  return invoke("start_scan", { root });
}

export function cancelScan(): Promise<void> {
  if (demo) return Promise.resolve();
  return invoke("cancel_scan");
}

export function getChildren(
  nodeId: number,
  depth = 2,
  sort: "size" | "name" = "size",
  limit = 900,
): Promise<ChildrenResponse> {
  const key = `${nodeId}|${depth}|${sort}|${limit}`;
  const hit = childrenCache.get(key);
  // Same object on hit — see the cache note above (consumers only read).
  if (hit) return Promise.resolve(hit);
  if (demo) {
    const r = demo.children(nodeId);
    // Rejections are never cached, so a later call can retry.
    if (!r) return Promise.reject(new Error(`demo: no node ${nodeId}`));
    return Promise.resolve(cacheChildren(key, r));
  }
  // Cache only fulfilled responses — a rejected invoke must not poison the
  // key (the caller's retry has to reach the wire again).
  return invoke<ChildrenResponse>("get_children", { nodeId, depth, sort, limit }).then((r) =>
    cacheChildren(key, r),
  );
}

/** Invokes the `get_preview(path)` Tauri command (signature stable: one
 *  `path` arg, one `PreviewPayload` result). Never rejects with a raw value:
 *  the Rust side rejects with a bare String, so failures are normalized to a
 *  real `Error` here — preview.ts owns the retry/failure policy and only ever
 *  sees an Error instance. */
export function getPreview(path: string): Promise<PreviewPayload> {
  // Demo branch first (getChildren template): the browser build answers from
  // the synthesized dataset and never touches the wire.
  if (demo) return Promise.resolve(demo.getPreview(path));
  return invoke<PreviewPayload>("get_preview", { path }).catch((err: unknown) => {
    if (err instanceof Error) throw err;
    throw new Error(typeof err === "string" && err ? err : "preview request failed");
  });
}

/** Opens the native folder picker and resolves the chosen absolute path, or
 *  null when the dialog was cancelled. Browser demo mode has no dialog. */
export function pickFolder(): Promise<string | null> {
  if (demo) return Promise.resolve(null);
  return invoke<string | null>("pick_folder");
}

export function getLargestFiles(limit = 40): Promise<LargestFile[]> {
  if (demo) return Promise.resolve(demo.largest.slice(0, limit));
  return invoke("get_largest_files", { limit });
}

export function getCategoryTotals(nodeId: number): Promise<number[]> {
  if (demo) return Promise.resolve(demo.categoryTotals(nodeId));
  return invoke("get_category_totals", { nodeId });
}

/** Reveals an item in Explorer (read-only shell action; opens the parent
 *  with the item selected). Resolves false in demo mode or when the shell
 *  refuses — callers show a quiet statusbar note, never an error dialog. */
export async function revealInExplorer(path: string): Promise<boolean> {
  if (demo) return false;
  try {
    await revealItemInDir(path);
    return true;
  } catch {
    return false;
  }
}

/** Opens a path with the system handler (folder in Explorer, file in its
 *  default app). Read-only by construction — this never modifies bytes. */
export async function openWithSystem(path: string): Promise<boolean> {
  if (demo) return false;
  try {
    await openPath(path);
    return true;
  } catch {
    return false;
  }
}

export function scanState(): Promise<ScanStateDto> {
  if (demo) return Promise.resolve(demo.scanState());
  return invoke("scan_state");
}

export function onScanProgress(cb: (p: ProgressPayload) => void): Promise<UnlistenFn> {
  if (demo) return Promise.resolve(() => undefined);
  return listen<ProgressPayload>("scan://progress", (e) => cb(e.payload));
}

export function onScanDone(cb: (p: DonePayload) => void): Promise<UnlistenFn> {
  if (demo) return Promise.resolve(() => undefined);
  return listen<DonePayload>("scan://done", (e) => cb(e.payload));
}

export function onScanError(cb: (p: ErrorPayload) => void): Promise<UnlistenFn> {
  if (demo) return Promise.resolve(() => undefined);
  return listen<ErrorPayload>("scan://error", (e) => cb(e.payload));
}
