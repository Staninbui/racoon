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
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Cloud logging API base URL.
const LOGGING_API_URL: &str = "https://logging.googleapis.com/v2/entries:list";

/// Page size per poll request when streaming logs.
const STREAM_PAGE_SIZE: usize = 250;

/// Maximum number of pages to fetch in a single streaming poll tick during bursts.
const MAX_POLL_PAGES: usize = 3;

/// Overlap window (in seconds) subtracted from `last_timestamp` when polling
/// to catch late-arriving logs (`receiveTimestamp` lag) and same-timestamp entries.
const STREAM_OVERLAP_SECS: i64 = 3;

/// Maximum number of recent entry keys retained for streaming deduplication.
const DEDUP_CAPACITY: usize = 2048;

/// Polling interval for streaming logs (in seconds).
const POLL_INTERVAL_SECS: u64 = 2;

/// Maximum backoff interval (in seconds) when rate-limited (HTTP 429) or on transient errors.
const MAX_BACKOFF_SECS: u64 = 60;

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

    /// Query log entries from Cloud Logging, automatically paginating when `limit > 1000`.
    pub async fn list_entries(
        &self,
        filter: Option<&LogFilter>,
        limit: usize,
    ) -> Result<Vec<LogEntry>, LogsError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let filter_str = filter.map(|f| f.build()).unwrap_or_default();
        debug!(
            project = %self.project,
            filter = %filter_str,
            limit = limit,
            "Querying logs"
        );

        let mut all_entries = Vec::with_capacity(limit.min(1000));
        let mut page_token: Option<String> = None;

        loop {
            let remaining = limit.saturating_sub(all_entries.len());
            if remaining == 0 {
                break;
            }

            let batch_size = remaining.min(1000);
            let (batch, next_token) = self
                .list_entries_paged(filter, batch_size, page_token.as_deref())
                .await?;

            let batch_empty = batch.is_empty();
            for entry in batch {
                if all_entries.len() < limit {
                    all_entries.push(entry);
                }
            }

            if all_entries.len() >= limit {
                break;
            }

            match next_token {
                Some(token) if !token.is_empty() && !batch_empty => {
                    page_token = Some(token);
                }
                _ => break,
            }
        }

        Ok(all_entries)
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

    /// Start streaming logs in real-time using overlap-window polling, deduplication,
    /// multi-page burst catch-up, and exponential backoff on rate limits.
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
            let mut has_seen_any = false;
            let mut dedup = StreamDeduplicator::new(DEDUP_CAPACITY);
            let mut consecutive_errors: u32 = 0;
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
                        let query_cursor = if has_seen_any {
                            let overlapped = last_timestamp - Duration::seconds(STREAM_OVERLAP_SECS);
                            overlapped.max(initial_timestamp)
                        } else {
                            initial_timestamp
                        };
                        poll_filter = poll_filter.timestamp_at_or_after(query_cursor);

                        match poll_logs(&http, &auth, &project, &poll_filter, &dedup).await {
                            Ok(entries) => {
                                consecutive_errors = 0;

                                if let Some(newest) = entries.first()
                                    && let Some(ts) = newest.timestamp
                                    && ts > last_timestamp
                                {
                                    last_timestamp = ts;
                                }

                                // Deliver oldest-first while filtering out already-emitted entries
                                for entry in entries.into_iter().rev() {
                                    if dedup.insert(&entry) {
                                        has_seen_any = true;
                                        if let Some(ts) = entry.timestamp
                                            && ts > last_timestamp
                                        {
                                            last_timestamp = ts;
                                        }
                                        if tx.send(entry).await.is_err() {
                                            debug!("Receiver dropped, stopping stream");
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                consecutive_errors = consecutive_errors.saturating_add(1);
                                let shift = consecutive_errors.min(5);
                                let backoff_secs = (POLL_INTERVAL_SECS << shift).min(MAX_BACKOFF_SECS);
                                warn!(
                                    error = %e,
                                    backoff_secs = backoff_secs,
                                    "Error polling logs; backing off"
                                );
                                tokio::select! {
                                    _ = stop_rx.recv() => break,
                                    _ = tx.closed() => break,
                                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)) => {}
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok((rx, StreamHandle { stop_tx }))
    }
}

/// Bounded FIFO deduplicator for streaming log entries so overlap-window polling
/// never emits duplicate entries while still capturing same-timestamp and late-arriving logs.
#[derive(Debug)]
struct StreamDeduplicator {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl StreamDeduplicator {
    fn new(capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            capacity: cap,
            order: VecDeque::with_capacity(cap),
            seen: HashSet::with_capacity(cap),
        }
    }

    fn contains(&self, entry: &LogEntry) -> bool {
        self.seen.contains(&entry.dedup_key())
    }

    /// Record `entry` and return `true` if it was NOT previously seen.
    fn insert(&mut self, entry: &LogEntry) -> bool {
        let key = entry.dedup_key();
        if self.seen.contains(&key) {
            return false;
        }
        if self.order.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.seen.remove(&oldest);
        }
        self.seen.insert(key.clone());
        self.order.push_back(key);
        true
    }
}

/// Helper function to poll logs (used by streaming), fetching up to `MAX_POLL_PAGES`
/// pages during high-throughput bursts until already-seen logs or the end of the window is reached.
async fn poll_logs(
    http: &Client,
    auth: &GcpAuth,
    project: &str,
    filter: &LogFilter,
    dedup: &StreamDeduplicator,
) -> Result<Vec<LogEntry>, LogsError> {
    let token = auth.token().await?;
    let filter_str = filter.build();
    let mut collected = Vec::new();
    let mut page_token: Option<String> = None;

    for _page in 0..MAX_POLL_PAGES {
        let request_body = ListEntriesRequest {
            resource_names: vec![format!("projects/{}", project)],
            filter: if filter_str.is_empty() {
                None
            } else {
                Some(filter_str.clone())
            },
            order_by: Some("timestamp desc".to_string()),
            page_size: Some(STREAM_PAGE_SIZE),
            page_token: page_token.clone(),
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

        let batch = response_body.entries.unwrap_or_default();
        if batch.is_empty() {
            break;
        }

        // If any entry in this batch has already been seen, we have caught up with the cursor
        let reached_seen = batch.iter().any(|e| dedup.contains(e));
        collected.extend(batch);

        if reached_seen {
            break;
        }

        match response_body.next_page_token {
            Some(next) if !next.is_empty() => page_token = Some(next),
            _ => break,
        }
    }

    Ok(collected)
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

    /// Add a timestamp filter strict lower bound (`timestamp > ...`).
    pub fn timestamp_after(mut self, timestamp: DateTime<Utc>) -> Self {
        self.parts
            .push(format!("timestamp>\"{}\"", timestamp.to_rfc3339()));
        self
    }

    /// Add an inclusive timestamp lower bound (`timestamp >= ...`).
    pub fn timestamp_at_or_after(mut self, timestamp: DateTime<Utc>) -> Self {
        self.parts
            .push(format!("timestamp>=\"{}\"", timestamp.to_rfc3339()));
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

    /// Get a short preview of the log message (UTF-8 character boundary safe).
    pub fn message_preview(&self) -> String {
        let message = self.message();
        if let Some((byte_idx, _)) = message.char_indices().nth(150) {
            format!("{}...", &message[..byte_idx])
        } else {
            message
        }
    }

    /// Stable deduplication key for streaming (uses `insert_id` when present,
    /// falling back to `(timestamp, severity, message_preview)`).
    pub fn dedup_key(&self) -> String {
        if let Some(ref id) = self.insert_id
            && !id.is_empty()
        {
            return format!("id:{}", id);
        }
        let ts = self
            .timestamp
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "no-ts".to_string());
        format!(
            "fallback:{}:{}:{}",
            ts,
            self.severity_or_default(),
            self.message_preview()
        )
    }

    /// Fast in-place case-insensitive check across log fields without serializing the entire
    /// `LogEntry` to a JSON string. `needle_lower` must already be lowercased.
    pub fn matches_query_lower(&self, needle_lower: &str) -> bool {
        if needle_lower.is_empty() {
            return true;
        }

        if let Some(ref text) = self.text_payload
            && contains_ignore_case(text, needle_lower)
        {
            return true;
        }

        if contains_ignore_case(self.severity_or_default(), needle_lower) {
            return true;
        }

        if let Some(ref res) = self.resource {
            if contains_ignore_case(&res.r#type, needle_lower) {
                return true;
            }
            if let Some(ref labels) = res.labels {
                for (k, v) in labels {
                    if contains_ignore_case(k, needle_lower)
                        || contains_ignore_case(v, needle_lower)
                    {
                        return true;
                    }
                }
            }
        }

        if let Some(ref labels) = self.labels {
            for (k, v) in labels {
                if contains_ignore_case(k, needle_lower) || contains_ignore_case(v, needle_lower) {
                    return true;
                }
            }
        }

        for opt in [
            self.trace.as_deref(),
            self.span_id.as_deref(),
            self.log_name.as_deref(),
            self.insert_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if contains_ignore_case(opt, needle_lower) {
                return true;
            }
        }

        if let Some(ref req) = self.http_request {
            for opt in [
                req.request_method.as_deref(),
                req.request_url.as_deref(),
                req.user_agent.as_deref(),
                req.remote_ip.as_deref(),
                req.latency.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if contains_ignore_case(opt, needle_lower) {
                    return true;
                }
            }
            if let Some(status) = req.status
                && status.to_string().contains(needle_lower)
            {
                return true;
            }
        }

        if let Some(ref json) = self.json_payload
            && json_value_contains_lower(json, needle_lower)
        {
            return true;
        }

        false
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

    /// Build a Surrounding Context filter (`±window_secs` around this entry's timestamp,
    /// pinned to the same resource instance when available, with all severity levels unlocked)
    /// and a short human-readable scope badge (`s` key).
    pub fn surrounding_context(&self, window_secs: i64) -> Option<(String, String)> {
        let ts = self.timestamp.or(self.receive_timestamp)?;
        let secs = window_secs.max(1);
        let half = Duration::seconds(secs);
        let start = ts - half;
        let end = ts + half;

        let mut clauses = Vec::new();
        let mut scope_label: Option<String> = None;

        const RESOURCE_INSTANCE_KEYS: &[&str] = &[
            "pod_name",
            "revision_name",
            "instance_id",
            "function_name",
            "database_id",
            "job_name",
            "service_name",
            "container_name",
        ];

        const ENTRY_INSTANCE_KEYS: &[&str] = &[
            "compute.googleapis.com/resource_name",
            "instanceId",
            "host",
            "k8s-pod/app",
            "service",
        ];

        if let Some(ref res) = self.resource {
            let rtype = res.r#type.trim();
            if !rtype.is_empty() && !rtype.chars().any(char::is_whitespace) {
                clauses.push(format!("resource.type=\"{}\"", rtype));
            }

            if let Some(ref rlabels) = res.labels {
                for &key in RESOURCE_INSTANCE_KEYS {
                    if let Some(val) = rlabels.get(key) {
                        let trimmed = val.trim();
                        if !trimmed.is_empty() && !trimmed.chars().any(char::is_whitespace) {
                            clauses.push(format!("resource.labels.{}=\"{}\"", key, trimmed));
                            scope_label = Some(shorten_id(trimmed, 20));
                            break;
                        }
                    }
                }
            }

            if scope_label.is_none() && !rtype.is_empty() {
                scope_label = Some(shorten_id(rtype, 20));
            }
        }

        if scope_label.is_none()
            && let Some(ref labels) = self.labels
        {
            for &key in ENTRY_INSTANCE_KEYS {
                if let Some(val) = labels.get(key) {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() && !trimmed.chars().any(char::is_whitespace) {
                        clauses.push(format!("labels.{}=\"{}\"", key, trimmed));
                        scope_label = Some(shorten_id(trimmed, 20));
                        break;
                    }
                }
            }
        }

        clauses.push(format!("timestamp>=\"{}\"", start.to_rfc3339()));
        clauses.push(format!("timestamp<=\"{}\"", end.to_rfc3339()));

        let time_str = ts.format("%H:%M:%S").to_string();
        let badge = match scope_label {
            Some(scope) => format!("{} ±{}s @ {}", scope, secs, time_str),
            None => format!("±{}s @ {}", secs, time_str),
        };

        Some((clauses.join(" "), badge))
    }
}

fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if haystack.is_ascii() && needle_lower.is_ascii() {
        let n_bytes = needle_lower.as_bytes();
        if n_bytes.is_empty() {
            return true;
        }
        if n_bytes.len() > haystack.len() {
            return false;
        }
        haystack
            .as_bytes()
            .windows(n_bytes.len())
            .any(|w| w.eq_ignore_ascii_case(n_bytes))
    } else {
        haystack.to_lowercase().contains(needle_lower)
    }
}

fn json_value_contains_lower(val: &serde_json::Value, needle_lower: &str) -> bool {
    match val {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => b.to_string().contains(needle_lower),
        serde_json::Value::Number(n) => n.to_string().contains(needle_lower),
        serde_json::Value::String(s) => contains_ignore_case(s, needle_lower),
        serde_json::Value::Array(arr) => arr
            .iter()
            .any(|item| json_value_contains_lower(item, needle_lower)),
        serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
            contains_ignore_case(k, needle_lower) || json_value_contains_lower(v, needle_lower)
        }),
    }
}

