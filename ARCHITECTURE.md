## Overview

DDM is a high-throughput, robust segmented download manager targeting Debian 12
(bookworm) on modern multi-core CPUs with fast NVMe storage. It is designed as
a CLI-first tool with a clean internal architecture and strong testability.

The implementation is organized as a small Rust workspace:

- **`ddm-core` crate**: Library crate that contains the engine and shared
  infrastructure:
  - **Config (`config`)**: Loads global tuning parameters from
    `~/.config/ddm/config.toml` (connection limits, segment bounds, retry
    policy, bandwidth caps).
  - **Logging (`logging`)**: Initializes structured logging to
    `~/.local/state/ddm/ddm.log` using the XDG base directory spec.
  - **URL model (`url_model`)**: Normalizes URLs, derives safe filenames (from
    `Content-Disposition` or URL path), and applies per-host policy hints.
  - **HEAD/metadata (`fetch_head`)**: Uses libcurl (multi) to probe URLs,
    verify `Content-Length` and `Accept-Ranges: bytes`, and capture
    ETag/Last-Modified.
  - **Segmenter (`segmenter`)**: Range math and segment planning, including
    bitmaps that track per-segment completion.
  - **Scheduler (`scheduler`)**: Coordinates jobs, manages per-host
    concurrency, retry/backoff, and the adaptive optimizer that adjusts
    segment counts based on throughput and error rates.
  - **Downloader (`downloader`)**: Core segmented engine that consumes direct
    URLs plus headers and drives multiple HTTP Range requests, writing to the
    appropriate file offsets.
  - **Storage (`storage`)**: Handles `fallocate`-based preallocation, buffered
    offset writes, fsync policy, and atomic finalize (download to `.part` then
    rename).
  - **Resume DB (`resume_db`)**: SQLite-based job and resume database (via
    `sqlx`), storing filenames, sizes, ETag/Last-Modified, segment bitmaps,
    and per-job settings.
  - **Checksum (`checksum`)**: Optional post-download verification (e.g.
    SHA-256) executed on demand to avoid impacting throughput.
  - **Resolver (`resolver`)**: Trait defining how higher-level inputs are
    turned into direct downloadable URLs plus headers. The core downloader
    only depends on this trait.
  - **Host policy (`host_policy`)**: Per-host cache keyed by
    `scheme+host+port` that tracks observed range support, throttling, and
    recommended segment limits.

- **`ddm-cli` crate**: Binary crate that provides the `ddm` command-line
  interface:
  - **CLI (`cli`)**: Parses commands (`add`, `run`, `status`, `pause`,
    `resume`, `remove`, `import-har`, `bench`) and maps them to high-level
    operations, delegating to `ddm-core`.
  - **`main`**: Sets up logging via `ddm-core::logging` and invokes the CLI
    dispatcher.

### Data flow

1. **CLI** parses the user command.
2. **Config** is loaded and passed down to the relevant components.
3. For `add` or `import-har`, a new job is created in the **resume DB** with
   an initial state and planned filename.
4. `run` starts the **scheduler**, which:
   - Consults **host policy** and **config** to determine per-host limits.
   - Uses **fetch_head** to validate metadata and discover size/range support.
   - Invokes the **segmenter** to plan segments and initialize the bitmap.
   - Delegates low-level I/O to **downloader** and **storage**.
5. The **downloader** drives segmented HTTP Range downloads using libcurl's
   multi interface, updating the bitmap in the **resume DB** as segments
   complete.
6. On completion, **storage** performs atomic finalize and **checksum**
   optionally verifies integrity.
7. The **CLI** subcommands `status`, `pause`, `resume`, and `remove` operate
   primarily by interacting with the **resume DB** and scheduler state.

### Resolver isolation

The core downloader and scheduler operate only on already-resolved direct
downloadable URLs plus request headers. Optional resolvers (like the HAR
resolver) live behind the `resolver` trait and are invoked explicitly by the
CLI (`import-har`). This keeps the core independent of any particular website
or authentication flow and makes it easier to audit and extend.

