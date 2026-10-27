# Technical Specification: Top 5 Bug Fixes

## 1. UTF-8 Safe Truncation
- **Target**: `LogEntry::message_preview` in `src/gcp/logs.rs`, token/key previews in `src/main.rs`.
- **Change**: Use `.chars()` iterator (`chars().count() > 150` / `chars().take(150).collect()`) instead of `&message[..150]`.
- **Verification**: Unit test with multi-byte CJK and Emoji strings where byte index 150 lands mid-codepoint.

## 2. Streaming Deduplication, Backoff & StreamHandle Lifecycle
- **Target**: `src/gcp/logs.rs` (`LogsClient::stream_entries`, `StreamHandle`), `src/ui/mod.rs` (`run_app`, `restart_live_stream`).
- **Change**:
  - Implement `Drop` for `StreamHandle` (`let _ = self.stop_tx.try_send(());`) so replacing or dropping a `StreamHandle` immediately signals the spawned polling task to stop.
  - Keep `stream_handle: Option<StreamHandle>` in `run_app` / `restart_live_stream`.
  - In `stream_entries`, query with `timestamp_from` (`timestamp >= last_timestamp`) and deduplicate entries using a bounded `VecDeque<String>` of recently seen keys (`insert_id` or fallback signature) so logs sharing the same timestamp are never missed or duplicated.
  - Add exponential backoff in the polling loop when `poll_logs` returns an error (especially HTTP 429 / 5xx), scaling delay up to 30s and resetting to `POLL_INTERVAL_SECS` on success.

## 3. Multi-Page Pagination in `LogsClient::list_entries`
- **Target**: `src/gcp/logs.rs` (`LogsClient::list_entries`).
- **Change**: Loop over `self.list_entries_paged(filter, batch_size, page_token.as_deref())` where `batch_size = (limit - collected.len()).min(1000)`, accumulating entries until `collected.len() >= limit` or `next_page_token` is `None`.
- **Verification**: Unit test verifying pagination logic and `limit > 1000` handling.

## 4. TUI Search Sync on Eviction & Filter Performance
- **Target**: `src/ui/app.rs` (`App::add_log`, `App::search`, `matches_local_filter`).
- **Change**:
  - When evicting the oldest entry in `add_log`, decrement indices in `self.search_matches` and remove index `0` if it was a match; if the new entry matches `self.search_query`, append its index.
  - In `matches_local_filter`, lazily compute the lowercase JSON fallback string at most once across all filter tokens instead of once per token.

## 5. README.md Sync
- **Target**: `README.md`.
- **Change**: Update Features, Keyboard Shortcuts (`c`, `Enter`, `x`, `E`), Configuration (`gemini-3.8-flash`), and AI Features sections.
