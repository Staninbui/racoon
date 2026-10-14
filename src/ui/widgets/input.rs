//! Input bar widget.
//!
//! Displays the command/filter input at the bottom of the screen,
//! along with keyboard hints.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ui::App;
use crate::ui::app::InputMode;

/// Render the input bar at the bottom of the screen.
pub fn render_input_bar(frame: &mut Frame, app: &App, area: Rect) {
    match app.input_mode {
        InputMode::Normal => render_hints(frame, app, area),
        _ => render_input_field(frame, app, area),
    }
}

/// Render keyboard hints in normal mode.
fn render_hints(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let hints = if let Some(ref error) = app.error_message {
        Paragraph::new(format!("❌ {}", error))
            .style(colors.error_style())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(colors.error_style()),
            )
    } else if let Some(ref trace_id) = app.active_trace {
        let filter_str = app.filter.as_deref().unwrap_or("");
        Paragraph::new(format!(
            "🔗 Trace Mode ({}) │ Filter: {}\nx/Esc: Exit Trace & Restore Stream │ E: Full-Trace AI Root Cause Analysis │ e: Explain Log",
            trace_id, filter_str
        ))
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
    } else if let Some(ref filter) = app.filter {
        Paragraph::new(format!(
            "Filter: {} │ Press 'f' to change, 'x' to trace request, 'E' for AI RCA, Esc to clear",
            filter
        ))
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
    } else {
        let hints_text = "\
j/k: Nav │ x: Trace │ e: Explain │ E: Trace RCA │ ?: AI Query │ m: Model │ /: Search │ f: Filter │ p: Pause │ q: Quit
g/G: First/Last │ PgUp/PgDn: ±10 │ t: Theme │ Ctrl+j/k: Scroll Detail │ Ctrl+n/p: Scroll AI │ Esc: Clear";
        Paragraph::new(hints_text).style(colors.dim_style()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(colors.border_style()),
        )
    };

    frame.render_widget(hints, area);
}

/// Render the input field when in input mode.
fn render_input_field(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = app.input_mode.prompt();
    let input = &app.input_buffer;

    let display_text = format!("{}{}_", prompt, input);

    let (title, border_color) = match app.input_mode {
        InputMode::Filter => (" Filter (Enter to apply, Esc to cancel) ", Color::Blue),
        InputMode::Search => (" Search (Enter to search, Esc to cancel) ", Color::Green),
        InputMode::AiQuery => (" AI Query (Enter to ask, Esc to cancel) ", Color::Yellow),
        InputMode::Normal => ("", Color::Gray),
    };

    let paragraph = Paragraph::new(display_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

    frame.render_widget(paragraph, area);
}
