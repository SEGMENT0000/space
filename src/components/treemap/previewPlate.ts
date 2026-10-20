import "../../styles/preview.css"; // plate styles live here (component-owned; index.css untouched)
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
// The app's single formatter module — no local byte/count/duration
// formatting lives here; call sites adapt to its semantics, never the reverse.
import { formatBytes, formatCount, formatDuration, formatPercent } from "../../lib/format";
// The eight-category palette: the META card's category row paints a keyed
// swatch from here (hue on a swatch only — chrome stays grey, §2.4/R8).
import { CATEGORY_LABELS, categoryStyle } from "../../lib/colors";

// ---------------------------------------------------------------------------
// In-tile preview plates — INS_BRIEF.md §2 (binding): the shared frame is an
// elevated, square, flat plate (--bg-elevated, radius 0, 0.5px hairline-
// strong edge, never a shadow) carrying per-family readout beds over the
// WebGL tiles.
//
// Pure DOM module: plates live in a `map-previews` overlay layer as pooled
// nodes (~24), positioned imperatively by the draw loop. They never enter the
// React render path and never scroll. All content is written via textContent
// (no innerHTML) so file bytes can never inject markup into the map overlay.
//
// Export surface (backwards compatible — the renderer codes against exactly
// this; every new field is optional, no existing signature was broken):
//   createPlate()                    → pooled `.tm-prev` root, hidden by default
//   fillPlate(el, p, opts, meta?)    → rewrite content for one payload, else the
//                                      fallback ladder (see the doc comment
//                                      on fillPlate): family renderer → META
//                                      card from tile metadata → honest dots
//   fillFallbackPlate(el, r, meta?)  → honest unavailable state + real reason;
//                                      with tile metadata it paints the
//                                      step-2 metadata card instead
//
// The renderer owns visibility (hidden attr or inline display), positioning
// (transform) and the --tile-w/--tile-h custom properties the frame math uses.
// ---------------------------------------------------------------------------

/** Structural mirror of the §B PreviewPayload DTO (kept in sync with
 *  `src/lib/types.ts` / the finalized model.rs). Every field is optional
 *  here so an absent sub-object reads as "no payload" for the fallback
 *  ladder; fields Rust never emits are not mirrored. */
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
  /** Real when cheaply parsed, absent (never guessed) otherwise — printed
   *  in the head meta only when present (formatDuration clock stamp). */
  durationMs?: number;
  w?: number;
  h?: number;
  /** Real parsed container facts (format/duration/codec/channels/sample
   *  rate/…) — rendered in the meta kv rhythm below the head. */
  pairs?: PreviewPair[];
}
export interface PreviewText {
  /** Landed model.rs shape: the read head's lines, one array. */
  lines?: string[];
  truncated?: boolean;
  /** Exact total only when the whole file was inside the 64 KB cap. */
  totalLines?: number;
  /** BOM-detected encoding — "utf-8" | "utf-16le" | "utf-16be", shown in
   *  the header meta uppercased (inherently-upper data). */
  encoding?: string;
}
export interface PreviewImage {
  dataUrl?: string;
  /** ORIGINAL source dimensions (the dataUrl is the ≤512px re-encode). */
  w?: number;
  h?: number;
  mime?: string;
}
export interface PreviewHexRow {
  off?: number;
  /** Space-separated lowercase hex, 8 bytes per row from Rust. */
  bytes?: string;
  /** Printable ASCII (0x20..=0x7E) or "·" — one glyph per byte. */
  ascii?: string;
}
export interface PreviewHex {
  rows?: PreviewHexRow[];
  /** Always 0 (no seek offset server-side). */
  base?: number;
  /** Total file size in bytes. */
  total?: number;
}
export interface PreviewListEntry {
  name?: string;
  /** File byte size; omitted for directories (subtree size needs a walk). */
  size?: number;
  /** "dir" | "file". */
  kind?: string;
  /** Archive compression method ("Store" | "Deflate" | "Bzip2" | "Zstd" |
   *  …) on zip file rows only; tar file rows are "Store"; folder/ISO/
   *  sheet and dir rows omit it. */
  method?: string;
}
export interface PreviewList {
  entries?: PreviewListEntry[];
  truncated?: boolean;
  /** Listing source: "folder" | "zip" | "tar" | "iso" | "sheet". */
  kind?: string;
  /** Exact entry count when enumerated cheaply. */
  total?: number;
  /** Folder cards: wrapper directories descended through before the rendered
   *  rows (e.g. ["User Data"] for Chrome). Named in the head meta so the rows
   *  are never mistaken for the folder's direct children. */
  via?: string[];
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
  /** Column count (== columns.len() when the columns were readable). */
  cols?: number;
  /** Column names in declaration order (leading tables only). */
  columns?: string[];
  /** First ≤5 rows as cell display text — display-capped, blobs stubbed,
   *  null rendered honestly by the backend; the plate passes text through. */
  rows?: string[][];
}
export interface PreviewDb {
  tables?: PreviewDbTable[];
  truncated?: boolean;
}

/** Tile metadata — the plate contract's `meta?`. Everything the renderer
 *  knows about a tile WITHOUT a payload, so a card can always be built:
 *  name, type from ext, category, size, share of parent (§2.4 geometry).
 *  Every field optional; a caller may hand over its whole tile-ref object.
 *  The index signature keeps a ref carrying extra fields (depth, ids, …)
 *  assignable as an object literal — rows are only ever built for fields
 *  that are actually present, never for the extras. */
