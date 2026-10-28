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
    } else if let Some(ref status) = app.status_message {
        Paragraph::new(format!("✅ {}", status))
            .style(Style::default().fg(Color::Green))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
    } else if app.show_radar {
        Paragraph::new(format!(
            "🚨 Error Fingerprint & Spike Radar │ {} patterns ({} total errs) │ Velocity: {}\nj/k: Select Cluster │ Enter: Drill down │ y: Copy JSON │ e: AI Explain │ c/Esc: Back to Log List",
            app.radar.clusters.len(),
            app.radar.total_error_logs,
            app.radar.sparkline()
        ))
        .style(Style::default().fg(Color::LightRed))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightRed)),
        )
    } else if let Some(ref scope) = app.active_surrounding {
        let filter_str = app.filter.as_deref().unwrap_or("");
        Paragraph::new(format!(
            "🕵️ Surrounding Mode ({}) │ 🎯 marks anchor log │ Filter: {}\ns/Esc: Exit & Restore │ E: Context AI RCA │ y/Y: Copy │ o: Open GCP │ e: Explain",
            scope, filter_str
        ))
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
    } else if let Some(ref trace_id) = app.active_trace {
        let filter_str = app.filter.as_deref().unwrap_or("");
        Paragraph::new(format!(
            "🔗 Trace Mode ({}) │ Filter: {}\nx/Esc: Exit Trace & Restore │ s: ±5s Context │ E: Full-Trace AI RCA │ y/Y: Copy │ o: Open GCP",
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
            "Filter: {} │ f: Change │ s: ±5s │ c: Radar │ x: Trace │ E: AI RCA │ y/Y: Copy │ o: GCP │ Esc: Clear",
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
j/k: Nav │ s: ±5s Context │ c: Radar │ x: Trace │ e/E: AI Explain/RCA │ y/Y: Copy │ o: Open GCP │ ?: Ask AI │ m: Model
/: Search │ f: Filter │ g/G: First/Last │ p: Pause │ t: Theme │ Ctrl+j/k: Scroll Detail │ Ctrl+n/p: Scroll AI │ Esc: Clear │ q: Quit";
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
        InputMode::Filter => (
            " Filter (↑/↓ history, Enter to apply, Esc to cancel) ",
            Color::Blue,
        ),
        InputMode::Search => (
            " Search (↑/↓ history, Enter to search, Esc to cancel) ",
            Color::Green,
        ),
        InputMode::AiQuery => (
            " AI Query (↑/↓ history, Enter to ask, Esc to cancel) ",
            Color::Yellow,
        ),
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
