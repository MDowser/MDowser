// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::content_type`: strict `Content-Type` validation.
//!
//! REQUIREMENT:
//! - We request and expect a markdown type (`text/markdown` plus the
//!   `text/x-markdown`, `application/markdown` and `application/x-markdown`
//!   aliases real servers use), with or without `; charset=utf-8`.
//! - Case-insensitive comparison, tolerant of extra whitespace.
//! - `text/plain`, `text/html`, octet-stream and anything else are rejected:
//!   only a type that IDENTIFIES markdown is accepted.
//! - Any other type is rejected and the next route is tried.
//!
//! Accepted examples:
//! - `text/markdown`
//! - `text/markdown; charset=utf-8`
//! - `Text/Markdown ; Charset=UTF-8`
//! - `text/markdown;charset="utf-8"`
//! - `text/x-markdown`, `application/markdown`, `application/x-markdown`
//!
//! Rejected examples:
//! - `text/html`, `text/plain`, `application/octet-stream`, missing, etc.
//! - `text/markdown; charset=iso-8859-1` (non-UTF-8 charset).

/// Splits a header on `;`, keeping double-quoted values whole
/// (`boundary="a;b"` stays one parameter).
///
/// Lazy and borrowing: a hostile header with thousands of `;` parameters
/// never materializes a `Vec` (the header value reaches this gate
/// untruncated, bounded only by the transport's own header limit), and
/// callers that need just the MIME type stop at the first item.
///
/// Pure ASCII scan on byte indices: `"`, `\` and `;` are single-byte, so
/// every slice is on a char boundary (never panics on UTF-8 values).
struct SplitParams<'a> {
    raw: &'a str,
    start: usize,
    done: bool,
}

impl<'a> Iterator for SplitParams<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.done {
            return None;
        }
        // The state at a segment start is always "not in quotes": a split
        // only happens on a `;` outside quotes.
        let mut in_quotes = false;
        let mut escaped = false;
        let bytes = self.raw.as_bytes();
        let mut i = self.start;
        while i < bytes.len() {
            let b = bytes[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            match b {
                b'\\' if in_quotes => escaped = true,
                b'"' => in_quotes = !in_quotes,
                b';' if !in_quotes => {
                    let part = &self.raw[self.start..i];
                    self.start = i + 1;
                    return Some(part);
                }
                _ => {}
            }
            i += 1;
        }
        self.done = true;
        Some(&self.raw[self.start..])
    }
}

fn split_params(raw: &str) -> SplitParams<'_> {
    SplitParams {
        raw,
        start: 0,
        done: false,
    }
}

/// Is this `Content-Type` value a valid markdown type?
///
/// - `None` (missing header) => `false`. We require an explicit declaration.
/// - Ignores case and whitespace around `;`, `=` and values.
/// - Accepts the markdown MIME types listed in
///   [`crate::config::MARKDOWN_CONTENT_TYPES`] (the standard one plus the
///   common `x-`/`application` aliases); `text/plain` is NOT one of them.
/// - Accepts single/double quotes around `utf-8`.
/// - A present non-UTF-8 `charset` parameter => `false`.
/// - Other unknown parameters are ignored (tolerance), including quoted
///   values containing `;` (`boundary="a;b"`).
pub fn is_markdown_content_type(value: Option<&str>) -> bool {
    // Missing header: absent -> reject.
    let raw = match value {
        Some(v) => v,
        None => return false,
    };

    // Split on `;` (quote-aware): first part = type, rest = parameters.
    let mut parts = split_params(raw);

    // MIME type: lowercase + trim. Must be a markdown-identifying type.
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    if !crate::config::MARKDOWN_CONTENT_TYPES.contains(&mime.as_str()) {
        return false;
    }

    // No parameters => valid (bare `text/markdown`).
    // With parameters => only `charset=utf-8` is accepted (when present).
    // Bare `; charset` (no `=`) is rejected as malformed.
    for param in parts {
        let p = param.trim();
        if p.is_empty() {
            continue;
        }
        // `name=value` (tolerant of spaces around `=`).
        if !p.contains('=') {
            // Parameter without `=` (e.g. `text/markdown; charset`): strict reject.
            // Only bare type without `;` is valid; any `;`-part must be `k=v`.
            return false;
        }
        let mut kv = p.splitn(2, '=');
        let name = kv.next().unwrap_or("").trim().to_ascii_lowercase();
        let val_raw = kv.next().unwrap_or("").trim();
        // Strip wrapping quotes when present (either kind, mixed or repeated:
        // `"'utf-8'"` -> `utf-8`).
        let val = val_raw
            .trim_matches(|c| c == '"' || c == '\'')
            .trim()
            .to_ascii_lowercase();
        if name == "charset" {
            // UTF-8 only (we accept the `utf8` alias for tolerance).
            if val != "utf-8" && val != "utf8" {
                return false;
            }
        }
        // Other parameters are ignored (e.g. `boundary=...`).
    }

    // No charset is also valid (charset is optional).
    true
}

