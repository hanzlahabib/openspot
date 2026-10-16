//! Wi-Fi scanning behind a [`WlanProvider`] trait.
//!
//! Backends: Windows (`wlanapi`), Linux (`nl80211`, unverified on hardware),
//! macOS (CoreWLAN, unverified at runtime) and a deterministic [`FakeProvider`] for tests
//! and demos.

// Only the Windows and macOS backends need FFI.
#![cfg_attr(not(any(windows, target_os = "macos")), forbid(unsafe_code))]

mod channels;
mod connect;
mod connection;
// Pure CoreWLAN conversions, compiled everywhere so they are tested on any host.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
mod corewlan;
mod error;
mod events;
mod fake;
pub mod hidden;
#[cfg(target_os = "linux")]
mod linux_backend;
#[cfg(target_os = "macos")]
mod macos_backend;
mod merge;
mod model;
mod profile_xml;
// Decoding is pure byte parsing, so it is compiled (and tested) on every platform.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
mod nl80211;
mod vendor;
#[cfg(windows)]
mod windows_backend;
#[cfg(windows)]
mod windows_connect;

use std::time::Duration;

pub use channels::{CLEAR_DBM, ChannelScore, candidates, recommend};
pub use connect::{ConnectTarget, ConnectionGuard, DEFAULT_CONNECT_TIMEOUT};
pub use connection::{Connection, ConnectionState, kbps_to_mbps, phy_from_dot11, quality_to_dbm};
pub use error::{EMPTY_SCAN_HINT, WlanError};
pub use events::{EventStream, WlanEvent, classify_acm, radio_is_on};
pub use fake::FakeProvider;
pub use merge::merge_probes;
pub use model::{AccessPoint, Adapter, MacAddress, NetworkType};
pub use profile_xml::{PROFILE_PREFIX, ProfileSecurity, build_profile, profile_name};
pub use openspot_ie::{Band, ChannelWidth, PhyMode, SecurityMode};

/// How long a single probe waits for the driver's scan-complete notification.
/// Drivers typically finish in 1-4 s; 8 s covers slow multi-band cards without
/// hanging the UI for long when the radio is stuck.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Parameters for one logical scan (one or more probes merged together).
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Number of back-to-back probes merged into one result. A single probe often
    /// misses APs; 2-3 is a good trade-off between completeness and time.
    pub probes: u8,
    pub probe_timeout: Duration,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            probes: 1,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
        }
    }
}

/// A source of Wi-Fi scan results.
pub trait WlanProvider: Send {
    /// Lists wireless adapters currently present.
    fn adapters(&self) -> Result<Vec<Adapter>, WlanError>;

    /// Scans on the given adapter and returns the merged result of all probes.
    fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError>;

    // Connection management (A-01). Optional: backends that cannot do it keep these defaults,
    // so use [`ConnectionGuard`] rather than calling them directly.

    /// Name of the profile `adapter` is connected with, `None` when not connected.
    fn connected_profile(&self, _adapter: &Adapter) -> Result<Option<String>, WlanError> {
        Err(WlanError::Unsupported("connection management"))
    }

    /// Stores (or replaces) a WLAN profile from its XML.
    fn set_profile(&self, _adapter: &Adapter, _profile_xml: &str) -> Result<(), WlanError> {
        Err(WlanError::Unsupported("connection management"))
    }

    /// Connects with a stored profile and blocks until associated, failed, or `timeout`.
    fn connect_profile(&self, _adapter: &Adapter, _profile: &str, _timeout: Duration) -> Result<(), WlanError> {
        Err(WlanError::Unsupported("connection management"))
    }

    fn delete_profile(&self, _adapter: &Adapter, _profile: &str) -> Result<(), WlanError> {
        Err(WlanError::Unsupported("connection management"))
    }

    fn disconnect(&self, _adapter: &Adapter) -> Result<(), WlanError> {
        Err(WlanError::Unsupported("connection management"))
    }


    /// The network the adapter is associated with, or `None` when it is not connected.
    fn current_connection(&self, _adapter: &Adapter) -> Result<Option<Connection>, WlanError> {
        Err(WlanError::Unsupported("current connection info"))
    }

    /// Subscribes to adapter arrival/removal and radio power changes. Dropping the
    /// returned stream unsubscribes.
    fn subscribe_events(&self) -> Result<EventStream, WlanError> {
        Err(WlanError::Unsupported("adapter event notifications"))
    }
}

/// The native provider for this OS.
pub fn native_provider() -> Result<Box<dyn WlanProvider>, WlanError> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows_backend::WindowsProvider::open()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux_backend::LinuxProvider::open()?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos_backend::MacosProvider::open()?))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err(WlanError::UnsupportedPlatform)
    }
}
