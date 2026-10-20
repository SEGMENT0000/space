// Synthetic-but-realistic scan used when the app runs outside Tauri (the
// browser dev server, screenshots). Labeled: nothing here is a real disk.
// Structure mirrors the reference scan from PRODUCT.md; all names are
// fictional — no real people, accounts, or machine paths appear here.

import { extOf } from "./ext";
import { formatBytes } from "./format";
import type {
  ChildEntry,
  ChildrenResponse,
  DriveInfo,
  LargestFile,
  PreviewEntryKind,
  PreviewFamily,
  PreviewListKind,
  PreviewPayload,
  ScanStateDto,
} from "./types";

const MB = 1048576;
const KB = 1024;
const GB = 1073741824;

interface DNode {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  cat: number;
  fileCount: number;
  path: string;
  kids?: DNode[];
}

function d(
  id: number,
  name: string,
  size: number,
  cat: number,
  fileCount: number,
  path: string,
  kids?: DNode[],
): DNode {
  return { id, name, size, isDir: true, cat, fileCount, path, kids };
}
function f(id: number, name: string, size: number, cat: number, path: string): DNode {
  return { id, name, size, isDir: false, cat, fileCount: 0, path };
}

const CATS = { MEDIA: 0, DOCS: 1, CODE: 2, CACHE: 3, APPS: 4, ARCH: 5, SYS: 6, OTHER: 7 };

const RAW_FOOTAGE = d(11, "raw-footage", 28.1 * MB, CATS.MEDIA, 41, "D:\\clips\\raw-footage", [
  f(111, "take_01.mp4", 12.4 * MB, CATS.MEDIA, "D:\\clips\\raw-footage\\take_01.mp4"),
  f(112, "take_02.mp4", 9.8 * MB, CATS.MEDIA, "D:\\clips\\raw-footage\\take_02.mp4"),
  f(113, "img_2213.jpg", 3.2 * MB, CATS.MEDIA, "D:\\clips\\raw-footage\\img_2213.jpg"),
  f(114, "voice_note.m4a", 1.9 * MB, CATS.MEDIA, "D:\\clips\\raw-footage\\voice_note.m4a"),
  f(115, "thumb.png", 640 * KB, CATS.MEDIA, "D:\\clips\\raw-footage\\thumb.png"),
]);

const CLIPS = d(10, "clips", 40.9 * MB, CATS.OTHER, 56, "D:\\clips", [
  RAW_FOOTAGE,
  d(12, "b-roll", 3.97 * MB, CATS.MEDIA, 5, "D:\\clips\\b-roll", [
    f(121, "broll_clip.mp4", 3.1 * MB, CATS.MEDIA, "D:\\clips\\b-roll\\broll_clip.mp4"),
    f(122, "s_poster.png", 880 * KB, CATS.MEDIA, "D:\\clips\\b-roll\\s_poster.png"),
  ]),
  d(13, "portfolio", 3.93 * MB, CATS.DOCS, 4, "D:\\clips\\portfolio", [
    f(131, "photoshoot.zip", 3.4 * MB, CATS.ARCH, "D:\\clips\\portfolio\\photoshoot.zip"),
    f(132, "resume_2026.pdf", 210 * KB, CATS.DOCS, "D:\\clips\\portfolio\\resume_2026.pdf"),
  ]),
  d(14, "audio", 1.62 * MB, CATS.OTHER, 6, "D:\\clips\\audio", [
    f(141, "session_notes.docx", 340 * KB, CATS.DOCS, "D:\\clips\\audio\\session_notes.docx"),
    f(142, "playlist.m3u", 12 * KB, CATS.OTHER, "D:\\clips\\audio\\playlist.m3u"),
  ]),
  d(15, "stories", 1.27 * MB, CATS.MEDIA, 3, "D:\\clips\\stories", [
    f(151, "story_01.mp4", 1.1 * MB, CATS.MEDIA, "D:\\clips\\stories\\story_01.mp4"),
  ]),
  f(16, "title_card.png", 1.17 * MB, CATS.MEDIA, "D:\\clips\\title_card.png"),
  d(17, "spider", 467 * KB, CATS.OTHER, 4, "D:\\clips\\spider", [
    f(171, "spiderman_wallpaper.jpg", 420 * KB, CATS.MEDIA, "D:\\clips\\spider\\spiderman_wallpaper.jpg"),
  ]),
  d(18, "shorts", 343 * KB, CATS.MEDIA, 2, "D:\\clips\\shorts"),
  f(19, "shot-list", 52.4 * KB, CATS.DOCS, "D:\\clips\\shot-list"),
]);

const WINDOWS = d(20, "Windows", 5_320_000_000, CATS.SYS, 188, "D:\\Windows", [
  f(201, "pagefile.sys", 2 * GB, CATS.SYS, "D:\\Windows\\pagefile.sys"),
  d(202, "WinSxS", 1_400_000_000, CATS.SYS, 60, "D:\\Windows\\WinSxS", [
    f(2021, "kernel32.dll.mui", 186 * KB, CATS.SYS, "D:\\Windows\\WinSxS\\kernel32.dll.mui"),
  ]),
  d(203, "System32", 900_000_000, CATS.SYS, 40, "D:\\Windows\\System32", [
    f(2031, "ntoskrnl.exe", 12 * MB, CATS.SYS, "D:\\Windows\\System32\\ntoskrnl.exe"),
  ]),
  d(204, "SoftwareDistribution", 480_000_000, CATS.CACHE, 20, "D:\\Windows\\SoftwareDistribution", [
    f(2041, "update.cab", 140 * MB, CATS.CACHE, "D:\\Windows\\SoftwareDistribution\\update.cab"),
  ]),
]);

const USERS = d(30, "Users", 2_147_483_648, CATS.OTHER, 96, "D:\\Users", [
  d(31, "casey", 1_900_000_000, CATS.OTHER, 80, "D:\\Users\\casey", [
    d(311, "Downloads", 800_000_000, CATS.ARCH, 20, "D:\\Users\\casey\\Downloads", [
      f(3111, "ubuntu-24.04.iso", 1.6 * GB, CATS.ARCH, "D:\\Users\\casey\\Downloads\\ubuntu-24.04.iso"),
    ]),
    d(312, "Videos", 620_000_000, CATS.MEDIA, 25, "D:\\Users\\casey\\Videos", [
      f(3121, "blender_render.mp4", 780 * MB, CATS.MEDIA, "D:\\Users\\casey\\Videos\\blender_render.mp4"),
    ]),
    d(313, "AppData", 280_000_000, CATS.CACHE, 30, "D:\\Users\\casey\\AppData", [
      f(3131, "Cache.data", 96 * MB, CATS.CACHE, "D:\\Users\\casey\\AppData\\Cache.data"),
    ]),
  ]),
]);

const PROGRAM_FILES = d(40, "Program Files", 943_718_400, CATS.APPS, 61, "D:\\Program Files", [
  d(401, "Blender", 320_000_000, CATS.APPS, 15, "D:\\Program Files\\Blender", [
    f(4011, "blender.exe", 64 * MB, CATS.APPS, "D:\\Program Files\\Blender\\blender.exe"),
  ]),
  d(402, "Microsoft VS Code", 310_000_000, CATS.CODE, 30, "D:\\Program Files\\Microsoft VS Code", [
    f(4021, "code.exe", 140 * MB, CATS.CODE, "D:\\Program Files\\Microsoft VS Code\\code.exe"),
  ]),
]);

const RECOVERY = d(50, "Recovery", 472_446_400, CATS.SYS, 12, "D:\\Recovery");

const ROOT: DNode = d(1, "D:", 0, CATS.OTHER, 413, "D:\\", [
  WINDOWS,
  USERS,
  CLIPS,
  PROGRAM_FILES,
  RECOVERY,
]);
ROOT.size = WINDOWS.size + USERS.size + CLIPS.size + PROGRAM_FILES.size + RECOVERY.size;

const INDEX = new Map<number, DNode>();
const PARENT = new Map<number, DNode>();
const BY_PATH = new Map<string, DNode>();
(function index(nodes: DNode[], parent: DNode | null) {
  for (const n of nodes) {
    INDEX.set(n.id, n);
    BY_PATH.set(n.path, n); // preview synthesis resolves paths, not ids (§B)
    if (parent) PARENT.set(n.id, parent);
    if (n.kids) index(n.kids, n);
  }
})([ROOT], null);

// A directory's bytes ARE its children's bytes — the scanner sums the walk,
// so a parent can never be larger than the sum of what it contains. The
// hand-authored demo sizes were approximate; the honest-remainder layout
// correctly exposed the gaps as mystery “other” area. Normalize bottom-up
// so the demo exercises the real invariant, the map's proportions are true,
// and the rail's category total agrees with its headline.
(function normalize(n: DNode): number {
  if (n.kids && n.kids.length > 0) {
    let sum = 0;
    for (const k of n.kids) sum += normalize(k);
    n.size = sum;
  }
  return n.size;
})(ROOT);

function toEntry(n: DNode, withSub: boolean): ChildEntry {
  return {
    id: n.id,
    name: n.name,
    size: n.size,
    isDir: n.isDir,
    fileCount: n.fileCount,
    cat: n.cat,
    kidCount: n.kids?.length ?? 0,
    sub:
      withSub && n.kids
        ? n.kids.map((k) => ({ ...toEntry(k, false), sub: k.kids?.map((kk) => toEntry(kk, false)) }))
        : undefined,
  };
}

function lineageOf(id: number): { id: number; name: string }[] {
  const out: { id: number; name: string }[] = [];
  let cur: DNode | undefined = INDEX.get(id);
  while (cur) {
    out.unshift({ id: cur.id, name: cur.name });
    cur = PARENT.get(cur.id);
  }
  return out;
}

function subtree(node: DNode): DNode[] {
  const out: DNode[] = [node];
  for (const k of node.kids ?? []) out.push(...subtree(k));
  return out;
}

export interface DemoState {
  rootId: number;
  rootPath: string;
  totalBytes: number;
  totalFiles: number;
  drives: DriveInfo[];
  largest: LargestFile[];
  scanState(): ScanStateDto;
  children(nodeId: number): ChildrenResponse | null;
  categoryTotals(nodeId: number): number[];
  getPreview(path: string): PreviewPayload;
}

