//! Engine contract tests: exact totals, categories, largest files, paths,
//! cancellation, read-only behaviour and clean failures. Everything is built
//! in a unique temporary directory; no user data is touched.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use space::engine::{classify_entry, classify_file, scan_blocking, spawn_scan};
use space::model::{Category, NO_NODE, NodeId, ScanEvent, ScanOutput, Tree};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let nonce = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // `%TEMP%` itself is a cache path, and classification is deliberately
        // path-aware. Put the test root beside the temp directory (under
        // LocalAppData on Windows, or the current directory as a portable
        // fallback) so the fixtures exercise ordinary path contexts.
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("space-engine-tests"))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.join(".space-engine-tests"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("space-engine-tests"));
        let path = base.join(format!(
            "space-engine-{}-{}-{}-{}",
            tag,
            std::process::id(),
            stamp,
            nonce
        ));
        fs::create_dir_all(&path).expect("create temp root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn mkdir(&self, relative: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, bytes).expect("write temp file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fresh_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn scan(root: &Path) -> ScanOutput {
    scan_blocking(root.to_path_buf(), None, fresh_cancel()).expect("scan should succeed")
}

fn find_child(tree: &Tree, parent: NodeId, name: &str) -> Option<NodeId> {
    tree.children(parent)
        .into_iter()
        .find(|id| tree.name(*id) == name)
}

fn assert_reconciles(tree: &Tree, output: &ScanOutput) {
    let summary = &output.summary;
    assert_eq!(summary.root_path, tree.root_path());
    assert_eq!(summary.total_bytes, tree.node(tree.root()).size);
    assert_eq!(summary.files, tree.node(tree.root()).file_count);

    let mut file_bytes = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut categories = space::model::CategoryTotals::default();

    for index in 0..tree.len() {
        let id = index as NodeId;
        let node = tree.node(id);
        if node.is_dir {
            if id != tree.root() {
                dir_count += 1;
            }
            let child_sum: u64 = tree
                .children(id)
                .iter()
                .map(|child| tree.node(*child).size)
                .sum();
            assert_eq!(
                child_sum,
                node.size,
                "directory {} does not sum its children",
                tree.name(id)
            );
        } else {
            file_bytes += node.size;
            file_count += 1;
            categories.add(node.cat, node.size);
        }
    }

    assert_eq!(summary.files, file_count);
    assert_eq!(summary.total_bytes, file_bytes);
    assert_eq!(summary.dirs, dir_count);
    assert_eq!(categories, summary.categories);
    assert_eq!(tree.category_totals(tree.root()), summary.categories);
    assert_eq!(summary.categories.total(), summary.total_bytes);

    assert!(!tree.is_empty());
    assert_eq!(tree.node(tree.root()).parent, NO_NODE);
    assert!(!tree.root_path().is_empty());
}

#[test]
fn scan_counts_reconciles_and_classifies_by_path() {
    let temp = TempDir::new("totals");
    temp.mkdir("empty");
    temp.mkdir("nested/deeper");
    temp.write("nested/deeper/deep.txt", b"deep");
    temp.write("zero.bin", b"");
    temp.write("caf\u{00e9}-\u{6587}\u{4ef6}-\u{1f642}.txt", b"unicode");
    temp.write("photo.jpg", &[7u8; 10]);
    temp.write("clip.mp4", &[7u8; 20]);
    temp.write("notes.pdf", b"pdf");
    temp.write("main.rs", &[7u8; 30]);
    temp.write("bundle.zip", &[7u8; 40]);
    temp.write("mystery", b"myst");
    temp.write("Documents/letter", b"letter");
    temp.write("Pictures/IMG_0001", b"picture");
    temp.write("node_modules/pkg/index.js", b"12345678");
    temp.write("Cache/blob", &[7u8; 9]);

    let output = scan(temp.path());
    let tree = &output.tree;
    let summary = &output.summary;

    assert_eq!(summary.files, 13);
    assert_eq!(summary.dirs, 8);
    assert_eq!(summary.total_bytes, 148);
    assert_eq!(summary.errors, 0);
    assert_reconciles(tree, &output);

    assert_eq!(summary.categories.get(Category::Media), 37);
    assert_eq!(summary.categories.get(Category::Documents), 20);
    assert_eq!(summary.categories.get(Category::Code), 38);
    assert_eq!(summary.categories.get(Category::Caches), 9);
    assert_eq!(summary.categories.get(Category::Archives), 40);
    assert_eq!(summary.categories.get(Category::System), 0);
    assert_eq!(summary.categories.get(Category::Apps), 0);
    assert_eq!(summary.categories.get(Category::Other), 4);

    let empty = find_child(tree, tree.root(), "empty").expect("empty directory node");
    assert!(tree.node(empty).is_dir);
    assert_eq!(tree.node(empty).size, 0);
    assert_eq!(tree.child_count(empty), 0);

    let zero = find_child(tree, tree.root(), "zero.bin").expect("zero-byte node");
    assert_eq!(tree.node(zero).size, 0);
    assert_eq!(tree.node(zero).file_count, 1);
    assert_eq!(tree.node(zero).cat, Category::Other);

    let unicode = find_child(
        tree,
        tree.root(),
        "caf\u{00e9}-\u{6587}\u{4ef6}-\u{1f642}.txt",
    )
    .expect("unicode node");
    assert_eq!(tree.node(unicode).size, 7);
    assert_eq!(tree.node(unicode).cat, Category::Documents);

    let pictures = find_child(tree, tree.root(), "Pictures").expect("Pictures node");
    let image = find_child(tree, pictures, "IMG_0001").expect("extensionless image");
    assert_eq!(tree.node(image).cat, Category::Media);

    let documents = find_child(tree, tree.root(), "Documents").expect("Documents node");
    let letter = find_child(tree, documents, "letter").expect("extensionless document");
    assert_eq!(tree.node(letter).cat, Category::Documents);

    let modules = find_child(tree, tree.root(), "node_modules").expect("node_modules node");
    let package = find_child(tree, modules, "pkg").expect("package node");
    let script = find_child(tree, package, "index.js").expect("script node");
    assert_eq!(tree.node(script).cat, Category::Code);

    let cache = find_child(tree, tree.root(), "Cache").expect("cache node");
    let blob = find_child(tree, cache, "blob").expect("extensionless cache blob");
    assert_eq!(tree.node(blob).cat, Category::Caches);

    // With fewer than 1000 files the list is all files, including zero bytes.
    let largest = tree.largest_files(2000);
    assert_eq!(largest.len(), 13);
    assert_eq!(tree.node(largest[0]).size, 40);
    assert_eq!(tree.node(*largest.last().unwrap()).size, 0);
    assert!(
        largest
            .windows(2)
            .all(|pair| { tree.node(pair[0]).size >= tree.node(pair[1]).size })
    );
}

#[test]
fn largest_files_are_exact_top_thousand() {
    let temp = TempDir::new("largest");
    for size in 1..=1005usize {
        temp.write(&format!("file-{size:04}.dat"), &vec![0u8; size]);
    }

    let output = scan(temp.path());
    assert_reconciles(&output.tree, &output);
    assert_eq!(output.summary.files, 1005);

    let largest = output.tree.largest_files(2000);
    assert_eq!(largest.len(), 1000);
    assert_eq!(output.tree.node(largest[0]).size, 1005);
    assert_eq!(output.tree.node(*largest.last().unwrap()).size, 6);

    let mut ids = HashSet::new();
    for id in largest {
        assert!(ids.insert(*id), "duplicate largest-file id");
        let node = output.tree.node(*id);
        assert!(!node.is_dir);
    }
    assert!(
        largest
            .windows(2)
            .all(|pair| { output.tree.node(pair[0]).size >= output.tree.node(pair[1]).size })
    );
    assert_eq!(output.tree.largest_files(10), &largest[..10]);
}

#[test]
fn classification_table_covers_context_rules() {
    use Category::*;

    let cases: &[(&str, &str, bool, Category)] = &[
        ("kernel32.dll", r"C:\Windows\System32", false, System),
        ("driver.sys", r"C:\Windows\System32\drivers", false, System),
        ("foo.exe", r"C:\Windows", false, Apps),
        ("setup.exe", r"C:\Program Files\Foo", false, Apps),
        ("app.dll", r"C:\Program Files\Foo", false, Apps),
        ("generated.dll", r"C:\proj\node_modules\pkg", false, Apps),
        ("cached.bin", r"C:\Windows\Temp", false, System),
        ("setup.dmg", r"D:\Downloads", false, Archives),
        (
            "update.tmp",
            r"C:\Users\u\AppData\Local\Temp",
            false,
            Caches,
        ),
        (
            "f_000001",
            r"C:\Users\u\AppData\Local\Google\Chrome\User Data\Default\Cache",
            false,
            Caches,
        ),
        ("index.js", r"C:\proj\node_modules\pkg", false, Code),
        ("photo.jpg", r"D:\", false, Media),
        ("report.pdf", r"D:\", false, Documents),
        ("archive.zip", r"D:\", false, Archives),
        ("pagefile.sys", r"C:\", false, System),
        ("ntuser.dat", r"C:\Users\u", false, System),
        ("SYSTEM", r"C:\Windows\System32\config", false, System),
        ("noext", r"D:\misc", false, Other),
        ("README", r"C:\Users\u\Documents", false, Documents),
        ("IMG_0001", r"D:\Pictures", false, Media),
        ("notes", r"D:\Archives", false, Archives),
        ("Makefile", r"C:\proj", false, Code),
        (".gitignore", r"C:\proj", false, Code),
        ("data.bin", r"D:\", false, Other),
        (
            "thumbcache_256.db",
            r"C:\Users\u\AppData\Local",
            false,
            Caches,
        ),
        // Byte offsets 6 and len-3 must never split a UTF-8 character.
        ("\u{00e9}\u{20ac}\u{20ac}a.db", r"D:\misc", false, Other),
        (
            "\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}.db",
            r"D:\misc",
            false,
            Other,
        ),
        (
            "\u{0444}\u{043e}\u{0442}\u{043e}.JPG",
            r"D:\misc",
            false,
            Media,
        ),
        ("node_modules", r"C:\proj", true, Code),
        ("Temp", r"C:\Users\u\AppData\Local", true, Caches),
        ("Program Files", r"C:\", true, Apps),
        ("Windows", r"C:\", true, System),
        ("ProgramData", r"C:\", true, System),
        ("Documents", r"C:\Users\u", true, Documents),
        ("Pictures", r"C:\Users\u", true, Media),
        ("src", r"C:\proj", true, Code),
        ("foo", r"C:\proj", true, Other),
    ];

    for (name, parent, is_dir, expected) in cases {
        let actual = classify_entry(name, Path::new(parent), *is_dir);
        assert_eq!(
            actual, *expected,
            "classify_entry({name:?}, {parent:?}, is_dir={is_dir})"
        );
        if !*is_dir {
            assert_eq!(classify_file(name, Path::new(parent)), *expected);
        }
    }
}

#[test]
fn known_extensions_win_over_folder_context() {
    use Category::*;
    let files = [
        ("photo.JPG", Media),
        ("notes.txt", Documents),
        ("readme.MD", Documents),
        ("main.rs", Code),
        ("libspace.rmeta", Code),
        ("compiler.o", Code),
        ("compiler.OBJ", Code),
        ("compiler.lib", Code),
        ("setup.exe", Apps),
        ("package.crate", Archives),
        ("bundle.zip", Archives),
        ("stale.tmp", Caches),
        ("driver.sys", System),
    ];
    for parent in [
        r"C:\project\target\debug",
        r"C:\project\node_modules\package",
        r"C:\Users\u\.cargo\registry",
        r"C:\Users\u\AppData\Local\Temp",
        r"C:\Program Files\App",
        r"C:\Windows\System32",
    ] {
        for (name, expected) in files {
            assert_eq!(
                classify_file(name, Path::new(parent)),
                expected,
                "{parent} / {name}"
            );
        }
    }
    for name in ["artifact", "artifact.unknown", "artifact.扩展名"] {
        for parent in [
            r"C:\project\target",
            r"C:\project\node_modules",
            r"C:\Users\u\.cargo",
        ] {
            assert_eq!(classify_file(name, Path::new(parent)), Code);
        }
        assert_eq!(classify_file(name, Path::new(r"D:\misc")), Other);
    }
    assert_eq!(
        classify_entry("photos.jpg", Path::new(r"D:\misc"), true),
        Other
    );
}

#[test]
fn dlls_are_system_only_inside_the_windows_tree() {
    for parent in [
        r"C:\Windows",
        r"c:\wInDoWs\Temp",
        r"\\?\C:\Windows\System32",
        r"\\server\share\Windows\System32",
        r"\\?\UNC\server\share\Windows\System32",
    ] {
        assert_eq!(
            classify_file("library.DLL", Path::new(parent)),
            Category::System,
            "{parent}"
        );
    }
    for parent in [
        r"D:\misc",
        r"C:\project\target\debug",
        r"C:\project\drivers",
        r"C:\project\System32",
        r"C:\project\Windows",
        r"C:\WindowsApps\Package",
        r"C:\ProgramData\App",
    ] {
        assert_eq!(
            classify_file("library.dll", Path::new(parent)),
            Category::Apps,
            "{parent}"
        );
    }
}

#[test]
fn scanner_preserves_file_types_inside_build_folders() {
    let temp = TempDir::new("build-types");
    let files = [
        ("target/photo.png", Category::Media),
        ("target/notes.txt", Category::Documents),
        ("target/metadata.rmeta", Category::Code),
        ("target/compiler.obj", Category::Code),
        ("target/program.dll", Category::Apps),
        ("node_modules/pkg/archive.zip", Category::Archives),
        ("node_modules/pkg/license.md", Category::Documents),
        (".cargo/registry/package.crate", Category::Archives),
        (".cargo/registry/artifact.unrecognized", Category::Code),
    ];
    for (path, _) in files {
        temp.write(path, b"12345678");
    }
    let output = scan(temp.path());
    assert_reconciles(&output.tree, &output);
    for (relative, expected) in files {
        let path = temp.path().join(relative);
        let id = (0..output.tree.len())
            .map(|index| index as NodeId)
            .find(|id| output.tree.path(*id) == path)
            .expect("scanned file");
        assert_eq!(output.tree.node(id).cat, expected, "{relative}");
    }
}

#[test]
fn empty_root_scans_to_a_single_node() {
    let temp = TempDir::new("empty-root");
    let output = scan(temp.path());
    assert_eq!(output.summary.files, 0);
    assert_eq!(output.summary.dirs, 0);
    assert_eq!(output.summary.total_bytes, 0);
    assert_eq!(output.summary.errors, 0);
    assert_eq!(output.tree.len(), 1);
    assert!(output.tree.largest_files(1000).is_empty());
    assert_reconciles(&output.tree, &output);
}

#[test]
fn invalid_roots_fail_cleanly() {
    let temp = TempDir::new("invalid");
    let missing = temp.path().join("does-not-exist");
    let file = temp.write("not-a-folder.txt", b"x");

    let error = scan_blocking(missing.clone(), None, fresh_cancel()).unwrap_err();
    assert!(!error.is_empty());

    let error = scan_blocking(file.clone(), None, fresh_cancel()).unwrap_err();
    assert!(!error.is_empty());

    let (tx, _rx) = mpsc::channel();
    assert!(spawn_scan(missing, tx, fresh_cancel()).is_err());

    let (tx, _rx) = mpsc::channel();
    assert!(spawn_scan(file, tx, fresh_cancel()).is_err());
}

#[test]
fn scan_is_read_only() {
    let temp = TempDir::new("read-only");
    temp.write("a.txt", b"alpha");
    temp.write("nested/b.bin", &[1u8; 32]);
    temp.write("nested/c.dat", b"gamma");

    let paths = [
        temp.path().join("a.txt"),
        temp.path().join("nested/b.bin"),
        temp.path().join("nested/c.dat"),
    ];
    let before: Vec<(Vec<u8>, Option<SystemTime>)> = paths
        .iter()
        .map(|path| {
            (
                fs::read(path).expect("read before"),
                fs::metadata(path)
                    .ok()
                    .and_then(|meta| meta.modified().ok()),
            )
        })
        .collect();
    let entries_before = fs::read_dir(temp.path()).unwrap().count();

    let output = scan(temp.path());
    assert_reconciles(&output.tree, &output);

    for (path, (contents, modified)) in paths.iter().zip(before) {
        assert_eq!(fs::read(path).expect("read after"), contents);
        let after_modified = fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok());
        assert_eq!(after_modified, modified, "mtime changed for {path:?}");
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), entries_before);
}

#[cfg(windows)]
#[test]
fn junctions_are_not_traversed() {
    let temp = TempDir::new("junction");
    temp.write("payload/real.txt", b"real");
    let target = temp.path().join("payload");
    let junction = temp.path().join("shortcut");

    // Junctions do not require the symlink privilege, but `mklink` is a cmd
    // built-in rather than a Rust API. Skip cleanly where it is unavailable.
    let created = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(&target)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !created {
        eprintln!("skipping junction test: mklink /J is unavailable");
        return;
    }

    let output = scan(temp.path());
    assert_eq!(output.summary.files, 1);
    assert_eq!(output.summary.dirs, 1);
    assert_eq!(output.summary.total_bytes, 4);
    assert_reconciles(&output.tree, &output);
    assert!(
        find_child(&output.tree, output.tree.root(), "shortcut").is_none(),
        "a directory reparse point became a tree node"
    );
    let payload = find_child(&output.tree, output.tree.root(), "payload").expect("payload");
    assert!(find_child(&output.tree, payload, "real.txt").is_some());
}

#[cfg(windows)]
#[test]
fn denied_entries_are_counted_and_never_fatal() {
    use std::process::Command;

    let temp = TempDir::new("denied-entry");
    temp.write("visible/ok.txt", b"ok");
    let denied = temp.mkdir("denied");
    fs::write(denied.join("secret.txt"), b"secret").expect("denied fixture");

    // Deny Everyone rather than the current user so the test still exercises a
    // real ACL failure under an elevated runner. The fixture is created first.
    let status = Command::new("icacls")
        .arg(&denied)
        .arg("/inheritance:r")
        .arg("/deny")
        .arg("*S-1-1-0:(OI)(CI)(RX)")
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        let _ = Command::new("icacls").arg(&denied).arg("/reset").status();
        eprintln!("skipping denied-entry test: icacls is unavailable");
        return;
    }

    let output = scan(temp.path());
    let reset = Command::new("icacls").arg(&denied).arg("/reset").status();
    assert!(
        matches!(reset, Ok(status) if status.success()),
        "restore fixture ACL before assertions"
    );

    assert_eq!(output.summary.files, 1, "only the readable file remains");
    assert_eq!(output.summary.total_bytes, 2);
    assert!(
        output.summary.errors >= 1,
        "denied directory must be counted"
    );
    assert_reconciles(&output.tree, &output);
}

#[test]
fn long_paths_are_scanned() {
    let temp = TempDir::new("long");
    let mut relative = String::new();
    for level in 0..8 {
        if !relative.is_empty() {
            relative.push('\\');
        }
        relative.push_str(&format!("level-{level:02}-{}", "x".repeat(36)));
    }
    let file = format!("{relative}\\deep.txt");
    temp.write(&file, b"long");
    assert!(temp.path().join(&file).to_string_lossy().len() > 300);

    let output = scan(temp.path());
    assert_eq!(output.summary.files, 1);
    assert_eq!(output.summary.total_bytes, 4);
    assert_reconciles(&output.tree, &output);

    let mut id = output.tree.root();
    for level in 0..8 {
        let name = format!("level-{level:02}-{}", "x".repeat(36));
        id = find_child(&output.tree, id, &name).expect("long-path directory");
    }
    let file_id = find_child(&output.tree, id, "deep.txt").expect("long-path file");
    assert_eq!(output.tree.node(file_id).size, 4);
}

#[test]
fn cancellation_before_start_is_immediate() {
    let temp = TempDir::new("cancel-pre");
    temp.write("file.txt", b"x");
    let cancel = Arc::new(AtomicBool::new(true));
    let started = Instant::now();
    let error = scan_blocking(temp.path().to_path_buf(), None, cancel).unwrap_err();
    assert!(error.to_lowercase().contains("cancel"));
    assert!(started.elapsed() < Duration::from_millis(150));
}

#[test]
fn cancellation_stops_a_running_scan_promptly() {
    let temp = TempDir::new("cancel-running");
    for directory in 0..300 {
        for file in 0..60 {
            temp.write(&format!("d{directory:03}/f{file:03}.dat"), &[0u8; 64]);
        }
    }

    let (tx, rx) = mpsc::channel();
    let handle = spawn_scan(temp.path().to_path_buf(), tx, fresh_cancel()).expect("spawn");
    std::thread::sleep(Duration::from_millis(2));
    let cancel_at = Instant::now();
    handle.cancel.store(true, Ordering::Relaxed);

    let mut terminal = 0usize;
    let mut saw_failed = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(ScanEvent::Progress(progress)) => {
                assert!(!progress.current.is_empty());
            }
            Ok(ScanEvent::Done(output)) => {
                terminal += 1;
                assert_reconciles(&output.tree, &output);
                break;
            }
            Ok(ScanEvent::Failed(message)) => {
                terminal += 1;
                saw_failed = true;
                assert!(message.to_lowercase().contains("cancel"), "{message}");
                break;
            }
            Err(error) => panic!("no terminal event: {error}"),
        }
    }

    handle.join.join().expect("scan thread");
    assert_eq!(terminal, 1);
    let elapsed = cancel_at.elapsed();
    assert!(
        elapsed < Duration::from_millis(150),
        "cancel took {elapsed:?}"
    );
    // A second terminal event is never allowed.
    assert!(rx.try_recv().is_err());
    // On a machine fast enough to finish 18,000 tiny files in 2 ms the result
    // is a legitimate Done; otherwise cancellation must have been reported.
    if !saw_failed {
        eprintln!("note: scan completed before cancellation was observed");
    }
}

#[test]
fn spawn_scan_sends_progress_then_one_terminal_event() {
    let temp = TempDir::new("events");
    for index in 0..40 {
        temp.write(&format!("d{index:02}/f{index:02}.txt"), b"hello");
    }

    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    let handle = spawn_scan(temp.path().to_path_buf(), tx, fresh_cancel()).expect("spawn");
    handle.join.join().expect("scan thread");

    let mut progress_count = 0usize;
    let output = loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(ScanEvent::Progress(progress)) => {
                progress_count += 1;
                assert!(!progress.current.is_empty());
                assert!(progress.files <= 40);
            }
            Ok(ScanEvent::Done(output)) => break output,
            Ok(ScanEvent::Failed(message)) => panic!("scan failed: {message}"),
            Err(error) => panic!("no terminal event: {error}"),
        }
    };

    assert!(rx.try_recv().is_err(), "a second terminal event arrived");
    assert_reconciles(&output.tree, &output);
    assert_eq!(output.summary.files, 40);
    assert_eq!(output.summary.total_bytes, 200);

    // At most 8 progress reports per second, plus scheduling slack of one.
    let elapsed = started.elapsed();
    let allowed = (elapsed.as_millis() / 125) as usize + 2;
    assert!(
        progress_count <= allowed,
        "{progress_count} progress events in {elapsed:?}"
    );

    // `scan_blocking` is the same walker without an event channel.
    let blocking = scan(temp.path());
    assert_eq!(blocking.summary.files, output.summary.files);
    assert_eq!(blocking.summary.dirs, output.summary.dirs);
    assert_eq!(blocking.summary.total_bytes, output.summary.total_bytes);
    assert_eq!(blocking.summary.categories, output.summary.categories);
}

#[test]
fn disappearing_entries_do_not_panic() {
    let temp = TempDir::new("churn");
    for directory in 0..40 {
        for file in 0..25 {
            temp.write(&format!("d{directory:02}/f{file:02}.dat"), &[0u8; 16]);
        }
    }

    let (tx, rx) = mpsc::channel();
    let handle = spawn_scan(temp.path().to_path_buf(), tx, fresh_cancel()).expect("spawn");

    // Delete half the tree while the walk is in flight. The scanner only
    // enumerates, so entries vanishing between discovery and listing must be
    // tolerated without a panic and without breaking reconciliation.
    let root = temp.path().to_path_buf();
    let churn = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2));
        for directory in (0..40).step_by(2) {
            let dir = root.join(format!("d{directory:02}"));
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    });

    handle.join.join().expect("scan thread");
    churn.join().expect("churn thread");

    let output = loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(ScanEvent::Progress(_)) => {}
            Ok(ScanEvent::Done(output)) => break output,
            Ok(ScanEvent::Failed(message)) => {
                panic!("scan failed while entries disappeared: {message}")
            }
            Err(error) => panic!("no terminal event: {error}"),
        }
    };
    assert_reconciles(&output.tree, &output);
}
