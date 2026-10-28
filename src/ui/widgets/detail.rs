//! Log detail and AI response widget.
//!
//! Displays the full JSON of the selected log entry (with trace/span or error
//! cluster metadata), and AI analysis when available.

use ratatui::prelude::*;
use ratatui::widgets::Scrollbar;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::App;

/// Render the log detail pane (cluster/trace summary + JSON view).
pub fn render_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let content = if let Some(cluster) = app.selected_cluster() {
        let json = serde_json::to_string_pretty(&cluster.sample_entry)
            .unwrap_or_else(|_| "[Failed to serialize]".to_string());
        format!(
            "🚨 Error Cluster [x{} occurrences │ {} │ first: {} │ last: {}]\nFingerprint: {}\n[Enter: Drill down into logs │ e: AI Explain │ c/Esc: Back]\n────────────────────────────────────────────────────\n{}",
            cluster.count,
            cluster.severity,
            cluster.first_seen,
            cluster.last_seen,
            cluster.fingerprint,
            json
        )
    } else if let Some(entry) = app.selected_log() {
        let json = serde_json::to_string_pretty(entry)
            .unwrap_or_else(|_| "[Failed to serialize]".to_string());

        if let Some((filter_clause, _)) = entry.correlation_id() {
            let span_info = entry
                .span_id
                .as_deref()
                .map(|s| format!(" │ Span: {}", s))
                .unwrap_or_default();
            format!(
                "🔗 {}{}  [x: Toggle Trace | s: ±5s Context | E: AI RCA]\n────────────────────────────────────────────────────\n{}",
                filter_clause, span_info, json
            )
        } else if let Some((_, scope_badge)) = entry.surrounding_context(5) {
            format!(
                "🕵️ Scope: {}  [s: Toggle ±5s Surrounding Logs | E: Context AI RCA]\n────────────────────────────────────────────────────\n{}",
                scope_badge, json
            )
        } else {
            json
        }
    } else {
        "No log selected".to_string()
    };

    let line_count = content.lines().count() as u16;

    let detail_focused = app.focused_pane == crate::ui::ActivePane::Detail;
    let detail_max = app.maximized_pane == Some(crate::ui::ActivePane::Detail);
    let detail_title = if detail_max {
        " Detail [⛶ MAXIMIZED — z/Esc: restore │ j/k: scroll │ y: copy │ o: GCP] "
    } else if detail_focused {
        " ● Detail (j/k: scroll, z: maximize, y: copy JSON, o: open GCP) "
    } else {
        " Detail (Tab: focus, z: maximize, y: copy JSON, o: open GCP, Ctrl+j/k: scroll) "
    };
    let detail_border = if detail_focused {
        colors.border_focused_style()
    } else {
        colors.border_style()
    };

    let styled_lines: Vec<Line> = if let Some(ref q) = app.search_query {
        let needle = q.to_lowercase();
        content
            .lines()
            .map(|l| {
                if !needle.is_empty() && l.to_lowercase().contains(&needle) {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(colors.fg).bg(colors.search_match_bg),
                    ))
                } else {
                    Line::from(Span::styled(l.to_string(), Style::default().fg(colors.fg)))
                }
            })
            .collect()
    } else {
        content
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(colors.fg))))
            .collect()
    };

    let paragraph = Paragraph::new(styled_lines)
        .block(
            Block::default()
                .title(detail_title)
                .borders(Borders::ALL)
                .border_style(detail_border),
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
            "Press 'e' to explain selected log (or selected Error Cluster)",
            "Press 's' to inspect surrounding logs ([ / ] to resize ±5s..±300s)",
            "Press 'c' to toggle Error Fingerprint & Spike Radar",
            "Press '+' to pin service/resource or '-' to mute noisy pattern",
            "Press 'b' to cycle through saved SRE filter bookmarks",
            "Press 'x' to isolate full request trace across services",
            "Press 'E' (Shift+e) for Full-Trace / Context AI Root Cause Analysis",
            "Press 'w' to save a Markdown Incident Snapshot Report",
            "Press 'Tab' to switch pane focus and 'z' to toggle full-screen maximize",
            "Press 'y' / 'Y' to copy Log JSON or AI Analysis to clipboard",
            "Press 'o' to open selected log in Google Cloud Logs Explorer",
            "Press '?' to ask a natural-language question or follow-up",
            "Press 'h' or 'F1' to open full Keyboard Shortcut Cheat Sheet",
            "Press 'm' to switch AI Provider & Model interactively",
        ]
        .join("\n");
        (help, colors.dim_style())
    };

    let ai_focused = app.focused_pane == crate::ui::ActivePane::Ai;
    let ai_max = app.maximized_pane == Some(crate::ui::ActivePane::Ai);
    let ai_title = match (app.ai_pii_guard, ai_max, ai_focused) {
        (true, true, _) => {
            " AI Analysis 🛡️ PII Guard [⛶ MAXIMIZED — z/Esc: restore │ j/k: scroll │ Y: copy] "
        }
        (false, true, _) => " AI Analysis [⛶ MAXIMIZED — z/Esc: restore │ j/k: scroll │ Y: copy] ",
        (true, false, true) => {
            " ● AI Analysis 🛡️ PII Guard (j/k: scroll, z: maximize, Y: copy, w: report) "
        }
        (false, false, true) => " ● AI Analysis (j/k: scroll, z: maximize, Y: copy, w: report) ",
        (true, false, false) => {
            " AI Analysis 🛡️ PII Guard (Tab: focus, z: max, Y: copy, Ctrl+n/p: scroll) "
        }
        (false, false, false) => " AI Analysis (Tab: focus, z: max, Y: copy, Ctrl+n/p: scroll) ",
    };

    let ai_border_style = if ai_focused {
        Style::default()
            .fg(colors.ai_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.ai_border)
    };

    let paragraph = Paragraph::new(content)
        .style(style)
        .block(
            Block::default()
                .title(ai_title)
                .borders(Borders::ALL)
                .border_style(ai_border_style),
        )
        .wrap(Wrap { trim: true })
        .scroll((app.ai_scroll, 0));

    frame.render_widget(paragraph, area);
}

