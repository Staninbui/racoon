//! Keyboard event handling for the UI
//!
//! This module handles all keyboard input and translates it into
//! application state changes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, InputMode};

/// Handle a keyboard event
///
/// This function dispatches to mode-specific handlers based on the current input mode.
pub fn handle_key_event(app: &mut App, key: KeyEvent) {
    // Clear any error message on key press
    app.clear_error();

    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::Filter | InputMode::AiQuery | InputMode::Search => {
            handle_input_mode(app, key);
        }
    }
}

/// Handle keys in normal navigation mode.
fn handle_normal_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
        }

        // Navigation - vim style
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_previous();
        }
        KeyCode::Char('g') => {
            // gg to go to top (simplified: just 'g')
            app.select_first();
        }
        KeyCode::Char('G') => {
            app.select_last();
        }
        KeyCode::Home => {
            app.select_first();
        }
        KeyCode::End => {
            app.select_last();
        }
        KeyCode::PageDown => {
            // Move down 10 items
            for _ in 0..10 {
                app.select_next();
            }
        }
        KeyCode::PageUp => {
            // Move up 10 items
            for _ in 0..10 {
                app.select_previous();
            }
        }

        // Enter filter mode
        KeyCode::Char('/') => {
            app.enter_input_mode(InputMode::Search);
        }
        KeyCode::Char('f') => {
            app.enter_input_mode(InputMode::Filter);
        }

        // Enter AI query mode
        KeyCode::Char('?') => {
            app.enter_input_mode(InputMode::AiQuery);
        }

        // AI explain current log
        KeyCode::Char('e') => {
            if app.selected_log().is_some() {
                app.start_ai_loading();
                // Note: Actual AI call is triggered in main.rs
                // This just sets the loading state
            }
        }

        // Toggle streaming
        KeyCode::Char('p') => {
            app.toggle_streaming();
        }

        // Refresh / clear error
        KeyCode::Char('r') => {
            app.clear_error();
        }

        // Clear AI response
        KeyCode::Esc => {
            app.clear_ai_response();
        }

        _ => {}
    }
}

/// Handle keys in input mode (filter, search, AI query).
fn handle_input_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        // Cancel input
        KeyCode::Esc => {
            app.exit_input_mode();
        }

        // Submit input
        KeyCode::Enter => {
            let (text, mode) = app.submit_input();

            if !text.is_empty() {
                match mode {
                    InputMode::Filter => {
                        app.filter = Some(text);
                    }
                    InputMode::Search => {
                        // TODO: Implement search within logs
                        app.set_error("Search not yet implemented".to_string());
                    }
                    InputMode::AiQuery => {
                        // Note: Actual AI call is triggered in main.rs
                        app.start_ai_loading();
                    }
                    InputMode::Normal => {
                        // Do nothing
                    }
                }
            }
        }

        // Clear all input
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input_buffer.clear();
        }

        // Text input
        KeyCode::Char(c) => {
            app.input_char(c);
        }

        // Backspace
        KeyCode::Backspace => {
            app.input_backspace();
        }

        _ => {}
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

    use crate::gcp::LogEntry;
}