# DDM implementation progress

Use this file to see what’s done and what’s left. When starting a new chat, share this file so the new context knows the current state.

---

## To do (do first) — DONE

1. ~~**Progress durability under errors**~~ – **Fixed.** Process results as they arrive from `rx.recv()`. Mark bitmap on each Ok, push coalesced bitmap to DB, record first error and drain all, return error after loop. Partial success is saved when one segment fails.
2. **Progress channel / coalescing** – **Done.** Progress coalesced every N completions so drops are intentional.
3. ~~**Abort flag for non-retryable errors**~~ – **Done.** On `ErrorKind::Other` we set `abort_requested`; workers stop pulling more work.

---

## Review summary & roadmap

### What looks good

- **Modularity** – Core engine is independent of CLI; “future” pieces are isolated (resolver, host_policy, checksum).
- **Resume DB** – Compact bitmap stored as BLOB, flexible per-job JSON settings, XDG-friendly storage path.
- **Segmenter + bitmap** – Simple and test-covered.
- **Safe resume** – Compares ETag / Last-Modified / Content-Length and forces explicit restart if the remote changed (good correctness baseline).
- **Downloader** – Worker pool bounded by `max_concurrent`, per-segment Range GETs, shared pwrite-style storage writer.
- **Storage lifecycle** – `.part` temp file + atomic rename finalize.

### Fix before continuing (blocker) — DONE

**Blocker bug (fixed):** The segment integrity check was broken by a bug in `download_one_segment()`.

- **Cause:** `Cell<u64>` is `Copy`/`Clone` by value, so `bytes_written.clone()` gave a **separate** cell; the write callback updated its copy but `received = bytes_written.get()` was read from the original (always 0).
- **Fix applied:** Replaced with `Arc<AtomicU64>`; write callback uses `fetch_add` for offset and the post-perform check uses `load()`, so both share the same counter. Integrity check now works correctly.

---

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

### Remaining high-value fixes (next priorities, after blocker)

7. **Surface storage errors properly** – A disk write failure currently returns `Ok(0)` inside the curl write callback, which becomes a curl write error; the classifier maps it to **Connection**, so storage failures (disk full, permission denied) get retried as if they were connection issues.  
   **Fix:** Stash the actual IO error from `write_at()` in a shared variable; when curl fails with a write error, return a dedicated `SegmentError::Storage(...)` and classify it as **Other** (no retry).

8. **Make job failure state explicit** – `JobState::Error` exists, but `run_one_job()` doesn't set it on failure. A segment failure can leave the job "running" until the next `recover_running_jobs()` pass.  
   **Fix:** On any download error, set state = `Error` (optionally store a `last_error` string later). Only `recover_running_jobs()` should convert `running` → `queued` (for crash recovery), not for normal failures.

9. **Segment timeout is too rigid for large segments** – Per-segment `easy.timeout(300s)` can kill legitimate large transfers on slow links or when the host throttles. Prefer a **low-speed timeout** (abort only if throughput drops under X for Y seconds) rather than a hard wall-clock timeout.

### Hardware / tuning notes (Ryzen 7 3700X + NVMe)

- Bottleneck is usually network/server throttling, not CPU/disk. Current approach (8–16 concurrent segments) is the right shape.
- **Segments/connections:** Start at 8 or 16 per host; too many can reduce throughput if the host throttles.
- **Timeouts:** A hard 300s per segment can hurt large segments on slow links; consider “low speed” thresholds instead of absolute timeouts.
- **Disk:** Ensure downloads land on NVMe, not a slower filesystem.
- **Kernel TCP (optional):** BBR congestion control and socket buffer tuning can help depending on path.

### Missing vs CLI surface

- `import-har` and `bench` are stubbed but unimplemented.
- `host_policy` / `checksum` are placeholders (fine for now).

### Current status (summary)

- **Architecture:** Strong. Recent changes are in the right direction and mostly correct.
- **Done:** bytes_written fix; segment integrity; abort on write fail; storage errors; job state set to Error on failure; low-speed timeout for segments; progress output; abort deadlock fix (drain work queue when aborting so main thread doesn’t wait forever). **Next:** Range enforcement, import-har, bench, persist HostPolicy, global scheduling.

### Recommended next steps (best ROI sequence)

