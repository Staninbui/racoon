//! GCP Cloud Logging filter builder (`LogFilter`) and client-side filter evaluator (`matches_local_filter`).

use chrono::{DateTime, Utc};

use super::entry::{LogEntry, contains_ignore_case};
use crate::utils::fingerprint_message;

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

/// Return `true` if `filter_expr` contains a client-side pseudo-filter (such as `fingerprint:...`)
/// that should NOT be sent to the remote GCP Cloud Logging REST API.
pub fn is_local_only_filter(filter_expr: &str) -> bool {
    filter_expr.contains("fingerprint:")
}

/// Tokenize a GCP filter expression while preserving spaces inside `"..."` or `'...'` quotes
/// and stripping outer parentheses.
pub fn tokenize_filter_expr(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut active_quote: Option<char> = None;

    for ch in expr.trim().chars() {
        match active_quote {
            Some(q) => {
                current.push(ch);
                if ch == q {
                    active_quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    active_quote = Some(ch);
                    current.push(ch);
                } else if ch.is_whitespace() || ch == '(' || ch == ')' {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                } else {
                    current.push(ch);
                }
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Evaluate whether a `LogEntry` satisfies a filter expression locally.
///
/// Supports:
/// - `fingerprint:<normalized_pattern>` (Spike Radar cluster drill-down)
/// - Boolean connectives `AND`, `OR`, `NOT`, and `-` prefix negation
/// - Quoted phrases containing whitespace (`jsonPayload.message="Connection reset by peer"`)
/// - Severity comparisons (`severity>=ERROR`, `severity>WARNING`, `severity=INFO`)
/// - Resource, project, trace, span, label, JSON payload, and timestamp bounds
pub fn matches_local_filter(entry: &LogEntry, filter_expr: &str) -> bool {
    let expr = filter_expr.trim();
    if expr.is_empty() {
        return true;
    }

    if let Some(target_fp) = expr.strip_prefix("fingerprint:")
        && !target_fp.trim_start().starts_with('"')
        && !target_fp.trim_start().starts_with('\'')
    {
        return fingerprint_message(&entry.message_preview()) == target_fp.trim();
    }

    let tokens = tokenize_filter_expr(expr);
    let mut negate_next = false;

    for token in tokens {
        if token.eq_ignore_ascii_case("AND") || token.eq_ignore_ascii_case("OR") {
            continue;
        }
        if token.eq_ignore_ascii_case("NOT") {
            negate_next = !negate_next;
            continue;
        }

        let (effective_token, token_negated) = if let Some(stripped) = token.strip_prefix('-')
            && !stripped.is_empty()
        {
            (stripped, !negate_next)
        } else {
            (token.as_str(), negate_next)
        };
        negate_next = false;

        let matched = matches_single_clause(entry, effective_token);
        let outcome = if token_negated { !matched } else { matched };
        if !outcome {
            return false;
        }
    }

    true
}

fn strip_quotes(s: &str) -> &str {
    s.trim().trim_matches('"').trim_matches('\'')
}

fn matches_single_clause(entry: &LogEntry, token: &str) -> bool {
    if let Some(fp_raw) = token.strip_prefix("fingerprint:") {
        let target_fp = strip_quotes(fp_raw);
        return fingerprint_message(&entry.message_preview()) == target_fp;
    }
    let token_upper = token.to_uppercase();
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY>=") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) >= severity_rank(target_sev);
    }
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY>") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) > severity_rank(target_sev);
    }
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY<=") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) <= severity_rank(target_sev);
    }
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY<") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) < severity_rank(target_sev);
    }
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY!=") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) != severity_rank(target_sev);
    }
    if let Some(target_sev) = token_upper.strip_prefix("SEVERITY=") {
        let target_sev = strip_quotes(target_sev);
        return severity_rank(entry.severity_or_default()) == severity_rank(target_sev);
    }
    if let Some(res_type) = token.strip_prefix("resource.type=") {
        let res_type = strip_quotes(res_type);
        return entry.resource_type() == Some(res_type);
    }
    if let Some(proj_val) = token
        .strip_prefix("project=")
        .or_else(|| token.strip_prefix("project_id="))
    {
        let proj_val = strip_quotes(proj_val);
        return entry.resolve_project_id("").as_deref() == Some(proj_val);
    }
    if let Some(trace_val) = token.strip_prefix("trace=") {
        let trace_val = strip_quotes(trace_val);
        return entry.trace.as_deref() == Some(trace_val);
    }
    if let Some(span_val) = token.strip_prefix("spanId=") {
        let span_val = strip_quotes(span_val);
        return entry.span_id.as_deref() == Some(span_val);
    }
    if let Some(ins_val) = token.strip_prefix("insertId=") {
        let ins_val = strip_quotes(ins_val);
        return entry.insert_id.as_deref() == Some(ins_val);
    }
    if let Some(rest) = token.strip_prefix("labels.") {
        if let Some((key, val)) = rest.split_once('=') {
            let target = strip_quotes(val);
            return entry
                .labels
                .as_ref()
                .and_then(|m| m.get(key))
                .is_some_and(|v| v == target);
        }
        if let Some((key, val)) = rest.split_once(':') {
            let target_lower = strip_quotes(val).to_lowercase();
            return entry
                .labels
                .as_ref()
                .and_then(|m| m.get(key))
                .is_some_and(|v| contains_ignore_case(v, &target_lower));
        }
    }
    if let Some(rest) = token.strip_prefix("resource.labels.") {
        if let Some((key, val)) = rest.split_once('=') {
            let target = strip_quotes(val);
            return entry
                .resource
                .as_ref()
                .and_then(|r| r.labels.as_ref())
                .and_then(|m| m.get(key))
                .is_some_and(|v| v == target);
        }
        if let Some((key, val)) = rest.split_once(':') {
            let target_lower = strip_quotes(val).to_lowercase();
            return entry
                .resource
                .as_ref()
                .and_then(|r| r.labels.as_ref())
                .and_then(|m| m.get(key))
                .is_some_and(|v| contains_ignore_case(v, &target_lower));
        }
    }
    if let Some(rest) = token.strip_prefix("jsonPayload.") {
        if let Some((key, val)) = rest.split_once('=') {
            let target = strip_quotes(val);
            return entry
                .json_payload
                .as_ref()
                .and_then(|j| j.get(key))
                .is_some_and(|v| {
                    v.as_str() == Some(target) || v.to_string().trim_matches('"') == target
                });
        }
        if let Some((key, val)) = rest.split_once(':') {
            let target_lower = strip_quotes(val).to_lowercase();
            return entry
                .json_payload
                .as_ref()
                .and_then(|j| j.get(key))
                .is_some_and(|v| {
                    let s = v
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string());
                    contains_ignore_case(&s, &target_lower)
                });
        }
    }
    if let Some(rest) = token
        .strip_prefix("textPayload:")
        .or_else(|| token.strip_prefix("textPayload="))
    {
        let target_lower = strip_quotes(rest).to_lowercase();
        return entry
            .text_payload
            .as_deref()
            .is_some_and(|t| contains_ignore_case(t, &target_lower));
    }
    if let Some(ts_raw) = token.strip_prefix("timestamp>=") {
        let ts_str = strip_quotes(ts_raw);
        if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
            let bound_utc = bound.with_timezone(&chrono::Utc);
            return entry
                .timestamp
                .or(entry.receive_timestamp)
                .is_some_and(|entry_ts| entry_ts >= bound_utc);
        }
        return true;
    }
    if let Some(ts_raw) = token.strip_prefix("timestamp>") {
        let ts_str = strip_quotes(ts_raw);
        if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
            let bound_utc = bound.with_timezone(&chrono::Utc);
            return entry
                .timestamp
                .or(entry.receive_timestamp)
                .is_some_and(|entry_ts| entry_ts > bound_utc);
        }
        return true;
    }
    if let Some(ts_raw) = token.strip_prefix("timestamp<=") {
        let ts_str = strip_quotes(ts_raw);
        if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
            let bound_utc = bound.with_timezone(&chrono::Utc);
            return entry
                .timestamp
                .or(entry.receive_timestamp)
                .is_some_and(|entry_ts| entry_ts <= bound_utc);
        }
        return true;
    }
    if let Some(ts_raw) = token.strip_prefix("timestamp<") {
        let ts_str = strip_quotes(ts_raw);
        if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
            let bound_utc = bound.with_timezone(&chrono::Utc);
            return entry
                .timestamp
                .or(entry.receive_timestamp)
                .is_some_and(|entry_ts| entry_ts < bound_utc);
        }
        return true;
    }

    let needle = if let Some((_, val)) = token.split_once('=').or_else(|| token.split_once(':')) {
        strip_quotes(val).to_lowercase()
    } else {
        strip_quotes(token).to_lowercase()
    };
    if needle.is_empty() {
        return true;
    }

    entry.matches_query_lower(&needle)
}

/// Rank GCP log severity levels numerically (`DEFAULT` = 0 .. `EMERGENCY` = 8).
pub fn severity_rank(sev: &str) -> u8 {
    match sev.to_uppercase().as_str() {
        "DEFAULT" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "NOTICE" => 3,
        "WARNING" | "WARN" => 4,
        "ERROR" => 5,
        "CRITICAL" => 6,
        "ALERT" => 7,
        "EMERGENCY" => 8,
        _ => 0,
    }
}
