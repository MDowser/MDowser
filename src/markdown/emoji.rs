// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::emoji`: `:shortcode:` expansion.
//!
//! - Expands known shortcodes in text nodes: `:rocket:` becomes 🚀 in
//!   paragraphs, headings, list items and table cells.
//! - Code spans and fenced/indented code blocks are never touched (the
//!   caller only expands `Event::Text` outside `CodeBlock`).
//! - Conservative pattern: `:name:` where `name` is 1..=64 bytes of
//!   `[A-Za-z0-9_+-]` AND a known shortcode (so clock times like `10:30:`
//!   and scheme-prefixed URLs never match). `:+1:` and `:-1:` are valid
//!   shortcodes and do expand.
//! - Expansion happens once per fetch, inside `parse_events`, so repaints
//!   pay nothing.

use std::borrow::Cow;

/// Expands known shortcodes in one text run. Borrows when unchanged.
pub fn expand_shortcodes(text: &str) -> Cow<'_, str> {
    if !text.contains(':') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b':' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        // The documented `1..=64`-byte name cap also bounds the scan: a
        // `:aaaa…:` run of megabytes never walks a single long candidate.
        while j < bytes.len()
            && j - start < 64
            && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'_' | b'-' | b'+'))
        {
            j += 1;
        }
        if j > start && j < bytes.len() && bytes[j] == b':' {
            let name = &text[start..j];
            // Known-shortcode lookup is the filter: `10:30:` or a URL
            // scheme never matches a real shortcode.
            if let Some(emoji) = emojis::get_by_shortcode(name) {
                let buf = out.get_or_insert_with(|| String::with_capacity(text.len()));
                buf.push_str(&text[last..i]);
                buf.push_str(emoji.as_str());
                i = j + 1;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&text[last..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::expand_shortcodes;

    #[test]
    fn expands_known_shortcodes() {
        assert_eq!(
            expand_shortcodes("Ship it :rocket: now").as_ref(),
            "Ship it 🚀 now"
        );
        assert_eq!(expand_shortcodes(":smile::+1:").as_ref(), "😄👍");
        assert_eq!(
            expand_shortcodes(":warning: careful").as_ref(),
            "⚠️ careful"
        );
    }

    #[test]
    fn leaves_unknown_and_non_shortcodes_alone() {
        // Unknown name / no letter / too short / time-like / URLs.
        assert_eq!(
            expand_shortcodes(":not_an_emoji_at_all:").as_ref(),
            ":not_an_emoji_at_all:"
        );
        assert_eq!(expand_shortcodes("10:30:00").as_ref(), "10:30:00");
        assert_eq!(
            expand_shortcodes("https://ex.com/a").as_ref(),
            "https://ex.com/a"
        );
        assert_eq!(expand_shortcodes("a:b").as_ref(), "a:b");
    }

    #[test]
    fn borrows_when_unchanged() {
        assert!(matches!(
            expand_shortcodes("plain text"),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
