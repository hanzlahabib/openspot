//! Per-access-point heatmap export (R-06): one PNG per AP, into a folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use openspot_heatmap::Metric;
use openspot_model::Project;
use openspot_wlan::MacAddress;

use crate::error::ReportError;
use crate::names::sanitize_file_name;
use crate::options::ReportOptions;
use crate::report::heatmap::render_heatmap;
use crate::report::{Observer, Plan, network_rows};

/// What an export produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PerApExport {
    pub written: Vec<PathBuf>,
    /// Zones left out because they have no floor plan to draw on.
    pub skipped_zones: Vec<String>,
}

/// `MM-MM-MM-MM-MM-MM`: colons are not allowed in Windows file names.
fn bssid_for_file(bssid: &MacAddress) -> String {
    bssid.to_string().replace(':', "-")
}

/// File name for one AP's heatmap. The leading `zone.snapshot` numbers keep names unique
/// even when zones or snapshots share a name; the BSSID keeps APs with equal SSIDs apart.
pub fn file_name(zone_index: usize, snapshot_index: usize, zone: &str, snapshot: &str, ap_name: &str, bssid: &MacAddress) -> String {
    format!(
        "{}.{} {} - {} - {} ({}).png",
        zone_index + 1,
        snapshot_index + 1,
        sanitize_file_name(zone),
        sanitize_file_name(snapshot),
        sanitize_file_name(ap_name),
        bssid_for_file(bssid),
    )
}

