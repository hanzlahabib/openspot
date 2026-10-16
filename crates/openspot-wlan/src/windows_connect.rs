//! Windows implementation of connection management (A-01) over the Native Wifi API:
//! `WlanSetProfile`, `WlanConnect`, `WlanDisconnect`, `WlanDeleteProfile` and
//! `WlanQueryInterface(wlan_intf_opcode_current_connection)`.
//!
//! The previous profile is read with this file's own `WlanQueryInterface` call, independent of
//! any other module's notion of the current connection.
//!
//! UNVERIFIED: compiled for `x86_64-pc-windows-gnu` only. It has never run against a radio
//! (the development machine has none), so behaviour on real drivers is untested.

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_STATE, ERROR_NOT_FOUND, ERROR_SERVICE_NOT_ACTIVE, HANDLE, WIN32_ERROR};
use windows::Win32::NetworkManagement::WiFi::{
    L2_NOTIFICATION_DATA, WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_NOTIFICATION_DATA, WLAN_CONNECTION_PARAMETERS,
    WLAN_NOTIFICATION_ACM, WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_NONE, WlanConnect, WlanDeleteProfile,
    WlanDisconnect, WlanFreeMemory, WlanQueryInterface, WlanRegisterNotification, WlanSetProfile, dot11_BSS_type_infrastructure,
    wlan_connection_mode_profile, wlan_interface_state_connected, wlan_intf_opcode_current_connection,
    wlan_notification_acm_connection_attempt_fail, wlan_notification_acm_connection_complete,
};
use windows::core::{GUID, HSTRING, PCWSTR};

use crate::error::WlanError;

/// `WlanSetProfile` flag 0 = an all-user profile, readable by the WLAN service for any account.
const PROFILE_FLAGS_ALL_USER: u32 = 0;
/// Return value of a successful Native Wifi call.
const SUCCESS: u32 = 0;

fn os_err(code: u32, context: &'static str) -> WlanError {
    let win = WIN32_ERROR(code);
    if win == ERROR_SERVICE_NOT_ACTIVE {
        WlanError::ServiceDisabled
    } else if win == ERROR_ACCESS_DENIED {
        WlanError::AccessDenied
    } else {
        WlanError::Os { code, context }
    }
}

/// Profile the interface is connected with, `None` when it is not connected.
pub(crate) fn connected_profile(handle: HANDLE, interface: &GUID) -> Result<Option<String>, WlanError> {
    let mut size = 0u32;
    let mut data: *mut c_void = ptr::null_mut();
    // SAFETY: handle is open; out-pointers reference live locals; `data` is an OS allocation
    // freed below on success.
    let rc = unsafe {
        WlanQueryInterface(handle, interface, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None)
    };
    // Not connected is reported as ERROR_INVALID_STATE, which is an answer, not a failure.
    if WIN32_ERROR(rc) == ERROR_INVALID_STATE {
        return Ok(None);
    }
    if rc != SUCCESS {
        return Err(os_err(rc, "WlanQueryInterface"));
    }
    // SAFETY: on success `data` points to a WLAN_CONNECTION_ATTRIBUTES for this opcode.
    let name = unsafe {
        let attrs = &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>();
        let connected = attrs.isState == wlan_interface_state_connected;
        let len = attrs.strProfileName.iter().position(|&c| c == 0).unwrap_or(attrs.strProfileName.len());
        let name = String::from_utf16_lossy(&attrs.strProfileName[..len]);
        WlanFreeMemory(data.cast_const());
        connected.then_some(name)
    };
    Ok(name)
}

pub(crate) fn set_profile(handle: HANDLE, interface: &GUID, xml: &str) -> Result<(), WlanError> {
    let xml = HSTRING::from(xml);
    let mut reason = 0u32;
    // SAFETY: all pointers reference live locals; a null security descriptor means default.
    let rc = unsafe {
        WlanSetProfile(handle, interface, PROFILE_FLAGS_ALL_USER, &xml, PCWSTR::null(), true, None, &mut reason)
    };
    if rc != SUCCESS {
        // The reason code explains schema errors; surface it as the OS code context-free.
        return Err(if reason != 0 { WlanError::ConnectFailed(reason) } else { os_err(rc, "WlanSetProfile") });
    }
    Ok(())
}

