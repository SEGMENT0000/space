import "../../styles/preview.css"; // plate styles live here (component-owned; index.css untouched)
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
// The app's single formatter module — no local byte/count/duration
// formatting lives here; call sites adapt to its semantics, never the reverse.
import { formatBytes, formatCount, formatElapsed } from "../../lib/format";

// ---------------------------------------------------------------------------
// In-tile preview plates — INS_BRIEF.md §2 (binding), restyled to the
// instrument-panel world: a deep sunken chassis plate (radius --r-plate,
// 0.5px hairline-strong edge, never a shadow) carrying per-family readout
// beds over the WebGL tiles.
//
// Pure DOM module: plates live in a `map-previews` overlay layer as pooled
// nodes (~24), positioned imperatively by the draw loop. They never enter the
// React render path and never scroll. All content is written via textContent
// (no innerHTML) so file bytes can never inject markup into the map overlay.
//
// Export surface (byte-stable — the renderer codes against exactly this):
//   createPlate()            → pooled `.tm-prev` root, hidden by default
//   fillPlate(el, p, opts)   → rewrite content for one payload (idempotent)
//   fillFallbackPlate(el, r) → honest unavailable state + real reason
//
// The renderer owns visibility (hidden attr or inline display), positioning
// (transform) and the --tile-w/--tile-h custom properties the frame math uses.
// ---------------------------------------------------------------------------

/** Structural mirror of the §B PreviewPayload DTO. Every field is optional so
 *  the frozen `src/lib/types.ts` shape stays assignable here regardless of
 *  which fields the Rust side marks required; ambiguous fields are `unknown`
 *  and normalized at render time. */
export interface PreviewPayload {
  family?: string;
  text?: PreviewText;
  image?: PreviewImage;
  hex?: PreviewHex;
  list?: PreviewList;
  meta?: PreviewMeta;
  db?: PreviewDb;
  media?: PreviewMedia;
}
export interface PreviewMedia {
  path: string;
  mime: string;
  kind: "video" | "audio";
  durationMs?: number;
  w?: number;
  h?: number;
}
export interface PreviewText {
  /** Landed model.rs shape: one newline-joined string (split at render). */
  content?: string;
  /** Legacy/array shape, kept tolerant. */
  lines?: string[];
  truncated?: boolean;
  totalLines?: number;
  /** BOM-detected encoding ("UTF-8", "UTF-16LE", …) — shown in header meta. */
  encoding?: string;
}
export interface PreviewImage {
  dataUrl?: string;
  w?: number;
  h?: number;
  mime?: string;
}
export interface PreviewHexRow {
  off?: number | string;
  bytes?: number[] | string;
  ascii?: string;
}
export interface PreviewHex {
  rows?: PreviewHexRow[];
  base?: number;
  total?: number;
}
export interface PreviewListEntry {
  name?: string;
  size?: number | string;
  kind?: string;
  method?: string; // archive listing only ("method-if-present" §2.3)
}
export interface PreviewList {
  entries?: PreviewListEntry[];
  truncated?: boolean;
  kind?: string;
}
export interface PreviewMeta {
  pairs?: PreviewPair[];
}
export interface PreviewPair {
  k?: string;
  v?: string;
}
export interface PreviewDbTable {
  name?: string;
  cols?: unknown; // number | string[] | {name,…}[] from the backend — count it
  rows?: number;
}
export interface PreviewDb {
  tables?: PreviewDbTable[];
  truncated?: boolean;
}

interface Box {
  w: number;
  h: number;
}
interface Shell {
  name: HTMLElement;
  meta: HTMLElement;
  body: HTMLElement;
}
/** Built column spec: labels drive the key strip, widths drive `--cols`. */
interface Cols {
  labels: string[];
  widths: string[];
  numeric: boolean[];
  rows: string[][];
}

// --------------------------------------------------------------- helpers ---

function h(tag: string, cls?: string, text?: string): HTMLElement {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (text != null) el.textContent = text;
  return el;
}

function num(v: unknown): number {
  return typeof v === "number" && isFinite(v) ? v : 0;
}

/** Forced-layout read for a plate that may currently be display:none
 *  (pooled nodes ship hidden; the draw loop reveals them only after fill).
 *  Flips the plate's inline display for ONE synchronous layout, reads,
 *  restores — the flip can never paint. Fill-time only, never per frame. */
function measureRoot<T>(root: HTMLElement, read: () => T): T {
  const prev = root.style.display;
  root.style.display = "flex";
  const v = read();
  root.style.display = prev;
  return v;
}

// Byte/count/duration formatting is imported from lib/format.ts — the app's
// single formatter module: formatBytes for sizes, formatCount for counts,
// formatElapsed for durations. Its magnitude semantics (adaptive digits,
// "N B" under 1024) are the contract; call sites adapt to it, never the
// reverse. formatCount stays locale-fixed ("en-US") so pooled plates are
// deterministic across machines.

