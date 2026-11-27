//! Gateway: HID++ request/reply and notifications over hidapi.
//!
//! One [`HidTransport`] is one physical HID++ endpoint: a receiver (Bolt/Unifying) or a
//! directly connected device (Bluetooth). On Windows the short (0x10) and long (0x11)
//! HID++ reports live on separate HID collections, so a transport owns one blocking
//! reader thread per collection and writes long reports only. Reader threads sleep in
//! the kernel until a report arrives: no polling.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::domain::{HidppError, Result};
use crate::hidpp::{self, Message};

pub const LOGITECH_VID: u16 = 0x046D;
const USAGE_PAGE_RECEIVER: u16 = 0xFF00; // receivers: usage 1 = short, 2 = long
const USAGE_PAGE_BLE: u16 = 0xFF43; // Bluetooth LE devices: usage 0x0202 = long

/// Something a transport tells the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Notification { transport: u64, msg: Message },
    Closed { transport: u64 },
}

pub trait Transport: Send + Sync {
    fn id(&self) -> u64;
    fn name(&self) -> &str;
    fn is_receiver(&self) -> bool;
    fn alive(&self) -> bool;
    fn request(
        &self,
        device: u8,
        feature_index: u8,
        func: u8,
        params: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>>;
    fn write_receiver_register(&self, register: u8, value: [u8; 3]) -> Result<()>;
}

/// One HID collection as reported by enumeration (plain data, so grouping is testable).
#[derive(Debug, Clone)]
pub struct HidInfo {
    pub path: CString,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub product: String,
}

/// A HID++ endpoint and the HID collection paths that make it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub name: String,
    pub product_id: u16,
    pub is_receiver: bool,
    pub paths: Vec<CString>,
    pub long_path: CString,
    pub short_path: Option<CString>,
}

/// Windows exposes one path per HID collection of an interface, e.g.
/// `\\?\HID#VID_046D&PID_C548&MI_02&Col01#7&1dc7413e&0&0000#{guid}` and
/// `...&Col02#7&1dc7413e&0&0001#{guid}`. Strip the collection and its instance suffix
/// so both map to one key.
fn group_key(path: &str) -> String {
    let is_hex = |s: &str| s.chars().all(|c| c.is_ascii_hexdigit());
    let mut s = path.to_ascii_lowercase();
    if let Some(i) = s.find("&col") {
        if s.len() >= i + 6 && is_hex(&s[i + 4..i + 6]) {
            s.replace_range(i..i + 6, "");
        }
    }
    if let Some(j) = s.find("#{") {
        if j >= 5 && s.as_bytes()[j - 5] == b'&' && is_hex(&s[j - 4..j]) {
            s.replace_range(j - 5..j, "");
        }
    }
    s
}

/// Group enumeration entries into HID++ endpoints.
pub fn find_endpoints(infos: &[HidInfo]) -> Vec<Endpoint> {
    let mut groups: BTreeMap<String, Vec<&HidInfo>> = BTreeMap::new();
    for info in infos {
        let hidpp = info.vendor_id == LOGITECH_VID
            && ((info.usage_page == USAGE_PAGE_RECEIVER && matches!(info.usage, 1 | 2))
                || (info.usage_page == USAGE_PAGE_BLE && info.usage == 0x0202));
        if hidpp {
            let key = group_key(&info.path.to_string_lossy());
            groups.entry(key).or_default().push(info);
        }
    }
    groups
        .into_values()
        .filter_map(|members| {
            let long = members.iter().find(|m| matches!(m.usage, 2 | 0x0202))?;
            let short = members.iter().find(|m| m.usage == 1);
            let mut paths: Vec<CString> = Vec::new();
            for m in &members {
                if !paths.contains(&m.path) {
                    paths.push(m.path.clone());
                }
            }
            let product = if long.product.is_empty() {
                format!("{:04x}", long.product_id)
            } else {
                long.product.clone()
            };
            Some(Endpoint {
                name: format!("{product} ({:04x})", long.product_id),
                product_id: long.product_id,
                is_receiver: long.usage_page == USAGE_PAGE_RECEIVER,
                paths,
                long_path: long.path.clone(),
                short_path: short.map(|s| s.path.clone()),
            })
        })
        .collect()
}

