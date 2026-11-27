# Goal

**Pressing an Easy-Switch key (1, 2 or 3) on the Logitech MX Keys S switches both the
keyboard *and* the MX Master 3S mouse to that computer — one action, no touching the
mouse.**

Today switching takes two actions: press the keyboard key, then lift the mouse and press
the button underneath it. This project removes the second action.

This file follows the Claude Code [`/goal`](https://code.claude.com/docs/en/goal.md)
method: one measurable end state, each criterion with a check that produces evidence,
and explicit constraints. Work is done when every criterion below passes.

## Success criteria

| # | Criterion | Check |
|---|-----------|-------|
| G1 | Pressing Easy-Switch *N* while on a computer running LogiMxPlus moves the mouse to channel *N* (or the mapped channel) within 2 s, with no mouse interaction. | Manual: `LogiMxPlus.exe -v run`, press a key, see `mouse sent to channel N` in the log and the cursor appear on the target computer. |
| G2 | The keyboard still switches on its own; LogiMxPlus never blocks or remaps the Easy-Switch keys. | Same manual test, and `LogiMxPlus.exe status` shows `easy-switch reporting 3/3` with no diversion (analytics reporting only). |
| G3 | Works through a Logi Bolt USB receiver (the preferred, more secure link) and through direct Bluetooth. | Bolt: manual test on this PC. Bluetooth: `find_endpoints` test covers usage page `0xFF43`; manual test on a Bluetooth host. |
| G4 | Survives reconnects: after the keyboard leaves and comes back, sleep/wake, or receiver re-plug, reporting is re-armed without restarting. | `runtime::tests::keyboard_reconnect_rearms_reporting`; manual round trip 2 → 1 → 2. |
| G5 | If the mouse is asleep when the key is pressed, the switch is applied as soon as it wakes (within `pending_ttl`). | `service::tests::unreachable_mouse_gets_switch_when_it_connects`, `runtime::tests::mouse_waking_up_receives_pending_switch`. |
| G6 | Starts automatically at login with no console window (Windows now; macOS and Linux later, see [PLATFORMS.md](PLATFORMS.md)). | `install`, then sign out/in and see the process + log file. |
| G7 | Code passes the quality gate. | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` exits 0. |
| G8 | Idle cost is negligible: one process, under 2 MB private memory, under 10 ms CPU per minute. | `Get-Process LogiMxPlus` over 60 s (see [PERFORMANCE.md](PERFORMANCE.md)). |

## Status (2026-11-27)

| # | Status | Evidence |
|---|--------|----------|
| G1 | ✅ Met on this PC (Bolt), with both the Python prototype and the Rust build | Rust, 15:37:29.577: `Easy-Switch 1 pressed` → `mouse sent to channel 1`, and the mouse left 0.78 s after the press. The user confirmed the cursor appeared on the channel-1 computer. |
| G2 | ✅ | The keyboard switched itself in the same test. Nothing was diverted. |
| G3 | ◐ | Bolt verified. Bluetooth covered by unit test only. |
| G4 | ✅ | Unit test, plus a live re-arm on keyboard return seen in the log. |
| G5 | ✅ | Unit tests. |
| G6 | ◐ | This PC: the Rust `LogiMxPlus.exe` is installed with `scripts/install-windows.ps1`, run outside the app container, and starts at login through an HKCU Run entry. Not yet observed across a real sign-out/sign-in. Channel-1 PC: not installed. macOS and Linux: not started. |
| G7 | ✅ | The cargo gate passes (35 tests). The Python reference in `python/` still passes its own gate (39 tests). |
| G8 | ✅ | 1 process, 0.89 MB private memory, 0 ms CPU in 60 s. |

## Prerequisite

MX Keys S firmware **81.2.17+**. Older firmware sends no signal before switching (see
[PROTOCOL.md](PROTOCOL.md)), so G1 cannot be met on it by any software running only
on the computer being left.

## Constraints

- Never persist changes on the devices: use only volatile HID++ settings (analytics
  reporting), so a computer without LogiMxPlus behaves exactly like stock.
- Never divert the Easy-Switch keys (they are not divertable on MX Keys S anyway).
- No network, no cloud, no Logi Options+ dependency.

## Suggested `/goal` line

```
/goal every criterion in docs/GOAL.md is met: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` exits 0 and its output is shown, and a manual Easy-Switch press on this PC logs "mouse sent to channel"; or stop after 25 turns.
```
