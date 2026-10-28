//! Application state for the TUI.
//!
//! This module defines the core state that drives the UI, including
//! the log buffer, selection state, error spike radar, and input handling.

use std::collections::VecDeque;

use super::radar::{ErrorCluster, SpikeRadar};
use super::theme::{Theme, ThemeColors};
use crate::gcp::LogEntry;
pub use crate::gcp::logs::{matches_local_filter, severity_rank};

/// Default maximum number of logs to keep in memory.
const DEFAULT_MAX_LOGS: usize = 100;

/// Which TUI pane currently holds keyboard focus (`Tab` / `Shift+Tab` to cycle, `z` to maximize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePane {
    #[default]
    Logs,
    Detail,
    Ai,
}

/// The main application state.
#[derive(Debug)]
pub struct App {
    /// Buffered log entries in a ring-buffer queue (newest last).
    pub logs: VecDeque<LogEntry>,

    /// Full offline log dataset when viewing a local file (`racoon view`).
    pub offline_logs: Option<Vec<LogEntry>>,

    /// Maximum number of logs to retain in streaming buffer.
    pub max_logs: usize,

    /// Currently selected log index.
    pub selected_index: usize,

    /// Scroll offset for the log list.
    pub scroll_offset: usize,

    /// Real-time Error Fingerprinting & Spike Radar engine.
    pub radar: SpikeRadar,

    /// Whether the top pane is currently showing the Error Spike Radar (`c` key).
    pub show_radar: bool,

    /// Currently selected cluster index in Radar mode.
    pub radar_selected_index: usize,

    /// Current input mode.
    pub input_mode: InputMode,

    /// Text in the query/search input.
    pub input_buffer: String,

    /// Previously submitted non-empty filter/search/AI queries (oldest first).
    pub input_history: Vec<String>,

    /// Current cursor position within `input_history` when navigating with Up/Down.
    pub history_index: Option<usize>,

    /// Unsubmitted draft saved when starting Up/Down history navigation.
    pub history_draft: String,

    /// Current filter string (applied to logs).
    pub filter: Option<String>,

    /// Short display ID when Distributed Trace Context mode (`x` key) is active.
    pub active_trace: Option<String>,

    /// Scope badge when Surrounding Logs mode (`s` key, ±5s same-instance window) is active.
    pub active_surrounding: Option<String>,

    /// Deduplication key of the anchor log entry around which Surrounding Logs mode was opened.
    pub anchor_log_key: Option<String>,

    /// Anchor log entry around which Surrounding Logs mode (`s` key) was opened.
    pub anchor_entry: Option<LogEntry>,

    /// Half-window radius in seconds (`±N`s) for Surrounding Logs mode (`[` / `]` to resize).
    pub surrounding_window_secs: i64,

    /// Whether the view is currently drilled down into an Error Cluster from Spike Radar.
    pub active_cluster_drilldown: bool,

    /// Whether one or more noisy patterns have been muted via `-` key.
    pub active_pattern_mute: bool,

    /// Whether the view is pinned to a specific service/resource via `+` key.
    pub active_service_pin: bool,

    /// Whether the keyboard shortcut help modal (`h` / `F1` key) is open.
    pub show_help: bool,

    /// Latest unacknowledged spike/new-error alert banner from `SpikeRadar`.
    pub active_spike_alert: Option<String>,

    /// Whether a terminal bell (`\x07`) should be emitted on the next render tick.
    pub pending_bell: bool,

    /// Currently focused pane (`Logs`, `Detail`, or `Ai`).
    pub focused_pane: ActivePane,

    /// Pane currently maximized to full-screen (`z` key), if any.
    pub maximized_pane: Option<ActivePane>,

    /// Saved filter bookmarks (`(name, gcp_filter)`) cycled via `b` key.
    pub bookmarks: Vec<(String, String)>,

    /// Active index in `bookmarks` when cycling with `b` key.
    pub bookmark_index: Option<usize>,

    /// Saved filter prior to entering Trace, Surrounding, or Radar cluster drill-down mode, restored on exit.
    pub previous_filter: Option<String>,

    /// Handle to the active background log streaming task (dropped when restarting or exiting).
    pub stream_handle: Option<crate::gcp::StreamHandle>,

    /// Whether AI is available.
    pub ai_available: bool,

    /// AI analysis result for the selected log.
    pub ai_response: Option<String>,

    /// Whether AI analysis is in progress.
    pub ai_loading: bool,

    /// AI query that was sent to the LLM.
    pub ai_query: Option<String>,

    /// Currently configured AI provider and model label (e.g., "google/gemini-3.8-flash").
    pub ai_model_label: String,

    /// Whether Pre-AI PII & Secret Guard is enabled.
    pub ai_pii_guard: bool,

    /// GCP project ID or offline file label.
    pub project: String,

    /// Whether the session is monitoring or viewing logs across multiple GCP projects/views.
    pub is_multi_project: bool,

    /// Whether we're actively streaming logs.
    pub is_streaming: bool,

    /// Flag to signal the app should exit.
    pub should_quit: bool,

    /// Error message to display (if any).
    pub error_message: Option<String>,

    /// Transient confirmation/status message to display (e.g., clipboard yank or browser launch).
    pub status_message: Option<String>,

    /// Scroll offset for the detail pane.
    pub detail_scroll: u16,

    /// Scroll offset for the AI pane.
    pub ai_scroll: u16,

    /// Current search query.
    pub search_query: Option<String>,

    /// Indices of logs matching the search.
    pub search_matches: Vec<usize>,

    /// Current match index (for next/prev navigation).
    pub search_match_index: usize,

    /// Current theme.
    pub theme: Theme,
}

impl App {
    /// Create a new App instance.
    pub fn new(project: String) -> Self {
        let is_multi_project = crate::gcp::parse_resource_names(&project).len() > 1;
        Self {
            logs: VecDeque::with_capacity(DEFAULT_MAX_LOGS),
            offline_logs: None,
            max_logs: DEFAULT_MAX_LOGS,
            selected_index: 0,
            scroll_offset: 0,
            radar: SpikeRadar::new(),
            show_radar: false,
            radar_selected_index: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_history: Vec::new(),
            history_index: None,
            history_draft: String::new(),
            filter: None,
            active_trace: None,
            active_surrounding: None,
            anchor_log_key: None,
            anchor_entry: None,
            surrounding_window_secs: 5,
            active_cluster_drilldown: false,
            active_pattern_mute: false,
            active_service_pin: false,
            show_help: false,
            active_spike_alert: None,
            pending_bell: false,
            focused_pane: ActivePane::Logs,
            maximized_pane: None,
            bookmarks: crate::config::default_sre_bookmarks(),
            bookmark_index: None,
            previous_filter: None,
            stream_handle: None,
            ai_available: false,
            ai_response: None,
            ai_loading: false,
            ai_query: None,
            ai_model_label: "google/gemini-3.8-flash".to_string(),
            ai_pii_guard: true,
            project,
            is_multi_project,
            is_streaming: true,
            should_quit: false,
            error_message: None,
            status_message: None,
            detail_scroll: 0,
            ai_scroll: 0,
            search_query: None,
            search_matches: Vec::new(),
            search_match_index: 0,
            theme: Theme::Dark,
        }
    }

