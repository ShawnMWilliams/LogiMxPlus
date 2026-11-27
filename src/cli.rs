//! Command line entry point.

use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config;
use crate::device::HidppDevice;
use crate::hidpp;
use crate::logging;
use crate::platform;
use crate::runtime::{Discovery, HidDiscovery, Runtime};
use crate::service::SwitchableDevice;

macro_rules! out {
    ($($arg:tt)*) => { platform::print(&format!("{}\n", format_args!($($arg)*))) };
}

const USAGE: &str = "\
LogiMxPlus: your Logitech mouse follows the keyboard's Easy-Switch keys.

Usage: LogiMxPlus [-v] [command]

Commands:
  run          run in the background (the default; this is what login starts)
  status       show devices, firmware, host channels and Easy-Switch reporting
  switch N     switch mouse and keyboard to channel N (1-3) now
  install      start automatically at login
  uninstall    stop starting at login
  config       create the config file and print its path

Options:
  -v, --verbose  debug logging (every HID++ report)
  --version      print the version";

pub fn main() -> ExitCode {
    platform::init_console();
    let mut verbose = false;
    let mut words = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "--version" | "-V" => {
                out!("{}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" | "help" => {
                out!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => words.push(arg),
        }
    }
    let code = match words.first().map(String::as_str).unwrap_or("run") {
        "run" => run(verbose),
        "status" => status(),
        "switch" => match words
            .get(1)
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=3).contains(n))
        {
            Some(n) => switch(n, verbose),
            None => {
                out!("usage: LogiMxPlus switch <1|2|3>");
                2
            }
        },
        "install" => install(),
        "uninstall" => uninstall(),
        "config" => config_cmd(),
        other => {
            out!("unknown command {other:?}\n\n{USAGE}");
            2
        }
    };
    ExitCode::from(code)
}

fn log_path() -> std::path::PathBuf {
    config::config_dir().join("logimxplus.log")
}

fn load_config() -> Option<config::Config> {
    match config::load(&config::config_path()) {
        Ok(c) => Some(c),
        Err(e) => {
            log::error!("bad config {}: {e}", config::config_path().display());
            out!("bad config {}: {e}", config::config_path().display());
            None
        }
    }
}

fn run(verbose: bool) -> u8 {
    logging::init(&log_path(), verbose);
    let Some(_guard) = platform::single_instance(&config::config_dir().join("logimxplus.lock")) else {
        log::error!("another LogiMxPlus instance is already running");
        return 1;
    };
    let Some(config) = load_config() else { return 1 };
    log::info!(
        "LogiMxPlus {}; config {}; log {}",
        env!("CARGO_PKG_VERSION"),
        config::config_path().display(),
        log_path().display()
    );
    Runtime::new(config, HidDiscovery::new()).run()
}

fn status() -> u8 {
    let mut discovery = HidDiscovery::new();
    let endpoints = discovery.endpoints();
    if endpoints.is_empty() {
        out!("No Logitech HID++ receivers or Bluetooth devices found.");
        return 1;
    }
    let (tx, _rx) = mpsc::channel();
    for (n, ep) in endpoints.iter().enumerate() {
        out!(
            "{}  {}",
            ep.name,
            if ep.is_receiver { "receiver" } else { "direct" }
        );
        let Some(t) = discovery.open(ep, n as u64 + 1, tx.clone()) else {
            continue;
        };
        let indices: Vec<u8> = if t.is_receiver() {
            hidpp::RECEIVER_SLOTS.collect()
        } else {
            vec![hidpp::DIRECT_INDEX]
        };
        for index in indices {
            let mut dev = HidppDevice::new(t.clone(), index);
            if dev.identify().is_err() {
                continue;
            }
            let mut line = format!("  [{index}] {} ({})", dev.name, dev.kind.label());
            if let Ok(fw) = dev.firmware() {
                line += &format!("  firmware {fw}");
            }
            if let Ok(info) = dev.host_info() {
                line += &format!("  host channel {} of {}", info.current + 1, info.count);
            }
            if dev.kind == crate::domain::DeviceKind::Keyboard {
                if let Ok(armed) = dev.easy_switch_reporting() {
                    line += &format!("  easy-switch reporting {armed}/3 keys");
                }
            }
            out!("{line}");
        }
    }
    0
}

fn switch(channel: u8, verbose: bool) -> u8 {
    logging::init(&log_path(), verbose);
    let Some(config) = load_config() else { return 1 };
    let mut rt = Runtime::new(config, HidDiscovery::new());
    rt.scan(Instant::now());
    if !rt.has_devices() {
        out!("Neither keyboard nor mouse is connected to this computer.");
        return 1;
    }
    let result = rt.service.switch_all(channel - 1, Instant::now());
    std::thread::sleep(Duration::from_millis(200)); // let the last report go out
    match result {
        Ok(()) => 0,
        Err(e) => {
            out!("switch failed: {e}");
            1
        }
    }
}

fn install() -> u8 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            out!("cannot find my own path: {e}");
            return 1;
        }
    };
    if let Err(e) = config::write_default(&config::config_path()) {
        out!("cannot write config: {e}");
    }
    match platform::install_autostart(&exe) {
        Ok(msg) => {
            out!("{msg}");
            out!("config: {}", config::config_path().display());
            0
        }
        Err(e) => {
            out!("{e}");
            1
        }
    }
}

fn uninstall() -> u8 {
    match platform::uninstall_autostart() {
        Ok(msg) => {
            out!("{msg}");
            0
        }
        Err(e) => {
            out!("{e}");
            1
        }
    }
}

fn config_cmd() -> u8 {
    let path = config::config_path();
    match config::write_default(&path) {
        Ok(()) => {
            out!("{}", path.display());
            if let Some(entry) = platform::autostart_entry() {
                out!("login entry: {entry}");
            }
            0
        }
        Err(e) => {
            out!("cannot write {}: {e}", path.display());
            1
        }
    }
}
