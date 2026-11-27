# LogiMxPlus

Press an **Easy-Switch** key on your Logitech **MX Keys S** and your **MX Master 3S**
mouse switches to the same computer. No more lifting the mouse to press the button
underneath it.

No Logitech software, no network, nothing extra running on the devices. It works over a
**Logi Bolt** USB receiver. Bluetooth, macOS and Linux are supported in code but not yet
tested (see below).

## Tested hardware

Verified end to end on 2026-11-27. Pressing Easy-Switch 1 on the keyboard moved the
mouse to channel 1, with no mouse interaction.

| Device | Model / ID | Firmware tested | Result |
|---|---|---|---|
| Keyboard | Logitech **MX Keys S** (Bolt wireless ID `B378`) | **`RBK81.02_B0017`** (Logitech's "81.2.17") | ✅ Works |
| Keyboard | Logitech MX Keys S | `RBK81.01_B0015` | ❌ Too old: sends no signal before switching |
| Mouse | Logitech **MX Master 3S** (Bolt wireless ID `B034`) | `RBM22.02_B0009` | ✅ Works, no update needed |
| Receiver | **Logi Bolt** USB receiver (`046D:C548`) | — | ✅ Both devices paired to it |
| Computer | Windows 11 (build 26300), Python 3.12 via uv | — | ✅ |

**Not tested yet:**

- **Bluetooth.** Handled in code: HID usage page `FF43`.
- **macOS and Linux.** Handled in code.
- **Other keyboards.** The original MX Keys reports key presses differently; LogiMxPlus
  handles that path too, but no one has tested it on real hardware.
- **Other mice.** Any mouse with Easy-Switch should accept the switch command.

## Requirement: keyboard firmware 81.2.17 or newer

The MX Keys S only says where it is going on firmware **81.2.17 / `RBK81.02_B0017`** or
newer (April 2026, the "Enhanced Easy-Switch" update). Older firmware such as
`RBK81.01_B0015` switches silently, and nothing on the computer can tell which key was
pressed. **The mouse needs no firmware update.**

Check your versions without Logitech software:

```powershell
logimxplus status
```

It prints something like:

```
USB Receiver (c548)  receiver
  [3] MX Master 3S (mouse)  firmware RBM22.02_B0009  host channel 2 of 3
  [4] MX Keys S (keyboard)  firmware RBK81.02_B0017  host channel 2 of 3  easy-switch reporting 3/3 keys
```

Logitech publishes this firmware only through Logi Options+, and their older standalone
updater is discontinued. If you don't want Options+ on your computer, install it on a
spare machine:

1. Plug the Bolt receiver into the spare machine.
2. Update the keyboard there.
3. Uninstall Options+ and bring the receiver back. The firmware stays on the keyboard.

If you keep [Logi Options+](https://www.logitech.com/software/logi-options-plus.html)
installed, leave its own *Enhanced Easy-Switch* feature **off**. Two programs moving the
mouse at once fight each other.

## How it works

The MX Keys S Easy-Switch keys can't be intercepted: the keyboard always switches itself.
On current firmware, the keyboard sends a HID++ `CHANGE_HOST` (0x1814) notification
just before it leaves, to the computer it is leaving. The notification says
`[this host, target host]`. LogiMxPlus then sends the mouse `CHANGE_HOST` for the same
channel.

```
press Easy-Switch 1 on computer B
  keyboard ──(0x1814 notice: leaving 2 for 1)──▶ LogiMxPlus on B
  keyboard ──switches itself to channel 1 (~1 s later)
  LogiMxPlus on B ──CHANGE_HOST(channel 1)──▶ mouse ──▶ computer A
```

For keyboards that report key presses instead, such as the original MX Keys, LogiMxPlus
also arms HID++ 0x1B04 *analytics key events* on the Easy-Switch keys (CIDs
`0xD1`–`0xD3`). It handles either signal.

Notes:

- **Run it on every computer** you switch between. The press is reported to the computer
  the keyboard is *leaving*, which is the computer the mouse is on too.
- **The settings are volatile.** Nothing is written to the devices permanently, so on a
  computer without LogiMxPlus the keyboard and mouse behave exactly as stock.
  LogiMxPlus re-arms them after every reconnect.
- **If the mouse is asleep** when you press the key, the switch is held for up to two
  minutes and applied the moment the mouse wakes up. Nudge the mouse and it follows.

## Install

You need [uv](https://docs.astral.sh/uv/).

```powershell
uv tool install git+https://github.com/ShawnMWilliams/LogiMxPlus
logimxplus status      # shows the keyboard and mouse it can see
logimxplus install     # start automatically at login (no console window)
```

On Windows, `scripts\install-windows.ps1` does all of that in one step and starts the
program right away. It takes `-Source <path>` to install from a local checkout.

**Is it running?** In Task Manager it appears as **`logimxplusw.exe`**, with Python
worker processes under it. The log's last lines should include `keyboard ready`.

> **Installing from the Claude desktop app's terminal?** Windows quietly redirects that
> app's AppData writes into its private container. The install would land there, and
> the login entry would point at it. Run the script from a normal PowerShell window,
> or through a one-off Scheduled Task, instead.

To run it in the foreground with debug logging:

```powershell
logimxplus -v run
```

Other commands:

| Command | What it does |
|---|---|
| `logimxplus status` | List devices, their current channel, and whether Easy-Switch reporting is armed |
| `logimxplus switch N` | Switch mouse and keyboard to channel N now (handy for a hotkey) |
| `logimxplus config` | Create the config file and print its path |
| `logimxplus uninstall` | Remove the login item |

Logs: `%APPDATA%\LogiMxPlus\logimxplus.log` (Windows),
`~/Library/Application Support/LogiMxPlus/` (macOS), `~/.config/logimxplus/` (Linux).

### macOS

Grant **Input Monitoring** to the Python that runs LogiMxPlus (System Settings → Privacy &
Security → Input Monitoring). Otherwise macOS hides the HID++ interface.

### Linux

Your user needs read/write access to `/dev/hidraw*` for Logitech devices. The udev rules
shipped with Solaar work.

## Configuration

If you paired each computer to the **same channel number** on both devices, you need no
configuration. Otherwise run `logimxplus config` and edit `[channels]`:

```toml
[channels]
# keyboard key = mouse channel for the same computer
1 = 1
2 = 3
3 = 2
```

## Logi Options+

LogiMxPlus doesn't need Options+.

- **Running both:** Options+ may arm the same keys. If switching is unreliable while
  Options+ runs, quit Options+.
- **Logitech's own feature:** recent Options+ versions have an official
  [Enhanced Easy-Switch](https://support.logi.com/hc/articles/38848802088855-Enhanced-Easy-Switch-Feature-Guide-Support)
  feature that does something similar. It requires Options+ on every computer.

## Development

```powershell
uv sync
uv run ruff check . ; uv run mypy ; uv run pytest
```

Layout (`src/logimxplus`):

| Module | Layer | Role |
|---|---|---|
| `domain.py` | Domain | Types, Easy-Switch CIDs, errors |
| `hidpp.py` | Domain | Pure HID++ encode/decode (no I/O) |
| `transport.py` | Gateway | hidapi transport: request/reply matching, notifications, discovery |
| `devices.py` | Gateway | One HID++ 2.0 device: features, host info, change host, reporting |
| `service.py` | Service | When and where the mouse switches (debounce, pending, mapping) |
| `runtime.py` | Composition root | Finds devices, re-arms on reconnect, routes events to the service |
| `cli.py`, `autostart.py` | Entry | CLI and login-item install |

Goal and success criteria: [docs/GOAL.md](docs/GOAL.md). Protocol notes:
[docs/PROTOCOL.md](docs/PROTOCOL.md).

Prior art worth reading: [CleverSwitch](https://github.com/MikalaiBarysevich/CleverSwitch)
(same idea), [Solaar](https://github.com/pwr-Solaar/Solaar), [logiops](https://github.com/PixlOne/logiops).

## License

MIT