export interface PlateMeta {
  /** file/folder name as the tile shows it */
  name?: string;
  /** bytes */
  size?: number;
  isDir?: boolean;
  /** category index into CATEGORY_LABELS (0..7) */
  cat?: number;
  /** extension without the dot ("png") — type row source; falls back to the
   *  extension parsed from `name` when absent */
  ext?: string;
  /** frame total in bytes — the share-of-parent denominator */
  parentSize?: number;
  [key: string]: unknown;
}

/** The plate box the draw loop qualifies/fills at (raw tile px, or the
 *  canvas-clamped painted box + the 8px inset — same units the CSS calc
 *  subtracts back out). */
export interface PlateTile {
  w: number;
  h: number;
}

/** Options for fillPlate. `meta` (tile metadata) and `parentSize` are new
 *  and optional; fileName/theme/tile keep their exact previous types, so
 *  every existing call site compiles and behaves unchanged. */
export interface FillPlateOptions {
  fileName?: string;
  theme: "light" | "dark";
  tile: PlateTile;
  /** tile metadata for the fallback ladder — see PlateMeta */
  meta?: PlateMeta;
  /** share-of-parent denominator (frame total, bytes), when the caller keeps
   *  it beside the tile rather than inside `meta` */
  parentSize?: number;
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

// Byte/count/duration/percent formatting is imported from lib/format.ts —
// the app's single formatter module: formatBytes for sizes, formatCount for
// counts, formatDuration for media durations (clock stamp), formatPercent
// for the share-of-parent fraction. Its magnitude semantics (adaptive
// digits, "N B" under 1024) are the contract; call sites adapt to it,
// never the reverse.
// formatCount stays locale-fixed ("en-US") so pooled plates are
// deterministic across machines.

/** Adapter over formatBytes for the DTO size field (u64 bytes when the
 *  backend had one; absent → "" so the cell is empty, never a placeholder). */
function fmtSize(v: number | undefined): string {
  return v == null ? "" : formatBytes(v);
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

function parseOff(off: number | undefined): number {
  return typeof off === "number" && isFinite(off) && off > 0 ? Math.floor(off) : 0;
}

/** Rust sends space-separated lowercase hex ("4d 5a 90 00"). Unparseable
 *  input → [] which yields NO signature annotation (never guess bytes). */
function toBytes(raw: string | undefined): number[] {
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

/** Family render gates (§2.0 table, px): below a family's gate it is NOT
 *  rendered — the plate steps down the fallback ladder instead (tile-
 *  metadata META card → honest dots state). Mirrored here rather than
 *  trusted from the qualify step because the painted box is canvas-clamped:
 *  a partially off-canvas tile can pass the qualify gate on its raw size
 *  and land under the family gate at fill time. Rows are computed from the
 *  real painted pixels, never from the raw tile. */
const FAMILY_GATE: Record<string, readonly [number, number]> = {
  text: [160, 118],
  image: [120, 92],
  list: [180, 120],
  db: [180, 120],
  meta: [120, 92],
  hex: [330, 140],
  media: [240, 120],
};

/** §2.4 proportion cap for the META family (payload cards AND the fallback
 *  metadata card): a metadata card is a readout, not a sheet — past
 *  460×340 it reads as an empty box (live shot: pagefile.sys at ~720×600
 *  carrying five short rows). Mirrored by the `[data-family="meta"]`
 *  max-width/max-height rule in preview.css — the element clamps there,
 *  this clamp keeps the row/head-tier math on the same painted box;
 *  BOTH must change together. The plate's top/left anchor (the 4px
 *  inset) and the family gates are untouched: the cap only ever trims a
 *  plate that already qualified. No other family is capped (LIST/DB are
 *  bounded by the canvas; the rest fill their box by nature). */
const META_CAP_W = 460;
const META_CAP_H = 340;

/** Payload presence for the ladder: a sub-object only "exists" when it can
 *  actually produce rows. A tagged family with an empty sub-object is
 *  data-less — it steps down the ladder rather than painting an empty bed
 *  (the ladder's never-an-empty-state rule). */
function usableText(tx: PreviewText | undefined): boolean {
  return !!tx && Array.isArray(tx.lines);
}
function usableHex(hx: PreviewHex | undefined): boolean {
  return !!hx && Array.isArray(hx.rows) && hx.rows.length > 0;
}
function usableList(ls: PreviewList | undefined): boolean {
  return !!ls && Array.isArray(ls.entries) && ls.entries.length > 0;
}
function usableDb(db: PreviewDb | undefined): boolean {
  return !!db && Array.isArray(db.tables) && db.tables.length > 0;
}
function usableMeta(mp: PreviewMeta | undefined): boolean {
  return !!mp && Array.isArray(mp.pairs) && mp.pairs.length > 0;
}

/** Ladder step 1 — pick the family renderer, or "" when no sub-object is
 *  renderable. Family tag first (§A discriminant; svg→text, none→meta via
 *  the content rescue), then the content-driven rescue when the tag is
 *  missing/unknown. Never guesses a family the payload doesn't carry. */
function resolvePlan(payload: PreviewPayload, fam: string): string {
  if ((fam === "TEXT" || fam === "SVG") && usableText(payload.text)) return "text";
  if (fam === "HEX" && usableHex(payload.hex)) return "hex";
  if (fam === "IMAGE" && payload.image) return "image";
  if (fam === "LIST" && usableList(payload.list)) return "list";
  if (fam === "DB" && (usableDb(payload.db) || usableList(payload.list))) {
    return usableDb(payload.db) ? "db" : "list";
  }
  if (fam === "META" && usableMeta(payload.meta)) return "meta";
  if (fam === "MEDIA" && payload.media) return "media";
  // content-driven rescue when the family tag is missing/unknown
  if (usableText(payload.text)) return "text";
  if (usableHex(payload.hex)) return "hex";
  if (payload.image) return "image";
  if (usableDb(payload.db)) return "db";
  if (usableList(payload.list)) return "list";
  if (usableMeta(payload.meta)) return "meta";
  if (payload.media) return "media";
  return "";
}

/** TEXT (§2.1): numbered grid, greyscale syntax, reserved 16px footer band.
 *  `sizeStr` is the tile's real byte size (header meta: `UTF-8 · 4.2 KB ·
 *  128 L` — every field real or omitted). Returns null when the payload
 *  carries no line to number: the ladder steps down instead of painting
 *  an empty bed. */
function renderText(body: HTMLElement, tx: PreviewText, box: Box, sizeStr: string): string | null {
  body.className = "tm-prev__body tm-text";
  // Landed Rust shape: `lines[]` straight from the read head.
  const lines = Array.isArray(tx.lines) ? tx.lines : [];
  const total =
    typeof tx.totalLines === "number" && tx.totalLines >= lines.length ? tx.totalLines : null;
  const payloadMore = tx.truncated === true && total === null; // held head only
  const visible = Math.max(0, Math.floor((box.h - 16) / 16)); // §2.0 formula
  const rendered = Math.min(lines.length, visible);
  const hidden = Math.max(0, (total ?? lines.length) - rendered);
  // Nothing to number and nothing withheld → no bed to paint (ladder step 2)
  if (rendered === 0 && hidden === 0 && !payloadMore) return null;

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
  const size = sizeStr ? sizeStr + " · " : "";
  return `${enc}${size}${formatCount(total ?? lines.length)} L`;
}

/** HEX (§2.1): sunken bed, re-chunked 8/16 bytes per row, single hairline
 *  between hex and ascii, real signature bytes only. Returns null when no
 *  byte row survives parsing — the ladder steps down rather than painting
 *  an empty dump (unparseable input never becomes an annotation either). */
function renderHex(
  body: HTMLElement,
  hx: PreviewHex,
  box: Box,
  tileW: number,
  root: HTMLElement
): string | null {
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
  if (!shown.length) return null; // no real bytes → no dump (ladder step 2)
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
  root: HTMLElement,
  sizeStr: string
): string {
  // head meta: extension + real byte size, every field or none (§2.2
  // example `PNG · 842 KB · sRGB` — no sRGB field exists in the payload,
  // so it is omitted rather than assumed)
  const head = (): string => {
    const ex = extOf(fileName, im.mime);
    return ex && sizeStr ? `${ex} · ${sizeStr}` : ex || sizeStr;
  };
  body.className = "tm-prev__body tm-img";
  const src = im.dataUrl; // narrow once — property narrowing could lapse below
  if (!src) {
    renderFailed(body, "no image data");
    return head();
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
  return head(); // header meta: real fields only
}

function str(v: unknown): string {
  return v == null ? "" : String(v);
}

/** §2.3 column presets built from what the payload actually carries:
 *  archive-with-methods `1fr 7ch 7ch`, else `1fr 7ch`. A method that no
 *  entry has is dropped instead of shipping an always-empty track (honest
 *  "if-present"); the landed backend fills `method` on zip file rows, so
 *  the track lights up exactly when real methods exist. The 7ch value
 *  tracks are the width of formatBytes' widest output ("99.9 KB") and the
 *  widest method word ("Deflate"), so every cell is always fully visible —
 *  the §2.3 preset's 6ch predates that formatter. */
function listCols(list: PreviewList): Cols {
  const entries = Array.isArray(list.entries) ? list.entries : [];
  const kind = typeof list.kind === "string" ? list.kind : "";
  const labels = [kind === "db" ? "table" : "name"];
  const widths = ["1fr"];
  const numeric = [false];
  const hasMethod = entries.some((e) => str(e.method).trim() !== "");
  if (hasMethod) {
    labels.push("method");
    widths.push("7ch");
    numeric.push(false);
  }
  const hasSize = entries.some((e) => e.size != null && str(e.size).trim() !== "");
  if (hasSize) {
    labels.push("size");
    widths.push("7ch");
    numeric.push(true);
  }
  const rows = entries.map((e) => {
    const name = str(e.name);
    // A trailing slash is the cheapest real kind channel: folder rows and
    // archive directory rows read as containers without a second column.
    const cells = [str(e.kind) === "dir" ? `${name}/` : name];
    if (hasMethod) cells.push(str(e.method));
    if (hasSize) cells.push(fmtSize(e.size));
    return cells;
  });
  return { labels, widths, numeric, rows };
}

/** DB (§2.3): schema list on top (table · columns), then — when the payload
 *  carries real rows — a dense grid per contributing table: declaration-
 *  order column headers + the delivered rows as display-text cells. Every
 *  section is computed from the plate's real pixel height; the grid only
 *  draws when header + ≥1 row fit beside the schema, cuts are honest (one
 *  `…` footer carries withheld rows/tables and payload truncation). Cells
 *  never wrap, never scroll; a 24-column cap keeps a wide table a glimpse.
 *  The old rows-count column is gone with the DTO: row counts are not a
 *  schema fact, so the schema section shows table · columns only. */
const DB_GRID_COL_CAP = 24;
const DB_SCHEMA_CAP = 5; // schema rows when a grid shares the body

function renderDb(body: HTMLElement, db: PreviewDb, box: Box): string {
  body.className = "tm-prev__body tm-list tm-db";
  const tables = Array.isArray(db.tables) ? db.tables : [];
  const truncated = db.truncated === true;
  // contributing tables: those the backend actually queried rows for
  const groups = tables.filter((t) => Array.isArray(t.rows) && (t.rows as string[][]).length > 0);
  let delivered = 0;
  for (const t of groups) delivered += (t.rows as string[][]).length;

  // ---- pixel budget. One 16px footer band is always reserved: any cut or
  // a payload-declared truncation needs it, and never reflowing after fill
  // is worth the spare band on a fully-fitting plate.
  const hasGrid = groups.length > 0;
  const multi = groups.length > 1; // per-table name bands
  const gridFixed = groups.length * (16 + (multi ? 16 : 0)); // header (+ name) per group
  const gridMin = 16 + 18 + (multi ? 16 : 0); // header + one row (+ name)
  let schemaRows = Math.min(tables.length, hasGrid ? DB_SCHEMA_CAP : TABLE_ROW_CAP);
  if (hasGrid) {
    const maxSchemaPx = box.h - gridMin - 16;
    schemaRows = Math.min(schemaRows, Math.max(0, Math.floor((maxSchemaPx - 16) / 22)));
  } else {
    schemaRows = Math.min(schemaRows, Math.max(0, Math.floor((box.h - 32) / 22)));
  }
  const schemaPx = schemaRows > 0 ? 16 + schemaRows * 22 : 0;
  const gridSpace = hasGrid ? Math.max(0, box.h - schemaPx - 16) : 0;

  // ---- schema section (top)
  if (schemaRows > 0) {
    const sect = h("div", "tm-db__schema");
    sect.style.setProperty("--cols", "1fr 7ch");
    const keys = h("div", "tm-list__keys");
    keys.append(h("span", "tm-list__key", "table"), h("span", "tm-list__key is-num", "columns"));
    sect.append(keys);
    for (let i = 0; i < schemaRows; i++) {
      const t = tables[i];
      const row = h("div", "tm-list__row");
      row.append(
        h("span", "tm-list__name", str(t.name)),
        h("span", "tm-list__val", typeof t.cols === "number" ? formatCount(t.cols) : "")
      );
      sect.append(row);
    }
    body.append(sect);
  }

  // ---- rows grid (bottom): groups in payload order until the pixels run
  // out; a group that cannot fit its header + one row is dropped whole.
  let rowsShown = 0;
  let budget = Math.max(0, gridSpace - gridFixed);
  if (hasGrid && budget >= 18) {
    const grid = h("div", "tm-db__grid");
    for (const t of groups) {
      const fixed = 16 + (multi ? 16 : 0); // header (+ name band)
      if (budget < fixed + 18) break; // this and every later group is cut
      const colsAll = Array.isArray(t.columns) ? t.columns.slice(0, DB_GRID_COL_CAP) : [];
      const rawRows = Array.isArray(t.rows) ? (t.rows as string[][]) : [];
      const rowsAll = rawRows.map((r) => (Array.isArray(r) ? r.slice(0, DB_GRID_COL_CAP) : []));
      // column count: header truth, else the widest delivered row — never
      // invented; a rows payload with neither headers nor cells is not a grid
      let nCols = colsAll.length;
      for (const r of rowsAll) if (r.length > nCols) nCols = r.length;
      if (nCols === 0) break;
      const grp = h("div", "tm-db__group");
      grp.style.setProperty("--grid-cols", `repeat(${nCols}, minmax(0, 1fr))`);
      if (multi) grp.append(h("div", "tm-db__tname", str(t.name)));
      if (colsAll.length > 0) {
        const head = h("div", "tm-db__head");
        for (const c of colsAll) head.append(h("span", "tm-db__cell is-key", c));
        grp.append(head);
        budget -= 16;
      }
      const fitRows = Math.min(rowsAll.length, Math.floor(budget / 18));
      for (let i = 0; i < fitRows; i++) {
        const row = h("div", "tm-db__row");
        const cells = rowsAll[i];
        for (let c = 0; c < nCols; c++) row.append(h("span", "tm-db__cell", cells[c] ?? ""));
        grp.append(row);
      }
      budget -= fitRows * 18;
      rowsShown += fitRows;
      grid.append(grp);
      if (fitRows < rowsAll.length) break; // pixels gone: remaining groups fully cut
    }
    if (grid.childNodes.length > 0) body.append(grid);
  }

  // ---- honest footer: what THIS plate withheld + the payload's own lower bound
  const schemaCut = tables.length - schemaRows;
  const rowsCut = delivered - rowsShown;
  if (schemaCut > 0 || rowsCut > 0 || truncated) {
    const f = h("div", "tm-list__more");
    f.append(document.createTextNode("… "));
    if (schemaCut > 0) {
      f.append(h("span", "num", formatCount(schemaCut) + (truncated ? "+" : "")));
      f.append(document.createTextNode(` more ${noun1("tables", schemaCut)}`));
      if (rowsCut > 0) f.append(document.createTextNode(" · "));
    }
    if (rowsCut > 0) {
      f.append(h("span", "num", formatCount(rowsCut) + (truncated ? "+" : "")));
      f.append(document.createTextNode(` more ${noun1("rows", rowsCut)}`));
    }
    if (schemaCut === 0 && rowsCut === 0) {
      f.append(document.createTextNode("more tables")); // truncated, counts unknowable
    }
    body.append(f);
  }
  // head meta: delivered facts only — the table count and how many real
  // rows the payload handed us (some may be cut from display by pixels).
  return `${formatCount(tables.length)} ${noun1("tables", tables.length)}` +
    (delivered > 0 ? ` · ${formatCount(delivered)} rows` : "");
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

/** §2.4: names/paths NEVER right-align, even when the name happens to be
 *  all digits — "2024" is still a name, not a figure. */
const LEFT_KEY_RE = /^(?:name|file name|filename|path|directory|folder|source|target)$/i;
const CAT_KEY_RE = /^category$/i;

function kvRow(pair: PreviewPair, theme: "light" | "dark"): HTMLElement {
  const kv = h("div", "tm-meta__kv");
  const key = str(pair.k);
  const raw = str(pair.v).trim();
  kv.append(h("span", "tm-meta__k", key));
  const v = h("span", "tm-meta__v");
  const catIdx = CAT_KEY_RE.test(key)
    ? CATEGORY_LABELS.findIndex((lb) => lb.toLowerCase() === raw.toLowerCase())
    : -1;
  if (catIdx >= 0) {
    // Two-Channel (R6/R9): the category hue rides a keyed swatch, the word
    // carries the meaning. Hues live only on swatches — chrome stays grey.
    const sw = h("span", "tm-meta__swatch");
    sw.style.background = categoryStyle(catIdx, theme).css;
    v.append(sw, document.createTextNode(raw));
  } else {
    // numbers right, strings/paths left — never right-align a path (§2.4)
    if (!LEFT_KEY_RE.test(key) && isNumValue(raw)) v.classList.add("is-num");
    if (WARN_KEY_RE.test(key) || WARN_VAL_RE.test(raw)) v.classList.add("tm-warn");
    if (EMPTY_VALUES.includes(raw.toLowerCase())) {
      // §2.4 unknown: keyed swatch + word, never a bare em-dash
      v.append(h("span", "tex-hatch tm-meta__swatch"), document.createTextNode("unknown"));
    } else if (FLAG_RE.test(raw)) {
      v.append(h("span", "kbd tm-meta__flag", raw)); // chip geometry from .kbd
    } else {
      v.textContent = raw;
    }
  }
  kv.append(v);
  return kv;
}

/** META (§2.4): padded readout, one hero `.stat` capped at the 21px scale
 *  unless tile-w ≥ 320 (30px tier), kv rows carved by hairlines, optional
 *  right-edge rail carrying ONE live datum (`withRail: false` for cards
 *  built from tile metadata — the rail's datum candidates belong to parsed
 *  payloads, and a rail repeating a visible row would be decoration).
 *
 *  Returns the HEAD META string — the share only ("24%"): the hero stat
 *  owns the size, so the size figure appears exactly ONCE on the card
 *  (live shot had it three times: head, hero, kv row). A payload `share`
 *  pair is moved up to the head verbatim and dropped from the rows; with
 *  no share pair the head stays empty — a caller with a real denominator
 *  (fillPlate) may compute one; this function never does. */
function renderMeta(
  body: HTMLElement,
  mp: PreviewMeta,
  box: Box,
  tileW: number,
  tileH: number,
  root: HTMLElement,
  theme: "light" | "dark",
  withRail = true
): string {
  body.className = "tm-prev__body tm-meta";
  const pairs = Array.isArray(mp.pairs) ? mp.pairs : [];
  // Degraded card (§2.4 honesty): a `note` pair FIRST carries the verbatim
  // backend reason ("symlink" | "unsupported" | "access denied" |
  // "file in use") — rendered as the card's leading reason line, --ink-2, carved
  // by a hairline. A note anywhere else stays an ordinary kv row (pair
  // order is preserved as received); an empty note value is absent.
  const note =
    pairs.length > 0 && /^note$/i.test(str(pairs[0].k).trim()) ? str(pairs[0].v).trim() : "";
  const base = note ? pairs.slice(1) : pairs;
  const heroIdx = base.findIndex(
    (pr) => /^size$/i.test(str(pr.k)) && SIZE_LOOKS_RE.test(str(pr.v))
  );
  const hero = heroIdx >= 0 ? base[heroIdx] : null;
  const heroVal = hero ? str(hero.v).trim() : "";
  // §2.4 de-duplication (size figure appears once; share rides the head):
  //  - a later `size` pair is dropped ONLY when it repeats the hero's
  //    exact value — a different size value is a distinct datum and stays;
  //  - the first non-empty `share` pair moves up to the head meta and
  //    leaves the rows (an empty share pair keeps rendering its honest
  //    "unknown" swatch, as any empty value does).
  let headShare = "";
  const rest: PreviewPair[] = [];
  for (let i = 0; i < base.length; i++) {
    if (i === heroIdx) continue;
    const pr = base[i];
    const k = str(pr.k).trim();
    const v = str(pr.v).trim();
    if (v !== "" && /^share$/i.test(k)) {
      if (!headShare) headShare = v;
      continue;
    }
    if (hero && /^size$/i.test(k) && v === heroVal) continue;
    rest.push(pr);
  }

  // §2.4: the 30px hero tier opens at tile-w ≥ 320 — the same boundary as
  // the 28px head, so one refill covers both tiers. Below it the hero
  // reads at the plain 21px `.stat` scale.
  const heroBig = tileW >= 320;
  const heroH = hero ? (heroBig ? 51 : 41) : 0; // micro 15 + gap 3 + stat-v 23/33
  const sep = hero && rest.length > 0 ? 12 : 0; // flex gap 3 + hairline 1 + pad 8
  const noteH = note ? 25 : 0; // 18px line + 6px pad + 1px hairline
  const avail = box.h - 16 - heroH - sep - noteH; // plate padding 8×2
  const full = Math.floor(avail / 18);
  const needsFooter = rest.length > full;
  const rows = Math.max(0, Math.min(rest.length, Math.floor((avail - (needsFooter ? 16 : 0)) / 18)));

  if (note) body.append(h("div", "tm-meta__reason", note));

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
    for (let i = 0; i < rows; i++) sect.append(kvRow(rest[i], theme));
    body.append(sect);
  }
  if (needsFooter) body.append(footer("tm-meta__more", "fields", rest.length - rows, false));

  // Right-edge rail (§2.4): both tile dimensions must qualify, the datum
  // must be real, and it must fit the rail's full length EXACTLY — measure
  // once at fill time and cut the rail whole otherwise (never clipped
  // mid-glyph; a clipped annotation is a lie).
  if (withRail && tileW >= 360 && tileH >= 240) {
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
  return headShare; // head meta: the share only — the hero owns the size (§2.4)
}

// ------------------------------------------- tile-metadata card (step 2) ---

/** Ladder step 2 gate: is there at least one REAL field to build a card
 *  from? Absent/empty fields never count — a card is built from what we
 *  were given, or not at all. */
function hasTileMeta(m: PlateMeta | null): boolean {
  if (!m) return false;
  if (typeof m.name === "string" && m.name.trim() !== "") return true;
  // same predicate tileMetaPairs uses — a field that would be OMITTED from
  // the card must not qualify the card either (never an empty state)
  if (typeof m.size === "number" && isFinite(m.size) && m.size >= 0) return true;
  if (typeof m.ext === "string" && m.ext.trim() !== "") return true;
  if (m.isDir === true) return true;
  if (typeof m.cat === "number" && Number.isInteger(m.cat) && m.cat >= 0 && m.cat < CATEGORY_LABELS.length)
    return true;
  return false;
}

/** The §2.4 rows a tile can honestly offer: name · type from ext ·
 *  category · size (hero) · share of parent. Each row exists only when its
 *  field is real — missing metadata is absent from the card, never a dash,
 *  never a guess. */
function tileMetaPairs(m: PlateMeta, name: string, shareDenom: number): PreviewPair[] {
  const pairs: PreviewPair[] = [];
  const nm = typeof m.name === "string" && m.name.trim() !== "" ? m.name : name;
  if (nm) pairs.push({ k: "name", v: nm });
  // type from the extension (inherently-upper data — extensions print as
  // spelled); a directory's type is `folder`, extension irrelevant.
  const ext = typeof m.ext === "string" && m.ext.trim() !== "" ? m.ext.trim() : extOf(nm);
  const type = m.isDir === true ? "folder" : ext;
  if (type) pairs.push({ k: "type", v: type });
  const cat = m.cat;
  if (typeof cat === "number" && Number.isInteger(cat) && cat >= 0 && cat < CATEGORY_LABELS.length) {
    pairs.push({ k: "category", v: CATEGORY_LABELS[cat] });
  }
  const size = typeof m.size === "number" && isFinite(m.size) && m.size >= 0 ? m.size : null;
  if (size !== null) pairs.push({ k: "size", v: formatBytes(size) });
  if (size !== null && isFinite(shareDenom) && shareDenom > 0) {
    pairs.push({ k: "share", v: formatPercent(size / shareDenom) });
  }
  return pairs;
}

/** Paint the step-2 card: a §2.4 META readout built purely from tile
 *  metadata, on the shared frame (head strip retained — the file stays
 *  labelled). Rail suppressed: its datum candidates (time/format/signature)
 *  belong to parsed payloads. */
function paintTileMetaCard(
  el: HTMLElement,
  m: PlateMeta,
  tile: PlateTile,
  theme: "light" | "dark",
  shareDenom: number,
  fileName: string
): void {
  // §2.4 proportion cap: the CSS `[data-family="meta"]` max-* clamps the
  // ELEMENT to 460×340; the row math, head tier and rail test must clamp
  // with it (mirrored constants — see META_CAP_W/H) or rows would be cut
  // by pixels without their footer. Anchor and gates stay untouched.
  const tw = Math.min(Math.max(0, num(tile.w)), META_CAP_W);
  const th = Math.min(Math.max(0, num(tile.h)), META_CAP_H);
  el.className = "tm-prev";
  el.dataset.theme = theme;
  el.dataset.head = tw >= 320 ? "lg" : "sm"; // §2.0: 28px head at painted ≥320
  el.dataset.family = "meta";
  const shell = buildShell(el);
  shell.name.textContent = fileName || (typeof m.name === "string" ? m.name : "");
  shell.meta.textContent = renderMeta(
    shell.body,
    { pairs: tileMetaPairs(m, shell.name.textContent, shareDenom) },
    bodyBox({ w: tw, h: th }),
    tw,
    th,
    el,
    theme,
    false
  );
}

/** The painted tile box the draw loop writes on the element (CSS custom
 *  props, px) before any fill — read back when this module is entered
 *  without a tile argument, so the ladder still sizes a card from real
 *  pixels instead of guessing. */
function readTileBox(el: HTMLElement): { w: number; h: number } {
  const w = parseFloat(el.style.getPropertyValue("--tile-w"));
  const h = parseFloat(el.style.getPropertyValue("--tile-h"));
  return { w: isFinite(w) && w > 0 ? w : 0, h: isFinite(h) && h > 0 ? h : 0 };
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

/** PRIMARY entry — rewrite one pooled plate for one payload (idempotent).
 *
 *  Fallback ladder (a plate is never an empty state):
 *   1. usable payload family, at/above its §2.0 gate → that family's
 *      renderer (a family UNDER its gate is not rendered — it steps down);
 *   2. payload null/absent/degenerate → a META card built from the tile's
 *      own metadata (`opts.meta`, or the optional `meta` argument):
 *      name, type from ext, category, hero size, share of parent;
 *   3. metadata also missing → the honest dots state: `no preview` plus
 *      the real reason in `.num` (fillFallbackPlate).
 *
 *  Backwards compatible: `p` widens to null/undefined, `fileName` stays
 *  assignable, `theme`/`tile` are untouched, and `meta` is optional both
 *  as `opts.meta` and as the trailing argument. */
export function fillPlate(
  el: HTMLElement,
  p: PreviewPayload | null | undefined,
  opts: FillPlateOptions,
  meta?: PlateMeta
): void {
  const t = opts.tile || { w: 0, h: 0 };
  const tile = { w: Math.max(0, num(t.w)), h: Math.max(0, num(t.h)) };
  const payload: PreviewPayload = p || {};
  const m: PlateMeta | null = opts.meta ?? meta ?? null;
  const theme: "light" | "dark" = opts.theme === "dark" ? "dark" : "light";

  // pooled node: reset modifiers + full content rebuild every call (cheap,
  // and payload identity carries no entryId — a skip-guard could serve stale
  // bytes when the pool recycles a same-named file)
  el.className = "tm-prev";
  el.dataset.theme = theme;
  el.dataset.head = tile.w >= 320 ? "lg" : "sm"; // §2.0: 28px head at ≥320
  const shell = buildShell(el);
  const box = bodyBox(tile);
  const fam = typeof payload.family === "string" ? payload.family.toUpperCase() : "";
  const fileName =
    opts.fileName || (m && typeof m.name === "string" ? m.name : "") || "";
  shell.name.textContent = fileName;

  // ---- step 1: a usable family, above its gate → the family renderer
  let plan = resolvePlan(payload, fam);
  let gateNote = "";
  if (plan) {
    const g = FAMILY_GATE[plan];
    if (g && (tile.w < g[0] || tile.h < g[1])) {
      plan = ""; // §2.0: below a family's gate, don't render it
      gateNote = `needs ${g[0]} × ${g[1]}`; // step 3's honest reason
    }
  }

  // real byte size for the families whose head meta carries it
  const sizeStr =
    m && typeof m.size === "number" && isFinite(m.size) && m.size >= 0
      ? formatBytes(m.size)
      : "";

  let metaText: string | null = "";
  switch (plan) {
    case "text":
      metaText = renderText(shell.body, payload.text as PreviewText, box, sizeStr);
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
        fileName,
        el,
        sizeStr
      );
      break;
    case "list": {
      metaText = renderTable(
        shell.body,
        listCols(payload.list as PreviewList),
        box,
        fam === "DB" ? "tables" : "entries",
        (payload.list as PreviewList).truncated === true
      );
      // §2.3: the listing source names the section — "zip · 47 entries",
      // "folder · 12 entries". The payload spells the word (lower-case
      // data); absent kind → the plain count, as before. A folder card that
      // descended a single-child wrapper names the chain: "folder via User
      // Data · 20 entries" — real ancestry, not a guessed path.
      const list = payload.list as PreviewList;
      const lk = typeof list.kind === "string" ? list.kind.trim() : "";
      const via = Array.isArray(list.via)
        ? list.via
            .filter((v) => typeof v === "string" && v.trim() !== "")
            .join(" / ")
        : "";
      // The head count is the payload's REAL total when it knows one, not
      // the number of rows this card happened to receive: a folder card is
      // built from a bounded child query, and "32 entries" for a 100-child
      // folder would understate the truth. The table's footer still carries
      // the withheld rows with the honest "+" lower-bound mark.
      const entries = Array.isArray(list.entries) ? list.entries : [];
      const total =
        typeof list.total === "number" && Number.isFinite(list.total) && list.total > entries.length
          ? list.total
          : entries.length;
      const countText = `${formatCount(total)} ${noun1("entries", total)}`;
      if (lk) metaText = `${lk}${via ? ` via ${via}` : ""} · ${countText}`;
      break;
    }
    case "db":
      metaText = renderDb(shell.body, payload.db as PreviewDb, box);
      break;
    case "meta": {
      // §2.4 proportion cap: CSS clamps the ELEMENT (`data-family="meta"`);
      // the row math, head tier and rail test clamp with it (mirrored
      // constants — see META_CAP_W/H). The gate above stays on the FULL
      // painted box — the cap only trims a plate that already qualified.
      const cw = Math.min(tile.w, META_CAP_W);
      const ch = Math.min(tile.h, META_CAP_H);
      el.dataset.head = cw >= 320 ? "lg" : "sm"; // tier follows the painted width
      metaText = renderMeta(
        shell.body,
        payload.meta as PreviewMeta,
        bodyBox({ w: cw, h: ch }),
        cw,
        ch,
        el,
        theme
      );
      // head meta = SHARE only (the hero stat owns the size). A payload
      // `share` pair wins (renderMeta returned it); else compute from the
      // caller's real tile metadata + denominator; else omit — a share is
      // printed only when its denominator is real, never fabricated.
      if (!metaText) {
        const denom = m?.parentSize ?? opts.parentSize ?? 0;
        const bytes =
          m && typeof m.size === "number" && isFinite(m.size) && m.size >= 0 ? m.size : 0;
        if (bytes > 0 && isFinite(denom) && denom > 0) {
          metaText = formatPercent(bytes / denom);
        }
      }
      break;
    }
    case "media":
      metaText = renderMedia(
        shell.body,
        payload.media as PreviewMedia,
        box,
        fileName,
        el,
        theme
      );
      break;
  }

  if (plan && metaText !== null) {
    shell.meta.textContent = metaText;
    el.dataset.family = plan;
    return;
  }

  // ---- step 2: no renderable payload → the META card from tile metadata
  if (m && hasTileMeta(m)) {
    if (tile.w >= 120 && tile.h >= 92) {
      paintTileMetaCard(
        el,
        m,
        tile,
        theme,
        m.parentSize ?? opts.parentSize ?? 0,
        fileName
      );
      return;
    }
    gateNote = "needs 120 × 92"; // blocked at the META gate — the real reason
  }

  // ---- step 3: metadata missing too → the honest dots state + real reason
  fillFallbackPlate(el, gateNote || "no preview data");
}

/** MEDIA — inert instrument plate. The binding law gives plates no pointer
 *  events (map gestures win, integrator note 1), so the native transport
 *  controls are out: video previews its real first frame (framed like
 *  IMAGE), audio gets a centred stat. Real container facts ride the head
 *  meta (extension, parsed duration as a formatDuration clock stamp, pixel
 *  dimensions) and the parsed `pairs` render below the media area in the
 *  meta kv rhythm — every field real or omitted, never invented. */
function renderMedia(
  body: HTMLElement,
  m: PreviewMedia,
  box: Box,
  fileName: string,
  root: HTMLElement,
  theme: "light" | "dark"
): string {
  body.className = "tm-prev__body tm-media";
  // head meta — the same string on every path, so the file stays labelled
  // even when the bed degrades: extension · parsed duration · pixel dims.
  const head = (): string => {
    const parts: string[] = [];
    const ex = extOf(fileName, m.mime);
    if (ex) parts.push(ex);
    const ms = num(m.durationMs);
    if (ms > 0) parts.push(formatDuration(ms)); // clock stamp, not prose
    const mw = num(m.w);
    const mh = num(m.h);
    if (mw > 0 && mh > 0) parts.push(`${mw} × ${mh}`);
    return parts.join(" · ");
  };
  if (!isTauri()) {
    renderFailed(body, "needs the desktop app"); // demo/web build: no asset scope
    return head();
  }
  const media: HTMLMediaElement =
    m.kind === "video" ? document.createElement("video") : document.createElement("audio");
  try {
    media.src = convertFileSrc(m.path);
  } catch {
    renderFailed(body, "path unavailable");
    return head();
  }
  media.preload = "metadata";
  media.controls = false; // inert plate: no transport, no pointer events

  // --- media area: first frame (video) / centred stat (audio) + stamp
  const view = h("div", "tm-media__view");
  let stamp: HTMLElement | null = null;
  const fig = h("span", "stat-v");
  fig.hidden = true; // appears only with a real duration — never a dash
  const setMs = (ms: number): void => {
    if (!(ms > 0) || !root.contains(media)) return;
    const t = formatDuration(ms);
    if (m.kind === "video") {
      if (!stamp) {
        stamp = h("span", "num tm-media__stamp");
        view.append(stamp);
      }
      stamp.textContent = t;
    } else {
      fig.hidden = false;
      fig.textContent = t;
    }
  };

  if (m.kind === "video") {
    media.className = "tm-media__el";
    view.append(media);
  } else {
    // audio: no pixels to preview — a centred `.stat` readout
    const stack = h("div", "tm-stack");
    const stat = h("div", "stat");
    stat.append(h("span", "micro", "audio"), fig);
    stack.append(stat);
    view.append(stack);
    media.className = "tm-media__src"; // metadata loader only — no visuals
  }
  body.append(view);
  if (m.kind === "audio") body.append(media); // hidden loader, outside the view

  // --- container facts: the parsed pairs in the meta kv rhythm, in the
  // plate's lower band. Budget: the media area keeps the majority (≤40%
  // of the body for pairs); an over-cap tail is cut with the honest
  // footer. Fully-empty pairs never render a row.
  const pairs = Array.isArray(m.pairs) ? m.pairs.filter((pr) => str(pr.k) || str(pr.v)) : [];
  if (pairs.length > 0) {
    const cap = Math.floor(box.h * 0.4);
    const shown0 = Math.floor((cap - 12) / 18); // bed padding 6×2
    const cut = pairs.length > shown0;
    const shown = Math.max(0, cut ? Math.floor((cap - 12 - 16) / 18) : shown0);
    if (shown > 0) {
      const bed = h("div", "tm-media__pairs");
      for (let i = 0; i < shown; i++) bed.append(kvRow(pairs[i], theme));
      body.append(bed);
      if (cut) body.append(footer("tm-meta__more", "fields", pairs.length - shown, false));
    }
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
  return head(); // real fields only, each absent field omitted
}

/** Honest unavailable state (§2.0 step 3): dots bed over the whole plate,
 *  keyed hatch swatch + sentence-case `no preview` + the real reason in
 *  `.num`. With tile metadata in the optional third argument it paints the
 *  step-2 metadata card instead — so a caller holding metadata never lands
 *  on the dots state when a card can still be built. Reason printed only
 *  when it adds information (the renderer passes `no preview` as its
 *  reason; echoing the word above it would be noise, not honesty). */
export function fillFallbackPlate(el: HTMLElement, reason: string, meta?: PlateMeta): void {
  if (meta && hasTileMeta(meta)) {
    const tile = readTileBox(el);
    if (tile.w >= 120 && tile.h >= 92) {
      const theme: "light" | "dark" =
        (el.dataset.theme ?? document.documentElement.dataset.theme) === "light"
          ? "light"
          : "dark";
      paintTileMetaCard(el, meta, tile, theme, meta.parentSize ?? 0, meta.name || "");
      return;
    }
  }
  el.className = "tm-prev tm-prev--fallback"; // modifier drops the head strip
  el.dataset.head = "sm";
  el.removeAttribute("data-family");
  const shell = buildShell(el); // clears any stale pooled name/meta text
  shell.body.className = "tm-prev__body tex-dots"; // dots bed over the plate
  const stack = h("div", "tm-stack");
  stack.append(
    h("span", "tex-hatch tm-fallback__swatch"),
    h("span", "micro", "no preview")
  );
  const note = reason && reason !== "no preview" ? reason : "";
  if (note) stack.append(h("span", "num tm-stack__note", note));
  shell.body.append(stack);
}
