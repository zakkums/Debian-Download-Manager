//! Minimal HTTP/1.1 server that supports HEAD and Range GET for integration tests.
//!
//! Serves a single static body. Responds to HEAD with Content-Length and
//! Accept-Ranges: bytes; responds to GET with Range with 206 Partial Content.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

#[derive(Debug, Clone, Copy)]
pub struct RangeServerOptions {
    /// If false, HEAD returns 405 (simulates servers that block HEAD).
    pub head_allowed: bool,
    /// If false, GET ignores Range and always returns 200 with the full body.
    pub support_ranges: bool,
    /// If false, omit `Accept-Ranges: bytes` header even if ranges work.
    pub advertise_ranges: bool,
}

impl Default for RangeServerOptions {
    fn default() -> Self {
        Self {
            head_allowed: true,
            support_ranges: true,
            advertise_ranges: true,
        }
    }
}

/// Starts a server in a background thread serving `body`. Returns the base URL
/// (e.g. "http://127.0.0.1:12345/"). The server runs until the process exits.
pub fn start(body: Vec<u8>) -> String {
    start_with_options(body, RangeServerOptions::default())
}

/// Like `start` but allows customizing server behavior (HEAD blocked, ranges missing, etc.).
pub fn start_with_options(body: Vec<u8>, opts: RangeServerOptions) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let body = Arc::new(body);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let body = Arc::clone(&body);
            thread::spawn(move || handle(stream, &body, opts));
        }
    });
    format!("http://127.0.0.1:{}/", port)
}

fn handle(mut stream: std::net::TcpStream, body: &[u8], opts: RangeServerOptions) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(0) => return,
        Ok(n) => n,
        Err(_) => return,
    };
    let request = match std::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => return,
    };
    let (method, range) = parse_request(request);
    let total = body.len() as u64;
    if method.eq_ignore_ascii_case("HEAD") {
        if !opts.head_allowed {
            let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n");
            return;
        }
        let accept_ranges = if opts.advertise_ranges && opts.support_ranges {
            "Accept-Ranges: bytes\r\n"
        } else {
            ""
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}\
\r\n",
            total, accept_ranges
        );
        let _ = stream.write_all(response.as_bytes());
        return;
    }
    if method.eq_ignore_ascii_case("GET") {
        let use_range = opts.support_ranges;
        let (status, range_header, slice) = if use_range {
            if let Some((start, end_incl)) = range {
            let start = start.min(total);
            let end_incl = end_incl.min(total.saturating_sub(1));
            if start > end_incl {
                (
                    "416 Range Not Satisfiable",
                    format!("bytes */{}", total),
                    &body[0..0],
                )
            } else {
                let start = start as usize;
                let end_excl = (end_incl + 1).min(total) as usize;
                let slice = body.get(start..end_excl).unwrap_or(&body[0..0]);
                (
                    "206 Partial Content",
                    format!("bytes {}-{}/{}", start, end_excl.saturating_sub(1), total),
                    slice,
                )
            }
            } else {
            (
                "200 OK",
                format!("bytes 0-{}/{}", total.saturating_sub(1), total),
                body,
            )
            }
        } else {
            (
                "200 OK",
                format!("bytes 0-{}/{}", total.saturating_sub(1), total),
                body,
            )
        };
        let accept_ranges = if opts.advertise_ranges && opts.support_ranges {
            "Accept-Ranges: bytes\r\n"
        } else {
            ""
        };
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Range: {}\r\n{}\
\r\n",
            status, slice.len(), range_header, accept_ranges
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(slice);
        return;
    }
    let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n");
}

/// Returns (method, optional (start, end_inclusive) for Range: bytes=X-Y).
fn parse_request(request: &str) -> (&str, Option<(u64, u64)>) {
    let mut method = "";
    let mut range = None;
    for line in request.lines() {
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if method.is_empty() {
            method = line.split_whitespace().next().unwrap_or("");
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("range") {
                let value = value.trim();
                if value.to_lowercase().starts_with("bytes=") {
                    let part = value[6..].trim();
                    if let Some((a, b)) = part.split_once('-') {
                        let start = a.trim().parse::<u64>().unwrap_or(0);
                        let end = b.trim();
                        let end_incl = if end.is_empty() {
                            u64::MAX
                        } else {
                            end.parse::<u64>().unwrap_or(0)
                        };
                        range = Some((start, end_incl));
                    }
                }
            }
        }
    }
    (method, range)
}
