# Full `PROGRESS.md` snapshot (2026-02-11)

This is a verbatim snapshot of the repository's `PROGRESS.md` as of 2026-02-11.
It exists so `PROGRESS.md` can stay under the project guideline of **< 200 lines**
without losing any historical detail.

sha256: `1f0e7f56c04f390d77656a14a7b3dab27f04e7599fc6c81e778a39079dd5ce7d`

---

# DDM implementation progress

Use this file to see what's done and what's left. When starting a new chat, share this file so the new context knows the current state.

---

## Status summary

- **Done:** Core engine, resume DB, scheduler, downloader (Easy + threads + curl multi backend), safe resume, retry/backoff, progress durability, abort deadlock fix, Range pre-write validation, redirect-safe header capture, saturating budget release, import-har, bench, HostPolicy persistence, global scheduling, docs, integration test, curl multi phase 2 (config `download_backend`, Easy2 + Multi loop).
- **In progress:** (none)
- **Next (ROI order):** See **Roadmap** below — Tier 0 (CLI honesty, overwrites/collisions, download_dir) first.

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
- [x] **Redirect-safe header capture** – With `follow_location(true)`, curl sends headers for each response (e.g. 302 then 206). We clear the header vector when a line starts with `HTTP/` so only the final response's headers are kept; `parse_http_status` / `parse_content_range` then see 206 and correct Content-Range, avoiding false InvalidRangeResponse on CDN/file-site redirects.
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

## Gaps / issues (will bite you next)

### A) CLI semantics mismatch (user-facing correctness)

1. **ddm remove does not remove "data"**  
   CLI help says: "Remove a job (and optionally its data)", but `crates/ddm-cli/src/cli/commands/remove.rs` only calls `db.remove_job(id)`.

2. **ddm pause is not a real pause**  
   Pause only sets DB state to paused and does not signal an in-flight download to stop. In the current design (single foreground `ddm run` process), there's no IPC/control channel, so this is expected—but the CLI implies stronger behavior.

**Fix direction:** either (a) adjust CLI text to be honest ("affects next run"), or (b) introduce a long-running daemon + IPC so pause/resume/remove can affect running work.

### B) Filename/path collisions and overwrite risk

- `url_model::derive_filename()` produces a single filename.
- Scheduler stores filename strings, not absolute paths.
- Storage finalize uses `std::fs::rename(temp, final)` which replaces existing files on Unix.

**Implications:** Two jobs with the same derived name will collide. Running `ddm run` from different directories can cause "resume" to silently start fresh because the .part file isn't in that directory. Existing final files can be overwritten.

**Fix direction:** Store a per-job `download_dir` and resolved full paths in job settings, and implement a collision strategy (suffixing, job-id prefixing, or "fail if exists unless --overwrite").

### C) Hard failure on non-Range servers

In `scheduler/run/single.rs` (and shared run path): `if !head.accept_ranges → bail "server does not support Range requests"`. DDM cannot download from servers without Accept-Ranges, without Content-Length (chunked/dynamic), or when Range is supported but HEAD is blocked.

**Fix direction:** Fallback path: if no ranges → single-stream GET (no Range header), no segmented resume (or resume via temp file size if safe). If no Content-Length → stream to disk without preallocation, treat as non-resumable unless server supports range and size can be inferred later.

### D) Panics in the threaded downloader control plane

In `crates/ddm-core/src/downloader/run.rs`: `rx.recv().expect("worker result")`, `h.join().unwrap_or_else(|e| panic!(...))`. If any worker panics or the channel breaks, the whole program panics.

**Fix direction:** Convert to Result propagation: treat worker panics as SegmentError::Internal (or similar); join failures → anyhow::Error with context.

### E) Docs drift (small, but important)

`docs_http_client_choice.md` claims "no curl::multi usage", but the codebase implements a multi backend. Update the doc (or split into "default backend" vs "optional backend").

### F) DB schema/migrations are "v0.1 OK", but not future-proof

`resume_db/db.rs` creates the table via `CREATE TABLE IF NOT EXISTS ...` with no schema versioning. Fine early; once you add download directory, per-job override filename, per-segment metrics, priority/queue ordering, you'll want explicit migrations.

**Fix direction:** Add a lightweight schema version table or switch to sqlx migrations.

### G) Privacy/security: HAR cookie persistence is sensitive

Cookies are gated behind `--allow-cookies`, but when stored in SQLite unencrypted this becomes a credential store.

**Fix direction options:** (1) Safest: don't persist cookies—use only for current run unless user explicitly exports a "profile" file. (2) Pragmatic: persist, but isolate DB permissions (0600), avoid logging config/settings, add prominent warnings. (3) Advanced: encrypt at rest (key management complexity).

---

## Roadmap (prioritized — what's next)

### Tier 0 — Fix "user trust" issues (highest ROI)

- **Make CLI text match reality**
  - Update **remove** help or implement `--delete-files`.
  - Update **pause** help to clarify it only affects scheduling unless you build a daemon.
- **Prevent overwrites + collisions**
  - Add collision strategy: `{name} (1).ext` or `{job_id}-{name}`.
  - Add `--overwrite` explicit flag.
- **Store download directory per job**
  - Add `download_dir` to JobSettings (settings_json) and use it consistently. Fixes "resume from a different directory breaks".

**Acceptance criteria:** Two identical URLs added twice don't clobber each other. Resuming works even if you run `ddm run` from another directory (job knows where its files live).

### Tier 1 — Make it "work everywhere"

- **Non-range fallback**  
  If Accept-Ranges missing: single GET download path. If Content-Length missing: stream to disk without prealloc; disable segmented resume.
- **HEAD blocked fallback**  
  If HEAD fails but GET works: probe via GET headers (or a small ranged GET if supported).

**Acceptance criteria:** You can download from a basic server that doesn't advertise ranges. DDM handles "HEAD not allowed" gracefully.

### Tier 2 — Use the config fields you already exposed

- **Enforce max_bytes_per_sec** — e.g. libcurl receive speed limiting per easy handle; global or per-job (global simplest).
- **Use segment_buffer_bytes** — Set curl buffer size per easy handle where supported.

**Acceptance criteria:** Config changes demonstrably affect throughput and/or memory.

### Tier 3 — "Real download manager" control plane

- **True pause/resume/cancel**  
  Either: (1) daemon (`ddm daemon`) + local IPC socket (Unix domain), or (2) single-process "interactive" mode that listens for commands while downloading. Minimum viable: `ddm run` starts a control socket; `ddm pause <id>` sends message to stop scheduling new segments + triggers abort flag.

**Acceptance criteria:** Pause stops network activity for a running job within ~1s. Resume continues without losing completed segments.

### Tier 4 — Packaging and usability

- **Project basics**
  - README.md with install/run examples + config explanation.
  - LICENSE file.
  - (continued in `docs/progress/PROGRESS_FULL_2026-02-11.part2.md`)

