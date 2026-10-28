//! Log list and Error Spike Radar widget.
//!
//! Displays either the scrollable list of log entries (with severity coloring
//! and distributed trace badges) or the ranked Error Fingerprint & Spike Radar
//! clusters when `app.show_radar` is active.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::ui::App;

/// Render the log list or the Error Spike Radar widget.
pub fn render_log_list(frame: &mut Frame, app: &App, area: Rect) {
    if app.show_radar {
        render_radar_list(frame, app, area);
        return;
    }

    let logs = &app.logs;
    let colors = app.colors();

    let items: Vec<ListItem> = logs
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let severity = entry.severity_or_default();
            let timestamp = entry.timestamp_str();
            let message = entry.message_preview();
            let anchor_mark = if app.active_surrounding.is_some() && app.is_anchor_log(entry) {
                "🎯 "
            } else {
                ""
            };
            let proj_tag = if app.is_multi_project {
                entry
                    .short_project_badge()
                    .map(|p| format!(" [{}]", p))
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let line = if let Some(badge) = entry.short_trace_badge() {
                format!(
                    "{}{} [{}]{} [{}] {}",
                    anchor_mark, timestamp, severity, proj_tag, badge, message
                )
            } else {
                format!(
                    "{}{} [{}]{} {}",
                    anchor_mark, timestamp, severity, proj_tag, message
                )
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

    let title = if let Some(ref scope) = app.active_surrounding {
        format!(
            " 🕵️ Surrounding Context: {} ({} logs) — s/Esc: exit, E: Context AI RCA ",
            scope,
            logs.len()
        )
    } else if let Some(ref trace_id) = app.active_trace {
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
        format!(
            " Logs ({}) — 'c': Error Spike Radar │ 's': ±5s Surrounding Context ",
            logs.len()
        )
    };

    let visible_height = area.height.saturating_sub(2) as usize;
    let offset = app.selected_index.saturating_sub(visible_height / 2);

    let border_style = if app.active_surrounding.is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if app.active_trace.is_some() {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
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

/// Render the ranked Error Fingerprint & Spike Radar clusters (`c` key).
fn render_radar_list(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let clusters = &app.radar.clusters;

    let items: Vec<ListItem> = if clusters.is_empty() {
        vec![
            ListItem::new(" 🟢 No WARNING/ERROR/CRITICAL patterns detected in current buffer.")
                .style(colors.dim_style()),
        ]
    } else {
        clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| {
                let spike_badge = if cluster.is_spike { "🚨 " } else { "🔥 " };
                let line = format!(
                    " {}[x{:>3}] [{}] {}  (first: {}, last: {})",
                    spike_badge,
                    cluster.count,
                    cluster.severity,
                    cluster.fingerprint,
                    cluster.first_seen,
                    cluster.last_seen
                );

                let mut style = colors.severity_style(&cluster.severity);
                if i == app.radar_selected_index {
                    style = colors.selection_style();
                }

                ListItem::new(line).style(style)
            })
            .collect()
    };

    let title = format!(
        " 🚨 Error Fingerprint & Spike Radar ({} patterns │ {} total errs │ Velocity: {}) — Enter: drill down, e: AI explain, c/Esc: back ",
        clusters.len(),
        app.radar.total_error_logs,
        app.radar.sparkline()
    );

    let visible_height = area.height.saturating_sub(2) as usize;
    let offset = app.radar_selected_index.saturating_sub(visible_height / 2);

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(colors.selection_style());

    let mut state = ListState::default();
    if !clusters.is_empty() {
        state.select(Some(app.radar_selected_index));
        *state.offset_mut() = offset;
    }

    frame.render_stateful_widget(list, area, &mut state);
}
