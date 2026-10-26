//! Application state and update logic. The view layer is intentionally thin.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use iced::keyboard::{self, key::Named};
use iced::{Point, Size, Task, Vector, window};

use crate::engine;
use crate::model::{
    Category, CategoryTotals, DriveInfo, NO_NODE, NodeId, ScanEvent, ScanOutput, ScanProgress,
};
use crate::preview::{Preview, Service};
use crate::treemap::Camera;

use super::filter::FilterView;
use super::fmt;
use super::{FILTER_INPUT, FOCUS_SINK, PALETTE_INPUT, platform};

pub const RAIL_WIDTH: f32 = 318.0;
pub const APP_BAR_HEIGHT: f32 = 52.0;
pub const SUMMARY_HEIGHT: f32 = 88.0;
pub const PANEL_HEADER_HEIGHT: f32 = 64.0;
pub const TOOLBAR_HEIGHT: f32 = 44.0;
pub const STATUS_HEIGHT: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Drives,
    Scan,
    Explorer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerMode {
    Map,
    Categories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Size,
    Name,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Size => "Size",
            Self::Name => "Name",
        }
    }
    pub fn toggle(self) -> Self {
        match self {
            Self::Size => Self::Name,
            Self::Name => Self::Size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    None,
    Filter,
    Palette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

#[derive(Debug, Default)]
pub struct FilterState {
    pub query: String,
    pub hidden: [bool; 8],
}

impl FilterState {
    pub fn hidden_count(&self) -> usize {
        self.hidden.iter().filter(|h| **h).count()
    }

    pub fn allows(&self, cat: Category) -> bool {
        !self.hidden[cat.id() as usize]
    }

    pub fn active(&self) -> bool {
        !self.query.trim().is_empty() || self.hidden_count() > 0
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.hidden = [false; 8];
    }
}

pub struct ActiveScan {
    pub rx: Receiver<ScanEvent>,
    pub cancel: Arc<AtomicBool>,
    pub progress: ScanProgress,
    pub target: PathBuf,
    pub started: Instant,
    pub cancelling: bool,
    /// Bytes observed per progress tick, newest last; never synthesized.
    pub samples: Vec<u64>,
}

impl ActiveScan {
    pub fn progress_snapshot(&self) -> &ScanProgress {
        &self.progress
    }
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub node: NodeId,
    pub at: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Drives,
    Rescan,
    ToggleTheme,
    Refit,
    CopyPath,
    Open,
    Reveal,
    Up,
    Back,
    Forward,
    PickFolder,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Self::Drives => "Go to volumes",
            Self::Rescan => "Rescan this volume",
            Self::ToggleTheme => "Toggle light / dark",
            Self::Refit => "Refit treemap",
            Self::CopyPath => "Copy path",
            Self::Open => "Open",
            Self::Reveal => "Reveal in Explorer",
            Self::Up => "Go up one level",
            Self::Back => "History back",
            Self::Forward => "History forward",
            Self::PickFolder => "Scan a folder…",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::ToggleTheme => "T",
            Self::Refit => "Ctrl 0",
            Self::CopyPath => "Ctrl C",
            Self::Open => "Enter",
            Self::Up => "Backspace",
            Self::Back => "Alt ←",
            Self::Forward => "Alt →",
            Self::PickFolder => "Ctrl O",
            _ => "",
        }
    }
}

#[derive(Debug, Clone)]
pub enum PaletteEntry {
    Command(Command),
    Node(NodeId),
}

#[derive(Debug, Clone)]
pub struct PaletteHit {
    pub id: NodeId,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub cat: Category,
}

#[derive(Debug, Default)]
pub struct PaletteState {
    pub query: String,
    pub index: usize,
    pub commands: Vec<Command>,
    pub hits: Vec<PaletteHit>,
}

/// Opt-in frame pacing recorder (`SPACE_FRAME_LOG=1`). Intervals are measured
/// between view passes, which is where a presented frame begins in Iced. Idle
/// gaps longer than 250 ms are ignored so hidden-window waits do not skew data.
#[derive(Default)]
pub struct FrameStats {
    last: Option<Instant>,
    samples: Vec<f32>,
}

impl FrameStats {
    pub fn record(&mut self) {
        let now = Instant::now();
        let Some(last) = self.last.replace(now) else {
            return;
        };
        let elapsed = now.saturating_duration_since(last).as_secs_f32() * 1000.0;
        if !(0.05..=250.0).contains(&elapsed) {
            return;
        }
        self.samples.push(elapsed);
        if self.samples.len() < 120 {
            return;
        }
        self.samples.sort_by(f32::total_cmp);
        let middle = self.samples.len() / 2;
        let p95 = ((self.samples.len() - 1) as f32 * 0.95).round() as usize;
        eprintln!(
            "space frame n={} median={:.2}ms p95={:.2}ms max={:.2}ms",
            self.samples.len(),
            self.samples[middle],
            self.samples[p95],
            self.samples[self.samples.len() - 1]
        );
        self.samples.clear();
    }
}

