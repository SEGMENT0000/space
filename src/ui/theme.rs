//! Native design tokens and type primitives. UI styling should resolve here.
//!
//! The language is a precision instrument: quiet paper-like surfaces, hairline
//! structure, one sage accent, Geist at real weight steps, and mono figures for
//! measurements. Color belongs to the treemap and to state, never to chrome.

#![allow(dead_code)]

use iced::color;
use iced::font::{Family, Weight};
use iced::{Color, Font};

use crate::model::Category;

// ---------------------------------------------------------------------------
// Typefaces
// ---------------------------------------------------------------------------

/// UI family. Geist is the only sans-serif used by the product.
pub const FONT_UI: &str = "Geist";
/// Figure, path and keycap family. Geist Mono is the only monospace used.
pub const FONT_MONO: &str = "Geist Mono";

/// The complete type scale. Sizes are tightly related; do not invent
/// intermediate steps. Line heights are absolute pixel values.
pub const TEXT_XS: f32 = 11.0; // micro labels, keycaps
pub const TEXT_SM: f32 = 12.0; // captions, table metadata, paths
pub const TEXT_BASE: f32 = 13.0; // body, controls, tree rows
pub const TEXT_MD: f32 = 15.0; // section labels, emphasized body
pub const TEXT_HEADING: f32 = 17.0; // panel and screen headings
pub const TEXT_LG: f32 = 22.0; // screen titles, large figures
pub const TEXT_XL: f32 = 28.0; // empty-state displays, hero figures

pub const LINE_XS: f32 = 14.0;
pub const LINE_SM: f32 = 16.0;
pub const LINE_BASE: f32 = 18.0;
pub const LINE_MD: f32 = 20.0;
pub const LINE_HEADING: f32 = 22.0;
pub const LINE_LG: f32 = 28.0;
pub const LINE_XL: f32 = 34.0;

/// Numeric figures are mono so columns align and measurements feel exact.
pub const FIGURE_SM: f32 = 12.0;
pub const FIGURE_MD: f32 = 15.0;
pub const FIGURE_LG: f32 = 22.0;
pub const FIGURE_XL: f32 = 28.0;

pub const LINE_FIGURE_SM: f32 = 16.0;
pub const LINE_FIGURE_MD: f32 = 20.0;
pub const LINE_FIGURE_LG: f32 = 26.0;
pub const LINE_FIGURE_XL: f32 = 32.0;

pub const PATH_TEXT: f32 = 12.0;
pub const LINE_PATH: f32 = 16.0;
pub const KEYCAP_TEXT: f32 = 11.0;
pub const LINE_KEYCAP: f32 = 14.0;

/// Regular for prose, Medium for controls and labels, SemiBold for headings
/// and figures, Bold only for the wordmark or a single hero number.
pub fn font_regular() -> Font {
    ui_font(Weight::Normal)
}

pub fn font_medium() -> Font {
    ui_font(Weight::Medium)
}

pub fn font_semibold() -> Font {
    ui_font(Weight::Semibold)
}

pub fn font_bold() -> Font {
    ui_font(Weight::Bold)
}

pub fn mono_regular() -> Font {
    mono_font(Weight::Normal)
}

pub fn mono_medium() -> Font {
    mono_font(Weight::Medium)
}

pub fn mono_semibold() -> Font {
    mono_font(Weight::Semibold)
}

// ---------------------------------------------------------------------------
// Spacing, shape and structure
// ---------------------------------------------------------------------------

/// One four-pixel grid. The named steps are the only spacing values allowed.
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;
pub const SPACE_8: f32 = 32.0;
pub const SPACE_12: f32 = 48.0;

/// Shape hierarchy: marks and tiles 2, controls 4, panels 6, overlays 8,
/// sheets 10. Nothing is fully rounded except intentional status pills.
pub const RADIUS_XS: f32 = 2.0;
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 8.0;
pub const RADIUS_XL: f32 = 10.0;

pub const CONTROL_SM: f32 = 24.0;
pub const CONTROL_MD: f32 = 28.0;
pub const CONTROL_LG: f32 = 32.0;

pub const TOPBAR_HEIGHT: f32 = 52.0;
pub const STATUSBAR_HEIGHT: f32 = 28.0;
pub const INSPECTOR_WIDTH: f32 = 300.0;
pub const ROW_HEIGHT: f32 = 28.0;

// One-pixel rules carry structure. Stronger rules are reserved for selected
// boundaries and parent outlines; focus is a two-pixel ring with a one-pixel
// offset.
pub const HAIRLINE_WIDTH: f32 = 1.0;
pub const HAIRLINE_STRONG_WIDTH: f32 = 1.5;
pub const FOCUS_WIDTH: f32 = 2.0;
pub const FOCUS_OFFSET: f32 = 1.0;

