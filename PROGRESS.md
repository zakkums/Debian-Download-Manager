# DDM implementation progress

Use this file to see what’s done and what’s left. When starting a new chat, share this file so the new context knows the current state.

---

## Review summary & roadmap

### What looks good

- **Modularity** – Core engine is independent of CLI; “future” pieces are isolated (resolver, host_policy, checksum).
- **Resume DB** – Compact bitmap stored as BLOB, flexible per-job JSON settings, XDG-friendly storage path.
- **Segmenter + bitmap** – Simple and test-covered.
- **Safe resume** – Compares ETag / Last-Modified / Content-Length and forces explicit restart if the remote changed (good correctness baseline).
- **Downloader** – Worker pool bounded by `max_concurrent`, per-segment Range GETs, shared pwrite-style storage writer.
- **Storage lifecycle** – `.part` temp file + atomic rename finalize.

### Correctness & performance gaps (priority order)

Work through these in order before adding new features.

1. **Segment integrity not verified** – A segment “succeeds” on 2xx but bytes are not checked. If the server closes early or returns a partial body, the final file can be silently corrupted.  
   **Fix:** After `transfer.perform()`, check `bytes_written.get()` equals `segment.len()`. If not, return an error so the retry policy retries.

2. **Write failures mapped to Pause** – In `write_function`, if `write_at` fails the code returns `curl::easy::WriteError::Pause`, which can pause the transfer instead of failing cleanly.  
   **Fix:** Return an abort-style error so the segment fails and retries, not “pause”.

3. **Resume progress not durable mid-download** – Bitmap is updated in memory and only written to SQLite when the whole run completes. A crash at 80% loses segment completion info.  
   **Fix:** Batch-commit bitmap to DB every N segments (e.g. 2–4) or every T seconds (e.g. 1s), or commit as results arrive via a channel from workers → scheduler.

4. **`--force-restart` doesn’t fully restart the temp file** – Metadata/bitmap are reset, but if `.part` exists it is opened and written into. If segment count or file size changes, behavior can be wrong.  
   **Fix:** On restart, delete/truncate the temp file, then recreate and preallocate.

5. **Preallocation uses `set_len` only** – Works but doesn’t guarantee blocks are allocated (sparse files). For better throughput and less fragmentation on Linux, use real block allocation.  
   **Fix:** Use `libc::fallocate` / `posix_fallocate` on Linux when available; fall back to `set_len` if it fails.

6. **Job state recovery edge case** – If the process crashes while a job is “running”, it stays “running” in the DB and the scheduler only runs “queued” jobs, so the job can be stranded.  
   **Fix:** On startup (or before scheduling), normalize `running` → `queued` unless real “active worker” tracking is implemented.

### Hardware / tuning notes (Ryzen 7 3700X + NVMe)

- Bottleneck is usually network/server throttling, not CPU/disk. Current approach (8–16 concurrent segments) is the right shape.
- **Segments/connections:** Start at 8 or 16 per host; too many can reduce throughput if the host throttles.
- **Timeouts:** A hard 300s per segment can hurt large segments on slow links; consider “low speed” thresholds instead of absolute timeouts.
- **Disk:** Ensure downloads land on NVMe, not a slower filesystem.
- **Kernel TCP (optional):** BBR congestion control and socket buffer tuning can help depending on path.

### Missing vs CLI surface

- `import-har` and `bench` are stubbed but unimplemented.
- `host_policy` / `checksum` are placeholders (fine for now).

### Recommended next steps (best ROI sequence)

1. **Segment integrity check + abort-on-write-fail** – Prevents silent corruption and enables correct retry.
2. **Durable progress commits** – Resume actually works under crashes.
3. **Force-restart cleans temp file** – Predictable behavior when restarting.
4. **`fallocate` on Linux** – Performance polish for preallocation.
5. **Progress UI / stats** – So improvements can be measured.
6. Only after the above: consider curl multi (threads are fine up to current segment counts).

---

## Code layout & modularity

