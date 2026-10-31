//! On-tile art plumbing for the UI: a shared result store, a bounded icon
//! raster cache for the bundled Lucide SVGs, and the background planner that
//! requests thumbnails, text snippets and folder summaries for visible Large
//! tiles.
//!
//! Safety: icons are rasterized from compile-time bytes with resvg (scripts
//! are dropped by usvg; no network, no execution), files are only ever read by
//! [`crate::preview::TileService`] with hard caps, and nothing here writes to
//! or executes scanned paths.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use iced::widget::image;
use iced::{Color, Size};
use parking_lot::Mutex;

use crate::model::{CategoryTotals, NO_NODE, NodeId, ScanOutput, Tree};
use crate::preview::{
    FactKind, TILE_IMAGE_EDGE_MAX, TileArt, TileKind, TileService, tile_kind_for_name,
};
use crate::treemap::{self, Camera, TileTier};
use crate::ui::canvas;
use crate::ui::filter::FilterView;
use crate::ui::icon::{Icon, svg_source};

/// Display map handed to the canvas each frame: one art entry per node.
pub type ArtMap = Arc<HashMap<NodeId, Arc<UiArt>>>;

/// UI-side art. Decoded thumbnails are wrapped in one stable `image::Handle`
/// created exactly once per decode: `Handle::from_rgba` mints a fresh unique
/// id per call, so building handles at draw time would upload the texture
/// again on every frame.
pub enum UiArt {
    Image {
        handle: image::Handle,
        width: u32,
        height: u32,
    },
    Text {
        lines: Arc<[String]>,
    },
    Facts {
        kind: FactKind,
        lines: Arc<[String]>,
    },
    Folder {
        files: u64,
        items: u32,
        categories: CategoryTotals,
    },
    Unavailable,
}

fn ui_art(art: TileArt) -> UiArt {
    match art {
        TileArt::Image {
            width,
            height,
            rgba,
        } => UiArt::Image {
            handle: image::Handle::from_rgba(width, height, rgba.to_vec()),
            width,
            height,
        },
        TileArt::Text { lines } => UiArt::Text { lines },
        TileArt::Facts { kind, lines } => UiArt::Facts { kind, lines },
        TileArt::Folder {
            files,
            items,
            categories,
        } => UiArt::Folder {
            files,
            items,
            categories,
        },
        TileArt::Unavailable => UiArt::Unavailable,
    }
}

/// At most this many new worker jobs are spawned per update; disk work is
/// additionally capped by the service's three-permit gate.
const MAX_NEW_PER_UPDATE: usize = 8;

/// Decoded art keyed by node id. One art per node (the edge bucket of the
/// first request wins); zooming inside a bucket reuses it.
#[derive(Default)]
pub struct TileArtStore {
    inner: Mutex<HashMap<NodeId, Arc<UiArt>>>,
}

impl TileArtStore {
    pub fn snapshot(&self) -> ArtMap {
        Arc::new(self.inner.lock().clone())
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }

    fn insert(&self, node: NodeId, art: Arc<TileArt>) {
        let ui = match Arc::try_unwrap(art) {
            Ok(art) => ui_art(art),
            Err(shared) => ui_art((*shared).clone()),
        };
        self.inner.lock().insert(node, Arc::new(ui));
    }
}

/// Thumbnail edge bucket for a screen-space tile rect. Bucketed so small
/// zoom changes reuse the cached thumbnail instead of re-decoding.
pub fn image_edge_bucket(w: f32, h: f32) -> u32 {
    let longest = w.max(h).max(32.0);
    let bucket = ((longest / 96.0).ceil() as u32).clamp(2, 4);
    match bucket {
        2 => 128,
        3 => 256,
        // Largest bucket honors the service's hard image edge cap.
        _ => TILE_IMAGE_EDGE_MAX,
    }
}

fn icon_bucket_px(size: f32) -> u32 {
    let px = size.max(8.0);
    (((px / 16.0).ceil() as u32) * 16).clamp(16, 160)
}

#[derive(Debug, Clone)]
struct TileRequest {
    node: NodeId,
    kind: TileKind,
    edge: u32,
    path: PathBuf,
}

struct TileResult {
    generation: u64,
    node: NodeId,
    art: Option<Arc<TileArt>>,
}

