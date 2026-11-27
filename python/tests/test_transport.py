from __future__ import annotations

import queue
import threading
from typing import Any

import pytest

from logimxplus import hidpp
from logimxplus.domain import DeviceUnreachable, HidppError
from logimxplus.transport import HidTransport, Transport, find_endpoints


class FakeHandle:
    """Answers requests via a responder callback; can inject notifications."""

    def __init__(self, responder: Any = None) -> None:
        self.inbox: queue.Queue[list[int]] = queue.Queue()
        self.written: list[bytes] = []
        self.responder = responder

    def write(self, data: list[int]) -> int:
        self.written.append(bytes(data))
        if self.responder:
            for reply in self.responder(bytes(data)):
                self.inbox.put(list(reply))
        return len(data)

    def read(self, max_length: int, timeout_ms: int = 0) -> list[int]:
        try:
            return self.inbox.get(timeout=timeout_ms / 1000)
        except queue.Empty:
            return []

    def close(self) -> None:
        pass


def echo_ok(req: bytes) -> list[bytes]:
    noise = bytes([0x11, req[1], req[2], req[3] & 0xF0 | 0x01, 9, 9])  # other software
    return [noise, req[:4] + b"\x2a"]


def test_request_returns_matching_reply_and_skips_foreign_ones() -> None:
    h = FakeHandle(echo_ok)
    t = HidTransport("fake", True, [h], h)
    try:
        assert t.request(3, 5, 1, b"\x00")[0] == 0x2A
    finally:
        t.close()


def test_hidpp10_error_means_unreachable() -> None:
    h = FakeHandle(lambda r: [bytes([0x10, r[1], 0x8F, r[2], r[3], 0x04, 0])])
    t = HidTransport("fake", True, [h], h)
    try:
        with pytest.raises(DeviceUnreachable) as exc:
            t.request(1, 0, 1)
        assert exc.value.code == 4
    finally:
        t.close()


def test_hidpp20_error_raises() -> None:
    h = FakeHandle(lambda r: [bytes([0x11, r[1], 0xFF, r[2], r[3], 0x05, 0])])
    t = HidTransport("fake", True, [h], h)
    try:
        with pytest.raises(HidppError) as exc:
            t.request(4, 9, 3)
        assert exc.value.code == 5 and not isinstance(exc.value, DeviceUnreachable)
    finally:
        t.close()


def test_timeout_is_unreachable_without_code() -> None:
    h = FakeHandle()
    t = HidTransport("fake", True, [h], h)
    try:
        with pytest.raises(DeviceUnreachable) as exc:
            t.request(4, 9, 3, timeout=0.2)
        assert exc.value.code is None
    finally:
        t.close()


def test_notifications_reach_handler_from_any_handle() -> None:
    short, long_ = FakeHandle(), FakeHandle()
    t = HidTransport("fake", True, [short, long_], long_, short)
    got: list[hidpp.Message] = []
    done = threading.Event()

    def handler(_: Transport, msg: hidpp.Message) -> None:
        got.append(msg)
        if len(got) == 2:
            done.set()

    t.set_handler(handler)
    try:
        short.inbox.put([0x10, 3, 0x41, 0x10, 0x01, 0x34, 0xB0])
        long_.inbox.put([0x11, 4, 9, 0x20, 0x00, 0xD1, 0x01] + [0] * 13)
        assert done.wait(2)
        assert {m.sub_id for m in got} == {0x41, 9}
    finally:
        t.close()


def test_receiver_register_write_uses_short_handle() -> None:
    short, long_ = FakeHandle(), FakeHandle()
    t = HidTransport("fake", True, [short, long_], long_, short)
    try:
        t.write_receiver_register(0x00, b"\x00\x09\x00")
        assert short.written == [bytes([0x10, 0xFF, 0x80, 0x00, 0x00, 0x09, 0x00])]
        assert long_.written == []
    finally:
        t.close()


def _info(path: str, page: int, usage: int, pid: int = 0xC548) -> dict[str, Any]:
    return {"vendor_id": 0x046D, "product_id": pid, "usage_page": page, "usage": usage,
            "path": path.encode(), "product_string": "USB Receiver"}


def test_find_endpoints_groups_windows_collections() -> None:
    infos = [
        _info(r"\\?\HID#VID_046D&PID_C548&MI_02&Col01#7&1dc7413e&0&0000#{x}", 0xFF00, 1),
        _info(r"\\?\HID#VID_046D&PID_C548&MI_02&Col02#7&1dc7413e&0&0001#{x}", 0xFF00, 2),
        _info(r"\\?\HID#VID_046D&PID_C548&MI_01&Col01#7&30f75d7c&0&0000#{x}", 0x0001, 2),
        _info(r"\\?\HID#{00001812}_VID&0002046d_PID&b378&Col03#9&1#{x}", 0xFF43, 0x0202, 0xB378),
    ]
    eps = find_endpoints(infos)
    assert len(eps) == 2
    receiver = next(e for e in eps if e.is_receiver)
    assert len(receiver.paths) == 2 and receiver.short_path and b"Col01" in receiver.short_path
    assert b"Col02" in receiver.long_path
    ble = next(e for e in eps if not e.is_receiver)
    assert ble.product_id == 0xB378 and ble.short_path is None


def test_find_endpoints_dedupes_identical_paths() -> None:
    infos = [_info("DevSrvsID:42", 0xFF00, 1), _info("DevSrvsID:42", 0xFF00, 2)]
    (ep,) = find_endpoints(infos)
    assert ep.paths == (b"DevSrvsID:42",)
