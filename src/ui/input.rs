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
    /// Request Full-Trace / Multi-Log AI Root Cause Analysis.
    AnalyzeTrace,
    /// Request AI query generation.
    GenerateQuery(String),
    /// Request AI question generation.
    AskQuestion(String),
    /// Apply or clear a GCP filter (restarts stream in live mode).
    ApplyFilter(Option<String>),
    /// Open interactive AI provider & model selector.
    SwitchModel,
}

/// Handle a keyboard event
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    app.clear_error();

    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::Filter | InputMode::AiQuery | InputMode::Search => handle_input_mode(app, key),
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
        KeyCode::Char('g') | KeyCode::Home => {
            app.select_first();
            Action::None
        }
        KeyCode::Char('G') | KeyCode::End => {
            app.select_last();
            Action::None
        }
        KeyCode::PageDown => {
            for _ in 0..10 {
                app.select_next();
            }
            Action::None
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                app.select_previous();
            }
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
                app.set_error("AI not available. Press 'm' or configure API key.".to_string());
            }
            Action::None
        }

        // Toggle Error Fingerprinting & Spike Radar view (`c` key)
        KeyCode::Char('c') => {
            app.toggle_radar();
            Action::None
        }

        // In Radar view, pressing Enter drills down into the selected error cluster
        KeyCode::Enter if app.show_radar => {
            if app.drill_into_selected_cluster().is_none() {
                app.set_error("No error cluster selected to drill into.".to_string());
            }
            Action::None
        }

        // Toggle Distributed Trace Context Explorer (`x` key)
        KeyCode::Char('x') => match app.toggle_trace_context() {
            Ok(new_filter) => Action::ApplyFilter(new_filter),
            Err(err_msg) => {
                app.set_error(err_msg);
                Action::None
            }
        },

        // Toggle Surrounding Logs Context (`s` key — ±5s same-instance window)
        KeyCode::Char('s') => match app.toggle_surrounding_context() {
            Ok(new_filter) => Action::ApplyFilter(new_filter),
            Err(err_msg) => {
                app.set_error(err_msg);
                Action::None
            }
        },

        // AI explain current log (`e` key)
        KeyCode::Char('e') => {
            if app.ai_available && app.selected_log().is_some() {
                Action::ExplainLog
            } else if !app.ai_available {
                app.set_error("AI not available. Press 'm' or configure API key.".to_string());
                Action::None
            } else {
                Action::None
            }
        }

        // Full-Trace / Multi-Log AI Root Cause Analysis (`E` key — Shift+e)
        KeyCode::Char('E') => {
            if app.ai_available && !app.logs.is_empty() {
                Action::AnalyzeTrace
            } else if !app.ai_available {
                app.set_error("AI not available. Press 'm' or configure API key.".to_string());
                Action::None
            } else {
                app.set_error("No logs loaded to analyze.".to_string());
                Action::None
            }
        }

        // Toggle streaming
        KeyCode::Char('p') => {
            app.toggle_streaming();
            Action::None
        }

        // Toggle theme
        KeyCode::Char('t') => {
            app.toggle_theme();
            Action::None
        }

        // Switch AI model interactively
        KeyCode::Char('m') => Action::SwitchModel,

        // Clear search / exit Radar mode / exit Trace or Surrounding mode / clear filter
        KeyCode::Esc => {
            if app.show_radar {
                app.show_radar = false;
                return Action::None;
            }
            app.clear_search();
            app.clear_ai_response();
            if app.active_surrounding.is_some() {
                let restored = app.exit_surrounding_context();
                return Action::ApplyFilter(restored);
            }
            if app.active_trace.is_some() {
                let restored = app.exit_trace_context();
                return Action::ApplyFilter(restored);
            }
            let had_filter = app.filter.is_some();
            app.set_filter(None);
            if had_filter {
                Action::ApplyFilter(None)
            } else {
                Action::None
            }
        }

        _ => Action::None,
    }
}

