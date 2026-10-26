//! Real-disk benchmark harness for the `space` scan engine.
//!
//! Uses only the public library API (`engine::scan_blocking`). Reports wall
//! time, the engine's own index/scan timer, counts, throughput, sampled peak
//! working set, exact category totals and the largest files. No numbers are
//! synthesized: anything the platform does not report is printed as `n/a`.
//!
//! Usage:
//!   space-bench [PATH] [--json] [--warm] [--runs N]
//!
//! `--warm` runs twice (the first run is labelled cold, the second warm).
//! "Cold" means first run in this process — the harness never purges the OS
//! file cache. `--runs N` performs N runs; run 1 is cold, the rest warm.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use space::engine::scan_blocking;
use space::model::{Category, ScanEvent, ScanOutput, Tree};
use space::preview::Service;
use space::treemap::{self, Item, Rect};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(20);

struct Options {
    root: PathBuf,
    json: bool,
    runs: usize,
}

struct Run {
    index: usize,
    label: &'static str,
    elapsed_ms: u64,
    index_ms: u64,
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    files_per_sec: f64,
    bytes_per_sec: f64,
    peak_working_set: Option<u64>,
    categories: [u64; 8],
    category_scope: String,
    category_cold_us: u64,
    category_warm_us: u64,
    layout_us: u64,
    layout_tiles: usize,
    preview_cold_ms: Option<u64>,
    preview_warm_us: Option<u64>,
    preview_kind: Option<&'static str>,
    preview_path: Option<String>,
    largest: Vec<Largest>,
}

struct Largest {
    path: String,
    size: u64,
    category: Category,
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--preview") {
        if let Err(message) = preview_mode(&raw) {
            eprintln!("space-bench: {message}");
            std::process::exit(2);
        }
        return;
    }
    if raw.first().map(String::as_str) == Some("--cancel") {
        if let Err(message) = cancel_mode(&raw) {
            eprintln!("space-bench: {message}");
            std::process::exit(2);
        }
        return;
    }

    let options = match parse_args() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(message) => {
            eprintln!("space-bench: {message}");
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    };

    let mut runs = Vec::with_capacity(options.runs);
    for index in 0..options.runs {
        let label = if index == 0 { "cold" } else { "warm" };
        match run_once(&options.root, index + 1, label) {
            Ok(run) => runs.push(run),
            Err(message) => {
                if options.json {
                    println!(
                        "{{\"tool\":\"space-bench\",\"version\":\"{}\",\"error\":\"{}\"}}",
                        env!("CARGO_PKG_VERSION"),
                        json_escape(&message)
                    );
                } else {
                    eprintln!("space-bench: {message}");
                }
                std::process::exit(2);
            }
        }
    }

    if options.json {
        print_json(&options.root, &runs);
    } else {
        print_human(&options.root, &runs);
    }
}

fn usage() -> &'static str {
    "usage: space-bench [PATH] [--json] [--warm] [--runs N]\n\
     \n\
     PATH     folder to scan (default: C:\\ on Windows, / elsewhere)\n\
     --json   machine-readable output\n\
     --warm   run twice; report the first as cold and the second as warm\n\
     --runs N perform N runs (run 1 cold, later runs warm)\n\
     \n\
     diagnostic modes:\n\
     space-bench --preview PATH [--json]     time one real preview, cold and cached\n\
     space-bench --cancel PATH [DELAY_MS]    start a scan, cancel it, report latency"
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut root: Option<PathBuf> = None;
    let mut json = false;
    let mut warm = false;
    let mut runs = 1usize;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--warm" => warm = true,
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(None);
            }
            "--runs" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--runs needs a count".to_string())?;
                runs = parse_runs(&value)?;
            }
            _ if arg.starts_with("--runs=") => {
                let value = &arg["--runs=".len()..];
                runs = parse_runs(value)?;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option {arg}"));
            }
            _ => {
                if root.is_some() {
                    return Err("only one PATH may be given".into());
                }
                root = Some(PathBuf::from(arg));
            }
        }
    }

    if warm {
        runs = runs.max(2);
    }
    let root = root.unwrap_or_else(default_root);
    Ok(Some(Options { root, json, runs }))
}

fn parse_runs(value: &str) -> Result<usize, String> {
    let runs: usize = value
        .parse()
        .map_err(|_| format!("invalid run count {value:?}"))?;
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if runs > 100 {
        return Err("--runs is capped at 100".into());
    }
    Ok(runs)
}

fn default_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\")
    } else {
        PathBuf::from("/")
    }
}

