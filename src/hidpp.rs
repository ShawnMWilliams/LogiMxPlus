//! Pure HID++ encoding and decoding. No I/O.
//!
//! Report layout (HID++ 2.0):
//!   short: `10 dev sub addr p0 p1 p2`                    (7 bytes)
//!   long:  `11 dev featIdx (func<<4 | swId) p0 .. p15`    (20 bytes)
//!
//! Notifications carry software id 0. Requests carry [`SW_ID`] so replies can be told
//! apart from notifications and from other software talking to the same device.

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const SHORT_LEN: usize = 7;
pub const LONG_LEN: usize = 20;
pub const SW_ID: u8 = 0x0B;

/// Device index for a device connected directly (Bluetooth / USB cable).
pub const DIRECT_INDEX: u8 = 0xFF;
pub const RECEIVER_SLOTS: std::ops::RangeInclusive<u8> = 1..=6;

const ERROR_20: u8 = 0xFF;
const ERROR_10: u8 = 0x8F;
const NOTIFY_CONNECTION: u8 = 0x41;
const LINK_NOT_ESTABLISHED: u8 = 0x40;
const REGISTER_SUB_IDS: [u8; 4] = [0x80, 0x81, 0x82, 0x83];

pub mod feature {
    pub const DEVICE_FW_VERSION: u16 = 0x0003;
    pub const DEVICE_NAME: u16 = 0x0005;
    pub const CHANGE_HOST: u16 = 0x1814;
    pub const REPROG_CONTROLS_V4: u16 = 0x1B04;
    pub const WIRELESS_STATUS: u16 = 0x1D4B;
}

pub mod func {
    pub const ROOT_GET_FEATURE: u8 = 0;
    pub const ROOT_PING: u8 = 1;
    pub const FW_GET_COUNT: u8 = 0;
    pub const FW_GET_INFO: u8 = 1;
    pub const NAME_GET_COUNT: u8 = 0;
    pub const NAME_GET_NAME: u8 = 1;
    pub const NAME_GET_TYPE: u8 = 2;
    pub const CHANGE_HOST_GET_INFO: u8 = 0;
    pub const CHANGE_HOST_SET: u8 = 1;
    pub const REPROG_GET_REPORTING: u8 = 2;
    pub const REPROG_SET_REPORTING: u8 = 3;
}

pub mod event {
    /// 0x1814 fn0, newer keyboard firmware: `[host it is leaving, target host]`.
    pub const CHANGE_HOST_ANNOUNCE: u8 = 0;
    /// 0x1B04 analyticsKeyEvents.
    pub const REPROG_ANALYTICS: u8 = 2;
    /// 0x1D4B wireless device status (reconnect).
    pub const WIRELESS_STATUS: u8 = 0;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub report_id: u8,
    pub device: u8,
    /// Feature index (2.0) or sub id (1.0).
    pub sub_id: u8,
    /// `func<<4 | sw` (2.0) or register address (1.0).
    pub address: u8,
    pub data: Vec<u8>,
}

impl Message {
    pub fn func(&self) -> u8 {
        self.address >> 4
    }

    pub fn sw_id(&self) -> u8 {
        self.address & 0x0F
    }

    pub fn is_error(&self) -> bool {
        self.sub_id == ERROR_20 || self.sub_id == ERROR_10
    }

    /// True for unsolicited messages (events), false for replies to requests.
    pub fn is_notification(&self) -> bool {
        if self.is_error() {
            return false;
        }
        if self.report_id == REPORT_SHORT && (0x40..0x80).contains(&self.sub_id) {
            return true; // HID++ 1.0 receiver notification, e.g. 0x41 connection
        }
        if self.device == DIRECT_INDEX && REGISTER_SUB_IDS.contains(&self.sub_id) {
            return false; // HID++ 1.0 reply to a receiver register read/write
        }
        self.sw_id() == 0
    }
}

pub fn parse(raw: &[u8]) -> Option<Message> {
    if raw.len() < 4 || (raw[0] != REPORT_SHORT && raw[0] != REPORT_LONG) {
        return None;
    }
    Some(Message {
        report_id: raw[0],
        device: raw[1],
        sub_id: raw[2],
        address: raw[3],
        data: raw[4..].to_vec(),
    })
}

