use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use openspot_model::Project;
use openspot_wlan::{AccessPoint, MacAddress};

use super::*;
use crate::testdata::*;
use crate::options::{ReportKind, Sections, Titles, ViewOptions, Visualization};
use crate::testpdf::Parsed;

struct Fixture {
    project: Project,
    aliases: BTreeMap<MacAddress, String>,
    branding: ReportBranding,
    logo: Option<Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        Self { project: project(), aliases: aliases(), branding: branding(), logo: None }
    }

    fn data(&self) -> ReportData<'_> {
        ReportData { project: &self.project, aliases: &self.aliases, branding: &self.branding, logo: self.logo.as_deref() }
    }

    fn generate(&self, opts: &ReportOptions) -> Vec<u8> {
        generate_pdf(&self.data(), opts, &NoObserver).unwrap()
    }
}

fn adv(vizs: &[Visualization]) -> ReportOptions {
    ReportOptions { visualizations: vizs.to_vec(), ..ReportOptions::advanced(NOW) }
}

/// Page indices whose text contains `needle`.
fn pages_with(p: &Parsed, needle: &str) -> Vec<usize> {
    (0..p.page_count()).filter(|&i| p.page_text(i).contains(needle)).collect()
}

#[test]
fn multi_zone_report_has_expected_pages_text_images_and_metadata() {
    let fx = Fixture::new();
    let bytes = fx.generate(&adv(&[Visualization::SignalLevel, Visualization::Sir, Visualization::BandCoverage]));
    let p = Parsed::new(&bytes);

    // 4 snapshots x 3 visualizations, one page each, after cover and summary.
    let heat_pages: Vec<usize> = (0..p.page_count()).filter(|&i| p.page_text(i).contains("Access points")).collect();
    assert_eq!(heat_pages.len(), 12);
    assert!(p.page_count() >= 14);

    let cover = p.page_text(0);
    for want in ["Wi-Fi Site Survey Report", "Advanced report", "HQ Survey", "Contoso Ltd", "12 Example Street, Z\u{fc}rich", "Acme Networks", "survey@acme.example", "2026-10-16", "2026-10-01 to 2026-10-03"] {
        assert!(cover.contains(want), "cover is missing {want:?}:\n{cover}");
    }
    assert!(!cover.contains("Page 1 of"), "the cover has no footer");

    let summary = p.page_text(1);
    assert!(summary.contains("3 zones, 4 snapshots, 20 measurement points, 4 networks."), "{summary}");
    assert!(summary.contains("Lobby (Caf\u{e9} Z\u{fc}rich)"));
    assert!(summary.contains("\u{fffd}\u{fffd}\u{fffd}"), "CJK SSID falls back to replacement characters");
    assert!(summary.contains("Raspberry Pi"), "vendor from the OUI table");
    assert!(summary.contains("not set"), "zones without a scale say so");
    assert!(summary.contains("F4:F2:6D:00:00:02"));

    // Heatmap numbering: stable visualization numbers, 1-based zone and snapshot.
    for number in ["#1.1.1", "#1.1.3", "#1.1.4", "#1.2.1", "#2.1.4", "#3.1.3"] {
        assert!(!pages_with(&p, number).is_empty(), "no page numbered {number}");
    }
    assert!(pages_with(&p, "#1.1.2").is_empty(), "unselected visualization 2 must not appear");

    // The roof zone has no plan: placeholder text instead of an image.
    let roof = pages_with(&p, "#3.1.1")[0];
    assert!(p.page_text(roof).contains("no floor plan"));
    assert_eq!(p.op_count(p.page_ids()[roof], "Do"), 0);
    let with_plan = pages_with(&p, "#1.1.1")[0];
    assert_eq!(p.op_count(p.page_ids()[with_plan], "Do"), 1);

    // Only the two zones with a plan have images: (2 + 1) snapshots x 3 visualizations.
    assert_eq!(p.image_objects().len(), 9);

    // Footer numbering counts the physical pages.
    let last = p.page_count() - 1;
    assert!(p.page_text(last).contains(&format!("Page {} of {}", last + 1, p.page_count())));

    assert_eq!(p.info(b"Title").as_deref(), Some("Wi-Fi Site Survey Report"));
    assert_eq!(p.info(b"Author").as_deref(), Some("Acme Networks"));
    assert_eq!(p.info(b"Subject").as_deref(), Some("HQ Survey"));
    assert!(p.info(b"CreationDate").unwrap().starts_with("D:20261016120000"), "{:?}", p.info(b"CreationDate"));
}

