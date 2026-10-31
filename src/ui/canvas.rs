//! Interactive treemap canvas. Geometry is computed from real scan data; labels
//! are measured/clipped per tile; every hit maps to the pixels actually drawn.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Text};
use iced::{Color, Point, Rectangle as IcedRect, Renderer, Size, Vector};

use crate::model::{Category, NO_NODE, NodeId, ScanOutput, Tree};

use crate::preview::FactKind;
use crate::render::{DrawList, DrawTile, TreemapPrimitive, TreemapState};
use crate::treemap::{self, Camera, Item, Rect, TileTier};
use crate::ui::filter::{FilterNode, FilterView};
use crate::ui::fmt;
use crate::ui::icon::{Icon, file_icon};
use crate::ui::tile_art::{ArtMap, IconRaster, UiArt};
use iced::widget::shader;
use parking_lot::Mutex;

use super::Message;
use super::theme::{self, mono_font, ui_font};

/// Hard safety budget for one scene build. The camera decides how deep the
/// scene opens; this only stops pathological trees from producing an
/// unbounded tile list in a single frame.
const MAX_TILES: usize = 9000;
/// A folder opens when its *screen* rect can hold a readable header and a
/// useful body. Because the test runs after the camera transform, zooming
/// opens one more level exactly when that level has enough pixels.
const NEST_MIN_W: f32 = 156.0;
const NEST_MIN_H: f32 = 108.0;
/// One extra nesting level is admitted for every [`NEST_ZOOM_STEP`] of camera
/// zoom, with a single level at fit. Without this, a long single-child chain
/// (whose child rect barely shrinks) would open right down to its files at
/// 100%, even though nothing was zoomed. The size gate above still decides
/// whether each admitted level has enough pixels to be readable.
const NEST_ZOOM_STEP: f32 = 1.25;
const NEST_BASE_DEPTH: u8 = 1;
/// Header strip and region gutters are specified in screen pixels and divided
/// by the camera scale when converted to content space, so they stay constant
/// while zooming instead of growing into thick slabs.
const ROOT_HEADER_PX: f32 = 26.0;
const HEADER_PX: f32 = 20.0;
const ROOT_GUTTER_PX: f32 = 2.4;
const GUTTER_PX: f32 = 1.6;
const NEST_INSET_PX: f32 = 2.0;
/// Culling tolerance: strokes and shadows near an edge may still bleed in.
const CULL_MARGIN_PX: f32 = 12.0;
/// Above this many visible children a folder is drawn as its largest members
/// plus one honest tail tile. The tail is labelled with its count and bytes and
/// is not selectable; it exists only because sub-pixel tiles communicate
/// nothing.
const MAX_REAL_CHILDREN: usize = 63;
/// A layout-only id for the aggregated tail tile. It is never a model node.
const TAIL_ID: NodeId = NodeId::MAX - 1;

#[derive(Debug, Clone)]
pub struct CanvasPalette {
    pub bed: Color,
    pub ink: Color,
    pub ink_muted: Color,
    pub ink_faint: Color,
    pub hairline: Color,
    pub accent: Color,
    pub category_fill: [Color; 8],
    pub category_ink: [Color; 8],
    pub category_line: [Color; 8],
}

impl CanvasPalette {
    /// Category fill for a nesting depth. Depth reads as a subtle tint toward
    /// the bed, never a different hue family.
    fn fill(&self, cat: Category, depth: u8) -> Color {
        let base = self.category_fill[cat.id() as usize];
        let toward_bed = 0.08 * f32::from(depth.min(3));
        mix(base, self.bed, toward_bed)
    }

    fn convert(&self, tiles: &[SceneTile]) -> DrawList {
        DrawList::from_tiles(
            (0.0, 0.0),
            tiles.iter().map(|tile| {
                let fill = if tile.aggregate {
                    mix(self.bed, self.ink_faint, 0.10)
                } else {
                    self.fill(tile.cat, tile.depth)
                };
                DrawTile::new(tile.rect, color_components(fill)).with_radius(if tile.depth == 0 {
                    4.0
                } else {
                    2.5
                })
            }),
        )
    }
}

/// Mirror of the interactive canvas scene, rendered by the instanced WGPU
/// pipeline. It shares [`build_scene`] with the canvas overlay, so fills and
/// labels can never disagree about where a tile is.
pub struct TreemapGpuProgram {
    pub output: Arc<ScanOutput>,
    pub dir: NodeId,
    pub hidden: [bool; 8],
    pub filter: Option<Arc<FilterView>>,
    pub camera: Camera,
    pub palette: CanvasPalette,
}

impl shader::Program<Message> for TreemapGpuProgram {
    type State = TreemapState;
    type Primitive = TreemapPrimitive;

    fn draw(&self, state: &Self::State, _cursor: Cursor, bounds: IcedRect) -> Self::Primitive {
        let tiles = build_tiles(
            &self.output.tree,
            self.dir,
            bounds,
            &self.camera,
            &self.hidden,
            self.filter.as_deref(),
        );
        let draw = self.palette.convert(&tiles);
        TreemapPrimitive::new(Arc::new(draw), bounds, state)
    }
}

pub struct TreemapCanvas {
    pub output: Arc<ScanOutput>,
    pub dir: NodeId,
    pub hidden: [bool; 8],
    pub query: String,
    pub filter: Option<Arc<FilterView>>,
    pub camera: Camera,
    pub selected: Option<NodeId>,
    pub palette: CanvasPalette,
    pub empty_hint: Option<String>,
    /// When the GPU program paints the fills, the canvas draws only overlays.
    pub fills: bool,
    /// Decoded on-tile art (thumbnails, snippets, folder summaries) by node.
    pub art: ArtMap,
    /// Rasterized bundled icons for on-tile glyphs.
    pub icons: Arc<IconRaster>,
    /// Actual draw bounds, reported to the app so the tile planner sees the
    /// real viewport (the old `Viewport` message was never published).
    pub report: Arc<Mutex<Option<Size>>>,
}

#[derive(Debug, Default)]
pub struct CanvasState {
    hover: Option<NodeId>,
    drag: Option<Drag>,
    last_click: Option<(Instant, NodeId)>,
    cache: Option<SceneCache>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    start: Point,
    last: Point,
    moved: bool,
}

#[derive(Debug)]
struct SceneCache {
    key: SceneKey,
    tiles: Vec<SceneTile>,
}

#[derive(Debug, Clone, PartialEq)]
struct SceneKey {
    dir: NodeId,
    width: u32,
    height: u32,
    scale: u32,
    ox: u32,
    oy: u32,
    hidden: [bool; 8],
    query: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SceneTile {
    pub node: NodeId,
    pub name: String,
    pub cat: Category,
    pub size: u64,
    pub depth: u8,
    /// Screen-space rect after the camera transform.
    pub rect: Rect,
    pub header_h: f32,
    pub has_children: bool,
    pub is_dir: bool,
    pub empty_dir: bool,
    pub aggregate: bool,
}

#[derive(Debug, Clone)]
struct SceneChild {
    id: NodeId,
    name: String,
    cat: Category,
    size: u64,
    is_dir: bool,
}

impl TreemapCanvas {
    fn scene_key(&self, bounds: IcedRect) -> SceneKey {
        SceneKey {
            dir: self.dir,
            width: bounds.width.to_bits(),
            height: bounds.height.to_bits(),
            scale: self.camera.scale.to_bits(),
            ox: self.camera.offset_x.to_bits(),
            oy: self.camera.offset_y.to_bits(),
            hidden: self.hidden,
            query: self.query.clone(),
        }
    }

