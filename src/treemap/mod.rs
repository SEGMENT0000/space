//! Pure treemap geometry. No Iced or WGPU types belong in this module.
//!
//! Layout math runs in `f64` and is converted to `f32` only at the module
//! boundary. That keeps accumulated rounding far below one screen pixel while
//! producing the plain `Rect`s consumed by the renderer and hit testing.
//!
//! Invariants of [`squarify`] (with a valid, positive area):
//! - one tile per positive-size, non-sentinel item, in stable size-descending
//!   order;
//! - every rect is finite, non-negative and inside `area` (floating-point
//!   epsilon applies only to shared edges);
//! - rects do not overlap; gutters only shrink rects and can never move them
//!   outside their cell;
//! - with `gutter == 0`, rect area is proportional to item size.
//!
//! The camera treats its viewport as top-left local coordinates `(0, 0)`.
//! `Camera::transform` maps content to those local screen coordinates.

use crate::model::{Category, NO_NODE, NodeId};

/// Maximum supported camera zoom. Keeps transforms well inside `f32` range.
pub const MAX_SCALE: f32 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Shrink by `d` on every side. The inset is clamped per axis to half the
    /// extent, so a huge gutter yields a zero-size rect at the cell centre
    /// instead of a rect outside the tile.
    pub fn inset(&self, d: f32) -> Rect {
        let d = if d.is_finite() { d.max(0.0) } else { 0.0 };
        let dx = d.min(self.w.max(0.0) * 0.5);
        let dy = d.min(self.h.max(0.0) * 0.5);
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - dx * 2.0).max(0.0),
            h: (self.h - dy * 2.0).max(0.0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.5 || self.h <= 0.5
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub id: NodeId,
    pub size: u64,
    pub cat: Category,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub id: NodeId,
    pub cat: Category,
    pub is_dir: bool,
    pub size: u64,
    pub rect: Rect,
}

/// Squarified treemap (Bruls et al.). `items` need not be sorted.
///
/// Zero-size items and the `NO_NODE` sentinel are omitted. The remaining tiles
/// are returned in stable descending-size order (equal sizes keep input order).
/// An area that is non-finite, has non-positive extent or overflows `f32`
/// produces an empty layout.
pub fn squarify(area: Rect, items: &[Item], gutter: f32) -> Vec<Tile> {
    let Some((ax, ay, aw, ah)) = valid_area(area) else {
        return Vec::new();
    };

    let mut order: Vec<Item> = items
        .iter()
        .copied()
        .filter(|i| i.size > 0 && i.id != NO_NODE)
        .collect();
    if order.is_empty() {
        return Vec::new();
    }
    // Stable: equal sizes preserve the caller's order, keeping output
    // deterministic even for duplicate sizes.
    order.sort_by_key(|item| std::cmp::Reverse(item.size));

    let total: u128 = order.iter().map(|i| i.size as u128).sum();
    if total == 0 {
        return Vec::new();
    }
    let total_f = total as f64;
    let area_px = aw * ah;
    let scaled: Vec<f64> = order
        .iter()
        .map(|i| (i.size as f64 / total_f) * area_px)
        .collect();

    let mut cells = vec![Cell::ZERO; order.len()];
    layout_cells(&scaled, ax, ay, ax + aw, ay + ah, &mut cells);

    let g = if gutter.is_finite() {
        gutter.max(0.0)
    } else {
        0.0
    };
    let half_g = (g as f64 * 0.5).min(f32::MAX as f64) as f32;

    order
        .iter()
        .zip(cells)
        .map(|(item, cell)| {
            let rect = cell.to_rect().inset(half_g);
            Tile {
                id: item.id,
                cat: item.cat,
                is_dir: item.is_dir,
                size: item.size,
                rect,
            }
        })
        .collect()
}

/// Lay out a directory's children inside an already placed parent tile.
///
/// Exactly `squarify(parent.rect, children, gutter)`: the child rects cover the
/// parent's full rect (including its own gutter border), because the parent's
/// gutter was already applied when the parent was laid out. Returns an empty
/// vector for an invalid parent rect.
pub fn layout_children(parent: &Tile, children: &[Item], gutter: f32) -> Vec<Tile> {
    squarify(parent.rect, children, gutter)
}

fn valid_area(area: Rect) -> Option<(f64, f64, f64, f64)> {
    let x = area.x as f64;
    let y = area.y as f64;
    let w = area.w as f64;
    let h = area.h as f64;
    if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
        return None;
    }
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let right = x + w;
    let bottom = y + h;
    let lo = f32::MIN as f64;
    let hi = f32::MAX as f64;
    if !right.is_finite() || !bottom.is_finite() {
        return None;
    }
    if x < lo || y < lo || right > hi || bottom > hi {
        return None;
    }
    Some((x, y, w, h))
}

