//! WGPU-backed treemap renderer.
//!
//! The renderer is a single instanced draw call per treemap widget. Geometry
//! is built on the CPU as a plain [`DrawList`]; the GPU-side pipeline lives in
//! [`gpu`] and is hidden behind Iced's custom shader widget.
//!
//! Colors in a [`DrawTile`] are **sRGB** RGBA components (the same convention
//! as [`iced::Color`]). They are converted to linear RGB while the instance
//! data is uploaded, because Iced renders to an sRGB target by default.
//!
//! ```ignore
//! let draw = Arc::new(DrawList::from_tiles((width, height), tiles));
//! let widget = iced::widget::shader(TreemapProgram::new(draw))
//!     .width(iced::Fill)
//!     .height(iced::Fill);
//! ```

mod gpu;

pub use gpu::{TreemapPipeline, TreemapPrimitive, TreemapState};

use crate::treemap::Rect;
use iced::Rectangle;
use iced::mouse;
use iced::widget::shader;
use std::sync::Arc;

/// One filled, optionally stroked, rounded rectangle on the treemap canvas.
///
/// Coordinates are logical pixels relative to the top-left of the canvas
/// (the shader widget's bounds), not screen coordinates.
#[derive(Debug, Clone, Copy)]
pub struct DrawTile {
    pub rect: Rect,
    /// Fill color in sRGB RGBA, clamped to `0..=1` when uploaded.
    pub color: [f32; 4],
    /// Inner stroke color in sRGB RGBA.
    pub border: [f32; 4],
    /// Inner stroke width in logical pixels. `0.0` disables the stroke.
    pub border_width: f32,
    /// Outer corner radius in logical pixels.
    pub radius: f32,
}

impl DrawTile {
    /// A plain tile without a stroke or rounded corners.
    pub fn new(rect: Rect, color: [f32; 4]) -> Self {
        Self {
            rect,
            color,
            border: [0.0; 4],
            border_width: 0.0,
            radius: 0.0,
        }
    }

    /// Sets an inner stroke.
    pub fn with_border(mut self, border: [f32; 4], border_width: f32) -> Self {
        self.border = border;
        self.border_width = border_width;
        self
    }

    /// Sets the outer corner radius.
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// True when the tile can contribute pixels to a canvas of `size`.
    pub fn is_visible_in(&self, size: (f32, f32)) -> bool {
        tile_visible(self, size.0, size.1)
    }
}

/// A frame's worth of treemap tiles.
///
/// `size` records the logical canvas the list was laid out for. It is metadata
/// used by callers/tests; the shader projects against the widget bounds so
/// that overlay labels and hit testing stay aligned if a frame is drawn at a
/// slightly different size.
#[derive(Debug, Clone, Default)]
pub struct DrawList {
    pub size: (f32, f32),
    pub tiles: Vec<DrawTile>,
}

impl DrawList {
    /// Creates a draw list from tiles. `size` is left at `(0, 0)`; use
    /// [`DrawList::with_size`] or [`DrawList::from_tiles`] to record it.
    pub fn new(tiles: Vec<DrawTile>) -> Self {
        Self {
            size: (0.0, 0.0),
            tiles,
        }
    }

    /// Creates a draw list with an explicit logical canvas size.
    pub fn from_tiles(size: (f32, f32), tiles: impl IntoIterator<Item = DrawTile>) -> Self {
        Self {
            size,
            tiles: tiles.into_iter().collect(),
        }
    }

    /// Records the logical canvas size.
    pub fn with_size(mut self, size: (f32, f32)) -> Self {
        self.size = size;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Union of all finite, non-degenerate tile rectangles.
    ///
    /// Returns [`Rect::ZERO`] for an empty list. Rectangles are not clamped to
    /// `size`; partially off-canvas tiles contribute their full extent.
    pub fn content_bounds(&self) -> Rect {
        let mut bounds: Option<Rect> = None;

        for tile in &self.tiles {
            let r = tile.rect;
            if !rect_is_sane(r) || r.w <= 0.01 || r.h <= 0.01 {
                continue;
            }

            bounds = Some(match bounds {
                None => r,
                Some(b) => {
                    let x = b.x.min(r.x);
                    let y = b.y.min(r.y);
                    Rect::new(
                        x,
                        y,
                        b.right().max(r.right()) - x,
                        b.bottom().max(r.bottom()) - y,
                    )
                }
            });
        }

        bounds.unwrap_or(Rect::ZERO)
    }

    /// Alias for [`DrawList::content_bounds`].
    pub fn bounds(&self) -> Rect {
        self.content_bounds()
    }

    /// Number of tiles that can produce pixels inside a canvas of `size`.
    ///
    /// This mirrors the instance culling done by the renderer and is useful
    /// for diagnostics and headless tests. It is `0` for a zero-sized canvas.
    pub fn visible_tiles(&self, size: (f32, f32)) -> usize {
        self.tiles
            .iter()
            .filter(|tile| tile.is_visible_in(size))
            .count()
    }
}

/// A shader widget specialized for the treemap renderer.
///
/// ```ignore
/// use space::render::{TreemapProgram, TreemapShader};
///
/// let shader: TreemapShader<Message> =
///     iced::widget::shader(TreemapProgram::new(draw))
///         .width(iced::Fill)
///         .height(iced::Fill);
/// ```
pub type TreemapShader<Message> = shader::Shader<Message, TreemapProgram>;

/// The custom shader program used by the treemap widget.
///
/// The program itself is cheap to clone. Its GPU buffers live in
/// [`TreemapState`], which Iced keeps in the widget tree, so replacing the
/// draw list every frame does not reallocate GPU resources unnecessarily.
#[derive(Debug, Clone)]
pub struct TreemapProgram {
    draw: Arc<DrawList>,
}

impl TreemapProgram {
    pub fn new(draw: Arc<DrawList>) -> Self {
        Self { draw }
    }

