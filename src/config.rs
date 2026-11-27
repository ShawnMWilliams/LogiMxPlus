//! User configuration (TOML) and per-OS file locations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

pub const DEFAULT_CONFIG: &str = r#"# LogiMxPlus configuration.

# Optional: only use devices whose name contains these strings.
# keyboard_name = "MX Keys"
# mouse_name = "MX Master"

# Seconds a switch stays pending when the mouse is asleep and cannot be reached.
pending_ttl = 120

# Maps each keyboard Easy-Switch key (1-3) to the mouse's Easy-Switch channel (1-3)
# for the same computer. Leave it as-is if you paired both devices to each computer
# on the same channel number. (Keep this table last: TOML keys below a [table]
# header belong to that table.)
[channels]
1 = 1
2 = 2
3 = 3
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Zero-based keyboard slot -> zero-based mouse slot.
    pub channel_map: HashMap<u8, u8>,
    pub keyboard_name: Option<String>,
    pub mouse_name: Option<String>,
    pub pending_ttl: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel_map: HashMap::new(),
            keyboard_name: None,
            mouse_name: None,
            pending_ttl: Duration::from_secs(120),
        }
    }
}

#[derive(Deserialize)]
struct Raw {
    channels: Option<HashMap<String, i64>>,
    keyboard_name: Option<String>,
    mouse_name: Option<String>,
    pending_ttl: Option<f64>,
}

pub fn parse(text: &str) -> Result<Config, String> {
    let raw: Raw = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut channel_map = HashMap::new();
    for (key, value) in raw.channels.unwrap_or_default() {
        let kb: i64 = key
            .trim()
            .parse()
            .map_err(|_| format!("bad channel key {key:?}"))?;
        if !(1..=3).contains(&kb) || !(1..=3).contains(&value) {
            return Err(format!("channel mapping {kb} = {value} must use values 1-3"));
        }
        channel_map.insert((kb - 1) as u8, (value - 1) as u8);
    }
    let ttl = raw.pending_ttl.unwrap_or(120.0);
    if !(0.0..=86_400.0).contains(&ttl) {
        return Err(format!("pending_ttl {ttl} must be between 0 and 86400 seconds"));
    }
    Ok(Config {
        channel_map,
        keyboard_name: raw.keyboard_name.filter(|s| !s.is_empty()),
        mouse_name: raw.mouse_name.filter(|s| !s.is_empty()),
        pending_ttl: Duration::from_secs_f64(ttl),
    })
}

pub fn config_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("LogiMxPlus")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/LogiMxPlus")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("logimxplus")
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

pub fn write_default(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if !path.exists() {
        std::fs::write(path, DEFAULT_CONFIG)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_identity() {
        let c = parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(c.channel_map, HashMap::from([(0, 0), (1, 1), (2, 2)]));
        assert_eq!(c.keyboard_name, None);
        assert_eq!(c.pending_ttl, Duration::from_secs(120));
    }

    #[test]
    fn custom_mapping_and_names() {
        let c = parse("keyboard_name = \"MX Keys\"\n[channels]\n1 = 3\n").unwrap();
        assert_eq!(c.channel_map, HashMap::from([(0, 2)]));
        assert_eq!(c.keyboard_name.as_deref(), Some("MX Keys"));
    }

    #[test]
    fn invalid_channel_rejected() {
        assert!(parse("[channels]\n1 = 4\n").is_err());
        assert!(parse("[channels]\nx = 1\n").is_err());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let c = load(Path::new("definitely/not/here.toml")).unwrap();
        assert!(c.channel_map.is_empty());
    }
}
