//! Linux-safe filename sanitization.

/// Sanitizes a candidate filename for safe use on Linux.
///
/// - Replaces NUL, `/`, `\`, and control characters with `_`
/// - Trims leading/trailing spaces and dots
/// - Collapses consecutive underscores
/// - Limits length to 255 bytes (Linux NAME_MAX)
pub fn sanitize_filename_for_linux(name: &str) -> String {
    const NAME_MAX: usize = 255;

    let mut out = String::with_capacity(name.len());
    let mut prev_underscore = false;

    for c in name.chars() {
        let replacement = if c == '\0' || c == '/' || c == '\\' || c.is_control() {
            '_'
        } else if c == ' ' || c == '\t' {
            '_'
        } else {
            c
        };

        if replacement == '_' {
            if !prev_underscore {
                out.push('_');
            }
            prev_underscore = true;
        } else {
            out.push(replacement);
            prev_underscore = false;
        }
    }

    let trimmed = out.trim_matches(|c| c == ' ' || c == '\t' || c == '.' || c == '_');

    if trimmed.len() > NAME_MAX {
        let mut take = NAME_MAX;
        while take > 0 && !trimmed.is_char_boundary(take) {
            take -= 1;
        }
        trimmed[..take].to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_slash_and_backslash() {
        assert_eq!(sanitize_filename_for_linux("a/b\\c.txt"), "a_b_c.txt");
    }

    #[test]
    fn trims_dots_and_spaces() {
        assert_eq!(
            sanitize_filename_for_linux("  ..  file.txt  ..  "),
            "file.txt"
        );
    }

    #[test]
    fn collapses_underscores() {
        assert_eq!(
            sanitize_filename_for_linux("file___name.txt"),
            "file_name.txt"
        );
    }

    #[test]
    fn control_chars() {
        assert_eq!(
            sanitize_filename_for_linux("file\x00name.txt"),
            "file_name.txt"
        );
    }
}
