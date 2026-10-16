//! Windows backend over the Native Wifi API (`wlanapi.dll`).
//!
//! Flow per probe: `WlanScan` -> wait for the ACM `ScanComplete`/`ScanFail`
//! notification -> `WlanGetNetworkBssList` -> parse each BSS entry.

use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    L2_NOTIFICATION_DATA, WLAN_BSS_ENTRY, WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_INTERFACE_INFO_LIST, WLAN_INTF_OPCODE, WLAN_NOTIFICATION_ACM,
    WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_MSM, WLAN_NOTIFICATION_SOURCE_NONE,
    WLAN_NOTIFICATION_SOURCES, WLAN_PHY_RADIO_STATE, WlanCloseHandle, WlanEnumInterfaces,
    WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle, WlanQueryInterface,
    WlanRegisterNotification, WlanScan, dot11_BSS_type_any, dot11_BSS_type_independent,
    wlan_intf_opcode_channel_number, wlan_intf_opcode_current_connection, wlan_intf_opcode_rssi,
    wlan_notification_acm_scan_complete, wlan_notification_acm_scan_fail,
};
use windows::core::GUID;

use crate::connection::{
    Connection, ConnectionState, kbps_to_mbps, phy_from_dot11, quality_to_dbm,
};
use crate::error::{WIN32_INVALID_STATE, WlanError};
use crate::events::{EventStream, MSM_RADIO_STATE_CHANGE, WlanEvent, classify_acm, radio_is_on};
use crate::hidden::resolve_from_connection;
use crate::merge::merge_probes;
use crate::model::{AccessPoint, Adapter, MacAddress, NetworkType};
use crate::windows_connect;
use crate::{ScanOptions, WlanProvider};

/// Native Wifi client API version 2 (Vista and later).
const CLIENT_VERSION: u32 = 2;
/// Offset between 802.11 capability-information bit 4 (Privacy) and the mask.
const CAPABILITY_PRIVACY: u16 = 1 << 4;

#[derive(Debug, Clone, Copy)]
enum ScanEvent {
    Complete,
    Failed(u32),
}

/// Notification payload shared with the OS callback thread.
struct Listener {
    interface: GUID,
    tx: Sender<ScanEvent>,
}

#[derive(Debug)]
pub(crate) struct WindowsProvider {
    handle: HANDLE,
}

// The WLAN client handle may be used from any thread; the API serialises calls.
unsafe impl Send for WindowsProvider {}

fn os_err(code: u32, context: &'static str) -> WlanError {
    WlanError::from_win32(code, context)
}

/// Sender shared with the hot-plug callback.
struct EventListener {
    tx: Sender<WlanEvent>,
}

/// A dedicated WLAN handle with the hot-plug callback registered. A separate handle is
/// required because a handle holds a single notification registration, and scans
/// register/unregister on the provider's handle.
struct EventRegistration {
    handle: HANDLE,
    // Keeps the callback context alive until after unregistration in `drop`.
    _listener: Box<EventListener>,
}

// The handle is only used in `drop`; the WLAN API allows any thread.
unsafe impl Send for EventRegistration {}

impl Drop for EventRegistration {
    fn drop(&mut self) {
        // SAFETY: handle is open; unregistering waits for in-flight callbacks, so the
        // listener is no longer referenced when it is dropped after this body.
        unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_NONE,
                true,
                None,
                None,
                None,
                None,
            );
            WlanCloseHandle(self.handle, None);
        }
    }
}

impl WindowsProvider {
    pub(crate) fn open() -> Result<Self, WlanError> {
        let mut negotiated = 0u32;
        let mut handle = HANDLE::default();
        // SAFETY: out-pointers reference live locals.
        let rc = unsafe { WlanOpenHandle(CLIENT_VERSION, None, &mut negotiated, &mut handle) };
        if rc != 0 {
            return Err(os_err(rc, "WlanOpenHandle"));
        }
        Ok(Self { handle })
    }