/** Adapter over formatBytes for the tolerant DTO size field: a number,
 *  a numeric string, or a preformatted string that passes through. */
function fmtSize(v: number | string | undefined): string {
  if (v == null) return "";
  if (typeof v === "number") return formatBytes(v);
  const s = v.trim();
  return /^\d+$/.test(s) ? formatBytes(parseInt(s, 10)) : s;
}

/** Numbers/units right-align to the common axis; paths & stamps stay left
 *  (§2.4 "never right-align a path" — stamps contain -/: so they read left). */
function isNumValue(v: string): boolean {
  return /^[+-]?[\d][\d\s,.]*([A-Za-z%°]{0,6})?$/.test(v.trim());
}

/** Keyword/directive line starts (§2.1: keywords/directives `--ink` 600).
 *  Line-prefix only — no parser; a prose line that happens to open with
 *  one of these just reads bolder ink, still greyscale. */
const KEYWORD_LINE_RE =
  /^(?:@|as\b|async\b|await\b|begin\b|case\b|catch\b|class\b|const\b|create\b|def\b|default\b|do\b|elif\b|else\b|end\b|enum\b|except\b|export\b|extends\b|final\b|finally\b|fn\b|for\b|from\b|func\b|function\b|impl\b|import\b|interface\b|let\b|match\b|mod\b|module\b|namespace\b|new\b|package\b|private\b|protected\b|pub\b|public\b|raise\b|return\b|select\b|static\b|struct\b|switch\b|throw\b|trait\b|try\b|type\b|use\b|val\b|var\b|virtual\b|while\b|with\b|yield\b)/i;

/** §2.1 greyscale syntax — line-prefix only, no parser, no colours:
 *  comments & numeric literals → --ink-2, keywords/directives → --ink 600,
 *  blanks → --ink-3 (no glyphs), everything else the base voice. */
function lineClass(line: string): string {
  const t = line.trimStart();
  if (!t) return "is-blank"; // empty glyphs only → --ink-3 never carries text
  if (
    t.startsWith("//") ||
    t.startsWith("#") ||
    t.startsWith("--") ||
    t.startsWith("/*") ||
    t.startsWith("*")
  )
    return "is-comment";
  if (KEYWORD_LINE_RE.test(t)) return "is-key";
  if (/^[+-]?[.\d]/.test(t)) return "is-literal"; // a numeric literal leads the line
  return "";
}

/** Inherently-upper data (hex offsets/ranges) prints uppercase; dump bytes
 *  follow the §2.1 example verbatim (`4d 5a`) → lowercase. No 32-bit wrap —
 *  offsets are u64 (files > 4 GB are normal here). */
function hex8(n: number): string {
  return Math.max(0, Math.floor(n)).toString(16).padStart(8, "0").toUpperCase();
}

function byteHex(b: number): string {
  return Math.max(0, Math.min(255, b | 0)).toString(16).padStart(2, "0");
}

function asciiOf(bytes: number[]): string {
  let s = "";
  for (const b of bytes) s += b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : "·";
  return s;
}

function parseOff(off: number | string | undefined): number {
  if (typeof off === "number") return num(off);
  if (typeof off !== "string") return 0;
  const s = off.trim().replace(/^0x/i, "");
  if (!s) return 0;
  const v = /[a-fA-F]/.test(s) ? parseInt(s, 16) : parseInt(s, 10);
  return isFinite(v) && v > 0 ? v : 0;
}

/** Tolerates `number[]`, `"4d 5a …"` and a single packed hex run. Unparseable
 *  input → [] which yields NO signature annotation (never guess bytes). */
function toBytes(raw: number[] | string | undefined): number[] {
  if (Array.isArray(raw)) {
    const out: number[] = [];
    for (const b of raw) if (typeof b === "number" && isFinite(b)) out.push(b & 0xff);
    return out;
  }
  if (typeof raw !== "string") return [];
  const out: number[] = [];
  for (let tok of raw.trim().split(/\s+/)) {
    if (!tok) continue;
    tok = tok.replace(/^0x/i, "");
    if (!/^[0-9a-fA-F]+$/.test(tok) || tok.length % 2 === 1) return [];
    for (let i = 0; i < tok.length; i += 2) out.push(parseInt(tok.slice(i, i + 2), 16));
  }
  return out;
}

// §2.1 signature prefixes — matched ONLY against real first-row bytes.
const SIGNATURES: Array<{ sig: number[]; label: string }> = [
  { sig: [0x4d, 0x5a], label: "MZ/PE" },
  { sig: [0x7f, 0x45, 0x4c, 0x46], label: "ELF" },
  { sig: [0x50, 0x4b, 0x03, 0x04], label: "ZIP" },
  { sig: [0x89, 0x50, 0x4e, 0x47], label: "PNG" },
  { sig: [0x47, 0x49, 0x46, 0x38], label: "GIF" },
  { sig: [0x52, 0x49, 0x46, 0x46], label: "RIFF" },
  { sig: [0x66, 0x4c, 0x61, 0x44], label: "FLAC" },
  { sig: [0x4f, 0x67, 0x67, 0x53], label: "OGG" },
];

