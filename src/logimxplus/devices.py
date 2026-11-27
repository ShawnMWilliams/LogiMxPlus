"""Gateway: a single HID++ 2.0 device (keyboard or mouse) behind a transport."""

from __future__ import annotations

import logging

from . import hidpp
from .domain import EASY_SWITCH_CIDS, DeviceId, DeviceKind, DeviceUnreachable, HidppError, HostInfo
from .hidpp import Feature
from .transport import Transport

log = logging.getLogger(__name__)


class HidppDevice:
    def __init__(self, transport: Transport, index: int) -> None:
        self.transport = transport
        self.index = index
        self._features: dict[int, int | None] = {}
        self.name = ""
        self.kind = DeviceKind.UNKNOWN

    @property
    def id(self) -> DeviceId:
        return DeviceId(self.transport.name, self.index)

    def __repr__(self) -> str:
        return f"<{self.name or 'device'} {self.kind.name} @ {self.transport.name}[{self.index}]>"

    def _req(self, feature_index: int, func: int, params: bytes = b"",
             timeout: float = 2.0) -> bytes:
        return self.transport.request(self.index, feature_index, func, params, timeout)

    def ping(self, timeout: float = 1.5) -> tuple[int, int]:
        """Return the HID++ protocol version; raises DeviceUnreachable if absent."""
        r = self._req(0, hidpp.ROOT_PING, b"\x00\x00\xaa", timeout)
        return r[0], r[1]

    def feature_index(self, feature: Feature) -> int | None:
        if feature not in self._features:
            r = self._req(0, hidpp.ROOT_GET_FEATURE, hidpp.feature_id_params(feature))
            self._features[feature] = r[0] or None
        return self._features[feature]

    def _require(self, feature: Feature) -> int:
        idx = self.feature_index(feature)
        if idx is None:
            raise HidppError(f"{self!r} lacks feature {feature.name}")
        return idx

    def identify(self) -> HidppDevice:
        """Read name and kind. Raises DeviceUnreachable when the device is not connected."""
        major, _ = self.ping()
        if major < 2:
            raise HidppError(f"{self!r} speaks HID++ {major}.x, need 2.0+")
        idx = self._require(Feature.DEVICE_NAME)
        length = self._req(idx, hidpp.NAME_GET_COUNT)[0]
        name = b""
        while len(name) < length:
            chunk = self._req(idx, hidpp.NAME_GET_NAME, bytes([len(name)]))
            name += chunk[: length - len(name)]
        self.name = name.split(b"\x00")[0].decode("utf-8", "replace")
        try:
            self.kind = DeviceKind(self._req(idx, hidpp.NAME_GET_TYPE)[0])
        except ValueError:
            self.kind = DeviceKind.UNKNOWN
        return self

    def firmware(self) -> str:
        """Main application firmware version, e.g. ``RBK81.02_B0017``."""
        idx = self._require(Feature.DEVICE_FW_VERSION)
        for entity in range(self._req(idx, 0)[0]):
            info = self._req(idx, 1, bytes([entity]))
            if info[0] == 0:  # 0 = main application
                return hidpp.format_firmware(info)
        raise HidppError(f"{self!r} reports no main firmware")

    def host_info(self) -> HostInfo:
        r = self._req(self._require(Feature.CHANGE_HOST), hidpp.CHANGE_HOST_GET_INFO)
        return HostInfo(count=r[0], current=r[1])

    def change_host(self, slot: int) -> None:
        """Switch the device to zero-based host ``slot``.

        The device drops the link as it switches, so the reply is optional: a
        timeout after the write went out is success.
        """
        idx = self._require(Feature.CHANGE_HOST)
        try:
            self._req(idx, hidpp.CHANGE_HOST_SET, bytes([slot]), timeout=0.5)
        except DeviceUnreachable as exc:
            if exc.code is not None:  # receiver said the device is not there
                raise
            log.debug("%r: no reply to setCurrentHost (expected while it switches)", self)

    def easy_switch_reporting(self) -> dict[int, bool]:
        """Which Easy-Switch keys currently report analytics key events."""
        idx = self._require(Feature.REPROG_CONTROLS_V4)
        return {
            cid: hidpp.analytics_enabled(
                self._req(idx, hidpp.REPROG_GET_REPORTING, hidpp.cid_params(cid)))
            for cid in EASY_SWITCH_CIDS
        }

    def enable_easy_switch_reporting(self, enabled: bool = True) -> None:
        """Ask the keyboard to report Easy-Switch presses without changing what they do.

        Easy-Switch keys are not divertable, but they support analytics key events:
        the keyboard still switches hosts, and also tells us which key was pressed.
        Settings are volatile, so this must be re-applied after every reconnect.
        """
        idx = self._require(Feature.REPROG_CONTROLS_V4)
        for cid in EASY_SWITCH_CIDS:
            self._req(idx, hidpp.REPROG_SET_REPORTING, hidpp.set_analytics_params(cid, enabled))

    def reprog_feature_index(self) -> int | None:
        return self.feature_index(Feature.REPROG_CONTROLS_V4)

    def wireless_status_index(self) -> int | None:
        return self.feature_index(Feature.WIRELESS_STATUS)
