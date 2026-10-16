use openspot_heatmap::Grid;

use super::mask::GRAY_RGB;
use super::*;

fn grid(cols: usize, rows: usize, cell: f64) -> Grid {
    Grid { cols, rows, cell_px: cell, values: vec![Some(1.0); cols * rows] }
}

fn alpha_at(rgba: &[u8], cols: usize, col: usize, row: usize) -> u8 {
    rgba[(row * cols + col) * 4 + 3]
}

#[test]
fn rectangle_draft_completes_on_the_second_click_in_any_corner_order() {
    let mut d = CropDraft::default();
    assert_eq!(d.click((50.0, 60.0)), None);
    assert_eq!(d.points.len(), 1);
    assert_eq!(d.click((10.0, 20.0)), Some(CropArea::Rect { min: (10.0, 20.0), max: (50.0, 60.0) }));
    assert!(d.points.is_empty(), "ready for a new crop");
}

#[test]
fn polygon_draft_needs_three_vertices_to_finish() {
    let mut d = CropDraft::default();
    d.set_mode(CropMode::Polygon);
    assert_eq!(d.click((0.0, 0.0)), None);
    assert_eq!(d.click((10.0, 0.0)), None);
    assert_eq!(d.finish(), None);
    assert_eq!(d.points.len(), 2, "draft kept so the user can continue");
    d.click((0.0, 10.0));
    assert!(matches!(d.finish(), Some(CropArea::Polygon(p)) if p.len() == 3));
    assert!(d.points.is_empty());
}

#[test]
fn switching_mode_discards_the_draft_and_rect_mode_cannot_finish() {
    let mut d = CropDraft::default();
    d.click((1.0, 1.0));
    d.set_mode(CropMode::Polygon);
    assert!(d.points.is_empty());
    d.set_mode(CropMode::Rect);
    assert_eq!(d.finish(), None);
    d.click((1.0, 1.0));
    assert!(d.remove_last());
    assert!(!d.remove_last());
}

#[test]
fn crop_grid_empties_cells_whose_center_is_outside() {
    let mut g = grid(4, 4, 10.0);
    // Centers are at 5, 15, 25, 35. This rectangle covers the centers 15 and 25 only.
    crop_grid(&mut g, &CropArea::rect((10.0, 10.0), (30.0, 30.0)));
    for row in 0..4 {
        for col in 0..4 {
            let inside = (1..=2).contains(&col) && (1..=2).contains(&row);
            assert_eq!(g.get(col, row).is_some(), inside, "({col},{row})");
        }
    }
}

#[test]
fn crop_grid_with_polygon_uses_cell_centers() {
    let mut g = grid(4, 4, 10.0);
    let tri = CropArea::polygon(vec![(0.0, 0.0), (40.0, 0.0), (0.0, 40.0)]).unwrap();
    crop_grid(&mut g, &tri);
    assert!(g.get(0, 0).is_some() && g.get(1, 1).is_some());
    assert!(g.get(3, 3).is_none() && g.get(3, 1).is_none());
}

#[test]
fn gray_out_tints_only_cells_beyond_the_radius() {
    let g = grid(4, 1, 10.0);
    let mut rgba = vec![0u8; 4 * 4];
    // Sample at x=5 (first cell center), radius 10: cell 1 (center 15) is exactly on the
    // boundary and counts as measured; cell 2 (25) and 3 (35) are not.
    tint_unmeasured(&mut rgba, &g, None, GrayOut { samples: &[(5.0, 5.0)], radius_px: 10.0 }, 120);
    assert_eq!(alpha_at(&rgba, 4, 0, 0), 0);
    assert_eq!(alpha_at(&rgba, 4, 1, 0), 0);
    assert_eq!(alpha_at(&rgba, 4, 2, 0), 120);
    assert_eq!(&rgba[12..15], &GRAY_RGB);
}

#[test]
fn gray_out_without_samples_tints_everything_inside_the_crop_only() {
    let g = grid(4, 1, 10.0);
    let mut rgba = vec![0u8; 16];
    let crop = CropArea::rect((10.0, 0.0), (30.0, 10.0));
    tint_unmeasured(&mut rgba, &g, Some(&crop), GrayOut { samples: &[], radius_px: 10.0 }, 90);
    let alphas: Vec<u8> = (0..4).map(|c| alpha_at(&rgba, 4, c, 0)).collect();
    assert_eq!(alphas, [0, 90, 90, 0], "cells outside the crop stay transparent");
}

#[test]
fn gray_out_replaces_the_metric_color_of_unmeasured_cells() {
    let g = grid(2, 1, 10.0);
    let mut rgba = vec![200, 10, 10, 255, 200, 10, 10, 255];
    tint_unmeasured(&mut rgba, &g, None, GrayOut { samples: &[(5.0, 5.0)], radius_px: 3.0 }, 255);
    assert_eq!(&rgba[0..4], &[200, 10, 10, 255], "measured cell keeps its color");
    assert_eq!(&rgba[4..8], &[128, 128, 128, 255]);
}
