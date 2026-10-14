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
//! use gcp_racoon::ui::{App, run_app};
//! use gcp_racoon::gcp::LogEntry;
//! use tokio::sync::mpsc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let (tx, rx) = mpsc::channel::<LogEntry>(256);
//!     let app = App::new("my-project".to_string());
//!     run_app(app, rx, None, None).await?;
//!     Ok(())
//! }
//! ```
pub mod app;
pub mod input;
pub mod theme;
pub mod widgets;

pub use app::{App, InputMode};

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc;

use crate::ai::{GeminiClient, ModelsRegistry, UserIntent, select_with_terminal};
use crate::config::Config;
use crate::gcp::{LogEntry, LogFilter, LogsClient};
use crate::ui::theme::Theme;
use crate::utils::sanitize_query;
use widgets::{render_ai_pane, render_detail_pane, render_input_bar, render_log_list};

type Term = Terminal<CrosstermBackend<io::Stdout>>;

/// AI request types for the background worker.
pub enum AiRequest {
    /// Explain a single log entry.
    ExplainLog(Box<LogEntry>),
    /// Ask a question and get an analyzed answer (includes current buffered logs for offline fallback).
    AskQuestion {
        question: String,
        local_logs: Vec<LogEntry>,
    },
    /// Dynamically switch the active AI client after selecting a new provider/model in TUI.
    UpdateClient(Option<GeminiClient>),
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
    /// Filter set by AI.
    FilterSet { filter: String, message: String },
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
/// * `gemini` - Optional AI client
/// * `logs_client` - Optional GCP Logs client (present in live `tail` mode, `None` in offline `view` mode)
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
    // Create AI communication channels
    let (ai_tx, mut ai_rx) = mpsc::channel::<AiResponse>(16);
    let (ai_request_tx, mut ai_request_rx) = mpsc::channel::<AiRequest>(16);

    if let Some(ref c) = gemini {
        app.ai_available = true;
        app.ai_model_label = format!("{}/{}", c.provider().id(), c.model());
    }

