//! Log detail and AI response widget.
//!
//! Displays the full JSON of the selected log entry, and AI analysis
//! when available.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::App;

/// Render the log detail pane (JSON view only).
pub fn render_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
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
pub fn render_ai_pane(frame: &mut Frame, app: &App, area: Rect) {
    let (content, style) = if app.ai_loading {
        let loading_text = if let Some(ref query) = app.ai_query {
            format!("⏳ Analyzing: \"{}\"...", query)
        } else {
            "⏳ Analyzing...".to_string()
        };
        (loading_text, Style::default().fg(Color::Yellow))
    } else if let Some(ref response) = app.ai_response {
        (response.clone(), Style::default().fg(Color::Cyan))
    } else {
        let help = vec![
            "Press 'e' to explain selected log",
            "Press '?' to ask a question about your logs",
            "",
            "Example questions:",
            "  • Are there any errors today?",
            "  • 最近有发生过严重报错吗?",
            "  • What caused the timeout at 14:00?",
        ].join("\n");
        (help, Style::default().fg(Color::DarkGray))
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