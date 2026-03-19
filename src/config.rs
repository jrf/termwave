//! Configuration file support. Loads/saves from ~/.config/termwave/config.toml.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: String,
    pub theme: String,
    pub fps: u64,
    pub bars: usize,
    pub low_freq: f32,
    pub high_freq: f32,
    pub smoothing: f32,
    pub monstercat: bool,
    pub noise_floor: f32,
    /// Color bars by position (true) or amplitude (false).
    pub gradient_by_position: bool,
    /// Width of each bar in terminal columns.
    pub bar_width: usize,
    /// Spacing between bars in terminal columns.
    pub bar_spacing: usize,
    /// Sensitivity in percent (100 = normal).
    pub sensitivity: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: "spectrum".to_string(),
            theme: "classic".to_string(),
            fps: 60,
            bars: 0,
            low_freq: 20.0,
            high_freq: 20000.0,
            smoothing: 0.5,
            monstercat: false,
            noise_floor: 0.0,
            gradient_by_position: false,
            bar_width: 2,
            bar_spacing: 1,
            sensitivity: 100,
        }
    }
}

/// Get the config file path (~/.config/termwave/config.toml).
pub fn config_path() -> PathBuf {
    dirs().join("config.toml")
}

fn dirs() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("termwave")
}

/// Load config from disk, falling back to defaults for missing fields.
pub fn load() -> Config {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save config to disk with descriptive comments.
pub fn save(config: &Config) -> std::io::Result<()> {
    // Round floats to avoid long decimal strings like 0.35000002
    let mut rounded = config.clone();
    rounded.smoothing = (rounded.smoothing * 100.0).round() / 100.0;
    rounded.noise_floor = (rounded.noise_floor * 10000.0).round() / 10000.0;
    rounded.low_freq = rounded.low_freq.round();
    rounded.high_freq = rounded.high_freq.round();

    let dir = dirs();
    fs::create_dir_all(&dir)?;
    let raw = toml::to_string_pretty(&rounded).unwrap_or_default();
    let commented = add_comments(&raw);
    fs::write(config_path(), commented)
}

/// Insert descriptive comments above known config keys.
fn add_comments(toml: &str) -> String {
    let comments: &[(&str, &str)] = &[
        ("mode =", "# Visualization mode: spectrum, wave, scope, stereo"),
        ("theme =", "# Color theme name"),
        ("fps =", "# Target frames per second"),
        ("bars =", "# Number of bars (0 = auto-fill terminal width)"),
        ("low_freq =", "# Low frequency cutoff in Hz"),
        ("high_freq =", "# High frequency cutoff in Hz"),
        ("smoothing =", "# Temporal smoothing factor (0.0 = none, 0.99 = heavy)"),
        ("monstercat =", "# Monstercat-style smoothing (connects bar tops in a smooth curve)"),
        ("noise_floor =", "# Noise gate threshold (bars below this are zeroed)"),
        ("gradient_by_position =", "# Color by position (true) or amplitude (false)"),
        ("bar_width =", "# Width of each bar in terminal columns (1-8)"),
        ("bar_spacing =", "# Gap between bars in terminal columns (0-4)"),
        ("sensitivity =", "# Sensitivity in percent (100 = normal, higher = louder)"),
    ];

    let mut result = String::with_capacity(toml.len() * 2);
    for line in toml.lines() {
        let trimmed = line.trim();
        for &(key, comment) in comments {
            if trimmed.starts_with(key) {
                result.push_str(comment);
                result.push('\n');
                break;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}
