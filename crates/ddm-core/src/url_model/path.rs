//! Filename extraction from URL path.

/// Extracts the last path segment from a URL for use as a filename hint.
///
/// Returns `None` if the URL cannot be parsed or the path is empty/root.
pub fn filename_from_url_path(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let path = parsed.path();
    let segment = path.split('/').filter(|s| !s.is_empty()).last()?;
    if segment.is_empty() || segment == "." || segment == ".." {
        return None;
    }
    Some(segment.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal() {
        assert_eq!(
            filename_from_url_path("https://example.com/a/b/file.deb").as_deref(),
            Some("file.deb")
        );
        assert_eq!(
            filename_from_url_path("https://example.com/single").as_deref(),
            Some("single")
        );
    }

    #[test]
    fn root_or_empty() {
        assert_eq!(filename_from_url_path("https://example.com/"), None);
        assert_eq!(filename_from_url_path("https://example.com"), None);
    }

    #[test]
    fn with_query() {
        assert_eq!(
            filename_from_url_path("https://example.com/file.zip?token=abc").as_deref(),
            Some("file.zip")
        );
    }
}
