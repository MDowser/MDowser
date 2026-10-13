// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::tests::*;
use super::{BULLET_GUTTER, BULLET_SCALE, BULLET_TEXT_GAP, ITEM_INDENT};

/// Tight lists keep a compact rhythm: items sit about one body line
/// apart plus the implicit item spacing, instead of the old loose stack
/// (a paragraph gap + a per-item gap made every list read as separate
/// paragraphs).
#[test]
fn list_items_keep_a_compact_rhythm() {
    let (texts, _) = render_probe("- a\n- b\n- c\n");
    let (_, y0) = bullet_at(&texts, 0);
    let (_, y1) = bullet_at(&texts, 1);
    let (_, y2) = bullet_at(&texts, 2);
    for step in [y1 - y0, y2 - y1] {
        assert!(
            (24.0..=36.0).contains(&step),
            "tight list step {step} must stay near one body line: {texts:?}"
        );
    }
}

/// The document bullet's INK must sit where a body-size `·` typed on the
/// first text row would: that dot's ink center is scaled back to body
/// size and pinned to the row top. Centering the oversized line box used
/// to hang the dot at the text baseline.
#[test]
fn bullet_ink_centers_on_the_first_text_row() {
    let shapes = render_shapes("- Hola mundo\n", 1100.0, 750.0);
    let (bpos, bgalley) = galley_at(&shapes, "\u{00B7}");
    let (tpos, tgalley) = galley_at(&shapes, "Hola mundo");
    let (bink, ascent) = glyph_ink(bgalley);
    let (_, tbaseline) = glyph_ink(tgalley);
    // The scaled dot's ink rides `ascent - ink.y` above its baseline;
    // scaling it back to body size puts it at the body dot's optical
    // center on the first row's baseline.
    let expected_y = tpos.y + tbaseline - (ascent - bink.y) / BULLET_SCALE;
    let actual_y = bpos.y + bink.y;
    assert!(
        (actual_y - expected_y).abs() < 0.75,
        "bullet ink center {actual_y} must match a body-size dot at \
         {expected_y} (pos {bpos:?}, ink {bink:?})"
    );
    // Horizontally the INK sits close to the text column (the gap from
    // the text's left edge back to the dot's center).
    let actual_x = bpos.x + bink.x;
    let expected_x = tpos.x - BULLET_TEXT_GAP;
    assert!(
        (actual_x - expected_x).abs() < 0.75,
        "bullet ink center {actual_x} must hug the text column at {expected_x}"
    );
}

/// Ordered markers (`1. `) share the first text row's baseline; box
/// centering used to hang the number a few px below the text line.
#[test]
fn ordered_marker_shares_the_first_row_baseline() {
    let shapes = render_shapes("1. uno\n", 1100.0, 750.0);
    let (mpos, mgalley) = galley_at(&shapes, "1. ");
    let (tpos, tgalley) = galley_at(&shapes, "uno");
    let (_, mbaseline) = glyph_ink(mgalley);
    let (_, tbaseline) = glyph_ink(tgalley);
    let marker_baseline = mpos.y + mbaseline;
    let text_baseline = tpos.y + tbaseline;
    assert!(
        (marker_baseline - text_baseline).abs() < 0.75,
        "ordered marker baseline {marker_baseline} must match the text \
         baseline {text_baseline}"
    );
}

/// Bullet lists paint the same document bullet `·` at every depth,
/// whatever the source marker was (`-`, `*`, `+`), and the item text
/// starts a full gutter after the bullet. The literal source markers
/// must never be painted.
#[test]
fn bullets_are_document_dots_at_every_depth() {
    let (texts, _) = render_probe("- a\n  * b\n    + c\n");
    for marker in ["- ", "* ", "+ "] {
        assert!(
            !texts.iter().any(|(t, _, _)| t == marker),
            "source marker {marker:?} must not be painted: {texts:?}"
        );
    }
    // Item texts indent one structural step per nesting level.
    let ax = x_of(&texts, "a");
    let bx = x_of(&texts, "b");
    let cx = x_of(&texts, "c");
    assert!(
        (bx - ax - ITEM_INDENT).abs() < 1.0 && (cx - bx - ITEM_INDENT).abs() < 1.0,
        "nested item text must indent one step per level: {texts:?}"
    );
    // Each bullet sits left of its text, inside that item's gutter.
    for (n, needle) in ["a", "b", "c"].into_iter().enumerate() {
        let (bx, _) = bullet_at(&texts, n);
        let tx = x_of(&texts, needle);
        assert!(
            bx < tx && tx - bx <= BULLET_GUTTER + 1.0,
            "bullet must sit in the gutter before its text: {texts:?}"
        );
    }
}

/// RTL items keep the marker on the right side of the text (the flow's
/// own direction), vertically on the first row.
#[test]
fn rtl_list_marker_sits_on_the_right() {
    let (texts, _) = render_probe("- \u{0645}\u{0631}\u{062d}\u{0628}\u{0627}\n");
    let (bx, _) = bullet_at(&texts, 0);
    let tx = x_of(&texts, "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627}");
    assert!(
        bx > tx,
        "RTL bullet must sit right of the item text: {texts:?}"
    );
}