pub fn build_request(device: u8, feature_index: u8, func: u8, params: &[u8]) -> [u8; LONG_LEN] {
    debug_assert!(
        params.len() <= LONG_LEN - 4,
        "too many parameters for a long report"
    );
    let mut r = [0u8; LONG_LEN];
    r[0] = REPORT_LONG;
    r[1] = device;
    r[2] = feature_index;
    r[3] = ((func & 0x0F) << 4) | SW_ID;
    let n = params.len().min(LONG_LEN - 4);
    r[4..4 + n].copy_from_slice(&params[..n]);
    r
}

pub fn build_register_write(register: u8, value: [u8; 3]) -> [u8; SHORT_LEN] {
    [
        REPORT_SHORT,
        DIRECT_INDEX,
        0x80,
        register,
        value[0],
        value[1],
        value[2],
    ]
}

/// True if `msg` answers our request (success or error).
pub fn is_reply_to(msg: &Message, device: u8, feature_index: u8, func: u8) -> bool {
    if msg.device != device {
        return false;
    }
    let expected = ((func & 0x0F) << 4) | SW_ID;
    match msg.sub_id {
        ERROR_20 => msg.address == feature_index && msg.data.first() == Some(&expected),
        ERROR_10 => msg.address == feature_index,
        _ => msg.sub_id == feature_index && msg.address == expected,
    }
}

pub fn error_code(msg: &Message) -> Option<u8> {
    if msg.is_error() {
        msg.data.get(1).copied()
    } else {
        None
    }
}

/// HID++ 1.0 error: the receiver answered because the device is absent.
pub fn is_receiver_error(msg: &Message) -> bool {
    msg.sub_id == ERROR_10
}

// --- parameter encoders ------------------------------------------------------------

pub fn feature_id_params(feature: u16) -> [u8; 2] {
    feature.to_be_bytes()
}

pub fn cid_params(cid: u16) -> [u8; 2] {
    cid.to_be_bytes()
}

/// setCidReporting: leave divert/persist/rawXY/remap alone, set only analytics.
pub fn set_analytics_params(cid: u16, enabled: bool) -> [u8; 6] {
    let [hi, lo] = cid.to_be_bytes();
    // flags 0 (no *valid bits) and remap 0 leave everything else unchanged;
    // byte 5: bit0 analytics on, bit1 valid.
    [hi, lo, 0x00, 0x00, 0x00, if enabled { 0x03 } else { 0x02 }]
}

// --- decoders ----------------------------------------------------------------------

/// For a receiver 0x41 notification return whether the link is up, else `None`.
pub fn connection_link_up(msg: &Message) -> Option<bool> {
    if msg.report_id != REPORT_SHORT || msg.sub_id != NOTIFY_CONNECTION {
        return None;
    }
    msg.data.first().map(|flags| flags & LINK_NOT_ESTABLISHED == 0)
}

/// Decode analyticsKeyEvents: 3-byte records `CID(be16) event`, ended by a zero CID.
pub fn analytics_events(data: &[u8]) -> Vec<(u16, u8)> {
    data.chunks_exact(3)
        .map(|r| (u16::from_be_bytes([r[0], r[1]]), r[2]))
        .take_while(|(cid, _)| *cid != 0)
        .collect()
}

/// Decode a getCidReporting reply: byte 5 bit0 = analytics key events on.
pub fn analytics_enabled(reply: &[u8]) -> bool {
    reply.get(5).is_some_and(|b| b & 0x01 != 0)
}

