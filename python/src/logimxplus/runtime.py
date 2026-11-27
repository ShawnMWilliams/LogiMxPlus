"""Composition root: finds the keyboard and mouse and routes HID++ events to the service."""

from __future__ import annotations

import logging
import threading
import time
from collections.abc import Callable

from . import hidpp
from .config import Config
from .devices import HidppDevice
from .domain import EASY_SWITCH_CIDS, DeviceId, DeviceKind, HidppError
from .hidpp import Feature
from .service import SwitchService
from .transport import Endpoint, HidTransport, Transport, discover, open_endpoint

log = logging.getLogger(__name__)

RESCAN_INTERVAL_S = 5.0
RECEIVER_NOTIFICATION_FLAGS = bytes([0x00, 0x09, 0x00])


class Runtime:
    def __init__(self, config: Config, service: SwitchService | None = None,
                 discover_fn: Callable[[], list[Endpoint]] = discover,
                 open_fn: Callable[[Endpoint], HidTransport] = open_endpoint) -> None:
        self.config = config
        self.service = service or SwitchService(config.channel_map, config.pending_ttl)
        self._discover = discover_fn
        self._open = open_fn
        self._transports: dict[bytes, Transport] = {}
        self.keyboard: HidppDevice | None = None
        self.mouse: HidppDevice | None = None
        self._kb_reprog: int | None = None
        self._kb_change_host: int | None = None
        self._lock = threading.RLock()

    # --- discovery ----------------------------------------------------------------------

    def scan(self) -> None:
        with self._lock:
            for key, t in list(self._transports.items()):
                if not t.alive:
                    log.info("%s went away", t.name)
                    t.close()
                    del self._transports[key]
                    self._forget_transport(t)
            for ep in self._discover():
                if ep.long_path in self._transports:
                    continue
                try:
                    t = self._open(ep)
                except Exception as exc:
                    log.warning("cannot open %s: %s", ep.name, exc)
                    continue
                log.info("opened %s", ep.name)
                self._transports[ep.long_path] = t
                t.set_handler(self._on_notification)
                if t.is_receiver:
                    # Register 0x00: wireless + software-present notification flags, so the
                    # receiver reports devices connecting and disconnecting (0x41).
                    t.write_receiver_register(0x00, RECEIVER_NOTIFICATION_FLAGS)
                for index in hidpp.RECEIVER_SLOTS if t.is_receiver else (hidpp.DIRECT_INDEX,):
                    self._attach(t, index)

    def _forget_transport(self, t: Transport) -> None:
        if self.keyboard and self.keyboard.transport is t:
            self.keyboard = None
            self.service.keyboard_disconnected()
        if self.mouse and self.mouse.transport is t:
            self.mouse = None
            self.service.mouse_disconnected()

    def _matches(self, dev: HidppDevice, wanted: str | None) -> bool:
        return wanted is None or wanted.lower() in dev.name.lower()

    def _attach(self, t: Transport, index: int, retries: int = 1) -> None:
        dev = HidppDevice(t, index)
        for attempt in range(retries):
            try:
                dev.identify()
                break
            except HidppError as exc:
                if attempt + 1 == retries:
                    log.debug("%s[%d]: not available (%s)", t.name, index, exc)
                    return
                time.sleep(0.5)
        try:
            if dev.kind == DeviceKind.KEYBOARD and self._matches(dev, self.config.keyboard_name) \
                    and dev.reprog_feature_index() and dev.feature_index(Feature.CHANGE_HOST):
                log.info("keyboard: %r", dev)
                self.keyboard = dev
                self._kb_reprog = dev.reprog_feature_index()
                self._kb_change_host = dev.feature_index(Feature.CHANGE_HOST)
                self.service.keyboard_connected(dev)
            elif dev.kind in (DeviceKind.MOUSE, DeviceKind.TRACKBALL, DeviceKind.TRACKPAD) \
                    and self._matches(dev, self.config.mouse_name) \
                    and dev.feature_index(Feature.CHANGE_HOST):
                log.info("mouse: %r", dev)
                self.mouse = dev
                self.service.mouse_connected(dev)
            else:
                log.info("ignoring %r", dev)
        except HidppError as exc:
            log.warning("%r: setup failed: %s", dev, exc)

    def _detach(self, device_id: DeviceId) -> None:
        if self.keyboard and self.keyboard.id == device_id:
            log.info("keyboard disconnected")
            self.keyboard = None
            self.service.keyboard_disconnected()
        if self.mouse and self.mouse.id == device_id:
            log.info("mouse disconnected")
            self.mouse = None
            self.service.mouse_disconnected()

    # --- events -------------------------------------------------------------------------

    def _on_notification(self, t: Transport, msg: hidpp.Message) -> None:
        kb = self.keyboard
        if kb and kb.transport is t and msg.device == kb.index \
                and msg.sub_id == self._kb_change_host and msg.func == hidpp.CHANGE_HOST_EVENT:
            # Newer firmware (Enhanced Easy-Switch) announces the switch on 0x1814:
            # payload [host it is leaving, target host], both zero-based.
            if len(msg.data) >= 2 and msg.data[1] < 3:
                log.debug("change-host announcement %d -> %d", msg.data[0], msg.data[1])
                self.service.easy_switch_pressed(msg.data[1])
            return
        if kb and kb.transport is t and msg.device == kb.index \
                and msg.sub_id == self._kb_reprog and msg.func == hidpp.REPROG_EVENT_ANALYTICS:
            for cid, event in hidpp.analytics_events(msg.data):
                log.debug("analytics key event cid=0x%04X event=%d", cid, event)
                if cid in EASY_SWITCH_CIDS:
                    self.service.easy_switch_pressed(EASY_SWITCH_CIDS[cid])
                    return
            return
        link = hidpp.connection_link_up(msg)
        if link is not None:
            device_id = DeviceId(t.name, msg.device)
            with self._lock:
                if link:
                    log.debug("%s[%d] link up", t.name, msg.device)
                    self._detach(device_id)
                    self._attach(t, msg.device, retries=3)
                else:
                    log.debug("%s[%d] link down", t.name, msg.device)
                    self._detach(device_id)
            return
        for dev in (self.keyboard, self.mouse):
            if dev and dev.transport is t and msg.device == dev.index \
                    and msg.sub_id == dev.wireless_status_index() \
                    and msg.func == hidpp.WIRELESS_STATUS_EVENT:
                log.debug("%r reports a reconnect", dev)
                with self._lock:
                    self._detach(dev.id)
                    self._attach(t, msg.device, retries=3)
                return
        log.debug("unhandled notification from %s: %s", t.name, msg)

    # --- main loop ----------------------------------------------------------------------

    def run(self, stop: threading.Event) -> None:
        log.info("LogiMxPlus running")
        while not stop.is_set():
            try:
                self.scan()
            except Exception:
                log.exception("scan failed")
            stop.wait(RESCAN_INTERVAL_S)
        self.close()

    def close(self) -> None:
        with self._lock:
            for t in self._transports.values():
                t.close()
            self._transports.clear()
