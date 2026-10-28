//! Domain model for Google Cloud Logging entries (`LogEntry`, `MonitoredResource`, `HttpRequest`).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::client::extract_first_gcp_project_id;

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

    /// Resolve the GCP project ID associated with this log entry.
    ///
    /// Checks `resource.labels["project_id"]`, `log_name` (`projects/<id>/logs/...`),
    /// `trace` (`projects/<id>/traces/...`), and falls back to `fallback_project` if it is
    /// not an offline file label.
    pub fn resolve_project_id(&self, fallback_project: &str) -> Option<String> {
        if let Some(ref res) = self.resource
            && let Some(ref labels) = res.labels
            && let Some(pid) = labels.get("project_id")
            && !pid.trim().is_empty()
        {
            return Some(pid.trim().to_string());
        }

        for resource_path in [self.log_name.as_deref(), self.trace.as_deref()]
            .into_iter()
            .flatten()
        {
            if let Some(rest) = resource_path.strip_prefix("projects/")
                && let Some(pid) = rest.split('/').next()
                && !pid.trim().is_empty()
            {
                return Some(pid.trim().to_string());
            }
        }

        extract_first_gcp_project_id(fallback_project)
    }

    /// Return a short `[project]` badge (max 16 chars) for Multi-Project TUI display.
    pub fn short_project_badge(&self) -> Option<String> {
        let pid = self.resolve_project_id("")?;
        Some(shorten_id(&pid, 16))
    }

    /// Construct a Google Cloud Logs Explorer deep link URL for this log entry (`o` key).
    pub fn gcp_console_url(&self, fallback_project: &str) -> Option<String> {
        let project_id = self.resolve_project_id(fallback_project)?;

        let mut clauses = Vec::new();
        if let Some(rt) = self.resource_type() {
            let rt = rt.trim();
            if !rt.is_empty() {
                clauses.push(format!("resource.type=\"{}\"", rt));
            }
        }

        if let Some(ref id) = self.insert_id
            && !id.trim().is_empty()
        {
            clauses.push(format!("insertId=\"{}\"", id.trim()));
        } else if let Some((corr_clause, _)) = self.correlation_id() {
            clauses.push(corr_clause);
        } else if let Some(ref sev) = self.severity
            && !sev.trim().is_empty()
        {
            clauses.push(format!("severity={}", sev.trim()));
        }

        let mut url = String::from("https://console.cloud.google.com/logs/query");
        if !clauses.is_empty() {
            let query_str = clauses.join("\n");
            url.push_str(&format!(";query={}", crate::utils::url_encode(&query_str)));
        }

        if let Some(ts) = self.timestamp.or(self.receive_timestamp) {
            let ts_str = ts.to_rfc3339();
            url.push_str(&format!(
                ";cursorTimestamp={}",
                crate::utils::url_encode(&ts_str)
            ));
        }

        url.push_str(&format!(
            "?project={}",
            crate::utils::url_encode(&project_id)
        ));
        Some(url)
    }
}

pub(crate) fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
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
