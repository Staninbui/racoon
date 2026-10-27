//! Unified AI client implementation (Google Gemini, OpenAI GPT, Anthropic Claude, Agnes AI).
//!
//! Handles communication with multiple LLM providers for AI-powered
//! log analysis and query generation.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use super::models::ProviderKind;
use super::prompts;
use crate::gcp::{LogEntry, LogFilter, LogsClient};
use crate::utils::sanitize_query;

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
    /// User is asking a question about logs.
    Question { question: String },
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
        }
    }

    /// Return the active provider.
    pub fn provider(&self) -> ProviderKind {
        self.provider
    }

    /// Return the active model identifier.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Create a client from environment variable or config file.
    pub fn from_env() -> Result<Self, AiError> {
        let config = crate::config::Config::load();
        if let Ok(client) = Self::from_config(&config) {
            return Ok(client);
        }
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| AiError::NoApiKey)?;
        Ok(Self::new(api_key))
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
        Ok(Self::with_provider_and_model(
            provider, api_key, model, base_url,
        ))
    }

    /// Check if an API key is available for the active or default provider.
    pub fn is_available() -> bool {
        let config = crate::config::Config::load();
        config.get_api_key().is_some() || get_api_key().is_some()
    }

    /// Explain a log entry using AI.
    pub async fn explain_log(&self, entry: &LogEntry) -> Result<String, AiError> {
        let log_json =
            serde_json::to_string_pretty(entry).map_err(|e| AiError::ParseError(e.to_string()))?;

        let prompt = prompts::explain_log_prompt(&log_json);

        debug!(
            provider = self.provider.id(),
            model = %self.model,
            "Explaining log entry: {}",
            entry.insert_id.as_deref().unwrap_or("unknown")
        );

        self.generate(&prompt).await
    }

    /// Detect user intent from natural language input.
    pub async fn detect_intent(&self, input: &str) -> Result<UserIntent, AiError> {
        let prompt = format!(
            r#"Analyze this user input and determine the intent:

Input: "{input}"

Possible intents:
1. QUESTION - User is asking about logs (e.g., "Are there any errors?", "What happened at 14:00?")
2. SET_FILTER - User wants to change the current filter (e.g., "设置过滤条件为INFO以上", "only show errors", "filter by severity warning")
3. GENERATE_QUERY - User wants to see a query (e.g., "show me the query for...", "what's the query for...")

Response format (JSON only, no explanation):
{{"intent": "QUESTION" | "SET_FILTER" | "GENERATE_QUERY", "gcp_query": "<query if applicable>", "description": "<brief description>"}}

Response:"#,
            input = input
        );

        let response = self.generate(&prompt).await?;

        // Parse JSON response
        let cleaned = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let json: serde_json::Value = serde_json::from_str(cleaned)
            .map_err(|e| AiError::ParseError(format!("Failed to parse intent: {}", e)))?;

        let intent_type = json["intent"].as_str().unwrap_or("QUESTION");
        let gcp_query = json["gcp_query"].as_str().unwrap_or("").to_string();
        let description = json["description"].as_str().unwrap_or("").to_string();

        match intent_type {
            "SET_FILTER" => Ok(UserIntent::SetFilter {
                filter: gcp_query,
                description,
            }),
            "GENERATE_QUERY" => Ok(UserIntent::GenerateQuery { query: gcp_query }),
            _ => Ok(UserIntent::Question {
                question: input.to_string(),
            }),
        }
    }

    /// Generate a GCP Logging query from natural language.
    pub async fn generate_query(&self, description: &str) -> Result<String, AiError> {
        let prompt = prompts::generate_query_prompt(description);

        debug!("Generating query for: {}", description);

        let response = self.generate(&prompt).await?;

        // Clean up the response - remove Markdown code blocks if present
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

    /// Analyze multiple related logs for root cause.
    pub async fn analyze_logs(&self, entries: &[LogEntry]) -> Result<String, AiError> {
        if entries.is_empty() {
            return Ok("No logs to analyze.".to_string());
        }

        // Serialize logs (limit to avoid token limits)
        let logs_json: Vec<String> = entries
            .iter()
            .take(10)
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect();

        let prompt = prompts::analyze_logs_prompt(&logs_json.join("\n---\n"));

        debug!("Analyzing {} log entries", entries.len().min(10));

        self.generate(&prompt).await
    }

    /// Ask a question about the logs and get a human-readable answer.
    pub async fn ask_question(
        &self,
        question: &str,
        logs_client: &LogsClient,
    ) -> Result<QuestionAnswer, AiError> {
        debug!(question = %question, "Processing AI question");

        // Step 1: Generate a query from the question
        let raw_query = self.generate_query(question).await?;
        debug!(raw_query = %raw_query, "Generated raw query");
        let query = sanitize_query(&raw_query);
        debug!(sanitized_query = %query, "Sanitized query");

        // Step 2: Fetch logs using the generated query
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

        // Step 3: Analyze logs and generate answer
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
        let logs_summary = if entries.is_empty() {
            "No matching logs found.".to_string()
        } else {
            entries
                .iter()
                .take(20)
                .map(|e| {
                    format!(
                        "[{}] [{}] {}",
                        e.timestamp_str(),
                        e.severity_or_default(),
                        e.message_preview()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            r#"You are an expert Site Reliability Engineer (SRE) analyzing GCP logs.

## User's Question
"{question}"

## Query Results
Found {count} matching log entries:

```
{logs}
```

## Your Task
Answer the user's question directly and concisely based on the logs above.

Guidelines:
1. Answer in the SAME LANGUAGE as the question (if Chinese, respond in Chinese)
2. Be direct - start with "Yes" or "No" if the question is yes/no
3. If errors are found, briefly explain:
   - What happened
   - When it happened
   - Potential impact
   - Suggested next steps
4. If no relevant logs found, say so clearly
5. Keep the answer concise but informative (max 200 words)

Your answer:"#,
            question = question,
            count = entries.len(),
            logs = logs_summary
        );

        self.generate(&prompt).await
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

    /// Generate content using the configured AI provider (`Google`, `OpenAi`, `Anthropic`, or `Agnes`).
    async fn generate(&self, prompt: &str) -> Result<String, AiError> {
        match self.provider {
            ProviderKind::Google => self.generate_google(prompt).await,
            ProviderKind::OpenAi | ProviderKind::Agnes => {
                self.generate_openai_compatible(prompt).await
            }
            ProviderKind::Anthropic => self.generate_anthropic(prompt).await,
        }
    }

    /// Generate content via Google Gemini REST API (`generateContent`).
    async fn generate_google(&self, prompt: &str) -> Result<String, AiError> {
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
                max_output_tokens: Some(1024),
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
    async fn generate_openai_compatible(&self, prompt: &str) -> Result<String, AiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let request_body = OpenAiChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: Some(0.3),
            max_tokens: Some(1024),
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
    async fn generate_anthropic(&self, prompt: &str) -> Result<String, AiError> {
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let request_body = AnthropicMessagesRequest {
            model: self.model.clone(),
            max_tokens: 1024,
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
}
