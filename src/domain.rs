//! Domain types. Standard library only.

use std::fmt;

/// Device type as reported by HID++ feature 0x0005 getDeviceType.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    RemoteControl,
    Numpad,
    Mouse,
    Trackpad,
    Trackball,
    Presenter,
    Receiver,
    Unknown,
}

impl DeviceKind {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Keyboard,
            1 => Self::RemoteControl,
            2 => Self::Numpad,
            3 => Self::Mouse,
            4 => Self::Trackpad,
            5 => Self::Trackball,
            6 => Self::Presenter,
            7 => Self::Receiver,
            _ => Self::Unknown,
        }
    }

    pub fn is_pointer(self) -> bool {
        matches!(self, Self::Mouse | Self::Trackball | Self::Trackpad)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::RemoteControl => "remote control",
            Self::Numpad => "numpad",
            Self::Mouse => "mouse",
            Self::Trackpad => "trackpad",
            Self::Trackball => "trackball",
            Self::Presenter => "presenter",
            Self::Receiver => "receiver",
            Self::Unknown => "unknown",
        }
    }
}

/// Control IDs of the keyboard's Easy-Switch keys and their zero-based host slot.
pub const EASY_SWITCH_CIDS: [(u16, u8); 3] = [(0x00D1, 0), (0x00D2, 1), (0x00D3, 2)];

pub fn easy_switch_slot(cid: u16) -> Option<u8> {
    EASY_SWITCH_CIDS.iter().find(|(c, _)| *c == cid).map(|(_, s)| *s)
}

/// Easy-Switch state of a device. `current` is zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostInfo {
    pub count: u8,
    pub current: u8,
}

/// A device on a transport: receiver slot 1-6, or 0xFF when connected directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId {
    pub transport: u64,
    pub index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppError {
    /// Asleep, out of range, on another host, or the transport is gone.
    /// `code` is the receiver's HID++ 1.0 error code when it answered for the device.
    Unreachable { code: Option<u8>, detail: String },
    /// The device answered with a HID++ 2.0 error, or lacks a feature.
    Protocol { code: Option<u8>, detail: String },
}

impl HidppError {
    pub fn unreachable(code: Option<u8>, detail: impl Into<String>) -> Self {
        Self::Unreachable {
            code,
            detail: detail.into(),
        }
    }

    pub fn protocol(code: Option<u8>, detail: impl Into<String>) -> Self {
        Self::Protocol {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for HidppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable {
                code: Some(c),
                detail,
            } => write!(f, "{detail} (code {c})"),
            Self::Unreachable { code: None, detail } => write!(f, "{detail}"),
            Self::Protocol {
                code: Some(c),
                detail,
            } => write!(f, "{detail} (error {c})"),
            Self::Protocol { code: None, detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for HidppError {}

pub type Result<T> = std::result::Result<T, HidppError>;
