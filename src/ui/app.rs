//! Application state for the TUI.
//!
//! This module defines the core state that drives the UI, including
//! the log buffer, selection state, error spike radar, and input handling.

use std::collections::VecDeque;

use super::radar::{ErrorCluster, SpikeRadar, fingerprint_message};
use super::theme::{Theme, ThemeColors};
use crate::gcp::LogEntry;

/// Default maximum number of logs to keep in memory.
const DEFAULT_MAX_LOGS: usize = 100;

/// The main application state.
#[derive(Debug)]
pub struct App {
    /// Buffered log entries in a ring-buffer queue (newest last).
    pub logs: VecDeque<LogEntry>,

    /// Full offline log dataset when viewing a local file (`racoon view`).
    pub offline_logs: Option<Vec<LogEntry>>,

    /// Maximum number of logs to retain in streaming buffer.
    pub max_logs: usize,

    /// Currently selected log index.
    pub selected_index: usize,

    /// Scroll offset for the log list.
    pub scroll_offset: usize,

    /// Real-time Error Fingerprinting & Spike Radar engine.
    pub radar: SpikeRadar,

    /// Whether the top pane is currently showing the Error Spike Radar (`c` key).
    pub show_radar: bool,

    /// Currently selected cluster index in Radar mode.
    pub radar_selected_index: usize,

    /// Current input mode.
    pub input_mode: InputMode,

    /// Text in the query/search input.
    pub input_buffer: String,

    /// Current filter string (applied to logs).
    pub filter: Option<String>,

    /// Short display ID when Distributed Trace Context mode (`x` key) is active.
    pub active_trace: Option<String>,

    /// Scope badge when Surrounding Logs mode (`s` key, ±5s same-instance window) is active.
    pub active_surrounding: Option<String>,

    /// Deduplication key of the anchor log entry around which Surrounding Logs mode was opened.
    pub anchor_log_key: Option<String>,

    /// Saved filter prior to entering Trace or Surrounding mode, restored on exit.
    pub previous_filter: Option<String>,

    /// Whether AI is available.
    pub ai_available: bool,

    /// AI analysis result for the selected log.
    pub ai_response: Option<String>,

    /// Whether AI analysis is in progress.
    pub ai_loading: bool,

    /// AI query that was sent to the LLM.
    pub ai_query: Option<String>,

    /// Currently configured AI provider and model label (e.g., "google/gemini-3.8-flash").
    pub ai_model_label: String,

    /// Whether Pre-AI PII & Secret Guard is enabled.
    pub ai_pii_guard: bool,

    /// GCP project ID or offline file label.
    pub project: String,

    /// Whether we're actively streaming logs.
    pub is_streaming: bool,

    /// Flag to signal the app should exit.
    pub should_quit: bool,

    /// Error message to display (if any).
    pub error_message: Option<String>,

    /// Scroll offset for the detail pane.
    pub detail_scroll: u16,

    /// Scroll offset for the AI pane.
    pub ai_scroll: u16,

    /// Current search query.
    pub search_query: Option<String>,

    /// Indices of logs matching the search.
    pub search_matches: Vec<usize>,

    /// Current match index (for next/prev navigation).
    pub search_match_index: usize,

    /// Current theme.
    pub theme: Theme,
}

impl App {
    /// Create a new App instance.
    pub fn new(project: String) -> Self {
        Self {
            logs: VecDeque::with_capacity(DEFAULT_MAX_LOGS),
            offline_logs: None,
            max_logs: DEFAULT_MAX_LOGS,
            selected_index: 0,
            scroll_offset: 0,
            radar: SpikeRadar::new(),
            show_radar: false,
            radar_selected_index: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            filter: None,
            active_trace: None,
            active_surrounding: None,
            anchor_log_key: None,
            previous_filter: None,
            ai_available: false,
            ai_response: None,
            ai_loading: false,
            ai_query: None,
            ai_model_label: "google/gemini-3.8-flash".to_string(),
            ai_pii_guard: true,
            project,
            is_streaming: true,
            should_quit: false,
            error_message: None,
            detail_scroll: 0,
            ai_scroll: 0,
            search_query: None,
            search_matches: Vec::new(),
            search_match_index: 0,
            theme: Theme::Dark,
        }
    }

