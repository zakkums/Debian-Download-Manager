# DDM implementation progress

Use this file to see what's done and what's left. When starting a new chat, share this file so the new context knows the current state.

---

## Status summary

- **Done:** Core engine, resume DB, scheduler, downloader (Easy + threads + curl multi backend), safe resume, retry/backoff, progress durability, abort deadlock fix, Range pre-write validation, redirect-safe header capture, saturating budget release, import-har, bench, HostPolicy persistence, global scheduling, docs, integration test, curl multi phase 2 (config `download_backend`, Easy2 + Multi loop).
- **In progress:** (none)
- **Next (ROI order):** (none).

---

## Done (complete)

Everything below is implemented and merged.

### Engine & infrastructure

- [x] **Workspace layout** – `ddm-core` (lib) + `ddm-cli` (binary), modular `src/` layout.
- [x] **Config** – `~/.config/ddm/config.toml`, `DdmConfig` (connection/segment bounds, optional retry, max_bytes_per_sec, segment_buffer_bytes), load-or-init. Retry policy wired from config in execute; bandwidth/buffer fields reserved for future use.
- [x] **Logging** – Structured logging to `~/.local/state/ddm/ddm.log` (XDG state).
- [x] **Resume DB (SQLite)** – Jobs table (url, filenames, size, etag, last_modified, segment_count, completed_bitmap, state, settings_json). `add_job`, `list_jobs`, `set_state`, `remove_job`, `get_job`, `update_metadata`, `update_bitmap`, `recover_running_jobs`. In-memory tests.
- [x] **HEAD / metadata (`fetch_head`)** – HEAD via curl, parse Content-Length, Accept-Ranges, ETag, Last-Modified, Content-Disposition. Unit tests.
- [x] **URL model (`url_model`)** – Derive safe filename from URL path or Content-Disposition; sanitize; multi-file module. Unit tests.
- [x] **Segmenter (`segmenter`)** – `plan_segments`, `Segment`, `range_header_value()`, `SegmentBitmap` (to_bytes/from_bytes, set_completed/is_completed/all_completed). Unit tests.
- [x] **Storage (`storage`)** – Create, preallocate (fallocate on Unix), write_at, sync, finalize (.part → final). Unit tests.
- [x] **Downloader (`downloader`)** – Segmented Range GETs; one Easy handle per segment in a bounded worker pool of OS threads; Range 206 + Content-Range enforced (post-perform and pre-write). Optional curl multi backend (`download_backend = "multi"` in config): single-threaded Easy2 + Multi loop with per-segment retry and backoff. Unit test for bitmap filtering.
- [x] **Safe resume (`safe_resume`)** – Validate ETag/Last-Modified/size; force-restart if remote changed; download only missing segments. Unit tests.
- [x] **Scheduler (`scheduler`)** – run_one_job / run_next_job; per-host and global connection limits; `GlobalConnectionBudget`; execute phase with progress coalescing and durable bitmap commits.
- [x] **Retry / backoff (`retry`)** – ErrorKind, RetryPolicy, classify SegmentError (Curl, Http, InvalidRangeResponse, PartialTransfer, Storage), run_with_retry. Tests.

### Correctness & robustness (all done)

- [x] **Segment integrity** – After perform(), verify bytes_written == segment.len(); `SegmentError::PartialTransfer`; shared `Arc<AtomicU64>` for write callback and check.
- [x] **Abort on write failure** – write_function returns Ok(0) on write_at failure so transfer aborts (no WriteError::Pause).
- [x] **Durable progress** – Bitmap persisted to DB as segments complete (progress_tx + update_bitmap); crash at 80% does not lose progress.
- [x] **Force-restart cleans temp file** – Remove existing .part before create+preallocate when needs_metadata.
- [x] **Preallocate with fallocate** – posix_fallocate on Unix; fallback to set_len.
- [x] **Job state recovery** – recover_running_jobs() sets running → queued; CLI run calls it before scheduling.
- [x] **bytes_written shared counter (blocker)** – Cell<u64> replaced with Arc<AtomicU64>; integrity check works.
- [x] **Surface storage errors** – SegmentError::Storage(io::Error); classify as Other (no retry).
- [x] **Set job state to Error on failure** – run_one_job sets state = Error on download error; only recover_running_jobs() does running → queued.
- [x] **Low-speed timeout** – low_speed_limit(1024), low_speed_time(60s); hard timeout 3600s safety net.
- [x] **Abort deadlock fix** – On ErrorKind::Other, drain work queue and subtract drained from to_receive so main thread does not block.
- [x] **Range 206 + Content-Range** – Require 206 for range requests; validate Content-Range after perform(); InvalidRangeResponse(code).
- [x] **Range pre-write validation** – First write_function checks headers (parse_http_status + parse_content_range); if not 206 or mismatch, return 0 to abort before writing any byte.
- [x] **Progress durability under errors** – Process results as they arrive; mark bitmap and persist on each Ok; drain and record first error; return error after loop.
- [x] **Progress coalescing** – Coalesce every N completions; abort flag on non-retryable error.
- [x] **Redirect-safe header capture** – With `follow_location(true)`, curl sends headers for each response (e.g. 302 then 206). We clear the header vector when a line starts with `HTTP/` so only the final response’s headers are kept; `parse_http_status` / `parse_content_range` then see 206 and correct Content-Range, avoiding false InvalidRangeResponse on CDN/file-site redirects.
- [x] **Saturating budget release** – `GlobalConnectionBudget::release()` implemented with a compare-exchange loop so it saturates at 0 and is safe under concurrent use (no underflow when multiple jobs run in parallel).

### Features

