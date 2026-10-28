//! Multi-provider AI model registry and self-updating cache.
//!
//! Manages supported AI providers (Google Gemini, OpenAI GPT, Anthropic Claude, Agnes AI),
//! built-in curated model lists, remote `/models` discovery, and local cache persistence
//! at `~/.config/racoon/models_cache.json`.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::config::Config;

/// Supported AI model providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Google Gemini (`generativelanguage.googleapis.com`)
    Google,
    /// Google Cloud Vertex AI (`aiplatform.googleapis.com` via GCP ADC / IAM Impersonation)
    VertexAi,
    /// OpenAI GPT (`api.openai.com`)
    OpenAi,
    /// Anthropic Claude (`api.anthropic.com`)
    Anthropic,
    /// Agnes AI (`apihub.agnes-ai.com`)
    Agnes,
}

impl ProviderKind {
    /// Return all supported providers in display order.
    pub fn all() -> &'static [ProviderKind] {
        &[
            ProviderKind::Google,
            ProviderKind::VertexAi,
            ProviderKind::OpenAi,
            ProviderKind::Anthropic,
            ProviderKind::Agnes,
        ]
    }

    /// Canonical configuration key for this provider.
    pub fn id(&self) -> &'static str {
        match self {
            ProviderKind::Google => "google",
            ProviderKind::VertexAi => "vertex",
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Agnes => "agnes",
        }
    }

    /// Human-friendly display name for menus and CLI output.
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderKind::Google => "Google (Gemini API)",
            ProviderKind::VertexAi => "Google Cloud Vertex AI (ADC / Zero-Key)",
            ProviderKind::OpenAi => "OpenAI (GPT)",
            ProviderKind::Anthropic => "Anthropic (Claude)",
            ProviderKind::Agnes => "Agnes AI",
        }
    }

    /// Environment variable name used for this provider's API key (or ADC project for Vertex AI).
    pub fn env_var(&self) -> &'static str {
        match self {
            ProviderKind::Google => "GEMINI_API_KEY",
            ProviderKind::VertexAi => "GOOGLE_CLOUD_PROJECT (uses GCP ADC)",
            ProviderKind::OpenAi => "OPENAI_API_KEY",
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::Agnes => "AGNES_API_KEY",
        }
    }

    /// Default API base URL for this provider.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::Google => "https://generativelanguage.googleapis.com/v1beta/models",
            ProviderKind::VertexAi => "https://us-central1-aiplatform.googleapis.com/v1",
            ProviderKind::OpenAi => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Agnes => "https://apihub.agnes-ai.com/v1",
        }
    }

    /// Parse a provider string from config or user input.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "google" | "gemini" => ProviderKind::Google,
            "vertex" | "vertex-ai" | "vertex_ai" | "gcp-vertex" | "vertexai" => {
                ProviderKind::VertexAi
            }
            "openai" | "gpt" => ProviderKind::OpenAi,
            "anthropic" | "claude" => ProviderKind::Anthropic,
            "agnes" | "agnes-ai" | "agnes_ai" => ProviderKind::Agnes,
            _ => ProviderKind::Google,
        }
    }
}

/// Metadata for a single selectable AI model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    /// API model identifier (e.g., `"gemini-3.8-flash"`, `"claude-sonnet-4-5"`).
    pub id: String,
    /// Short display title.
    pub display_name: String,
    /// Brief description of the model's strengths.
    pub description: String,
}

impl ModelInfo {
    pub fn new(id: &str, display_name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            description: description.to_string(),
        }
    }
}

/// Status of updating a single provider's model list.
#[derive(Debug, Clone)]
pub struct ProviderUpdateStatus {
    pub provider: ProviderKind,
    pub count: usize,
    pub source: String,
}

/// Persistent model registry cached at `~/.config/racoon/models_cache.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsRegistry {
    /// Version of `racoon` that wrote this cache.
    pub app_version: String,
    /// Last update timestamp (RFC3339).
    pub updated_at: String,
    /// Models indexed by `ProviderKind::id()`.
    pub providers: HashMap<String, Vec<ModelInfo>>,
}

impl Default for ModelsRegistry {
    fn default() -> Self {
        Self::default_builtin()
    }
}