    /// Set the active AI provider/model label displayed in the header.
    pub fn with_ai_model_label(mut self, label: String) -> Self {
        self.ai_model_label = label;
        self
    }

    /// Set the maximum number of logs to keep in memory.
    pub fn with_max_logs(mut self, max_logs: usize) -> Self {
        let cap = max_logs.max(1);
        self.max_logs = cap;
        self
    }

    /// Set an initial filter string to display in the UI.
    pub fn with_filter(mut self, filter: Option<String>) -> Self {
        self.filter = filter.filter(|f| !f.trim().is_empty());
        self
    }

    /// Initialize with a static set of offline logs (disables live streaming).
    pub fn with_offline_logs(mut self, entries: Vec<LogEntry>) -> Self {
        self.max_logs = entries.len().max(self.max_logs);
        self.radar.rebuild_from_logs(&entries);
        self.logs = VecDeque::from(entries.clone());
        self.offline_logs = Some(entries);
        self.is_streaming = false;
        self.selected_index = 0;
        self
    }

    /// Toggle Error Fingerprint & Spike Radar view (`c` key).
    pub fn toggle_radar(&mut self) {
        self.show_radar = !self.show_radar;
        if self.show_radar {
            self.radar_selected_index = 0;
            self.detail_scroll = 0;
        }
    }

    /// Return the currently selected `ErrorCluster` when in Radar view.
    pub fn selected_cluster(&self) -> Option<&ErrorCluster> {
        if self.show_radar {
            self.radar.clusters.get(self.radar_selected_index)
        } else {
            None
        }
    }

    /// Drill down into the currently selected `ErrorCluster` in Radar view (`Enter` key):
    /// switches back to the Log List filtered to entries matching that cluster's fingerprint.
    pub fn drill_into_selected_cluster(&mut self) -> Option<String> {
        let cluster = self.selected_cluster()?.clone();
        self.show_radar = false;
        let filter_expr = format!("fingerprint:{}", cluster.fingerprint);
        self.set_filter(Some(filter_expr));
        // Build a GCP-compatible severity filter for live stream if needed
        Some(format!("severity>={}", cluster.severity))
    }

    /// Toggle Distributed Trace Context mode (`x` key).
    pub fn toggle_trace_context(&mut self) -> Result<Option<String>, String> {
        if self.active_trace.is_some() {
            let restored = self.exit_trace_context();
            return Ok(restored);
        }

        let entry = self
            .selected_log()
            .ok_or_else(|| "No log entry selected.".to_string())?;

        let (filter_clause, short_id) = entry.correlation_id().ok_or_else(|| {
            "Selected log has no trace, spanId, or request_id to follow.".to_string()
        })?;

        self.show_radar = false;
        if self.active_surrounding.is_none() && self.active_trace.is_none() {
            self.previous_filter = self.filter.clone();
        }
        self.active_surrounding = None;
        self.anchor_log_key = None;
        self.active_trace = Some(short_id);
        self.set_filter(Some(filter_clause.clone()));
        Ok(Some(filter_clause))
    }

    /// Exit Trace Context mode and restore the previous filter.
    pub fn exit_trace_context(&mut self) -> Option<String> {
        self.active_trace = None;
        let restored = self.previous_filter.take();
        self.set_filter(restored.clone());
        restored
    }

    /// Toggle Surrounding Logs Context mode (`s` key — ±5s around selected log on same resource instance).
    pub fn toggle_surrounding_context(&mut self) -> Result<Option<String>, String> {
        if self.active_surrounding.is_some() {
            let restored = self.exit_surrounding_context();
            return Ok(restored);
        }

        let entry = self
            .selected_log()
            .cloned()
            .ok_or_else(|| "No log entry selected.".to_string())?;

        let (filter_clause, badge) = entry.surrounding_context(5).ok_or_else(|| {
            "Selected log has no timestamp for ±5s surrounding context.".to_string()
        })?;

        let anchor_key = entry.dedup_key();

        self.show_radar = false;
        if self.active_surrounding.is_none() && self.active_trace.is_none() {
            self.previous_filter = self.filter.clone();
        }
        self.active_trace = None;
        self.active_surrounding = Some(badge);
        self.anchor_log_key = Some(anchor_key);
        self.set_filter(Some(filter_clause.clone()));
        self.select_anchor_log();
        Ok(Some(filter_clause))
    }