    /// Returns a new program that draws `draw` but reuses the same widget
    /// state (`TreemapState`) when Iced diffs the widget tree.
    pub fn with_draw_list(self, draw: Arc<DrawList>) -> Self {
        Self { draw }
    }

    /// Replaces the draw list in place. Widget state is unaffected.
    pub fn set_draw_list(&mut self, draw: Arc<DrawList>) {
        self.draw = draw;
    }

    pub fn draw_list(&self) -> &Arc<DrawList> {
        &self.draw
    }

    /// Builds the Iced shader widget. Equivalent to
    /// `iced::widget::shader(program)`.
    pub fn into_shader<Message>(self) -> TreemapShader<Message> {
        shader::Shader::new(self)
    }
}

impl<Message> shader::Program<Message> for TreemapProgram {
    type State = TreemapState;
    type Primitive = TreemapPrimitive;

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        TreemapPrimitive::new(Arc::clone(&self.draw), bounds, state)
    }
}

/// Convenience constructor for the shader widget.
///
/// The message type is inferred from the surrounding `Element`.
pub fn treemap<Message>(draw: Arc<DrawList>) -> TreemapShader<Message> {
    TreemapProgram::new(draw).into_shader()
}

/// Converts sRGB RGBA components to linear RGB with an unchanged alpha.
///
/// Out-of-range or non-finite channels are clamped/zeroed defensively.
pub fn srgb_to_linear(color: [f32; 4]) -> [f32; 4] {
    [
        srgb_channel_to_linear(color[0]),
        srgb_channel_to_linear(color[1]),
        srgb_channel_to_linear(color[2]),
        sanitize_channel(color[3]),
    ]
}

pub(crate) fn srgb_channel_to_linear(value: f32) -> f32 {
    let value = sanitize_channel(value);

    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn sanitize_channel(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(crate) fn rect_is_sane(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.right().is_finite()
        && rect.bottom().is_finite()
}

pub(crate) fn tile_visible(tile: &DrawTile, width: f32, height: f32) -> bool {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return false;
    }

    let r = tile.rect;
    if !rect_is_sane(r) || r.w <= 0.01 || r.h <= 0.01 {
        return false;
    }

    r.x < width && r.right() > 0.0 && r.y < height && r.bottom() > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(x: f32, y: f32, w: f32, h: f32) -> DrawTile {
        DrawTile::new(Rect::new(x, y, w, h), [0.5, 0.5, 0.5, 1.0])
    }

    #[test]
    fn zero_sized_canvas_culls_everything() {
        let draw = DrawList::new(vec![tile(0.0, 0.0, 100.0, 100.0)]);

        assert_eq!(draw.visible_tiles((0.0, 0.0)), 0);
        assert_eq!(draw.visible_tiles((-20.0, 50.0)), 0);
        assert_eq!(draw.visible_tiles((f32::NAN, 50.0)), 0);
    }

    #[test]
    fn instance_encoding_never_panics_for_empty_or_degenerate_input() {
        let empty = DrawList::default();
        let (bytes, count) = gpu::encode_instances(&empty, 0.0, 0.0, true);
        assert!(bytes.is_empty());
        assert_eq!(count, 0);

        let degenerate = DrawList::new(vec![
            DrawTile::new(Rect::new(f32::NAN, 0.0, 10.0, 10.0), [1.0; 4]),
            DrawTile::new(Rect::new(0.0, 0.0, 0.0, 10.0), [1.0; 4]),
            DrawTile::new(Rect::new(1000.0, 0.0, 10.0, 10.0), [1.0; 4]),
        ]);
        let (bytes, count) = gpu::encode_instances(&degenerate, 100.0, 100.0, true);
        assert!(bytes.is_empty());
        assert_eq!(count, 0);
    }

    #[test]
    fn instance_encoding_uses_one_stride_per_visible_tile() {
        let draw = DrawList::new(vec![tile(0.0, 0.0, 10.0, 10.0), tile(5.0, 5.0, 20.0, 20.0)]);
        let (bytes, count) = gpu::encode_instances(&draw, 100.0, 100.0, true);

        assert_eq!(count, 2);
        assert_eq!(bytes.len(), 2 * gpu::INSTANCE_SIZE);
    }
}
