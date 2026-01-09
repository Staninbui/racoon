//! Log detail and AI response widget.
//!
//! Displays the full JSON of the selected log entry, and AI analysis
//! when available.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::App;

/// Render the detail pane (log JSON + AI response).
pub fn render_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
    // Split vertically: top for log detail, bottom for AI
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60), // Log detail
            Constraint::Percentage(40), // AI Response
        ])
        .split(area);

    render_log_detail(frame, app, chunks[0]);
    render_ai_pane(frame, app, chunks[1]);
}

/// Render the log detail (JSON) view.
fn render_log_detail(frame: &mut Frame, app: &App, area: Rect) {
    let content = if let Some(entry) = app.selected_log() {
        // Pretty-print the log entry as JSON
        serde_json::to_string_pretty(entry)
            .unwrap_or_else(|_| "[Failed to serialize]".to_string())
    } else {
        "No log selected".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(" Detail ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
        )
        .wrap(Wrap { trim: false })
        .scroll((0, 0)); // TODO: Add scrolling support

    frame.render_widget(paragraph, area);
}

/// Render the AI response pane.
fn render_ai_pane(frame: &mut Frame, app: &App, area: Rect) {
    let (content, style) = if app.ai_loading {
        (
            "⏳ Analyzing with AI...".to_string(),
            Style::default().fg(Color::Yellow),
        )
    } else if let Some(ref response) = app.ai_response {
        (response.clone(), Style::default().fg(Color::Cyan))
    } else {
        (
            "Press 'e' to explain selected log with AI\nPress '?' to ask a natural language question".to_string(),
            Style::default().fg(Color::DarkGray),
        )
    };


    let paragraph = Paragraph::new(content)
        .style(style)
        .block(
            Block::default()
                .title(" AI Analysis ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}