impl ModelsRegistry {
    /// Construct a fresh registry populated with built-in curated models for all providers.
    pub fn default_builtin() -> Self {
        let mut providers = HashMap::new();
        for kind in ProviderKind::all() {
            providers.insert(kind.id().to_string(), builtin_models(*kind));
        }

        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            updated_at: Utc::now().to_rfc3339(),
            providers,
        }
    }

    /// Path to `~/.config/racoon/models_cache.json`.
    pub fn cache_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("racoon")
            .join("models_cache.json")
    }

    /// Load the model registry from disk, or initialize/overwrite it if missing or
    /// if the binary version (`CARGO_PKG_VERSION`) has changed.
    pub fn load() -> Self {
        let path = Self::cache_path();
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(mut reg) = serde_json::from_str::<ModelsRegistry>(&content)
        {
            let current_version = env!("CARGO_PKG_VERSION");
            if reg.app_version == current_version {
                // Ensure all 4 providers exist in the map
                for kind in ProviderKind::all() {
                    reg.providers
                        .entry(kind.id().to_string())
                        .or_insert_with(|| builtin_models(*kind));
                }
                return reg;
            }
            debug!(
                cached_version = %reg.app_version,
                current_version = %current_version,
                "Binary version changed; overwriting models cache with updated baseline"
            );
        }

        let reg = Self::default_builtin();
        let _ = reg.save();
        reg
    }

    /// Save the model registry to `~/.config/racoon/models_cache.json`.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = Self::cache_path();
        self.save_to_file(&path)?;
        Ok(path)
    }

    /// Save the model registry to a specific path.
    pub fn save_to_file(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Return available models for a given provider.
    pub fn models_for(&self, provider: ProviderKind) -> Vec<ModelInfo> {
        self.providers
            .get(provider.id())
            .cloned()
            .unwrap_or_else(|| builtin_models(provider))
    }

    /// Refresh model lists from remote provider APIs (when API keys are available)
    /// and overwrite the local cache.
    ///
    /// When `reset_baseline` is true (e.g., during `racoon update`), all providers are
    /// first reset to the latest built-in baseline before remote updates are applied.
    pub async fn refresh_from_remote(
        &mut self,
        config: &Config,
        reset_baseline: bool,
    ) -> Vec<ProviderUpdateStatus> {
        if reset_baseline {
            *self = Self::default_builtin();
        }

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut statuses = Vec::new();

        for &kind in ProviderKind::all() {
            let base_url = config
                .get_provider_base_url(kind.id())
                .unwrap_or_else(|| kind.default_base_url().to_string());

            if let Some(api_key) = config.get_provider_api_key(kind.id()) {
                match fetch_provider_models(&http, kind, &base_url, &api_key).await {
                    Ok(remote_models) if !remote_models.is_empty() => {
                        let merged = merge_with_builtin(kind, remote_models);
                        let count = merged.len();
                        self.providers.insert(kind.id().to_string(), merged);
                        statuses.push(ProviderUpdateStatus {
                            provider: kind,
                            count,
                            source: "remote API (live synced)".to_string(),
                        });
                        continue;
                    }
                    Ok(_) => {
                        let fallback = builtin_models(kind);
                        let count = fallback.len();
                        self.providers.insert(kind.id().to_string(), fallback);
                        statuses.push(ProviderUpdateStatus {
                            provider: kind,
                            count,
                            source: "built-in baseline (remote returned empty)".to_string(),
                        });
                    }
                    Err(e) => {
                        warn!(provider = kind.id(), error = %e, "Failed to fetch remote models, using built-in baseline");
                        let fallback = builtin_models(kind);
                        let count = fallback.len();
                        self.providers.insert(kind.id().to_string(), fallback);
                        statuses.push(ProviderUpdateStatus {
                            provider: kind,
                            count,
                            source: format!("built-in baseline (API fallback: {})", e),
                        });
                    }
                }
            } else {
                let fallback = builtin_models(kind);
                let count = fallback.len();
                self.providers.insert(kind.id().to_string(), fallback);
                let source = if kind == ProviderKind::VertexAi {
                    "built-in baseline (uses GCP ADC / IAM credentials)".to_string()
                } else {
                    format!("built-in baseline (set {} for live sync)", kind.env_var())
                };
                statuses.push(ProviderUpdateStatus {
                    provider: kind,
                    count,
                    source,
                });
            }
        }

        self.app_version = env!("CARGO_PKG_VERSION").to_string();
        self.updated_at = Utc::now().to_rfc3339();
        let _ = self.save();

        statuses
    }
}

