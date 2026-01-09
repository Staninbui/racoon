//! Application state for the TUI.
//!
//! This module defines the core state that drives the UI, including
//! the log buffer, selection state, and input handling.

use crate::gcp::LogEntry;

/// Maximum number of logs to keep in memory.
const MAX_LOGS: usize = 100;

/// The main application state.
#[derive(Debug)]
pub struct App {
    /// Buffered log entries (newest last).
    pub logs: Vec<LogEntry>,

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

    /// AI analysis result for the selected log.
    pub ai_response: Option<String>,

    /// Whether AI analysis is in progress.
    pub ai_loading: bool,

    /// GCP project ID.
    pub project: String,

    /// Whether we're actively streaming logs.
    pub is_streaming: bool,

    /// Flag to signal the app should exit.
    pub should_quit: bool,

    /// Error message to display (if any).
    pub error_message: Option<String>,
}

impl App {
    /// Create a new App instance.
    ///
    /// # Arguments
    ///
    /// * `project` - The GCP project ID being monitored
    pub fn new(project: String) -> Self {
        Self {
            logs: Vec::with_capacity(MAX_LOGS),
            selected_index: 0,
            scroll_offset: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            filter: None,
            ai_response: None,
            ai_loading: false,
            project,
            is_streaming: true,
            should_quit: false,
            error_message: None,
        }
    }

    /// Add a new log entry to the buffer.
    ///
    /// Maintains the MAX_LOGS limit by removing oldest entries.
    pub fn add_log(&mut self, entry: LogEntry) {
        self.logs.push(entry);

        // Remove oldest if we exceed the limit
        while self.logs.len() > MAX_LOGS {
            self.logs.remove(0);
            // Adjust selection if needed
            if self.selected_index > 0 {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
        }
    }

    /// Move selection to the next log entry.
    pub fn select_next(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.logs.len() - 1);
            self.clear_ai_response();
        }
    }

    /// Move selection to the previous log entry.
    pub fn select_previous(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
            self.clear_ai_response();
        }
    }

    /// Move selection to the first log entry.
    pub fn select_first(&mut self) {
        self.selected_index = 0;
        self.clear_ai_response();
    }

    /// Move selection to the last log entry.
    pub fn select_last(&mut self) {
        if !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
            self.clear_ai_response();
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
    }

    /// Set the AI response for the current selection.
    pub fn set_ai_response(&mut self, response: String) {
        self.ai_response = Some(response);
        self.ai_loading = false;
    }

    /// Start AI loading state.
    pub fn start_ai_loading(&mut self) {
        self.ai_loading = true;
        self.ai_response = None;
    }

    /// Toggle streaming on/off.
    pub fn toggle_streaming(&mut self) {
        self.is_streaming = !self.is_streaming;
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

        // Add more than MAX_LOGS entries
        for i in 0..150 {
            let mut entry = LogEntry::default();
            entry.insert_id = Some(format!("log-{}", i));
            app.add_log(entry);
        }

        assert_eq!(app.logs.len(), MAX_LOGS);
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new("test".to_string());

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