/// The only elevation recipe: floating overlays, never cards.
pub const SHADOW_OFFSET_X: f32 = 0.0;
pub const SHADOW_OFFSET_Y: f32 = 8.0;
pub const SHADOW_BLUR: f32 = 24.0;

pub const ICON_SM: f32 = 14.0;
pub const ICON_MD: f32 = 16.0;
pub const ICON_LG: f32 = 20.0;
pub const ICON_STROKE: f32 = 1.5;

// ---------------------------------------------------------------------------
// Treemap geometry and semantics
// ---------------------------------------------------------------------------

/// One-pixel gutters let the bed breathe between leaves. Groups inset three
/// pixels so nesting reads as containment, not as another border.
pub const TREEMAP_LEAF_GUTTER: f32 = 1.0;
pub const TREEMAP_GROUP_INSET: f32 = 3.0;
pub const TREEMAP_TILE_RADIUS: f32 = 2.0;

/// Depth is expressed by a subtle tint toward the raised surface (light) or
/// the bed (dark). Eight percent per level, capped at twenty-four percent.
/// Deep recursion must never turn into a different color family.
pub const TREEMAP_DEPTH_TINT: f32 = 0.08;
pub const TREEMAP_DEPTH_TINT_MAX: f32 = 0.24;

/// Hidden categories recede toward the bed without disappearing entirely.
pub const TREEMAP_HIDDEN_TINT: f32 = 0.84;

/// Parent-group outlines are category ink interpolated toward the fill.
/// They are stronger than a hairline but thinner than a selected tile border.
pub const TREEMAP_PARENT_LINE_WIDTH: f32 = 1.5;
pub const TREEMAP_PARENT_LINE_MIX_LIGHT: f32 = 0.42;
pub const TREEMAP_PARENT_LINE_MIX_DARK: f32 = 0.30;

// ---------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------

/// One curve for the whole product: quick departure, long settle, no
/// overshoot. Hover, state, overlay and camera durations stay under 240 ms.
pub const MOTION_CURVE: [f32; 4] = [0.16, 1.0, 0.3, 1.0];

pub const MOTION_HOVER_MS: u64 = 120;
pub const MOTION_STATE_MS: u64 = 160;
pub const MOTION_OVERLAY_MS: u64 = 200;
pub const MOTION_CAMERA_MS: u64 = 220;
pub const MOTION_CEILING_MS: u64 = 240;

