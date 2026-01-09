//! Input bar widget.
//!
//! Displays the command/filter input at the bottom of the screen,
//! along with keyboard hints.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ui::app::InputMode;
use crate::ui::App;

/// Render the input bar at the bottom of the screen.
pub fn render_input_bar(frame: &mut Frame, app: &App, area: Rect) {
    match app.input_mode {
        InputMode::Normal => render_hints(frame, app, area),
        _ => render_input_field(frame, app, area),
    }
}

/// Render keyboard hints in normal mode.
fn render_hints(frame: &mut Frame, app: &App, area: Rect) {
    let hints = if let Some(ref error) = app.error_message {
        // Show error in red
        Paragraph::new(format!("❌ {}", error))
            .style(Style::default().fg(Color::Red))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red))
            )
    } else if let Some(ref filter) = app.filter {
        // Show current filter
        Paragraph::new(format!("Filter: {} │ Press 'f' to change, Esc to clear", filter))
            .style(Style::default().fg(Color::Cyan))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
            )
    } else {
        // Show keyboard hints
        let hints_text = "j/k: Navigate │ e: AI Explain │ ?: AI Query │ f: Filter │ p: Pause │ q: Quit";
        Paragraph::new(hints_text)
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
            )
    };

    frame.render_widget(hints, area);
}

/// Render the input field when in input mode.
fn render_input_field(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = app.input_mode.prompt();
    let input = &app.input_buffer;

    // Show cursor with a block character
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
                .border_style(Style::default().fg(border_color))
        );

    frame.render_widget(paragraph, area);
}