//! Linux backend over `nl80211` (generic netlink).
//!
//! UNVERIFIED on hardware: the development machine is WSL2 without a Wi-Fi radio. Attribute
//! decoding is unit-tested against byte fixtures ([`crate::nl80211`]); the socket flow below
//! only compiles and fails cleanly with `NoAdapters` where `nl80211` is absent.
//!
//! Flow per probe: subscribe to the `scan` multicast group, `NL80211_CMD_TRIGGER_SCAN`,
//! wait for `NEW_SCAN_RESULTS`/`SCAN_ABORTED`, then dump with `NL80211_CMD_GET_SCAN`.
//! Triggering needs `CAP_NET_ADMIN`; without it the backend returns only BSS entries the
//! kernel saw within [`MAX_CACHED_AGE_MS`] (the dump itself is unprivileged).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;

use neli::attr::Attribute;
use neli::consts::nl::NlmF;
use neli::consts::socket::NlFamily;
use neli::err::RouterError;
use neli::genl::{AttrTypeBuilder, Genlmsghdr, GenlmsghdrBuilder, NlattrBuilder, NoUserHeader};
use neli::nl::NlPayload;
use neli::router::synchronous::{NlRouter, NlRouterReceiverHandle};
use neli::types::GenlBuffer;
use neli::utils::Groups;

use crate::error::WlanError;
use crate::merge::merge_probes;
use crate::model::{AccessPoint, Adapter};
use crate::nl80211::{self, Interface};
use crate::{ScanOptions, WlanProvider};

/// Generic netlink family and multicast group carrying scan notifications.
const FAMILY: &str = "nl80211";
const SCAN_GROUP: &str = "scan";
/// nl80211 genl protocol version.
const GENL_VERSION: u8 = 1;
/// `EBUSY`: a scan is already running; waiting for its completion is the right response.
const EBUSY: i32 = 16;
/// Without `CAP_NET_ADMIN` we cannot trigger a scan and read what the kernel already has.
/// Entries older than this are dropped so a stale cache is not presented as a fresh scan
/// (the kernel itself expires BSS entries after about 30 s).
const MAX_CACHED_AGE_MS: u32 = 30_000;

#[neli::neli_enum(serialized_type = "u8")]
enum Cmd {
    Unspec = 0,
    GetInterface = 5,
    GetScan = 32,
    TriggerScan = 33,
    NewScanResults = 34,
    ScanAborted = 35,
}
impl neli::consts::genl::Cmd for Cmd {}

#[neli::neli_enum(serialized_type = "u16")]
enum Attr {
    Unspec = 0,
    Ifindex = 3,
    Ifname = 4,
    Iftype = 5,
    Bss = 47,
}
impl neli::consts::genl::NlAttrType for Attr {}

type Message = neli::nl::Nlmsghdr<u16, Genlmsghdr<Cmd, Attr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanEvent {
    Done,
    Aborted,
}

fn router_error<T, P>(e: RouterError<T, P>) -> WlanError {
    match e {
        RouterError::Nlmsgerr(err) => {
            let errno = err.error().saturating_neg();
            nl80211::error_for_errno(errno)
                .unwrap_or(WlanError::Os { code: u32::try_from(errno).unwrap_or(0), context: "nl80211" })
        }
        // `resolve_genl_family` reports a missing family as a message error: no cfg80211.
        RouterError::Msg(_) => WlanError::NoAdapters,
        _ => WlanError::Os { code: 0, context: "netlink transport" },
    }
}

/// A control socket with the nl80211 family resolved.
struct Control {
    router: NlRouter,
    family: u16,
}

impl Control {
    fn open() -> Result<Self, WlanError> {
        let (router, _) = NlRouter::connect(NlFamily::Generic, None, Groups::empty()).map_err(|_| {
            WlanError::Os { code: 0, context: "opening a generic netlink socket" }
        })?;
        let family = router.resolve_genl_family(FAMILY).map_err(router_error)?;
        Ok(Self { router, family })
    }

