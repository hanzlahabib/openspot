//! Per-point network switching with a guaranteed way back (roadmap A-01).
//!
//! [`ConnectionGuard`] remembers which profile the adapter was using, connects to each target
//! network in turn, and on [`ConnectionGuard::restore`] (or drop) reconnects the previous
//! profile, or disconnects if there was none, and deletes any temporary profile it created.
//! The OS calls live behind default-`Unsupported` methods on [`WlanProvider`], so the state
//! machine is tested here with a recording fake.
//!
//! UNVERIFIED on hardware: the Windows implementation (`windows_connect`) has only been
//! compiled, never run against a radio.

use std::time::Duration;

use crate::error::WlanError;
use crate::model::Adapter;
use crate::profile_xml::{ProfileSecurity, build_profile, profile_name};
use crate::WlanProvider;

/// Time allowed for association plus authentication (WPA 4-way handshake, DHCP excluded).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectTarget {
    /// A profile the user already saved in Windows (its name; usually the SSID). No key is
    /// handled by OpenSpot.
    SavedProfile(String),
    /// A temporary OpenSpot-owned profile built from these credentials and removed on restore.
    Temporary { ssid: String, security: ProfileSecurity },
}

/// Restores the adapter's previous connection when dropped.
#[must_use = "dropping the guard immediately restores the previous connection"]
pub struct ConnectionGuard<'a> {
    provider: &'a dyn WlanProvider,
    adapter: Adapter,
    /// Profile in use when the guard was created; `None` = was not connected.
    previous: Option<String>,
    /// Temporary profile we created and must delete.
    created: Option<String>,
    restored: bool,
}

impl std::fmt::Debug for ConnectionGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionGuard")
            .field("adapter", &self.adapter.id)
            .field("previous", &self.previous)
            .field("created", &self.created)
            .field("restored", &self.restored)
            .finish()
    }
}

impl<'a> ConnectionGuard<'a> {
    /// Records the current connection of `adapter`. Fails with [`WlanError::Unsupported`] on
    /// backends without connection management, before anything has been changed.
    pub fn begin(provider: &'a dyn WlanProvider, adapter: &Adapter) -> Result<Self, WlanError> {
        let previous = provider.connected_profile(adapter)?;
        Ok(Self { provider, adapter: adapter.clone(), previous, created: None, restored: false })
    }

    /// Profile that will be restored.
    pub fn previous_profile(&self) -> Option<&str> {
        self.previous.as_deref()
    }

    /// Connects to `target`, replacing the previous target of this guard. On failure the
    /// guard stays usable (the original connection is still restored at the end).
    pub fn connect(&mut self, target: &ConnectTarget, timeout: Duration) -> Result<(), WlanError> {
        self.delete_created();
        let name = match target {
            ConnectTarget::SavedProfile(name) => name.clone(),
            ConnectTarget::Temporary { ssid, security } => {
                let xml = build_profile(ssid, security)?;
                self.provider.set_profile(&self.adapter, &xml)?;
                let name = profile_name(ssid);
                self.created = Some(name.clone());
                name
            }
        };
        let result = self.provider.connect_profile(&self.adapter, &name, timeout);
        if result.is_err() {
            self.delete_created();
        }
        result
    }

    fn delete_created(&mut self) -> Option<WlanError> {
        let name = self.created.take()?;
        self.provider.delete_profile(&self.adapter, &name).err()
    }