#[test]
fn legends_match_the_metric() {
    let fx = Fixture::new();
    let p = Parsed::new(&fx.generate(&adv(&[Visualization::SignalLevel, Visualization::BandCoverage, Visualization::SignalTroubleshooting])));
    let signal = p.page_text(pages_with(&p, "#1.1.1")[0]);
    assert!(signal.contains("Legend (dBm)") && signal.contains("-90") && signal.contains("-35"), "{signal}");
    let band = p.page_text(pages_with(&p, "#1.1.4")[0]);
    assert!(band.contains("2.4 GHz only") && band.contains("Several bands"));
    let trouble = p.page_text(pages_with(&p, "#1.1.7")[0]);
    assert!(trouble.contains("Critical") && trouble.contains("Good: >= -67 dBm"), "{trouble}");
}

#[test]
fn zero_visualizations_still_yields_a_valid_report() {
    let fx = Fixture::new();
    let p = Parsed::new(&fx.generate(&adv(&[])));
    assert_eq!(p.page_count(), 2, "cover + summary only");
    assert!(p.page_text(1).contains("No heatmap visualizations were selected"));
    assert_eq!(p.image_objects().len(), 0);
}

#[test]
fn nothing_selected_at_all_is_an_explicit_empty_report() {
    let fx = Fixture::new();
    let mut o = adv(&[]);
    o.sections = Sections { cover: false, summary: false, ap_tables: false };
    let p = Parsed::new(&fx.generate(&o));
    assert_eq!(p.page_count(), 1);
    assert!(p.page_text(0).contains("Empty report"));
}

#[test]
fn deselected_zones_and_snapshots_are_left_out() {
    let fx = Fixture::new();
    let mut o = adv(&[Visualization::SignalLevel]);
    o.zones = Some(BTreeSet::from([fx.project.zones[0].id, fx.project.zones[2].id]));
    o.snapshots = Some(BTreeSet::from([fx.project.zones[0].snapshots[1].id, fx.project.zones[2].snapshots[0].id]));
    let p = Parsed::new(&fx.generate(&o));
    // Zone 1 keeps only its second snapshot; zone 2 is out; zone 3 stays.
    assert!(pages_with(&p, "#1.1.1").is_empty());
    assert_eq!(pages_with(&p, "#1.2.1").len(), 1);
    assert!(pages_with(&p, "#2.1.1").is_empty());
    assert_eq!(pages_with(&p, "#3.1.1").len(), 1);
    assert!(p.page_text(1).contains("2 zones, 2 snapshots, 10 measurement points"));

    // Selecting nothing keeps the report valid.
    let mut none = adv(&[Visualization::SignalLevel]);
    none.zones = Some(BTreeSet::new());
    let p = Parsed::new(&fx.generate(&none));
    assert_eq!(p.page_count(), 2);
    assert!(p.page_text(1).contains("No zones or snapshots were selected"));
}

#[test]
fn quick_report_ignores_selections() {
    let fx = Fixture::new();
    let mut o = ReportOptions::quick(NOW);
    o.visualizations = Visualization::ALL.to_vec();
    o.zones = Some(BTreeSet::new());
    assert_eq!(o.kind, ReportKind::Quick);
    let p = Parsed::new(&fx.generate(&o));
    assert_eq!(pages_with(&p, "Access points").len(), 4, "one signal map per snapshot");
    assert!(p.page_text(0).contains("Quick report"));
}

#[test]
fn custom_titles_and_notes_appear_with_unicode() {
    let fx = Fixture::new();
    let mut o = adv(&[]);
    o.titles = Titles { title: "Bericht f\u{fc}r Z\u{fc}rich".into(), subtitle: "Etage 2".into(), notes: "Scope: offices only.\nSecond line.".into() };
    let p = Parsed::new(&fx.generate(&o));
    assert!(p.page_text(0).contains("Bericht f\u{fc}r Z\u{fc}rich") && p.page_text(0).contains("Etage 2"));
    assert!(p.page_text(1).contains("Scope: offices only."));
    assert_eq!(p.info(b"Title").as_deref(), Some("Bericht f\u{fc}r Z\u{fc}rich"));
}