#[derive(Clone, Copy)]
struct Cell {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Cell {
    const ZERO: Cell = Cell {
        x0: 0.0,
        y0: 0.0,
        x1: 0.0,
        y1: 0.0,
    };

    fn to_rect(self) -> Rect {
        let x = self.x0 as f32;
        let y = self.y0 as f32;
        let right = self.x1 as f32;
        let bottom = self.y1 as f32;
        Rect {
            x,
            y,
            w: (right - x).max(0.0),
            h: (bottom - y).max(0.0),
        }
    }
}

/// Worst aspect ratio of `[sum, min, max]` in a strip of length `side`.
// Comparison is written as a negation so NaN also fails the validity test.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn worst(sum: f64, min: f64, max: f64, side: f64) -> f64 {
    if !(sum > 0.0) || !(min > 0.0) || !(side > 0.0) {
        return f64::INFINITY;
    }
    let s2 = sum * sum;
    let side2 = side * side;
    let a = side2 * max / s2;
    let b = s2 / (side2 * min);
    let w = a.max(b);
    if w.is_finite() { w } else { f64::INFINITY }
}

/// Iterative squarify. `areas` are in input order and sum to the free area.
/// Every slot in `out` is written, so items are never silently dropped.
// NaN must fail the positive-extent checks rather than compare as greater.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn layout_cells(areas: &[f64], ax: f64, ay: f64, right: f64, bottom: f64, out: &mut [Cell]) {
    let n = areas.len();
    let mut fx = ax;
    let mut fy = ay;
    let mut fw = right - ax;
    let mut fh = bottom - ay;
    let mut start = 0usize;

    while start < n {
        if !(fw > 0.0) || !(fh > 0.0) {
            let empty = Cell {
                x0: fx,
                y0: fy,
                x1: fx,
                y1: fy,
            };
            for slot in &mut out[start..] {
                *slot = empty;
            }
            break;
        }

        let side = fw.min(fh);
        let mut end = start + 1;
        let mut sum = areas[start];
        let mut min = areas[start];
        let mut max = areas[start];
        let current = worst(sum, min, max, side);

        while end < n {
            let a = areas[end];
            let next_sum = sum + a;
            let next_min = min.min(a);
            let next_max = max.max(a);
            let next = worst(next_sum, next_min, next_max, side);
            // `!(next <= current)` also stops on NaN.
            if !(next <= current) {
                break;
            }
            sum = next_sum;
            min = next_min;
            max = next_max;
            end += 1;
        }

        if fw >= fh {
            // Vertical strip: fixed width, stack the row downwards.
            let width = if fh > 0.0 { (sum / fh).min(fw) } else { 0.0 };
            let strip_right = (fx + width).min(right);
            let row_bottom = fy + fh;
            let mut y = fy;
            for k in start..end {
                let h = if k + 1 == end {
                    (row_bottom - y).max(0.0)
                } else if width > 0.0 {
                    (areas[k] / width).max(0.0).min(row_bottom - y)
                } else {
                    0.0
                };
                out[k] = Cell {
                    x0: fx,
                    y0: y,
                    x1: strip_right,
                    y1: y + h,
                };
                y += h;
            }
            fx = strip_right;
            fw = (right - fx).max(0.0);
        } else {
            // Horizontal strip: fixed height, stack the row to the right.
            let height = if fw > 0.0 { (sum / fw).min(fh) } else { 0.0 };
            let strip_bottom = (fy + height).min(bottom);
            let row_right = fx + fw;
            let mut x = fx;
            for k in start..end {
                let w = if k + 1 == end {
                    (row_right - x).max(0.0)
                } else if height > 0.0 {
                    (areas[k] / height).max(0.0).min(row_right - x)
                } else {
                    0.0
                };
                out[k] = Cell {
                    x0: x,
                    y0: fy,
                    x1: x + w,
                    y1: strip_bottom,
                };
                x += w;
            }
            fy = strip_bottom;
            fh = (bottom - fy).max(0.0);
        }
        start = end;
    }
}

/// Camera maps viewport-local content coordinates to screen coordinates.
/// At refit: `scale == 1`, offsets are zero. Scale is never below 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

impl Camera {
    pub fn is_identity(&self) -> bool {
        self.scale.is_finite()
            && (self.scale - 1.0).abs() < 0.001
            && self.offset_x.is_finite()
            && self.offset_y.is_finite()
            && self.offset_x.abs() < 0.5
            && self.offset_y.abs() < 0.5
    }