fn bezier_axis(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn bezier_derivative(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// Evaluate the standard motion curve at linear progress `0.0..=1.0`.
pub fn ease(progress: f32) -> f32 {
    let x = progress.clamp(0.0, 1.0);
    if x <= 0.0 || x >= 1.0 {
        return x;
    }

    let [x1, y1, x2, y2] = MOTION_CURVE;

    // Newton-Raphson converges in a few steps for this well-behaved curve.
    let mut u = x;
    for _ in 0..8 {
        let error = bezier_axis(u, x1, x2) - x;
        if error.abs() < 1.0e-6 {
            break;
        }
        let slope = bezier_derivative(u, x1, x2);
        if slope.abs() < 1.0e-6 {
            break;
        }
        u = (u - error / slope).clamp(0.0, 1.0);
    }

    // Bisection keeps the result deterministic even at flat derivatives.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..12 {
        let mid = 0.5 * (lo + hi);
        if bezier_axis(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let u = 0.5 * (lo + hi);
    bezier_axis(u, y1, y2)
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// All color and category tokens for one appearance.
///
/// `dark` and `is_dark` are intentionally both present: `is_dark` is the
/// canonical token and `dark` keeps existing call sites source-compatible.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub dark: bool,

    /// The application bed. Also the treemap bed.
    pub bg: Color,
    /// Chrome and card surface.
    pub surface: Color,
    /// Floating surfaces: menus, palette, dialog sheets.
    pub surface_raised: Color,
    /// Wells, tracks and inset fields.
    pub surface_sunken: Color,
    /// Transient hover wash.
    pub surface_hover: Color,
    /// Transient pressed wash.
    pub surface_pressed: Color,

    pub ink: Color,
    pub ink_muted: Color,
    /// Lowest text token. AA on the bed, never below 11 px.
    pub ink_faint: Color,

    pub hairline: Color,
    /// Boundary token for overlays and controls that need 3:1 non-text
    /// contrast. Not a text color.
    pub hairline_strong: Color,

    /// The single interaction accent. No second accent exists.
    pub accent: Color,
    /// Text and icons placed on `accent`.
    pub accent_ink: Color,
    /// Selected rows, active palette rows and soft accent wash.
    pub accent_soft: Color,
    /// Text-selection wash. Must remain distinct from `accent_soft`.
    pub selection: Color,

    pub danger: Color,
    /// Optional tinted danger well; keep text at `danger` or `ink`.
    pub danger_soft: Color,
    pub warning: Color,
    pub success: Color,

    /// Modal scrim. The only translucent full-screen token.
    pub scrim: Color,
    /// Elevation shadow. Floating overlays only; cards never use it.
    pub shadow: Color,

    /// Solid category fill used for swatches and treemap leaves.
    pub category_fill: [Color; 8],
    /// Category ink for labels on the fill or on the soft tint.
    pub category_ink: [Color; 8],
    /// Very low-chroma category wash for rows and chips.
    pub category_soft: [Color; 8],
}

impl Theme {
    pub fn light() -> Self {
        Self {
            is_dark: false,
            dark: false,

            bg: color!(0xF5F6F5),
            surface: color!(0xFBFCFB),
            surface_raised: color!(0xFFFFFF),
            surface_sunken: color!(0xEDF0EE),
            surface_hover: color!(0xF0F2F1),
            surface_pressed: color!(0xE5E8E6),

            ink: color!(0x1B201D),
            ink_muted: color!(0x525A56),
            ink_faint: color!(0x666E6A),

            hairline: color!(0xE2E5E3),
            hairline_strong: color!(0x87908A),

            accent: color!(0x256B5A),
            accent_ink: color!(0xF7FBF9),
            accent_soft: color!(0xDDEDE7),
            selection: color!(0xD3E7DE),

            danger: color!(0xB24A35),
            danger_soft: color!(0xFAEDE9),
            warning: color!(0x8A621C),
            success: color!(0x3F704D),

            scrim: Color::from_rgba(19.0 / 255.0, 24.0 / 255.0, 21.0 / 255.0, 0.24),
            shadow: Color::from_rgba(19.0 / 255.0, 24.0 / 255.0, 21.0 / 255.0, 0.16),

            category_fill: [
                color!(0xF3C8B5), // Media
                color!(0xEFE1B4), // Documents
                color!(0xCFE0C1), // Code
                color!(0xB9E4E9), // Caches
                color!(0xD6D6F0), // Apps
                color!(0xF2CEDC), // Archives
                color!(0xBBC6CE), // System
                color!(0xDAD1C6), // Other
            ],
            category_soft: [
                color!(0xFAE6DE),
                color!(0xF8F2DD),
                color!(0xE9F1E3),
                color!(0xE0F3F5),
                color!(0xEDEDF8),
                color!(0xF9E9EF),
                color!(0xE0E5E9),
                color!(0xEEEAE5),
            ],
            category_ink: [
                color!(0x5B392B),
                color!(0x594924),
                color!(0x36533A),
                color!(0x2E4C55),
                color!(0x453F5F),
                color!(0x5D3A47),
                color!(0x3C474F),
                color!(0x4F493D),
            ],
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            dark: true,

            bg: color!(0x121514),
            surface: color!(0x181C1A),
            surface_raised: color!(0x202522),
            surface_sunken: color!(0x0D100F),
            surface_hover: color!(0x202622),
            surface_pressed: color!(0x29302B),

            ink: color!(0xEDF1EF),
            ink_muted: color!(0xAAB1AD),
            ink_faint: color!(0x89918D),

            hairline: color!(0x282E2A),
            hairline_strong: color!(0x667069),

            accent: color!(0x7CC4AE),
            accent_ink: color!(0x0B241E),
            accent_soft: color!(0x173A31),
            selection: color!(0x23453B),

            danger: color!(0xE2846D),
            danger_soft: color!(0x3A241F),
            warning: color!(0xD6A85C),
            success: color!(0x7DBB8C),

            scrim: Color::from_rgba(0.0, 0.0, 0.0, 0.58),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.55),

            category_fill: [
                color!(0x5D3B34), // Media
                color!(0x534A2D), // Documents
                color!(0x384127), // Code
                color!(0x21474F), // Caches
                color!(0x3D3E59), // Apps
                color!(0x513849), // Archives
                color!(0x33393E), // System
                color!(0x4A4238), // Other
            ],
            category_soft: [
                color!(0x46302A),
                color!(0x403A26),
                color!(0x2D3421),
                color!(0x1C383D),
                color!(0x303244),
                color!(0x3E2E39),
                color!(0x292E31),
                color!(0x39342D),
            ],
            category_ink: [
                color!(0xF1D5C9),
                color!(0xEFE2BD),
                color!(0xD6E8D3),
                color!(0xCFE9EC),
                color!(0xDFDCF4),
                color!(0xF2D6E1),
                color!(0xDBE1E4),
                color!(0xE8E2D4),
            ],
        }
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn category(&self, cat: Category) -> Color {
        self.category_fill[cat.id() as usize]
    }

    pub fn category_soft(&self, cat: Category) -> Color {
        self.category_soft[cat.id() as usize]
    }

    pub fn category_ink(&self, cat: Category) -> Color {
        self.category_ink[cat.id() as usize]
    }

    /// Ink to place directly on a solid category fill. Category fills are
    /// tuned so this is the same category ink used on neutral surfaces.
    pub fn on_category(&self, cat: Category) -> Color {
        self.category_ink(cat)
    }

    /// A 1.5 px containment outline for a parent group or category chip edge.
    pub fn category_line(&self, cat: Category) -> Color {
        let toward_fill = if self.is_dark {
            TREEMAP_PARENT_LINE_MIX_DARK
        } else {
            TREEMAP_PARENT_LINE_MIX_LIGHT
        };
        mix(self.category_ink(cat), self.category(cat), toward_fill)
    }

    /// Bridge to stock Iced widgets (text input, scrollable, buttons that do
    /// not carry an explicit style). Custom widgets should read the tokens.
    pub fn to_iced(self) -> iced::Theme {
        iced::Theme::custom(
            if self.is_dark {
                "space dark"
            } else {
                "space light"
            },
            iced::theme::Palette {
                background: self.bg,
                text: self.ink,
                primary: self.accent,
                success: self.success,
                warning: self.warning,
                danger: self.danger,
            },
        )
    }
}

/// Straight sRGB interpolation. Alpha is interpolated too, so callers should
/// pass opaque colors when they need an opaque result.
pub fn mix(from: Color, to: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

// ---------------------------------------------------------------------------
// Shared primitives
// ---------------------------------------------------------------------------

pub fn ui_font(weight: Weight) -> Font {
    Font {
        family: Family::Name(FONT_UI),
        weight,
        ..Font::DEFAULT
    }
}

pub fn mono_font(weight: Weight) -> Font {
    Font {
        family: Family::Name(FONT_MONO),
        weight,
        ..Font::DEFAULT
    }
}

/// Clip a label at a character budget and mark the elision explicitly.
pub fn truncated(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 1 {
        return "…".into();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Contrast guards
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(channel: f32) -> f32 {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(color: Color) -> f32 {
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast(a: Color, b: Color) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn themes_agree_on_appearance() {
        assert!(!Theme::light().is_dark);
        assert!(!Theme::light().dark);
        assert!(Theme::dark().is_dark);
        assert!(Theme::dark().dark);
        assert!(Theme::for_dark(true).is_dark);
        assert!(!Theme::for_dark(false).is_dark);
    }

    #[test]
    fn core_text_is_aa_on_all_neutral_beds() {
        for theme in [Theme::light(), Theme::dark()] {
            for bed in [theme.bg, theme.surface, theme.surface_sunken] {
                for color in [
                    theme.ink,
                    theme.ink_muted,
                    theme.ink_faint,
                    theme.accent,
                    theme.danger,
                    theme.warning,
                    theme.success,
                ] {
                    assert!(
                        contrast(color, bed) >= 4.5,
                        "text token misses AA: {color:?} on {bed:?} = {}",
                        contrast(color, bed)
                    );
                }
            }
        }
    }

    #[test]
    fn accent_and_danger_text_pairs_are_aa() {
        for theme in [Theme::light(), Theme::dark()] {
            assert!(contrast(theme.accent_ink, theme.accent) >= 4.5);
            assert!(contrast(theme.danger, theme.danger_soft) >= 4.5);
            assert!(contrast(theme.ink, theme.selection) >= 4.5);
        }
    }

    #[test]
    fn every_category_is_legible_on_fill_and_soft() {
        for theme in [Theme::light(), Theme::dark()] {
            for cat in Category::ALL {
                let ink = theme.category_ink(cat);
                assert_eq!(theme.on_category(cat), ink);
                assert!(
                    contrast(ink, theme.category(cat)) >= 4.5,
                    "category ink on fill misses AA: {cat:?} = {}",
                    contrast(ink, theme.category(cat))
                );
                assert!(
                    contrast(ink, theme.category_soft(cat)) >= 4.5,
                    "category ink on soft misses AA: {cat:?} = {}",
                    contrast(ink, theme.category_soft(cat))
                );
                for bed in [theme.bg, theme.surface, theme.surface_sunken] {
                    assert!(
                        contrast(ink, bed) >= 4.5,
                        "category ink on neutral bed misses AA: {cat:?} = {}",
                        contrast(ink, bed)
                    );
                }
            }
        }
    }

    #[test]
    fn motion_curve_is_bounded_and_monotonic() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        let mut previous = 0.0;
        for step in 0..=100 {
            let value = ease(step as f32 / 100.0);
            assert!(value >= previous - 1.0e-6);
            assert!((0.0..=1.0).contains(&value));
            previous = value;
        }
    }
}
