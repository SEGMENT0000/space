//! Backend commands of the `space` disk-space analyzer.
//!
//! The frontend calls these over Tauri's IPC and receives the DTOs defined in
//! `model.rs`.

mod classify;
mod drives;
mod model;
mod preview;
mod scanner;

use std::sync::Arc;

use tauri::{AppHandle, State};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(std::sync::Arc::new(scanner::Engine::new()))
        .manage(preview::PreviewState::new())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            start_scan,
            cancel_scan,
            get_children,
            get_largest_files,
            get_category_totals,
            scan_state,
            get_preview,
            pick_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Lists the available disks.
#[tauri::command]
fn list_drives() -> Vec<model::DriveInfo> {
    drives::list_drives()
}

/// Starts a scan of `root`, running on a worker thread.
#[tauri::command]
fn start_scan(
    app: AppHandle,
    engine: State<'_, Arc<scanner::Engine>>,
    root: String,
) -> Result<(), String> {
    scanner::Engine::start_scan(app, engine, root)
}

/// Cancels the running scan, if any.
#[tauri::command]
fn cancel_scan(engine: State<'_, Arc<scanner::Engine>>) {
    engine.cancel_scan();
}

/// Children of a node, one level deep (and one deeper with `depth`).
#[tauri::command]
fn get_children(
    engine: State<'_, Arc<scanner::Engine>>,
    node_id: u32,
    depth: u32,
    sort: String,
    limit: u32,
) -> Result<model::ChildrenResponse, String> {
    engine.get_children(node_id, depth, &sort, limit)
}

/// The largest files of the finished scan.
#[tauri::command]
fn get_largest_files(
    engine: State<'_, Arc<scanner::Engine>>,
    limit: u32,
) -> Result<Vec<model::LargestFile>, String> {
    engine.get_largest_files(limit)
}

/// Per-category byte totals of a node's subtree.
#[tauri::command]
fn get_category_totals(
    engine: State<'_, Arc<scanner::Engine>>,
    node_id: u32,
) -> Result<Vec<u64>, String> {
    engine.get_category_totals(node_id)
}

/// Snapshot of the engine state, for UI recovery.
#[tauri::command]
fn scan_state(engine: State<'_, Arc<scanner::Engine>>) -> model::ScanStateDto {
    engine.scan_state()
}

/// Content preview for a file OR folder (FLEET_SPEC §B). Async so heavy
/// preview I/O runs on Tauri's runtime instead of blocking IPC; concurrency
/// and caching live in `preview::PreviewState`.
#[tauri::command]
async fn get_preview(
    state: State<'_, preview::PreviewState>,
    path: String,
) -> Result<model::PreviewPayload, String> {
    preview::get_preview(state.inner(), path).await
}

/// Opens the native folder-picker dialog and resolves the chosen absolute
/// path, or `None` when the user cancels. Read-only: the dialog never writes.
#[tauri::command]
async fn pick_folder() -> Result<Option<String>, String> {
    match rfd::AsyncFileDialog::new()
        .set_title("Choose a folder or project to scan")
        .pick_folder()
        .await
    {
        Some(handle) => Ok(Some(handle.path().to_string_lossy().to_string())),
        None => Ok(None),
    }
}
