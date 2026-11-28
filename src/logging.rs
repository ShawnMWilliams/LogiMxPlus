//! Minimal file logger (no extra dependencies): one line per record, local time. Also
//! echoes to the console when one is attached.
//!
//! Size-capped: once the log passes 1 MB (at start-up or while running) it becomes
//! `.log.1`, replacing the previous one, and a fresh file starts. So the logs never take
//! more than about 2 MB on disk, however long the program runs.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::platform;

const MAX_BYTES: u64 = 1_000_000;

/// The log file plus a running byte count, so the cap costs no extra disk access.
struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    written: u64,
}

impl LogFile {
    fn open(path: &Path, max_bytes: u64) -> Self {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut log = Self {
            path: path.to_owned(),
            max_bytes,
            file: None,
            written: 0,
        };
        log.reopen();
        if log.written > max_bytes {
            log.rotate();
        }
        log
    }

    fn reopen(&mut self) {
        self.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.written = std::fs::metadata(&self.path).map_or(0, |m| m.len());
    }

    fn rotate(&mut self) {
        self.file = None; // close before renaming
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
        self.reopen();
    }

    fn write(&mut self, line: &str) {
        if let Some(f) = self.file.as_mut() {
            if f.write_all(line.as_bytes()).is_ok() {
                self.written += line.len() as u64;
            }
        }
        if self.written > self.max_bytes {
            self.rotate();
        }
    }
}

struct Logger {
    level: log::LevelFilter,
    file: Mutex<LogFile>,
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
        self.file.lock().unwrap_or_else(|e| e.into_inner()).write(&line);
        if self.echo {
            platform::eprint(&line);
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap_or_else(|e| e.into_inner()).file.as_mut() {
            let _ = f.flush();
        }
    }
}

pub fn init(path: &Path, verbose: bool) {
    let file = LogFile::open(path, MAX_BYTES);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("logimxplus-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn size(path: &Path) -> u64 {
        std::fs::metadata(path).map_or(0, |m| m.len())
    }

    #[test]
    fn rotates_while_running_and_keeps_one_old_file() {
        let dir = temp_dir("running");
        let path = dir.join("logimxplus.log");
        let old = path.with_extension("log.1");
        let mut log = LogFile::open(&path, 100);
        let line = "0123456789012345678901234567890123456789
"; // 41 bytes
        for _ in 0..10 {
            log.write(line);
        }
        assert!(size(&path) <= 100, "current log stays under the cap");
        assert!(
            size(&old) > 100 && size(&old) <= 100 + 41,
            "one old file, just over the cap"
        );
        let files = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, 2, "never more than two files");
        drop(log);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotates_an_oversized_log_at_start_up() {
        let dir = temp_dir("startup");
        let path = dir.join("logimxplus.log");
        std::fs::write(&path, vec![b'x'; 150]).unwrap();
        let log = LogFile::open(&path, 100);
        assert_eq!(log.written, 0);
        assert_eq!(size(&path.with_extension("log.1")), 150);
        drop(log);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
