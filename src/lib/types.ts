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
// is populated; every other sub-object is absent. All sub-fields stay optional
// so payloads from older Rust builds remain type-valid.

export type PreviewFamily = "text" | "hex" | "image" | "list" | "meta" | "db" | "media";

export interface PreviewPayload {
  family: PreviewFamily;
  text?: { lines: string[]; truncated: boolean; totalLines?: number; encoding?: string };
  image?: { dataUrl: string; w: number; h: number; mime: string };
  hex?: { rows: { off: number; bytes: string; ascii: string }[]; base: number; total: number };
  list?: {
    entries: { name: string; size?: number; kind: string }[];
    truncated: boolean;
    total?: number;
    kind?: string;
  };
  meta?: { pairs: { k: string; v: string }[] };
  db?: { tables: { name: string; cols: number; rows?: number }[]; truncated: boolean };
  /** MEDIA family: playable local file. `path` is absolute; the frontend
   *  converts it to an asset-URL via convertFileSrc (assetProtocol scope). */
  media?: {
    path: string;
    mime: string;
    kind: "video" | "audio";
    durationMs?: number;
    w?: number;
    h?: number;
  };
}
