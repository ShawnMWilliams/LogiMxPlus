//! OS integration: console output for a windowless exe, local timestamps, single
//! instance, and starting at login.
//!
//! Windows is the tested platform. The macOS and Linux branches compile and follow the
//! documented conventions but have not been run on real hardware; see docs/PLATFORMS.md.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Console attached to on Windows when the exe (a GUI-subsystem program, so no console
/// of its own) is started from a terminal. `None` means: use the normal std streams.
static CONSOLE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

pub fn init_console() {
    CONSOLE.get_or_init(attach_parent_console);
}

#[cfg(windows)]
fn attach_parent_console() -> Option<Mutex<File>> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain Win32 calls with no pointers passed.
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
        return None; // stdout was handed to us (pipe, file, or console): std works
    }
    // SAFETY: as above.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return None; // started without a terminal (e.g. at login)
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open("CONOUT$")
        .ok()
        .map(Mutex::new)
}

#[cfg(not(windows))]
fn attach_parent_console() -> Option<Mutex<File>> {
    None
}

/// Whether text written with [`print`] / [`eprint`] goes anywhere visible.
pub fn has_console() -> bool {
    if matches!(CONSOLE.get(), Some(Some(_))) {
        return true;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
        // SAFETY: no pointers passed.
        let h = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        !h.is_null() && h != INVALID_HANDLE_VALUE
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub fn print(text: &str) {
    match CONSOLE.get() {
        Some(Some(c)) => {
            let _ = c
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .write_all(text.as_bytes());
        }
        _ => {
            let _ = std::io::stdout().write_all(text.as_bytes());
        }
    }
}

pub fn eprint(text: &str) {
    match CONSOLE.get() {
        Some(Some(c)) => {
            let _ = c
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .write_all(text.as_bytes());
        }
        _ => {
            let _ = std::io::stderr().write_all(text.as_bytes());
        }
    }
}

/// `YYYY-MM-DD HH:MM:SS.mmm` in local time (Windows) or UTC (elsewhere, marked `Z`).
pub fn local_timestamp() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        // SAFETY: GetLocalTime fills the struct we pass.
        let t = unsafe {
            let mut t = std::mem::zeroed();
            GetLocalTime(&mut t);
            t
        };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
        )
    }
    #[cfg(not(windows))]
    {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = d.as_secs();
        let (days, rem) = (secs / 86_400, secs % 86_400);
        let (y, m, day) = civil_from_days(days as i64);
        format!(
            "{y:04}-{m:02}-{day:02} {:02}:{:02}:{:02}.{:03}Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60,
            d.subsec_millis()
        )
    }
}

/// Howard Hinnant's days-to-civil algorithm.
#[cfg(not(windows))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Hand back the memory pages touched during start-up (enumeration, device setup).
/// Windows trims an idle process like this on its own after a while; doing it once,
/// right after start-up, gets the low steady-state figure immediately. Pages needed
/// again later are faulted back in cheaply. No-op off Windows.
pub fn trim_working_set() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        // SAFETY: the pseudo-handle refers to this process; (-1, -1) asks Windows to
        // remove as many pages as possible from its working set.
        unsafe {
            SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }
}

/// Holds the single-instance lock for the life of the process.
pub struct InstanceGuard(#[allow(dead_code)] File);

pub fn single_instance(lock_path: &Path) -> Option<InstanceGuard> {
    if let Some(dir) = lock_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .ok()?;
    file.try_lock().ok()?;
    Some(InstanceGuard(file))
}

// --- start at login ------------------------------------------------------------------

#[cfg(windows)]
mod autostart {
    use std::path::Path;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "LogiMxPlus";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn install(exe: &Path) -> Result<String, String> {
        let command = format!("\"{}\" run", exe.display());
        let data = wide(&command);
        let bytes = u32::try_from(data.len() * 2).map_err(|e| e.to_string())?;
        // SAFETY: all pointers reference live, NUL-terminated UTF-16 buffers.
        let rc = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                wide(VALUE).as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        if rc == 0 {
            Ok(format!("starts at login: {command}"))
        } else {
            Err(format!("could not write the login entry (error {rc})"))
        }
    }

    pub fn uninstall() -> Result<String, String> {
        // SAFETY: pointers reference live, NUL-terminated UTF-16 buffers.
        let rc =
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(VALUE).as_ptr()) };
        match rc {
            0 => Ok("login entry removed".into()),
            2 => Ok("no login entry was registered".into()), // ERROR_FILE_NOT_FOUND
            _ => Err(format!("could not remove the login entry (error {rc})")),
        }
    }

    pub fn current() -> Option<String> {
        let mut buf = vec![0u16; 2048];
        let mut bytes = u32::try_from(buf.len() * 2).ok()?;
        // SAFETY: buffer and size describe a live allocation; other pointers are live strings.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                wide(VALUE).as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if rc != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

#[cfg(target_os = "macos")]
mod autostart {
    //! LaunchAgent. Untested on real hardware (see docs/PLATFORMS.md).
    use std::path::{Path, PathBuf};

    const LABEL: &str = "io.github.shawnmwilliams.logimxplus";

    fn plist() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
    }

    pub fn install(exe: &Path) -> Result<String, String> {
        let path = plist();
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>Label</key><string>{LABEL}</string>\n\
             <key>ProgramArguments</key><array><string>{}</string><string>run</string></array>\n\
             <key>RunAtLoad</key><true/>\n<key>KeepAlive</key><true/>\n\
             </dict></plist>\n",
            exe.display()
        );
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        let _ = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&path)
            .status();
        Ok(format!("installed {}", path.display()))
    }

    pub fn uninstall() -> Result<String, String> {
        let path = plist();
        if !path.exists() {
            return Ok("no LaunchAgent was installed".into());
        }
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .status();
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        Ok(format!("removed {}", path.display()))
    }

    pub fn current() -> Option<String> {
        plist().exists().then(|| plist().display().to_string())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod autostart {
    //! XDG autostart. Untested on real hardware (see docs/PLATFORMS.md).
    use std::path::{Path, PathBuf};

    fn desktop() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("autostart/logimxplus.desktop")
    }

    pub fn install(exe: &Path) -> Result<String, String> {
        let path = desktop();
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=LogiMxPlus\nExec=\"{}\" run\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        Ok(format!("installed {}", path.display()))
    }

    pub fn uninstall() -> Result<String, String> {
        let path = desktop();
        if !path.exists() {
            return Ok("no autostart entry was installed".into());
        }
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        Ok(format!("removed {}", path.display()))
    }

    pub fn current() -> Option<String> {
        desktop().exists().then(|| desktop().display().to_string())
    }
}

pub use autostart::{
    current as autostart_entry, install as install_autostart, uninstall as uninstall_autostart,
};