    fn scene(&self, bounds: IcedRect) -> Vec<SceneTile> {
        build_tiles(
            &self.output.tree,
            self.dir,
            bounds,
            &self.camera,
            &self.hidden,
            self.filter.as_deref(),
        )
    }
}

impl canvas::Program<Message> for TreemapCanvas {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: IcedRect,
        _cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let key = self.scene_key(bounds);
        if state.cache.as_ref().map(|c| &c.key) != Some(&key) {
            state.cache = Some(SceneCache {
                key,
                tiles: self.scene(bounds),
            });
        }
        let tiles = &state.cache.as_ref().unwrap().tiles;

        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                if let Some(drag) = &mut state.drag {
                    let dx = local.x - drag.last.x;
                    let dy = local.y - drag.last.y;
                    if (local.x - drag.start.x).abs() + (local.y - drag.start.y).abs() > 4.0 {
                        drag.moved = true;
                    }
                    drag.last = local;
                    if drag.moved {
                        return Some(
                            canvas::Action::publish(Message::Pan(
                                Vector::new(dx, dy),
                                bounds.size(),
                            ))
                            .and_capture(),
                        );
                    }
                } else if _cursor.position_over(bounds).is_some() {
                    let hit = hit(tiles, local).map(|t| t.node);
                    if hit != state.hover {
                        state.hover = hit;
                        let msg = if hit == Some(NO_NODE) || hit.is_none() {
                            Message::Hover(None)
                        } else {
                            Message::Hover(hit)
                        };
                        return Some(canvas::Action::publish(msg));
                    }
                } else if state.hover.take().is_some() {
                    return Some(canvas::Action::publish(Message::Hover(None)));
                }
                None
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = _cursor.position_over(bounds)?;
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                state.drag = Some(Drag {
                    start: local,
                    last: local,
                    moved: false,
                });
                Some(canvas::Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let drag = state.drag.take()?;
                if drag.moved {
                    return Some(canvas::Action::capture());
                }
                let hit = hit(tiles, drag.last).map(|t| t.node);
                let Some(id) = hit else {
                    return Some(canvas::Action::publish(Message::ClearSelection));
                };
                if id == NO_NODE {
                    return Some(canvas::Action::publish(Message::ClearSelection));
                }
                let now = Instant::now();
                let double = state
                    .last_click
                    .map(|(at, last)| {
                        last == id && now.duration_since(at) < Duration::from_millis(420)
                    })
                    .unwrap_or(false);
                state.last_click = Some((now, id));
                let message = if double {
                    Message::Drill(id)
                } else {
                    Message::Select(id)
                };
                Some(canvas::Action::publish(message).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let position = _cursor.position_over(bounds)?;
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                let hit = hit(tiles, local).map(|t| t.node);
                match hit {
                    Some(id) if id != NO_NODE => Some(
                        canvas::Action::publish(Message::Context(
                            id,
                            Point::new(bounds.x + local.x, bounds.y + local.y),
                        ))
                        .and_capture(),
                    ),
                    _ => Some(canvas::Action::publish(Message::CloseContext).and_capture()),
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let position = _cursor.position_over(bounds)?;
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if y.abs() < 0.01 {
                    return None;
                }
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                let factor = 1.18f32.powf(y).clamp(0.55, 1.8);
                Some(
                    canvas::Action::publish(Message::Zoom(factor, local, bounds.size()))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                if state.hover.take().is_some() {
                    Some(canvas::Action::publish(Message::Hover(None)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: IcedRect,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        *self.report.lock() = Some(bounds.size());
        let key = self.scene_key(bounds);
        let owned;
        let tiles = if let Some(cache) = &state.cache {
            if cache.key == key {
                &cache.tiles
            } else {
                owned = self.scene(bounds);
                &owned
            }
        } else {
            owned = self.scene(bounds);
            &owned
        };

        let mut frame = Frame::new(renderer, bounds.size());
        if self.fills {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.palette.bed);
        }

        if tiles.is_empty() {
            let hint = self
                .empty_hint
                .clone()
                .unwrap_or_else(|| "Nothing to draw here.".into());
            frame.fill_text(Text {
                content: hint,
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0 - 8.0),
                color: self.palette.ink_muted,
                size: 13.0.into(),
                font: ui_font(iced::font::Weight::Medium),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: Vertical::Center,
                ..Text::default()
            });
            return vec![frame.into_geometry()];
        }

        for tile in tiles {
            let radius: f32 = if tile.depth == 0 { 4.0 } else { 2.5 };
            let path = rounded_path(tile.rect, radius);
            let fill = if tile.aggregate {
                mix(self.palette.bed, self.palette.ink_faint, 0.10)
            } else {
                self.palette.fill(tile.cat, tile.depth)
            };
            if self.fills {
                frame.fill(&path, fill);
            }

            if tile.aggregate || tile.cat == Category::Caches || tile.empty_dir {
                let ink = if tile.aggregate {
                    self.palette.ink_muted
                } else {
                    self.palette.category_ink[tile.cat.id() as usize]
                };
                // Parent fills are behind every child in the GPU layer. Only
                // hatch the header of a container, never its children's art.
                let hatch_rect = if tile.has_children {
                    Rect {
                        h: tile.header_h,
                        ..tile.rect
                    }
                } else {
                    tile.rect
                };
                for (from, to) in hatch_segments(hatch_rect, bounds.size()) {
                    frame.stroke(
                        &Path::line(from, to),
                        canvas::Stroke::default()
                            .with_color(mix(fill, ink, 0.20))
                            .with_width(1.0),
                    );
                }
            }

            if tile.has_children && tile.header_h > 0.0 {
                let y = tile.rect.y + tile.header_h;
                frame.fill_rectangle(
                    Point::new(tile.rect.x, y),
                    Size::new(tile.rect.w, 1.0),
                    mix(
                        self.palette.category_line[tile.cat.id() as usize],
                        fill,
                        0.35,
                    ),
                );
            }

            if tile.has_children || tile.depth == 0 {
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(if tile.aggregate {
                            mix(fill, self.palette.ink_muted, 0.24)
                        } else {
                            self.palette.category_line[tile.cat.id() as usize]
                        })
                        .with_width(1.0),
                );
            } else if !tile.aggregate {
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(mix(
                            self.palette.category_line[tile.cat.id() as usize],
                            fill,
                            0.48,
                        ))
                        .with_width(0.7),
                );
            }

            let hovered = state.hover == Some(tile.node) && tile.node != NO_NODE;
            let selected = self.selected == Some(tile.node) && tile.node != NO_NODE;
            if hovered || selected {
                let inset = 1.5;
                let outline = rounded_path(
                    Rect {
                        x: tile.rect.x + inset,
                        y: tile.rect.y + inset,
                        w: (tile.rect.w - inset * 2.0).max(0.0),
                        h: (tile.rect.h - inset * 2.0).max(0.0),
                    },
                    (radius - 0.5).max(0.0),
                );
                if selected {
                    frame.stroke(
                        &outline,
                        canvas::Stroke::default()
                            .with_color(self.palette.accent)
                            .with_width(2.0),
                    );
                } else {
                    frame.stroke(
                        &outline,
                        canvas::Stroke::default()
                            .with_color(mix(self.palette.accent, self.palette.ink, 0.35))
                            .with_width(1.2),
                    );
                }
            }

            self.draw_tile_content(&mut frame, tile, fill);
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: IcedRect,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some_and(|d| d.moved) {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(_bounds) {
            if state.hover.is_some() {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::Grab
            }
        } else {
            mouse::Interaction::default()
        }
    }
}

impl TreemapCanvas {
    /// Draws everything that sits on a tile. Large leaf/folder tiles get real
    /// content (thumbnail, snippet, summary or icon) plus the measured name
    /// and large size figure; smaller tiers degrade to icon + measured label
    /// and finally to a clean fill. Every text run is fitted with the measured
    /// Geist width estimate, so labels never spill their tile.
    fn draw_tile_content(&self, frame: &mut Frame, tile: &SceneTile, fill: Color) {
        let r = tile.rect;
        if r.w < 24.0 || r.h < 16.0 {
            return;
        }
        if tile.aggregate {
            self.draw_aggregate_label(frame, tile);
            return;
        }
        let tier = treemap::tile_tier(r.w, r.h);
        if tier == TileTier::Large && !tile.has_children {
            self.draw_large(frame, tile, fill);
            return;
        }
        if !tile.has_children && compact_art_eligible(r.w, r.h) {
            self.draw_compact_art(frame, tile, fill);
            return;
        }
        self.draw_compact(frame, tile, tier);
    }

    /// Medium/small leaves big enough for a real body: art sits above a bottom
    /// name band. The bands come from [`compact_art_layout`], so the name can
    /// never sit on top of the thumbnail or snippet. Unsupported or still
    /// loading art degrades to the bundled icon in the same body.
    fn draw_compact_art(&self, frame: &mut Frame, tile: &SceneTile, fill: Color) {
        let layout = compact_art_layout(tile.rect);
        if !tile.empty_dir
            && layout.art.w >= 24.0
            && layout.art.h >= 16.0
            && !self.draw_compact_art_body(frame, tile, layout.art, fill)
        {
            let icon = if tile.is_dir {
                Icon::Folder
            } else {
                file_icon(&tile.name)
            };
            let box_px = layout.art.w.min(layout.art.h).clamp(14.0, 40.0);
            self.draw_icon(
                frame,
                icon,
                box_px,
                Point::new(
                    layout.art.x + (layout.art.w - box_px) * 0.5,
                    layout.art.y + (layout.art.h - box_px) * 0.5,
                ),
                mix(
                    self.palette.category_ink[tile.cat.id() as usize],
                    fill,
                    0.30,
                ),
            );
        }
        self.draw_compact_art_name(frame, layout.name, tile);
    }

    /// Shared body renderer for compact tiles. Returns `false` when nothing
    /// was painted, so the caller can fall back to a bundled icon.
    fn draw_compact_art_body(
        &self,
        frame: &mut Frame,
        tile: &SceneTile,
        body: Rect,
        fill: Color,
    ) -> bool {
        match self.art.get(&tile.node).map(|art| art.as_ref()) {
            Some(UiArt::Image {
                handle,
                width,
                height,
            }) => {
                self.draw_thumbnail(frame, body, *width, *height, handle);
                true
            }
            Some(UiArt::Text { lines }) if body.h >= SNIPPET_LINE_H => {
                self.draw_snippet(frame, body, lines, fill);
                true
            }
            Some(UiArt::Facts { kind, lines }) if body.h >= SNIPPET_LINE_H + 14.0 => {
                let glyph = (*kind == FactKind::Executable).then(|| file_icon(&tile.name));
                self.draw_facts(frame, body, *kind, lines, fill, glyph);
                true
            }
            Some(UiArt::Folder {
                files,
                items,
                categories,
            }) if categories.total() == tile.size
                && categories.total() > 0
                && body.h >= SUMMARY_STATS_H + SUMMARY_BAR_H + 3.0 =>
            {
                self.draw_folder_summary(
                    frame,
                    body,
                    *files,
                    *items,
                    categories,
                    self.palette.category_ink[tile.cat.id() as usize],
                );
                true
            }
            _ => false,
        }
    }

    /// Compact art keeps a name and a separate mono figure below the image.
    fn draw_compact_art_name(&self, frame: &mut Frame, band: Rect, tile: &SceneTile) {
        if band.w < 16.0 || band.h < 12.0 {
            return;
        }
        let ink = self.palette.category_ink[tile.cat.id() as usize];
        let size_pt = theme::FIGURE_MD;
        let size_text = fmt::bytes_short(tile.size);
        let show_size =
            band.h >= theme::LINE_SM + size_pt && mono_text_width(&size_text, size_pt) <= band.w;
        let name = fit_text(&tile.name, band.w, theme::TEXT_SM);
        let weight = if tile.is_dir {
            iced::font::Weight::Semibold
        } else {
            iced::font::Weight::Medium
        };
        frame.with_clip(rect(band), |frame| {
            if !name.is_empty() {
                frame.fill_text(Text {
                    content: name,
                    position: Point::new(band.x, band.y),
                    color: ink,
                    size: theme::TEXT_SM.into(),
                    font: ui_font(weight),
                    max_width: band.w,
                    ..Text::default()
                });
            }
            if show_size {
                frame.fill_text(Text {
                    content: size_text,
                    position: Point::new(band.x, band.bottom() - size_pt),
                    color: ink,
                    size: size_pt.into(),
                    font: mono_font(iced::font::Weight::Semibold),
                    ..Text::default()
                });
            }
        });
    }

    /// Header labels for container tiles and the legacy compact tiers. The
    /// size figure is shown only when it measures into the band.
    fn draw_compact(&self, frame: &mut Frame, tile: &SceneTile, tier: TileTier) {
        let r = tile.rect;
        if tier == TileTier::Tiny {
            return;
        }
        let pad = if tile.depth == 0 { 8.0 } else { 6.0 };
        let size = theme::TEXT_SM;
        let label_rect = Rect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: if tile.has_children {
                tile.header_h
            } else {
                r.h
            },
        };
        let max_width = (label_rect.w - pad * 2.0).max(0.0);
        if max_width < 22.0 || label_rect.h < size + 2.0 {
            return;
        }

        let size_text = fmt::bytes_short(tile.size);
        let figure_size = if tile.has_children {
            theme::FIGURE_SM
        } else {
            theme::FIGURE_MD
        };
        let size_width = mono_text_width(&size_text, figure_size);
        // Medium file tiles carry a small icon left of the name.
        let icon_for = (tier == TileTier::Medium && !tile.has_children).then(|| {
            (
                if tile.is_dir {
                    Icon::Folder
                } else {
                    file_icon(&tile.name)
                },
                20.0f32,
            )
        });
        let name_left = pad + icon_for.map_or(0.0, |(_, w)| w + 5.0);
        let name_max = (max_width - (name_left - pad)).max(16.0);

        let header_size = (tile.has_children || tile.aggregate)
            && label_rect.h >= 22.0
            && label_rect.w > 142.0
            && size_width + 30.0 < max_width;
        let leaf_size = !tile.has_children
            && !tile.aggregate
            && tile.rect.h > 54.0
            && tile.rect.w > 92.0
            && size_width + pad * 2.0 < label_rect.w;
        let name_width = (name_max - if header_size { size_width + 12.0 } else { 0.0 }).max(16.0);
        let name = fit_text(&tile.name, name_width, size);
        let color = if tile.aggregate || tile.node == NO_NODE {
            self.palette.ink_muted
        } else {
            self.palette.category_ink[tile.cat.id() as usize]
        };
        let weight = if tile.has_children {
            iced::font::Weight::Semibold
        } else {
            iced::font::Weight::Medium
        };

        frame.with_clip(rect(label_rect), |frame| {
            if let Some((glyph, box_px)) = icon_for
                && !name.is_empty()
            {
                let cy = label_rect.y + pad + size * 0.5;
                self.draw_icon(
                    frame,
                    glyph,
                    box_px,
                    Point::new(label_rect.x + pad, cy - box_px * 0.5),
                    color,
                );
            }
            if !name.is_empty() {
                frame.fill_text(Text {
                    content: name,
                    position: Point::new(label_rect.x + name_left, label_rect.y + pad - 1.0),
                    color,
                    size: size.into(),
                    font: ui_font(weight),
                    max_width: name_width,
                    ..Text::default()
                });
            }
            if header_size {
                frame.fill_text(Text {
                    content: size_text,
                    position: Point::new(label_rect.right() - pad, label_rect.y + pad + 1.0),
                    color,
                    size: figure_size.into(),
                    font: mono_font(iced::font::Weight::Normal),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    ..Text::default()
                });
            }
            if leaf_size {
                frame.fill_text(Text {
                    content: fmt::bytes_short(tile.size),
                    position: Point::new(
                        label_rect.x + pad,
                        label_rect.bottom() - pad - figure_size,
                    ),
                    color,
                    size: figure_size.into(),
                    font: mono_font(iced::font::Weight::Semibold),
                    ..Text::default()
                });
            }
        });
    }

    /// XL content: art in the body band, measured name on top, large mono size
    /// at the bottom. The bands come from `tile_content_layout`, which keeps
    /// them disjoint, so nothing can overlap or clip.
    fn draw_large(&self, frame: &mut Frame, tile: &SceneTile, fill: Color) {
        let r = tile.rect;
        let root = tile.depth == 0;
        let pad = if root { 12.0 } else { 8.0 };
        let name_h = theme::LINE_BASE + 4.0;
        let size_h = theme::LINE_FIGURE_XL + 2.0;
        let layout = treemap::tile_content_layout(r, pad, name_h, size_h);
        let ink = self.palette.category_ink[tile.cat.id() as usize];
        let faint = ink;

        // Extension badge, top-right of the name band.
        let ext = if tile.is_dir {
            String::new()
        } else {
            extension_upper(&tile.name)
        };
        let name_size = theme::TEXT_BASE;
        let mut badge_w = 0.0;
        if !ext.is_empty() {
            let badge_size = theme::TEXT_XS;
            badge_w = mono_text_width(&ext, badge_size) + 6.0;
            if badge_w + 6.0 < layout.name.w {
                frame.fill_text(Text {
                    content: ext.clone(),
                    position: Point::new(
                        layout.name.right() - badge_w,
                        layout.name.y + (layout.name.h - badge_size) * 0.5,
                    ),
                    color: faint,
                    size: badge_size.into(),
                    font: mono_font(iced::font::Weight::Medium),
                    ..Text::default()
                });
            } else {
                badge_w = 0.0;
            }
        }
        let name = fit_text(
            &tile.name,
            (layout.name.w - badge_w - 6.0).max(12.0),
            name_size,
        );
        if !name.is_empty() {
            frame.fill_text(Text {
                content: name,
                position: Point::new(layout.name.x, layout.name.y + 2.0),
                color: ink,
                size: name_size.into(),
                font: ui_font(iced::font::Weight::Semibold),
                max_width: layout.name.w - badge_w - 6.0,
                ..Text::default()
            });
        }

        // Large mono size figure, bottom-left; shrinks only if it cannot fit.
        let size_text = fmt::bytes_short(tile.size);
        let size_pt = [
            theme::FIGURE_XL,
            theme::FIGURE_LG,
            theme::FIGURE_MD,
            theme::FIGURE_SM,
        ]
        .into_iter()
        .find(|size| mono_text_width(&size_text, *size) <= layout.size.w)
        .unwrap_or(theme::FIGURE_SM);
        let size_y = (layout.size.y + layout.size.h - size_pt).max(layout.size.y);
        frame.fill_text(Text {
            content: size_text,
            position: Point::new(layout.size.x, size_y),
            color: ink,
            size: size_pt.into(),
            font: mono_font(iced::font::Weight::Semibold),
            max_width: layout.size.w,
            ..Text::default()
        });

        let body = layout.body;
        if body.w < 40.0 || body.h < 28.0 {
            return;
        }
        match self.art.get(&tile.node).map(|art| art.as_ref()) {
            Some(UiArt::Image {
                handle,
                width,
                height,
            }) => self.draw_thumbnail(frame, body, *width, *height, handle),
            Some(UiArt::Text { lines }) => {
                self.draw_snippet(frame, body, lines, fill);
            }
            Some(UiArt::Facts { kind, lines }) => {
                let glyph = (*kind == FactKind::Executable).then(|| file_icon(&tile.name));
                self.draw_facts(frame, body, *kind, lines, fill, glyph);
            }
            Some(UiArt::Folder {
                files,
                items,
                categories,
            }) if categories.total() == tile.size => {
                self.draw_folder_summary(frame, body, *files, *items, categories, ink);
            }
            // Partial filter containers show their matching size and a folder
            // icon; the full folder's composition would contradict that size.
            Some(UiArt::Folder { .. }) | Some(UiArt::Unavailable) => {
                self.draw_icon(
                    frame,
                    if tile.is_dir {
                        Icon::Folder
                    } else {
                        file_icon(&tile.name)
                    },
                    body.h.min(72.0),
                    Point::new(
                        body.x + (body.w - body.h.min(72.0)) * 0.5,
                        body.y + (body.h - body.h.min(72.0)) * 0.5,
                    ),
                    mix(
                        self.palette.category_ink[tile.cat.id() as usize],
                        fill,
                        0.30,
                    ),
                );
            }
            None if tile.is_dir || crate::preview::tile_kind_for_name(&tile.name).is_some() => {
                // Neutral loading placeholder: the tile keeps its name and
                // size; the panel only marks that art is still in flight.
                frame.fill(&rounded_path(body, 3.0), mix(fill, self.palette.bed, 0.12));
                frame.fill_text(Text {
                    content: "loading".into(),
                    position: Point::new(body.x + body.w * 0.5, body.y + body.h * 0.5 - 6.0),
                    color: faint,
                    size: theme::TEXT_XS.into(),
                    font: mono_font(iced::font::Weight::Normal),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    max_width: body.w,
                    ..Text::default()
                });
            }
            None => {
                // Apps, archives, media and unknown binaries never get an art
                // request: draw their bundled icon right away instead of a
                // loading plate that could never resolve.
                self.draw_icon(
                    frame,
                    if tile.is_dir {
                        Icon::Folder
                    } else {
                        file_icon(&tile.name)
                    },
                    body.h.min(72.0),
                    Point::new(
                        body.x + (body.w - body.h.min(72.0)) * 0.5,
                        body.y + (body.h - body.h.min(72.0)) * 0.5,
                    ),
                    mix(
                        self.palette.category_ink[tile.cat.id() as usize],
                        fill,
                        0.30,
                    ),
                );
            }
        }
    }

    fn draw_thumbnail(
        &self,
        frame: &mut Frame,
        body: Rect,
        width: u32,
        height: u32,
        handle: &iced::widget::image::Handle,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let sw = width as f32;
        let sh = height as f32;
        let scale = (body.w / sw).min(body.h / sh);
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let (dw, dh) = (sw * scale, sh * scale);
        let at = Point::new(body.x + (body.w - dw) * 0.5, body.y + (body.h - dh) * 0.5);
        // Aspect-fit guarantees the bitmap stays inside `body`; the clip is a
        // second line of defence against rounding at the rounded tile edge.
        let image = iced::advanced::image::Image::new(handle.clone());
        let target = IcedRect::new(at, Size::new(dw, dh));
        frame.with_clip(rect(body), |frame| {
            frame.draw_image(target, image);
        });
    }

    /// Factual preview (archive entries, SQLite tables, media tags, version
    /// info, metadata or hex) with a small mono family label above it. Executable
    /// facts also carry the file's icon beside the label.
    fn draw_facts(
        &self,
        frame: &mut Frame,
        body: Rect,
        kind: FactKind,
        lines: &[String],
        fill: Color,
        glyph: Option<Icon>,
    ) {
        if body.w < 32.0 || body.h < SNIPPET_LINE_H + 14.0 || lines.is_empty() {
            return;
        }
        let label_color = mix(self.palette.ink, fill, 0.42);
        let icon_px = glyph.map(|_| body.h.min(28.0).clamp(14.0, 28.0));
        let label_width = (body.w - icon_px.map_or(0.0, |px| px + 6.0)).max(20.0);
        frame.with_clip(rect(body), |frame| {
            frame.fill_text(Text {
                content: kind.label().into(),
                position: Point::new(body.x, body.y),
                color: label_color,
                size: theme::TEXT_XS.into(),
                font: mono_font(iced::font::Weight::Semibold),
                max_width: label_width,
                ..Text::default()
            });
        });
        if let (Some(glyph), Some(px)) = (glyph, icon_px) {
            self.draw_icon(
                frame,
                glyph,
                px,
                Point::new(body.right() - px, body.y - 2.0),
                label_color,
            );
        }
        let text = Rect {
            x: body.x,
            y: body.y + SNIPPET_LINE_H,
            w: body.w,
            h: (body.h - SNIPPET_LINE_H).max(0.0),
        };
        self.draw_snippet(frame, text, lines, fill);
    }

    fn draw_snippet(&self, frame: &mut Frame, body: Rect, lines: &[String], fill: Color) {
        let layout = layout_snippet(lines, body);
        if layout.rows.is_empty() {
            return;
        }
        let color = mix(self.palette.ink, fill, 0.12);
        let clip = IcedRect::new(Point::new(body.x, body.y), Size::new(body.w, layout.clip_h));
        frame.with_clip(clip, |frame| {
            for (i, line) in layout.rows.iter().enumerate() {
                frame.fill_text(Text {
                    content: line.clone(),
                    position: Point::new(body.x, body.y + i as f32 * SNIPPET_LINE_H),
                    color,
                    size: SNIPPET_LINE_PT.into(),
                    font: mono_font(iced::font::Weight::Normal),
                    // One logical source line is one row. Wrapping is disabled
                    // outright; fitting already guaranteed the row fits.
                    max_width: f32::INFINITY,
                    ..Text::default()
                });
            }
        });
        if layout.has_more && layout.continuation_h >= theme::TEXT_XS {
            // A solid continuation mark leaves the source readable. No fade
            // or gradient is painted over the final source line.
            frame.fill_text(Text {
                content: "…".into(),
                position: Point::new(body.x, body.bottom() - theme::TEXT_XS),
                color,
                size: theme::TEXT_XS.into(),
                font: mono_font(iced::font::Weight::Medium),
                ..Text::default()
            });
        }
    }

    fn draw_folder_summary(
        &self,
        frame: &mut Frame,
        body: Rect,
        files: u64,
        items: u32,
        categories: &crate::model::CategoryTotals,
        ink: Color,
    ) {
        let total = categories.total();
        let layout = folder_summary_layout(body);
        if total == 0 || layout.bar.is_none() {
            return;
        }
        let stats = format!(
            "{} files · {} items",
            fmt::count(files),
            fmt::count(u64::from(items))
        );
        if layout.stats.h >= 12.0 && layout.stats.w >= 32.0 {
            frame.with_clip(rect(layout.stats), |frame| {
                frame.fill_text(Text {
                    content: fit_mono_text(&stats, layout.stats.w, theme::TEXT_XS),
                    position: Point::new(layout.stats.x, layout.stats.y + 1.0),
                    color: ink,
                    size: theme::TEXT_XS.into(),
                    font: mono_font(iced::font::Weight::Medium),
                    max_width: layout.stats.w,
                    ..Text::default()
                });
            });
        }

        let top = top_categories(categories);
        for (row, (cat, bytes)) in layout.rows.iter().zip(top.iter()) {
            if row.w < 46.0 {
                continue;
            }
            let row_color = ink;
            let swatch = 7.0;
            let swatch_y = row.y + (row.h - swatch) * 0.5;
            frame.fill(
                &rounded_path(
                    Rect {
                        x: row.x,
                        y: swatch_y,
                        w: swatch,
                        h: swatch,
                    },
                    1.5,
                ),
                self.palette.category_fill[cat.id() as usize],
            );
            let size_text = fmt::bytes_short(*bytes);
            let size_w = mono_text_width(&size_text, theme::TEXT_XS);
            let name_max = (row.w - swatch - 5.0 - size_w - 6.0).max(0.0);
            if name_max >= 12.0 {
                frame.with_clip(rect(*row), |frame| {
                    frame.fill_text(Text {
                        content: fit_text(cat.label(), name_max, theme::TEXT_XS),
                        position: Point::new(row.x + swatch + 5.0, row.y + 1.0),
                        color: row_color,
                        size: theme::TEXT_XS.into(),
                        font: ui_font(iced::font::Weight::Medium),
                        max_width: name_max,
                        ..Text::default()
                    });
                });
            }
            frame.fill_text(Text {
                content: size_text,
                position: Point::new(row.right(), row.y + 1.5),
                color: ink,
                size: theme::TEXT_XS.into(),
                font: mono_font(iced::font::Weight::Medium),
                align_x: iced::alignment::Horizontal::Right.into(),
                max_width: row.w,
                ..Text::default()
            });
        }

        if let Some(bar) = layout.bar {
            let active = Category::ALL
                .into_iter()
                .filter(|cat| categories.get(*cat) > 0)
                .count();
            let gap = 1.0f32;
            let usable = (bar.w - gap * active.saturating_sub(1) as f32).max(1.0);
            let mut x = bar.x;
            let mut drawn = 0usize;
            for cat in Category::ALL {
                let bytes = categories.get(cat);
                if bytes == 0 {
                    continue;
                }
                drawn += 1;
                let remaining = bar.right() - x;
                if remaining <= 0.0 {
                    break;
                }
                let w = if drawn == active {
                    remaining
                } else {
                    (usable * (bytes as f64 / total as f64) as f32)
                        .max(1.0)
                        .min((remaining - gap).max(1.0))
                };
                frame.fill_rectangle(
                    Point::new(x, bar.y),
                    Size::new(w, bar.h),
                    self.palette.category_fill[cat.id() as usize],
                );
                x += w + gap;
            }
            frame.stroke(
                &Path::rectangle(Point::new(bar.x, bar.y), Size::new(bar.w, bar.h)),
                canvas::Stroke::default()
                    .with_color(self.palette.hairline)
                    .with_width(0.8),
            );
        }
    }

    fn draw_aggregate_label(&self, frame: &mut Frame, tile: &SceneTile) {
        let r = tile.rect;
        if r.w < 60.0 || r.h < 30.0 {
            return;
        }
        let pad = 8.0;
        let width = r.w - pad * 2.0;
        let size_text = fmt::bytes_short(tile.size);
        let figure_size = if r.w >= 180.0 && r.h >= 100.0 {
            theme::FIGURE_LG
        } else {
            theme::FIGURE_MD
        };
        let show_size = r.h >= 56.0 && mono_text_width(&size_text, figure_size) <= width;
        frame.with_clip(rect(r), |frame| {
            frame.fill_text(Text {
                content: fit_text(&tile.name, width, theme::TEXT_SM),
                position: Point::new(r.x + pad, r.y + pad),
                color: self.palette.ink_muted,
                size: theme::TEXT_SM.into(),
                font: ui_font(iced::font::Weight::Medium),
                max_width: r.w - pad * 2.0,
                ..Text::default()
            });
            if show_size {
                frame.fill_text(Text {
                    content: size_text,
                    position: Point::new(r.x + pad, r.bottom() - pad - figure_size),
                    color: self.palette.ink_muted,
                    size: figure_size.into(),
                    font: mono_font(iced::font::Weight::Semibold),
                    max_width: r.w - pad * 2.0,
                    ..Text::default()
                });
            }
        });
    }

    /// Rasterized bundled icon scaled into a `size` px box, top-left at `at`.
    fn draw_icon(&self, frame: &mut Frame, glyph: Icon, size: f32, at: Point, color: Color) {
        let handle = self.icons.handle(glyph, size, color);
        frame.draw_image(
            IcedRect::new(at, Size::new(size, size)),
            iced::advanced::image::Image::new(handle),
        );
    }
}

fn extension_upper(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("");
    if ext.is_empty() || ext.eq_ignore_ascii_case(name) || ext.len() > 6 {
        String::new()
    } else {
        ext.to_ascii_uppercase()
    }
}

/// Parallel 45-degree strokes, clipped analytically to both the tile and the
/// viewport. Work depends on visible pixels, even at maximum zoom.
fn hatch_segments(r: Rect, viewport: Size) -> Vec<(Point, Point)> {
    let inset = r.inset(2.0);
    let left = inset.x.max(0.0);
    let top = inset.y.max(0.0);
    let right = inset.right().min(viewport.width);
    let bottom = inset.bottom().min(viewport.height);
    if right <= left || bottom <= top {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut diagonal = ((left + top) / 10.0).ceil() * 10.0;
    while diagonal < right + bottom {
        let x1 = left.max(diagonal - bottom);
        let x2 = right.min(diagonal - top);
        if x2 > x1 {
            lines.push((Point::new(x1, diagonal - x1), Point::new(x2, diagonal - x2)));
        }
        diagonal += 10.0;
    }
    lines
}

/// Point size and fixed row pitch for on-tile text snippets. Geist Mono is
/// monospaced, so every row advances by exactly one pitch.
const SNIPPET_LINE_PT: f32 = theme::TEXT_XS;
const SNIPPET_LINE_H: f32 = 15.0;
/// Conservative Geist Mono advance as a fraction of the point size. Real
/// Geist Mono advances are ~0.6 em; 0.64 leaves headroom for hinting and
/// rasterization so a fitted row can never wrap.
const SNIPPET_MONO_ADVANCE_EM: f32 = 0.64;
const SNIPPET_CONTINUATION_H: f32 = 12.0;
const SUMMARY_STATS_H: f32 = 15.0;
const SUMMARY_ROW_H: f32 = 14.0;
const SUMMARY_BAR_H: f32 = 12.0;

/// Smallest screen tile that can stack a usable art body above its name.
/// Below this both bands collapse into a single name row (`draw_compact`).
const COMPACT_ART_MIN: f32 = 90.0;
const COMPACT_ART_PAD: f32 = 6.0;
const COMPACT_ART_NAME_H: f32 = 32.0;
const COMPACT_ART_GAP: f32 = 2.0;

/// Compact art is for leaves only: a non-Large tile that is at least
/// 90x90. It shares the same 8-per-update / 3-job planner gates as Large art.
pub(crate) fn compact_art_eligible(w: f32, h: f32) -> bool {
    w.is_finite()
        && h.is_finite()
        && w >= COMPACT_ART_MIN
        && h >= COMPACT_ART_MIN
        && treemap::tile_tier(w, h) != TileTier::Large
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CompactArtLayout {
    art: Rect,
    name: Rect,
}

/// Stacks a compact tile into a disjoint art body and a bottom name band. The
/// body takes every remaining pixel; the name band is capped so short tiles
/// still keep a readable body. Both bands stay inside the inset tile.
fn compact_art_layout(r: Rect) -> CompactArtLayout {
    let pad = COMPACT_ART_PAD;
    let inner_w = (r.w - pad * 2.0).max(0.0);
    let inner_h = (r.h - pad * 2.0).max(0.0);
    // On a normal eligible tile the gap is the full 2 px; the proportional
    // fallback keeps degenerate rects inside the tile instead of overflowing.
    let gap = COMPACT_ART_GAP.min(inner_h * 0.1);
    let name_h = COMPACT_ART_NAME_H.min((inner_h - gap).max(0.0));
    let art_h = (inner_h - name_h - gap).max(0.0);
    let art = Rect {
        x: r.x + pad,
        y: r.y + pad,
        w: inner_w,
        h: art_h,
    };
    CompactArtLayout {
        art,
        name: Rect {
            x: art.x,
            y: art.y + art_h + gap,
            w: inner_w,
            h: name_h,
        },
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FolderSummaryLayout {
    stats: Rect,
    rows: Vec<Rect>,
    bar: Option<Rect>,
}

/// Stacks the folder summary into disjoint bands: a stats line, up to three
/// category rows, and a 12 px composition bar. The bar only appears when the
/// body is tall enough for the stats line plus a clear 2 px gap.
fn folder_summary_layout(body: Rect) -> FolderSummaryLayout {
    if body.w <= 0.0 || body.h <= 0.0 || !body.w.is_finite() || !body.h.is_finite() {
        return FolderSummaryLayout {
            stats: Rect::ZERO,
            rows: Vec::new(),
            bar: None,
        };
    }
    let stats = Rect {
        x: body.x,
        y: body.y,
        w: body.w,
        h: SUMMARY_STATS_H.min(body.h),
    };
    if body.h < SUMMARY_STATS_H + SUMMARY_BAR_H + 3.0 {
        return FolderSummaryLayout {
            stats,
            rows: Vec::new(),
            bar: None,
        };
    }
    let bar = Rect {
        x: body.x,
        y: body.bottom() - SUMMARY_BAR_H,
        w: body.w,
        h: SUMMARY_BAR_H,
    };
    let rows_top = stats.bottom() + 2.0;
    let rows_bottom = bar.y - 2.0;
    let available = (rows_bottom - rows_top).max(0.0);
    let count = ((available / SUMMARY_ROW_H).floor() as usize).min(3);
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        rows.push(Rect {
            x: body.x,
            y: rows_top + i as f32 * SUMMARY_ROW_H,
            w: body.w,
            h: SUMMARY_ROW_H,
        });
    }
    FolderSummaryLayout {
        stats,
        rows,
        bar: Some(bar),
    }
}

/// Largest categories by exact bytes, with category id as a stable tie-break.
fn top_categories(totals: &crate::model::CategoryTotals) -> Vec<(Category, u64)> {
    let mut entries: Vec<(Category, u64)> = Category::ALL
        .into_iter()
        .map(|cat| (cat, totals.get(cat)))
        .filter(|(_, bytes)| *bytes > 0)
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id().cmp(&b.0.id())));
    entries.truncate(3);
    entries
}

/// The rows that can be painted in a snippet body, after horizontal fitting
/// and vertical row reservation. Kept pure so geometry can be regression
/// tested without a renderer.
#[derive(Debug, Clone, PartialEq)]
struct SnippetLayout {
    rows: Vec<String>,
    has_more: bool,
    continuation_h: f32,
    clip_h: f32,
}

fn layout_snippet(lines: &[String], body: Rect) -> SnippetLayout {
    let empty = SnippetLayout {
        rows: Vec::new(),
        has_more: false,
        continuation_h: 0.0,
        clip_h: 0.0,
    };
    if body.w <= 0.0
        || body.h <= 0.0
        || !body.w.is_finite()
        || !body.h.is_finite()
        || lines.is_empty()
    {
        return empty;
    }

    let full_rows = (body.h / SNIPPET_LINE_H).floor() as usize;
    if full_rows == 0 {
        return empty;
    }
    let has_more = lines.len() > full_rows;
    // Reserve a continuation band only when rows continue below the tile.
    // Otherwise the last line gets the full body height.
    let continuation_h = if has_more {
        SNIPPET_CONTINUATION_H.min((body.h - SNIPPET_LINE_H).max(0.0))
    } else {
        0.0
    };
    let clip_h = body.h - continuation_h;
    let row_capacity = ((clip_h / SNIPPET_LINE_H).floor() as usize).clamp(1, full_rows);
    let take = lines.len().min(row_capacity);
    let rows = lines
        .iter()
        .take(take)
        .map(|line| fit_mono_text(line, body.w, SNIPPET_LINE_PT))
        .collect();
    SnippetLayout {
        rows,
        has_more: has_more && take < lines.len(),
        continuation_h,
        clip_h,
    }
}

/// Expands tabs to four-column stops, then truncates to the widest run that
/// fits `max_width` at the conservative mono advance. The result can never be
/// wider than the body under the same measurement used by the draw path.
fn fit_mono_text(text: &str, max_width: f32, size: f32) -> String {
    if text.is_empty()
        || !max_width.is_finite()
        || max_width <= 0.0
        || !size.is_finite()
        || size <= 0.0
    {
        return String::new();
    }
    let expanded = expand_tabs(text);
    if mono_text_width(&expanded, size) <= max_width {
        return expanded;
    }
    let ellipsis = '…';
    let ellipsis_w = mono_char_width(ellipsis, size);
    if ellipsis_w > max_width {
        return String::new();
    }
    let budget = max_width - ellipsis_w;
    let mut out = String::new();
    let mut used = 0.0f32;
    for ch in expanded.chars() {
        let width = mono_char_width(ch, size);
        if used + width > budget {
            break;
        }
        used += width;
        out.push(ch);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        ellipsis.to_string()
    } else {
        format!("{trimmed}{ellipsis}")
    }
}

fn expand_tabs(text: &str) -> String {
    const TAB: usize = 4;
    let mut out = String::with_capacity(text.len());
    let mut column = 0usize;
    for ch in text.chars() {
        if ch == '\t' {
            let spaces = TAB - (column % TAB);
            for _ in 0..spaces {
                out.push(' ');
            }
            column += spaces;
        } else if ch == '\n' || ch == '\r' {
            // Defensive: one logical line per row, even if a caller slipped a
            // newline through the bounded preview path.
            column = 0;
        } else {
            out.push(ch);
            column += mono_cells(ch);
        }
    }
    out
}

fn mono_text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|ch| mono_char_width(ch, size))
        .sum::<f32>()
}

fn mono_char_width(ch: char, size: f32) -> f32 {
    mono_cells(ch) as f32 * size * SNIPPET_MONO_ADVANCE_EM
}

/// Terminal-cell width. Combining marks are counted as one cell (conservative:
/// over-estimating truncates a little sooner rather than wrapping), CJK and
/// emoji as two.
fn mono_cells(ch: char) -> usize {
    if ch == '\n' || ch == '\r' || (ch.is_control() && ch != '\t') {
        0
    } else if ch == '\t' {
        4
    } else if is_wide(ch) {
        2
    } else {
        1
    }
}

fn is_wide(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{115F}'
            | '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE10}'..='\u{FE19}'
            | '\u{FE30}'..='\u{FE6F}'
            | '\u{FF00}'..='\u{FF60}'
            | '\u{FFE0}'..='\u{FFE6}'
            | '\u{1F300}'..='\u{1FAFF}'
            | '\u{20000}'..='\u{3FFFD}'
    )
}

/// Truncates `text` to `max_width` using the same measured Geist width
/// estimate the treemap labels use, so fitting never depends on a character
/// count. Returns an empty string when not even one character fits.
fn fit_text(text: &str, max_width: f32, size: f32) -> String {
    if text.is_empty() || !(size.is_finite() && size > 0.0) || !(max_width.is_finite()) {
        return String::new();
    }
    if treemap::estimate_text_width(text, size) <= max_width {
        return text.to_string();
    }
    let budget = max_width - treemap::estimate_text_width("…", size);
    if budget <= 0.0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0.0f32;
    for ch in text.chars() {
        let mut probe = [0u8; 4];
        let width = treemap::estimate_text_width(ch.encode_utf8(&mut probe), size);
        if used + width > budget {
            break;
        }
        used += width;
        out.push(ch);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}…")
    }
}

fn rect(r: Rect) -> IcedRect {
    IcedRect::new(Point::new(r.x, r.y), Size::new(r.w, r.h))
}

fn rounded_path(r: Rect, radius: f32) -> Path {
    Path::rounded_rectangle(
        Point::new(r.x, r.y),
        Size::new(r.w.max(0.0), r.h.max(0.0)),
        radius.into(),
    )
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn hidden_flags(hidden: &[bool]) -> [bool; 8] {
    let mut out = [false; 8];
    for (i, h) in hidden.iter().take(8).enumerate() {
        out[i] = *h;
    }
    out
}

fn color_components(color: Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

fn build_tiles(
    tree: &Tree,
    dir: NodeId,
    bounds: IcedRect,
    camera: &Camera,
    hidden: &[bool; 8],
    filter: Option<&FilterView>,
) -> Vec<SceneTile> {
    let mut tiles = Vec::new();
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return tiles;
    }
    let area = Rect::new(0.0, 0.0, bounds.width, bounds.height);
    let viewport = bounds.size();
    let hidden = hidden_flags(hidden);
    let view = SceneView {
        camera,
        hidden: &hidden,
        viewport,
    };
    if let Some(filter) = filter {
        build_filtered_scene(tree, &filter.roots, area, 0, view, &mut tiles);
    } else {
        build_scene(tree, dir, area, 0, view, &mut tiles);
    }
    tiles
}

/// Camera, category mask and viewport shared by every level of one scene build.
#[derive(Clone, Copy)]
struct SceneView<'a> {
    camera: &'a Camera,
    hidden: &'a [bool; 8],
    viewport: Size,
}

/// How many nesting depths the camera currently admits. Depth 0 is the root's
/// direct children, so a budget of 1 opens exactly one level at fit.
fn nest_depth_budget(scale: f32) -> u8 {
    let extra = (scale.max(1.0).ln() / NEST_ZOOM_STEP.ln()).floor();
    NEST_BASE_DEPTH.saturating_add((extra.max(0.0) as u32).min(64) as u8)
}

/// Lazily builds the visible tree: a folder is opened exactly when its
/// transformed rect is large enough on screen, and only branches that
/// intersect the viewport are visited. There is no fixed depth limit; zooming
/// in keeps opening levels until the tile budget or the tree ends.
fn build_scene(
    tree: &Tree,
    dir: NodeId,
    area: Rect,
    depth: u8,
    view: SceneView<'_>,
    out: &mut Vec<SceneTile>,
) {
    if out.len() >= MAX_TILES || area.is_empty() {
        return;
    }
    let children = tree.children(dir);
    let mut children: Vec<SceneChild> = children
        .into_iter()
        .filter_map(|id| {
            let node = tree.node(id);
            if view.hidden[node.cat.id() as usize] {
                return None;
            }
            Some(SceneChild {
                id,
                name: tree.name(id).to_string(),
                cat: node.cat,
                size: node.size,
                is_dir: node.is_dir,
            })
        })
        .collect();
    if children.is_empty() {
        return;
    }

    // A pass-through folder chain (`a / b / c`) is drawn as one tile instead
    // of four nested headers stacked inside each other. This is display-only:
    // the deepest folder is the hit-test target, and the tree is untouched.
    for child in &mut children {
        *child = collapse_visible_chain(tree, child.clone(), view.hidden);
    }

    let tiles = layout_scene_children(&mut children, area, depth, view.camera);
    for tile in tiles {
        let idx = children.iter().position(|child| child.id == tile.id);
        let Some(idx) = idx else { continue };
        let child = &children[idx];
        let is_real = child.id != TAIL_ID;
        let node = is_real.then(|| tree.node(child.id));
        let screen = view.camera.transform(tile.rect);
        if !intersects_viewport(screen, view.viewport) {
            continue;
        }
        let can_nest = node.is_some_and(|node| {
            node.is_dir
                && screen.w >= NEST_MIN_W
                && screen.h >= NEST_MIN_H
                && depth < nest_depth_budget(view.camera.scale)
                && node.first_child != NO_NODE
        });
        let empty_dir = node.is_some_and(|node| {
            node.is_dir && node.first_child == NO_NODE && screen.area() > 1_200.0
        });
        let strip = if can_nest { header_px(depth) } else { 0.0 };
        out.push(make_tile(child, screen, depth, can_nest, empty_dir, strip));
        if can_nest {
            let inner = child_area(tile.rect, strip, view.camera.scale);
            build_scene(tree, child.id, inner, depth.saturating_add(1), view, out);
        }
    }
}

fn header_px(depth: u8) -> f32 {
    if depth == 0 {
        ROOT_HEADER_PX
    } else {
        HEADER_PX
    }
}

/// The content rect available below a container header, inset in *screen*
/// pixels so the strip and its border stay constant at every zoom level.
fn child_area(rect: Rect, header_px: f32, scale: f32) -> Rect {
    let scale = if scale.is_finite() && scale >= 1.0 {
        scale
    } else {
        1.0
    };
    let inset = NEST_INSET_PX / scale;
    Rect {
        x: rect.x + inset,
        y: rect.y + header_px / scale,
        w: (rect.w - inset * 2.0).max(0.0),
        h: (rect.h - header_px / scale - inset).max(0.0),
    }
}

fn intersects_viewport(rect: Rect, viewport: Size) -> bool {
    rect.w > 0.0
        && rect.h > 0.0
        && rect.right() >= -CULL_MARGIN_PX
        && rect.bottom() >= -CULL_MARGIN_PX
        && rect.x <= viewport.width + CULL_MARGIN_PX
        && rect.y <= viewport.height + CULL_MARGIN_PX
}

/// A filtered scene keeps the real hierarchy: matching entries appear below
/// their real ancestor folders, but only branches that contain a match are
/// shown. Container tiles report the sum of the matching bytes beneath them.
fn build_filtered_scene(
    tree: &Tree,
    nodes: &[FilterNode],
    area: Rect,
    depth: u8,
    view: SceneView<'_>,
    out: &mut Vec<SceneTile>,
) {
    if out.len() >= MAX_TILES || area.is_empty() || nodes.is_empty() {
        return;
    }

    let mut children: Vec<SceneChild> = Vec::new();
    let mut descendants: Vec<(NodeId, Option<&[FilterNode]>)> = Vec::new();
    for node in nodes {
        let (child, nested) = filtered_scene_child(tree, node);
        descendants.push((child.id, nested));
        children.push(child);
    }

    let tiles = layout_scene_children(&mut children, area, depth, view.camera);
    for tile in tiles {
        let Some(idx) = children.iter().position(|child| child.id == tile.id) else {
            continue;
        };
        let child = &children[idx];
        let nested = descendants
            .iter()
            .find(|(id, _)| *id == child.id)
            .and_then(|(_, nested)| *nested)
            .filter(|children| !children.is_empty());
        let screen = view.camera.transform(tile.rect);
        if !intersects_viewport(screen, view.viewport) {
            continue;
        }
        let can_nest = nested.is_some()
            && screen.w >= NEST_MIN_W
            && screen.h >= NEST_MIN_H
            && depth < nest_depth_budget(view.camera.scale);
        let strip = if can_nest { header_px(depth) } else { 0.0 };
        out.push(make_tile(child, screen, depth, can_nest, false, strip));
        if can_nest {
            let inner = child_area(tile.rect, strip, view.camera.scale);
            build_filtered_scene(
                tree,
                nested.unwrap_or_default(),
                inner,
                depth.saturating_add(1),
                view,
                out,
            );
        }
    }
}

/// Follow a directory chain while each step has exactly one visible child, and
/// that child is a directory carrying the entire byte count. Files, hidden
/// branches and folders with their own direct bytes stop the collapse.
fn collapse_visible_chain(tree: &Tree, mut child: SceneChild, hidden: &[bool; 8]) -> SceneChild {
    if !child.is_dir {
        return child;
    }
    loop {
        let id = child.id;
        let node = tree.node(id);
        if !node.is_dir || node.first_child == NO_NODE {
            break;
        }
        let mut visible = tree
            .children(id)
            .into_iter()
            .filter(|next| !hidden[tree.node(*next).cat.id() as usize]);
        let Some(next) = visible.next() else {
            break;
        };
        if visible.next().is_some() {
            break;
        }
        let next_node = tree.node(next);
        // A single child that is a file does not turn a folder into a file
        // tile, and a child smaller than its folder means the folder has its
        // own direct bytes that must stay visible.
        if !next_node.is_dir || next_node.size != node.size {
            break;
        }
        child.id = next;
        child.name.push_str(" / ");
        child.name.push_str(tree.name(next));
        child.cat = next_node.cat;
        child.size = next_node.size;
    }
    child
}

/// Filtered scenes keep real containers, but a container whose only filtered
/// child is another container can collapse the same way. Matching folders are
/// leaves and are never collapsed into a different node.
fn collapse_filter_chain<'a>(tree: &Tree, node: &'a FilterNode) -> (&'a FilterNode, String) {
    let mut deepest = node;
    let mut name = tree.name(node.id).to_string();
    while !deepest.is_match && deepest.children.len() == 1 {
        let next = &deepest.children[0];
        if !next.is_dir {
            break;
        }
        name.push_str(" / ");
        name.push_str(tree.name(next.id));
        deepest = next;
    }
    (deepest, name)
}