function matchSignature(bytes: number[]): { bytes: number[]; label: string } | null {
  for (const s of SIGNATURES) {
    if (bytes.length < s.sig.length) continue;
    let hit = true;
    for (let i = 0; i < s.sig.length; i++)
      if (bytes[i] !== s.sig[i]) {
        hit = false;
        break;
      }
    if (hit) return { bytes: s.sig, label: s.label }; // callers read .bytes — never hand them the raw table row
  }
  return null;
}

function extOf(fileName: string, mime?: string): string {
  if (typeof mime === "string" && mime.includes("/")) {
    const sub = mime.slice(mime.indexOf("/") + 1).replace(/\+.*$/, "");
    if (sub) return sub.toUpperCase();
  }
  const m = /\.([A-Za-z0-9]+)$/.exec(fileName || "");
  return m ? m[1].toUpperCase() : "";
}

/** Frame geometry (§2.0): inset 8, plate border 1, head + its 1px rule. */
function bodyBox(tile: { w: number; h: number }): Box {
  const head = tile.w >= 320 ? 28 : 22;
  return { w: Math.max(0, tile.w - 9), h: Math.max(0, tile.h - 10 - head) };
}

function buildShell(root: HTMLElement): Shell {
  const head = h("div", "tm-prev__head");
  const name = h("span", "tm-prev__name");
  const meta = h("span", "tm-prev__meta");
  head.append(name, meta);
  const body = h("div", "tm-prev__body");
  root.replaceChildren(head, body); // pooled node: full rewrite, never append
  return { name, meta, body };
}

/** Shared truncation footer (16px): count in `.num`, honest lower bound `+`
 *  when the payload says more exist beyond what it handed us. */
/** "1 entries" is wrong copy: for exactly one, the trailing noun
 *  singularizes (entries→entry, tables→table, lines→line). */
function noun1(noun: string, n: number): string {
  if (n !== 1) return noun;
  return noun.endsWith("ies") ? noun.slice(0, -3) + "y" : noun.endsWith("s") ? noun.slice(0, -1) : noun;
}

function footer(moreCls: string, noun: string, hidden: number, payloadMore: boolean): HTMLElement {
  const f = h("div", moreCls);
  f.append(document.createTextNode("… "));
  if (hidden > 0) {
    f.append(h("span", "num", formatCount(hidden) + (payloadMore ? "+" : "")));
    f.append(document.createTextNode(` more ${noun1(noun, hidden)}`));
  } else {
    f.append(document.createTextNode(`more ${noun}`)); // exact count unknowable
  }
  return f;
}

// ------------------------------------------------------------- families ---

/** TEXT (§2.1): numbered grid, greyscale syntax, reserved 16px footer band. */
function renderText(body: HTMLElement, tx: PreviewText, box: Box): string {
  body.className = "tm-prev__body tm-text";
  // Landed Rust sends `content` (one newline-joined string); keep `lines[]`
  // tolerated so either payload shape renders identically.
  const lines = Array.isArray(tx.lines)
    ? tx.lines
    : typeof tx.content === "string"
      ? tx.content.split(/\r?\n/)
      : [];
  const total =
    typeof tx.totalLines === "number" && tx.totalLines >= lines.length ? tx.totalLines : null;
  const payloadMore = tx.truncated === true && total === null; // held head only
  const visible = Math.max(0, Math.floor((box.h - 16) / 16)); // §2.0 formula
  const rendered = Math.min(lines.length, visible);
  const hidden = Math.max(0, (total ?? lines.length) - rendered);

  const grid = h("div", "tm-text__grid");
  // gutter width fixed once per fill: max(3ch, digits of last visible line) + 14px
  grid.style.setProperty("--ln-d", String(Math.max(1, rendered).toString().length));
  for (let i = 0; i < rendered; i++) {
    const cls = lineClass(lines[i] ?? "");
    grid.append(
      h("div", "tm-text__ln", String(i + 1)), // every line numbered, no skips
      h("div", "tm-text__code" + (cls ? " " + cls : ""), lines[i] ?? "")
    );
  }
  body.append(grid);
  if (hidden > 0 || payloadMore) {
    body.append(footer("tm-text__more", "lines", hidden, payloadMore));
  }
  // BOM-detected encoding is inherently-upper data (UTF-8 / UTF-16LE) — legal
  // in the mono meta voice; every field shown is one we truly have.
  const enc =
    typeof tx.encoding === "string" && tx.encoding ? tx.encoding.toUpperCase() + " · " : "";
  return `${enc}${formatCount(total ?? lines.length)} L`;
}

