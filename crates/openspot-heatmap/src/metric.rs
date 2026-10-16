use openspot_model::Snapshot;
use openspot_wlan::{Band, MacAddress, PhyMode};

use crate::{Gradient, Palette, Sample, SpeedKind, TX_RATE_UNIT, speed_samples, tx_rate_samples};

/// RSSI assigned where a wanted network was not heard at all, so coverage visibly
/// ends instead of the map extrapolating from other points.
pub const FLOOR_DBM: f64 = -100.0;

/// What the heatmap shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Metric {
    /// Strongest signal among these networks at each point (empty = all networks).
    Signal(Vec<MacAddress>),
    /// Second-strongest signal among these networks (empty = all). Shows where roaming
    /// clients would have a fallback AP; points hearing fewer than two get the floor.
    SecondarySignal(Vec<MacAddress>),
    /// Co-channel signal-to-interference ratio for these networks (empty = all); see
    /// [`crate::sir_db`].
    Sir(Vec<MacAddress>),
    /// Which bands are audible (at or above `min_rssi_dbm`) at each point, as flat
    /// categories: none, 2.4 only, 5 only, 6 only, several.
    BandCoverage { wanted: Vec<MacAddress>, min_rssi_dbm: i32 },
    /// Newest PHY generation of the strongest wanted network at each point.
    PhyCoverage(Vec<MacAddress>),
    /// Troubleshooting view of a continuous `base` metric: each region is classed
    /// Critical (value <= `critical`), Good (>= `acceptable`) or Acceptable in between.
    Troubleshoot { base: Box<Metric>, acceptable: i32, critical: i32 },
    /// How many networks are audible at each point, counting only those at or above
    /// `min_rssi_dbm`.
    ApCount { min_rssi_dbm: i32 },
    /// Active-scan throughput or jitter, from the results of these SSIDs (empty = all). Points
    /// without a measurement are left out of the map; see [`crate::speed_samples`].
    Speed { kind: SpeedKind, ssids: Vec<String> },
    /// Negotiated transmit rate of the adapter's own connection at each point (higher is
    /// better). Reflects the link to the network the laptop was connected to, not every AP;
    /// points without a recorded rate are left out of the map; see [`crate::tx_rate_samples`].
    TxRate,
}

impl Metric {
    /// Flat labelled colors for categorical metrics, `None` for continuous ones.
    /// Default `(acceptable, critical)` thresholds if this metric supports the
    /// troubleshooting view. Signal values follow common guidance: -67 dBm carries voice
    /// and video, below -75 dBm connections get unreliable; SIR below 10 dB hurts throughput.
    pub fn default_thresholds(&self) -> Option<(i32, i32)> {
        match self {
            Self::Signal(_) => Some((-67, -75)),
            Self::SecondarySignal(_) => Some((-75, -85)),
            Self::Sir(_) => Some((15, 10)),
            _ => None,
        }
    }

    pub fn palette(&self) -> Option<Palette> {
        const GREY: [u8; 3] = [170, 170, 170];
        // Okabe-Ito colors: distinguishable with common forms of color blindness.
        const ORANGE: [u8; 3] = [230, 159, 0];
        const BLUE: [u8; 3] = [0, 114, 178];
        const GREEN: [u8; 3] = [0, 158, 115];
        const PURPLE: [u8; 3] = [204, 121, 167];
        const VERMILLION: [u8; 3] = [213, 94, 0];
        match self {
            Self::Troubleshoot { .. } => Some(Palette::new(vec![
                ("Critical", [215, 25, 28]),
                ("Acceptable", [255, 221, 80]),
                ("Good", [26, 150, 65]),
            ])),
            Self::BandCoverage { .. } => Some(Palette::new(vec![
                ("No signal", GREY),
                ("2.4 GHz only", ORANGE),
                ("5 GHz only", BLUE),
                ("6 GHz only", GREEN),
                ("Several bands", PURPLE),
            ])),
            Self::PhyCoverage(_) => Some(Palette::new(vec![
                ("No signal", GREY),
                ("802.11 a/b/g", VERMILLION),
                ("802.11n (Wi-Fi 4)", ORANGE),
                ("802.11ac (Wi-Fi 5)", BLUE),
                ("802.11ax (Wi-Fi 6/6E)", GREEN),
                ("802.11be (Wi-Fi 7)", PURPLE),
            ])),
            _ => None,
        }
    }

    pub fn gradient(&self) -> Gradient {
        match self {
            Self::Troubleshoot { base, .. } => base.gradient(),
            Self::Signal(_) | Self::SecondarySignal(_) => Gradient::signal_dbm(),
            Self::Sir(_) => Gradient::ratio_db(),
            Self::ApCount { .. } => Gradient::count(15),
            Self::Speed { kind, .. } => kind.gradient(),
            Self::TxRate => crate::tx_rate::gradient(),
            // Categorical metrics are colored through `palette`; this is never used for them.
            Self::BandCoverage { .. } | Self::PhyCoverage(_) => Gradient::signal_dbm(),
        }
    }

