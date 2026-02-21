## HTTP client choice: libcurl Easy handles and optional Multi

DDM uses libcurl via the Rust `curl` crate. The segmented downloader supports
**two backends** (configurable via `download_backend` in config):

- **easy** (default): One Easy handle per segment in a bounded pool of **OS threads**.
- **multi**: A single `curl::multi` handle with multiple Easy2 handles in one thread;
  better connection reuse and one event loop for many segments.

HEAD probes use a single Easy handle. Both backends support Range requests,
retry with backoff, and durable progress.

### Rationale

- **Maturity and robustness**: libcurl is a battle-tested HTTP client library
  used widely across Linux distributions (including Debian). It has excellent
  support for HTTP/HTTPS, redirects, proxies, and a wide range of edge cases.
- **HTTP correctness**: libcurl's implementation closely tracks the HTTP
  specifications and handles many subtle behaviors (redirect handling,
  connection reuse, TLS details, header quirks) that are easy to get wrong in
  custom async stacks.
- **Easy + threads (default)**: Per-segment Easy handles in OS threads give
  clear isolation per transfer and bounded concurrency. Adequate for typical
  segment counts (e.g. 4–16 per host).
- **Multi (optional)**: The `multi` backend uses one thread and one multi handle
  for all segments of a job, with efficient connection reuse. Choose via
  `download_backend = "multi"` in config.
- **System integration on Debian**: Debian 12 ships libcurl as a core dependency;
  linking keeps the footprint small and inherits security updates.
- **Async Rust HTTP clients (e.g. hyper/reqwest)**:
  - Pros: idiomatic Rust, strong ecosystem, native async support.
  - Cons: re-implementing libcurl's robustness and long-tail HTTP behaviors
    would require additional work; some corner cases may lag libcurl's coverage.

Given the focus on robustness and HTTP correctness on Debian 12, libcurl is a
natural fit. The implementation supports both Easy+threads and curl::multi
backends; Rust's type system and module boundaries wrap libcurl in a safe,
testable abstraction while exposing the performance and battle-tested behavior
of the underlying library.
