//! Interactive treemap canvas. Geometry is computed from real scan data; labels
//! are measured/clipped per tile; every hit maps to the pixels actually drawn.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Text};
use iced::{Color, Font, Point, Rectangle as IcedRect, Renderer, Size, Vector};

use crate::model::{Category, NO_NODE, NodeId, ScanOutput, Tree};
use crate::render::{DrawList, DrawTile, TreemapPrimitive, TreemapState};
use crate::treemap::{self, Camera, Item, Rect};
use crate::ui::filter::{FilterNode, FilterView};
use crate::ui::fmt;
use iced::widget::shader;

use super::Message;

const MAX_DEPTH: u8 = 4;
const MAX_TILES: usize = 6000;
/// Above this many visible children a folder is drawn as its largest members
/// plus one honest tail tile. The tail is labelled with its count and bytes and
/// is not selectable; it exists only because sub-pixel tiles communicate
/// nothing.
const MAX_CHILDREN: usize = 64;
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
struct SceneTile {
    node: NodeId,
    name: String,
    cat: Category,
    size: u64,
    depth: u8,
    /// Screen-space rect after the camera transform.
    rect: Rect,
    header_h: f32,
    has_children: bool,
    empty_dir: bool,
    aggregate: bool,
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

            if tile.aggregate {
                // A quiet diagonal weave marks a summary tile that holds many
                // sub-pixel items. It is a total, not a file.
                let step = 11.0;
                let line = mix(self.palette.hairline, fill, 0.55);
                let mut x = tile.rect.x - tile.rect.h;
                while x < tile.rect.right() {
                    let x1 = x.max(tile.rect.x);
                    let x2 = (x + tile.rect.h).min(tile.rect.right());
                    let y1 = tile.rect.bottom() - (x1 - tile.rect.x);
                    let y2 = tile.rect.y + (x2 - tile.rect.x);
                    frame.with_clip(rect(tile.rect), |frame| {
                        frame.stroke(
                            &Path::line(Point::new(x1, y1), Point::new(x2, y2)),
                            canvas::Stroke::default().with_color(line).with_width(0.6),
                        );
                    });
                    x += step;
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
                        .with_color(self.palette.category_line[tile.cat.id() as usize])
                        .with_width(if tile.depth == 0 { 1.4 } else { 1.0 }),
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

            if tile.empty_dir {
                // A quiet hatch communicates an empty interior without a plate.
                let hatch = mix(self.palette.ink_faint, fill, 0.55);
                let step = 11.0;
                let mut x = tile.rect.x - tile.rect.h;
                while x < tile.rect.right() {
                    let x1 = x.max(tile.rect.x);
                    let x2 = (x + tile.rect.h).min(tile.rect.right());
                    let y1 = tile.rect.bottom() - (x1 - tile.rect.x);
                    let y2 = tile.rect.y + (x2 - tile.rect.x);
                    frame.stroke(
                        &Path::line(Point::new(x1, y1), Point::new(x2, y2)),
                        canvas::Stroke::default().with_color(hatch).with_width(0.7),
                    );
                    x += step;
                }
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

            self.draw_label(&mut frame, tile);
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
    fn draw_label(&self, frame: &mut Frame, tile: &SceneTile) {
        let r = tile.rect;
        if r.w < 36.0 || r.h < 15.0 {
            return;
        }
        let pad = if tile.depth == 0 { 8.0 } else { 6.0 };
        let size = if r.h < 21.0 {
            10.0
        } else if tile.has_children {
            if tile.depth == 0 { 12.5 } else { 11.5 }
        } else if tile.depth == 0 {
            13.0
        } else {
            11.5
        };
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
        let size_width = size_text.chars().count() as f32 * 6.1;
        let header_size = (tile.has_children || tile.aggregate)
            && label_rect.h >= 22.0
            && label_rect.w > 142.0
            && size_width + 30.0 < max_width;
        let leaf_size = !tile.has_children
            && !tile.aggregate
            && tile.rect.h > 54.0
            && tile.rect.w > 92.0
            && size_width + pad * 2.0 < label_rect.w;
        let name_width = (max_width - if header_size { size_width + 12.0 } else { 0.0 }).max(22.0);
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
            frame.fill_text(Text {
                content: name,
                position: Point::new(label_rect.x + pad, label_rect.y + pad - 1.0),
                color,
                size: size.into(),
                font: ui_font(weight),
                max_width: name_width,
                ..Text::default()
            });
            if header_size {
                frame.fill_text(Text {
                    content: size_text,
                    position: Point::new(label_rect.right() - pad, label_rect.y + pad + 1.0),
                    color,
                    size: 11.0.into(),
                    font: mono_font(iced::font::Weight::Normal),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    ..Text::default()
                });
            }
            if leaf_size {
                frame.fill_text(Text {
                    content: fmt::bytes_short(tile.size),
                    position: Point::new(label_rect.x + pad, label_rect.bottom() - pad - 12.0),
                    color,
                    size: 11.0.into(),
                    font: mono_font(iced::font::Weight::Normal),
                    ..Text::default()
                });
            }
        });
    }
}

fn ui_font(weight: iced::font::Weight) -> Font {
    Font {
        family: iced::font::Family::Name("Geist"),
        weight,
        ..Font::DEFAULT
    }
}

fn mono_font(weight: iced::font::Weight) -> Font {
    Font {
        family: iced::font::Family::Name("Geist Mono"),
        weight,
        ..Font::DEFAULT
    }
}

fn fit_text(text: &str, max_width: f32, size: f32) -> String {
    let per_char = size * 0.56;
    let capacity = (max_width / per_char).floor().max(0.0) as usize;
    let count = text.chars().count();
    if count <= capacity {
        return text.to_string();
    }
    if capacity <= 1 {
        return "…".into();
    }
    let mut out: String = text.chars().take(capacity - 1).collect();
    out.push('…');
    out
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
    if let Some(view) = filter {
        build_filtered_scene(tree, &view.roots, area, 0, camera, &mut tiles);
    } else {
        build_scene(tree, dir, area, 0, camera, hidden, &mut tiles);
    }
    tiles
}

fn build_scene(
    tree: &Tree,
    dir: NodeId,
    area: Rect,
    depth: u8,
    camera: &Camera,
    hidden: &[bool],
    out: &mut Vec<SceneTile>,
) {
    if depth > MAX_DEPTH || out.len() > MAX_TILES || area.is_empty() {
        return;
    }
    let hidden = hidden_flags(hidden);
    let children = tree.children(dir);
    let mut children: Vec<SceneChild> = children
        .into_iter()
        .filter_map(|id| {
            let node = tree.node(id);
            if hidden[node.cat.id() as usize] {
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

    if children.len() > MAX_CHILDREN {
        let tail = children.split_off(MAX_CHILDREN - 1);
        children.push(tail_tile(&tail, "smaller items"));
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
    let gutter = if depth == 0 { 2.4 } else { 1.6 };
    let tiles = treemap::squarify(area, &items, gutter);
    for tile in tiles {
        let idx = items.iter().position(|i| i.id == tile.id);
        let Some(idx) = idx else { continue };
        let child = &children[idx];
        let is_real = child.id != TAIL_ID;
        let node = is_real.then(|| tree.node(child.id));
        let screen_area = tile.rect.area() * camera.scale * camera.scale;
        let can_nest = node.is_some_and(|node| {
            node.is_dir
                && depth < MAX_DEPTH
                && screen_area > if depth == 0 { 9_000.0 } else { 16_000.0 }
                && tile.rect.h > 66.0
                && tile.rect.w > 72.0
                && node.first_child != NO_NODE
        });
        let empty_dir = node.is_some_and(|node| {
            node.is_dir && node.first_child == NO_NODE && tile.rect.area() > 1_200.0
        });
        let header_h = if can_nest {
            if depth == 0 { 26.0 } else { 20.0 }
        } else {
            0.0
        };
        out.push(make_tile(
            child, tile.rect, depth, camera, can_nest, empty_dir,
        ));
        if can_nest {
            let inner = Rect {
                x: tile.rect.x + 2.0,
                y: tile.rect.y + header_h,
                w: (tile.rect.w - 4.0).max(0.0),
                h: (tile.rect.h - header_h - 2.0).max(0.0),
            };
            build_scene(tree, child.id, inner, depth + 1, camera, &hidden, out);
        }
    }
}

/// A filtered scene keeps the real hierarchy: matching entries appear below
/// their real ancestor folders, but only branches that contain a match are
/// shown. Container tiles report the sum of the matching bytes beneath them.
fn build_filtered_scene(
    tree: &Tree,
    nodes: &[FilterNode],
    area: Rect,
    depth: u8,
    camera: &Camera,
    out: &mut Vec<SceneTile>,
) {
    if depth > MAX_DEPTH || out.len() > MAX_TILES || area.is_empty() || nodes.is_empty() {
        return;
    }

    let mut children: Vec<SceneChild> = Vec::new();
    let mut descendants: Vec<Option<&[FilterNode]>> = Vec::new();
    if nodes.len() > MAX_CHILDREN {
        let split = MAX_CHILDREN - 1;
        for node in &nodes[..split] {
            children.push(scene_child(tree, node));
            descendants.push((!node.is_match).then_some(node.children.as_slice()));
        }
        let tail = tail_filter_nodes(tree, &nodes[split..], "smaller matches");
        children.push(tail);
        descendants.push(None);
    } else {
        for node in nodes {
            children.push(scene_child(tree, node));
            descendants.push((!node.is_match).then_some(node.children.as_slice()));
        }
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
    let gutter = if depth == 0 { 2.4 } else { 1.6 };
    let tiles = treemap::squarify(area, &items, gutter);
    for tile in tiles {
        let Some(idx) = items.iter().position(|i| i.id == tile.id) else {
            continue;
        };
        let child = &children[idx];
        let nested = descendants[idx].filter(|children| !children.is_empty());
        let screen_area = tile.rect.area() * camera.scale * camera.scale;
        let can_nest = nested.is_some()
            && depth < MAX_DEPTH
            && screen_area > if depth == 0 { 9_000.0 } else { 16_000.0 }
            && tile.rect.h > 66.0
            && tile.rect.w > 72.0;
        let header_h = if can_nest {
            if depth == 0 { 26.0 } else { 20.0 }
        } else {
            0.0
        };
        out.push(make_tile(child, tile.rect, depth, camera, can_nest, false));
        if can_nest {
            let inner = Rect {
                x: tile.rect.x + 2.0,
                y: tile.rect.y + header_h,
                w: (tile.rect.w - 4.0).max(0.0),
                h: (tile.rect.h - header_h - 2.0).max(0.0),
            };
            build_filtered_scene(
                tree,
                nested.unwrap_or_default(),
                inner,
                depth + 1,
                camera,
                out,
            );
        }
    }
}

fn scene_child(tree: &Tree, node: &FilterNode) -> SceneChild {
    SceneChild {
        id: node.id,
        name: tree.name(node.id).to_string(),
        cat: node.cat,
        size: node.size,
        is_dir: node.is_dir,
    }
}

fn tail_tile(tail: &[SceneChild], noun: &str) -> SceneChild {
    let mut per_category = [0u64; 8];
    for child in tail {
        per_category[child.cat.id() as usize] =
            per_category[child.cat.id() as usize].saturating_add(child.size);
    }
    let cat = Category::ALL
        .into_iter()
        .max_by_key(|cat| per_category[cat.id() as usize])
        .unwrap_or(Category::Other);
    SceneChild {
        id: TAIL_ID,
        name: format!("{} {noun}", fmt::count(tail.len() as u64)),
        cat,
        size: tail.iter().map(|child| child.size).sum(),
        is_dir: false,
    }
}

fn tail_filter_nodes(tree: &Tree, tail: &[FilterNode], noun: &str) -> SceneChild {
    let mapped: Vec<SceneChild> = tail.iter().map(|node| scene_child(tree, node)).collect();
    tail_tile(&mapped, noun)
}

fn make_tile(
    child: &SceneChild,
    rect: Rect,
    depth: u8,
    camera: &Camera,
    can_nest: bool,
    empty_dir: bool,
) -> SceneTile {
    let header_h = if can_nest {
        if depth == 0 { 26.0 } else { 20.0 }
    } else {
        0.0
    };
    let screen = camera.transform(rect);
    let screen_header = header_h * camera.scale;
    let aggregate = child.id == TAIL_ID;
    SceneTile {
        node: if aggregate { NO_NODE } else { child.id },
        name: child.name.clone(),
        cat: child.cat,
        size: child.size,
        depth,
        rect: screen,
        header_h: screen_header,
        has_children: can_nest,
        empty_dir,
        aggregate,
    }
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
}
