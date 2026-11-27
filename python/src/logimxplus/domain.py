"""Domain types. Standard library only."""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum


class DeviceKind(IntEnum):
    """Device type as reported by HID++ feature 0x0005 getDeviceType."""

    KEYBOARD = 0
    REMOTE_CONTROL = 1
    NUMPAD = 2
    MOUSE = 3
    TRACKPAD = 4
    TRACKBALL = 5
    PRESENTER = 6
    RECEIVER = 7
    UNKNOWN = 0xFF


# Control IDs of the keyboard's Easy-Switch keys -> zero-based host slot.
EASY_SWITCH_CIDS: dict[int, int] = {0x00D1: 0, 0x00D2: 1, 0x00D3: 2}


@dataclass(frozen=True)
class HostInfo:
    """Easy-Switch state of a device. ``current`` is zero-based."""

    count: int
    current: int


@dataclass(frozen=True)
class DeviceId:
    """Identifies a device on a transport (receiver slot 1-6, or 0xFF for direct)."""

    transport: str
    index: int


class HidppError(Exception):
    """A HID++ request failed with a protocol error code."""

    def __init__(self, message: str, code: int | None = None) -> None:
        super().__init__(message)
        self.code = code


class DeviceUnreachable(HidppError):
    """The device did not answer: asleep, out of range, or connected to another host."""
