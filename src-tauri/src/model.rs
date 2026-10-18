//! Shared data model for `space`.
//! This file is the frozen contract between the Rust engine and the React UI.
//! Field names are serialized camelCase to match `src/lib/types.ts`.

use serde::Serialize;

/// Category ids — MUST match `CATEGORY_LABELS` in `src/lib/colors.ts`.
pub const CAT_MEDIA: u8 = 0;
pub const CAT_DOCUMENTS: u8 = 1;
pub const CAT_CODE: u8 = 2;
pub const CAT_CACHES: u8 = 3;
pub const CAT_APPS: u8 = 4;
pub const CAT_ARCHIVES: u8 = 5;
pub const CAT_SYSTEM: u8 = 6;
pub const CAT_OTHER: u8 = 7;

/// Sentinel for "no parent" (root node).
pub const NO_PARENT: u32 = u32::MAX;

/// A mounted volume shown on the drives screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    /// "ssd" | "hdd" | "removable" | "unknown"
    pub kind: String,
    pub is_removable: bool,
}

/// Emitted ~8x/second on `scan://progress` while a scan is running.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    /// Directory most recently picked up by the walker.
    pub current: String,
    pub elapsed_ms: u64,
}

/// Emitted once on `scan://done` when a scan finishes successfully.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonePayload {
    pub root: String,
    pub root_id: u32,
    pub total_bytes: u64,
    pub total_files: u64,
    pub total_dirs: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    /// Per-category byte totals, indexed by category id (len == 8).
    pub categories: Vec<u64>,
}

/// Emitted on `scan://error` if the scan fails fatally.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub message: String,
}

/// One child of a directory, as returned by `get_children`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildEntry {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// Leaf files under this node (1 for a file).
    pub file_count: u64,
    /// Dominant category id.
    pub cat: u8,
    /// Direct children count (0 for files).
    pub kid_count: u32,
    /// Top sub-entries (by size, max 8), only populated when depth == 2 and is_dir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<Vec<ChildEntry>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageEntry {
    pub id: u32,
    pub name: String,
}

/// Response of `get_children` — everything needed to render one treemap level.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenResponse {
    pub parent_id: u32,
    pub parent_size: u64,
    pub parent_cat: u8,
    /// Total direct children before `limit` was applied.
    pub total_entries: u32,
    pub entries: Vec<ChildEntry>,
    /// Root ..= parent, for breadcrumbs.
    pub lineage: Vec<LineageEntry>,
    /// Display path, e.g. `C:\Users\you`.
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargestFile {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub cat: u8,
    pub path: String,
}

/// Snapshot of engine state, for UI recovery on reload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStateDto {
    /// "idle" | "scanning" | "ready" | "error"
    pub state: String,
    pub root: Option<String>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Preview DTOs (FLEET_SPEC §B — APPEND ONLY; mirrors `src/lib/types.ts`)
// ---------------------------------------------------------------------------

/// Content preview returned by `get_preview`. Tagged by `family`; exactly one
/// of the sibling blocks (text/image/hex/list/meta/db) is populated, except
/// that executables carry `meta` + a `hex` head together (§A "META + HEX head").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPayload {
    /// "text" | "hex" | "image" | "list" | "meta" | "db"
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImagePreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex: Option<HexPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<ListPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db: Option<DbPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaPreview>,
}

/// TEXT family head: ≤64 KB read, ≤200 lines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub lines: Vec<String>,
    pub truncated: bool,
    /// Exact total only when the whole file was inside the 64 KB cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<u64>,
    /// "utf-8" | "utf-16le" | "utf-16be" — BOM consumed; utf-8 is lossy-decoded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// IMAGE family: downscaled re-encode (longest edge ≤256 px, JPEG q≈80).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreview {
    pub data_url: String,
    pub w: u32,
    pub h: u32,
    /// Always "image/jpeg" — the payload is the re-encoded thumbnail.
    pub mime: String,
}

/// One 8-byte row of the hexdump head; frontend regroups to 8/16 per row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexRow {
    /// Absolute file offset of this row (bytes).
    pub off: u64,
    /// Space-separated lowercase hex, 8 bytes.
    pub bytes: String,
    /// Printable ASCII (0x20..=0x7E) or "·".
    pub ascii: String,
}

/// HEX family: first 4 KB of the file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexPreview {
    pub rows: Vec<HexRow>,
    /// Always 0 (no seek offset is applied server-side).
    pub base: u64,
    /// Total file size in bytes.
    pub total: u64,
}

/// LIST family row — folder entries, archive central dir, tar walk, sheets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEntry {
    pub name: String,
    /// File byte size; omitted for directories (subtree size needs a walk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// "dir" | "file"
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPreview {
    pub entries: Vec<ListEntry>,
    pub truncated: bool,
    /// Exact entry count when enumerated cheaply (≤50k names / central dir).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// META family pair — value is a pre-formatted display string, k is a short key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaPair {
    pub k: String,
    pub v: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaPreview {
    pub pairs: Vec<MetaPair>,
}

/// DB family: table names + column counts only (no COUNT(*), never row data).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbTable {
    pub name: String,
    pub cols: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbPreview {
    pub tables: Vec<DbTable>,
    pub truncated: bool,
}

/// MEDIA family: a playable local media file. The frontend converts `path`
/// to an asset-URL via convertFileSrc (asset protocol scope) and plays it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPreview {
    pub path: String,
    pub mime: String,
    /// "video" | "audio"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<u32>,
}

/// Internal arena node. Not serialized.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: u32,
    pub is_dir: bool,
    /// File size for files; full subtree size for directories (filled post-scan).
    pub size: u64,
    /// Subtree leaf count (filled post-scan).
    pub file_count: u64,
    /// Dominant category (approx. for dirs; exact for files).
    pub cat: u8,
    /// Direct children count.
    pub kid_count: u32,
}
