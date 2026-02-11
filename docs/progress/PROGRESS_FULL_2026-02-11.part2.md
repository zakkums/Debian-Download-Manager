## Continuation of `docs/progress/PROGRESS_FULL_2026-02-11.md` (part 2)

This file contains lines 181+ of the original snapshot, split to keep each file under 200 lines.

---

  - rust-toolchain.toml (pin toolchain; edition 2024 is fine via rustup but not Debian's old rustc).
- **Completions + manpage**  
  Clap can generate shell completions; add manpage generation.

---

## In progress

- (none)

---

## Not started (next in ROI order)

- Tier 2: Enforce `max_bytes_per_sec`, use `segment_buffer_bytes`.
- Then Tier 3 → Tier 4 per roadmap above.

---

## Done (this session)

- [x] **Tier 0: download_dir per job** – `JobSettings.download_dir` (stored in settings_json); CLI `add` accepts `--download-dir` (default: current dir); run path uses job's download_dir when set so resume works from any working directory.
- [x] **Tier 0: Collision strategy** – `url_model::unique_filename_among()`; `ResumeDb::list_final_filenames_in_dir()`; run path resolves unique final name when needs_metadata so two identical URLs get e.g. `file.iso` and `file (1).iso`.
- [x] **Tier 0: --overwrite** – CLI `run --overwrite`; run fails if final file exists unless `--overwrite`.
- [x] **Tier 0: CLI text match reality** – Remove: help updated; `--delete-files` and `--download-dir` implemented. Pause: help updated to state it only affects scheduling.
- [x] **Tier 1: HEAD-blocked + non-range fallback** – `fetch_head::probe_best_effort` (HEAD + Range-GET probe); single-stream GET download fallback (`downloader::download_single`, `execute_single_download_phase`, `run::fallback`); integration tests cover HEAD blocked and Range unsupported.
- [x] **Curl multi – phase 2** – Implemented curl::multi handle; single-threaded event loop, Easy2 + Handler per segment; config `download_backend` (easy | multi); parity with Easy+threads (206/Content-Range, progress, bitmap). Per-segment retry in multi added later.
- [x] **Execute module &lt;200 lines** – Split `scheduler/execute/mod.rs` (was 201 lines) into `execute/run_download.rs`; all source files now &lt;200 lines per code layout guideline.

---

## Done (tests for new code)

- [x] **Tests for multi backend** – Unit tests for multi handler (header clear on HTTP/, write rejects non-206, write accepts 206 and writes at offset); integration test `multi_backend_download_completes_and_file_matches` runs full download with `download_backend = "multi"`.

---

## Done (retry in multi backend)

- [x] **Retry in multi backend** – Per-segment retry with backoff inside multi loop; optional `RetryPolicy` passed from `download_segments_multi`; retry_after queue with `Instant`; refill from pending and retry_after; `next_retry_wait_ms` for wait timing; `refill.rs` and `result.rs` keep `run.rs` &lt; 200 lines.

---

## Reference (historical / narrative)

### Blocker bug (fixed)

Segment integrity was broken by `Cell<u64>` (clone gave a separate cell). Fixed with `Arc<AtomicU64>`; write callback and post-perform share the same counter.

### Abort deadlock (fixed)

When ErrorKind::Other, workers stopped pulling; main thread waited for count results but fewer were sent. Fix: drain queue and subtract drained from to_receive in downloader/run.rs.

### Range pre-write validation (done)

Server could return 200 + full body for a Range request; we validated after perform() so bytes were already written. Fix: in first write_function, parse status and Content-Range from headers; if not 206 or mismatch, return 0 to abort before writing any byte.

### Docs vs code (done)

Docs said "libcurl multi"; code uses Easy + threads. ARCHITECTURE and docs_http_client_choice updated to match.

### Redirect header false-fail (fixed)

With `follow_location(true)`, curl's header callback receives headers for every response (e.g. 302 redirect then 206). We were storing all lines and `parse_http_status(headers.first())` saw the first status (302), so the pre-write check aborted with InvalidRangeResponse even when the final response was 206. Fix: in the header callback, when a line starts with `HTTP/`, clear the header vector then push that line so "current headers" always correspond to the final response.

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