/// Decides which visible Large tiles need async art. Pure: it consumes the
/// same scene tile list the GPU fills and hit-testing use, so requests can
/// never target a tile that is not drawn.
fn plan_requests(
    tree: &Tree,
    tiles: &[canvas::SceneTile],
    bounds: (f32, f32),
    pending: &HashSet<NodeId>,
    has_art: impl Fn(NodeId) -> bool,
    limit: usize,
) -> Vec<TileRequest> {
    let mut out: Vec<TileRequest> = Vec::new();
    for tile in tiles {
        if out.len() >= limit {
            break;
        }
        if tile.node == NO_NODE
            || tile.aggregate
            || pending.contains(&tile.node)
            || has_art(tile.node)
        {
            continue;
        }
        if treemap::tile_tier(tile.rect.w, tile.rect.h) != TileTier::Large {
            // Compact art is only worth decoding when the tile can actually
            // show a thumbnail or snippet above its name. 90x90 is the
            // smallest body that keeps both bands usable.
            if !canvas::compact_art_eligible(tile.rect.w, tile.rect.h) {
                continue;
            }
        }
        // Visible with a small tolerance: a mostly off-screen tile still counts.
        if tile.rect.right() < 0.0
            || tile.rect.bottom() < 0.0
            || tile.rect.x > bounds.0
            || tile.rect.y > bounds.1
        {
            continue;
        }
        let kind = if tile.is_dir && !tile.has_children {
            TileKind::Folder
        } else if tile.is_dir {
            continue;
        } else {
            // Every file family has an on-tile preview; unknown names fall
            // back to metadata/hex facts.
            tile_kind_for_name(&tile.name).unwrap_or(TileKind::Meta)
        };
        let edge = image_edge_bucket(tile.rect.w, tile.rect.h);
        let path = if kind == TileKind::Folder {
            PathBuf::new()
        } else {
            tree.path(tile.node)
        };
        out.push(TileRequest {
            node: tile.node,
            kind,
            edge,
            path,
        });
    }
    out
}

fn folder_art(tree: &Tree, node: NodeId) -> Option<Arc<TileArt>> {
    if node as usize >= tree.len() || !tree.node(node).is_dir {
        return None;
    }
    Some(Arc::new(TileArt::Folder {
        files: tree.node(node).file_count,
        items: tree.child_count(node),
        categories: tree.category_totals(node),
    }))
}

/// Owns the tile-art service, the display store, the icon raster cache and
/// the worker results. Bumped on navigation/filter/mode changes so stale
/// results are dropped instead of painted after the view moved on.
pub struct TilePlanner {
    service: TileService,
    store: Arc<TileArtStore>,
    icons: Arc<IconRaster>,
    tx: Sender<TileResult>,
    rx: Receiver<TileResult>,
    pending: HashSet<NodeId>,
    generation: u64,
    /// Opt-in "Rich previews (uses Windows)". Off by default.
    rich: bool,
}

impl Default for TilePlanner {
    fn default() -> Self {
        Self::new()
    }
}

impl TilePlanner {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            service: TileService::new(),
            store: Arc::new(TileArtStore::default()),
            icons: Arc::new(IconRaster::new()),
            tx,
            rx,
            pending: HashSet::new(),
            generation: 0,
            rich: false,
        }
    }

    /// Opt-in Windows shell thumbnails. Invalidates every decoded tile and
    /// in-flight request so the next visible frame is rebuilt under the new
    /// setting; the cache key also carries the flag.
    pub fn set_rich(&mut self, rich: bool) {
        if self.rich == rich {
            return;
        }
        self.rich = rich;
        self.reset();
    }

    pub fn rich(&self) -> bool {
        self.rich
    }

    /// Snapshot + icon cache for the canvas; clones are cheap.
    pub fn handles(&self) -> (ArtMap, Arc<IconRaster>) {
        (self.store.snapshot(), Arc::clone(&self.icons))
    }

    /// Invalidate in-flight results (navigation, filter, mode changes).
    pub fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending.clear();
    }

    /// Full reset for a new scan: also drops the decoded art.
    pub fn reset(&mut self) {
        self.bump();
        self.store.clear();
    }

    /// Drain finished jobs, then issue bounded requests for visible Large
    /// tiles. Called from the app tick; the UI thread never touches disk.
    pub fn update(
        &mut self,
        output: &Arc<ScanOutput>,
        dir: NodeId,
        bounds: Option<Size>,
        camera: Camera,
        hidden: [bool; 8],
        filter: Option<Arc<FilterView>>,
    ) {
        while let Ok(result) = self.rx.try_recv() {
            self.pending.remove(&result.node);
            if result.generation != self.generation {
                continue;
            }
            let art = result.art.unwrap_or_else(|| Arc::new(TileArt::Unavailable));
            self.store.insert(result.node, art);
        }

        let Some(bounds) = bounds else { return };
        if bounds.width < 64.0 || bounds.height < 64.0 {
            return;
        }
        let tiles = canvas::scene_tiles(
            &output.tree,
            dir,
            bounds,
            &camera,
            &hidden,
            filter.as_deref(),
        );
        let requests = plan_requests(
            &output.tree,
            &tiles,
            (bounds.width, bounds.height),
            &self.pending,
            |node| self.store.has(node),
            MAX_NEW_PER_UPDATE,
        );
        for request in requests {
            if !self.pending.insert(request.node) {
                continue;
            }
            let generation = self.generation;
            let rich = self.rich;
            let tx = self.tx.clone();
            let service = self.service.clone();
            let output = Arc::clone(output);
            std::thread::Builder::new()
                .name("space-tile".into())
                .spawn(move || {
                    let art = match request.kind {
                        TileKind::Folder => folder_art(&output.tree, request.node),
                        _ => service.art(&request.path, request.kind, request.edge, rich),
                    };
                    let _ = tx.send(TileResult {
                        generation,
                        node: request.node,
                        art,
                    });
                })
                .ok();
        }
    }
}

