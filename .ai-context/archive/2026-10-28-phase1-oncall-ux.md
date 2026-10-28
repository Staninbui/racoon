# Active Task: Phase 1 On-Call UX & High-ROI Feature Iteration

## Objective
Implement Phase 1 of the user-centric feature roadmap for `gcp-racoon`, addressing the top real-world SRE/developer pain points during live log tailing, incident investigation, and post-mortem reporting:

1. **Smart Structured Log Message & HTTP Summary Extraction (`src/gcp/logs/entry.rs`)**:
   - Extract human-readable summaries from common structured JSON log schemas (`message`, `msg`, `event`, `error`, `err`, `reason`, `description`, nested `error.message`) and `HttpRequest` access logs (`POST 503 https://... (5.004s)`) so non-standard JSON logs and load-balancer entries render cleanly in the TUI log list.
2. **Pane Focus (`Tab`) & Full-Screen Maximize Toggle (`z` key) (`src/ui/app.rs`, `src/ui/input.rs`, `src/ui/mod.rs`, `src/ui/widgets/`)**:
   - Support cycling focus across `Logs`, `Detail`, and `Ai` panes with `Tab`, and toggling full-screen maximize on the active pane with `z` (and `Esc` to unmaximize) so long stacktraces and AI RCA reports are easy to read on compact terminal windows.
3. **Persistent Command & Filter History (`src/config.rs`, `src/ui/app.rs`, `src/ui/mod.rs`)**:
   - Persist `input_history` across TUI sessions to `~/.config/racoon/history.json` so complex GCP filters, search queries, and AI prompts survive restarts.
4. **1-Keystroke Noise Exclusion / Pattern Mute (`-` key) (`src/gcp/logs/filter.rs`, `src/ui/app.rs`, `src/ui/input.rs`)**:
   - Press `-` on any noisy log entry or Error Spike Radar cluster to immediately mute/exclude its normalized pattern (`-fingerprint:"..."`), with `Esc` restoring the previous filter.
5. **Dynamic Surrounding Context Window Scaling (`[` / `]`) & Historical Time-Range CLI (`--from` / `--to` / `--since`) (`src/ui/app.rs`, `src/cli.rs`, `src/utils/mod.rs`, `src/main.rs`)**:
   - Dynamically expand or shrink the `s` Surrounding Context window (`±5s` ↔ `±15s` ↔ `±30s` ↔ `±60s` ↔ `±300s`) using `]` / `[` in TUI.
   - Support `--from` and `--to` (RFC3339, `YYYY-MM-DD HH:MM:SS`, or relative duration) across `tail`, `query`, `export`, and `download`, plus `--since` on `export` and `download`.
6. **1-Keystroke Incident Snapshot Markdown Export (`w` key) (`src/ui/app.rs`, `src/ui/input.rs`, `src/ui/mod.rs`)**:
   - Export a complete, shareable Markdown incident report (`incident-<timestamp>.md` including filter context, Spike Radar clusters, AI RCA report, and correlated log timeline) directly from the TUI.

## Target ROI & Success Metrics
- **On-Call Investigation Speed**: 1-keystroke noise suppression (`-`), 1-keystroke full-screen inspection (`z`), and instant surrounding window scaling (`[` / `]`).
- **Zero Context Loss**: 100% persistence of filter/search/AI history across sessions; 1-keystroke Markdown incident snapshot export (`w`).
- **Quality Gate**: Strict Vertical-Slice TDD (tests written first and verified failing before implementation), `cargo test` 100% passing, `cargo clippy --all-targets -- -D warnings` 0 warnings.
