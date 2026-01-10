//! Configuration management for racoon.
//!
//! Loads configuration from:
//! 1. Config file (~/.config/racoon/config.toml)
//! 2. Environment variables (override config file)
//! 3. CLI arguments (highest priority, handled in main.rs)
//!
//! # Config File Location
//!
//! - Linux/macOS: `~/.config/racoon/config.toml`
//! - Windows: `%APPDATA%\racoon\config.toml`
//!
//! # Example Config
//!
//! ```toml
//! [ai]
//! api_key = "AIza..."
//!
//! [defaults]
//! project = "my-project-123"
//! since = "1h"
//! ```

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, warn};

/// Configuration errors.
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Main configuration structure.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Config {
    /// AI (Gemini) settings.
    #[serde(default)]
    pub ai: AiConfig,

    /// Default values for CLI arguments.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// UI settings.
    #[serde(default)]
    pub ui: UiConfig,

    /// Logging settings.
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// AI configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct AiConfig {
    /// Gemini API key (can also use GEMINI_API_KEY env var).
    pub api_key: Option<String>,

    /// Model name to use.
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "gemini-3-flash-preview".to_string()
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            model: default_model(),
        }
    }
}

/// Default values configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct DefaultsConfig {
    /// Default GCP project ID.
    pub project: Option<String>,

    /// Default time range for queries (e.g., "1h", "30m", "24h").
    #[serde(default = "default_since")]
    pub since: String,

    /// Default log limit.
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Default severity filter.
    pub severity: Option<String>,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            project: None,
            since: default_since(),
            limit: default_limit(),
            severity: None,
        }
    }
}

fn default_since() -> String {
    "1h".to_string()
}

fn default_limit() -> usize {
    100
}

/// UI configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct UiConfig {
    /// Maximum number of logs to keep in memory.
    #[serde(default = "default_max_logs")]
    pub max_logs: usize,

    /// Enable mouse support in TUI.
    #[serde(default)]
    pub mouse: bool,

    /// Color theme (dark/light).
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            max_logs: default_max_logs(),
            mouse: false,
            theme: default_theme(),
        }
    }
}

fn default_max_logs() -> usize {
    100
}

fn default_theme() -> String {
    "dark".to_string()
}

/// Logging configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Log level (debug/info/warn/error).
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Config {
    /// Load configuration from default locations.
    ///
    /// Returns default config if file doesn't exist or fails to parse.
    pub fn load() -> Self {
        let path = Self::default_path();

        if path.exists() {
            match Self::load_from_file(&path) {
                Ok(config) => {
                    debug!(path = %path.display(), "Loaded config file");
                    return config;
                }
                Err(e) => {
                    warn!(error = %e, path = %path.display(), "Failed to load config, using defaults");
                }
            }
        } else {
            debug!(path = %path.display(), "Config file not found, using defaults");
        }

        Self::default()
    }

    /// Load config from a specific file path.
    pub fn load_from_file(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the default config file path.
    ///
    /// - Linux/macOS: `~/.config/racoon/config.toml`
    /// - Windows: `%APPDATA%\racoon\config.toml`
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("racoon")
            .join("config.toml")
    }

    /// Get Gemini API key (env var takes precedence over config file).
    pub fn get_api_key(&self) -> Option<String> {
        std::env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| self.ai.api_key.clone())
    }

    /// Initialize default config file if it doesn't exist.
    ///
    /// Returns the path to the config file.
    pub fn init() -> Result<PathBuf, ConfigError> {
        let path = Self::default_path();

        if !path.exists() {
            // Create parent directories
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Write default config
            let default_content = include_str!("../config_template.toml");
            std::fs::write(&path, default_content)?;

            debug!(path = %path.display(), "Created default config file");
        }

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.defaults.since, "1h");
        assert_eq!(config.defaults.limit, 100);
        assert_eq!(config.ui.max_logs, 100);
        assert_eq!(config.ai.model, "gemini-3-flash-preview");
    }

    #[test]
    fn test_load_from_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let content = r#"
[ai]
api_key = "test-key"

[defaults]
project = "test-project"
since = "2h"
limit = 50
"#;

        std::fs::write(&path, content).unwrap();

        let config = Config::load_from_file(&path.to_path_buf()).unwrap();
        assert_eq!(config.ai.api_key, Some("test-key".to_string()));
        assert_eq!(config.defaults.project, Some("test-project".to_string()));
        assert_eq!(config.defaults.since, "2h");
        assert_eq!(config.defaults.limit, 50);
    }

    #[test]
    fn test_get_api_key_env_priority() {
        let config = Config {
            ai: AiConfig {
                api_key: Some("config-key".to_string()),
                model: default_model(),
            },
            ..Default::default()
        };

        // Set env var
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "env-key");
        }

        // Env var should take priority
        assert_eq!(config.get_api_key(), Some("env-key".to_string()));

        unsafe {
            std::env::remove_var("GEMINI_API_KEY");
        }

        // Now config value should be returned
        assert_eq!(config.get_api_key(), Some("config-key".to_string()));
    }
}