    fn request(
        &self,
        cmd: Cmd,
        flags: NlmF,
        ifindex: Option<u32>,
    ) -> Result<NlRouterReceiverHandle<u16, Genlmsghdr<Cmd, Attr>>, WlanError> {
        fn build_err<E>(_: E) -> WlanError {
            WlanError::Os { code: 0, context: "building a netlink attribute" }
        }
        let mut attrs = GenlBuffer::new();
        if let Some(index) = ifindex {
            let ty = AttrTypeBuilder::default().nla_type(Attr::Ifindex).build().map_err(build_err)?;
            attrs.push(NlattrBuilder::default().nla_type(ty).nla_payload(index).build().map_err(build_err)?);
        }
        let payload = GenlmsghdrBuilder::<Cmd, Attr, NoUserHeader>::default()
            .cmd(cmd)
            .version(GENL_VERSION)
            .attrs(attrs)
            .build()
            .map_err(|_| WlanError::Os { code: 0, context: "building a netlink message" })?;
        self.router.send(self.family, flags, NlPayload::Payload(payload)).map_err(router_error)
    }

    fn interfaces(&self) -> Result<Vec<Interface>, WlanError> {
        let mut out = Vec::new();
        for msg in self.request(Cmd::GetInterface, NlmF::DUMP, None)? {
            let msg: Message = msg.map_err(router_error)?;
            let NlPayload::Payload(p) = msg.nl_payload() else { continue };
            let (mut index, mut name, mut iftype) = (None, None, None);
            for attr in p.attrs().get_attr_handle().iter() {
                match attr.nla_type().nla_type() {
                    Attr::Ifindex => index = attr.get_payload_as::<u32>().ok(),
                    Attr::Ifname => name = Some(attr.payload().as_ref().to_vec()),
                    Attr::Iftype => iftype = attr.get_payload_as::<u32>().ok(),
                    _ => {}
                }
            }
            out.extend(Interface::from_parts(index, name.as_deref(), iftype));
        }
        Ok(out)
    }

    fn dump_bss(&self, ifindex: u32, max_age_ms: Option<u32>) -> Result<Vec<AccessPoint>, WlanError> {
        let mut out = Vec::new();
        for msg in self.request(Cmd::GetScan, NlmF::DUMP, Some(ifindex))? {
            let msg: Message = msg.map_err(router_error)?;
            let NlPayload::Payload(p) = msg.nl_payload() else { continue };
            for attr in p.attrs().get_attr_handle().iter() {
                if *attr.nla_type().nla_type() != Attr::Bss {
                    continue;
                }
                let nest = attr.payload().as_ref();
                let fresh = max_age_ms.is_none_or(|max| nl80211::seen_ms_ago(nest).is_some_and(|age| age <= max));
                if fresh {
                    out.extend(nl80211::parse_bss(nest));
                }
            }
        }
        Ok(out)
    }

    /// Sends `TRIGGER_SCAN`. `Ok(true)` when a scan is (now) running, `Ok(false)` when the
    /// caller lacks the privilege and must fall back to the kernel's cache.
    fn trigger(&self, ifindex: u32) -> Result<bool, WlanError> {
        let recv = self.request(Cmd::TriggerScan, NlmF::ACK, Some(ifindex))?;
        for msg in recv {
            match msg {
                Ok(_) => {}
                Err(e) => {
                    return match router_error(e) {
                        WlanError::PermissionDenied => Ok(false),
                        WlanError::Os { code, .. } if i32::try_from(code) == Ok(EBUSY) => Ok(true),
                        other => Err(other),
                    };
                }
            }
        }
        Ok(true)
    }
}

/// Scan notifications for one interface, delivered by a forwarding thread. The thread ends
/// when the router is dropped (its processing thread then closes the multicast channel).
struct ScanEvents {
    rx: Receiver<ScanEvent>,
    _router: NlRouter,
}

