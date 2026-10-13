// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    BOOKMARK_GRID_MAX_VISIBLE_ROWS, DIR_INDEXES, FAVICON_PATHS, FAVICON_REQUEST_TIMEOUT_SECS,
    FETCH_SLOT_WAIT_SECS, IMAGE_JOB_TIMEOUT_SECS, MARKDOWN_CONTENT_TYPES, MAX_INLINE_IMAGE_H,
    MAX_INLINE_VECTOR_H, MAX_ROUTE_CANDIDATES, MAX_TITLE_CHARS, REQUEST_TIMEOUT_SECS,
    TAB_TITLE_CHARS, TOTAL_FETCH_TIMEOUT_SECS, USER_AGENT, WINDOW_INITIAL_H, WINDOW_INITIAL_W,
    WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_TITLE_CHARS, WINDOW_TITLE_ERROR_URL_CHARS,
};

#[test]
fn user_agent_is_defined() {
    // The browser must always send a defined User-Agent.
    assert!(!USER_AGENT.is_empty());
    assert!(USER_AGENT.starts_with("MDowser/"));
    assert!(USER_AGENT.contains(env!("CARGO_PKG_VERSION")));
    assert!(USER_AGENT.contains("CommonMark"));
}

#[test]
fn favicon_probes_are_conventional_and_ordered() {
    // `.ico` then `.png`, with the modern SVG last (many sites only
    // publish that one). Order matters: the first decodable wins.
    assert_eq!(FAVICON_PATHS[0], "favicon.ico");
    assert_eq!(FAVICON_PATHS[1], "favicon.png");
    assert!(FAVICON_PATHS.contains(&"favicon.svg"));
}

#[test]
fn directory_indexes_are_conventional_and_ordered() {
    // Shared by the HTTPS resolver and the local reader: the standard
    // spellings only (`index.md`, then `README.md`).
    assert_eq!(DIR_INDEXES, ["index.md", "README.md"]);
}

#[test]
fn markdown_types_identify_markdown_but_never_plain_text() {
    assert!(MARKDOWN_CONTENT_TYPES.contains(&"text/markdown"));
    assert!(MARKDOWN_CONTENT_TYPES.contains(&"text/x-markdown"));
    assert!(MARKDOWN_CONTENT_TYPES.contains(&"application/markdown"));
    assert!(MARKDOWN_CONTENT_TYPES.contains(&"application/x-markdown"));
    assert!(!MARKDOWN_CONTENT_TYPES.contains(&"text/plain"));
}

#[test]
fn visual_timeout_covers_the_worst_case_cascade() {
    // Slot wait + every candidate (the largest cascade) must fit inside
    // the visual timeout or a legitimate load could be abandoned mid-flight.
    let worst_case = FETCH_SLOT_WAIT_SECS + MAX_ROUTE_CANDIDATES as u64 * REQUEST_TIMEOUT_SECS;
    assert!(TOTAL_FETCH_TIMEOUT_SECS > worst_case);
}

#[test]
fn favicon_probe_budget_fits_the_entry_deadline() {
    // The probes are sequential inside ONE image slot: the worst case
    // must stay under the entry deadline, or a dead host would keep the
    // placeholder (and the slot) alive after the UI gave up on the icon.
    let worst = FAVICON_PATHS.len() as u64 * FAVICON_REQUEST_TIMEOUT_SECS;
    assert!(
        worst < IMAGE_JOB_TIMEOUT_SECS,
        "favicon probes worst case {worst}s must fit {}s",
        IMAGE_JOB_TIMEOUT_SECS
    );
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn ui_layout_bounds_are_sane() {
    // Pins the documented relationships between chrome caps (const-only
    // by design): a regression that edits one cap without the other
    // fails here instead of silently bloating the rail/window.
    assert!(BOOKMARK_GRID_MAX_VISIBLE_ROWS >= 1);
    assert!(MAX_INLINE_VECTOR_H > 0.0);
    assert!(MAX_INLINE_IMAGE_H > 0.0);
    assert!(
        MAX_INLINE_VECTOR_H <= MAX_INLINE_IMAGE_H,
        "vector clamp {} must not exceed line cap {}",
        MAX_INLINE_VECTOR_H,
        MAX_INLINE_IMAGE_H
    );
    // Window/tab titles stay far below the stored-title cap; the
    // `Error: ` prefix plus its URL must fit the window cap.
    assert!(TAB_TITLE_CHARS >= 1);
    assert!(WINDOW_TITLE_CHARS >= TAB_TITLE_CHARS);
    assert!(WINDOW_TITLE_CHARS <= MAX_TITLE_CHARS);
    assert!(
        WINDOW_TITLE_ERROR_URL_CHARS + "Error: ".len() <= WINDOW_TITLE_CHARS,
        "error URL cap must leave room for the prefix"
    );
    // Window geometry stays usable: the initial size fits the minimum,
    // and the minimum leaves room for the toolbar plus a reading line.
    assert!(WINDOW_INITIAL_W >= WINDOW_MIN_W);
    assert!(WINDOW_INITIAL_H >= WINDOW_MIN_H);
    assert!(WINDOW_MIN_W >= 320.0);
    assert!(WINDOW_MIN_H >= 240.0);
}

#[test]
fn deadline_helper_never_panics_and_moves_forward() {
    // Normal deadlines land in the future; even `u64::MAX` fails
    // closed (now) instead of panicking the calling thread.
    let before = std::time::Instant::now();
    let soon = super::deadline_after(60);
    assert!(soon >= before);
    let saturated = super::deadline_after(u64::MAX);
    assert!(saturated >= before);
}
