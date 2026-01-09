//! Terminal User Interface module for racoon.
//!
//! This module provides a real-time TUI for viewing and analyzing GCP logs.
//! Built with ratatui and crossterm.
//!
//! # Architecture
//!
//! - `App`: Application state (logs, selection, input mode)
//! - `input`: Keyboard input handling
//! - `widgets`: Reusable UI components (log list, detail pane, input bar)
//!
//! # Usage
//!
//! ```rust,ignore
//! use racoon::ui::{App, run_app};
//! use racoon::gcp::LogEntry;
//! use tokio::sync::mpsc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let (tx, rx) = mpsc::channel::<LogEntry>(256);
//!     let app = App::new("my-project".to_string());
//!     run_app(app, rx).await?;
//!     Ok(())
//! }
//! ```
pub mod app;
pub mod input;
pub mod widgets;

pub use app::{App, InputMode};

use std::io;
use std::time::Duration;

use anyhow::{Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc;

use crate::gcp::LogEntry;
use widgets::{render_detail_pane, render_input_bar, render_log_list};

type Term = Terminal<CrosstermBackend<io::Stdout>>;

/// Run the TUI application.
///
/// This is the main entry point for the TUI. It initializes the terminal,
/// runs the event loop, and restores the terminal on exit.
///
/// # Arguments
///
/// * `app` - The application state
/// * `log_rx` - Channel receiver for incoming log entries
///
/// # Returns
///
/// Returns `Ok(())` when the user quits, or an error if something goes wrong.
pub async fn run_app(
    mut app: App,
    mut log_rx: mpsc::Receiver<LogEntry>,
) -> Result<()> {
    // Initialize terminal
    let mut terminal = ratatui::init();
    terminal.clear()?;

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app, &mut log_rx).await;

    // Restore terminal (always, even on error)
    ratatui::restore();

    result
}

/// The main event loop.
async fn run_event_loop(
    terminal: &mut Term,
    app: &mut App,
    log_rx: &mut mpsc::Receiver<LogEntry>,
) -> Result<()> {
    loop {
        // 1. Draw UI
        terminal.draw(|frame| {
            render_ui(frame, app);
        })?;

        // 2. Handle events (non-blocking with timeout)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Handle Ctrl+C globally
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                }

                // Delegate to event handler
                input::handle_key_event(app, key);
            }
        }

        // 3. Check for new logs
        while let Ok(entry) = log_rx.try_recv() {
            app.add_log(entry);
        }

        // 4. Check if we should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Render the complete UI.
fn render_ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Main layout: header, body, footer
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Body
            Constraint::Length(3),  // Footer (input bar)
        ])
        .split(area);

    // Render header
    render_header(frame, app, main_chunks[0]);

    // Body layout: log list (left) + detail pane (right)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55),  // Log list
            Constraint::Percentage(45),  // Detail pane
        ])
        .split(main_chunks[1]);

    // Render log list
    render_log_list(frame, app, body_chunks[0]);

    // Render detail pane (or AI response pane)
    render_detail_pane(frame, app, body_chunks[1]);

    // Render input bar
    render_input_bar(frame, app, main_chunks[2]);
}

/// Render the header bar.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let project = &app.project;
    let log_count = app.logs.len();
    let status = if app.is_streaming { "● LIVE" } else { "○ PAUSED" };

    let status_style = if app.is_streaming {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };

    let header_text = format!(
        " 🦝 racoon │ {} │ {} logs │ ",
        project,
        log_count
    );

    let header = Paragraph::new(Line::from(vec![
        Span::raw(header_text),
        Span::styled(status, status_style),
    ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
        );

    frame.render_widget(header, area);
}