    fn interfaces(&self) -> Result<Vec<(GUID, Adapter)>, WlanError> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        // SAFETY: handle is open; `list` receives an OS allocation freed below.
        let rc = unsafe { WlanEnumInterfaces(self.handle, None, &mut list) };
        if rc != 0 {
            return Err(os_err(rc, "WlanEnumInterfaces"));
        }
        // SAFETY: on success `list` points to a header followed by `dwNumberOfItems` entries.
        let out = unsafe {
            let l = &*list;
            slice::from_raw_parts(l.InterfaceInfo.as_ptr(), l.dwNumberOfItems as usize)
                .iter()
                .map(|i| {
                    let len = i
                        .strInterfaceDescription
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(0);
                    let description = String::from_utf16_lossy(&i.strInterfaceDescription[..len]);
                    (
                        i.InterfaceGuid,
                        Adapter {
                            id: format!("{:?}", i.InterfaceGuid),
                            description,
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        // SAFETY: allocated by WlanEnumInterfaces.
        unsafe { WlanFreeMemory(list.cast::<c_void>().cast_const()) };
        Ok(out)
    }

    /// Registers for ACM notifications for the duration of `f`, then unregisters.
    fn with_scan_events<T>(
        &self,
        interface: GUID,
        f: impl FnOnce(&Receiver<ScanEvent>) -> Result<T, WlanError>,
    ) -> Result<T, WlanError> {
        let (tx, rx) = mpsc::channel();
        let listener = Box::new(Listener { interface, tx });
        let context = ptr::from_ref::<Listener>(&listener).cast::<c_void>();
        // SAFETY: `listener` outlives the registration: we unregister (which waits for
        // in-flight callbacks) before it is dropped at the end of this function.
        let rc = unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                true,
                Some(on_notification),
                Some(context),
                None,
                None,
            )
        };
        if rc != 0 {
            return Err(os_err(rc, "WlanRegisterNotification"));
        }
        let result = f(&rx);
        // SAFETY: same handle; source NONE removes the registration.
        unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_NONE,
                true,
                None,
                None,
                None,
                None,
            );
        }
        drop(listener);
        result
    }

