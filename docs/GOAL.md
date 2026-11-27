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
| G1 | Pressing Easy-Switch *N* while on a computer running LogiMxPlus moves the mouse to channel *N* (or the mapped channel) within 2 s, with no mouse interaction. | Manual: `logimxplus run -v`, press a key, see `mouse sent to channel N` in the log and the cursor appear on the target computer. |
| G2 | The keyboard still switches on its own; LogiMxPlus never blocks or remaps the Easy-Switch keys. | Same manual test, and `logimxplus status` shows `easy-switch reporting 3/3` with no diversion (analytics reporting only). |
| G3 | Works through a Logi Bolt USB receiver (the preferred, more secure link) and through direct Bluetooth. | Bolt: manual test on this PC. Bluetooth: `find_endpoints` test covers usage page `0xFF43`; manual test on a Bluetooth host. |
| G4 | Survives reconnects: after the keyboard leaves and comes back, sleep/wake, or receiver re-plug, reporting is re-armed without restarting. | `tests/test_runtime.py::test_keyboard_reconnect_rearms_reporting`; manual round trip 2 → 1 → 2. |
| G5 | If the mouse is asleep when the key is pressed, the switch is applied as soon as it wakes (within `pending_ttl`). | `tests/test_service.py::test_unreachable_mouse_gets_switch_when_it_connects`, `tests/test_runtime.py::test_mouse_waking_up_receives_pending_switch`. |
| G6 | Starts automatically at login with no console window, on Windows and macOS. | `logimxplus install`, then sign out/in and see the process + log file. |
| G7 | Code passes the quality gate. | `uv run ruff check . && uv run mypy && uv run pytest` exits 0. |

## Constraints

- Never persist changes on the devices: use only volatile HID++ settings (analytics
  reporting), so a computer without LogiMxPlus behaves exactly like stock.
- Never divert the Easy-Switch keys (they are not divertable on MX Keys S anyway).
- No network, no cloud, no Logi Options+ dependency.

## Suggested `/goal` line

```
/goal every criterion in docs/GOAL.md is met: `uv run ruff check . && uv run mypy && uv run pytest` exits 0 and its output is shown, and a manual Easy-Switch press on this PC logs "mouse sent to channel"; or stop after 25 turns.
```
