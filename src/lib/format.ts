// Byte / number formatting with tabular-friendly output.
// Pure and deterministic: grouping separators are placed by hand in en-US
// style (no Intl), so pooled plates, hover cards and screenshot harnesses
// read identically on every host locale. Every string producer prints "—"
// for an unknown (non-finite/negative) value — the PRODUCT.md honest-number
// rule; nothing here ever fabricates a 0.

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Thin no-break space (U+202F) between a figure and its unit: keeps
 *  "12.4 MB" one unbreakable pair in wrapped DOM readouts and sits tighter
 *  than a full space in Geist Mono tabular figures. Exported so
 *  hand-assembled strings (aria-labels, stats) reuse the exact character. */
export const UNIT_SEP = "\u202f";

/** The honest unknown. */
const DASH = "—";

function pad2(n: number): string {
  return n < 10 ? "0" + n : String(n);
}

/** Bounded memo: tile/rail/plate paths re-ask for the same byte counts
 *  every frame, and one shared string instance per distinct value keeps
 *  that path allocation-light. Keys are the raw numbers (no string
 *  conversion on lookup); the table clears at the cap, which amortizes to
 *  nothing even under monotonically-growing scan counters. */
const MEMO_CAP = 4096;
const bytesMemo = new Map<number, string>();

/** 1536 -> "1.50 KB" (thin separator). Fixed precision per magnitude band
 *  (3 significant figures: 9.99 / 99.9 / 999) — widest output is 7 chars,
 *  the preview plate's fixed 7ch value track, and same-band figures align
 *  perfectly under tabular numerals. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return DASH;
  const memo = bytesMemo.get(n);
  if (memo !== undefined) return memo;
  let out: string;
  if (n < 1024) {
    out = `${Math.round(n)}${UNIT_SEP}B`;
  } else {
    const { num, unit } = splitBytes(n);
    out = `${num}${UNIT_SEP}${unit}`;
  }
  if (bytesMemo.size >= MEMO_CAP) bytesMemo.clear();
  bytesMemo.set(n, out);
  return out;
}

/** 1536 -> { value: 1.5, unit: "KB" } — raw split, no separator, no rounding. */
export function bytesValue(n: number): { value: number; unit: string } {
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return { value: v, unit: UNITS[u] };
}

/** 1536 -> { num: "1.50", unit: "KB" } — for big numerals with a smaller
 *  unit. Digits adapt to magnitude: 2 below 10, 1 below 100, 0 from 100. */
export function splitBytes(n: number): { num: string; unit: string } {
  const { value, unit } = bytesValue(n);
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return { num: value.toFixed(digits), unit };
}

/** 12345 -> "12,345"; grouped only from 10,000 up — below that plain digits
 *  read cleaner in mono. Separators are hand-placed (locale-fixed en-US by
 *  contract — pooled plates must be deterministic across machines). */
export function formatCount(n: number): string {
  if (!Number.isFinite(n)) return DASH;
  const neg = n < 0;
  const s = Math.abs(n).toString();
  if (s.indexOf("e") >= 0) return DASH; // astronomically large — nothing honest to print
  const dot = s.indexOf(".");
  const intPart = dot < 0 ? s : s.slice(0, dot);
  const frac = dot < 0 ? "" : s.slice(dot);
  if (intPart.length < 5) return (neg ? "-" : "") + intPart + frac;
  let grouped = "";
  for (let k = 0; k < intPart.length; k++) {
    if (k > 0 && (intPart.length - k) % 3 === 0) grouped += ",";
    grouped += intPart[k];
  }
  return (neg ? "-" : "") + grouped + frac;
}

/** 84000 -> "1m 24s"; under a minute "12s"; past an hour "1h 02m 03s".
 *  Seconds always pad to two from the minutes step up, so tabular columns
 *  of durations stay aligned. */
export function formatElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return DASH;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (s >= 3600) return `${Math.floor(s / 3600)}h ${pad2(m)}m ${pad2(r)}s`;
  return `${m}m ${pad2(r)}s`;
}

