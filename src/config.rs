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

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

/// Named environment profile (`[profiles.<name>]`) for rapid multi-environment switching (`--profile`).
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct ProfileConfig {
    /// Target GCP project ID(s) or Log View resource names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Optional IAM service account email to impersonate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impersonate_service_account: Option<String>,

    /// Default GCP filter expression for this profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    /// Default minimum severity for this profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,

    /// Default relative time range (`--since`) for this profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Built-in high-value SRE filter bookmarks available in the TUI (`b` key).
pub fn default_sre_bookmarks() -> Vec<(String, String)> {
    vec![
        (
            "Errors & Criticals".to_string(),
            "severity>=ERROR".to_string(),
        ),
        (
            "Warnings & Above".to_string(),
            "severity>=WARNING".to_string(),
        ),
        (
            "HTTP 5xx Failures".to_string(),
            "httpRequest.status>=500".to_string(),
        ),
        (
            "Cloud Run Errors".to_string(),
            "resource.type=\"cloud_run_revision\" severity>=ERROR".to_string(),
        ),
        (
            "GKE Container Errors".to_string(),
            "resource.type=\"k8s_container\" severity>=ERROR".to_string(),
        ),
    ]
}

/// Configuration errors.
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Main configuration structure.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Config {
    /// AI settings (active provider, model, and per-provider credentials).
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

    /// Named environment profiles (`[profiles.<name>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfileConfig>,

    /// Saved GCP filter bookmarks (`[bookmarks]`) accessible via `b` key in TUI.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bookmarks: BTreeMap<String, String>,
}

/// AI configuration.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiConfig {
    /// Active AI provider ("google", "openai", "anthropic", "agnes").
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Legacy/fallback API key (can also use provider-specific keys or env vars).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Active model name to use.
    #[serde(default = "default_model")]
    pub model: String,

    /// Whether to scrub PII and secrets locally before sending logs to AI (default: true).
    #[serde(default = "default_redact_pii")]
    pub redact_pii: bool,

    /// Per-provider credentials and optional custom base URLs.
    #[serde(default)]
    pub providers: ProvidersConfig,
}

/// Per-provider configuration block (`[ai.providers.<name>]`).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ProvidersConfig {
    /// Google Gemini configuration (`GEMINI_API_KEY`).
    #[serde(default)]
    pub google: ProviderEndpointConfig,

    /// Google Cloud Vertex AI configuration (uses GCP ADC / Impersonation by default).
    #[serde(default)]
    pub vertex: VertexProviderConfig,

    /// OpenAI GPT configuration (`OPENAI_API_KEY`).
    #[serde(default)]
    pub openai: ProviderEndpointConfig,

    /// Anthropic Claude configuration (`ANTHROPIC_API_KEY`).
    #[serde(default)]
    pub anthropic: ProviderEndpointConfig,

    /// Agnes AI configuration (`AGNES_API_KEY`).
    #[serde(default)]
    pub agnes: ProviderEndpointConfig,
}

/// Vertex AI endpoint configuration (`[ai.providers.vertex]`).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct VertexProviderConfig {
    /// Optional GCP project ID for Vertex AI (defaults to `VERTEX_PROJECT`, `GOOGLE_CLOUD_PROJECT`, or `[defaults].project`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Optional GCP region for Vertex AI (defaults to `VERTEX_LOCATION` or `"us-central1"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Optional static OAuth2 access token override (normally omitted so ADC / Impersonation is used automatically).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Optional custom Vertex AI base URL override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// Endpoint and credential settings for an individual AI provider.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ProviderEndpointConfig {
    /// Provider API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Optional custom API base URL override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

fn default_provider() -> String {
    "google".to_string()
}

fn default_model() -> String {
    "gemini-3.8-flash".to_string()
}

fn default_redact_pii() -> bool {
    true
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key: None,
            model: default_model(),
            redact_pii: default_redact_pii(),
            providers: ProvidersConfig::default(),
        }
    }
}

