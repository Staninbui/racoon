//! Interactive terminal selector for choosing AI provider and model.
//!
//! Uses `ratatui` and `crossterm` to provide a two-step keyboard-driven menu
//! (Step 1: Choose Provider -> Step 2: Choose Model) without requiring users
//! to memorize or type long model identifiers.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use super::models::{ModelInfo, ModelsRegistry, ProviderKind};
use crate::config::Config;

/// Current stage in the two-step model selection wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorStep {
    /// Choosing one of the 4 AI providers.
    ChooseProvider,
    /// Choosing a model belonging to the selected provider.
    ChooseModel,
}

/// Interactive selector state.
pub struct ModelSelectorState {
    step: SelectorStep,
    providers: Vec<ProviderKind>,
    provider_index: usize,
    models: Vec<ModelInfo>,
    model_index: usize,
    active_provider: ProviderKind,
    active_model: String,
}

impl ModelSelectorState {
    pub fn new(config: &Config, registry: &ModelsRegistry) -> Self {
        let providers = ProviderKind::all().to_vec();
        let active_provider = ProviderKind::parse(&config.ai.provider);
        let provider_index = providers
            .iter()
            .position(|&p| p == active_provider)
            .unwrap_or(0);
        let models = registry.models_for(providers[provider_index]);
        let model_index = models
            .iter()
            .position(|m| m.id == config.ai.model)
            .unwrap_or(0);

        Self {
            step: SelectorStep::ChooseProvider,
            providers,
            provider_index,
            models,
            model_index,
            active_provider,
            active_model: config.ai.model.clone(),
        }
    }

    fn selected_provider(&self) -> ProviderKind {
        self.providers[self.provider_index]
    }
}

/// Run the interactive terminal model selector (initializes and restores terminal).
///
/// Returns `Ok(Some((provider, model)))` if the user confirmed a choice,
/// or `Ok(None)` if they cancelled with `q` / `Esc`.
pub fn run_model_selector(
    config: &Config,
    registry: &ModelsRegistry,
) -> Result<Option<(ProviderKind, ModelInfo)>> {
    let mut terminal = ratatui::init();
    terminal.clear()?;

    let result = select_with_terminal(&mut terminal, config, registry);

    ratatui::restore();
    result
}

/// Run the interactive model selector on an existing `ratatui` terminal instance
/// (used when pressing `m` inside the main TUI).
pub fn select_with_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    config: &Config,
    registry: &ModelsRegistry,
) -> Result<Option<(ProviderKind, ModelInfo)>> {
    let mut state = ModelSelectorState::new(config, registry);
    selector_loop(terminal, &mut state, config, registry)
}

fn selector_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut ModelSelectorState,
    config: &Config,
    registry: &ModelsRegistry,
) -> Result<Option<(ProviderKind, ModelInfo)>> {
    loop {
        terminal.draw(|frame| {
            render_selector(frame, state, config, registry);
        })?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(None);
            }

            match state.step {
                SelectorStep::ChooseProvider => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    KeyCode::Down | KeyCode::Char('j') => {
                        state.provider_index = (state.provider_index + 1) % state.providers.len();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.provider_index == 0 {
                            state.provider_index = state.providers.len() - 1;
                        } else {
                            state.provider_index -= 1;
                        }
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        let chosen_provider = state.selected_provider();
                        state.models = registry.models_for(chosen_provider);
                        state.model_index = state
                            .models
                            .iter()
                            .position(|m| {
                                chosen_provider == state.active_provider
                                    && m.id == state.active_model
                            })
                            .unwrap_or(0);
                        state.step = SelectorStep::ChooseModel;
                    }
                    _ => {}
                },
                SelectorStep::ChooseModel => match key.code {
                    KeyCode::Char('q') => return Ok(None),
                    KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                        state.step = SelectorStep::ChooseProvider;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !state.models.is_empty() {
                            state.model_index = (state.model_index + 1) % state.models.len();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if !state.models.is_empty() {
                            if state.model_index == 0 {
                                state.model_index = state.models.len() - 1;
                            } else {
                                state.model_index -= 1;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(model) = state.models.get(state.model_index).cloned() {
                            return Ok(Some((state.selected_provider(), model)));
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}

fn render_selector(
    frame: &mut Frame,
    state: &ModelSelectorState,
    config: &Config,
    registry: &ModelsRegistry,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // List
            Constraint::Length(3), // Footer hints
        ])
        .split(area);

    let header_title = format!(
        " 🦝 racoon Model Selector │ Current: {} / {} ",
        state.active_provider.display_name(),
        state.active_model
    );
    let header = Paragraph::new(header_title)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    frame.render_widget(header, chunks[0]);

    match state.step {
        SelectorStep::ChooseProvider => {
            let items: Vec<ListItem> = state
                .providers
                .iter()
                .map(|&provider| {
                    let has_key = config.get_provider_api_key(provider.id()).is_some();
                    let key_badge = if has_key {
                        "🔑 API Key Ready".to_string()
                    } else {
                        format!("⚠️  Set {}", provider.env_var())
                    };
                    let active_badge = if provider == state.active_provider {
                        " [ACTIVE]"
                    } else {
                        ""
                    };
                    let count = registry.models_for(provider).len();
                    let line = format!(
                        " {:<22} │ {:<24} │ {} models{}",
                        provider.display_name(),
                        key_badge,
                        count,
                        active_badge
                    );
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" Step 1/2: Select AI Provider ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Blue)),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("❯ ");

            let mut list_state = ListState::default();
            list_state.select(Some(state.provider_index));
            frame.render_stateful_widget(list, chunks[1], &mut list_state);

            let footer =
                Paragraph::new(" j/k or ↑/↓: Navigate │ Enter/→: Choose Provider │ Esc/q: Cancel ")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(Block::default().borders(Borders::ALL));
            frame.render_widget(footer, chunks[2]);
        }
        SelectorStep::ChooseModel => {
            let provider = state.selected_provider();
            let items: Vec<ListItem> = state
                .models
                .iter()
                .map(|m| {
                    let is_current =
                        provider == state.active_provider && m.id == state.active_model;
                    let badge = if is_current { " [CURRENT]" } else { "" };
                    let line = format!(" {:<28} │ {}{}", m.id, m.description, badge);
                    ListItem::new(line)
                })
                .collect();

            let title = format!(
                " Step 2/2: Select Model for {} ({}) ",
                provider.display_name(),
                state.models.len()
            );

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Green)),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::Green)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("❯ ");

            let mut list_state = ListState::default();
            list_state.select(Some(state.model_index));
            frame.render_stateful_widget(list, chunks[1], &mut list_state);

            let footer = Paragraph::new(
                " j/k or ↑/↓: Navigate │ Enter: Confirm & Save │ Backspace/←/Esc: Back to Providers │ q: Cancel ",
            )
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
            frame.render_widget(footer, chunks[2]);
        }
    }
}
