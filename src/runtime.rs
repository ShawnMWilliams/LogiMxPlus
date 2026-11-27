//! Composition root: finds the keyboard and mouse, re-arms on reconnect, and routes
//! HID++ events to the service. Single-threaded event loop: it sleeps on a channel
//! until a transport reports something or a (rare) rescan is due.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::device::HidppDevice;
use crate::domain::{easy_switch_slot, DeviceId, DeviceKind, Result};
use crate::hidpp::{self, event, feature, Message};
use crate::service::SwitchService;
use crate::transport::{self, Endpoint, Event, HidTransport, Transport};

/// Receiver register 0x00: wireless + software-present notification flags, so the
/// receiver reports devices connecting and disconnecting (0x41).
const RECEIVER_NOTIFICATION_FLAGS: [u8; 3] = [0x00, 0x09, 0x00];
/// Rescan often only while nothing is open (receiver unplugged, Bluetooth off).
const RESCAN_IDLE: Duration = Duration::from_secs(5);
/// Otherwise rarely: catches a second receiver or a Bluetooth device appearing.
const RESCAN_ACTIVE: Duration = Duration::from_secs(60);

/// Finds and opens HID++ endpoints. The real one uses hidapi; tests use a fake.
pub trait Discovery {
    fn endpoints(&mut self) -> Vec<Endpoint>;
    fn open(&mut self, ep: &Endpoint, id: u64, events: Sender<Event>) -> Option<Arc<dyn Transport>>;
}

pub struct HidDiscovery {
    api: Option<hidapi::HidApi>,
}

impl HidDiscovery {
    pub fn new() -> Self {
        // Enumerate only Logitech devices, on demand (see transport::enumerate).
        hidapi::HidApi::disable_device_discovery();
        let api = hidapi::HidApi::new()
            .map_err(|e| log::error!("cannot initialise HID access: {e}"))
            .ok();
        Self { api }
    }
}

impl Discovery for HidDiscovery {
    fn endpoints(&mut self) -> Vec<Endpoint> {
        self.api
            .as_mut()
            .map(|api| transport::find_endpoints(&transport::enumerate(api)))
            .unwrap_or_default()
    }

    fn open(&mut self, ep: &Endpoint, id: u64, events: Sender<Event>) -> Option<Arc<dyn Transport>> {
        let api = self.api.as_ref()?;
        match HidTransport::open(api, ep, id, events) {
            Ok(t) => Some(Arc::new(t)),
            Err(e) => {
                log::warn!("cannot open {}: {e}", ep.name);
                None
            }
        }
    }
}

/// Routing info for an attached device (the device itself is owned by the service).
#[derive(Debug, Clone, Copy)]
struct Attached {
    id: DeviceId,
    change_host: Option<u8>,
    reprog: Option<u8>,
    wireless: Option<u8>,
}

pub struct Runtime<D: Discovery> {
    config: Config,
    pub service: SwitchService,
    discovery: D,
    transports: HashMap<u64, Arc<dyn Transport>>,
    opened: HashMap<CString, u64>,
    next_id: u64,
    keyboard: Option<Attached>,
    mouse: Option<Attached>,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
}