export function buildDemo(): DemoState {
  const largest: LargestFile[] = [
    f(901, "ubuntu-24.04.iso", 1.6 * GB, CATS.ARCH, "D:\\Users\\casey\\Downloads\\ubuntu-24.04.iso"),
    f(902, "pagefile.sys", 2 * GB, CATS.SYS, "D:\\Windows\\pagefile.sys"),
    f(903, "blender_render.mp4", 780 * MB, CATS.MEDIA, "D:\\Users\\casey\\Videos\\blender_render.mp4"),
    f(904, "update.cab", 140 * MB, CATS.CACHE, "D:\\Windows\\SoftwareDistribution\\update.cab"),
    f(905, "code.exe", 140 * MB, CATS.CODE, "D:\\Program Files\\Microsoft VS Code\\code.exe"),
    f(906, "Cache.data", 96 * MB, CATS.CACHE, "D:\\Users\\casey\\AppData\\Cache.data"),
    f(907, "blender.exe", 64 * MB, CATS.APPS, "D:\\Program Files\\Blender\\blender.exe"),
    f(908, "ntoskrnl.exe", 12 * MB, CATS.SYS, "D:\\Windows\\System32\\ntoskrnl.exe"),
    f(909, "take_01.mp4", 12.4 * MB, CATS.MEDIA, "D:\\clips\\raw-footage\\take_01.mp4"),
    f(910, "title_card.png", 1.17 * MB, CATS.MEDIA, "D:\\clips\\title_card.png"),
  ].map((n, i) => ({ id: 900 + i + 1, name: n.name, size: n.size, cat: n.cat, path: n.path }));

  // Memoized so synthesis is deterministic: the same path always hands back
  // the very same payload object (canvas dataUrls included).
  const previewMemo = new Map<string, PreviewPayload>();

  return {
    rootId: ROOT.id,
    rootPath: ROOT.path,
    totalBytes: ROOT.size,
    totalFiles: ROOT.fileCount,
    drives: [
      {
        name: "System",
        mount: "C:\\",
        fs: "NTFS",
        total: 512 * GB,
        free: 187 * GB,
        kind: "ssd",
        isRemovable: false,
      },
      {
        name: "Data",
        mount: "D:\\",
        fs: "NTFS",
        total: 1.86 * 1024 * GB,
        free: 1.02 * 1024 * GB,
        kind: "ssd",
        isRemovable: false,
      },
      {
        name: "Backup",
        mount: "E:\\",
        fs: "exFAT",
        total: 931 * GB,
        free: 402 * GB,
        kind: "removable",
        isRemovable: true,
      },
    ],
    largest: largest.sort((a, b) => b.size - a.size),
    scanState: () => ({ state: "ready", root: ROOT.path, error: null }),
    children(nodeId) {
      const node = INDEX.get(nodeId);
      if (!node) return null;
      return {
        parentId: node.id,
        parentSize: node.size,
        parentCat: node.cat,
        totalEntries: node.kids?.length ?? 0,
        entries: (node.kids ?? []).map((k) => toEntry(k, true)),
        lineage: lineageOf(node.id),
        path: node.path,
      };
    },
    categoryTotals(nodeId) {
      const node = INDEX.get(nodeId);
      const totals = new Array(8).fill(0);
      if (!node) return totals;
      // leaf-only sums under the node (excluding the node itself)
      for (const n of subtree(node)) {
        if (n.id === nodeId) continue;
        if (n.isDir && n.kids?.length) continue;
        totals[n.cat] += n.size;
      }
      return totals;
    },
    getPreview(path) {
      const hit = previewMemo.get(path);
      if (hit) return hit; // memoized — same path → byte-identical payload
      const payload = synthPreview(path);
      previewMemo.set(path, payload);
      return payload;
    },
  };
}

// ---------------------------------------------------------------- preview synth
// Demo previews (FLEET §A/§B): one synthesized payload per family, keyed and
// memoized by path so the same path always yields byte-identical content.
// Content is real-shaped — actual JSON/TS/ini/log syntax, real file-signature
// bytes, plausible numbers — never lorem ipsum, never gray noise. The demo
// must be indistinguishable in craft from the Rust payloads.
// The one family the demo never emits is MEDIA: there are no real bytes to
// play behind a fictional path, and the web build has no asset-protocol
// scope (previewPlate's own MEDIA plate answers "needs the desktop app"
// outside Tauri) — so playable files land on their honest META readout.

type Rng = () => number;