- **Modular multi-folder, multi-file design** – Prefer small, focused modules. Use **subdirectories** (e.g. `url_model/`) when a feature spans multiple concerns or would make a single file long.
- **Avoid long files** – Aim for **< 250 lines per file** (excluding tests). If a module grows beyond that, split it into a folder with `mod.rs` and one or more submodules (e.g. `content_disposition.rs`, `sanitize.rs`).
- **Keep structure sorted and clear** – Group related code in the same module; keep public API in `mod.rs` or re-exported from submodules. New, large features should be added as **multi-file modules from the start** so the codebase stays navigable and easy to extend.

---

## Done

- [x] **Workspace layout** – `ddm-core` (lib) + `ddm-cli` (binary), modular `src/` layout.
- [x] **CLI scaffold** – `clap` subcommands: `add`, `run`, `status`, `pause`, `resume`, `remove`, `import-har`, `bench`.
- [x] **Config** – `~/.config/ddm/config.toml`, `DdmConfig` (connection/segment bounds), load-or-init.
- [x] **Logging** – Structured logging to `~/.local/state/ddm/ddm.log` (XDG state).
- [x] **Resume DB (SQLite)** – `ResumeDb` in `resume_db`, jobs table (url, filenames, size, etag, last_modified, segment_count, completed_bitmap, state, settings_json). `add_job`, `list_jobs`, `set_state`, `remove_job`. `get_job(id)` returns full metadata + settings; `update_metadata(id, JobMetadata)` fills `final_filename`, `temp_filename`, `total_size`, `etag`, `last_modified`, `segment_count`, `completed_bitmap`. In-memory tests.
- [x] **CLI wired to DB** – `add`/`status`/`pause`/`resume`/`remove` use `ResumeDb`; async main with tokio.
- [x] **Tests** – Unit tests for config, resolver, resume_db (in-memory); CLI parse tests. `cargo test` passes.
- [x] **Docs** – `ARCHITECTURE.md`, `docs_http_client_choice.md`, Testing section.
- [x] **HEAD / metadata probe (`fetch_head`)** – `probe(url, custom_headers)` via curl: HEAD request, parse `Content-Length`, `Accept-Ranges: bytes`, `ETag`, `Last-Modified`, `Content-Disposition`. Unit tests for header parsing.
- [x] **URL model (`url_model`)** – `derive_filename(url, content_disposition)` derives safe filename from URL path or Content-Disposition; `parse_content_disposition_filename` (quoted, token, `filename*=UTF-8''`); `filename_from_url_path`; `sanitize_filename_for_linux` (no `/`, NUL, control chars; trim dots/spaces; 255-byte limit). Unit tests for derivation, CD parsing, URL path, and sanitization. Implemented as multi-file module: `url_model/mod.rs`, `content_disposition.rs`, `path.rs`, `sanitize.rs`.
- [x] **Segmenter (`segmenter`)** – `plan_segments(total_size, segment_count)`; `Segment` with `start`/`end` (half-open) and `range_header_value()` for HTTP Range; `SegmentBitmap` with `new`/`from_bytes`/`to_bytes` (DB BLOB), `set_completed`/`is_completed`/`all_completed`. Unit tests for range math and bitmap.
- [x] **Storage (`storage`)** – `StorageWriterBuilder::create` / `preallocate` / `build`; `StorageWriter::write_at` (pwrite), `sync`, `finalize` (rename `.part` → final); `temp_path()`. Unit tests for create/preallocate/write/finalize and concurrent-style write_at.
- [x] **Downloader (`downloader`)** – `download_segments(url, headers, segments, storage, bitmap)`: N concurrent GETs via curl (one thread per incomplete segment), Range header, write to storage at offset, update bitmap on completion. Input: direct URL + optional headers only. Unit test for bitmap filtering.
- [x] **Safe resume (`safe_resume`)** – On start, re-validate ETag/Last-Modified and size; if changed, require explicit user override (`--force-restart`); else download only missing segments per bitmap. Module: `safe_resume/` with `validate.rs` (comparison logic) and `mod.rs`; `validate_for_resume(job, head)`; `ValidationError::RemoteChanged`. Scheduler `run_one_job` / `run_next_job`: probe → validate → update metadata if force or first run → open/create storage → download only incomplete segments → persist bitmap and finalize if done. CLI `run` with `--force-restart`; storage `open_existing` for resume. Unit tests for validation (no metadata OK, same OK, etag/size/last_modified changed Err).
- [x] **Scheduler (`scheduler`)** – Coordinate jobs; respect per-host and global connection limits. `download_segments(..., max_concurrent: Option<usize>)` with worker-pool when set; scheduler passes `min(max_connections_per_host, max_total_connections, segment_count)`; `ddm run` processes all queued jobs in a loop (FIFO by job id). One job at a time, each job’s segments bounded by config.
- [x] **Retry / backoff (`retry`)** – Error classification (timeouts, 429/503, connection resets); exponential backoff. `retry/`: policy (ErrorKind, RetryPolicy, RetryDecision); classify (SegmentError, classify_http_status, classify_curl_error, run_with_retry). Downloader uses SegmentError and run_with_retry per segment; scheduler passes RetryPolicy::default(). Tests: policy + classify. Per-host concurrency reduction deferred to host_policy.
- [x] **Segment integrity** – After `transfer.perform()`, verify `bytes_written.get() == segment.len()`; on mismatch return `SegmentError::PartialTransfer { expected, received }` for retry. Prevents silent corruption.
- [x] **Abort on write failure** – In downloader `write_function`, return `Ok(0)` when `write_at` fails so libcurl aborts the transfer (segment fails and retries); no longer use `WriteError::Pause`. Retry classify: `PartialTransfer` and curl write errors map to `ErrorKind::Connection`.
- [x] **Durable progress commits** – Bitmap persisted to SQLite as segments complete. `ResumeDb::update_bitmap(id, bitmap)`; downloader accepts optional `progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>` and sends bitmap after each completed segment; scheduler runs a receiver task that calls `update_bitmap` so a crash mid-download doesn’t lose progress.
- [x] **Force-restart cleans temp file** – When `needs_metadata` (force-restart or remote changed), scheduler removes existing `.part` with `tokio::fs::remove_file` before creating storage; then create + preallocate so segment count/size changes don’t leave bad state.
- [x] **Preallocate with fallocate** – On Unix, `StorageWriterBuilder::preallocate` tries `posix_fallocate` first (real block allocation); on failure or non-Unix falls back to `set_len`. `libc` under `[target.'cfg(unix)'.dependencies]`.
- [x] **Job state recovery** – `ResumeDb::recover_running_jobs()` sets all `running` → `queued`; CLI `run` calls it before the scheduling loop so crashed jobs are not stranded. Unit test `recover_running_jobs_resets_to_queued`.