    /// Exit Surrounding Logs Context mode and restore the previous filter.
    pub fn exit_surrounding_context(&mut self) -> Option<String> {
        self.active_surrounding = None;
        self.anchor_log_key = None;
        let restored = self.previous_filter.take();
        self.set_filter(restored.clone());
        restored
    }

    /// Move `selected_index` to the anchor log entry if present in `self.logs`.
    pub fn select_anchor_log(&mut self) {
        if let Some(ref key) = self.anchor_log_key
            && let Some(idx) = self.logs.iter().position(|e| e.dedup_key() == *key)
        {
            self.selected_index = idx;
        }
    }

    /// Check whether `entry` is the anchor log around which Surrounding Logs mode was opened.
    pub fn is_anchor_log(&self, entry: &LogEntry) -> bool {
        self.anchor_log_key
            .as_ref()
            .is_some_and(|k| *k == entry.dedup_key())
    }

    /// Add a new log entry to the buffer.
    pub fn add_log(&mut self, entry: LogEntry) {
        if let Some(ref filter_expr) = self.filter
            && !matches_local_filter(&entry, filter_expr)
        {
            return;
        }

        self.radar.record_entry(&entry);

        let was_at_bottom = self.is_at_bottom();
        let matches_active_search = self
            .search_query
            .as_ref()
            .map(|q| entry.matches_query_lower(&q.to_lowercase()))
            .unwrap_or(false);

        self.logs.push_back(entry);
        if matches_active_search {
            self.search_matches.push(self.logs.len() - 1);
        }

        while self.logs.len() > self.max_logs {
            self.logs.pop_front();
            if self.selected_index > 0 {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            if !self.search_matches.is_empty() {
                self.search_matches.retain_mut(|idx| {
                    if *idx == 0 {
                        false
                    } else {
                        *idx -= 1;
                        true
                    }
                });
                if self.search_match_index >= self.search_matches.len() {
                    self.search_match_index = 0;
                }
            }
        }

        if was_at_bottom && self.is_streaming {
            self.selected_index = self.logs.len().saturating_sub(1);
        }
    }

    /// Update the active filter and apply it locally (for offline mode or in-memory logs).
    pub fn set_filter(&mut self, filter: Option<String>) {
        let normalized = filter
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty());
        self.filter = normalized.clone();

        if let Some(ref all_logs) = self.offline_logs {
            if let Some(ref expr) = normalized {
                self.logs = all_logs
                    .iter()
                    .filter(|e| matches_local_filter(e, expr))
                    .cloned()
                    .collect();
            } else {
                self.logs = VecDeque::from(all_logs.clone());
            }
            self.radar.rebuild_from_logs(&self.logs);
        } else if let Some(ref expr) = normalized {
            self.logs.retain(|e| matches_local_filter(e, expr));
            self.radar.rebuild_from_logs(&self.logs);
        }

        self.selected_index = 0;
        self.scroll_offset = 0;
        self.detail_scroll = 0;
        if let Some(q) = self.search_query.clone() {
            self.search(&q);
        }
    }

