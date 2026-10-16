//! What a report contains. The wizard in the app edits one of these.

use std::collections::BTreeSet;

use openspot_heatmap::{GradientPreset, Metric, Smoothing};
use openspot_model::{Uuid, Zone};

/// Weaker networks are not usable by a client, so they do not count as covering a point.
/// Matches the survey view's default for the AP-count and band-coverage maps.
pub const MIN_USABLE_RSSI_DBM: i32 = -80;

/// Default cap on rows in a per-heatmap AP table: enough for a typical site, short enough
/// that a crowded environment does not bury the heatmaps under neighbours' routers.
pub const DEFAULT_MAX_AP_ROWS: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    /// One signal-level heatmap per snapshot, no choices.
    Quick,
    /// Honors every field of [`ReportOptions`].
    Advanced,
}

/// A heatmap type that can appear in a report. The discriminant is the stable number
/// shown in the heatmap numbering (`#zone.snapshot.visualization`), so "#1.1.3" means the
/// same map in every report regardless of which others were selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visualization {
    SignalLevel = 1,
    SecondarySignal = 2,
    Sir = 3,
    BandCoverage = 4,
    PhyCoverage = 5,
    ApCount = 6,
    SignalTroubleshooting = 7,
}

impl Visualization {
    pub const ALL: [Self; 7] = [
        Self::SignalLevel,
        Self::SecondarySignal,
        Self::Sir,
        Self::BandCoverage,
        Self::PhyCoverage,
        Self::ApCount,
        Self::SignalTroubleshooting,
    ];

    pub fn number(self) -> u8 {
        self as u8
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::SignalLevel => "Signal level",
            Self::SecondarySignal => "Secondary signal level",
            Self::Sir => "Signal-to-interference ratio",
            Self::BandCoverage => "Frequency band coverage",
            Self::PhyCoverage => "PHY mode coverage",
            Self::ApCount => "Number of access points",
            Self::SignalTroubleshooting => "Signal level (troubleshooting)",
        }
    }

    /// The heatmap metric, over all networks.
    pub fn metric(self) -> Metric {
        match self {
            Self::SignalLevel => Metric::Signal(Vec::new()),
            Self::SecondarySignal => Metric::SecondarySignal(Vec::new()),
            Self::Sir => Metric::Sir(Vec::new()),
            Self::BandCoverage => Metric::BandCoverage { wanted: Vec::new(), min_rssi_dbm: MIN_USABLE_RSSI_DBM },
            Self::PhyCoverage => Metric::PhyCoverage(Vec::new()),
            Self::ApCount => Metric::ApCount { min_rssi_dbm: MIN_USABLE_RSSI_DBM },
            Self::SignalTroubleshooting => {
                let base = Metric::Signal(Vec::new());
                // Signal metrics always have default thresholds.
                let (acceptable, critical) = base.default_thresholds().unwrap_or((-67, -75));
                Metric::Troubleshoot { base: Box::new(base), acceptable, critical }
            }
        }
    }
}

/// What to draw on top of (or instead of) the floor plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Show the floor plan under the heatmap; off gives a plain white background.
    pub show_plan: bool,
    /// Connect measurement points in the order they were taken.
    pub show_path: bool,
    pub show_points: bool,
    /// Estimated positions of the access points listed on the page.
    pub show_ap_markers: bool,
    pub gradient: GradientPreset,
    pub smoothing: Smoothing,
    /// Heatmap opacity, 0-255.
    pub opacity: u8,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            show_plan: true,
            show_path: false,
            show_points: true,
            show_ap_markers: true,
            gradient: GradientPreset::Default,
            smoothing: Smoothing::Normal,
            opacity: 170,
        }
    }
}

/// Free text the user controls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Titles {
    /// Cover title; empty uses the default.
    pub title: String,
    pub subtitle: String,
    /// Paragraph(s) printed on the summary page, e.g. scope or conclusions.
    pub notes: String,
}

/// Which parts of the report to include besides the heatmap pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sections {
    pub cover: bool,
    pub summary: bool,
    pub ap_tables: bool,
}

impl Default for Sections {
    fn default() -> Self {
        Self { cover: true, summary: true, ap_tables: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportOptions {
    pub kind: ReportKind,
    /// Zones to include; `None` means all.
    pub zones: Option<BTreeSet<Uuid>>,
    /// Snapshots to include; `None` means all.
    pub snapshots: Option<BTreeSet<Uuid>>,
    pub visualizations: Vec<Visualization>,
    pub view: ViewOptions,
    pub titles: Titles,
    pub sections: Sections,
    /// Rows per AP table (strongest first); `None` lists every network.
    pub max_ap_rows: Option<usize>,
    /// Report date and PDF creation time, Unix seconds. Injected so output is reproducible.
    pub generated_at: i64,
}

impl ReportOptions {
    /// The one-click report: everything, signal level only.
    pub fn quick(generated_at: i64) -> Self {
        Self {
            kind: ReportKind::Quick,
            zones: None,
            snapshots: None,
            visualizations: vec![Visualization::SignalLevel],
            view: ViewOptions::default(),
            titles: Titles::default(),
            sections: Sections::default(),
            max_ap_rows: Some(DEFAULT_MAX_AP_ROWS),
            generated_at,
        }
    }

    pub fn advanced(generated_at: i64) -> Self {
        Self { kind: ReportKind::Advanced, ..Self::quick(generated_at) }
    }

    /// The options actually applied: a Quick report ignores selections, keeping only the
    /// user's titles and the date.
    pub(crate) fn resolved(&self) -> Self {
        match self.kind {
            ReportKind::Advanced => self.clone(),
            ReportKind::Quick => Self { titles: self.titles.clone(), ..Self::quick(self.generated_at) },
        }
    }

    pub(crate) fn includes_zone(&self, zone: &Zone) -> bool {
        self.zones.as_ref().is_none_or(|set| set.contains(&zone.id))
    }

    pub(crate) fn includes_snapshot(&self, id: &Uuid) -> bool {
        self.snapshots.as_ref().is_none_or(|set| set.contains(id))
    }

    /// Selected visualizations in numbering order, without duplicates.
    pub(crate) fn sorted_visualizations(&self) -> Vec<Visualization> {
        let set: BTreeSet<_> = self.visualizations.iter().copied().collect();
        set.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_stable_and_unique() {
        let nums: Vec<u8> = Visualization::ALL.iter().map(|v| v.number()).collect();
        assert_eq!(nums, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn quick_ignores_selections_but_keeps_titles() {
        let mut o = ReportOptions::advanced(5);
        o.kind = ReportKind::Quick;
        o.visualizations = Visualization::ALL.to_vec();
        o.zones = Some(BTreeSet::new());
        o.titles.title = "Mine".into();
        let r = o.resolved();
        assert_eq!(r.visualizations, vec![Visualization::SignalLevel]);
        assert!(r.zones.is_none());
        assert_eq!(r.titles.title, "Mine");
        assert_eq!(r.generated_at, 5);
    }

    #[test]
    fn visualizations_are_deduplicated_in_numbering_order() {
        let mut o = ReportOptions::advanced(0);
        o.visualizations = vec![Visualization::Sir, Visualization::SignalLevel, Visualization::Sir];
        assert_eq!(o.sorted_visualizations(), vec![Visualization::SignalLevel, Visualization::Sir]);
    }

    #[test]
    fn troubleshooting_uses_signal_defaults() {
        match Visualization::SignalTroubleshooting.metric() {
            Metric::Troubleshoot { acceptable, critical, .. } => assert_eq!((acceptable, critical), (-67, -75)),
            other => panic!("unexpected metric {other:?}"),
        }
    }
}
