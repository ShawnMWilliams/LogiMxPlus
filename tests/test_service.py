from __future__ import annotations

import pytest

from logimxplus.domain import DeviceUnreachable, HidppError, HostInfo
from logimxplus.service import SwitchService


class FakeDevice:
    def __init__(self, current: int = 1, reachable: bool = True) -> None:
        self.current = current
        self.reachable = reachable
        self.switched_to: list[int] = []
        self.reporting_enabled = False

    def host_info(self) -> HostInfo:
        if not self.reachable:
            raise DeviceUnreachable("asleep", 4)
        return HostInfo(count=3, current=self.current)

    def change_host(self, slot: int) -> None:
        if not self.reachable:
            raise DeviceUnreachable("asleep", 4)
        self.switched_to.append(slot)

    def enable_easy_switch_reporting(self, enabled: bool = True) -> None:
        self.reporting_enabled = enabled


class Clock:
    def __init__(self) -> None:
        self.now = 1000.0

    def __call__(self) -> float:
        return self.now


@pytest.fixture
def clock() -> Clock:
    return Clock()


def make(clock: Clock, channel_map: dict[int, int] | None = None) -> SwitchService:
    return SwitchService(channel_map, pending_ttl=60, clock=clock)


def test_press_moves_mouse_to_same_channel(clock: Clock) -> None:
    svc, kb, mouse = make(clock), FakeDevice(1), FakeDevice(1)
    svc.keyboard_connected(kb)
    svc.mouse_connected(mouse)
    assert kb.reporting_enabled
    svc.easy_switch_pressed(0)
    assert mouse.switched_to == [0]
    assert kb.switched_to == []  # the keyboard switches itself


def test_channel_map_is_applied(clock: Clock) -> None:
    svc, mouse = make(clock, {0: 2}), FakeDevice(1)
    svc.keyboard_connected(FakeDevice(1))
    svc.mouse_connected(mouse)
    svc.easy_switch_pressed(0)
    assert mouse.switched_to == [2]


def test_pressing_current_host_key_does_nothing(clock: Clock) -> None:
    svc, mouse = make(clock), FakeDevice(1)
    svc.keyboard_connected(FakeDevice(1))
    svc.mouse_connected(mouse)
    svc.easy_switch_pressed(1)
    assert mouse.switched_to == []


def test_mouse_already_on_target_is_not_switched(clock: Clock) -> None:
    svc, mouse = make(clock), FakeDevice(2)
    svc.keyboard_connected(FakeDevice(1))
    svc.mouse_connected(mouse)
    svc.easy_switch_pressed(2)
    assert mouse.switched_to == []


def test_repeat_press_is_debounced(clock: Clock) -> None:
    svc, mouse = make(clock), FakeDevice(1)
    svc.keyboard_connected(FakeDevice(1))
    svc.mouse_connected(mouse)
    svc.easy_switch_pressed(0)
    svc.easy_switch_pressed(0)  # e.g. the release record
    assert mouse.switched_to == [0]


def test_unreachable_mouse_gets_switch_when_it_connects(clock: Clock) -> None:
    svc, mouse = make(clock), FakeDevice(1, reachable=False)
    svc.keyboard_connected(FakeDevice(1))
    svc.mouse_connected(mouse)
    svc.easy_switch_pressed(2)
    assert svc.pending_slot == 2
    mouse.reachable = True
    clock.now += 5
    svc.mouse_disconnected()
    svc.mouse_connected(mouse)
    assert mouse.switched_to == [2]
    assert svc.pending_slot is None


def test_pending_switch_expires(clock: Clock) -> None:
    svc = make(clock)
    svc.keyboard_connected(FakeDevice(1))
    svc.easy_switch_pressed(0)  # no mouse here
    clock.now += 61
    mouse = FakeDevice(1)
    svc.mouse_connected(mouse)
    assert mouse.switched_to == []


def test_keyboard_returning_cancels_pending(clock: Clock) -> None:
    svc = make(clock)
    kb = FakeDevice(1)
    svc.keyboard_connected(kb)
    svc.easy_switch_pressed(0)
    assert svc.pending_slot == 0
    svc.keyboard_disconnected()
    svc.keyboard_connected(kb)
    assert svc.pending_slot is None


def test_switch_all_moves_mouse_then_keyboard(clock: Clock) -> None:
    svc, kb, mouse = make(clock), FakeDevice(1), FakeDevice(1)
    svc.keyboard_connected(kb)
    svc.mouse_connected(mouse)
    svc.switch_all(0)
    assert mouse.switched_to == [0] and kb.switched_to == [0]


def test_keyboard_setup_failure_is_tolerated(clock: Clock) -> None:
    class Broken(FakeDevice):
        def enable_easy_switch_reporting(self, enabled: bool = True) -> None:
            raise HidppError("nope")

    svc = make(clock)
    svc.keyboard_connected(Broken(1))  # must not raise
    assert svc.keyboard is not None
