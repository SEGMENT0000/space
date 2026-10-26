//! Behavioural tests for the bounded preview service.
//!
//! Fixtures are written to a unique temporary directory and removed on drop.
//! The tests only call the public `space::preview` API; they never launch the
//! GUI and never need network access.

use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use space::preview::{ListKind, MetaPreview, Preview, Service};

// ---------------------------------------------------------------------------
// Temporary fixtures
// ---------------------------------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("space-preview-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        write_file(&self.path, name, bytes)
    }

    fn subdir(&self, name: &str) -> PathBuf {
        let p = self.path.join(name);
        fs::create_dir_all(&p).expect("create temp subdir");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(&path, bytes).expect("write fixture");
    path
}

fn text_of(preview: Preview) -> space::preview::TextPreview {
    match preview {
        Preview::Text(t) => t,
        other => panic!("expected TEXT, got {}", other.kind()),
    }
}

fn meta_of(preview: Preview) -> MetaPreview {
    match preview {
        Preview::Meta(m) => m,
        other => panic!("expected META, got {}", other.kind()),
    }
}

fn pair_value<'a>(pairs: &'a [space::preview::MetaPair], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|p| p.key == key)
        .map(|p| p.value.as_str())
}

fn names(preview: &space::preview::ListPreview) -> Vec<&str> {
    preview.entries.iter().map(|e| e.name.as_str()).collect()
}

fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(File::create(path).expect("create zip"));
    for (name, data) in files {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file(*name, options).expect("zip member");
        writer.write_all(data).expect("zip data");
    }
    writer.finish().expect("finish zip");
}

/// Build an uncompressed POSIX tar fixture with valid header checksums.
fn tar_fixture(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, typeflag, data) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        tar_octal_field(&mut header[100..108], 0o644);
        tar_octal_field(&mut header[108..116], 0);
        tar_octal_field(&mut header[116..124], 0);
        tar_octal_field(&mut header[124..136], data.len() as u64);
        tar_octal_field(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = *typeflag;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let sum: u64 = header.iter().map(|&b| u64::from(b)).sum();
        let octal = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(octal.as_bytes());

        out.extend_from_slice(&header);
        out.extend_from_slice(data);
        let pad = (512 - data.len() % 512) % 512;
        out.extend(std::iter::repeat_n(0, pad));
    }
    out.extend(std::iter::repeat_n(0, 1024));
    out
}

fn tar_octal_field(field: &mut [u8], value: u64) {
    field.fill(b'0');
    let digits = format!("{value:o}");
    let width = field.len() - 1;
    assert!(digits.len() <= width, "value does not fit tar field");
    field[width - digits.len()..width].copy_from_slice(digits.as_bytes());
    field[field.len() - 1] = 0;
}