    /// Reads a fixed-size value with `WlanQueryInterface`; the error is the raw Win32 code.
    fn query<T: Copy>(&self, interface: &GUID, opcode: WLAN_INTF_OPCODE) -> Result<T, u32> {
        let mut size = 0u32;
        let mut data: *mut c_void = ptr::null_mut();
        // SAFETY: handle open; out-pointers reference live locals; `data` is freed below.
        let rc = unsafe {
            WlanQueryInterface(
                self.handle,
                interface,
                opcode,
                None,
                &mut size,
                &mut data,
                None,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        let value = if data.is_null() || (size as usize) < size_of::<T>() {
            None
        } else {
            // SAFETY: the OS returned at least `size_of::<T>()` readable bytes; the read is
            // unaligned-safe and T is plain-old-data (Copy).
            Some(unsafe { data.cast::<T>().read_unaligned() })
        };
        // SAFETY: allocated by WlanQueryInterface.
        unsafe { WlanFreeMemory(data.cast_const()) };
        // ERROR_INVALID_DATA: the payload did not have the size the opcode documents.
        value.ok_or(13)
    }

    fn connection(&self, interface: GUID) -> Result<Option<Connection>, WlanError> {
        let attrs: WLAN_CONNECTION_ATTRIBUTES =
            match self.query(&interface, wlan_intf_opcode_current_connection) {
                Ok(a) => a,
                Err(WIN32_INVALID_STATE) => return Ok(None),
                Err(rc) => return Err(os_err(rc, "WlanQueryInterface(current_connection)")),
            };
        let a = &attrs.wlanAssociationAttributes;
        let quality = u8::try_from(a.wlanSignalQuality.min(100)).unwrap_or(100);
        let ssid_len = (a.dot11Ssid.uSSIDLength as usize).min(a.dot11Ssid.ucSSID.len());
        // The dedicated RSSI opcode is a measurement; fall back to the quality mapping.
        let rssi_dbm = self
            .query::<i32>(&interface, wlan_intf_opcode_rssi)
            .unwrap_or_else(|_| quality_to_dbm(quality));
        let channel = self
            .query::<u32>(&interface, wlan_intf_opcode_channel_number)
            .ok();
        Ok(Some(Connection {
            state: ConnectionState::from_raw(attrs.isState.0),
            ssid: String::from_utf8_lossy(&a.dot11Ssid.ucSSID[..ssid_len]).into_owned(),
            bssid: MacAddress(a.dot11Bssid),
            phy: phy_from_dot11(a.dot11PhyType.0),
            rx_rate_mbps: kbps_to_mbps(a.ulRxRate),
            tx_rate_mbps: kbps_to_mbps(a.ulTxRate),
            signal_quality: quality,
            rssi_dbm,
            channel,
        }))
    }

    fn guid_for(&self, adapter: &Adapter) -> Result<GUID, WlanError> {
        self.interfaces()?
            .into_iter()
            .find(|(_, a)| a.id == adapter.id)
            .map(|(g, _)| g)
            .ok_or_else(|| WlanError::AdapterNotFound(adapter.id.clone()))
    }

    fn probe(&self, interface: GUID, timeout: Duration) -> Result<Vec<AccessPoint>, WlanError> {
        self.with_scan_events(interface, |rx| {
            // SAFETY: handle open, GUID is a live local.
            let rc = unsafe { WlanScan(self.handle, &interface, None, None, None) };
            if rc != 0 {
                return Err(os_err(rc, "WlanScan"));
            }
            match rx.recv_timeout(timeout) {
                Ok(ScanEvent::Complete) => {}
                Ok(ScanEvent::Failed(reason)) => return Err(WlanError::ScanFailed(reason)),
                Err(_) => return Err(WlanError::ScanTimeout),
            }
            self.read_bss_list(interface)
        })
    }

    fn read_bss_list(&self, interface: GUID) -> Result<Vec<AccessPoint>, WlanError> {
        let mut list: *mut WLAN_BSS_LIST = ptr::null_mut();
        // SAFETY: out-pointer is a live local; the list is freed below.
        let rc = unsafe {
            WlanGetNetworkBssList(
                self.handle,
                &interface,
                None,
                dot11_BSS_type_any,
                false,
                None,
                &mut list,
            )
        };
        if rc != 0 {
            return Err(os_err(rc, "WlanGetNetworkBssList"));
        }
        // SAFETY: header followed by `dwNumberOfItems` entries; each entry's IEs live at
        // `ulIeOffset` bytes from the entry start and stay inside the allocation.
        let aps = unsafe {
            let l = &*list;
            slice::from_raw_parts(l.wlanBssEntries.as_ptr(), l.dwNumberOfItems as usize)
                .iter()
                .map(|e| access_point(e))
                .collect::<Vec<_>>()
        };
        // SAFETY: allocated by WlanGetNetworkBssList.
        unsafe { WlanFreeMemory(list.cast::<c_void>().cast_const()) };
        Ok(aps)
    }
}

/// # Safety
/// `e` must be an entry of a live `WLAN_BSS_LIST` so its IE range is readable.
unsafe fn access_point(e: &WLAN_BSS_ENTRY) -> AccessPoint {
    let ssid_len = (e.dot11Ssid.uSSIDLength as usize).min(e.dot11Ssid.ucSSID.len());
    let ssid = String::from_utf8_lossy(&e.dot11Ssid.ucSSID[..ssid_len]).into_owned();
    // SAFETY: caller guarantees the IE blob is inside the list allocation.
    let ies = unsafe {
        let base = ptr::from_ref(e).cast::<u8>().add(e.ulIeOffset as usize);
        slice::from_raw_parts(base, e.ulIeSize as usize).to_vec()
    };
    AccessPoint::from_beacon(
        ssid,
        MacAddress(e.dot11Bssid),
        e.lRssi,
        e.ulChCenterFrequency,
        e.usCapabilityInformation & CAPABILITY_PRIVACY != 0,
        e.usBeaconPeriod,
        ies,
    )
    .with_network_type(if e.dot11BssType == dot11_BSS_type_independent {
        NetworkType::AdHoc
    } else {
        NetworkType::Infrastructure
    })
}

/// Runs on an OS thread pool thread; must not panic or block.
unsafe extern "system" fn on_notification(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    // SAFETY: the OS passes a valid notification; `context` is our `Listener`, alive
    // until the registration is removed.
    let (data, listener) = unsafe { (&*data, &*context.cast::<Listener>()) };
    if data.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM
        || data.InterfaceGuid != listener.interface
    {
        return;
    }
    let event = match WLAN_NOTIFICATION_ACM(data.NotificationCode as i32) {
        c if c == wlan_notification_acm_scan_complete => ScanEvent::Complete,
        c if c == wlan_notification_acm_scan_fail => {
            // For ScanFail the payload is a WLAN_REASON_CODE (u32).
            let reason = if data.pData.is_null() {
                0
            } else {
                unsafe { *data.pData.cast::<u32>() }
            };
            ScanEvent::Failed(reason)
        }
        _ => return,
    };
    // Receiver may already be gone after a timeout; nothing to do then.
    let _ = listener.tx.send(event);
}

/// Hot-plug and radio callback. Runs on an OS thread: it must not panic across the FFI
/// boundary, so the body is wrapped in `catch_unwind`.
unsafe extern "system" fn on_event(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    let _ = std::panic::catch_unwind(|| {
        if data.is_null() || context.is_null() {
            return;
        }
        // SAFETY: the OS passes a valid notification; `context` is our `EventListener`,
        // alive until the registration is removed.
        let (data, listener) = unsafe { (&*data, &*context.cast::<EventListener>()) };
        let id = format!("{:?}", data.InterfaceGuid);
        let event = if data.NotificationSource == WLAN_NOTIFICATION_SOURCE_ACM {
            classify_acm(data.NotificationCode as i32, &id)
        } else if data.NotificationSource == WLAN_NOTIFICATION_SOURCE_MSM
            && data.NotificationCode as i32 == MSM_RADIO_STATE_CHANGE
            && !data.pData.is_null()
            && data.dwDataSize as usize >= size_of::<WLAN_PHY_RADIO_STATE>()
        {
            // SAFETY: size checked above; payload is a WLAN_PHY_RADIO_STATE.
            let state = unsafe { data.pData.cast::<WLAN_PHY_RADIO_STATE>().read_unaligned() };
            Some(WlanEvent::RadioChanged {
                id,
                on: radio_is_on(
                    state.dot11SoftwareRadioState.0,
                    state.dot11HardwareRadioState.0,
                ),
            })
        } else {
            None
        };
        if let Some(event) = event {
            // The receiver may have been dropped already.
            let _ = listener.tx.send(event);
        }
    });
}

impl WlanProvider for WindowsProvider {
    fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
        let list = self.interfaces()?;
        if list.is_empty() {
            return Err(WlanError::NoAdapters);
        }
        Ok(list.into_iter().map(|(_, a)| a).collect())
    }

    fn scan(
        &self,
        adapter: &Adapter,
        options: &ScanOptions,
    ) -> Result<Vec<AccessPoint>, WlanError> {
        let guid = self.guid_for(adapter)?;
        let probes = (0..options.probes.max(1))
            .map(|_| self.probe(guid, options.probe_timeout))
            .collect::<Result<Vec<_>, _>>()?;
        let mut aps = merge_probes(probes);
        // Name the hidden BSS we are associated with; failure here must not fail the scan.
        if let Ok(Some(conn)) = self.connection(guid) {
            resolve_from_connection(&mut aps, &conn);
        }
        Ok(aps)
    }

    fn current_connection(&self, adapter: &Adapter) -> Result<Option<Connection>, WlanError> {
        self.connection(self.guid_for(adapter)?)
    }

    fn subscribe_events(&self) -> Result<EventStream, WlanError> {
        let mut negotiated = 0u32;
        let mut handle = HANDLE::default();
        // SAFETY: out-pointers reference live locals.
        let rc = unsafe { WlanOpenHandle(CLIENT_VERSION, None, &mut negotiated, &mut handle) };
        if rc != 0 {
            return Err(os_err(rc, "WlanOpenHandle"));
        }
        let (tx, rx) = mpsc::channel();
        let listener = Box::new(EventListener { tx });
        let context = ptr::from_ref::<EventListener>(&listener).cast::<c_void>();
        let sources = WLAN_NOTIFICATION_SOURCES(
            WLAN_NOTIFICATION_SOURCE_ACM.0 | WLAN_NOTIFICATION_SOURCE_MSM.0,
        );
        // SAFETY: the listener is heap-allocated and owned by the returned registration,
        // whose Drop unregisters (waiting for in-flight callbacks) before freeing it.
        let rc = unsafe {
            WlanRegisterNotification(
                handle,
                sources,
                true,
                Some(on_event),
                Some(context),
                None,
                None,
            )
        };
        if rc != 0 {
            // SAFETY: handle came from WlanOpenHandle above.
            unsafe { WlanCloseHandle(handle, None) };
            return Err(os_err(rc, "WlanRegisterNotification"));
        }
        Ok(EventStream::new(
            rx,
            Box::new(EventRegistration {
                handle,
                _listener: listener,
            }),
        ))
    }

    fn connected_profile(&self, adapter: &Adapter) -> Result<Option<String>, WlanError> {
        windows_connect::connected_profile(self.handle, &self.guid_of(adapter)?)
    }

    fn set_profile(&self, adapter: &Adapter, profile_xml: &str) -> Result<(), WlanError> {
        windows_connect::set_profile(self.handle, &self.guid_of(adapter)?, profile_xml)
    }

    fn connect_profile(&self, adapter: &Adapter, profile: &str, timeout: Duration) -> Result<(), WlanError> {
        windows_connect::connect_profile(self.handle, &self.guid_of(adapter)?, profile, timeout)
    }

    fn delete_profile(&self, adapter: &Adapter, profile: &str) -> Result<(), WlanError> {
        windows_connect::delete_profile(self.handle, &self.guid_of(adapter)?, profile)
    }

    fn disconnect(&self, adapter: &Adapter) -> Result<(), WlanError> {
        windows_connect::disconnect(self.handle, &self.guid_of(adapter)?)
    }
}

impl WindowsProvider {
    fn guid_of(&self, adapter: &Adapter) -> Result<GUID, WlanError> {
        self.interfaces()?
            .into_iter()
            .find(|(_, a)| a.id == adapter.id)
            .map(|(g, _)| g)
            .ok_or_else(|| WlanError::AdapterNotFound(adapter.id.clone()))
    }
}

impl Drop for WindowsProvider {
    fn drop(&mut self) {
        // SAFETY: handle came from WlanOpenHandle and is closed exactly once.
        unsafe { WlanCloseHandle(self.handle, None) };
    }
}
