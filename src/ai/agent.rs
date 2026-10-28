//! Unified AI agent & orchestrator (Google Gemini, Vertex AI, OpenAI GPT, Anthropic Claude, Agnes AI).
//!
//! Handles high-level SRE workflows: log explanation (`explain_log`), multi-log root-cause
//! analysis (`analyze_logs`), intent classification (`detect_intent`), and natural-language
//! query generation (`generate_query`). Low-level provider HTTP protocols live in [`super::providers`]
//! and client-side LRU caching lives in [`super::cache`].

use std::sync::{Arc, Mutex};

use reqwest::Client;
use thiserror::Error;
use tracing::debug;

use super::cache::AiResponseCache;
use super::compact::{compact_log_for_ai, compact_logs_for_rca, try_local_intent};
use super::models::ProviderKind;
use super::prompts;
use super::redact::{RedactionStats, redact_sensitive_text};
pub use crate::gcp::extract_first_gcp_project_id;
use crate::gcp::{GcpAuth, LogEntry, LogFilter, LogsClient};
use crate::utils::{default_http_client, fingerprint_message, sanitize_query};

/// Default model name (Google Gemini).
const DEFAULT_MODEL: &str = "gemini-3.8-flash";

/// Errors that can occur during AI operations.
#[derive(Error, Debug)]
pub enum AiError {
    /// API key not found in environment or config.
    #[error(
        "AI API key not found. Set GEMINI_API_KEY environment variable or configure in ~/.config/racoon/config.toml"
    )]
    NoApiKey,

    /// Provider-specific API key not found.
    #[error(
        "{provider} API key not found. Set {env_var} environment variable or configure [ai.providers] in ~/.config/racoon/config.toml"
    )]
    NoApiKeyForProvider { provider: String, env_var: String },

    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to parse API response.
    #[error("Failed to parse response: {0}")]
    ParseError(String),

    /// API returned an error.
    #[error("AI API error: {0}")]
    ApiError(String),

    /// Rate limit exceeded.
    #[error("Rate limit exceeded. Please wait and try again.")]
    RateLimited,

    /// Invalid API key.
    #[error("Invalid API key. Check your configured API key.")]
    InvalidApiKey,
}

/// Result of an AI question-answering operation.
#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    /// The GCP query that was generated to find relevant logs.
    pub query: String,
    /// Number of logs found matching the query.
    pub log_count: usize,
    /// The AI's answer to the user's question.
    pub answer: String,
}

/// Types of user intents detected from input.
#[derive(Debug, Clone)]
pub enum UserIntent {
    /// User is asking a question about logs (with an optional pre-generated GCP query to avoid a second LLM call).
    Question {
        question: String,
        gcp_query: Option<String>,
    },
    /// User wants to set a filter.
    SetFilter { filter: String, description: String },
    /// User wants to generate a query (for viewing).
    GenerateQuery { query: String },
}

/// Unified AI agent/client supporting Google Gemini, Vertex AI (ADC), OpenAI GPT, Anthropic Claude, and Agnes AI.
#[derive(Clone)]
pub struct AiClient {
    /// HTTP client for API requests
    pub(crate) http: Client,
    /// Active AI provider
    pub(crate) provider: ProviderKind,
    /// Provider API key (or optional static Bearer token for Vertex AI)
    pub(crate) api_key: String,
    /// Model name to use
    pub(crate) model: String,
    /// Base URL for the provider API
    pub(crate) base_url: String,
    /// Whether to scrub PII and secrets locally before sending prompts to the LLM
    pub(crate) redact_pii: bool,
    /// Optional GCP authentication manager (used by `ProviderKind::VertexAi` for zero-key ADC / impersonation)
    pub(crate) gcp_auth: Option<GcpAuth>,
    /// Target GCP project ID for Vertex AI requests
    pub(crate) vertex_project: Option<String>,
    /// Target GCP region for Vertex AI requests (e.g. `us-central1` or `global`)
    pub(crate) vertex_location: String,
    /// Client-side LRU cache for instant 0-token repeat explanations & RCAs
    pub(crate) cache: Arc<Mutex<AiResponseCache>>,
}

/// Alias for [`AiClient`] emphasizing its role as the high-level AI agent orchestrator.
pub type AiAgent = AiClient;

/// Backward-compatible alias for [`AiClient`].
pub type GeminiClient = AiClient;

