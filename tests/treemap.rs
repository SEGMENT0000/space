//! Headless property tests for the pure treemap geometry module.
//!
//! No external RNG: a tiny deterministic LCG keeps failures reproducible.

use space::model::{Category, NO_NODE, NodeId};
use space::treemap::{self, Camera, Item, Rect, Tile};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(
            seed.wrapping_mul(2_862_933_555_777_941_757)
                .wrapping_add(3_037_000_493),
        )
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | self.next_u32() as u64
    }

    /// Inclusive range. `hi` must be small enough that `hi - lo + 1` fits.
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        assert!(lo <= hi);
        lo + self.next_u32() % (hi - lo + 1)
    }

    fn f32(&mut self, lo: f32, hi: f32) -> f32 {
        let t = self.next_u32() as f32 / u32::MAX as f32;
        lo + (hi - lo) * t
    }
}

fn item(id: NodeId, size: u64) -> Item {
    Item {
        id,
        size,
        cat: Category::from_id((id % 8) as u8),
        is_dir: id.is_multiple_of(3),
    }
}

fn random_items(rng: &mut Lcg, n: usize) -> Vec<Item> {
    (0..n)
        .map(|i| {
            let size = match rng.range(0, 11) {
                0 => 0,
                1 => u64::MAX - rng.range(0, 4) as u64,
                2 => 1u64 << rng.range(0, 62),
                3 => rng.next_u64(),
                _ => rng.range(1, 2_000_000) as u64,
            };
            item(i as NodeId + 1, size)
        })
        .collect()
}

fn expected_tiles(items: &[Item]) -> Vec<Item> {
    let mut out: Vec<Item> = items
        .iter()
        .copied()
        .filter(|i| i.size > 0 && i.id != NO_NODE)
        .collect();
    out.sort_by_key(|item| std::cmp::Reverse(item.size));
    out
}

fn check_geometry(area: Rect, tiles: &[Tile], eps: f32) {
    let right = area.right();
    let bottom = area.bottom();
    assert!(area.x.is_finite() && area.y.is_finite() && area.w > 0.0 && area.h > 0.0);
    for (i, t) in tiles.iter().enumerate() {
        let r = t.rect;
        assert!(
            r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite(),
            "tile {i} is not finite: {r:?}"
        );
        assert!(
            r.w >= 0.0 && r.h >= 0.0,
            "tile {i} has negative extent: {r:?}"
        );
        assert!(r.x >= area.x - eps, "tile {i} left oob: {r:?} in {area:?}");
        assert!(r.y >= area.y - eps, "tile {i} top oob: {r:?} in {area:?}");
        assert!(
            r.right() <= right + eps,
            "tile {i} right oob: {r:?} in {area:?}"
        );
        assert!(
            r.bottom() <= bottom + eps,
            "tile {i} bottom oob: {r:?} in {area:?}"
        );
    }
    for i in 0..tiles.len() {
        for j in i + 1..tiles.len() {
            let a = tiles[i].rect;
            let b = tiles[j].rect;
            let ox = a.right().min(b.right()) - a.x.max(b.x);
            let oy = a.bottom().min(b.bottom()) - a.y.max(b.y);
            assert!(
                ox <= eps || oy <= eps,
                "tiles {i} and {j} overlap by ({ox}, {oy}): {a:?} {b:?}"
            );
        }
    }
}

fn check_order_and_payload(items: &[Item], tiles: &[Tile]) {
    let expected = expected_tiles(items);
    assert_eq!(tiles.len(), expected.len());
    for (t, e) in tiles.iter().zip(expected.iter()) {
        assert_eq!(t.id, e.id);
        assert_eq!(t.size, e.size);
        assert_eq!(t.cat, e.cat);
        assert_eq!(t.is_dir, e.is_dir);
    }
    for pair in tiles.windows(2) {
        assert!(
            pair[0].size >= pair[1].size,
            "output is not descending: {} then {}",
            pair[0].size,
            pair[1].size
        );
    }
}

