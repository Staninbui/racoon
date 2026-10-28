# Active Task: Comprehensive 3-Phase Architecture & Code Quality Refactoring

## Objective
Execute a comprehensive, behavior-preserving architectural refactoring and bug-fixing initiative across `gcp-racoon` in 3 structured phases:
1. **Phase 1 (P0 Bug Fixes & Runtime Hardening)**:
   - Fix `StreamHandle` drop/lifecycle bug in `restart_live_stream` (`src/ui/mod.rs`, `src/gcp/logs.rs`) where newly restarted streams immediately terminate because `_handle` is dropped at the end of the `match` arm.
   - Fix `matches_local_filter` (`src/ui/app.rs`) so boolean operators (`AND`, `OR`, `NOT`), quoted phrases with spaces, and parenthesized clauses do not falsely drop valid GCP logs.
   - Fix Error Spike Radar `Enter` drill-down (`drill_into_selected_cluster`) to save and restore `previous_filter` on `Esc` without sending local-only `fingerprint:...` pseudo-filters to GCP REST API.
   - Fix `export_logs` (`src/storage/export.rs`) overwriting `ExportOptions.format` with path-inferred format.
   - Add default HTTP timeouts (`connect_timeout = 10s`, `timeout = 30s`) across all `reqwest::Client` instances and pre-compile regexes in `utils::fix_relative_timestamps` using `OnceLock<Regex>`.
2. **Phase 2 (P1 Architectural Decoupling & Deduplication)**:
   - Eliminate `ai -> ui` reverse dependency by moving `fingerprint_message` to the domain/utility layer (with backward-compatible re-exports).
   - Move TUI widget `src/ai/selector.rs` into `src/ui/selector.rs` (preserving re-exports in `src/ai`).
   - Unify `ProviderKind` alias resolution in `Config::get_provider_api_key` and `Config::get_provider_base_url` (`src/config.rs`) and unify `extract_first_gcp_project_id`.
   - Remove legacy manual `split('=')` TOML parser `get_api_key()` in `src/ai/gemini.rs`.
   - Complete `RacoonError` in `src/error.rs` to cover all domain error types.
3. **Phase 3 (P2 God-Object Modular Splitting & State Machine Cleanup)**:
   - Split `src/gcp/logs.rs` (1,363 lines) into `src/gcp/logs/{mod.rs, client.rs, entry.rs, filter.rs}`.
   - Split `src/ai/gemini.rs` (1,145 lines) into `src/ai/{gemini.rs, providers.rs, cache.rs}` and consolidate all prompts into `src/ai/prompts.rs`.
   - Refactor `src/ui/app.rs` (1,365 lines) by moving filter parsing to `gcp::logs::filter` and unifying context-mode transitions (Radar Drill-down / Trace / Surrounding) with proper `previous_filter` restoration.
   - Deduplicate GCP Auth & Project initialization boilerplate in `src/main.rs`.

## Target ROI & Success Metrics
- **Reliability**: 0 premature stream terminations on filter/trace switch; 0 indefinite network hangs on stalled TCP connections; 100% accuracy on multi-clause local filters.
- **Maintainability**: Reduce maximum single-file line count by >40% (no 1,300+ line God files); 0 circular/reverse dependencies between `ai` and `ui`.
- **Compatibility & Quality Gate**: 100% public library API backward compatibility (`lib.rs` exports unchanged), `cargo test` 100% passing, `cargo clippy -- -D warnings` 0 warnings.