- [x] **Progress output** – ProgressStats; bytes done, ETA, MiB/s; CLI run prints throttled progress line.
- [x] **import-har** – HAR parse, follow redirects, resolve final URL; JobSettings.custom_headers; CLI `ddm import-har <path> [--allow-cookies]`.
- [x] **HAR resolver selection** – Pick entry whose response looks like a real download (200/206 + Content-Length, prefer 206 + Accept-Ranges); fallback to redirect-chain if none. Unit test `resolve_har_prefers_download_like_entry`.
- [x] **Progress in-flight bytes** – Per-segment atomics updated in write callback; ProgressStats.bytes_in_flight; effective_bytes() for smoother rate; CLI progress uses effective rate.
- [x] **bench** – run_bench 4/8/16 segments; recommend_segment_count; CLI `ddm bench <url>`.
- [x] **Persist HostPolicy** – PersistedHostPolicy JSON; save_to_path/load_from_path; CLI run loads/saves.
- [x] **Global scheduling limits** – GlobalConnectionBudget; reserve/release in execute.
- [x] **Parallel scheduler** – `run_jobs_parallel` (up to N jobs in flight); shared `Arc<Mutex<HostPolicy>>` and `Arc<GlobalConnectionBudget>`; CLI `ddm run --jobs N` (default 1).
- [x] **Docs reality check** – ARCHITECTURE.md and docs_http_client_choice.md updated to describe per-segment Easy + threads; curl multi noted as future option.
- [x] **Checksum** – `checksum::sha256_path(path)` (chunked read, SHA-256, hex); CLI `ddm checksum <path>`. Unit tests; off the hot path.

### CLI & tests

- [x] **CLI scaffold** – clap subcommands: add, run, status, pause, resume, remove, import-har, bench, checksum.
- [x] **CLI wired to DB** – add/status/pause/resume/remove use ResumeDb; async main with tokio.
- [x] **Tests** – Unit tests in ddm-core (config, resolver, resume_db, fetch_head, url_model, segmenter, storage, downloader, safe_resume, retry, host_policy, bench); downloader/multi handler tests (header clear, write 206/non-206); CLI parse tests. `cargo test` passes.
- [x] **Integration test** – Local HTTP server with Range support (`tests/common/range_server.rs`); `tests/integration_range_download.rs`: multi-segment download (Easy and multi backend) against local server, file content verified. Resume behavior covered by unit tests (bitmap, safe_resume). `ResumeDb::open_at(path)` added for test DB placement.

---

## In progress

- (none)

---

## Done (this session)

- [x] **Curl multi – phase 2** – Implemented curl::multi handle; single-threaded event loop, Easy2 + Handler per segment; config `download_backend` (easy | multi); parity with Easy+threads (206/Content-Range, progress, bitmap). No per-segment retry in multi yet.
- [x] **Execute module &lt;200 lines** – Split `scheduler/execute/mod.rs` (was 201 lines) into `execute/run_download.rs`; all source files now &lt;200 lines per code layout guideline.

---

## Not started (in priority order, best ROI)

- (none)

---

## Done (tests for new code)

- [x] **Tests for multi backend** – Unit tests for multi handler (header clear on HTTP/, write rejects non-206, write accepts 206 and writes at offset); integration test `multi_backend_download_completes_and_file_matches` runs full download with `download_backend = "multi"`.

---

## Done (retry in multi backend)

- [x] **Retry in multi backend** – Per-segment retry with backoff inside multi loop; optional `RetryPolicy` passed from `download_segments_multi`; retry_after queue with `Instant`; refill from pending and retry_after; `next_retry_wait_ms` for wait timing; `refill.rs` and `result.rs` keep `run.rs` < 200 lines.

---

## Reference (historical / narrative)

### Blocker bug (fixed)

Segment integrity was broken by `Cell<u64>` (clone gave a separate cell). Fixed with `Arc<AtomicU64>`; write callback and post-perform share the same counter.

### Abort deadlock (fixed)

When ErrorKind::Other, workers stopped pulling; main thread waited for count results but fewer were sent. Fix: drain queue and subtract drained from to_receive in downloader/run.rs.

### Range pre-write validation (done)

Server could return 200 + full body for a Range request; we validated after perform() so bytes were already written. Fix: in first write_function, parse status and Content-Range from headers; if not 206 or mismatch, return 0 to abort before writing.

### Docs vs code (done)

Docs said "libcurl multi"; code uses Easy + threads. ARCHITECTURE and docs_http_client_choice updated to match.

### Redirect header false-fail (fixed)

With `follow_location(true)`, curl’s header callback receives headers for every response (e.g. 302 redirect then 206). We were storing all lines and `parse_http_status(headers.first())` saw the first status (302), so the pre-write check aborted with InvalidRangeResponse even when the final response was 206. Fix: in the header callback, when a line starts with `HTTP/`, clear the header vector then push that line so “current headers” always correspond to the final response.

### Budget release underflow (fixed)

`GlobalConnectionBudget::release()` did load then fetch_sub with a stale value; safe with single-threaded budget usage but would underflow with parallel jobs. Fix: implement release with a compare-exchange loop that saturates at 0 (no wraparound).

---

## Code layout & modularity

- Prefer small, focused modules; subdirectories when a feature spans multiple concerns.
- Aim for **< 200 lines per file** (excluding tests). Split into folder + mod.rs + submodules when longer.
- Keep public API in mod.rs or re-exported; new large features as multi-file modules from the start.

---

## Quick reference

- **Run tests:** `cargo test`
- **Run CLI:** `cargo run -p ddm-cli -- <subcommand> ...`
- **Config:** `~/.config/ddm/config.toml`
- **State / DB / logs:** `~/.local/state/ddm/` (jobs.db, ddm.log)
