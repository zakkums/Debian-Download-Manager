## HTTP client choice: libcurl Easy handles + worker threads

DDM uses libcurl via the Rust `curl` crate. The segmented downloader uses
**one Easy handle per segment** in a bounded pool of **OS threads** (no
`curl::multi` usage). HEAD probes use a single Easy handle.

### Rationale

- **Maturity and robustness**: libcurl is a battle-tested HTTP client library
  used widely across Linux distributions (including Debian). It has excellent
  support for HTTP/HTTPS, redirects, proxies, and a wide range of edge cases.
- **HTTP correctness**: libcurl's implementation closely tracks the HTTP
  specifications and handles many subtle behaviors (redirect handling,
  connection reuse, TLS details, header quirks) that are easy to get wrong in
  custom async stacks.
- **Easy + threads**: Per-segment Easy handles in OS threads give clear
  isolation per transfer (timeouts, write callbacks, header validation) and
  bounded concurrency via a worker pool. This is sufficient for current
  segment counts (e.g. 4–16 per host).
- **System integration on Debian**: Debian 12 already ships libcurl as a core
  dependency, so linking against the system library is straightforward and
  keeps the footprint small while inheriting security updates via the
  distribution.

### Alternatives considered

- **libcurl multi interface**: Would allow many concurrent transfers in a
  single thread/event loop and more efficient connection reuse. Could be
  adopted later for efficiency; the current Easy + threads design is correct
  and adequate for typical segment counts.
- **Async Rust HTTP clients (e.g. hyper/reqwest)**:
  - Pros: idiomatic Rust, strong ecosystem, native async support.
  - Cons: re-implementing some of libcurl's robustness and long-tail HTTP
    behaviors would require additional work; some advanced features and
    corner cases may lag libcurl's coverage.

Given the focus on robustness and HTTP correctness on Debian 12, libcurl is a
natural fit. The implementation uses Easy handles in OS threads for the
downloader; Rust's type system and module boundaries wrap libcurl in a safe,
testable abstraction while exposing the performance and battle-tested behavior
of the underlying library.
