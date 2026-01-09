//! Gemini API client implementation.
//!
//! Handles communication with Google's Gemini API for AI-powered
//! log analysis and query generation.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug};

use super::prompts;
use crate::gcp::{LogEntry, LogFilter, LogsClient};

/// Gemini API endpoint for content generation.

const GEMINI_API_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-flash-preview:generateContent";

/// Errors that can occur during AI operations.
#[derive(Error, Debug)]
pub enum AiError {
    /// API key not found in environment or config.
    #[error("Gemini API key not found. Set GEMINI_API_KEY environment variable or configure in ~/.config/racoon/config.toml")]
    NoApiKey,

    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to parse API response.
    #[error("Failed to parse response: {0}")]
    ParseError(String),

    /// API returned an error.
    #[error("Gemini API error: {0}")]
    ApiError(String),

    /// Rate limit exceeded.
    #[error("Rate limit exceeded. Please wait and try again.")]
    RateLimited,

    /// Invalid API key.
    #[error("Invalid API key. Check your GEMINI_API_KEY.")]
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

/// Client for interacting with Google's Gemini API.
///
/// Provides methods for log explanation and query generation.
#[derive(Clone)]
pub struct GeminiClient {
    /// HTTP client for API requests
    http: Client,
    /// Gemini API key
    api_key: String,
}

impl GeminiClient {
    /// Create a new GeminiClient with the provided API key.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The Gemini API key
    ///
    /// # Example
    ///
    /// ```rust
    /// use racoon::ai::GeminiClient;
    ///
    /// let client = GeminiClient::new("your-api-key".to_string());
    /// ```
    pub fn new(api_key: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
        }
    }

    /// Create a GeminiClient from environment variable or config file.
    ///
    /// Looks for the API key in the following order:
    /// 1. `GEMINI_API_KEY` environment variable
    /// 2. `~/.config/racoon/config.toml` file
    ///
    /// # Errors
    ///
    /// Returns `AiError::NoApiKey` if no API key is found.
    pub fn from_env() -> Result<Self, AiError> {
        let api_key = get_api_key().ok_or(AiError::NoApiKey)?;
        Ok(Self::new(api_key))
    }

    /// Check if an API key is available.
    ///
    /// This is a quick check without actually validating the key.
    pub fn is_available() -> bool {
        get_api_key().is_some()
    }

    /// Explain a log entry using AI.
    ///
    /// Analyzes the log entry and provides:
    /// - What the log entry means
    /// - Potential impact
    /// - Suggested troubleshooting steps (for errors)
    ///
    /// # Arguments
    ///
    /// * `entry` - The log entry to explain
    ///
    /// # Returns
    ///
    /// A human-readable explanation of the log entry.
    pub async fn explain_log(&self, entry: &LogEntry) -> Result<String, AiError> {
        let log_json = serde_json::to_string_pretty(entry)
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let prompt = prompts::explain_log_prompt(&log_json);

        debug!("Explaining log entry: {}", entry.insert_id.as_deref().unwrap_or("unknown"));

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
        let json: serde_json::Value = serde_json::from_str(
            response.trim().trim_start_matches("```json").trim_end_matches("```")
        ).map_err(|e| AiError::ParseError(format!("Failed to parse intent: {}", e)))?;

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
    ///
    /// Converts a natural language description into a valid GCP
    /// Logging query string.
    ///
    /// # Arguments
    ///
    /// * `description` - Natural language description of what to search for
    ///
    /// # Returns
    ///
    /// A GCP Logging query string.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use racoon::ai::GeminiClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = GeminiClient::from_env()?;
    /// let query = client.generate_query("errors from Cloud Run in the last hour").await?;
    /// // Returns something like: resource.type="cloud_run_revision" severity>=ERROR timestamp>="2024-01-01T00:00:00Z"
    /// # Ok(())
    /// # }
    /// ```
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
    ///
    /// Looks at a set of logs and tries to identify the root cause
    /// of any issues. Limited to 10 logs to avoid token limits.
    ///
    /// # Arguments
    ///
    /// * `entries` - The log entries to analyze (should be related/sequential)
    ///
    /// # Returns
    ///
    /// An analysis of the logs including potential root cause.
    pub async fn analyze_logs(&self, entries: &[LogEntry]) -> Result<String, AiError> {
        if entries.is_empty() {
            return Ok("No logs to analyze.".to_string());
        }

        // Serialize logs (limit to avoid token limits)
        let logs_json: Vec<String> = entries
            .iter()
            .take(10)  // Limit to 10 logs to avoid token limits
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect();

        let prompt = prompts::analyze_logs_prompt(&logs_json.join("\n---\n"));

        debug!("Analyzing {} log entries", entries.len().min(10));

        self.generate(&prompt).await
    }

    /// Ask a question about the logs and get a human-readable answer.
    ///
    /// This method performs a complete Q&A flow:
    /// 1. Generates a GCP Logging query based on the question
    /// 2. Fetches matching logs from GCP
    /// 3. Analyzes the logs with AI
    /// 4. Returns a human-readable answer
    pub async fn ask_question(
        &self,
        question: &str,
        logs_client: &LogsClient,
    ) -> Result<QuestionAnswer, AiError> {
        tracing::info!(question = question, "Processing AI question");

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
                // If the AI-generated query fails, try a simpler fallback
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
        // Format logs for the prompt
        let logs_summary = if entries.is_empty() {
            "No matching logs found.".to_string()
        } else {
            entries
                .iter()
                .take(20) // Limit to avoid token limits
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
    ///
    /// Convenience method for common "are there any errors?" questions.
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

    /// Generate content using the Gemini API.
    ///
    /// This is the core method that sends prompts to Gemini.
    /// The default generation config setting are
    /// - temperature: 0.3
    /// - max_output_tokens: 1024
    /// - top_p: 0.8
    /// - top_k: 40
    async fn generate(&self, prompt: &str) -> Result<String, AiError> {
        let url = format!("{}?key={}", GEMINI_API_URL, self.api_key);

        let request_body = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.3),  // Lower temperature for more consistent output
                max_output_tokens: Some(1024),
                top_p: Some(0.8),
                top_k: Some(40),
            }),
        };

