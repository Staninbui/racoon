//! Log list widget.
//!
//! Displays a scrollable list of log entries with severity-based coloring.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::ui::App;

/// Render the log list widget.
pub fn render_log_list(frame: &mut Frame, app: &App, area: Rect) {
    let logs = &app.logs;

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
            let mut style = severity_style(severity);

            // Highlight search matches with a different background
            if app.is_search_match(i) {
                style = style.bg(Color::DarkGray);
            }

            // Highlight selected item
            if i == app.selected_index {
                style = style.add_modifier(Modifier::REVERSED);
            }

            // Highlight selected item differently
            let style = if i == app.selected_index {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };

            ListItem::new(line).style(style)
        })
        .collect();

    // Update title to show search status
    let title = if let Some(ref query) = app.search_query {
        format!(
            " Logs ({}) - Search: '{}' [{}/{}] ",
            logs.len(),
            query,
            if app.search_matches.is_empty() { 0 } else { app.search_match_index + 1 },
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
                .border_style(Style::default().fg(Color::Blue))
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    // Create list state with selection and scroll offset
    let mut state = ListState::default();
    state.select(Some(app.selected_index));
    *state.offset_mut() = offset;  // Keep selection centered

    frame.render_stateful_widget(list, area, &mut state);
}

/// Get the style for a severity level.
fn severity_style(severity: &str) -> Style {
    match severity {
        "EMERGENCY" | "ALERT" | "CRITICAL" => {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        }
        "ERROR" => Style::default().fg(Color::Red),
        "WARNING" => Style::default().fg(Color::Yellow),
        "NOTICE" => Style::default().fg(Color::Cyan),
        "INFO" => Style::default().fg(Color::Green),
        "DEBUG" => Style::default().fg(Color::Gray),
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_style_colors() {
        assert_eq!(severity_style("ERROR").fg, Some(Color::Red));
        assert_eq!(severity_style("WARNING").fg, Some(Color::Yellow));
        assert_eq!(severity_style("INFO").fg, Some(Color::Green));
    }
}