/// Default values configuration.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DefaultsConfig {
    /// Default GCP project ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Optional target service account email for IAM Service Account Impersonation (`--impersonate-service-account`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impersonate_service_account: Option<String>,

    /// Default time range for queries (e.g., "1h", "30m", "24h").
    #[serde(default = "default_since")]
    pub since: String,

    /// Default log limit.
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Default severity filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            project: None,
            impersonate_service_account: None,
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
#[derive(Debug, Serialize, Deserialize, Clone)]
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
#[derive(Debug, Serialize, Deserialize, Clone)]
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

    /// Save configuration to a specific file path.
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Save configuration to the default config file path.
    pub fn save(&self) -> Result<PathBuf, ConfigError> {
        let path = Self::default_path();
        self.save_to_file(&path)?;
        Ok(path)
    }

    /// Update active AI provider and model in memory.
    pub fn set_active_model(&mut self, provider: &str, model: &str) {
        self.ai.provider = provider.to_lowercase();
        self.ai.model = model.to_string();
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

    /// Get the persistent TUI input history file path (`~/.config/racoon/history.json`).
    pub fn history_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("racoon")
            .join("history.json")
    }

    /// Apply a named profile (`--profile <name>`) on top of `self.defaults`, returning the profile.
    pub fn apply_profile(&mut self, name: &str) -> Result<ProfileConfig, String> {
        let profile = self
            .profiles
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Profile '{}' not found in config.toml", name))?;

        if let Some(ref proj) = profile.project
            && !proj.trim().is_empty()
        {
            self.defaults.project = Some(proj.trim().to_string());
        }
        if let Some(ref sa) = profile.impersonate_service_account
            && !sa.trim().is_empty()
        {
            self.defaults.impersonate_service_account = Some(sa.trim().to_string());
        }
        if let Some(ref sev) = profile.severity
            && !sev.trim().is_empty()
        {
            self.defaults.severity = Some(sev.trim().to_string());
        }
        if let Some(ref since) = profile.since
            && !since.trim().is_empty()
        {
            self.defaults.since = since.trim().to_string();
        }

        Ok(profile)
    }

    /// Return the combined list of user-configured `[bookmarks]` and built-in SRE filter bookmarks.
    pub fn effective_bookmarks(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .bookmarks
            .iter()
            .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();

        for (name, filter) in default_sre_bookmarks() {
            if !out
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case(&name) || v == &filter)
            {
                out.push((name, filter));
            }
        }
        out
    }

    /// Get API key for a specific provider (environment variable takes precedence over config file).
    pub fn get_provider_api_key(&self, provider: &str) -> Option<String> {
        use crate::ai::ProviderKind;

        let active_kind = ProviderKind::try_parse(&self.ai.provider);
        let fallback_key = |target: ProviderKind| {
            if active_kind == Some(target) {
                self.ai.api_key.clone().filter(|k| !k.is_empty())
            } else {
                None
            }
        };

        match ProviderKind::try_parse(provider) {
            Some(ProviderKind::Google) => std::env::var("GEMINI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .or_else(|| {
                    self.ai
                        .providers
                        .google
                        .api_key
                        .clone()
                        .filter(|k| !k.is_empty())
                })
                .or_else(|| self.ai.api_key.clone().filter(|k| !k.is_empty())),
            Some(ProviderKind::OpenAi) => std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .or_else(|| {
                    self.ai
                        .providers
                        .openai
                        .api_key
                        .clone()
                        .filter(|k| !k.is_empty())
                })
                .or_else(|| fallback_key(ProviderKind::OpenAi)),
            Some(ProviderKind::Anthropic) => std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .or_else(|| {
                    self.ai
                        .providers
                        .anthropic
                        .api_key
                        .clone()
                        .filter(|k| !k.is_empty())
                })
                .or_else(|| fallback_key(ProviderKind::Anthropic)),
            Some(ProviderKind::Agnes) => std::env::var("AGNES_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .or_else(|| {
                    self.ai
                        .providers
                        .agnes
                        .api_key
                        .clone()
                        .filter(|k| !k.is_empty())
                })
                .or_else(|| fallback_key(ProviderKind::Agnes)),
            Some(ProviderKind::VertexAi) => std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
                .ok()
                .or_else(|| std::env::var("VERTEX_API_KEY").ok())
                .filter(|k| !k.is_empty())
                .or_else(|| {
                    self.ai
                        .providers
                        .vertex
                        .api_key
                        .clone()
                        .filter(|k| !k.is_empty())
                }),
            None => self.ai.api_key.clone().filter(|k| !k.is_empty()),
        }
    }

    /// Get custom base URL for a specific provider if configured.
    pub fn get_provider_base_url(&self, provider: &str) -> Option<String> {
        use crate::ai::ProviderKind;

        match ProviderKind::try_parse(provider)? {
            ProviderKind::Google => self.ai.providers.google.base_url.clone(),
            ProviderKind::VertexAi => self.ai.providers.vertex.base_url.clone(),
            ProviderKind::OpenAi => self.ai.providers.openai.base_url.clone(),
            ProviderKind::Anthropic => self.ai.providers.anthropic.base_url.clone(),
            ProviderKind::Agnes => self.ai.providers.agnes.base_url.clone(),
        }
        .filter(|u| !u.trim().is_empty())
    }

    /// Resolve the target service account email for IAM Service Account Impersonation
    /// (CLI `--impersonate-service-account` > `GOOGLE_IMPERSONATE_SERVICE_ACCOUNT` > `[defaults].impersonate_service_account`).
    pub fn resolve_impersonate_service_account(
        &self,
        cli_override: Option<&str>,
    ) -> Option<String> {
        cli_override
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("GOOGLE_IMPERSONATE_SERVICE_ACCOUNT")
                    .ok()
                    .or_else(|| std::env::var("CLOUDSDK_AUTH_IMPERSONATE_SERVICE_ACCOUNT").ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                self.defaults
                    .impersonate_service_account
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
    }

    /// Resolve the GCP project ID for Vertex AI requests
    /// (`VERTEX_PROJECT` > `GOOGLE_CLOUD_PROJECT` > `[ai.providers.vertex].project` > `cli_project` > `[defaults].project`).
    pub fn get_vertex_project(&self, cli_project: Option<&str>) -> Option<String> {
        let raw = std::env::var("VERTEX_PROJECT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("GOOGLE_CLOUD_PROJECT")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                self.ai
                    .providers
                    .vertex
                    .project
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                cli_project
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                self.defaults
                    .project
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })?;

        crate::gcp::extract_first_gcp_project_id(&raw)
    }

    /// Resolve the GCP region/location for Vertex AI (`VERTEX_LOCATION` > `CLOUD_ML_REGION` > `[ai.providers.vertex].location` > `"us-central1"`).
    pub fn get_vertex_location(&self) -> String {
        std::env::var("VERTEX_LOCATION")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("CLOUD_ML_REGION")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                self.ai
                    .providers
                    .vertex
                    .location
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| "us-central1".to_string())
            .trim()
            .to_string()
    }

    /// Get API key for the currently active AI provider.
    pub fn get_api_key(&self) -> Option<String> {
        self.get_provider_api_key(&self.ai.provider)
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
        assert_eq!(config.ai.provider, "google");
        assert_eq!(config.ai.model, "gemini-3.8-flash");
    }

    #[test]
    fn test_load_and_save_multi_provider_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let content = r#"
