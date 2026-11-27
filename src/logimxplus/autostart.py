"""Start LogiMxPlus at login: HKCU Run key (Windows), LaunchAgent (macOS), XDG (Linux)."""

from __future__ import annotations

import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

APP_ID = "LogiMxPlus"
LAUNCH_AGENT_LABEL = "tech.logimxplus.agent"


def launch_command() -> list[str]:
    exe = Path(sys.executable)
    if sys.platform == "win32":
        # Prefer the windowless launcher installed with the package, so Task Manager
        # lists the process as "logimxplusw.exe" rather than a generic "Python".
        launcher = shutil.which("logimxplusw")
        if launcher:
            return [launcher, "run"]
        pythonw = exe.with_name("pythonw.exe")
        if pythonw.exists():
            exe = pythonw
    return [str(exe), "-m", "logimxplus", "run"]


def _quote_windows(args: list[str]) -> str:
    return subprocess.list2cmdline(args)


def install() -> str:
    cmd = launch_command()
    if sys.platform == "win32":
        import winreg

        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"Software\Microsoft\Windows\CurrentVersion\Run", 0,
                            winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, APP_ID, 0, winreg.REG_SZ, _quote_windows(cmd))
        return "registered in HKCU\\...\\Run"
    if sys.platform == "darwin":
        plist = Path.home() / "Library" / "LaunchAgents" / f"{LAUNCH_AGENT_LABEL}.plist"
        plist.parent.mkdir(parents=True, exist_ok=True)
        plist.write_bytes(plistlib.dumps({
            "Label": LAUNCH_AGENT_LABEL,
            "ProgramArguments": cmd,
            "RunAtLoad": True,
            "KeepAlive": True,
        }))
        subprocess.run(["launchctl", "load", "-w", str(plist)], check=False)
        return f"installed {plist}"
    desktop = Path.home() / ".config" / "autostart" / "logimxplus.desktop"
    desktop.parent.mkdir(parents=True, exist_ok=True)
    desktop.write_text(
        "[Desktop Entry]\nType=Application\nName=LogiMxPlus\n"
        f"Exec={' '.join(cmd)}\nX-GNOME-Autostart-enabled=true\n", encoding="utf-8")
    return f"installed {desktop}"


def uninstall() -> str:
    if sys.platform == "win32":
        import winreg

        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                                r"Software\Microsoft\Windows\CurrentVersion\Run", 0,
                                winreg.KEY_SET_VALUE) as key:
                winreg.DeleteValue(key, APP_ID)
        except FileNotFoundError:
            return "was not registered"
        return "removed from HKCU\\...\\Run"
    if sys.platform == "darwin":
        plist = Path.home() / "Library" / "LaunchAgents" / f"{LAUNCH_AGENT_LABEL}.plist"
        if plist.exists():
            subprocess.run(["launchctl", "unload", "-w", str(plist)], check=False)
            plist.unlink()
            return f"removed {plist}"
        return "was not installed"
    desktop = Path.home() / ".config" / "autostart" / "logimxplus.desktop"
    if desktop.exists():
        desktop.unlink()
        return f"removed {desktop}"
    return "was not installed"