    // Spawn AI worker task (supports hot-swapping client via AiRequest::UpdateClient)
    let ai_tx_clone = ai_tx.clone();
    let worker_logs_client = logs_client.clone();
    let mut current_client = gemini;
    tokio::spawn(async move {
        while let Some(request) = ai_request_rx.recv().await {
            match request {
                AiRequest::UpdateClient(new_client) => {
                    current_client = new_client;
                    continue;
                }
                AiRequest::ExplainLog(entry) => {
                    let result = if let Some(ref client) = current_client {
                        match client.explain_log(&entry).await {
                            Ok(text) => AiResponse::Explanation(text),
                            Err(e) => AiResponse::Error(e.to_string()),
                        }
                    } else {
                        AiResponse::Error(
                            "AI client not configured. Press 'm' to select a provider/model."
                                .to_string(),
                        )
                    };
                    if ai_tx_clone.send(result).await.is_err() {
                        break;
                    }
                }
                AiRequest::AskQuestion {
                    question: input,
                    local_logs,
                } => {
                    let result = if let Some(ref client) = current_client {
                        match client.detect_intent(&input).await {
                            Ok(UserIntent::SetFilter {
                                filter,
                                description,
                            }) => {
                                let clean_filter = sanitize_query(&filter);
                                AiResponse::FilterSet {
                                    filter: clean_filter.clone(),
                                    message: format!(
                                        "✅ 已设置过滤条件: {}\n\n{}",
                                        clean_filter, description
                                    ),
                                }
                            }
                            Ok(UserIntent::Question { question }) => {
                                if let Some(ref lc) = worker_logs_client {
                                    match client.ask_question(&question, lc).await {
                                        Ok(qa) => AiResponse::QuestionAnswer {
                                            query: qa.query,
                                            log_count: qa.log_count,
                                            answer: qa.answer,
                                        },
                                        Err(e) => AiResponse::Error(e.to_string()),
                                    }
                                } else {
                                    match client.analyze_logs(&local_logs).await {
                                        Ok(answer) => AiResponse::QuestionAnswer {
                                            query: "(offline buffer)".to_string(),
                                            log_count: local_logs.len(),
                                            answer,
                                        },
                                        Err(e) => AiResponse::Error(e.to_string()),
                                    }
                                }
                            }
                            Ok(UserIntent::GenerateQuery { query }) => {
                                let clean_query = sanitize_query(&query);
                                AiResponse::FilterSet {
                                    filter: clean_query.clone(),
                                    message: format!(
                                        "📋 Generated query: {}\n\n(applied as active filter)",
                                        clean_query
                                    ),
                                }
                            }
                            Err(e) => AiResponse::Error(e.to_string()),
                        }
                    } else {
                        AiResponse::Error(
                            "AI client not configured. Press 'm' to select a provider/model."
                                .to_string(),
                        )
                    };

                    if ai_tx_clone.send(result).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Initialize terminal
    let mut terminal = ratatui::init();
    terminal.clear()?;

    // Main event loop
    let result = run_event_loop(
        &mut terminal,
        &mut app,
        &mut log_rx,
        &mut ai_rx,
        Some(&ai_request_tx),
        logs_client.as_ref(),
    )
    .await;

    // Restore terminal (always, even on error)
    ratatui::restore();

    result
}

/// Restart the live GCP log stream when the active filter changes.
async fn restart_live_stream(
    app: &mut App,
    log_rx: &mut mpsc::Receiver<LogEntry>,
    logs_client: &LogsClient,
    filter_expr: Option<&str>,
) {
    let lookback = chrono::Duration::hours(1);
    let mut gcp_filter = LogFilter::new().since(lookback);
    if let Some(expr) = filter_expr
        && !expr.trim().is_empty()
    {
        gcp_filter = gcp_filter.raw(expr);
    }

    match logs_client.stream_entries(Some(gcp_filter), lookback).await {
        Ok((new_rx, _handle)) => {
            app.reset_for_new_stream();
            *log_rx = new_rx;
        }
        Err(e) => {
            app.set_error(format!("Failed to update stream filter: {}", e));
        }
    }
}

/// The main event loop.
async fn run_event_loop(
    terminal: &mut Term,
    app: &mut App,
    log_rx: &mut mpsc::Receiver<LogEntry>,
    ai_rx: &mut mpsc::Receiver<AiResponse>,
    ai_tx: Option<&mpsc::Sender<AiRequest>>,
    logs_client: Option<&LogsClient>,
) -> Result<()> {
    loop {
        // 1. Draw UI
        terminal.draw(|frame| {
            render_ui(frame, app);
        })?;

        // 2. Handle events (non-blocking with timeout)
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            // Handle Ctrl+C globally
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.should_quit = true;
            }

            // Delegate to event handler
            let action = input::handle_key_event(app, key);

            match action {
                input::Action::ApplyFilter(new_filter) => {
                    if let Some(lc) = logs_client {
                        restart_live_stream(app, log_rx, lc, new_filter.as_deref()).await;
                    }
                }
                input::Action::SwitchModel => {
                    let mut config = Config::load();
                    let registry = ModelsRegistry::load();
                    if let Ok(Some((provider, model))) =
                        select_with_terminal(terminal, &config, &registry)
                    {
                        config.set_active_model(provider.id(), &model.id);
                        let _ = config.save();
                        app.ai_model_label = format!("{}/{}", provider.id(), model.id);

                        match GeminiClient::from_config(&config) {
                            Ok(new_client) => {
                                app.ai_available = true;
                                if let Some(tx) = ai_tx {
                                    let _ =
                                        tx.send(AiRequest::UpdateClient(Some(new_client))).await;
                                }
                                app.set_ai_response(format!(
                                    "✅ 已切换 AI 模型: {} ({})\n\n{}",
                                    model.display_name,
                                    provider.display_name(),
                                    model.description
                                ));
                            }
                            Err(e) => {
                                app.ai_available = false;
                                if let Some(tx) = ai_tx {
                                    let _ = tx.send(AiRequest::UpdateClient(None)).await;
                                }
                                app.set_error(format!(
                                    "已选择 {}/{}，但缺少凭证: {}",
                                    provider.id(),
                                    model.id,
                                    e
                                ));
                            }
                        }
                    }
                }
                input::Action::ExplainLog => {
                    if let Some(tx) = ai_tx
                        && let Some(entry) = app.selected_log().cloned()
                    {
                        app.start_ai_loading();
                        let _ = tx.send(AiRequest::ExplainLog(Box::new(entry))).await;
                    }
                }
                input::Action::AskQuestion(question) => {
                    if let Some(tx) = ai_tx {
                        app.start_ai_loading();
                        app.ai_query = Some(question.clone());
                        let _ = tx
                            .send(AiRequest::AskQuestion {
                                question,
                                local_logs: app.logs.clone(),
                            })
                            .await;
                    }
                }
                input::Action::GenerateQuery(_) | input::Action::None => {}
            }
        }

        // 3. Check for new logs (only add if streaming is enabled)
        while let Ok(entry) = log_rx.try_recv() {
            if app.is_streaming {
                app.add_log(entry);
            }
            // When paused, we still drain the channel to prevent backpressure,
            // but we don't add the logs to the UI
        }

        // 4. Check for AI responses
        while let Ok(response) = ai_rx.try_recv() {
            match response {
                AiResponse::Explanation(text) => {
                    app.set_ai_response(text);
                }
                AiResponse::FilterSet { filter, message } => {
                    // Set the filter, update local/live stream, and show confirmation
                    app.set_filter(Some(filter.clone()));
                    if let Some(lc) = logs_client {
                        restart_live_stream(app, log_rx, lc, Some(&filter)).await;
                    }
                    app.set_ai_response(message);
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
            Constraint::Length(4),  // Footer (input bar)
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
            Constraint::Percentage(50), // AI response
            Constraint::Percentage(50), // Detail pane
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
    let colors = app.colors();
    let project = &app.project;
    let log_count = app.logs.len();
    let theme_indicator = match app.theme {
        Theme::Dark => "🌙",
        Theme::Light => "☀️",
    };
    let status = if app.offline_logs.is_some() {
        "📁 OFFLINE"
    } else if app.is_streaming {
        "● LIVE"
    } else {
        "○ PAUSED"
    };
    let status_style = if app.is_streaming {
        Style::default()
            .fg(colors.status_live)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.status_paused)
    };

    let header_text = format!(
        " 🦝 racoon │ {} │ {} logs │ 🤖 {} │ {} │ ",
        project, log_count, app.ai_model_label, theme_indicator,
    );

    let header = Paragraph::new(Line::from(vec![
        Span::styled(header_text, colors.header_style()),
        Span::styled(status, status_style),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(colors.border_focused_style()),
    );

    frame.render_widget(header, area);
}
