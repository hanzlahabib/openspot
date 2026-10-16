//! Background scan loop. Scans block for seconds on some drivers, so they run on a
//! dedicated thread and report back over a channel.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openspot_model::{LinkInfo, SpeedResult};
use openspot_wlan::{AccessPoint, Adapter, EventStream, ScanOptions, WlanError, WlanEvent, WlanProvider};

use crate::active_scan::{ActiveScanJob, run_job};
use crate::link::link_from_connection;

/// Probes merged for each survey measurement: a single probe often misses APs, and
/// the user is standing still waiting for the beep, so a few seconds extra is fine.
const SURVEY_PROBES: u8 = 3;

/// How often the worker looks for adapter notifications while it waits for commands. Hot-plug
/// must be noticed even when scanning is paused or the interval is long; half a second is
/// prompt for a notice and costs nothing measurable.
const NOTIFICATION_POLL: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub enum Command {
    /// One-off measurement at map position `(x, y)`; answered with [`Event::SurveyScan`].
    SurveyScan { x: f64, y: f64 },
    /// Like `SurveyScan`, then the job's speed tests; answered with one [`Event::SurveyScan`]
    /// carrying both, plus [`Event::ActiveProgress`] updates in between.
    SurveyScanActive { x: f64, y: f64, job: Box<ActiveScanJob> },
    SetAdapter(Adapter),
    SetInterval(Duration),
    SetPaused(bool),
    ScanNow,
}

pub enum Event {
    Scan(Vec<AccessPoint>),
    /// `speed` is empty for a passive-only scan; `link` is the adapter's own connection right
    /// after the scan, `None` when it was not connected or the platform cannot say.
    SurveyScan { x: f64, y: f64, result: Result<Vec<AccessPoint>, String>, speed: Vec<SpeedResult>, link: Option<LinkInfo> },
    ActiveProgress(String),
    Error(String),
    /// An adapter appeared or disappeared, or a radio was switched. `adapters` is the
    /// provider's list taken right after, for the events that change it.
    Hardware { event: WlanEvent, adapters: Option<Vec<Adapter>> },
}

#[derive(Debug)]
pub struct Worker {
    commands: Sender<Command>,
    // Joined implicitly: dropping `commands` makes the thread exit after its current scan.
    _thread: JoinHandle<()>,
}

impl Worker {
    /// Starts scanning immediately. `on_event` is called after every event is queued
    /// (use it to request a UI repaint).
    pub fn spawn(
        provider: Box<dyn WlanProvider>,
        adapter: Adapter,
        interval: Duration,
        on_event: impl Fn() + Send + 'static,
    ) -> (Self, Receiver<Event>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let thread = thread::spawn(move || run(provider, adapter, interval, &cmd_rx, &ev_tx, &on_event));
        (Self { commands: cmd_tx, _thread: thread }, ev_rx)
    }

    pub fn send(&self, command: Command) {
        // The thread only exits when the sender is dropped, so a send error means it
        // already stopped; there is nothing left to command.
        let _ = self.commands.send(command);
    }
}