/** FNV-1a: tiny, stable, dependency-free path → seed. */
function seedOf(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** mulberry32 — deterministic PRNG, plenty for plausible bytes and numbers. */
function rngFrom(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function pick<T>(rand: Rng, xs: readonly T[]): T {
  return xs[Math.floor(rand() * xs.length) % xs.length];
}

function intOf(rand: Rng, lo: number, hi: number): number {
  return lo + Math.floor(rand() * (hi - lo + 1));
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** "D:\clips\raw-footage\img_2213.jpg" -> "img_2213" */
function stemOf(path: string): string {
  const base = path.slice(path.lastIndexOf("\\") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("\\") + 1);
}

/** "2026-03-04 11:20" — UTC getters so the demo doesn't shift with timezone. */
function mtimeOf(seed: number): string {
  const rand = rngFrom(seed ^ 0x9e3779b9);
  const start = Date.UTC(2023, 0, 1);
  const span = Date.UTC(2026, 8, 1) - start;
  const t = new Date(start + Math.floor(rand() * span));
  return `${t.getUTCFullYear()}-${pad2(t.getUTCMonth() + 1)}-${pad2(t.getUTCDate())} ${pad2(t.getUTCHours())}:${pad2(t.getUTCMinutes())}`;
}

/** "resume_2026" -> "Resume 2026" */
function titleOf(stem: string): string {
  return stem
    .split(/[_\-\s]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

/** "take_01" -> "take01" (identifiers need a leading letter). */
function camelOf(stem: string): string {
  const s = stem
    .replace(/[^a-zA-Z0-9]+(.)?/g, (_m, c?: string) => (c ? c.toUpperCase() : ""))
    .replace(/^[0-9]+/, "");
  return s || "collect";
}

/** Head lines + body lines cycled until the file reaches `count` lines. */
function fill(count: number, head: string[], body: string[]): string[] {
  const out = head.slice(0, count);
  for (let i = 0; out.length < count; i++) out.push(body[i % body.length]);
  return out;
}

/** n weights that sum to exactly `total` — archive sizes add up like real ones. */
function scaledSizes(rand: Rng, n: number, total: number): number[] {
  const w: number[] = [];
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const x = 0.4 + rand();
    w.push(x);
    sum += x;
  }
  return w.map((x) => Math.max(512, Math.round((x / sum) * total)));
}

// §A family mapping (extension → family). SVG previews as TEXT this wave
// (INS_BRIEF integrator note 2 — and PreviewFamily has no "svg" member).
const TEXT_EXTS = new Set(
  (
    "txt md rtf log logs csv json yaml yml toml xml " +
    "ini cfg conf env properties reg plist " +
    "docx odt " +
    "rs ts tsx js jsx mjs cjs py java c cpp h hpp cc go sh ps1 bat sql vue " +
    "m3u m3u8 vtt srt ass ssa eml mbox " +
    "pem crt cer asc key pub eps svg " +
    "obj stl ply gltf glb dxf step iges"
  ).split(" "),
);
const IMAGE_EXTS = new Set("jpg jpeg png gif webp bmp tif tiff ico heic avif".split(" "));
const HEX_EXTS = new Set(
  (
    "bin dat dmp pak mdb accdb sys drv tmp temp part crdownload " +
    "fw rom efi fd fbx blend dwg idx sub"
  ).split(" "),
);
const DB_EXTS = new Set("db sqlite sqlite3".split(" "));
// Exactly the extensions the Rust dispatcher lists (preview.rs): zip/epub
// via their central directory, tar, iso/img trees, xlsx/ods sheets. rar, 7z,
// cab, dmg and vhd/vhdx/wim are NOT listed — the zip probe fails and §B
// falls back to a META magic card, so the demo falls back with it.
const LIST_EXTS = new Set("zip tar epub xlsx ods iso img".split(" "));
// Zip members whose payload ships already compressed are stored, not
// deflated — the archive-tool words the Rust central-dir reader prints.
const ZIP_STORE_EXTS = new Set("jpg jpeg png gif webp mp4 mkv mov m4a mp3 flac ogg zip pdf".split(" "));

function familyFor(ext: string): PreviewFamily {
  if (!ext) return "hex"; // §A: extensionless names sniff as binary
  if (TEXT_EXTS.has(ext)) return "text";
  if (IMAGE_EXTS.has(ext)) return "image";
  if (HEX_EXTS.has(ext)) return "hex";
  if (DB_EXTS.has(ext)) return "db";
  if (LIST_EXTS.has(ext)) return "list";
  // META is the honest default: a properties readout never fabricates content
  // it didn't decode.
  return "meta";
}

// Groups consumed by metaPayload.
const PE_EXTS = new Set("exe dll msi sys drv efi mui".split(" "));
const ELF_EXTS = new Set("so dylib".split(" "));
const PKG_EXTS = new Set("deb rpm apk jar".split(" "));
const AUDIO_EXTS = new Set("mp3 wav flac aac ogg m4a".split(" "));
const VIDEO_EXTS = new Set("mp4 mkv avi mov webm m4v".split(" "));
const COMPRESS_EXTS = new Set("gz tgz bz2 xz zst br lz4".split(" "));
const FONT_EXTS = new Set("ttf otf woff woff2 ttc".split(" "));
const SCI_EXTS = new Set("h5 hdf5 nc fits dcm npy parquet mat".split(" "));
const ENC_EXTS = new Set("enc age gpg pgp vault".split(" "));
const KEYSTORE_EXTS = new Set("der pfx p12 jks".split(" "));
const MAILBOX_EXTS = new Set("msg pst ost".split(" "));
const PRESENT_EXTS = new Set("pptx odp ppt".split(" "));
const BACKUP_EXTS = new Set("bak backup bkp".split(" "));

/** Real signature strings (uppercase — inherently-upper data per DESIGN:177). */
const SIGNATURES: Record<string, string> = {
  jpg: "FF D8 FF E0",
  jpeg: "FF D8 FF E0",
  png: "89 50 4E 47 0D 0A 1A 0A",
  gif: "47 49 46 38 39 61",
  webp: "52 49 46 46 .. 57 45 42 50",
  mp4: "00 00 00 18 66 74 79 70",
  m4a: "00 00 00 18 66 74 79 70",
  mov: "00 00 00 18 66 74 79 70",
  mp3: "49 44 33 34",
  wav: "52 49 46 46",
  flac: "66 4C 61 43",
  ogg: "4F 67 67 53",
  aac: "FF F1",
  pdf: "%PDF-1.7",
  zip: "50 4B 03 04",
  docx: "50 4B 03 04",
  xlsx: "50 4B 03 04",
  epub: "50 4B 03 04",
  apk: "50 4B 03 04",
  exe: "4D 5A 90 00",
  dll: "4D 5A 90 00",
  msi: "4D 5A 90 00",
  mui: "4D 5A 90 00",
  so: "7F 45 4C 46 02 01 01 00",
  dylib: "CF FA ED FE 07 00 00 01",
  gz: "1F 8B 08",
  tgz: "1F 8B 08",
  bz2: "42 5A 68",
  xz: "FD 37 7A 58 5A 00",
  zst: "28 B5 2F FD",
  cab: "4D 53 43 46",
  ttf: "00 01 00 00",
  otf: "4F 54 54 4F",
  woff: "77 4F 46 46",
  woff2: "77 4F 46 32",
  deb: "21 3C 61 72 63 68 3E",
  rpm: "ED AB EE DB",
  iso: "CD001 @ 0x8000",
  img: "CD001 @ 0x8000",
  vhdx: "76 68 64 78 66 69 6C 65",
  wim: "4D 53 57 49 4D",
  "7z": "37 7A BC AF 27 1C",
  rar: "52 61 72 21 1A 07",
  vhd: "63 6F 6E 65 63 74 69 78",
  vmdk: "4B 44 4D 56",
  dcm: "44 49 43 4D",
  fits: "SIMPLE  = T",
  h5: "89 48 44 46 0D 0A 1A 0A",
  hdf5: "89 48 44 46 0D 0A 1A 0A",
  msg: "D0 CF 11 E0 A1 B1 1A E1",
  pst: "D0 CF 11 E0 A1 B1 1A E1",
  ost: "D0 CF 11 E0 A1 B1 1A E1",
  jks: "FE ED FE ED",
  der: "30 82",
  pfx: "30 82",
  p12: "30 82",
  gpg: "8C 0D 04 00",
  age: "61 67 65 2D 65 6E 63 72",
  dwg: "41 43 31 30 32 37",
  blend: "42 4C 45 4E 44 45 52 5F",
  mdb: "00 01 00 00 53 74 61 6E",
  accdb: "00 01 00 00 53 74 61 6E",
};

const IMAGE_DIMS: readonly (readonly [number, number])[] = [
  [176, 112],
  [128, 128],
  [160, 96],
  [96, 128],
];

/** ORIGINAL source dimensions — types.ts: `w`/`h` are always the source's,
 *  never the thumbnail's. Plausible phone/camera sizes, all under the 8 MP
 *  decode cap. */
const SOURCE_DIMS: readonly (readonly [number, number])[] = [
  [1920, 1080],
  [4032, 3024],
  [3840, 2160],
  [2048, 1365],
  [3024, 4032],
];

// Flat two-tone pairs only — a gradient-free thumbnail still reads as a photo
// at plate size, and gradients are banned outright.
const TONE_PAIRS: readonly (readonly [string, string])[] = [
  ["#c3d0dd", "#3f5a74"], // overcast sky / night water
  ["#ddd2bd", "#6d5a43"], // sand / dry earth
  ["#cfd8cd", "#44574a"], // haze / forest floor
  ["#e0d3cf", "#7c5f57"], // dusk / brick
  ["#ccd6da", "#5b6d78"], // fog / asphalt
  ["#d8cfc0", "#8a7358"], // paper / kraft
];

/** Folder tile → LIST (FLEET §G): its real demo children, size-sorted.
 *  `kind: "folder"` names the enumerator (preview.rs folder walk); row kinds
 *  are the real "dir" | "file" union. */
function folderList(node: DNode): PreviewPayload {
  const entries: { name: string; size?: number; kind: PreviewEntryKind }[] = (node.kids ?? [])
    .slice()
    .sort((a, b) => b.size - a.size || a.name.localeCompare(b.name))
    .map((k) => ({ name: k.name, size: k.size, kind: k.isDir ? "dir" : "file" }));
  return {
    family: "list",
    list: { entries, truncated: entries.length > 200, total: entries.length, kind: "folder" },
  };
}

/** Stamp the bytes a hexdump annotator actually recognizes (INS §2.1). */
function stampMagic(buf: Uint8Array, ext: string, name: string): void {
  const put = (off: number, xs: number[]) => {
    for (let i = 0; i < xs.length && off + i < buf.length; i++) buf[off + i] = xs[i];
  };
  const putStr = (off: number, s: string) => put(off, Array.from(s, (c) => c.charCodeAt(0)));
  // Paging files are records, not executables — a zero head is the honest fake.
  if (/^(pagefile|swapfile|hiberfil)\./i.test(name)) {
    put(0x00, new Array(128).fill(0));
    return;
  }
  // PE/DOS: real MZ stub text, e_lfanew at 0x3C, PE signature + x64 machine
  // word at 0xF0 — so a plate that annotates "DOS header" finds real data.
  const mz = () => {
    put(0x00, [0x4d, 0x5a, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00]);
    put(0x10, [0xb8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    putStr(0x4e, "This program cannot be run in DOS mode.\r\r\n$");
    put(0x3c, [0xf0, 0x00, 0x00, 0x00]); // e_lfanew -> PE header at 0xF0
    put(0xf0, [0x50, 0x45, 0x00, 0x00, 0x64, 0x86, 0x06, 0x00]); // "PE\0\0", x86-64, 6 sections
  };
  const elf = (shared: boolean) => {
    put(0x00, [0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    put(0x10, [shared ? 3 : 2, 0, 0x3e, 0x00, 1, 0, 0, 0]); // ET_DYN/ET_EXEC, EM_X86_64
  };
  switch (ext) {
    case "exe":
    case "dll":
    case "msi":
    case "sys":
    case "drv":
    case "efi":
    case "mui":
      mz();
      return;
    case "so":
      elf(true);
      return;
    case "dylib":
      elf(false);
      return;
    case "zip":
    case "apk": {
      // A plausible local file header: version 2.0, deflate, real member name.
      const member = `${stemOf(name).toLowerCase()}/IMG_0417.jpg`;
      putStr(0x00, "PK\u0003\u0004");
      put(0x04, [0x14, 0x00, 0x00, 0x00]);
      put(0x08, [0x08, 0x00]); // method 8 = deflate
      put(0x1a, [member.length, 0x00]); // name length
      put(0x1c, [0x00, 0x00]); // extra length
      putStr(0x1e, member);
      return;
    }
    case "cab":
      putStr(0x00, "MSCF");
      put(0x18, [0x03, 0x01]); // cabinet version 1.3
      return;
    case "png":
      put(0x00, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
      put(0x08, [0x00, 0x00, 0x00, 0x0d]); // IHDR length
      putStr(0x0c, "IHDR");
      put(0x10, [0x00, 0x05, 0x00, 0x00]); // width 1280
      put(0x14, [0x00, 0x03, 0x00, 0x00]); // height 768
      put(0x18, [0x08, 0x02, 0x00, 0x00, 0x00]); // 8-bit truecolor
      return;
    case "jpg":
    case "jpeg":
      put(0x00, [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]);
      putStr(0x06, "JFIF\0");
      put(0x0b, [0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
      return;
    case "gif":
      putStr(0x00, "GIF89a");
      return;
    case "mp4":
    case "m4a":
    case "mov":
      put(0x00, [0x00, 0x00, 0x00, 0x18]); // box size 24
      putStr(0x04, "ftyp");
      putStr(0x08, "isom");
      put(0x0c, [0x00, 0x00, 0x02, 0x00]); // minor version
      putStr(0x10, "isom");
      return;
    case "gz":
    case "tgz":
      put(0x00, [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03]);
      return;
    case "pdf":
      putStr(0x00, "%PDF-1.7\n");
      put(0x09, [0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
      return;
    case "iso":
    case "img":
      put(0x00, new Array(32).fill(0)); // system area before the PVD
      return;
    case "vhdx":
      putStr(0x00, "vhdxfile");
      return;
    case "wim":
      putStr(0x00, "MSWIM");
      return;
    case "dwg":
      putStr(0x00, "AC1027");
      return;
    case "blend":
      putStr(0x00, "BLENDER_v300");
      return;
    case "mdb":
      put(0x00, [0x00, 0x01, 0x00, 0x00]);
      putStr(0x04, "Standard Jet DB\0");
      return;
    case "accdb":
      put(0x00, [0x00, 0x01, 0x00, 0x00]);
      putStr(0x04, "Standard ACE DB\0");
      return;
    default:
      return; // unidentified bytes stay seeded noise — honest for unknowns
  }
}

const TEXT_CAP = 120; // §A / types.ts: TEXT head cap 64 KB / 120 lines
const TEXT_MIN = 16; // small enough files can still close structural syntax

function textPayload(path: string, ext: string, seed: number, node: DNode | undefined): PreviewPayload {
  const rand = rngFrom(seed);
  const size = node?.size ?? 0;
  // Estimate the real line count from size (~34 B/line average) so the
  // truncation footer "… N more lines" carries a plausible number.
  const totalLines = Math.max(TEXT_MIN, Math.floor(size / 34));
  const truncated = totalLines > TEXT_CAP;
  const count = Math.min(totalLines, TEXT_CAP);
  const lines = genText(ext, count, rand, stemOf(path));
  // All synthesized text is ASCII in UTF-8 clothing — the same lossy utf-8
  // readout the Rust head emits when no BOM says otherwise (model.rs TextPreview).
  return { family: "text", text: { lines, truncated, totalLines, encoding: "utf-8" } };
}

function hexPayload(path: string, ext: string, seed: number, node: DNode | undefined): PreviewPayload {
  const rand = rngFrom(seed);
  const total = node?.size ?? 0;
  // §A: hexdump of the first 4 KB. Rust emits 8 bytes per row and the frontend
  // regroups to 8/16 per tile width (model.rs HexRow) — mirror that exactly.
  const span = total > 0 ? Math.min(4096, Math.max(8, Math.ceil(total / 8) * 8)) : 4096;
  const buf = new Uint8Array(4096);
  for (let i = 0; i < buf.length; i++) buf[i] = Math.floor(rand() * 256);
  stampMagic(buf, ext, nameOf(path));
  const rows: { off: number; bytes: string; ascii: string }[] = [];
  for (let off = 0; off < span; off += 8) {
    let bytes = "";
    let ascii = "";
    for (let i = 0; i < 8; i++) {
      const b = buf[off + i];
      bytes += (i ? " " : "") + b.toString(16).padStart(2, "0");
      ascii += b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : "·";
    }
    rows.push({ off, bytes, ascii });
  }
  // Unknown-size paths report what the rows actually cover, never 0 with rows.
  return { family: "hex", hex: { rows, base: 0, total: total || span } };
}

function imagePayload(
  path: string,
  ext: string,
  seed: number,
  node: DNode | undefined
): PreviewPayload {
  const rand = rngFrom(seed);
  // types.ts: `w`/`h` are the ORIGINAL source dimensions; the payload bitmap
  // is the ≤512-edge re-encode, which the canvas below stands in for.
  const [w, h] = pick(rand, SOURCE_DIMS);
  const [tw, th] = pick(rand, IMAGE_DIMS); // canvas thumbnail, ≤256 longest edge
  // No DOM (SSR/tests) or no 2D context: hand back an honest META readout —
  // the REAL extension (never a hardcoded "png"), the source dimensions, and
  // the true signature — rather than a fabricated raster. The browser demo
  // always has a canvas.
  const fallback = (): PreviewPayload => {
    const fb = metaPayload(path, ext, seed, node);
    fb.meta?.pairs.unshift({ k: "thumbnail", v: `${tw} × ${th} px (JPEG q80)` });
    fb.meta?.pairs.unshift({ k: "dimensions", v: `${w} × ${h} px` });
    return fb;
  };
  if (typeof document === "undefined") return fallback();
  const canvas = document.createElement("canvas");
  canvas.width = tw;
  canvas.height = th;
  const ctx = canvas.getContext("2d");
  if (!ctx) return fallback();
  const [a, b] = pick(rand, TONE_PAIRS);
  ctx.fillStyle = a;
  ctx.fillRect(0, 0, tw, th);
  // Horizon split at a seeded ratio — reads as sky/ground at thumbnail size.
  ctx.fillStyle = b;
  ctx.fillRect(0, Math.round(th * (0.35 + rand() * 0.4)), tw, th);
  // Always image/jpeg, like the Rust re-encode (preview.rs build_image).
  return {
    family: "image",
    image: { dataUrl: canvas.toDataURL("image/jpeg", 0.8), w, h, mime: "image/jpeg" },
  };
}

function metaPayload(path: string, ext: string, seed: number, node: DNode | undefined): PreviewPayload {
  const rand = rngFrom(seed);
  const size = node?.size ?? 0;
  const stem = node ? stemOf(node.name) : "";
  // Locked/protected system files preview exactly like the live app's
  // degraded card (preview.rs degraded_meta): the reason verbatim as the
  // FIRST pair, then the mtime and the extension's type label, size when
  // stat-able — and NO signature, because the bytes were never readable.
  if (/^(pagefile|swapfile|hiberfil)\./i.test(node?.name ?? nameOf(path))) {
    return {
      family: "meta",
      meta: {
        pairs: [
          { k: "note", v: "access denied" },
          { k: "modified", v: mtimeOf(seed) },
          { k: "type", v: `executable (.${ext || "sys"})` },
          { k: "size", v: size > 0 ? formatBytes(size) : "unknown" },
        ],
      },
    };
  }
  const pairs: { k: string; v: string }[] = [];
  const dur = `${intOf(rand, 0, 24)}:${pad2(intOf(rand, 0, 59))}:${pad2(intOf(rand, 0, 59))}`;
  if (AUDIO_EXTS.has(ext)) {
    pairs.push({ k: "format", v: "MPEG-4 audio" });
    pairs.push({ k: "codec", v: "AAC LC" });
    pairs.push({ k: "bitrate", v: pick(rand, ["128 kb/s", "192 kb/s", "256 kb/s"]) });
    pairs.push({ k: "sample rate", v: pick(rand, ["44.1 kHz", "48 kHz"]) });
    pairs.push({ k: "channels", v: pick(rand, ["1 (mono)", "2 (stereo)"]) });
    pairs.push({ k: "duration", v: dur });
  } else if (VIDEO_EXTS.has(ext)) {
    pairs.push({ k: "format", v: ext === "webm" ? "WebM container" : "MPEG-4 container" });
    pairs.push({ k: "video", v: pick(rand, ["H.264 / AVC High@L4.1", "HEVC Main@L5", "VP9 Profile 0"]) });
    pairs.push({ k: "resolution", v: pick(rand, ["1920 × 1080", "1280 × 720", "3840 × 2160"]) });
    pairs.push({ k: "frame rate", v: pick(rand, ["24 fps", "30 fps", "60 fps"]) });
    pairs.push({ k: "audio", v: "AAC LC, 48 kHz" });
    pairs.push({ k: "duration", v: dur });
  } else if (PE_EXTS.has(ext)) {
    const native = ext === "sys" || ext === "drv";
    pairs.push({ k: "format", v: native ? "PE32+ native driver (x64)" : "PE32+ executable (x64)" });
    pairs.push({ k: "subsystem", v: native ? "Native" : pick(rand, ["Windows GUI", "Windows CUI"]) });
    pairs.push({ k: "sections", v: String(intOf(rand, 4, 12)) });
    pairs.push({ k: "compiled", v: mtimeOf(seed ^ 0x5bf03635) });
  } else if (ELF_EXTS.has(ext)) {
    pairs.push({
      k: "format",
      v: ext === "so" ? "ELF64 shared object (x86-64)" : "Mach-O 64-bit bundle (x86-64)",
    });
    pairs.push({ k: "sections", v: String(intOf(rand, 18, 40)) });
    pairs.push({ k: "interpreter", v: "/lib64/ld-linux-x86-64.so.2" });
  } else if (ext === "pdf" || ext === "mobi") {
    pairs.push({ k: "format", v: ext === "pdf" ? "PDF 1.7" : "Mobipocket 7" });
    pairs.push({ k: "pages", v: String(intOf(rand, 8, 420)) });
    pairs.push({ k: "producer", v: pick(rand, ["Adobe PDF Library 17", "Chromium PDF Writer", "LibreOffice 7.6"]) });
    pairs.push({ k: "encrypted", v: "no" });
  } else if (COMPRESS_EXTS.has(ext)) {
    const methods: Record<string, string> = {
      gz: "deflate (gzip)",
      tgz: "deflate (gzip)",
      bz2: "bzip2",
      xz: "LZMA2",
      zst: "Zstandard",
      br: "Brotli",
      lz4: "LZ4 block",
    };
    pairs.push({ k: "format", v: `${methods[ext] ?? ext} compressed data` });
    pairs.push({ k: "stored size", v: formatBytes(size) });
    pairs.push({ k: "original size", v: formatBytes(Math.round(size * (2.2 + rand() * 3))) });
  } else if (
    ext === "7z" ||
    ext === "rar" ||
    ext === "cab" ||
    ext === "dmg" ||
    ext === "wim" ||
    ext === "vhd" ||
    ext === "vhdx" ||
    ext === "vmdk"
  ) {
    // Rust never LISTs these — the zip probe fails and §B falls back to a
    // magic card — so the demo's card carries the same honest class words.
    const labels: Record<string, string> = {
      "7z": "7z archive (LZMA2)",
      rar: "RAR archive (RAR5)",
      cab: "cabinet file (MSZIP)",
      dmg: "disk image (UDZO)",
      wim: "Windows imaging format (LZX)",
      vhd: "Virtual PC disk (VHD)",
      vhdx: "Hyper-V disk (VHDX)",
      vmdk: "VMware disk (VMDK)",
    };
    pairs.push({ k: "format", v: labels[ext] ?? "disk image" });
  } else if (FONT_EXTS.has(ext)) {
    const formats: Record<string, string> = {
      ttf: "TrueType font",
      otf: "OpenType (CFF)",
      woff: "WOFF web font",
      woff2: "WOFF2 web font",
      ttc: "TrueType collection",
    };
    pairs.push({ k: "format", v: formats[ext] ?? "font" });
    pairs.push({ k: "family", v: pick(rand, ["Noto Sans", "Source Serif 4", "JetBrains Mono", "IBM Plex Sans"]) });
    pairs.push({ k: "style", v: pick(rand, ["Regular", "Bold", "SemiBold", "Italic"]) });
    pairs.push({ k: "glyphs", v: intOf(rand, 220, 2900).toLocaleString("en-US") });
    pairs.push({ k: "tables", v: String(intOf(rand, 12, 24)) });
  } else if (SCI_EXTS.has(ext)) {
    const formats: Record<string, string> = {
      h5: "HDF5",
      hdf5: "HDF5",
      nc: "NetCDF-4",
      fits: "FITS image",
      dcm: "DICOM",
      npy: "NumPy array",
      parquet: "Parquet (snappy)",
      mat: "MATLAB v7.3 (HDF5)",
    };
    pairs.push({ k: "format", v: formats[ext] ?? "scientific data" });
    pairs.push({ k: "objects", v: String(intOf(rand, 3, 260)) });
    pairs.push({ k: "byte order", v: pick(rand, ["little-endian", "big-endian"]) });
  } else if (ENC_EXTS.has(ext)) {
    const formats: Record<string, string> = {
      gpg: "OpenPGP message",
      pgp: "OpenPGP message",
      age: "age-encrypted file",
      enc: "encrypted container",
      vault: "encrypted vault",
    };
    pairs.push({ k: "format", v: formats[ext] ?? "encrypted data" });
    pairs.push({ k: "cipher", v: pick(rand, ["AES-256", "X25519 + ChaCha20-Poly1305", "RSA-4096"]) });
  } else if (KEYSTORE_EXTS.has(ext)) {
    const formats: Record<string, string> = {
      der: "DER certificate",
      pfx: "PKCS#12 keystore",
      p12: "PKCS#12 keystore",
      jks: "Java keystore",
    };
    pairs.push({ k: "format", v: formats[ext] ?? "keystore" });
    pairs.push({ k: "subject", v: "CN=*.example.com" });
    pairs.push({ k: "valid until", v: `202${intOf(rand, 6, 9)}-0${intOf(rand, 1, 9)}-1${intOf(rand, 0, 9)}` });
  } else if (MAILBOX_EXTS.has(ext)) {
    pairs.push({ k: "format", v: ext === "msg" ? "Outlook message (OLE compound)" : "Outlook storage (Unicode PST)" });
    pairs.push({ k: "messages", v: intOf(rand, 400, 24000).toLocaleString("en-US") });
    pairs.push({ k: "largest folder", v: pick(rand, ["Inbox", "Sent Items", "Archive"]) });
  } else if (PRESENT_EXTS.has(ext)) {
    pairs.push({ k: "format", v: "presentation (OOXML)" });
    pairs.push({ k: "slides", v: String(intOf(rand, 6, 80)) });
    pairs.push({ k: "notes pages", v: String(intOf(rand, 4, 60)) });
  } else if (BACKUP_EXTS.has(ext)) {
    pairs.push({ k: "format", v: pick(rand, ["disk backup image", "incremental backup", "database snapshot"]) });
    pairs.push({ k: "created", v: mtimeOf(seed ^ 0x27d4eb2f) });
  } else if (ext === "ai") {
    pairs.push({ k: "format", v: "Adobe Illustrator artwork (PDF-compatible)" });
    pairs.push({ k: "version", v: `Illustrator ${intOf(rand, 24, 29)}` });
    pairs.push({ k: "artboards", v: String(intOf(rand, 1, 12)) });
  } else if (PKG_EXTS.has(ext)) {
    const labels: Record<string, string> = {
      deb: "Debian binary package (ar)",
      rpm: "RPM package",
      apk: "Android package (ZIP, deflate)",
      jar: "Java archive (ZIP, deflate)",
    };
    pairs.push({ k: "format", v: labels[ext] ?? "package" });
    pairs.push({ k: "entries", v: intOf(rand, 8, 4000).toLocaleString("en-US") });
    if (ext === "jar") pairs.push({ k: "main class", v: `${stem || "Main"}.Main` });
  } else {
    // Unknown extension — say so plainly; the plate renders "unknown" honestly.
    pairs.push({ k: "format", v: "unknown" });
  }
  // Universal tail: every META plate carries the same honest readout.
  pairs.push({ k: "size", v: size > 0 ? formatBytes(size) : "unknown" });
  pairs.push({ k: "modified", v: mtimeOf(seed) });
  pairs.push({
    k: "signature",
    v:
      SIGNATURES[ext] ??
      Array.from({ length: 4 }, () => Math.floor(rand() * 256).toString(16).toUpperCase().padStart(2, "0")).join(" "),
  });
  const payload: PreviewPayload = { family: "meta", meta: { pairs } };
  // Executables carry a hex head alongside the readout — model.rs: "except
  // that executables carry `meta` + a `hex` head together (§A 'META + HEX
  // head')". The bytes are the same stamped MZ/ELF head the HEX family gets,
  // so a plate that annotates "DOS header" finds real data either way.
  if (PE_EXTS.has(ext) || ELF_EXTS.has(ext)) {
    const head = hexPayload(path, ext, seed, node);
    if (head.hex) payload.hex = head.hex;
  }
  return payload;
}

function listPayload(path: string, ext: string, seed: number, node: DNode | undefined): PreviewPayload {
  const rand = rngFrom(seed);
  const stem = stemOf(path) || "archive";
  const total = node?.size ?? 0;
  // Row kind is the real "dir" | "file" union (preview.rs ListEntry); the
  // optional compression method rides zip/tar FILE rows only.
  const entries: { name: string; size?: number; kind: PreviewEntryKind; method?: string }[] = [];
  if (ext === "xlsx" || ext === "ods") {
    const names = pick(rand, [
      ["Summary", "Q1 revenue", "Pipeline", "Raw export", "Notes"],
      ["Inventory", "Suppliers", "Invoices 2026", "Summary"],
      ["Sheet1", "Data", "Charts"],
    ]);
    for (const nm of names) entries.push({ name: nm, kind: "file" });
  } else if (ext === "epub") {
    // A zip like any other: the spec requires `mimetype` stored
    // uncompressed, the XML members deflate.
    entries.push({ name: "mimetype", size: 20, kind: "file", method: "Store" });
    entries.push({ name: "META-INF/container.xml", size: 364, kind: "file", method: "Deflate" });
    const chapters = intOf(rand, 6, 14);
    for (let i = 1; i <= chapters; i++) {
      entries.push({
        name: `OEBPS/text/chapter-${String(i).padStart(2, "0")}.xhtml`,
        size: intOf(rand, 4096, 24000),
        kind: "file",
        method: "Deflate",
      });
    }
    entries.push({ name: "OEBPS/toc.ncx", size: intOf(rand, 2048, 8000), kind: "file", method: "Deflate" });
  } else if (ext === "iso" || ext === "img") {
    entries.push({ name: "[BOOT]", kind: "dir" });
    entries.push({ name: "boot", kind: "dir" });
    entries.push({ name: "casper", kind: "dir" });
    entries.push({ name: "efi", kind: "dir" });
    entries.push({ name: "boot/grub/grub.cfg", size: intOf(rand, 2048, 6144), kind: "file" });
    entries.push({ name: "casper/vmlinuz", size: intOf(rand, 8, 14) * MB, kind: "file" });
    entries.push({ name: "casper/initrd", size: intOf(rand, 60, 120) * MB, kind: "file" });
    entries.push({ name: "casper/filesystem.squashfs", size: Math.round(total * 0.86) || 1.4 * GB, kind: "file" });
    entries.push({ name: "efi/boot/bootx64.efi", size: intOf(rand, 96, 256) * KB, kind: "file" });
    entries.push({ name: "md5sum.txt", size: intOf(rand, 20480, 90000), kind: "file" });
  } else {
    // zip / tar — central directory or sequential walk only (§A). Tar
    // members are always "Store"; zip members deflate unless the payload
    // shipped already compressed (preview.rs zip_method).
    const n = intOf(rand, 40, 90);
    const sizes = scaledSizes(rand, n, total || 3 * MB);
    const isTar = ext === "tar";
    const pushFile = (name: string, size: number): void => {
      entries.push({
        name,
        size,
        kind: "file",
        method: isTar ? "Store" : ZIP_STORE_EXTS.has(extOf(name)) ? "Store" : "Deflate",
      });
    };
    if (/photo|shoot|img|snap|wallpaper/i.test(stem)) {
      for (let i = 0; i < n; i++) {
        pushFile(`${stem}/IMG_${String(401 + i).padStart(4, "0")}.jpg`, sizes[i]);
      }
    } else {
      const dirs = ["src", "src/lib", "assets", "docs", "data", ""];
      const bases = ["index", "scan", "render", "manifest", "notes", "logo", "export"];
      const exts = [".ts", ".md", ".png", ".csv", ".json"];
      for (let i = 0; i < n; i++) {
        const dir = pick(rand, dirs);
        const base = `${pick(rand, bases)}_${i}${pick(rand, exts)}`;
        pushFile(dir ? `${dir}/${base}` : base, sizes[i]);
      }
    }
    entries.sort((a, b) => (b.size ?? 0) - (a.size ?? 0));
  }
  // Listing source (preview.rs enumerators): the word names what produced
  // the rows — the plate keys its section label and columns off it.
  const kind: PreviewListKind =
    ext === "xlsx" || ext === "ods"
      ? "sheet"
      : ext === "tar"
        ? "tar"
        : ext === "iso" || ext === "img"
          ? "iso"
          : "zip";
  return { family: "list", list: { entries, truncated: false, total: entries.length, kind } };
}

function dbPayload(seed: number): PreviewPayload {
  const rand = rngFrom(seed);
  // Mirror preview.rs: tables in sqlite_master ORDER (alphabetical), `cols`
  // from pragma names in declaration order, and sampled rows for the FIRST
  // THREE tables only — never a row COUNT(*).
  const schemas = pick(rand, [
    [
      { name: "categories", columns: ["id", "label", "hex", "bytes"] },
      { name: "entries", columns: ["id", "path", "size", "cat", "scanned_at"] },
      { name: "nodes", columns: ["id", "parent", "name", "size", "is_dir", "payload"] },
      { name: "scan_runs", columns: ["id", "root", "files", "dirs", "errors", "elapsed_ms", "finished_at"] },
      { name: "settings", columns: ["key", "value", "updated_at"] },
    ],
    [
      { name: "albums", columns: ["id", "title", "artist_id", "added_at"] },
      { name: "artists", columns: ["id", "name"] },
      { name: "library_meta", columns: ["key", "value"] },
      { name: "playlists", columns: ["id", "name", "track_count", "cover"] },
      { name: "tracks", columns: ["id", "path", "title", "duration_ms", "bytes", "art"] },
    ],
    [
      { name: "audit_log", columns: ["id", "actor", "action", "at", "detail"] },
      { name: "events", columns: ["id", "kind", "at", "payload"] },
      { name: "files", columns: ["id", "path", "bytes", "hash"] },
      { name: "sessions", columns: ["id", "started_at", "ended_at", "remote"] },
      { name: "users", columns: ["id", "name", "created_at", "last_seen"] },
    ],
  ]);
  const tables: { name: string; cols: number; columns?: string[]; rows?: string[][] }[] = [];
  schemas.forEach((t, i) => {
    const table: (typeof tables)[number] = { name: t.name, cols: t.columns.length, columns: [...t.columns] };
    // DB_ROWS_TABLES = 3: leading tables get bounded sample rows, the rest
    // ship the schema card alone.
    if (i < 3) table.rows = dbSampleRows(rand, t.name, t.columns);
    tables.push(table);
  });
  return { family: "db", db: { tables, truncated: false } };
}

/** ≤5 sample rows as display text — the same cell rules as preview.rs
 *  db_cell: integers raw, text scrubbed and short, NULLs the literal
 *  "null", blobs a byte-size stub, never raw bytes. */
function dbSampleRows(rand: Rng, table: string, columns: string[]): string[][] {
  const paths = [
    "D:\\clips\\raw-footage\\take_01.mp4",
    "D:\\Windows\\System32\\ntoskrnl.exe",
    "D:\\Users\\casey\\Downloads\\ubuntu-24.04.iso",
    "D:\\Program Files\\Blender\\blender.exe",
  ];
  const names = ["casey", "scanner", "scheduler", "take_01", "blender", "ntoskrnl"];
  const words = ["ok", "pending", "indexed", "skipped", "warn"];
  const t0 = seedOf(table);
  const rows: string[][] = [];
  for (let r = 0, n = intOf(rand, 2, 5); r < n; r++) {
    const cells: string[] = [];
    for (const c of columns) {
      // A sprinkling of real NULLs — the plate prints the word, never a blank.
      if (/(^parent$|^ended_at$|_ended_at$|^remote$|^detail$|^cover$|^art$|^hash$)/.test(c) && (r + c.length) % 3 === 0) {
        cells.push("null");
      } else if (c === "id") {
        cells.push(String(1001 + r));
      } else if (/_at$/.test(c) || c === "at") {
        cells.push(`${mtimeOf(t0 + r).replace(" ", "T")}:00Z`);
      } else if (c === "path" || c === "root") {
        cells.push(pick(rand, paths));
      } else if (/(^size$|^bytes$|elapsed_ms|duration_ms|track_count|^files$|^dirs$|^errors$)/.test(c)) {
        cells.push(String(intOf(rand, 1, 900000000)));
      } else if (c === "cat") {
        cells.push(String(intOf(rand, 0, 7)));
      } else if (c === "is_dir") {
        cells.push(rand() < 0.3 ? "1" : "0");
      } else if (/(payload|cover|art|hash|blob|data)/.test(c)) {
        cells.push(`<blob ${intOf(rand, 16, 65536)} B>`);
      } else if (/(^name$|^title$|^label$|^actor$)/.test(c)) {
        cells.push(pick(rand, names));
      } else {
        cells.push(pick(rand, words));
      }
    }
    rows.push(cells);
  }
  return rows;
}

/** One payload per path — the DemoState.getPreview workhorse (§B demo fallback). */
function synthPreview(path: string): PreviewPayload {
  const node = BY_PATH.get(path);
  if (node?.isDir) return folderList(node);
  // A directory the demo tree doesn't model (trailing separator) still gets an
  // honest empty listing rather than invented entries.
  if (!node && /[\\/]$/.test(path)) {
    return { family: "list", list: { entries: [], truncated: false, total: 0 } };
  }
  const ext = extOf(path);
  const seed = seedOf(path);
  // Locked paging files preview as the live app's degraded card (§B
  // degraded_meta): never a hex dump of bytes the OS would not let us read.
  if (/^(pagefile|swapfile|hiberfil)\./i.test(nameOf(path))) {
    return metaPayload(path, ext, seed, node);
  }
  switch (familyFor(ext)) {
    case "text":
      return textPayload(path, ext, seed, node);
    case "hex":
      return hexPayload(path, ext, seed, node);
    case "image":
      return imagePayload(path, ext, seed, node);
    case "list":
      return listPayload(path, ext, seed, node);
    case "db":
      return dbPayload(seed);
    default:
      return metaPayload(path, ext, seed, node);
  }
}

const CODE_CLIKE_EXTS = new Set("c cpp h hpp cc".split(" "));

/** Extension → text generator (§A TEXT group). Real syntax, never lorem. */
function genText(ext: string, count: number, rand: Rng, stem: string): string[] {
  if (ext === "json" || ext === "gltf") return genJson(count, rand, stem);
  if (ext === "csv") return genCsv(count, rand);
  if (ext === "yaml" || ext === "yml") return genYaml(count, rand, stem);
  if (ext === "toml") return genToml(count, rand, stem);
  if (ext === "ini" || ext === "cfg" || ext === "conf" || ext === "env" || ext === "properties") {
    return genIni(count, rand, stem);
  }
  if (ext === "reg") return genReg(count);
  if (ext === "plist") return genPlist(count, rand);
  if (ext === "log" || ext === "logs" || ext === "tlg" || ext === "out") return genLog(count, rand);
  if (ext === "md") return genMd(count, rand, stem);
  if (ext === "rtf") return genRtf(count, rand, stem);
  if (ext === "m3u" || ext === "m3u8") return genM3u(count, rand);
  if (ext === "vtt" || ext === "srt" || ext === "ass" || ext === "ssa") return genSubs(count, rand);
  if (ext === "eml" || ext === "mbox") return genEml(count, rand, stem);
  if (ext === "docx" || ext === "odt") return genWordXml(count, rand, stem);
  if (ext === "xml") return genConfigXml(count, rand, stem);
  if (ext === "pem" || ext === "crt" || ext === "cer" || ext === "asc" || ext === "key" || ext === "pub") {
    return genPem(count, rand, stem);
  }
  if (ext === "svg") return genSvg(count, rand, stem);
  if (ext === "obj" || ext === "stl" || ext === "ply" || ext === "dxf" || ext === "step" || ext === "iges" || ext === "eps") {
    return genCadAscii(count, rand, ext, stem);
  }
  if (ext === "ts" || ext === "tsx" || ext === "js" || ext === "jsx" || ext === "mjs" || ext === "cjs" || ext === "vue") {
    return genCodeJs(count, rand, stem);
  }
  if (ext === "py") return genCodePy(count, rand, stem);
  if (ext === "rs") return genCodeRs(count, rand, stem);
  if (ext === "go") return genCodeGo(count, rand, stem);
  if (ext === "java") return genCodeJava(count, rand, stem);
  if (ext === "sh") return genScriptSh(count, rand);
  if (ext === "ps1") return genScriptPs1(count, rand);
  if (ext === "bat") return genScriptBat(count, rand);
  if (ext === "sql") return genSql(count, rand, stem);
  if (CODE_CLIKE_EXTS.has(ext)) return genCodeC(count, rand, stem);
  return genNotes(count, rand);
}

function genJson(count: number, rand: Rng, stem: string): string[] {
  // Balanced, actual-shaped JSON: head + generated records + tail, clamped so
  // the array always closes exactly at `count` lines.
  const head = [
    `{`,
    `  "name": "${stem.replace(/\s+/g, "-").toLowerCase()}",`,
    `  "version": "2.${intOf(rand, 0, 9)}.${intOf(rand, 0, 9)}",`,
    `  "private": true,`,
    `  "scripts": {`,
    `    "dev": "vite",`,
    `    "build": "tsc -b && vite build",`,
    `    "preview": "vite preview"`,
    `  },`,
    `  "items": [`,
  ];
  const tail = [
    `  ],`,
    `  "meta": { "count": ${intOf(rand, 800, 9999)}, "generated": "${mtimeOf(Math.floor(rand() * 0xffffffff))}" }`,
    `}`,
  ];
  const items = ["install.wim", "index.dat", "capture_0417.jpg", "notes.md", "export.csv", "shader.cache", "thumb.png"];
  const states = ["ready", "pending", "indexed"];
  const n = Math.max(0, count - head.length - tail.length);
  const mid: string[] = [];
  for (let i = 0; i < n; i++) {
    mid.push(
      `    { "id": ${1001 + i}, "name": "${pick(rand, items)}", "bytes": ${intOf(rand, 4096, 900000)}, "state": "${pick(rand, states)}" }${i + 1 < n ? "," : ""}`,
    );
  }
  return [...head, ...mid, ...tail];
}

function genCsv(count: number, rand: Rng): string[] {
  const out = [`timestamp,host,dir,files,bytes,elapsed_ms`];
  const hosts = ["d-workstation", "d-nas", "e-backup"];
  const dirs = ["clips", "Windows", "Users/casey", "Program Files", "Recovery"];
  while (out.length < count) {
    const d = new Date(Date.UTC(2026, 2, 4) - intOf(rand, 0, 960) * 3600000);
    const stamp = `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())}T${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())}:${pad2(d.getUTCSeconds())}Z`;
    out.push(
      `${stamp},${pick(rand, hosts)},${pick(rand, dirs)},${intOf(rand, 120, 99000)},${intOf(rand, 4096, 900000000)},${intOf(rand, 3, 2400)}`,
    );
  }
  return out;
}

function genYaml(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `name: ${stem.replace(/\s+/g, "-").toLowerCase()}`,
    `version: 2.${intOf(rand, 0, 9)}.${intOf(rand, 0, 9)}`,
    ``,
    `scan:`,
    `  roots:`,
    `    - D:\\`,
    `    - E:\\Backups`,
    `  depth: ${intOf(rand, 2, 4)}`,
    `  follow_symlinks: false`,
    ``,
    `limits:`,
    `  max_entries: ${pick(rand, [900, 1500, 2400])}`,
    `  preview_text_bytes: 65536`,
    `  preview_hex_bytes: 4096`,
    `  scan_threads: ${pick(rand, [4, 8, 12])}`,
    ``,
    `recent:`,
  ];
  const body = [
    `  - path: D:\\clips`,
    `    visited: 2026-03-04T11:20:14Z`,
    `    entries: 56`,
    `  - path: D:\\Users\\casey\\Downloads`,
    `    visited: 2026-03-03T18:42:07Z`,
    `    entries: 20`,
  ];
  return fill(count, head, body);
}

function genToml(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `title = "${stem}"`,
    `version = "2.${intOf(rand, 0, 9)}.${intOf(rand, 0, 9)}"`,
    ``,
    `[scan]`,
    `roots = ["D:\\\\", "E:\\\\Backups"]`,
    `depth = ${intOf(rand, 2, 4)}`,
    `threads = ${pick(rand, [4, 8, 12])}`,
    ``,
    `[preview]`,
    `text_bytes = 65536`,
    `hex_bytes = 4096`,
    `image_max_edge = 256`,
    ``,
    `[[recent]]`,
  ];
  const body = [
    `path = "D:\\\\clips"`,
    `visited = 2026-03-04T11:20:14Z`,
    `entries = 56`,
    ``,
    `[[recent]]`,
    `path = "D:\\\\Windows"`,
    `visited = 2026-03-02T09:12:41Z`,
    `entries = 188`,
  ];
  return fill(count, head, body);
}

function genIni(count: number, rand: Rng, stem: string): string[] {
  type IniPair = readonly [string, () => string];
  const sections: readonly (readonly [string, readonly IniPair[]])[] = [
    [
      "application",
      [
        ["name", () => stem],
        ["environment", () => pick(rand, ["production", "staging", "dev"])],
        ["log_level", () => pick(rand, ["info", "debug", "warn"])],
        ["check_updates", () => pick(rand, ["true", "false"])],
      ],
    ],
    [
      "storage",
      [
        ["root", () => pick(rand, ["D:\\", "D:\\clips", "D:\\Users\\casey"])],
        ["cache_dir", () => "C:\\Users\\casey\\AppData\\Local\\space\\cache"],
        ["cache_mb", () => String(pick(rand, [256, 512, 1024]))],
        ["follow_symlinks", () => "false"],
      ],
    ],
    [
      "limits",
      [
        ["max_entries", () => String(pick(rand, [900, 1500, 2400]))],
        ["scan_threads", () => String(pick(rand, [4, 8, 12]))],
        ["preview_bytes", () => pick(rand, ["65536", "262144", "1048576"])],
        ["timeout_ms", () => pick(rand, ["2000", "5000", "10000"])],
      ],
    ],
    [
      "logging",
      [
        ["file", () => "space.log"],
        ["rotate_mb", () => String(pick(rand, [5, 10, 25]))],
        ["retain_days", () => String(pick(rand, [7, 14, 30]))],
      ],
    ],
  ];
  const out: string[] = [];
  let si = 0;
  let ki = 0;
  while (out.length < count) {
    const [sec, keys] = sections[si % sections.length];
    if (ki === 0) {
      if (out.length > 0) {
        out.push("");
        if (rand() < 0.4) out.push(`; last synced ${mtimeOf(seedOf(stem) + si)}`);
      }
      out.push(`[${sec}]`);
    }
    const [k, v] = keys[ki % keys.length];
    out.push(`${k} = ${v()}`);
    ki++;
    if (ki % keys.length === 0) si++;
  }
  return out.slice(0, count);
}

function genReg(count: number): string[] {
  const head = [
    `Windows Registry Editor Version 5.00`,
    ``,
    `[HKEY_CURRENT_USER\\Software\\space]`,
    `"LastRoot"="D:\\\\"`,
    `"ScanDepth"=dword:00000002`,
  ];
  const body = [
    ``,
    `[HKEY_CURRENT_USER\\Software\\space\\Limits]`,
    `"MaxEntries"=dword:00000384`,
    `"TextBytes"=dword:00010000`,
    `"HexBytes"=dword:00001000`,
    ``,
    `[HKEY_CURRENT_USER\\Software\\space\\Recent]`,
    `"0"="D:\\\\clips"`,
    `"1"="D:\\\\Windows"`,
    `"2"="D:\\\\Users\\\\casey"`,
  ];
  return fill(count, head, body);
}

function genPlist(count: number, rand: Rng): string[] {
  const head = [
    `<?xml version="1.0" encoding="UTF-8"?>`,
    `<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">`,
    `<plist version="1.0">`,
    `<dict>`,
    `  <key>Label</key>`,
    `  <string>com.example.space.scan</string>`,
    `  <key>ProgramArguments</key>`,
    `  <array>`,
    `    <string>/usr/local/bin/space</string>`,
    `    <string>--watch</string>`,
    `  </array>`,
  ];
  const body = [
    `  <key>StartInterval</key>`,
    `  <integer>${pick(rand, [60, 300, 900])}</integer>`,
    `  <key>RunAtLoad</key>`,
    `  <true/>`,
    `  <key>StandardOutPath</key>`,
    `  <string>/tmp/space.log</string>`,
    `  <key>LowPriorityIO</key>`,
    `  <true/>`,
  ];
  const out = fill(count, head, body);
  // Close the plist on the last two lines when there is room.
  if (count - 2 >= head.length) {
    out[count - 2] = `</dict>`;
    out[count - 1] = `</plist>`;
  }
  return out;
}

function genLog(count: number, rand: Rng): string[] {
  // Timestamped engine log: levels weighted the way real logs are (mostly
  // INFO, some WARN, rare ERROR), clock advancing 5–400 ms per line.
  let ms = Date.UTC(2026, 2, 4, 11, 20, 14) + intOf(rand, 0, 4000);
  const targets = ["engine::commit", "scanner::walk", "cache::lru", "preview::decode", "classify::rules", "fs::read"];
  const out: string[] = [];
  for (let i = 0; i < count; i++) {
    ms += intOf(rand, 5, 400);
    const d = new Date(ms);
    const stamp = `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())} ${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())}:${pad2(d.getUTCSeconds())}.${String(d.getUTCMilliseconds()).padStart(3, "0")}`;
    const r = rand();
    let level: string;
    let msg: string;
    if (r > 0.985) {
      level = "ERROR";
      msg = `read failed: D:\\Users\\casey\\AppData\\Local\\Temp\\part_${intOf(rand, 100, 999)} (os error ${intOf(rand, 2, 32)})`;
    } else if (r > 0.94) {
      level = "WARN ";
      msg = `slow query ${intOf(rand, 520, 2400)}ms (threshold 500ms)`;
    } else if (r > 0.6) {
      level = "INFO ";
      msg = `batch ${intOf(rand, 1000, 9999)} committed in ${intOf(rand, 8, 90)}ms (${intOf(rand, 2000, 40000).toLocaleString("en-US")} entries)`;
    } else {
      level = "DEBUG";
      msg = `readdir D:\\clips\\${pick(rand, ["raw-footage", "b-roll", "portfolio", "audio"])} -> ${intOf(rand, 3, 64)} entries`;
    }
    out.push(`${stamp}  ${level} ${targets[i % targets.length].padEnd(15)} ${msg}`);
  }
  return out;
}

function genMd(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `# ${titleOf(stem)}`,
    ``,
    `Scan notes for \`${stem}\` — last reviewed ${mtimeOf(seedOf(stem))}.`,
    ``,
    `## Summary`,
    ``,
  ];
  const body = [
    `- ${intOf(rand, 12, 90)} files indexed, ${formatBytes(intOf(rand, 4 * MB, 900 * MB))} total`,
    `- largest: take_01.mp4 (12.4 MB)`,
    `- category split: media 62%, apps 18%, caches 11%, other 9%`,
    `- duplicate photoshoot.zip copy found under E:\\Backups`,
    ``,
    `## Checklist`,
    ``,
    `- [x] triage Downloads (${intOf(rand, 12, 24)} installers)`,
    `- [ ] re-encode blender_render.mp4 to H.265`,
    `- [ ] verify spider/ backup completeness`,
    `- [x] move ubuntu-24.04.iso off D:`,
    ``,
    `## Notes`,
    ``,
    `WinSxS is the usual suspect for the "missing" 40 GB — leave it to DISM`,
    `unless the drive actually fills up.`,
    ``,
    `The duplicate check ran over ${intOf(rand, 4000, 20000).toLocaleString("en-US")} paths; the only`,
    `true duplicates were the two photoshoot.zip copies (3.4 MB each).`,
  ];
  return fill(count, head, body);
}

function genRtf(count: number, _rand: Rng, stem: string): string[] {
  const head = [
    `{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fswiss Helvetica;}}`,
    `{\\colortbl;\\red31\\green41\\blue51;}`,
    `\\paperw12240\\paperh15840\\margl1440\\margr1440`,
    `\\f0\\fs24\\cf1 `,
  ];
  const body = [
    `\\b ${titleOf(stem)}\\b0\\par`,
    `\\par Drive D: cleanup notes, continued from the last scan.\\par`,
    `\\par \\b Downloads\\b0  18 installers removed (4.2 GB); kept the Ubuntu ISO.\\par`,
    `\\par \\b Videos\\b0  blender_render.mp4 re-encode pending (780 MB).\\par`,
    `\\par \\b WinSxS\\b0  deferred — needs a reboot window, ask before DISM.\\par`,
    `\\par Duplicate check: both photoshoot.zip copies verified.\\par`,
    `\\par Next review: after the E: backup rotation.\\par`,
  ];
  return fill(count, head, body);
}

function genM3u(count: number, rand: Rng): string[] {
  const artists = ["Northbound", "Mira Vale", "Cassette Hours", "Low Tide Choir", "Paper Kites"];
  const titles = ["Static Lines", "Paper Radio", "Harbour Lights", "Winter Frequency", "Slow Ferry", "Dust and Signal"];
  const out = [`#EXTM3U`];
  let i = 0;
  while (out.length + 2 <= count) {
    out.push(`#EXTINF:${intOf(rand, 148, 412)},${pick(rand, artists)} - ${pick(rand, titles)}`);
    out.push(`D:\\Music\\${pick(rand, artists)}\\${String(i + 1).padStart(2, "0")} - ${pick(rand, titles)}.mp3`);
    i++;
  }
  while (out.length < count) out.push(`D:\\Music\\singles\\track_${intOf(rand, 10, 99)}.mp3`);
  return out.slice(0, count);
}

function genSubs(count: number, rand: Rng): string[] {
  const lines = [
    "[rain on the window]",
    "You said the drive was empty.",
    "It was. Last night.",
    "[keyboard, then a pause]",
    "1.8 terabytes. Where does it even sit?",
    "[chair creaks]",
    "Mostly in WinSxS. Ask DISM.",
    "[laughter]",
    "And the photoshoot copies?",
    "Both. Twice on D:, once on E:.",
  ];
  const stamp = (x: number) => `${pad2(Math.floor(x / 60))}:${pad2(x % 60)},${String(intOf(rand, 0, 999)).padStart(3, "0")}`;
  const out: string[] = [];
  let t = intOf(rand, 0, 90);
  let idx = 1;
  if (rand() < 0.5) out.push("WEBVTT");
  while (out.length < count) {
    const b = t + intOf(rand, 14, 42);
    out.push(String(idx));
    out.push(`${stamp(t)} --> ${stamp(b)}`);
    out.push(lines[(idx - 1) % lines.length]);
    out.push("");
    t = b + intOf(rand, 8, 60);
    idx++;
  }
  return out.slice(0, count);
}

function genEml(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `From: casey@example.com`,
    `To: space-dev@example.com`,
    `Subject: Re: ${titleOf(stem)}`,
    `Date: ${pick(rand, ["Fri, 13 Feb 2026 18:42:07 +0000", "Mon, 02 Mar 2026 09:15:44 +0000", "Thu, 26 Feb 2026 21:03:12 +0000"])}`,
    `Message-ID: <20260213184207.9f2c1@example.com>`,
    `MIME-Version: 1.0`,
    `Content-Type: text/plain; charset=utf-8`,
    `Content-Transfer-Encoding: 7bit`,
    ``,
  ];
  const body = [
    `The immutable-snapshot change cut p95 get_children from 84 ms to 3 ms on`,
    `the 1.8 TB library. Holding the read guard only long enough to clone the`,
    `Arc was the whole trick — every get_* call used to sort under the global`,
    `mutex.`,
    ``,
    `Remaining before the wave closes:`,
    `  - per-directory classify flags (the per-file lowercase pass is silly)`,
    `  - name-sort keys extracted once per node instead of per comparison`,
    `  - category totals persisted into ScanData so finish_scan is O(1)`,
    ``,
    `Benchmark attached for D: (1.86 TB, 413 files at root).`,
  ];
  return fill(count, head, body);
}

function genWordXml(count: number, rand: Rng, stem: string): string[] {
  // docx/odt are zips of XML — the TEXT preview is the document XML itself
  // (§A: "docx odt → TEXT (zip/xml)"), so the lines are real wordprocessing
  // markup with plausible text runs.
  const title = titleOf(stem);
  const head = [
    `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`,
    `<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">`,
    `  <w:body>`,
    `    <w:p><w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>${title}</w:t></w:r></w:p>`,
  ];
  const body = [
    `    <w:p><w:r><w:t>Email: casey@example.com</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Experience</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>2023–2026  Systems engineer, storage tooling — built a real-time</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>treemap renderer for multi-terabyte disk scans (Tauri, WebGL).</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>2021–2023  Backend engineer — scanner throughput 4× on cold caches.</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Skills</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>Rust, TypeScript, SQLite, profiling, storage forensics.</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Notes</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>Reviewed against the ${pick(rand, ["Q1", "Q2", "H1"])} headcount plan; two</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:t>references attached separately, page break below.</w:t></w:r></w:p>`,
    `    <w:p><w:r><w:br w:type="page"/></w:r></w:p>`,
  ];
  return fill(count, head, body);
}

function genConfigXml(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `<?xml version="1.0" encoding="UTF-8"?>`,
    `<config name="${stem.replace(/\s+/g, "-").toLowerCase()}">`,
    `  <scan root="D:\\" depth="${intOf(rand, 2, 4)}" threads="${pick(rand, [4, 8, 12])}"/>`,
    `  <limits maxEntries="${pick(rand, [900, 1500, 2400])}" textBytes="65536" hexBytes="4096"/>`,
  ];
  const body = [
    `  <exclude pattern="**/AppData/Local/Temp"/>`,
    `  <exclude pattern="**/.cache"/>`,
    `  <category name="media" extensions="jpg png mp4 mkv wav flac"/>`,
    `  <category name="code" extensions="rs ts js py go java"/>`,
    `  <category name="archives" extensions="zip 7z tar gz iso"/>`,
    `  <recent path="D:\\clips" visited="2026-03-04T11:20:14Z"/>`,
    `  <recent path="D:\\Windows" visited="2026-03-02T09:12:41Z"/>`,
  ];
  const out = fill(count, head, body);
  if (count > head.length) out[count - 1] = `</config>`; // close the root tag
  return out;
}

function genPem(count: number, rand: Rng, stem: string): string[] {
  // Real-shaped base64 armor: 64-char lines, honest BEGIN/END markers.
  const kind = /key/i.test(stem) ? "PRIVATE KEY" : "CERTIFICATE";
  const b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const body: string[] = [];
  for (let i = 0; i < 80; i++) {
    let line = "";
    for (let j = 0; j < 64; j++) line += b64[Math.floor(rand() * b64.length)];
    body.push(line);
  }
  const out = fill(count, [`-----BEGIN ${kind}-----`], body);
  if (count > 1) out[count - 1] = `-----END ${kind}-----`;
  return out;
}

function genSvg(count: number, _rand: Rng, stem: string): string[] {
  const head = [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48" width="48" height="48" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">`,
    `  <title>${titleOf(stem)}</title>`,
  ];
  const body = [
    `  <circle cx="24" cy="24" r="21"/>`,
    `  <path d="M8 24h12l4-8 4 16 4-8h8"/>`,
    `  <rect x="10" y="10" width="28" height="28" rx="2"/>`,
    `  <path d="M14 34V14l20 20V14"/>`,
    `  <path d="M24 3v6M24 39v6M3 24h6M39 24h6"/>`,
    `  <polyline points="6,34 14,26 22,30 30,18 42,22"/>`,
  ];
  return fill(count, head, body);
}

function genCadAscii(count: number, rand: Rng, ext: string, stem: string): string[] {
  // §A routes several ascii 3D/CAD formats to TEXT — each gets its own real
  // header and record shape, never a shared placeholder.
  const f = (x: number) => x.toFixed(4);
  if (ext === "obj") {
    const out = [`# ${titleOf(stem)}`, `mtllib ${stem}.mtl`, `o ${stem}`];
    let i = 0;
    while (out.length < count) {
      const body = [
        `v ${f(rand() * 40 - 20)} ${f(rand() * 40 - 20)} ${f(rand() * 12)}`,
        `vn ${f(rand() * 2 - 1)} ${f(rand() * 2 - 1)} ${f(rand() * 2 - 1)}`,
        `vt ${f(rand())} ${f(rand())}`,
        `f ${i + 1}/${i + 1}/${i + 1} ${i + 2}/${i + 2}/${i + 2} ${i + 3}/${i + 3}/${i + 3}`,
      ];
      for (const line of body) {
        if (out.length >= count) break;
        out.push(line);
      }
      i += 3;
    }
    return out.slice(0, count);
  }
  if (ext === "stl") {
    const out = [`solid ${stem}`];
    while (out.length + 1 < count) {
      const body = [
        `  facet normal ${f(rand() * 2 - 1)} ${f(rand() * 2 - 1)} ${f(rand() * 2 - 1)}`,
        `    outer loop`,
        `      vertex ${f(rand() * 40)} ${f(rand() * 40)} ${f(rand() * 12)}`,
        `      vertex ${f(rand() * 40)} ${f(rand() * 40)} ${f(rand() * 12)}`,
        `      vertex ${f(rand() * 40)} ${f(rand() * 40)} ${f(rand() * 12)}`,
        `    endloop`,
        `  endfacet`,
      ];
      for (const line of body) {
        if (out.length + 1 >= count) break;
        out.push(line);
      }
    }
    if (count > 1) out[count - 1] = `endsolid ${stem}`;
    return out.slice(0, count);
  }
  if (ext === "ply") {
    const verts = Math.max(4, Math.floor(count * 0.7));
    const head = [
      `ply`,
      `format ascii 1.0`,
      `comment created ${mtimeOf(seedOf(stem))}`,
      `element vertex ${verts}`,
      `property float x`,
      `property float y`,
      `property float z`,
      `element face ${Math.max(1, count - verts - 10)}`,
      `property list uchar int vertex_indices`,
      `end_header`,
    ];
    const out = head.slice();
    let i = 0;
    while (out.length < count) {
      out.push(`${f(rand() * 40 - 20)} ${f(rand() * 40 - 20)} ${f(rand() * 12)}`);
      i++;
      if (i > verts && out.length < count - 1) out.push(`3 ${i - 1} ${i} ${i + 1}`);
    }
    return out.slice(0, count);
  }
  if (ext === "step") {
    const out = [
      `ISO-10303-21;`,
      `HEADER;`,
      `FILE_DESCRIPTION(('${titleOf(stem)}'),'2;1');`,
      `FILE_NAME('${stem}.step','2026-03-04T11:20:14',('space'),('example','',''),'','','');`,
      `FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));`,
      `ENDSEC;`,
      `DATA;`,
    ];
    let i = 101;
    while (out.length + 1 < count) {
      out.push(`#${i}=CARTESIAN_POINT('',(${f(rand() * 40 - 20)},${f(rand() * 40 - 20)},${f(rand() * 12)}));`);
      i++;
    }
    if (count > 1) out[count - 1] = `END-ISO-10303-21;`;
    return out.slice(0, count);
  }
  if (ext === "iges") {
    const out: string[] = [];
    let i = 1;
    while (out.length < count) {
      const rec = `110,${f(rand() * 40)},${f(rand() * 40)},${f(rand() * 40)},${f(rand() * 40)},${f(rand() * 40)},${f(rand() * 12)};`;
      out.push(`${rec.padEnd(72)}S${String(i).padStart(6, "0")}`);
      i++;
    }
    return out;
  }
  if (ext === "dxf") {
    const out = [`0`, `SECTION`, `2`, `ENTITIES`];
    // One LINE entity = 12 lines; keep whole entities so no holes are created.
    while (out.length + 12 <= count - 2) {
      const x = f(rand() * 400);
      const y = f(rand() * 300);
      out.push(`0`, `LINE`, `8`, `0`, `10`, x, `20`, y, `11`, f(Number(x) + rand() * 80), `21`, f(Number(y) + rand() * 80));
    }
    while (out.length + 2 <= count) out.push(`0`, `ENDSEC`);
    if (out.length < count) out.push(`EOF`);
    return out.slice(0, count);
  }
  // eps — PostScript header + drawing commands.
  const head = [
    `%!PS-Adobe-3.0 EPSF-3.0`,
    `%%BoundingBox: 0 0 480 320`,
    `%%Title: ${stem}.eps`,
    `%%CreationDate: ${mtimeOf(seedOf(stem))}`,
    `%%EndComments`,
  ];
  const body = [
    `/line { setlinewidth 1.0 setlinecap moveto lineto stroke } bind def`,
    `0.12 0.16 0.21 setrgbcolor`,
    `48 48 moveto 432 48 lineto 432 272 lineto 48 272 lineto closepath stroke`,
    `72 96 moveto 180 220 lineto 300 120 lineto 408 240 lineto stroke`,
    `%%EOF`,
  ];
  return fill(count, head, body);
}

function genCodeJs(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `import { stat } from "node:fs/promises";`,
    `import type { Entry, ScanOptions } from "./types";`,
    ``,
    `const BATCH = ${pick(rand, [256, 512, 1024])};`,
    ``,
    `/** Collect ${stem} entries above the size floor. */`,
    `export async function ${camelOf(stem)}(root: string, opts: ScanOptions = {}): Promise<Entry[]> {`,
    `  const out: Entry[] = [];`,
    `  const floor = opts.minSize ?? ${pick(rand, [0, 1024, 65536])};`,
    `  for await (const dirent of walk(root, { depth: opts.depth ?? ${intOf(rand, 1, 6)} })) {`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `    if (!dirent.isFile() || dirent.size <= floor) continue;`,
      `    const info = await stat(dirent.path);`,
      `    out.push({ id: ${1000 + i}, path: dirent.path, size: info.size, cat: ${intOf(rand, 0, 7)} });`,
      `    if (out.length >= BATCH) break;`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genCodePy(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `#!/usr/bin/env python3`,
    `"""Index the ${stem} tree into the manifest."""`,
    `from __future__ import annotations`,
    ``,
    `import os`,
    `import sys`,
    `from pathlib import Path`,
    ``,
    `FLOOR = ${pick(rand, [1024, 65536, 1048576])}`,
    `LIMIT = ${pick(rand, [900, 1500, 2400])}`,
    ``,
    `def collect(root: str) -> list[dict]:`,
    `    entries: list[dict] = []`,
    `    for dirpath, _dirs, names in os.walk(root):`,
    `        for name in names:`,
    `            full = os.path.join(dirpath, name)`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `            size = os.path.getsize(full)`,
      `            if size <= FLOOR:`,
      `                continue`,
      `            entries.append({"path": full, "size": size, "cat": (hash(name) ^ ${i}) % 8})`,
      `            if len(entries) >= LIMIT:`,
      `                return sorted(entries, key=lambda e: -e["size"])`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genCodeRs(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `use std::collections::BTreeMap;`,
    `use std::path::{Path, PathBuf};`,
    ``,
    `const FLOOR: u64 = ${pick(rand, [1024, 65536, 1048576])};`,
    ``,
    `/// Entry manifest for the ${stem} tree.`,
    `pub struct Manifest {`,
    `    pub root: PathBuf,`,
    `    pub entries: BTreeMap<PathBuf, u64>,`,
    `}`,
    ``,
    `impl Manifest {`,
    `    pub fn scan(root: &Path, limit: usize) -> std::io::Result<Self> {`,
    `        let mut entries = BTreeMap::new();`,
    `        for entry in walkdir(root)? {`,
    `            let size = entry.metadata()?.len();`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `            if size > FLOOR {`,
      `                entries.insert(entry.path().to_path_buf(), size);`,
      `            }`,
      `            if entries.len() + ${i} >= limit {`,
      `                break;`,
      `            }`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genCodeGo(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `package main`,
    ``,
    `import (`,
    `    "fmt"`,
    `    "os"`,
    `    "path/filepath"`,
    `    "sort"`,
    `)`,
    ``,
    `const floor = int64(${pick(rand, [1024, 65536, 1048576])})`,
    ``,
    `// Entry is one indexed file for ${stem}.`,
    `type Entry struct {`,
    `    Path string`,
    `    Size int64`,
    `}`,
    ``,
    `func collect(root string, limit int) ([]Entry, error) {`,
    `    var out []Entry`,
    `    err := filepath.Walk(root, func(path string, info os.FileInfo, err error) error {`,
    `        if err != nil {`,
    `            return nil // skip unreadable paths; the scan must finish`,
    `        }`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `        if info.IsDir() || info.Size() <= floor {`,
      `            return nil`,
      `        }`,
      `        out = append(out, Entry{path, info.Size()}) // #${i}`,
      `        if len(out) >= limit {`,
      `            return filepath.SkipAll`,
      `        }`,
      `        return nil`,
      `    })`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genCodeJava(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `package dev.example.space.scan;`,
    ``,
    `import java.nio.file.*;`,
    `import java.util.*;`,
    ``,
    `/** Manifest walker for ${stem}. */`,
    `public final class ${titleOf(stem) || "Manifest"}Walker {`,
    `    private static final long FLOOR = ${pick(rand, [1024, 65536, 1048576])}L;`,
    `    private static final int LIMIT = ${pick(rand, [900, 1500, 2400])};`,
    ``,
    `    record Entry(Path path, long size, int cat) {}`,
    ``,
    `    public List<Entry> collect(Path root) throws Exception {`,
    `        List<Entry> out = new ArrayList<>();`,
    `        try (var stream = Files.walk(root, ${intOf(rand, 2, 6)})) {`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `            for (Path p : (Iterable<Path>) stream::iterator) {`,
      `                long size = Files.size(p);`,
      `                if (size <= FLOOR) continue;`,
      `                out.add(new Entry(p, size, Math.floorMod(p.hashCode() ^ ${i}, 8)));`,
      `                if (out.size() >= LIMIT) break;`,
      `            }`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genCodeC(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `#include <dirent.h>`,
    `#include <stdint.h>`,
    `#include <stdio.h>`,
    `#include <string.h>`,
    ``,
    `/* manifest walker for ${stem} */`,
    `#define FLOOR ${pick(rand, [1024, 65536, 1048576])}ULL`,
    `#define MAX_ENTRIES ${pick(rand, [900, 1500, 2400])}`,
    ``,
    `typedef struct {`,
    `    char path[512];`,
    `    uint64_t size;`,
    `    int cat;`,
    `} entry_t;`,
    ``,
    `static int compare_size(const void *a, const void *b) {`,
    `    const entry_t *ea = (const entry_t *)a, *eb = (const entry_t *)b;`,
    `    return (eb->size > ea->size) - (eb->size < ea->size);`,
    `}`,
    ``,
    `int walk_dir(const char *dir, entry_t *entries, int count) {`,
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const body = [
      `    uint64_t size = size_of(dir);`,
      `    if (size <= FLOOR) return count;`,
      `    entry_t *e = &entries[count++];`,
      `    snprintf(e->path, sizeof e->path, "%s/item_%d.bin", dir, ${i});`,
      `    e->size = size;`,
      `    e->cat = cat_of(e->path);`,
      `    if (count >= MAX_ENTRIES) return count;`,
    ];
    for (const line of body) {
      if (out.length >= count) break;
      out.push(line);
    }
    i++;
  }
  return out;
}

function genScriptSh(count: number, rand: Rng): string[] {
  const head = [
    `#!/usr/bin/env bash`,
    `set -euo pipefail`,
    ``,
    `ROOT="${pick(rand, ["/mnt/data", "D:/", "/Volumes/Backup"])}"`,
    `OUT="$ROOT/manifest.tsv"`,
    ``,
    `# biggest files first; skip anything under a cache directory`,
    `find "$ROOT" \\( -path '*/AppData/Local/Temp' -o -path '*/.cache' \\) -prune \\`,
    `  -o -type f -size +${pick(rand, ["100M", "250M", "1G"])} -printf '%s\\t%p\\n' \\`,
    `  | sort -rn | head -${pick(rand, [50, 100, 200])} > "$OUT"`,
    ``,
  ];
  const body = [
    `wc -l "$OUT"`,
    `du -sh "$ROOT"/* 2>/dev/null | sort -rh | head -20`,
    `echo "manifest written to $OUT"`,
    `logger -t space "manifest refresh complete"`,
  ];
  return fill(count, head, body);
}

function genScriptPs1(count: number, rand: Rng): string[] {
  const head = [
    `$ErrorActionPreference = "Stop"`,
    `$started = Get-Date`,
    ``,
    `$root = "D:\\"`,
    `$floor = ${pick(rand, [100 * MB, 250 * MB, GB])}`,
    `$out = Join-Path $env:TEMP "space-manifest.csv"`,
    ``,
    `Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue |`,
    `  Where-Object { $_.Length -gt $floor -and $_.FullName -notmatch 'AppData' } |`,
    `  Sort-Object Length -Descending |`,
    `  Select-Object -First 100 |`,
    `  Export-Csv $out -NoTypeInformation`,
    ``,
    `Write-Host "wrote $out"`,
  ];
  const body = [
    `Get-ChildItem $root -Directory | ForEach-Object {`,
    `  "{0,12:N0}  {1}" -f (Get-ChildItem $_.FullName -Recurse -File | Measure-Object Length -Sum).Sum, $_.Name`,
    `}`,
    `Write-Verbose "scan took $((Get-Date) - $started)"`,
  ];
  return fill(count, head, body);
}

function genScriptBat(count: number, rand: Rng): string[] {
  const head = [
    `@echo off`,
    `setlocal`,
    `rem nightly manifest for drive D:`,
    ``,
    `set ROOT=D:\\`,
    `set OUT=%TEMP%\\space-manifest.txt`,
    ``,
    `dir /s /a-d "%ROOT%" | findstr /c:"File(s)" > "%OUT%"`,
    `for /f "tokens=3" %%A in ('dir /s /a-d "%ROOT%" ^| findstr /c:"File(s)"') do set BYTES=%%A`,
    ``,
    `echo manifest: %OUT% (%BYTES% bytes)`,
    `endlocal`,
  ];
  const body = [
    `rem retry once if the drive was busy`,
    `if errorlevel 1 timeout /t ${pick(rand, [15, 30, 60])} /nobreak >nul`,
    `powershell -NoProfile -Command "Get-ChildItem %ROOT% -Recurse | Measure-Object Length -Sum"`,
    `echo done %DATE% %TIME%`,
  ];
  return fill(count, head, body);
}

function genSql(count: number, rand: Rng, stem: string): string[] {
  const head = [
    `-- manifest index for ${stem}`,
    `PRAGMA journal_mode = WAL;`,
    ``,
    `CREATE TABLE IF NOT EXISTS entries (`,
    `    id INTEGER PRIMARY KEY,`,
    `    path TEXT NOT NULL UNIQUE,`,
    `    size INTEGER NOT NULL,`,
    `    cat INTEGER NOT NULL DEFAULT 7,`,
    `    scanned_at TEXT NOT NULL`,
    `);`,
    ``,
    `CREATE INDEX IF NOT EXISTS idx_entries_size ON entries (size DESC);`,
    ``,
    `SELECT cat, COUNT(*) AS files, SUM(size) AS bytes`,
    `FROM entries`,
    `GROUP BY cat`,
    `ORDER BY bytes DESC;`,
    ``,
  ];
  const paths = [
    "D:\\clips\\raw-footage\\img_2213.jpg",
    "D:\\Windows\\System32\\ntoskrnl.exe",
    "D:\\Users\\casey\\Downloads\\ubuntu-24.04.iso",
    "D:\\Program Files\\Blender\\blender.exe",
  ];
  const inserts: string[] = [];
  for (let i = 0; i < 16; i++) {
    inserts.push(
      `INSERT INTO entries (path, size, cat, scanned_at) VALUES ('${pick(rand, paths)}', ${intOf(rand, 4096, 900000000)}, ${intOf(rand, 0, 7)}, '2026-03-04T11:20:14Z');`,
    );
  }
  return fill(count, head, inserts);
}

function genNotes(count: number, rand: Rng): string[] {
  // Free-form txt notes — dated entries that reference the demo dataset
  // itself, so cross-referencing the map and this file reads consistent.
  const head = [`Drive D: cleanup notes`, `=====================`, ``];
  const subjects: [string, string, string][] = [
    ["Downloads", "18 installers removed", "4.2 GB"],
    ["clips/raw-footage", "take_01.mp4 kept for the edit", "12.4 MB"],
    ["AppData/Local/Temp", "stale shader caches purged", "812 MB"],
    ["Videos", "blender_render.mp4 re-encode pending", "780 MB"],
    ["Windows/WinSxS", "component store cleanup deferred (needs reboot)", "1.4 GB"],
    ["Program Files", "old Blender 3.6 uninstalled", "1.1 GB"],
    ["Users/casey/Downloads", "ubuntu-24.04.iso copied to E: backup", "1.6 GB"],
    ["clips/b-roll", "broll_clip.mp4 backed up twice - verify E:", "3.1 MB"],
  ];
  const out = head.slice();
  let i = 0;
  while (out.length < count) {
    const [where, what, size] = subjects[i % subjects.length];
    const d = new Date(Date.UTC(2026, 1, 6) + i * 86400000 * intOf(rand, 1, 3));
    const stamp = `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())} ${pad2(intOf(rand, 9, 22))}:${pad2(intOf(rand, 0, 59))}`;
    out.push(`${stamp}  ${where}: ${what} (${size})`);
    i++;
    if (out.length < count && i % 4 === 0) out.push("");
  }
  return out.slice(0, count);
}