impl TileArtStore {
    fn has(&self, node: NodeId) -> bool {
        self.inner.lock().contains_key(&node)
    }
}

/// One rasterized icon handle per (icon, size bucket, colour).
type IconCache = HashMap<(Icon, u32, [u8; 4]), image::Handle>;

/// Rasterizes the bundled Lucide SVGs to straight RGBA at a bounded pixel
/// size, one handle per (icon, size bucket, colour) with a small map cache.
pub struct IconRaster {
    cache: Mutex<IconCache>,
}

impl Default for IconRaster {
    fn default() -> Self {
        Self::new()
    }
}

impl IconRaster {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn handle(&self, icon: Icon, size: f32, color: Color) -> image::Handle {
        let bucket = icon_bucket_px(size);
        let rgba = [
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8,
            (color.a * 255.0).round() as u8,
        ];
        let mut cache = self.cache.lock();
        // Bounded by eviction, not by LRU precision: the key space here is
        // icons (28) x buckets (10) x a handful of theme colours.
        if cache.len() > 256 {
            cache.clear();
        }
        if let Some(handle) = cache.get(&(icon, bucket, rgba)) {
            return handle.clone();
        }
        let handle = match rasterize(icon, bucket, rgba) {
            Some(handle) => handle,
            // A raster failure must never blank a tile: the draw side keeps
            // the name, size and extension badge regardless.
            None => image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
        };
        cache.insert((icon, bucket, rgba), handle.clone());
        handle
    }
}

