pub mod engine;
pub mod model;
pub mod preview;
pub mod render;
pub mod treemap;
pub mod ui;

pub use model::*;

/// Environment variable used by the review harness to deep-link a screen.
pub const SCREEN_ENV: &str = "SPACE_SCREEN";
/// Environment variable used by the review harness to preselect a scan target.
pub const SCAN_ENV: &str = "SPACE_SCAN";
/// Environment variable used by the review harness to force a theme.
pub const THEME_ENV: &str = "SPACE_THEME";
