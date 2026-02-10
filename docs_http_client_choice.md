## HTTP client choice: libcurl multi via `curl` crate

DDM uses libcurl via the Rust `curl` crate, with the multi interface for the
segmented downloader.

### Rationale

- **Maturity and robustness**: libcurl is a battle-tested HTTP client library
  used widely across Linux distributions (including Debian). It has excellent
  support for HTTP/HTTPS, redirects, proxies, and a wide range of edge cases.
- **HTTP correctness**: libcurl's implementation closely tracks the HTTP
  specifications and handles many subtle behaviors (redirect handling,
  connection reuse, TLS details, header quirks) that are easy to get wrong in
  custom async stacks.
- **Multi interface**: The multi interface is specifically designed for high
  concurrency and throughput. It allows DDM to manage many concurrent
  connections in a single event loop while reusing connections efficiently.
- **System integration on Debian**: Debian 12 already ships libcurl as a core
  dependency, so linking against the system library is straightforward and
  keeps the footprint small while inheriting security updates via the
  distribution.

### Alternatives considered

- **Async Rust HTTP clients (e.g. hyper/reqwest)**:
  - Pros: idiomatic Rust, strong ecosystem, native async support.
  - Cons: re-implementing some of libcurl's robustness and long-tail HTTP
    behaviors would require additional work; some advanced features and
    corner cases may lag libcurl's coverage.

Given the focus on maximum throughput, robustness, and HTTP correctness on
Debian 12, libcurl multi is a natural fit. Rust's type system and module
boundaries are used to wrap libcurl in a safe, testable abstraction, while
still exposing the performance and battle-tested behavior of the underlying
library.

