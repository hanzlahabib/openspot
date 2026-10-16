use super::*;
use crate::prefs::SurveyPrefs;
use crate::survey::{ApMarkerSize, SampleDetail};

#[test]
fn visibility_settings_round_trip_through_prefs() {
    let s = Survey {
        ap_marker_size: ApMarkerSize::Large,
        show_ap_titles: false,
        show_path: false,
        show_floor_plan: false,
        sample_detail: SampleDetail::Detailed,
        ..Survey::default()
    };
    let p = s.prefs();
    assert_eq!((p.ap_marker_size, p.sample_detail), (2, 1));
    let mut t = Survey::default();
    t.apply_prefs(&p);
    assert_eq!(t.ap_marker_size, ApMarkerSize::Large);
    assert!(!t.show_ap_titles && !t.show_path && !t.show_floor_plan);
    assert_eq!(t.sample_detail, SampleDetail::Detailed);
}

#[test]
fn defaults_show_everything_as_before() {
    let s = Survey::default();
    assert!(s.show_floor_plan && s.show_path && s.show_ap_titles && s.show_points && s.show_ap_markers);
    assert_eq!((s.ap_marker_size, s.sample_detail), (ApMarkerSize::Medium, SampleDetail::Simple));
}

#[test]
fn out_of_range_indices_fall_back_to_defaults() {
    let mut s = Survey::default();
    s.apply_prefs(&SurveyPrefs { ap_marker_size: 99, sample_detail: 99, ..SurveyPrefs::default() });
    assert_eq!((s.ap_marker_size, s.sample_detail), (ApMarkerSize::Medium, SampleDetail::Simple));
}

#[test]
fn hiding_the_floor_plan_blanks_the_export_background() {
    let mut s = Survey::default();
    s.create_blank_map(20.0, 10.0).unwrap();
    let with_plan = image::load_from_memory(&s.render_png().unwrap()).unwrap().to_rgba8();
    assert_ne!(with_plan.get_pixel(0, 0), &image::Rgba([255, 255, 255, 255]), "grid line at the origin");
    s.show_floor_plan = false;
    let blank = image::load_from_memory(&s.render_png().unwrap()).unwrap().to_rgba8();
    assert!(blank.pixels().all(|p| p == &image::Rgba([255, 255, 255, 255])));
}