/// Writes `<dir>/<name>.png` for every AP in every selected snapshot: the signal map of
/// that AP alone. Zone/snapshot selection and view options come from `options`; the
/// visualization list is ignored. Creates `dir` if needed and overwrites same-named files.
pub fn export_per_ap_heatmaps(
    project: &Project,
    aliases: &BTreeMap<MacAddress, String>,
    options: &ReportOptions,
    dir: &Path,
    observer: &dyn Observer,
) -> Result<PerApExport, ReportError> {
    let opts = options.resolved();
    let plan = Plan::new(project, &opts);
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| ReportError::Io { path, source }
    };
    std::fs::create_dir_all(dir).map_err(io(dir))?;

    let mut result = PerApExport {
        skipped_zones: plan.zones.iter().filter(|pz| pz.zone.map.is_none()).map(|pz| pz.zone.name.clone()).collect(),
        ..PerApExport::default()
    };
    let jobs: Vec<_> = plan
        .zones
        .iter()
        .filter(|pz| pz.zone.map.is_some())
        .flat_map(|pz| pz.snapshots.iter().map(move |&(si, snap)| (pz, si, snap, network_rows([snap], aliases))))
        .collect();
    let total: usize = jobs.iter().map(|j| j.3.len()).sum();
    let mut done = 0;
    for (pz, si, snapshot, rows) in &jobs {
        for row in rows {
            if observer.is_cancelled() {
                return Err(ReportError::Cancelled);
            }
            let name = row.display_name();
            observer.progress(done, total, &name);
            let metric = Metric::Signal(vec![row.bssid]);
            // The PNG export draws points into the raster, there is no vector layer here.
            let png = render_heatmap(pz.zone, snapshot, &opts.view, &metric, opts.view.show_points)?;
            if let Some(bytes) = png {
                let path = dir.join(file_name(pz.index, *si, &pz.zone.name, &snapshot.name, &name, &row.bssid));
                std::fs::write(&path, bytes).map_err(io(&path))?;
                result.written.push(path);
            }
            done += 1;
        }
    }
    observer.progress(total, total, "Done");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::options::{ViewOptions, Visualization};
    use openspot_heatmap::Smoothing;
    use openspot_model::Project;
    use crate::report::NoObserver;
    use crate::testdata::*;

    fn export(project: &Project, opts: &ReportOptions, dir: &Path) -> PerApExport {
        export_per_ap_heatmaps(project, &aliases(), opts, dir, &NoObserver).unwrap()
    }

    #[test]
    fn file_names_are_safe_unique_and_readable() {
        let n = file_name(0, 1, "Ground: floor", "After/Before", "Caf\u{e9}?", &LOBBY);
        assert_eq!(n, "1.2 Ground_ floor - After_Before - Caf\u{e9}_ (B8-27-EB-00-00-01).png");
        assert!(!n.contains([':', '/', '\\', '?']));
        // Same names in different zones or snapshots still differ.
        assert_ne!(file_name(0, 0, "Z", "S", "AP", &LOBBY), file_name(1, 0, "Z", "S", "AP", &LOBBY));
        assert_ne!(file_name(0, 0, "Z", "S", "AP", &LOBBY), file_name(0, 1, "Z", "S", "AP", &LOBBY));
        // Same SSID, different radios.
        assert_ne!(file_name(0, 0, "Z", "S", "AP", &LOBBY), file_name(0, 0, "Z", "S", "AP", &CAFE));
        // Even with maximal components the name fits a 255-byte file system limit.
        let big = "\u{1f4f6}".repeat(100);
        assert!(file_name(98, 98, &big, &big, &big, &LOBBY).len() <= 255);
    }

    #[test]
    fn writes_one_png_per_ap_per_snapshot_and_skips_zones_without_a_plan() {
        let dir = TempDir::new();
        let mut opts = ReportOptions::advanced(NOW);
        opts.visualizations.clear(); // irrelevant for this export
        let out = export(&project(), &opts, dir.path());
        // Ground floor: 2 snapshots x 4 APs; upper floor: 1 x 4; roof has no plan.
        assert_eq!(out.written.len(), 12);
        assert_eq!(out.skipped_zones, vec!["Roof (no plan)".to_owned()]);
        let names: BTreeSet<String> = out.written.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 12, "all names distinct");
        assert!(names.contains("1.1 Ground floor - Before - Lobby (Caf\u{e9} Z\u{fc}rich) (B8-27-EB-00-00-01).png"));
        // The CJK SSID survives in the file name; the file system handles Unicode.
        assert!(names.iter().any(|n| n.contains("\u{65e5}\u{672c}\u{8a9e}")));
        for p in &out.written {
            let img = image::open(p).unwrap();
            assert_eq!((img.width(), img.height()), (400, 300));
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 12);
    }

    #[test]
    fn each_image_shows_only_its_own_access_point() {
        let dir = TempDir::new();
        let p = project();
        let mut opts = ReportOptions::advanced(NOW);
        opts.zones = Some(BTreeSet::from([p.zones[1].id]));
        opts.visualizations = vec![Visualization::SignalLevel];
        // Exact values at sample points need no smoothing and no dots covering the pixel.
        opts.view = ViewOptions { show_points: false, smoothing: Smoothing::Off, ..ViewOptions::default() };
        let out = export(&p, &opts, dir.path());
        assert_eq!(out.written.len(), 4);
        let pixel = |needle: &str, x, y| {
            let path = out.written.iter().find(|p| p.to_string_lossy().contains(needle)).unwrap();
            *image::open(path).unwrap().to_rgba8().get_pixel(x, y)
        };
        // Point 1 (120,100) hears LOBBY at -45 and CJK at -70, the printer not at all; point
        // 5 (280,200) hears the printer but not CJK. A map of one AP treats "not heard" as the
        // floor level, so these two unheard spots must look identical, and unlike a heard one.
        let printer_at_p1 = pixel("F4-F2-6D-00-00-04", 120, 100);
        let cjk_at_p5 = pixel("F4-F2-6D-00-00-03", 280, 200);
        assert_eq!(printer_at_p1, cjk_at_p5, "unheard spots share the floor colour");
        assert_ne!(pixel("B8-27-EB-00-00-01", 120, 100), printer_at_p1, "LOBBY is loud at point 1");
        assert_ne!(pixel("F4-F2-6D-00-00-03", 120, 100), cjk_at_p5, "CJK is heard at point 1");
    }

    #[test]
    fn cancelling_stops_writing() {
        struct After(std::cell::Cell<usize>);
        impl Observer for After {
            fn is_cancelled(&self) -> bool {
                self.0.set(self.0.get() + 1);
                self.0.get() > 3
            }
        }
        let dir = TempDir::new();
        let err = export_per_ap_heatmaps(&project(), &aliases(), &ReportOptions::advanced(NOW), dir.path(), &After(Default::default())).unwrap_err();
        assert!(matches!(err, ReportError::Cancelled));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn creates_the_target_folder() {
        let dir = TempDir::new();
        let target = dir.path().join("nested").join("out");
        let p = project();
        let mut opts = ReportOptions::advanced(NOW);
        opts.zones = Some(BTreeSet::from([p.zones[1].id]));
        let out = export(&p, &opts, &target);
        assert_eq!(out.written.len(), 4);
        assert!(target.is_dir());
    }

    #[test]
    fn unwritable_target_is_a_typed_io_error() {
        let dir = TempDir::new();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let err = export_per_ap_heatmaps(&project(), &aliases(), &ReportOptions::advanced(NOW), &blocker.join("sub"), &NoObserver).unwrap_err();
        assert!(matches!(err, ReportError::Io { .. }), "{err:?}");
    }
}
