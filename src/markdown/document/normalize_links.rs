// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::normalize_links`: the split-link repair pass.
//!
//! An exporter that breaks `[label` blank line `body](url)` across a blank
//! line is joined back into one link, with code spans/blocks masked out so
//! code is never touched.

use super::normalize::{fence_update, is_indented_code, FenceState};

/// Cheap pre-check for [`join_broken_links`]: a link split across a blank line
/// needs BOTH a `](` close and a blank line (a line of only spaces/tabs
/// counts, as many editors leave trailing whitespace behind). Only a document
/// with both pays for the full masked scan.
pub(super) fn needs_link_join(text: &str) -> bool {
    text.contains("](") && text.lines().any(|l| l.trim().is_empty())
}

/// Removes blank lines that fall INSIDE a balanced `[...](...)` span, so a
/// markdown exporter's block-split card link (`[![icon] Vendor<h3>Name</h3>`
/// blank line `Description](url)`) parses as ONE link instead of a literal
/// `[` and a stray `](url)`.
///
/// - Fenced, indented AND inline code is masked first: a split-link
///   lookalike in code is never touched (and an unmatched `` `[` `` cannot
///   be matched by a later `](` that is ordinary text).
/// - Bracket scanning is balanced (`[` / `](`) and escape-aware (`\[`), so an
///   ordinary `[reference]` that is never closed with `](` leaves nothing to
///   join. Nested spans (`[![a](b)](c)`) merge into the outer span.
/// - Each removed blank run becomes a single space, so the joined label keeps
///   word separation.
/// - Bounded by `config::MAX_LINK_JOINS` spans and `MAX_LINK_JOIN_DEPTH`
///   open brackets; beyond either the scan stops and the rest is untouched.
///
/// Returns `None` when no blank line was removed, so the caller keeps its
/// existing allocation instead of cloning the body.
pub(super) fn join_broken_links(text: &str) -> Option<String> {
    let masked = mask_code(text);
    let spans = link_spans(&masked);
    if spans.is_empty() {
        return None;
    }
    let ranges = merge_spans(spans);
    let removals = blank_runs_within(text, &ranges);
    if removals.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for (a, b) in removals {
        if a < last {
            continue;
        }
        out.push_str(&text[last..a]);
        out.push(' ');
        last = b;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// Copies `text` with every CODE region overwritten by spaces — fenced and
/// indented blocks (whole lines) and inline backtick spans — so the bracket
/// scanner cannot see a `[` that is really code. Byte length is preserved
/// (each byte becomes one space), so offsets still index `text`.
pub(super) fn mask_code(text: &str) -> Vec<u8> {
    let mut masked = Vec::new();
    mask_code_into(&mut masked, text);
    masked
}

/// [`mask_code`] into a caller-owned buffer: `markup_pass` re-masks after
/// stripping comment spans, and reusing the buffer saves one body-sized
/// allocation per pass.
pub(super) fn mask_code_into(masked: &mut Vec<u8>, text: &str) {
    masked.clear();
    masked.extend_from_slice(text.as_bytes());
    let mut fence: FenceState = None;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let is_code = fence_update(&mut fence, content) || is_indented_code(content);
        if is_code {
            for b in &mut masked[offset..offset + content.len()] {
                *b = b' ';
            }
        }
        offset += line.len();
    }
    mask_inline_code(masked);
}

/// Overwrites inline code spans (`` `code` ``) with spaces. An unmatched
/// opening backtick renders literally in CommonMark and is left alone.
///
/// Bounded by a scan budget (`2 x` the body length): without it, a crafted
/// body of backtick runs of distinct lengths and no closers made every opener
/// scan to the end (O(n^2) on a background thread). When the budget runs out
/// the remaining backticks are left unmasked — the pass is a best-effort
/// pre-filter, never a correctness requirement.
pub(super) fn mask_inline_code(masked: &mut [u8]) {
    let n = masked.len();
    let mut budget = n.saturating_mul(2);
    let mut i = 0usize;
    while i < n {
        match masked[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'`' => {
                let open_len = backtick_run(masked, i);
                if let Some(close) = closing_backticks(masked, i + open_len, open_len, &mut budget)
                {
                    for b in &mut masked[i..close] {
                        *b = b' ';
                    }
                    i = close + open_len;
                    continue;
                }
                if budget == 0 {
                    return;
                }
                i += open_len;
                continue;
            }
            _ => i += 1,
        }
    }
}

/// Length of the backtick run starting at `i` (0 when the byte is not one).
pub(super) fn backtick_run(masked: &[u8], mut i: usize) -> usize {
    let start = i;
    while i < masked.len() && masked[i] == b'`' {
        i += 1;
    }
    i - start
}

/// Index of the next backtick run of exactly `len` (the CommonMark closer for
/// an inline code span), or `None` when there is none or `budget` runs out.
pub(super) fn closing_backticks(
    masked: &[u8],
    mut j: usize,
    len: usize,
    budget: &mut usize,
) -> Option<usize> {
    while j < masked.len() {
        if *budget == 0 {
            return None;
        }
        if masked[j] == b'`' {
            let run = backtick_run(masked, j);
            *budget = budget.saturating_sub(run);
            if run == len {
                return Some(j);
            }
            j += run;
        } else {
            *budget = budget.saturating_sub(1);
            j += 1;
        }
    }
    None
}

/// `(start, end)` byte spans of balanced inline links/images. `end` is one
/// past the closing `)` of the destination; a bare `[reference]` (no `](`)
/// never opens a span.
pub(super) fn link_spans(masked: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let n = masked.len();
    let mut i = 0usize;
    while i < n {
        match masked[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                if stack.len() < crate::config::MAX_LINK_JOIN_DEPTH {
                    stack.push(i);
                }
            }
            b']' if i + 1 < n && masked[i + 1] == b'(' => {
                if let Some(start) = stack.pop() {
                    let mut j = i + 2;
                    let mut depth = 1i32;
                    while j < n && depth > 0 {
                        match masked[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                    if depth == 0 {
                        spans.push((start, j));
                        if spans.len() >= crate::config::MAX_LINK_JOINS {
                            break;
                        }
                    }
                    i = j;
                    continue;
                }
            }
            // A bare `]` closes its bracket pair WITHOUT opening a link
            // (`[the docs]`): pop so a later `](` cannot match across it and
            // invent a span that straddles an unrelated blank line.
            b']' => {
                stack.pop();
            }
            _ => {}
        }
        i += 1;
    }
    spans
}

/// Sorts spans and merges overlapping/adjacent ones, so nested spans cost one
/// pass instead of one per level.
pub(super) fn merge_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (s, e) in spans {
        match out.last_mut() {
            Some((_, last_end)) if s <= *last_end => {
                if e > *last_end {
                    *last_end = e;
                }
            }
            _ => out.push((s, e)),
        }
    }
    out
}

/// Blank-line runs (2+ newlines separated only by spaces/tabs) inside the
/// given byte ranges, plus the adjacent indentation, so replacing each with
/// one space cannot leave doubled spaces.
pub(super) fn blank_runs_within(text: &str, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut removals = Vec::new();
    for &(s, e) in ranges {
        let mut k = s;
        while k < e {
            if b[k] != b'\n' {
                k += 1;
                continue;
            }
            // A blank line is only spaces/tabs between two newlines.
            let mut m = k + 1;
            while m < e && (b[m] == b' ' || b[m] == b'\t') {
                m += 1;
            }
            if m >= e || b[m] != b'\n' {
                k += 1;
                continue;
            }
            // Extend across every further whitespace-only line.
            let mut end = m + 1;
            loop {
                let mut p = end;
                while p < e && (b[p] == b' ' || b[p] == b'\t') {
                    p += 1;
                }
                if p < e && b[p] == b'\n' {
                    end = p + 1;
                } else {
                    break;
                }
            }
            // Swallow the next line's indentation and this line's trailing
            // spaces; the single replacement space covers both.
            while end < e && (b[end] == b' ' || b[end] == b'\t') {
                end += 1;
            }
            let mut start = k;
            while start > s && (b[start - 1] == b' ' || b[start - 1] == b'\t') {
                start -= 1;
            }
            removals.push((start, end));
            k = end;
        }
    }
    removals
}