fn ulp_f32(v: f32) -> f32 {
    let a = v.abs();
    if !a.is_finite() || a == f32::MAX {
        return 0.0;
    }
    if a == 0.0 {
        return f32::from_bits(1);
    }
    f32::from_bits(a.to_bits() + 1) - a
}

fn check_area_proportions(area: Rect, items: &[Item], tiles: &[Tile]) {
    let expected = expected_tiles(items);
    let total: u128 = expected.iter().map(|i| i.size as u128).sum();
    if total == 0 {
        assert!(tiles.is_empty());
        return;
    }
    let area_px = area.w as f64 * area.h as f64;
    let mut actual_total = 0.0f64;
    for (t, e) in tiles.iter().zip(expected.iter()) {
        let want = (e.size as f64 / total as f64) * area_px;
        let got = t.rect.w as f64 * t.rect.h as f64;
        actual_total += got;
        // Absolute pixel resolution limits how exactly a very thin f32 rect
        // can represent its share; include that quantisation in the budget.
        let ex = ulp_f32(t.rect.x.abs().max(t.rect.right().abs()));
        let ey = ulp_f32(t.rect.y.abs().max(t.rect.bottom().abs()));
        let quant = (t.rect.h * ex + t.rect.w * ey + ex * ey) as f64;
        let tol = (want * 0.02).max(1e-4).max(quant * 2.0);
        assert!(
            (got - want).abs() <= tol,
            "tile {} area {got} differs from {want} (tol {tol}, rect {:?})",
            t.id,
            t.rect
        );
    }
    let tol = area_px * 0.002 + 1e-3;
    assert!(
        (actual_total - area_px).abs() <= tol,
        "total tile area {actual_total} differs from {area_px}"
    );
}

#[test]
fn empty_single_zero_and_sentinel() {
    let a = Rect::new(0.0, 0.0, 320.0, 200.0);
    assert!(treemap::squarify(a, &[], 2.0).is_empty());
    assert!(treemap::squarify(a, &[item(1, 0)], 2.0).is_empty());
    assert!(treemap::squarify(a, &[item(NO_NODE, 10)], 2.0).is_empty());

    let tiles = treemap::squarify(a, &[item(7, 42)], 0.0);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].id, 7);
    assert!((tiles[0].rect.area() - a.area()).abs() < 0.01);

    let mixed = [
        item(1, 0),
        item(2, 5),
        item(NO_NODE, 100),
        item(3, 0),
        item(4, 1),
    ];
    let tiles = treemap::squarify(a, &mixed, 0.0);
    assert_eq!(tiles.len(), 2);
    assert_eq!(tiles[0].id, 2);
    assert_eq!(tiles[1].id, 4);
}

#[test]
fn equal_sizes_make_readable_cells_instead_of_thin_strips() {
    let area = Rect::new(0.0, 0.0, 800.0, 800.0);
    let items: Vec<_> = (0..64).map(|id| item(id, 1_000)).collect();
    let tiles = treemap::squarify(area, &items, 0.0);
    check_geometry(area, &tiles, 0.01);
    check_area_proportions(area, &items, &tiles);
    for tile in tiles {
        let aspect = tile.rect.w.max(tile.rect.h) / tile.rect.w.min(tile.rect.h);
        assert!(
            aspect <= 1.2,
            "equal file sizes should make compact cells: {aspect}"
        );
        assert!(tile.rect.w >= 90.0 && tile.rect.h >= 90.0);
    }
}

