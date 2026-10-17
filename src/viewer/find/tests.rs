// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::find` unit tests: query caps, case-insensitive ranges and the
//! per-frame ordinal claiming (the bar, highlight and navigation share it).

use super::*;

#[test]
fn empty_needle_matches_nothing() {
    assert!(find_ranges("hello", "").is_empty());
    assert_eq!(count_occurrences("hello", ""), 0);
}

#[test]
fn ascii_case_insensitive_ranges() {
    let ranges = find_ranges("Hello HELLO hello", "hello");
    assert_eq!(ranges.len(), 3);
    assert_eq!(
        &"hello hello hello".to_lowercase()[ranges[0].0..ranges[0].1],
        "hello"
    );
}

#[test]
fn non_overlapping_repeats() {
    assert_eq!(find_ranges("aaa", "aa").len(), 1);
}

#[test]
fn unicode_case_folding_counts() {
    assert_eq!(count_occurrences("Straße STRASSE", "strasse"), 1);
}

/// Regression: a case folding that changes byte length before the match
/// (`İ` lowercases to two bytes) shifts the ranges computed in the
/// lowercased copy. `is_valid_slice` accepted them because they landed on
/// char boundaries (`text[3..6]` = `"OO "`), so the precise-split painter
/// washed the wrong characters; `ranges_resolve` must reject them and let
/// the whole-fragment fallback paint instead.
#[test]
fn length_changing_foldings_do_not_resolve() {
    let text = "İFOO BAR";
    let (ranges, _) = find_ranges_capped(text, "foo");
    assert_eq!(ranges.len(), 1);
    assert!(
        is_valid_slice(text, ranges[0].0, ranges[0].1),
        "the shifted range still lands on char boundaries (the old check passed)"
    );
    assert!(
        !ranges_resolve(text, &ranges, "foo"),
        "a shifted range must not resolve to the real match"
    );
    // The exact offsets still resolve when the fold is length-preserving.
    let text = "A FOO bar";
    let (ranges, _) = find_ranges_capped(text, "foo");
    assert!(ranges_resolve(text, &ranges, "foo"));
    // ASCII ranges keep resolving through the fast path.
    let (ranges, _) = find_ranges_capped("HELLO hello", "hello");
    assert!(ranges_resolve("HELLO hello", &ranges, "hello"));
}

#[test]
fn query_truncation_keeps_graphemes() {
    let long = "e\u{0301}".repeat(crate::config::MAX_FIND_QUERY_CHARS + 10);
    let cut = truncate_query(&long);
    assert!(cut.chars().count() <= crate::config::MAX_FIND_QUERY_CHARS);
    assert!(is_valid_slice(&cut, 0, cut.len()));
}

#[test]
fn claim_caps_at_config_max() {
    let mut render = FindRender::new("a", 0, false);
    render.next_ordinal = crate::config::MAX_FIND_MATCHES - 1;
    let start = render.claim(5);
    assert_eq!(start, crate::config::MAX_FIND_MATCHES - 1);
    assert_eq!(render.next_ordinal, crate::config::MAX_FIND_MATCHES);
    assert!(render.capped);
}

/// A fragment holding MORE occurrences than the cap reports `capped`
/// (the bar shows `N+`) without ever returning the overflow.
#[test]
fn ranges_report_the_per_fragment_cap() {
    let text = "x".repeat(crate::config::MAX_FIND_MATCHES + 10);
    let (ranges, capped) = find_ranges_capped(&text, "x");
    assert_eq!(ranges.len(), crate::config::MAX_FIND_MATCHES);
    assert!(capped, "more occurrences follow the returned ones");

    // Exactly at the cap without overflow: not capped.
    let text = "x".repeat(crate::config::MAX_FIND_MATCHES);
    let (ranges, capped) = find_ranges_capped(&text, "x");
    assert_eq!(ranges.len(), crate::config::MAX_FIND_MATCHES);
    assert!(!capped);
}

#[test]
fn find_state_reset_drops_query() {
    let mut state = FindState {
        open: true,
        query: "rust".to_string(),
        current: 2,
        total: 5,
        capped: false,
        request_focus: false,
        scroll_to_current: false,
        generation: 7,
        esc_elsewhere: false,
        count_label: String::new(),
        count_key: None,
    };
    assert!(state.active());
    state.reset();
    assert!(!state.open && state.query.is_empty() && state.total == 0);
}