1. ~~**Fix bytes_written shared counter**~~ – Done.
2. ~~**Segment integrity check + abort-on-write-fail**~~ – Done.
3. ~~**Surface storage errors**~~ – Done; `SegmentError::Storage(io::Error)`, classify as Other.
4. ~~**Set job state to Error on failure**~~ – Done.
5. ~~**Durable progress commits**~~ – Resume actually works under crashes.
6. ~~**Force-restart cleans temp file**~~ – Predictable behavior when restarting.
7. ~~**`fallocate` on Linux**~~ – Performance polish for preallocation.
8. ~~**Progress UI / stats**~~ – Bytes done, ETA, rate during `ddm run`.
9. Only after the above: consider curl multi (threads are fine up to current segment counts).

---

## Code layout & modularity

- **Modular multi-folder, multi-file design** – Prefer small, focused modules. Use **subdirectories** (e.g. `url_model/`) when a feature spans multiple concerns or would make a single file long.
- **Avoid long files** – Aim for **< 200 lines per file** (excluding tests). If a module grows beyond that, split it into a folder with `mod.rs` and one or more submodules (e.g. `content_disposition.rs`, `sanitize.rs`).
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
- [x] **Segment integrity** – After `transfer.perform()`, verify `bytes_written.load() == segment.len()`; on mismatch return `SegmentError::PartialTransfer { expected, received }` for retry. Uses `Arc<AtomicU64>` so the write callback and post-perform check share the same counter.
- [x] **Abort on write failure** – In downloader `write_function`, return `Ok(0)` when `write_at` fails so libcurl aborts the transfer (segment fails and retries); no longer use `WriteError::Pause`. Retry classify: `PartialTransfer` and curl write errors map to `ErrorKind::Connection`.
- [x] **Durable progress commits** – Bitmap persisted to SQLite as segments complete. `ResumeDb::update_bitmap(id, bitmap)`; downloader accepts optional `progress_tx: Option<&tokio::sync::mpsc::Sender<Vec<u8>>>` and sends bitmap after each completed segment; scheduler runs a receiver task that calls `update_bitmap` so a crash mid-download doesn’t lose progress.
- [x] **Force-restart cleans temp file** – When `needs_metadata` (force-restart or remote changed), scheduler removes existing `.part` with `tokio::fs::remove_file` before creating storage; then create + preallocate so segment count/size changes don’t leave bad state.
- [x] **Preallocate with fallocate** – On Unix, `StorageWriterBuilder::preallocate` tries `posix_fallocate` first (real block allocation); on failure or non-Unix falls back to `set_len`. `libc` under `[target.'cfg(unix)'.dependencies]`.
- [x] **Job state recovery** – `ResumeDb::recover_running_jobs()` sets all `running` → `queued`; CLI `run` calls it before the scheduling loop so crashed jobs are not stranded. Unit test `recover_running_jobs_resets_to_queued`.
- [x] **Fix bytes_written shared counter (blocker)** – In `download_one_segment()` replaced `Cell<u64>` with `Arc<AtomicU64>`; write callback uses `fetch_add` for offset, post-perform uses `load()`. Segment integrity check now works correctly.
- [x] **Surface storage errors** – Write callback stashes IO error from `write_at()` in `Arc<Mutex<Option<io::Error>>>`; when `perform()` fails with curl write error, return `SegmentError::Storage(io_err)`. Classify `Storage` as `ErrorKind::Other` (no retry). Unit test `storage_classified_as_other`.
- [x] **Set job state to Error on failure** – In `run_one_job()`, after setting state to `Running`, the rest of the run is wrapped in an async block whose result is checked; on any error we call `db.set_state(job_id, JobState::Error).await` (best-effort) then propagate. Only `recover_running_jobs()` converts `running` → `queued` (crash recovery).
- [x] **Low-speed timeout for segments** – Per-segment transfer uses curl `low_speed_limit(1024)` and `low_speed_time(60s)`: abort only if throughput drops below 1 KiB/s for 60s. Hard wall-clock timeout relaxed to 3600s as a safety net so large segments on slow links are not killed by a rigid 300s limit.
- [x] **Progress output** – `scheduler::ProgressStats` (bytes_done, total_bytes, elapsed_secs, segments_done/count); `bytes_per_sec()`, `eta_secs()`, `fraction()`. Execute sends stats when bitmap updates; CLI `run` spawns receiver and prints throttled line: done/total MiB, %, MiB/s, ETA.
- [x] **Abort deadlock fix** – In `download_segments()` worker mode, on ErrorKind::Other we set abort and drain the work queue, then subtract drained count from expected results so `rx.recv()` doesn’t block indefinitely when max_concurrent < remaining segments.
- [x] **Enforce real Range behavior** – In `download_one_segment()` require HTTP 206 for range requests; return `SegmentError::InvalidRangeResponse(code)` on 200/other 2xx; parse and validate Content-Range when present. Prevents corruption when server ignores Range and returns full body.
- [x] **Implement import-har** – HAR module: parse HAR 1.2, follow redirects (301/302/307/308, redirectURL or Location), resolve final URL; extract Cookie only when `include_cookies`; `JobSettings.custom_headers`; scheduler uses job.settings.custom_headers for probe/download; CLI `ddm import-har <path> [--allow-cookies]` creates job from resolved URL.
- [x] **Implement bench** – `bench::run_bench(url, headers, cfg, max_bytes)`: HEAD, then for 4/8/16 segments download up to 20 MiB (or max_bytes), measure throughput and DownloadSummary; `recommend_segment_count()` picks best throughput (prefer no errors). CLI `ddm bench <url>` prints table (Segs, Bytes, Time, MiB/s, Throttle, Errors) and recommended segment count.
- [x] **Persist HostPolicy** – `PersistedHostPolicy` (JSON with string keys "scheme:host:port"); `HostKey::to_string_key`/`from_string_key`; `to_snapshot()`/`from_snapshot()` in state; `save_to_path`/`load_from_path` in persist; CLI `run` loads from default path (or new if missing) and saves after run.
- [x] **Global scheduling limits** – `GlobalConnectionBudget` in `scheduler/budget.rs`: `reserve(n)` / `release(n)`; CLI `run` creates budget from `max_total_connections` and passes to `run_next_job`; execute phase reserves slots before download and releases on drop so future parallel jobs share the budget.

