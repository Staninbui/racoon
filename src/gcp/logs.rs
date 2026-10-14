//! GCP Cloud Logging API client.
//!
//! This module provides functionality to query and stream logs from
//! Google Cloud Logging using the REST API v2.
//!
//! # API Reference
//!
//! - [entries.list](https://cloud.google.com/logging/docs/reference/v2/rest/v2/entries/list)

use super::auth::GcpAuth;
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Cloud logging API base URL.
const LOGGING_API_URL: &str = "https://logging.googleapis.com/v2/entries:list";

/// Default page size for log queries.
const DEFAULT_PAGE_SIZE: usize = 100;

/// Polling interval for streaming logs (in seconds).
const POLL_INTERVAL_SECS: u64 = 2;

/// Errors that can occur when interacting with Cloud Logging API.
#[derive(Error, Debug)]
pub enum LogsError {
    /// HTTP request field
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to parse API response
    #[error("Failed to parse response: {0}")]
    ParseError(String),

    /// Authentication error during API call
    #[error("Authentication error: {0}")]
    AuthError(#[from] super::auth::AuthError),

    /// API returned an error response
    #[error("API error ({code}): {message}")]
    ApiError { code: u16, message: String },

    /// Invalid filter syntax
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),

    /// Channel send error (for streaming)
    #[error("Failed to send log entry: channel closed")]
    ChannelClosed,
}

/// Client for interacting with GCP logging API.
#[derive(Clone)]
pub struct LogsClient {
    /// Http client for API requests.
    http: Client,
    /// GCP authentication manager.
    auth: GcpAuth,
    /// Target GCP project ID.
    project: String,
}

impl LogsClient {
    /// Create a new LogsClient for the specified project.
    pub fn new(auth: GcpAuth, project: String) -> Self {
        Self {
            http: Client::new(),
            auth,
            project,
        }
    }

    /// Get the project ID this client is configured for.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Query log entries from Cloud Logging.
    pub async fn list_entries(
        &self,
        filter: Option<&LogFilter>,
        page_size: usize,
    ) -> Result<Vec<LogEntry>, LogsError> {
        let token = self.auth.token().await?;
        let filter_str = filter.map(|f| f.build()).unwrap_or_default();

        debug!(
            project = %self.project,
            filter = %filter_str,
            page_size = page_size,
            "Querying logs"
        );

        let request_body = ListEntriesRequest {
            resource_names: vec![format!("projects/{}", self.project)],
            filter: if filter_str.is_empty() {
                None
            } else {
                Some(filter_str)
            },
            order_by: Some("timestamp desc".to_string()),
            page_size: Some(page_size.min(1000)),
            page_token: None,
        };

        let response = self
            .http
            .post(LOGGING_API_URL)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(LogsError::ApiError {
                code: status.as_u16(),
                message: error_body,
            });
        }

        let response_body: ListEntriesResponse = response
            .json()
            .await
            .map_err(|e| LogsError::ParseError(e.to_string()))?;

