//! Content-Disposition header parsing (filename and filename*).

/// Extracts the filename from a raw Content-Disposition header value.
///
/// Supports:
/// - `filename="value"` (quoted; strips quotes and unescapes)
/// - `filename=value` (token)
/// - `filename*=UTF-8''percent-encoded` (RFC 5987; decoded)
/// If both `filename` and `filename*` exist, `filename*` takes precedence.
pub fn parse_content_disposition_filename(header_value: &str) -> Option<String> {
    let value = header_value.trim();
    let mut filename_from_token: Option<String> = None;

    for param in value.split(';') {
        let param = param.trim();
        if let Some((name, v)) = param.split_once('=') {
            let name = ascii_lowercase(name.trim());
            let v = v.trim();

            if name == "filename*" {
                if let Some(rest) = v.strip_prefix("utf-8''").or_else(|| v.strip_prefix("UTF-8''")) {
                    if let Ok(decoded) = percent_decode(rest) {
                        let decoded = decode_quoted_filename(&decoded);
                        if !decoded.is_empty() {
                            return Some(decoded);
                        }
                    }
                }
            }

            if name == "filename" {
                let unquoted = if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    decode_quoted_filename(&v[1..v.len() - 1])
                } else {
                    v.to_string()
                };
                if !unquoted.is_empty() {
                    filename_from_token = Some(unquoted);
                }
            }
        }
    }

    filename_from_token
}

/// Decode backslash-escaped quotes in a quoted filename value.
pub(super) fn decode_quoted_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == '"' || next == '\\' {
                    out.push(chars.next().unwrap());
                    continue;
                }
            }
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Simple percent-decode for filename* value (RFC 5987).
pub(super) fn percent_decode(input: &str) -> Result<String, std::str::Utf8Error> {
    let mut out = Vec::new();
    let mut bytes = input.as_bytes().iter().cloned();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let h = bytes.next().and_then(hex_digit);
            let l = bytes.next().and_then(hex_digit);
            match (h, l) {
                (Some(high), Some(low)) => out.push(high << 4 | low),
                _ => {
                    out.push(b'%');
                    if let Some(x) = h {
                        out.push(x);
                    }
                    if let Some(x) = l {
                        out.push(x);
                    }
                }
            }
        } else {
            out.push(b);
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn ascii_lowercase(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('A'..='Z').contains(&c) {
                ((c as u8) - b'A' + b'a') as char
            } else {
                c
            }
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quoted() {
        let r = parse_content_disposition_filename("attachment; filename=\"report.pdf\"");
        assert_eq!(r.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn parse_token() {
        let r = parse_content_disposition_filename("attachment; filename=report.pdf");
        assert_eq!(r.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn parse_filename_star_utf8() {
        let r = parse_content_disposition_filename("attachment; filename*=UTF-8''caf%C3%A9.txt");
        assert_eq!(r.as_deref(), Some("café.txt"));
    }

    #[test]
    fn parse_filename_star_precedence() {
        let r = parse_content_disposition_filename(
            "attachment; filename=\"fallback.bin\"; filename*=UTF-8''real%20name.dat",
        );
        assert_eq!(r.as_deref(), Some("real name.dat"));
    }
}