    /// Set the active AI provider/model label displayed in the header.
    pub fn with_ai_model_label(mut self, label: String) -> Self {
        self.ai_model_label = label;
        self
    }

    /// Set the maximum number of logs to keep in memory.
    pub fn with_max_logs(mut self, max_logs: usize) -> Self {
        let cap = max_logs.max(1);
        self.max_logs = cap;
        self
    }

    /// Set an initial filter string to display in the UI.
    pub fn with_filter(mut self, filter: Option<String>) -> Self {
        self.filter = filter.filter(|f| !f.trim().is_empty());
        self
    }

    /// Configure filter bookmarks (`(name, filter)`) available via the `b` key.
    pub fn with_bookmarks(mut self, bookmarks: Vec<(String, String)>) -> Self {
        if !bookmarks.is_empty() {
            self.bookmarks = bookmarks;
        }
        self
    }

    /// Cycle to the next saved filter bookmark (`b` key), applying its filter immediately.
    pub fn cycle_next_bookmark(&mut self) -> Option<(String, String)> {
        if self.bookmarks.is_empty() {
            return None;
        }
        let next_idx = match self.bookmark_index {
            Some(idx) => (idx + 1) % self.bookmarks.len(),
            None => 0,
        };
        self.bookmark_index = Some(next_idx);
        let (name, filter) = self.bookmarks[next_idx].clone();
        self.clear_context_overlay();
        self.set_filter(Some(filter.clone()));
        self.set_status(format!(
            "🔖 Bookmark [{}/{}] {}: {} (Esc to clear)",
            next_idx + 1,
            self.bookmarks.len(),
            name,
            filter
        ));
        Some((name, filter))
    }

    /// Attach a live stream handle so its lifecycle is tied to the TUI session.
    pub fn with_stream_handle(mut self, handle: crate::gcp::StreamHandle) -> Self {
        self.stream_handle = Some(handle);
        self
    }

    /// Initialize with a static set of offline logs (disables live streaming).
    pub fn with_offline_logs(mut self, entries: Vec<LogEntry>) -> Self {
        let distinct_projects: std::collections::HashSet<String> = entries
            .iter()
            .filter_map(|e| e.resolve_project_id(""))
            .collect();
        if distinct_projects.len() > 1 {
            self.is_multi_project = true;
        }
        self.max_logs = entries.len().max(self.max_logs);
        self.radar.rebuild_from_logs(&entries);
        self.logs = VecDeque::from(entries.clone());
        self.offline_logs = Some(entries);
        self.is_streaming = false;
        self.selected_index = 0;
        self
    }

    /// Toggle Error Fingerprint & Spike Radar view (`c` key).
    pub fn toggle_radar(&mut self) {
        self.show_radar = !self.show_radar;
        self.active_spike_alert = None;
        if self.show_radar {
            self.radar_selected_index = 0;
            self.detail_scroll = 0;
        }
    }

    /// Toggle the keyboard shortcut Help Modal (`h` / `F1` key).
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Consume and return whether a terminal bell (`\x07`) should be emitted.
    pub fn take_pending_bell(&mut self) -> bool {
        std::mem::take(&mut self.pending_bell)
    }

    /// Dismiss the active spike alert banner in the header.
    pub fn dismiss_spike_alert(&mut self) {
        self.active_spike_alert = None;
    }

    /// Return the currently selected `ErrorCluster` when in Radar view.
    pub fn selected_cluster(&self) -> Option<&ErrorCluster> {
        if self.show_radar {
            self.radar.clusters.get(self.radar_selected_index)
        } else {
            None
        }
    }

    fn has_active_overlay(&self) -> bool {
        self.active_surrounding.is_some()
            || self.active_trace.is_some()
            || self.active_cluster_drilldown
            || self.active_pattern_mute
            || self.active_service_pin
    }

    /// Cycle keyboard focus forward across panes (`Logs -> Detail -> Ai -> Logs`).
    pub fn focus_next_pane(&mut self) {
        self.focused_pane = match self.focused_pane {
            ActivePane::Logs => ActivePane::Detail,
            ActivePane::Detail => ActivePane::Ai,
            ActivePane::Ai => ActivePane::Logs,
        };
        if self.maximized_pane.is_some() {
            self.maximized_pane = Some(self.focused_pane);
        }
    }

    /// Cycle keyboard focus backward across panes (`Logs -> Ai -> Detail -> Logs`).
    pub fn focus_prev_pane(&mut self) {
        self.focused_pane = match self.focused_pane {
            ActivePane::Logs => ActivePane::Ai,
            ActivePane::Detail => ActivePane::Logs,
            ActivePane::Ai => ActivePane::Detail,
        };
        if self.maximized_pane.is_some() {
            self.maximized_pane = Some(self.focused_pane);
        }
    }

    /// Toggle full-screen maximize for the currently focused pane (`z` key).
    pub fn toggle_maximize_pane(&mut self) {
        if self.maximized_pane.is_some() {
            self.maximized_pane = None;
        } else {
            self.maximized_pane = Some(self.focused_pane);
        }
    }

    /// Drill down into the currently selected `ErrorCluster` in Radar view (`Enter` key):
    /// switches back to the Log List filtered to entries matching that cluster's fingerprint.
    pub fn drill_into_selected_cluster(&mut self) -> Option<String> {
        let cluster = self.selected_cluster()?.clone();
        self.show_radar = false;
        if !self.has_active_overlay() {
            self.previous_filter = self.filter.clone();
        }
        self.active_cluster_drilldown = true;
        let filter_expr = format!("fingerprint:{}", cluster.fingerprint);
        self.set_filter(Some(filter_expr));
        // Build a GCP-compatible severity filter for live stream if needed
        Some(format!("severity>={}", cluster.severity))
    }

    /// Exit Radar cluster drill-down mode and restore the previous filter.
    pub fn exit_cluster_drilldown(&mut self) -> Option<String> {
        self.active_cluster_drilldown = false;
        let restored = self.previous_filter.take();
        self.set_filter(restored.clone());
        restored
    }