/** HEX (§2.1): sunken bed, re-chunked 8/16 bytes per row, single hairline
 *  between hex and ascii, real signature bytes only. */
function renderHex(
  body: HTMLElement,
  hx: PreviewHex,
  box: Box,
  tileW: number,
  root: HTMLElement
): string {
  body.className = "tm-prev__body tm-hex";
  const src = Array.isArray(hx.rows) ? hx.rows : [];
  const per = tileW < 420 ? 8 : 16; // §2.1 density rule
  type Run = { off: number; bytes: number[] };
  const runs: Run[] = [];
  for (const r of src) {
    const off = parseOff(r.off);
    const bytes = toBytes(r.bytes);
    for (let i = 0; i < bytes.length; i += per) {
      runs.push({ off: off + i, bytes: bytes.slice(i, i + per) });
    }
  }
  const visible = Math.max(0, Math.floor((box.h - 16) / 16)); // bed padding 8 + 8
  const shown = runs.slice(0, visible);
  const sig = shown.length ? matchSignature(shown[0].bytes) : null;
  let note: HTMLElement | null = null;
  let row0: HTMLElement | null = null;

  for (let r = 0; r < shown.length; r++) {
    const run = shown[r];
    const row = h("div", "tm-hex__row");
    if (r === 0) row0 = row;
    row.append(h("span", "tm-hex__off", hex8(run.off)));
    const wrap = h("span", "tm-hex__bytes");
    for (let j = 0; j < run.bytes.length; j++) {
      const marked = sig && r === 0 && j < sig.bytes.length;
      wrap.append(h("span", marked ? "tm-hex__b is-sig" : "tm-hex__b", byteHex(run.bytes[j])));
    }
    row.append(wrap, h("span", "tm-hex__ascii", asciiOf(run.bytes)));
    if (sig && r === 0) {
      note = h("span", "tm-hex__note", sig.label);
      row.append(note);
    }
    body.append(row);
  }
  if (!shown.length) return "HEX";
  // The note is the signature's text key (Two-Channel): when the dump row
  // outruns the plate it would clip mid-glyph — cut it whole instead. The
  // sig bytes themselves keep their accent-soft mark either way.
  if (note && row0) {
    const clipped = measureRoot(root, () => row0!.scrollWidth > row0!.clientWidth);
    if (clipped) note.remove();
  }
  // range describes what is ON SCREEN — honest coverage, no footer needed
  const last = shown[shown.length - 1];
  return `HEX · 0x${hex8(shown[0].off)}–0x${hex8(last.off + Math.max(0, last.bytes.length - 1))}`;
}

/** In-family unavailable state (§2.2 failed decode; also desktop-only
 *  media): keyed hatch over the sunken bed + centred sentence-case words
 *  + the real reason — the head strip stays, so the file stays labelled.
 *  Full body rewrite (pooled node), never an append. */
function renderFailed(body: HTMLElement, reason: string): void {
  body.className = "tm-prev__body tm-failed tex-hatch";
  body.replaceChildren();
  const stack = h("div", "tm-stack");
  stack.append(h("span", "micro", "preview unavailable"));
  if (reason) stack.append(h("span", "num tm-stack__note", reason));
  body.append(stack);
}

/** IMAGE (§2.2): contained (never cropped) inside a hairline frame hugging
 *  the img, crop marks + dim tab only at tile-w ≥ 160, failed decode → the
 *  in-family hatch state (head strip retained). */
function renderImage(
  body: HTMLElement,
  im: PreviewImage,
  box: Box,
  tileW: number,
  fileName: string,
  root: HTMLElement
): string {
  body.className = "tm-prev__body tm-img";
  const src = im.dataUrl; // narrow once — property narrowing could lapse below
  if (!src) {
    renderFailed(body, "no image data");
    return extOf(fileName, im.mime);
  }
  const big = tileW >= 160;
  const frame = h("div", big ? "tm-img__frame crop" : "tm-img__frame");
  const img = document.createElement("img");
  img.className = "tm-img__el";
  img.alt = "";
  frame.append(img);
  const dim = big ? h("span", "tm-img__dim") : null;
  if (dim) frame.append(dim);
  body.append(frame);

  const apply = (iw: number, ih: number) => {
    if (!(iw > 0) || !(ih > 0)) return;
    const areaW = Math.max(1, box.w - 18); // bed pad 8×2 + frame border 1×2
    const areaH = Math.max(1, box.h - 18);
    // contain, capped at 1×: thumbnails arrive pre-sized — upscaling only
    // pixelates and would make the dim tab lie about displayed pixels.
    const s = Math.min(areaW / iw, areaH / ih, 1);
    img.style.width = `${Math.max(1, Math.floor(iw * s))}px`;
    img.style.height = `${Math.max(1, Math.floor(ih * s))}px`;
    if (dim) dim.textContent = `${iw} × ${ih}`;
  };
  const pw = num(im.w);
  const ph = num(im.h);
  if (pw > 0 && ph > 0) apply(pw, ph);
  // Pooled plates get refilled with other payloads; a late event from a
  // detached img must never stomp the plate's current content.
  img.addEventListener("load", () => {
    if (!root.contains(img)) return;
    if (img.naturalWidth > 0 && img.naturalHeight > 0) {
      apply(img.naturalWidth, img.naturalHeight); // real pixels win over DTO dims
    }
  });
  img.addEventListener("error", () => {
    if (!root.contains(img)) return;
    renderFailed(body, "decode error");
  });
  img.src = src;
  return extOf(fileName, im.mime); // header meta: one real field, or none
}

