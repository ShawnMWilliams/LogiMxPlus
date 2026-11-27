from logimxplus import hidpp


def test_build_request_is_long_report_with_sw_id() -> None:
    r = hidpp.build_request(3, 0x0A, 1, b"\x01")
    assert len(r) == hidpp.LONG_LEN
    assert r[:5] == bytes([0x11, 3, 0x0A, 0x10 | hidpp.SW_ID, 0x01])
    assert set(r[5:]) == {0}


def test_reply_matching_success_and_errors() -> None:
    ok = hidpp.parse(bytes([0x11, 4, 9, 0x20 | hidpp.SW_ID, 0, 0xD1]))
    assert ok and hidpp.is_reply_to(ok, 4, 9, 2) and not ok.is_notification
    assert not hidpp.is_reply_to(ok, 3, 9, 2)
    assert not hidpp.is_reply_to(ok, 4, 9, 3)

    err20 = hidpp.parse(bytes([0x11, 4, 0xFF, 9, 0x20 | hidpp.SW_ID, 0x02, 0]))
    assert err20 and hidpp.is_reply_to(err20, 4, 9, 2) and hidpp.error_code(err20) == 2

    err10 = hidpp.parse(bytes([0x10, 1, 0x8F, 0x00, 0x1B, 0x04, 0]))
    assert err10 and hidpp.is_reply_to(err10, 1, 0, 1) and hidpp.error_code(err10) == 4


def test_other_software_reply_is_not_ours() -> None:
    other = hidpp.parse(bytes([0x11, 4, 9, 0x21, 0, 0xD1]))  # sw id 1 (e.g. Options+)
    assert other and not hidpp.is_reply_to(other, 4, 9, 2)
    assert not other.is_notification


def test_notification_detection() -> None:
    event = hidpp.parse(bytes([0x11, 4, 9, 0x20, 0x00, 0xD2, 0x01]))
    assert event and event.is_notification and event.func == 2
    conn = hidpp.parse(bytes([0x10, 3, 0x41, 0x10, 0x41, 0x25, 0xB0]))
    assert conn and conn.is_notification
    register_ack = hidpp.parse(bytes([0x10, 0xFF, 0x80, 0x00, 0, 0, 0]))
    assert register_ack and not register_ack.is_notification


def test_connection_link_state() -> None:
    up = hidpp.parse(bytes([0x10, 3, 0x41, 0x10, 0x01, 0x34, 0xB0]))
    down = hidpp.parse(bytes([0x10, 3, 0x41, 0x10, 0x41, 0x34, 0xB0]))
    other = hidpp.parse(bytes([0x11, 3, 0x41, 0x10, 0x41]))
    assert up and hidpp.connection_link_up(up) is True
    assert down and hidpp.connection_link_up(down) is False
    assert other and hidpp.connection_link_up(other) is None


def test_analytics_events_decoding() -> None:
    data = bytes([0x00, 0xD2, 0x01, 0x00, 0x00, 0x00] + [0] * 10)
    assert hidpp.analytics_events(data) == [(0xD2, 1)]
    two = bytes([0x00, 0xD1, 0x01, 0x00, 0xE4, 0x00, 0, 0, 0])
    assert hidpp.analytics_events(two) == [(0xD1, 1), (0xE4, 0)]
    assert hidpp.analytics_events(b"") == []


def test_set_analytics_params_does_not_touch_divert() -> None:
    p = hidpp.set_analytics_params(0xD3, True)
    assert p == bytes([0x00, 0xD3, 0x00, 0x00, 0x00, 0x03])
    assert hidpp.set_analytics_params(0xD3, False)[5] == 0x02


def test_format_firmware() -> None:
    reply = bytes([0x00]) + b"RBK" + bytes([0x81, 0x02, 0x00, 0x17]) + bytes(8)
    assert hidpp.format_firmware(reply) == "RBK81.02_B0017"


def test_analytics_enabled_from_reporting_reply() -> None:
    assert hidpp.analytics_enabled(bytes([0, 0xD1, 0, 0, 0, 1]))
    assert not hidpp.analytics_enabled(bytes([0, 0xD1, 0, 0, 0, 0]))
    assert not hidpp.analytics_enabled(bytes([0, 0xD1]))