pub struct App {
    pub screen: Screen,
    pub dark: bool,
    pub drives: Vec<DriveInfo>,
    pub drives_loading: bool,
    pub scan: Option<ActiveScan>,
    pub scan_error: Option<String>,
    pub output: Option<Arc<ScanOutput>>,
    pub current: NodeId,
    pub selection: Option<NodeId>,
    pub hover: Option<NodeId>,
    pub camera: Camera,
    pub history: Vec<NodeId>,
    pub history_pos: usize,
    pub filter: FilterState,
    /// Immutable result set derived from the filter query, current folder and
    /// hidden categories. Shared by the GPU fills, hit-test canvas and status.
    pub filter_view: Option<Arc<FilterView>>,
    pub sort: SortMode,
    pub mode: ExplorerMode,
    pub focus: Focus,
    pub palette: PaletteState,
    pub palette_open: bool,
    /// Iced's text input owns Enter through `on_submit`, but raw key delivery
    /// also observes the same physical press after the palette has closed. The
    /// guard keeps that trailing event from triggering a second action, such as
    /// opening the file a palette jump just selected.
    enter_guard_until: Option<Instant>,
    pub context: Option<ContextMenu>,
    pub preview: Option<Preview>,
    pub preview_for: Option<NodeId>,
    pub preview_loading: bool,
    pub preview_seq: u64,
    preview_tx: Sender<(u64, Preview)>,
    preview_rx: Receiver<(u64, Preview)>,
    preview_service: Service,
    pub composition: CategoryTotals,
    pub composition_for: NodeId,
    pub composition_loading: bool,
    comp_seq: u64,
    comp_tx: Sender<(u64, NodeId, CategoryTotals)>,
    comp_rx: Receiver<(u64, NodeId, CategoryTotals)>,
    pub toast: Option<Toast>,
    pub drag_hover: bool,
    pub inspector_open: bool,
    pub window_size: Size,
    pub map_size: Size,
    pub last_scan_label: Option<String>,
    pub now: Instant,
    pub frame_log: bool,
    pub(crate) frames: RefCell<FrameStats>,
    window: Option<window::Id>,
    shot_dir: Option<PathBuf>,
    shot_seq: u32,
}

#[derive(Debug, Clone)]
pub enum Message {
    WindowEvent {
        id: window::Id,
        event: window::Event,
    },
    Tick,
    DrivesLoaded(Vec<DriveInfo>),
    RefreshDrives,
    Go(Screen),
    StartScan(PathBuf),
    PickFolder,
    FolderPicked(Option<PathBuf>),
    CancelScan,
    RetryScan,
    Select(NodeId),
    ClearSelection,
    Drill(NodeId),
    Hover(Option<NodeId>),
    GoUp,
    Back,
    Forward,
    Refit,
    Zoom(f32, Point, Size),
    Pan(Vector, Size),
    Context(NodeId, Point),
    CloseContext,
    Command(Command),
    FilterChanged(String),
    FocusFilter,
    BlurFocus,
    ToggleCategory(Category),
    ClearFilters,
    ToggleSort,
    SetMode(ExplorerMode),
    ToggleRail,
    PaletteToggle,
    PaletteClose,
    PaletteQuery(String),
    PaletteMove(i32),
    PaletteChoose(usize),
    PaletteAccept,
    ThemeToggle,
    CopyPath,
    OpenSelected,
    RevealSelected,
    RevealHit(NodeId),
    Toast(String),
    ToastError(String),
    DismissToast,
    Keyboard {
        event: keyboard::Event,
        /// True when a focused widget already consumed the key. The shell still
        /// receives global shortcuts (Esc, Ctrl+K, F12) so overlays never trap
        /// the keyboard, but widget-owned editing keys stay untouched.
        captured: bool,
    },
    /// Keystrokes captured by the invisible focus sink. Intentionally ignored.
    SinkInput(String),
    CloseOverlays,
    DismissScanError,
    Viewport(Size),
    /// Save the next rendered frame (F12). Enabled by `SPACE_SHOT_DIR`.
    CaptureRequest,
    Captured(Box<window::Screenshot>),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (preview_tx, preview_rx) = channel();
        let (comp_tx, comp_rx) = channel();
        let dark = std::env::var(crate::THEME_ENV)
            .map(|v| v.eq_ignore_ascii_case("dark"))
            .unwrap_or(false);
        let mut app = Self {
            screen: Screen::Drives,
            dark,
            drives: Vec::new(),
            drives_loading: true,
            scan: None,
            scan_error: None,
            output: None,
            current: 0,
            selection: None,
            hover: None,
            camera: Camera::default(),
            history: vec![0],
            history_pos: 0,
            filter: FilterState::default(),
            filter_view: None,
            sort: SortMode::Size,
            mode: ExplorerMode::Map,
            focus: Focus::None,
            palette: PaletteState::default(),
            palette_open: false,
            enter_guard_until: None,
            context: None,
            preview: None,
            preview_for: None,
            preview_loading: false,
            preview_seq: 0,
            preview_tx,
            preview_rx,
            preview_service: Service::new(),
            composition: CategoryTotals::default(),
            composition_for: 0,
            composition_loading: false,
            comp_seq: 0,
            comp_tx,
            comp_rx,
            toast: None,
            drag_hover: false,
            inspector_open: true,
            window_size: Size::new(1320.0, 860.0),
            map_size: Size::new(900.0, 500.0),
            last_scan_label: None,
            now: Instant::now(),
            frame_log: std::env::var_os("SPACE_FRAME_LOG").is_some_and(|value| {
                let value = value.to_string_lossy();
                !value.is_empty() && value != "0"
            }),
            frames: RefCell::new(FrameStats::default()),
            window: None,
            shot_dir: std::env::var_os("SPACE_SHOT_DIR").map(PathBuf::from),
            shot_seq: 0,
        };