    pub fn transform(&self, r: Rect) -> Rect {
        let s = self.scale_for_math();
        let ox = if self.offset_x.is_finite() {
            self.offset_x as f64
        } else {
            0.0
        };
        let oy = if self.offset_y.is_finite() {
            self.offset_y as f64
        } else {
            0.0
        };
        Rect {
            x: (r.x as f64 * s + ox) as f32,
            y: (r.y as f64 * s + oy) as f32,
            w: (r.w as f64 * s) as f32,
            h: (r.h as f64 * s) as f32,
        }
    }

    /// Exact inverse of [`Camera::transform`] (up to `f32` rounding).
    pub fn content_point(&self, x: f32, y: f32) -> (f32, f32) {
        let s = self.scale_for_math();
        let ox = if self.offset_x.is_finite() {
            self.offset_x as f64
        } else {
            0.0
        };
        let oy = if self.offset_y.is_finite() {
            self.offset_y as f64
        } else {
            0.0
        };
        (((x as f64 - ox) / s) as f32, ((y as f64 - oy) / s) as f32)
    }

    /// Multiply zoom while keeping the content point under `anchor` fixed.
    /// Pan clamping wins at the content edges, where the anchor necessarily
    /// moves because exposing background is not allowed.
    pub fn zoom_at(&mut self, factor: f32, anchor: (f32, f32), viewport: Rect, max_scale: f32) {
        if !anchor.0.is_finite() || !anchor.1.is_finite() {
            return;
        }
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        if !viewport.w.is_finite() || !viewport.h.is_finite() {
            return;
        }
        self.make_finite();
        let max = sanitize_max_scale(max_scale);
        if self.scale > max {
            self.scale = max;
            self.clamp(viewport);
        }
        let old = self.scale;
        let new = (old * factor).clamp(1.0, max);
        if (new - old).abs() <= f32::EPSILON {
            self.scale = new;
            self.clamp(viewport);
            return;
        }
        let (cx, cy) = self.content_point(anchor.0, anchor.1);
        self.scale = new;
        self.offset_x = anchor.0 - (cx as f64 * new as f64) as f32;
        self.offset_y = anchor.1 - (cy as f64 * new as f64) as f32;
        self.clamp(viewport);
    }

    pub fn pan(&mut self, dx: f32, dy: f32, viewport: Rect) {
        self.make_finite();
        if dx.is_finite() {
            self.offset_x += dx;
        }
        if dy.is_finite() {
            self.offset_y += dy;
        }
        self.clamp(viewport);
    }

    /// Clamp scale to `[1, MAX_SCALE]` and keep the content covering the
    /// viewport. `viewport` contributes its size only: camera space is
    /// viewport-local, so its origin is `(0, 0)`.
    pub fn clamp(&mut self, viewport: Rect) {
        self.make_finite();
        let vw = if viewport.w.is_finite() {
            viewport.w.max(0.0) as f64
        } else {
            0.0
        };
        let vh = if viewport.h.is_finite() {
            viewport.h.max(0.0) as f64
        } else {
            0.0
        };
        let s = self.scale as f64;
        let min_x = vw * (1.0 - s);
        let min_y = vh * (1.0 - s);
        self.offset_x = (self.offset_x as f64).clamp(min_x.min(0.0), 0.0) as f32;
        self.offset_y = (self.offset_y as f64).clamp(min_y.min(0.0), 0.0) as f32;
    }

    fn make_finite(&mut self) {
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(1.0, MAX_SCALE);
        if !self.offset_x.is_finite() {
            self.offset_x = 0.0;
        }
        if !self.offset_y.is_finite() {
            self.offset_y = 0.0;
        }
    }

    fn scale_for_math(&self) -> f64 {
        if self.scale.is_finite() && self.scale >= 1.0 {
            self.scale as f64
        } else {
            1.0
        }
    }
}

fn sanitize_max_scale(max_scale: f32) -> f32 {
    if max_scale.is_finite() {
        max_scale.clamp(1.0, MAX_SCALE)
    } else {
        MAX_SCALE
    }
}

/// Hit test in screen coordinates. Returns the topmost tile index whose
/// transformed rect contains `point`, or `None` outside the content.
pub fn hit_test(tiles: &[Tile], camera: &Camera, point: (f32, f32)) -> Option<usize> {
    if !point.0.is_finite() || !point.1.is_finite() {
        return None;
    }
    let (cx, cy) = camera.content_point(point.0, point.1);
    if !cx.is_finite() || !cy.is_finite() {
        return None;
    }
    tiles.iter().rposition(|t| t.rect.contains(cx, cy))
}

#[derive(Debug, Clone)]
pub struct Label {
    pub tile: usize,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub max_width: f32,
    pub max_height: f32,
    /// True when the text was shortened to fit.
    pub truncated: bool,
}