fn run_once(root: &Path, index: usize, label: &'static str) -> Result<Run, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let shared_peak = Arc::clone(&peak);
    let shared_stop = Arc::clone(&stop);

    let sampler = thread::Builder::new()
        .name("space-bench-mem".into())
        .spawn(move || sample_until_stopped(shared_peak, shared_stop))
        .map_err(|error| format!("cannot start memory sampler: {error}"))?;

    let started = Instant::now();
    let output = scan_blocking(root.to_path_buf(), None, cancel);
    let wall = started.elapsed();

    // One final sample captures the finished tree, then stop the sampler.
    let final_sample = current_process_memory();
    if let Some(bytes) = final_sample {
        peak.fetch_max(bytes, Ordering::Relaxed);
    }
    stop.store(true, Ordering::Release);
    let _ = sampler.join();
    let peak_working_set = match peak.load(Ordering::Relaxed) {
        0 => None,
        bytes => Some(bytes),
    };

    let output = output.map_err(|message| format!("scan failed: {message}"))?;
    let wall_ms = wall.as_millis().max(1) as u64;
    let seconds = wall_ms as f64 / 1000.0;
    let mut categories = [0u64; 8];
    for (slot, value) in categories.iter_mut().zip(output.summary.categories.0) {
        *slot = value;
    }

    let largest = largest_files(&output);

    // Exact category composition on the largest direct directory: the root is
    // primed by the scan, so a nested subtree is the meaningful cold case.
    let mut category_id = 0u32;
    let mut category_size = 0u64;
    for child in output.tree.children(0) {
        let node = output.tree.node(child);
        if node.is_dir && node.size > category_size {
            category_id = child;
            category_size = node.size;
        }
    }
    let category_scope = if category_id == 0 {
        "<root>".to_string()
    } else {
        output.tree.path(category_id).to_string_lossy().into_owned()
    };
    let category_started = Instant::now();
    let _ = output.tree.category_totals(category_id);
    let category_cold_us = category_started.elapsed().as_micros().max(1) as u64;
    let category_started = Instant::now();
    let _ = output.tree.category_totals(category_id);
    let category_warm_us = category_started.elapsed().as_micros().max(1) as u64;

    // Recursive scene layout at a representative 1200x700 viewport, capped
    // like the canvas (depth 4, at most 4,000 tiles). Median of five runs.
    let area = Rect::new(0.0, 0.0, 1200.0, 700.0);
    let mut layout_samples = Vec::with_capacity(5);
    let mut scene_tiles = 0usize;
    for _ in 0..5 {
        let mut tiles = Vec::new();
        let started = Instant::now();
        layout_scene(&output.tree, 0, area, 0, &mut tiles);
        std::hint::black_box(tiles.len());
        layout_samples.push(started.elapsed().as_micros().max(1) as u64);
        scene_tiles = tiles.len();
    }
    layout_samples.sort_unstable();
    let layout_us = layout_samples[layout_samples.len() / 2];

    // Preview the largest readable candidate cold, then the same file warm.
    let previews = preview_metric(&output);

    Ok(Run {
        index,
        label,
        elapsed_ms: wall_ms,
        // The tree/index is built during the walk; the engine does not expose
        // a separate finalize phase, so this is the engine's own timer for
        // producing the finished index.
        index_ms: output.summary.elapsed_ms.max(1),
        files: output.summary.files,
        dirs: output.summary.dirs,
        bytes: output.summary.total_bytes,
        errors: output.summary.errors,
        files_per_sec: output.summary.files as f64 / seconds,
        bytes_per_sec: output.summary.total_bytes as f64 / seconds,
        peak_working_set,
        categories,
        category_scope,
        category_cold_us,
        category_warm_us,
        layout_us,
        layout_tiles: scene_tiles,
        preview_cold_ms: previews.as_ref().map(|p| p.cold_ms),
        preview_warm_us: previews.as_ref().map(|p| p.warm_us),
        preview_kind: previews.as_ref().map(|p| p.kind),
        preview_path: previews.map(|p| p.path),
        largest,
    })
}

