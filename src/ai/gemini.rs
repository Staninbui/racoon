//! Unified AI client implementation (Google Gemini, OpenAI GPT, Anthropic Claude, Agnes AI).
//!
//! Handles communication with multiple LLM providers for AI-powered
//! log analysis and query generation.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use super::compact::{compact_log_for_ai, compact_logs_for_rca, try_local_intent};
use super::models::ProviderKind;
use super::prompts;
use super::redact::{RedactionStats, redact_sensitive_text};
use crate::gcp::{LogEntry, LogFilter, LogsClient};
use crate::ui::fingerprint_message;
use crate::utils::sanitize_query;

/// Default model name (Google Gemini).
const DEFAULT_MODEL: &str = "gemini-3.8-flash";

/// Maximum entries retained in the client-side AI response LRU cache.
const AI_CACHE_CAPACITY: usize = 64;

/// Bounded in-memory LRU cache mapping normalized prompt/fingerprint keys to AI responses.
#[derive(Debug, Default)]
struct AiResponseCache {
    entries: VecDeque<(String, String)>,
}

impl AiResponseCache {
    fn get(&mut self, key: &str) -> Option<String> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(pos)?;
        self.entries.push_back((k, v.clone()));
        Some(v)
    }

    fn insert(&mut self, key: String, value: String) {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(pos);
        }
        self.entries.push_back((key, value));
        while self.entries.len() > AI_CACHE_CAPACITY {
            self.entries.pop_front();
        }
    }
}

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

/// Unified AI client supporting Google Gemini, OpenAI GPT, Anthropic Claude, and Agnes AI.
///
/// Named `GeminiClient` (with alias `AiClient`) for backward compatibility across the crate.
#[derive(Clone)]
pub struct GeminiClient {
    /// HTTP client for API requests
    http: Client,
    /// Active AI provider
    provider: ProviderKind,
    /// Provider API key
    api_key: String,
    /// Model name to use
    model: String,
    /// Base URL for the provider API
    base_url: String,
    /// Whether to scrub PII and secrets locally before sending prompts to the LLM
    redact_pii: bool,
    /// Client-side LRU cache for instant 0-token repeat explanations & RCAs
    cache: Arc<Mutex<AiResponseCache>>,
}

/// Alias for `GeminiClient` representing the multi-provider AI client.
pub type AiClient = GeminiClient;

