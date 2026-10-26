//! Headless tests for the treemap draw list and shader widget API.
//!
//! Nothing here creates a GPU device; the tests only exercise data
//! construction, culling, bounds, and color packing.

use space::render::{DrawList, DrawTile, TreemapProgram, TreemapShader, srgb_to_linear};
use space::treemap::Rect;
use std::sync::Arc;

fn tile(x: f32, y: f32, w: f32, h: f32) -> DrawTile {
    DrawTile::new(Rect::new(x, y, w, h), [0.5, 0.5, 0.5, 1.0])
}

#[test]
fn empty_draw_list_has_no_bounds_or_visible_tiles() {
    let draw = DrawList::default();

    assert!(draw.is_empty());
    assert_eq!(draw.content_bounds(), Rect::ZERO);
    assert_eq!(draw.visible_tiles((800.0, 600.0)), 0);
    assert_eq!(draw.visible_tiles((0.0, 0.0)), 0);
}

#[test]
fn content_bounds_unions_valid_tiles() {
    let draw = DrawList::new(vec![
        tile(10.0, 20.0, 30.0, 40.0),
        tile(-5.0, 5.0, 10.0, 10.0),
        // Degenerate tiles are ignored by the union.
        tile(100.0, 100.0, 0.0, 10.0),
        tile(200.0, 0.0, 10.0, 10.0),
    ]);

    let bounds = draw.content_bounds();
    assert_eq!(bounds.x, -5.0);
    assert_eq!(bounds.y, 0.0);
    assert_eq!(bounds.right(), 210.0);
    assert_eq!(bounds.bottom(), 60.0);
}

#[test]
fn from_tiles_records_size_and_preserves_order() {
    let draw = DrawList::from_tiles(
        (640.0, 480.0),
        vec![tile(0.0, 0.0, 10.0, 10.0), tile(20.0, 20.0, 5.0, 5.0)],
    );

    assert_eq!(draw.size, (640.0, 480.0));
    assert_eq!(draw.tiles.len(), 2);
    assert_eq!(draw.tiles[1].rect, Rect::new(20.0, 20.0, 5.0, 5.0));
}

#[test]
fn srgb_colors_are_linearized_with_alpha_unchanged() {
    let linear = srgb_to_linear([0.0, 0.5, 1.0, 0.25]);

    assert!(linear[0].abs() < 1e-6);
    assert!((linear[1] - 0.214_041_14).abs() < 1e-5);
    assert!((linear[2] - 1.0).abs() < 1e-6);
    assert!((linear[3] - 0.25).abs() < 1e-6);

    // The linear segment of the sRGB transfer function.
    let dark = srgb_to_linear([0.04, 0.0, 0.0, 1.0]);
    assert!((dark[0] - 0.04 / 12.92).abs() < 1e-7);
}

#[test]
fn color_packing_is_defensive() {
    let packed = srgb_to_linear([-1.0, 2.0, f32::NAN, 1.5]);

    assert_eq!(packed, [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn zero_sized_canvas_and_degenerate_tiles_are_culled() {
    let draw = DrawList::new(vec![
        tile(0.0, 0.0, 100.0, 100.0),
        DrawTile::new(Rect::new(f32::NAN, 0.0, 10.0, 10.0), [1.0; 4]),
        tile(0.0, 0.0, 0.0, 10.0),
        tile(500.0, 500.0, 10.0, 10.0),
    ]);

    assert_eq!(draw.visible_tiles((0.0, 0.0)), 0);
    assert_eq!(draw.visible_tiles((-1.0, 100.0)), 0);
    assert_eq!(draw.visible_tiles((f32::NAN, 100.0)), 0);
    assert_eq!(draw.visible_tiles((100.0, 100.0)), 1);
}

#[test]
fn partially_visible_tiles_stay_visible() {
    // Straddles the left edge and the bottom-right corner.
    let draw = DrawList::new(vec![
        tile(-20.0, 10.0, 30.0, 30.0),
        tile(90.0, 90.0, 40.0, 40.0),
        tile(-30.0, 10.0, 30.0, 30.0),
    ]);

    assert_eq!(draw.visible_tiles((100.0, 100.0)), 2);
}

#[test]
fn draw_tile_defaults_are_transparent_and_square() {
    let tile = DrawTile::new(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);

    assert_eq!(tile.border, [0.0; 4]);
    assert_eq!(tile.border_width, 0.0);
    assert_eq!(tile.radius, 0.0);
}

#[test]
fn program_can_swap_draw_lists_and_builds_a_shader_widget() {
    let first = Arc::new(DrawList::new(vec![]));
    let second = Arc::new(DrawList::from_tiles(
        (320.0, 200.0),
        vec![tile(0.0, 0.0, 320.0, 200.0)],
    ));

    let mut program = TreemapProgram::new(Arc::clone(&first));
    assert!(Arc::ptr_eq(program.draw_list(), &first));

    program.set_draw_list(Arc::clone(&second));
    assert!(Arc::ptr_eq(program.draw_list(), &second));

    let replaced = program.with_draw_list(Arc::clone(&first));
    assert!(Arc::ptr_eq(replaced.draw_list(), &first));

    // This is the exact construction the UI uses. It must type-check without
    // a renderer and must work with any application message type.
    let _widget: TreemapShader<()> = iced::widget::shader(TreemapProgram::new(Arc::clone(&second)));

    let _typed = space::render::treemap::<()>(Arc::clone(&second));
}
