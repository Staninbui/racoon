//! Log list widget.
//!
//! Displays a scrollable list of log entries with severity-based coloring.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::ui::App;

/// Render the log list widget.
pub fn render_log_list(frame: &mut Frame, app: &App, area: Rect) {
    let logs = &app.logs;
    let colors = app.colors();

    // Convert logs to list items with coloring
    let items: Vec<ListItem> = logs
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let severity = entry.severity_or_default();
            let timestamp = entry.timestamp_str();
            let message = entry.message_preview();

            // Format the line
            let line = format!("{} [{}] {}", timestamp, severity, message);

            // Color based on severity
            let mut style = colors.severity_style(severity);

            // Highlight search matches with a different background
            if app.is_search_match(i) {
                style = style.bg(colors.search_match_bg);
            }

            // Highlight selected item
            if i == app.selected_index {
                style = colors.selection_style();
            }

            ListItem::new(line).style(style)
        })
        .collect();

    // Update title to show search status
    let title = if let Some(ref query) = app.search_query {
        format!(
            " Logs ({}) - Search: '{}' [{}/{}], n/N: next/prev, Esc: clear",
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

    // Calculate scroll offset to keep selection centered in view
    let visible_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let offset = app.selected_index.saturating_sub(visible_height / 2);

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(colors.border_focused_style()),
        )
        .highlight_style(colors.selection_style());

    // Create list state with selection and scroll offset
    let mut state = ListState::default();
    state.select(Some(app.selected_index));
    *state.offset_mut() = offset; // Keep selection centered

    frame.render_stateful_widget(list, area, &mut state);
}