impl GeminiClient {
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
            http: Client::new(),
            provider,
            api_key,
            model,
            base_url: resolved_base_url,
            redact_pii: true,
            cache: Arc::new(Mutex::new(AiResponseCache::default())),
        }
    }

    /// Configure whether Pre-AI PII & Secret redaction is enabled.
    pub fn with_redact_pii(mut self, redact_pii: bool) -> Self {
        self.redact_pii = redact_pii;
        self
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
        let provider = ProviderKind::parse(&config.ai.provider);
        let api_key = config.get_provider_api_key(provider.id()).ok_or_else(|| {
            AiError::NoApiKeyForProvider {
                provider: provider.display_name().to_string(),
                env_var: provider.env_var().to_string(),
            }
        })?;
        let model = config.ai.model.clone();
        let base_url = config.get_provider_base_url(provider.id());
        Ok(
            Self::with_provider_and_model(provider, api_key, model, base_url)
                .with_redact_pii(config.ai.redact_pii),
        )
    }

    /// Check if an API key is available for the active or default provider.
    pub fn is_available() -> bool {
        let config = crate::config::Config::load();
        config.get_api_key().is_some() || get_api_key().is_some()
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

        let prompt = format!(
            r#"Classify user input for GCP Cloud Logging and synthesize a GCP filter query if applicable.
Input: "{input}"
Intents:
- QUESTION: asking about logs (include "gcp_query" to fetch relevant logs)
- SET_FILTER: changing active filter
- GENERATE_QUERY: requesting query syntax
Return JSON only:
{{"intent":"QUESTION"|"SET_FILTER"|"GENERATE_QUERY","gcp_query":"<valid GCP filter>","description":"<short note>"}}"#
        );

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

    /// Generate content using the configured AI provider with a task-specific `max_output_tokens` cap.
    async fn generate_with_max_tokens(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, AiError> {
        match self.provider {
            ProviderKind::Google => self.generate_google(prompt, max_tokens).await,
            ProviderKind::OpenAi | ProviderKind::Agnes => {
                self.generate_openai_compatible(prompt, max_tokens).await
            }
            ProviderKind::Anthropic => self.generate_anthropic(prompt, max_tokens).await,
        }
    }

    /// Generate content via Google Gemini REST API (`generateContent`).
    async fn generate_google(&self, prompt: &str, max_tokens: u32) -> Result<String, AiError> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url.trim_end_matches('/'),
            self.model,
            self.api_key
        );

        let request_body = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.3),
                max_output_tokens: Some(max_tokens),
                top_p: Some(0.8),
                top_k: Some(40),
            }),
        };

        let response = self.http.post(&url).json(&request_body).send().await?;
        let status = response.status();

        if status == 429 {
            return Err(AiError::RateLimited);
        }

        if status == 400 || status == 401 || status == 403 {
            let error_body = response.text().await.unwrap_or_default();
            if error_body.contains("API_KEY_INVALID") {
                return Err(AiError::InvalidApiKey);
            }
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or_else(|| AiError::ParseError("No content in response".to_string()))?;

        Ok(text)
    }

    /// Generate content via OpenAI-compatible `/chat/completions` (used by OpenAI and Agnes AI).
    async fn generate_openai_compatible(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, AiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let request_body = OpenAiChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: Some(0.3),
            max_tokens: Some(max_tokens),
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if status == 429 {
            return Err(AiError::RateLimited);
        }
        if status == 401 || status == 403 {
            return Err(AiError::InvalidApiKey);
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: OpenAiChatResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| AiError::ParseError("No choices in response".to_string()))?;

        Ok(text)
    }

    /// Generate content via Anthropic `/messages` API.
    async fn generate_anthropic(&self, prompt: &str, max_tokens: u32) -> Result<String, AiError> {
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let request_body = AnthropicMessagesRequest {
            model: self.model.clone(),
            max_tokens,
            temperature: Some(0.3),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if status == 429 {
            return Err(AiError::RateLimited);
        }
        if status == 401 || status == 403 {
            return Err(AiError::InvalidApiKey);
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: AnthropicMessagesResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .content
            .iter()
            .find_map(|b| b.text.clone())
            .ok_or_else(|| AiError::ParseError("No text content in response".to_string()))?;

        Ok(text)
    }
}

/// Get API key from environment or config file (legacy helper).
fn get_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("GEMINI_API_KEY")
        && !key.is_empty()
    {
        return Some(key);
    }

    let config_path = dirs::config_dir()?.join("racoon/config.toml");

    if config_path.exists()
        && let Ok(content) = std::fs::read_to_string(&config_path)
    {
        for line in content.lines() {
            let line = line.trim();
            if (line.starts_with("gemini_api_key") || line.starts_with("api_key"))
                && let Some(value) = line.split('=').nth(1)
            {
                let key = value.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }

    None
}

// ============================================================================
// Provider API Request/Response types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Part {
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: ChatMessage,
}

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(default)]
    text: Option<String>,
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

    #[test]
    fn test_get_api_key_from_env() {
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "test-api-key-123");
        }
        let key = get_api_key();
        assert_eq!(key, Some("test-api-key-123".to_string()));
        unsafe {
            std::env::remove_var("GEMINI_API_KEY");
        }
    }

    #[tokio::test]
    async fn test_explain_log_pattern_lru_cache_zero_tokens_on_second_log() {
        let client = GeminiClient::new("dummy-key-never-called".to_string());

        // Seed cache for the normalized pattern of a connection timeout log
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

        // Now explain a DIFFERENT log entry with a different dynamic latency (750ms vs 500ms)
        // and different insert_id ("log-2") — it matches the same normalized fingerprint and hits cache with 0 network calls!
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

        // Even with a dummy API key and no network, `detect_intent` resolves local filter commands in 0 tokens!
        let intent = client.detect_intent("只看错误").await.unwrap();
        assert!(matches!(
            intent,
            UserIntent::SetFilter { ref filter, .. } if filter == "severity>=ERROR"
        ));
    }
}
