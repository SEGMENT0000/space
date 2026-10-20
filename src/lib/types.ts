// TypeScript mirror of `src-tauri/src/model.rs`.
// This file is the frozen IPC contract. Do not change shapes without
// changing the Rust side in the same commit.

/** Category ids — MUST match CATEGORY_* in model.rs and labels in colors.ts */
export const CAT = {
  MEDIA: 0,
  DOCUMENTS: 1,
  CODE: 2,
  CACHES: 3,
  APPS: 4,
  ARCHIVES: 5,
  SYSTEM: 6,
  OTHER: 7,
} as const;

export const CAT_COUNT = 8;

export interface DriveInfo {
  name: string;
  mount: string;
  fs: string;
  total: number;
  free: number;
  kind: "ssd" | "hdd" | "removable" | "unknown";
  isRemovable: boolean;
}

export interface ProgressPayload {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  current: string;
  elapsedMs: number;
}

export interface DonePayload {
  root: string;
  rootId: number;
  totalBytes: number;
  totalFiles: number;
  totalDirs: number;
  errors: number;
  elapsedMs: number;
  categories: number[];
}

export interface ErrorPayload {
  message: string;
}

export interface ChildEntry {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  fileCount: number;
  cat: number;
  kidCount: number;
  sub?: ChildEntry[];
}

export interface LineageEntry {
  id: number;
  name: string;
}

export interface ChildrenResponse {
  parentId: number;
  parentSize: number;
  parentCat: number;
  totalEntries: number;
  entries: ChildEntry[];
  lineage: LineageEntry[];
  path: string;
}

export interface LargestFile {
  id: number;
  name: string;
  size: number;
  cat: number;
  path: string;
}

export interface ScanStateDto {
  state: "idle" | "scanning" | "ready" | "error";
  root: string | null;
  error: string | null;
}

// --- Preview contract (FLEET §B, mirror of model.rs) — APPEND-ONLY ------------
// ONE tagged union per file OR folder: `family` says which optional sub-object
// is populated; every other sub-object is absent. Synced to the FINALIZED
// backend contract (model.rs v2, additive DTO): required fields are the ones
// Rust always serializes; `#[serde(skip_serializing_if = "Option::is_none")]`
// fields are optional here. Payloads from older Rust builds stay renderable
// through the plate's tolerant fallback ladder.

export type PreviewFamily = "text" | "hex" | "image" | "list" | "meta" | "db" | "media";

/** TEXT encoding — BOM-consumed read mode, lower-case as Rust spells it. */
export type PreviewEncoding = "utf-8" | "utf-16le" | "utf-16be";

/** LIST listing source — the preview.rs enumerators that produced the rows.
 *  (Older demo/legacy payloads may still say "archive"; the renderer treats
 *  unknown words as plain listings, never guessing a source.) */
export type PreviewListKind = "folder" | "zip" | "tar" | "iso" | "sheet";

/** LIST row kind — a directory row or a file row. */
export type PreviewEntryKind = "dir" | "file";

export interface PreviewPayload {
  family: PreviewFamily;
  /** TEXT head: ≤64 KB read, ≤120 lines. `totalLines` is the exact count
   *  only when the whole file fit the cap; `encoding` is BOM-detected. */
  text?: { lines: string[]; truncated: boolean; totalLines?: number; encoding?: PreviewEncoding };
  /** IMAGE: downscaled re-encode (longest edge ≤512 px, JPEG q≈80).
   *  `w`/`h` are the ORIGINAL source dimensions, not the thumbnail's. */
  image?: { dataUrl: string; w: number; h: number; mime: string };
  /** HEX: first 4 KB, 8 bytes per row; `base` is always 0, `total` the file
   *  size in bytes. Frontend regroups rows to 8/16 per tile width. */
  hex?: { rows: { off: number; bytes: string; ascii: string }[]; base: number; total: number };
  /** LIST: folder entries, archive central dir, tar walk, ISO tree, sheets.
   *  `kind` names the listing source; `total` is the exact entry count when
   *  it was cheap to enumerate (≤50k names / central dir). */
  list?: {
    entries: {
      name: string;
      /** Omitted for directory rows (subtree size needs a walk). */
      size?: number;
      /** "dir" | "file" (see PreviewEntryKind). Typed as plain string so
       *  payloads from older demo/backend builds stay type-valid — the
   *  renderer never trusts a kind it doesn't recognize. */
      kind: string;
      /** Archive compression method ("Store" | "Deflate" | "Bzip2" |
       *  "Zstd" | …) on zip file rows only; tar file rows are "Store";
       *  folder/ISO/sheet and dir rows omit it. */
      method?: string;
    }[];
    truncated: boolean;
    total?: number;
    /** Listing source (see PreviewListKind); absent on legacy listings. */
    kind?: string;
    /** Folder cards only: wrapper directories descended through before the
     *  rendered rows (e.g. ["User Data"] for Chrome). The head meta names the
     *  chain so the rows are never mistaken for the folder's direct children. */
    via?: string[];
  };
  /** META: flat display pairs. A DEGRADED card (locked/symlink/denied file)
   *  puts a `note` pair FIRST with the verbatim reason ("symlink" |
   *  "unsupported" | "access denied" | "file in use"), then format/type/
   *  signature/size/modified as available — `size` and `signature` are
   *  genuinely ABSENT for unreadable files, never placeholdered. */
  meta?: { pairs: { k: string; v: string }[] };
  /** DB: schema per table, plus the first rows of the leading tables via
   *  bounded LIMIT queries. `cols` == `columns.len()` when the columns were
   *  readable; `columns` are declaration-order names; `rows` are the first
   *  ≤5 rows of the first ≤3 tables as cell display text (≤64 chars+"…",
   *  blobs as a byte-size stub, null rendered honestly). */
  db?: {
    tables: { name: string; cols: number; columns?: string[]; rows?: string[][] }[];
    truncated: boolean;
  };
  /** MEDIA family: playable local file. `path` is absolute; the frontend
   *  converts it to an asset-URL via convertFileSrc (assetProtocol scope).
   *  `durationMs` is real when cheaply parsed, absent (never guessed)
   *  otherwise; `pairs` are real parsed container facts (format, duration,
   *  codec, channels, sample rate, bit depth, id3 tag, …). */
  media?: {
    path: string;
    mime: string;
    kind: "video" | "audio";
    durationMs?: number;
    w?: number;
    h?: number;
    pairs?: { k: string; v: string }[];
  };
}