function str(v: unknown): string {
  return v == null ? "" : String(v);
}

function colCount(cols: unknown): number | null {
  if (typeof cols === "number" && isFinite(cols)) return cols;
  if (Array.isArray(cols)) return cols.length;
  return null;
}

/** §2.3 column presets built from what the payload actually carries:
 *  archive-with-methods `1fr 5ch 7ch`, db `1fr 7ch 7ch`, else `1fr 7ch`.
 *  A method/column that no entry has is dropped instead of shipping an
 *  always-empty track (honest "if-present"). The 7ch value track is the
 *  width of formatBytes' widest output ("99.9 KB"), so a size cell is
 *  always fully visible — the §2.3 preset's 6ch predates that formatter. */
function listCols(list: PreviewList): Cols {
  const entries = Array.isArray(list.entries) ? list.entries : [];
  const kind = typeof list.kind === "string" ? list.kind : "";
  const isArchive = kind === "archive";
  const labels = [isArchive ? "name" : kind === "db" ? "table" : "name"];
  const widths = ["1fr"];
  const numeric = [false];
  const hasMethod = entries.some((e) => str(e.method).trim() !== "");
  if (isArchive && hasMethod) {
    labels.push("method");
    widths.push("5ch");
    numeric.push(false);
  }
  const hasSize = entries.some((e) => e.size != null && str(e.size).trim() !== "");
  if (hasSize) {
    labels.push("size");
    widths.push("7ch");
    numeric.push(true);
  }
  const rows = entries.map((e) => {
    const cells = [str(e.name)];
    if (isArchive && hasMethod) cells.push(str(e.method));
    if (hasSize) cells.push(fmtSize(e.size));
    return cells;
  });
  return { labels, widths, numeric, rows };
}

function dbCols(db: PreviewDb): Cols {
  const tables = Array.isArray(db.tables) ? db.tables : [];
  const labels = ["table"];
  const widths = ["1fr"];
  const numeric = [false];
  const hasCols = tables.some((t) => colCount(t.cols) != null);
  const hasRows = tables.some((t) => typeof t.rows === "number");
  if (hasCols) {
    labels.push("columns");
    widths.push("7ch");
    numeric.push(true);
  }
  if (hasRows) {
    labels.push("rows");
    widths.push("7ch");
    numeric.push(true);
  }
  const rows = tables.map((t) => {
    const cells = [str(t.name)];
    if (hasCols) {
      const c = colCount(t.cols);
      cells.push(c == null ? "" : formatCount(c));
    }
    if (hasRows) cells.push(typeof t.rows === "number" ? formatCount(t.rows) : "");
    return cells;
  });
  return { labels, widths, numeric, rows };
}

/** LIST/DB (§2.3): 22px rows from actual body pixels, key strip only when
 *  ≥2 columns AND body ≥160, clipped never scrolled, honest footer.
 *  The name column NEVER vanishes: when the body is too narrow to carry a
 *  legible name track beside the fixed size track, the size track is
 *  dropped instead (§2.3 if-present honesty — a dropped figure, never a
 *  dropped/empty name column). Keys and rows share the one spec. */
/** Row cap for LIST/DB table plates: see renderTable — 20 rows is the
 *  instrument/glimpse boundary; the footer carries the remainder. */
const TABLE_ROW_CAP = 20;