fn shorten_id(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if let Some((byte_idx, _)) = trimmed.char_indices().nth(max_len) {
        trimmed[..byte_idx].to_string()
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

    #[test]
    fn test_message_preview_utf8_multibyte_no_panic() {
        // 60 Chinese characters = 180 UTF-8 bytes (> 150 bytes, < 150 chars):
        // previously panicked with byte index 150 or returned truncated bytes.
        let cjk_60 = "数据库连接超时错误".repeat(7); // 63 chars, 189 bytes
        let entry = LogEntry {
            text_payload: Some(cjk_60.clone()),
            ..Default::default()
        };
        assert_eq!(entry.message_preview(), cjk_60);

        // 160 multi-byte characters + emoji crossing byte index 150
        let long_cjk = format!("🚨生产故障告警：{}", "微服务调用链超时重试失败".repeat(15));
        let long_entry = LogEntry {
            text_payload: Some(long_cjk),
            ..Default::default()
        };
        let preview = long_entry.message_preview();
        assert!(preview.ends_with("..."));
        // 150 chars + 3 chars for "..."
        assert_eq!(preview.chars().count(), 153);
    }

    #[test]
    fn test_stream_deduplicator_same_timestamp_and_eviction() {
        let mut dedup = StreamDeduplicator::new(16);
        let ts = Utc::now();

        let e1 = LogEntry {
            insert_id: Some("id-1".to_string()),
            timestamp: Some(ts),
            text_payload: Some("First log at ts".to_string()),
            ..Default::default()
        };
        let e2 = LogEntry {
            insert_id: Some("id-2".to_string()),
            timestamp: Some(ts), // identical timestamp!
            text_payload: Some("Second log at exact same ts".to_string()),
            ..Default::default()
        };

        assert!(dedup.insert(&e1));
        assert!(!dedup.insert(&e1)); // duplicate suppressed
        assert!(dedup.insert(&e2)); // same timestamp but distinct insert_id allowed!
        assert!(dedup.contains(&e1));
        assert!(dedup.contains(&e2));
    }

    #[test]
    fn test_log_entry_matches_query_lower_without_serialization() {
        let entry = LogEntry {
            severity: Some("ERROR".to_string()),
            text_payload: Some("Connection reset by peer".to_string()),
            json_payload: Some(serde_json::json!({
                "service": "checkout-api",
                "nested": { "error_code": "E_CONN_RESET", "retryable": true }
            })),
            ..Default::default()
        };

        assert!(entry.matches_query_lower("connection reset"));
        assert!(entry.matches_query_lower("checkout-api"));
        assert!(entry.matches_query_lower("e_conn_reset"));
        assert!(!entry.matches_query_lower("nonexistent_token"));
    }

    #[test]
    fn test_log_entry_surrounding_context() {
        let ts = DateTime::parse_from_rfc3339("2026-10-27T13:00:10Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut rlabels = HashMap::new();
        rlabels.insert("pod_name".to_string(), "order-svc-7d9f8b-x2k9p".to_string());

        let entry = LogEntry {
            timestamp: Some(ts),
            severity: Some("ERROR".to_string()),
            resource: Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: Some(rlabels),
            }),
            text_payload: Some("Connection reset by peer".to_string()),
            ..Default::default()
        };

        let (filter, badge) = entry.surrounding_context(5).unwrap();
        assert!(filter.contains("resource.type=\"k8s_container\""));
        assert!(filter.contains("resource.labels.pod_name=\"order-svc-7d9f8b-x2k9p\""));
        assert!(filter.contains("timestamp>=\"2026-10-27T13:00:05+00:00\""));
        assert!(filter.contains("timestamp<=\"2026-10-27T13:00:15+00:00\""));
        assert!(!filter.contains("severity"));
        assert_eq!(badge, "order-svc-7d9f8b-x2k ±5s @ 13:00:10");
    }
}
