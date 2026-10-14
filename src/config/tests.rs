// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    centered_window_pos, initial_window_size, BOOKMARK_GRID_MAX_VISIBLE_ROWS, DIR_INDEXES,
    FAVICON_PATHS, FAVICON_REQUEST_TIMEOUT_SECS, FETCH_SLOT_WAIT_SECS, IMAGE_JOB_TIMEOUT_SECS,
    MARKDOWN_CONTENT_TYPES, MAX_INLINE_IMAGE_H, MAX_INLINE_VECTOR_H, MAX_ROUTE_CANDIDATES,
    MAX_TITLE_CHARS, REQUEST_TIMEOUT_SECS, TAB_TITLE_CHARS, TOTAL_FETCH_TIMEOUT_SECS, USER_AGENT,
    WINDOW_INITIAL_FRACTION, WINDOW_INITIAL_H, WINDOW_INITIAL_MAX_H, WINDOW_INITIAL_MAX_W,
    WINDOW_INITIAL_W, WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_TITLE_CHARS, WINDOW_TITLE_ERROR_URL_CHARS,
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

#[test]
#[allow(clippy::assertions_on_constants)]
fn window_geometry_constants_are_consistent() {
    assert!(
        WINDOW_INITIAL_FRACTION > 0.0 && WINDOW_INITIAL_FRACTION <= 1.0,
        "the initial fraction must be a real fraction of the monitor"
    );
    assert!(WINDOW_INITIAL_MAX_W >= WINDOW_MIN_W);
    assert!(WINDOW_INITIAL_MAX_H >= WINDOW_MIN_H);
    // The creation-time fallback (used before the first frame adjusts the
    // window) stays within the computed size's bounds.
    assert!(WINDOW_INITIAL_W <= WINDOW_INITIAL_MAX_W);
    assert!(WINDOW_INITIAL_H <= WINDOW_INITIAL_MAX_H);
}

#[test]
fn initial_window_size_scales_with_the_monitor_and_respects_bounds() {
    // A 1080p monitor at 100 %: the fraction fits under both caps.
    assert_eq!(initial_window_size(1920.0, 1080.0), (1536.0, 864.0));
    // A large monitor: the caps win (never a nearly full-screen window).
    assert_eq!(
        initial_window_size(2560.0, 1440.0),
        (WINDOW_INITIAL_MAX_W, WINDOW_INITIAL_MAX_H)
    );
    // A small monitor: the fraction still clears the minimum exactly.
    assert_eq!(initial_window_size(800.0, 600.0), (640.0, 480.0));
    // A monitor smaller than the minimum: the minimum wins.
    assert_eq!(
        initial_window_size(320.0, 200.0),
        (WINDOW_MIN_W, WINDOW_MIN_H)
    );
    // Never larger than the monitor.
    let (w, h) = initial_window_size(1000.0, 700.0);
    assert!(w <= 1000.0 && h <= 700.0, "{w}x{h} overflows its monitor");
    // Non-finite input falls back to the minimum instead of panicking.
    assert_eq!(
        initial_window_size(f32::NAN, f32::INFINITY),
        (WINDOW_MIN_W, WINDOW_MIN_H)
    );
}

#[test]
fn centered_window_pos_centers_and_clamps() {
    // 1920x1080 monitor, 1536x864 outer window: even margins.
    assert_eq!(
        centered_window_pos(0.0, 0.0, 1920.0, 1080.0, 1536.0, 864.0),
        (192.0, 108.0)
    );
    // A monitor offset to the right keeps its own origin.
    assert_eq!(
        centered_window_pos(1920.0, 0.0, 1920.0, 1080.0, 1536.0, 864.0),
        (2112.0, 108.0)
    );
    // A window larger than its monitor opens at the monitor origin.
    assert_eq!(
        centered_window_pos(0.0, 0.0, 800.0, 600.0, 1000.0, 800.0),
        (0.0, 0.0)
    );
    // Non-finite input falls back to the origin on that AXIS only.
    assert_eq!(
        centered_window_pos(5.0, 7.0, f32::NAN, 600.0, 100.0, 100.0),
        (5.0, 257.0)
    );
}
