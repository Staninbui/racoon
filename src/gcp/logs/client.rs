//! GCP Cloud Logging REST v2 API client and real-time streaming engine.

use std::collections::{HashSet, VecDeque};

use chrono::{Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::entry::LogEntry;
use super::filter::LogFilter;
use crate::gcp::auth::GcpAuth;
use crate::utils::default_http_client;

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
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to parse API response
    #[error("Failed to parse response: {0}")]
    ParseError(String),

    /// Authentication error during API call
    #[error("Authentication error: {0}")]
    AuthError(#[from] crate::gcp::auth::AuthError),

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

/// Parse a project or resource specification into Cloud Logging `resourceNames`.
///
/// Supports:
/// - Single project ID: `"my-project"` -> `["projects/my-project"]`
/// - Comma-separated multi-project IDs: `"prod-gw,prod-order,prod-pay"` ->
///   `["projects/prod-gw", "projects/prod-order", "projects/prod-pay"]`
/// - Full Cloud Logging resource / Log View paths:
///   `"projects/my-proj/locations/global/buckets/_Default/views/_AllLogs"`,
///   `"organizations/12345"`, `"folders/67890"`, `"billingAccounts/ABCDEF"`
pub fn parse_resource_names(project_spec: &str) -> Vec<String> {
    let mut resources = Vec::new();
    for part in project_spec.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("projects/")
            || trimmed.starts_with("organizations/")
            || trimmed.starts_with("folders/")
            || trimmed.starts_with("billingAccounts/")
        {
            resources.push(trimmed.to_string());
        } else {
            resources.push(format!("projects/{}", trimmed));
        }
    }
    if resources.is_empty() && !project_spec.trim().is_empty() {
        resources.push(format!("projects/{}", project_spec.trim()));
    }
    resources
}

/// Extract a clean single GCP project ID from a project specification that may be
/// comma-separated (`prod-apigw,prod-order`) or a full resource path (`projects/my-proj/locations/...`).
pub fn extract_first_gcp_project_id(raw: &str) -> Option<String> {
    let first = raw.split(',').next()?.trim();
    if first.is_empty()
        || first.starts_with("file:")
        || first.ends_with(".json")
        || first.ends_with(".ndjson")
        || first.ends_with(".jsonl")
        || first.contains('\\')
    {
        return None;
    }
    if let Some(rest) = first.strip_prefix("projects/") {
        let proj = rest.split('/').next()?.trim();
        if !proj.is_empty() {
            return Some(proj.to_string());
        }
        return None;
    }
    if first.contains('/') {
        return None;
    }
    Some(first.to_string())
}

/// Client for interacting with GCP logging API.
#[derive(Clone)]
pub struct LogsClient {
    /// Http client for API requests.
    http: Client,
    /// GCP authentication manager.
    auth: GcpAuth,
    /// Target GCP project ID(s) or resource specification.
    project: String,
    /// Resolved Cloud Logging `resourceNames` list.
    resource_names: Vec<String>,
}

impl LogsClient {
    /// Create a new LogsClient for the specified project(s) or Log View resource name(s).
    pub fn new(auth: GcpAuth, project: String) -> Self {
        let resource_names = parse_resource_names(&project);
        Self {
            http: default_http_client(),
            auth,
            project,
            resource_names,
        }
    }

    /// Get the project ID or multi-project spec this client is configured for.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Get the underlying `GcpAuth` handle (including any active IAM Service Account Impersonation).
    pub fn auth(&self) -> &GcpAuth {
        &self.auth
    }

    /// Get the resolved `resourceNames` sent to Cloud Logging `entries:list`.
    pub fn resource_names(&self) -> &[String] {
        &self.resource_names
    }

    /// Return `true` when querying/streaming across multiple GCP projects or views simultaneously.
    pub fn is_multi_project(&self) -> bool {
        self.resource_names.len() > 1
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
            resource_names: self.resource_names.clone(),
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
        let resource_names = self.resource_names.clone();
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

                        match poll_logs(&http, &auth, &resource_names, &poll_filter, &dedup).await {
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
pub(crate) struct StreamDeduplicator {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl StreamDeduplicator {
    pub(crate) fn new(capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            capacity: cap,
            order: VecDeque::with_capacity(cap),
            seen: HashSet::with_capacity(cap),
        }
    }

    pub(crate) fn contains(&self, entry: &LogEntry) -> bool {
        self.seen.contains(&entry.dedup_key())
    }

    /// Record `entry` and return `true` if it was NOT previously seen.
    pub(crate) fn insert(&mut self, entry: &LogEntry) -> bool {
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
    resource_names: &[String],
    filter: &LogFilter,
    dedup: &StreamDeduplicator,
) -> Result<Vec<LogEntry>, LogsError> {
    let token = auth.token().await?;
    let filter_str = filter.build();
    let mut collected = Vec::new();
    let mut page_token: Option<String> = None;

    for _page in 0..MAX_POLL_PAGES {
        let request_body = ListEntriesRequest {
            resource_names: resource_names.to_vec(),
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

/// Handle to control a running background log stream.
///
/// Automatically signals the background polling task to stop when dropped.
#[derive(Debug)]
pub struct StreamHandle {
    stop_tx: mpsc::Sender<()>,
}

impl StreamHandle {
    /// Construct a `StreamHandle` from a stop channel sender.
    pub fn new(stop_tx: mpsc::Sender<()>) -> Self {
        Self { stop_tx }
    }

    /// Explicitly stop the log stream.
    pub async fn stop(&self) {
        let _ = self.stop_tx.try_send(());
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.try_send(());
    }
}

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
