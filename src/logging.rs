//! Minimal file logger (no extra dependencies): one line per record, local time,
//! rotated to `.log.1` at startup once it passes 1 MB. Also echoes to the console
//! when one is attached.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use crate::platform;

const MAX_BYTES: u64 = 1_000_000;

struct Logger {
    level: log::LevelFilter,
    file: Mutex<Option<File>>,
    echo: bool,
}

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= self.level && meta.target().starts_with("logimxplus")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target().trim_start_matches("logimxplus::");
        let line = format!(
            "{} {:<5} {target}: {}\n",
            platform::local_timestamp(),
            record.level(),
            record.args()
        );
        if let Some(f) = self.file.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
        if self.echo {
            platform::eprint(&line);
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = f.flush();
        }
    }
}

pub fn init(path: &Path, verbose: bool) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    let level = if verbose {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    };
    let logger = Logger {
        level,
        file: Mutex::new(file),
        echo: platform::has_console(),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}