impl ScanEvents {
    fn subscribe(control: &Control, ifindex: u32) -> Result<Self, WlanError> {
        let group = control.router.resolve_nl_mcast_group(FAMILY, SCAN_GROUP).map_err(router_error)?;
        let (router, mut stream) = NlRouter::connect(NlFamily::Generic, None, Groups::new_groups(&[group]))
            .map_err(|_| WlanError::Os { code: 0, context: "opening the scan notification socket" })?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || forward_events(&mut stream, ifindex, &tx));
        Ok(Self { rx, _router: router })
    }

    fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }
}

fn forward_events(stream: &mut NlRouterReceiverHandle<u16, Genlmsghdr<u8, u16>>, ifindex: u32, tx: &Sender<ScanEvent>) {
    while let Some(Ok(msg)) = stream.next_typed::<u16, Genlmsghdr<Cmd, Attr>>() {
        let NlPayload::Payload(p) = msg.nl_payload() else { continue };
        let event = match p.cmd() {
            Cmd::NewScanResults => ScanEvent::Done,
            Cmd::ScanAborted => ScanEvent::Aborted,
            _ => continue,
        };
        let for_us = p
            .attrs()
            .get_attr_handle()
            .iter()
            .any(|a| *a.nla_type().nla_type() == Attr::Ifindex && a.get_payload_as::<u32>().ok() == Some(ifindex));
        if for_us && tx.send(event).is_err() {
            return;
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct LinuxProvider;

impl LinuxProvider {
    pub(crate) fn open() -> Result<Self, WlanError> {
        // Fail early (typed) where nl80211 is absent so `native_provider()` reports it.
        let control = Control::open()?;
        if control.interfaces()?.is_empty() {
            return Err(WlanError::NoAdapters);
        }
        Ok(Self)
    }
}

impl WlanProvider for LinuxProvider {
    fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
        let adapters: Vec<Adapter> = Control::open()?
            .interfaces()?
            .into_iter()
            .map(|i| Adapter { id: i.name.clone(), description: i.name })
            .collect();
        if adapters.is_empty() { Err(WlanError::NoAdapters) } else { Ok(adapters) }
    }

    fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
        let control = Control::open()?;
        let ifindex = control
            .interfaces()?
            .into_iter()
            .find(|i| i.name == adapter.id)
            .map(|i| i.index)
            .ok_or_else(|| WlanError::AdapterNotFound(adapter.id.clone()))?;
        let events = ScanEvents::subscribe(&control, ifindex)?;
        let mut probes = Vec::new();
        for _ in 0..options.probes.max(1) {
            events.drain();
            if control.trigger(ifindex)? {
                match events.rx.recv_timeout(options.probe_timeout) {
                    Ok(ScanEvent::Done) => {}
                    Ok(ScanEvent::Aborted) => return Err(WlanError::ScanFailed(0)),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                        return Err(WlanError::ScanTimeout);
                    }
                }
                probes.push(control.dump_bss(ifindex, None)?);
            } else {
                probes.push(control.dump_bss(ifindex, Some(MAX_CACHED_AGE_MS))?);
            }
        }
        Ok(merge_probes(probes))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The numeric values below come from `include/uapi/linux/nl80211.h`; they are typed
    /// out here independently of the enum definitions so a typo in either is caught.
    #[test]
    fn neli_enums_carry_the_kernel_header_values() {
        assert_eq!(u8::from(Cmd::GetInterface), 5);
        assert_eq!(u8::from(Cmd::GetScan), 32);
        assert_eq!(u8::from(Cmd::TriggerScan), 33);
        assert_eq!(u8::from(Cmd::NewScanResults), 34);
        assert_eq!(u8::from(Cmd::ScanAborted), 35);
        assert_eq!(u16::from(Attr::Ifindex), 3);
        assert_eq!(u16::from(Attr::Ifname), 4);
        assert_eq!(u16::from(Attr::Iftype), 5);
        assert_eq!(u16::from(Attr::Bss), 47);
    }
}
