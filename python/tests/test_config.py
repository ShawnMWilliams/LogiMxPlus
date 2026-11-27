from pathlib import Path

import pytest

from logimxplus.config import DEFAULT_CONFIG, load_config, parse_config


def test_default_config_is_identity() -> None:
    cfg = parse_config(DEFAULT_CONFIG)
    assert cfg.channel_map == {0: 0, 1: 1, 2: 2}
    assert cfg.keyboard_name is None and cfg.pending_ttl == 120


def test_custom_mapping_and_names() -> None:
    cfg = parse_config('keyboard_name = "MX Keys"\n[channels]\n1 = 3\n')
    assert cfg.channel_map == {0: 2}
    assert cfg.keyboard_name == "MX Keys"


def test_invalid_channel_rejected() -> None:
    with pytest.raises(ValueError):
        parse_config("[channels]\n1 = 4\n")


def test_missing_file_gives_defaults(tmp_path: Path) -> None:
    assert load_config(tmp_path / "nope.toml").channel_map == {}