        let response = self.http
            .post(&url)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();

        // Handle specific error codes
        if status == 429 {
            return Err(AiError::RateLimited);
        }

        if status == 400 || status == 401 || status == 403 {
            let error_body = response.text().await.unwrap_or_default();
            if error_body.contains("API_KEY_INVALID") {
                return Err(AiError::InvalidApiKey);
            }
            return Err(AiError::ApiError(format!("HTTP {}: {}", status, error_body)));
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!("HTTP {}: {}", status, error_body)));
        }

        // Parse response
        let response_body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        // Extract text from response
        let text = response_body
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or_else(|| AiError::ParseError("No content in response".to_string()))?;

        Ok(text)
    }
}

/// Get API key from environment or config file.
fn get_api_key() -> Option<String> {
    // 1. Try environment variable first
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            debug!("Using API key from GEMINI_API_KEY environment variable");
            return Some(key);
        }
    }

    // 2. Try config file
    let config_path = dirs::config_dir()?.join("racoon/config.toml");

    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            // Simple TOML parsing for api_key = "..."
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("gemini_api_key") || line.starts_with("api_key") {
                    if let Some(value) = line.split('=').nth(1) {
                        let key = value.trim().trim_matches('"').trim_matches('\'');
                        if !key.is_empty() {
                            debug!("Using API key from {:?}", config_path);
                            return Some(key.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

// ============================================================================
// Gemini API Request/Response types
// ============================================================================
/// Request body for Gemini API.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

/// Content block in Gemini request/response.
#[derive(Debug, Serialize, Deserialize)]
struct Content {
    parts: Vec<Part>,
}

/// Part of content (text).
#[derive(Debug, Serialize, Deserialize)]
struct Part {
    text: String,
}

/// Generation configuration for Gemini.
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

/// Response from Gemini API.
#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

/// Candidate response from Gemini.
#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

// ============================================================================
// Helper functions (outside impl block)
// ============================================================================

/// Sanitize an AI-generated query to make it valid for GCP Logging API.
fn sanitize_query(query: &str) -> String {
    let mut result = query.trim().to_string();

    // Remove Markdown code blocks if present
    result = result
        .trim_start_matches("```")
        .trim_start_matches("gcp")
        .trim_start_matches("sql")
        .trim_end_matches("```")
        .trim()
        .to_string();

    // Remove any newlines (query should be single line)
    result = result.replace('\n', " ").replace('\r', " ");

    // Collapse multiple spaces
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    // Convert patterns like timestamp>="-2h" to timestamp>="2026-01-09T16:00:00Z"
    result = fix_relative_timestamps(&result);

    result
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

/// Convert relative time patterns to RFC3339 timestamps.
///
/// Handles patterns like:
/// - "-2h" -> actual timestamp 2 hours ago
/// - "-1d" -> actual timestamp 1 day ago
/// - "-30m" -> actual timestamp 30 minutes ago
fn fix_relative_timestamps(query: &str) -> String {
    use chrono::{Duration, Utc};

    let mut result = query.to_string();

    // Pattern: timestamp>="-Xh" or timestamp>="-Xd" or timestamp>="-Xm"
    // Match quoted relative times
    let patterns = [
        (r#""-(\d+)h""#, "hours"),
        (r#""-(\d+)d""#, "days"),
        (r#""-(\d+)m""#, "minutes"),
        (r#""-(\d+)s""#, "seconds"),
    ];

    for (pattern, unit) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            let replacements: Vec<(String, String)> = re
                .captures_iter(&result.clone())  // Clone to avoid borrow conflict
                .filter_map(|captures| {
                    let num_match = captures.get(1)?;
                    let num: i64 = num_match.as_str().parse().ok()?;

                    let duration = match unit {
                        "hours" => Duration::hours(num),
                        "days" => Duration::days(num),
                        "minutes" => Duration::minutes(num),
                        "seconds" => Duration::seconds(num),
                        _ => return None,
                    };

                    let timestamp = Utc::now() - duration;
                    let rfc3339 = timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string();

                    let old_value = captures.get(0)?.as_str().to_string();
                    let new_value = format!("\"{}\"", rfc3339);

                    Some((old_value, new_value))
                })
                .collect();

            // Now apply replacements
            for (old_value, new_value) in replacements {
                debug!(
                    old = %old_value,
                    new = %new_value,
                    "Converted relative timestamp"
                );
                result = result.replace(&old_value, &new_value);
            }
        }
    }

    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GeminiClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
    }

    #[test]
    fn test_is_available_without_key() {
        // This test depends on environment; just ensure it doesn't panic
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