[ai]
provider = "anthropic"
model = "claude-sonnet-4-5"

[ai.providers.google]
api_key = "google-key"

[ai.providers.openai]
api_key = "openai-key"

[ai.providers.anthropic]
api_key = "claude-key"

[ai.providers.agnes]
api_key = "agnes-key"

[defaults]
project = "test-project"
since = "2h"
limit = 50
"#;

        std::fs::write(&path, content).unwrap();

        let mut config = Config::load_from_file(&path.to_path_buf()).unwrap();
        assert_eq!(config.ai.provider, "anthropic");
        assert_eq!(config.ai.model, "claude-sonnet-4-5");
        assert_eq!(
            config.get_provider_api_key("google"),
            Some("google-key".to_string())
        );
        assert_eq!(
            config.get_provider_api_key("openai"),
            Some("openai-key".to_string())
        );
        assert_eq!(
            config.get_provider_api_key("anthropic"),
            Some("claude-key".to_string())
        );
        assert_eq!(
            config.get_provider_api_key("agnes"),
            Some("agnes-key".to_string())
        );

        // Switch model and save
        config.set_active_model("agnes", "agnes-3.0-pro");
        config.save_to_file(&path.to_path_buf()).unwrap();

        let reloaded = Config::load_from_file(&path.to_path_buf()).unwrap();
        assert_eq!(reloaded.ai.provider, "agnes");
        assert_eq!(reloaded.ai.model, "agnes-3.0-pro");
    }

    #[test]
    fn test_get_api_key_env_priority() {
        let config = Config {
            ai: AiConfig {
                provider: "google".to_string(),
                api_key: Some("config-key".to_string()),
                model: default_model(),
                redact_pii: true,
                providers: ProvidersConfig::default(),
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

    #[test]
    fn test_config_profiles_and_effective_bookmarks() {
        let toml_str = r#"
[defaults]
project = "default-proj"
since = "1h"

[profiles.prod-checkout]
project = "prod-apigw,prod-checkout"
impersonate_service_account = "sre@prod.iam.gserviceaccount.com"
filter = "severity>=ERROR"
since = "30m"

[bookmarks]
"Payment Timeouts" = "resource.type=\"cloud_run_revision\" \"payment timeout\""
"#;
        let mut config: Config = toml::from_str(toml_str).unwrap();
        let profile = config.apply_profile("prod-checkout").unwrap();
        assert_eq!(profile.filter.as_deref(), Some("severity>=ERROR"));
        assert_eq!(
            config.defaults.project.as_deref(),
            Some("prod-apigw,prod-checkout")
        );
        assert_eq!(
            config.defaults.impersonate_service_account.as_deref(),
            Some("sre@prod.iam.gserviceaccount.com")
        );
        assert_eq!(config.defaults.since, "30m");

        let bookmarks = config.effective_bookmarks();
        assert!(
            bookmarks
                .iter()
                .any(|(name, f)| { name == "Payment Timeouts" && f.contains("payment timeout") })
        );
        assert!(bookmarks.iter().any(|(_, f)| f == "severity>=ERROR"));
    }
}