/// Built-in curated model lists for each provider.
pub fn builtin_models(provider: ProviderKind) -> Vec<ModelInfo> {
    match provider {
        ProviderKind::Google | ProviderKind::VertexAi => vec![
            ModelInfo::new(
                "gemini-3.8-flash",
                "Gemini 3.8 Flash",
                "Most intelligent Flash model for engineering, autonomous agents & workflows (default)",
            ),
            ModelInfo::new(
                "gemini-4-argon",
                "Gemini 4 Argon",
                "Flagship Gemini 4 reasoning model with 1M context window & 1M long decode",
            ),
            ModelInfo::new(
                "gemini-3.6-flash",
                "Gemini 3.6 Flash",
                "Balanced speed and multimodal capabilities across general agentic tasks",
            ),
            ModelInfo::new(
                "gemini-3.5-flash-lite",
                "Gemini 3.5 Flash-Lite",
                "Fastest, most cost-effective 3.5 model for high-throughput execution",
            ),
            ModelInfo::new(
                "gemini-3.1-pro-preview",
                "Gemini 3.1 Pro Preview",
                "Advanced Gemini 3.1 reasoning and log root-cause analysis preview model",
            ),
            ModelInfo::new(
                "gemini-3.1-flash-lite",
                "Gemini 3.1 Flash-Lite",
                "Frontier-class performance rivaling larger models at a fraction of the cost",
            ),
            ModelInfo::new(
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "Frontier-class preview model for low-latency log diagnostics",
            ),
            ModelInfo::new(
                "gemini-2.5-pro",
                "Gemini 2.5 Pro",
                "Deep reasoning and coding model in the 2.5 family",
            ),
            ModelInfo::new(
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                "Price-performance model for low-latency reasoning tasks in the 2.5 family",
            ),
        ],
        ProviderKind::OpenAi => vec![
            ModelInfo::new(
                "gpt-4.1",
                "GPT-4.1",
                "Flagship OpenAI model for precise query synthesis & SRE debugging",
            ),
            ModelInfo::new(
                "gpt-4.1-mini",
                "GPT-4.1 Mini",
                "Fast, cost-efficient model for real-time log explanation",
            ),
            ModelInfo::new(
                "gpt-4o",
                "GPT-4o",
                "Omni flagship model for general log Q&A and incident triage",
            ),
            ModelInfo::new(
                "gpt-4o-mini",
                "GPT-4o Mini",
                "Lightweight model for rapid filter generation",
            ),
            ModelInfo::new(
                "o3-mini",
                "o3-mini",
                "Reasoning model for multi-service failure correlation",
            ),
        ],
        ProviderKind::Anthropic => vec![
            ModelInfo::new(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                "Top-tier engineering & SRE incident analysis model",
            ),
            ModelInfo::new(
                "claude-opus-4-1",
                "Claude Opus 4.1",
                "Deepest reasoning for complex production outages",
            ),
            ModelInfo::new(
                "claude-3-7-sonnet-latest",
                "Claude 3.7 Sonnet",
                "Hybrid reasoning model for stack traces & GCP filters",
            ),
            ModelInfo::new(
                "claude-3-5-haiku-latest",
                "Claude 3.5 Haiku",
                "Ultra-fast log explanation and natural language filter builder",
            ),
        ],
        ProviderKind::Agnes => vec![
            ModelInfo::new(
                "agnes-3.0-pro",
                "Agnes 3.0 Pro",
                "Flagship Agnes AI model for deep reasoning & multi-step SRE tasks",
            ),
            ModelInfo::new(
                "agnes-2.5-pro-alpha",
                "Agnes 2.5 Pro Alpha",
                "High-capability reasoning & log comprehension model",
            ),
            ModelInfo::new(
                "echo",
                "Agnes Echo",
                "Fast interactive conversational & filter generation model",
            ),
            ModelInfo::new(
                "pavo",
                "Agnes Pavo",
                "Lightweight model tuned for low-latency log inspection",
            ),
        ],
    }
}

/// Merge remotely discovered models with built-in curated descriptions, keeping built-in models
/// prioritized at the top and appending newly discovered remote models.
fn merge_with_builtin(provider: ProviderKind, remote: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut result = builtin_models(provider);
    for m in remote {
        if !result.iter().any(|existing| existing.id == m.id) {
            result.push(m);
        }
    }
    result
}

