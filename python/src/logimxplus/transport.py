"""Gateway: HID++ request/response and notification transport over hidapi.

One ``HidTransport`` represents one physical HID++ endpoint: a receiver (Bolt/Unifying)
or a directly connected device (Bluetooth). On Windows the short (0x10) and long (0x11)
HID++ reports live on separate HID collections, so a transport may own several handles;
it writes long reports only and reads every handle.
"""

from __future__ import annotations

import logging
import queue
import re
import threading
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any, Protocol

from . import hidpp
from .domain import DeviceUnreachable, HidppError

log = logging.getLogger(__name__)

LOGITECH_VID = 0x046D
USAGE_PAGE_RECEIVER = 0xFF00  # receivers: usage 1 = short, 2 = long
USAGE_PAGE_BLE = 0xFF43  # Bluetooth LE devices: usage 0x0202 = long

NotificationHandler = Callable[["Transport", hidpp.Message], None]


class HidHandle(Protocol):
    def write(self, data: list[int]) -> int: ...
    def read(self, max_length: int, timeout_ms: int = 0) -> list[int]: ...
    def close(self) -> None: ...


class Transport(Protocol):
    @property
    def name(self) -> str: ...
    @property
    def is_receiver(self) -> bool: ...
    @property
    def alive(self) -> bool: ...
    def request(
        self, device: int, feature_index: int, func: int, params: bytes = b"",
        timeout: float = 2.0,
    ) -> bytes: ...
    def send(self, device: int, feature_index: int, func: int, params: bytes = b"") -> None: ...
    def write_receiver_register(self, register: int, value: bytes) -> None: ...
    def set_handler(self, handler: NotificationHandler) -> None: ...
    def close(self) -> None: ...


@dataclass(frozen=True)
class Endpoint:
    """A discovered HID++ endpoint and the HID paths that make it up."""

    name: str
    product_id: int
    is_receiver: bool
    paths: tuple[bytes, ...]
    long_path: bytes
    short_path: bytes | None = None


class HidTransport:
    def __init__(self, name: str, is_receiver: bool, handles: Sequence[HidHandle],
                 long_handle: HidHandle, short_handle: HidHandle | None = None) -> None:
        self._name = name
        self._is_receiver = is_receiver
        self._handles = list(handles)
        self._long = long_handle
        self._short = short_handle
        self._replies: queue.Queue[hidpp.Message] = queue.Queue()
        self._events: queue.Queue[hidpp.Message | None] = queue.Queue()
        self._handler: NotificationHandler | None = None
        self._req_lock = threading.Lock()
        self._write_lock = threading.Lock()
        self._alive = True
        self._threads = [
            threading.Thread(target=self._read_loop, args=(h,), daemon=True,
                             name=f"hid-read-{name}-{i}")
            for i, h in enumerate(self._handles)
        ]
        self._threads.append(
            threading.Thread(target=self._dispatch_loop, daemon=True, name=f"hid-events-{name}"))
        for t in self._threads:
            t.start()

    @property
    def name(self) -> str:
        return self._name

    @property
    def is_receiver(self) -> bool:
        return self._is_receiver

    @property
    def alive(self) -> bool:
        return self._alive

    def set_handler(self, handler: NotificationHandler) -> None:
        self._handler = handler

    def _read_loop(self, handle: HidHandle) -> None:
        while self._alive:
            try:
                raw = handle.read(64, 200)
            except (OSError, ValueError) as exc:
                log.warning("%s: read failed (%s); transport closed", self._name, exc)
                self._alive = False
                self._events.put(None)
                return
            if not raw:
                continue
            msg = hidpp.parse(raw)
            if msg is None:
                continue
            log.debug("%s < %s", self._name, bytes(raw).hex())
            if msg.is_notification:
                self._events.put(msg)
            else:
                self._replies.put(msg)

    def _dispatch_loop(self) -> None:
        while True:
            msg = self._events.get()
            if msg is None:
                return
            handler = self._handler
            if handler is None:
                continue
            try:
                handler(self, msg)
            except Exception:
                log.exception("%s: notification handler failed", self._name)

    def _write(self, device: int, feature_index: int, func: int, params: bytes) -> None:
        if not self._alive:
            raise DeviceUnreachable(f"{self._name} is closed")
        report = hidpp.build_request(device, feature_index, func, params)
        log.debug("%s > %s", self._name, report.hex())
        with self._write_lock:
            try:
                written = self._long.write(list(report))
            except (OSError, ValueError) as exc:
                self._alive = False
                raise DeviceUnreachable(f"{self._name}: write failed: {exc}") from exc
        if written < 0:
            raise DeviceUnreachable(f"{self._name}: write failed")

    def send(self, device: int, feature_index: int, func: int, params: bytes = b"") -> None:
        self._write(device, feature_index, func, params)

    def write_receiver_register(self, register: int, value: bytes) -> None:
        """HID++ 1.0 short register write to the receiver itself (fire and forget)."""
        report = bytes([hidpp.REPORT_SHORT, hidpp.DIRECT_INDEX, 0x80, register]) + value[:3]
        handle = self._short or self._long
        log.debug("%s > %s", self._name, report.ljust(hidpp.SHORT_LEN, b"\0").hex())
        with self._write_lock:
            handle.write(list(report.ljust(hidpp.SHORT_LEN, b"\x00")))

    def request(self, device: int, feature_index: int, func: int, params: bytes = b"",
                timeout: float = 2.0) -> bytes:
        with self._req_lock:
            while not self._replies.empty():  # drop stale replies
                self._replies.get_nowait()
            self._write(device, feature_index, func, params)
            deadline = time.monotonic() + timeout
            while (remaining := deadline - time.monotonic()) > 0:
                try:
                    msg = self._replies.get(timeout=remaining)
                except queue.Empty:
                    break
                if not hidpp.is_reply_to(msg, device, feature_index, func):
                    continue
                if msg.is_error:
                    code = hidpp.error_code(msg)
                    if msg.sub_id == hidpp.ERROR_10:
                        raise DeviceUnreachable(
                            f"{self._name}[{device}] not reachable (code {code})", code)
                    raise HidppError(
                        f"{self._name}[{device}] feature {feature_index} func {func} "
                        f"error {code}", code)
                return msg.data
            raise DeviceUnreachable(f"{self._name}[{device}] timed out")

    def close(self) -> None:
        self._alive = False
        self._events.put(None)
        for t in self._threads:
            if t is not threading.current_thread():
                t.join(timeout=1)
        for h in self._handles:
            try:
                h.close()
            except Exception:
                log.debug("close failed", exc_info=True)


