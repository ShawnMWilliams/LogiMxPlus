"""Service: decides when and where the mouse switches. Depends on interfaces only."""

from __future__ import annotations

import logging
import threading
import time
from collections.abc import Callable, Mapping
from typing import Protocol

from .domain import DeviceUnreachable, HidppError, HostInfo

log = logging.getLogger(__name__)


class SwitchableDevice(Protocol):
    def host_info(self) -> HostInfo: ...
    def change_host(self, slot: int) -> None: ...


class Keyboard(SwitchableDevice, Protocol):
    def enable_easy_switch_reporting(self, enabled: bool = True) -> None: ...


class SwitchService:
    """Makes the mouse follow the keyboard's Easy-Switch keys.

    ``channel_map`` maps a zero-based keyboard host slot to the zero-based mouse host
    slot of the same computer (identity by default).

    If the mouse cannot be reached when a key is pressed (asleep, or the link is still
    coming up), the switch is kept pending for ``pending_ttl`` seconds and applied as
    soon as the mouse connects. The keyboard coming back cancels it.
    """

    DEBOUNCE_S = 1.5

    def __init__(self, channel_map: Mapping[int, int] | None = None, pending_ttl: float = 120.0,
                 clock: Callable[[], float] = time.monotonic) -> None:
        self._map = dict(channel_map or {})
        self._ttl = pending_ttl
        self._clock = clock
        self._lock = threading.RLock()
        self.keyboard: Keyboard | None = None
        self.mouse: SwitchableDevice | None = None
        self._keyboard_host: int | None = None
        self._pending: tuple[int, float] | None = None
        self._last_press: tuple[int, float] | None = None

    # --- queries ------------------------------------------------------------------------

    @property
    def pending_slot(self) -> int | None:
        with self._lock:
            if self._pending and self._clock() - self._pending[1] <= self._ttl:
                return self._pending[0]
            return None

    def mouse_slot_for(self, keyboard_slot: int) -> int:
        return self._map.get(keyboard_slot, keyboard_slot)

    # --- device lifecycle ---------------------------------------------------------------

    def keyboard_connected(self, keyboard: Keyboard) -> None:
        with self._lock:
            self.keyboard = keyboard
            if self._pending is not None:
                log.info("keyboard is back on this computer; cancelling pending mouse switch")
                self._pending = None
            try:
                keyboard.enable_easy_switch_reporting(True)
                self._keyboard_host = keyboard.host_info().current
                log.info("keyboard ready on host slot %d; Easy-Switch reporting enabled",
                         self._keyboard_host + 1)
            except HidppError as exc:
                log.warning("could not set up keyboard: %s", exc)

    def keyboard_disconnected(self) -> None:
        with self._lock:
            self.keyboard = None

    def mouse_connected(self, mouse: SwitchableDevice) -> None:
        with self._lock:
            self.mouse = mouse
            slot = self.pending_slot
            self._pending = None
            if slot is not None:
                log.info("mouse connected with a pending switch; sending it to slot %d", slot + 1)
                self._switch_mouse(slot)

    def mouse_disconnected(self) -> None:
        with self._lock:
            self.mouse = None

    # --- events -------------------------------------------------------------------------

    def easy_switch_pressed(self, keyboard_slot: int) -> None:
        with self._lock:
            now = self._clock()
            if self._last_press and self._last_press[0] == keyboard_slot \
                    and now - self._last_press[1] < self.DEBOUNCE_S:
                return
            self._last_press = (keyboard_slot, now)
            if keyboard_slot == self._keyboard_host:
                log.info("Easy-Switch %d pressed: already on that host, nothing to do",
                         keyboard_slot + 1)
                return
            target = self.mouse_slot_for(keyboard_slot)
            log.info("Easy-Switch %d pressed: moving mouse to its channel %d",
                     keyboard_slot + 1, target + 1)
            self._switch_mouse(target)

    def switch_all(self, keyboard_slot: int) -> None:
        """Manual switch: mouse first (keyboard leaving would take our event source)."""
        with self._lock:
            self._switch_mouse(self.mouse_slot_for(keyboard_slot))
            if self.keyboard is not None:
                self.keyboard.change_host(keyboard_slot)

    # --- internals ----------------------------------------------------------------------

    def _switch_mouse(self, slot: int) -> None:
        mouse = self.mouse
        if mouse is None:
            log.info("mouse not connected here; switch to channel %d is pending", slot + 1)
            self._pending = (slot, self._clock())
            return
        try:
            try:
                if mouse.host_info().current == slot:
                    log.info("mouse already on channel %d", slot + 1)
                    return
            except HidppError:
                pass  # asleep or slow; try the switch anyway
            mouse.change_host(slot)
            log.info("mouse sent to channel %d", slot + 1)
        except DeviceUnreachable as exc:
            log.info("mouse unreachable (%s); switch to channel %d is pending", exc, slot + 1)
            self._pending = (slot, self._clock())