#[test]
fn output_is_deterministic_for_fixed_inputs_and_date() {
    let fx = Fixture::new();
    let o = adv(&[Visualization::SignalLevel, Visualization::ApCount]);
    assert_eq!(fx.generate(&o), fx.generate(&o));
    let later = ReportOptions { generated_at: NOW + 86_400, ..o.clone() };
    assert_ne!(fx.generate(&o), fx.generate(&later));
}

#[test]
fn view_options_change_the_drawn_shapes() {
    let fx = Fixture::new();
    let page_ops = |view: ViewOptions, op: &str| {
        let mut o = adv(&[Visualization::SignalLevel]);
        o.view = view;
        o.zones = Some(BTreeSet::from([fx.project.zones[1].id])); // 5 points
        let p = Parsed::new(&fx.generate(&o));
        p.op_count(p.page_ids()[pages_with(&p, "#2.1.1")[0]], op)
    };
    let none = ViewOptions { show_points: false, show_path: false, show_ap_markers: false, ..ViewOptions::default() };
    let points = ViewOptions { show_points: true, ..none };
    let path = ViewOptions { show_path: true, ..none };
    let markers = ViewOptions { show_ap_markers: true, ..none };

    // Each point is a circle of four Bezier segments; nothing else on the page uses curves.
    assert_eq!(page_ops(none, "c"), 0);
    assert_eq!(page_ops(points, "c"), 5 * 4);
    // A path adds one move and (points - 1) line segments; other lines are rules/legend rects.
    assert_eq!(page_ops(path, "l") - page_ops(none, "l"), 4);
    // Markers are triangles (2 line segments + close) for APs heard at 3+ points: LOBBY and CAFE.
    assert_eq!(page_ops(markers, "l") - page_ops(none, "l"), 2 * 2);
}

#[test]
fn hiding_the_plan_gives_a_white_background() {
    let fx = Fixture::new();
    let corner_pixel = |show_plan: bool| {
        let mut o = adv(&[Visualization::SignalLevel]);
        o.view.show_plan = show_plan;
        o.zones = Some(BTreeSet::from([fx.project.zones[1].id]));
        let p = Parsed::new(&fx.generate(&o));
        let img = p.image_objects()[0];
        let w = img.dict.get(b"Width").unwrap().as_i64().unwrap() as usize;
        let rgb = img.decompressed_content().unwrap();
        assert_eq!(rgb.len(), w * img.dict.get(b"Height").unwrap().as_i64().unwrap() as usize * 3);
        [rgb[0], rgb[1], rgb[2]]
    };
    assert_eq!(corner_pixel(true), [225, 215, 190], "the plan colour shows through far from data");
    assert_eq!(corner_pixel(false), [255, 255, 255]);
}

#[test]
fn ap_table_lists_markers_and_honors_the_row_limit() {
    let fx = Fixture::new();
    let mut o = adv(&[Visualization::SignalLevel]);
    o.zones = Some(BTreeSet::from([fx.project.zones[1].id]));
    let p = Parsed::new(&fx.generate(&o));
    let runs = p.page_runs(p.page_ids()[pages_with(&p, "#2.1.1")[0]]);
    // Strongest first: Lobby(1), Guest(2) get markers; the 2-point CJK AP and the 1-point
    // printer are too rare to locate and show "-".
    let idx = runs.iter().position(|r| r == "Lobby (Caf\u{e9} Z\u{fc}rich)").unwrap();
    assert_eq!(runs[idx - 1], "1");
    let rare = runs.iter().position(|r| r == "Printer").unwrap();
    assert_eq!(runs[rare - 1], "-");

    o.max_ap_rows = Some(2);
    let p = Parsed::new(&fx.generate(&o));
    let text = p.page_text(pages_with(&p, "#2.1.1")[0]);
    assert!(text.contains("Strongest 2 of 4 networks"));
    assert!(!text.contains("Printer"));
}

