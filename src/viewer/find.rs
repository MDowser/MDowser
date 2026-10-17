// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::find`: in-page find (`Ctrl/Cmd+F`) over the parsed document.
//!
//! - The query searches the RENDERED text fragments (which derive from the
//!   `Fetched.events` parsed once at fetch time), not a second parser pass:
//!   prose, headings, list items, quotes and table cells share the fragment
//!   path; fenced code blocks count their cached body.
//! - Matching is a case-insensitive substring (per-`char` lowercasing, no
//!   regex). Exotic case foldings that change byte length (`İ` → `i̇`)
//!   fall back to a whole-fragment highlight instead of a mis-sliced split.
//! - Per-frame render state (`FindRender`) assigns ordinals in document
//!   order while painting; the bar (`overlays::find`) reads `FindState` for
//!   the count and drives `current` + scroll. Totals lag at most one frame.

use crate::util::truncate_chars;
use eframe::egui;

/// Per-tab find state. Lives on `Tab` so each tab keeps its own query; the
/// viewer resets the count every frame it renders and `clear_overlay` closes
/// it on navigation/tab switch.
#[derive(Debug, Clone, Default)]
pub struct FindState {
    /// True while the find bar is open (only on `Loaded`/`Licenses`).
    pub open: bool,
    /// Raw query as typed (capped at `MAX_FIND_QUERY_CHARS` by the bar).
    pub query: String,
    /// Index of the current match (into the last rendered total).
    pub current: usize,
    /// Matches rendered last frame (capped at `MAX_FIND_MATCHES`).
    pub total: usize,
    /// True when `total` hit the cap (the bar shows `N+`).
    pub capped: bool,
    /// Focus the query field on the next bar frame (just opened).
    pub request_focus: bool,
    /// Scroll the current match into view on the next document frame
    /// (query change or Prev/Next).
    pub scroll_to_current: bool,
    /// `doc_generation` of the last render (stale totals are dropped when
    /// the visible document is replaced).
    pub generation: u64,
    /// True when ANOTHER surface owns Escape this frame (a focused field,
    /// the page-info card, an open menu/popup or a floating window): the bar
    /// must not close on the same press that the other surface acts on.
    /// Written by `app::shortcuts` before the viewer draws (the bar itself
    /// owns the close so it stays self-contained for direct rendering).
    pub esc_elsewhere: bool,
    /// Cached bar count label (`"3/12"`, `"No matches"`), rebuilt only when
    /// `(current, total, capped)` changes: the bar paints every frame while
    /// open, and the `format!` otherwise allocated per frame.
    count_label: String,
    count_key: Option<(usize, usize, bool)>,
}

impl FindState {
    /// Shows the bar for `generation` (keeps the query for re-search).
    /// Named `show` (not `open`): the `open` FIELD owns that word, and a
    /// same-name method on top of it read as a field access with arguments.
    pub fn show(&mut self, generation: u64) {
        self.open = true;
        self.generation = generation;
        self.request_focus = true;
        self.scroll_to_current = true;
    }

    /// Closes the bar and drops the count (keeps the query for reopen).
    pub fn close(&mut self) {
        self.open = false;
        self.current = 0;
        self.total = 0;
        self.capped = false;
        self.request_focus = false;
        self.scroll_to_current = false;
        self.esc_elsewhere = false;
        self.count_key = None;
    }

    /// Drops everything (navigation/tab switch: no stale highlight).
    pub fn reset(&mut self) {
        self.open = false;
        self.query.clear();
        self.current = 0;
        self.total = 0;
        self.capped = false;
        self.request_focus = false;
        self.scroll_to_current = false;
        self.esc_elsewhere = false;
        self.count_key = None;
    }

    /// True when the document should be searched this frame.
    pub fn active(&self) -> bool {
        self.open && !self.query.is_empty()
    }

    /// Bar count label for `(current, total, capped)`, memoized: repeated
    /// frames with an unchanged count borrow the cached `String` instead of
    /// formatting a new one per frame.
    pub fn count_text(&mut self, current: usize, total: usize, capped: bool) -> &str {
        let key = (current, total, capped);
        if self.count_key != Some(key) {
            self.count_key = Some(key);
            self.count_label = if total == 0 {
                "No matches".to_string()
            } else if capped {
                format!("{}/{}+", current + 1, total)
            } else {
                format!("{}/{}", current + 1, total)
            };
        }
        &self.count_label
    }
}

/// Per-frame render cursor. Built once per document frame from `FindState`;
/// `next_ordinal` counts matches in document order so highlight, count and
/// Prev/Next navigation share one source of truth.
pub(crate) struct FindRender {
    /// Lowercased query (built once per frame, not per fragment).
    pub query_lower: String,
    /// Ordinal of the current match.
    pub current: usize,
    /// Next ordinal to assign.
    pub next_ordinal: usize,
    /// True when the cap was hit (remaining occurrences render normally).
    pub capped: bool,
    /// Scroll the current match into view when painted.
    pub scroll: bool,
    /// Screen rect of the painted CURRENT match (this frame), so the caller
    /// can tell whether it ended up visible (the scroll request is retried
    /// while it is not, see `show_markdown_with_find`).
    pub current_rect: Option<egui::Rect>,
}

impl FindRender {
    pub fn new(query: &str, current: usize, scroll: bool) -> Self {
        Self {
            query_lower: query.to_lowercase(),
            current,
            next_ordinal: 0,
            capped: false,
            scroll,
            current_rect: None,
        }
    }

