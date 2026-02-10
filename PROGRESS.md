# DDM implementation progress

Use this file to see what’s done and what’s left. When starting a new chat, share this file so the new context knows the current state.

---

## Done

- [x] **Workspace layout** – `ddm-core` (lib) + `ddm-cli` (binary), modular `src/` layout.
- [x] **CLI scaffold** – `clap` subcommands: `add`, `run`, `status`, `pause`, `resume`, `remove`, `import-har`, `bench`.
- [x] **Config** – `~/.config/ddm/config.toml`, `DdmConfig` (connection/segment bounds), load-or-init.
- [x] **Logging** – Structured logging to `~/.local/state/ddm/ddm.log` (XDG state).
- [x] **Resume DB (SQLite)** – `ResumeDb` in `resume_db`, jobs table (url, filenames, size, etag, last_modified, segment_count, completed_bitmap, state, settings_json). `add_job`, `list_jobs`, `set_state`, `remove_job`. In-memory tests.
- [x] **CLI wired to DB** – `add`/`status`/`pause`/`resume`/`remove` use `ResumeDb`; async main with tokio.
- [x] **Tests** – Unit tests for config, resolver, resume_db (in-memory); CLI parse tests. `cargo test` passes.
- [x] **Docs** – `ARCHITECTURE.md`, `docs_http_client_choice.md`, Testing section.
- [x] **HEAD / metadata probe (`fetch_head`)** – `probe(url, custom_headers)` via curl: HEAD request, parse `Content-Length`, `Accept-Ranges: bytes`, `ETag`, `Last-Modified`, `Content-Disposition`. Unit tests for header parsing.

---

## In progress

- [ ] **URL model (`url_model`)** – Next: derive safe filename from URL path or Content-Disposition; sanitize for Linux.

---

## Not started (order is a suggested sequence)

### Core download pipeline

- [ ] **Segmenter (`segmenter`)** – Range math (split total size into N segments); HTTP Range header bounds; segment completion bitmap (serialize/deserialize for DB).
- [ ] **Storage (`storage`)** – Preallocate with `fallocate`; buffered offset writes (e.g. 1–8 MiB segment buffer, pwrite at offset); fsync policy (periodic or at completion); atomic finalize (download to `.part` then rename).
- [ ] **Downloader (`downloader`)** – Segmented engine: N concurrent HTTP Range requests (libcurl multi or equivalent), write each segment to correct offset, update bitmap on completion. Input: direct URL + optional headers only.
- [ ] **Resume DB extensions** – Store/update `final_filename`, `temp_filename`, `total_size`, `etag`, `last_modified`, `segment_count`, `completed_bitmap`; possibly `get_job(id)` for scheduler.
- [ ] **Safe resume** – On start: re-validate ETag/Last-Modified and size; if changed, require explicit user override; else download only missing segments per bitmap.
- [ ] **Scheduler (`scheduler`)** – Coordinate jobs; call fetch_head → segmenter → downloader → storage; respect per-host and global connection limits; trigger resume logic.

### Robustness and tuning

- [ ] **Retry / backoff** – Error classification (timeouts, 429/503, connection resets); exponential backoff; per-host concurrency reduction on repeated throttling.
- [ ] **Host policy (`host_policy`)** – Cache keyed by scheme+host+port: range support, throttling history, recommended max segments.
- [ ] **Adaptive optimizer** – Start at 4 segments; increase to 8/16 if throughput improves; reduce on throttling or high error rate.
- [ ] **Progress output** – Bytes done, ETA, per-connection rate, total rate (no GUI; CLI-friendly).

### Optional and polish

- [ ] **Checksum (`checksum`)** – Optional SHA-256 after completion; off the hot path.
- [ ] **Config extensions** – Retry policy, bandwidth cap, segment buffer size in `config.toml`.
- [ ] **HAR resolver (optional)** – Parse HAR → direct URL + minimal headers; `import-har` flow; cookie warning and `--allow-cookies`; keep core resolver-agnostic.
- [ ] **Bench mode** – `ddm bench <url>`: try different segment counts, report throughput.

### Integration and quality

- [ ] **Integration test** – Local HTTP server with Range support; multi-segment download + resume.
- [ ] **Tests for new code** – Unit tests for segmenter (range math, bitmap), url_model (sanitize); update PROGRESS when adding tests. (fetch_head has unit tests for header parsing.)

---

## Quick reference

- **Run tests:** `cargo test`
- **Run CLI:** `cargo run -p ddm-cli -- <subcommand> ...`
- **Config:** `~/.config/ddm/config.toml`
- **State / DB / logs:** `~/.local/state/ddm/` (jobs.db, ddm.log)