    /// Mute/exclude the pattern (normalized fingerprint) of the currently selected log entry
    /// or Error Spike Radar cluster (`-` key). Restored on `Esc`.
    pub fn mute_selected_pattern(&mut self) -> Result<String, String> {
        let fp = if let Some(cluster) = self.selected_cluster() {
            cluster.fingerprint.clone()
        } else if let Some(entry) = self.selected_log() {
            crate::utils::fingerprint_message(&entry.message_preview())
        } else {
            return Err("No log entry or error cluster selected to mute.".to_string());
        };

        if !self.has_active_overlay() {
            self.previous_filter = self.filter.clone();
        }
        self.active_pattern_mute = true;
        let escaped_fp = fp.replace('"', "'");
        let mute_clause = format!("-fingerprint:\"{}\"", escaped_fp);
        let new_filter = match self
            .filter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(existing) => format!("{} {}", existing, mute_clause),
            None => mute_clause,
        };
        self.set_filter(Some(new_filter));
        if self.radar_selected_index >= self.radar.clusters.len() && !self.radar.clusters.is_empty()
        {
            self.radar_selected_index = self.radar.clusters.len() - 1;
        }
        self.set_status(format!("🔇 Muted pattern: {} (Esc to restore)", fp));
        Ok(fp)
    }

    /// Pin/isolate the service or resource of the currently selected log entry (`+` / `=` key).
    /// Restored on `Esc`.
    pub fn pin_selected_service(&mut self) -> Result<String, String> {
        let entry = if let Some(cluster) = self.selected_cluster() {
            cluster.sample_entry.clone()
        } else if let Some(entry) = self.selected_log() {
            entry.clone()
        } else {
            return Err("No log entry selected to pin service.".to_string());
        };

        let (clause, badge) = entry.service_filter_clause().ok_or_else(|| {
            "Selected log has no service_name, container_name, or resource.type to pin.".to_string()
        })?;

        if !self.has_active_overlay() {
            self.previous_filter = self.filter.clone();
        }
        self.active_service_pin = true;
        let new_filter = match self
            .filter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(existing) if existing.contains(&clause) => existing.to_string(),
            Some(existing) => format!("{} {}", existing, clause),
            None => clause,
        };
        self.set_filter(Some(new_filter.clone()));
        self.set_status(format!(
            "📌 Pinned service/resource: {} (Esc to restore)",
            badge
        ));
        Ok(new_filter)
    }

    /// Toggle Distributed Trace Context mode (`x` key).
    pub fn toggle_trace_context(&mut self) -> Result<Option<String>, String> {
        if self.active_trace.is_some() {
            let restored = self.exit_trace_context();
            return Ok(restored);
        }

        let entry = self
            .selected_log()
            .ok_or_else(|| "No log entry selected.".to_string())?;

        let (filter_clause, short_id) = entry.correlation_id().ok_or_else(|| {
            "Selected log has no trace, spanId, or request_id to follow.".to_string()
        })?;

        self.show_radar = false;
        if !self.has_active_overlay() {
            self.previous_filter = self.filter.clone();
        }
        self.active_surrounding = None;
        self.anchor_log_key = None;
        self.anchor_entry = None;
        self.active_cluster_drilldown = false;
        self.active_pattern_mute = false;
        self.active_trace = Some(short_id);
        self.set_filter(Some(filter_clause.clone()));
        Ok(Some(filter_clause))
    }

    /// Exit Trace Context mode and restore the previous filter.
    pub fn exit_trace_context(&mut self) -> Option<String> {
        self.active_trace = None;
        let restored = self.previous_filter.take();
        self.set_filter(restored.clone());
        restored
    }

    /// Toggle Surrounding Logs Context mode (`s` key — ±5s around selected log on same resource instance).
    pub fn toggle_surrounding_context(&mut self) -> Result<Option<String>, String> {
        if self.active_surrounding.is_some() {
            let restored = self.exit_surrounding_context();
            return Ok(restored);
        }

        let entry = self
            .selected_log()
            .cloned()
            .ok_or_else(|| "No log entry selected.".to_string())?;

        self.surrounding_window_secs = 5;
        let (filter_clause, badge) = entry
            .surrounding_context(self.surrounding_window_secs)
            .ok_or_else(|| {
                "Selected log has no timestamp for ±5s surrounding context.".to_string()
            })?;

        let anchor_key = entry.dedup_key();

        self.show_radar = false;
        if !self.has_active_overlay() {
            self.previous_filter = self.filter.clone();
        }
        self.active_trace = None;
        self.active_cluster_drilldown = false;
        self.active_pattern_mute = false;
        self.active_surrounding = Some(badge);
        self.anchor_log_key = Some(anchor_key);
        self.anchor_entry = Some(entry);
        self.set_filter(Some(filter_clause.clone()));
        self.select_anchor_log();
        Ok(Some(filter_clause))
    }

    /// Expand (`expand = true`, `]` key) or shrink (`expand = false`, `[` key) the Surrounding Context
    /// time window across `[5s, 15s, 30s, 60s, 300s]`.
    pub fn resize_surrounding_window(&mut self, expand: bool) -> Result<String, String> {
        const STEPS: &[i64] = &[5, 15, 30, 60, 300];
        if self.active_surrounding.is_none() {
            return Err("Surrounding context mode is not active (press 's' first).".to_string());
        }
        let anchor = self
            .anchor_entry
            .clone()
            .ok_or_else(|| "No anchor log entry available for surrounding context.".to_string())?;

        let current_idx = STEPS
            .iter()
            .position(|&s| s >= self.surrounding_window_secs)
            .unwrap_or(0);
        let new_idx = if expand {
            (current_idx + 1).min(STEPS.len() - 1)
        } else {
            current_idx.saturating_sub(1)
        };
        self.surrounding_window_secs = STEPS[new_idx];

        let (filter_clause, badge) = anchor
            .surrounding_context(self.surrounding_window_secs)
            .ok_or_else(|| "Anchor log has no timestamp.".to_string())?;
        self.active_surrounding = Some(badge.clone());
        self.set_filter(Some(filter_clause.clone()));
        self.select_anchor_log();
        self.set_status(format!(
            "🕵️ Surrounding window updated: {} ([ / ] to resize)",
            badge
        ));
        Ok(filter_clause)
    }

    /// Exit Surrounding Logs Context mode and restore the previous filter.
    pub fn exit_surrounding_context(&mut self) -> Option<String> {
        self.active_surrounding = None;
        self.anchor_log_key = None;
        self.anchor_entry = None;
        self.surrounding_window_secs = 5;
        let restored = self.previous_filter.take();
        self.set_filter(restored.clone());
        restored
    }

    /// Clear all temporary context overlay states (Trace, Surrounding, Radar cluster drill-down, Mute, Service Pin)
    /// when a brand-new user or AI filter is explicitly applied.
    pub fn clear_context_overlay(&mut self) {
        self.active_trace = None;
        self.active_surrounding = None;
        self.anchor_log_key = None;
        self.anchor_entry = None;
        self.surrounding_window_secs = 5;
        self.active_cluster_drilldown = false;
        self.active_pattern_mute = false;
        self.active_service_pin = false;
        self.previous_filter = None;
    }

    /// If any temporary context overlay (Surrounding, Trace, Radar drill-down, Pattern Mute, or Service Pin) is active,
    /// exit it and restore `previous_filter`, returning `Some(restored_filter)`.
    pub fn exit_active_overlay(&mut self) -> Option<Option<String>> {
        if self.active_surrounding.is_some() {
            Some(self.exit_surrounding_context())
        } else if self.active_trace.is_some() {
            Some(self.exit_trace_context())
        } else if self.active_cluster_drilldown {
            Some(self.exit_cluster_drilldown())
        } else if self.active_pattern_mute || self.active_service_pin {
            self.active_pattern_mute = false;
            self.active_service_pin = false;
            let restored = self.previous_filter.take();
            self.set_filter(restored.clone());
            Some(restored)
        } else {
            None
        }
    }

    /// Move `selected_index` to the anchor log entry if present in `self.logs`.
    pub fn select_anchor_log(&mut self) {
        if let Some(ref key) = self.anchor_log_key
            && let Some(idx) = self.logs.iter().position(|e| e.dedup_key() == *key)
        {
            self.selected_index = idx;
        }
    }

    /// Check whether `entry` is the anchor log around which Surrounding Logs mode was opened.
    pub fn is_anchor_log(&self, entry: &LogEntry) -> bool {
        self.anchor_log_key
            .as_ref()
            .is_some_and(|k| *k == entry.dedup_key())
    }

    /// Add a new log entry to the buffer.
    pub fn add_log(&mut self, entry: LogEntry) {
        if let Some(ref filter_expr) = self.filter
            && !matches_local_filter(&entry, filter_expr)
        {
            return;
        }

        if let Some(alert) = self.radar.record_entry(&entry) {
            self.active_spike_alert = Some(alert.summary());
            if self.is_streaming {
                self.pending_bell = true;
            }
        }

        let was_at_bottom = self.is_at_bottom();
        let matches_active_search = self
            .search_query
            .as_ref()
            .map(|q| entry.matches_query_lower(&q.to_lowercase()))
            .unwrap_or(false);

        self.logs.push_back(entry);
        if matches_active_search {
            self.search_matches.push(self.logs.len() - 1);
        }

        while self.logs.len() > self.max_logs {
            self.logs.pop_front();
            if self.selected_index > 0 {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            if !self.search_matches.is_empty() {
                self.search_matches.retain_mut(|idx| {
                    if *idx == 0 {
                        false
                    } else {
                        *idx -= 1;
                        true
                    }
                });
                if self.search_match_index >= self.search_matches.len() {
                    self.search_match_index = 0;
                }
            }
        }

        if was_at_bottom && self.is_streaming {
            self.selected_index = self.logs.len().saturating_sub(1);
        }
    }

    /// Update the active filter and apply it locally (for offline mode or in-memory logs).
    pub fn set_filter(&mut self, filter: Option<String>) {
        let normalized = filter
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty());
        self.filter = normalized.clone();

        if let Some(ref all_logs) = self.offline_logs {
            if let Some(ref expr) = normalized {
                self.logs = all_logs
                    .iter()
                    .filter(|e| matches_local_filter(e, expr))
                    .cloned()
                    .collect();
            } else {
                self.logs = VecDeque::from(all_logs.clone());
            }
            self.radar.rebuild_from_logs(&self.logs);
        } else if let Some(ref expr) = normalized {
            self.logs.retain(|e| matches_local_filter(e, expr));
            self.radar.rebuild_from_logs(&self.logs);
        }

        self.selected_index = 0;
        self.scroll_offset = 0;
        self.detail_scroll = 0;
        if let Some(q) = self.search_query.clone() {
            self.search(&q);
        }
    }

    /// Clear buffered logs when restarting a live GCP log stream with a new filter.
    pub fn reset_for_new_stream(&mut self) {
        self.logs.clear();
        self.radar.clear();
        self.selected_index = 0;
        self.radar_selected_index = 0;
        self.scroll_offset = 0;
        self.detail_scroll = 0;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    /// Check if the selected index is at or near the bottom.
    pub fn is_at_bottom(&self) -> bool {
        if self.logs.is_empty() {
            return true;
        }
        self.selected_index >= self.logs.len().saturating_sub(2)
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_ai_down(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_add(1);
    }

    pub fn scroll_ai_up(&mut self) {
        self.ai_scroll = self.ai_scroll.saturating_sub(1);
    }

    pub fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
    }

    pub fn reset_ai_scroll(&mut self) {
        self.ai_scroll = 0;
    }

    pub fn select_next(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index =
                    (self.radar_selected_index + 1).min(self.radar.clusters.len() - 1);
            }
        } else if !self.logs.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.logs.len() - 1);
        }
    }

    pub fn select_previous(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index = self.radar_selected_index.saturating_sub(1);
            }
        } else if !self.logs.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    pub fn select_first(&mut self) {
        if self.show_radar {
            self.radar_selected_index = 0;
        } else {
            self.selected_index = 0;
        }
    }

    pub fn select_last(&mut self) {
        if self.show_radar {
            if !self.radar.clusters.is_empty() {
                self.radar_selected_index = self.radar.clusters.len() - 1;
            }
        } else if !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    /// Return the currently selected `LogEntry` (or the selected cluster's sample entry in Radar view).
    pub fn selected_log(&self) -> Option<&LogEntry> {
        if self.show_radar {
            self.radar
                .clusters
                .get(self.radar_selected_index)
                .map(|c| &c.sample_entry)
        } else {
            self.logs.get(self.selected_index)
        }
    }

    pub fn clear_ai_response(&mut self) {
        self.ai_response = None;
        self.ai_loading = false;
        self.ai_query = None;
        self.ai_scroll = 0;
    }

    pub fn set_ai_response(&mut self, response: String) {
        self.ai_response = Some(response);
        self.ai_loading = false;
        self.ai_scroll = 0;
    }

    pub fn start_ai_loading(&mut self) {
        self.ai_loading = true;
        self.ai_response = None;
    }

    pub fn toggle_streaming(&mut self) {
        if self.offline_logs.is_none() {
            self.is_streaming = !self.is_streaming;
        }
    }

    pub fn enter_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
        self.input_buffer.clear();
        self.history_index = None;
        self.history_draft.clear();
    }

    pub fn exit_input_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.history_index = None;
        self.history_draft.clear();
    }

    pub fn input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// Navigate backward (`Up` arrow) through previously submitted inputs.
    pub fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.history_draft = self.input_buffer.clone();
                let idx = self.input_history.len() - 1;
                self.history_index = Some(idx);
                self.input_buffer = self.input_history[idx].clone();
            }
            Some(idx) => {
                let new_idx = idx.saturating_sub(1);
                self.history_index = Some(new_idx);
                self.input_buffer = self.input_history[new_idx].clone();
            }
        }
    }

    /// Navigate forward (`Down` arrow) through previously submitted inputs, restoring draft at bottom.
    pub fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.input_history.len() {
                let new_idx = idx + 1;
                self.history_index = Some(new_idx);
                self.input_buffer = self.input_history[new_idx].clone();
            } else {
                self.history_index = None;
                self.input_buffer = self.history_draft.clone();
            }
        }
    }

    pub fn submit_input(&mut self) -> (String, InputMode) {
        let text = self.input_buffer.clone();
        let mode = self.input_mode;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            if self.input_history.last().map(|s| s.as_str()) != Some(text.as_str()) {
                self.input_history.push(text.clone());
            }
            if self.input_history.len() > 100 {
                self.input_history.remove(0);
            }
        }
        self.exit_input_mode();
        (text, mode)
    }

    pub fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
        self.status_message = None;
    }

    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
        self.error_message = None;
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Format the currently selected log entry (or Error Cluster in Radar view) as pretty JSON for clipboard yank (`y` key).
    pub fn yank_selected_log_text(&self) -> Result<String, String> {
        if let Some(cluster) = self.selected_cluster() {
            let sample_json = serde_json::to_string_pretty(&cluster.sample_entry)
                .map_err(|e| format!("Failed to serialize cluster sample log: {}", e))?;
            Ok(format!(
                "// Error Cluster [x{} occurrences | {} | first: {} | last: {}]\n// Fingerprint: {}\n{}",
                cluster.count,
                cluster.severity,
                cluster.first_seen,
                cluster.last_seen,
                cluster.fingerprint,
                sample_json
            ))
        } else if let Some(entry) = self.selected_log() {
            serde_json::to_string_pretty(entry)
                .map_err(|e| format!("Failed to serialize log entry: {}", e))
        } else {
            Err("No log entry selected to copy.".to_string())
        }
    }

    /// Return the current AI analysis Markdown for clipboard yank (`Y` / `Shift+y` key).
    pub fn yank_ai_response_text(&self) -> Result<String, String> {
        self.ai_response.clone().ok_or_else(|| {
            "No AI analysis available to copy yet (press 'e' or 'E' first).".to_string()
        })
    }

    /// Build the Google Cloud Logs Explorer deep link URL for the currently selected log entry (`o` key).
    pub fn selected_log_console_url(&self) -> Result<String, String> {
        let entry = self
            .selected_log()
            .ok_or_else(|| "No log entry selected.".to_string())?;
        entry.gcp_console_url(&self.project).ok_or_else(|| {
            "Cannot determine GCP project ID for Console URL (offline log has no project metadata)."
                .to_string()
        })
    }

    /// Load `input_history` from a JSON file on disk (ignoring missing or malformed files).
    pub fn load_history_from_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path)?;
        if let Ok(entries) = serde_json::from_str::<Vec<String>>(&content) {
            self.input_history = entries
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .rev()
                .take(100)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }
        Ok(())
    }

    /// Save `input_history` to a JSON file on disk (creating parent directories if needed).
    pub fn save_history_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let json =
            serde_json::to_string_pretty(&self.input_history).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Format a self-contained Markdown incident report capturing the current TUI context,
    /// Error Spike Radar clusters, AI Root Cause Analysis, and log timeline (`w` key).
    pub fn format_incident_report(&self) -> String {
        let now_str = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        let filter_str = self.filter.as_deref().unwrap_or("(none)");
        let mode_str = if let Some(ref scope) = self.active_surrounding {
            format!("Surrounding Context ({})", scope)
        } else if let Some(ref trace) = self.active_trace {
            format!("Distributed Trace ({})", trace)
        } else if self.offline_logs.is_some() {
            "Offline File".to_string()
        } else if self.is_streaming {
            "Live Stream".to_string()
        } else {
            "Paused Stream".to_string()
        };

        let mut out = String::with_capacity(4096);
        out.push_str("# 🦝 Racoon Incident Snapshot\n\n");
        out.push_str(&format!("- **Generated At**: `{}`\n", now_str));
        out.push_str(&format!("- **Project / Source**: `{}`\n", self.project));
        out.push_str(&format!("- **Active Mode**: `{}`\n", mode_str));
        out.push_str(&format!("- **Active Filter**: `{}`\n", filter_str));
        out.push_str(&format!("- **Buffered Logs**: `{}`\n", self.logs.len()));
        out.push_str(&format!("- **AI Model**: `{}`\n\n", self.ai_model_label));

        out.push_str("## 🚨 Error Fingerprint & Spike Radar\n\n");
        if self.radar.clusters.is_empty() {
            out.push_str("_No WARNING/ERROR/CRITICAL patterns detected in current buffer._\n\n");
        } else {
            out.push_str(&format!(
                "- **Total Error/Warning Logs**: `{}` │ **Velocity Sparkline**: `{}`\n\n",
                self.radar.total_error_logs,
                self.radar.sparkline()
            ));
            for cluster in self.radar.clusters.iter().take(10) {
                let badge = if cluster.is_spike { "🚨" } else { "🔥" };
                out.push_str(&format!(
                    "- {} **[x{}]** `[{}]` `{}` _(first: {}, last: {})_\n",
                    badge,
                    cluster.count,
                    cluster.severity,
                    cluster.fingerprint,
                    cluster.first_seen,
                    cluster.last_seen
                ));
            }
            out.push('\n');
        }

        if let Some(ref ai_text) = self.ai_response {
            out.push_str("## 🤖 AI Root Cause Analysis\n\n");
            out.push_str(ai_text.trim());
            out.push_str("\n\n");
        }

        if let Some(selected) = self.selected_log() {
            out.push_str("## 🎯 Selected Log Entry\n\n```json\n");
            if let Ok(pretty) = serde_json::to_string_pretty(selected) {
                out.push_str(&pretty);
            }
            out.push_str("\n```\n\n");
        }

        out.push_str("## 📋 Log Timeline (Recent Entries)\n\n```text\n");
        for entry in self.logs.iter().take(30) {
            let mark = if self.active_surrounding.is_some() && self.is_anchor_log(entry) {
                "🎯 "
            } else {
                "   "
            };
            out.push_str(&format!("{}{}\n", mark, entry.to_line()));
        }
        out.push_str("```\n");

        out
    }

    /// Write the formatted Markdown incident report to `path`, returning the number of bytes written.
    pub fn save_incident_report_to_path(&self, path: &std::path::Path) -> std::io::Result<usize> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let report = self.format_incident_report();
        let bytes = report.len();
        std::fs::write(path, report)?;
        Ok(bytes)
    }

    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }

        let query_lower = query.to_lowercase();

        self.search_matches = self
            .logs
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches_query_lower(&query_lower))
            .map(|(i, _)| i)
            .collect();

        self.search_query = Some(query.to_string());
        self.search_match_index = 0;

        if let Some(&first_match) = self.search_matches.first() {
            self.selected_index = first_match;
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
        self.selected_index = self.search_matches[self.search_match_index];
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        if self.search_match_index == 0 {
            self.search_match_index = self.search_matches.len() - 1;
        } else {
            self.search_match_index -= 1;
        }
        self.selected_index = self.search_matches[self.search_match_index];
    }

    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    pub fn is_search_match(&self, index: usize) -> bool {
        self.search_matches.contains(&index)
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn colors(&self) -> ThemeColors {
        self.theme.colors()
    }

    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
    }
}

