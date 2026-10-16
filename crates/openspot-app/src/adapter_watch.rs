//! Reacts to adapter hot-plug and radio power events (S-05): keeps the adapter list current,
//! decides which adapter stays active and words the notices and the radio-off banner.
//!
//! Everything here is pure; the worker thread feeds it events and the app applies the result.

use std::collections::BTreeSet;

use openspot_wlan::{Adapter, WlanEvent};

/// What to do with the adapter the worker is scanning on after the list changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveAdapter {
    /// Still present, now at this index of the new list.
    Keep(usize),
    /// The previous one is gone (or there was none); scan on the adapter at this index.
    Switch(usize),
    /// No adapter left to scan on.
    None,
}

/// Picks the active adapter in `adapters`, preferring the one with `active_id`.
pub fn choose_active(adapters: &[Adapter], active_id: Option<&str>) -> ActiveAdapter {
    match active_id.and_then(|id| adapters.iter().position(|a| a.id == id)) {
        Some(i) => ActiveAdapter::Keep(i),
        None if adapters.is_empty() => ActiveAdapter::None,
        None => ActiveAdapter::Switch(0),
    }
}

/// The result of one event.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Update {
    /// The adapter list to install, when the event changed it.
    pub adapters: Option<Vec<Adapter>>,
    /// One line for the status bar.
    pub notice: Option<String>,
}

/// Radio power state of the adapters seen so far.
#[derive(Debug, Default)]
pub struct AdapterWatch {
    radio_off: BTreeSet<String>,
}

fn describe<'a>(adapters: &'a [Adapter], id: &'a str) -> &'a str {
    adapters.iter().find(|a| a.id == id).map_or(id, |a| a.description.as_str())
}

impl AdapterWatch {
    /// Applies `event`. `previous` is the list the UI currently shows; `listing` is the
    /// provider's list taken right after the event, `None` when it could not be read.
    ///
    /// Repeated or unknown notifications (the OS may send several for one change) produce no
    /// update, so the user is not told twice.
    pub fn apply(&mut self, previous: &[Adapter], event: &WlanEvent, listing: Option<Vec<Adapter>>) -> Update {
        match event {
            WlanEvent::AdapterArrived { id } => {
                let Some(listing) = listing else { return Update::default() };
                if previous.iter().any(|a| a.id == *id) || !listing.iter().any(|a| a.id == *id) {
                    return Update::default();
                }
                let notice = format!("Wi-Fi adapter connected: {}", describe(&listing, id));
                Update { adapters: Some(listing), notice: Some(notice) }
            }
            WlanEvent::AdapterRemoved { id } => {
                if !previous.iter().any(|a| a.id == *id) {
                    return Update::default();
                }
                self.radio_off.remove(id);
                // A failed re-listing still has to drop the adapter that just went away.
                let rest = listing.unwrap_or_else(|| previous.iter().filter(|a| a.id != *id).cloned().collect());
                let notice = format!("Wi-Fi adapter removed: {}", describe(previous, id));
                Update { adapters: Some(rest), notice: Some(notice) }
            }
            WlanEvent::RadioChanged { id, on } => {
                let changed = if *on { self.radio_off.remove(id) } else { self.radio_off.insert(id.clone()) };
                if !changed {
                    return Update::default();
                }
                let state = if *on { "on" } else { "off" };
                Update { adapters: None, notice: Some(format!("Wi-Fi radio turned {state}: {}", describe(previous, id))) }
            }
        }
    }

    pub fn radio_is_off(&self, adapter_id: &str) -> bool {
        self.radio_off.contains(adapter_id)
    }

