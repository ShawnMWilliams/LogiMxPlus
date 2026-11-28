# Changelog

## 0.3.2: works after sleep and hibernation (2026-11-28) — [release](https://github.com/ShawnMWilliams/LogiMxPlus/releases/tag/v0.3.2)

- **Fix:** after a computer woke from hibernation, it stopped sending the mouse away when
  you pressed Easy-Switch. The Bolt receiver had lost power and forgotten the setting
  that makes it report devices connecting. So when the keyboard and mouse came back,
  LogiMxPlus never heard about them and ignored the keyboard's switch announcement.
  Restarting LogiMxPlus fixed it, because start-up turns that setting back on.
- **Now:** when Windows reports a wake from sleep or hibernation, LogiMxPlus waits 2 s,
  then reopens the receiver, turns notifications back on and finds the devices again.
  If the wake notification never arrives, a jump in the clock triggers the same thing
  within a minute. No polling was added.
- **Cost, measured after 3 minutes idle:** still 0 ms CPU, 3 threads and about 0.40 MB in
  Task Manager. Committed memory went from 0.89 MB to 0.96 MB, most likely for the
  Windows power-notification library.

## 0.3.1: stays running when its terminal closes (2026-11-27) — [release](https://github.com/ShawnMWilliams/LogiMxPlus/releases/tag/v0.3.1)

- **Fix:** `LogiMxPlus.exe run` started from a PowerShell window no longer attaches to
  that window's console. Before, closing the window ended LogiMxPlus, because Windows
  stops every process attached to a console when it closes. Starting at sign-in was not
  affected. Only printing commands (`status`, `config`, …) and `-v run` attach now.
- **Lower start-up memory:** start-up memory is handed back once start-up finishes, so
  Task Manager shows about 0.12 MB right after start instead of about 0.65 MB. In normal
  use it settles at about 0.36–0.4 MB, the pages it really uses (see
  [PERFORMANCE.md](docs/PERFORMANCE.md)).
- **Docs:** the README has an install without git.

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