fn iso_fixture() -> Vec<u8> {
    const SECTOR: usize = 2048;
    let mut image = vec![0u8; SECTOR * 19];

    let mut pvd = [0u8; SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    // Logical block size, both-endian.
    pvd[128..130].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    pvd[130..132].copy_from_slice(&(SECTOR as u16).to_be_bytes());
    let root = iso_record(&[0], 18, SECTOR as u32, true);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    image[16 * SECTOR..17 * SECTOR].copy_from_slice(&pvd);

    let mut dir = Vec::new();
    dir.extend_from_slice(&iso_record(&[0], 18, SECTOR as u32, true));
    dir.extend_from_slice(&iso_record(&[1], 18, SECTOR as u32, true));
    dir.extend_from_slice(&iso_record(b"HELLO.TXT;1", 19, 123, false));
    dir.extend_from_slice(&iso_record(b"SUBDIR", 19, SECTOR as u32, true));
    image[18 * SECTOR..18 * SECTOR + dir.len()].copy_from_slice(&dir);
    image
}

fn iso_record(name: &[u8], extent: u32, size: u32, is_dir: bool) -> Vec<u8> {
    let mut len = 33 + name.len();
    if len % 2 == 1 {
        len += 1;
    }
    let mut rec = vec![0u8; len];
    rec[0] = len as u8;
    rec[2..6].copy_from_slice(&extent.to_le_bytes());
    rec[6..10].copy_from_slice(&extent.to_be_bytes());
    rec[10..14].copy_from_slice(&size.to_le_bytes());
    rec[14..18].copy_from_slice(&size.to_be_bytes());
    rec[25] = if is_dir { 0x02 } else { 0x00 };
    rec[32] = name.len() as u8;
    rec[33..33 + name.len()].copy_from_slice(name);
    rec
}

// ---------------------------------------------------------------------------
// TEXT / SVG
// ---------------------------------------------------------------------------

#[test]
fn text_preview_caps_lines_reports_total_and_scrubs_controls() {
    let tmp = TempDir::new();
    let mut body = String::new();
    for i in 0..200 {
        body.push_str(&format!("line {i:03}\n"));
    }
    // A bidi override and a C0 control must never reach the rendered lines.
    body.push_str("safe\u{202e}text\u{0007}\n");
    let path = tmp.file("many.rs", body.as_bytes());

    let text = text_of(Service::new().preview(&path));
    assert_eq!(text.lines.len(), 120, "line cap");
    assert!(text.truncated);
    assert_eq!(text.total_lines, Some(201));
    assert_eq!(text.encoding.as_deref(), Some("utf-8"));
    assert_eq!(text.language.as_deref(), Some("rust"));
    assert!(text.lines.iter().all(|line| {
        !line.chars().any(|c| c.is_control() && c != '\t')
            && !line.contains('\u{202e}')
            && !line.contains('\u{0007}')
    }));
}

#[test]
fn text_preview_stops_at_64_kib_and_keeps_utf16_bom() {
    let tmp = TempDir::new();

    // 200 lines x ~350 bytes is larger than the 64 KiB head read.
    let mut long = String::new();
    for i in 0..200 {
        long.push_str(&format!("{i:03} {}\n", "x".repeat(346)));
    }
    assert!(long.len() > 64 * 1024);
    let long_path = tmp.file("long.txt", long.as_bytes());
    let text = text_of(Service::new().preview(&long_path));
    assert_eq!(text.lines.len(), 120);
    assert!(text.truncated);
    assert_eq!(text.total_lines, None, "partial file cannot claim a total");

    let mut utf16 = Vec::new();
    utf16.extend_from_slice(&[0xFF, 0xFE]);
    for unit in "hello\nworld\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let utf16_path = tmp.file("utf16.txt", &utf16);
    let text = text_of(Service::new().preview(&utf16_path));
    assert_eq!(text.encoding.as_deref(), Some("utf-16le"));
    assert_eq!(text.lines, vec!["hello".to_string(), "world".to_string()]);
    assert_eq!(text.total_lines, Some(2));
}

#[test]
fn svg_preview_is_scriptless_and_scrubbed() {
    let tmp = TempDir::new();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg">
<script>alert(1)</script>
<rect width="10" height="10" onclick="alert(2)" fill="red"/>
<a href="javascript:alert(3)">link</a>
</svg>"#;
    let path = tmp.file("drawing.svg", svg);

    let text = text_of(Service::new().preview(&path));
    let joined = text.lines.join("\n").to_ascii_lowercase();
    assert!(!joined.contains("<script"), "script element removed");
    assert!(!joined.contains("onclick"), "event handler removed");
    assert!(!joined.contains("javascript:"), "javascript URL removed");
    assert!(joined.contains("<svg"), "document itself is still shown");
    assert_eq!(text.language.as_deref(), Some("svg"));
}

// ---------------------------------------------------------------------------
// HEX
// ---------------------------------------------------------------------------

#[test]
fn binary_preview_is_hexdump_of_first_4_kib() {
    let tmp = TempDir::new();
    let mut bytes = vec![b'A'; 5000];
    bytes[1] = 0;
    let path = tmp.file("blob.bin", &bytes);

    match Service::new().preview(&path) {
        Preview::Hex(hex) => {
            assert_eq!(hex.total, 5000, "total is the file length, not the head");
            assert_eq!(hex.rows.len(), 256, "4 KiB / 16 bytes");
            assert_eq!(hex.rows[0].offset, 0);
            assert_eq!(hex.rows[1].offset, 16);
            assert_eq!(hex.rows[255].offset, 4080);
            assert_eq!(hex.rows[0].ascii, "A·AAAAAAAAAAAAAA");
            assert!(hex.rows[0].hex.starts_with("41 00 41"));
            assert!(hex.rows.iter().all(|row| row.hex.split(' ').count() == 16));
        }
        other => panic!("expected HEX, got {}", other.kind()),
    }
}

#[test]
fn unknown_binary_extension_falls_back_to_hex() {
    let tmp = TempDir::new();
    let path = tmp.file("mystery.weird", &[0u8, 1, 2, 3, 0, 255]);
    match Service::new().preview(&path) {
        Preview::Hex(hex) => {
            assert_eq!(hex.total, 6);
            assert_eq!(hex.rows.len(), 1);
        }
        other => panic!("expected HEX, got {}", other.kind()),
    }
}

// ---------------------------------------------------------------------------
// IMAGE
// ---------------------------------------------------------------------------

#[test]
fn png_preview_downsizes_to_512_and_returns_rgba() {
    let tmp = TempDir::new();
    let source = image::RgbaImage::from_fn(800, 400, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 77, 200])
    });
    let path = tmp.path().join("wide.png");
    source.save(&path).expect("encode png fixture");

    match Service::new().preview(&path) {
        Preview::Image(image) => {
            assert_eq!((image.source_width, image.source_height), (800, 400));
            assert_eq!((image.width, image.height), (512, 256));
            assert_eq!(image.rgba.len(), 512 * 256 * 4);
            assert_eq!(image.mime, "image/png");
        }
        other => panic!("expected IMAGE, got {}", other.kind()),
    }
}