pub struct HidTransport {
    id: u64,
    name: String,
    is_receiver: bool,
    alive: Arc<AtomicBool>,
    long_writer: Mutex<hidapi::HidDevice>,
    short_writer: Option<Mutex<hidapi::HidDevice>>,
    /// Holding this lock serializes requests.
    replies: Mutex<Receiver<Message>>,
}

impl HidTransport {
    /// Open every collection of `ep`: a blocking reader thread each, plus write handles.
    pub fn open(
        api: &hidapi::HidApi,
        ep: &Endpoint,
        id: u64,
        events: Sender<Event>,
    ) -> std::result::Result<Self, hidapi::HidError> {
        let alive = Arc::new(AtomicBool::new(true));
        let (reply_tx, reply_rx) = mpsc::channel();
        let long_writer = api.open_path(&ep.long_path)?;
        let short_writer = match &ep.short_path {
            Some(p) => Some(api.open_path(p)?),
            None => None,
        };
        let mut readers = Vec::new();
        for path in &ep.paths {
            readers.push(api.open_path(path)?);
        }
        for (n, reader) in readers.into_iter().enumerate() {
            let (alive, events, reply_tx, name) =
                (alive.clone(), events.clone(), reply_tx.clone(), ep.name.clone());
            std::thread::Builder::new()
                .name(format!("hid-read-{id}-{n}"))
                .spawn(move || read_loop(&reader, id, &name, &alive, &events, &reply_tx))
                .map_err(|e| hidapi::HidError::HidApiError {
                    message: e.to_string(),
                })?;
        }
        Ok(Self {
            id,
            name: ep.name.clone(),
            is_receiver: ep.is_receiver,
            alive,
            long_writer: Mutex::new(long_writer),
            short_writer: short_writer.map(Mutex::new),
            replies: Mutex::new(reply_rx),
        })
    }

    fn write(&self, handle: &Mutex<hidapi::HidDevice>, report: &[u8]) -> Result<()> {
        if !self.alive() {
            return Err(HidppError::unreachable(None, format!("{} is closed", self.name)));
        }
        log::trace!("{} > {}", self.name, hex(report));
        let dev = handle.lock().unwrap_or_else(|e| e.into_inner());
        dev.write(report).map(|_| ()).map_err(|e| {
            self.alive.store(false, Ordering::SeqCst);
            HidppError::unreachable(None, format!("{}: write failed: {e}", self.name))
        })
    }
}

fn read_loop(
    reader: &hidapi::HidDevice,
    id: u64,
    name: &str,
    alive: &AtomicBool,
    events: &Sender<Event>,
    replies: &Sender<Message>,
) {
    let mut buf = [0u8; 64];
    loop {
        match reader.read_timeout(&mut buf, -1) {
            Ok(0) => continue,
            Ok(n) => {
                log::trace!("{name} < {}", hex(&buf[..n]));
                let Some(msg) = hidpp::parse(&buf[..n]) else {
                    continue;
                };
                let sent = if msg.is_notification() {
                    events.send(Event::Notification { transport: id, msg }).is_ok()
                } else {
                    replies.send(msg).is_ok()
                };
                if !sent {
                    return; // runtime is gone
                }
            }
            Err(e) => {
                log::warn!("{name}: read failed ({e}); transport closed");
                alive.store(false, Ordering::SeqCst);
                let _ = events.send(Event::Closed { transport: id });
                return;
            }
        }
    }
}

