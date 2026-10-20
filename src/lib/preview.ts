// Preview pipeline for the treemap plate system (FLEET §G). loadPreview is
// the ONE entry point for files AND folders — Rust sniffs dir-vs-file and the
// family itself. The cache is keyed by entryId (never path: paths resolve in
// ExplorerView).
//
// CONTRACT — read before wiring UI against this module:
//   • `null` means UNREADABLE / FAILED, never "empty content". Callers must
//     render their honest fallback card for null; an empty box would lie
//     about a file that exists but could not be previewed.
//   • Nothing here ever rejects. Any failure — IPC error, stale generation,
//     exhausted retry — resolves null.
//   • Cache: LRU by entryId, cap PREVIEW_CACHE_MAX (400), oldest dropped
//     first; in-flight ids are never evicted.
//   • In-flight dedupe: concurrent requests for one entryId share one
//     promise; failures are retried EXACTLY once with backoff, then the id is
//     remembered as failed until the next generation (invalidatePreviews /
//     setPreviewGeneration), so a broken tile never re-stamps IPC on every
//     frame.
//   • Queued delivery: requestPreview / requestPreviews coalesce into one
//     drain per animation frame with PREVIEW_CONCURRENCY (8) requests in
//     flight, so a screen of 200 tiles cannot stampede the backend.
//   • Generation safety: there is no abort in Tauri invoke, so cancellation
//     is a generation check — responses (and queued jobs) captured under an
//     older generation resolve null instead of caching stale data.
import { getPreview } from "./ipc";
import type { PreviewPayload } from "./types";

/** Delivery callback for the queued API. Always invoked asynchronously (a
 *  microtask), even on cache hits and failures, so callers can safely call
 *  back into this module from a listener. `payload === null` means
 *  unreadable/failed — render the fallback card. */
export type PreviewResultHandler = (entryId: number, payload: PreviewPayload | null) => void;

/** Item shape accepted by requestPreviews — entryId plus the path that only
 *  the view layer can resolve (the treemap itself is path-dumb). */
export interface PreviewRequest {
  id: number;
  path: string;
}

const PREVIEW_CACHE_MAX = 400; // §G: LRU cap
const PREVIEW_CONCURRENCY = 8; // requests in flight from the queue drain
const RETRY_DELAY_MS = 250; // single-retry backoff

// Insertion order doubles as recency: touch() re-inserts so the FIRST key is
// always the least-recently-used one.
const cache = new Map<number, PreviewPayload>();
const inflight = new Map<number, Promise<PreviewPayload | null>>();
// Failures remembered per generation: after the one retry fails, the id
// resolves null on sight until invalidatePreviews()/setPreviewGeneration().
const failed = new Set<number>();

// Queue for the batched API. Map preserves enqueue order (fair, first-request
// first-served), dedupes an id that is re-requested while still queued, and
// holds every listener awaiting that id's single result.
const queue = new Map<number, { path: string; gen: number; listeners: Set<PreviewResultHandler> }>();
let active = 0; // queued requests currently awaiting loadPreview
let drainScheduled = false;

let generation = 0;

function touch(id: number, payload: PreviewPayload): void {
  cache.delete(id);
  cache.set(id, payload); // re-insert moves the key to the recent end
}

