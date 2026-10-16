//! Friendly access-point names that belong to one project and travel with its file.

use std::collections::BTreeMap;

use openspot_wlan::MacAddress;

/// Per-project aliases (I-06). The application-wide alias list lives in the user's
/// preferences instead; which of the two is in use is a preference, so this type holds only
/// the data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectAliases {
    /// Non-blank name per BSSID.
    pub by_bssid: BTreeMap<MacAddress, String>,
}

impl ProjectAliases {
    pub fn is_empty(&self) -> bool {
        self.by_bssid.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_bssid.len()
    }
}
