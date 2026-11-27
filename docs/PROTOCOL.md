# HID++ notes

These are the facts LogiMxPlus relies on. Each was checked on real hardware: an MX Keys S
and an MX Master 3S on a Logi Bolt receiver (USB `046D:C548`), Windows 11, November 2026.
Byte layouts were cross-checked against Solaar, logiops and CleverSwitch.

## Transport

| Item | Value |
|---|---|
| Short report | `10 DI SUB ADDR p0 p1 p2` (7 bytes) |
| Long report | `11 DI FI (FN<<4 \| SW) p0..p15` (20 bytes) |
| Device index (receiver) | 1–6 per paired device; `FF` = the receiver itself |
| Device index (Bluetooth) | `FF` |
| Software id | We use `0xB`. Notifications carry SW 0. |
| Windows, Bolt receiver | Two collections on `MI_02`: `FF00:0001` short, `FF00:0002` long. Open both; write long; read both. |
| Bluetooth LE | Usage page `FF43`, usage `0202`, long reports only |
| Receiver notifications | Enable with register `0x00` = `00 09 00` (wireless + software present). Connection event `10 DI 41 proto FLAGS wpid` — FLAGS bit `0x40` = link down. |
| HID++ 1.0 errors | `10 DI 8F ...` code `04` (paired but not connected), `09` (empty slot) |
| HID++ 2.0 errors | `11 DI FF FI FNSW code` |

## Features used

| Feature | Index on MX Keys S / MX Master 3S | Use |
|---|---|---|
| `0x0005` DEVICE_NAME | 1 / 1 | fn0 length, fn1 name chunk, fn2 type (0 keyboard, 3 mouse) |
| `0x1814` CHANGE_HOST | 10 / 10 | fn0 → `[nbHost, currHost(0-based), flags]`; fn1 `[host]` switches. **No reply**: the link drops. |
| `0x1815` HOSTS_INFO | 11 / 11 | Host names per slot. Not unique (two slots both say "Logitech Bolt receiver"), so not used for mapping. |
| `0x1B04` REPROG_CONTROLS_V4 | 9 / 9 | See below |
| `0x1D4B` WIRELESS_STATUS | 4 / 4 | event 0 on reconnect → re-arm |

Indices are always looked up at runtime through IRoot (`0x0000` fn0).

## Easy-Switch keys on the MX Keys S

`getCidInfo` on this unit:

```
cid=0x00D1 tid=0x00AE flags=0x04 flags2=0x04   Host Switch Channel 1
cid=0x00D2 tid=0x00AF flags=0x04 flags2=0x04   Host Switch Channel 2
cid=0x00D3 tid=0x00B0 flags=0x04 flags2=0x04   Host Switch Channel 3
```

- **Not divertable.** `flags=0x04` means nonstandard only. The divertable bit `0x20` is not
  set, unlike the original MX Keys, so the Solaar-style "divert and re-send CHANGE_HOST"
  approach does not work.
- **Analytics key events are supported.** `flags2` bit `0x04` is set.
- **How LogiMxPlus arms them:** `setCidReporting` with `cid, 00, 0000, 03` (analytics on +
  valid). `getCidReporting` then returns byte 5 = `01`.
- **The event:** event 2 of the 1B04 feature, payload `CID(2) event(1)` records.
- **The setting is volatile.** It is re-applied on every reconnect.

## What the keyboard actually reports (tested 2026-11-27)

**Firmware `RBK81.01_B0015` (0x1814 v1, 0x1B04 v5): nothing.**

- Analytics was armed on every CID that supports it, and every echo came back `03`.
- Pressing Easy-Switch 2 (the current host), F1 and F2 produced **no** notification of
  any kind.
- Pressing Easy-Switch 1 produced only the receiver's link-down event:
  `10 04 41 10 41 78 b3` (wpid `B378`).

This matches CleverSwitch issue
[#113](https://github.com/MikalaiBarysevich/CleverSwitch/issues/113).

**Firmware 81.2.17+ (Enhanced Easy-Switch): an announcement before leaving.**

- On an Easy-Switch press the keyboard sends `11 DI FI(0x1814) 00 cur tgt ...` to the host
  it is leaving, just before the link drops: about 30 ms on our hardware (see below;
  CleverSwitch reports up to about 1 s). `cur` and `tgt` are zero-based.
- No enabling step is needed. Analytics events still never arrive.
- Sources: CleverSwitch issues
  [#102](https://github.com/MikalaiBarysevich/CleverSwitch/issues/102) and #113, and
  [Logitech release notes](https://support.logi.com/hc/en-us/articles/39066494614423-MX-Keys-S-Firmware-Release-Notes).

LogiMxPlus handles both the 0x1814 announcement and 0x1B04 analytics events.

### Confirmed on hardware with firmware `RBK81.02_B0017` (2026-11-27)

- **The keyboard's feature version changed.** On the old firmware the keyboard reports
  0x1814 as version **1**. On the new firmware it reports version **2**. The mouse
  (`RBM22.02_B0009`) stays at version 1 and needs nothing new.
- **The announcement arrives about 30 ms before the link drops.**
  - Pressing Easy-Switch 1 while on host 2 produced the raw report
    `11 04 0a 00 01 00 …`: device 4 (the keyboard), feature index 10 (0x1814), function
    0, `[current 1, target 0]`.
  - The receiver's link-down report for the keyboard, `10 04 41 10 41 78 b3`, followed
    about 30 ms later.
- **The mouse may or may not acknowledge.** In one run it answered `setCurrentHost`
  (`11 03 0a 1b …`) about 770 ms after the press, when the radio was free again. In
  another it dropped the link without replying. The code treats a timeout as success.
- **The mouse leaves 0.66–0.8 s after the press.** The rest is radio time, as the
  keyboard's own switch clears the receiver.
- **0x1B04 analytics events never arrive on the MX Keys S, on either firmware.** The
  feature accepts the setting and still sends nothing.
