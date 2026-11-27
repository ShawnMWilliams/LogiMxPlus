"""Pure HID++ encoding and decoding. No I/O.

Report layout (HID++ 2.0):
    short: 0x10 dev sub addr p0 p1 p2                      (7 bytes)
    long:  0x11 dev featIdx (func<<4 | swId) p0 .. p15      (20 bytes)

Notifications carry software id 0. Requests we send carry ``SW_ID`` so replies can
be told apart from notifications and from other software on the same device.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum

REPORT_SHORT = 0x10
REPORT_LONG = 0x11
SHORT_LEN = 7
LONG_LEN = 20
SW_ID = 0x0B

DIRECT_INDEX = 0xFF  # device connected directly (Bluetooth / USB cable), not via receiver
RECEIVER_SLOTS = range(1, 7)

ERROR_20 = 0xFF  # HID++ 2.0 error sub id
ERROR_10 = 0x8F  # HID++ 1.0 error sub id (receiver answers for an absent device)
NOTIFY_CONNECTION = 0x41  # receiver: device connected / disconnected
LINK_NOT_ESTABLISHED = 0x40
REGISTER_SUB_IDS = (0x80, 0x81, 0x82, 0x83)  # set/get short/long register

HIDPP10_ERR_CONNECT_FAIL = 0x04
HIDPP10_ERR_UNKNOWN_DEVICE = 0x08
HIDPP10_ERR_RESOURCE = 0x09


class Feature(IntEnum):
    ROOT = 0x0000
    DEVICE_NAME = 0x0005
    CHANGE_HOST = 0x1814
    HOSTS_INFO = 0x1815
    REPROG_CONTROLS_V4 = 0x1B04
    WIRELESS_STATUS = 0x1D4B


# Function numbers
ROOT_GET_FEATURE = 0
ROOT_PING = 1
NAME_GET_COUNT = 0
NAME_GET_NAME = 1
NAME_GET_TYPE = 2
CHANGE_HOST_GET_INFO = 0
CHANGE_HOST_SET = 1
REPROG_GET_REPORTING = 2
REPROG_SET_REPORTING = 3

# Events
REPROG_EVENT_ANALYTICS = 2
WIRELESS_STATUS_EVENT = 0

# setCidReporting byte 5: analytics key events (bit0) + valid (bit1)
ANALYTICS_ON = 0x03
ANALYTICS_OFF = 0x02


@dataclass(frozen=True)
class Message:
    report_id: int
    device: int
    sub_id: int  # feature index (2.0) or sub id (1.0)
    address: int  # func<<4|sw (2.0) or register address (1.0)
    data: bytes

    @property
    def func(self) -> int:
        return self.address >> 4

    @property
    def sw_id(self) -> int:
        return self.address & 0x0F

    @property
    def is_error(self) -> bool:
        return self.sub_id in (ERROR_20, ERROR_10)

    @property
    def is_notification(self) -> bool:
        """True for unsolicited messages (events), false for replies to requests."""
        if self.is_error:
            return False
        if self.report_id == REPORT_SHORT and 0x40 <= self.sub_id < 0x80:
            return True  # HID++ 1.0 receiver notification (e.g. 0x41 connection)
        if self.device == DIRECT_INDEX and self.sub_id in REGISTER_SUB_IDS:
            return False  # HID++ 1.0 reply to a receiver register read/write
        return self.sw_id == 0


def parse(raw: bytes | list[int]) -> Message | None:
    b = bytes(raw)
    if len(b) < 4 or b[0] not in (REPORT_SHORT, REPORT_LONG):
        return None
    return Message(report_id=b[0], device=b[1], sub_id=b[2], address=b[3], data=b[4:])


def build_request(device: int, feature_index: int, func: int, params: bytes = b"") -> bytes:
    if len(params) > LONG_LEN - 4:
        raise ValueError("too many parameters for a long report")
    head = bytes([REPORT_LONG, device, feature_index, ((func & 0x0F) << 4) | SW_ID])
    return (head + params).ljust(LONG_LEN, b"\x00")


def is_reply_to(msg: Message, device: int, feature_index: int, func: int) -> bool:
    """True if ``msg`` answers our request (success or error)."""
    if msg.device != device:
        return False
    expected_addr = ((func & 0x0F) << 4) | SW_ID
    if msg.sub_id == ERROR_20:
        return msg.address == feature_index and len(msg.data) >= 1 and msg.data[0] == expected_addr
    if msg.sub_id == ERROR_10:
        return msg.address == feature_index
    return msg.sub_id == feature_index and msg.address == expected_addr


def error_code(msg: Message) -> int | None:
    if msg.sub_id == ERROR_20 and len(msg.data) >= 2:
        return msg.data[1]
    if msg.sub_id == ERROR_10 and len(msg.data) >= 2:
        return msg.data[1]
    return None


# --- parameter encoders -----------------------------------------------------------------


def feature_id_params(feature: int) -> bytes:
    return bytes([(feature >> 8) & 0xFF, feature & 0xFF])


def set_analytics_params(cid: int, enabled: bool) -> bytes:
    """setCidReporting: leave divert/persist/rawXY/remap alone, set only analytics."""
    return bytes([
        (cid >> 8) & 0xFF, cid & 0xFF,
        0x00,  # flags: no *valid bits set, so divert etc. stay unchanged
        0x00, 0x00,  # remap: 0 = unchanged
        ANALYTICS_ON if enabled else ANALYTICS_OFF,
    ])


def cid_params(cid: int) -> bytes:
    return bytes([(cid >> 8) & 0xFF, cid & 0xFF])


# --- decoders ---------------------------------------------------------------------------


def connection_link_up(msg: Message) -> bool | None:
    """For a receiver 0x41 notification return whether the link is up, else None."""
    if msg.report_id != REPORT_SHORT or msg.sub_id != NOTIFY_CONNECTION or not msg.data:
        return None
    return not (msg.data[0] & LINK_NOT_ESTABLISHED)


def analytics_events(data: bytes) -> list[tuple[int, int]]:
    """Decode an analyticsKeyEvents payload into (cid, event) pairs.

    Payload is a list of 3-byte records: CID (big endian, 2 bytes) + event byte,
    terminated by a zero CID.
    """
    out: list[tuple[int, int]] = []
    for i in range(0, len(data) - 2, 3):
        cid = (data[i] << 8) | data[i + 1]
        if cid == 0:
            break
        out.append((cid, data[i + 2]))
    return out


def analytics_enabled(reporting_reply: bytes) -> bool:
    """Decode getCidReporting reply: byte 5 bit0 = analytics key events on."""
    return len(reporting_reply) > 5 and bool(reporting_reply[5] & 0x01)
