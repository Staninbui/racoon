# Active Task: Phase 3 Final UX Closure (Help Modal, Live Spike Alerting, Service Pin `+` & Release Prep)

## Objective
Implement the final polish and UX closure features for `gcp-racoon` (`v0.2.0`):

1. **Interactive Keyboard Shortcut Help Modal (`h` / `F1` key) (`src/ui/app.rs`, `src/ui/input.rs`, `src/ui/mod.rs`, `src/ui/widgets/detail.rs`)**:
   - Toggle a categorized full-pane/overlay keyboard cheat sheet with `h` or `F1` (and close with `h`, `F1`, or `Esc`).
2. **Real-Time Spike & New Error Alerting (`SpikeAlert`, Header Badge & Terminal Bell) (`src/ui/radar.rs`, `src/ui/app.rs`, `src/ui/mod.rs`)**:
   - Emit `SpikeAlert` from `SpikeRadar::record_entry` when a brand-new `ERROR`/`CRITICAL` fingerprint appears or an existing pattern crosses the spike threshold (`count == 3`).
   - Surface the active alert in `App` (`active_spike_alert`, `pending_bell`) and render a prominent `🚨` alert indicator in the TUI header bar with terminal bell notification during live streaming.
3. **1-Keystroke Positive Service / Resource Filter Pin (`+` / `=` key) & Detail JSON Search Highlighting (`src/gcp/logs/entry.rs`, `src/ui/app.rs`, `src/ui/input.rs`, `src/ui/widgets/detail.rs`)**:
   - Add `LogEntry::service_filter_clause` and `App::pin_selected_service` (`+` / `=` key in Normal mode) to isolate logs from the same `service_name` / `container_name` / `pod_name` / `resource.type`, with `Esc` restoring the previous filter.
   - Highlight active `/` search matches inside the Detail JSON pane.
4. **Release Documentation & Changelog (`CHANGELOG.md`, `Cargo.toml`, `README.md`)**:
   - Bump crate version to `0.2.0` and document all architectural refactoring + Phase 1–3 features in `CHANGELOG.md` and `README.md`.

## Target ROI & Success Metrics
- **Discoverability**: 1-keystroke access (`h` / `F1`) to all ~20 TUI shortcuts grouped by workflow.
- **Passive Monitoring**: Instant visual + terminal bell notification on new error patterns or spikes.
- **Triage Precision**: 1-keystroke service isolation (`+` key) complementing `-` pattern mute.
- **Quality Gate**: Strict Vertical-Slice TDD, `cargo test` 100% passing, `cargo clippy --all-targets -- -D warnings` 0 warnings.