function renderTable(body: HTMLElement, t: Cols, box: Box, noun: string, more: boolean): string {
  body.className = "tm-prev__body tm-list";
  const narrow = box.w < 132;
  const labels = narrow ? t.labels.slice(0, 1) : t.labels;
  const widths = narrow ? ["1fr"] : t.widths;
  const numeric = narrow ? [false] : t.numeric;
  const rows = narrow ? t.rows.map((r) => r.slice(0, 1)) : t.rows;
  body.style.setProperty("--cols", widths.join(" "));
  const strip = labels.length >= 2 && box.h >= 160 ? 16 : 0;
  const full = Math.floor((box.h - strip) / 22);
  const needsFooter = rows.length > full;
  const usable = box.h - strip - (needsFooter ? 16 : 0);
  // Row cap: a preview plate is a GLIMPSE, not a scrolled ledger. Whatever
  // the tile's height could host, more than 20 rendered rows in one panel
  // stop previewing and start replacing (the map must stay the map); the
  // honest footer carries the remainder instead.
  const shown = Math.max(0, Math.min(rows.length, TABLE_ROW_CAP, Math.floor(usable / 22)));
  const hidden = rows.length - shown;

  if (strip > 0) {
    const keys = h("div", "tm-list__keys");
    labels.forEach((lb, i) => {
      keys.append(h("span", numeric[i] ? "tm-list__key is-num" : "tm-list__key", lb));
    });
    body.append(keys);
  }
  const rowsEl = h("div", "tm-list__rows");
  for (let i = 0; i < shown; i++) {
    const row = h("div", "tm-list__row");
    rows[i].forEach((cell, c) => {
      row.append(h("span", c === 0 ? "tm-list__name" : "tm-list__val", cell));
    });
    rowsEl.append(row);
  }
  body.append(rowsEl);
  if (needsFooter || more) body.append(footer("tm-list__more", noun, hidden, more));
  return `${formatCount(rows.length)} ${noun1(noun, rows.length)}`;
}

// meta value rendering -------------------------------------------------------

const FLAG_RE = /^(true|false|read-?only|compressed)$/i;
const WARN_KEY_RE = /\bwarn/i;
const WARN_VAL_RE = /^\s*(warning|failed|missing)\b/i;
const EMPTY_VALUES = ["", "?", "-", "–", "—"];
/** hero: key /^size$/i AND value looks like a size (number + unit) */
const SIZE_LOOKS_RE = /^[+-]?[\d][\d\s,.]*\s*[A-Za-z]{1,6}$/;
const SIZE_SPLIT_RE = /^([\d][\d\s,.]*)\s*([A-Za-z%°]{1,6})$/;
/** §2.4 rail datum candidates — only keys the payload truly spells. */
const RAIL_TIME_KEY_RE = /^(mtime|modified|last modified|modified at|date|created|created at|compiled)$/i;
const RAIL_FMT_KEY_RE = /^(format|file format|type|mime|mime type|kind|arch|architecture)$/i;
const RAIL_SIG_KEY_RE = /^signature$/i;

/** ONE real datum for the §2.4 right-edge rail, chosen from the pairs the
 *  payload actually carries: a time stamp first, else the format, else the
 *  signature bytes (inherently-upper hex). Keys print exactly as the payload
 *  spells them — the sentence-case law bans invented uppercase labels, so no
 *  `MTIME` costume unless the data itself says so. No match → no rail. */
function railDatum(pairs: PreviewPair[]): string {
  let fmt = "";
  let sig = "";
  for (const pr of pairs) {
    const k = str(pr.k).trim();
    const v = str(pr.v).trim();
    if (!v) continue;
    if (RAIL_TIME_KEY_RE.test(k)) return `${k} ${v}`;
    if (!fmt && RAIL_FMT_KEY_RE.test(k)) fmt = `${k} ${v}`;
    if (!sig && RAIL_SIG_KEY_RE.test(k)) sig = v;
  }
  return fmt || sig;
}

function kvRow(pair: PreviewPair): HTMLElement {
  const kv = h("div", "tm-meta__kv");
  const key = str(pair.k);
  const raw = str(pair.v).trim();
  kv.append(h("span", "tm-meta__k", key));
  const v = h("span", "tm-meta__v");
  if (isNumValue(raw)) v.classList.add("is-num"); // numbers right, strings left
  if (WARN_KEY_RE.test(key) || WARN_VAL_RE.test(raw)) v.classList.add("tm-warn");
  if (EMPTY_VALUES.includes(raw.toLowerCase())) {
    // §2.4 unknown: keyed swatch + word, never a bare em-dash
    v.append(h("span", "tex-hatch tm-meta__swatch"), document.createTextNode("unknown"));
  } else if (FLAG_RE.test(raw)) {
    v.append(h("span", "kbd tm-meta__flag", raw)); // chip geometry from .kbd
  } else {
    v.textContent = raw;
  }
  kv.append(v);
  return kv;
}

/** META (§2.4): padded readout, one hero `.stat` capped at the 21px scale
 *  unless tile-w ≥ 360, kv rows carved by hairlines, optional right-edge
 *  rail carrying ONE live datum. */