const LABEL_PAD: f32 = 8.0;
const MIN_TILE_W: f32 = 42.0;
const MIN_TILE_H: f32 = 26.0;
const MIN_TEXT_W: f32 = 24.0;

/// One label per eligible tile. Labels are measured with a deliberately
/// conservative Geist width estimate, truncated on grapheme-cluster
/// boundaries, and omitted entirely when no useful text fits.
///
/// `names` may return either `&str` or `String`; both are accepted.
pub fn labels<F, S>(tiles: &[Tile], names: F) -> Vec<Label>
where
    F: Fn(NodeId) -> S,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    for (i, t) in tiles.iter().enumerate() {
        if !t.rect.x.is_finite()
            || !t.rect.y.is_finite()
            || !t.rect.w.is_finite()
            || !t.rect.h.is_finite()
            || t.rect.w < MIN_TILE_W
            || t.rect.h < MIN_TILE_H
        {
            continue;
        }
        let size = if t.rect.w > 190.0 && t.rect.h > 90.0 {
            12.5
        } else {
            11.0
        };
        let max_width = (t.rect.w - LABEL_PAD * 2.0).max(0.0);
        let max_height = (t.rect.h - LABEL_PAD * 2.0).max(0.0);
        if max_width < MIN_TEXT_W || max_height < size {
            continue;
        }

        let raw = names(t.id);
        let full = sanitize_name(raw.as_ref());
        if full.is_empty() {
            continue;
        }

        let clusters = text_clusters(&full);
        if clusters.is_empty() {
            continue;
        }
        let full_width: f32 = clusters.iter().map(|c| c.width_px(size)).sum();
        let (text, truncated) = if full_width <= max_width {
            (full, false)
        } else {
            let ellipsis = estimate_text_width("…", size);
            let budget = max_width - ellipsis;
            if budget <= 0.0 {
                continue;
            }
            let mut width = 0.0f32;
            let mut end = 0usize;
            for c in &clusters {
                let cw = c.width_px(size);
                if width + cw > budget {
                    break;
                }
                width += cw;
                end = c.end;
            }
            if end == 0 {
                continue;
            }
            let mut short = full[..end].to_string();
            while short.ends_with(' ') {
                short.pop();
            }
            if short.is_empty() {
                continue;
            }
            short.push('…');
            (short, true)
        };

        if !text.is_empty() {
            out.push(Label {
                tile: i,
                text,
                x: t.rect.x + LABEL_PAD,
                y: t.rect.y + LABEL_PAD,
                size,
                max_width,
                max_height,
                truncated,
            });
        }
    }
    out
}

/// Conservative rendered width estimate for Geist at `size` pixels.
///
/// The estimate intentionally over-states typical glyphs and counts wide /
/// emoji clusters as at least one em. Zero-width marks add nothing. It is used
/// by label fitting and is public so overlays can make the same decision.
pub fn estimate_text_width(text: &str, size: f32) -> f32 {
    if text.is_empty() || !size.is_finite() || size <= 0.0 {
        return 0.0;
    }
    let em: f64 = text_clusters(text).iter().map(|c| c.width_em as f64).sum();
    let px = em * size as f64 * 1.02;
    px.min(f32::MAX as f64) as f32
}

#[derive(Clone, Copy)]
struct TextCluster {
    end: usize,
    width_em: f32,
    emoji: bool,
    regional_pair: bool,
}

impl TextCluster {
    fn width_px(&self, size: f32) -> f32 {
        let mut em = self.width_em;
        if self.emoji || self.regional_pair {
            em = em.max(1.0);
        }
        (em as f64 * size as f64 * 1.02).min(f32::MAX as f64) as f32
    }
}

fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(1024));
    let mut last_space = true;
    for ch in raw.chars() {
        if is_bidi_control(ch) || ch == '\u{FEFF}' || ch == '\u{00AD}' {
            continue;
        }
        let ch = if ch.is_control() { ' ' } else { ch };
        if ch == ' ' || ch == '\u{200B}' {
            if !last_space && !out.is_empty() {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn text_clusters(text: &str) -> Vec<TextCluster> {
    let mut out: Vec<TextCluster> = Vec::new();
    let mut join_next = false;
    let mut prev_regional = false;

    for (idx, ch) in text.char_indices() {
        let end = idx + ch.len_utf8();
        let regional = is_regional_indicator(ch);
        let continuation = !out.is_empty()
            && (join_next
                || is_combining(ch)
                || is_variation_selector(ch)
                || is_emoji_modifier(ch)
                || ch == '\u{20E3}'
                || is_tag_char(ch)
                || (regional && prev_regional && !out.last().is_some_and(|c| c.regional_pair)));

        let mut joined = false;
        if continuation && let Some(c) = out.last_mut() {
            c.end = end;
            c.width_em = c.width_em.max(char_width_em(ch));
            if is_variation_selector(ch) && ch == '\u{FE0F}' {
                c.emoji = true;
            }
            if is_emoji_modifier(ch) || is_tag_char(ch) || ch == '\u{200D}' {
                c.emoji = true;
            }
            if regional {
                c.regional_pair = true;
                c.emoji = true;
            }
            if is_wide(ch) {
                c.emoji = true;
            }
            joined = true;
        }
        if !joined {
            out.push(TextCluster {
                end,
                width_em: char_width_em(ch),
                emoji: is_wide(ch) || is_emoji_base(ch),
                regional_pair: false,
            });
        }
        prev_regional = regional;
        join_next = ch == '\u{200D}';
    }
    out
}

fn char_width_em(ch: char) -> f32 {
    if is_combining(ch)
        || is_variation_selector(ch)
        || is_emoji_modifier(ch)
        || is_tag_char(ch)
        || ch == '\u{200D}'
        || ch == '\u{20E3}'
    {
        return 0.0;
    }
    if is_wide(ch) || is_emoji_base(ch) {
        return 1.05;
    }
    match ch {
        ' ' => 0.32,
        '\'' | '`' | '|' | '!' | '.' | ',' | ':' | ';' | 'i' | 'l' | 'j' | 'I' => 0.34,
        '(' | ')' | '[' | ']' | '{' | '}' | '/' | '\\' | 'f' | 't' | 'r' => 0.44,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.92,
        'A'..='Z' => 0.72,
        '0'..='9' => 0.64,
        'a'..='z' => 0.60,
        _ if (ch as u32) < 0x80 => 0.62,
        _ => 0.72,
    }
}

fn is_variation_selector(ch: char) -> bool {
    matches!(ch as u32, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}

fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch as u32, 0x1F3FB..=0x1F3FF)
}

fn is_tag_char(ch: char) -> bool {
    matches!(ch as u32, 0xE0020..=0xE007F)
}

fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

fn is_bidi_control(ch: char) -> bool {
    matches!(ch as u32, 0x202A..=0x202E | 0x2066..=0x2069)
}

fn is_emoji_base(ch: char) -> bool {
    matches!(ch as u32,
        0x231A..=0x231B
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x1F004
        | 0x1F0CF
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F1E6..=0x1F1FF
        | 0x1F201
        | 0x1F21A
        | 0x1F22F
        | 0x1F232..=0x1F23A
        | 0x1F250..=0x1F251
        | 0x1F300..=0x1FAFF
    )
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F
        | 0x2329..=0x232A
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF
        | 0x20000..=0x3FFFD
    )
}

fn is_combining(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x05BF
        | 0x05C1..=0x05C2
        | 0x05C4..=0x05C5
        | 0x05C7
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x06E7..=0x06E8
        | 0x06EA..=0x06ED
        | 0x0711
        | 0x0730..=0x074A
        | 0x07A6..=0x07B0
        | 0x07EB..=0x07F3
        | 0x0816..=0x0819
        | 0x081B..=0x0823
        | 0x0825..=0x0827
        | 0x0829..=0x082D
        | 0x0859..=0x085B
        | 0x08D3..=0x0902
        | 0x093A
        | 0x093C
        | 0x0941..=0x0948
        | 0x094D
        | 0x0951..=0x0957
        | 0x0962..=0x0963
        | 0x0981
        | 0x09BC
        | 0x09C1..=0x09C4
        | 0x09CD
        | 0x09E2..=0x09E3
        | 0x0A01..=0x0A02
        | 0x0A3C
        | 0x0A41..=0x0A42
        | 0x0A47..=0x0A48
        | 0x0A4B..=0x0A4D
        | 0x0A51
        | 0x0A70..=0x0A71
        | 0x0A75
        | 0x0A81..=0x0A82
        | 0x0ABC
        | 0x0AC1..=0x0AC5
        | 0x0AC7..=0x0AC8
        | 0x0ACD
        | 0x0AE2..=0x0AE3
        | 0x0B01
        | 0x0B3C
        | 0x0B3F
        | 0x0B41..=0x0B44
        | 0x0B4D
        | 0x0B56
        | 0x0B62..=0x0B63
        | 0x0B82
        | 0x0BC0
        | 0x0BCD
        | 0x0C00
        | 0x0C04
        | 0x0C3E..=0x0C40
        | 0x0C46..=0x0C48
        | 0x0C4A..=0x0C4D
        | 0x0C55..=0x0C56
        | 0x0C62..=0x0C63
        | 0x0C81
        | 0x0CBC
        | 0x0CBF
        | 0x0CC6
        | 0x0CCC..=0x0CCD
        | 0x0CE2..=0x0CE3
        | 0x0D00..=0x0D01
        | 0x0D3B..=0x0D3C
        | 0x0D41..=0x0D44
        | 0x0D4D
        | 0x0D62..=0x0D63
        | 0x0D81
        | 0x0DCA
        | 0x0DD2..=0x0DD4
        | 0x0DD6
        | 0x0E31
        | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E
        | 0x0EB1
        | 0x0EB4..=0x0EBC
        | 0x0EC8..=0x0ECD
        | 0x0F18..=0x0F19
        | 0x0F35
        | 0x0F37
        | 0x0F39
        | 0x0F71..=0x0F7E
        | 0x0F80..=0x0F84
        | 0x0F86..=0x0F87
        | 0x0F8D..=0x0F97
        | 0x0F99..=0x0FBC
        | 0x0FC6
        | 0x102D..=0x1030
        | 0x1032..=0x1037
        | 0x1039..=0x103A
        | 0x103D..=0x103E
        | 0x1058..=0x1059
        | 0x105E..=0x1060
        | 0x1071..=0x1074
        | 0x1082
        | 0x1085..=0x1086
        | 0x108D
        | 0x109D
        | 0x135D..=0x135F
        | 0x1712..=0x1714
        | 0x1732..=0x1734
        | 0x1752..=0x1753
        | 0x1772..=0x1773
        | 0x17B4..=0x17B5
        | 0x17B7..=0x17BD
        | 0x17C6
        | 0x17C9..=0x17D3
        | 0x17DD
        | 0x180B..=0x180D
        | 0x18A9
        | 0x1920..=0x1922
        | 0x1927..=0x1928
        | 0x1932
        | 0x1939..=0x193B
        | 0x1A17..=0x1A18
        | 0x1A1B
        | 0x1A56
        | 0x1A58..=0x1A5E
        | 0x1A60
        | 0x1A62
        | 0x1A65..=0x1A6C
        | 0x1A73..=0x1A7C
        | 0x1A7F
        | 0x1AB0..=0x1ACE
        | 0x1B00..=0x1B03
        | 0x1B34
        | 0x1B36..=0x1B3A
        | 0x1B3C
        | 0x1B42
        | 0x1B6B..=0x1B73
        | 0x1B80..=0x1B81
        | 0x1BA2..=0x1BA5
        | 0x1BA8..=0x1BA9
        | 0x1BAB..=0x1BAD
        | 0x1BE6
        | 0x1BE8..=0x1BE9
        | 0x1BED
        | 0x1BEF..=0x1BF1
        | 0x1C2C..=0x1C33
        | 0x1C36..=0x1C37
        | 0x1CD0..=0x1CD2
        | 0x1CD4..=0x1CE0
        | 0x1CE2..=0x1CE8
        | 0x1CED
        | 0x1CF4
        | 0x1CF8..=0x1CF9
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20F0
        | 0x2CEF..=0x2CF1
        | 0x2D7F
        | 0x2DE0..=0x2DFF
        | 0x302A..=0x302D
        | 0x3099..=0x309A
        | 0xA66F
        | 0xA674..=0xA67D
        | 0xA69E..=0xA69F
        | 0xA6F0..=0xA6F1
        | 0xA802
        | 0xA806
        | 0xA80B
        | 0xA825..=0xA826
        | 0xA8C4..=0xA8C5
        | 0xA8E0..=0xA8F1
        | 0xA8FF
        | 0xA926..=0xA92D
        | 0xA947..=0xA951
        | 0xA980..=0xA982
        | 0xA9B3
        | 0xA9B6..=0xA9B9
        | 0xA9BC..=0xA9BD
        | 0xA9E5
        | 0xAA29..=0xAA2E
        | 0xAA31..=0xAA32
        | 0xAA35..=0xAA36
        | 0xAA43
        | 0xAA4C
        | 0xAA7C
        | 0xAAB0
        | 0xAAB2..=0xAAB4
        | 0xAAB7..=0xAAB8
        | 0xAABE..=0xAABF
        | 0xAAC1
        | 0xAAEC..=0xAAED
        | 0xAAF6
        | 0xABE5
        | 0xABE8
        | 0xABED
        | 0xFB1E
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0x101FD
        | 0x102E0
        | 0x10376..=0x1037A
        | 0x10A01..=0x10A03
        | 0x10A05..=0x10A06
        | 0x10A0C..=0x10A0F
        | 0x10A38..=0x10A3A
        | 0x10A3F
        | 0x10AE5..=0x10AE6
        | 0x10D24..=0x10D27
        | 0x10EAB..=0x10EAC
        | 0x10F46..=0x10F50
        | 0x10F82..=0x10F85
        | 0x11001
        | 0x11038..=0x11046
        | 0x11070
        | 0x11073..=0x11074
        | 0x1107F..=0x11081
        | 0x110B3..=0x110B6
        | 0x110B9..=0x110BA
        | 0x110C2
        | 0x11100..=0x11102
        | 0x11127..=0x1112B
        | 0x1112D..=0x11134
        | 0x11173
        | 0x11180..=0x11181
        | 0x111B6..=0x111BE
        | 0x111C9..=0x111CC
        | 0x111CF
        | 0x1122F..=0x11231
        | 0x11234
        | 0x11236..=0x11237
        | 0x1123E
        | 0x112DF
        | 0x112E3..=0x112EA
        | 0x11300..=0x11301
        | 0x1133B..=0x1133C
        | 0x11340
        | 0x11366..=0x1136C
        | 0x11370..=0x11374
        | 0x11438..=0x1143F
        | 0x11442..=0x11444
        | 0x11446
        | 0x1145E
        | 0x114B3..=0x114B8
        | 0x114BA
        | 0x114BF..=0x114C0
        | 0x114C2..=0x114C3
        | 0x115B2..=0x115B5
        | 0x115BC..=0x115BD
        | 0x115BF..=0x115C0
        | 0x115DC..=0x115DD
        | 0x11633..=0x1163A
        | 0x1163D
        | 0x1163F..=0x11640
        | 0x116AB
        | 0x116AD
        | 0x116B0..=0x116B5
        | 0x116B7
        | 0x1171D..=0x1171F
        | 0x11722..=0x11725
        | 0x11727..=0x1172B
        | 0x1182F..=0x11837
        | 0x11839..=0x1183A
        | 0x1193B..=0x1193C
        | 0x1193E
        | 0x11943
        | 0x119D4..=0x119D7
        | 0x119DA..=0x119DB
        | 0x119E0
        | 0x11A01..=0x11A0A
        | 0x11A33..=0x11A38
        | 0x11A3B..=0x11A3E
        | 0x11A47
        | 0x11A51..=0x11A56
        | 0x11A59..=0x11A5B
        | 0x11A8A..=0x11A96
        | 0x11A98..=0x11A99
        | 0x11C30..=0x11C36
        | 0x11C38..=0x11C3D
        | 0x11C3F
        | 0x11C92..=0x11CA7
        | 0x11CAA..=0x11CB0
        | 0x11CB2..=0x11CB3
        | 0x11CB5..=0x11CB6
        | 0x11D31..=0x11D36
        | 0x11D3A
        | 0x11D3C..=0x11D3D
        | 0x11D3F..=0x11D45
        | 0x11D47
        | 0x11D90..=0x11D91
        | 0x11D95
        | 0x11D97
        | 0x11EF3..=0x11EF4
        | 0x16AF0..=0x16AF4
        | 0x16B30..=0x16B36
        | 0x16F4F
        | 0x16F8F..=0x16F92
        | 0x16FE4
        | 0x1BC9D..=0x1BC9E
        | 0x1CF00..=0x1CF2D
        | 0x1CF30..=0x1CF46
        | 0x1D167..=0x1D169
        | 0x1D17B..=0x1D182
        | 0x1D185..=0x1D18B
        | 0x1D1AA..=0x1D1AD
        | 0x1D242..=0x1D244
        | 0x1DA00..=0x1DA36
        | 0x1DA3B..=0x1DA6C
        | 0x1DA75
        | 0x1DA84
        | 0x1DA9B..=0x1DA9F
        | 0x1DAA1..=0x1DAAF
        | 0x1E000..=0x1E02A
        | 0x1E130..=0x1E136
        | 0x1E2AE
        | 0x1E2EC..=0x1E2EF
        | 0x1E8D0..=0x1E8D6
        | 0x1E944..=0x1E94A
        | 0xE0100..=0xE01EF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32, size: u64) -> Item {
        Item {
            id,
            size,
            cat: Category::Other,
            is_dir: false,
        }
    }

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 640.0, 360.0)
    }

    fn check_geometry(area: Rect, tiles: &[Tile], eps: f32) {
        let right = area.right();
        let bottom = area.bottom();
        for t in tiles {
            let r = t.rect;
            assert!(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite());
            assert!(r.w >= 0.0 && r.h >= 0.0);
            assert!(r.x >= area.x - eps && r.y >= area.y - eps);
            assert!(r.right() <= right + eps && r.bottom() <= bottom + eps);
        }
        for i in 0..tiles.len() {
            for j in i + 1..tiles.len() {
                let a = tiles[i].rect;
                let b = tiles[j].rect;
                let ox = a.right().min(b.right()) - a.x.max(b.x);
                let oy = a.bottom().min(b.bottom()) - a.y.max(b.y);
                assert!(ox <= eps || oy <= eps, "overlap {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn empty_and_single() {
        assert!(squarify(area(), &[], 2.0).is_empty());
        assert!(squarify(area(), &[item(0, 0)], 0.0).is_empty());
        let tiles = squarify(area(), &[item(0, 5)], 0.0);
        assert_eq!(tiles.len(), 1);
        assert!((tiles[0].rect.area() - area().area()).abs() < 0.01);
    }

    #[test]
    fn tiles_stay_inside_and_do_not_overlap() {
        let a = Rect::new(10.0, 20.0, 600.0, 300.0);
        let items: Vec<Item> = (1..=40).map(|i| item(i, (i * 37) as u64)).collect();
        let tiles = squarify(a, &items, 2.0);
        assert_eq!(tiles.len(), 40);
        check_geometry(a, &tiles, 0.01);
        for pair in tiles.windows(2) {
            assert!(pair[0].size >= pair[1].size);
        }
    }

    #[test]
    fn huge_gutter_stays_inside() {
        let a = Rect::new(5.0, 7.0, 120.0, 80.0);
        let items = [item(1, 3), item(2, 2), item(3, 1)];
        for gutter in [1e9, f32::INFINITY, f32::NAN, -5.0] {
            let tiles = squarify(a, &items, gutter);
            assert_eq!(tiles.len(), 3);
            check_geometry(a, &tiles, 0.01);
            for t in &tiles {
                assert!(t.rect.w >= 0.0 && t.rect.h >= 0.0);
            }
        }
    }

    #[test]
    fn zoom_anchor_and_hit() {
        let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut cam = Camera::default();
        cam.zoom_at(2.0, (400.0, 300.0), vp, 16.0);
        assert_eq!(cam.scale, 2.0);
        assert!((cam.offset_x + 400.0).abs() < 0.01);
        let (cx, cy) = cam.content_point(400.0, 300.0);
        assert!((cx - 400.0).abs() < 0.01 && (cy - 300.0).abs() < 0.01);

        let tiles = squarify(vp, &[item(1, 3), item(2, 1)], 0.0);
        let t = tiles[0];
        let sx = t.rect.x * cam.scale + cam.offset_x + 1.0;
        let sy = t.rect.y * cam.scale + cam.offset_y + 1.0;
        assert_eq!(hit_test(&tiles, &cam, (sx, sy)), Some(0));
        assert_eq!(
            hit_test(&tiles, &cam, (cam.offset_x - 1.0, cam.offset_y - 1.0)),
            None
        );
    }

    #[test]
    fn camera_pan_clamps() {
        let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut cam = Camera::default();
        cam.pan(100.0, 100.0, vp);
        assert!(cam.is_identity());
        cam.zoom_at(4.0, (400.0, 300.0), vp, 16.0);
        cam.pan(10_000.0, -10_000.0, vp);
        assert!(cam.offset_x <= 0.0 && cam.offset_x >= 800.0 * (1.0 - 4.0) - 0.01);
        assert!(cam.offset_y <= 0.0 && cam.offset_y >= 600.0 * (1.0 - 4.0) - 0.01);
        let content = cam.transform(Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(content.x <= 0.01 && content.y <= 0.01);
        assert!(content.right() >= 799.99 && content.bottom() >= 599.99);
    }

    #[test]
    fn labels_fit_and_truncate_by_chars() {
        let tiles = squarify(area(), &[item(1, 9), item(2, 3), item(3, 1)], 0.0);
        let names = |id: NodeId| match id {
            1 => "a very long file name that definitely will not fit".to_string(),
            2 => "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}".to_string(),
            _ => "ok".to_string(),
        };
        let labels = labels(&tiles, names);
        assert!(!labels.is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for l in &labels {
            assert!(seen.insert(l.tile));
            let t = tiles[l.tile].rect;
            assert!(l.x >= t.x - 0.01 && l.y >= t.y - 0.01);
            assert!(l.x + l.max_width <= t.right() + 0.01);
            assert!(l.y + l.max_height <= t.bottom() + 0.01);
            assert!(l.x + estimate_text_width(&l.text, l.size) <= t.right() + 0.01);
            if l.truncated {
                assert!(l.text.ends_with('…'));
            } else {
                assert!(!l.text.contains('…'));
            }
        }
    }

    #[test]
    fn layout_children_stays_in_parent() {
        let parent = squarify(area(), &[item(1, 100)], 0.0)[0];
        let kids: Vec<Item> = (10..20).map(|i| item(i, i as u64)).collect();
        let child_tiles = layout_children(&parent, &kids, 1.0);
        assert_eq!(child_tiles.len(), kids.len());
        check_geometry(parent.rect, &child_tiles, 0.01);
    }
}
