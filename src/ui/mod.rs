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

use crate::gcp::{LogEntry, LogsClient};
use crate::ai::GeminiClient;
use widgets::{render_detail_pane, render_input_bar, render_log_list, render_ai_pane};

type Term = Terminal<CrosstermBackend<io::Stdout>>;

/// AI request types for the background worker.
#[derive(Debug)]
pub enum AiRequest {
    /// Explain a single log entry.
    ExplainLog(LogEntry),
    /// Ask a question and get an analyzed answer.
    AskQuestion(String),
}

/// AI response from the background worker.
#[derive(Debug)]
pub enum AiResponse {
    /// Explanation of a log entry.
    Explanation(String),
    /// Answer to a question.
    QuestionAnswer {
        query: String,
        log_count: usize,
        answer: String,
    },
    /// Error occurred.
    Error(String),
}

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
    gemini: Option<GeminiClient>,
    logs_client: Option<LogsClient>,
) -> Result<()> {
    // Create AI communication channels if AI is available
    let (ai_tx, mut ai_rx) = mpsc::channel::<AiResponse>(16);
    let (ai_request_tx, mut ai_request_rx) = mpsc::channel::<AiRequest>(16);

    // Spawn AI worker task if Gemini is available
    if let Some(client) = gemini {
        let ai_tx_clone = ai_tx.clone();
        let logs_client = logs_client.clone();
        tokio::spawn(async move {
            while let Some(request) = ai_request_rx.recv().await {
                let result = match request {
                    AiRequest::ExplainLog(entry) => {
                        match client.explain_log(&entry).await {
                            Ok(text) => AiResponse::Explanation(text),
                            Err(e) => AiResponse::Error(e.to_string()),
                        }
                    }
                    AiRequest::AskQuestion(question) => {
                        if let Some(ref lc) = logs_client {
                            match client.ask_question(&question, lc).await {
                                Ok(qa) => AiResponse::QuestionAnswer {
                                    query: qa.query,
                                    log_count: qa.log_count,
                                    answer: qa.answer,
                                },
                                Err(e) => AiResponse::Error(e.to_string()),
                            }
                        } else {
                            AiResponse::Error("Logs client not available".to_string())
                        }
                    }
                };

                if ai_tx_clone.send(result).await.is_err() {
                    break;  // Main task dropped, exit
                }
            }
        });
        app.ai_available = true;
    }

    // Initialize terminal
    let mut terminal = ratatui::init();
    terminal.clear()?;

    // Store the request sender in a local variable for the event loop
    let ai_request_tx = if app.ai_available { Some(ai_request_tx) } else { None };

    // Main event loop
    let result = run_event_loop(
        &mut terminal,
        &mut app,
        &mut log_rx,
        &mut ai_rx,
        ai_request_tx.as_ref(),
    ).await;

    // Restore terminal (always, even on error)
    ratatui::restore();

    result
}

/// The main event loop.
async fn run_event_loop(
    terminal: &mut Term,
    app: &mut App,
    log_rx: &mut mpsc::Receiver<LogEntry>,
    ai_rx: &mut mpsc::Receiver<AiResponse>,
    ai_tx: Option<&mpsc::Sender<AiRequest>>,
) -> Result<()> {
    loop {
        // 1. Draw UI
        terminal.draw(|frame| {
            render_ui(frame, app);
        })?;

        // 2. Handle events (non-blocking with timeout)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Handle Ctrl+C globally
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                }

                // Delegate to event handler
                let action = input::handle_key_event(app, key);

                // Process any AI actions
                if let Some(tx) = ai_tx {
                    match action {
                        input::Action::ExplainLog => {
                            if let Some(entry) = app.selected_log().cloned() {
                                app.start_ai_loading();
                                let _ = tx.send(AiRequest::ExplainLog(entry)).await;
                            }
                        }
                        input::Action::AskQuestion(question) => {
                            app.start_ai_loading();
                            app.ai_query = Some(question.clone());  // Store the question
                            let _ = tx.send(AiRequest::AskQuestion(question)).await;
                        }
                        input::Action::GenerateQuery(_) => {
                            // Keep for backward compatibility if needed
                        }
                        input::Action::None => {}
                    }
                }
            }
        }

        // 3. Check for new logs
        while let Ok(entry) = log_rx.try_recv() {
            app.add_log(entry);
        }

        // 4. Check for AI responses
        while let Ok(response) = ai_rx.try_recv() {
            match response {
                AiResponse::Explanation(text) => {
                    app.set_ai_response(text);
                }
                AiResponse::QuestionAnswer {
                    query,
                    log_count,
                    answer,
                } => {
                    // Format the full response
                    let formatted = format!(
                        "📊 Query: {}\n📋 Found: {} logs\n\n{}",
                        query, log_count, answer
                    );
                    app.set_ai_response(formatted);
                }
                AiResponse::Error(e) => {
                    app.set_error(format!("AI Error: {}", e));
                    app.ai_loading = false;
                }
            }
        }

        // 5. Check if we should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Render the complete UI.
fn render_ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Main layout: header, logs, detail, footer
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(12), // Logs (10 rows)
            Constraint::Min(10),    // Body
            Constraint::Length(3),  // Footer (input bar)
        ])
        .split(area);

    // Render header
    render_header(frame, app, main_chunks[0]);

    // Render log list
    render_log_list(frame, app, main_chunks[1]);

    // Body layout: AI response (left) + detail pane (right)
    let detail_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),  // AI response
            Constraint::Percentage(50),  // Detail pane
        ])
        .split(main_chunks[2]);

    // Render AI pane
    render_ai_pane(frame, app, detail_chunks[0]);

    // Render detail pane
    render_detail_pane(frame, app, detail_chunks[1]);

    // Render input bar
    render_input_bar(frame, app, main_chunks[3]);
}

/// Render the header bar.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let project = &app.project;
    let log_count = app.logs.len();
    let (status_text, status_style) = if app.is_streaming {
        ("● LIVE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        ("○ PAUSED", Style::default().fg(Color::Yellow))
    };

    let ai_indicator = if app.ai_available {
        Span::styled(" AI ", Style::default().fg(Color::Cyan))
    } else {
        Span::styled(" (no AI) ", Style::default().fg(Color::DarkGray))
    };

    let header = Paragraph::new(Line::from(vec![
        Span::raw(format!(" 🦝 racoon │ {} │ {} logs │ ", project, log_count)),
        Span::styled(status_text, status_style),
        ai_indicator,
    ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
        );

    frame.render_widget(header, area);
}