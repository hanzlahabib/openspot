use super::*;
use openspot_model::CropArea;

/// 100 x 50 m plan at 16 px/m, three points in the left half.
fn survey() -> Survey {
    let mut s = Survey::default();
    s.create_blank_map(100.0, 50.0).unwrap();
    for (x, y) in [(200.0, 200.0), (400.0, 200.0), (300.0, 400.0)] {
        s.add_scan(x, y, vec![ap(1, -50)]);
    }
    s
}

fn filled(s: &Survey) -> Vec<(usize, usize)> {
    let g = s.compute_grid().unwrap();
    (0..g.rows).flat_map(|r| (0..g.cols).map(move |c| (c, r))).filter(|&(c, r)| g.get(c, r).is_some()).collect()
}

#[test]
fn crop_limits_the_heatmap_and_hover_values() {
    let mut s = survey();
    let all = filled(&s).len();
    // Left part of the filled area only.
    s.set_crop(Some(CropArea::rect((0.0, 0.0), (300.0, 600.0))));
    let g = s.compute_grid().unwrap();
    let cropped = filled(&s);
    assert!(!cropped.is_empty() && cropped.len() < all);
    for (c, _) in cropped {
        let x = (c as f64 + 0.5) * g.cell_px;
        assert!(x <= 300.0, "cell centered at x={x} is outside the crop");
    }
}

#[test]
fn removing_the_crop_restores_the_full_heatmap() {
    let mut s = survey();
    let all = filled(&s).len();
    s.set_crop(Some(CropArea::rect((0.0, 0.0), (10.0, 10.0))));
    assert!(filled(&s).len() < all);
    s.set_crop(None);
    assert_eq!(filled(&s).len(), all);
}

#[test]
fn crop_and_gray_flag_dirty_the_project_and_the_cache_only_when_changed() {
    let mut s = survey();
    let key = s.heat_key();
    s.dirty = false;
    s.set_crop(None);
    s.set_gray_unmeasured(false);
    assert!(!s.dirty);
    s.set_gray_unmeasured(true);
    assert!(s.dirty);
    assert_ne!(s.heat_key(), key);
}

#[test]
fn rectangle_draft_through_the_survey_sets_the_crop() {
    let mut s = survey();
    s.crop_click(10.0, 10.0);
    assert!(s.crop().is_none());
    s.crop_click(110.0, 90.0);
    assert_eq!(s.crop(), Some(&CropArea::rect((10.0, 10.0), (110.0, 90.0))));
}

#[test]
fn polygon_draft_through_the_survey_sets_the_crop() {
    use crate::survey::crop::CropMode;
    let mut s = survey();
    s.crop_set_mode(CropMode::Polygon);
    s.crop_click(0.0, 0.0);
    s.crop_click(50.0, 0.0);
    assert!(!s.crop_finish_polygon());
    s.crop_click(0.0, 50.0);
    assert!(s.crop_finish_polygon());
    assert!(matches!(s.crop(), Some(CropArea::Polygon(p)) if p.len() == 3));
}

#[test]
fn gray_out_covers_far_cells_and_leaves_measured_ones_in_metric_color() {
    let mut s = survey();
    s.set_gray_unmeasured(true);
    let g = s.compute_grid().unwrap();
    let rgba = s.heat_rgba(&g);
    let px = |x: f64, y: f64| {
        let (c, r) = ((x / g.cell_px) as usize, (y / g.cell_px) as usize);
        let i = (r * g.cols + c) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let gray = [128, 128, 128, s.opacity];
    assert_ne!(px(250.0, 200.0), gray, "50 px from a sample, within the 96 px automatic radius");
    assert_eq!(px(300.0, 280.0), gray, "inside the hull but farther than the radius from every sample");
    assert_eq!(px(1500.0, 750.0), gray, "far corner of the plan is unmeasured");
    s.set_gray_unmeasured(false);
    assert_ne!(s.heat_rgba(&g)[((750.0 / g.cell_px) as usize * g.cols + (1500.0 / g.cell_px) as usize) * 4..][..4], gray);
}

#[test]
fn gray_out_radius_follows_the_data_point_radius_preset() {
    use openspot_model::RadiusPreset;
    let gray_cells = |preset| {
        let mut s = survey();
        s.set_radius_preset(Some(preset));
        s.set_gray_unmeasured(true);
        let g = s.compute_grid().unwrap();
        s.heat_rgba(&g).chunks(4).filter(|p| p[..3] == [128, 128, 128]).count()
    };
    assert!(gray_cells(RadiusPreset::Tiny) > gray_cells(RadiusPreset::Large));
}

#[test]
fn crop_and_gray_survive_save_and_open() {
    let mut s = survey();
    s.set_crop(Some(CropArea::rect((1.0, 2.0), (30.0, 40.0))));
    s.set_gray_unmeasured(true);
    let dir = std::env::temp_dir().join(format!("openspot-crop-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    s.save_to(&dir.join("c.ospot")).unwrap();
    let mut t = Survey::default();
    t.open_from(&dir.join("c.ospot")).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert_eq!(t.crop(), Some(&CropArea::rect((1.0, 2.0), (30.0, 40.0))));
    assert!(t.gray_unmeasured());
}