pub(crate) fn delete_profile(handle: HANDLE, interface: &GUID, name: &str) -> Result<(), WlanError> {
    let name = HSTRING::from(name);
    // SAFETY: handle open, GUID and string live for the call.
    let rc = unsafe { WlanDeleteProfile(handle, interface, &name, None) };
    // Already gone is the desired end state.
    if rc != SUCCESS && WIN32_ERROR(rc) != ERROR_NOT_FOUND {
        return Err(os_err(rc, "WlanDeleteProfile"));
    }
    Ok(())
}

pub(crate) fn disconnect(handle: HANDLE, interface: &GUID) -> Result<(), WlanError> {
    // SAFETY: handle open, GUID live for the call.
    let rc = unsafe { WlanDisconnect(handle, interface, None) };
    if rc != SUCCESS {
        return Err(os_err(rc, "WlanDisconnect"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ConnectEvent {
    Complete(u32),
    Failed(u32),
}

struct Listener {
    interface: GUID,
    tx: Sender<ConnectEvent>,
}

/// Connects with `profile` and blocks until the connection completes, fails, or times out.
pub(crate) fn connect_profile(handle: HANDLE, interface: &GUID, profile: &str, timeout: Duration) -> Result<(), WlanError> {
    // WlanConnect to the profile we are already on may never produce a completion event.
    if connected_profile(handle, interface)?.as_deref() == Some(profile) {
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let listener = Box::new(Listener { interface: *interface, tx });
    let context = ptr::from_ref::<Listener>(&listener).cast::<c_void>();
    // SAFETY: `listener` outlives the registration: it is unregistered (which waits for
    // in-flight callbacks) before `listener` is dropped at the end of this function.
    let rc = unsafe {
        WlanRegisterNotification(handle, WLAN_NOTIFICATION_SOURCE_ACM, true, Some(on_notification), Some(context), None, None)
    };
    if rc != SUCCESS {
        return Err(os_err(rc, "WlanRegisterNotification"));
    }
    let result = start_and_wait(handle, interface, profile, timeout, &rx);
    // SAFETY: same handle; source NONE removes the registration.
    unsafe {
        WlanRegisterNotification(handle, WLAN_NOTIFICATION_SOURCE_NONE, true, None, None, None, None);
    }
    drop(listener);
    result
}

fn start_and_wait(
    handle: HANDLE,
    interface: &GUID,
    profile: &str,
    timeout: Duration,
    rx: &Receiver<ConnectEvent>,
) -> Result<(), WlanError> {
    let name = HSTRING::from(profile);
    let params = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: PCWSTR(name.as_ptr()),
        pDot11Ssid: ptr::null_mut(),
        pDesiredBssidList: ptr::null_mut(),
        dot11BssType: dot11_BSS_type_infrastructure,
        dwFlags: 0,
    };
    // SAFETY: `params` and `name` live for the call; null SSID/BSSID list selects by profile.
    let rc = unsafe { WlanConnect(handle, interface, &params, None) };
    if rc != SUCCESS {
        return Err(os_err(rc, "WlanConnect"));
    }
    match rx.recv_timeout(timeout) {
        Ok(ConnectEvent::Complete(0)) => Ok(()),
        Ok(ConnectEvent::Complete(reason) | ConnectEvent::Failed(reason)) => Err(WlanError::ConnectFailed(reason)),
        Err(_) => Err(WlanError::ConnectTimeout),
    }
}

/// Runs on an OS thread pool thread; must not panic or block.
unsafe extern "system" fn on_notification(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    // SAFETY: the OS passes a valid notification; `context` is our `Listener`, alive until
    // the registration is removed.
    let (data, listener) = unsafe { (&*data, &*context.cast::<Listener>()) };
    if data.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM || data.InterfaceGuid != listener.interface || data.pData.is_null() {
        return;
    }
    // For both connection events the payload is a WLAN_CONNECTION_NOTIFICATION_DATA.
    // SAFETY: pData is non-null and documented to have that type for these codes.
    let reason = unsafe { (*data.pData.cast::<WLAN_CONNECTION_NOTIFICATION_DATA>()).wlanReasonCode };
    let event = match WLAN_NOTIFICATION_ACM(data.NotificationCode as i32) {
        c if c == wlan_notification_acm_connection_complete => ConnectEvent::Complete(reason),
        c if c == wlan_notification_acm_connection_attempt_fail => ConnectEvent::Failed(reason),
        _ => return,
    };
    // The receiver is gone after a timeout; nothing to do then.
    let _ = listener.tx.send(event);
}
