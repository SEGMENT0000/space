// Preview cache for the treemap plate system (FLEET §G). loadPreview is the
// ONE entry point for files AND folders — Rust sniffs dir-vs-file and the
// family itself. The cache is keyed by entryId (never path: paths resolve in
// ExplorerView), dedupes in-flight requests, and never throws: any failure
// resolves null and is NOT remembered, so the next zoom retries cleanly.
import { getPreview } from "./ipc";
import type { PreviewPayload } from "./types";

const PREVIEW_CACHE_MAX = 300; // §G: LRU cap ~300

// Insertion order doubles as recency: touch() re-inserts so the FIRST key is
// always the least-recently-used one.
const cache = new Map<number, PreviewPayload>();
const inflight = new Map<number, Promise<PreviewPayload | null>>();

// Epoch guard: there is no abort in Tauri invoke, so cancellation is a
// generation check — clearPreviews() bumps the epoch and any result that was
// already in flight resolves null instead of being cached.
let epoch = 0;

function touch(id: number, payload: PreviewPayload): void {
  cache.delete(id);
  cache.set(id, payload); // re-insert moves the key to the recent end
}

function evictLru(): void {
  while (cache.size > PREVIEW_CACHE_MAX) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break; // unreachable guard; keeps the loop total
    cache.delete(oldest);
  }
}

export function loadPreview(entryId: number, path: string): Promise<PreviewPayload | null> {
  const hit = cache.get(entryId);
  if (hit) {
    touch(entryId, hit); // a cache hit is a "use" — refresh its recency
    return Promise.resolve(hit);
  }
  const pending = inflight.get(entryId);
  // WHY dedupe: deep zoom fires onNeedPreview repeatedly for the same tile as
  // it crosses the size threshold on every frame — the second caller gets the
  // first caller's promise, not a second invoke.
  if (pending) return pending;

  const myEpoch = epoch;
  const promise = getPreview(path)
    .then((payload) => {
      // Epoch moved on (clearPreviews fired mid-flight): drop the payload and
      // cache nothing.
      if (myEpoch !== epoch) return null;
      touch(entryId, payload);
      evictLru();
      return payload;
    })
    // Never throws per §G. The failure is not cached, so a later request for
    // the same entry retries the wire.
    .catch(() => null)
    .finally(() => {
      // Only clear OUR entry — a newer load (post-clear) may have replaced it.
      if (inflight.get(entryId) === promise) inflight.delete(entryId);
    });
  inflight.set(entryId, promise);
  return promise;
}

export function peekPreview(entryId: number): PreviewPayload | null {
  const hit = cache.get(entryId);
  // Peeked tiles are the hot ones — treat the peek as an LRU use.
  if (hit) touch(entryId, hit);
  return hit ?? null;
}

export function clearPreviews(): void {
  // The map changed hands (new scan, demo swap, ExplorerView nav-reset):
  // cached payloads describe the old dataset and in-flight ones would land
  // stale. Clearing both maps plus the epoch bump means late results resolve
  // null instead of resurrecting old data.
  cache.clear();
  inflight.clear();
  epoch += 1;
}