    /// Reconnects the previous profile (or disconnects), deletes temporary profiles, and
    /// reports the first error. Safe to call twice; every step runs even if an earlier one fails.
    pub fn restore(&mut self) -> Result<(), WlanError> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        let back = match &self.previous {
            Some(name) => self.provider.connect_profile(&self.adapter, name, DEFAULT_CONNECT_TIMEOUT),
            None => self.provider.disconnect(&self.adapter),
        };
        let cleanup = self.delete_created();
        back.and_then(|()| cleanup.map_or(Ok(()), Err))
    }
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        // Errors cannot be returned from drop; callers that care call `restore` first.
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{AccessPoint, ScanOptions};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Query,
        SetProfile(String),
        Connect(String),
        DeleteProfile(String),
        Disconnect,
    }

    /// Records every connection call. `current` is updated by connects/disconnects so the
    /// final state can be asserted, not just the call list.
    struct Recorder {
        calls: Mutex<Vec<Call>>,
        current: Mutex<Option<String>>,
        fail_connect_to: Option<&'static str>,
    }

    impl Recorder {
        fn new(current: Option<&str>) -> Self {
            Self { calls: Mutex::default(), current: Mutex::new(current.map(str::to_owned)), fail_connect_to: None }
        }
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
        fn log(&self, c: Call) {
            self.calls.lock().unwrap().push(c);
        }
    }

    impl WlanProvider for Recorder {
        fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
            Ok(vec![adapter()])
        }
        fn scan(&self, _: &Adapter, _: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
            Ok(vec![])
        }
        fn connected_profile(&self, _: &Adapter) -> Result<Option<String>, WlanError> {
            self.log(Call::Query);
            Ok(self.current.lock().unwrap().clone())
        }
        fn set_profile(&self, _: &Adapter, xml: &str) -> Result<(), WlanError> {
            let name = xml.split("<name>").nth(1).and_then(|s| s.split("</name>").next()).unwrap().to_owned();
            self.log(Call::SetProfile(name));
            Ok(())
        }
        fn connect_profile(&self, _: &Adapter, name: &str, _: Duration) -> Result<(), WlanError> {
            self.log(Call::Connect(name.into()));
            if self.fail_connect_to == Some(name) {
                return Err(WlanError::ConnectFailed(7));
            }
            *self.current.lock().unwrap() = Some(name.into());
            Ok(())
        }
        fn delete_profile(&self, _: &Adapter, name: &str) -> Result<(), WlanError> {
            self.log(Call::DeleteProfile(name.into()));
            Ok(())
        }
        fn disconnect(&self, _: &Adapter) -> Result<(), WlanError> {
            self.log(Call::Disconnect);
            *self.current.lock().unwrap() = None;
            Ok(())
        }
    }

    fn adapter() -> Adapter {
        Adapter { id: "a".into(), description: String::new() }
    }

    fn saved(n: &str) -> ConnectTarget {
        ConnectTarget::SavedProfile(n.into())
    }

    const T: Duration = Duration::from_secs(1);

    #[test]
    fn connect_then_drop_restores_the_previous_profile() {
        let p = Recorder::new(Some("Home"));
        {
            let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
            assert_eq!(g.previous_profile(), Some("Home"));
            g.connect(&saved("Office"), T).unwrap();
            assert_eq!(p.current.lock().unwrap().as_deref(), Some("Office"));
        }
        assert_eq!(p.current.lock().unwrap().as_deref(), Some("Home"));
        assert_eq!(p.calls(), [Call::Query, Call::Connect("Office".into()), Call::Connect("Home".into())]);
    }

    #[test]
    fn without_a_previous_connection_restore_disconnects() {
        let p = Recorder::new(None);
        {
            let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
            g.connect(&saved("Office"), T).unwrap();
        }
        assert_eq!(p.current.lock().unwrap().as_deref(), None);
        assert_eq!(p.calls().last(), Some(&Call::Disconnect));
    }

    #[test]
    fn switching_between_targets_keeps_the_original_as_the_restore_point() {
        let p = Recorder::new(Some("Home"));
        {
            let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
            g.connect(&saved("A"), T).unwrap();
            g.connect(&saved("B"), T).unwrap();
        }
        assert_eq!(p.current.lock().unwrap().as_deref(), Some("Home"));
        assert_eq!(p.calls().iter().filter(|c| **c == Call::Connect("Home".into())).count(), 1);
    }

    #[test]
    fn temporary_profile_is_created_connected_and_deleted() {
        let p = Recorder::new(Some("Home"));
        {
            let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
            let t = ConnectTarget::Temporary { ssid: "Lab".into(), security: ProfileSecurity::Open };
            g.connect(&t, T).unwrap();
        }
        assert_eq!(
            p.calls(),
            [
                Call::Query,
                Call::SetProfile("OpenSpot-Lab".into()),
                Call::Connect("OpenSpot-Lab".into()),
                Call::Connect("Home".into()),
                Call::DeleteProfile("OpenSpot-Lab".into()),
            ]
        );
    }

    #[test]
    fn failed_connect_removes_the_temp_profile_and_restore_still_runs() {
        let mut p = Recorder::new(Some("Home"));
        p.fail_connect_to = Some("OpenSpot-Lab");
        {
            let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
            let t = ConnectTarget::Temporary { ssid: "Lab".into(), security: ProfileSecurity::Open };
            assert!(matches!(g.connect(&t, T), Err(WlanError::ConnectFailed(7))));
            assert!(p.calls().contains(&Call::DeleteProfile("OpenSpot-Lab".into())));
        }
        assert_eq!(p.current.lock().unwrap().as_deref(), Some("Home"));
    }

    #[test]
    fn invalid_credentials_fail_before_touching_the_adapter() {
        let p = Recorder::new(Some("Home"));
        let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
        let bad = ConnectTarget::Temporary { ssid: "Lab".into(), security: ProfileSecurity::Wpa2Personal { passphrase: "x".into() } };
        assert!(matches!(g.connect(&bad, T), Err(WlanError::InvalidProfile(_))));
        assert_eq!(p.calls(), [Call::Query]);
    }

    #[test]
    fn explicit_restore_is_idempotent_and_drop_does_not_repeat_it() {
        let p = Recorder::new(Some("Home"));
        let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
        g.connect(&saved("A"), T).unwrap();
        g.restore().unwrap();
        g.restore().unwrap();
        drop(g);
        assert_eq!(p.calls().iter().filter(|c| **c == Call::Connect("Home".into())).count(), 1);
    }

    #[test]
    fn restore_failure_is_reported_but_cleanup_still_happens() {
        let mut p = Recorder::new(Some("Home"));
        p.fail_connect_to = Some("Home");
        let mut g = ConnectionGuard::begin(&p, &adapter()).unwrap();
        let t = ConnectTarget::Temporary { ssid: "Lab".into(), security: ProfileSecurity::Open };
        g.connect(&t, T).unwrap();
        assert!(matches!(g.restore(), Err(WlanError::ConnectFailed(7))));
        assert!(p.calls().contains(&Call::DeleteProfile("OpenSpot-Lab".into())));
    }

    #[test]
    fn backends_without_support_say_so_before_changing_anything() {
        let p = crate::FakeProvider;
        assert!(matches!(ConnectionGuard::begin(&p, &adapter()), Err(WlanError::Unsupported(_))));
    }
}