    /// Clear buffered logs when restarting a live GCP log stream with a new filter.
    pub fn reset_for_new_stream(&mut self) {
        self.logs.clear();
        self.radar.clear();
        self.selected_index = 0;
        self.radar_selected_index = 0;
        self.scroll_offset = 0;
        self.detail_scroll = 0;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    /// Check if the selected index is at or near the bottom.
    pub fn is_at_bottom(&self) -> bool {
        if self.logs.is_empty() {
            return true;
        }
        self.selected_index >= self.logs.len().saturating_sub(2)
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_ai_down(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_add(1);
    }

    pub fn scroll_ai_up(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_sub(1);
    }

    pub fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
    }

    pub fn reset_ai_scroll(&mut self) {
        self.ai_scroll = 0;
    }

    pub fn select_next(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index =
                    (self.radar_selected_index + 1).min(self.radar.clusters.len() - 1);
            }
        } else if !self.logs.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.logs.len() - 1);
        }
    }

    pub fn select_previous(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index = self.radar_selected_index.saturating_sub(1);
            }
        } else if !self.logs.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    pub fn select_first(&mut self) {
        if self.show_radar {
            self.radar_selected_index = 0;
        } else {
            self.selected_index = 0;
        }
    }

    pub fn select_last(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index = self.radar.clusters.len() - 1;
            }
        } else if !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    /// Return the currently selected `LogEntry` (or the selected cluster's sample entry in Radar view).
    pub fn selected_log(&self) -> Option<&LogEntry> {
        if self.show_radar {
            self.radar
                .clusters
                .get(self.radar_selected_index)
                .map(|c| &c.sample_entry)
        } else {
            self.logs.get(self.selected_index)
        }
    }

    pub fn clear_ai_response(&mut self) {
        self.ai_response = None;
        self.ai_loading = false;
        self.ai_query = None;
        self.ai_scroll = 0;
    }

    pub fn set_ai_response(&mut self, response: String) {
        self.ai_response = Some(response);
        self.ai_loading = false;
        self.ai_scroll = 0;
    }

    pub fn start_ai_loading(&mut self) {
        self.ai_loading = true;
        self.ai_response = None;
    }

    pub fn toggle_streaming(&mut self) {
        if self.offline_logs.is_none() {
            self.is_streaming = !self.is_streaming;
        }
    }

    pub fn enter_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
        self.input_buffer.clear();
    }

    pub fn exit_input_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    pub fn input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    pub fn submit_input(&mut self) -> (String, InputMode) {
        let text = self.input_buffer.clone();
        let mode = self.input_mode;
        self.exit_input_mode();
        (text, mode)
    }

    pub fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
    }

    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }

        let query_lower = query.to_lowercase();

        self.search_matches = self
            .logs
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches_query_lower(&query_lower))
            .map(|(i, _)| i)
            .collect();

        self.search_query = Some(query.to_string());
        self.search_match_index = 0;

        if let Some(&first_match) = self.search_matches.first() {
            self.selected_index = first_match;
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
        self.selected_index = self.search_matches[self.search_match_index];
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        if self.search_match_index == 0 {
            self.search_match_index = self.search_matches.len() - 1;
        } else {
            self.search_match_index -= 1;
        }
        self.selected_index = self.search_matches[self.search_match_index];
    }

    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    pub fn is_search_match(&self, index: usize) -> bool {
        self.search_matches.contains(&index)
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn colors(&self) -> ThemeColors {
        self.theme.colors()
    }

    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
    }
}