#[test]
fn long_ap_tables_continue_on_new_pages_with_repeated_headers() {
    let mut fx = Fixture::new();
    // 80 more neighbours heard at one point of the upper-floor snapshot.
    let extra: Vec<AccessPoint> = (0..80u8).map(|i| ap(MacAddress([0xF4, 0xF2, 0x6D, 1, 0, i]), &format!("Neighbour {i}"), -85 + i32::from(i % 5), 2_437_000)).collect();
    fx.project.zones[1].snapshots[0].points[0].access_points.extend(extra);
    let mut o = adv(&[Visualization::SignalLevel]);
    o.zones = Some(BTreeSet::from([fx.project.zones[1].id]));
    o.sections.summary = false;
    o.max_ap_rows = None;
    let p = Parsed::new(&fx.generate(&o));
    assert!(p.page_count() >= 3, "cover + at least two heatmap pages, got {}", p.page_count());
    let runs = p.all_runs();
    for i in 0..80u8 {
        let b = format!("F4:F2:6D:01:00:{i:02X}");
        assert_eq!(runs.iter().filter(|r| **r == b).count(), 1, "{b} must appear exactly once");
    }
    let header_count = runs.iter().filter(|r| *r == "BSSID").count();
    assert!(header_count >= 2, "table header repeats after a page break");
}

#[test]
fn logo_is_embedded_and_bad_logo_is_a_typed_error() {
    let mut fx = Fixture::new();
    fx.logo = Some(png(200, 80, [10, 120, 200, 128]));
    let p = Parsed::new(&fx.generate(&adv(&[])));
    assert_eq!(p.op_count(p.page_ids()[0], "Do"), 1);
    assert_eq!(p.image_objects().len(), 2, "logo + soft mask for its transparency");

    fx.logo = Some(b"definitely not an image".to_vec());
    let err = generate_pdf(&fx.data(), &adv(&[]), &NoObserver).unwrap_err();
    assert!(matches!(err, ReportError::Logo(_)), "{err:?}");
}

#[test]
fn project_without_data_still_produces_a_report() {
    let fx = Fixture { project: Project::new("Empty"), aliases: BTreeMap::new(), branding: ReportBranding::default(), logo: None };
    let p = Parsed::new(&fx.generate(&adv(&[Visualization::SignalLevel])));
    assert!(p.page_text(0).contains("No measurements"));
    assert!(p.page_text(1).contains("No networks were measured."));
    // The default zone has no plan: placeholder page.
    assert!(p.page_text(2).contains("no floor plan"));
    assert!(p.page_text(2).contains("This snapshot has no measurement points."));
    assert_eq!(p.info(b"Author").as_deref(), Some("OpenSpot"));
}

struct Recorder {
    cancel_after: Option<usize>,
    seen: std::cell::RefCell<Vec<(usize, usize)>>,
    polls: Cell<usize>,
}

impl Observer for Recorder {
    fn is_cancelled(&self) -> bool {
        let n = self.polls.get();
        self.polls.set(n + 1);
        self.cancel_after.is_some_and(|limit| n >= limit)
    }

    fn progress(&self, done: usize, total: usize, _step: &str) {
        self.seen.borrow_mut().push((done, total));
    }
}

#[test]
fn progress_is_monotonic_and_cancellation_aborts() {
    let fx = Fixture::new();
    let o = adv(&[Visualization::SignalLevel]);
    let rec = Recorder { cancel_after: None, seen: Default::default(), polls: Cell::new(0) };
    generate_pdf(&fx.data(), &o, &rec).unwrap();
    let seen = rec.seen.borrow();
    // cover + summary + 4 heatmaps, then the final "writing" tick.
    assert!(seen.iter().all(|&(_, total)| total == 6));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(seen.last(), Some(&(6, 6)));

    let rec = Recorder { cancel_after: Some(3), seen: Default::default(), polls: Cell::new(0) };
    let err = generate_pdf(&fx.data(), &o, &rec).unwrap_err();
    assert!(matches!(err, ReportError::Cancelled));
    assert!(rec.seen.borrow().len() < 6, "no work after cancellation");
}

/// Manual aid: `OPENSPOT_REPORT_SAMPLE=/tmp/sample.pdf cargo test -p openspot-report -- --ignored dump_sample`
/// writes the fixture report so it can be opened in a real PDF viewer.
#[test]
#[ignore = "writes a file; run manually"]
fn dump_sample() {
    let Ok(path) = std::env::var("OPENSPOT_REPORT_SAMPLE") else { return };
    let mut fx = Fixture::new();
    fx.logo = Some(png(200, 80, [10, 120, 200, 255]));
    let o = ReportOptions { view: ViewOptions { show_path: true, ..ViewOptions::default() }, ..adv(&Visualization::ALL) };
    std::fs::write(path, fx.generate(&o)).unwrap();
}