impl<D: Discovery> Runtime<D> {
    pub fn new(config: Config, discovery: D) -> Self {
        let service = SwitchService::new(config.channel_map.clone(), config.pending_ttl);
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            config,
            service,
            discovery,
            transports: HashMap::new(),
            opened: HashMap::new(),
            next_id: 1,
            keyboard: None,
            mouse: None,
            events_tx,
            events_rx,
        }
    }

    pub fn has_devices(&self) -> bool {
        self.keyboard.is_some() || self.mouse.is_some()
    }

    // --- discovery ------------------------------------------------------------------

    pub fn scan(&mut self, now: Instant) {
        let dead: Vec<u64> = self
            .transports
            .iter()
            .filter(|(_, t)| !t.alive())
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            self.forget_transport(id);
        }
        for ep in self.discovery.endpoints() {
            if self.opened.contains_key(&ep.long_path) {
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            let Some(t) = self.discovery.open(&ep, id, self.events_tx.clone()) else {
                continue;
            };
            log::info!("opened {}", ep.name);
            self.opened.insert(ep.long_path.clone(), id);
            self.transports.insert(id, t.clone());
            if t.is_receiver() {
                if let Err(e) = t.write_receiver_register(0x00, RECEIVER_NOTIFICATION_FLAGS) {
                    log::warn!("{}: cannot enable connection notifications: {e}", t.name());
                }
                for index in hidpp::RECEIVER_SLOTS {
                    self.attach(&t, index, 1, now);
                }
            } else {
                self.attach(&t, hidpp::DIRECT_INDEX, 1, now);
            }
        }
    }

    fn forget_transport(&mut self, id: u64) {
        if let Some(t) = self.transports.remove(&id) {
            log::info!("{} went away", t.name());
        }
        self.opened.retain(|_, v| *v != id);
        if self.keyboard.is_some_and(|a| a.id.transport == id) {
            self.keyboard = None;
            self.service.keyboard_disconnected();
        }
        if self.mouse.is_some_and(|a| a.id.transport == id) {
            self.mouse = None;
            self.service.mouse_disconnected();
        }
    }

    fn name_matches(name: &str, wanted: Option<&str>) -> bool {
        wanted.is_none_or(|w| name.to_lowercase().contains(&w.to_lowercase()))
    }

    fn attach(&mut self, t: &Arc<dyn Transport>, index: u8, attempts: u32, now: Instant) {
        let mut dev = HidppDevice::new(t.clone(), index);
        for attempt in 1..=attempts {
            match dev.identify() {
                Ok(()) => break,
                Err(e) if attempt == attempts => {
                    log::debug!("{}[{index}]: not available ({e})", t.name());
                    return;
                }
                Err(_) => thread::sleep(Duration::from_millis(500)),
            }
        }
        if let Err(e) = self.classify(dev, now) {
            log::warn!("{}[{index}]: setup failed: {e}", t.name());
        }
    }

    fn classify(&mut self, mut dev: HidppDevice, now: Instant) -> Result<()> {
        let change_host = dev.feature_index(feature::CHANGE_HOST)?;
        let wireless = dev.feature_index(feature::WIRELESS_STATUS)?;
        if change_host.is_none() {
            log::info!("ignoring {dev:?} (no Easy-Switch)");
            return Ok(());
        }
        if dev.kind == DeviceKind::Keyboard
            && Self::name_matches(&dev.name, self.config.keyboard_name.as_deref())
        {
            let reprog = dev.feature_index(feature::REPROG_CONTROLS_V4)?;
            log::info!("keyboard: {dev:?}");
            self.keyboard = Some(Attached {
                id: dev.id(),
                change_host,
                reprog,
                wireless,
            });
            self.service.keyboard_connected(Box::new(dev));
        } else if dev.kind.is_pointer() && Self::name_matches(&dev.name, self.config.mouse_name.as_deref()) {
            log::info!("mouse: {dev:?}");
            self.mouse = Some(Attached {
                id: dev.id(),
                change_host,
                reprog: None,
                wireless,
            });
            self.service.mouse_connected(Box::new(dev), now);
        } else {
            log::info!("ignoring {dev:?}");
        }
        Ok(())
    }

    fn detach(&mut self, id: DeviceId) {
        if self.keyboard.is_some_and(|a| a.id == id) {
            log::info!("keyboard disconnected");
            self.keyboard = None;
            self.service.keyboard_disconnected();
        }
        if self.mouse.is_some_and(|a| a.id == id) {
            log::info!("mouse disconnected");
            self.mouse = None;
            self.service.mouse_disconnected();
        }
    }

    fn reattach(&mut self, transport: u64, index: u8, now: Instant) {
        self.detach(DeviceId { transport, index });
        if let Some(t) = self.transports.get(&transport).cloned() {
            self.attach(&t, index, 3, now);
        }
    }

    // --- events ---------------------------------------------------------------------

    pub fn handle(&mut self, ev: Event, now: Instant) {
        match ev {
            Event::Closed { transport } => self.forget_transport(transport),
            Event::Notification { transport, msg } => self.on_notification(transport, &msg, now),
        }
    }

    fn on_notification(&mut self, transport: u64, msg: &Message, now: Instant) {
        let from = DeviceId {
            transport,
            index: msg.device,
        };
        if let Some(kb) = self.keyboard.filter(|k| k.id == from) {
            // Newer MX Keys S firmware: 0x1814 fn0 announces [leaving host, target host].
            if Some(msg.sub_id) == kb.change_host && msg.func() == event::CHANGE_HOST_ANNOUNCE {
                if let (Some(&cur), Some(&target)) = (msg.data.first(), msg.data.get(1)) {
                    if target < 3 {
                        log::debug!("change-host announcement {cur} -> {target}");
                        self.service.easy_switch_pressed(target, now);
                    }
                }
                return;
            }
            // Keyboards that report key presses instead (0x1B04 analytics events).
            if Some(msg.sub_id) == kb.reprog && msg.func() == event::REPROG_ANALYTICS {
                for (cid, ev) in hidpp::analytics_events(&msg.data) {
                    log::debug!("analytics key event cid=0x{cid:04X} event={ev}");
                    if let Some(slot) = easy_switch_slot(cid) {
                        self.service.easy_switch_pressed(slot, now);
                        return;
                    }
                }
                return;
            }
        }
        if let Some(up) = hidpp::connection_link_up(msg) {
            if up {
                log::debug!("device {} link up", msg.device);
                self.reattach(transport, msg.device, now);
            } else {
                log::debug!("device {} link down", msg.device);
                self.detach(from);
            }
            return;
        }
        for dev in [self.keyboard, self.mouse].into_iter().flatten() {
            if dev.id == from && Some(msg.sub_id) == dev.wireless && msg.func() == event::WIRELESS_STATUS {
                log::debug!("device {} reports a reconnect", msg.device);
                self.reattach(transport, msg.device, now);
                return;
            }
        }
        log::trace!("unhandled notification: {msg:?}");
    }

    // --- main loop --------------------------------------------------------------------

    /// Run until the process exits.
    pub fn run(&mut self) -> ! {
        log::info!("LogiMxPlus running");
        let mut next_scan = Instant::now();
        loop {
            let now = Instant::now();
            if now >= next_scan {
                self.scan(now);
                next_scan = now
                    + if self.transports.is_empty() {
                        RESCAN_IDLE
                    } else {
                        RESCAN_ACTIVE
                    };
            }
            match self
                .events_rx
                .recv_timeout(next_scan.saturating_duration_since(Instant::now()))
            {
                Ok(ev) => {
                    let closed = matches!(ev, Event::Closed { .. });
                    self.handle(ev, Instant::now());
                    if closed {
                        next_scan = Instant::now() + Duration::from_secs(2); // re-plugged?
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => unreachable!("runtime holds a sender"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::HidppError;
    use crate::hidpp::func;
    use std::sync::Mutex;

    const FEATURES: [(u16, u8); 5] = [(0x0005, 1), (0x1814, 10), (0x1815, 11), (0x1B04, 9), (0x1D4B, 4)];

    #[derive(Debug, Clone, Default)]
    struct SimDevice {
        name: String,
        kind: u8,
        host: u8,
        disconnected: bool,
        analytics: Vec<u16>,
        host_changes: Vec<u8>,
    }

    fn sim(name: &str, kind: u8) -> SimDevice {
        SimDevice {
            name: name.into(),
            kind,
            host: 1,
            ..SimDevice::default()
        }
    }

    struct SimReceiver {
        devices: Mutex<HashMap<u8, SimDevice>>,
        registers: Mutex<Vec<(u8, [u8; 3])>>,
    }

    impl Transport for SimReceiver {
        fn id(&self) -> u64 {
            1
        }
        fn name(&self) -> &str {
            "sim"
        }
        fn is_receiver(&self) -> bool {
            true
        }
        fn alive(&self) -> bool {
            true
        }
        fn write_receiver_register(&self, register: u8, value: [u8; 3]) -> Result<()> {
            self.registers.lock().unwrap().push((register, value));
            Ok(())
        }
        fn request(&self, device: u8, fi: u8, f: u8, p: &[u8], _: Duration) -> Result<Vec<u8>> {
            let mut devices = self.devices.lock().unwrap();
            let dev = match devices.get_mut(&device) {
                Some(d) if !d.disconnected => d,
                _ => return Err(HidppError::unreachable(Some(4), "absent")),
            };
            match (fi, f) {
                (0, func::ROOT_PING) => Ok(vec![4, 5, p[2]]),
                (0, _) => {
                    let id = u16::from_be_bytes([p[0], p[1]]);
                    Ok(vec![
                        FEATURES.iter().find(|(f, _)| *f == id).map_or(0, |(_, i)| *i),
                        0,
                        0,
                    ])
                }
                (1, func::NAME_GET_COUNT) => Ok(vec![dev.name.len() as u8]),
                (1, func::NAME_GET_NAME) => Ok(dev.name.as_bytes()[usize::from(p[0])..].to_vec()),
                (1, _) => Ok(vec![dev.kind]),
                (10, func::CHANGE_HOST_GET_INFO) => Ok(vec![3, dev.host, 0]),
                (10, _) => {
                    dev.host_changes.push(p[0]);
                    dev.host = p[0];
                    Err(HidppError::unreachable(None, "timed out")) // link drops, no reply
                }
                (9, func::REPROG_SET_REPORTING) => {
                    if p[5] & 1 != 0 {
                        dev.analytics.push(u16::from_be_bytes([p[0], p[1]]));
                    }
                    Ok(p.to_vec())
                }
                _ => Err(HidppError::protocol(Some(2), "unsupported")),
            }
        }
    }

    struct SimDiscovery(Arc<SimReceiver>);

    impl Discovery for SimDiscovery {
        fn endpoints(&mut self) -> Vec<Endpoint> {
            let p = CString::new("sim").unwrap();
            vec![Endpoint {
                name: "sim".into(),
                product_id: 0xC548,
                is_receiver: true,
                paths: vec![p.clone()],
                long_path: p,
                short_path: None,
            }]
        }
        fn open(&mut self, _: &Endpoint, _: u64, _: Sender<Event>) -> Option<Arc<dyn Transport>> {
            Some(self.0.clone())
        }
    }

    fn rig(devs: &[(u8, SimDevice)], config: Config) -> (Runtime<SimDiscovery>, Arc<SimReceiver>) {
        let rx = Arc::new(SimReceiver {
            devices: Mutex::new(devs.iter().cloned().collect()),
            registers: Mutex::new(Vec::new()),
        });
        let mut rt = Runtime::new(config, SimDiscovery(rx.clone()));
        rt.scan(Instant::now());
        (rt, rx)
    }

    fn standard() -> (Runtime<SimDiscovery>, Arc<SimReceiver>) {
        rig(
            &[(3, sim("MX Master 3S", 3)), (4, sim("MX Keys S", 0))],
            Config::default(),
        )
    }

    fn notify(rt: &mut Runtime<SimDiscovery>, raw: &[u8]) {
        let msg = hidpp::parse(raw).unwrap();
        rt.handle(Event::Notification { transport: 1, msg }, Instant::now());
    }

    fn dev(rx: &SimReceiver, index: u8) -> SimDevice {
        rx.devices.lock().unwrap()[&index].clone()
    }

    fn long(head: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = head.to_vec();
        v.extend_from_slice(data);
        v.resize(20, 0);
        v
    }

    #[test]
    fn scan_finds_devices_and_arms_keyboard() {
        let (rt, rx) = standard();
        assert!(rt.keyboard.is_some() && rt.mouse.is_some());
        assert_eq!(rt.mouse.unwrap().id.index, 3);
        assert_eq!(dev(&rx, 4).analytics, vec![0xD1, 0xD2, 0xD3]);
        assert_eq!(*rx.registers.lock().unwrap(), vec![(0x00, [0x00, 0x09, 0x00])]);
    }

    #[test]
    fn change_host_announcement_moves_mouse() {
        let (mut rt, rx) = standard();
        notify(&mut rt, &long([0x11, 4, 10, 0x00], &[0x01, 0x00])); // leaving 2 for 1
        assert_eq!(dev(&rx, 3).host_changes, vec![0]);
        assert!(dev(&rx, 4).host_changes.is_empty());
    }

    #[test]
    fn analytics_event_moves_mouse() {
        let (mut rt, rx) = standard();
        notify(&mut rt, &long([0x11, 4, 9, 0x20], &[0x00, 0xD3, 0x01]));
        assert_eq!(dev(&rx, 3).host_changes, vec![2]);
    }

    #[test]
    fn events_from_other_devices_are_ignored() {
        let (mut rt, rx) = standard();
        notify(&mut rt, &long([0x11, 3, 10, 0x00], &[0x01, 0x00])); // from the mouse
        assert!(dev(&rx, 3).host_changes.is_empty());
    }

    #[test]
    fn keyboard_reconnect_rearms_reporting() {
        let (mut rt, rx) = standard();
        notify(&mut rt, &[0x10, 4, 0x41, 0x10, 0x41, 0x78, 0xB3]); // link down
        assert!(rt.keyboard.is_none());
        rx.devices.lock().unwrap().get_mut(&4).unwrap().analytics.clear();
        notify(&mut rt, &[0x10, 4, 0x41, 0x10, 0x01, 0x78, 0xB3]); // link up
        assert!(rt.keyboard.is_some());
        assert_eq!(dev(&rx, 4).analytics, vec![0xD1, 0xD2, 0xD3]);
    }

    #[test]
    fn mouse_waking_up_receives_pending_switch() {
        let mut asleep = sim("MX Master 3S", 3);
        asleep.disconnected = true;
        let (mut rt, rx) = rig(&[(3, asleep), (4, sim("MX Keys S", 0))], Config::default());
        assert!(rt.mouse.is_none());
        notify(&mut rt, &long([0x11, 4, 10, 0x00], &[0x01, 0x02]));
        rx.devices.lock().unwrap().get_mut(&3).unwrap().disconnected = false;
        notify(&mut rt, &[0x10, 3, 0x41, 0x10, 0x02, 0x34, 0xB0]);
        assert_eq!(dev(&rx, 3).host_changes, vec![2]);
    }

    #[test]
    fn name_filter_skips_other_keyboards() {
        let config = Config {
            keyboard_name: Some("MX Keys".into()),
            ..Config::default()
        };
        let (rt, _) = rig(&[(1, sim("K380", 0))], config);
        assert!(rt.keyboard.is_none());
    }
}