#[test]
fn duplicate_sizes_keep_input_order() {
    let items: Vec<Item> = (1..=50).map(|id| item(id, 7)).collect();
    let a = Rect::new(0.0, 0.0, 800.0, 500.0);
    let tiles = treemap::squarify(a, &items, 0.0);
    assert_eq!(tiles.len(), items.len());
    for (t, e) in tiles.iter().zip(items.iter()) {
        assert_eq!(t.id, e.id);
    }
    let again = treemap::squarify(a, &items, 0.0);
    for (x, y) in tiles.iter().zip(again.iter()) {
        assert_eq!(x.id, y.id);
        assert_eq!(x.rect.x.to_bits(), y.rect.x.to_bits());
        assert_eq!(x.rect.y.to_bits(), y.rect.y.to_bits());
        assert_eq!(x.rect.w.to_bits(), y.rect.w.to_bits());
        assert_eq!(x.rect.h.to_bits(), y.rect.h.to_bits());
    }
}

#[test]
fn huge_u64_sizes_use_u128_total() {
    let items = [
        item(1, u64::MAX),
        item(2, u64::MAX),
        item(3, u64::MAX - 1),
        item(4, u64::MAX / 3),
        item(5, 1),
        item(6, 0),
    ];
    let a = Rect::new(-20.0, 15.0, 900.0, 400.0);
    let tiles = treemap::squarify(a, &items, 0.0);
    assert_eq!(tiles.len(), 5);
    check_order_and_payload(&items, &tiles);
    check_geometry(a, &tiles, 0.01);
    check_area_proportions(a, &items, &tiles);
}

#[test]
fn gutter_never_escapes_and_never_negative() {
    let a = Rect::new(10.0, 20.0, 300.0, 180.0);
    let items: Vec<Item> = (1..=30).map(|i| item(i, i as u64)).collect();
    for gutter in [0.0, 1.0, 6.0, 300.0, 1e9, f32::INFINITY, f32::NAN, -4.0] {
        let tiles = treemap::squarify(a, &items, gutter);
        assert_eq!(tiles.len(), items.len(), "gutter {gutter}");
        check_geometry(a, &tiles, 0.02);
        for t in &tiles {
            assert!(
                t.rect.w >= 0.0 && t.rect.h >= 0.0,
                "gutter {gutter}: {:?}",
                t.rect
            );
        }
    }
}

#[test]
fn tiny_and_invalid_areas_are_safe() {
    let items: Vec<Item> = (1..=20).map(|i| item(i, i as u64)).collect();
    for (w, h) in [(0.5, 0.5), (1e-6, 1e-6), (0.001, 50.0), (50.0, 0.001)] {
        let a = Rect::new(0.0, 0.0, w, h);
        let tiles = treemap::squarify(a, &items, 0.0);
        assert_eq!(tiles.len(), items.len());
        check_geometry(a, &tiles, 0.0001);
    }
    for a in [
        Rect::new(0.0, 0.0, 0.0, 100.0),
        Rect::new(0.0, 0.0, 100.0, 0.0),
        Rect::new(0.0, 0.0, -10.0, 100.0),
        Rect::new(f32::NAN, 0.0, 100.0, 100.0),
        Rect::new(0.0, 0.0, f32::INFINITY, 100.0),
        Rect::new(3.0e38, 0.0, 3.0e38, 100.0),
    ] {
        assert!(treemap::squarify(a, &items, 0.0).is_empty(), "area {a:?}");
    }
}

#[test]
fn pathological_aspect_ratios() {
    for (w, h) in [(1000.0, 3.0), (3.0, 1000.0), (2000.0, 1.0), (1.0, 2000.0)] {
        let a = Rect::new(0.0, 0.0, w, h);
        let mut rng = Lcg::new((w as u64) * 31 + h as u64);
        let items = random_items(&mut rng, 150);
        let tiles = treemap::squarify(a, &items, 0.0);
        check_order_and_payload(&items, &tiles);
        check_geometry(a, &tiles, 0.01);
        check_area_proportions(a, &items, &tiles);
    }
}

