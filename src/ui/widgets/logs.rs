//! Log list widget.
//!
//! Displays a scrollable list of log entries with severity-based coloring
//! and distributed trace badges.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::ui::App;

/// Render the log list widget.
pub fn render_log_list(frame: &mut Frame, app: &App, area: Rect) {
    let logs = &app.logs;
    let colors = app.colors();

    // Convert logs to list items with coloring and optional trace badge
    let items: Vec<ListItem> = logs
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let severity = entry.severity_or_default();
            let timestamp = entry.timestamp_str();
            let message = entry.message_preview();

            let line = if let Some(badge) = entry.short_trace_badge() {
                format!("{} [{}] [{}] {}", timestamp, severity, badge, message)
            } else {
                format!("{} [{}] {}", timestamp, severity, message)
            };

            let mut style = colors.severity_style(severity);

            if app.is_search_match(i) {
                style = style.bg(colors.search_match_bg);
            }

            if i == app.selected_index {
                style = colors.selection_style();
            }

            ListItem::new(line).style(style)
        })
        .collect();

    // Update title to show Trace Context mode or search status
    let title = if let Some(ref trace_id) = app.active_trace {
        format!(
            " 🔗 Trace Context: {} ({} logs) — x/Esc: exit, E: Full-Trace AI RCA ",
            trace_id,
            logs.len()
        )
    } else if let Some(ref query) = app.search_query {
        format!(
            " Logs ({}) - Search: '{}' [{}/{}], n/N: next/prev, Esc: clear ",
            logs.len(),
            query,
            if app.search_matches.is_empty() {
                0
            } else {
                app.search_match_index + 1
            },
            app.search_matches.len()
        )
    } else {
        format!(" Logs ({}) ", logs.len())
    };

    let visible_height = area.height.saturating_sub(2) as usize;
    let offset = app.selected_index.saturating_sub(visible_height / 2);

    let border_style = if app.active_trace.is_some() {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        colors.border_focused_style()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(colors.selection_style());

    let mut state = ListState::default();
    state.select(Some(app.selected_index));
    *state.offset_mut() = offset;

    frame.render_stateful_widget(list, area, &mut state);
}
