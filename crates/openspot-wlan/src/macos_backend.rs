//! macOS backend over CoreWLAN (`CWWiFiClient` / `CWInterface`).
//!
//! UNVERIFIED at runtime: no Mac was available. It is type-checked with
//! `cargo check --target aarch64-apple-darwin -p openspot-wlan`, and the conversions it
//! relies on are unit-tested in [`crate::corewlan`].
//!
//! Limits (from Apple's documentation): scanning blocks until the OS finishes, so
//! `probe_timeout` cannot be applied; and macOS 14+ withholds SSID/BSSID unless the app is
//! authorised for Location Services, which surfaces as [`WlanError::AccessDenied`].

use objc2_core_wlan::{CWInterface, CWNetwork, CWSecurity, CWWiFiClient};
use objc2_foundation::NSString;

use crate::connection::{Connection, ConnectionState};
use crate::corewlan::{check_not_redacted, frequency_khz, parse_bssid};
use crate::error::WlanError;
use crate::merge::merge_probes;
use crate::model::{AccessPoint, Adapter};
use crate::{ScanOptions, WlanProvider};

#[derive(Debug, Default)]
pub(crate) struct MacosProvider;

impl MacosProvider {
    pub(crate) fn open() -> Result<Self, WlanError> {
        let provider = Self;
        provider.adapters()?;
        Ok(provider)
    }

    fn interface(&self, adapter: &Adapter) -> Result<objc2::rc::Retained<CWInterface>, WlanError> {
        // SAFETY: CWWiFiClient is thread-safe to message; `interfaces` returns retained objects.
        let list = unsafe { CWWiFiClient::sharedWiFiClient().interfaces() }.ok_or(WlanError::NoAdapters)?;
        list.iter()
            // SAFETY: `interfaceName` is a plain getter on a live CWInterface.
            .find(|i| unsafe { i.interfaceName() }.is_some_and(|n| n.to_string() == adapter.id))
            .ok_or_else(|| WlanError::AdapterNotFound(adapter.id.clone()))
    }
}

fn access_point(network: &CWNetwork) -> Option<AccessPoint> {
    // SAFETY: plain property getters on a live CWNetwork.
    unsafe {
        let bssid = parse_bssid(&network.bssid()?.to_string())?;
        let channel = network.wlanChannel()?;
        let freq = frequency_khz(channel.channelBand().0 as i64, channel.channelNumber() as i64)?;
        let ssid = network.ssid().map(|s| s.to_string()).unwrap_or_default();
        let ies = network.informationElementData().map(|d| d.to_vec()).unwrap_or_default();
        Some(AccessPoint::from_beacon(
            ssid,
            bssid,
            network.rssiValue() as i32,
            freq,
            // CoreWLAN exposes no capability field; "not open" is the Privacy bit's meaning.
            !network.supportsSecurity(CWSecurity::None),
            u16::try_from(network.beaconInterval()).unwrap_or(0),
            ies,
        ))
    }
}

impl WlanProvider for MacosProvider {
    fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
        // SAFETY: see `interface`.
        let list = unsafe { CWWiFiClient::sharedWiFiClient().interfaces() }.ok_or(WlanError::NoAdapters)?;
        let adapters: Vec<Adapter> = list
            .iter()
            // SAFETY: plain getter.
            .filter_map(|i| unsafe { i.interfaceName() }.map(|n| n.to_string()))
            .map(|name| Adapter { id: name.clone(), description: name })
            .collect();
        if adapters.is_empty() { Err(WlanError::NoAdapters) } else { Ok(adapters) }
    }

    fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
        let interface = self.interface(adapter)?;
        // SAFETY: plain getter.
        if !unsafe { interface.powerOn() } {
            return Err(WlanError::RadioOff);
        }
        let mut probes = Vec::new();
        for _ in 0..options.probes.max(1) {
            // SAFETY: blocking scan on a live interface; `None` name = undirected scan, and
            // `includeHidden` asks CoreWLAN to keep hidden networks in the results (S-04).
            let found = unsafe { interface.scanForNetworksWithName_includeHidden_error(None::<&NSString>, true) }
                .map_err(|_| WlanError::Os { code: 0, context: "CWInterface scan" })?;
            let networks: Vec<_> = found.iter().collect();
            let aps: Vec<AccessPoint> = networks.iter().filter_map(|n| access_point(n)).collect();
            check_not_redacted(networks.len(), aps.len())?;
            probes.push(aps);
        }
        Ok(merge_probes(probes))
    }

    fn current_connection(&self, adapter: &Adapter) -> Result<Option<Connection>, WlanError> {
        let interface = self.interface(adapter)?;
        // SAFETY: plain property getters on a live CWInterface.
        unsafe {
            // The channel is not location-gated, so it tells associated from idle.
            let Some(channel) = interface.wlanChannel() else { return Ok(None) };
            let bssid = interface.bssid().and_then(|b| parse_bssid(&b.to_string())).ok_or(WlanError::AccessDenied)?;
            let rate = interface.transmitRate();
            Ok(Some(Connection {
                state: ConnectionState::Connected,
                ssid: interface.ssid().map(|s| s.to_string()).unwrap_or_default(),
                bssid,
                phy: None,
                rx_rate_mbps: rate,
                tx_rate_mbps: rate,
                signal_quality: 0,
                rssi_dbm: interface.rssiValue() as i32,
                channel: u32::try_from(channel.channelNumber()).ok(),
            }))
        }
    }
}