/// Decode 0x0003 getFwInfo: type, 3-char prefix, BCD number, BCD revision, build (be16).
pub fn format_firmware(reply: &[u8]) -> String {
    if reply.len() < 8 {
        return String::from("?");
    }
    let prefix: String = reply[1..4]
        .iter()
        .filter(|b| b.is_ascii_graphic())
        .map(|&b| b as char)
        .collect();
    format!(
        "{prefix}{:02X}.{:02X}_B{:02X}{:02X}",
        reply[4], reply[5], reply[6], reply[7]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(raw: &[u8]) -> Message {
        parse(raw).expect("valid report")
    }

    #[test]
    fn build_request_is_long_report_with_sw_id() {
        let r = build_request(3, 0x0A, 1, &[0x01]);
        assert_eq!(r.len(), LONG_LEN);
        assert_eq!(&r[..5], &[0x11, 3, 0x0A, 0x10 | SW_ID, 0x01]);
        assert!(r[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn reply_matching_success_and_errors() {
        let ok = msg(&[0x11, 4, 9, 0x20 | SW_ID, 0, 0xD1]);
        assert!(is_reply_to(&ok, 4, 9, 2) && !ok.is_notification());
        assert!(!is_reply_to(&ok, 3, 9, 2));
        assert!(!is_reply_to(&ok, 4, 9, 3));

        let err20 = msg(&[0x11, 4, 0xFF, 9, 0x20 | SW_ID, 0x02, 0]);
        assert!(is_reply_to(&err20, 4, 9, 2));
        assert_eq!(error_code(&err20), Some(2));

        let err10 = msg(&[0x10, 1, 0x8F, 0x00, 0x1B, 0x04, 0]);
        assert!(is_reply_to(&err10, 1, 0, 1) && is_receiver_error(&err10));
        assert_eq!(error_code(&err10), Some(4));
    }

    #[test]
    fn other_software_reply_is_not_ours() {
        let other = msg(&[0x11, 4, 9, 0x21, 0, 0xD1]); // sw id 1, e.g. Options+
        assert!(!is_reply_to(&other, 4, 9, 2));
        assert!(!other.is_notification());
    }

    #[test]
    fn notification_detection() {
        let event = msg(&[0x11, 4, 9, 0x20, 0x00, 0xD2, 0x01]);
        assert!(event.is_notification() && event.func() == 2);
        assert!(msg(&[0x10, 3, 0x41, 0x10, 0x41, 0x25, 0xB0]).is_notification());
        assert!(!msg(&[0x10, 0xFF, 0x80, 0x00, 0, 0, 0]).is_notification());
    }

    #[test]
    fn connection_link_state() {
        assert_eq!(
            connection_link_up(&msg(&[0x10, 3, 0x41, 0x10, 0x01, 0x34, 0xB0])),
            Some(true)
        );
        assert_eq!(
            connection_link_up(&msg(&[0x10, 3, 0x41, 0x10, 0x41, 0x34, 0xB0])),
            Some(false)
        );
        assert_eq!(connection_link_up(&msg(&[0x11, 3, 0x41, 0x10, 0x41])), None);
    }

    #[test]
    fn analytics_event_decoding() {
        let mut one = vec![0x00, 0xD2, 0x01, 0x00, 0x00, 0x00];
        one.extend([0; 10]);
        assert_eq!(analytics_events(&one), vec![(0xD2, 1)]);
        let two = [0x00, 0xD1, 0x01, 0x00, 0xE4, 0x00, 0, 0, 0];
        assert_eq!(analytics_events(&two), vec![(0xD1, 1), (0xE4, 0)]);
        assert!(analytics_events(&[]).is_empty());
    }

    #[test]
    fn set_analytics_params_does_not_touch_divert() {
        assert_eq!(
            set_analytics_params(0xD3, true),
            [0x00, 0xD3, 0x00, 0x00, 0x00, 0x03]
        );
        assert_eq!(set_analytics_params(0xD3, false)[5], 0x02);
    }

    #[test]
    fn analytics_enabled_from_reply() {
        assert!(analytics_enabled(&[0, 0xD1, 0, 0, 0, 1]));
        assert!(!analytics_enabled(&[0, 0xD1, 0, 0, 0, 0]));
        assert!(!analytics_enabled(&[0, 0xD1]));
    }

    #[test]
    fn firmware_formatting() {
        let mut reply = vec![0x00, b'R', b'B', b'K', 0x81, 0x02, 0x00, 0x17];
        reply.extend([0; 8]);
        assert_eq!(format_firmware(&reply), "RBK81.02_B0017");
    }

    #[test]
    fn register_write_report() {
        assert_eq!(
            build_register_write(0x00, [0x00, 0x09, 0x00]),
            [0x10, 0xFF, 0x80, 0x00, 0x00, 0x09, 0x00]
        );
    }
}