fn preview_mode(args: &[String]) -> Result<(), String> {
    let path = args
        .get(1)
        .ok_or_else(|| "--preview needs a file path".to_string())?;
    let json = args.iter().any(|arg| arg == "--json");
    let service = Service::new();
    let started = Instant::now();
    let preview = service.preview(Path::new(path));
    let cold_ms = started.elapsed().as_millis().max(1) as u64;
    let kind = preview.kind();
    let mut warm = Vec::with_capacity(5);
    for _ in 0..5 {
        let started = Instant::now();
        let again = service.preview(Path::new(path));
        std::hint::black_box(again);
        warm.push(started.elapsed().as_micros().max(1) as u64);
    }
    warm.sort_unstable();
    let warm_us = warm[warm.len() / 2];

    if json {
        println!(
            "{{\"tool\":\"space-bench\",\"version\":\"{}\",\"mode\":\"preview\",\"path\":\"{}\",\"kind\":\"{}\",\"cold_ms\":{},\"warm_us\":{},\"cache_entries\":{}}}",
            env!("CARGO_PKG_VERSION"),
            json_escape(path),
            json_escape(kind),
            cold_ms,
            warm_us,
            service.cache_len()
        );
    } else {
        println!("space-bench {}  preview", env!("CARGO_PKG_VERSION"));
        println!("path:  {path}");
        println!("kind:  {kind}");
        println!("cold:  {}", fmt_ms(cold_ms));
        println!("warm:  {}  (median of 5; cache hits)", fmt_us(warm_us));
        println!("cache: {} entries", service.cache_len());
    }
    Ok(())
}

fn cancel_mode(args: &[String]) -> Result<(), String> {
    let path = args
        .get(1)
        .ok_or_else(|| "--cancel needs a folder path".to_string())?;
    let delay_ms = args
        .get(2)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("invalid delay {value:?}"))
        })
        .transpose()?
        .unwrap_or(25);
    let json = args.iter().any(|arg| arg == "--json");
    let root = PathBuf::from(path);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let (tx, rx) = std::sync::mpsc::channel();
    let started = Instant::now();
    let worker = thread::Builder::new()
        .name("space-bench-cancel".into())
        .spawn(move || scan_blocking(root, Some(tx), worker_cancel))
        .map_err(|error| format!("cannot start scan: {error}"))?;

    thread::sleep(Duration::from_millis(delay_ms));
    let requested = Instant::now();
    cancel.store(true, Ordering::Release);
    let result = worker
        .join()
        .map_err(|_| "scan worker panicked".to_string())?;
    let latency_us = requested.elapsed().as_micros().max(1) as u64;
    let total_ms = started.elapsed().as_millis().max(1) as u64;

    let mut files = 0u64;
    let mut dirs = 0u64;
    let mut bytes = 0u64;
    while let Ok(event) = rx.try_recv() {
        if let ScanEvent::Progress(progress) = event {
            files = progress.files;
            dirs = progress.dirs;
            bytes = progress.bytes;
        }
    }
    let (cancelled, message) = match result {
        Ok(_) => (
            false,
            "scan completed before cancellation was observed".to_string(),
        ),
        Err(message) => (message == "Scan cancelled", message),
    };

    if json {
        println!(
            "{{\"tool\":\"space-bench\",\"version\":\"{}\",\"mode\":\"cancel\",\"root\":\"{}\",\"cancel_after_ms\":{},\"cancel_latency_us\":{},\"total_ms\":{},\"cancelled\":{},\"files_seen\":{},\"dirs_seen\":{},\"bytes_seen\":{},\"message\":\"{}\"}}",
            env!("CARGO_PKG_VERSION"),
            json_escape(path),
            delay_ms,
            latency_us,
            total_ms,
            cancelled,
            files,
            dirs,
            bytes,
            json_escape(&message)
        );
    } else {
        println!("space-bench {}  cancel", env!("CARGO_PKG_VERSION"));
        println!("root:        {path}");
        println!("cancel after {delay_ms} ms");
        println!("latency:     {}", fmt_us(latency_us));
        println!("terminated:  {total_ms} ms after start");
        println!("cancelled:   {cancelled}  ({message})");
        println!(
            "observed:    {} files, {} dirs, {}",
            fmt_int(files),
            fmt_int(dirs),
            fmt_bytes(bytes)
        );
    }
    if cancelled {
        Ok(())
    } else {
        Err("cancellation was not observed".into())
    }
}

struct PreviewMetric {
    cold_ms: u64,
    warm_us: u64,
    kind: &'static str,
    path: String,
}

/// Mirrors the visible-scene limits in `ui::canvas`: four levels, at most 64
/// children per folder, and a 4,000-tile ceiling.
fn layout_scene(tree: &Tree, id: u32, area: Rect, depth: u8, out: &mut Vec<Rect>) {
    const MAX_DEPTH: u8 = 4;
    const MAX_CHILDREN: usize = 64;
    const MAX_TILES: usize = 4_000;

    if depth > MAX_DEPTH || out.len() >= MAX_TILES || area.is_empty() {
        return;
    }
    let mut items: Vec<Item> = tree
        .children(id)
        .into_iter()
        .map(|child| {
            let node = tree.node(child);
            Item {
                id: child,
                size: node.size,
                cat: node.cat,
                is_dir: node.is_dir,
            }
        })
        .filter(|item| item.size > 0)
        .collect();
    if items.len() > MAX_CHILDREN {
        items.sort_by_key(|item| std::cmp::Reverse(item.size));
        items.truncate(MAX_CHILDREN);
    }
    for tile in treemap::squarify(area, &items, 2.0) {
        out.push(tile.rect);
        if out.len() >= MAX_TILES {
            break;
        }
        if tile.is_dir && depth < MAX_DEPTH {
            layout_scene(tree, tile.id, tile.rect.inset(3.0), depth + 1, out);
        }
    }
}