/// Nested lists must indent one step per level, and continuation lines
/// must align with their own first line (was: all markers at x=0 of the
/// item and continuations indented past the first line).
#[test]
fn nested_lists_indent_and_continuations_align() {
    let (texts, _) = render_probe("- a\n  - b\n    - c\n");
    let (x0, y0) = bullet_at(&texts, 0);
    let (x1, y1) = bullet_at(&texts, 1);
    let (x2, y2) = bullet_at(&texts, 2);
    assert!(
        y0 <= y1 && y1 <= y2,
        "bullets must paint in document order: {texts:?}"
    );
    assert!(
        x1 > x0 && x2 > x1,
        "nested markers must indent step by step: {texts:?}"
    );

    // Nested task item: checkbox + text + hard-break continuation all
    // share the same base indent; the continuation must NOT be pushed
    // another level deeper.
    let (texts, _) = render_probe("- outer\n  - [x] inner task  \n    cont\n");
    let first = x_of(&texts, "inner task");
    let cont = x_of(&texts, "cont");
    assert!(
        (first - cont).abs() < 1.0,
        "continuation must align with the first line: {texts:?}"
    );
    assert!(
        first > x_of(&texts, "outer"),
        "nested item must indent past its parent text: {texts:?}"
    );
}

/// Ordered items keep `N. ` and a stable text column; task item text
/// lands on (about) the same column, so mixed lists look aligned.
#[test]
fn ordered_and_task_item_text_columns_align() {
    let (texts, _) = render_probe("1. one\n2. two\n\n- [ ] task\n");
    let one = x_of(&texts, "one");
    let two = x_of(&texts, "two");
    assert!(
        (one - two).abs() < 1.0,
        "ordered items must share one text column: {texts:?}"
    );
    assert!(
        (one - x_of(&texts, "task")).abs() < 8.0,
        "task text must land near the ordered column: {texts:?}"
    );
}

/// An item whose first block is a quote or a nested list must draw its
/// marker at the ITEM indentation, BEFORE the nested block (was: after
/// the nested list, or inside the quote next to the quoted text).
#[test]
fn item_marker_precedes_nested_first_block() {
    let (texts, _) = render_probe("- - a\n");
    let (outer_x, outer_y) = bullet_at(&texts, 0);
    let (inner_x, inner_y) = bullet_at(&texts, 1);
    assert!(
        outer_y <= inner_y,
        "outer bullet must come before the nested list: {texts:?}"
    );
    assert!(
        outer_x < inner_x,
        "outer bullet must stay left of the nested marker: {texts:?}"
    );

    let (texts, _) = render_probe("- > quote\n");
    let (marker_x, marker_y) = bullet_at(&texts, 0);
    let quote_x = x_of(&texts, "quote");
    let quote_y = y_of(&texts, "quote");
    assert!(
        marker_x < quote_x,
        "bullet must stay left of the quote: {texts:?}"
    );
    assert!(
        marker_y <= quote_y,
        "bullet must not sit below the quote: {texts:?}"
    );
}

/// A rule under an H1/H2 inside a list item starts at the item's text
/// column, not at the item's left edge.
#[test]
fn heading_rule_inside_item_is_indented() {
    let (texts, lines) = render_probe("- # H one\n");
    let heading_x = x_of(&texts, "H one");
    assert!(!lines.is_empty(), "H1 must draw a separator");
    for (x0, x1) in &lines {
        assert!(
            *x0 >= heading_x - 2.0 && *x1 > *x0,
            "rule must align with the heading text (heading at {heading_x}): {lines:?}"
        );
    }
}
/// A `**bold**` run must PAINT on the same baseline as the text around it.
///
/// epaint lays each section out on its own font's baseline
/// (`ascent + half the line gap`): regular and bold are the SAME bundled
/// Noto Sans face at two weights, so their metrics match and the painted
/// runs share the body baseline with no `FontTweak` offset. The check
/// compares the painted ink of the same letter in all three runs.
#[test]
fn bold_runs_share_the_body_baseline() {
    // No spaces: a leading space glyph has no ink box to compare. Every run
    // starts with the same letter, so the painted ink tops are comparable.
    let shapes = render_shapes("xx**x**xx\n", 1100.0, 750.0);
    let ink_top = |needle: &str| {
        let (pos, galley) = galley_at(&shapes, needle);
        let (ink, _) = glyph_ink(galley);
        pos.y + ink.y
    };
    let regular = ink_top("xx");
    let bold = ink_top("x");
    let trailing = ink_top("xx");
    // Half a pixel of tolerance: the two faces rasterize into different
    // atlas rows, so their ink boxes can round differently.
    assert!(
        (bold - regular).abs() <= 0.5,
        "bold must paint on the body baseline (regular {regular}, bold {bold})"
    );
    assert!(
        (trailing - regular).abs() <= 0.5,
        "the rest of the line must not shift (regular {regular}, trailing {trailing})"
    );
}
