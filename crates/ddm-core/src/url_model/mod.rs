//! URL modeling and filename derivation.
//!
//! Derives safe local filenames from URL path or Content-Disposition header,
//! sanitized for Linux filesystems.

mod content_disposition;
mod path;
mod sanitize;

pub use content_disposition::parse_content_disposition_filename;
pub use path::filename_from_url_path;
pub use sanitize::sanitize_filename_for_linux;

/// Default filename when URL path and Content-Disposition yield nothing usable.
const DEFAULT_FILENAME: &str = "download.bin";

/// Derives a safe filename for saving a download.
///
/// Prefers the filename from `content_disposition` (if present and parseable),
/// otherwise uses the last path segment of `url`. The result is sanitized for
/// Linux (no `/`, NUL, or control chars; no leading/trailing dots or spaces;
/// reserved names like "." or ".." replaced).
///
/// # Examples
///
/// - `derive_filename("https://example.com/archive.zip", None)` → `"archive.zip"`
/// - `derive_filename("https://example.com/", Some("attachment; filename=\"report.pdf\""))` → `"report.pdf"`
pub fn derive_filename(url: &str, content_disposition: Option<&str>) -> String {
    let candidate = content_disposition
        .and_then(parse_content_disposition_filename)
        .filter(|s| !s.is_empty())
        .or_else(|| filename_from_url_path(url));

    let raw = match candidate {
        Some(c) => c,
        None => return DEFAULT_FILENAME.to_string(),
    };

    let sanitized = sanitize_filename_for_linux(&raw);
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        DEFAULT_FILENAME.to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_filename_from_url_path() {
        assert_eq!(
            derive_filename("https://example.com/archive.zip", None),
            "archive.zip"
        );
        assert_eq!(
            derive_filename("https://cdn.example.com/path/to/debian-12.iso", None),
            "debian-12.iso"
        );
    }

    #[test]
    fn derive_filename_from_content_disposition() {
        assert_eq!(
            derive_filename(
                "https://example.com/",
                Some("attachment; filename=\"report.pdf\"")
            ),
            "report.pdf"
        );
        assert_eq!(
            derive_filename(
                "https://example.com/x",
                Some("attachment; filename=simple.bin")
            ),
            "simple.bin"
        );
    }

    #[test]
    fn derive_filename_content_disposition_overrides_url() {
        assert_eq!(
            derive_filename(
                "https://example.com/archive.zip",
                Some("attachment; filename=\"real-name.tar.gz\"")
            ),
            "real-name.tar.gz"
        );
    }

    #[test]
    fn derive_filename_empty_url_path_fallback() {
        assert_eq!(
            derive_filename("https://example.com/", None),
            "download.bin"
        );
        assert_eq!(
            derive_filename("https://example.com", None),
            "download.bin"
        );
    }

    #[test]
    fn derive_filename_reserved_names_fallback() {
        assert_eq!(derive_filename("https://example.com/.", None), "download.bin");
        assert_eq!(derive_filename("https://example.com/..", None), "download.bin");
    }
}