#[test]
fn randomized_geometry_and_proportions() {
    for seed in 0..60u64 {
        let mut rng = Lcg::new(seed);
        let n = rng.range(0, 140) as usize;
        let items = random_items(&mut rng, n);
        let a = Rect::new(
            rng.range(0, 50) as f32,
            rng.range(0, 50) as f32,
            rng.range(20, 1500) as f32,
            rng.range(20, 1000) as f32,
        );
        let gutter = if seed % 2 == 0 {
            0.0
        } else {
            rng.f32(0.0, 12.0)
        };
        let tiles = treemap::squarify(a, &items, gutter);
        check_order_and_payload(&items, &tiles);
        check_geometry(a, &tiles, 0.02);
        if gutter == 0.0 {
            check_area_proportions(a, &items, &tiles);
        }
    }
}

#[test]
fn layout_children_is_nested_and_bounded() {
    let a = Rect::new(0.0, 0.0, 1000.0, 700.0);
    let parents = treemap::squarify(a, &[item(1, 800), item(2, 200)], 2.0);
    let children: Vec<Item> = (10..60).map(|i| item(i, (i * 11) as u64)).collect();
    for parent in &parents {
        let kids = treemap::layout_children(parent, &children, 1.0);
        assert_eq!(kids.len(), children.len());
        check_geometry(parent.rect, &kids, 0.02);
    }
}

#[test]
fn ten_thousand_items_performance_smoke() {
    let mut rng = Lcg::new(0x0005_EED1_0000);
    let items: Vec<Item> = (0..10_000)
        .map(|i| item(i as NodeId + 1, rng.range(1, 50_000_000) as u64))
        .collect();
    let a = Rect::new(0.0, 0.0, 1600.0, 900.0);
    let start = Instant::now();
    let tiles = treemap::squarify(a, &items, 1.0);
    let elapsed = start.elapsed();
    assert_eq!(tiles.len(), items.len());
    check_geometry(a, &tiles, 0.05);
    assert!(
        elapsed < Duration::from_secs(2),
        "10k squarify took {elapsed:?}"
    );
}

#[test]
fn camera_refit_identity_and_transform() {
    let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
    let cam = Camera::default();
    assert!(cam.is_identity());
    let r = Rect::new(10.0, 20.0, 100.0, 50.0);
    assert_eq!(cam.transform(r), r);
    assert_eq!(cam.transform(vp), vp);
    assert_eq!(cam.content_point(123.5, 456.25), (123.5, 456.25));
}

#[test]
fn camera_inverse_round_trip_is_exact() {
    let vp = Rect::new(0.0, 0.0, 1000.0, 700.0);
    let mut rng = Lcg::new(0xCAFE);
    for _ in 0..40 {
        let mut cam = Camera::default();
        for _ in 0..3 {
            cam.zoom_at(
                rng.f32(1.05, 2.5),
                (rng.f32(0.0, vp.w), rng.f32(0.0, vp.h)),
                vp,
                32.0,
            );
            cam.pan(rng.f32(-120.0, 120.0), rng.f32(-120.0, 120.0), vp);
        }
        for _ in 0..40 {
            let p = (rng.f32(0.0, vp.w), rng.f32(0.0, vp.h));
            let (cx, cy) = cam.content_point(p.0, p.1);
            let back = cam.transform(Rect::new(cx, cy, 0.0, 0.0));
            assert!(
                (back.x - p.0).abs() <= 0.02 && (back.y - p.1).abs() <= 0.02,
                "inverse drift p={p:?} back={:?} cam={cam:?}",
                (back.x, back.y)
            );
        }
        for t in treemap::squarify(vp, &[item(1, 3), item(2, 1)], 0.0) {
            let st = cam.transform(t.rect);
            let (cx, cy) = cam.content_point(st.x + st.w * 0.5, st.y + st.h * 0.5);
            assert!((cx - (t.rect.x + t.rect.w * 0.5)).abs() <= 0.03);
            assert!((cy - (t.rect.y + t.rect.h * 0.5)).abs() <= 0.03);
        }
    }
}