function renderMeta(
  body: HTMLElement,
  mp: PreviewMeta,
  box: Box,
  tileW: number,
  tileH: number,
  root: HTMLElement
): string {
  body.className = "tm-prev__body tm-meta";
  const pairs = Array.isArray(mp.pairs) ? mp.pairs : [];
  const heroIdx = pairs.findIndex(
    (pr) => /^size$/i.test(str(pr.k)) && SIZE_LOOKS_RE.test(str(pr.v))
  );
  const hero = heroIdx >= 0 ? pairs[heroIdx] : null;
  const rest = heroIdx >= 0 ? pairs.filter((_, i) => i !== heroIdx) : pairs;

  // §2.4 as amended: the 30px hero tier opens at tile-w ≥ 360 — the
  // oversized "186.1 MB" figure on mid-size tiles was the defect. Below
  // the tier the hero reads at the plain 21px `.stat` scale.
  const heroBig = tileW >= 360;
  const heroH = hero ? (heroBig ? 51 : 41) : 0; // micro 15 + gap 3 + stat-v 23/33
  const sep = hero && rest.length > 0 ? 12 : 0; // flex gap 3 + hairline 1 + pad 8
  const avail = box.h - 16 - heroH - sep; // plate padding 8×2
  const full = Math.floor(avail / 18);
  const needsFooter = rest.length > full;
  const rows = Math.max(0, Math.min(rest.length, Math.floor((avail - (needsFooter ? 16 : 0)) / 18)));

  if (hero) {
    const sect = h("div", "tm-meta__sect");
    const stat = h("div", "stat");
    if (heroBig) stat.classList.add("is-hero");
    const split = SIZE_SPLIT_RE.exec(str(hero.v).trim());
    stat.append(h("span", "micro", str(hero.k)));
    const fig = h("span", "stat-v");
    if (split) {
      fig.append(document.createTextNode(split[1].trim()), h("span", "u", split[2]));
    } else {
      fig.textContent = str(hero.v);
    }
    stat.append(fig);
    sect.append(stat);
    body.append(sect);
  }
  if (rows > 0) {
    const sect = h("div", "tm-meta__sect");
    for (let i = 0; i < rows; i++) sect.append(kvRow(rest[i]));
    body.append(sect);
  }
  if (needsFooter) body.append(footer("tm-meta__more", "fields", rest.length - rows, false));

  // Right-edge rail (§2.4): both tile dimensions must qualify, the datum
  // must be real, and it must fit the rail's full length EXACTLY — measure
  // once at fill time and cut the rail whole otherwise (never clipped
  // mid-glyph; a clipped annotation is a lie).
  if (tileW >= 360 && tileH >= 240) {
    const datum = railDatum(pairs);
    if (datum) {
      const rail = h("div", "tm-meta__rail");
      const txt = h("span", "tm-meta__rail-text", datum);
      rail.append(txt);
      body.append(rail);
      body.classList.add("tm-meta--rail"); // clear the rail band
      const fits = measureRoot(root, () => txt.offsetHeight <= rail.clientHeight - 16);
      if (!fits) {
        rail.remove();
        body.classList.remove("tm-meta--rail");
      }
    }
  }
  return ""; // META header meta: no field we truly have → omit (§2.0 rule)
}

// --------------------------------------------------------------- exports ---

/** Pooled node: `.tm-prev` root (head + body), hidden until the draw loop
 *  reveals it via the `hidden` attribute or inline display. */
export function createPlate(): HTMLElement {
  const root = h("div", "tm-prev");
  root.hidden = true;
  root.dataset.head = "sm";
  buildShell(root);
  return root;
}

export function fillPlate(
  el: HTMLElement,
  p: PreviewPayload,
  opts: { fileName: string; theme: "light" | "dark"; tile: { w: number; h: number } }
): void {
  const t = opts.tile || { w: 0, h: 0 };
  const tile = { w: Math.max(0, num(t.w)), h: Math.max(0, num(t.h)) };
  const payload: PreviewPayload = p || {};

  // pooled node: reset modifiers + full content rebuild every call (cheap,
  // and payload identity carries no entryId — a skip-guard could serve stale
  // bytes when the pool recycles a same-named file)
  el.className = "tm-prev";
  el.dataset.theme = opts.theme === "dark" ? "dark" : "light";
  el.dataset.head = tile.w >= 320 ? "lg" : "sm"; // §2.0: 28px head at ≥320
  const shell = buildShell(el);
  const box = bodyBox(tile);
  const fam = typeof payload.family === "string" ? payload.family.toUpperCase() : "";

  const plan =
    (fam === "TEXT" || fam === "SVG") && payload.text
      ? "text" // §integrator-note-2: SVG previews as TEXT this wave
      : fam === "HEX" && payload.hex
        ? "hex"
        : fam === "IMAGE" && payload.image
          ? "image"
          : fam === "LIST" && payload.list
            ? "list"
            : fam === "DB" && (payload.db || payload.list)
              ? payload.db
                ? "db"
                : "list"
              : fam === "META" && payload.meta
                ? "meta"
                : fam === "MEDIA" && payload.media
                  ? "media"
                  : // content-driven rescue when the family tag is missing/unknown
                  payload.text
                  ? "text"
                  : payload.hex
                    ? "hex"
                    : payload.image
                      ? "image"
                      : payload.db
                        ? "db"
                        : payload.list
                          ? "list"
                          : payload.meta
                            ? "meta"
                            : payload.media
                              ? "media"
                              : "";

  shell.name.textContent = opts.fileName || "";
  if (!plan) {
    fillFallbackPlate(el, "no preview data");
    return;
  }

  let metaText = "";
  switch (plan) {
    case "text":
      metaText = renderText(shell.body, payload.text as PreviewText, box);
      break;
    case "hex":
      metaText = renderHex(shell.body, payload.hex as PreviewHex, box, tile.w, el);
      break;
    case "image":
      metaText = renderImage(
        shell.body,
        payload.image as PreviewImage,
        box,
        tile.w,
        opts.fileName || "",
        el
      );
      break;
    case "list":
      metaText = renderTable(
        shell.body,
        listCols(payload.list as PreviewList),
        box,
        fam === "DB" ? "tables" : "entries",
        (payload.list as PreviewList).truncated === true
      );
      break;
    case "db":
      metaText = renderTable(
        shell.body,
        dbCols(payload.db as PreviewDb),
        box,
        "tables",
        (payload.db as PreviewDb).truncated === true
      );
      break;
    case "meta":
      metaText = renderMeta(shell.body, payload.meta as PreviewMeta, box, tile.w, tile.h, el);
      break;
    case "media":
      metaText = renderMedia(shell.body, payload.media as PreviewMedia, box, opts.fileName || "", el);
      break;
  }
  shell.meta.textContent = metaText;
  el.dataset.family = plan;
}