/// Fetch available models from a provider's live `/models` endpoint.
async fn fetch_provider_models(
    http: &Client,
    provider: ProviderKind,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, String> {
    let base = base_url.trim_end_matches('/');
    match provider {
        ProviderKind::VertexAi => Ok(builtin_models(ProviderKind::VertexAi)),
        ProviderKind::Google => {
            let url = format!("{}?key={}", base, api_key);
            let resp = http
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("HTTP error: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status().as_u16()));
            }
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("JSON error: {}", e))?;
            let mut models = Vec::new();
            if let Some(arr) = body.get("models").and_then(|v| v.as_array()) {
                for item in arr {
                    let raw_name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let id = raw_name.strip_prefix("models/").unwrap_or(raw_name);
                    if !id.starts_with("gemini-")
                        || id.contains("embedding")
                        || id.contains("imagen")
                        || id.contains("image")
                        || id.contains("nano-banana")
                        || id.contains("tts")
                        || id.contains("live")
                        || id.contains("transcribe")
                        || id.contains("translate")
                        || id.contains("omni")
                        || id.contains("robotics")
                        || id.contains("computer-use")
                        || id.contains("aqa")
                        || id.starts_with("gemini-2.0-")
                    {
                        continue;
                    }
                    let methods = item
                        .get("supportedGenerationMethods")
                        .and_then(|v| v.as_array());
                    let supports_generate = methods
                        .map(|m| m.iter().any(|v| v.as_str() == Some("generateContent")))
                        .unwrap_or(true);
                    if !supports_generate {
                        continue;
                    }
                    let display_name = item
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id);
                    let description = item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Google Gemini model");
                    models.push(ModelInfo::new(
                        id,
                        display_name,
                        &truncate_desc(description),
                    ));
                }
            }
            Ok(models)
        }
        ProviderKind::OpenAi | ProviderKind::Agnes => {
            let url = format!("{}/models", base);
            let resp = http
                .get(&url)
                .bearer_auth(api_key)
                .send()
                .await
                .map_err(|e| format!("HTTP error: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status().as_u16()));
            }
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("JSON error: {}", e))?;
            let mut models = Vec::new();
            if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
                for item in arr {
                    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if id.is_empty() {
                        continue;
                    }
                    if provider == ProviderKind::OpenAi && !is_openai_chat_model(id) {
                        continue;
                    }
                    let label = if provider == ProviderKind::Agnes {
                        "Agnes AI model"
                    } else {
                        "OpenAI chat/reasoning model"
                    };
                    models.push(ModelInfo::new(id, id, label));
                }
            }
            Ok(models)
        }
        ProviderKind::Anthropic => {
            let url = format!("{}/models", base);
            let resp = http
                .get(&url)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
                .map_err(|e| format!("HTTP error: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status().as_u16()));
            }
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("JSON error: {}", e))?;
            let mut models = Vec::new();
            if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
                for item in arr {
                    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if !id.starts_with("claude-") {
                        continue;
                    }
                    let display_name = item
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id);
                    models.push(ModelInfo::new(id, display_name, "Anthropic Claude model"));
                }
            }
            Ok(models)
        }
    }
}

fn is_openai_chat_model(id: &str) -> bool {
    let lower = id.to_lowercase();
    let valid_prefix = lower.starts_with("gpt-4")
        || lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("chatgpt-");
    let excluded = lower.contains("audio")
        || lower.contains("realtime")
        || lower.contains("tts")
        || lower.contains("whisper")
        || lower.contains("embedding")
        || lower.contains("dall-e")
        || lower.contains("moderation")
        || lower.contains("transcribe")
        || lower.contains("image");
    valid_prefix && !excluded
}

fn truncate_desc(s: &str) -> String {
    let clean = s.replace(['\n', '\r'], " ");
    if clean.chars().count() > 80 {
        let truncated: String = clean.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_provider_kind_parsing() {
        assert_eq!(ProviderKind::parse("google"), ProviderKind::Google);
        assert_eq!(ProviderKind::parse("gemini"), ProviderKind::Google);
        assert_eq!(ProviderKind::parse("vertex"), ProviderKind::VertexAi);
        assert_eq!(ProviderKind::parse("vertexai"), ProviderKind::VertexAi);
        assert_eq!(ProviderKind::parse("vertex-ai"), ProviderKind::VertexAi);
        assert_eq!(ProviderKind::parse("openai"), ProviderKind::OpenAi);
        assert_eq!(ProviderKind::parse("gpt"), ProviderKind::OpenAi);
        assert_eq!(ProviderKind::parse("anthropic"), ProviderKind::Anthropic);
        assert_eq!(ProviderKind::parse("claude"), ProviderKind::Anthropic);
        assert_eq!(ProviderKind::parse("agnes"), ProviderKind::Agnes);
    }

    #[test]
    fn test_builtin_models_for_all_providers() {
        let reg = ModelsRegistry::default_builtin();
        for &kind in ProviderKind::all() {
            let models = reg.models_for(kind);
            assert!(
                !models.is_empty(),
                "Provider {:?} should have builtin models",
                kind
            );
        }
        let google_models = reg.models_for(ProviderKind::Google);
        assert!(google_models.iter().any(|m| m.id == "gemini-4-argon"));
        assert!(google_models.iter().any(|m| m.id == "gemini-3.8-flash"));
        assert!(
            google_models
                .iter()
                .any(|m| m.id == "gemini-3.5-flash-lite")
        );
        assert!(
            reg.models_for(ProviderKind::Agnes)
                .iter()
                .any(|m| m.id == "agnes-3.0-pro")
        );
    }

    #[test]
    fn test_registry_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("models_cache.json");

        let mut reg = ModelsRegistry::default_builtin();
        reg.providers.insert(
            "agnes".to_string(),
            vec![ModelInfo::new(
                "agnes-custom-1",
                "Agnes Custom 1",
                "Custom test model",
            )],
        );
        reg.save_to_file(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: ModelsRegistry = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            loaded.models_for(ProviderKind::Agnes)[0].id,
            "agnes-custom-1"
        );
    }
}
