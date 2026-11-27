# Python → Rust: the performance gain

LogiMxPlus runs all day in the background. It does one thing: wait for the keyboard to
announce an Easy-Switch, then tell the mouse to follow. Almost all of its life is spent
idle, so idle cost is the number that matters.

The first working version was a Python prototype, shipped as a PyInstaller `.exe`. It
was rewritten in Rust the same day. Both versions passed the same real-hardware test on
the same machine (Easy-Switch 1 on an MX Keys S moved an MX Master 3S to channel 1).

## Results

| Idle, same machine | Python prototype (v0.2.0) | **Rust (v0.3.0)** | Change |
|---|---|---|---|
| Processes | 2 | **1** | −50% |
| CPU time | 94 ms per minute (0.16% of a core) | **0 ms per minute** | effectively −100% |
| Private memory | 16.3 MB | **0.89 MB** | **18× less** |
| Working set (incl. shared DLLs) | 30.7 MB | 5.2 MB | 6× less |
| Threads | 8 | 3 | −63% |
| Executable size | 8.5 MB | **0.48 MB** | **17× smaller** |
| Task Manager "Memory" in normal use (private working set) | 13.4 MB (12.5 + 0.9) | **~0.36–0.4 MB** | **~33× less** |
| Unpacks to `%TEMP%` on every start | yes (Python runtime) | **no** | — |
| Needs anything installed | no (Python bundled) | no | — |
| Press → mouse leaves | ~0.8 s | ~0.8 s | same (radio-bound) |

The versions measured:

- **Python:** commit [`73935f2`](https://github.com/ShawnMWilliams/LogiMxPlus/commit/73935f2),
  source in [`python/`](../python).
- **Rust:** the current code at the repo root.

### How it was measured

- **Machine:** Windows 11 (build 26300), x64. Logi Bolt receiver with the MX Keys S
  (firmware `RBK81.02_B0017`) and MX Master 3S (`RBM22.02_B0009`) connected.
- **Method:** the program was installed and running normally, started the same way login
  starts it, and both devices were connected and idle. The numbers come from
  `Get-Process LogiMxPlus`: the `TotalProcessorTime` delta over a 30 s (Python) or 60 s
  (Rust) window, plus `PrivateMemorySize64`, `WorkingSet64` and the thread count.
  Python figures are the sum of its two processes.
- **Task Manager agrees:** it showed the Python version as two LogiMxPlus rows (12.5 MB +
  0.9 MB) and the Rust version as a single row of about 0.4 MB, on both of the user's
  machines.
- **Which memory number to trust:**
  - **Private memory** (committed) is the program's real footprint: what Windows has
    reserved for it. It's the most stable figure, so the headline "18× less" uses it.
  - **Task Manager's "Memory"** (private working set) is the part currently held in RAM.
    It moves: Windows trims it when the program is idle, and it grows back when the
    program does work. The Rust figure above (~0.36 MB measured, ~0.4 MB seen in Task
    Manager) was taken after 14 minutes and 77 device events (switches, reconnects,
    re-arming the keyboard), which is normal use.

## Why it improved

Some of the gain comes from the language and some from a better design. Both are listed
here, so the comparison is honest.

### From the language (Rust vs. Python)

1. **No interpreter to carry.**
   - **Python:** the exe has to ship and boot a whole Python runtime (~8 MB) plus the
     HID library.
   - **Rust:** compiles to one native binary containing only the code that's used.
     Release builds use `opt-level = "s"`, LTO, `panic = "abort"` and stripped symbols.
2. **One process instead of two.** A one-file PyInstaller exe works in two stages. A
   small launcher unpacks Python into a temp folder, starts the real program as a child
   process, and then waits for it. A Rust exe *is* the program.
3. **Memory.**
   - **Python:** every Python process carries the interpreter, its object heap, imported
     modules and the garbage collector, about 15 MB before doing any work.
   - **Rust:** has no runtime or garbage collector. Its whole working state (a few
     buffers, a hash map of feature indexes, three threads' stacks) fits in under 1 MB
     of private memory.
4. **No startup unpacking.** The Python exe extracted its runtime to `%TEMP%\_MEI…` on
   every launch. The Rust exe just runs.
5. **Hands its start-up memory back (v0.3.1).** Finding and setting up devices touches
   some memory once. After start-up the program calls `SetProcessWorkingSetSize(-1, -1)`.
   - **Effect:** Task Manager shows about 0.12 MB right after start instead of about
     0.65 MB.
   - **Not the steady-state number:** once the program starts handling switches and
     reconnects, the pages it really needs come back, and it settles at about 0.36–0.4 MB.
     That's the honest everyday figure, and it's the one in the table.
   - **No further trimming:** trimming repeatedly would only make Task Manager look
     smaller while making every switch fetch those pages again.
   - **Committed memory is unchanged** at about 0.9 MB.

### From the redesign (done during the port, and possible in either language)

1. **Blocking reads instead of polling.**
   - **Python:** each of its two HID reader threads (one per HID collection) woke every
     200 ms to check for data: 10 wake-ups a second, forever.
   - **Rust:** the reader threads block inside the Windows kernel (overlapped `ReadFile`
     with an infinite wait) and use zero CPU until the receiver actually sends a report.
     The main loop also sleeps on a channel.
2. **Rescanning only when needed.**
   - **Python:** re-enumerated HID devices every 5 seconds.
   - **Rust:** rescans every 5 s only while *nothing* is connected, and otherwise once a
     minute. A device going away is detected directly by the failing read, not by
     polling.
3. **Narrower enumeration.** The Rust build enumerates only Logitech devices
   (vendor `046D`) instead of every HID device on the system.

The first point is where the Python version's 0.16% CPU came from. A redesigned Python
version could drop most of that CPU. It would still carry two processes, about 15 MB of
memory and an 8.5 MB exe; only the language change removes those.

## What didn't change

- **Switch speed.** About 0.8 s from key press to the mouse leaving, in both versions.
  The command goes out as soon as the keyboard announces the switch. The rest is the
  radio, as the keyboard's own switch clears the receiver. Software can't speed that
  up.
- **Behaviour.** Same commands, config file, log location and protocol handling. The
  unit tests were ported one for one (35 in Rust; the Python suite of 39 stays in
  `python/`).

## Reproduce it

```powershell
$p = Get-Process LogiMxPlus; $c1 = ($p | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum
Start-Sleep 60
$p = Get-Process LogiMxPlus; $c2 = ($p | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum
"CPU ms/min: $($c2 - $c1)"
$p | Select-Object Id, @{n='Private MB';e={[math]::Round($_.PrivateMemorySize64/1MB,2)}}, @{n='Threads';e={$_.Threads.Count}}
```
