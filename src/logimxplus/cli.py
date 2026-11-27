"""Command line entry point."""

from __future__ import annotations

import argparse
import contextlib
import logging
import logging.handlers
import os
import signal
import sys
import threading
from collections.abc import Iterator
from pathlib import Path
from typing import IO

from . import __version__, autostart, hidpp
from .config import config_dir, config_path, load_config, write_default_config
from .devices import HidppDevice
from .domain import HidppError
from .runtime import Runtime
from .transport import discover, open_endpoint

log = logging.getLogger("logimxplus")


def _out(text: str) -> None:
    if sys.stdout is not None:  # None under pythonw
        print(text)


def _setup_logging(verbose: bool) -> Path:
    path = config_dir() / "logimxplus.log"
    path.parent.mkdir(parents=True, exist_ok=True)
    root = logging.getLogger()
    root.setLevel(logging.DEBUG if verbose else logging.INFO)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    file_handler = logging.handlers.RotatingFileHandler(
        path, maxBytes=1_000_000, backupCount=3, encoding="utf-8")
    file_handler.setFormatter(fmt)
    root.addHandler(file_handler)
    if sys.stderr is not None:
        console = logging.StreamHandler(sys.stderr)
        console.setFormatter(fmt)
        root.addHandler(console)
    return path


@contextlib.contextmanager
def _single_instance() -> Iterator[bool]:
    lock_path = config_dir() / "logimxplus.lock"
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    fh: IO[str] = open(lock_path, "a+")  # noqa: SIM115 - held for the process lifetime
    try:
        if sys.platform == "win32":
            import msvcrt

            try:
                msvcrt.locking(fh.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError:
                yield False
                return
        else:
            import fcntl

            try:
                fcntl.flock(fh.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                yield False
                return
        fh.seek(0)
        fh.truncate()
        fh.write(str(os.getpid()))
        fh.flush()
        yield True
    finally:
        fh.close()


def cmd_run(args: argparse.Namespace) -> int:
    log_path = _setup_logging(args.verbose)
    with _single_instance() as acquired:
        if not acquired:
            log.error("another LogiMxPlus instance is already running")
            return 1
        config = load_config()
        log.info("LogiMxPlus %s; config %s; log %s", __version__, config_path(), log_path)
        stop = threading.Event()
        signal.signal(signal.SIGINT, lambda *_: stop.set())
        signal.signal(signal.SIGTERM, lambda *_: stop.set())
        runtime = Runtime(config)
        thread = threading.Thread(target=runtime.run, args=(stop,), daemon=True)
        thread.start()
        while thread.is_alive():
            thread.join(0.5)
    return 0


def cmd_status(_: argparse.Namespace) -> int:
    endpoints = discover()
    if not endpoints:
        _out("No Logitech HID++ receivers or Bluetooth devices found.")
        return 1
    for ep in endpoints:
        _out(f"{ep.name}  {'receiver' if ep.is_receiver else 'direct'}")
        t = open_endpoint(ep)
        try:
            indices = hidpp.RECEIVER_SLOTS if t.is_receiver else (hidpp.DIRECT_INDEX,)
            for index in indices:
                dev = HidppDevice(t, index)
                try:
                    dev.identify()
                except HidppError:
                    continue
                line = f"  [{index}] {dev.name} ({dev.kind.name.lower()})"
                with contextlib.suppress(HidppError):
                    line += f"  firmware {dev.firmware()}"
                with contextlib.suppress(HidppError):
                    info = dev.host_info()
                    line += f"  host channel {info.current + 1} of {info.count}"
                with contextlib.suppress(HidppError):
                    if dev.reprog_feature_index():
                        rep = dev.easy_switch_reporting()
                        armed = sum(rep.values())
                        line += f"  easy-switch reporting {armed}/{len(rep)} keys"
                _out(line)
        finally:
            t.close()
    return 0


def cmd_switch(args: argparse.Namespace) -> int:
    _setup_logging(args.verbose)
    runtime = Runtime(load_config())
    try:
        runtime.scan()
        if runtime.mouse is None and runtime.keyboard is None:
            log.error("neither keyboard nor mouse is connected to this computer")
            return 1
        runtime.service.switch_all(args.channel - 1)
    finally:
        runtime.close()
    return 0


def cmd_install(_: argparse.Namespace) -> int:
    write_default_config()
    _out(autostart.install())
    _out(f"config: {config_path()}")
    return 0


def cmd_uninstall(_: argparse.Namespace) -> int:
    _out(autostart.uninstall())
    return 0


def cmd_config(_: argparse.Namespace) -> int:
    _out(str(write_default_config()))
    return 0


def _attach_parent_console() -> None:
    """A windowless exe has no stdout; borrow the terminal it was started from, if any."""
    if sys.stdout is not None or not getattr(sys, "frozen", False):
        return
    if sys.platform == "win32":
        import ctypes

        attach_parent_process = -1
        if ctypes.windll.kernel32.AttachConsole(attach_parent_process):
            stream = open("CONOUT$", "w", encoding="utf-8", errors="replace", buffering=1)  # noqa: SIM115
            sys.stdout = sys.stderr = stream


def main(argv: list[str] | None = None) -> int:
    _attach_parent_console()
    parser = argparse.ArgumentParser(
        prog="logimxplus",
        description="Make your Logitech mouse follow the keyboard's Easy-Switch keys.")
    parser.add_argument("--version", action="version", version=__version__)
    parser.add_argument("-v", "--verbose", action="store_true", help="debug logging")
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("run", help="run in the foreground (the default)").set_defaults(func=cmd_run)
    sub.add_parser("status", help="show devices and their host channels").set_defaults(
        func=cmd_status)
    sw = sub.add_parser("switch", help="switch mouse and keyboard to a channel now")
    sw.add_argument("channel", type=int, choices=(1, 2, 3))
    sw.set_defaults(func=cmd_switch)
    sub.add_parser("install", help="start automatically at login").set_defaults(func=cmd_install)
    sub.add_parser("uninstall", help="stop starting at login").set_defaults(func=cmd_uninstall)
    sub.add_parser("config", help="create the config file and print its path").set_defaults(
        func=cmd_config)
    args = parser.parse_args(argv)
    func = getattr(args, "func", cmd_run)
    return int(func(args))


if __name__ == "__main__":
    raise SystemExit(main())