#[test]
fn anchored_zoom_keeps_cursor_point_fixed() {
    let vp = Rect::new(0.0, 0.0, 1000.0, 800.0);
    let mut cam = Camera::default();
    let anchor = (500.0, 400.0);
    cam.zoom_at(2.0, anchor, vp, 16.0);
    assert!((cam.scale - 2.0).abs() < 1e-6);
    assert!((cam.offset_x + 500.0).abs() < 0.01);
    assert!((cam.offset_y + 400.0).abs() < 0.01);
    let fixed = cam.content_point(anchor.0, anchor.1);
    assert!((fixed.0 - 500.0).abs() < 0.01 && (fixed.1 - 400.0).abs() < 0.01);

    // Interior anchor, result stays inside the allowed pan range.
    let anchor = (350.0, 300.0);
    let before = cam.content_point(anchor.0, anchor.1);
    cam.zoom_at(1.5, anchor, vp, 16.0);
    let after = cam.content_point(anchor.0, anchor.1);
    assert!((cam.scale - 3.0).abs() < 1e-5);
    assert!((before.0 - after.0).abs() < 0.01 && (before.1 - after.1).abs() < 0.01);
}

#[test]
fn camera_clamps_scale_and_never_exposes_background() {
    let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut rng = Lcg::new(0xBEEF);
    let mut cam = Camera::default();
    for _ in 0..100 {
        cam.zoom_at(
            rng.f32(0.2, 4.0),
            (rng.f32(0.0, vp.w), rng.f32(0.0, vp.h)),
            vp,
            24.0,
        );
        cam.pan(rng.f32(-5000.0, 5000.0), rng.f32(-5000.0, 5000.0), vp);
        assert!(cam.scale >= 1.0 && cam.scale <= treemap::MAX_SCALE + 1e-5);
        assert!(cam.offset_x.is_finite() && cam.offset_y.is_finite());
        assert!(cam.offset_x <= 0.01, "left edge exposed: {cam:?}");
        assert!(cam.offset_y <= 0.01, "top edge exposed: {cam:?}");
        assert!(
            cam.offset_x >= vp.w * (1.0 - cam.scale) - 0.02,
            "right edge exposed: {cam:?}"
        );
        assert!(
            cam.offset_y >= vp.h * (1.0 - cam.scale) - 0.02,
            "bottom edge exposed: {cam:?}"
        );
        let content = cam.transform(vp);
        assert!(content.x <= 0.02 && content.y <= 0.02);
        assert!(content.right() >= vp.right() - 0.02);
        assert!(content.bottom() >= vp.bottom() - 0.02);
    }

    cam.zoom_at(1000.0, (400.0, 300.0), vp, 8.0);
    assert!((cam.scale - 8.0).abs() < 1e-4);
    cam.zoom_at(0.0001, (400.0, 300.0), vp, 8.0);
    assert!((cam.scale - 1.0).abs() < 1e-6);
    assert!(cam.is_identity());
}

#[test]
fn camera_rejects_non_finite_input_without_panicking() {
    let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut cam = Camera::default();
    cam.zoom_at(f32::NAN, (10.0, 10.0), vp, 8.0);
    cam.zoom_at(2.0, (f32::INFINITY, 10.0), vp, 8.0);
    cam.pan(f32::INFINITY, f32::NAN, vp);
    cam.clamp(Rect::new(0.0, 0.0, f32::NAN, 600.0));
    assert!(cam.scale.is_finite());
    assert!(cam.offset_x.is_finite() && cam.offset_y.is_finite());
    assert!(cam.scale >= 1.0);
}