impl Transport for HidTransport {
    fn id(&self) -> u64 {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_receiver(&self) -> bool {
        self.is_receiver
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn request(
        &self,
        device: u8,
        feature_index: u8,
        func: u8,
        params: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let replies = self.replies.lock().unwrap_or_else(|e| e.into_inner());
        while replies.try_recv().is_ok() {} // drop stale replies
        self.write(
            &self.long_writer,
            &hidpp::build_request(device, feature_index, func, params),
        )?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = match replies.recv_timeout(remaining) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    return Err(HidppError::unreachable(
                        None,
                        format!("{}[{device}] timed out", self.name),
                    ))
                }
            };
            if !hidpp::is_reply_to(&msg, device, feature_index, func) {
                continue;
            }
            let code = hidpp::error_code(&msg);
            if hidpp::is_receiver_error(&msg) {
                return Err(HidppError::unreachable(
                    code,
                    format!("{}[{device}] not reachable", self.name),
                ));
            }
            if msg.is_error() {
                return Err(HidppError::protocol(
                    code,
                    format!("{}[{device}] feature {feature_index} func {func}", self.name),
                ));
            }
            return Ok(msg.data);
        }
    }

    fn write_receiver_register(&self, register: u8, value: [u8; 3]) -> Result<()> {
        let report = hidpp::build_register_write(register, value);
        self.write(self.short_writer.as_ref().unwrap_or(&self.long_writer), &report)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Enumerate Logitech HID collections only (cheaper than enumerating everything).
pub fn enumerate(api: &mut hidapi::HidApi) -> Vec<HidInfo> {
    if let Err(e) = api
        .reset_devices()
        .and_then(|()| api.add_devices(LOGITECH_VID, 0))
    {
        log::warn!("HID enumeration failed: {e}");
        return Vec::new();
    }
    api.device_list()
        .map(|d| HidInfo {
            path: d.path().to_owned(),
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            product: d.product_string().unwrap_or_default().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str, page: u16, usage: u16, pid: u16) -> HidInfo {
        HidInfo {
            path: CString::new(path).unwrap(),
            vendor_id: LOGITECH_VID,
            product_id: pid,
            usage_page: page,
            usage,
            product: "USB Receiver".into(),
        }
    }

    #[test]
    fn groups_windows_collections() {
        let infos = [
            info(
                r"\\?\HID#VID_046D&PID_C548&MI_02&Col01#7&1dc7413e&0&0000#{x}",
                0xFF00,
                1,
                0xC548,
            ),
            info(
                r"\\?\HID#VID_046D&PID_C548&MI_02&Col02#7&1dc7413e&0&0001#{x}",
                0xFF00,
                2,
                0xC548,
            ),
            info(
                r"\\?\HID#VID_046D&PID_C548&MI_01&Col01#7&30f75d7c&0&0000#{x}",
                0x0001,
                2,
                0xC548,
            ),
            info(
                r"\\?\HID#{00001812}_VID&0002046d_PID&b378&Col03#9&1#{x}",
                0xFF43,
                0x0202,
                0xB378,
            ),
        ];
        let eps = find_endpoints(&infos);
        assert_eq!(eps.len(), 2);
        let rx = eps.iter().find(|e| e.is_receiver).unwrap();
        assert_eq!(rx.paths.len(), 2);
        assert!(rx
            .short_path
            .as_ref()
            .unwrap()
            .to_string_lossy()
            .contains("Col01"));
        assert!(rx.long_path.to_string_lossy().contains("Col02"));
        let ble = eps.iter().find(|e| !e.is_receiver).unwrap();
        assert_eq!(ble.product_id, 0xB378);
        assert!(ble.short_path.is_none());
    }

    #[test]
    fn dedupes_identical_paths() {
        let infos = [
            info("DevSrvsID:42", 0xFF00, 1, 0xC548),
            info("DevSrvsID:42", 0xFF00, 2, 0xC548),
        ];
        let eps = find_endpoints(&infos);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].paths.len(), 1);
    }

    #[test]
    fn ignores_other_vendors_and_usages() {
        let mut other = info("p", 0xFF00, 2, 0xC548);
        other.vendor_id = 0x1234;
        assert!(find_endpoints(&[other, info("q", 0x0001, 2, 0xC548)]).is_empty());
    }
}