/// Evaluate whether a `LogEntry` satisfies a filter expression locally.
pub fn matches_local_filter(entry: &LogEntry, filter_expr: &str) -> bool {
    let expr = filter_expr.trim();
    if expr.is_empty() {
        return true;
    }

    if let Some(target_fp) = expr.strip_prefix("fingerprint:") {
        return fingerprint_message(&entry.message_preview()) == target_fp.trim();
    }

    for token in expr.split_whitespace() {
        let token_upper = token.to_uppercase();
        if let Some(target_sev) = token_upper.strip_prefix("SEVERITY>=") {
            let target_sev = target_sev.trim_matches('"').trim_matches('\'');
            if severity_rank(entry.severity_or_default()) < severity_rank(target_sev) {
                return false;
            }
            continue;
        }
        if let Some(target_sev) = token_upper.strip_prefix("SEVERITY>") {
            let target_sev = target_sev.trim_matches('"').trim_matches('\'');
            if severity_rank(entry.severity_or_default()) <= severity_rank(target_sev) {
                return false;
            }
            continue;
        }
        if let Some(target_sev) = token_upper.strip_prefix("SEVERITY=") {
            let target_sev = target_sev.trim_matches('"').trim_matches('\'');
            if severity_rank(entry.severity_or_default()) != severity_rank(target_sev) {
                return false;
            }
            continue;
        }
        if let Some(res_type) = token.strip_prefix("resource.type=") {
            let res_type = res_type.trim_matches('"').trim_matches('\'');
            if entry.resource_type() != Some(res_type) {
                return false;
            }
            continue;
        }
        if let Some(trace_val) = token.strip_prefix("trace=") {
            let trace_val = trace_val.trim_matches('"').trim_matches('\'');
            if entry.trace.as_deref() != Some(trace_val) {
                return false;
            }
            continue;
        }
        if let Some(span_val) = token.strip_prefix("spanId=") {
            let span_val = span_val.trim_matches('"').trim_matches('\'');
            if entry.span_id.as_deref() != Some(span_val) {
                return false;
            }
            continue;
        }
        if let Some(rest) = token.strip_prefix("labels.")
            && let Some((key, val)) = rest.split_once('=')
        {
            let target = val.trim_matches('"').trim_matches('\'');
            let matched = entry
                .labels
                .as_ref()
                .and_then(|m| m.get(key))
                .is_some_and(|v| v == target);
            if !matched {
                return false;
            }
            continue;
        }
        if let Some(rest) = token.strip_prefix("resource.labels.")
            && let Some((key, val)) = rest.split_once('=')
        {
            let target = val.trim_matches('"').trim_matches('\'');
            let matched = entry
                .resource
                .as_ref()
                .and_then(|r| r.labels.as_ref())
                .and_then(|m| m.get(key))
                .is_some_and(|v| v == target);
            if !matched {
                return false;
            }
            continue;
        }
        if let Some(rest) = token.strip_prefix("jsonPayload.")
            && let Some((key, val)) = rest.split_once('=')
        {
            let target = val.trim_matches('"').trim_matches('\'');
            let matched = entry
                .json_payload
                .as_ref()
                .and_then(|j| j.get(key))
                .is_some_and(|v| {
                    v.as_str() == Some(target) || v.to_string().trim_matches('"') == target
                });
            if !matched {
                return false;
            }
            continue;
        }
        if let Some(ts_raw) = token.strip_prefix("timestamp>=") {
            let ts_str = ts_raw.trim_matches('"').trim_matches('\'');
            if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                let bound_utc = bound.with_timezone(&chrono::Utc);
                match entry.timestamp.or(entry.receive_timestamp) {
                    Some(entry_ts) if entry_ts < bound_utc => return false,
                    None => return false,
                    _ => {}
                }
            }
            continue;
        }
        if let Some(ts_raw) = token.strip_prefix("timestamp>") {
            let ts_str = ts_raw.trim_matches('"').trim_matches('\'');
            if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                let bound_utc = bound.with_timezone(&chrono::Utc);
                match entry.timestamp.or(entry.receive_timestamp) {
                    Some(entry_ts) if entry_ts <= bound_utc => return false,
                    None => return false,
                    _ => {}
                }
            }
            continue;
        }
        if let Some(ts_raw) = token.strip_prefix("timestamp<=") {
            let ts_str = ts_raw.trim_matches('"').trim_matches('\'');
            if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                let bound_utc = bound.with_timezone(&chrono::Utc);
                match entry.timestamp.or(entry.receive_timestamp) {
                    Some(entry_ts) if entry_ts > bound_utc => return false,
                    None => return false,
                    _ => {}
                }
            }
            continue;
        }
        if let Some(ts_raw) = token.strip_prefix("timestamp<") {
            let ts_str = ts_raw.trim_matches('"').trim_matches('\'');
            if let Ok(bound) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                let bound_utc = bound.with_timezone(&chrono::Utc);
                match entry.timestamp.or(entry.receive_timestamp) {
                    Some(entry_ts) if entry_ts >= bound_utc => return false,
                    None => return false,
                    _ => {}
                }
            }
            continue;
        }

        let needle = if let Some((_, val)) = token.split_once('=') {
            val.trim_matches('"').trim_matches('\'').to_lowercase()
        } else {
            token.trim_matches('"').trim_matches('\'').to_lowercase()
        };
        if needle.is_empty() {
            continue;
        }

        if !entry.matches_query_lower(&needle) {
            return false;
        }
    }

    true
}