fn filtered_scene_child<'a>(
    tree: &Tree,
    node: &'a FilterNode,
) -> (SceneChild, Option<&'a [FilterNode]>) {
    let (deepest, name) = collapse_filter_chain(tree, node);
    let child = SceneChild {
        id: deepest.id,
        name,
        cat: deepest.cat,
        size: deepest.size,
        is_dir: deepest.is_dir,
    };
    let nested = (!deepest.is_match).then_some(deepest.children.as_slice());
    (child, nested)
}

/// Keep at most 63 readable children and one proportional remainder. The
/// cutoff uses the actual transformed rectangles, including skinny strips,
/// rather than file count alone. Every retry strictly reduces `keep`, so this
/// is bounded by 64 layouts; the full tail is summed only once.
fn layout_scene_children(
    children: &mut Vec<SceneChild>,
    area: Rect,
    depth: u8,
    camera: &Camera,
) -> Vec<treemap::Tile> {
    children.retain(|child| child.size > 0);
    children.sort_by_key(|child| std::cmp::Reverse(child.size));
    let count = children.len();
    let mut keep = count.min(MAX_REAL_CHILDREN);
    let mut remainder: u64 = children[keep..].iter().map(|child| child.size).sum();
    children.truncate(keep);
    let gutter_px = if depth == 0 {
        ROOT_GUTTER_PX
    } else {
        GUTTER_PX
    };
    let scale = if camera.scale.is_finite() && camera.scale >= 1.0 {
        camera.scale
    } else {
        1.0
    };
    let gutter = gutter_px / scale;
    loop {
        if keep < count {
            let noun = if count - keep == 1 {
                "smaller item"
            } else {
                "smaller items"
            };
            children.push(SceneChild {
                id: TAIL_ID,
                name: format!("{} {noun}", fmt::count((count - keep) as u64)),
                cat: Category::Other,
                size: remainder,
                is_dir: false,
            });
        }
        let items: Vec<Item> = children
            .iter()
            .map(|child| Item {
                id: child.id,
                size: child.size,
                cat: child.cat,
                is_dir: child.is_dir,
            })
            .collect();
        let tiles = treemap::squarify(area, &items, gutter);
        let cutoff = tiles
            .iter()
            .filter(|tile| tile.id != TAIL_ID)
            .filter(|tile| {
                let screen = camera.transform(tile.rect);
                treemap::tile_tier(screen.w, screen.h) == TileTier::Tiny
            })
            .filter_map(|tile| children.iter().position(|child| child.id == tile.id))
            .min();
        let Some(cutoff) = cutoff else { return tiles };
        remainder += children[cutoff..keep]
            .iter()
            .map(|child| child.size)
            .sum::<u64>();
        children.truncate(cutoff);
        keep = cutoff;
    }
}

