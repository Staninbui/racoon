//! Keyboard event handling for the UI
//!
//! This module handles all keyboard input and translates it into
//! application state changes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{ActivePane, App, InputMode};

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
    /// Copy text to the system clipboard with a human-readable description.
    CopyToClipboard { text: String, label: &'static str },
    /// Open a URL in the default system browser.
    OpenInBrowser(String),
    /// Export a self-contained Markdown incident report (`w` key).
    SaveIncidentReport,
}

/// Handle a keyboard event
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    app.clear_error();
    app.clear_status();

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

        // Navigation (routes to the currently focused pane: Logs, Detail, or AI)
        KeyCode::Char('j') | KeyCode::Down => {
            match app.focused_pane {
                ActivePane::Logs => app.select_next(),
                ActivePane::Detail => app.scroll_detail_down(),
                ActivePane::Ai => app.scroll_ai_down(),
            }
            Action::None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            match app.focused_pane {
                ActivePane::Logs => app.select_previous(),
                ActivePane::Detail => app.scroll_detail_up(),
                ActivePane::Ai => app.scroll_ai_up(),
            }
            Action::None
        }
        KeyCode::Char('g') | KeyCode::Home => {
            match app.focused_pane {
                ActivePane::Logs => app.select_first(),
                ActivePane::Detail => app.reset_detail_scroll(),
                ActivePane::Ai => app.reset_ai_scroll(),
            }
            Action::None
        }
        KeyCode::Char('G') | KeyCode::End => {
            match app.focused_pane {
                ActivePane::Logs => app.select_last(),
                ActivePane::Detail => {
                    for _ in 0..50 {
                        app.scroll_detail_down();
                    }
                }
                ActivePane::Ai => {
                    for _ in 0..50 {
                        app.scroll_ai_down();
                    }
                }
            }
            Action::None
        }
        KeyCode::PageDown => {
            for _ in 0..10 {
                match app.focused_pane {
                    ActivePane::Logs => app.select_next(),
                    ActivePane::Detail => app.scroll_detail_down(),
                    ActivePane::Ai => app.scroll_ai_down(),
                }
            }
            Action::None
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                match app.focused_pane {
                    ActivePane::Logs => app.select_previous(),
                    ActivePane::Detail => app.scroll_detail_up(),
                    ActivePane::Ai => app.scroll_ai_up(),
                }
            }
            Action::None
        }

        // Cycle pane focus (`Tab` / `Shift+Tab`) and toggle full-screen maximize (`z`)
        KeyCode::Tab => {
            app.focus_next_pane();
            Action::None
        }
        KeyCode::BackTab => {
            app.focus_prev_pane();
            Action::None
        }
        KeyCode::Char('z') => {
            app.toggle_maximize_pane();
            Action::None
        }

        // Mute / exclude selected log pattern (`-` key)
        KeyCode::Char('-') => {
            if let Err(err_msg) = app.mute_selected_pattern() {
                app.set_error(err_msg);
            }
            Action::None
        }

        // Resize Surrounding Context window (`[` to shrink, `]` / `+` / `=` to expand)
        KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=')
            if app.active_surrounding.is_some() =>
        {
            match app.resize_surrounding_window(true) {
                Ok(new_filter) => Action::ApplyFilter(Some(new_filter)),
                Err(err_msg) => {
                    app.set_error(err_msg);
                    Action::None
                }
            }
        }
        KeyCode::Char('[') if app.active_surrounding.is_some() => {
            match app.resize_surrounding_window(false) {
                Ok(new_filter) => Action::ApplyFilter(Some(new_filter)),
                Err(err_msg) => {
                    app.set_error(err_msg);
                    Action::None
                }
            }
        }

        // Save self-contained Markdown incident snapshot (`w` key)
        KeyCode::Char('w') => Action::SaveIncidentReport,

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

        // Yank selected log JSON (or Error Cluster summary + JSON) to clipboard (`y` key)
        KeyCode::Char('y') => match app.yank_selected_log_text() {
            Ok(text) => {
                let label = if app.show_radar {
                    "error cluster JSON"
                } else {
                    "selected log JSON"
                };
                Action::CopyToClipboard { text, label }
            }
            Err(err_msg) => {
                app.set_error(err_msg);
                Action::None
            }
        },

        // Yank AI analysis Markdown to clipboard (`Y` / `Shift+y` key)
        KeyCode::Char('Y') => match app.yank_ai_response_text() {
            Ok(text) => Action::CopyToClipboard {
                text,
                label: "AI analysis Markdown",
            },
            Err(err_msg) => {
                app.set_error(err_msg);
                Action::None
            }
        },

        // Open selected log entry in Google Cloud Logs Explorer (`o` key)
        KeyCode::Char('o') => match app.selected_log_console_url() {
            Ok(url) => Action::OpenInBrowser(url),
            Err(err_msg) => {
                app.set_error(err_msg);
                Action::None
            }
        },

        // Clear maximize / search / exit Radar mode / exit Trace or Surrounding mode / clear filter
        KeyCode::Esc => {
            if app.maximized_pane.is_some() {
                app.maximized_pane = None;
                return Action::None;
            }
            if app.show_radar {
                app.show_radar = false;
                return Action::None;
            }
            app.clear_search();
            app.clear_ai_response();
            if let Some(restored) = app.exit_active_overlay() {
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

        KeyCode::Up => {
            app.history_prev();
            Action::None
        }

        KeyCode::Down => {
            app.history_next();
            Action::None
        }

        KeyCode::Enter => {
            let (text, mode) = app.submit_input();

            if text.is_empty() {
                return Action::None;
            }

            match mode {
                InputMode::Filter => {
                    app.clear_context_overlay();
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

        // Pressing Esc after drilling into a cluster must restore the previous log list / filter
        handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_radar_drilldown_preserves_and_restores_previous_filter() {
        let mut app = App::new("test".to_string()).with_offline_logs(vec![
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Redis connection failed on port 6379".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("WARNING".to_string()),
                text_payload: Some("High memory usage 85%".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Request completed".to_string()),
                ..Default::default()
            },
        ]);

        // Start with severity>=WARNING (2 logs visible)
        app.set_filter(Some("severity>=WARNING".to_string()));
        assert_eq!(app.logs.len(), 2);

        // Open Radar ('c') and drill into top cluster ('Enter') -> 1 log visible
        handle_key_event(&mut app, make_key(KeyCode::Char('c')));
        handle_key_event(&mut app, make_key(KeyCode::Enter));
        assert!(!app.show_radar);
        assert_eq!(app.logs.len(), 1);

        // Press Esc -> should restore previous filter ("severity>=WARNING", 2 logs visible)
        let esc_action = handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(
            esc_action,
            Action::ApplyFilter(Some(ref f)) if f == "severity>=WARNING"
        ));
        assert_eq!(app.filter.as_deref(), Some("severity>=WARNING"));
        assert_eq!(app.logs.len(), 2);
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

    #[test]
    fn test_yank_keys_y_and_shift_y_and_open_browser_o() {
        let mut app = App::new("prod-gcp-proj".to_string()).with_offline_logs(vec![LogEntry {
            insert_id: Some("ins-100".to_string()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Gateway timeout".to_string()),
            ..Default::default()
        }]);

        let y_action = handle_key_event(&mut app, make_key(KeyCode::Char('y')));
        assert!(matches!(
            y_action,
            Action::CopyToClipboard {
                label: "selected log JSON",
                ref text
            } if text.contains("Gateway timeout")
        ));

        // Pressing `Y` before AI response is present sets an error message
        let shift_y_empty = handle_key_event(&mut app, make_key(KeyCode::Char('Y')));
        assert!(matches!(shift_y_empty, Action::None));
        assert!(app.error_message.is_some());

        app.set_ai_response("### Root Cause\nUpstream timed out".to_string());
        let shift_y_ok = handle_key_event(&mut app, make_key(KeyCode::Char('Y')));
        assert!(matches!(
            shift_y_ok,
            Action::CopyToClipboard {
                label: "AI analysis Markdown",
                ref text
            } if text.contains("Upstream timed out")
        ));

        let o_action = handle_key_event(&mut app, make_key(KeyCode::Char('o')));
        assert!(matches!(
            o_action,
            Action::OpenInBrowser(ref url) if url.contains("project=prod-gcp-proj")
        ));
    }

    #[test]
    fn test_input_mode_up_down_history_keys() {
        let mut app = App::new("test".to_string());

        handle_key_event(&mut app, make_key(KeyCode::Char('f')));
        for c in "severity>=ERROR".chars() {
            handle_key_event(&mut app, make_key(KeyCode::Char(c)));
        }
        handle_key_event(&mut app, make_key(KeyCode::Enter));

        handle_key_event(&mut app, make_key(KeyCode::Char('f')));
        assert_eq!(app.input_buffer, "");

        handle_key_event(&mut app, make_key(KeyCode::Up));
        assert_eq!(app.input_buffer, "severity>=ERROR");

        handle_key_event(&mut app, make_key(KeyCode::Down));
        assert_eq!(app.input_buffer, "");
    }

    #[test]
    fn test_tab_focus_z_maximize_mute_and_incident_export_keys() {
        use crate::ui::app::ActivePane;

        let mut app = App::new("prod-proj".to_string()).with_offline_logs(vec![
            LogEntry {
                severity: Some("WARNING".to_string()),
                text_payload: Some("Noisy probe 1".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Critical failure".to_string()),
                ..Default::default()
            },
        ]);

        // Tab cycles focus Logs -> Detail -> Ai -> Logs
        handle_key_event(&mut app, make_key(KeyCode::Tab));
        assert_eq!(app.focused_pane, ActivePane::Detail);

        // In Detail focus, 'j' scrolls detail pane instead of moving log selection
        handle_key_event(&mut app, make_key(KeyCode::Char('j')));
        assert_eq!(app.detail_scroll, 1);
        assert_eq!(app.selected_index, 0);

        // 'z' maximizes the currently focused pane (Detail)
        handle_key_event(&mut app, make_key(KeyCode::Char('z')));
        assert_eq!(app.maximized_pane, Some(ActivePane::Detail));

        // Pressing Esc exits maximize mode first
        handle_key_event(&mut app, make_key(KeyCode::Esc));
        assert!(app.maximized_pane.is_none());

        // Return focus to Logs (`Shift+Tab` / BackTab)
        handle_key_event(&mut app, make_key(KeyCode::BackTab));
        assert_eq!(app.focused_pane, ActivePane::Logs);

        // '-' mutes the selected noisy log pattern
        handle_key_event(&mut app, make_key(KeyCode::Char('-')));
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].message(), "Critical failure");

        // 'w' triggers SaveIncidentReport action
        let w_action = handle_key_event(&mut app, make_key(KeyCode::Char('w')));
        assert!(matches!(w_action, Action::SaveIncidentReport));
    }
}