        let target = deep_link_target();
        let requested_screen = std::env::var(crate::SCREEN_ENV)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        if let Some(path) = target {
            app.begin_scan(path);
            app.screen = Screen::Scan;
        } else {
            app.screen = match requested_screen.as_deref() {
                Some("scan") => Screen::Scan,
                Some("explorer") => Screen::Explorer,
                _ => Screen::Drives,
            };
        }

        let task = Task::perform(async { engine::list_drives() }, Message::DrivesLoaded);
        (app, task)
    }

    pub fn title(&self) -> String {
        "space".into()
    }

    pub fn theme(&self) -> iced::Theme {
        super::theme::Theme::for_dark(self.dark).to_iced()
    }

    pub fn tree(&self) -> Option<&crate::model::Tree> {
        self.output.as_ref().map(|o| &o.tree)
    }

    pub fn current_node(&self) -> Option<&crate::model::Node> {
        self.tree().map(|t| t.node(self.current))
    }

    pub fn selected_node(&self) -> Option<&crate::model::Node> {
        let id = self.selection?;
        self.tree().map(|t| t.node(id))
    }

    pub fn focus_node(&self) -> NodeId {
        self.selection.unwrap_or(self.current)
    }

    pub fn visible_children(&self, id: NodeId) -> Vec<NodeId> {
        let Some(tree) = self.tree() else {
            return Vec::new();
        };
        if !self.filter.query.trim().is_empty() {
            return self
                .filter_view
                .as_ref()
                .map(|view| view.match_ids())
                .unwrap_or_default();
        }
        let mut out = tree.children(id);
        out.retain(|child| self.filter.allows(tree.node(*child).cat));
        out
    }

    /// Recompute the recursive filter result. This is synchronous but bounded;
    /// it only runs when the query, current folder or category mask changes.
    fn rebuild_filter(&mut self) {
        let query = self.filter.query.trim().to_string();
        let view = if query.is_empty() {
            None
        } else {
            self.output.as_ref().and_then(|output| {
                FilterView::build(&output.tree, self.current, &query, &self.filter.hidden)
            })
        };
        self.filter_view = view.map(Arc::new);
    }

    fn set_focus(&mut self, focus: Focus) -> Task<Message> {
        self.focus = focus;
        match focus {
            Focus::Filter => iced::widget::operation::focus(FILTER_INPUT),
            Focus::Palette => iced::widget::operation::focus(PALETTE_INPUT),
            Focus::None => iced::widget::operation::focus(FOCUS_SINK),
        }
    }

    fn begin_scan(&mut self, path: PathBuf) {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        match engine::spawn_scan(path.clone(), tx, cancel.clone()) {
            Ok(handle) => {
                self.scan = Some(ActiveScan {
                    rx,
                    cancel: handle.cancel,
                    progress: ScanProgress {
                        files: 0,
                        dirs: 0,
                        bytes: 0,
                        errors: 0,
                        current: path.to_string_lossy().into_owned(),
                        elapsed_ms: 0,
                    },
                    target: path,
                    started: Instant::now(),
                    cancelling: false,
                    samples: Vec::new(),
                });
                self.scan_error = None;
                self.screen = Screen::Scan;
                self.focus = Focus::None;
                self.palette_open = false;
            }
            Err(e) => {
                self.scan = None;
                self.scan_error = Some(e.to_string());
                self.screen = Screen::Scan;
            }
        }
    }

    fn finish_scan(&mut self, output: Box<ScanOutput>) {
        let summary = output.summary.clone();
        let output = Arc::new(*output);
        self.composition = summary.categories;
        self.composition_for = 0;
        self.scan = None;
        self.scan_error = None;
        self.output = Some(output);
        self.current = 0;
        self.selection = None;
        self.hover = None;
        self.camera = Camera::default();
        self.history = vec![0];
        self.history_pos = 0;
        self.filter.clear();
        self.filter_view = None;
        self.preview = None;
        self.preview_for = None;
        self.preview_loading = false;
        self.palette_open = false;
        self.context = None;
        self.mode = ExplorerMode::Map;
        self.last_scan_label = Some(summary.root_path.clone());
        self.screen = Screen::Explorer;
        self.focus = Focus::None;
        self.toast = Some(Toast {
            text: format!(
                "Scanned {} in {} · {} files",
                fmt::bytes(summary.total_bytes),
                fmt::elapsed(summary.elapsed_ms),
                fmt::count(summary.files)
            ),
            kind: ToastKind::Info,
            until: Instant::now() + Duration::from_secs(4),
        });
    }

    fn navigate(&mut self, id: NodeId, record: bool) {
        let Some(tree) = self.tree() else { return };
        if !tree.node(id).is_dir {
            return;
        }
        if record {
            self.history.truncate(self.history_pos + 1);
            if self.history.last().copied() != Some(id) {
                self.history.push(id);
                self.history_pos = self.history.len() - 1;
            }
        }
        self.current = id;
        self.selection = None;
        self.hover = None;
        self.camera = Camera::default();
        self.preview = None;
        self.preview_for = None;
        self.preview_loading = false;
        self.context = None;
        self.rebuild_filter();
        self.load_composition();
    }

    fn go_up(&mut self) {
        let Some(tree) = self.tree() else { return };
        let parent = tree.node(self.current).parent;
        if parent != NO_NODE {
            self.navigate(parent, true);
        }
    }

    fn back(&mut self) {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            let id = self.history[self.history_pos];
            self.navigate(id, false);
        }
    }

    fn forward(&mut self) {
        if self.history_pos + 1 < self.history.len() {
            self.history_pos += 1;
            let id = self.history[self.history_pos];
            self.navigate(id, false);
        }
    }

    fn load_composition(&mut self) {
        let Some(output) = &self.output else { return };
        if self.current == 0 {
            self.composition = output.summary.categories;
            self.composition_for = 0;
            self.composition_loading = false;
            return;
        }
        self.comp_seq += 1;
        let seq = self.comp_seq;
        let id = self.current;
        self.composition_loading = true;
        let output = Arc::clone(output);
        let tx = self.comp_tx.clone();
        std::thread::Builder::new()
            .name("space-composition".into())
            .spawn(move || {
                let totals = output.tree.category_totals(id);
                let _ = tx.send((seq, id, totals));
            })
            .ok();
    }

    fn select(&mut self, id: NodeId) -> Task<Message> {
        let is_file = {
            let Some(tree) = self.tree() else {
                return Task::none();
            };
            if id as usize >= tree.len() {
                return Task::none();
            }
            !tree.node(id).is_dir
        };
        self.selection = Some(id);
        self.context = None;
        if is_file {
            self.request_preview(id);
        } else {
            self.preview = None;
            self.preview_for = None;
            self.preview_loading = false;
        }
        self.set_focus(Focus::None)
    }

    fn request_preview(&mut self, id: NodeId) {
        let Some(tree) = self.tree() else { return };
        if tree.node(id).is_dir {
            return;
        }
        let path = tree.path(id);
        self.preview_seq += 1;
        let seq = self.preview_seq;
        self.preview = None;
        self.preview_for = Some(id);
        self.preview_loading = true;
        let service = self.preview_service.clone();
        let tx = self.preview_tx.clone();
        std::thread::Builder::new()
            .name("space-preview".into())
            .spawn(move || {
                let preview = service.preview(&path);
                let _ = tx.send((seq, preview));
            })
            .ok();
    }

    fn copy_path(&mut self) -> Task<Message> {
        let Some(tree) = self.tree() else {
            return Task::none();
        };
        let id = self.focus_node();
        let path = tree.path_string(id);
        Task::batch([
            iced::clipboard::write(path.clone()),
            Task::done(Message::Toast("Path copied".into())),
        ])
    }

    fn open_selected(&mut self) -> Task<Message> {
        let Some(tree) = self.tree() else {
            return Task::none();
        };
        let id = self.focus_node();
        if tree.node(id).is_dir {
            self.navigate(id, true);
            Task::none()
        } else {
            let path = tree.path(id);
            match platform::open(&path) {
                Ok(()) => Task::none(),
                Err(e) => Task::done(Message::ToastError(e)),
            }
        }
    }

    fn reveal(&mut self, id: NodeId) -> Task<Message> {
        let Some(tree) = self.tree() else {
            return Task::none();
        };
        let path = tree.path(id);
        match platform::reveal(&path) {
            Ok(()) => Task::none(),
            Err(e) => Task::done(Message::ToastError(e)),
        }
    }

    fn refresh_palette(&mut self) {
        let commands = [
            Command::Drives,
            Command::Rescan,
            Command::PickFolder,
            Command::ToggleTheme,
            Command::Refit,
            Command::Up,
            Command::Back,
            Command::Forward,
            Command::CopyPath,
            Command::Open,
            Command::Reveal,
        ];
        let q = self.palette.query.trim().to_lowercase();
        self.palette.commands = commands
            .into_iter()
            .filter(|c| q.is_empty() || c.label().to_lowercase().contains(&q))
            .collect();

        let hits = if q.is_empty() {
            Vec::new()
        } else if let Some(tree) = self.tree() {
            tree.search(&q, 8, 120_000)
                .into_iter()
                .take(8)
                .map(|id| {
                    let node = tree.node(id);
                    PaletteHit {
                        id,
                        path: tree.path_string(id),
                        size: node.size,
                        is_dir: node.is_dir,
                        cat: node.cat,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        self.palette.hits = hits;
        let total = self.palette.commands.len() + self.palette.hits.len();
        self.palette.index = self.palette.index.min(total.saturating_sub(1));
    }

    fn palette_total(&self) -> usize {
        self.palette.commands.len() + self.palette.hits.len()
    }

    fn accept_palette(&mut self) -> Task<Message> {
        let index = self.palette.index;
        let entry = if index < self.palette.commands.len() {
            Some(PaletteEntry::Command(self.palette.commands[index]))
        } else {
            self.palette
                .hits
                .get(index - self.palette.commands.len())
                .cloned()
                .map(|h| PaletteEntry::Node(h.id))
        };
        self.palette_open = false;
        self.enter_guard_until = Some(Instant::now() + Duration::from_millis(350));
        let focus = self.set_focus(Focus::None);
        let action = match entry {
            Some(PaletteEntry::Command(cmd)) => self.command(cmd),
            Some(PaletteEntry::Node(id)) => {
                let Some(tree) = self.tree() else {
                    return focus;
                };
                if tree.node(id).is_dir {
                    self.navigate(id, true);
                    Task::none()
                } else {
                    let parent = tree.node(id).parent;
                    self.navigate(parent, true);
                    self.select(id)
                }
            }
            None => Task::none(),
        };
        Task::batch([focus, action])
    }

    fn command(&mut self, cmd: Command) -> Task<Message> {
        match cmd {
            Command::Drives => {
                self.screen = Screen::Drives;
                self.drives_loading = true;
                self.palette_open = false;
                Task::perform(async { engine::list_drives() }, Message::DrivesLoaded)
            }
            Command::Rescan => {
                if let Some(output) = &self.output {
                    let path = PathBuf::from(&output.summary.root_path);
                    self.begin_scan(path);
                }
                Task::none()
            }
            Command::PickFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Scan a folder")
                        .pick_folder()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::FolderPicked,
            ),
            Command::ToggleTheme => {
                self.dark = !self.dark;
                Task::none()
            }
            Command::Refit => {
                self.camera = Camera::default();
                Task::none()
            }
            Command::Up => {
                self.go_up();
                Task::none()
            }
            Command::Back => {
                self.back();
                Task::none()
            }
            Command::Forward => {
                self.forward();
                Task::none()
            }
            Command::CopyPath => self.copy_path(),
            Command::Open => self.open_selected(),
            Command::Reveal => {
                let id = self.focus_node();
                self.reveal(id)
            }
        }
    }

    fn tick(&mut self) -> Task<Message> {
        self.now = Instant::now();
        let mut finished: Option<ScanEvent> = None;
        let mut cancelling = false;
        let mut events = Vec::new();
        if let Some(scan) = &self.scan {
            cancelling = scan.cancelling;
            while let Ok(event) = scan.rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                ScanEvent::Progress(p) => {
                    if let Some(scan) = &mut self.scan {
                        let delta = p.bytes.saturating_sub(scan.progress.bytes);
                        if delta > 0 {
                            scan.samples.push(delta);
                            if scan.samples.len() > 72 {
                                scan.samples.remove(0);
                            }
                        }
                        scan.progress = p;
                    }
                }
                other => {
                    finished = Some(other);
                    break;
                }
            }
        }
        if let Some(event) = finished {
            self.scan = None;
            match event {
                ScanEvent::Done(output) => self.finish_scan(output),
                ScanEvent::Failed(message) => {
                    if cancelling {
                        self.screen = Screen::Drives;
                        self.toast = Some(Toast {
                            text: "Scan cancelled".into(),
                            kind: ToastKind::Info,
                            until: Instant::now() + Duration::from_secs(3),
                        });
                    } else {
                        self.scan_error = Some(message);
                        self.screen = Screen::Scan;
                    }
                }
                ScanEvent::Progress(_) => {}
            }
        }

        while let Ok((seq, id, totals)) = self.comp_rx.try_recv() {
            if seq == self.comp_seq && id == self.current {
                self.composition = totals;
                self.composition_for = id;
                self.composition_loading = false;
            }
        }
        while let Ok((seq, preview)) = self.preview_rx.try_recv() {
            if seq == self.preview_seq {
                self.preview = Some(preview);
                self.preview_loading = false;
            }
        }
        if let Some(toast) = &self.toast
            && self.now >= toast.until
        {
            self.toast = None;
        }
        Task::none()
    }

    fn handle_key(&mut self, event: keyboard::Event, captured: bool) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        let key = key.as_ref();
        let command = modifiers.command();

        // `accept_palette` runs from the input's `on_submit`; the same physical
        // Enter is then delivered again as a raw key event. Consume it so a
        // palette jump selects/previews its target instead of immediately
        // opening it a second time.
        if matches!(key, keyboard::Key::Named(Named::Enter))
            && let Some(until) = self.enter_guard_until.take()
            && Instant::now() < until
        {
            return Task::none();
        }

        // Global shortcuts available even while a text field is focused. Escape
        // is handled below so a focused input cannot swallow it.
        if matches!(key, keyboard::Key::Named(Named::F12)) {
            return Task::done(Message::CaptureRequest);
        }
        if command && matches!(key, keyboard::Key::Character("k")) {
            return self.toggle_palette();
        }
        if command && matches!(key, keyboard::Key::Character("f")) {
            if self.screen == Screen::Explorer {
                self.palette_open = false;
                return self.set_focus(Focus::Filter);
            }
            return Task::none();
        }

        if self.palette_open {
            match key {
                keyboard::Key::Named(Named::Escape) => {
                    self.palette_open = false;
                    return self.set_focus(Focus::None);
                }
                // The palette input owns Enter through `on_submit`; arrow keys
                // are not captured by Iced's text input, but a scrollable may
                // own them when the input is not focused.
                keyboard::Key::Named(Named::Enter) if !captured => {
                    return self.accept_palette();
                }
                keyboard::Key::Named(Named::ArrowDown) if !captured => {
                    if self.palette_total() > 0 {
                        self.palette.index = (self.palette.index + 1).min(self.palette_total() - 1);
                    }
                    return Task::none();
                }
                keyboard::Key::Named(Named::ArrowUp) if !captured => {
                    self.palette.index = self.palette.index.saturating_sub(1);
                    return Task::none();
                }
                _ => return Task::none(),
            }
        }

        if self.focus == Focus::Filter {
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                self.filter.query.clear();
                self.filter_view = None;
                if self.screen == Screen::Explorer {
                    return self.set_focus(Focus::None);
                }
            }
            return Task::none();
        }

        match key {
            keyboard::Key::Named(Named::Escape) => {
                if self.toast.is_some() {
                    self.toast = None;
                } else if self.context.is_some() {
                    self.context = None;
                } else if self.filter.active() {
                    self.filter.clear();
                    self.filter_view = None;
                } else {
                    self.selection = None;
                    self.preview = None;
                }
                self.focus = Focus::None;
                Task::none()
            }
            keyboard::Key::Named(Named::Backspace) => {
                if self.screen == Screen::Explorer {
                    self.go_up();
                }
                Task::none()
            }
            keyboard::Key::Named(Named::Enter) => {
                if self.screen == Screen::Explorer {
                    self.open_selected()
                } else {
                    Task::none()
                }
            }
            keyboard::Key::Named(Named::ArrowLeft) if modifiers.alt() => {
                self.back();
                Task::none()
            }
            keyboard::Key::Named(Named::ArrowRight) if modifiers.alt() => {
                self.forward();
                Task::none()
            }
            keyboard::Key::Named(Named::ArrowDown) if self.screen == Screen::Explorer => {
                self.step_selection(1);
                Task::none()
            }
            keyboard::Key::Named(Named::ArrowUp) if self.screen == Screen::Explorer => {
                self.step_selection(-1);
                Task::none()
            }
            _ if command && matches!(key, keyboard::Key::Character("c")) => self.copy_path(),
            _ if command && matches!(key, keyboard::Key::Character("0")) => {
                self.camera = Camera::default();
                Task::none()
            }
            _ if command && matches!(key, keyboard::Key::Character("o")) => {
                self.command(Command::PickFolder)
            }
            _ if !command
                && matches!(
                    key,
                    keyboard::Key::Character("+") | keyboard::Key::Character("=")
                ) =>
            {
                let anchor = Point::new(self.map_size.width / 2.0, self.map_size.height / 2.0);
                self.camera.zoom_at(
                    1.25,
                    (anchor.x, anchor.y),
                    super::rect_for(self.map_size),
                    12.0,
                );
                Task::none()
            }
            _ if !command
                && matches!(
                    key,
                    keyboard::Key::Character("-") | keyboard::Key::Character("_")
                ) =>
            {
                let anchor = Point::new(self.map_size.width / 2.0, self.map_size.height / 2.0);
                self.camera.zoom_at(
                    0.8,
                    (anchor.x, anchor.y),
                    super::rect_for(self.map_size),
                    12.0,
                );
                Task::none()
            }
            _ if !command && matches!(key, keyboard::Key::Character("/")) => {
                if self.screen == Screen::Explorer {
                    self.set_focus(Focus::Filter)
                } else {
                    Task::none()
                }
            }
            _ if !command && matches!(key, keyboard::Key::Character("t")) => {
                self.dark = !self.dark;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn step_selection(&mut self, delta: i32) {
        let children = self.visible_children(self.current);
        if children.is_empty() {
            return;
        }
        let current_index = self
            .selection
            .and_then(|id| children.iter().position(|c| *c == id));
        let next = match current_index {
            Some(i) => (i as i32 + delta).clamp(0, children.len() as i32 - 1) as usize,
            None => 0,
        };
        let id = children[next];
        self.selection = Some(id);
        if !self.tree().map(|t| t.node(id).is_dir).unwrap_or(false) {
            self.request_preview(id);
        }
    }

    fn toggle_palette(&mut self) -> Task<Message> {
        self.palette_open = !self.palette_open;
        self.context = None;
        if self.palette_open {
            self.palette.query.clear();
            self.palette.index = 0;
            self.refresh_palette();
            let _ = self.set_focus(Focus::Palette);
            iced::widget::operation::focus(PALETTE_INPUT)
        } else {
            self.set_focus(Focus::None)
        }
    }

    /// Review-harness capture: writes the rendered client area to
    /// `SPACE_SHOT_DIR`. The shipping UI never takes screenshots on its own.
    fn save_screenshot(&mut self, shot: window::Screenshot) {
        let Some(dir) = self.shot_dir.clone() else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        self.shot_seq = self.shot_seq.saturating_add(1);
        let path = dir.join(format!("shot-{:02}.png", self.shot_seq));
        let (width, height) = (shot.size.width, shot.size.height);
        let result = image::save_buffer(
            &path,
            &shot.rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        );
        match result {
            Ok(()) => eprintln!(
                "space shot {} {}x{} scale={}",
                path.display(),
                width,
                height,
                shot.scale_factor
            ),
            Err(error) => eprintln!("space shot failed: {error}"),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowEvent { id, event } => {
                self.window = Some(id);
                match event {
                    window::Event::Resized(size) => {
                        self.window_size = size;
                        Task::none()
                    }
                    window::Event::FileHovered(_) => {
                        self.drag_hover = true;
                        Task::none()
                    }
                    window::Event::FilesHoveredLeft => {
                        self.drag_hover = false;
                        Task::none()
                    }
                    window::Event::FileDropped(path) => {
                        self.drag_hover = false;
                        if path.is_dir() {
                            self.begin_scan(path);
                        } else if let Some(parent) = path.parent() {
                            self.begin_scan(parent.to_path_buf());
                        }
                        Task::none()
                    }
                    _ => Task::none(),
                }
            }
            Message::Tick => self.tick(),
            Message::DrivesLoaded(drives) => {
                self.drives = drives;
                self.drives_loading = false;
                Task::none()
            }
            Message::RefreshDrives => {
                self.drives_loading = true;
                Task::perform(async { engine::list_drives() }, Message::DrivesLoaded)
            }
            Message::Go(screen) => {
                if screen == Screen::Explorer && self.output.is_none() {
                    return Task::none();
                }
                self.screen = screen;
                self.context = None;
                self.palette_open = false;
                self.set_focus(Focus::None)
            }
            Message::StartScan(path) => {
                if !path.exists() {
                    return Task::done(Message::ToastError(
                        "That folder is no longer available.".into(),
                    ));
                }
                self.begin_scan(path);
                Task::none()
            }
            Message::PickFolder => self.command(Command::PickFolder),
            Message::FolderPicked(Some(path)) => {
                if path.is_dir() {
                    self.begin_scan(path);
                }
                Task::none()
            }
            Message::FolderPicked(None) => Task::none(),
            Message::CancelScan => {
                if let Some(scan) = &mut self.scan {
                    scan.cancelling = true;
                    scan.cancel.store(true, Ordering::Relaxed);
                }
                Task::none()
            }
            Message::RetryScan => {
                if let Some(scan) = &self.scan {
                    let target = scan.target.clone();
                    self.scan = None;
                    self.begin_scan(target);
                }
                Task::none()
            }
            Message::Select(id) => self.select(id),
            Message::ClearSelection => {
                self.selection = None;
                self.preview = None;
                self.preview_for = None;
                self.preview_loading = false;
                self.context = None;
                Task::none()
            }
            Message::Drill(id) => {
                if self.tree().map(|t| t.node(id).is_dir).unwrap_or(false) {
                    self.navigate(id, true);
                    self.set_focus(Focus::None)
                } else {
                    self.select(id)
                }
            }
            Message::Hover(id) => {
                self.hover = id;
                Task::none()
            }
            Message::GoUp => {
                self.go_up();
                Task::none()
            }
            Message::Back => {
                self.back();
                Task::none()
            }
            Message::Forward => {
                self.forward();
                Task::none()
            }
            Message::Refit => {
                self.camera = Camera::default();
                Task::none()
            }
            Message::Zoom(factor, anchor, size) => {
                self.map_size = size;
                self.camera
                    .zoom_at(factor, (anchor.x, anchor.y), super::rect_for(size), 12.0);
                Task::none()
            }
            Message::Pan(delta, size) => {
                self.map_size = size;
                self.camera.pan(delta.x, delta.y, super::rect_for(size));
                Task::none()
            }
            Message::Context(node, at) => {
                self.selection = Some(node);
                self.context = Some(ContextMenu { node, at });
                self.set_focus(Focus::None)
            }
            Message::CloseContext => {
                self.context = None;
                Task::none()
            }
            Message::Command(cmd) => self.command(cmd),
            Message::FilterChanged(query) => {
                self.filter.query = query;
                self.palette_open = false;
                self.rebuild_filter();
                Task::none()
            }
            Message::FocusFilter => {
                if self.screen == Screen::Explorer {
                    self.set_focus(Focus::Filter)
                } else {
                    Task::none()
                }
            }
            Message::BlurFocus => self.set_focus(Focus::None),
            Message::ToggleCategory(cat) => {
                self.filter.hidden[cat.id() as usize] = !self.filter.hidden[cat.id() as usize];
                self.rebuild_filter();
                Task::none()
            }
            Message::ClearFilters => {
                self.filter.clear();
                self.filter_view = None;
                Task::none()
            }
            Message::ToggleSort => {
                self.sort = self.sort.toggle();
                Task::none()
            }
            Message::SetMode(mode) => {
                self.mode = mode;
                Task::none()
            }
            Message::ToggleRail => {
                self.inspector_open = !self.inspector_open;
                Task::none()
            }
            Message::PaletteToggle => self.toggle_palette(),
            Message::PaletteClose => {
                self.palette_open = false;
                self.set_focus(Focus::None)
            }
            Message::PaletteQuery(query) => {
                self.palette.query = query;
                self.palette.index = 0;
                self.refresh_palette();
                Task::none()
            }
            Message::PaletteMove(delta) => {
                if self.palette_total() > 0 {
                    let total = self.palette_total() as i32;
                    let next = (self.palette.index as i32 + delta).clamp(0, total - 1);
                    self.palette.index = next as usize;
                }
                Task::none()
            }
            Message::PaletteChoose(index) => {
                if index < self.palette_total() {
                    self.palette.index = index;
                    return self.accept_palette();
                }
                Task::none()
            }
            Message::PaletteAccept => self.accept_palette(),
            Message::ThemeToggle => {
                self.dark = !self.dark;
                Task::none()
            }
            Message::CopyPath => self.copy_path(),
            Message::OpenSelected => self.open_selected(),
            Message::RevealSelected => {
                let id = self.focus_node();
                self.reveal(id)
            }
            Message::RevealHit(id) => {
                if let Some(tree) = self.tree() {
                    let parent = tree.node(id).parent;
                    self.navigate(parent, true);
                    self.select(id)
                } else {
                    Task::none()
                }
            }
            Message::Toast(text) => {
                self.toast = Some(Toast {
                    text,
                    kind: ToastKind::Info,
                    until: Instant::now() + Duration::from_secs(3),
                });
                Task::none()
            }
            Message::ToastError(text) => {
                self.toast = Some(Toast {
                    text,
                    kind: ToastKind::Error,
                    until: Instant::now() + Duration::from_secs(5),
                });
                Task::none()
            }
            Message::DismissToast => {
                self.toast = None;
                Task::none()
            }
            Message::Keyboard { event, captured } => self.handle_key(event, captured),
            Message::SinkInput(_) => Task::none(),
            Message::CloseOverlays => {
                self.palette_open = false;
                self.context = None;
                self.set_focus(Focus::None)
            }
            Message::DismissScanError => {
                self.scan_error = None;
                self.screen = Screen::Drives;
                Task::none()
            }
            Message::Viewport(size) => {
                self.map_size = size;
                Task::none()
            }
            Message::CaptureRequest => {
                let Some(id) = self.window else {
                    return Task::none();
                };
                if self.shot_dir.is_none() {
                    return Task::none();
                }
                iced::window::screenshot(id).map(|shot| Message::Captured(Box::new(shot)))
            }
            Message::Captured(shot) => {
                self.save_screenshot(*shot);
                Task::none()
            }
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let tick_ms = if self.scan.is_some() { 60 } else { 220 };
        iced::Subscription::batch([
            iced::time::every(Duration::from_millis(tick_ms)).map(|_| Message::Tick),
            iced::window::events().map(|(id, event)| Message::WindowEvent { id, event }),
            // `keyboard::listen` drops captured events, which includes Escape
            // while a text field is focused. Raw delivery keeps global keys
            // reliable; `handle_key` uses the status to avoid re-running keys
            // the focused widget already acted on.
            iced::event::listen_raw(|event, status, _window| match event {
                iced::Event::Keyboard(event @ keyboard::Event::KeyPressed { .. }) => {
                    Some(Message::Keyboard {
                        event,
                        captured: status == iced::event::Status::Captured,
                    })
                }
                _ => None,
            }),
        ])
    }

    pub fn visible_count(&self) -> usize {
        self.visible_children(self.current).len()
    }
}

fn deep_link_target() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(crate::SCAN_ENV) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Some(path);
        }
    }
    for arg in std::env::args().skip(1) {
        if arg.starts_with('-') {
            continue;
        }
        let path = PathBuf::from(arg);
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}