        Ok(response_body.entries.unwrap_or_default())
    }

    /// Query log entries with pagination support.
    pub async fn list_entries_paged(
        &self,
        filter: Option<&LogFilter>,
        page_size: usize,
        page_token: Option<&str>,
    ) -> Result<(Vec<LogEntry>, Option<String>), LogsError> {
        let token = self.auth.token().await?;
        let filter_str = filter.map(|f| f.build()).unwrap_or_default();

        let request_body = ListEntriesRequest {
            resource_names: vec![format!("projects/{}", self.project)],
            filter: if filter_str.is_empty() {
                None
            } else {
                Some(filter_str)
            },
            order_by: Some("timestamp desc".to_string()),
            page_size: Some(page_size.min(1000)),
            page_token: page_token.map(String::from),
        };

        let response = self
            .http
            .post(LOGGING_API_URL)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(LogsError::ApiError {
                code: status.as_u16(),
                message: error_body,
            });
        }

        let response_body: ListEntriesResponse = response
            .json()
            .await
            .map_err(|e| LogsError::ParseError(e.to_string()))?;

        Ok((
            response_body.entries.unwrap_or_default(),
            response_body.next_page_token,
        ))
    }

    /// Start streaming logs in real-time using polling.
    pub async fn stream_entries(
        &self,
        filter: Option<LogFilter>,
        lookback: Duration,
    ) -> Result<(mpsc::Receiver<LogEntry>, StreamHandle), LogsError> {
        self.auth.verify().await?;

        let (tx, rx) = mpsc::channel::<LogEntry>(256);
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);

        let http = self.http.clone();
        let project = self.project.clone();
        let auth = self.auth.clone();

        let initial_timestamp = Utc::now() - lookback;

        tokio::spawn(async move {
            let mut last_timestamp = initial_timestamp;
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS));

            loop {
                tokio::select! {
                    _ = stop_rx.recv() => {
                        debug!("Stream stopped by handle");
                        break;
                    }
                    _ = tx.closed() => {
                        debug!("Receiver dropped, stopping stream");
                        break;
                    }
                    _ = interval.tick() => {
                        let mut poll_filter = filter.clone().unwrap_or_default();
                        poll_filter = poll_filter.timestamp_after(last_timestamp);

                        match poll_logs(&http, &auth, &project, &poll_filter).await {
                            Ok(entries) => {
                                if let Some(newest) = entries.first()
                                    && let Some(ts) = newest.timestamp
                                {
                                    last_timestamp = ts;
                                }

                                for entry in entries.into_iter().rev() {
                                    if tx.send(entry).await.is_err() {
                                        debug!("Receiver dropped, stopping stream");
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Error polling logs: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok((rx, StreamHandle { stop_tx }))
    }
}

/// Helper function to poll logs (used by streaming)
async fn poll_logs(
    http: &Client,
    auth: &GcpAuth,
    project: &str,
    filter: &LogFilter,
) -> Result<Vec<LogEntry>, LogsError> {
    let token = auth.token().await?;
    let filter_str = filter.build();

    let request_body = ListEntriesRequest {
        resource_names: vec![format!("projects/{}", project)],
        filter: if filter_str.is_empty() {
            None
        } else {
            Some(filter_str)
        },
        order_by: Some("timestamp desc".to_string()),
        page_size: Some(DEFAULT_PAGE_SIZE),
        page_token: None,
    };

    let response = http
        .post(LOGGING_API_URL)
        .bearer_auth(&token)
        .json(&request_body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(LogsError::ApiError {
            code: status.as_u16(),
            message: error_body,
        });
    }

    let response_body: ListEntriesResponse = response
        .json()
        .await
        .map_err(|e| LogsError::ParseError(e.to_string()))?;

    Ok(response_body.entries.unwrap_or_default())
}

/// Handle to control a log stream
#[derive(Debug)]
pub struct StreamHandle {
    stop_tx: mpsc::Sender<()>,
}

impl StreamHandle {
    /// Stop the log stream
    pub async fn stop(&self) {
        let _ = self.stop_tx.try_send(());
    }
}

/// Builder for constructing GCP Logging filter strings.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    parts: Vec<String>,
}

impl LogFilter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a minimum severity filter.
    pub fn severity_min(mut self, severity: &str) -> Self {
        self.parts
            .push(format!("severity>={}", severity.to_uppercase()));
        self
    }

    /// Add an exact severity filter
    pub fn severity_exact(mut self, severity: &str) -> Self {
        self.parts
            .push(format!("severity={}", severity.to_uppercase()));
        self
    }

    /// Add a resource type filter.
    pub fn resource_type(mut self, resource_type: &str) -> Self {
        self.parts
            .push(format!("resource.type=\"{}\"", resource_type));
        self
    }

    /// Add a text search filter.
    pub fn text_contains(mut self, text: &str) -> Self {
        self.parts.push(format!("\"{}\"", text));
        self
    }

    /// Add a timestamp filter lower bound.
    pub fn timestamp_after(mut self, timestamp: DateTime<Utc>) -> Self {
        self.parts
            .push(format!("timestamp>\"{}\"", timestamp.to_rfc3339()));
        self
    }

    /// Add a timestamp filter upper bound.
    pub fn timestamp_before(mut self, timestamp: DateTime<Utc>) -> Self {
        self.parts
            .push(format!("timestamp<=\"{}\"", timestamp.to_rfc3339()));
        self
    }

    /// Add a timestamp range (last N duration)
    pub fn since(mut self, duration: chrono::Duration) -> Self {
        let since = Utc::now() - duration;
        self.parts
            .push(format!("timestamp>=\"{}\"", since.to_rfc3339()));
        self
    }

    /// Add a raw filter expression
    pub fn raw(mut self, expression: &str) -> Self {
        self.parts.push(expression.to_string());
        self
    }

    /// Build the final filter string.
    pub fn build(&self) -> String {
        self.parts.join(" ")
    }

    /// Check if the filter is empty.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// A single log entry from Cloud Logging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Unique identifier assigned by Logging when the entry was created.
    #[serde(default)]
    pub insert_id: Option<String>,

    /// The log entry's timestamp.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,

    /// Timestamp when the entry was received by Logging.
    #[serde(default)]
    pub receive_timestamp: Option<DateTime<Utc>>,

    /// Severity level of the log entry.
    #[serde(default)]
    pub severity: Option<String>,

    /// Full resource name of the log.
    #[serde(default)]
    pub log_name: Option<String>,

    /// Plain text message (if not JSON).
    #[serde(default)]
    pub text_payload: Option<String>,

    /// JSON structured log data.
    #[serde(default)]
    pub json_payload: Option<serde_json::Value>,

    /// The monitored resource producing this entry.
    #[serde(default)]
    pub resource: Option<MonitoredResource>,

    /// Labels attached to this log entry.
    #[serde(default)]
    pub labels: Option<HashMap<String, String>>,

    /// HTTP request info if this is a request log.
    #[serde(default)]
    pub http_request: Option<HttpRequest>,

    /// Trace ID for distributed tracing.
    #[serde(default)]
    pub trace: Option<String>,

    /// Span ID within the trace.
    #[serde(default)]
    pub span_id: Option<String>,
}

impl LogEntry {
    /// Get severity with a default fallback.
    pub fn severity_or_default(&self) -> &str {
        self.severity.as_deref().unwrap_or("DEFAULT")
    }

    /// Get a short preview of the log message.
    pub fn message_preview(&self) -> String {
        let message = self.message();
        if message.len() > 150 {
            format!("{}...", &message[..150])
        } else {
            message
        }
    }

    /// Get the full log message.
    pub fn message(&self) -> String {
        if let Some(text) = &self.text_payload {
            return text.clone();
        }

        if let Some(json) = &self.json_payload {
            if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                return msg.to_string();
            }
            return serde_json::to_string(json).unwrap_or_else(|_| "[invalid json]".to_string());
        }

        "[no message]".to_string()
    }

    /// Get the formatted timestamp.
    pub fn timestamp_str(&self) -> String {
        self.timestamp
            .map(|ts| ts.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "--:--:--".to_string())
    }

    /// Get the resource type.
    pub fn resource_type(&self) -> Option<&str> {
        self.resource.as_ref().map(|r| r.r#type.as_str())
    }

    /// Convert to a compact single-line representation.
    pub fn to_line(&self) -> String {
        format!(
            "{} [{}] {}",
            self.timestamp_str(),
            self.severity_or_default(),
            self.message_preview()
        )
    }

    /// Extract a correlation filter clause and short display identifier from this log entry.
    ///
    /// Checks `trace`, `span_id`, `labels`, and `json_payload` (for `trace_id`, `traceId`,
    /// `request_id`, `requestId`, `correlation_id`, `correlationId`).
    ///
    /// Returns `Some((gcp_filter_clause, short_display_id))` when present.
    pub fn correlation_id(&self) -> Option<(String, String)> {
        if let Some(ref t) = self.trace
            && !t.trim().is_empty()
        {
            let raw_id = t.rsplit('/').next().unwrap_or(t);
            let short_id = shorten_id(raw_id, 8);
            return Some((format!("trace=\"{}\"", t), short_id));
        }

        if let Some(ref s) = self.span_id
            && !s.trim().is_empty()
        {
            let short_id = shorten_id(s, 8);
            return Some((format!("spanId=\"{}\"", s), short_id));
        }

        const KEYS: &[&str] = &[
            "trace_id",
            "traceId",
            "request_id",
            "requestId",
            "correlation_id",
            "correlationId",
        ];

        if let Some(ref labels) = self.labels {
            for &key in KEYS {
                if let Some(val) = labels.get(key)
                    && !val.trim().is_empty()
                {
                    let short_id = shorten_id(val, 8);
                    return Some((format!("labels.{}=\"{}\"", key, val), short_id));
                }
            }
        }

        if let Some(ref json) = self.json_payload {
            for &key in KEYS {
                if let Some(val) = json.get(key).and_then(|v| v.as_str())
                    && !val.trim().is_empty()
                {
                    let short_id = shorten_id(val, 8);
                    return Some((format!("jsonPayload.{}=\"{}\"", key, val), short_id));
                }
            }
        }

        None
    }

    /// Return a compact badge (e.g., `"🔗105445aa"`) when this log entry carries a trace or correlation ID.
    pub fn short_trace_badge(&self) -> Option<String> {
        self.correlation_id()
            .map(|(_, short_id)| format!("🔗{}", short_id))
    }
}

fn shorten_id(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() > max_len {
        trimmed.chars().take(max_len).collect()
    } else {
        trimmed.to_string()
    }
}

/// Information about the monitored resource that produced a log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredResource {
    /// The monitored resource type (e.g., "cloud_run_revision").
    #[serde(rename = "type")]
    pub r#type: String,

    /// Labels describing the resource.
    #[serde(default)]
    pub labels: Option<HashMap<String, String>>,
}

/// HTTP request information attached to a log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    /// The request method (GET, POST, etc.).
    #[serde(default)]
    pub request_method: Option<String>,

    /// The requested URL.
    #[serde(default)]
    pub request_url: Option<String>,

    /// The HTTP status code.
    #[serde(default)]
    pub status: Option<u16>,

    /// The response size in bytes.
    #[serde(default)]
    pub response_size: Option<String>,

    /// The user agent string.
    #[serde(default)]
    pub user_agent: Option<String>,

    /// The remote IP address.
    #[serde(default)]
    pub remote_ip: Option<String>,

    /// The latency of the request.
    #[serde(default)]
    pub latency: Option<String>,
}

// ============================================================================
// API Request/Response types (internal)
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListEntriesRequest {
    resource_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEntriesResponse {
    #[serde(default)]
    entries: Option<Vec<LogEntry>>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_filter_builder() {
        let filter = LogFilter::new()
            .severity_min("ERROR")
            .resource_type("cloud_run_revision");

        let built = filter.build();
        assert!(built.contains("severity>=ERROR"));
        assert!(built.contains("resource.type=\"cloud_run_revision\""));
    }

    #[test]
    fn test_log_filter_text_contains() {
        let filter = LogFilter::new().text_contains("connection timeout");
        assert_eq!(filter.build(), "\"connection timeout\"");
    }

    #[test]
    fn test_log_entry_message() {
        let entry = LogEntry {
            text_payload: Some("Hello world".to_string()),
            ..Default::default()
        };
        assert_eq!(entry.message(), "Hello world");

        let entry = LogEntry {
            json_payload: Some(serde_json::json!({"message": "From JSON"})),
            ..Default::default()
        };
        assert_eq!(entry.message(), "From JSON");

        let entry = LogEntry::default();
        assert_eq!(entry.message(), "[no message]");
    }

    #[test]
    fn test_log_entry_to_line() {
        let entry = LogEntry {
            timestamp: Some(Utc::now()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Something went wrong".to_string()),
            ..Default::default()
        };

        let line = entry.to_line();
        assert!(line.contains("[ERROR]"));
        assert!(line.contains("Something went wrong"));
    }

    #[test]
    fn test_log_entry_default() {
        let entry = LogEntry::default();
        assert_eq!(entry.severity_or_default(), "DEFAULT");
        assert_eq!(entry.timestamp_str(), "--:--:--");
    }

    #[test]
    fn test_log_entry_correlation_id() {
        let entry = LogEntry {
            trace: Some("projects/demo-proj/traces/105445aa7843bc8bf206b12000100000".to_string()),
            ..Default::default()
        };
        let (filter, short_id) = entry.correlation_id().unwrap();
        assert_eq!(
            filter,
            "trace=\"projects/demo-proj/traces/105445aa7843bc8bf206b12000100000\""
        );
        assert_eq!(short_id, "105445aa");
        assert_eq!(entry.short_trace_badge().as_deref(), Some("🔗105445aa"));

        let json_entry = LogEntry {
            json_payload: Some(serde_json::json!({
                "message": "Order failed",
                "request_id": "req-9988776655"
            })),
            ..Default::default()
        };
        let (j_filter, j_short) = json_entry.correlation_id().unwrap();
        assert_eq!(j_filter, "jsonPayload.request_id=\"req-9988776655\"");
        assert_eq!(j_short, "req-9988");
    }
}
