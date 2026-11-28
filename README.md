# LogiMxPlus

Press an **Easy-Switch** key on your Logitech **MX Keys S** and your **MX Master 3S**
mouse switches to the same computer. No more lifting the mouse to press the button
underneath it.

- **Tiny.** One 0.5 MB native program, written in Rust, and nothing else to install.
- **Idle cost.** Under 1 MB of private memory and no measurable CPU.

> **Rewritten from Python to Rust:** 18× less memory, 17× smaller, no measurable CPU,
> and one process instead of two. Numbers and reasons are in
> **[docs/PERFORMANCE.md](docs/PERFORMANCE.md)**.
- **Self-contained.** No Logitech software, no network, no permanent changes to the
  devices.
- **Tested on:** Windows 11 with a Logi Bolt USB receiver.
- **macOS and Linux:** designed for, but not yet built or tested; see
  [docs/PLATFORMS.md](docs/PLATFORMS.md).

## Tested hardware

Verified end to end on 2026-11-27 with both the Rust build and the earlier Python
prototype. With LogiMxPlus on two Windows PCs, Easy-Switch moved the mouse with the
keyboard **in both directions**, with no mouse interaction. It also started by itself
after a restart.

| Device | Model / ID | Firmware tested | Result |
|---|---|---|---|
| Keyboard | Logitech **MX Keys S** (Bolt wireless ID `B378`) | **`RBK81.02_B0017`** (Logitech's "81.2.17") | ✅ Works |
| Keyboard | Logitech MX Keys S | `RBK81.01_B0015` | ❌ Too old: sends no signal before switching |
| Mouse | Logitech **MX Master 3S** (Bolt wireless ID `B034`) | `RBM22.02_B0009` | ✅ Works, no update needed |
| Receiver | **Logi Bolt** USB receiver (`046D:C548`) | n/a | ✅ Both devices paired to it |
| Computer | Windows 11 (build 26300), x64 | n/a | ✅ |

**Not tested yet:**

- **Bluetooth.** Handled in code (HID usage page `FF43`).
- **macOS and Linux.** Code paths exist; see [docs/PLATFORMS.md](docs/PLATFORMS.md).
- **Other keyboards.** The original MX Keys reports key presses a different way, and
  LogiMxPlus handles that too, but it hasn't been tested on real hardware.
- **Other mice.** Any Logitech mouse with Easy-Switch should accept the switch command.

## Requirement: keyboard firmware 81.2.17 or newer

The MX Keys S only says where it is going on firmware **81.2.17 / `RBK81.02_B0017`** or
newer (April 2026, the "Enhanced Easy-Switch" update). Older firmware such as
`RBK81.01_B0015` switches silently, and nothing on the computer can tell which key was
pressed. **The mouse needs no firmware update.**

Check your versions without Logitech software:

```powershell
LogiMxPlus.exe status
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
  keyboard ──switches itself to channel 1
  LogiMxPlus on B ──CHANGE_HOST(channel 1)──▶ mouse ──▶ computer A   (~0.8 s after the press)
```

For keyboards that report key presses instead, such as the original MX Keys, LogiMxPlus
also arms HID++ 0x1B04 *analytics key events* on the Easy-Switch keys (CIDs
`0xD1`–`0xD3`). It handles either signal.

Notes:

- **Run it on every computer** you switch between. The press is reported to the computer
  the keyboard is *leaving*, which is the computer the mouse is on too.
- **The settings are volatile.** Nothing is written to the devices permanently, so on a
  computer without LogiMxPlus the keyboard and mouse behave exactly as stock.
  LogiMxPlus re-arms them after every reconnect, and after the computer wakes from
  sleep or hibernation (the receiver can lose power and forget them).
- **If the mouse is asleep** when you press the key, the switch is held for up to two
  minutes and applied the moment the mouse wakes up. Nudge the mouse and it follows.

## Install (Windows)

`LogiMxPlus.exe` needs only 64-bit Windows 10 or 11. It talks to the receiver through
Windows' built-in HID driver.

### Install without git (recommended)

You only need the exe, not the repository. Open a regular **PowerShell** window and run
these four commands. They also work for upgrading an existing install.

1. Stop any running copy and make the install folder:

   ```powershell
   Stop-Process -Name LogiMxPlus -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\Programs\LogiMxPlus" | Out-Null
   ```

2. Download the latest release:

   ```powershell
   Invoke-WebRequest https://github.com/ShawnMWilliams/LogiMxPlus/releases/latest/download/LogiMxPlus.exe -OutFile "$env:LOCALAPPDATA\Programs\LogiMxPlus\LogiMxPlus.exe"
   ```

3. Start it automatically at sign-in:

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\LogiMxPlus\LogiMxPlus.exe" -ArgumentList install -Wait
   ```

4. Start it now:

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\LogiMxPlus\LogiMxPlus.exe" -ArgumentList run
   ```

Notes:

- **You can close the PowerShell window afterwards.** LogiMxPlus keeps running in the
  background (fixed in 0.3.1).
- **Download it with PowerShell, as above.** A browser marks the file as "from the
  internet", and Windows SmartScreen then warns about the unsigned exe the first time:
  click **More info → Run anyway**, or run `Unblock-File` on it.

### Install with the script

If you've cloned the repository, `scripts\install-windows.ps1` does the same four steps.
It also removes older Python-based installs.

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

Without arguments it downloads the latest release. Pass `-Exe <path>` to install a
specific exe: one downloaded from
[Releases](https://github.com/ShawnMWilliams/LogiMxPlus/releases), a CI artifact, or your
own `cargo build --release` (`target\release\logimxplus.exe`).

The script does the following:

- Stops any running copy, including the older Python-based version.
- Copies the exe to `%LOCALAPPDATA%\Programs\LogiMxPlus\LogiMxPlus.exe`.
- Registers it to start at login (an `HKCU\...\Run` entry).
- Starts it right away.

**Is it running?** Task Manager lists one **LogiMxPlus** process. The log's last line
should say `keyboard ready`.

> **Installing from the Claude desktop app's terminal?** Windows quietly redirects that
> app's AppData writes into its private container. The install would land there, and the
> login entry would point at it. Run the script from a normal PowerShell window, or through
> a one-off Scheduled Task, instead.

**Uninstall:**

```powershell
& "$env:LOCALAPPDATA\Programs\LogiMxPlus\LogiMxPlus.exe" uninstall
Stop-Process -Name LogiMxPlus
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\LogiMxPlus"
```

### Commands

| Command | What it does |
|---|---|
| `LogiMxPlus.exe status` | List devices, their firmware, their current channel, and whether Easy-Switch reporting is armed |
| `LogiMxPlus.exe -v run` | Run in the foreground with trace logging of every HID++ report. Plain `run` is what login starts. |
| `LogiMxPlus.exe switch N` | Switch the mouse and keyboard to channel N now (handy for a hotkey) |
| `LogiMxPlus.exe config` | Create the config file and print its path and the login entry |
| `LogiMxPlus.exe install` / `uninstall` | Add or remove the login entry |

The log is at `%APPDATA%\LogiMxPlus\logimxplus.log`. It is capped at 1 MB: when it
passes that, it becomes `logimxplus.log.1` (replacing the previous one) and a new log
starts. So the logs never use more than about 2 MB, which is weeks of normal use.

## Configuration

If you paired each computer to the **same channel number** on both devices, you need no
configuration. Otherwise run `LogiMxPlus.exe config` and edit `[channels]`:

```toml
[channels]
# keyboard key = mouse channel for the same computer
1 = 1
2 = 3
3 = 2
```

## Efficiency

Measured on the test machine, idle, over 60 seconds:

| | Python prototype | **Rust** |
|---|---|---|
| Processes | 2 | **1** |
| CPU | ~94 ms per minute (0.16% of a core) | **0 ms** |
| Private memory | 16.3 MB | **0.89 MB** |
| Exe size | 8.5 MB | **0.48 MB** |

The full comparison, how it was measured, and which gains came from the language versus
the redesign are in [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

Why it's this cheap:

- **No polling.** The HID reader threads block in the kernel until the receiver sends a
  report, and the main loop sleeps on a channel.
- **Rare rescans.** It looks for new devices only when nothing is open (every 5 s), and
  otherwise once a minute.
- **Narrow enumeration.** Enumeration is limited to Logitech devices.

## Known limitations

- **A computer at its sign-in screen can't send the mouse away.** LogiMxPlus starts when
  you sign in. Switching *to* a computer that's at its sign-in screen works, because the
  computer you're leaving handles it. Switching *away* from a computer that nobody has
  signed in to yet doesn't move the mouse. Fixing this would mean running LogiMxPlus as a
  Windows service (see Ideas below).
- **The exe isn't code-signed.** SmartScreen warns once if you download it with a browser.
  The PowerShell install avoids this.
- **Not yet tested:** Bluetooth, macOS, Linux, and keyboards or mice other than the MX
  Keys S and MX Master 3S.
- **Logi Options+ may compete.** Options+'s own Enhanced Easy-Switch may move the mouse
  too. Turn that feature off, or quit Options+.

## Ideas for later

- **Windows service mode.** It would work at the sign-in screen too. It needs an
  administrator install, and HID access from session 0 needs checking.
- **macOS and Linux builds.** The checklist is in [docs/PLATFORMS.md](docs/PLATFORMS.md).
- **Code signing.** It would remove the SmartScreen prompt; a certificate costs money.
- **A Bluetooth test.** The code path exists (usage page `FF43`) but hasn't been run.

## Logi Options+

LogiMxPlus doesn't need Options+.

- **Running both:** Options+ may arm the same keys. If switching is unreliable while
  Options+ runs, quit Options+.
- **Logitech's own feature:** recent Options+ versions have an official
  [Enhanced Easy-Switch](https://support.logi.com/hc/articles/38848802088855-Enhanced-Easy-Switch-Feature-Guide-Support)
  feature that does something similar. It requires Options+ on every computer.

## Development

```powershell
cargo fmt --check ; cargo clippy --all-targets -- -D warnings ; cargo test
cargo build --release
```

Layout (`src/`):

| Module | Layer | Role |
|---|---|---|
| `domain.rs` | Domain | Types, Easy-Switch CIDs, errors |
| `hidpp.rs` | Domain | Pure HID++ encode/decode (no I/O) |
| `transport.rs` | Gateway | hidapi transport: blocking readers, request/reply matching, discovery |
| `device.rs` | Gateway | One HID++ 2.0 device: features, firmware, host info, change host, reporting |
| `service.rs` | Service | When and where the mouse switches (debounce, pending, mapping) |
| `runtime.rs` | Composition root | Event loop; finds devices, re-arms on reconnect, routes events |
| `cli.rs`, `config.rs`, `logging.rs`, `platform.rs` | Edges | CLI, TOML config, file log, OS integration |

Other docs:

- [docs/GOAL.md](docs/GOAL.md): the goal and its success criteria.
- [docs/PROTOCOL.md](docs/PROTOCOL.md): protocol notes.
- [docs/PLATFORMS.md](docs/PLATFORMS.md): what macOS and Linux still need.
- `python/`: the original Python prototype, kept for reference.

Prior art worth reading: [CleverSwitch](https://github.com/MikalaiBarysevich/CleverSwitch)
(same idea), [Solaar](https://github.com/pwr-Solaar/Solaar), and
[logiops](https://github.com/PixlOne/logiops).

## License

MIT