impl AiClient {
    /// Create a new client using the default Google Gemini provider and model.
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    /// Create a new Google Gemini client with a specific model.
    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_provider_and_model(ProviderKind::Google, api_key, model, None)
    }

    /// Create a client for any supported provider, model, and optional custom base URL.
    pub fn with_provider_and_model(
        provider: ProviderKind,
        api_key: String,
        model: String,
        base_url: Option<String>,
    ) -> Self {
        let resolved_base_url = base_url
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| provider.default_base_url().to_string());
        Self {
            http: default_http_client(),
            provider,
            api_key,
            model,
            base_url: resolved_base_url,
            redact_pii: true,
            gcp_auth: None,
            vertex_project: None,
            vertex_location: "us-central1".to_string(),
            cache: Arc::new(Mutex::new(AiResponseCache::default())),
        }
    }

    /// Configure whether Pre-AI PII & Secret redaction is enabled.
    pub fn with_redact_pii(mut self, redact_pii: bool) -> Self {
        self.redact_pii = redact_pii;
        self
    }

    /// Attach a `GcpAuth` instance for zero-API-key Vertex AI authentication (supports IAM impersonation).
    pub fn with_gcp_auth(mut self, gcp_auth: Option<GcpAuth>) -> Self {
        self.gcp_auth = gcp_auth;
        self
    }

    /// Configure the target GCP project and location for Vertex AI requests.
    pub fn with_vertex_target(mut self, project: Option<String>, location: String) -> Self {
        self.vertex_project = project.and_then(|p| extract_first_gcp_project_id(&p));
        if !location.trim().is_empty() {
            self.vertex_location = location.trim().to_string();
        }
        self
    }

    /// Return the configured Vertex AI project ID, if any.
    pub fn vertex_project(&self) -> Option<&str> {
        self.vertex_project.as_deref()
    }

    /// Return the configured Vertex AI region/location.
    pub fn vertex_location(&self) -> &str {
        &self.vertex_location
    }

    /// Return whether Pre-AI PII & Secret redaction is enabled.
    pub fn redact_pii(&self) -> bool {
        self.redact_pii
    }

    /// Return the active provider.
    pub fn provider(&self) -> ProviderKind {
        self.provider
    }

    /// Return the active model identifier.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Look up a cached AI response by key (`0` tokens used on hit).
    pub fn get_cached_response(&self, key: &str) -> Option<String> {
        self.cache.lock().ok()?.get(key)
    }

    /// Insert a response into the client-side LRU cache.
    pub fn put_cached_response(&self, key: String, value: String) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(key, value);
        }
    }

    /// Create a client from environment variable or config file.
    pub fn from_env() -> Result<Self, AiError> {
        let config = crate::config::Config::load();
        if let Ok(client) = Self::from_config(&config) {
            return Ok(client);
        }
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| AiError::NoApiKey)?;
        Ok(Self::new(api_key).with_redact_pii(config.ai.redact_pii))
    }

    /// Create client from config (checks provider-specific env var first, then config file).
    pub fn from_config(config: &crate::config::Config) -> Result<Self, AiError> {
        Self::from_config_with_gcp(config, None, None)
    }

    /// Create client from config with optional shared `GcpAuth` and CLI `--project` context.
    ///
    /// When `provider == ProviderKind::VertexAi`, no external API key is required; the client
    /// authenticates via GCP Application Default Credentials (or `--impersonate-service-account`).
    pub fn from_config_with_gcp(
        config: &crate::config::Config,
        gcp_auth: Option<GcpAuth>,
        cli_project: Option<&str>,
    ) -> Result<Self, AiError> {
        let provider = ProviderKind::parse(&config.ai.provider);
        let model = config.ai.model.clone();
        let base_url = config.get_provider_base_url(provider.id());

        if provider == ProviderKind::VertexAi {
            let api_key = config.get_provider_api_key("vertex").unwrap_or_default();
            let vertex_project = config.get_vertex_project(cli_project);
            let vertex_location = config.get_vertex_location();
            return Ok(
                Self::with_provider_and_model(provider, api_key, model, base_url)
                    .with_redact_pii(config.ai.redact_pii)
                    .with_gcp_auth(gcp_auth)
                    .with_vertex_target(vertex_project, vertex_location),
            );
        }

        let api_key = config.get_provider_api_key(provider.id()).ok_or_else(|| {
            AiError::NoApiKeyForProvider {
                provider: provider.display_name().to_string(),
                env_var: provider.env_var().to_string(),
            }
        })?;
        Ok(
            Self::with_provider_and_model(provider, api_key, model, base_url)
                .with_redact_pii(config.ai.redact_pii)
                .with_gcp_auth(gcp_auth),
        )
    }

    /// Check if an API key (or Vertex AI ADC provider) is available for the active or default provider.
    pub fn is_available() -> bool {
        let config = crate::config::Config::load();
        if ProviderKind::parse(&config.ai.provider) == ProviderKind::VertexAi {
            return true;
        }
        config.get_api_key().is_some()
    }

    /// Explain a log entry using AI (with semantic compaction, PII redaction, and fingerprint LRU cache).
    pub async fn explain_log(&self, entry: &LogEntry) -> Result<String, AiError> {
        let pattern_fp = fingerprint_message(&entry.message());
        let cache_key = format!(
            "explain:{}:{}:{}:{}:{}",
            self.provider.id(),
            self.model,
            entry.severity_or_default(),
            entry.resource_type().unwrap_or("-"),
            pattern_fp
        );

        if let Some(cached) = self.get_cached_response(&cache_key) {
            return Ok(format!(
                "⚡ Cached Analysis (0 tokens used │ matched error pattern)\n\n{}",
                cached
            ));
        }

        let (compacted_json, compaction_stats) = compact_log_for_ai(entry);

        let (payload_json, redact_stats) = if self.redact_pii {
            redact_sensitive_text(&compacted_json)
        } else {
            (compacted_json, RedactionStats::default())
        };

        let prompt = prompts::explain_log_prompt(&payload_json);

        debug!(
            provider = self.provider.id(),
            model = %self.model,
            raw_tokens = compaction_stats.raw_tokens,
            compacted_tokens = compaction_stats.compacted_tokens,
            redactions = redact_stats.total_redactions,
            "Explaining log entry: {}",
            entry.insert_id.as_deref().unwrap_or("unknown")
        );

        let explanation = self.generate_with_max_tokens(&prompt, 450).await?;

        let mut banners = Vec::new();
        if let Some(b) = compaction_stats.badge() {
            banners.push(b);
        }
        if let Some(b) = redact_stats.banner() {
            banners.push(b);
        }

        let formatted = if banners.is_empty() {
            explanation
        } else {
            format!("{}\n\n{}", banners.join("\n"), explanation)
        };

        self.put_cached_response(cache_key, formatted.clone());
        Ok(formatted)
    }

    /// Detect user intent from natural language input (with 0-token local fast-path and single-call query synthesis).
    pub async fn detect_intent(&self, input: &str) -> Result<UserIntent, AiError> {
        if let Some(local_intent) = try_local_intent(input) {
            return Ok(local_intent);
        }

        let prompt = prompts::detect_intent_prompt(input);
        let response = self.generate_with_max_tokens(&prompt, 160).await?;

        let cleaned = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let json: serde_json::Value = serde_json::from_str(cleaned)
            .map_err(|e| AiError::ParseError(format!("Failed to parse intent: {}", e)))?;

        let intent_type = json["intent"].as_str().unwrap_or("QUESTION");
        let gcp_query = json["gcp_query"].as_str().unwrap_or("").trim().to_string();
        let description = json["description"].as_str().unwrap_or("").to_string();

        match intent_type {
            "SET_FILTER" => Ok(UserIntent::SetFilter {
                filter: gcp_query,
                description,
            }),
            "GENERATE_QUERY" => Ok(UserIntent::GenerateQuery { query: gcp_query }),
            _ => Ok(UserIntent::Question {
                question: input.to_string(),
                gcp_query: (!gcp_query.is_empty()).then_some(gcp_query),
            }),
        }
    }

    /// Generate a GCP Logging query from natural language.
    pub async fn generate_query(&self, description: &str) -> Result<String, AiError> {
        let prompt = prompts::generate_query_prompt(description);

        debug!("Generating query for: {}", description);

        let response = self.generate_with_max_tokens(&prompt, 160).await?;

        let query = response
            .trim()
            .trim_start_matches("```")
            .trim_start_matches("gcp")
            .trim_start_matches('\n')
            .trim_end_matches("```")
            .trim()
            .to_string();

        Ok(query)
    }

    /// Analyze multiple related logs for root cause (using Shared Context + RLE pattern folding + PII redaction + cache).
    pub async fn analyze_logs(&self, entries: &[LogEntry]) -> Result<String, AiError> {
        if entries.is_empty() {
            return Ok("No logs to analyze.".to_string());
        }

        let (compacted_timeline, compaction_stats) = compact_logs_for_rca(entries, 15);
        let (scrubbed_timeline, redact_stats) = if self.redact_pii {
            redact_sensitive_text(&compacted_timeline)
        } else {
            (compacted_timeline, RedactionStats::default())
        };

        let cache_key = format!(
            "rca:{}:{}:{}",
            self.provider.id(),
            self.model,
            scrubbed_timeline
        );
        if let Some(cached) = self.get_cached_response(&cache_key) {
            return Ok(format!("⚡ Cached RCA (0 tokens used)\n\n{}", cached));
        }

        let prompt = prompts::analyze_logs_prompt(&scrubbed_timeline);

        debug!(
            raw_entries = compaction_stats.raw_entries,
            unique_events = compaction_stats.unique_events,
            raw_tokens = compaction_stats.raw_tokens,
            compacted_tokens = compaction_stats.compacted_tokens,
            redactions = redact_stats.total_redactions,
            "Analyzing log entries"
        );

        let analysis = self.generate_with_max_tokens(&prompt, 700).await?;

        let mut banners = Vec::new();
        if let Some(b) = compaction_stats.badge() {
            banners.push(b);
        }
        if let Some(b) = redact_stats.banner() {
            banners.push(b);
        }

        let formatted = if banners.is_empty() {
            analysis
        } else {
            format!("{}\n\n{}", banners.join("\n"), analysis)
        };

        self.put_cached_response(cache_key, formatted.clone());
        Ok(formatted)
    }

    /// Ask a question about the logs and get a human-readable answer.
    pub async fn ask_question(
        &self,
        question: &str,
        logs_client: &LogsClient,
    ) -> Result<QuestionAnswer, AiError> {
        self.ask_question_with_query(question, None, logs_client)
            .await
    }

    /// Ask a question reusing an optional pre-generated `gcp_query` from `detect_intent` (saves 1 full LLM call).
    pub async fn ask_question_with_query(
        &self,
        question: &str,
        pregenerated_query: Option<&str>,
        logs_client: &LogsClient,
    ) -> Result<QuestionAnswer, AiError> {
        debug!(question = %question, "Processing AI question");

        let raw_query = match pregenerated_query.filter(|q| !q.trim().is_empty()) {
            Some(q) => q.to_string(),
            None => self.generate_query(question).await?,
        };
        let query = sanitize_query(&raw_query);
        debug!(sanitized_query = %query, "Sanitized query");

        let entries = match fetch_logs_with_fallback(logs_client, &query).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(error = %e, "Query failed, trying fallback");
                let fallback_filter = LogFilter::new()
                    .severity_min("WARNING")
                    .since(chrono::Duration::hours(24));
                logs_client
                    .list_entries(Some(&fallback_filter), 50)
                    .await
                    .map_err(|e| AiError::ApiError(format!("Fallback query also failed: {}", e)))?
            }
        };

        let log_count = entries.len();
        debug!(log_count = log_count, "Fetched logs");

        let answer = self.analyze_and_answer(question, &entries).await?;

        Ok(QuestionAnswer {
            query,
            log_count,
            answer,
        })
    }

    /// Analyze fetched logs and formulate an answer to the question.
    async fn analyze_and_answer(
        &self,
        question: &str,
        entries: &[LogEntry],
    ) -> Result<String, AiError> {
        let (raw_summary, compaction_stats) = if entries.is_empty() {
            ("No matching logs found.".to_string(), Default::default())
        } else {
            compact_logs_for_rca(entries, 15)
        };

        let (logs_summary, stats) = if self.redact_pii {
            redact_sensitive_text(&raw_summary)
        } else {
            (raw_summary, RedactionStats::default())
        };

        let prompt = prompts::answer_question_prompt(question, &logs_summary, entries.len());

        let answer = self.generate_with_max_tokens(&prompt, 450).await?;
        let mut banners = Vec::new();
        if let Some(b) = compaction_stats.badge() {
            banners.push(b);
        }
        if let Some(b) = stats.banner() {
            banners.push(b);
        }
        if banners.is_empty() {
            Ok(answer)
        } else {
            Ok(format!("{}\n\n{}", banners.join("\n"), answer))
        }
    }

    /// Quick check for errors in a time range.
    pub async fn check_for_errors(
        &self,
        logs_client: &LogsClient,
        hours_back: u32,
    ) -> Result<QuestionAnswer, AiError> {
        let question = format!(
            "Are there any critical or error level logs in the last {} hours? \
             If yes, summarize what happened.",
            hours_back
        );
        self.ask_question(&question, logs_client).await
    }
}