/** 3723000 -> "1:02:03" — clock stamp for media durations (m:ss under an
 *  hour). Elapsed prose ("1h 02m 03s") and clock stamps stay distinct. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return DASH;
  const s = Math.floor(ms / 1000);
  const r = s % 60;
  const m = Math.floor((s % 3600) / 60);
  if (s >= 3600) return `${Math.floor(s / 3600)}:${pad2(m)}:${pad2(r)}`;
  return `${m}:${pad2(r)}`;
}

/** Shorten a long path for display while always keeping its TAIL — the
 *  filename is the identity. C:\Users\casey\AppData\Local\Temp\cache.db
 *  -> C:\Users\casey\…\cache.db; at tighter budgets the head shrinks first
 *  (root letter kept as long as it fits), e.g. C:\Users\seg\...\functions.
 *  Only a tail that alone exceeds `maxLen` is cut, and it is cut from the
 *  left so the ending — the part that names the file — survives. */
export function ellipsizePath(p: string, maxLen = 64): string {
  if (p.length <= maxLen) return p;
  const sep = p.indexOf("\\") >= 0 ? "\\" : "/";
  const parts = p.split(/[\\/]/);
  // Root identity rides the head side: a drive letter ("C:") or the UNC
  // authority ("\\server") — losing the volume in a multi-volume app would
  // lose meaning, so it is kept until the budget is truly exhausted.
  let head: string[] = [];
  let from = 0;
  if (parts.length >= 2 && parts[0] === "" && parts[1] === "") {
    head = parts.slice(0, 3); // "", "", "server" rejoins as \\server
    from = 3;
  } else if (/^[A-Za-z]:$/.test(parts[0])) {
    head = parts.slice(0, 1);
    from = 1;
  } else if (parts[0] === "") {
    head = [""]; // absolute POSIX root: ["", "home"] rejoins as /home
    from = 1;
  }
  const segs = parts.slice(from).filter((s) => s !== ""); // also drops a trailing "\" (dir paths)
  if (segs.length === 0) return p; // bare root/authority — nothing to elide

  // Middle-truncation candidates, fullest first: up to two head segments
  // past the root, up to two tail segments. Tail beats head on ties — the
  // file's own name is the identity; the root letter survives until the
  // budget is truly gone.
  const MAX_HEAD = Math.min(2, segs.length);
  const MAX_TAIL = Math.min(2, segs.length);
  const order: readonly (readonly [number, number])[] = [
    [MAX_HEAD, MAX_TAIL],
    [MAX_HEAD, 1],
    [1, MAX_TAIL],
    [1, 1],
    [0, MAX_TAIL],
    [0, 1],
  ];
  for (const [h, t] of order) {
    if (h + t > segs.length || (h === 0 && t === 0)) continue;
    const headStr = h > 0 ? head.concat(segs.slice(0, h)).join(sep) : head.join(sep);
    const tail = segs.slice(segs.length - t).join(sep);
    const out = headStr ? `${headStr}${sep}…${sep}${tail}` : `…${sep}${tail}`;
    if (out.length <= maxLen) return out;
  }
  // Even the last segment alone is too long: cut it from the left so the
  // tail of the name — the part that identifies the file — survives.
  const last = segs[segs.length - 1];
  return `…${last.slice(Math.max(0, last.length - (maxLen - 1)))}`;
}

/** 0.153 -> "15%", 0.051 -> "5.1%". Below 10% one decimal, from 10% whole
 *  numbers (the drives ledger prints its own "<0.1%" for the sub-permille
 *  band); unknown or negative input prints the dash, never a fabricated 0. */
export function formatPercent(frac: number): string {
  if (!Number.isFinite(frac) || frac < 0) return DASH;
  const p = frac * 100;
  if (p >= 100) return "100%";
  if (p >= 10) return `${p.toFixed(0)}%`;
  if (p > 0) return `${p.toFixed(1)}%`;
  return "0%";
}

/** Word choice for a count: plural(n, "file") -> "file" / "files". Pass the
 *  irregular form explicitly when the naive +s is wrong (e.g. "entry"). */
export function plural(n: number, one: string, many?: string): string {
  return n === 1 ? one : many ?? `${one}s`;
}

/** 1 -> "1 file", 12345 -> "12,345 files" — count + correct noun in one
 *  readout, so "1 items" can never ship. */
export function formatFiles(n: number): string {
  return `${formatCount(n)} ${plural(n, "file")}`;
}

/** 1 -> "1 item", 12345 -> "12,345 items" — see formatFiles. */
export function formatItems(n: number): string {
  return `${formatCount(n)} ${plural(n, "item")}`;
}
