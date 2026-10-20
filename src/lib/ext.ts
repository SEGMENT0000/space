// Authoritative file-type metadata for the whole front end. One O(1) Map
// lookup answers: what IS this file (kind), how should it read aloud
// (label), may we render its bytes as text (isText), which glyph carries
// its shape (icon id), and which preview family suits it (preview — the
// hint mirrors lib/types.ts PreviewFamily; the Rust side sniffs signatures
// and may override it, this is the optimistic guess for plate qualification
// and the honest fallback when no payload arrives).
//
// Design rules baked in here:
// - Windows is the primary target, so the SYSTEM long tail is first class:
//   sys/drv/dll/mui (PE system files), vhdx/vhd/vmdk/vdi/wim/esd/swm (disk
//   images), cab (installer cabinets), dmp/mdmp/hdmp (crash dumps), pf
//   (prefetch), evtx/etl (logs), nvph (NVIDIA cache), mst (installer
//   transform), reg/lnk/url (shell files), tmp/temp/part/crdownload
//   (temporaries), nls/tlb/ocx/cpl/scr/com/efi — the C:\ walk hits these
//   by the million and every one of them deserves a real name, not "File".
// - The default chain is ext → kind → default and NEVER throws: unknown
//   extension → honest META readout (a properties plate fabricates no
//   content it did not decode); extensionless file → HEX head (§A: a
//   nameless binary sniffs by bytes). Every FileMeta is a frozen shared
//   object, so a lookup allocates nothing beyond extOf's slice.
// - Kinds are orthogonal to the 8 treemap categories (FLEET §G: "do NOT
//   expand categories — preview families are orthogonal"). A .sys file is
//   kind `executable` here and category System there; classify.rs owns
//   category, this file owns type.

import type { PreviewFamily } from "./types";

/** Lowercased extension without the dot; "" when the base name has none.
    Accepts a path or a bare name — the directory part is stripped first, so
    a dot inside a folder name cannot leak into the result. */