/// Render the full-pane Keyboard Shortcut Help Cheat Sheet (`h` / `F1` key).
pub fn render_help_modal(frame: &mut Frame, app: &App, area: Rect) {
    let colors = app.colors();
    let cheat_sheet = "\
🧭 NAVIGATION & PANES
  j / k or ↓ / ↑     Move selection (Logs) or scroll active pane (Detail / AI)
  g / G or Home/End  Jump to first / last log entry (or top / bottom of pane)
  PageUp / PageDown  Scroll 10 lines up / down in the focused pane
  Tab / Shift+Tab    Cycle keyboard focus across Logs → Detail → AI panes
  z                  Toggle full-screen maximize for the focused pane
  Ctrl+j / Ctrl+k    Scroll Detail JSON pane down / up from any focus
  Ctrl+n / Ctrl+p    Scroll AI Analysis pane down / up from any focus

🚨 INCIDENT TRIAGE & CONTEXT
  c                  Toggle Error Fingerprint & Spike Radar (velocity sparklines)
  Enter (in Radar)   Drill down into logs matching the selected Error Cluster
  s                  Toggle ±5s Surrounding Context on the same instance/Pod
  [ / ]              Shrink / expand Surrounding Context window (±5s ↔ ±15s ↔ ±30s ↔ ±60s ↔ ±300s)
  x                  Isolate all logs sharing the selected entry's Trace / Span / Request ID

🔍 FILTERING, BOOKMARKS & NOISE CONTROL
  f                  Enter GCP Filter input mode (↑ / ↓ to recall persistent history)
  b                  Cycle through saved user & built-in SRE filter bookmarks
  + / =              Pin / isolate logs from the selected entry's service or resource
  -                  Mute / exclude the selected noisy log pattern (-fingerprint:\"...\")
  / , n / N          Search buffered logs (highlights matches in both List & Detail JSON)
  Esc                Step-by-step restore: close Help/Maximize → exit Context/Mute/Pin → clear Filter

🤖 AI ROOT CAUSE ANALYSIS
  e                  Explain selected log entry (or selected Error Cluster) with AI
  E (Shift+e)        Run Full-Trace / Multi-Log Root Cause Analysis (RCA)
  ?                  Ask natural-language question (retains prior AI diagnosis for multi-turn follow-up)
  m                  Open interactive AI Provider & Model selector (Gemini, Vertex ADC, OpenAI, Claude, Agnes)

📤 EXPORT & ACTIONS
  w                  Save self-contained Markdown Incident Snapshot (incident-<timestamp>.md)
  y / Y              Copy selected Log JSON (y) or AI Analysis Markdown (Y) to clipboard
  o                  Open selected log in Google Cloud Logs Explorer in browser
  p / t / h / q      Pause/Resume stream (p) │ Toggle Theme (t) │ Toggle Help (h/F1) │ Quit (q)";

    let paragraph = Paragraph::new(cheat_sheet)
        .style(Style::default().fg(colors.fg))
        .block(
            Block::default()
                .title(
                    " ⌨️  Racoon Keyboard Shortcuts Cheat Sheet (Press 'h', 'F1', or 'Esc' to close) ",
                )
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
