# Active Task: Phase 2 Multi-Environment Profiles, Contextual AI Follow-Up, Stdin Ingestion & Release Diff

## Objective
Implement Phase 2 of the user-centric feature roadmap for `gcp-racoon`, delivering multi-environment workflow efficiency, stateful AI follow-up conversations, universal log pipe ingestion, and post-deployment regression diffing:

1. **Named Environment Profiles (`--profile`) & Filter Bookmarks (`b` key in TUI) (`src/config.rs`, `src/cli.rs`, `src/ui/app.rs`, `src/ui/input.rs`, `src/main.rs`)**:
   - Support `[profiles.<name>]` and `[bookmarks]` in `config.toml`.
   - Add global `--profile <name>` CLI flag to apply environment-specific `project`, `impersonate_service_account`, `filter`, `severity`, and `since`.
   - Add `b` key in TUI to cycle through user-configured + built-in high-value SRE filter bookmarks (`Errors & Criticals`, `Warnings & Above`, `HTTP 5xx Failures`, `Cloud Run Errors`, `GKE Container Errors`).
2. **Contextual AI Multi-Turn Follow-up (`?` key with active RCA/Explanation context) (`src/ai/prompts.rs`, `src/ai/agent.rs`, `src/ui/mod.rs`)**:
   - Add `AiClient::ask_followup` (with PII redaction and semantic log compaction) so asking a follow-up question via `?` when `ai_response` is present retains the previous AI diagnosis and active log/trace context.
3. **Stdin Pipe (`racoon view -`) & Plain-Text Log Line Ingestion (`src/storage/import.rs`, `src/main.rs`)**:
   - Support `racoon view -` to ingest piped logs from `stdin` (`kubectl logs ... | racoon view -`, `docker logs`, `gcloud logging read`).
   - Auto-detect JSON arrays, NDJSON, and plain-text log lines (extracting timestamps and severity levels like `ERROR`, `WARN`, `INFO` automatically).
4. **Post-Deployment Error Fingerprint Diff (`racoon diff`) (`src/ui/radar.rs`, `src/cli.rs`, `src/main.rs`)**:
   - Add `racoon diff` command and `compute_log_diff(baseline, current) -> LogDiffReport` to compare two local log files (`--baseline-file` / `--current-file`) or two GCP time windows (`--baseline 2h..1h --current 1h..now`), highlighting **New Error Regressions (🆕)**, **Frequency Spikes (📈)**, and **Resolved Patterns (✅)**, with optional `--ai` release risk summary.

## Target ROI & Success Metrics
- **Multi-Env Switching**: 1-flag environment switch (`--profile`) and 1-keystroke bookmark cycling (`b`).
- **AI Continuity**: 100% retention of prior RCA/Explanation context during follow-up `?` queries.
- **Universal Compatibility**: Seamless `stdin` and plain-text log ingestion (`racoon view -`) + automated release regression detection (`racoon diff`).
- **Quality Gate**: Strict Vertical-Slice TDD, `cargo test` 100% passing, `cargo clippy --all-targets -- -D warnings` 0 warnings.
