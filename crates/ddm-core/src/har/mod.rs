//! HAR (HTTP Archive) resolver: parse HAR files and resolve to direct download URL.
//!
//! Detects the 302 → direct file URL pattern: follows redirects from entries and
//! returns the final URL. Optionally extracts Cookie from the request (only when
//! the caller requests it, e.g. --allow-cookies).

mod parse;
mod resolve;

pub use resolve::resolve_har;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn resolve_har_follows_302() {
        let har = r#"{
            "log": {
                "version": "1.2",
                "entries": [
                    {
                        "request": { "url": "https://example.com/redirect", "headers": [] },
                        "response": { "status": 302, "redirectURL": "https://cdn.example.com/file.zip", "headers": [] }
                    },
                    {
                        "request": { "url": "https://cdn.example.com/file.zip", "headers": [] },
                        "response": { "status": 200, "headers": [] }
                    }
                ]
            }
        }"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(har.as_bytes()).unwrap();
        f.flush().unwrap();
        let spec = resolve_har(f.path(), false).unwrap();
        assert_eq!(spec.url, "https://cdn.example.com/file.zip");
        assert!(spec.headers.is_empty());
    }

    #[test]
    fn resolve_har_no_redirect_uses_first_url() {
        let har = r#"{
            "log": {
                "version": "1.2",
                "entries": [
                    {
                        "request": { "url": "https://direct.example.com/f.bin", "headers": [] },
                        "response": { "status": 200, "headers": [] }
                    }
                ]
            }
        }"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(har.as_bytes()).unwrap();
        f.flush().unwrap();
        let spec = resolve_har(f.path(), false).unwrap();
        assert_eq!(spec.url, "https://direct.example.com/f.bin");
    }

    #[test]
    fn resolve_har_include_cookies() {
        let har = r#"{
            "log": {
                "version": "1.2",
                "entries": [
                    {
                        "request": {
                            "url": "https://cdn.example.com/file.zip",
                            "headers": [ { "name": "Cookie", "value": "session=abc123" } ]
                        },
                        "response": { "status": 200, "headers": [] }
                    }
                ]
            }
        }"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(har.as_bytes()).unwrap();
        f.flush().unwrap();
        let spec = resolve_har(f.path(), true).unwrap();
        assert_eq!(spec.url, "https://cdn.example.com/file.zip");
        assert_eq!(
            spec.headers.get("Cookie").map(|s| s.as_str()),
            Some("session=abc123")
        );
    }

    #[test]
    fn resolve_har_empty_entries_err() {
        let har = r#"{"log":{"version":"1.2","entries":[]}}"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(har.as_bytes()).unwrap();
        f.flush().unwrap();
        assert!(resolve_har(f.path(), false).is_err());
    }

    #[test]
    fn resolve_har_prefers_download_like_entry() {
        let har = r#"{
            "log": {
                "version": "1.2",
                "entries": [
                    {
                        "request": { "url": "https://example.com/start", "headers": [] },
                        "response": { "status": 302, "redirectURL": "https://example.com/login", "headers": [] }
                    },
                    {
                        "request": { "url": "https://example.com/login", "headers": [] },
                        "response": { "status": 200, "headers": [] }
                    },
                    {
                        "request": { "url": "https://cdn.example.com/file.zip", "headers": [] },
                        "response": {
                            "status": 206,
                            "headers": [
                                { "name": "Content-Length", "value": "1024" },
                                { "name": "Accept-Ranges", "value": "bytes" }
                            ]
                        }
                    }
                ]
            }
        }"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(har.as_bytes()).unwrap();
        f.flush().unwrap();
        let spec = resolve_har(f.path(), false).unwrap();
        assert_eq!(spec.url, "https://cdn.example.com/file.zip");
    }
}