# --- discovery --------------------------------------------------------------------------


def _group_key(info: dict[str, Any]) -> str:
    r"""Windows exposes one path per HID collection of the same interface, e.g.
    ``\\?\HID#VID_046D&PID_C548&MI_02&Col01#7&1dc7413e&0&0000#{guid}`` and
    ``...&Col02#7&1dc7413e&0&0001#{guid}``. Strip the collection and its instance
    suffix so both map to one key."""
    path = bytes(info["path"]).decode("latin1").lower()
    path = re.sub(r"&col[0-9a-f]{2}", "", path)
    return re.sub(r"&[0-9a-f]{4}(#\{)", r"\1", path)


def find_endpoints(infos: Sequence[dict[str, Any]]) -> list[Endpoint]:
    """Group hidapi enumeration entries into HID++ endpoints."""
    groups: dict[str, list[dict[str, Any]]] = {}
    for info in infos:
        if info.get("vendor_id") != LOGITECH_VID:
            continue
        page, usage = info.get("usage_page"), info.get("usage")
        if (page == USAGE_PAGE_RECEIVER and usage in (1, 2)) or (
                page == USAGE_PAGE_BLE and usage == 0x0202):
            groups.setdefault(_group_key(info), []).append(info)
    endpoints: list[Endpoint] = []
    for members in groups.values():
        long_info = next(
            (m for m in members if m["usage"] in (2, 0x0202)), None)
        if long_info is None:
            continue  # need a long-report collection to talk HID++ 2.0
        short_info = next((m for m in members if m["usage"] == 1), None)
        paths = tuple(dict.fromkeys(bytes(m["path"]) for m in members))
        product = str(long_info.get("product_string") or f"{long_info['product_id']:04x}")
        endpoints.append(Endpoint(
            name=f"{product} ({long_info['product_id']:04x})",
            product_id=int(long_info["product_id"]),
            is_receiver=long_info["usage_page"] == USAGE_PAGE_RECEIVER,
            paths=paths,
            long_path=bytes(long_info["path"]),
            short_path=bytes(short_info["path"]) if short_info else None,
        ))
    return endpoints


def open_endpoint(ep: Endpoint) -> HidTransport:
    import hid

    handles: dict[bytes, HidHandle] = {}
    try:
        for path in ep.paths:
            h = hid.device()
            h.open_path(path)
            handles[path] = h
    except Exception:
        for h in handles.values():
            h.close()
        raise
    short = handles.get(ep.short_path) if ep.short_path else None
    return HidTransport(ep.name, ep.is_receiver, list(handles.values()), handles[ep.long_path],
                        short)


def discover() -> list[Endpoint]:
    import hid

    return find_endpoints(hid.enumerate(LOGITECH_VID, 0))
