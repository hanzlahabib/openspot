use thiserror::Error;

/// `ERROR_ACCESS_DENIED`: Windows 11 24H2+ returns it from WLAN calls when the app may
/// not read location-gated Wi-Fi data.
pub const WIN32_ACCESS_DENIED: u32 = 5;
/// `ERROR_INVALID_STATE`: `WlanQueryInterface` on a disconnected interface.
#[cfg_attr(not(windows), allow(dead_code))] // consumed by the Windows backend only
pub const WIN32_INVALID_STATE: u32 = 5023;
/// `ERROR_SERVICE_NOT_ACTIVE`: the "WLAN AutoConfig" service is stopped.
pub const WIN32_SERVICE_NOT_ACTIVE: u32 = 1062;
/// `ERROR_NDIS_DOT11_POWER_STATE_INVALID`: the radio is switched off (airplane mode or
/// hardware switch).
pub const WIN32_NDIS_POWER_STATE_INVALID: u32 = 0x8034_2002;

/// Failures a provider can report. Mirrors the conditions users need to act on.
#[derive(Debug, Error)]
pub enum WlanError {
    #[error("no native Wi-Fi backend for this operating system yet")]
    UnsupportedPlatform,
    #[error("this provider does not support {0}")]
    Unsupported(&'static str),
    #[error("the WLAN service is not running (start the \"WLAN AutoConfig\" service)")]
    ServiceDisabled,
    #[error("no wireless adapters found")]
    NoAdapters,
    #[error("adapter \"{0}\" not found")]
    AdapterNotFound(String),
    #[error("access denied; on Windows 11 24H2+ enable Location services for desktop apps")]
    AccessDenied,
    #[error("insufficient privileges to control the Wi-Fi adapter (CAP_NET_ADMIN is required to trigger scans)")]
    PermissionDenied,
    #[error("the Wi-Fi radio is switched off")]
    RadioOff,
    #[error("scan timed out waiting for the driver")]
    ScanTimeout,
    #[error("the driver reported that the scan failed (reason {0})")]
    ScanFailed(u32),
    #[error("invalid Wi-Fi profile: {0}")]
    InvalidProfile(String),
    #[error("could not connect (reason {0})")]
    ConnectFailed(u32),
    #[error("timed out waiting for the connection to complete")]
    ConnectTimeout,
    #[error("operating system error {code}: {context}")]
    Os { code: u32, context: &'static str },
}

impl WlanError {
    /// Maps a Win32/NDIS error code from a WLAN call to the typed error users can act on.
    pub fn from_win32(code: u32, context: &'static str) -> Self {
        match code {
            WIN32_SERVICE_NOT_ACTIVE => Self::ServiceDisabled,
            WIN32_ACCESS_DENIED => Self::AccessDenied,
            WIN32_NDIS_POWER_STATE_INVALID => Self::RadioOff,
            _ => Self::Os { code, context },
        }
    }

    /// A concrete next step for the user, when one exists.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::AccessDenied => Some(
                "Open Settings > Privacy & security > Location, turn on \"Location services\" and \
                 \"Let desktop apps access your location\", then run again.",
            ),
            Self::ServiceDisabled => Some(
                "Run \"net start wlansvc\" from an elevated prompt, or start \"WLAN AutoConfig\" in services.msc.",
            ),
            Self::PermissionDenied => Some(
                "Run as root, or grant the binary the capability: sudo setcap cap_net_admin+ep $(which openspot).",
            ),
            Self::RadioOff => {
                Some("Turn Wi-Fi on (Quick Settings, airplane mode, or the hardware switch).")
            }
            Self::NoAdapters => Some("Plug in or enable a Wi-Fi adapter."),
            _ => None,
        }
    }
}

/// Hint to show when a scan succeeded but found nothing. Windows 11 24H2+ silently returns
/// empty (or connected-network-only) results when location access is off, so an empty list
/// is ambiguous; this names the usual cause without claiming it.
pub const EMPTY_SCAN_HINT: &str = "The scan returned no access points. If Wi-Fi networks are in range, \
     location access may be off: enable Settings > Privacy & security > Location > \
     \"Let desktop apps access your location\" (Windows 11 24H2+).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win32_codes_map_to_typed_errors() {
        assert!(matches!(
            WlanError::from_win32(5, "x"),
            WlanError::AccessDenied
        ));
        assert!(matches!(
            WlanError::from_win32(1062, "x"),
            WlanError::ServiceDisabled
        ));
        assert!(matches!(
            WlanError::from_win32(0x8034_2002, "x"),
            WlanError::RadioOff
        ));
        assert!(matches!(
            WlanError::from_win32(87, "WlanScan"),
            WlanError::Os {
                code: 87,
                context: "WlanScan"
            }
        ));
    }

    #[test]
    fn actionable_errors_carry_hints_and_others_do_not() {
        assert!(
            WlanError::AccessDenied
                .hint()
                .is_some_and(|h| h.contains("Location"))
        );
        assert!(
            WlanError::ServiceDisabled
                .hint()
                .is_some_and(|h| h.contains("wlansvc"))
        );
        assert!(WlanError::RadioOff.hint().is_some());
        assert!(WlanError::PermissionDenied.hint().is_some_and(|h| h.contains("setcap")));
        assert!(WlanError::ScanTimeout.hint().is_none());
        assert!(
            WlanError::Os {
                code: 1,
                context: "c"
            }
            .hint()
            .is_none()
        );
    }
}
