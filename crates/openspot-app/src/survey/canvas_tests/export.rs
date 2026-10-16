use super::*;
use image::Rgba;
use openspot_model::CropArea;

const WHITE: Rgba<u8> = Rgba([250, 250, 250, 255]);

/// Blank 20 x 10 m plan (1600 x 800 px, 80 px/m) with measurements in the left part.
fn survey() -> Survey {
    let mut s = Survey::default();
    s.create_blank_map(20.0, 10.0).unwrap();
    s.opacity = 255;
    s.show_points = false;
    for (x, y) in [(200.0, 200.0), (500.0, 200.0), (350.0, 500.0)] {
        s.add_scan(x, y, vec![ap(1, -45)]);
    }
    s
}

fn decode(s: &Survey) -> image::RgbaImage {
    image::load_from_memory(&s.render_png().unwrap()).unwrap().to_rgba8()
}

#[test]
fn export_has_plan_size_and_draws_the_heatmap_over_measured_areas() {
    let s = survey();
    let img = decode(&s);
    assert_eq!((img.width(), img.height()), (1600, 800));
    assert_ne!(img.get_pixel(350, 250), &WHITE, "heat over the measured area");
    // Far from any sample (beyond the automatic extension) the plan is untouched.
    assert_eq!(img.get_pixel(1500, 750), plain_plan().get_pixel(1500, 750));
}

fn plain_plan() -> image::RgbaImage {
    let mut p = Survey::default();
    p.create_blank_map(20.0, 10.0).unwrap();
    decode(&p)
}

#[test]
fn crop_keeps_the_export_clean_outside_the_area() {
    let mut s = survey();
    let before = decode(&s);
    let probe = (350, 250);
    assert_ne!(before.get_pixel(probe.0, probe.1), &WHITE);
    s.set_crop(Some(CropArea::rect((0.0, 0.0), (300.0, 800.0))));
    let after = decode(&s);
    assert_ne!(after.get_pixel(200, 200), &WHITE, "inside the crop");
    // Same pixel as the plain plan outside the crop.
    let plain = plain_plan();
    assert_eq!(after.get_pixel(probe.0, probe.1), plain.get_pixel(probe.0, probe.1));
}

#[test]
fn gray_out_appears_in_the_export_and_points_are_drawn_on_request() {
    let mut s = survey();
    s.set_gray_unmeasured(true);
    let img = decode(&s);
    let far = img.get_pixel(1500, 700);
    assert!(far[0].abs_diff(128) < 2 && far[1].abs_diff(128) < 2, "opaque gray: {far:?}");
    s.show_points = true;
    let with_points = decode(&s);
    // The marker is a 5 px disc whose outer 2 px ring is white; 4 px from the center is ring.
    assert_eq!(with_points.get_pixel(204, 200), &Rgba([255, 255, 255, 255]));
    assert_ne!(img.get_pixel(204, 200), &Rgba([255, 255, 255, 255]));
    assert_eq!(with_points.get_pixel(212, 200), img.get_pixel(212, 200), "nothing drawn beyond the marker");
}

#[test]
fn export_without_a_plan_is_an_error() {
    assert!(Survey::default().render_png().is_err());
}
