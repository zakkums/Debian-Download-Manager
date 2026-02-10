# DDM implementation progress

Use this file to see what’s done and what’s left. When starting a new chat, share this file so the new context knows the current state.

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

---

## In progress

- (none)

---

## Not started (order is a suggested sequence)

### Robustness and tuning

- [x] **Host policy (`host_policy`)** – Cache keyed by scheme+host+port: range support, throttling history, recommended max segments.
- [x] **Adaptive optimizer** – Start at 4 segments; increase to 8/16 if throughput improves; reduce on throttling or high error rate.
- [ ] **Progress output** – Bytes done, ETA, per-connection rate, total rate (no GUI; CLI-friendly).

### Optional and polish

- [ ] **Checksum (`checksum`)** – Optional SHA-256 after completion; off the hot path.
- [ ] **Config extensions** – Retry policy, bandwidth cap, segment buffer size in `config.toml`.
- [ ] **HAR resolver (optional)** – Parse HAR → direct URL + minimal headers; `import-har` flow; cookie warning and `--allow-cookies`; keep core resolver-agnostic.
- [ ] **Bench mode** – `ddm bench <url>`: try different segment counts, report throughput.

### Integration and quality

- [ ] **Integration test** – Local HTTP server with Range support; multi-segment download + resume.
- [ ] **Tests for new code** – Unit tests for new modules; update PROGRESS when adding tests. (fetch_head: header parsing; url_model: derivation, CD parsing, sanitize; segmenter: range math, bitmap; storage: create/preallocate/write/finalize; downloader: bitmap filtering; host_policy: host key parsing, range support, throttling heuristics; adaptive: step up/down on throughput and throttle.)

---

## Quick reference

- **Run tests:** `cargo test`
- **Run CLI:** `cargo run -p ddm-cli -- <subcommand> ...`
- **Config:** `~/.config/ddm/config.toml`
- **State / DB / logs:** `~/.local/state/ddm/` (jobs.db, ddm.log)