function evictLru(): void {
  // In-flight ids are never evicted: an id being fetched is not in `cache`
  // yet, but the guard below keeps that true even if revalidation is ever
  // added — a "hot" key is deferred to the recent end instead of dropped.
  let guard = cache.size;
  while (cache.size > PREVIEW_CACHE_MAX && guard > 0) {
    guard -= 1;
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break; // unreachable guard; keeps the loop total
    const oldestPayload = cache.get(oldest);
    if (oldestPayload && inflight.has(oldest)) {
      touch(oldest, oldestPayload);
      continue; // deferred, not evicted (bounded by the guard)
    }
    cache.delete(oldest);
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

// One wire attempt. Resolves the payload (caching it), or null when the
// generation moved on mid-flight — a stale result is dropped, not cached and
// not counted as a failure. Rejects only when the IPC call itself failed.
async function attemptOnce(
  entryId: number,
  path: string,
  myGen: number,
): Promise<PreviewPayload | null> {
  const payload = await getPreview(path);
  // Generation moved on (invalidatePreviews / setPreviewGeneration fired
  // mid-flight): drop the payload and cache nothing.
  if (myGen !== generation) return null;
  touch(entryId, payload);
  evictLru();
  return payload;
}

// Exactly one retry with backoff, then remember the failure for this
// generation. A generation change mid-retry aborts cleanly (no retry, no
// failure memory — the next generation gets a fresh budget).
async function fetchWithRetry(
  entryId: number,
  path: string,
  myGen: number,
): Promise<PreviewPayload | null> {
  try {
    return await attemptOnce(entryId, path, myGen);
  } catch {
    if (myGen !== generation) return null;
    await delay(RETRY_DELAY_MS);
    if (myGen !== generation) return null;
    try {
      return await attemptOnce(entryId, path, myGen);
    } catch {
      if (myGen === generation) failed.add(entryId);
      return null;
    }
  }
}

/** The promise-based entry point. Cached hit → immediate payload; concurrent
 *  call for the same id → the shared in-flight promise; known failure in this
 *  generation → null without touching the wire. Never rejects: any failure
 *  (including an exhausted retry) resolves `null`, which means UNREADABLE —
 *  the UI draws its fallback card, never an empty box. */
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
  if (failed.has(entryId)) return Promise.resolve(null); // retried once; stop re-stamping IPC

  const myGen = generation;
  const promise = fetchWithRetry(entryId, path, myGen).finally(() => {
    // Only clear OUR entry — a newer load (post-invalidate) may have replaced it.
    if (inflight.get(entryId) === promise) inflight.delete(entryId);
  });
  inflight.set(entryId, promise);
  return promise;
}

/** Synchronous cache probe (recency-refreshing). Returns null on a miss —
 *  this never fetches and never reports a remembered failure distinctly. */
export function peekPreview(entryId: number): PreviewPayload | null {
  const hit = cache.get(entryId);
  // Peeked tiles are the hot ones — treat the peek as an LRU use.
  if (hit) touch(entryId, hit);
  return hit ?? null;
}

// Delivery is ALWAYS a microtask: listeners may safely call back into this
// module (requestPreview, loadPreview) without re-entrancy surprises, and the
// cached-hit / failed / queued paths all behave identically.
function deliver(entryId: number, payload: PreviewPayload | null, listeners: Set<PreviewResultHandler>): void {
  if (listeners.size === 0) return;
  queueMicrotask(() => {
    for (const listener of listeners) {
      try {
        listener(entryId, payload);
      } catch (err) {
        // A buggy listener must not take down the drain or its neighbours.
        console.error("space: preview listener failed", entryId, err);
      }
    }
  });
}

function scheduleDrain(): void {
  if (drainScheduled || queue.size === 0) return;
  drainScheduled = true;
  const raf: (cb: FrameRequestCallback) => number =
    typeof requestAnimationFrame === "function"
      ? requestAnimationFrame.bind(window)
      : (cb) => window.setTimeout(() => cb(0), 16); // headless/test fallback
  raf(() => {
    drainScheduled = false;
    drain();
  });
}

// One drain per frame: dispatch queued jobs up to the concurrency cap; each
// completion refills the freed slot until the queue is empty.
function drain(): void {
  while (active < PREVIEW_CONCURRENCY && queue.size > 0) {
    const next = queue.keys().next().value;
    if (next === undefined) break; // unreachable guard; keeps the loop total
    const job = queue.get(next);
    queue.delete(next);
    if (!job) continue;
    // Queued under an older generation: the path may describe a view the
    // user has left — resolve the listeners as failed, don't touch the wire.
    if (job.gen !== generation) {
      deliver(next, null, job.listeners);
      continue;
    }
    active += 1;
    void loadPreview(next, job.path)
      .then((payload) => deliver(next, payload, job.listeners))
      .finally(() => {
        active -= 1;
        if (queue.size > 0) scheduleDrain();
      });
  }
}

/** Fire-and-forget queued preview request. Cached results and remembered
 *  failures deliver on a microtask; anything else is coalesced into the next
 *  frame's drain (concurrency PREVIEW_CONCURRENCY). Calling again for an id
 *  already queued only adds a listener — one request, one wire call. */
export function requestPreview(entryId: number, path: string, onResult?: PreviewResultHandler): void {
  const listeners = onResult ? new Set([onResult]) : new Set<PreviewResultHandler>();
  const hit = cache.get(entryId);
  if (hit) {
    touch(entryId, hit); // a cache hit is a "use" — refresh its recency
    deliver(entryId, hit, listeners);
    return;
  }
  if (failed.has(entryId)) {
    deliver(entryId, null, listeners); // retried once already this generation
    return;
  }
  const queued = queue.get(entryId);
  if (queued) {
    if (onResult) queued.listeners.add(onResult);
    return; // already scheduled — the drain will carry the newest path
  }
  queue.set(entryId, { path, gen: generation, listeners });
  scheduleDrain();
}

/** Bulk variant: enqueue many tiles at once. Every item coalesces into ONE
 *  drain per animation frame regardless of how many callers fired this
 *  frame, so a 200-tile screen issues at most PREVIEW_CONCURRENCY requests
 *  at a time. Pass one `onResult` for the whole batch (it receives each id
 *  as its result settles). */
export function requestPreviews(
  items: ReadonlyArray<PreviewRequest>,
  onResult?: PreviewResultHandler,
): void {
  for (const item of items) requestPreview(item.id, item.path, onResult);
  // requestPreview already scheduled; the shared rAF slot coalesces the batch.
}

/** Full reset: new scan, demo swap, or nav-reset changed the dataset. Clears
 *  the cache, the remembered failures, and the queue (queued listeners
 *  resolve null so the UI shows fallback cards instead of waiting forever),
 *  and bumps the generation so every in-flight response is dropped rather
 *  than cached stale. */
export function invalidatePreviews(): void {
  cache.clear();
  failed.clear();
  generation += 1;
  const jobs = [...queue.entries()];
  queue.clear();
  for (const [id, job] of jobs) {
    deliver(id, null, job.listeners);
  }
}

/** Mirrors the view layer's requestGen/abort pattern: move the pipeline to a
 *  new generation WITHOUT clearing the cache (entries are keyed by stable
 *  entryId, so payloads remain valid). In-flight responses and queued jobs
 *  captured under the old generation resolve null, and the remembered
 *  failures are reset so the new generation gets a fresh retry budget. */
export function setPreviewGeneration(gen: number): void {
  if (gen === generation) return;
  generation = gen;
  failed.clear();
}

/** @deprecated Alias for invalidatePreviews() — kept so existing imports of
 *  clearPreviews() keep working. Prefer invalidatePreviews() in new code. */
export function clearPreviews(): void {
  invalidatePreviews();
}