    /// Ordinals `[next_ordinal, next_ordinal + n)` belong to the caller.
    /// Returns the start ordinal (capped at `MAX_FIND_MATCHES`).
    pub fn claim(&mut self, n: usize) -> usize {
        let start = self.next_ordinal;
        let left = crate::config::MAX_FIND_MATCHES.saturating_sub(start);
        let take = n.min(left);
        self.next_ordinal = start + take;
        if take < n {
            self.capped = true;
        }
        start
    }
}

/// Truncates a raw query to `MAX_FIND_QUERY_CHARS` without cutting a
/// grapheme cluster (same rule as every other display cap).
pub fn truncate_query(raw: &str) -> String {
    truncate_chars(raw, crate::config::MAX_FIND_QUERY_CHARS).into_owned()
}

/// Byte ranges of `needle_lower` inside `text` (both compared lowercased),
/// capped at `MAX_FIND_MATCHES`; the bool reports that MORE occurrences
/// follow (the `N+` the bar shows). Ranges live in the LOWERCASED space;
/// the caller must verify them as `text` boundaries (exotic foldings change
/// byte length) and fall back to a whole-fragment highlight when they do
/// not line up.
pub fn find_ranges_capped(text: &str, needle_lower: &str) -> (Vec<(usize, usize)>, bool) {
    if needle_lower.is_empty() {
        return (Vec::new(), false);
    }
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    let mut capped = false;
    while let Some(rel) = lower[from..].find(needle_lower) {
        if out.len() >= crate::config::MAX_FIND_MATCHES {
            // One more occurrence past the cap: the fragment is capped, and
            // the extra hit is never returned (bounded memory).
            capped = true;
            break;
        }
        let start = from + rel;
        let end = start + needle_lower.len();
        out.push((start, end));
        from = end;
        if from >= lower.len() {
            break;
        }
    }
    (out, capped)
}

/// [`find_ranges_capped`] without the cap flag, for callers that only need
/// the ranges within the display bound (tests pin the byte layout here;
/// production counts via [`count_occurrences`] without the `Vec`).
#[cfg(test)]
pub fn find_ranges(text: &str, needle_lower: &str) -> Vec<(usize, usize)> {
    find_ranges_capped(text, needle_lower).0
}

/// Counts occurrences of `needle_lower` in `text` (case-insensitive)
/// WITHOUT allocating the range vector: `find_ranges` builds a `Vec` just
/// for its `.len()`, which burned one allocation per visible code line per
/// frame while the find bar was open.
pub fn count_occurrences(text: &str, needle_lower: &str) -> usize {
    if needle_lower.is_empty() {
        return 0;
    }
    // ASCII fast path: no `to_lowercase()` allocation for the common
    // English/code text. `eq_ignore_ascii_case` on byte windows is exact
    // for ASCII on both sides (the query is already lowercased).
    if text.is_ascii() && needle_lower.is_ascii() {
        let (hay, needle) = (text.as_bytes(), needle_lower.as_bytes());
        if needle.len() > hay.len() {
            return 0;
        }
        let mut count = 0usize;
        let mut from = 0usize;
        while from + needle.len() <= hay.len() {
            if count >= crate::config::MAX_FIND_MATCHES {
                break;
            }
            match hay[from..]
                .windows(needle.len())
                .position(|w| w.eq_ignore_ascii_case(needle))
            {
                Some(rel) => {
                    count += 1;
                    from += rel + needle.len();
                }
                None => break,
            }
        }
        return count;
    }
    let lower = text.to_lowercase();
    let mut count = 0usize;
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(needle_lower) {
        if count >= crate::config::MAX_FIND_MATCHES {
            break;
        }
        count += 1;
        from += rel + needle_lower.len();
        if from >= lower.len() {
            break;
        }
    }
    count
}

/// True when `needle_lower` occurs in `text` (case-insensitive), stopping
/// at the first hit: the visible-row wash only needs a bool, not a count.
pub fn contains_match(text: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    // Fast path: case-sensitive hit avoids the lowercase allocation.
    if text.contains(needle_lower) {
        return true;
    }
    // ASCII fast path: byte-window case-insensitive scan, still no
    // allocation. Doubles as the pre-gate before `find_ranges_capped`, so
    // non-matching fragments cost one scan and no `Vec` per frame.
    if text.is_ascii() && needle_lower.is_ascii() {
        let (hay, needle) = (text.as_bytes(), needle_lower.as_bytes());
        if needle.len() > hay.len() {
            return false;
        }
        return hay
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle));
    }
    text.to_lowercase().contains(needle_lower)
}

/// True when every range in `ranges` really selects `needle_lower` in
/// `text` (case-insensitively) on character boundaries.
///
/// [`find_ranges_capped`] computes offsets in a LOWERCASED copy, and exotic
/// foldings change byte length (`İ` folds to two bytes), so offsets that
/// happen to land on boundaries can still cover the wrong characters. The
/// precise-split painters must bail to the whole-fragment fallback when the
/// ranges do not line up; the ASCII fast path (both sides ASCII) cannot
/// shift, so it only needs the boundary check.
pub fn ranges_resolve(text: &str, ranges: &[(usize, usize)], needle_lower: &str) -> bool {
    // ASCII cannot shift under case folding: the boundary check is exact.
    if text.is_ascii() && needle_lower.is_ascii() {
        return ranges.iter().all(|(s, e)| is_valid_slice(text, *s, *e));
    }
    ranges
        .iter()
        .all(|(s, e)| is_valid_slice(text, *s, *e) && text[*s..*e].to_lowercase() == needle_lower)
}

/// True when `(start, end)` slices `text` on `char` boundaries.
pub fn is_valid_slice(text: &str, start: usize, end: usize) -> bool {
    text.len() >= end && text.is_char_boundary(start) && text.is_char_boundary(end)
}

#[cfg(test)]
mod tests;