fn make_tile(
    child: &SceneChild,
    screen: Rect,
    depth: u8,
    can_nest: bool,
    empty_dir: bool,
    header_px: f32,
) -> SceneTile {
    let aggregate = child.id == TAIL_ID;
    SceneTile {
        node: if aggregate { NO_NODE } else { child.id },
        name: child.name.clone(),
        cat: child.cat,
        size: child.size,
        depth,
        rect: screen,
        header_h: header_px,
        has_children: can_nest,
        is_dir: child.is_dir,
        empty_dir,
        aggregate,
    }
}

/// Scene tiles for the app's tile planner: the same list the GPU fills and
/// hit-testing use, so preview requests always match drawn geometry.
pub(crate) fn scene_tiles(
    tree: &Tree,
    dir: NodeId,
    bounds: Size,
    camera: &Camera,
    hidden: &[bool; 8],
    filter: Option<&FilterView>,
) -> Vec<SceneTile> {
    build_tiles(
        tree,
        dir,
        IcedRect::new(Point::ORIGIN, bounds),
        camera,
        hidden,
        filter,
    )
}

fn hit(tiles: &[SceneTile], local: Point) -> Option<&SceneTile> {
    tiles
        .iter()
        .rev()
        .find(|t| t.rect.contains(local.x, local.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CategoryTotals, ScanSummary, TreeBuilder};

    fn fixture() -> TreemapCanvas {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        let file = builder.add_node("payload.bin", 0, false, 1024, Category::Other);
        let tree = builder.finish(vec![file], CategoryTotals::default());
        let summary = ScanSummary {
            root_path: "C:\\fixture".into(),
            total_bytes: 1024,
            files: 1,
            dirs: 0,
            errors: 0,
            elapsed_ms: 1,
            categories: CategoryTotals::default(),
        };
        TreemapCanvas {
            output: Arc::new(ScanOutput { tree, summary }),
            dir: 0,
            hidden: [false; 8],
            query: String::new(),
            filter: None,
            camera: Camera::default(),
            selected: None,
            palette: CanvasPalette {
                bed: Color::WHITE,
                ink: Color::BLACK,
                ink_muted: Color::BLACK,
                ink_faint: Color::BLACK,
                hairline: Color::BLACK,
                accent: Color::BLACK,
                category_fill: [Color::WHITE; 8],
                category_ink: [Color::BLACK; 8],
                category_line: [Color::BLACK; 8],
            },
            empty_hint: None,
            fills: false,
            art: Arc::new(std::collections::HashMap::new()),
            icons: Arc::new(IconRaster::new()),
            report: Arc::new(Mutex::new(None)),
        }
    }

    /// Iced delivers pointer events to a Canvas even when the pointer is
    /// outside its layout bounds. The canvas must ignore those presses,
    /// releases and wheels; otherwise it steals clicks from the legend,
    /// toolbar and inspector, pans by accident and clears selections.
    #[test]
    fn pointer_events_outside_canvas_bounds_are_ignored() {
        let canvas = fixture();
        let bounds = IcedRect::new(Point::new(100.0, 200.0), Size::new(400.0, 300.0));
        let outside = Cursor::Available(Point::new(10.0, 20.0));
        let inside = Cursor::Available(Point::new(150.0, 250.0));
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });

        let mut outside_state = CanvasState::default();
        assert!(
            canvas::Program::update(&canvas, &mut outside_state, &press, bounds, outside).is_none()
        );
        assert!(
            canvas::Program::update(&canvas, &mut outside_state, &release, bounds, outside)
                .is_none()
        );
        assert!(
            canvas::Program::update(&canvas, &mut outside_state, &wheel, bounds, outside).is_none()
        );

        let mut inside_state = CanvasState::default();
        assert!(
            canvas::Program::update(&canvas, &mut inside_state, &press, bounds, inside).is_some()
        );
        assert!(
            canvas::Program::update(&canvas, &mut inside_state, &wheel, bounds, inside).is_some()
        );
    }

    #[test]
    fn snippet_rows_fit_the_body_and_expand_tabs() {
        let body = Rect::new(10.0, 20.0, 118.0, 64.0);
        let lines = vec![
            "a\tb\tc".to_string(),
            "let this_line_is_far_too_long_for_the_tile_to_show_without_wrapping = 1;".to_string(),
            "emoji 😀 and 文档 width".to_string(),
        ];
        let layout = layout_snippet(&lines, body);

        assert!(!layout.rows.is_empty());
        for row in &layout.rows {
            assert!(!row.contains('\t'), "tabs must be expanded before drawing");
            assert!(!row.contains('\n') && !row.contains('\r'));
            assert!(
                mono_text_width(row, SNIPPET_LINE_PT) <= body.w + 0.01,
                "row {:?} is wider than the body: {} > {}",
                row,
                mono_text_width(row, SNIPPET_LINE_PT),
                body.w
            );
        }
        assert!(layout.rows[0].starts_with("a   b"));
        assert!(
            layout.rows[1].ends_with('…'),
            "long logical lines are elided, never wrapped"
        );
    }

    #[test]
    fn snippet_layout_reserves_continuation_only_when_lines_continue() {
        let body = Rect::new(0.0, 0.0, 200.0, 60.0);
        let layout = layout_snippet(
            &(0..20).map(|i| format!("line {i}")).collect::<Vec<_>>(),
            body,
        );
        assert_eq!(layout.rows.len(), 3);
        assert!(layout.has_more);
        assert!(layout.continuation_h > 0.0);
        assert!(layout.clip_h < body.h);
        let last_bottom =
            layout.rows.len() as f32 * SNIPPET_LINE_H + (SNIPPET_LINE_PT - SNIPPET_LINE_H);
        assert!(
            last_bottom <= layout.clip_h + 1.0,
            "painted rows must stay inside the reserved clip"
        );

        let short = layout_snippet(&["one".into(), "two".into()], body);
        assert!(!short.has_more);
        assert_eq!(short.continuation_h, 0.0);
        assert_eq!(short.clip_h, body.h);
        assert_eq!(short.rows.len(), 2);
    }

    #[test]
    fn snippet_layout_never_draws_more_rows_than_the_body_allows() {
        let body = Rect::new(0.0, 0.0, 80.0, 31.0);
        let layout = layout_snippet(
            &(0..40).map(|i| format!("row {i}")).collect::<Vec<_>>(),
            body,
        );
        assert_eq!(layout.rows.len(), 1);
        assert!(
            layout.rows.len() as f32 * SNIPPET_LINE_H <= body.h,
            "row bottoms must remain inside the body"
        );
    }

    #[test]
    fn folder_summary_bands_are_disjoint_and_the_bar_stays_compact() {
        let body = Rect::new(4.0, 6.0, 320.0, 140.0);
        let layout = folder_summary_layout(body);
        let bar = layout.bar.expect("a tall body gets a composition bar");
        assert!((10.0..=14.0).contains(&bar.h), "bar height {}", bar.h);
        assert!(layout.stats.bottom() <= layout.rows[0].y);
        for pair in layout.rows.windows(2) {
            assert!(pair[0].bottom() <= pair[1].y);
        }
        let last = layout.rows.last().expect("three rows").bottom();
        assert!(last + 2.0 <= bar.y, "rows must clear the bar");
        assert!(bar.bottom() <= body.bottom());

        let short = folder_summary_layout(Rect::new(0.0, 0.0, 200.0, 28.0));
        assert!(short.bar.is_none(), "too short for stats + bar");
        assert!(short.rows.is_empty());
    }

    #[test]
    fn folder_summary_rows_show_the_three_largest_categories() {
        let mut totals = CategoryTotals::default();
        totals.add(Category::Code, 900);
        totals.add(Category::Media, 500);
        totals.add(Category::Documents, 100);
        totals.add(Category::Other, 50);
        let top = top_categories(&totals);
        assert_eq!(
            top.iter().map(|(cat, _)| *cat).collect::<Vec<_>>(),
            vec![Category::Code, Category::Media, Category::Documents]
        );
        assert_eq!(top[0].1, 900);
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn compact_art_needs_a_90px_square_and_stays_below_large() {
        assert!(compact_art_eligible(90.0, 90.0));
        assert!(compact_art_eligible(150.0, 100.0), "medium bodies qualify");
        assert!(
            compact_art_eligible(100.0, 400.0),
            "tall small bodies qualify"
        );
        assert!(
            !compact_art_eligible(200.0, 130.0),
            "large tiles use draw_large"
        );
        assert!(!compact_art_eligible(89.9, 120.0));
        assert!(!compact_art_eligible(120.0, 89.9));
        assert!(!compact_art_eligible(f32::NAN, 120.0));
        assert!(!compact_art_eligible(120.0, f32::INFINITY));
    }

    #[test]
    fn compact_art_bands_are_disjoint_and_inside_the_tile() {
        for rect in [
            Rect::new(10.0, 20.0, 90.0, 90.0),
            Rect::new(0.0, 0.0, 150.0, 100.0),
            Rect::new(-12.0, 8.0, 220.0, 95.0),
        ] {
            let layout = compact_art_layout(rect);
            assert!(layout.art.w > 0.0 && layout.art.h >= 28.0);
            assert!(
                layout.art.bottom() + 1.0 <= layout.name.y,
                "a visible gap keeps art and name apart"
            );
            assert!(layout.name.h >= 16.0);
            assert!(layout.name.bottom() <= rect.bottom());
            assert!(layout.art.x >= rect.x && layout.art.right() <= rect.right());
            assert_eq!(layout.name.w, layout.art.w);
        }

        let degenerate = compact_art_layout(Rect::new(0.0, 0.0, 40.0, 10.0));
        assert!(degenerate.name.bottom() <= 10.0);
        assert!(degenerate.art.h >= 0.0);
    }

    fn chain_tree(outer_direct: u64) -> (Tree, NodeId, NodeId) {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        let outer = builder.add_node("outer", 0, true, 0, Category::Other);
        if outer_direct > 0 {
            builder.add_node("direct.bin", outer, false, outer_direct, Category::Other);
        }
        let inner = builder.add_node("inner", outer, true, 0, Category::Other);
        builder.add_node("payload.bin", inner, false, 1_000, Category::Other);
        builder.add_node("sibling.bin", 0, false, 500, Category::Other);
        let tree = builder.finish(vec![], CategoryTotals::default());
        (tree, outer, inner)
    }

    fn scene(tree: &Tree) -> Vec<SceneTile> {
        build_tiles(
            tree,
            0,
            IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
            &Camera::default(),
            &[false; 8],
            None,
        )
    }

    #[test]
    fn sixty_three_real_tiles_plus_one_non_selectable_remainder_is_a_hard_cap() {
        for count in [63, 64, 65, 100] {
            let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
            for i in 0..count {
                builder.add_node(
                    &format!("file-{i:03}.txt"),
                    0,
                    false,
                    1_000,
                    Category::Documents,
                );
            }
            let tree = builder.finish(vec![], CategoryTotals::default());
            let tiles = build_tiles(
                &tree,
                0,
                IcedRect::new(Point::ORIGIN, Size::new(1600.0, 1200.0)),
                &Camera::default(),
                &[false; 8],
                None,
            );
            assert_eq!(
                tiles.iter().filter(|tile| !tile.aggregate).count(),
                count.min(63)
            );
            assert_eq!(
                tiles.iter().map(|tile| tile.size).sum::<u64>(),
                count as u64 * 1_000
            );
            let tails: Vec<_> = tiles.iter().filter(|tile| tile.aggregate).collect();
            assert_eq!(tails.len(), usize::from(count > 63));
            if let Some(tail) = tails.first() {
                assert_eq!(tail.node, NO_NODE);
                assert!(!tail.is_dir && !tail.has_children);
                assert_eq!(tail.size, (count as u64 - 63) * 1_000);
                let center = Point::new(
                    tail.rect.x + tail.rect.w * 0.5,
                    tail.rect.y + tail.rect.h * 0.5,
                );
                assert_eq!(hit(&tiles, center).unwrap().node, NO_NODE);
            }
        }
    }

    #[test]
    fn tiny_items_merge_below_the_count_cap_without_losing_bytes() {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        for size in [4_000, 3_000, 2_000] {
            builder.add_node(
                &format!("large-{size}.txt"),
                0,
                false,
                size,
                Category::Documents,
            );
        }
        for i in 0..20 {
            builder.add_node(&format!("small-{i}.rs"), 0, false, 10, Category::Code);
        }
        builder.add_node("empty.txt", 0, false, 0, Category::Documents);
        let tree = builder.finish(vec![], CategoryTotals::default());
        let tiles = scene(&tree);
        assert_eq!(tiles.len(), 4);
        let tail = tiles.iter().find(|tile| tile.aggregate).unwrap();
        assert_eq!(tail.name, "20 smaller items");
        assert_eq!(tail.size, 200);
        assert_eq!(tiles.iter().map(|tile| tile.size).sum::<u64>(), 9_200);
        assert!(
            tiles
                .iter()
                .filter(|tile| !tile.aggregate)
                .all(|tile| treemap::tile_tier(tile.rect.w, tile.rect.h) != TileTier::Tiny)
        );

        let filter = FilterView::build(&tree, 0, "small-", &[false; 8]).unwrap();
        let filtered = build_tiles(
            &tree,
            0,
            IcedRect::new(Point::ORIGIN, Size::new(160.0, 100.0)),
            &Camera::default(),
            &[false; 8],
            Some(&filter),
        );
        assert_eq!(filtered.iter().filter(|tile| tile.aggregate).count(), 1);
        assert_eq!(
            filtered.iter().map(|tile| tile.size).sum::<u64>(),
            filter.matched_bytes
        );

        let mut hidden = [false; 8];
        hidden[Category::Code.id() as usize] = true;
        let visible = build_tiles(
            &tree,
            0,
            IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
            &Camera::default(),
            &hidden,
            None,
        );
        assert_eq!(visible.iter().map(|tile| tile.size).sum::<u64>(), 9_000);
        assert!(!visible.iter().any(|tile| tile.aggregate));
    }

    #[test]
    fn map_and_filter_open_folders_by_screen_size_and_drill_reveals_children() {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        let outer = builder.add_node("outer", 0, true, 0, Category::Other);
        let mut groups = Vec::new();
        for group_name in ["first", "second"] {
            let group = builder.add_node(group_name, outer, true, 0, Category::Other);
            groups.push(group);
            builder.add_node("needle-a.txt", group, false, 2_000, Category::Documents);
            builder.add_node("needle-b.png", group, false, 1_000, Category::Media);
        }
        let tree = builder.finish(vec![], CategoryTotals::default());
        let filter = FilterView::build(&tree, 0, "needle", &[false; 8]).unwrap();
        let viewport = IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let camera_viewport = Rect::new(0.0, 0.0, 800.0, 600.0);
        for view in [None, Some(&filter)] {
            let fit = build_tiles(&tree, 0, viewport, &Camera::default(), &[false; 8], view);
            for group in &groups {
                let summary = fit.iter().find(|tile| tile.node == *group).unwrap();
                assert!(summary.is_dir && !summary.has_children);
                assert_eq!(summary.size, 3_000);
            }
            assert!(!fit.iter().any(|tile| tile.name.starts_with("needle-")));

            // Three 1.25x steps admit the next levels, and the screen-size
            // gate opens both groups into their files without a drill action.
            let mut camera = Camera::default();
            for _ in 0..3 {
                camera.zoom_at(1.25, (400.0, 300.0), camera_viewport, treemap::MAX_SCALE);
            }
            let zoomed = build_tiles(&tree, 0, viewport, &camera, &[false; 8], view);
            for group in &groups {
                let summary = zoomed.iter().find(|tile| tile.node == *group).unwrap();
                assert!(summary.is_dir && summary.has_children);
                assert_eq!(summary.size, 3_000);
            }
            assert_eq!(
                zoomed
                    .iter()
                    .filter(|tile| tile.name.starts_with("needle-"))
                    .count(),
                4,
                "folder contents open by screen size and camera zoom, not by depth"
            );
        }
        let drilled = build_tiles(
            &tree,
            groups[0],
            IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
            &Camera::default(),
            &[false; 8],
            None,
        );
        assert_eq!(drilled.len(), 2);
        assert!(
            drilled
                .iter()
                .all(|tile| tile.depth == 0 && !tile.is_dir && !tile.aggregate)
        );
    }

    #[test]
    fn nesting_budget_tracks_zoom_without_an_absolute_cap() {
        assert_eq!(nest_depth_budget(1.0), 1);
        assert_eq!(nest_depth_budget(1.25), 2);
        assert!(nest_depth_budget(2.0) >= 3);
        let deepest = nest_depth_budget(treemap::MAX_SCALE);
        assert!(
            deepest >= 30,
            "maximum zoom must admit arbitrarily deep scenes, got {deepest}"
        );
        assert!(deepest <= 65);
    }

    #[test]
    fn camera_opens_one_level_at_fit_then_many_as_it_zooms() {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        let mut parent = 0;
        for level in 0..12 {
            if level > 0 {
                builder.add_node(
                    &format!("direct-{level}.bin"),
                    parent,
                    false,
                    1_000,
                    Category::Other,
                );
            }
            parent = builder.add_node(&format!("level-{level}"), parent, true, 0, Category::Other);
        }
        builder.add_node("deep.bin", parent, false, 10_000, Category::Other);
        let tree = builder.finish(vec![], CategoryTotals::default());
        let viewport = IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let at_fit = build_tiles(&tree, 0, viewport, &Camera::default(), &[false; 8], None);
        assert!(
            at_fit.iter().all(|tile| tile.depth <= 1),
            "fit must show one level, not the whole chain"
        );
        assert!(!at_fit.iter().any(|tile| tile.name == "deep.bin"));

        // Fifteen 1.25x steps admit one more level each (with one at fit), so
        // the same camera transform reaches the deepest file with no fixed
        // depth cap.
        let mut camera = Camera::default();
        let camera_viewport = Rect::new(0.0, 0.0, 800.0, 600.0);
        for _ in 0..15 {
            camera.zoom_at(1.25, (400.0, 300.0), camera_viewport, treemap::MAX_SCALE);
        }
        let zoomed = build_tiles(&tree, 0, viewport, &camera, &[false; 8], None);
        let deep = zoomed
            .iter()
            .find(|tile| tile.name == "deep.bin")
            .expect("zooming admits the deep file");
        assert!(
            deep.depth >= 10,
            "no fixed depth cap: deepest tile was at depth {}",
            deep.depth
        );
    }

    #[test]
    fn zoom_reveals_small_items_using_the_same_scene_as_hit_testing() {
        let mut builder = TreeBuilder::new("C:\\fixture", "fixture");
        builder.add_node("large.txt", 0, false, 100_000, Category::Documents);
        for i in 0..20 {
            builder.add_node(
                &format!("small-{i}.txt"),
                0,
                false,
                100,
                Category::Documents,
            );
        }
        let tree = builder.finish(vec![], CategoryTotals::default());
        let viewport = IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let camera_viewport = Rect::new(0.0, 0.0, 800.0, 600.0);
        let at_fit = build_tiles(&tree, 0, viewport, &Camera::default(), &[false; 8], None);
        let tail = at_fit
            .iter()
            .find(|tile| tile.aggregate)
            .expect("small files start in the proportional tail");
        let anchor = (
            tail.rect.x + tail.rect.w * 0.5,
            tail.rect.y + tail.rect.h * 0.5,
        );

        // Wheel steps toward the tail keep that content point fixed while the
        // camera opens the tiny files into real, hit-testable tiles. Viewport
        // culling means the whole-drive total is intentionally not summed here.
        let mut camera = Camera::default();
        for _ in 0..4 {
            camera.zoom_at(2.0, anchor, camera_viewport, treemap::MAX_SCALE);
        }
        let zoomed = build_tiles(&tree, 0, viewport, &camera, &[false; 8], None);
        assert!(
            zoomed.iter().filter(|tile| !tile.aggregate).count()
                > at_fit.iter().filter(|tile| !tile.aggregate).count(),
            "zooming toward the tail must open the small files"
        );
        assert!(zoomed.iter().all(|tile| !tile.aggregate));
        assert!(
            zoomed
                .iter()
                .filter(|tile| tile.name.starts_with("small-"))
                .all(|tile| tile.size == 100)
        );
        assert!(zoomed.iter().any(|tile| tile.name == "large.txt"));
        assert!(zoomed.iter().all(|tile| tile.depth <= 1));
    }

    #[test]
    fn hatching_is_parallel_clipped_and_bounded_at_maximum_zoom() {
        let viewport = Size::new(800.0, 600.0);
        for rect in [
            Rect::new(20.0, 30.0, 180.0, 110.0),
            Rect::new(-50_000.0, -50_000.0, 100_000.0, 100_000.0),
        ] {
            let lines = hatch_segments(rect, viewport);
            assert!(!lines.is_empty() && lines.len() <= 140);
            for (a, b) in lines {
                assert!((a.x + a.y - b.x - b.y).abs() < 0.01);
                for point in [a, b] {
                    assert!((0.0..=viewport.width).contains(&point.x));
                    assert!((0.0..=viewport.height).contains(&point.y));
                    assert!(point.x >= rect.x && point.x <= rect.right());
                    assert!(point.y >= rect.y && point.y <= rect.bottom());
                }
            }
        }
        assert!(hatch_segments(Rect::new(900.0, 0.0, 100.0, 100.0), viewport).is_empty());
    }

    #[test]
    fn single_child_directory_chains_collapse_to_the_deepest_folder() {
        let (tree, outer, inner) = chain_tree(0);
        let tiles = scene(&tree);
        let tile = tiles
            .iter()
            .find(|tile| tile.node == inner)
            .expect("collapsed chain tile");
        assert_eq!(tile.name, "outer / inner");
        assert!(tile.is_dir);
        assert_eq!(tile.size, tree.node(inner).size);
        assert!(
            tiles.iter().all(|tile| tile.node != outer),
            "the pass-through parent must not survive as a separate tile"
        );
    }

    #[test]
    fn a_folder_with_its_own_bytes_is_not_collapsed() {
        let (tree, outer, _) = chain_tree(400);
        let tiles = scene(&tree);
        let tile = tiles
            .iter()
            .find(|tile| tile.node == outer)
            .expect("outer tile");
        assert_eq!(tile.name, "outer");
        assert_eq!(tile.size, 1_400);
    }

    #[test]
    fn filtered_chain_collapses_containers_but_not_the_match() {
        let (tree, _, inner) = chain_tree(0);
        let filter = FilterView::build(&tree, 0, "payload", &[false; 8]).expect("filter view");
        let tiles = build_tiles(
            &tree,
            0,
            IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
            &Camera::default(),
            &[false; 8],
            Some(&filter),
        );
        let container = tiles
            .iter()
            .find(|tile| tile.node == inner)
            .expect("collapsed filtered container");
        assert_eq!(container.name, "outer / inner");
        assert!(container.has_children);
        assert!(
            tiles.iter().any(|tile| tile.name == "payload.bin"),
            "the match remains its own tile below the collapsed chain"
        );

        let matching_folder =
            FilterView::build(&tree, 0, "inner", &[false; 8]).expect("folder match view");
        let tiles = build_tiles(
            &tree,
            0,
            IcedRect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
            &Camera::default(),
            &[false; 8],
            Some(&matching_folder),
        );
        let matched = tiles
            .iter()
            .find(|tile| tile.node == inner)
            .expect("matched folder tile");
        assert_eq!(matched.name, "outer / inner");
        assert!(!matched.has_children, "a matched folder stays a leaf");
    }
}
