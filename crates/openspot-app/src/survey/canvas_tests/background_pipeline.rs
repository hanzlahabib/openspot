//! The canvas renders heatmaps on a background job; crop, gray-out, hidden-network
//! filtering and the influence radius (all from the survey-canvas features) must still be
//! applied to what that job returns.

use super::*;
use crate::survey::heat_job::HeatOutput;
use openspot_model::{CropArea, RadiusPreset};
use openspot_wlan::MacAddress;

fn survey() -> Survey {
    let mut s = Survey::default();
    s.create_blank_map(100.0, 50.0).unwrap();
    for (x, y) in [(200.0, 200.0), (400.0, 200.0), (300.0, 400.0)] {
        s.add_scan(x, y, vec![ap(1, -50)]);
    }
    s
}

/// Drives the job to completion the way the canvas does, one poll per "frame".
fn settle(s: &mut Survey) -> Option<HeatOutput> {
    for _ in 0..1000 {
        if let Some(result) = s.advance_heat() {
            return result;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("heat job did not finish in 10 s");
}

#[test]
fn background_result_has_the_crop_applied() {
    let mut s = survey();
    let full = settle(&mut s).expect("heatmap");
    let all = full.grid.values.iter().flatten().count();

    s.set_crop(Some(CropArea::rect((0.0, 0.0), (300.0, 600.0))));
    let cropped = settle(&mut s).expect("heatmap");
    let kept = cropped.grid.values.iter().flatten().count();
    assert!(kept > 0 && kept < all, "{kept} of {all}");
    // Transparent exactly where the grid is empty.
    for (cell, v) in cropped.grid.values.iter().enumerate() {
        assert_eq!(v.is_none(), cropped.rgba[cell * 4 + 3] == 0, "cell {cell}");
    }
}

#[test]
fn background_result_has_the_gray_out_applied_and_matches_the_synchronous_path() {
    let mut s = survey();
    s.set_gray_unmeasured(true);
    let out = settle(&mut s).expect("heatmap");
    assert!(out.rgba.chunks(4).any(|p| p[..3] == [128, 128, 128] && p[3] > 0), "no gray cells");
    // Same grid as the synchronous reference (which exports use), pixel for pixel.
    let sync = s.compute_grid().unwrap();
    assert_eq!(out.grid, sync);
    assert_eq!(out.rgba, s.heat_rgba(&sync));
}

#[test]
fn excluded_hidden_networks_no_longer_contribute_to_the_background_map() {
    let mut s = Survey::default();
    s.create_blank_map(50.0, 50.0).unwrap();
    let hidden = MacAddress([0, 0, 0, 0, 0, 9]);
    for (x, y) in [(100.0, 100.0), (300.0, 100.0), (200.0, 300.0)] {
        let ap = AccessPoint::from_beacon(String::new(), hidden, -55, 2_437_000, true, 100, vec![]);
        s.add_scan(x, y, vec![ap]);
    }
    let strongest = |out: HeatOutput| out.grid.values.iter().flatten().copied().fold(f32::MIN, f32::max);
    let shown = strongest(settle(&mut s).expect("hidden networks are shown by default"));
    assert!(shown > -60.0, "{shown}");

    // Excluded readings are dropped but the measurement points stay (so positions and counts
    // do not change): the map then reads "no signal" at them instead of disappearing.
    s.set_hidden_included(hidden, false);
    let excluded = strongest(settle(&mut s).expect("points are kept"));
    assert_eq!(excluded, openspot_heatmap::FLOOR_DBM as f32);
}

#[test]
fn a_bigger_radius_preset_reaches_further_in_the_background_job_too() {
    let mut s = survey();
    let default = settle(&mut s).expect("heatmap").grid.values.iter().flatten().count();
    s.set_radius_preset(Some(RadiusPreset::Extra));
    let wide = settle(&mut s).expect("heatmap").grid.values.iter().flatten().count();
    assert!(wide > default, "{wide} <= {default}");
}
