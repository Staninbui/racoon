//! Log detail and AI response widget.
//!
//! Displays the full JSON of the selected log entry (with trace/span metadata),
//! and AI analysis when available.

use ratatui::prelude::*;
use ratatui::widgets::Scrollbar;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::App;

/// Render the log detail pane (trace summary + JSON view).
pub fn render_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let content = if let Some(entry) = app.selected_log() {
        let json = serde_json::to_string_pretty(entry)
            .unwrap_or_else(|_| "[Failed to serialize]".to_string());

        if let Some((filter_clause, _)) = entry.correlation_id() {
            let span_info = entry
                .span_id
                .as_deref()
                .map(|s| format!(" │ Span: {}", s))
                .unwrap_or_default();
            format!(
                "🔗 {}{}  [x: Toggle Trace | E: Trace AI RCA]\n────────────────────────────────────────────────────\n{}",
                filter_clause, span_info, json
            )
        } else {
            json
        }
    } else {
        "No log selected".to_string()
    };

    let line_count = content.lines().count() as u16;

    let paragraph = Paragraph::new(content)
        .style(Style::default().fg(colors.fg))
        .block(
            Block::default()
                .title(" Detail (Ctrl+j/k to scroll) ")
                .borders(Borders::ALL)
                .border_style(colors.border_focused_style()),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    frame.render_widget(paragraph, area);

    if line_count > area.height {
        let scrollbar = Scrollbar::default()
            .orientation(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state = ratatui::widgets::ScrollbarState::default()
            .content_length(line_count as usize)
            .position(app.detail_scroll as usize);

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

/// Render the AI response pane.
pub fn render_ai_pane(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let (content, style) = if app.ai_loading {
        let loading_text = if let Some(ref query) = app.ai_query {
            format!("⏳ Analyzing: \"{}\"...", query)
        } else {
            "⏳ Analyzing...".to_string()
        };
        (loading_text, Style::default().fg(colors.ai_loading))
    } else if let Some(ref response) = app.ai_response {
        (response.clone(), colors.ai_style())
    } else {
        let help = [
            "Press 'e' to explain selected log",
            "Press 'x' to isolate full request trace across services",
            "Press 'E' (Shift+e) for Full-Trace / Multi-Log AI Root Cause Analysis",
            "Press '?' to ask a natural-language question about your logs",
            "Press 'm' to switch AI Provider & Model interactively",
        ]
        .join("\n");
        (help, colors.dim_style())
    };

    let paragraph = Paragraph::new(content)
        .style(style)
        .block(
            Block::default()
                .title(" AI Analysis (Ctrl+n/p to scroll) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors.ai_border)),
        )
        .wrap(Wrap { trim: true })
        .scroll((app.ai_scroll, 0));

    frame.render_widget(paragraph, area);
}