    /// True when smaller values are better (only UDP jitter), so legends can say so.
    pub fn lower_is_better(&self) -> bool {
        matches!(self, Self::Speed { kind, .. } if kind.lower_is_better())
    }

    pub fn unit(&self) -> &'static str {
        match self {
            Self::Troubleshoot { base, .. } => base.unit(),
            Self::Signal(_) | Self::SecondarySignal(_) => "dBm",
            Self::Sir(_) => "dB",
            Self::ApCount { .. } => "APs",
            Self::Speed { kind, .. } => kind.unit(),
            Self::TxRate => TX_RATE_UNIT,
            Self::BandCoverage { .. } | Self::PhyCoverage(_) => "",
        }
    }
}

/// One sample per scan point for `metric`.
pub fn samples(snapshot: &Snapshot, metric: &Metric) -> Vec<Sample> {
    if let Metric::Troubleshoot { base, .. } = metric {
        return samples(snapshot, base);
    }
    if let Metric::Speed { kind, ssids } = metric {
        return speed_samples(snapshot, *kind, ssids);
    }
    if matches!(metric, Metric::TxRate) {
        return tx_rate_samples(snapshot);
    }
    snapshot
        .points
        .iter()
        .map(|p| {
            let value = match metric {
                Metric::Signal(wanted) => p
                    .access_points
                    .iter()
                    .filter(|a| wanted.is_empty() || wanted.contains(&a.bssid))
                    .map(|a| f64::from(a.rssi_dbm))
                    .fold(FLOOR_DBM, f64::max),
                Metric::SecondarySignal(wanted) => {
                    let mut rssi: Vec<i32> = p
                        .access_points
                        .iter()
                        .filter(|a| wanted.is_empty() || wanted.contains(&a.bssid))
                        .map(|a| a.rssi_dbm)
                        .collect();
                    rssi.sort_unstable_by(|a, b| b.cmp(a));
                    rssi.get(1).map_or(FLOOR_DBM, |&v| f64::from(v))
                }
                Metric::BandCoverage { wanted, min_rssi_dbm } => {
                    let bands: std::collections::BTreeSet<_> = p
                        .access_points
                        .iter()
                        .filter(|a| (wanted.is_empty() || wanted.contains(&a.bssid)) && a.rssi_dbm >= *min_rssi_dbm)
                        .filter_map(|a| a.band)
                        .collect();
                    match (bands.len(), bands.iter().next()) {
                        (0, _) => 0.0,
                        (1, Some(Band::Ghz2_4)) => 1.0,
                        (1, Some(Band::Ghz5)) => 2.0,
                        (1, _) => 3.0,
                        _ => 4.0,
                    }
                }
                Metric::PhyCoverage(wanted) => p
                    .access_points
                    .iter()
                    .filter(|a| wanted.is_empty() || wanted.contains(&a.bssid))
                    .max_by_key(|a| a.rssi_dbm)
                    .map_or(0.0, |a| match a.phy.iter().next_back() {
                        None | Some(PhyMode::B | PhyMode::A | PhyMode::G) => 1.0,
                        Some(PhyMode::N) => 2.0,
                        Some(PhyMode::Ac) => 3.0,
                        Some(PhyMode::Ax) => 4.0,
                        Some(PhyMode::Be) => 5.0,
                    }),
                Metric::Troubleshoot { .. } | Metric::Speed { .. } | Metric::TxRate => unreachable!("handled before the loop"),
                Metric::Sir(wanted) => crate::sir_db(&p.access_points, wanted),
                Metric::ApCount { min_rssi_dbm } => {
                    p.access_points.iter().filter(|a| a.rssi_dbm >= *min_rssi_dbm).count() as f64
                }
            };
            Sample { x: p.x, y: p.y, value }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::ScanPoint;
    use openspot_wlan::AccessPoint;

    fn ap(last: u8, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new("s");
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, -50), ap(2, -80)]));
        s.points.push(ScanPoint::new(1.0, 0.0, 0, vec![ap(2, -60)]));
        s
    }

    #[test]
    fn signal_takes_strongest_of_wanted_and_floors_missing() {
        let all = samples(&snapshot(), &Metric::Signal(vec![]));
        assert_eq!((all[0].value, all[1].value), (-50.0, -60.0));
        let only1 = samples(&snapshot(), &Metric::Signal(vec![MacAddress([0, 0, 0, 0, 0, 1])]));
        assert_eq!((only1[0].value, only1[1].value), (-50.0, FLOOR_DBM));
    }

    #[test]
    fn secondary_signal_is_second_strongest_or_floor() {
        let s = samples(&snapshot(), &Metric::SecondarySignal(vec![]));
        assert_eq!((s[0].value, s[1].value), (-80.0, FLOOR_DBM));
        let only = samples(&snapshot(), &Metric::SecondarySignal(vec![MacAddress([0, 0, 0, 0, 0, 1])]));
        assert_eq!((only[0].value, only[1].value), (FLOOR_DBM, FLOOR_DBM));
    }

    #[test]
    fn sir_metric_uses_interference_model() {
        let mut s = Snapshot::new("s");
        // Two equal co-channel APs at one point (0 dB), a lone AP at the other (capped).
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, -50), ap(2, -50)]));
        s.points.push(ScanPoint::new(1.0, 0.0, 0, vec![ap(1, -50)]));
        let v = samples(&s, &Metric::Sir(vec![]));
        assert!(v[0].value.abs() < 1e-9);
        assert_eq!(v[1].value, crate::MAX_SIR_DB);
    }

    #[test]
    fn band_coverage_categories() {
        let mut s = Snapshot::new("s");
        let on = |last: u8, rssi: i32, khz: u32| {
            AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, vec![])
        };
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![]));
        s.points.push(ScanPoint::new(1.0, 0.0, 0, vec![on(1, -50, 2_437_000)]));
        s.points.push(ScanPoint::new(2.0, 0.0, 0, vec![on(2, -50, 5_180_000)]));
        s.points.push(ScanPoint::new(3.0, 0.0, 0, vec![on(3, -50, 5_975_000)]));
        s.points.push(ScanPoint::new(4.0, 0.0, 0, vec![on(1, -50, 2_437_000), on(2, -60, 5_180_000)]));
        s.points.push(ScanPoint::new(5.0, 0.0, 0, vec![on(1, -95, 2_437_000)])); // below threshold
        let m = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        let v: Vec<f64> = samples(&s, &m).iter().map(|x| x.value).collect();
        assert_eq!(v, vec![0.0, 1.0, 2.0, 3.0, 4.0, 0.0]);
        assert_eq!(m.palette().unwrap().entries().len(), 5);
    }

    #[test]
    fn phy_coverage_uses_strongest_networks_newest_phy() {
        let mut s = Snapshot::new("s");
        // HT capabilities element (id 45) -> 802.11n; HE capabilities (ext 35) -> ax.
        let ht = vec![45, 0];
        let he = vec![255, 1, 35];
        let on = |last: u8, rssi: i32, ies: Vec<u8>| {
            AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 5_180_000, true, 100, ies)
        };
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![on(1, -50, ht.clone()), on(2, -70, he.clone())]));
        s.points.push(ScanPoint::new(1.0, 0.0, 0, vec![on(1, -70, ht), on(2, -50, he)]));
        s.points.push(ScanPoint::new(2.0, 0.0, 0, vec![]));
        let v: Vec<f64> = samples(&s, &Metric::PhyCoverage(vec![])).iter().map(|x| x.value).collect();
        assert_eq!(v, vec![2.0, 4.0, 0.0]);
    }

    #[test]
    fn speed_metric_routes_through_the_speed_samples() {
        let mut s = snapshot();
        s.points[1].speed = vec![openspot_model::SpeedResult { http_down_mbps: Some(42.0), ..openspot_model::SpeedResult::new("n") }];
        let m = Metric::Speed { kind: SpeedKind::HttpDown, ssids: vec![] };
        let v = samples(&s, &m);
        assert_eq!(v.len(), 1, "only the measured point yields a sample");
        assert_eq!((v[0].x, v[0].value), (1.0, 42.0));
        assert_eq!(m.unit(), "Mbps");
        assert!(!m.lower_is_better());
        assert!(Metric::Speed { kind: SpeedKind::UdpJitter, ssids: vec![] }.lower_is_better());
        assert!(m.palette().is_none() && m.default_thresholds().is_none());
    }

    #[test]
    fn ap_count_respects_threshold() {
        let c = samples(&snapshot(), &Metric::ApCount { min_rssi_dbm: -70 });
        assert_eq!((c[0].value, c[1].value), (1.0, 1.0));
        let c = samples(&snapshot(), &Metric::ApCount { min_rssi_dbm: -90 });
        assert_eq!((c[0].value, c[1].value), (2.0, 1.0));
    }

    #[test]
    fn tx_rate_metric_routes_through_its_samples_and_is_continuous_higher_is_better() {
        let mut s = snapshot();
        s.points[1].link = Some(openspot_model::LinkInfo { bssid: MacAddress([2; 6]), tx_rate_mbps: 144.4, rx_rate_mbps: 130.0 });
        let v = samples(&s, &Metric::TxRate);
        assert_eq!(v.len(), 1, "the point without a link yields no sample");
        assert_eq!((v[0].x, v[0].value), (1.0, 144.4));
        let m = Metric::TxRate;
        assert_eq!(m.unit(), "Mbps");
        assert!(!m.lower_is_better());
        assert!(m.palette().is_none(), "continuous, drawn with a gradient");
        assert!(m.default_thresholds().is_none());
        assert_eq!(m.gradient().range(), (0.0, 1200.0));
        assert_eq!(crate::metric_key(&m), "TxRate");
    }
}