fn rasterize(icon: Icon, px: u32, rgba: [u8; 4]) -> Option<image::Handle> {
    let source = std::str::from_utf8(svg_source(icon)).ok()?;
    // Substituting currentColor before parsing: usvg resolves it to black by
    // default. The bundled Lucide assets contain no scripts or external refs.
    let substituted = source.replace(
        "currentColor",
        &format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]),
    );
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_str(&substituted, &options).ok()?;
    let scale = px as f32 / 24.0;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(&tree, transform, &mut pixmap_mut);
    // Pixmap bytes are premultiplied; iced wants straight RGBA.
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let a = u32::from(pixel[3]);
        if a > 0 && a < 255 {
            pixel[0] = (u32::from(pixel[0]) * 255 / a).min(255) as u8;
            pixel[1] = (u32::from(pixel[1]) * 255 / a).min(255) as u8;
            pixel[2] = (u32::from(pixel[2]) * 255 / a).min(255) as u8;
        }
    }
    Some(image::Handle::from_rgba(px, px, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, CategoryTotals, TreeBuilder};
    use crate::treemap::Rect;
    use crate::ui::canvas::SceneTile;

    fn tile(node: NodeId, name: &str, x: f32, y: f32, w: f32, h: f32, is_dir: bool) -> SceneTile {
        SceneTile {
            node,
            name: name.into(),
            cat: Category::Other,
            size: 1_000,
            depth: 0,
            rect: Rect::new(x, y, w, h),
            header_h: 0.0,
            has_children: false,
            is_dir,
            empty_dir: false,
            aggregate: false,
        }
    }

    #[test]
    fn image_edge_bucket_is_stable_and_bounded() {
        assert_eq!(image_edge_bucket(100.0, 80.0), 128);
        assert_eq!(image_edge_bucket(200.0, 150.0), 256);
        assert_eq!(image_edge_bucket(310.0, 200.0), 384);
        assert_eq!(image_edge_bucket(4000.0, 4000.0), 384);
        assert_eq!(image_edge_bucket(0.0, 0.0), 128);
    }

    #[test]
    fn planner_requests_only_large_visible_leaves_and_folders() {
        let mut builder = TreeBuilder::new("C:\\x", "x");
        let img = builder.add_node("photo.png", 0, false, 10, Category::Media);
        let exe = builder.add_node("app.exe", 0, false, 10, Category::Apps);
        let dir = builder.add_node("docs", 0, true, 10, Category::Documents);
        let tree = builder.finish(vec![], CategoryTotals::default());

        let tiles = vec![
            tile(img, "photo.png", 0.0, 0.0, 400.0, 300.0, false),
            tile(exe, "app.exe", 400.0, 0.0, 400.0, 300.0, false),
            tile(dir, "docs", 0.0, 300.0, 400.0, 300.0, true),
            tile(7, "notes.md", 0.0, 600.0, 90.0, 50.0, false), // too small
            tile(8, "photo.jpg", 9000.0, 0.0, 400.0, 300.0, false), // off-screen
        ];
        let requests = plan_requests(&tree, &tiles, (800.0, 600.0), &HashSet::new(), |_| false, 8);
        let kinds: Vec<(NodeId, TileKind)> = requests.iter().map(|r| (r.node, r.kind)).collect();
        assert!(kinds.contains(&(img, TileKind::Image)));
        assert!(
            kinds.contains(&(exe, TileKind::Executable)),
            "every file family gets a real preview request now"
        );
        assert!(kinds.contains(&(dir, TileKind::Folder)));
        assert!(
            !kinds.iter().any(|(node, _)| *node == 7),
            "small tiles are not requested"
        );
        assert!(
            !kinds.iter().any(|(node, _)| *node == 8),
            "off-screen tiles are not requested"
        );
    }

    #[test]
    fn planner_requests_compact_art_only_above_ninety_square() {
        let mut builder = TreeBuilder::new("C:\\x", "x");
        let medium = builder.add_node("medium.png", 0, false, 10, Category::Media);
        let small = builder.add_node("small.png", 0, false, 10, Category::Media);
        let short = builder.add_node("short.png", 0, false, 10, Category::Media);
        let tree = builder.finish(vec![], CategoryTotals::default());

        let tiles = vec![
            tile(medium, "medium.png", 0.0, 0.0, 150.0, 100.0, false),
            tile(small, "small.png", 160.0, 0.0, 90.0, 90.0, false),
            tile(short, "short.png", 260.0, 0.0, 120.0, 89.0, false),
        ];
        let requests = plan_requests(&tree, &tiles, (800.0, 600.0), &HashSet::new(), |_| false, 8);
        let nodes: Vec<NodeId> = requests.iter().map(|r| r.node).collect();
        assert!(nodes.contains(&medium), "150x100 is a compact art tile");
        assert!(nodes.contains(&small), "90x90 is the compact floor");
        assert!(
            !nodes.contains(&short),
            "one pixel under the floor keeps the measured name treatment"
        );
    }

    #[test]
    fn planner_skips_pending_and_already_decoded_nodes_and_respects_limit() {
        let mut builder = TreeBuilder::new("C:\\x", "x");
        let a = builder.add_node("a.png", 0, false, 10, Category::Media);
        let b = builder.add_node("b.png", 0, false, 10, Category::Media);
        let c = builder.add_node("c.png", 0, false, 10, Category::Media);
        let tree = builder.finish(vec![], CategoryTotals::default());
        let tiles = vec![
            tile(a, "a.png", 0.0, 0.0, 400.0, 300.0, false),
            tile(b, "b.png", 400.0, 0.0, 400.0, 300.0, false),
            tile(c, "c.png", 0.0, 300.0, 400.0, 300.0, false),
        ];
        let mut pending = HashSet::new();
        pending.insert(a);
        let requests = plan_requests(&tree, &tiles, (800.0, 600.0), &pending, |n| n == b, 8);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].node, c);
        assert!(
            !requests[0].path.as_os_str().is_empty(),
            "path resolved from the tree"
        );

        let requests = plan_requests(&tree, &tiles, (800.0, 600.0), &HashSet::new(), |_| false, 2);
        assert_eq!(requests.len(), 2, "new work per update is capped");
    }
}