---

## In progress

- (none)

---

## Not started (order = ROI sequence above; do correctness items first)

### Correctness & robustness (do first)

### Progress and tuning

- [ ] **Progress output** – Bytes done, ETA, per-connection rate, total rate (no GUI; CLI-friendly). Do after correctness items so improvements can be measured.

### Optional and polish

- [ ] **Checksum (`checksum`)** – Optional SHA-256 after completion; off the hot path.
- [ ] **Config extensions** – Retry policy, bandwidth cap, segment buffer size in `config.toml`.
- [ ] **HAR resolver (optional)** – Parse HAR → direct URL + minimal headers; `import-har` flow; cookie warning and `--allow-cookies`; keep core resolver-agnostic.
- [ ] **Bench mode** – `ddm bench <url>`: try different segment counts, report throughput.
- [ ] **Curl multi (later)** – Consider only after correctness + durable progress + progress UI; threads are fine for current segment counts.

### Integration and quality

- [ ] **Integration test** – Local HTTP server with Range support; multi-segment download + resume.
- [ ] **Tests for new code** – Unit tests for new modules; update PROGRESS when adding tests. (fetch_head: header parsing; url_model: derivation, CD parsing, sanitize; segmenter: range math, bitmap; storage: create/preallocate/write/finalize; downloader: bitmap filtering; host_policy: host key parsing, range support, throttling heuristics; adaptive: step up/down on throughput and throttle.)

---

## Quick reference

- **Run tests:** `cargo test`
- **Run CLI:** `cargo run -p ddm-cli -- <subcommand> ...`
- **Config:** `~/.config/ddm/config.toml`
- **State / DB / logs:** `~/.local/state/ddm/` (jobs.db, ddm.log)