/// Input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
    AiQuery,
    Search,
}

impl InputMode {
    pub fn prompt(&self) -> &'static str {
        match self {
            InputMode::Normal => "",
            InputMode::Filter => "Filter: ",
            InputMode::AiQuery => "AI Query: ",
            InputMode::Search => "Search: ",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_add_log_respects_limit() {
        let mut app = App::new("test-project".to_string());
        for i in 0..150 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), DEFAULT_MAX_LOGS);
    }

    #[test]
    fn test_app_custom_max_logs() {
        let mut app = App::new("test-project".to_string()).with_max_logs(25);
        for i in 0..40 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }
        assert_eq!(app.logs.len(), 25);
    }

    #[test]
    fn test_app_radar_toggle_and_drill_down() {
        let entries = vec![
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Timeout calling payment-svc:8080 after 500ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Timeout calling payment-svc:8080 after 750ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Health check OK".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.radar.clusters.len(), 1);
        assert_eq!(app.radar.clusters[0].count, 2);

        app.toggle_radar();
        assert!(app.show_radar);
        assert!(app.selected_cluster().is_some());

        // Drill down into cluster filters logs to the 2 matching entries
        app.drill_into_selected_cluster();
        assert!(!app.show_radar);
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_app_trace_context_toggle() {
        let entries = vec![
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Gateway request received".to_string()),
                trace: Some("projects/p/traces/trace-abc-12345".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Unrelated background job".to_string()),
                trace: Some("projects/p/traces/other-99999".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("DB timeout in payment service".to_string()),
                trace: Some("projects/p/traces/trace-abc-12345".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.logs.len(), 3);

        let filter = app.toggle_trace_context().unwrap();
        assert_eq!(
            filter.as_deref(),
            Some("trace=\"projects/p/traces/trace-abc-12345\"")
        );
        assert_eq!(app.active_trace.as_deref(), Some("trace-ab"));
        assert_eq!(app.logs.len(), 2);

        let restored = app.toggle_trace_context().unwrap();
        assert!(restored.is_none());
        assert!(app.active_trace.is_none());
        assert_eq!(app.logs.len(), 3);
    }

    #[test]
    fn test_app_local_filter_and_offline_logs() {
        let entries = vec![
            LogEntry {
                severity: Some("INFO".to_string()),
                text_payload: Some("Routine health check".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Database connection timeout".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.logs.len(), 2);

        app.set_filter(Some("severity>=ERROR".to_string()));
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].severity_or_default(), "ERROR");

        app.set_filter(None);
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new("test".to_string());
        app.is_streaming = false;

        for i in 0..10 {
            let entry = LogEntry {
                insert_id: Some(format!("log-{}", i)),
                ..Default::default()
            };
            app.add_log(entry);
        }

        assert_eq!(app.selected_index, 0);
        app.select_next();
        assert_eq!(app.selected_index, 1);
        app.select_last();
        assert_eq!(app.selected_index, 9);
        app.select_first();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_input_mode() {
        let mut app = App::new("test".to_string());

        app.enter_input_mode(InputMode::Filter);
        assert_eq!(app.input_mode, InputMode::Filter);

        app.input_char('t');
        app.input_char('e');
        app.input_char('s');
        app.input_char('t');
        assert_eq!(app.input_buffer, "test");

        app.input_backspace();
        assert_eq!(app.input_buffer, "tes");

        let (text, mode) = app.submit_input();
        assert_eq!(text, "tes");
        assert_eq!(mode, InputMode::Filter);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_app_search_matches_shift_on_ring_buffer_eviction() {
        let mut app = App::new("test".to_string()).with_max_logs(3);
        app.is_streaming = false;

        app.add_log(LogEntry {
            text_payload: Some("needle-first".to_string()),
            ..Default::default()
        });
        app.add_log(LogEntry {
            text_payload: Some("other".to_string()),
            ..Default::default()
        });
        app.add_log(LogEntry {
            text_payload: Some("needle-second".to_string()),
            ..Default::default()
        });

        app.search("needle");
        assert_eq!(app.search_matches, vec![0, 2]);

        // Adding a 4th log evicts index 0 ("needle-first"), shifts index 2 -> 1,
        // and adds a new match at index 2 ("needle-third").
        app.add_log(LogEntry {
            text_payload: Some("needle-third".to_string()),
            ..Default::default()
        });
        assert_eq!(app.logs.len(), 3);
        assert_eq!(app.search_matches, vec![1, 2]);
    }

    #[test]
    fn test_matches_local_filter_labels_and_json_payload() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("request_id".to_string(), "req-42".to_string());

        let entry = LogEntry {
            labels: Some(labels),
            json_payload: Some(serde_json::json!({
                "trace_id": "tr-999",
                "message": "Processed order"
            })),
            ..Default::default()
        };

        assert!(matches_local_filter(&entry, "labels.request_id=\"req-42\""));
        assert!(!matches_local_filter(
            &entry,
            "labels.request_id=\"req-00\""
        ));
        assert!(matches_local_filter(
            &entry,
            "jsonPayload.trace_id=\"tr-999\""
        ));
        assert!(!matches_local_filter(
            &entry,
            "jsonPayload.trace_id=\"tr-000\""
        ));
    }

    #[test]
    fn test_matches_local_filter_boolean_operators_and_quoted_phrases() {
        use crate::gcp::logs::MonitoredResource;
        let entry = LogEntry {
            severity: Some("ERROR".to_string()),
            resource: Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: None,
            }),
            text_payload: Some("Connection reset by peer on upstream".to_string()),
            json_payload: Some(serde_json::json!({
                "message": "Connection reset by peer",
                "service": "order-api"
            })),
            ..Default::default()
        };

        // Must support explicit `AND` without treating "AND" as a literal search word
        assert!(matches_local_filter(
            &entry,
            "severity>=ERROR AND resource.type=\"k8s_container\""
        ));
        // Must support quoted multi-word phrases
        assert!(matches_local_filter(
            &entry,
            "jsonPayload.message=\"Connection reset by peer\""
        ));
        assert!(matches_local_filter(&entry, "\"Connection reset by peer\""));
        // Must support NOT / negation
        assert!(matches_local_filter(
            &entry,
            "severity>=ERROR AND NOT resource.type=\"cloud_run_revision\""
        ));
        assert!(!matches_local_filter(
            &entry,
            "severity>=ERROR AND NOT resource.type=\"k8s_container\""
        ));
    }

    #[test]
    fn test_app_surrounding_context_toggle_and_anchor_selection() {
        use crate::gcp::logs::MonitoredResource;
        use chrono::{DateTime, Utc};

        let ts = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let pod_resource = |pod: &str| {
            let mut m = std::collections::HashMap::new();
            m.insert("pod_name".to_string(), pod.to_string());
            Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: Some(m),
            })
        };

        let entries = vec![
            // Too early (20s before crash on pod-a)
            LogEntry {
                insert_id: Some("e-early".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:00Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Booting service".to_string()),
                ..Default::default()
            },
            // 2s before crash on pod-a (INFO level — should be revealed by `s`!)
            LogEntry {
                insert_id: Some("e-before".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:18Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Pool connections at 99/100".to_string()),
                ..Default::default()
            },
            // Same second on a DIFFERENT pod (pod-b — should be excluded by `s`!)
            LogEntry {
                insert_id: Some("e-other-pod".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:20Z")),
                severity: Some("INFO".to_string()),
                resource: pod_resource("pod-b"),
                text_payload: Some("Healthy request on pod-b".to_string()),
                ..Default::default()
            },
            // The target ERROR log at 13:00:20Z on pod-a
            LogEntry {
                insert_id: Some("e-crash".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:20Z")),
                severity: Some("ERROR".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("FATAL: DB connection pool exhausted".to_string()),
                ..Default::default()
            },
            // 3s after crash on pod-a (WARNING level — should be revealed by `s`!)
            LogEntry {
                insert_id: Some("e-after".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:23Z")),
                severity: Some("WARNING".to_string()),
                resource: pod_resource("pod-a"),
                text_payload: Some("Readiness probe failed".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        // Simulate SRE filtering by severity>=ERROR initially (only 1 log visible: `e-crash`)
        app.set_filter(Some("severity>=ERROR".to_string()));
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].insert_id.as_deref(), Some("e-crash"));

        // Press `s` to open ±5s Surrounding Context on `pod-a` around 13:00:20Z
        let surrounding_filter = app.toggle_surrounding_context().unwrap();
        assert!(surrounding_filter.is_some());
        assert_eq!(
            app.active_surrounding.as_deref(),
            Some("pod-a ±5s @ 13:00:20")
        );

        // Now all 3 logs on `pod-a` within [13:00:15, 13:00:25] (INFO, ERROR, WARNING) are shown!
        assert_eq!(app.logs.len(), 3);
        assert_eq!(app.logs[0].insert_id.as_deref(), Some("e-before"));
        assert_eq!(app.logs[1].insert_id.as_deref(), Some("e-crash"));
        assert_eq!(app.logs[2].insert_id.as_deref(), Some("e-after"));

        // And `selected_index` automatically landed on the anchor ERROR log (index 1)!
        assert_eq!(app.selected_index, 1);
        assert!(app.is_anchor_log(&app.logs[1]));

        // Press `s` again to exit Surrounding Context and restore `severity>=ERROR`
        let restored = app.toggle_surrounding_context().unwrap();
        assert_eq!(restored.as_deref(), Some("severity>=ERROR"));
        assert!(app.active_surrounding.is_none());
        assert_eq!(app.logs.len(), 1);
    }

    #[test]
    fn test_app_input_history_navigation_and_draft_preservation() {
        let mut app = App::new("test".to_string());

        app.enter_input_mode(InputMode::Filter);
        app.input_buffer = "severity>=ERROR".to_string();
        app.submit_input();

        app.enter_input_mode(InputMode::Search);
        app.input_buffer = "connection reset".to_string();
        app.submit_input();

        // Consecutive duplicate should not be recorded twice
        app.enter_input_mode(InputMode::Search);
        app.input_buffer = "connection reset".to_string();
        app.submit_input();
        assert_eq!(app.input_history.len(), 2);

        // Start typing a new draft, then navigate Up/Down
        app.enter_input_mode(InputMode::Filter);
        app.input_buffer = "my draft".to_string();

        app.history_prev();
        assert_eq!(app.input_buffer, "connection reset");

        app.history_prev();
        assert_eq!(app.input_buffer, "severity>=ERROR");

        // Pressing Up at the oldest entry stays at oldest
        app.history_prev();
        assert_eq!(app.input_buffer, "severity>=ERROR");

        // Pressing Down moves forward and then restores the unsubmitted draft
        app.history_next();
        assert_eq!(app.input_buffer, "connection reset");

        app.history_next();
        assert_eq!(app.input_buffer, "my draft");
        assert!(app.history_index.is_none());
    }

    #[test]
    fn test_app_yank_and_console_url_helpers() {
        let mut app = App::new("prod-project-1".to_string()).with_offline_logs(vec![LogEntry {
            insert_id: Some("id-42".to_string()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Disk full".to_string()),
            ..Default::default()
        }]);

        let log_json = app.yank_selected_log_text().unwrap();
        assert!(log_json.contains("\"insertId\": \"id-42\""));
        assert!(log_json.contains("Disk full"));

        assert!(app.yank_ai_response_text().is_err());
        app.set_ai_response("### Root Cause\nDisk `/var/log` reached 100%".to_string());
        assert_eq!(
            app.yank_ai_response_text().unwrap(),
            "### Root Cause\nDisk `/var/log` reached 100%"
        );

        let url = app.selected_log_console_url().unwrap();
        assert!(url.contains("insertId%3D%22id-42%22"));
        assert!(url.ends_with("?project=prod-project-1"));
    }

    #[test]
    fn test_app_multi_project_detection_and_project_filter() {
        let live_multi = App::new("prod-apigw,prod-order,prod-payment".to_string());
        assert!(live_multi.is_multi_project);

        let entries = vec![
            LogEntry {
                log_name: Some("projects/prod-apigw/logs/stdout".to_string()),
                severity: Some("INFO".to_string()),
                text_payload: Some("Gateway received request".to_string()),
                ..Default::default()
            },
            LogEntry {
                log_name: Some("projects/prod-payment/logs/stderr".to_string()),
                severity: Some("ERROR".to_string()),
                text_payload: Some("Payment DB timeout".to_string()),
                ..Default::default()
            },
        ];

        // Even when opened from an offline file, App auto-detects multi-project logs!
        let mut offline_multi =
            App::new("file:incident.json".to_string()).with_offline_logs(entries);
        assert!(offline_multi.is_multi_project);
        assert_eq!(offline_multi.logs.len(), 2);

        // Filtering by `project=prod-payment` isolates logs from that specific project
        offline_multi.set_filter(Some("project=prod-payment".to_string()));
        assert_eq!(offline_multi.logs.len(), 1);
        assert_eq!(
            offline_multi.logs[0].short_project_badge().as_deref(),
            Some("prod-payment")
        );
    }

    #[test]
    fn test_app_pane_focus_cycle_and_maximize_toggle() {
        let mut app = App::new("test".to_string());
        assert_eq!(app.focused_pane, ActivePane::Logs);
        assert!(app.maximized_pane.is_none());

        app.focus_next_pane();
        assert_eq!(app.focused_pane, ActivePane::Detail);

        app.toggle_maximize_pane();
        assert_eq!(app.maximized_pane, Some(ActivePane::Detail));

        // Cycling focus while maximized switches the maximized pane as well
        app.focus_next_pane();
        assert_eq!(app.focused_pane, ActivePane::Ai);
        assert_eq!(app.maximized_pane, Some(ActivePane::Ai));

        app.toggle_maximize_pane();
        assert!(app.maximized_pane.is_none());
    }

    #[test]
    fn test_app_persistent_input_history_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let history_file = dir.path().join("history.json");

        let mut app = App::new("test".to_string());
        app.enter_input_mode(InputMode::Filter);
        app.input_buffer = "severity>=ERROR".to_string();
        app.submit_input();

        app.enter_input_mode(InputMode::Search);
        app.input_buffer = "timeout".to_string();
        app.submit_input();

        app.save_history_to_file(&history_file).unwrap();

        let mut reloaded_app = App::new("test".to_string());
        reloaded_app.load_history_from_file(&history_file).unwrap();
        assert_eq!(
            reloaded_app.input_history,
            vec!["severity>=ERROR".to_string(), "timeout".to_string()]
        );
    }

    #[test]
    fn test_app_mute_selected_pattern_and_restore_on_esc() {
        let entries = vec![
            LogEntry {
                severity: Some("WARNING".to_string()),
                text_payload: Some("GET /healthz 200 OK in 2ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("WARNING".to_string()),
                text_payload: Some("GET /healthz 200 OK in 4ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("DB connection pool exhausted".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        assert_eq!(app.logs.len(), 3);

        // Mute the noisy healthz pattern at index 0
        let muted_fp = app.mute_selected_pattern().unwrap();
        assert_eq!(muted_fp, "GET /healthz <N> OK in <N>ms");
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].message(), "DB connection pool exhausted");

        // Exiting overlay restores all 3 logs
        let restored = app.exit_active_overlay();
        assert_eq!(restored, Some(None));
        assert_eq!(app.logs.len(), 3);
    }

    #[test]
    fn test_app_dynamic_surrounding_window_resize() {
        use crate::gcp::logs::MonitoredResource;
        use chrono::{DateTime, Utc};

        let ts = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let pod_res = || {
            let mut m = std::collections::HashMap::new();
            m.insert("pod_name".to_string(), "pod-a".to_string());
            Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: Some(m),
            })
        };

        let entries = vec![
            // 12 seconds before anchor (outside ±5s, but inside ±15s!)
            LogEntry {
                insert_id: Some("e-12s-before".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:08Z")),
                severity: Some("INFO".to_string()),
                resource: pod_res(),
                text_payload: Some("Upstream slow response".to_string()),
                ..Default::default()
            },
            // Anchor error at 13:00:20Z
            LogEntry {
                insert_id: Some("e-anchor".to_string()),
                timestamp: Some(ts("2026-10-27T13:00:20Z")),
                severity: Some("ERROR".to_string()),
                resource: pod_res(),
                text_payload: Some("Crash".to_string()),
                ..Default::default()
            },
        ];

        let mut app = App::new("offline.json".to_string()).with_offline_logs(entries);
        app.selected_index = 1; // select `e-anchor`

        app.toggle_surrounding_context().unwrap();
        assert_eq!(app.surrounding_window_secs, 5);
        assert_eq!(app.logs.len(), 1); // only `e-anchor` is within ±5s

        // Expand window (`]`) from ±5s -> ±15s
        let new_filter = app.resize_surrounding_window(true).unwrap();
        assert_eq!(app.surrounding_window_secs, 15);
        assert!(new_filter.contains("2026-10-27T13:00:05+00:00"));
        assert_eq!(app.logs.len(), 2); // now `e-12s-before` is included!
        assert_eq!(app.selected_index, 1); // anchor log stays selected
    }

    #[test]
    fn test_app_format_and_save_incident_report() {
        let dir = tempfile::tempdir().unwrap();
        let report_path = dir.path().join("incident-test.md");

        let mut app = App::new("prod-checkout".to_string()).with_offline_logs(vec![LogEntry {
            insert_id: Some("err-99".to_string()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Payment gateway 504 timeout".to_string()),
            ..Default::default()
        }]);
        app.set_filter(Some("severity>=ERROR".to_string()));
        app.set_ai_response("### Root Cause\nDownstream Stripe webhook timed out.".to_string());

        let markdown = app.format_incident_report();
        assert!(markdown.contains("# 🦝 Racoon Incident Snapshot"));
        assert!(markdown.contains("prod-checkout"));
        assert!(markdown.contains("severity>=ERROR"));
        assert!(markdown.contains("Downstream Stripe webhook timed out."));
        assert!(markdown.contains("Payment gateway 504 timeout"));

        let bytes = app.save_incident_report_to_path(&report_path).unwrap();
        assert!(bytes > 100);
        assert!(report_path.exists());
    }

    #[test]
    fn test_app_cycle_bookmarks() {
        let mut app = App::new("prod-proj".to_string())
            .with_bookmarks(vec![
                ("Only Errors".to_string(), "severity>=ERROR".to_string()),
                ("Only Warnings".to_string(), "severity>=WARNING".to_string()),
            ])
            .with_offline_logs(vec![
                LogEntry {
                    severity: Some("INFO".to_string()),
                    text_payload: Some("Info log".to_string()),
                    ..Default::default()
                },
                LogEntry {
                    severity: Some("WARNING".to_string()),
                    text_payload: Some("Warn log".to_string()),
                    ..Default::default()
                },
                LogEntry {
                    severity: Some("ERROR".to_string()),
                    text_payload: Some("Err log".to_string()),
                    ..Default::default()
                },
            ]);

        let (name1, filter1) = app.cycle_next_bookmark().unwrap();
        assert_eq!(name1, "Only Errors");
        assert_eq!(filter1, "severity>=ERROR");
        assert_eq!(app.logs.len(), 1);

        let (name2, filter2) = app.cycle_next_bookmark().unwrap();
        assert_eq!(name2, "Only Warnings");
        assert_eq!(filter2, "severity>=WARNING");
        assert_eq!(app.logs.len(), 2);
    }

    #[test]
    fn test_app_help_modal_spike_alert_and_service_pin() {
        use crate::gcp::logs::MonitoredResource;
        use std::collections::HashMap;

        let mut labels_a = HashMap::new();
        labels_a.insert("service_name".to_string(), "checkout-api".to_string());
        let mut labels_b = HashMap::new();
        labels_b.insert("service_name".to_string(), "inventory-worker".to_string());

        let mut app = App::new("prod-proj".to_string());
        assert!(!app.show_help);
        app.toggle_help();
        assert!(app.show_help);
        app.toggle_help();
        assert!(!app.show_help);

        // Live streaming log with ERROR triggers active_spike_alert and pending_bell
        app.add_log(LogEntry {
            severity: Some("ERROR".to_string()),
            text_payload: Some("Checkout failed on order 42".to_string()),
            resource: Some(MonitoredResource {
                r#type: "cloud_run_revision".to_string(),
                labels: Some(labels_a),
            }),
            ..Default::default()
        });
        assert!(app.active_spike_alert.is_some());
        assert!(app.take_pending_bell());
        assert!(!app.take_pending_bell());

        app.add_log(LogEntry {
            severity: Some("INFO".to_string()),
            text_payload: Some("Inventory synced".to_string()),
            resource: Some(MonitoredResource {
                r#type: "cloud_run_revision".to_string(),
                labels: Some(labels_b),
            }),
            ..Default::default()
        });
        assert_eq!(app.logs.len(), 2);

        // Select first log (checkout-api) and pin its service (`+` key)
        app.selected_index = 0;
        let pinned_filter = app.pin_selected_service().unwrap();
        assert_eq!(
            pinned_filter,
            "resource.labels.service_name=\"checkout-api\""
        );
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.logs[0].message(), "Checkout failed on order 42");
    }
}
