//! Terminal User Interface module for racoon.
//!
//! This module provides a real-time TUI for viewing and analyzing GCP logs.
//! Built with ratatui and crossterm.

pub mod app;
pub mod input;
pub mod radar;
pub mod theme;
pub mod widgets;

pub use app::{App, InputMode};
pub use radar::{ErrorCluster, SpikeRadar, fingerprint_message};

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
    /// Perform Full-Trace / Multi-Log Root Cause Analysis (`E` key).
    AnalyzeTrace {
        filter_clause: Option<String>,
        local_logs: Vec<LogEntry>,
    },
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
    /// Explanation of a log entry or multi-log trace analysis.
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
pub async fn run_app(
    mut app: App,
    mut log_rx: mpsc::Receiver<LogEntry>,
    gemini: Option<GeminiClient>,
    logs_client: Option<LogsClient>,
) -> Result<()> {
    let (ai_tx, mut ai_rx) = mpsc::channel::<AiResponse>(16);
    let (ai_request_tx, mut ai_request_rx) = mpsc::channel::<AiRequest>(16);

    if let Some(ref c) = gemini {
        app.ai_available = true;
        app.ai_model_label = format!("{}/{}", c.provider().id(), c.model());
        app.ai_pii_guard = c.redact_pii();
    }

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
                AiRequest::AnalyzeTrace {
                    filter_clause,
                    local_logs,
                } => {
                    let result = if let Some(ref client) = current_client {
                        // Fetch full trace or surrounding context logs from GCP if live client is available
                        let mut trace_entries = if let (Some(lc), Some(expr)) =
                            (&worker_logs_client, filter_clause.as_deref())
                        {
                            let gcp_filter =
                                if expr.contains("timestamp>") || expr.contains("timestamp<") {
                                    LogFilter::new().raw(expr)
                                } else {
                                    LogFilter::new()
                                        .since(chrono::Duration::hours(24))
                                        .raw(expr)
                                };
                            match lc.list_entries(Some(&gcp_filter), 30).await {
                                Ok(mut fetched) if !fetched.is_empty() => {
                                    // `list_entries` returns newest first; reverse for chronological timeline
                                    fetched.reverse();
                                    fetched
                                }
                                _ => local_logs,
                            }
                        } else {
                            local_logs
                        };

                        if trace_entries.is_empty() {
                            AiResponse::Error("No logs found for trace analysis.".to_string())
                        } else {
                            // Keep up to 15 most relevant chronological entries
                            if trace_entries.len() > 15 {
                                trace_entries = trace_entries.into_iter().take(15).collect();
                            }
                            let count = trace_entries.len();
                            let header = match filter_clause {
                                Some(ref f) => {
                                    format!("🔗 Full-Trace RCA ({} logs │ {})\n\n", count, f)
                                }
                                None => {
                                    format!("🔗 Multi-Log Root Cause Analysis ({} logs)\n\n", count)
                                }
                            };
                            match client.analyze_logs(&trace_entries).await {
                                Ok(analysis) => {
                                    AiResponse::Explanation(format!("{}{}", header, analysis))
                                }
                                Err(e) => AiResponse::Error(e.to_string()),
                            }
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

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let result = run_event_loop(
        &mut terminal,
        &mut app,
        &mut log_rx,
        &mut ai_rx,
        Some(&ai_request_tx),
        logs_client.as_ref(),
    )
    .await;

    ratatui::restore();

    result
}

/// Restart the live GCP log stream (or fetch a bounded surrounding context snapshot) when the active filter changes.
async fn restart_live_stream(
    app: &mut App,
    log_rx: &mut mpsc::Receiver<LogEntry>,
    logs_client: &LogsClient,
    filter_expr: Option<&str>,
) {
    // When Surrounding Logs mode (`s` key) is active, fetch the bounded ±5s snapshot immediately
    // and select the anchor log entry so the user can inspect before/after logs right away.
    if app.active_surrounding.is_some()
        && let Some(expr) = filter_expr
    {
        let gcp_filter = LogFilter::new().raw(expr);
        match logs_client
            .list_entries(Some(&gcp_filter), app.max_logs)
            .await
        {
            Ok(mut entries) => {
                // `list_entries` returns newest first; reverse for chronological display (oldest at top)
                entries.reverse();
                app.reset_for_new_stream();
                for entry in entries {
                    app.add_log(entry);
                }
                app.select_anchor_log();
                let (_idle_tx, idle_rx) = mpsc::channel(1);
                *log_rx = idle_rx;
            }
            Err(e) => {
                app.set_error(format!("Failed to fetch surrounding logs: {}", e));
            }
        }
        return;
    }

    let lookback = if app.active_trace.is_some() {
        chrono::Duration::hours(24)
    } else {
        chrono::Duration::hours(1)
    };
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
        terminal.draw(|frame| {
            render_ui(frame, app);
        })?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.should_quit = true;
            }

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
                input::Action::AnalyzeTrace => {
                    if let Some(tx) = ai_tx {
                        let filter_clause =
                            if app.active_trace.is_some() || app.active_surrounding.is_some() {
                                app.filter.clone()
                            } else {
                                app.selected_log()
                                    .and_then(|e| {
                                        e.correlation_id().or_else(|| e.surrounding_context(5))
                                    })
                                    .map(|(clause, _)| clause)
                            };
                        app.start_ai_loading();
                        app.ai_query = Some(
                            filter_clause
                                .clone()
                                .unwrap_or_else(|| "Full-Trace Root Cause Analysis".to_string()),
                        );
                        let _ = tx
                            .send(AiRequest::AnalyzeTrace {
                                filter_clause,
                                local_logs: app.logs.iter().cloned().collect(),
                            })
                            .await;
                    }
                }
                input::Action::AskQuestion(question) => {
                    if let Some(tx) = ai_tx {
                        app.start_ai_loading();
                        app.ai_query = Some(question.clone());
                        let _ = tx
                            .send(AiRequest::AskQuestion {
                                question,
                                local_logs: app.logs.iter().cloned().collect(),
                            })
                            .await;
                    }
                }
                input::Action::GenerateQuery(_) | input::Action::None => {}
            }
        }

        while let Ok(entry) = log_rx.try_recv() {
            if app.is_streaming {
                app.add_log(entry);
            }
        }

        while let Ok(response) = ai_rx.try_recv() {
            match response {
                AiResponse::Explanation(text) => {
                    app.set_ai_response(text);
                }
                AiResponse::FilterSet { filter, message } => {
                    app.active_trace = None;
                    app.active_surrounding = None;
                    app.anchor_log_key = None;
                    app.previous_filter = None;
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

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Render the complete UI.
fn render_ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);

    render_header(frame, app, main_chunks[0]);
    render_log_list(frame, app, main_chunks[1]);

    let detail_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[2]);

    render_ai_pane(frame, app, detail_chunks[0]);
    render_detail_pane(frame, app, detail_chunks[1]);
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