#[test]
fn hit_test_matches_transformed_geometry() {
    let vp = Rect::new(0.0, 0.0, 900.0, 600.0);
    let mut rng = Lcg::new(0x417);
    let items: Vec<Item> = (1..=80)
        .map(|i| item(i, rng.range(1, 100_000) as u64))
        .collect();
    let tiles = treemap::squarify(vp, &items, 1.0);
    let mut cam = Camera::default();
    cam.zoom_at(2.5, (450.0, 300.0), vp, 32.0);
    cam.pan(-80.0, 40.0, vp);

    for (i, t) in tiles.iter().enumerate() {
        let st = cam.transform(t.rect);
        if st.w > 0.5 && st.h > 0.5 {
            let p = (st.x + st.w * 0.5, st.y + st.h * 0.5);
            assert_eq!(treemap::hit_test(&tiles, &cam, p), Some(i), "tile {i}");
        }
    }

    for _ in 0..500 {
        let p = (rng.f32(-50.0, vp.w + 50.0), rng.f32(-50.0, vp.h + 50.0));
        let got = treemap::hit_test(&tiles, &cam, p);
        let contains = |r: Rect, eps: f32| {
            p.0 >= r.x - eps && p.0 < r.right() + eps && p.1 >= r.y - eps && p.1 < r.bottom() + eps
        };
        let expanded = tiles
            .iter()
            .rposition(|t| contains(cam.transform(t.rect), 0.05));
        let strict = tiles
            .iter()
            .rposition(|t| contains(cam.transform(t.rect), -0.05));
        if let Some(i) = got {
            assert!(
                expanded.is_some(),
                "hit {i} not near any transformed tile at {p:?}"
            );
            assert!(contains(cam.transform(tiles[i].rect), 0.05));
        } else {
            assert_eq!(strict, None, "missed interior point {p:?} cam {cam:?}");
        }
        if expanded == strict {
            assert_eq!(got, expanded, "point {p:?} cam {cam:?}");
        }
    }
    let content = cam.transform(vp);
    let outside = (content.x - 20.0, content.y - 20.0);
    assert_eq!(treemap::hit_test(&tiles, &cam, outside), None);
    assert_eq!(treemap::hit_test(&tiles, &cam, (f32::NAN, 1.0)), None);
}

#[test]
fn labels_are_one_per_tile_and_inside_bounds() {
    let a = Rect::new(0.0, 0.0, 1000.0, 700.0);
    let mut rng = Lcg::new(0x1ABE1);
    let items: Vec<Item> = (1..=60)
        .map(|i| item(i, rng.range(1, 50_000) as u64))
        .collect();
    let tiles = treemap::squarify(a, &items, 0.0);
    let names = |id: NodeId| format!("node-{id}-with-a-deliberately-long-name.txt");
    let labels = treemap::labels(&tiles, names);
    assert!(!labels.is_empty());
    let mut seen = BTreeSet::new();
    for l in &labels {
        assert!(seen.insert(l.tile), "duplicate label for tile {}", l.tile);
        let r = tiles[l.tile].rect;
        assert!(!l.text.is_empty());
        assert!(l.size > 0.0 && l.max_width >= 0.0 && l.max_height >= 0.0);
        assert!(l.x >= r.x - 0.01 && l.y >= r.y - 0.01);
        assert!(l.x + l.max_width <= r.right() + 0.01);
        assert!(l.y + l.max_height <= r.bottom() + 0.01);
        let w = treemap::estimate_text_width(&l.text, l.size);
        assert!(
            l.x + w <= r.right() + 0.01,
            "label {:?} width {w} escapes {:?}",
            l.text,
            r
        );
        assert!(l.y + l.size <= r.bottom() + 0.01);
        if l.truncated {
            assert!(l.text.ends_with('…'));
        } else {
            assert!(!l.text.contains('…'));
        }
    }
}