fn preview_metric(output: &ScanOutput) -> Option<PreviewMetric> {
    let service = Service::new();
    for &id in output.tree.largest_files(24).iter() {
        let node = output.tree.node(id);
        if node.is_dir {
            continue;
        }
        let path = output.tree.path(id);
        let started = Instant::now();
        let preview = service.preview(&path);
        let cold_ms = started.elapsed().as_millis().max(1) as u64;
        if matches!(preview, space::preview::Preview::Unavailable { .. }) {
            continue;
        }
        let kind = preview.kind();
        let started = Instant::now();
        let again = service.preview(&path);
        let warm_us = started.elapsed().as_micros().max(1) as u64;
        if !matches!(again, space::preview::Preview::Unavailable { .. }) {
            return Some(PreviewMetric {
                cold_ms,
                warm_us,
                kind,
                path: path.to_string_lossy().into_owned(),
            });
        }
    }
    None
}

fn largest_files(output: &ScanOutput) -> Vec<Largest> {
    output
        .tree
        .largest_files(5)
        .iter()
        .map(|&id| {
            let node = output.tree.node(id);
            Largest {
                path: output.tree.path(id).to_string_lossy().into_owned(),
                size: node.size,
                category: node.cat,
            }
        })
        .collect()
}

fn sample_until_stopped(peak: Arc<AtomicU64>, stop: Arc<AtomicBool>) {
    let mut system = sysinfo::System::new();
    let pid = match sysinfo::get_current_pid() {
        Ok(pid) => pid,
        Err(_) => return,
    };
    loop {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if let Some(process) = system.process(pid) {
            peak.fetch_max(process.memory(), Ordering::Relaxed);
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(SAMPLE_INTERVAL);
    }
}

fn current_process_memory() -> Option<u64> {
    let pid = sysinfo::get_current_pid().ok()?;
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map(|process| process.memory())
}

fn print_human(root: &Path, runs: &[Run]) {
    println!(
        "space-bench {}  {} / {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("root: {}", root.display());
    println!("cold = first run in this process; no OS cache purge is performed.");
    println!();

    for run in runs {
        println!("run {}/{} ({})", run.index, runs.len(), run.label);
        println!("  elapsed          {}", fmt_ms(run.elapsed_ms));
        println!(
            "  index            {}  (tree/index is built during the walk)",
            fmt_ms(run.index_ms)
        );
        println!("  files            {}", fmt_int(run.files));
        println!("  dirs             {}", fmt_int(run.dirs));
        println!(
            "  bytes            {}  ({} B)",
            fmt_bytes(run.bytes),
            fmt_int(run.bytes)
        );
        println!(
            "  rate             {}/s · {}/s",
            fmt_rate(run.files_per_sec, "files"),
            fmt_bytes(run.bytes_per_sec as u64)
        );
        println!("  errors           {}", fmt_int(run.errors));
        match run.peak_working_set {
            Some(bytes) => println!(
                "  peak working set {}  (sampled every {} ms)",
                fmt_bytes(bytes),
                SAMPLE_INTERVAL.as_millis()
            ),
            None => println!("  peak working set n/a"),
        }
        println!(
            "  category totals  first {} / cached {}  (8 exact buckets in {})",
            fmt_us(run.category_cold_us),
            fmt_us(run.category_warm_us),
            run.category_scope
        );
        println!(
            "  scene layout     {}  ({} tiles; depth 4, 64 children/folder, 4k cap)",
            fmt_us(run.layout_us),
            fmt_int(run.layout_tiles as u64)
        );
        match (
            &run.preview_path,
            run.preview_cold_ms,
            run.preview_warm_us,
            run.preview_kind,
        ) {
            (Some(path), Some(cold), Some(warm), Some(kind)) => println!(
                "  preview          {} cold {} / cached {}  {}  {}",
                kind,
                fmt_ms(cold),
                fmt_us(warm),
                fmt_bytes(
                    run.largest
                        .iter()
                        .find(|file| file.path == *path)
                        .map(|file| file.size)
                        .unwrap_or(0)
                ),
                path
            ),
            _ => println!("  preview          n/a (no readable candidate)"),
        }
        println!("  categories");
        for (category, bytes) in Category::ALL.iter().zip(run.categories) {
            let share = if run.bytes == 0 {
                0.0
            } else {
                bytes as f64 / run.bytes as f64 * 100.0
            };
            println!(
                "    {:<10} {:>14}  {:>5.1}%",
                category.label(),
                fmt_bytes(bytes),
                share
            );
        }
        if !run.largest.is_empty() {
            println!("  largest files");
            for (position, file) in run.largest.iter().enumerate() {
                println!(
                    "    {:>2}. {:>10}  {:<9}  {}",
                    position + 1,
                    fmt_bytes(file.size),
                    file.category.label(),
                    file.path
                );
            }
        }
        println!();
    }
}

fn print_json(root: &Path, runs: &[Run]) {
    print!(
        "{{\"tool\":\"space-bench\",\"version\":\"{}\",\"os\":\"{}\",\"arch\":\"{}\",\"root\":\"{}\",\"note\":\"cold is the first run in this process; no OS cache purge\",\"runs\":[",
        env!("CARGO_PKG_VERSION"),
        json_escape(std::env::consts::OS),
        json_escape(std::env::consts::ARCH),
        json_escape(&root.to_string_lossy())
    );
    for (index, run) in runs.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!(
            "{{\"run\":{},\"label\":\"{}\",\"elapsed_ms\":{},\"index_ms\":{},\"files\":{},\"dirs\":{},\"bytes\":{},\"errors\":{},\"files_per_sec\":{:.3},\"bytes_per_sec\":{:.3},\"peak_working_set_bytes\":",
            run.index,
            run.label,
            run.elapsed_ms,
            run.index_ms,
            run.files,
            run.dirs,
            run.bytes,
            run.errors,
            run.files_per_sec,
            run.bytes_per_sec
        );
        match run.peak_working_set {
            Some(bytes) => print!("{bytes}"),
            None => print!("null"),
        }
        print!(",\"categories\":{{");
        for (category_index, category) in Category::ALL.iter().enumerate() {
            if category_index > 0 {
                print!(",");
            }
            print!(
                "\"{}\":{}",
                category.label(),
                run.categories[category_index]
            );
        }
        print!(
            "}},\"category_scope\":\"{}\",\"category_cold_us\":{},\"category_warm_us\":{},\"layout_us\":{},\"layout_tiles\":{},\"preview_cold_ms\":",
            json_escape(&run.category_scope),
            run.category_cold_us,
            run.category_warm_us,
            run.layout_us,
            run.layout_tiles
        );
        match run.preview_cold_ms {
            Some(value) => print!("{value}"),
            None => print!("null"),
        }
        print!(",\"preview_warm_us\":");
        match run.preview_warm_us {
            Some(value) => print!("{value}"),
            None => print!("null"),
        }
        print!(",\"preview_kind\":");
        match run.preview_kind {
            Some(value) => print!("\"{}\"", json_escape(value)),
            None => print!("null"),
        }
        print!(",\"preview_path\":");
        match &run.preview_path {
            Some(value) => print!("\"{}\"", json_escape(value)),
            None => print!("null"),
        }
        print!(",\"largest_files\":[");
        for (file_index, file) in run.largest.iter().enumerate() {
            if file_index > 0 {
                print!(",");
            }
            print!(
                "{{\"path\":\"{}\",\"size\":{},\"category\":\"{}\"}}",
                json_escape(&file.path),
                file.size,
                file.category.label()
            );
        }
        print!("]}}");
    }
    println!("]}}");
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out
}

fn fmt_ms(milliseconds: u64) -> String {
    if milliseconds < 1_000 {
        format!("{milliseconds} ms")
    } else {
        format!(
            "{:.3} s ({} ms)",
            milliseconds as f64 / 1000.0,
            fmt_int(milliseconds)
        )
    }
}

fn fmt_us(microseconds: u64) -> String {
    if microseconds < 1_000 {
        format!("{microseconds} µs")
    } else {
        format!(
            "{:.2} ms ({} µs)",
            microseconds as f64 / 1000.0,
            fmt_int(microseconds)
        )
    }
}

fn fmt_int(value: u64) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    for (index, character) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn fmt_rate(value: f64, unit: &str) -> String {
    if value >= 1_000_000.0 {
        format!("{:.2}M {unit}", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}k {unit}", value / 1_000.0)
    } else {
        format!("{value:.0} {unit}")
    }
}
