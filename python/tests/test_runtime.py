"""Runtime wiring against a simulated Bolt receiver with a keyboard and a mouse."""

from __future__ import annotations

from dataclasses import dataclass, field

from logimxplus import hidpp
from logimxplus.config import Config
from logimxplus.domain import DeviceUnreachable, HidppError
from logimxplus.runtime import Runtime
from logimxplus.transport import Endpoint, NotificationHandler

FEATURES = {0x0005: 1, 0x1814: 10, 0x1815: 11, 0x1B04: 9, 0x1D4B: 4}


@dataclass
class SimDevice:
    name: str
    kind: int
    host: int = 1
    connected: bool = True
    analytics: set[int] = field(default_factory=set)
    host_changes: list[int] = field(default_factory=list)


class SimReceiver:
    name = "sim receiver"
    is_receiver = True
    alive = True

    def __init__(self, devices: dict[int, SimDevice]) -> None:
        self.devices = devices
        self.handler: NotificationHandler | None = None
        self.registers: list[tuple[int, bytes]] = []

    def set_handler(self, handler: NotificationHandler) -> None:
        self.handler = handler

    def write_receiver_register(self, register: int, value: bytes) -> None:
        self.registers.append((register, value))

    def send(self, device: int, feature_index: int, func: int, params: bytes = b"") -> None:
        self.request(device, feature_index, func, params)

    def request(self, device: int, feature_index: int, func: int, params: bytes = b"",
                timeout: float = 2.0) -> bytes:
        dev = self.devices.get(device)
        if dev is None or not dev.connected:
            raise DeviceUnreachable("absent", 4)
        if feature_index == 0:
            if func == hidpp.ROOT_PING:
                return bytes([4, 5, params[2]])
            fid = (params[0] << 8) | params[1]
            return bytes([FEATURES.get(fid, 0), 0, 0])
        if feature_index == 1:
            name = dev.name.encode()
            if func == hidpp.NAME_GET_COUNT:
                return bytes([len(name)])
            if func == hidpp.NAME_GET_NAME:
                return name[params[0]:]
            return bytes([dev.kind])
        if feature_index == 10:
            if func == hidpp.CHANGE_HOST_GET_INFO:
                return bytes([3, dev.host, 0])
            dev.host_changes.append(params[0])
            dev.host = params[0]
            raise DeviceUnreachable("timed out")  # real devices drop the link, no reply
        if feature_index == 9 and func == hidpp.REPROG_SET_REPORTING:
            cid = (params[0] << 8) | params[1]
            if params[5] & 0x01:
                dev.analytics.add(cid)
            return params
        raise HidppError("unsupported", 2)

    def notify(self, raw: bytes) -> None:
        msg = hidpp.parse(raw)
        assert msg and self.handler
        self.handler(self, msg)

    def close(self) -> None:
        self.alive = False


def make_runtime(rx: SimReceiver, config: Config | None = None) -> Runtime:
    ep = Endpoint("sim", 0xC548, True, (b"p",), b"p")
    return Runtime(config or Config(), discover_fn=lambda: [ep],
                   open_fn=lambda _: rx)  # type: ignore[arg-type,return-value]


def analytics(cid: int) -> bytes:
    return bytes([0x11, 4, 9, 0x20, cid >> 8, cid & 0xFF, 0x01] + [0] * 13)


def test_scan_finds_devices_and_arms_keyboard() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    assert rt.keyboard is not None and rt.keyboard.name == "MX Keys S"
    assert rt.mouse is not None and rt.mouse.index == 3
    assert kb.analytics == {0xD1, 0xD2, 0xD3}
    assert rx.registers == [(0x00, bytes([0x00, 0x09, 0x00]))]


def test_easy_switch_event_moves_mouse() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    rx.notify(analytics(0xD1))
    assert mouse.host_changes == [0]
    assert kb.host_changes == []


def test_change_host_announcement_moves_mouse() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    rx.notify(bytes([0x11, 4, 10, 0x00, 0x01, 0x00] + [0] * 14))  # leaving 2 for 1
    assert mouse.host_changes == [0]


def test_events_from_other_devices_are_ignored() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    raw = bytearray(analytics(0xD1))
    raw[1] = 3  # same feature index, but from the mouse
    rx.notify(bytes(raw))
    assert mouse.host_changes == []


def test_keyboard_reconnect_rearms_reporting() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    rx.notify(bytes([0x10, 4, 0x41, 0x10, 0x41, 0x6A, 0xB3]))  # link down
    assert rt.keyboard is None
    kb.analytics.clear()  # device forgot its settings
    rx.notify(bytes([0x10, 4, 0x41, 0x10, 0x01, 0x6A, 0xB3]))  # link up
    assert rt.keyboard is not None and kb.analytics == {0xD1, 0xD2, 0xD3}


def test_mouse_waking_up_receives_pending_switch() -> None:
    kb, mouse = SimDevice("MX Keys S", 0), SimDevice("MX Master 3S", 3, connected=False)
    rx = SimReceiver({3: mouse, 4: kb})
    rt = make_runtime(rx)
    rt.scan()
    assert rt.mouse is None
    rx.notify(analytics(0xD3))
    mouse.connected = True
    rx.notify(bytes([0x10, 3, 0x41, 0x10, 0x02, 0x34, 0xB0]))
    assert mouse.host_changes == [2]


def test_name_filter_skips_other_keyboards() -> None:
    kb = SimDevice("K380", 0)
    rt = make_runtime(SimReceiver({1: kb}), Config(keyboard_name="MX Keys"))
    rt.scan()
    assert rt.keyboard is None