---

## In progress

- (none)

---

## Not started (order = ROI sequence above; do correctness items first)

### Correctness & robustness (do first)

- [x] **Progress durability under errors** – In `download_segments()` process results as they arrive; mark bitmap and persist on each Ok; drain all results and record first error; return error after loop.
- [x] **Progress coalescing** – Coalesce progress updates every N segments so `try_send()` drops are intentional.
- [x] **Abort flag** – On non-retryable error (e.g. Storage), signal workers to stop pulling more work.
- [x] **Abort deadlock fix** – When aborting (ErrorKind::Other), drain the work queue and reduce expected result count so the main thread doesn’t block forever (segments still in queue never get a worker; we now subtract drained count from `to_receive`).

### Progress and tuning

- [x] **Progress output** – Bytes done, ETA, total rate (MiB/s) shown during `ddm run`; throttled to every 500ms. `ProgressStats` in scheduler; CLI prints progress line (done/total MiB, %, rate, ETA).

### Next (high ROI, in order)

- [x] **Enforce real Range behavior** – Require HTTP 206 for range requests; reject 200 (full body) via `SegmentError::InvalidRangeResponse`; optionally validate Content-Range header matches requested range. Classify as Other (no retry).
- [x] **Implement import-har** – `har::resolve_har(path, include_cookies)` parses HAR, follows 302/redirectURL to final URL; optional Cookie from request; CLI `import-har` adds job with resolved URL; `JobSettings.custom_headers` stores cookies only when `--allow-cookies`.
- [x] **Implement bench** – `ddm bench <url>`: run 4/8/16 segments with capped concurrency (20 MiB cap per run); report throughput (MiB/s), throttle/error events, recommended segment count.
- [x] **Persist HostPolicy** – Save/load to `~/.local/state/ddm/host_policy.json`; `HostPolicy::default_path()`, `save_to_path()`, `load_from_path()`; CLI `run` loads at start and saves at end so tuning survives.
- [x] **Global scheduling limits** – Global connection budget across jobs so multiple downloads don’t each use full per-host concurrency.

### Optional and polish

- [ ] **Checksum (`checksum`)** – Optional SHA-256 after completion; off the hot path.
- [ ] **Config extensions** – Retry policy, bandwidth cap, segment buffer size in `config.toml`.
- [ ] **HAR resolver (optional)** – Parse HAR → direct URL + minimal headers; `import-har` flow; cookie warning and `--allow-cookies`; keep core resolver-agnostic.
- [x] **Bench mode** – `ddm bench <url>`: try 4/8/16 segments, report throughput and recommended count.
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
