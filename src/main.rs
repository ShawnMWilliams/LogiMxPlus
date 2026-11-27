//! LogiMxPlus: your Logitech mouse follows the keyboard's Easy-Switch keys.
//!
//! Layers (see README): `domain` and `hidpp` are pure; `transport` and `device` are
//! gateways to the HID hardware; `service` holds the switching rules; `runtime` is the
//! composition root; `cli`, `config`, `logging` and `platform` are the edges.

// A windowless program on Windows: no console flashes at login. CLI commands attach
// to the terminal they were started from (see platform::init_console).
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod cli;
mod config;
mod device;
mod domain;
mod hidpp;
mod logging;
mod platform;
mod runtime;
mod service;
mod transport;

fn main() -> std::process::ExitCode {
    cli::main()
}
