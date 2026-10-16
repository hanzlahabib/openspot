use super::*;
use crate::prefs::SurveyPrefs;
use openspot_heatmap::samples;

fn result(down: f64) -> Vec<SpeedResult> {
    vec![SpeedResult { iperf_tcp_down_mbps: Some(down), ..SpeedResult::new("net") }]
}

#[test]
fn speed_results_ride_along_with_the_point_through_undo_and_redo() {
    let mut s = Survey::default();
    s.add_scan_with_speed(1.0, 2.0, vec![], result(120.0));
    assert_eq!(s.snapshot().points[0].speed[0].iperf_tcp_down_mbps, Some(120.0));
    assert!(s.undo());
    assert!(s.snapshot().points.is_empty());
    assert!(s.redo());
    assert_eq!(s.snapshot().points[0].speed, result(120.0), "redo must restore the speed results too");
}

#[test]
fn plain_add_scan_leaves_speed_empty() {
    let mut s = Survey::default();
    s.add_scan(0.0, 0.0, vec![]);
    assert!(s.snapshot().points[0].speed.is_empty());
}

#[test]
fn speed_metric_choice_maps_only_measured_points() {
    let mut s = Survey::default();
    s.add_scan_with_speed(0.0, 0.0, vec![], result(80.0));
    s.add_scan(5.0, 5.0, vec![]);
    s.metric_choice = MetricChoice::Speed(SpeedKind::TcpDown);
    let m = s.metric();
    assert_eq!(m.unit(), "Mbps");
    assert!(!s.supports_troubleshooting());
    let v = samples(s.snapshot(), &m);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].value, 80.0);
    s.metric_choice = MetricChoice::Speed(SpeedKind::UdpJitter);
    assert_eq!(s.base_metric_unit(), "ms");
}

#[test]
fn active_scan_prefs_round_trip_through_survey_prefs() {
    let mut s = Survey::default();
    s.active.prefs.enabled = true;
    s.active.prefs.iperf_host = "10.1.1.1".into();
    s.active.prefs.ssids = vec!["A".into(), "B".into()];
    let json = serde_json::to_string(&s.prefs()).unwrap();
    let mut t = Survey::default();
    t.apply_prefs(&serde_json::from_str::<SurveyPrefs>(&json).unwrap());
    assert_eq!(t.active.prefs, s.active.prefs);
    assert_eq!(t.active.ssids_text, "A\nB");
}

#[test]
fn old_prefs_without_active_scan_still_load() {
    let old: SurveyPrefs = serde_json::from_str(r#"{"opacity": 99}"#).unwrap();
    assert!(!old.active_scan.enabled);
    let mut s = Survey::default();
    s.apply_prefs(&old);
    assert_eq!(s.opacity, 99);
}

#[test]
fn speed_hover_text_keeps_sub_millisecond_jitter_visible() {
    use openspot_heatmap::Grid;
    let mut s = Survey { metric_choice: MetricChoice::Speed(SpeedKind::UdpJitter), ..Survey::default() };
    s.textures.heat_grid = Some(Grid { cols: 1, rows: 1, cell_px: 10.0, values: vec![Some(0.4)] });
    assert_eq!(s.heat_text_at(1.0, 1.0).unwrap(), "0.4 ms");
}