/// Is this `Content-Type` value an image format the decoder supports?
///
/// - `None` (missing header) => `false`; callers may still fall back to
///   magic-byte sniffing (decode validates the real format anyway).
/// - Case-insensitive; parameters (`image/png; foo=bar`) are ignored.
/// - Only the formats listed in `config::IMAGE_CONTENT_TYPES`.
pub fn is_image_content_type(value: Option<&str>) -> bool {
    let Some(raw) = value else {
        return false;
    };
    let mime = split_params(raw)
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    crate::config::IMAGE_CONTENT_TYPES.contains(&mime.as_str())
}

/// `true` for the generic binary types (`application/octet-stream` and the
/// `binary/octet-stream` alias many object stores use by default).
///
/// Some correct image servers label files this way; the decoder still
/// validates the bytes before anything renders, so it is allowed as a
/// fallback container (anything else present, like `text/html`, is rejected
/// before the body is read).
pub fn is_octet_stream(value: &str) -> bool {
    let mime = split_params(value).next().unwrap_or("").trim();
    mime.eq_ignore_ascii_case("application/octet-stream")
        || mime.eq_ignore_ascii_case("binary/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::{is_image_content_type, is_markdown_content_type, is_octet_stream};

    #[test]
    fn accepts_valid_forms() {
        assert!(is_markdown_content_type(Some("text/markdown")));
        assert!(is_markdown_content_type(Some(
            "text/markdown; charset=utf-8"
        )));
        assert!(is_markdown_content_type(Some(
            "Text/Markdown ; Charset=UTF-8"
        )));
        assert!(is_markdown_content_type(Some(
            "text/markdown;charset=\"utf-8\""
        )));
        assert!(is_markdown_content_type(Some("  TEXT/MARKDOWN  ")));
    }

    #[test]
    fn accepts_markdown_aliases() {
        // Servers label markdown with these spellings; all identify markdown.
        for ct in [
            "text/x-markdown",
            "application/markdown",
            "application/x-markdown",
            "Application/Markdown; charset=utf-8",
        ] {
            assert!(is_markdown_content_type(Some(ct)), "{ct}");
        }
        // `text/plain` does NOT identify markdown: never accepted.
        assert!(!is_markdown_content_type(Some("text/plain")));
        assert!(!is_markdown_content_type(Some("text/plain; charset=utf-8")));
        // A markdown alias with a non-UTF-8 charset is still rejected.
        assert!(!is_markdown_content_type(Some(
            "application/markdown; charset=iso-8859-1"
        )));
    }

    #[test]
    fn accepts_quoted_params_with_semicolons() {
        // RFC-valid quoted parameter values may contain `;`: the naive
        // `split(';')` used to break them into a param without `=`.
        assert!(is_markdown_content_type(Some(
            "text/markdown; boundary=\"a;b\""
        )));
        assert!(is_markdown_content_type(Some(
            "text/markdown; charset=utf-8; boundary=\"a;b\""
        )));
        assert!(is_markdown_content_type(Some(
            "text/markdown; boundary=\"a\\\";b\""
        )));
        // A non-UTF-8 charset inside quotes is still rejected.
        assert!(!is_markdown_content_type(Some(
            "text/markdown; charset=\"a;b\""
        )));
    }

    #[test]
    fn recognizes_supported_image_types() {
        for ct in [
            "image/png",
            "image/jpeg",
            "image/jpg",
            "image/gif",
            "image/webp",
            "image/avif",
            "image/jxl",
            "image/svg+xml",
            "image/svg",
            "IMAGE/SVG+XML; charset=utf-8",
            "image/x-icon",
            "image/vnd.microsoft.icon",
            "IMAGE/PNG; charset=binary",
        ] {
            assert!(is_image_content_type(Some(ct)), "{ct}");
        }
        for ct in [
            None,
            Some("text/html"),
            Some("application/pdf"),
            Some("image/svg+xmlx"),
            Some(""),
        ] {
            assert!(!is_image_content_type(ct), "{ct:?}");
        }
        assert!(is_octet_stream("application/octet-stream"));
        assert!(is_octet_stream("Application/Octet-Stream; x=1"));
        // Generic object-store default: accepted as a generic container (the
        // magic sniff still validates the real format).
        assert!(is_octet_stream("binary/octet-stream"));
        assert!(is_octet_stream("Binary/Octet-Stream"));
        assert!(!is_octet_stream("text/html"));
        assert!(!is_octet_stream("binary/octet-streamx"));
    }

    #[test]
    fn recognizes_legacy_image_aliases() {
        // Legacy servers label PNG/JPEG this way; the decoder
        // still validates the magic bytes, so the alias is only a gate.
        assert!(is_image_content_type(Some("image/x-png")));
        assert!(is_image_content_type(Some("IMAGE/X-PNG")));
        assert!(is_image_content_type(Some("image/pjpeg")));
        assert!(is_image_content_type(Some("image/pjpeg; charset=binary")));
    }

    #[test]
    fn rejects_others() {
        assert!(!is_markdown_content_type(None));
        assert!(!is_markdown_content_type(Some("text/html")));
        assert!(!is_markdown_content_type(Some("text/plain")));
        assert!(!is_markdown_content_type(Some(
            "text/markdown; charset=iso-8859-1"
        )));
        // `application/markdown` is now an accepted alias; a non-markdown
        // application type is not.
        assert!(!is_markdown_content_type(Some("application/octet-stream")));
        // Bare parameter without `=` is malformed.
        assert!(!is_markdown_content_type(Some("text/markdown; charset")));
        // Mixed / repeated quotes still unwrap.
        assert!(is_markdown_content_type(Some(
            "text/markdown; charset=\"'utf-8'\""
        )));
    }

    #[test]
    fn tolerates_a_parameter_bomb_without_collecting_it() {
        // Lazy splitting: a hostile header with thousands of parameters is
        // scanned once (never collected into a `Vec`) and the verdict is the
        // same as for the short header.
        let mut ct = String::from("text/markdown; charset=utf-8");
        let mut bad = String::from("text/markdown");
        for i in 0..20_000 {
            ct.push_str(&format!("; p{i}=v"));
            bad.push_str(&format!("; p{i}=v"));
        }
        bad.push_str("; charset=iso-8859-1");
        assert!(is_markdown_content_type(Some(&ct)));
        assert!(!is_markdown_content_type(Some(&bad)));
        // The image/octet gates stop at the first item.
        let img = format!("image/png;{ct}");
        assert!(is_image_content_type(Some(&img)));
        let octet = format!("application/octet-stream;{ct}");
        assert!(is_octet_stream(&octet));
    }
}