/// Handle keys in input mode.
fn handle_input_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.exit_input_mode();
            Action::None
        }

        KeyCode::Enter => {
            let (text, mode) = app.submit_input();

            if text.is_empty() {
                return Action::None;
            }

            match mode {
                InputMode::Filter => {
                    app.active_trace = None;
                    app.active_surrounding = None;
                    app.anchor_log_key = None;
                    app.previous_filter = None;
                    app.set_filter(Some(text.clone()));
                    Action::ApplyFilter(Some(text))
                }
                InputMode::AiQuery => Action::AskQuestion(text),
                InputMode::Search => {
                    app.search(&text);
                    if app.search_matches.is_empty() {
                        app.set_error(format!("No matches for '{}'", text));
                    }
                    Action::None
                }
                InputMode::Normal => Action::None,
            }
        }

        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input_buffer.clear();
            Action::None
        }

        KeyCode::Char(c) => {
            app.input_char(c);
            Action::None
        }

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
    use crate::gcp::LogEntry;

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
        app.is_streaming = false;

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
    fn test_filter_submission_and_clear_triggers_action() {
        let mut app = App::new("test".to_string());

        handle_key_event(&mut app, make_key(KeyCode::Char('f')));
        assert_eq!(app.input_mode, InputMode::Filter);

        for c in "severity>=ERROR".chars() {
            handle_key_event(&mut app, make_key(KeyCode::Char(c)));
        }

        let action = handle_key_event(&mut app, make_key(KeyCode::Enter));
        assert!(matches!(action, Action::ApplyFilter(Some(ref f)) if f == "severity>=ERROR"));
        assert_eq!(app.filter.as_deref(), Some("severity>=ERROR"));

        let clear_action = handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(clear_action, Action::ApplyFilter(None)));
        assert!(app.filter.is_none());
    }

    #[test]
    fn test_radar_key_c_and_enter_drilldown() {
        let mut app = App::new("test".to_string()).with_offline_logs(vec![
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Redis connection failed on port 6379".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Request completed".to_string()),
                ..Default::default()
            },
        ]);

        handle_key_event(&mut app, make_key(KeyCode::Char('c')));
        assert!(app.show_radar);

        handle_key_event(&mut app, make_key(KeyCode::Enter));
        assert!(!app.show_radar);
        assert_eq!(app.logs.len(), 1);
    }

    #[test]
    fn test_trace_key_x_and_analyze_trace_key_shift_e() {
        let mut app = App::new("test".to_string()).with_offline_logs(vec![LogEntry {
            severity: Some("ERROR".to_string()),
            trace: Some("projects/demo/traces/abcdef123456".to_string()),
            text_payload: Some("DB error".to_string()),
            ..Default::default()
        }]);
        app.ai_available = true;

        let action = handle_key_event(&mut app, make_key(KeyCode::Char('x')));
        assert!(matches!(
            action,
            Action::ApplyFilter(Some(ref f)) if f == "trace=\"projects/demo/traces/abcdef123456\""
        ));
        assert_eq!(app.active_trace.as_deref(), Some("abcdef12"));

        let rca_action = handle_key_event(&mut app, make_key(KeyCode::Char('E')));
        assert!(matches!(rca_action, Action::AnalyzeTrace));

        let exit_action = handle_key_event(&mut app, make_key(KeyCode::Char('x')));
        assert!(matches!(exit_action, Action::ApplyFilter(None)));
        assert!(app.active_trace.is_none());
    }

    #[test]
    fn test_surrounding_key_s_and_esc_restore() {
        use chrono::{DateTime, Utc};
        let ts = DateTime::parse_from_rfc3339("2026-10-27T13:00:10Z")
            .unwrap()
            .with_timezone(&Utc);

        let mut app = App::new("test".to_string()).with_offline_logs(vec![LogEntry {
            insert_id: Some("err-1".to_string()),
            timestamp: Some(ts),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Crash".to_string()),
            ..Default::default()
        }]);

        let action = handle_key_event(&mut app, make_key(KeyCode::Char('s')));
        assert!(matches!(action, Action::ApplyFilter(Some(_))));
        assert!(app.active_surrounding.is_some());

        let esc_action = handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(esc_action, Action::ApplyFilter(None)));
        assert!(app.active_surrounding.is_none());
    }

    #[test]
    fn test_explain_requires_ai() {
        let mut app = App::new("test".to_string());
        app.add_log(LogEntry::default());

        app.ai_available = false;
        let action = handle_key_event(&mut app, make_key(KeyCode::Char('e')));
        assert!(matches!(action, Action::None));

        app.ai_available = true;
        let action = handle_key_event(&mut app, make_key(KeyCode::Char('e')));
        assert!(matches!(action, Action::ExplainLog));
    }
}