fn severity_rank(sev: &str) -> u8 {
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

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
    AiQuery,
    Search,
}

impl InputMode {
    pub fn prompt(&self) -> &'static str {
        match self {
            InputMode::Normal => "",
            InputMode::Filter => "Filter: ",
            InputMode::AiQuery => "AI Query: ",
            InputMode::Search => "Search: ",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_add_log_respects_limit() {
        let mut app = App::new("test-project".to_string());
        for i in 0..150 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), DEFAULT_MAX_LOGS);
    }

    #[test]
    fn test_app_custom_max_logs() {
        let mut app = App::new("test-project".to_string()).with_max_logs(25);
        for i in 0..40 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), 25);
    }

    #[test]
    fn test_app_radar_toggle_and_drill_down() {
        let entries = vec![
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Timeout calling payment-svc:8080 after 500ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Timeout calling payment-svc:8080 after 750ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Health check OK".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.radar.clusters.len(), 1);
        assert_eq!(app.radar.clusters[0].count, 2);

        app.toggle_radar();
        assert!(app.show_radar);
        assert!(app.selected_cluster().is_some());

        // Drill down into cluster filters logs to the 2 matching entries
        app.drill_into_selected_cluster();
        assert!(!app.show_radar);
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_app_trace_context_toggle() {
        let entries = vec![
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Gateway request received".to_string()),
                trace: Some("projects/p/traces/trace-abc-12345".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Unrelated background job".to_string()),
                trace: Some("projects/p/traces/other-99999".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("DB timeout in payment service".to_string()),
                trace: Some("projects/p/traces/trace-abc-12345".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.logs.len(), 3);

        let filter = app.toggle_trace_context().unwrap();
        assert_eq!(
            filter.as_deref(),
            Some("trace=\"projects/p/traces/trace-abc-12345\"")
        );
        assert_eq!(app.active_trace.as_deref(), Some("trace-ab"));
        assert_eq!(app.logs.len(), 2);

        let restored = app.toggle_trace_context().unwrap();
        assert!(restored.is_none());
        assert!(app.active_trace.is_none());
        assert_eq!(app.logs.len(), 3);
    }

    #[test]
    fn test_app_local_filter_and_offline_logs() {
        let entries = vec![
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Routine health check".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Database connection timeout".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.logs.len(), 2);

        app.set_filter(Some("severity>=ERROR".to_string()));
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].severity_or_default(), "ERROR");

        app.set_filter(None);
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new("test".to_string());
        app.is_streaming = false;

        for i in 0..10 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }

        assert_eq!(app.selected_index, 0);
        app.select_next();
        assert_eq!(app.selected_index, 1);
        app.select_last();
        assert_eq!(app.selected_index, 9);
        app.select_first();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_input_mode() {
        let mut app = App::new("test".to_string());

        app.enter_input_mode(InputMode::Filter);
        assert_eq!(app.input_mode, InputMode::Filter);

        app.input_char('t');
        app.input_char('e');
        app.input_char('s');
        app.input_char('t');
        assert_eq!(app.input_buffer, "test");

        app.input_backspace();
        assert_eq!(app.input_buffer, "tes");

        let (text, mode) = app.submit_input();
        assert_eq!(text, "tes");
        assert_eq!(mode, InputMode::Filter);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_app_search_matches_shift_on_ring_buffer_eviction() {
        let mut app = App::new("test".to_string()).with_max_logs(3);
        app.is_streaming = false;

        app.add_log(LogEntry {
            text_payload: Some("needle-first".to_string()),
            ..Default::default()
        });
        app.add_log(LogEntry {
            text_payload: Some("other".to_string()),
            ..Default::default()
        });
        app.add_log(LogEntry {
            text_payload: Some("needle-second".to_string()),
            ..Default::default()
        });

        app.search("needle");
        assert_eq!(app.search_matches, vec![0, 2]);

        // Adding a 4th log evicts index 0 ("needle-first"), shifts index 2 -> 1,
        // and adds a new match at index 2 ("needle-third").
        app.add_log(LogEntry {
            text_payload: Some("needle-third".to_string()),
            ..Default::default()
        });
        assert_eq!(app.logs.len(), 3);
        assert_eq!(app.search_matches, vec![1, 2]);
    }

    #[test]
    fn test_matches_local_filter_labels_and_json_payload() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("request_id".to_string(), "req-42".to_string());

        let entry = LogEntry {
            labels: Some(labels),
            json_payload: Some(serde_json::json!({
                "trace_id": "tr-999",
                "message": "Processed order"
            })),
            ..Default::default()
        };

        assert!(matches_local_filter(&entry, "labels.request_id=\"req-42\""));
        assert!(!matches_local_filter(
            &entry,
            "labels.request_id=\"req-00\""
        ));
        assert!(matches_local_filter(
            &entry,
            "jsonPayload.trace_id=\"tr-999\""
        ));
        assert!(!matches_local_filter(
            &entry,
            "jsonPayload.trace_id=\"tr-000\""
        ));
    }

    #[test]
    fn test_app_surrounding_context_toggle_and_anchor_selection() {
        use crate::gcp::logs::MonitoredResource;
        use chrono::{DateTime, Utc};

        let ts = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let pod_resource = |pod: &str| {
            let mut m = std::collections::HashMap::new();
            m.insert("pod_name".to_string(), pod.to_string());
            Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: Some(m),
            })
        };

        let entries = vec![
            // Too early (20s before crash on pod-a)
            LogEntry {
                insert_id: Some("e-early".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:00Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Booting service".to_string()),
                ..Default::default()
            },
            // 2s before crash on pod-a (INFO level — should be revealed by `s`!)
            LogEntry {
                insert_id: Some("e-before".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:18Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Pool connections at 99/100".to_string()),
                ..Default::default()
            },
            // Same second on a DIFFERENT pod (pod-b — should be excluded by `s`!)
            LogEntry {
                insert_id: Some("e-other-pod".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:20Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-b"),
                text_payload: Some("Healthy request on pod-b".to_string()),
                ..Default::default()
            },
            // The target ERROR log at 13:00:20Z on pod-a
            LogEntry {
                insert_id: Some("e-crash".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:20Z")),
                severity: Some("ERROR".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("FATAL: DB connection pool exhausted".to_string()),
                ..Default::default()
            },
            // 3s after crash on pod-a (WARNING level — should be revealed by `s`!)
            LogEntry {
                insert_id: Some("e-after".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:23Z")),
                severity: Some("WARNING".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Readiness probe failed".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        // Simulate SRE filtering by severity>=ERROR initially (only 1 log visible: `e-crash`)
        app.set_filter(Some("severity>=ERROR".to_string()));
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].insert_id.as_deref(), Some("e-crash"));

        // Press `s` to open ±5s Surrounding Context on `pod-a` around 13:00:20Z
        let surrounding_filter = app.toggle_surrounding_context().unwrap();
        assert!(surrounding_filter.is_some());
        assert_eq!(
            app.active_surrounding.as_deref(),
            Some("pod-a ±5s @ 13:00:20")
        );

        // Now all 3 logs on `pod-a` within [13:00:15, 13:00:25] (INFO, ERROR, WARNING) are shown!
        assert_eq!(app.logs.len(), 3);
        assert_eq!(app.logs[0].insert_id.as_deref(), Some("e-before"));
        assert_eq!(app.logs[1].insert_id.as_deref(), Some("e-crash"));
        assert_eq!(app.logs[2].insert_id.as_deref(), Some("e-after"));

        // And `selected_index` automatically landed on the anchor ERROR log (index 1)!
        assert_eq!(app.selected_index, 1);
        assert!(app.is_anchor_log(&app.logs[1]));

        // Press `s` again to exit Surrounding Context and restore `severity>=ERROR`
        let restored = app.toggle_surrounding_context().unwrap();
        assert_eq!(restored.as_deref(), Some("severity>=ERROR"));
        assert!(app.active_surrounding.is_none());
        assert_eq!(app.logs.len(), 1);
    }
}