/** MEDIA — inert instrument plate. The binding law gives plates no pointer
 *  events (map gestures win, integrator note 1), so the native transport
 *  controls are out: video previews its real first frame (framed like
 *  IMAGE), audio gets a centred stat; the one datum either way is the
 *  file's real duration (formatElapsed — DTO value, refined by the
 *  element's own metadata). */
function renderMedia(
  body: HTMLElement,
  m: PreviewMedia,
  _box: { w: number; h: number },
  fileName: string,
  root: HTMLElement
): string {
  body.className = "tm-prev__body tm-media";
  if (!isTauri()) {
    renderFailed(body, "needs the desktop app"); // demo/web build: no asset scope
    return extOf(fileName, m.mime);
  }
  const media: HTMLMediaElement =
    m.kind === "video" ? document.createElement("video") : document.createElement("audio");
  try {
    media.src = convertFileSrc(m.path);
  } catch {
    renderFailed(body, "path unavailable");
    return extOf(fileName, m.mime);
  }
  media.preload = "metadata";
  media.controls = false; // inert plate: no transport, no pointer events

  let stamp: HTMLElement | null = null;
  const fig = h("span", "stat-v");
  fig.hidden = true; // appears only with a real duration — never a dash
  const setMs = (ms: number): void => {
    if (!(ms > 0) || !root.contains(media)) return;
    const t = formatElapsed(ms);
    if (m.kind === "video") {
      if (!stamp) {
        stamp = h("span", "num tm-media__stamp");
        body.append(stamp);
      }
      stamp.textContent = t;
    } else {
      fig.hidden = false;
      fig.textContent = t;
    }
  };

  if (m.kind === "video") {
    media.className = "tm-media__el";
    body.append(media);
  } else {
    // audio: no pixels to preview — a centred `.stat` readout
    const stack = h("div", "tm-stack");
    const stat = h("div", "stat");
    stat.append(h("span", "micro", "audio"), fig);
    stack.append(stat);
    body.append(stack);
    media.className = "tm-media__src"; // metadata loader only — no visuals
    body.append(media);
  }
  // Pooled plates get refilled with other payloads; a late event from a
  // detached element must never stomp the plate's current content.
  media.addEventListener("loadedmetadata", () => {
    if (!root.contains(media)) return;
    const d = media.duration;
    if (typeof d === "number" && isFinite(d) && d > 0) setMs(d * 1000);
  });
  media.addEventListener("error", () => {
    if (!root.contains(media)) return;
    renderFailed(body, "decode error");
  });
  setMs(num(m.durationMs)); // DTO duration lands before metadata arrives
  return extOf(fileName, m.mime); // header meta: the real extension, or none
}

/** Honest unavailable state (§2.0 leaf interior): dots bed over the whole
 *  plate, keyed hatch swatch + `preview unavailable` + the real reason. */
export function fillFallbackPlate(el: HTMLElement, reason: string): void {
  el.className = "tm-prev tm-prev--fallback"; // modifier drops the head strip
  el.dataset.head = "sm";
  el.removeAttribute("data-family");
  const shell = buildShell(el); // clears any stale pooled name/meta text
  shell.body.className = "tm-prev__body tex-dots"; // dots bed over the plate
  const stack = h("div", "tm-stack");
  stack.append(
    h("span", "tex-hatch tm-fallback__swatch"),
    h("span", "micro", "preview unavailable")
  );
  if (reason) stack.append(h("span", "num tm-stack__note", reason));
  shell.body.append(stack);
}
