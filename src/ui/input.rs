//! Keyboard event handling for the UI
//!
//! This module handles all keyboard input and translates it into
//! application state changes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, InputMode};

/// Actions that may be triggered by keyboard events.
#[derive(Debug)]
pub enum Action {
    /// No action needed.
    None,
    /// Request AI explanation for current log.
    ExplainLog,
    /// Request AI query generation.
    GenerateQuery(String),
    /// Request AI question generation.
    AskQuestion(String),
}

/// Handle a keyboard event
///
/// This function dispatches to mode-specific handlers based on the current input mode.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    // Clear any error message on key press
    app.clear_error();

    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::Filter | InputMode::AiQuery | InputMode::Search => {
            handle_input_mode(app, key)
        }
    }
}

/// Handle keys in normal navigation mode.
fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
            Action::None
        }

        // Scroll detail pane with Ctrl+j/k
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_detail_down();
            Action::None
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_detail_up();
            Action::None
        }

        // Scroll AI pane with Ctrl+n/p (next/prev)
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_ai_down();
            Action::None
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_ai_up();
            Action::None
        }

        // Search navigation (vim style)
        KeyCode::Char('n') => {
            if app.search_query.is_some() {
                app.search_next();
            }
            Action::None
        }
        KeyCode::Char('N') => {
            if app.search_query.is_some() {
                app.search_prev();
            }
            Action::None
        }

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
            Action::None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_previous();
            Action::None
        }
        KeyCode::Char('g') => {
            app.select_first();
            Action::None
        }
        KeyCode::Char('G') => {
            app.select_last();
            Action::None
        }
        KeyCode::Home => {
            app.select_first();
            Action::None
        }
        KeyCode::End => {
            app.select_last();
            Action::None
        }
        KeyCode::PageDown => {
            for _ in 0..10 { app.select_next(); }
            Action::None
        }
        KeyCode::PageUp => {
            for _ in 0..10 { app.select_previous(); }
            Action::None
        }

        // Input modes
        KeyCode::Char('/') => {
            app.enter_input_mode(InputMode::Search);
            Action::None
        }
        KeyCode::Char('f') => {
            app.enter_input_mode(InputMode::Filter);
            Action::None
        }
        KeyCode::Char('?') => {
            if app.ai_available {
                app.enter_input_mode(InputMode::AiQuery);
            } else {
                app.set_error("AI not available. Set GEMINI_API_KEY.".to_string());
            }
            Action::None
        }

        // AI explain current log
        KeyCode::Char('e') => {
            if app.ai_available && app.selected_log().is_some() {
                Action::ExplainLog
            } else if !app.ai_available {
                app.set_error("AI not available. Set GEMINI_API_KEY.".to_string());
                Action::None
            } else {
                Action::None
            }
        }

        // Toggle streaming
        KeyCode::Char('p') => {
            app.toggle_streaming();
            Action::None
        }

        // Clear
        KeyCode::Esc => {
            app.clear_search();
            app.clear_ai_response();
            app.filter = None;
            Action::None
        }

        _ => Action::None,
    }
}

/// Handle keys in input mode.
fn handle_input_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        // Cancel
        KeyCode::Esc => {
            app.exit_input_mode();
            Action::None
        }

        // Submit
        KeyCode::Enter => {
            let (text, mode) = app.submit_input();

            if text.is_empty() {
                return Action::None;
            }

            match mode {
                InputMode::Filter => {
                    app.filter = Some(text);
                    Action::None
                }
                InputMode::AiQuery => {
                    // Return action for AI processing
                    Action::AskQuestion(text)
                }
                InputMode::Search => {
                    // Perform local search
                    app.search(&text);
                    let match_count = app.search_matches.len();
                    if match_count == 0 {
                        app.set_error(format!("No matches for '{}'", text));
                    } else {
                        // Show match count (optional: use a status message instead of error)
                        app.set_error(format!(
                            "Found {} match{} (n: next, N: prev, Esc: clear)",
                            match_count,
                            if match_count == 1 { "" } else { "es" }
                        ));
                    }
                    Action::None
                }
                InputMode::Normal => Action::None,
            }
        }

        // Clear line
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input_buffer.clear();
            Action::None
        }

        // Text input
        KeyCode::Char(c) => {
            app.input_char(c);
            Action::None
        }

        // Backspace
        KeyCode::Backspace => {
            app.input_backspace();
            Action::None
        }

        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn test_quit_on_q() {
        let mut app = App::new("test".to_string());
        assert!(!app.should_quit);

        handle_key_event(&mut app, make_key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn test_navigation_keys() {
        let mut app = App::new("test".to_string());

        // Add some logs
        for _ in 0..10 {
            app.add_log(LogEntry::default());
        }

        handle_key_event(&mut app, make_key(KeyCode::Char('j')));
        assert_eq!(app.selected_index, 1);

        handle_key_event(&mut app, make_key(KeyCode::Char('k')));
        assert_eq!(app.selected_index, 0);

        handle_key_event(&mut app, make_key(KeyCode::Char('G')));
        assert_eq!(app.selected_index, 9);
    }

    #[test]
    fn test_input_mode_toggle() {
        let mut app = App::new("test".to_string());

        handle_key_event(&mut app, make_key(KeyCode::Char('/')));
        assert_eq!(app.input_mode, InputMode::Search);

        handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_explain_requires_ai() {
        let mut app = App::new("test".to_string());
        app.add_log(LogEntry::default());

        // Without AI
        app.ai_available = false;
        let action = handle_key_event(&mut app, make_key(KeyCode::Char('e')));
        assert!(matches!(action, Action::None));

        // With AI
        app.ai_available = true;
        let action = handle_key_event(&mut app, make_key(KeyCode::Char('e')));
        assert!(matches!(action, Action::ExplainLog));
    }

    use crate::gcp::LogEntry;
}