export function extOf(path: string): string {
  const base = path.slice(path.lastIndexOf("\\") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

/** The file's nature, independent of its treemap category. */
export type FileKind =
  | "text"
  | "code"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "database"
  | "executable"
  | "font"
  | "disk"
  | "document"
  | "config"
  | "binary";

/** Stable icon ids — ext.ts stays dependency-free; the consumer maps the id
 *  to a lucide component (all names verified present in the installed
 *  build: File, FileText, FileCode, FileImage, Film, FileMusic,
 *  FileArchive, FileSpreadsheet, Database, Package, HardDrive, Type,
 *  FileCog). */
export type FileIconId =
  | "file"
  | "text"
  | "code"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "database"
  | "app"
  | "font"
  | "disk"
  | "sheet"
  | "config"
  | "key";

/** One frozen record per extension. `label` is sentence case, real data
 *  only (DESIGN.md label voice); extensions themselves are the inherently-
 *  upper data, and they appear in the filename, not here. */
export interface FileMeta {
  readonly label: string;
  readonly kind: FileKind;
  readonly isText: boolean;
  readonly icon: FileIconId;
  readonly preview: PreviewFamily;
}

const meta = (
  label: string,
  kind: FileKind,
  icon: FileIconId,
  preview: PreviewFamily,
  isText = false,
): FileMeta => Object.freeze({ label, kind, isText, icon, preview });

// ------------------------------------------------------------------ defaults
/** Unknown extension: nameless kind binary — a META readout is the only
 *  honest preview (§A: META never fabricates content it didn't decode). */
export const UNKNOWN_META = meta("File", "binary", "file", "meta");
/** No extension at all: §A maps extensionless names to the HEX head. */
export const EXTLESS_META = meta("File", "binary", "file", "hex");

// --------------------------------------------------------------------- table
// [meta, "ext ext …"] — one frozen meta per group, shared by every member.
// Ordered by real-world mass on a Windows volume, then the system long tail.
// prettier-ignore
const GROUPS: ReadonlyArray<readonly [FileMeta, string]> = [
  // --- text ---------------------------------------------------------------
  [meta("Text file",    "text", "text", "text", true), "txt text log logs nfo me readme"],
  [meta("Markdown",     "text", "text", "text", true), "md markdown"],
  [meta("Rich text",    "text", "text", "text", true), "rtf"],
  [meta("Subtitle",     "text", "text", "text", true), "srt ass ssa vtt"],
  [meta("Playlist",     "text", "audio", "text", true), "m3u m3u8 pls"],
  [meta("Email message","text", "text", "text", true), "eml"],
  [meta("Mailbox",      "text", "text", "text", true), "mbox"],
  [meta("Table",        "text", "sheet", "text", true), "csv tsv"],
  // --- code ---------------------------------------------------------------
  [meta("Source code",  "code", "code", "text", true),
   "rs ts tsx js jsx mjs cjs cts py pyw java kt kts go c cpp cc cxx h hpp hh hxx cs rb php swift m mm sh bash zsh fish ps1 psm1 bat cmd vbs au3 lua dart zig scala groovy gradle ex exs erl hrl hs ml mli fs fsx clj cljs elm r jl nim v asm s proto graphql sol tf hcl"],
  [meta("JSON",         "code", "code", "text", true), "json jsonc"],
  [meta("YAML",         "code", "code", "text", true), "yaml yml"],
  [meta("TOML",         "code", "code", "text", true), "toml"],
  [meta("XML",          "code", "code", "text", true), "xml xsl xslt xsd dtd"],
  [meta("Stylesheet",   "code", "code", "text", true), "css scss sass less styl"],
  [meta("Web page",     "code", "code", "text", true), "html htm vue svelte astro twig jinja"],
  [meta("SQL",          "code", "code", "text", true), "sql"],
  [meta("Notebook",     "code", "code", "text", true), "ipynb"],
  [meta("Lock file",    "code", "code", "text", true), "lock"],
  [meta("Build script", "code", "code", "text", true), "cmake mk makefile"],
  // --- config -------------------------------------------------------------
  [meta("Configuration","config", "config", "text", true), "ini cfg conf config env properties plist"],
  [meta("Registry file","config", "config", "text", true), "reg"],
  [meta("Internet shortcut", "config", "config", "text", true), "url"],
  // --- image --------------------------------------------------------------
  [meta("Image",        "image", "image", "image"), "jpg jpeg png gif webp bmp tif tiff ico heic avif apng jxl"],
  [meta("SVG image",    "image", "image", "text", true), "svg svgz"],
  [meta("RAW photo",    "image", "image", "meta"), "raw cr2 nef arw orf raf dng"],
  [meta("Photoshop document", "image", "image", "meta"), "psd"],
  // --- video --------------------------------------------------------------
  [meta("Video",        "video", "video", "media"), "mp4 m4v mov avi mkv webm wmv flv mpg mpeg 3gp 3g2 mts m2ts vob ogv rm rmvb mxf"],
  // --- audio --------------------------------------------------------------
  [meta("Audio",        "audio", "audio", "media"), "mp3 wav flac aac ogg m4a wma aiff ape opus mid midi ac3 dts amr"],
  // --- archive ------------------------------------------------------------
  // Multi-member containers list their central directory (§A LIST).
  [meta("Archive",      "archive", "archive", "list"), "zip rar 7z tar cab ace arj lha lzh"],
  // Single-stream compressors: method + unpacked size is all there is (META).
  [meta("Compressed",   "archive", "archive", "meta"), "gz tgz bz2 xz zst br lz4 lzma taz tbz2 txz"],
  // --- database -----------------------------------------------------------
  [meta("SQLite database", "database", "database", "db"), "db sqlite sqlite3 db3"],
  [meta("Access database", "database", "database", "hex"), "mdb accdb"],
  [meta("Exchange database", "database", "database", "hex"), "edb"],
  [meta("Outlook data file", "database", "database", "meta"), "pst ost"],
  [meta("SQL Server data",  "database", "database", "meta"), "mdf"],
  [meta("SQL Server log",   "database", "database", "meta"), "ldf"],
  [meta("Parquet data",     "database", "database", "meta"), "parquet"],
  [meta("HDF5 data",        "database", "database", "meta"), "h5 hdf5"],
  // --- executable ---------------------------------------------------------
  [meta("Application",      "executable", "app", "meta"), "exe"],
  [meta("Library",          "executable", "app", "meta"), "dll so dylib"],
  [meta("Installer",        "executable", "app", "meta"), "msi msp"],
  [meta("App package",      "executable", "app", "meta"), "msix appx appxbundle"],
  [meta("Package",          "executable", "app", "meta"), "deb rpm apk"],
  [meta("App bundle",       "executable", "app", "meta"), "app appimage"],
  [meta("Java archive",     "executable", "app", "meta"), "jar"],
  [meta("System file",      "executable", "app", "hex"), "sys"],
  [meta("Device driver",    "executable", "app", "hex"), "drv"],
  [meta("ActiveX control",  "executable", "app", "meta"), "ocx"],
  [meta("Control panel applet", "executable", "app", "meta"), "cpl"],
  [meta("Screen saver",     "executable", "app", "meta"), "scr"],
  [meta("MS-DOS program",   "executable", "app", "hex"), "com"],
  [meta("UEFI application", "executable", "app", "hex"), "efi"],
  [meta("Resource library", "executable", "app", "meta"), "mui"],
  [meta("Type library",     "executable", "app", "meta"), "tlb"],
  [meta("Installer transform", "executable", "app", "hex"), "mst"],
  // --- font ---------------------------------------------------------------
  [meta("Font",            "font", "font", "meta"), "ttf otf ttc woff woff2 eot fon pfb pfa"],
  // --- disk images --------------------------------------------------------
  [meta("Disc image",      "disk", "disk", "list"), "iso img"],
  [meta("Disk image",      "disk", "disk", "list"), "dmg"],
  [meta("Virtual disk",    "disk", "disk", "meta"), "vhd vhdx"],
  [meta("VMware disk",     "disk", "disk", "meta"), "vmdk"],
  [meta("VirtualBox disk", "disk", "disk", "meta"), "vdi"],
  [meta("Windows image",   "disk", "disk", "meta"), "wim esd swm"],
  // --- document -----------------------------------------------------------
  [meta("PDF document",    "document", "text", "meta"), "pdf"],
  [meta("Word document",   "document", "text", "meta"), "doc"],           // OLE: only META is honest
  [meta("Word document",   "document", "text", "text"), "docx docm"],
  [meta("Excel workbook",  "document", "sheet", "meta"), "xls"],
  [meta("Excel workbook",  "document", "sheet", "list"), "xlsx xlsm xlsb"],
  [meta("Presentation",    "document", "text", "meta"), "ppt"],
  [meta("Presentation",    "document", "text", "list"), "pptx ppsx odp"],
  [meta("OpenDocument text",     "document", "text", "text"), "odt"],
  [meta("OpenDocument sheet",    "document", "sheet", "list"), "ods"],
  [meta("E-book",          "document", "text", "list"), "epub"],
  [meta("E-book",          "document", "text", "meta"), "mobi"],
  [meta("Visio drawing",   "document", "text", "meta"), "vsdx"],
  [meta("Outlook message", "document", "text", "meta"), "msg"],
  [meta("Document",        "document", "text", "meta"), "djvu djv"],
  // --- certificates & keys ------------------------------------------------
  [meta("Certificate",     "text", "key", "text", true), "crt cer pem"],
  [meta("PGP data",        "text", "key", "text", true), "asc"],
  [meta("Key",             "text", "key", "text", true), "key pub"],
  [meta("Key store",       "binary", "key", "meta"), "der pfx p12 jks"],
  // --- 3D / CAD -----------------------------------------------------------
  // ASCII interchange formats preview as text (§A); Windows object files
  // (.obj under a build tree is COFF, not a mesh) and packed scenes sniff
  // HEX — on C:\ the binary reading is the honest one.
  [meta("3D model",       "binary", "file", "text", true), "stl ply gltf dxf step iges"],
  [meta("3D scene",       "binary", "file", "hex"), "fbx blend obj"],
  [meta("CAD drawing",    "binary", "file", "hex"), "dwg"],
  // --- Windows system long tail ------------------------------------------
  [meta("Windows shortcut", "binary", "file", "hex"), "lnk"],
  [meta("Temporary file",   "binary", "file", "hex"), "tmp temp"],
  [meta("Partial download", "binary", "file", "hex"), "part crdownload"],
  [meta("Crash dump",       "binary", "file", "hex"), "dmp mdmp hdmp"],
  [meta("Prefetch file",    "binary", "file", "hex"), "pf"],
  [meta("Event log",        "binary", "file", "hex"), "evtx"],
  [meta("Event trace",      "binary", "file", "hex"), "etl"],
  [meta("Language file",    "binary", "file", "hex"), "nls"],
  [meta("NVIDIA cache",     "binary", "file", "hex"), "nvph"],
  [meta("Debug symbols",    "binary", "database", "meta"), "pdb"],
  [meta("Static library",   "binary", "app", "meta"), "lib"],
  [meta("Subtitle index",   "binary", "file", "hex"), "idx sub"],
  // --- catch-alls inside known shapes ------------------------------------
  [meta("Data file",        "binary", "file", "hex"), "bin dat"],
  [meta("Resource bundle",  "binary", "file", "hex"), "pak"],
];

const EXTS = new Map<string, FileMeta>();
for (const [m, list] of GROUPS) for (const e of list.split(" ")) EXTS.set(e, m);

/** The kinds in this file's vocabulary — for filters/legends that group by
 *  type. Stable order, matches the FileKind union. */
export const FILE_KINDS = [
  "text",
  "code",
  "config",
  "image",
  "video",
  "audio",
  "archive",
  "database",
  "executable",
  "font",
  "disk",
  "document",
  "binary",
] as const;

// ------------------------------------------------------------------ lookup
// thumbs.db shares the .db extension with SQLite databases but is an OLE
// thumbnail cache — declared before fileMeta because a plate that promises
// tables it cannot list is a lie.
const THUMBS_META = meta("Thumbnail cache", "database", "database", "hex");

/** Metadata for a bare (ideally lowercased) extension. One Map hit; an
 *  already-lowercased ext never allocates. The default chain runs here:
 *  ext → UNKNOWN_META (an honest META readout) → EXTLESS_META when there
 *  was no extension to begin with. Never throws. */
export function metaForExt(ext: string): FileMeta {
  const hit = EXTS.get(ext);
  if (hit) return hit;
  if (!ext) return EXTLESS_META;
  // Lenient fallback for callers that skipped extOf: retry lowercased, one
  // allocation only on the miss path.
  const lower = ext.toLowerCase();
  return EXTS.get(lower) ?? UNKNOWN_META;
}

/** Metadata for a file NAME or PATH. Same frozen-object contract as
 *  metaForExt — safe to call per tile; consults THUMBS_META for the one
 *  ambiguous basename. */
export function fileMeta(pathOrName: string): FileMeta {
  const ext = extOf(pathOrName);
  const m = metaForExt(ext);
  if (ext === "db") {
    // Compare the base name without allocating unless we are already in the
    // one ambiguous case (.db files are rare; thumbs.db is not).
    const base = pathOrName.slice(pathOrName.lastIndexOf("\\") + 1);
    if (base.length === 9 && base.toLowerCase() === "thumbs.db")
      return THUMBS_META;
  }
  return m;
}

/** Sugar: the kind alone (grouping, Type sort, legend buckets). */
export function kindOf(pathOrName: string): FileKind {
  return fileMeta(pathOrName).kind;
}
