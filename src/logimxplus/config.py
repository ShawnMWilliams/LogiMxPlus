"""User configuration (TOML) and per-OS file locations."""

from __future__ import annotations

import os
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_CONFIG = """\
# LogiMxPlus configuration.

# Optional: only use devices whose name contains these strings.
# keyboard_name = "MX Keys"
# mouse_name = "MX Master"

# Seconds a switch stays pending when the mouse is asleep and cannot be reached.
pending_ttl = 120

# Maps each keyboard Easy-Switch key (1-3) to the mouse's Easy-Switch channel (1-3)
# for the same computer. Leave it as-is if you paired both devices to each computer
# on the same channel number. (Keep this table last: TOML keys below a [table]
# header belong to that table.)
[channels]
1 = 1
2 = 2
3 = 3
"""


@dataclass(frozen=True)
class Config:
    channel_map: dict[int, int] = field(default_factory=dict)  # zero-based kb slot -> mouse slot
    keyboard_name: str | None = None
    mouse_name: str | None = None
    pending_ttl: float = 120.0


def config_dir() -> Path:
    if sys.platform == "win32":
        base = Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming"))
        return base / "LogiMxPlus"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "LogiMxPlus"
    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return base / "logimxplus"


def config_path() -> Path:
    return config_dir() / "config.toml"


def parse_config(text: str) -> Config:
    data = tomllib.loads(text)
    channels: dict[int, int] = {}
    for key, value in dict(data.get("channels", {})).items():
        kb, mouse = int(key), int(value)
        if not (1 <= kb <= 3 and 1 <= mouse <= 3):
            raise ValueError(f"channel mapping {kb} = {mouse} must use values 1-3")
        channels[kb - 1] = mouse - 1
    return Config(
        channel_map=channels,
        keyboard_name=data.get("keyboard_name") or None,
        mouse_name=data.get("mouse_name") or None,
        pending_ttl=float(data.get("pending_ttl", 120)),
    )


def load_config(path: Path | None = None) -> Config:
    path = path or config_path()
    if not path.exists():
        return Config()
    return parse_config(path.read_text(encoding="utf-8"))


def write_default_config(path: Path | None = None) -> Path:
    path = path or config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_text(DEFAULT_CONFIG, encoding="utf-8")
    return path
