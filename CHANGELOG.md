# CHANGELOG

## [0.2.0] - 2026-10-29

### Added
- **On-Call TUI Ergonomics & Triage**:
  - Smart structured log message extraction (`message` → `msg` → `event` → `error` → nested `error.message` → `httpRequest` summary).
  - Pane focus cycling (`Tab` / `Shift+Tab`) across Logs, Detail, and AI panes, plus full-screen pane maximize (`z` key).
  - Interactive Keyboard Shortcut Cheat Sheet modal (`h` or `F1` key).
  - 1-keystroke noisy pattern mute (`-` key, `-fingerprint:"..."`) and 1-keystroke positive service/resource pin (`+` / `=` key).
  - Dynamic Surrounding Context window scaling (`[` / `]` keys across `±5s` ↔ `±15s` ↔ `±30s` ↔ `±60s` ↔ `±300s`).
  - Live Spike & New Error alerting (`SpikeAlert` header banner + terminal bell during live tailing).
  - 1-keystroke Markdown Incident Snapshot export (`w` key → `incident-<timestamp>.md`).
  - Search match highlighting in both the Log List and the Detail JSON pane (`/`).
- **Multi-Environment Profiles, Bookmarks & Persistent History**:
  - Named environment profiles (`[profiles.<name>]` in `config.toml`) and global `--profile <name>` CLI flag.
  - Saved filter bookmarks (`[bookmarks]` in `config.toml` + built-in SRE presets) cycled via `b` key in TUI.
  - Persistent input history across TUI sessions (`~/.config/racoon/history.json`).
- **Contextual AI, Universal Log Ingestion & Release Diff**:
  - Multi-turn contextual AI follow-up (`?` key retains prior AI RCA/explanation and selected log context).
  - Standard input pipe (`racoon view -`) and automatic plain-text / container log line parser.
  - Post-deployment error fingerprint comparison (`racoon diff`) across local files (`--baseline-file` / `--current-file`) or live GCP windows (`--baseline 2h..1h --current 1h..now`) with optional `--ai` risk summary.
  - Historical time-range CLI flags (`--from`, `--to`, `--since`) across `tail`, `query`, `export`, and `download`.

### Changed & Fixed
- Completed 3-phase modular architecture refactoring (`src/gcp/logs/*`, `src/ai/*`, `src/ui/*`), eliminating circular dependencies and God files.
- Fixed `StreamHandle` drop lifecycle when restarting live streams on filter/trace changes.
- Rewrote `matches_local_filter` with quote-aware tokenization and boolean operator support (`AND`, `OR`, `NOT`, `-`).
- Added safe default HTTP timeouts (`connect_timeout: 10s`, `timeout: 30s`) across all GCP and AI clients.

## [0.1.0] - 2026-01-10

- Initial release
