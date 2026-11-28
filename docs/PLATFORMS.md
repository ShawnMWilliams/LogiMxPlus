# macOS and Linux: notes for later

Status as of 2026-11-27: **Windows only.** The code is written to be cross-platform, but
it has not been built or run on macOS or Linux, and CI does not build those targets
yet. This file lists everything known to be needed. Work through it when there is a
machine to test on.

## What already exists in the code

- **HID transport.** `transport.rs` uses the `hidapi` crate, which supports all three
  OSes. Discovery groups collections by path. On macOS, hidapi reports one path per
  device (`DevSrvsID:…`), so the same path appears for both usages. `find_endpoints`
  de-duplicates it, and a unit test covers this case.
- **Bluetooth LE.** HID usage page `0xFF43` / usage `0x0202` is recognised as a
  directly connected device (device index `0xFF`), on every OS.
- **Paths** (`config.rs`):
  - macOS: `~/Library/Application Support/LogiMxPlus/`
  - Linux: `$XDG_CONFIG_HOME/logimxplus/` (defaults to `~/.config/logimxplus/`)
- **Start at login** (`platform.rs`):
  - macOS writes a LaunchAgent, `~/Library/LaunchAgents/io.github.shawnmwilliams.logimxplus.plist`,
    with `RunAtLoad` and `KeepAlive`, then runs `launchctl load -w`.
  - Linux writes an XDG autostart entry, `~/.config/autostart/logimxplus.desktop`.
- **Single instance.** Uses `File::try_lock` from std, which works on all OSes.
- **Timestamps.** UTC on non-Windows, marked `Z`. Windows uses local time.

## To do before macOS or Linux can be called supported

### Build

1. **Choose the hidapi backend in `Cargo.toml`.** Today only `windows-native` is enabled
   (with `default-features = false`).
   - **macOS:** the default C backend (IOKit) works. Consider the `macos-shared-device`
     feature so the CLI and the background process can open the device at the same time.
   - **Linux:** use `linux-native` (pure Rust hidraw, needs libudev) or the default
     `linux-static-hidraw` (C hidapi, needs `libudev-dev` at build time). Choose
     per-target with `[target.'cfg(...)'.dependencies]`.
2. **Add CI jobs.** Add `macos-latest` and `ubuntu-latest` to the CI matrix:
   `cargo build --release`, `cargo test`, `cargo clippy`. Upload the binaries as
   artifacts.
3. **Build universal macOS binaries.** Target `aarch64-apple-darwin` and
   `x86_64-apple-darwin` and combine them with `lipo`, or ship the two separately.

### macOS specifics

- **Input Monitoring permission.** macOS hides keyboard-class HID devices until the
  process is approved under System Settings → Privacy & Security → Input Monitoring. The
  receiver exposes keyboard collections, so the first run will likely prompt or silently
  see nothing. The README needs a step for this. Approving the `logimxplus` binary
  itself, rather than a Python interpreter, is one of the benefits of the Rust build.
- **Gatekeeper.** An unsigned binary downloaded from the internet is quarantined. Options:
  - Build locally with `cargo build --release`. Local builds are not quarantined.
  - Run `xattr -d com.apple.quarantine logimxplus`.
  - Sign and notarize it. That needs an Apple Developer ID ($99/yr).
- **Install location.** Probably `~/.local/bin/logimxplus` or `/usr/local/bin`.
  `install` registers whatever path the binary is running from, so install it first and
  then run `logimxplus install` from there.
- **Bluetooth peculiarity.** CleverSwitch reports that on macOS over Bluetooth, key
  reporting is bound to whichever app armed it last. If Logi Options+ is running, it may
  steal the arming. With the 0x1814 announcement (MX Keys S firmware 81.2.17+) arming
  matters less. Verify this.
- **Exit behaviour.** `KeepAlive` makes launchd restart the process if it exits. That
  includes the single-instance exit when a second copy is started by hand, which is
  fine.

### Linux specifics

- **Permissions.** The user needs read/write access to `/dev/hidraw*` for Logitech
  devices. Ship a udev rule, for example:

  ```
  # /etc/udev/rules.d/69-logimxplus.rules
  KERNEL=="hidraw*", ATTRS{idVendor}=="046d", TAG+="uaccess"
  ```

  Then run `sudo udevadm control --reload && sudo udevadm trigger`. Solaar's
  `42-logitech-unify-permissions.rules` also works.
- **Kernel driver.** `hid-logitech-dj` already handles Bolt and Unifying receivers and
  exposes the HID++ hidraw node. Check that our enumeration finds the receiver's
  `FF00` collections through hidraw. The kernel may present the receiver's HID++
  interface as a single hidraw node carrying both report IDs; `find_endpoints` copes
  with that, since a single path is fine.
- **Solaar coexistence.** If Solaar is running, it also talks HID++ to the receiver.
  Our software ID (`0xB`) is the same as Solaar's. Consider changing ours (for example
  to `0x9`) if both will run together.
- **Autostart.** The XDG `.desktop` entry works on GNOME and KDE. A systemd user unit
  (`~/.config/systemd/user/logimxplus.service`, `WantedBy=default.target`) is the more
  robust option for headless or tiling setups.
- **Wayland/X11.** Not relevant: LogiMxPlus never touches the display server.

### Testing checklist (per OS)

1. `logimxplus status` lists the receiver, keyboard and mouse with firmware versions.
2. `logimxplus -v run`: press Easy-Switch to another channel. Expect
   `change-host announcement` and then `mouse sent to channel N`, and the cursor
   appearing on the target computer.
3. Return: the log shows `keyboard ready … Easy-Switch reporting enabled` (the re-arm).
4. Unplug and re-plug the receiver. LogiMxPlus finds it again within ~5 s.
5. Sleep or hibernate, wake, then switch away from this computer. The mouse follows.
   (The log shows `woke from sleep: reopening devices`.) On Windows the wake comes from
   `PowerRegisterSuspendResumeNotification`. Elsewhere `platform::on_resume` is still a
   stub, and the runtime notices the clock jump within a minute instead. Wire it to
   `NSWorkspaceDidWakeNotification` (macOS) or logind's `PrepareForSleep` (Linux).
6. `logimxplus install`, then log out and back in. The process is running.
7. Measure idle CPU and memory, and compare with Windows (0 ms CPU per minute, under
   1 MB private).