/// Try to fetch logs with the given query, handling errors gracefully.
async fn fetch_logs_with_fallback(
    logs_client: &LogsClient,
    query: &str,
) -> Result<Vec<LogEntry>, String> {
    let filter = LogFilter::new().raw(query);
    logs_client
        .list_entries(Some(&filter), 50)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GeminiClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.provider(), ProviderKind::Google);
    }

    #[test]
    fn test_multi_provider_client_creation() {
        let client = GeminiClient::with_provider_and_model(
            ProviderKind::Agnes,
            "agnes-key".to_string(),
            "agnes-3.0-pro".to_string(),
            None,
        );
        assert_eq!(client.provider(), ProviderKind::Agnes);
        assert_eq!(client.model(), "agnes-3.0-pro");
        assert_eq!(client.base_url, "https://apihub.agnes-ai.com/v1");
    }

    #[test]
    fn test_is_available_without_key() {
        let _ = GeminiClient::is_available();
    }

    #[tokio::test]
    async fn test_explain_log_pattern_lru_cache_zero_tokens_on_second_log() {
        let client = GeminiClient::new("dummy-key-never-called".to_string());

        let sample_entry_1 = LogEntry {
            insert_id: Some("log-1".to_string()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Timeout calling payment-svc:8080 after 500ms".to_string()),
            ..Default::default()
        };
        let fp = fingerprint_message(&sample_entry_1.message());
        let cache_key = format!(
            "explain:{}:{}:{}:{}:{}",
            client.provider().id(),
            client.model(),
            sample_entry_1.severity_or_default(),
            sample_entry_1.resource_type().unwrap_or("-"),
            fp
        );
        client.put_cached_response(cache_key, "Root cause: payment-svc timeout".to_string());

        let sample_entry_2 = LogEntry {
            insert_id: Some("log-2".to_string()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Timeout calling payment-svc:8080 after 750ms".to_string()),
            ..Default::default()
        };
        let result = client.explain_log(&sample_entry_2).await.unwrap();
        assert!(result.contains("⚡ Cached Analysis (0 tokens used"));
        assert!(result.contains("Root cause: payment-svc timeout"));
    }

    #[tokio::test]
    async fn test_detect_intent_zero_token_local_fast_path() {
        let client = GeminiClient::new("dummy-key-never-called".to_string());

        let intent = client.detect_intent("只看错误").await.unwrap();
        assert!(matches!(
            intent,
            UserIntent::SetFilter { ref filter, .. } if filter == "severity>=ERROR"
        ));
    }

    #[test]
    fn test_vertex_ai_zero_key_client_and_endpoint_urls() {
        let mut config = crate::config::Config::default();
        config.set_active_model("vertex", "gemini-3.8-flash");
        config.ai.providers.vertex.location = Some("us-central1".to_string());

        let client =
            GeminiClient::from_config_with_gcp(&config, None, Some("prod-apigw,prod-order"))
                .expect("Vertex AI client should initialize without an external API key");
        assert_eq!(client.provider(), ProviderKind::VertexAi);
        assert_eq!(client.vertex_project(), Some("prod-apigw"));
        assert_eq!(client.vertex_location(), "us-central1");
        assert_eq!(
            client.vertex_endpoint_url().unwrap(),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/prod-apigw/locations/us-central1/publishers/google/models/gemini-3.8-flash:generateContent"
        );

        let global_client = client.with_vertex_target(
            Some(
                "projects/sec-audit-prod/locations/global/buckets/_Default/views/_AllLogs"
                    .to_string(),
            ),
            "global".to_string(),
        );
        assert_eq!(global_client.vertex_project(), Some("sec-audit-prod"));
        assert_eq!(
            global_client.vertex_endpoint_url().unwrap(),
            "https://aiplatform.googleapis.com/v1/projects/sec-audit-prod/locations/global/publishers/google/models/gemini-3.8-flash:generateContent"
        );
    }
}
