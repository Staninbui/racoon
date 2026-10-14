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

    /// Whether AI is available.
    pub ai_available: bool,

    /// AI analysis result for the selected log.
    pub ai_response: Option<String>,

    /// Whether AI analysis is in progress.
    pub ai_loading: bool,

    /// AI query that was sent to the LLM.
    pub ai_query: Option<String>,

    /// Currently configured AI provider and model label (e.g., "google/gemini-3-flash-preview").
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
    ///
    /// # Arguments
    ///
    /// * `project` - The GCP project ID being monitored
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

    /// Add a new log entry to the buffer.
    ///
    /// Maintains the `max_logs` limit by removing oldest entries.
    pub fn add_log(&mut self, entry: LogEntry) {
        // If a local filter is active and we don't have a live stream replacement,
        // ensure incoming entries match the active filter.
        if let Some(ref filter_expr) = self.filter
            && !matches_local_filter(&entry, filter_expr)
        {
            return;
        }

        // Check if user is at or near the bottom before adding
        let was_at_bottom = self.is_at_bottom();

        self.logs.push(entry);

        // Remove oldest if we exceed the limit
        while self.logs.len() > self.max_logs {
            self.logs.remove(0);
            // Adjust selection if needed
            if self.selected_index > 0 {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
        }

        // Auto-follow: if user was at bottom, stay at bottom
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
        // Consider "at bottom" if within last 2 items
        self.selected_index >= self.logs.len().saturating_sub(2)
    }

    /// Scroll detail pane down.
    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    /// Scroll detail pane up.
    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    /// Scroll AI pane down.
    pub fn scroll_ai_down(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_add(1);
    }

    /// Scroll AI pane up.
    pub fn scroll_ai_up(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_sub(1);
    }

    /// Reset scroll when content changes.
    pub fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
    }

    pub fn reset_ai_scroll(&mut self) {
        self.ai_scroll = 0;
    }

    /// Move selection to the next log entry.
    pub fn select_next(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.logs.len() - 1);
        }
    }

    /// Move selection to the previous log entry.
    pub fn select_previous(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    /// Move selection to the first log entry.
    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    /// Move selection to the last log entry.
    pub fn select_last(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    /// Get the currently selected log entry.
    pub fn selected_log(&self) -> Option<&LogEntry> {
        self.logs.get(self.selected_index)
    }

    /// Clear the AI response (called when selection changes).
    pub fn clear_ai_response(&mut self) {
        self.ai_response = None;
        self.ai_loading = false;
        self.ai_query = None;
        self.ai_scroll = 0; // reset scroll
    }

    /// Set the AI response for the current selection.
    pub fn set_ai_response(&mut self, response: String) {
        self.ai_response = Some(response);
        self.ai_loading = false;
        self.ai_scroll = 0; // reset scroll
    }

    /// Start AI loading state.
    pub fn start_ai_loading(&mut self) {
        self.ai_loading = true;
        self.ai_response = None;
    }

    /// Toggle streaming on/off.
    pub fn toggle_streaming(&mut self) {
        if self.offline_logs.is_none() {
            self.is_streaming = !self.is_streaming;
        }
    }

    /// Enter a specific input mode.
    pub fn enter_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
        self.input_buffer.clear();
    }

    /// Exit input mode and return to normal.
    pub fn exit_input_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    /// Handle a character input in input mode.
    pub fn input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    /// Handle backspace in input mode.
    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// Submit the current input.
    ///
    /// Returns the submitted text and the mode it was submitted in.
    pub fn submit_input(&mut self) -> (String, InputMode) {
        let text = self.input_buffer.clone();
        let mode = self.input_mode;
        self.exit_input_mode();
        (text, mode)
    }

    /// Set an error message to display.
    pub fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
    }

    /// Clear the current error message.
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Perform a local search within loaded logs.
    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }

        let query_lower = query.to_lowercase();

        // Find all matching log indices
        self.search_matches = self
            .logs
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Search in message
                let message = entry.message_preview().to_lowercase();
                if message.contains(&query_lower) {
                    return true;
                }

                // Search in severity
                let severity = entry.severity_or_default().to_lowercase();
                if severity.contains(&query_lower) {
                    return true;
                }

                // Search in full JSON (expensive but thorough)
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

        // Jump to first match
        if let Some(&first_match) = self.search_matches.first() {
            self.selected_index = first_match;
        }
    }

    /// Jump to next search match.
    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
        self.selected_index = self.search_matches[self.search_match_index];
    }

    /// Jump to previous search match.
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

    /// Clear search state.
    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    /// Check if a log index matches the current search.
    pub fn is_search_match(&self, index: usize) -> bool {
        self.search_matches.contains(&index)
    }

    /// Create with specified theme.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Get current theme colors.
    pub fn colors(&self) -> ThemeColors {
        self.theme.colors()
    }

    /// Toggle between dark and light theme.
    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
    }
}

/// Evaluate whether a `LogEntry` satisfies a filter expression locally.
///
/// Supports common GCP filter clauses (`severity>=...`, `severity=...`,
/// `resource.type="..."`) as well as general substring matching.
pub fn matches_local_filter(entry: &LogEntry, filter_expr: &str) -> bool {
    let expr = filter_expr.trim();
    if expr.is_empty() {
        return true;
    }

    // Check individual space-separated clauses if they look like structured expressions
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
        if token.starts_with("timestamp>") || token.starts_with("timestamp<") {
            // Timestamp bounds are handled by the server query
            continue;
        }

        // Fallback: substring match on message, severity, resource type, or JSON
        let needle = token.trim_matches('"').trim_matches('\'').to_lowercase();
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
    /// Normal navigation mode.
    Normal,

    /// Entering a GCP filter query.
    Filter,

    /// Entering a natural language query for AI.
    AiQuery,

    /// Searching within current logs.
    Search,
}

impl InputMode {
    /// Get the prompt text for this mode.
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

        // Add more than DEFAULT_MAX_LOGS entries
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
        app.is_streaming = false; // Disable auto-follow for testing

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
