//! Gateway: one HID++ 2.0 device (keyboard or mouse) behind a transport.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::domain::{DeviceId, DeviceKind, HidppError, HostInfo, Result, EASY_SWITCH_CIDS};
use crate::hidpp::{self, feature, func};
use crate::service::{Keyboard, SwitchableDevice};
use crate::transport::Transport;

const TIMEOUT: Duration = Duration::from_secs(2);

pub struct HidppDevice {
    transport: Arc<dyn Transport>,
    pub index: u8,
    features: HashMap<u16, Option<u8>>,
    pub name: String,
    pub kind: DeviceKind,
}

impl std::fmt::Debug for HidppDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<{} {} @ {}[{}]>",
            self.name,
            self.kind.label(),
            self.transport.name(),
            self.index
        )
    }
}

impl HidppDevice {
    pub fn new(transport: Arc<dyn Transport>, index: u8) -> Self {
        Self {
            transport,
            index,
            features: HashMap::new(),
            name: String::new(),
            kind: DeviceKind::Unknown,
        }
    }

    pub fn id(&self) -> DeviceId {
        DeviceId {
            transport: self.transport.id(),
            index: self.index,
        }
    }

    fn req(&self, feature_index: u8, f: u8, params: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        self.transport
            .request(self.index, feature_index, f, params, timeout)
    }

    /// HID++ protocol version; `Unreachable` if the device is not connected.
    pub fn ping(&self) -> Result<(u8, u8)> {
        let r = self.req(0, func::ROOT_PING, &[0, 0, 0xAA], Duration::from_millis(1500))?;
        Ok((r.first().copied().unwrap_or(0), r.get(1).copied().unwrap_or(0)))
    }

    pub fn feature_index(&mut self, feature_id: u16) -> Result<Option<u8>> {
        if let Some(idx) = self.features.get(&feature_id) {
            return Ok(*idx);
        }
        let r = self.req(
            0,
            func::ROOT_GET_FEATURE,
            &hidpp::feature_id_params(feature_id),
            TIMEOUT,
        )?;
        let idx = r.first().copied().filter(|&i| i != 0);
        self.features.insert(feature_id, idx);
        Ok(idx)
    }

    fn require(&mut self, feature_id: u16) -> Result<u8> {
        self.feature_index(feature_id)?
            .ok_or_else(|| HidppError::protocol(None, format!("{self:?} lacks feature {feature_id:04X}")))
    }

    /// Read name and kind.
    pub fn identify(&mut self) -> Result<()> {
        let (major, _) = self.ping()?;
        if major < 2 {
            return Err(HidppError::protocol(
                None,
                format!("{self:?} speaks HID++ {major}.x"),
            ));
        }
        let idx = self.require(feature::DEVICE_NAME)?;
        let len = usize::from(self.req(idx, func::NAME_GET_COUNT, &[], TIMEOUT)?[0]);
        let mut name = Vec::with_capacity(len);
        while name.len() < len {
            let offset = u8::try_from(name.len()).unwrap_or(u8::MAX);
            let chunk = self.req(idx, func::NAME_GET_NAME, &[offset], TIMEOUT)?;
            let take = (len - name.len()).min(chunk.len());
            if take == 0 {
                break;
            }
            name.extend_from_slice(&chunk[..take]);
        }
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        self.name = String::from_utf8_lossy(&name[..end]).into_owned();
        self.kind = DeviceKind::from_code(self.req(idx, func::NAME_GET_TYPE, &[], TIMEOUT)?[0]);
        Ok(())
    }

    /// Main application firmware, e.g. `RBK81.02_B0017`.
    pub fn firmware(&mut self) -> Result<String> {
        let idx = self.require(feature::DEVICE_FW_VERSION)?;
        let count = self.req(idx, func::FW_GET_COUNT, &[], TIMEOUT)?[0];
        for entity in 0..count {
            let info = self.req(idx, func::FW_GET_INFO, &[entity], TIMEOUT)?;
            if info.first() == Some(&0) {
                return Ok(hidpp::format_firmware(&info));
            }
        }
        Err(HidppError::protocol(None, "no main firmware entity"))
    }

    /// Which Easy-Switch keys currently report analytics key events.
    pub fn easy_switch_reporting(&mut self) -> Result<usize> {
        let idx = self.require(feature::REPROG_CONTROLS_V4)?;
        let mut armed = 0;
        for (cid, _) in EASY_SWITCH_CIDS {
            let r = self.req(idx, func::REPROG_GET_REPORTING, &hidpp::cid_params(cid), TIMEOUT)?;
            armed += usize::from(hidpp::analytics_enabled(&r));
        }
        Ok(armed)
    }
}

impl SwitchableDevice for HidppDevice {
    fn host_info(&mut self) -> Result<HostInfo> {
        let idx = self.require(feature::CHANGE_HOST)?;
        let r = self.req(idx, func::CHANGE_HOST_GET_INFO, &[], TIMEOUT)?;
        Ok(HostInfo {
            count: r[0],
            current: r[1],
        })
    }

    /// Switch to zero-based host `slot`. The device may drop the link instead of
    /// replying, so a plain timeout after the write went out counts as success.
    fn change_host(&mut self, slot: u8) -> Result<()> {
        let idx = self.require(feature::CHANGE_HOST)?;
        match self.req(idx, func::CHANGE_HOST_SET, &[slot], Duration::from_millis(500)) {
            Ok(_) | Err(HidppError::Unreachable { code: None, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl Keyboard for HidppDevice {
    /// Ask the keyboard to report Easy-Switch presses (0x1B04 analytics) without
    /// changing what they do. Newer MX Keys S firmware announces switches on 0x1814
    /// instead; arming is harmless there. Settings are volatile: re-apply on reconnect.
    fn enable_easy_switch_reporting(&mut self) -> Result<()> {
        let Some(idx) = self.feature_index(feature::REPROG_CONTROLS_V4)? else {
            return Ok(());
        };
        for (cid, _) in EASY_SWITCH_CIDS {
            self.req(
                idx,
                func::REPROG_SET_REPORTING,
                &hidpp::set_analytics_params(cid, true),
                TIMEOUT,
            )?;
        }
        Ok(())
    }
}
