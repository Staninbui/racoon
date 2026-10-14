//! Application state for the TUI.
//!
//! This module defines the core state that drives the UI, including
//! the log buffer, selection state, and input handling.

use super::theme::{Theme, ThemeColors};
use crate::gcp::LogEntry;

/// Default maximum number of logs to keep in memory.
const DEFAULT_MAX_LOGS: usize = 100;

/// The main application state.
#[derive(Debug)]
pub struct App {
    /// Buffered log entries (newest last).
    pub logs: Vec<LogEntry>,

    /// Full offline log dataset when viewing a local file (`racoon view`).
    pub offline_logs: Option<Vec<LogEntry>>,

    /// Maximum number of logs to retain in streaming buffer.
    pub max_logs: usize,

    /// Currently selected log index.
    pub selected_index: usize,

    /// Scroll offset for the log list.
    pub scroll_offset: usize,

    /// Current input mode.
    pub input_mode: InputMode,

    /// Text in the query/search input.
    pub input_buffer: String,

    /// Current filter string (applied to logs).
    pub filter: Option<String>,

    /// Short display ID when Distributed Trace Context mode (`x` key) is active.
    pub active_trace: Option<String>,

    /// Saved filter prior to entering Trace Context mode, restored when exiting Trace mode.
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
            logs: Vec::with_capacity(DEFAULT_MAX_LOGS),
            offline_logs: None,
            max_logs: DEFAULT_MAX_LOGS,
            selected_index: 0,
            scroll_offset: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            filter: None,
            active_trace: None,
            previous_filter: None,
            ai_available: false,
            ai_response: None,
            ai_loading: false,
            ai_query: None,
            ai_model_label: "google/gemini-3.8-flash".to_string(),
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
        self.logs = entries.clone();
        self.offline_logs = Some(entries);
        self.is_streaming = false;
        self.selected_index = 0;
        self
    }

    /// Toggle Distributed Trace Context mode (`x` key).
    ///
    /// - If not in Trace mode, extracts the correlation/trace filter from the currently
    ///   selected log entry, saves the current filter in `previous_filter`, activates
    ///   Trace mode, and returns `Ok(Some(trace_filter))`.
    /// - If already in Trace mode, exits Trace mode, restores `previous_filter`, and
    ///   returns `Ok(previous_filter)`.
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

        self.previous_filter = self.filter.clone();
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

    /// Add a new log entry to the buffer.
    pub fn add_log(&mut self, entry: LogEntry) {
        if let Some(ref filter_expr) = self.filter
            && !matches_local_filter(&entry, filter_expr)
        {
            return;
        }

        let was_at_bottom = self.is_at_bottom();

        self.logs.push(entry);

        while self.logs.len() > self.max_logs {
            self.logs.remove(0);
            if self.selected_index > 0 {
                self.selected_index = self.selected_index.saturating_sub(1);
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
                self.logs = all_logs.clone();
            }
        } else if let Some(ref expr) = normalized {
            self.logs.retain(|e| matches_local_filter(e, expr));
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
        self.selected_index = 0;
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
        if !self.logs.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.logs.len() - 1);
        }
    }

    pub fn select_previous(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    pub fn select_last(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    pub fn selected_log(&self) -> Option<&LogEntry> {
        self.logs.get(self.selected_index)
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
            .filter(|(_, entry)| {
                let message = entry.message_preview().to_lowercase();
                if message.contains(&query_lower) {
                    return true;
                }

                let severity = entry.severity_or_default().to_lowercase();
                if severity.contains(&query_lower) {
                    return true;
                }

                if let Ok(json) = serde_json::to_string(entry)
                    && json.to_lowercase().contains(&query_lower)
                {
                    return true;
                }

                false
            })
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
        if token.starts_with("timestamp>") || token.starts_with("timestamp<") {
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
        let msg_match = entry.message().to_lowercase().contains(&needle);
        let sev_match = entry
            .severity_or_default()
            .to_lowercase()
            .contains(&needle);
        let res_match = entry
            .resource_type()
            .map(|r| r.to_lowercase().contains(&needle))
            .unwrap_or(false);
        let json_match = serde_json::to_string(entry)
            .map(|j| j.to_lowercase().contains(&needle))
            .unwrap_or(false);

        if !(msg_match || sev_match || res_match || json_match) {
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
            let mut entry = LogEntry::default();
            entry.insert_id = Some(format!("log-{}", i));
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), DEFAULT_MAX_LOGS);
    }

    #[test]
    fn test_app_custom_max_logs() {
        let mut app = App::new("test-project".to_string()).with_max_logs(25);
        for i in 0..40 {
            let mut entry = LogEntry::default();
            entry.insert_id = Some(format!("log-{}", i));
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), 25);
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

        // Toggle trace context on first log
        let filter = app.toggle_trace_context().unwrap();
        assert_eq!(
            filter.as_deref(),
            Some("trace=\"projects/p/traces/trace-abc-12345\"")
        );
        assert_eq!(app.active_trace.as_deref(), Some("trace-ab"));
        assert_eq!(app.logs.len(), 2);

        // Toggle again exits trace mode and restores all 3 logs
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
            let mut entry = LogEntry::default();
            entry.insert_id = Some(format!("log-{}", i));
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
}