#[test]
fn labels_truncate_unicode_by_displayed_chars() {
    let a = Rect::new(0.0, 0.0, 260.0, 120.0);
    let tiles = treemap::squarify(a, &[item(1, 1)], 0.0);
    let long = "日本語のファイル名をとても長くしてみるテスト".repeat(4);
    let labels = treemap::labels(&tiles, |_| long.as_str());
    assert_eq!(labels.len(), 1);
    let l = &labels[0];
    assert!(l.truncated);
    let chars: Vec<char> = l.text.chars().collect();
    assert!(chars.len() < long.chars().count());
    assert_eq!(chars.last().copied(), Some('…'));
    assert_eq!(l.text.matches('…').count(), 1);
    let w = treemap::estimate_text_width(&l.text, l.size);
    assert!(l.x + w <= tiles[0].rect.right() + 0.01);
}

#[test]
fn labels_never_split_combining_or_emoji_clusters() {
    let a = Rect::new(0.0, 0.0, 150.0, 60.0);
    let tiles = treemap::squarify(a, &[item(1, 1)], 0.0);
    let names = [
        "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}",
        "👨\u{200D}👩\u{200D}👧\u{200D}👦 family family family",
        "❤\u{FE0F} 🇺🇸 🇯🇵 🇩🇪 🇫🇷 🇮🇹 🇪🇸 🇨🇦 🇬🇧",
        "👍🏽👍🏽👍🏽👍🏽👍🏽👍🏽👍🏽👍🏽👍🏽👍🏽",
    ];
    for name in names {
        let labels = treemap::labels(&tiles, |_| name);
        for l in &labels {
            assert!(l.text.ends_with('…') || l.text == name);
            if l.truncated {
                assert!(!l.text.ends_with('\u{200D}'));
                assert!(!l.text.ends_with('\u{FE0F}'));
                if let Some(before) = l.text.chars().rev().nth(1) {
                    let cp = before as u32;
                    assert!(
                        !(0x1F3FB..=0x1F3FF).contains(&cp),
                        "dangling skin tone: {:?}",
                        l.text
                    );
                }
            }
            assert!(
                l.x + treemap::estimate_text_width(&l.text, l.size) <= tiles[0].rect.right() + 0.01
            );
        }
    }
}

#[test]
fn labels_sanitize_control_chars_and_hide_tiny_tiles() {
    let a = Rect::new(0.0, 0.0, 300.0, 100.0);
    let tiles = treemap::squarify(a, &[item(1, 1)], 0.0);
    let labels = treemap::labels(&tiles, |_| "bad\nname\twith\u{0}control\u{202E}chars");
    assert_eq!(labels.len(), 1);
    let text = &labels[0].text;
    assert!(!text.chars().any(|c| c.is_control()));
    assert!(!text.contains('\u{202E}'));
    assert!(!text.starts_with(' ') && !text.ends_with(' '));
    assert!(!text.contains("  "));

    // Many equal items force sub-label-sized tiles.
    let items: Vec<Item> = (1..=400).map(|i| item(i, 1)).collect();
    let small = treemap::squarify(Rect::new(0.0, 0.0, 500.0, 400.0), &items, 1.0);
    let labels = treemap::labels(&small, |id| format!("entry-{id}"));
    for l in &labels {
        let r = small[l.tile].rect;
        assert!(r.w >= 42.0 && r.h >= 26.0, "label on tiny tile {:?}", r);
    }
}

#[test]
fn labels_accept_borrowed_and_owned_names() {
    let tiles = treemap::squarify(Rect::new(0.0, 0.0, 400.0, 200.0), &[item(1, 1)], 0.0);
    let borrowed = treemap::labels(&tiles, |_| "borrowed");
    let owned = treemap::labels(&tiles, |_| String::from("owned"));
    assert_eq!(borrowed.len(), 1);
    assert_eq!(owned.len(), 1);
    assert_eq!(borrowed[0].text, "borrowed");
    assert_eq!(owned[0].text, "owned");
    assert_eq!(treemap::estimate_text_width("", 12.0), 0.0);
    assert!(
        treemap::estimate_text_width("MMMM", 12.0) > treemap::estimate_text_width("iiii", 12.0)
    );
}
