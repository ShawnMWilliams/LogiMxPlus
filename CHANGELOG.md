# Changelog

## 0.3.0: Rust rewrite (2026-11-27) — [release](https://github.com/ShawnMWilliams/LogiMxPlus/releases/tag/v0.3.0)

- Rewritten in Rust. One 0.48 MB native exe with no runtime: 18× less memory, 17×
  smaller, no measurable idle CPU, and one process instead of two. See
  [docs/PERFORMANCE.md](docs/PERFORMANCE.md).
- Blocking HID reads (no polling). Rescans only while nothing is connected, otherwise
  once a minute. Enumeration is limited to Logitech devices.
- Same commands, config file, log location and behaviour. Verified on real hardware.
- The Python implementation moved to `python/` as a reference.
- macOS and Linux are designed for but not yet built; see [docs/PLATFORMS.md](docs/PLATFORMS.md).

## 0.2.0: standalone Windows exe (Python + PyInstaller)

- `LogiMxPlus.exe` with Python bundled, so Python no longer has to be installed.
  Shows in Task Manager as "LogiMxPlus".
- `status` shows device firmware versions.

## 0.1.0: first working version (Python)

- Follows the MX Keys S 0x1814 change-host announcement (firmware 81.2.17+) and moves
  the MX Master 3S to the same channel.
- 0x1B04 analytics key events for keyboards that report presses instead.
- Re-arms on reconnect, holds the switch for a sleeping mouse, and starts at login.
