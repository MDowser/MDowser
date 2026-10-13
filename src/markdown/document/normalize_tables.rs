// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::normalize_tables`: delimiter-row repair.
//!
//! Rows like `| --- | x |` that lost cells are padded so pulldown-cmark
//! parses the table instead of dropping it; fenced/indented code is never
//! touched.

use super::normalize::{fence_update, is_indented_code, FenceState};

/// Scans `text` and returns `(line_index, replacement)` for every delimiter
/// row that must be padded, in document order; an empty `Vec` means nothing
/// to do (no allocation: the fast path).
///
/// Fenced (```/~~~) and indented (4+ spaces / tab) code is NEVER touched:
/// padding inside it would corrupt the code shown and copied (`plain_text`
/// re-normalizes the same text before parsing). A verbatim line also breaks
/// the header/delimiter adjacency, so it can never be part of a fix.
pub(super) fn table_delimiter_fixes(text: &str) -> Vec<(usize, String)> {
    let mut fixes = Vec::new();
    let mut fence: FenceState = None;
    let mut prev_plain: Option<&str> = None;
    for (i, line) in text.lines().enumerate() {
        if fence_update(&mut fence, line) || is_indented_code(line) {
            prev_plain = None;
            continue;
        }
        if let Some(prev) = prev_plain {
            if prev.contains('|') && is_table_delimiter_line(line) {
                let h = header_cols(prev);
                let d = delimiter_cols(line);
                // `h <= MAX_TABLE_COLS`: an oversized line is rejected by
                // `parse_events` anyway; never build a body-sized pad first.
                if h > d && h > 1 && d > 0 && h <= crate::config::MAX_TABLE_COLS {
                    fixes.push((i, pad_delimiter(line, h - d)));
                }
            }
        }
        prev_plain = Some(line);
    }
    fixes
}
/// Number of columns in a header line: pipe segments ignoring outer pipes.
pub(super) fn header_cols(line: &str) -> usize {
    pipe_cols(line)
}

/// Number of columns in a delimiter line (same counting).
pub(super) fn delimiter_cols(line: &str) -> usize {
    pipe_cols(line)
}

/// Counts pipe-separated columns, ignoring one optional leading/trailing
/// UNESCAPED pipe. Escaped pipes (`\|`) are cell content, not separators:
/// pulldown's table parser skips them, so counting them here over-padded a
/// valid escaped-pipe header and made pulldown reject the whole table.
pub(super) fn pipe_cols(line: &str) -> usize {
    let t = line.trim();
    if t.is_empty() {
        return 0;
    }
    let leading = t.starts_with('|');
    // A trailing `|` is an outer pipe only when it is not escaped; count the
    // run of backslashes before it (odd = escaped).
    let trailing = t.ends_with('|') && {
        let before = &t[..t.len() - 1];
        before.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 0
    };
    let mut pipes = 0usize;
    let mut escaped = false;
    for b in t.bytes() {
        match b {
            b'\\' => escaped = !escaped,
            b'|' => {
                if !escaped {
                    pipes += 1;
                }
                escaped = false;
            }
            _ => escaped = false,
        }
    }
    // Cells = separators + 1, minus the empty segments an outer pipe adds.
    pipes + 1 - usize::from(leading) - usize::from(trailing)
}

/// Rebuilds `text` with the delimiter rows replaced by [`table_delimiter_fixes`].
pub(super) fn apply_delimiter_fixes(text: &str, fixes: Vec<(usize, String)>) -> String {
    if fixes.is_empty() {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len() + 64);
    let mut fixes = fixes.into_iter().peekable();
    for (i, line) in lines.iter().enumerate() {
        if fixes.peek().is_some_and(|(line_no, _)| *line_no == i) {
            if let Some((_, fixed)) = fixes.next() {
                out.push_str(&fixed);
            } else {
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
        // `lines()` strips terminators; restore `\n` between lines and
        // preserve the trailing newline state of the input.
        if i + 1 < lines.len() || text.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Adds `missing` ` --- ` columns to a delimiter row, preserving outer pipes.
pub(super) fn pad_delimiter(line: &str, missing: usize) -> String {
    // Preserve leading whitespace (indentation is already 0 for metadata,
    // but tables may sit inside lists/quotes with up to 3 spaces).
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);
    let mut s = rest.trim_end().to_string();
    let ends_with_pipe = s.trim_end().ends_with('|');
    for _ in 0..missing {
        if ends_with_pipe && s.trim_end().ends_with('|') {
            s.push_str(" --- |");
        } else {
            // No trailing pipe: `| --- | ---` + ` | ---`.
            s.push_str(" | ---");
        }
    }
    format!("{indent}{s}")
}

/// Returns `true` for table delimiter rows (`| --- | :---: |`, etc.).
/// - Only `|`, `-`, `:`, space/tab allowed; must contain `-` and `|`.
/// - A bare `---` (HR) returns `false` (no pipe).
pub(super) fn is_table_delimiter_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || !t.contains('|') || !t.contains('-') {
        return false;
    }
    t.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c == ' ' || c == '\t')
}
