//! Adapter hot-plug and radio notifications (S-05).

use std::any::Any;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

/// `wlan_notification_acm_interface_arrival`.
pub const ACM_INTERFACE_ARRIVAL: i32 = 13;
/// `wlan_notification_acm_interface_removal`.
pub const ACM_INTERFACE_REMOVAL: i32 = 14;
/// `wlan_notification_msm_radio_state_change`.
#[cfg_attr(not(windows), allow(dead_code))] // consumed by the Windows backend only
pub const MSM_RADIO_STATE_CHANGE: i32 = 7;
/// `dot11_radio_state_on`; 0 is unknown and 2 is off.
pub const DOT11_RADIO_STATE_ON: i32 = 1;

/// A change in the set or power state of wireless adapters. `id` matches [`crate::Adapter::id`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WlanEvent {
    AdapterArrived {
        id: String,
    },
    AdapterRemoved {
        id: String,
    },
    /// The radio was switched on or off (software toggle, airplane mode or hardware switch).
    RadioChanged {
        id: String,
        on: bool,
    },
}

/// Maps an ACM notification code to an event, ignoring every other code.
pub fn classify_acm(code: i32, id: &str) -> Option<WlanEvent> {
    match code {
        ACM_INTERFACE_ARRIVAL => Some(WlanEvent::AdapterArrived { id: id.to_owned() }),
        ACM_INTERFACE_REMOVAL => Some(WlanEvent::AdapterRemoved { id: id.to_owned() }),
        _ => None,
    }
}

/// A radio is usable only when both the software and the hardware switch are on; an
/// unknown state is treated as off rather than guessed on.
pub fn radio_is_on(software_state: i32, hardware_state: i32) -> bool {
    software_state == DOT11_RADIO_STATE_ON && hardware_state == DOT11_RADIO_STATE_ON
}

/// Receiver for [`WlanEvent`]s. Dropping it unregisters the OS notification.
pub struct EventStream {
    rx: Receiver<WlanEvent>,
    // Held only for its Drop (unregisters the native callback).
    _registration: Box<dyn Any + Send>,
}

impl fmt::Debug for EventStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

impl EventStream {
    pub fn new(rx: Receiver<WlanEvent>, registration: Box<dyn Any + Send>) -> Self {
        Self {
            rx,
            _registration: registration,
        }
    }

    /// Returns a pending event without blocking.
    pub fn try_recv(&self) -> Result<WlanEvent, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<WlanEvent, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrival_and_removal_codes_classify_and_others_are_ignored() {
        assert_eq!(
            classify_acm(13, "g"),
            Some(WlanEvent::AdapterArrived { id: "g".into() })
        );
        assert_eq!(
            classify_acm(14, "g"),
            Some(WlanEvent::AdapterRemoved { id: "g".into() })
        );
        assert_eq!(
            classify_acm(7, "g"),
            None,
            "scan-complete is not a hot-plug event"
        );
        assert_eq!(classify_acm(0, "g"), None);
    }

    #[test]
    fn radio_is_on_only_when_both_switches_are_on() {
        assert!(radio_is_on(1, 1));
        assert!(!radio_is_on(1, 2), "hardware switch off");
        assert!(!radio_is_on(2, 1), "software off");
        assert!(!radio_is_on(0, 1), "unknown is not on");
    }

    #[test]
    fn stream_forwards_events_and_runs_registration_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Guard(Arc<AtomicBool>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let stream = EventStream::new(rx, Box::new(Guard(dropped.clone())));
        tx.send(WlanEvent::RadioChanged {
            id: "g".into(),
            on: false,
        })
        .unwrap();
        assert_eq!(
            stream.try_recv(),
            Ok(WlanEvent::RadioChanged {
                id: "g".into(),
                on: false
            })
        );
        assert!(stream.try_recv().is_err());
        drop(stream);
        assert!(dropped.load(Ordering::SeqCst));
    }
}
