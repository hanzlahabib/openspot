use super::*;
use openspot_model::LinkInfo;

fn link(tx: f64) -> LinkInfo {
    LinkInfo { bssid: MacAddress([2, 0, 0, 0, 0, 1]), tx_rate_mbps: tx, rx_rate_mbps: tx / 2.0 }
}

#[test]
fn a_point_records_the_link_reported_with_its_scan() {
    let mut s = Survey::default();
    s.request_scan(1.0, 1.0);
    s.take_pending_scan();
    s.set_scan_link(Some(link(300.0)));
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.snapshot().points[0].link, Some(link(300.0)));

    // Not connected for the next one: no link, and nothing carried over from the last point.
    s.request_scan(2.0, 2.0);
    s.take_pending_scan();
    s.set_scan_link(None);
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -60)]), Vec::new());
    assert_eq!(s.snapshot().points[1].link, None);
}

#[test]
fn with_several_scans_per_point_the_latest_link_counts() {
    let mut s = Survey::default();
    s.set_samples_per_point(2);
    s.request_scan(1.0, 1.0);
    s.take_pending_scan();
    s.set_scan_link(Some(link(100.0)));
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    s.take_pending_scan();
    s.set_scan_link(Some(link(200.0)));
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.snapshot().points[0].link, Some(link(200.0)));
}

#[test]
fn a_failed_scan_does_not_leak_its_link_into_the_next_point() {
    let mut s = Survey::default();
    s.request_scan(1.0, 1.0);
    s.take_pending_scan();
    s.set_scan_link(Some(link(100.0)));
    s.on_scan_result(1.0, 1.0, Err("boom".into()), Vec::new());
    s.request_scan(2.0, 2.0);
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.snapshot().points[0].link, None);
}

#[test]
fn undo_and_redo_keep_the_link_with_the_point() {
    let mut s = Survey::default();
    s.set_scan_link(Some(link(150.0)));
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert!(s.undo());
    assert!(s.snapshot().points.is_empty());
    assert!(s.redo());
    assert_eq!(s.snapshot().points[0].link, Some(link(150.0)));
}

#[test]
fn link_survives_saving_and_opening_the_project() {
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("link.ospot");
    let mut s = Survey::default();
    s.set_scan_link(Some(link(540.5)));
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    s.save_to(&path).unwrap();
    let mut t = Survey::default();
    t.open_from(&path).unwrap();
    assert_eq!(t.snapshot().points[0].link, Some(link(540.5)));
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn the_tx_rate_choice_maps_only_points_with_a_link() {
    let mut s = Survey { metric_choice: MetricChoice::TxRate, ..Survey::default() };
    assert_eq!(s.metric(), Metric::TxRate);
    assert_eq!(s.base_metric_unit(), "Mbps");
    assert!(!s.supports_troubleshooting());
    for (x, y, tx) in [(0.0, 0.0, Some(100.0)), (100.0, 0.0, Some(500.0)), (0.0, 100.0, Some(300.0)), (100.0, 100.0, None)] {
        s.set_scan_link(tx.map(link));
        s.on_scan_result(x, y, Ok(vec![ap(1, -50)]), Vec::new());
    }
    let n = openspot_heatmap::samples(s.snapshot(), &s.metric()).len();
    assert_eq!(n, 3, "the point measured while disconnected is skipped, not drawn as zero");
}
