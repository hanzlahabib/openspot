use crate::error::WlanError;
use crate::merge::merge_probes;
use crate::model::{AccessPoint, Adapter, MacAddress};
use crate::{ScanOptions, WlanProvider};

/// Deterministic provider for tests, demos and development on platforms without a
/// native backend. RSSI jitters slightly per probe so [`merge_probes`] has work to do.
#[derive(Debug, Default)]
pub struct FakeProvider;

fn el(id: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![id, body.len() as u8];
    v.extend_from_slice(body);
    v
}

fn rsn(akm: u8) -> Vec<u8> {
    let mut b = vec![1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, akm, 0, 0];
    el(48, &std::mem::take(&mut b))
}

fn sample(probe: u8) -> Vec<AccessPoint> {
    let jitter = |base: i32, salt: u8| base + i32::from((probe.wrapping_add(salt)) % 3) - 1;
    let ap = |ssid: &str, last: u8, rssi: i32, khz: u32, ies: Vec<u8>, privacy: bool| {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0x02, 0, 0, 0, 0, last]), rssi, khz, privacy, 100, ies)
    };

    let wifi5 = [
        el(1, &[0x8C, 0x12, 0x98, 0x24]),
        el(45, &[0; 26]),
        el(61, &[36, 0b101, 0, 0, 0, 0]),
        el(191, &[0; 12]),
        el(192, &[1, 42, 0, 0, 0]),
        rsn(2),
    ]
    .concat();
    let wifi6e = [
        el(255, &[&[35u8][..], &[0; 20]].concat()),
        el(255, &[&[36u8][..], &[0, 0, 0x02, 0, 0, 0, 5, 0b10, 7, 0, 0]].concat()),
        rsn(8),
    ]
    .concat();
    let legacy = [el(1, &[0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x24]), el(3, &[1])].concat();

    vec![
        ap("OpenSpot-5G", 1, jitter(-48, 0), 5_180_000, wifi5, true),
        ap("OpenSpot-6E", 2, jitter(-61, 1), 5_975_000, wifi6e, true),
        ap("CoffeeShop", 3, jitter(-77, 2), 2_412_000, legacy, false),
    ]
}

impl WlanProvider for FakeProvider {
    fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
        Ok(vec![Adapter { id: "fake-0".into(), description: "Fake Wi-Fi adapter".into() }])
    }

    fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
        if adapter.id != "fake-0" {
            return Err(WlanError::AdapterNotFound(adapter.id.clone()));
        }
        Ok(merge_probes((0..options.probes.max(1)).map(sample)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Band, PhyMode, SecurityMode};

    #[test]
    fn fake_scan_is_parsed_through_the_real_ie_path() {
        let p = FakeProvider;
        let adapter = p.adapters().unwrap().remove(0);
        let aps = p.scan(&adapter, &ScanOptions { probes: 3, ..Default::default() }).unwrap();
        assert_eq!(aps.len(), 3);
        assert_eq!(aps[0].ssid, "OpenSpot-5G");
        assert_eq!(aps[0].channel, 36);
        assert!(aps[0].phy.contains(&PhyMode::Ac));
        assert!(aps[0].security.contains(&SecurityMode::Wpa2Personal));
        let e = aps.iter().find(|a| a.ssid == "OpenSpot-6E").unwrap();
        assert_eq!(e.band, Some(Band::Ghz6));
        assert!(e.security.contains(&SecurityMode::Wpa3Personal));
        let c = aps.iter().find(|a| a.ssid == "CoffeeShop").unwrap();
        assert!(c.security.contains(&SecurityMode::Open));
        assert!(c.phy.contains(&PhyMode::G) && c.phy.contains(&PhyMode::B));
    }

    #[test]
    fn unknown_adapter_is_an_error() {
        let bad = Adapter { id: "nope".into(), description: String::new() };
        assert!(matches!(FakeProvider.scan(&bad, &ScanOptions::default()), Err(WlanError::AdapterNotFound(_))));
    }
}