#[test]
fn oversized_image_dimensions_fall_back_to_meta() {
    let tmp = TempDir::new();
    // 9 MP is over the 8 MP decode cap even though the file itself is small.
    let source = image::RgbaImage::from_pixel(3000, 3000, image::Rgba([9, 8, 7, 255]));
    let path = tmp.path().join("huge.png");
    source.save(&path).expect("encode oversized png fixture");

    assert_eq!(Service::new().preview(&path).kind(), "META");
}

// ---------------------------------------------------------------------------
// LIST
// ---------------------------------------------------------------------------

#[test]
fn folder_listing_puts_directories_first_then_files_by_size() {
    let tmp = TempDir::new();
    tmp.subdir("beta_dir");
    tmp.subdir("alpha_dir");
    tmp.file("small.bin", &[0u8; 3]);
    tmp.file("big.bin", &[0u8; 20]);

    match Service::new().preview(tmp.path()) {
        Preview::List(list) => {
            assert_eq!(list.kind, "folder");
            assert!(!list.truncated);
            assert_eq!(list.total, Some(4));
            assert_eq!(
                names(&list),
                vec!["alpha_dir", "beta_dir", "big.bin", "small.bin"]
            );
            assert_eq!(list.entries[0].kind, ListKind::Dir);
            assert_eq!(list.entries[2].kind, ListKind::File);
            assert_eq!(list.entries[2].size, Some(20));
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn folder_listing_is_capped_at_200_rows() {
    let tmp = TempDir::new();
    for i in 0..210 {
        tmp.file(&format!("file-{i:03}.txt"), &[b'x'; 4]);
    }

    match Service::new().preview(tmp.path()) {
        Preview::List(list) => {
            assert_eq!(list.entries.len(), 200);
            assert!(list.truncated);
            assert_eq!(list.total, Some(210));
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn zip_listing_reads_the_central_directory_without_extracting() {
    let tmp = TempDir::new();
    let path = tmp.path().join("bundle.zip");
    make_zip(
        &path,
        &[("docs/readme.txt", b"hello"), ("image.bin", &[0u8; 32])],
    );

    match Service::new().preview(&path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "zip");
            assert!(!list.truncated);
            assert_eq!(list.total, Some(2));
            assert_eq!(names(&list), vec!["docs/readme.txt", "image.bin"]);
            assert_eq!(list.entries[0].size, Some(5));
            assert_eq!(list.entries[0].method.as_deref(), Some("Store"));
            assert_eq!(list.entries[1].kind, ListKind::File);
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn spreadsheet_listing_shows_sheet_names() {
    let tmp = TempDir::new();
    let path = tmp.path().join("book.xlsx");
    let workbook = br#"<?xml version="1.0"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
    <sheet name="Data &amp; More" sheetId="2" r:id="rId2"/>
  </sheets>
</workbook>"#;
    make_zip(&path, &[("xl/workbook.xml", workbook)]);

    match Service::new().preview(&path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "sheet");
            assert_eq!(names(&list), vec!["Sheet1", "Data & More"]);
            assert!(!list.truncated);
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn docx_listing_shows_heading_text_and_level() {
    let tmp = TempDir::new();
    let path = tmp.path().join("notes.docx");
    let document = br#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
    <w:p><w:r><w:t>Body text is not a heading.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Details &amp; notes</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
    make_zip(&path, &[("word/document.xml", document)]);

    match Service::new().preview(&path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "office");
            assert_eq!(names(&list), vec!["Overview", "Details & notes"]);
            assert_eq!(list.entries[0].method.as_deref(), Some("Heading 1"));
            assert_eq!(list.entries[1].method.as_deref(), Some("Heading 2"));
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn ods_and_odt_listings_read_their_native_xml() {
    let tmp = TempDir::new();

    let ods_path = tmp.path().join("book.ods");
    let sheet_xml = br#"<?xml version="1.0"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
                         xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
  <office:body><office:spreadsheet>
    <table:table table:name="Summary"/>
    <table:table table:name="Raw &amp; Extra"/>
  </office:spreadsheet></office:body>
</office:document-content>"#;
    make_zip(&ods_path, &[("content.xml", sheet_xml)]);

    match Service::new().preview(&ods_path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "sheet");
            assert_eq!(names(&list), vec!["Summary", "Raw & Extra"]);
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }

    let odt_path = tmp.path().join("notes.odt");
    let text_xml = br#"<?xml version="1.0"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
                         xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body><office:text>
    <text:h text:outline-level="1">Chapter One</text:h>
    <text:p>Body text is not a heading.</text:p>
    <text:h text:outline-level="2">Details &amp; More</text:h>
  </office:text></office:body>
</office:document-content>"#;
    make_zip(&odt_path, &[("content.xml", text_xml)]);

    match Service::new().preview(&odt_path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "office");
            assert_eq!(names(&list), vec!["Chapter One", "Details & More"]);
            assert_eq!(list.entries[0].method.as_deref(), Some("Heading 1"));
            assert_eq!(list.entries[1].method.as_deref(), Some("Heading 2"));
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn tar_listing_walks_headers_without_extracting() {
    let tmp = TempDir::new();
    let fixture = tar_fixture(&[
        ("docs/", b'5', b""),
        ("docs/readme.txt", b'0', b"hello"),
        ("link", b'2', b""),
    ]);
    let path = tmp.file("bundle.tar", &fixture);

    match Service::new().preview(&path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "tar");
            assert!(!list.truncated);
            assert_eq!(list.total, Some(3));
            assert_eq!(names(&list), vec!["docs", "docs/readme.txt", "link"]);
            assert_eq!(list.entries[0].kind, ListKind::Dir);
            assert_eq!(list.entries[1].size, Some(5));
            assert_eq!(list.entries[2].method.as_deref(), Some("symlink"));
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

#[test]
fn iso_listing_reads_the_primary_volume_root() {
    let tmp = TempDir::new();
    let path = tmp.file("disc.iso", &iso_fixture());

    match Service::new().preview(&path) {
        Preview::List(list) => {
            assert_eq!(list.kind, "iso");
            assert!(!list.truncated);
            assert_eq!(list.total, Some(2));
            assert_eq!(names(&list), vec!["HELLO.TXT", "SUBDIR"]);
            assert_eq!(list.entries[0].size, Some(123));
            assert_eq!(list.entries[1].kind, ListKind::Dir);
        }
        other => panic!("expected LIST, got {}", other.kind()),
    }
}

// ---------------------------------------------------------------------------
// DB
// ---------------------------------------------------------------------------

#[test]
fn sqlite_preview_is_read_only_and_bounded() {
    let tmp = TempDir::new();
    let path = tmp.path().join("sample.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("create sqlite");
        conn.execute_batch(
            "CREATE TABLE users (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL,
                 avatar BLOB
             );",
        )
        .expect("create table");
        let mut insert = conn
            .prepare("INSERT INTO users (name, avatar) VALUES (?1, ?2)")
            .expect("prepare insert");
        for i in 0..8 {
            let name = if i == 1 {
                "x".repeat(300)
            } else {
                format!("user {i}")
            };
            let blob: Option<Vec<u8>> = if i == 0 { Some(vec![0u8; 1024]) } else { None };
            insert
                .execute(rusqlite::params![name, blob])
                .expect("insert");
        }
    }
    let before = fs::read(&path).expect("read db before");

    match Service::new().preview(&path) {
        Preview::Db(db) => {
            assert!(!db.truncated);
            assert_eq!(db.tables.len(), 1);
            let table = &db.tables[0];
            assert_eq!(table.name, "users");
            assert_eq!(table.cols, 3);
            assert_eq!(
                table.columns.as_deref(),
                Some(&["id".to_string(), "name".to_string(), "avatar".to_string()][..])
            );
            let rows = table.rows.as_ref().expect("sampled rows");
            assert_eq!(rows.len(), 5, "row sample cap");
            assert_eq!(rows[0].len(), 3);
            assert_eq!(rows[0][2], "<blob 1024 B>");
            assert!(rows[1][1].chars().count() <= 65, "cell display cap");
        }
        other => panic!("expected DB, got {}", other.kind()),
    }

    let after = fs::read(&path).expect("read db after");
    assert_eq!(
        before, after,
        "opening a preview must not modify the database"
    );
}

// ---------------------------------------------------------------------------
// META / malformed input
// ---------------------------------------------------------------------------

#[test]
fn empty_and_malformed_files_degrade_without_panicking() {
    let tmp = TempDir::new();
    let service = Service::new();

    let empty = tmp.file("empty.txt", b"");
    match service.preview(&empty) {
        Preview::Meta(meta) => {
            assert_eq!(pair_value(&meta.pairs, "format"), Some("empty"));
            assert_eq!(pair_value(&meta.pairs, "size"), Some("0 B"));
        }
        other => panic!("expected META for empty file, got {}", other.kind()),
    }

    let broken_png = tmp.file("broken.png", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3]);
    assert_eq!(service.preview(&broken_png).kind(), "META");

    let broken_zip = tmp.file("broken.zip", b"PK\x03\x04 this is not a zip");
    assert_eq!(service.preview(&broken_zip).kind(), "META");

    let broken_db = tmp.file(
        "broken.db",
        b"SQLite format 3\0this is not actually a database at all",
    );
    assert_eq!(service.preview(&broken_db).kind(), "META");

    let missing = tmp.path().join("does-not-exist.txt");
    match service.preview(&missing) {
        Preview::Unavailable { reason } => assert_eq!(reason, "not found"),
        other => panic!("expected UNAVAILABLE, got {}", other.kind()),
    }

    match service.preview(Path::new("")) {
        Preview::Unavailable { reason } => assert_eq!(reason, "empty path"),
        other => panic!("expected UNAVAILABLE, got {}", other.kind()),
    }
}

#[test]
fn pdf_meta_reports_pages_from_a_bounded_scan() {
    let tmp = TempDir::new();
    let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                2 0 obj\n<< /Type /Pages /Count 2 >>\nendobj\n\
                3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
                4 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n%%EOF";
    let path = tmp.file("two-pages.pdf", pdf);
    let meta = meta_of(Service::new().preview(&path));
    assert_eq!(pair_value(&meta.pairs, "format"), Some("pdf 1.4"));
    assert_eq!(pair_value(&meta.pairs, "pages"), Some("2"));
}

#[test]
fn elf_meta_reports_architecture_and_kind() {
    let tmp = TempDir::new();
    let mut elf = vec![0u8; 64];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little-endian
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    elf[18..20].copy_from_slice(&0x3Eu16.to_le_bytes()); // x86-64
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[58..60].copy_from_slice(&64u16.to_le_bytes());
    let path = tmp.file("program.elf", &elf);

    let meta = meta_of(Service::new().preview(&path));
    assert_eq!(pair_value(&meta.pairs, "format"), Some("ELF64 LSB"));
    assert_eq!(pair_value(&meta.pairs, "arch"), Some("x86-64"));
    assert_eq!(pair_value(&meta.pairs, "kind"), Some("executable"));
    assert_eq!(pair_value(&meta.pairs, "entry"), Some("0x401000"));
}

#[test]
fn pe_meta_reads_the_optional_header_and_sections() {
    let tmp = TempDir::new();
    let mut pe = vec![0u8; 0x170];
    pe[0] = b'M';
    pe[1] = b'Z';
    pe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    pe[0x40..0x44].copy_from_slice(b"PE\0\0");
    pe[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes()); // x64
    pe[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // one section
    pe[0x48..0x4A].copy_from_slice(&240u16.to_le_bytes()); // optional size
    pe[0x4A..0x4C].copy_from_slice(&0x0022u16.to_le_bytes()); // executable
    pe[0x58..0x5A].copy_from_slice(&0x20Bu16.to_le_bytes()); // PE32+
    pe[0x58 + 56..0x58 + 60].copy_from_slice(&0x2000u32.to_le_bytes());
    pe[0x58 + 68..0x58 + 70].copy_from_slice(&3u16.to_le_bytes()); // console
    pe[0x148..0x14D].copy_from_slice(b".text");
    let path = tmp.file("program.exe", &pe);

    let meta = meta_of(Service::new().preview(&path));
    assert_eq!(pair_value(&meta.pairs, "format"), Some("PE32+"));
    assert_eq!(pair_value(&meta.pairs, "arch"), Some("x64"));
    assert_eq!(pair_value(&meta.pairs, "kind"), Some("executable"));
    assert_eq!(
        pair_value(&meta.pairs, "subsystem"),
        Some("windows console")
    );
    assert_eq!(pair_value(&meta.pairs, "section names"), Some(".text"));
    assert_eq!(pair_value(&meta.pairs, "image size"), Some("8.0 KB"));
}

#[test]
fn font_meta_reads_the_name_table_family() {
    let tmp = TempDir::new();
    let name = name_table("Test Font");
    let name_offset = 28u32;
    let mut ttf = vec![0u8; name_offset as usize + name.len()];
    ttf[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    ttf[4..6].copy_from_slice(&1u16.to_be_bytes());
    ttf[12..16].copy_from_slice(b"name");
    ttf[20..24].copy_from_slice(&name_offset.to_be_bytes());
    ttf[24..28].copy_from_slice(&(name.len() as u32).to_be_bytes());
    ttf[name_offset as usize..].copy_from_slice(&name);
    let path = tmp.file("text.ttf", &ttf);

    let meta = meta_of(Service::new().preview(&path));
    assert_eq!(pair_value(&meta.pairs, "format"), Some("TrueType"));
    assert_eq!(pair_value(&meta.pairs, "family"), Some("Test Font"));
}

fn name_table(family: &str) -> Vec<u8> {
    let encoded: Vec<u16> = family.encode_utf16().collect();
    let string_bytes = encoded.len() * 2;
    let mut table = vec![0u8; 18 + string_bytes];
    table[2..4].copy_from_slice(&1u16.to_be_bytes()); // one name record
    table[4..6].copy_from_slice(&18u16.to_be_bytes()); // string offset
    table[6..8].copy_from_slice(&3u16.to_be_bytes()); // Windows platform
    table[8..10].copy_from_slice(&1u16.to_be_bytes()); // Unicode BMP
    table[10..12].copy_from_slice(&0x0409u16.to_be_bytes());
    table[12..14].copy_from_slice(&1u16.to_be_bytes()); // family
    table[14..16].copy_from_slice(&(string_bytes as u16).to_be_bytes());
    for (i, unit) in encoded.iter().enumerate() {
        table[18 + i * 2..20 + i * 2].copy_from_slice(&unit.to_be_bytes());
    }
    table
}

#[test]
fn symlinks_are_reported_and_not_followed() {
    let tmp = TempDir::new();
    let target = tmp.file("target.txt", b"secret");
    let link = tmp.path().join("link.txt");
    if !try_symlink_file(&target, &link) {
        // Creating symlinks needs developer mode/admin on Windows. The check
        // below is still exercised on systems where it is permitted.
        return;
    }

    match Service::new().preview(&link) {
        Preview::Unavailable { reason } => {
            assert!(reason.contains("symlink"), "reason: {reason}");
        }
        other => panic!("expected UNAVAILABLE, got {}", other.kind()),
    }
}

#[cfg(windows)]
fn try_symlink_file(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(unix)]
fn try_symlink_file(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn unreadable_file_degrades_safely() {
    let tmp = TempDir::new();
    let path = tmp.file("locked.txt", b"this file will be held with no sharing");

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .expect("open exclusive lock");
        let reason = degraded_reason(Service::new().preview(&path));
        assert!(
            reason.contains("file in use") || reason.contains("access denied"),
            "reason: {reason}"
        );
        drop(lock);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original = fs::metadata(&path).expect("metadata").permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");
        let reason = degraded_reason(Service::new().preview(&path));
        assert!(
            reason.contains("access denied") || reason.contains("permission"),
            "reason: {reason}"
        );
        fs::set_permissions(&path, original).expect("restore chmod");
    }
}

fn degraded_reason(preview: Preview) -> String {
    match preview {
        Preview::Meta(meta) => meta.note.unwrap_or_default(),
        Preview::Unavailable { reason } => reason,
        other => panic!("expected degraded META/UNAVAILABLE, got {}", other.kind()),
    }
}

// ---------------------------------------------------------------------------
// MEDIA
// ---------------------------------------------------------------------------

#[test]
fn wav_preview_reads_metadata_and_states_the_thumbnail_limit() {
    let tmp = TempDir::new();
    let path = tmp.file("tone.wav", &wav_header(2));

    match Service::new().preview(&path) {
        Preview::Media(media) => {
            assert_eq!(media.kind, "audio");
            assert_eq!(media.mime, "audio/wav");
            assert!(media.thumbnail.is_none());
            assert_eq!(pair_value(&media.pairs, "sample rate"), Some("44100 Hz"));
            assert_eq!(pair_value(&media.pairs, "channels"), Some("2"));
            assert_eq!(pair_value(&media.pairs, "duration"), Some("2.0 s"));
            assert!(media.note.is_some());
        }
        other => panic!("expected MEDIA, got {}", other.kind()),
    }
}

#[test]
fn mp3_preview_decodes_embedded_cover_art() {
    let tmp = TempDir::new();
    let path = tmp.file("tagged.mp3", &mp3_with_png_cover());

    match Service::new().preview(&path) {
        Preview::Media(media) => {
            assert_eq!(media.kind, "audio");
            assert_eq!(media.mime, "audio/mpeg");
            let thumbnail = media.thumbnail.expect("embedded cover art");
            assert_eq!((thumbnail.width, thumbnail.height), (8, 8));
            assert_eq!((thumbnail.source_width, thumbnail.source_height), (8, 8));
            assert_eq!(thumbnail.mime, "image/png");
            assert_eq!(thumbnail.rgba.len(), 8 * 8 * 4);
            assert!(media.note.is_none());
            assert_eq!(pair_value(&media.pairs, "format"), Some("mp3 (id3v2.3)"));
        }
        other => panic!("expected MEDIA, got {}", other.kind()),
    }
}

#[test]
fn video_preview_is_honest_about_missing_thumbnails() {
    let tmp = TempDir::new();
    let path = tmp.file("clip.mp4", &minimal_mp4());

    match Service::new().preview(&path) {
        Preview::Media(media) => {
            assert_eq!(media.kind, "video");
            assert_eq!(media.mime, "video/mp4");
            assert!(media.thumbnail.is_none());
            let note = media.note.expect("honest note");
            assert!(
                note.contains("No video thumbnail"),
                "unexpected note: {note}"
            );
        }
        other => panic!("expected MEDIA, got {}", other.kind()),
    }
}

fn wav_header(seconds: u32) -> Vec<u8> {
    let sample_rate = 44_100u32;
    let channels = 2u16;
    let bits = 16u16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits) / 8;
    let data_len = byte_rate * seconds;
    let mut out = Vec::with_capacity(44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out
}

fn mp3_with_png_cover() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 34, 56, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode cover png");

    let mut apic = Vec::new();
    apic.push(0); // ISO-8859-1 text encoding
    apic.extend_from_slice(b"image/png\0");
    apic.push(3); // front cover
    apic.push(0); // empty description
    apic.extend_from_slice(&png);

    let mut tag_body = Vec::new();
    tag_body.extend_from_slice(b"APIC");
    tag_body.extend_from_slice(&(apic.len() as u32).to_be_bytes());
    tag_body.extend_from_slice(&[0, 0]); // frame flags
    tag_body.extend_from_slice(&apic);

    let mut file = Vec::new();
    file.extend_from_slice(b"ID3");
    file.push(3);
    file.push(0);
    file.push(0);
    file.extend_from_slice(&synchsafe(tag_body.len() as u32));
    file.extend_from_slice(&tag_body);
    // No audio frames: the preview only promises header metadata + cover art.
    file
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn minimal_mp4() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&24u32.to_be_bytes());
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(b"isom");
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(b"isom");
    out.extend_from_slice(b"mp41");
    out
}

// ---------------------------------------------------------------------------
// Cache / no-writes
// ---------------------------------------------------------------------------

#[test]
fn cache_is_bounded_reused_and_shared_by_clones() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Service>();

    let tmp = TempDir::new();
    let service = Service::new();
    assert_eq!(service.cache_len(), 0);

    let first = tmp.file("first.txt", b"alpha");
    assert_eq!(service.preview(&first).kind(), "TEXT");
    assert_eq!(service.cache_len(), 1);
    assert_eq!(service.preview(&first).kind(), "TEXT");
    assert_eq!(service.cache_len(), 1, "cache hit must not add an entry");

    // The cache key includes the length, so rewriting in place invalidates.
    tmp.file("first.txt", b"alpha beta gamma");
    assert_eq!(service.preview(&first).kind(), "TEXT");
    assert_eq!(service.cache_len(), 2);

    let cloned = service.clone();
    for i in 0..300 {
        let path = tmp.file(&format!("cache-{i:03}.txt"), format!("file {i}").as_bytes());
        assert_eq!(service.preview(&path).kind(), "TEXT");
        assert!(
            service.cache_len() <= 256,
            "cache grew to {} entries",
            service.cache_len()
        );
    }
    assert_eq!(service.cache_len(), 256, "entry cap");
    assert_eq!(cloned.cache_len(), 256, "clones share the same cache");
}

#[test]
fn previewing_does_not_write_files_or_add_directory_entries() {
    let tmp = TempDir::new();
    let text = tmp.file("note.txt", b"hello preview\n");
    let image_path = tmp.path().join("pixel.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
        .save(&image_path)
        .expect("png fixture");
    let zip_path = tmp.path().join("pair.zip");
    make_zip(&zip_path, &[("one.txt", b"one")]);
    let db_path = tmp.path().join("ledger.db");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("sqlite fixture");
        conn.execute_batch("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1), (2);")
            .expect("sqlite data");
    }

    let before_names = dir_names(tmp.path());
    let before_bytes: Vec<Vec<u8>> = vec![
        fs::read(&text).unwrap(),
        fs::read(&image_path).unwrap(),
        fs::read(&zip_path).unwrap(),
        fs::read(&db_path).unwrap(),
    ];

    let service = Service::new();
    for path in [&text, &image_path, &zip_path, &db_path] {
        let _ = service.preview(path);
    }
    let _ = service.preview(tmp.path());

    assert_eq!(before_names, dir_names(tmp.path()), "no new files");
    let after_bytes: Vec<Vec<u8>> = vec![
        fs::read(&text).unwrap(),
        fs::read(&image_path).unwrap(),
        fs::read(&zip_path).unwrap(),
        fs::read(&db_path).unwrap(),
    ];
    assert_eq!(before_bytes, after_bytes, "preview never rewrites content");
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read fixture dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
