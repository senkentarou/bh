use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub stats: StatsConfig,
}

#[derive(Debug, Deserialize)]
pub struct StatsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stats: StatsConfig::default(),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".config")
        .join("bh")
        .join("config.toml")
}

/// Load config from `~/.bh/config.toml`.
/// Returns `None` if the file does not exist (feature opt-out).
/// Returns `Some(Config)` if the file exists (feature opt-in).
pub fn load_config() -> Option<Config> {
    let path = config_path();
    let content = fs::read_to_string(&path).ok()?;
    match toml::from_str(&content) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("Warning: failed to parse {}: {e}", path.display());
            Some(Config::default())
        }
    }
}