    /// Persistent banner text for the state of the active adapter, if anything is wrong.
    pub fn banner(&self, active: Option<&Adapter>) -> Option<String> {
        match active {
            None => Some("No Wi-Fi adapter is available. Scanning resumes when one is plugged in.".to_owned()),
            Some(a) if self.radio_is_off(&a.id) => Some(format!(
                "The Wi-Fi radio of {} is switched off (airplane mode or a hardware switch). Scanning resumes when it is turned on.",
                a.description
            )),
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(n: u8) -> Adapter {
        Adapter { id: format!("id-{n}"), description: format!("Adapter {n}") }
    }

    fn arrived(n: u8) -> WlanEvent {
        WlanEvent::AdapterArrived { id: adapter(n).id }
    }

    fn removed(n: u8) -> WlanEvent {
        WlanEvent::AdapterRemoved { id: adapter(n).id }
    }

    fn radio(n: u8, on: bool) -> WlanEvent {
        WlanEvent::RadioChanged { id: adapter(n).id, on }
    }

    #[test]
    fn arrival_installs_the_new_listing_and_announces_the_adapter() {
        let mut w = AdapterWatch::default();
        let u = w.apply(&[adapter(1)], &arrived(2), Some(vec![adapter(1), adapter(2)]));
        assert_eq!(u.adapters, Some(vec![adapter(1), adapter(2)]));
        assert_eq!(u.notice.as_deref(), Some("Wi-Fi adapter connected: Adapter 2"));
    }

    #[test]
    fn removal_drops_the_adapter_and_names_it_from_the_old_list() {
        let mut w = AdapterWatch::default();
        let u = w.apply(&[adapter(1), adapter(2)], &removed(2), Some(vec![adapter(1)]));
        assert_eq!(u.adapters, Some(vec![adapter(1)]));
        assert_eq!(u.notice.as_deref(), Some("Wi-Fi adapter removed: Adapter 2"));
    }

    #[test]
    fn removal_still_works_when_the_provider_cannot_be_listed() {
        let mut w = AdapterWatch::default();
        let u = w.apply(&[adapter(1), adapter(2)], &removed(1), None);
        assert_eq!(u.adapters, Some(vec![adapter(2)]));
    }

    #[test]
    fn arrival_without_a_listing_cannot_invent_an_adapter() {
        let mut w = AdapterWatch::default();
        assert_eq!(w.apply(&[adapter(1)], &arrived(2), None), Update::default());
    }

    #[test]
    fn arrival_the_listing_does_not_show_yet_changes_nothing() {
        let mut w = AdapterWatch::default();
        assert_eq!(w.apply(&[adapter(1)], &arrived(2), Some(vec![adapter(1)])), Update::default());
    }

    #[test]
    fn duplicate_and_unknown_notifications_are_silent() {
        let mut w = AdapterWatch::default();
        let both = vec![adapter(1), adapter(2)];
        assert_eq!(w.apply(&both, &arrived(2), Some(both.clone())), Update::default(), "already known");
        assert_eq!(w.apply(&[adapter(1)], &removed(2), Some(vec![adapter(1)])), Update::default(), "never shown");
        w.apply(&both, &radio(1, false), None);
        assert_eq!(w.apply(&both, &radio(1, false), None), Update::default(), "radio already off");
        assert_eq!(w.apply(&both, &radio(2, true), None), Update::default(), "radio was never off");
    }

    #[test]
    fn radio_off_raises_the_banner_and_on_clears_it() {
        let mut w = AdapterWatch::default();
        let list = [adapter(1)];
        assert_eq!(w.banner(Some(&list[0])), None);
        let u = w.apply(&list, &radio(1, false), None);
        assert_eq!(u.notice.as_deref(), Some("Wi-Fi radio turned off: Adapter 1"));
        assert_eq!(u.adapters, None, "a power change does not touch the list");
        assert!(w.banner(Some(&list[0])).is_some_and(|b| b.contains("Adapter 1") && b.contains("switched off")));
        let u = w.apply(&list, &radio(1, true), None);
        assert_eq!(u.notice.as_deref(), Some("Wi-Fi radio turned on: Adapter 1"));
        assert_eq!(w.banner(Some(&list[0])), None);
    }

    #[test]
    fn the_banner_is_about_the_active_adapter_only() {
        let mut w = AdapterWatch::default();
        w.apply(&[adapter(1), adapter(2)], &radio(2, false), None);
        assert_eq!(w.banner(Some(&adapter(1))), None);
        assert!(w.banner(Some(&adapter(2))).is_some());
    }

    #[test]
    fn removing_an_adapter_forgets_its_radio_state() {
        let mut w = AdapterWatch::default();
        w.apply(&[adapter(1), adapter(2)], &radio(2, false), None);
        w.apply(&[adapter(1), adapter(2)], &removed(2), Some(vec![adapter(1)]));
        assert!(!w.radio_is_off(&adapter(2).id), "a re-plugged adapter starts with its radio on");
    }

    #[test]
    fn no_adapter_shows_its_own_banner() {
        let w = AdapterWatch::default();
        assert!(w.banner(None).is_some_and(|b| b.contains("No Wi-Fi adapter")));
    }

    #[test]
    fn active_adapter_follows_its_id_through_list_changes() {
        let list = [adapter(1), adapter(2), adapter(3)];
        assert_eq!(choose_active(&list, Some("id-3")), ActiveAdapter::Keep(2));
        assert_eq!(choose_active(&list[1..], Some("id-3")), ActiveAdapter::Keep(1), "index shifts, identity does not");
        assert_eq!(choose_active(&list[1..], Some("id-1")), ActiveAdapter::Switch(0), "active adapter removed");
        assert_eq!(choose_active(&list, None), ActiveAdapter::Switch(0), "recovering from no adapter");
        assert_eq!(choose_active(&[], Some("id-1")), ActiveAdapter::None);
    }
}