fn run(
    provider: Box<dyn WlanProvider>,
    mut adapter: Adapter,
    mut interval: Duration,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    on_event: &impl Fn(),
) {
    let options = ScanOptions::default();
    let mut paused = false;
    let mut scan_now = true;
    let mut next_scan = Instant::now();
    let notifications = subscribe(&*provider, events, on_event);

    loop {
        if let Some(stream) = &notifications
            && !forward_notifications(&*provider, stream, events, on_event)
        {
            return; // UI closed
        }
        if scan_now || (!paused && Instant::now() >= next_scan) {
            scan_now = false;
            let event = match provider.scan(&adapter, &options) {
                Ok(aps) => Event::Scan(aps),
                Err(e) => Event::Error(e.to_string()),
            };
            if events.send(event).is_err() {
                return; // UI closed
            }
            on_event();
            next_scan = Instant::now() + interval;
        }

        let first = match (&notifications, paused) {
            // Nothing to wake for: block until the UI sends a command.
            (None, true) => commands.recv().map_err(|_| RecvTimeoutError::Disconnected),
            (_, true) => commands.recv_timeout(NOTIFICATION_POLL),
            (None, false) => commands.recv_timeout(next_scan.saturating_duration_since(Instant::now())),
            (_, false) => commands.recv_timeout(next_scan.saturating_duration_since(Instant::now()).min(NOTIFICATION_POLL)),
        };
        match first {
            Ok(cmd) => {
                let mut pending = vec![cmd];
                pending.extend(commands.try_iter());
                for cmd in pending {
                    if let Some((x, y, job)) = survey_request(&cmd) {
                        let options = ScanOptions { probes: SURVEY_PROBES, ..ScanOptions::default() };
                        let result = provider.scan(&adapter, &options).map_err(|e| e.to_string());
                        let speed = match (&result, job) {
                            (Ok(_), Some(job)) => {
                                let mut progress = |message: String| {
                                    // A closed channel is noticed on the final send below.
                                    let _ = events.send(Event::ActiveProgress(message));
                                    on_event();
                                };
                                run_job(&*provider, &adapter, job, &mut progress)
                            }
                            _ => Vec::new(),
                        };
                        let link = current_link(&*provider, &adapter);
                        if events.send(Event::SurveyScan { x, y, result, speed, link }).is_err() {
                            return;
                        }
                        on_event();
                    } else {
                        apply(cmd, &mut adapter, &mut interval, &mut paused, &mut scan_now);
                    }
                }
                // Any command is a sign of activity: refresh right away unless paused.
                next_scan = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The adapter's connection for the point just measured. Failing to read it only means the
/// point has no link data, so the error is dropped rather than failing the measurement.
fn current_link(provider: &dyn WlanProvider, adapter: &Adapter) -> Option<LinkInfo> {
    provider.current_connection(adapter).ok().flatten().and_then(|c| link_from_connection(&c))
}

/// Registers for adapter notifications. A provider without them is normal (the fake one,
/// Linux for now) and stays silent; a real failure is reported once.
fn subscribe(provider: &dyn WlanProvider, events: &Sender<Event>, on_event: &impl Fn()) -> Option<EventStream> {
    match provider.subscribe_events() {
        Ok(stream) => Some(stream),
        Err(WlanError::Unsupported(_)) => None,
        Err(e) => {
            // A closed channel is noticed on the next scan send.
            let _ = events.send(Event::Error(format!("adapter notifications unavailable: {e}")));
            on_event();
            None
        }
    }
}

/// Passes pending notifications to the UI. Returns `false` once the UI has gone away.
fn forward_notifications(provider: &dyn WlanProvider, stream: &EventStream, events: &Sender<Event>, on_event: &impl Fn()) -> bool {
    while let Ok(event) = stream.try_recv() {
        // Only arrival and removal change the list; reading it after a radio toggle would
        // just race the driver for nothing.
        let adapters = match event {
            WlanEvent::AdapterArrived { .. } | WlanEvent::AdapterRemoved { .. } => provider.adapters().ok(),
            WlanEvent::RadioChanged { .. } => None,
        };
        if events.send(Event::Hardware { event, adapters }).is_err() {
            return false;
        }
        on_event();
    }
    true
}

/// Position and optional speed-test job of a survey measurement request.
fn survey_request(cmd: &Command) -> Option<(f64, f64, Option<&ActiveScanJob>)> {
    match cmd {
        Command::SurveyScan { x, y } => Some((*x, *y, None)),
        Command::SurveyScanActive { x, y, job } => Some((*x, *y, Some(job))),
        _ => None,
    }
}

fn apply(cmd: Command, adapter: &mut Adapter, interval: &mut Duration, paused: &mut bool, scan_now: &mut bool) {
    match cmd {
        Command::SetAdapter(a) => {
            *adapter = a;
            *scan_now = true;
        }
        Command::SetInterval(d) => *interval = d,
        Command::SetPaused(p) => *paused = p,
        Command::ScanNow => *scan_now = true,
        Command::SurveyScan { .. } | Command::SurveyScanActive { .. } => unreachable!("handled by the worker loop"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Sender as StdSender;
    use std::sync::{Arc, Mutex};

    use openspot_wlan::FakeProvider;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    /// Fake scanning plus a notification stream the test drives by hand.
    struct HotPlugProvider {
        adapters: Arc<Mutex<Vec<Adapter>>>,
        stream: Mutex<Option<EventStream>>,
    }

    impl WlanProvider for HotPlugProvider {
        fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
            Ok(self.adapters.lock().unwrap().clone())
        }

        fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
            FakeProvider.scan(adapter, options)
        }

        fn subscribe_events(&self) -> Result<EventStream, WlanError> {
            self.stream.lock().unwrap().take().ok_or(WlanError::Unsupported("a second subscription"))
        }
    }

    fn fake_adapter() -> Adapter {
        FakeProvider.adapters().unwrap().remove(0)
    }

    fn extra_adapter() -> Adapter {
        Adapter { id: "hotplug-1".into(), description: "USB adapter".into() }
    }

    /// A running worker plus the handles the test uses to drive it.
    struct Rig {
        worker: Worker,
        events: Receiver<Event>,
        notifications: StdSender<WlanEvent>,
        adapters: Arc<Mutex<Vec<Adapter>>>,
    }

    fn hot_plug_worker() -> Rig {
        let (tx, rx) = mpsc::channel();
        let adapters = Arc::new(Mutex::new(vec![fake_adapter()]));
        let provider = HotPlugProvider { adapters: adapters.clone(), stream: Mutex::new(Some(EventStream::new(rx, Box::new(())))) };
        let (worker, events) = Worker::spawn(Box::new(provider), fake_adapter(), Duration::from_secs(3600), || {});
        Rig { worker, events, notifications: tx, adapters }
    }

    /// Next event matching `pick`, skipping the others; panics after [`WAIT`].
    fn next<T>(events: &Receiver<Event>, mut pick: impl FnMut(Event) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = events.recv_timeout(left).expect("expected event did not arrive");
            if let Some(found) = pick(event) {
                return found;
            }
        }
    }

    #[test]
    fn hot_plug_is_forwarded_with_a_fresh_adapter_list_even_while_paused() {
        let Rig { worker, events, notifications: tx, adapters } = hot_plug_worker();
        next(&events, |e| matches!(e, Event::Scan(_)).then_some(()));
        worker.send(Command::SetPaused(true));

        adapters.lock().unwrap().push(extra_adapter());
        tx.send(WlanEvent::AdapterArrived { id: extra_adapter().id }).unwrap();
        let (event, listing) = next(&events, |e| match e {
            Event::Hardware { event, adapters } => Some((event, adapters)),
            _ => None,
        });
        assert_eq!(event, WlanEvent::AdapterArrived { id: extra_adapter().id });
        assert_eq!(listing, Some(vec![fake_adapter(), extra_adapter()]));

        adapters.lock().unwrap().retain(|a| a.id != extra_adapter().id);
        tx.send(WlanEvent::AdapterRemoved { id: extra_adapter().id }).unwrap();
        let listing = next(&events, |e| match e {
            Event::Hardware { adapters, .. } => Some(adapters),
            _ => None,
        });
        assert_eq!(listing, Some(vec![fake_adapter()]));
    }

    #[test]
    fn radio_changes_are_forwarded_without_relisting() {
        let Rig { events, notifications: tx, .. } = hot_plug_worker();
        tx.send(WlanEvent::RadioChanged { id: fake_adapter().id, on: false }).unwrap();
        let (event, listing) = next(&events, |e| match e {
            Event::Hardware { event, adapters } => Some((event, adapters)),
            _ => None,
        });
        assert_eq!(event, WlanEvent::RadioChanged { id: fake_adapter().id, on: false });
        assert_eq!(listing, None);
    }

    #[test]
    fn providers_without_notifications_still_scan_and_answer_survey_requests() {
        let (worker, events) = Worker::spawn(Box::new(FakeProvider), fake_adapter(), Duration::from_secs(3600), || {});
        next(&events, |e| matches!(e, Event::Scan(_)).then_some(()));
        worker.send(Command::SetPaused(true));
        worker.send(Command::SurveyScan { x: 3.0, y: 4.0 });
        let (x, y, ok) = next(&events, |e| match e {
            Event::SurveyScan { x, y, result, .. } => Some((x, y, result.is_ok())),
            _ => None,
        });
        assert_eq!((x, y, ok), (3.0, 4.0, true));
    }

    /// Fake scanning with a settable connection state.
    struct ConnectedProvider(Option<openspot_wlan::Connection>);

    impl WlanProvider for ConnectedProvider {
        fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
            FakeProvider.adapters()
        }

        fn scan(&self, adapter: &Adapter, options: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
            FakeProvider.scan(adapter, options)
        }

        fn current_connection(&self, _adapter: &Adapter) -> Result<Option<openspot_wlan::Connection>, WlanError> {
            Ok(self.0.clone())
        }
    }

    fn connection(state: openspot_wlan::ConnectionState) -> openspot_wlan::Connection {
        openspot_wlan::Connection {
            state,
            ssid: "net".into(),
            bssid: openspot_wlan::MacAddress([2, 0, 0, 0, 0, 7]),
            phy: None,
            rx_rate_mbps: 600.0,
            tx_rate_mbps: 720.5,
            signal_quality: 90,
            rssi_dbm: -55,
            channel: Some(36),
        }
    }

    fn survey_link(provider: impl WlanProvider + 'static) -> Option<LinkInfo> {
        let (worker, events) = Worker::spawn(Box::new(provider), fake_adapter(), Duration::from_secs(3600), || {});
        worker.send(Command::SurveyScan { x: 0.0, y: 0.0 });
        next(&events, |e| match e {
            Event::SurveyScan { link, .. } => Some(link),
            _ => None,
        })
    }

    #[test]
    fn survey_scans_carry_the_adapters_current_link() {
        let link = survey_link(ConnectedProvider(Some(connection(openspot_wlan::ConnectionState::Connected)))).unwrap();
        assert_eq!((link.bssid, link.tx_rate_mbps, link.rx_rate_mbps), (openspot_wlan::MacAddress([2, 0, 0, 0, 0, 7]), 720.5, 600.0));
    }

    #[test]
    fn survey_scans_have_no_link_when_disconnected_or_unsupported() {
        assert_eq!(survey_link(ConnectedProvider(None)), None);
        assert_eq!(survey_link(ConnectedProvider(Some(connection(openspot_wlan::ConnectionState::Disconnected)))), None);
        assert_eq!(survey_link(FakeProvider), None, "a provider without connection info still measures");
    }

    #[test]
    fn a_long_interval_does_not_delay_commands() {
        let (worker, events) = Worker::spawn(Box::new(FakeProvider), fake_adapter(), Duration::from_secs(3600), || {});
        next(&events, |e| matches!(e, Event::Scan(_)).then_some(()));
        worker.send(Command::ScanNow);
        next(&events, |e| matches!(e, Event::Scan(_)).then_some(()));
    }
}
