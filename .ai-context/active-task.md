# Active Task: Fix Top 5 Production Bugs & Engineering Bottlenecks

## Objective
Fix the 5 core bugs and production bottlenecks identified during the GCP Architect & DevOps review:
1. **UTF-8 Multi-byte Slicing Panic (`src/gcp/logs.rs`, `src/main.rs`, `src/ai/gemini.rs`)**: Replace byte-index slicing (`&message[..150]`, `&token[..20]`, `&k[..8]`) with char-boundary-safe truncation so Chinese/Japanese/Emoji logs never panic the TUI.
2. **Streaming Polling Reliability (`src/gcp/logs.rs`, `src/ui/mod.rs`)**:
   - Prevent dropping same-timestamp logs at poll boundaries by using `timestamp >= last_timestamp` + `insert_id`/entry deduplication window.
   - Add exponential backoff on transient/quota errors (HTTP 429 / 5xx) in the streaming loop.
   - Properly manage and stop `StreamHandle` on stream restart/drop so orphaned polling tasks do not leak GCP API quota.
3. **Pagination Support in `list_entries` & `export_logs` (`src/gcp/logs.rs`, `src/storage/export.rs`)**:
   - Connect `list_entries_paged` into `list_entries` so requests with `limit > 1000` (or multi-page responses) follow `next_page_token` until `limit` is reached or no more pages exist.
4. **TUI Buffer, Search Index Sync & Local Filter Optimization (`src/ui/app.rs`)**:
   - Keep `search_matches` and `search_match_index` consistent when `add_log` evicts oldest entries at `max_logs` capacity.
   - Optimize `matches_local_filter` and `search` to avoid redundant repeated `serde_json::to_string(entry)` allocations per token.
5. **Documentation Synchronization (`README.md`)**:
   - Document `c` (Error Spike Radar + `Enter` drill-down), `x` (Distributed Trace Context), `E` (Full-Trace AI RCA), and updated default model (`gemini-3.8-flash`) in `README.md`.
