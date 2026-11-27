# LogiMxPlus

Press an **Easy-Switch** key on your Logitech **MX Keys S** and your **MX Master 3S**
mouse switches to the same computer. No more lifting the mouse to press the button
underneath it.

Works over a **Logi Bolt** USB receiver and over Bluetooth, on Windows, macOS and Linux.

## How it works

The MX Keys S Easy-Switch keys can't be intercepted ("diverted"): the keyboard always
switches itself. They do support HID++ *analytics key events*, though. LogiMxPlus turns
those on, so the keyboard also **tells** the computer which Easy-Switch key was pressed,
just before it leaves. LogiMxPlus then sends the mouse the HID++ `CHANGE_HOST` (0x1814)
command for the same channel.

```
press Easy-Switch 1 on computer B
  keyboard ──(analytics event: key 0xD1)──▶ LogiMxPlus on B
  keyboard ──switches itself to channel 1
  LogiMxPlus on B ──CHANGE_HOST(channel 1)──▶ mouse ──▶ computer A
```

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
