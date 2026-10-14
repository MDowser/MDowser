// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::tests::*;
use super::{doc_id_salt, is_skip_link, show_markdown, Frag};
use eframe::egui;
use std::sync::Arc;

/// Footnote definitions render like numbered entries: the definition's
/// reference NUMBER and the note body share one line (`1. nota`) — the
/// marker used to sit on a line of its own with the body indented below —
/// and each note starts its own line. Numbers follow FIRST-REFERENCE order:
/// `[^b]` referenced first makes the `[^b]` definition the `1.` entry even
/// though `[^a]` is written first.
#[test]
fn footnote_markers_share_the_first_body_line() {
    let md = "Uno[^b] y [^a].\n\n[^a]: nota a\n\n[^b]: nota b\n";
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::build(&events);
    let shapes = render_shapes_with_anchors(md, 1100.0, 750.0, &anchors);
    // The marker galley is painted lower than the body line (small font), so
    // compare real BASELINES (`glyph_ink`), not galley tops.
    let baseline = |needle: &str| {
        let (pos, g) = galley_at(&shapes, needle);
        pos.y + glyph_ink(g).1
    };
    let x = |needle: &str| galley_at(&shapes, needle).0.x;

    // `[^b]` is referenced first: its definition is entry 1.
    assert!(
        (baseline("1. ") - baseline("nota b")).abs() < 0.5,
        "marker and body share the first line: {} vs {}",
        baseline("1. "),
        baseline("nota b")
    );
    assert!(
        x("1. ") < x("nota b"),
        "the number sits before the body: {} vs {}",
        x("1. "),
        x("nota b")
    );

    // `[^a]` is referenced second: entry 2, on the FIRST definition line;
    // `[^b]` (entry 1) follows it on its own line.
    assert!(
        (baseline("2. ") - baseline("nota a")).abs() < 0.5,
        "marker and body share the first line: {} vs {}",
        baseline("2. "),
        baseline("nota a")
    );
    assert!(
        baseline("nota b") > baseline("nota a"),
        "each note starts its own line: {} then {}",
        baseline("nota a"),
        baseline("nota b")
    );
}

/// A note body that cannot flow on the marker line keeps the marker on its
/// own row above the indented block, and a continuation paragraph hangs
/// under the note's text column — the marker still never gets separated
/// from content by surprise.
#[test]
fn footnote_special_and_continuation_blocks_align() {
    // First block is a list: marker row, then the indented list.
    let (texts, _) = render_probe("Uno[^a].\n\n[^a]:\n    - item uno\n");
    let marker = texts
        .iter()
        .find(|(t, _, _)| t == "1. ")
        .expect("marker rendered");
    let item = texts
        .iter()
        .find(|(t, _, _)| t == "item uno")
        .expect("list item rendered");
    assert!(
        item.2 > marker.2,
        "a block first body keeps the marker above it: {marker:?} vs {item:?}"
    );
    assert!(
        item.1 > marker.1,
        "the block is indented past the marker: {marker:?} vs {item:?}"
    );

    // A continuation paragraph starts on its own line at the text column.
    let (texts, _) = render_probe("Uno[^a].\n\n[^a]: nota uno\n\n    segundo parrafo\n");
    let body = texts
        .iter()
        .find(|(t, _, _)| t == "nota uno")
        .expect("note body rendered");
    let second = texts
        .iter()
        .find(|(t, _, _)| t == "segundo parrafo")
        .expect("continuation paragraph rendered");
    assert!(
        (body.1 - second.1).abs() < 0.5,
        "continuation blocks hang under the text column: {body:?} vs {second:?}"
    );
    assert!(
        second.2 > body.2,
        "the continuation follows the first paragraph: {body:?} then {second:?}"
    );
}

/// "Select all" (Ctrl/Cmd+A) highlights the whole document INCLUDING the
/// footnote definition's number: the marker is painted (not a label), so it
/// must draw the selection background itself.
#[test]
fn select_all_highlights_footnote_markers() {
    let md = "Uno[^a].\n\n[^a]: nota uno\n";
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::build(&events);
    let shapes = render_shapes_selected(md, 1100.0, 750.0, &anchors);
    let sel = egui::Visuals::default().selection.bg_fill;
    let marker_bg = shapes.iter().any(|s| match s {
        egui::epaint::Shape::Rect(r) => {
            // The marker's gutter only: the raised reference above and the
            // note text live further right.
            r.fill == sel && r.rect.min.x >= 4.0 && r.rect.max.x <= 26.0
        }
        _ => false,
    });
    assert!(
        marker_bg,
        "the footnote number must draw the select-all highlight"
    );
}

/// The header row of a table must render (pulldown-cmark closes
/// `TableHead` WITHOUT a `TableRow`: the cells used to be dropped).
#[test]
fn renders_table_headers() {
    let (texts, _) = render_probe("| Column A | Column B |\n|---|---|\n| 1 | 2 |\n");
    let joined: String = texts
        .iter()
        .map(|(t, _, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(joined.contains("Column A"), "header A rendered: {joined}");
    assert!(joined.contains("Column B"), "header B rendered: {joined}");
    assert!(
        joined.contains('1') && joined.contains('2'),
        "body rendered: {joined}"
    );
}

/// Headless smoke test of the recursive renderer: every shape must
/// render without panicking and produce at least one shape. Covers the
/// cases that used to break (loose lists, loose task lists, nested
/// quotes, rules inside quotes, frontmatter-like leading blocks...).
#[test]
fn renders_tricky_documents_without_panicking() {
    let docs = [
        "- a\n\n- b\n",
        "- [ ] milk\n\n- [x] bread\n",
        "- first\n\n  second paragraph\n\n- next\n",
        "> outer\n>\n> > inner\n",
        "> [!NOTE]\n> body\n",
        "> before\n>\n> ---\n>\n> after\n",
        "1. one\n\n2. two\n",
        "9. nine\n\n10. ten\n",
        "- item\n\n  ```rust\n  fn a() {}\n  ```\n\n- next\n",
        "- | not a table | x |\n\n- done\n",
        "| a | b |\n|---|---|\n| 1 | 2 |\n",
        "| a | b | c |\n| --- | --- |\n| 1 | 2 | 3 |\n",
        "---\nhello\n---\n# Body\n",
        "---\n\nRemember: it is https only\n",
        "para with `code` and [link](https://ex.com/x) and ![img](i.png)\n",
        "![only](i.png)\n",
        "text with <br> break\n",
        "Home | Docs | About\n",
        "Footnote[^1].\n\n[^1]: the note\n",
        "```\nplain\n```\n",
        "```rust\nlet a = 1;\n\nlet b = 2;\n```\n",
        "+++\nkey: v\n+++\n\nbody\n",
        "Setext\n======\n",
    ];
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    for (n, md) in docs.iter().enumerate() {
        let normalized = crate::markdown::document::normalize_body(md);
        let events = crate::markdown::document::parse_events(&normalized).expect("parse");
        let mut images = crate::media::cache::ImageCache::new();
        let mut emoji = crate::media::emoji::EmojiCache::new();
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut on_link = |_: &str| {};
                let mut links = Vec::new();
                // Inert: no network from tests (see `render_probe`).
                show_markdown(
                    ui,
                    &events,
                    // Tests use no anchors and no pending scroll target.
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &crate::protocol::rewrite::DestCache::build(
                        &base,
                        &crate::markdown::anchors::AnchorIndex::default(),
                        &events,
                    ),
                    &mut None,
                    doc_id_salt(&base),
                    false,
                    false,
                    1100.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty(), "doc {n} rendered no shapes: {md:?}");
    }
}

/// Content of an UNSELECTED `<Tab>` must not shift the ids or cached bodies
/// of the selected one. Regression: headings and code blocks were keyed by
/// RENDER ORDER, so once the user switched tabs the newly visible heading
/// inherited the sibling's anchor target and the newly visible code card
/// painted the sibling's cached body.
#[test]
fn switching_tabs_shows_that_tabs_content() {
    let md = "<Tabs>\n<Tab label=\"Uno\">\n\n# Uno heading\n\n```rust\nlet uno = 1;\n```\n\n</Tab>\n<Tab label=\"Dos\">\n\n# Dos heading\n\n```rust\nlet dos = 2;\n```\n\n</Tab>\n</Tabs>\n";
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let salt = doc_id_salt(&base);
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    // One frame with `select` as the tab choice (None = default selection).
    let mut frame = |select: Option<usize>| -> Vec<String> {
        if let Some(tab) = select {
            let labels = ["Uno".to_string(), "Dos".to_string()];
            let at = (0..events.len())
                .find(|&j| super::marker_at(&events, j).is_some_and(|(f, _)| f[0] == "tabs"))
                .expect("the tabs marker exists");
            ctx.data_mut(|d| d.insert_temp(super::tabs_state_id(salt, at, &labels), tab));
        }
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut on_link = |_: &str| {};
                let mut links = Vec::new();
                show_markdown(
                    ui,
                    &events,
                    &anchors,
                    &dests,
                    &mut None,
                    salt,
                    false,
                    false,
                    1100.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
        let shapes: Vec<egui::epaint::Shape> = out.shapes.iter().map(|c| c.shape.clone()).collect();
        probe_shapes(&shapes)
            .0
            .into_iter()
            .map(|(text, _, _)| text)
            .collect()
    };

    let first = frame(None);
    assert!(
        first.iter().any(|t| t == "let uno = 1;"),
        "tab 1's code renders first: {first:?}"
    );
    assert!(!first.iter().any(|t| t == "let dos = 2;"), "{first:?}");

    let second = frame(Some(1));
    assert!(
        second.iter().any(|t| t == "let dos = 2;"),
        "tab 2 must render ITS OWN code, never a sibling's cached body: {second:?}"
    );
    assert!(
        !second.iter().any(|t| t == "let uno = 1;"),
        "tab 1's body must be gone: {second:?}"
    );
    assert!(
        second.iter().any(|t| t == "Dos heading") && !second.iter().any(|t| t == "Uno heading"),
        "the selected tab's heading renders: {second:?}"
    );
}

/// Hostile nesting (`> > > …`, `- - - …`, `***…`) must render flattened
/// instead of recursing without bound: the renderer used to overflow the
/// UI thread's stack (an uncatchable abort) on a few KB of markdown.
#[test]
fn hostile_nesting_renders_flattened_without_overflow() {
    let quotes = format!("{} x\n", "> ".repeat(20_000));
    let (texts, _) = render_probe(&quotes);
    assert!(
        texts.iter().any(|(t, _, _)| t.contains('x')),
        "quote content survives the flatten"
    );

    let lists = format!("{} deep\n", "- ".repeat(20_000));
    // 48 levels of list rows stack well past the default window and each
    // level indents further right; give the probe room in both axes so
    // the flattened text is not clipped away.
    let (texts, _) = render_probe_sized_wh(&lists, 2400.0, 2400.0);
    let joined: String = texts
        .iter()
        .map(|(t, _, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        joined.contains("deep"),
        "list content survives the flatten ({} runs): {joined:?}",
        texts.len()
    );

    let emphasis = format!("{}x{}\n", "*".repeat(20_000), "*".repeat(20_000));
    let (texts, _) = render_probe(&emphasis);
    assert!(
        texts.iter().any(|(t, _, _)| t.contains('x')),
        "emphasis content survives the flatten"
    );
}

/// Link hitboxes feed the right-click menu ("Open link in new tab"):
/// interactive links report their screen rect and absolute URL; inert
/// (backdrop) links must not, or the menu would offer a dead action.
#[test]
fn collects_link_hitboxes_for_the_context_menu() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let events =
        crate::markdown::document::parse_events("see [docs](../docs/) now\n").expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();

    let mut interactive_hits = Vec::new();
    let mut out = ctx.run_ui(input.clone(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str| {};
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                true,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut interactive_hits,
            );
        });
    });
    out.textures_delta.clear();
    assert_eq!(interactive_hits.len(), 1, "one rendered link hitbox");
    assert_eq!(interactive_hits[0].url.as_ref(), "https://ex.com/docs/");
    assert!(
        interactive_hits[0].rect.width() > 0.0 && interactive_hits[0].rect.height() > 0.0,
        "hitbox must be a real rect: {:?}",
        interactive_hits[0].rect
    );

    let mut inert_hits = Vec::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str| {};
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut inert_hits,
            );
        });
    });
    out.textures_delta.clear();
    assert!(
        inert_hits.is_empty(),
        "inert backdrop links are not menu targets"
    );
}

/// A URL typed as plain text (`www.celticos.com`) renders as a real
/// interactive link: `markdown::autolink` gives it the same hitbox and
/// `https://` destination a markdown link gets, so the right-click menu
/// and navigation work unchanged.
#[test]
fn bare_url_renders_as_a_clickable_link() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let events =
        crate::markdown::document::parse_events("Visita www.celticos.com ahora\n").expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut clicks: Vec<String> = Vec::new();
    let mut hits = Vec::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |url: &str| clicks.push(url.to_string());
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                true,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut hits,
            );
        });
    });
    out.textures_delta.clear();
    assert_eq!(hits.len(), 1, "the bare URL is one link hitbox");
    assert_eq!(hits[0].url.as_ref(), "https://www.celticos.com");
    assert!(
        hits[0].rect.width() > 0.0 && hits[0].rect.height() > 0.0,
        "hitbox must be a real rect: {:?}",
        hits[0].rect
    );
    assert!(clicks.is_empty(), "no click was sent");
}

/// A `:::` admonition renders as a callout badge + body (badge in title
/// case: "Tip").
#[test]
fn renders_colon_container_as_callout() {
    let (texts, _) = render_probe("::: tip\nHelpful.\n:::\n");
    assert!(
        texts.iter().any(|(t, _, _)| t == "Tip"),
        "badge rendered: {texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Helpful.")),
        "body rendered: {texts:?}"
    );
}

/// An alert renders as a badge (title case: "Note"), with the raw
/// `[!NOTE]` marker removed (the parser splits the token into several
/// text events; the renderer coalesces them before detecting the marker).
#[test]
fn renders_alert_badge() {
    let (texts, _) = render_probe("> [!NOTE]\n> Body\n");
    assert!(
        texts.iter().any(|(t, _, _)| t == "Note"),
        "badge rendered: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("[!NOTE]")),
        "marker stripped: {texts:?}"
    );
}

/// Quote/alert boxes: every quote carries a FULL-width background (it
/// reaches the reading column's right edge, not just the widest line)
/// and consecutive boxes keep a visible vertical gap.
#[test]
fn quote_boxes_span_the_column_with_vertical_air() {
    let screen_w = 1100.0;
    let boxes = quote_boxes("> first quote\n\n> second quote\n", screen_w);
    assert_eq!(boxes.len(), 2, "two quote backgrounds: {boxes:?}");
    let (a, b) = (boxes[0], boxes[1]);
    assert!(
        a.width() > screen_w * 0.85,
        "quote box must span the reading column: {a:?}"
    );
    assert!(
        (a.width() - b.width()).abs() < 1.0,
        "both boxes share the column width: {a:?} vs {b:?}"
    );
    // The headless context uses egui's default 3px item spacing; the app
    // sets 8px (`theme::apply`), so the on-screen gap is larger.
    assert!(
        b.top() - a.bottom() >= 2.0,
        "quote boxes need vertical air: {a:?} vs {b:?}"
    );
}

/// Background rects of the plain-quote boxes in `md` (fill
/// `container_low`): the size filter drops the panel background, which
/// spans the whole viewport.
/// Background rects of the plain-quote boxes in `md` (fill
/// `container_low` in either theme): the size filter drops the panel
/// background, which spans the whole viewport.
fn quote_boxes(md: &str, screen_w: f32) -> Vec<egui::Rect> {
    let light = crate::ui::theme::light().container_low;
    let dark = crate::ui::theme::dark().container_low;
    render_shapes(md, screen_w, 750.0)
        .iter()
        .filter_map(|s| match s {
            egui::epaint::Shape::Rect(r) if r.fill == light || r.fill == dark => Some(r.rect),
            _ => None,
        })
        // The panel background spans the whole viewport (full height);
        // quote boxes are short.
        .filter(|r| r.width() > 200.0 && r.height() < 200.0)
        .collect()
}

/// The quote BAR takes the alert accent: the look-ahead must see the
/// marker even though `pulldown-cmark` emits it as `"["`, `"!NOTE"`,
/// `"]"` (it used to give up on the first `Text`).
#[test]
fn alert_bar_detection_handles_split_marker_events() {
    let events = crate::markdown::document::parse_events("> [!WARNING]\n> Body\n").expect("parse");
    assert_eq!(super::detect_callout_ahead(&events, 1), Some("WARNING"));
    // The `:::` container path normalizes to the same blockquote shape.
    let events = crate::markdown::document::parse_events("> plain quote\n").expect("parse");
    assert_eq!(super::detect_callout_ahead(&events, 1), None);
    // A strong opener is an inline Start, not text: no callout.
    let events = crate::markdown::document::parse_events("> **Note** body\n").expect("parse");
    assert_eq!(super::detect_callout_ahead(&events, 1), None);
}

/// One giant quoted paragraph must NOT be copied/uppercased whole by the
/// look-ahead (it runs per frame before any render cap); only the marker
/// prefix matters, so the scan is bounded to the longest marker.
#[test]
fn alert_lookahead_bounds_a_giant_quoted_paragraph() {
    let body = format!("> {}\n", "a".repeat(600_000));
    let events = crate::markdown::document::parse_events(&body).expect("parse");
    assert_eq!(super::detect_callout_ahead(&events, 1), None);
    // A marker followed by a huge body is still detected.
    let body = format!("> [!CAUTION] {}\n", "b".repeat(600_000));
    let events = crate::markdown::document::parse_events(&body).expect("parse");
    assert_eq!(super::detect_callout_ahead(&events, 1), Some("CAUTION"));
}

/// A crafted document with more blocks than the render budget must not
/// lay out every block (UI freeze) and must say so; the full text stays
/// available to Copy via `Fetched.plain_text`.
#[test]
fn block_budget_truncates_pathological_documents() {
    let n = crate::config::MAX_RENDERED_BLOCKS + 5;
    // A unique marker as the FIRST block (inside budget) and as the LAST
    // (past it): only the first may render.
    let mut md = String::from("head-marker\n\n");
    md.push_str(&"x\n\n".repeat(n));
    md.push_str("tail-marker\n");
    let (texts, _) = render_probe_sized(&md, 2_000_000.0);
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("head-marker")),
        "in-budget content renders"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("tail-marker")),
        "content past the block budget is skipped"
    );
    assert!(
        texts
            .iter()
            .any(|(t, _, _)| t.contains("more content hidden")),
        "truncation notice rendered"
    );
}

/// A wide table cannot bypass the per-frame block budget one ROW at a time:
/// one row used to spend a single block, so a 4096-column table laid out
/// every cell of every row every frame. The CELL budget caps what a table
/// may collect and render, and the cut lands right at the budget.
#[test]
fn table_cell_budget_truncates_a_wide_table() {
    let cols = 64;
    let rows = crate::config::MAX_RENDERED_TABLE_CELLS / cols + 8;
    let mut md = String::new();
    md.push_str(&"|h".repeat(cols));
    md.push_str("|\n");
    md.push_str(&"|-".repeat(cols));
    md.push_str("|\n");
    for r in 0..rows {
        md.push_str(&format!("|r{r}"));
        md.push_str(&"|c".repeat(cols - 1));
        md.push_str("|\n");
    }
    let (texts, _) = render_probe_sized(&md, 2_000_000.0);
    let row_at = |t: &str| t.strip_prefix('r').and_then(|n| n.parse::<usize>().ok());
    let last = texts
        .iter()
        .filter_map(|(t, _, _)| row_at(t))
        .max()
        .expect("the table renders its first rows");
    assert!(
        texts.iter().any(|(t, _, _)| row_at(t) == Some(0)),
        "the first row renders"
    );
    assert!(
        last + 8 <= rows,
        "the budget must cut the table before its last row: row {last} of {rows}"
    );
    assert!(
        last >= rows.saturating_sub(9),
        "the cut lands at the cell budget, not earlier: row {last} of {rows}"
    );
}

/// A code block whose lines exceed the per-document budget shows the
/// "hidden lines" notice AFTER the reserved body: the rows are painted, not
/// laid out, so the notice used to sit at the block's top and cover the
/// first visible line.
#[test]
fn hidden_code_lines_notice_sits_below_the_code() {
    let lines = crate::config::MAX_RENDERED_CODE_LINES + 3;
    let md = format!("```\n{}\n```\n", "line\n".repeat(lines));
    // A viewport tall enough to paint the whole reserved body AND the
    // notice below it (a short viewport clips the notice away, which is the
    // point of the reserved height, not of this assertion).
    let (texts, _) = render_probe_sized(&md, 260_000.0);
    let notice = texts
        .iter()
        .find(|(t, _, _)| t.contains("more line"))
        .expect("the hidden-lines notice renders");
    let lowest_row = texts
        .iter()
        .filter(|(t, _, _)| t.starts_with("line"))
        .map(|(_, _, y)| *y)
        .fold(f32::MIN, f32::max);
    assert!(
        notice.2 > lowest_row,
        "the notice sits below the painted rows: {} vs {}",
        notice.2,
        lowest_row
    );
}

/// One block's rendered text is capped: a paragraph of several MB is
/// truncated with a notice instead of becoming one giant label.
#[test]
fn oversized_blocks_are_truncated_with_a_notice() {
    use crate::config::MAX_RENDERED_BLOCK_CHARS as CAP;
    let long = "a".repeat(CAP + 1000);
    let capped = super::cap_block_text(vec![plain(&long)]);
    let text: String = capped.iter().map(|f| f.text.as_ref()).collect();
    assert!(text.contains("[truncated]"), "{text:.80}");
    assert!(
        text.chars().count() <= CAP + 32,
        "kept {} chars",
        text.chars().count()
    );
    // Under the cap nothing changes.
    let short = vec![plain("hello")];
    let kept = super::cap_block_text(short);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].text, "hello");
}

/// "Skip to content" chrome leaked by `.md` variants is dropped, not
/// rendered as the first line of every page.
#[test]
fn drops_skip_to_content_paragraphs() {
    let (texts, _) = render_probe("[Skip to content](#main-content)\n\n# Real heading\n");
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Skip")),
        "skip link must not render: {texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Real heading")),
        "document content still renders: {texts:?}"
    );
}

/// The skip-link filter only drops a leaked fragment LINK; a plain
/// paragraph that reads the same words is real content.
#[test]
fn skip_link_filter_needs_a_fragment_link() {
    // The real leak: a single link whose target does not exist (`""`).
    assert!(is_skip_link(&[inert_link("Skip to content")]));
    // A valid in-document anchor to the same phrase is also eligible.
    let anchor = Frag {
        link: Some(Arc::from("#skip-to-content")),
        ..inert_link("Skip to content")
    };
    assert!(is_skip_link(&[anchor]));
    // Plain text (no link) is content, never dropped.
    assert!(!is_skip_link(&[plain("Skip navigation")]));
    // A longer sentence is never swallowed.
    assert!(!is_skip_link(&[inert_link(
        "Skip to content and read this"
    )]));
    // Localized label without a link is content too.
    assert!(!is_skip_link(&[plain("Saltar al contenido")]));
}

/// A plain paragraph is never mistaken for a skip link, even as the very
/// first block of the document.
#[test]
fn plain_skip_navigation_paragraph_renders() {
    let (texts, _) = render_probe("Skip navigation\n\nBody\n");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Skip navigation")),
        "plain text stays: {texts:?}"
    );
}

/// Card lists flattened to one paragraph of long links stack one link
/// per row; a link split by inline HTML stays on ONE row.
#[test]
fn stacked_links_keep_each_link_on_one_row() {
    let md = "[WAF<h4>Short title</h4>Body](https://ex.com/a) \
              [A much longer link text that passes the forty character gate](https://ex.com/b)\n";
    let (texts, _) = render_probe(md);
    // Inline tags are transparent, so the first link's words are one run.
    let y_waf = y_of(&texts, "WAF Short title Body");
    let y_long = y_of(
        &texts,
        "A much longer link text that passes the forty character gate",
    );
    assert!(y_long > y_waf, "second link stacks on its own row");
}

/// Short link runs ("Docs Chat Forum") stay inline: the stack gate
/// needs at least one link with body text.
#[test]
fn short_link_runs_stay_inline() {
    let (texts, _) = render_probe("[Docs](https://a) [Chat](https://b) [Forum](https://c)\n");
    let y = y_of(&texts, "Docs");
    assert_eq!(y_of(&texts, "Chat"), y);
    assert_eq!(y_of(&texts, "Forum"), y);
}

/// A card link carrying a logo (`[![](icon) Vendor<h3>Name</h3>Body](url)`)
/// still qualifies for the stack: one card per row, logo on its own row.
#[test]
fn logo_card_links_stack_one_per_row() {
    let md = "[![](https://ex.com/a.svg) Vendor A<h3>Alpha</h3>A very long description that passes the forty character gate for stacking.](https://ex.com/a) [![](https://ex.com/b.svg) Vendor B<h3>Beta</h3>Another very long description that passes the forty character gate.](https://ex.com/b)\n";
    let (texts, _) = render_probe(md);
    let y_a = y_of_contains(&texts, "Alpha");
    let y_b = y_of_contains(&texts, "Beta");
    assert!(y_b > y_a, "second card stacks below: {texts:?}");
}

/// End-to-end: the exporter's blank-line-split card is joined by
/// `normalize_body` and then stacked, so no `[` / `](url)` leaks.
#[test]
fn exported_card_links_join_and_stack() {
    let md = "[![](https://ex.com/1.svg) Vendor A<h3>Alpha</h3>\n\nA very long description that passes the forty character gate for stacking.](https://ex.com/a) [![](https://ex.com/2.svg) Vendor B<h3>Beta</h3>\n\nAnother very long description that passes the forty character gate.](https://ex.com/b)\n";
    let (texts, _) = render_probe(md);
    let y_a = y_of_contains(&texts, "Alpha");
    let y_b = y_of_contains(&texts, "Beta");
    assert!(y_b > y_a, "second card stacks below the first: {texts:?}");
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("](")),
        "no leaked close: {texts:?}"
    );
}

/// An RTL paragraph is laid out from the right (base direction from the
/// first strong character, `viewer::bidi`): its prepared text sits
/// against the right edge of the reading column, unlike an LTR one.
#[test]
fn rtl_paragraph_is_right_aligned() {
    let (texts, _) = render_probe("Hola\n\nمرحبا بالعالم\n");
    let ltr_x = x_of(&texts, "Hola");
    // `viewer::bidi` swaps the RTL word order for the shaper.
    let rtl_x = x_of(&texts, "بالعالم مرحبا");
    assert!(ltr_x < 100.0, "LTR paragraph starts at the left: {texts:?}");
    assert!(
        rtl_x > 300.0,
        "RTL paragraph must be right-aligned: {texts:?}"
    );
}

/// A long RTL paragraph wraps into multiple lines and every line is
/// aligned from the right (`inline_flow` + egui's RTL placement).
#[test]
fn long_rtl_paragraph_wraps_and_right_aligns_lines() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 500.0),
        )),
        ..Default::default()
    };
    let md = format!("{}\n", "مرحبا بالعالم ".repeat(80));
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(&md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let galleys: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().contains(' ') => Some(t.galley.clone()),
            _ => None,
        })
        .collect();
    out.textures_delta.clear();
    assert!(!galleys.is_empty(), "paragraph galleys rendered");
    assert!(
        galleys.iter().any(|g| g.rows.len() > 1
            && g.job.halign == egui::Align::RIGHT
            && g.job.wrap.max_width.is_finite()),
        "long RTL paragraph must wrap with right-aligned lines: {:?}",
        galleys
            .iter()
            .map(|g| (g.rows.len(), g.job.halign))
            .collect::<Vec<_>>()
    );
}

/// A paragraph emoji paints an inline color image (not a text glyph),
/// while the surrounding text stays selectable text.
#[test]
fn paragraph_emoji_renders_as_image() {
    // The emoji path reserves from the process-wide texture budget; the
    // media budget tests hold the whole pool under the same lock, so a
    // parallel run would fall back to monochrome text and fail here.
    let _guard = crate::media::TEXTURE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 400.0),
        )),
        ..Default::default()
    };
    let md = "Hola 🚀 fin\n";
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut texts = Vec::new();
    let mut image_rects = Vec::new();
    for clipped in &out.shapes {
        match &clipped.shape {
            egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
            // `egui::Image` paints a textured rect; the font atlas is
            // texture 0, so any other texture is the emoji.
            egui::epaint::Shape::Rect(rect)
                if rect.fill_texture_id() != egui::TextureId::default() =>
            {
                image_rects.push(rect.rect);
            }
            _ => {}
        }
    }
    out.textures_delta.clear();
    assert!(texts.iter().any(|t| t == "Hola "), "{texts:?}");
    assert!(texts.iter().any(|t| t == " fin"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains('🚀')),
        "the emoji must not stay as text: {texts:?}"
    );
    assert_eq!(image_rects.len(), 1, "one emoji texture painted");
    let rect = image_rects[0];
    assert!(rect.width() > 10.0 && rect.width() < 30.0, "{rect:?}");
}

/// HTML comments are invisible in the rendered document, like in a
/// browser: a comment block paints neither text nor gap, an inline
/// comment leaves no fragment (`foo<!-- x -->bar` reads `foobar`), a
/// comment-only list item keeps its marker row (an empty `<li>` still
/// shows its bullet), and code keeps the literal source.
#[test]
fn html_comments_are_not_painted() {
    let (texts, _) = render_probe("Antes\n\n<!-- agent-instructions\nno leer\n-->\n\nDespués\n");
    assert!(texts.iter().any(|(t, _, _)| t == "Antes"), "{texts:?}");
    assert!(texts.iter().any(|(t, _, _)| t == "Después"), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|(t, _, _)| t.contains("agent-instructions") || t.contains("no leer")),
        "comment text is never painted: {texts:?}"
    );

    let (texts, _) = render_probe("foo<!-- oculto -->bar\n");
    assert!(
        texts.iter().any(|(t, _, _)| t == "foobar"),
        "an inline comment leaves no gap: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("oculto")),
        "inline comment text is never painted: {texts:?}"
    );

    let (texts, _) = render_probe("- item\n- <!-- note -->\n- next\n");
    assert_eq!(
        texts.iter().filter(|(t, _, _)| t == "\u{00B7}").count(),
        3,
        "every item keeps its marker: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("note")),
        "comment-only item paints no text: {texts:?}"
    );

    let (texts, _) = render_probe("```\n<!-- keep -->\n```\n");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("<!-- keep -->")),
        "code keeps the literal `<!--`: {texts:?}"
    );
}

/// `<Tabs items={["A", "B"]}>` labels its label-less `<Tab>` children with
/// the item strings (a common docs-generator convention):
/// the real labels show instead of the `Tab 1…N` fallback.
#[test]
fn tabs_items_label_the_panels() {
    let md = "<Tabs items={[\"Trace Details\", \"Sessions\"]}>\n<Tab>\n\nUno body\n\n</Tab>\n<Tab>\n\nDos body\n\n</Tab>\n</Tabs>\n";
    let (texts, _) = render_probe(md);
    assert!(
        texts.iter().any(|(t, _, _)| t == "Trace Details"),
        "{texts:?}"
    );
    assert!(texts.iter().any(|(t, _, _)| t == "Sessions"), "{texts:?}");
    assert!(
        !texts.iter().any(|(t, _, _)| t.starts_with("Tab ")),
        "no fallback labels: {texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Uno body")),
        "{texts:?}"
    );
}

/// `<Tabs>` renders a bar and only the selected panel's content (the first
/// tab by default); the hidden panel never reaches the screen.
#[test]
fn tabs_show_only_the_selected_panel() {
    let md = "<Tabs>\n<Tab label=\"First\">\nPanel one\n</Tab>\n<Tab label=\"Second\">\nPanel two\n</Tab>\n</Tabs>\n";
    let (texts, _) = render_probe(md);
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("First")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Second")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Panel one")),
        "first panel is shown: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Panel two")),
        "second panel is hidden: {texts:?}"
    );
}

/// `<Accordion>` starts collapsed: the title shows, the body does not.
#[test]
fn accordion_section_starts_collapsed() {
    let (texts, _) = render_probe("<Accordion title=\"More\">\nInside text\n</Accordion>\n");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("More")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Inside text")),
        "collapsed body is hidden: {texts:?}"
    );
}

/// Two `<Accordion>`s with the same title (here both untitled, so both fall
/// back to `Details`) keep their OWN open/closed state: the key used to be
/// the title alone, so opening or closing one toggled the other.
#[test]
fn same_titled_accordions_toggle_independently() {
    let md =
        "<Accordion>\n\nFirst body\n\n</Accordion>\n\n<Accordion>\n\nSecond body\n\n</Accordion>\n";
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let salt = doc_id_salt(&base);
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let markers: Vec<usize> = (0..events.len())
        .filter(|&j| super::marker_at(&events, j).is_some_and(|(f, _)| f[0] == "sec"))
        .collect();
    assert_eq!(markers.len(), 2, "two accordion markers");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut frame = |open_first: bool| -> Vec<String> {
        ctx.data_mut(|d| {
            d.insert_temp(
                super::section_state_id(salt, markers[0], "Details"),
                open_first,
            )
        });
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut on_link = |_: &str| {};
                let mut links = Vec::new();
                show_markdown(
                    ui,
                    &events,
                    &anchors,
                    &dests,
                    &mut None,
                    salt,
                    false,
                    false,
                    1100.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
        let shapes: Vec<egui::epaint::Shape> = out.shapes.iter().map(|c| c.shape.clone()).collect();
        probe_shapes(&shapes)
            .0
            .into_iter()
            .map(|(text, _, _)| text)
            .collect()
    };
    let collapsed = frame(false);
    assert!(
        !collapsed.iter().any(|t| t.contains("First body")),
        "a closed section hides its body"
    );
    let opened = frame(true);
    assert!(
        opened.iter().any(|t| t.contains("First body")),
        "the toggled section opens: {opened:?}"
    );
    assert!(
        !opened.iter().any(|t| t.contains("Second body")),
        "the same-titled sibling stays closed: {opened:?}"
    );
}

/// A `#fragment` whose target EXISTS but is not rendered this pass (inside
/// an unselected `<Tab>` panel here) must stay pending — it used to be
/// dropped at the end of the frame, so the request could never be applied
/// when the reader opened that tab. An unknown target is still dropped
/// instead of being retried every frame.
#[test]
fn known_but_unrendered_anchor_stays_pending() {
    let md = "<Tabs>\n<Tab label=\"Uno\">\n\n# Uno heading\n\nuno body\n\n</Tab>\n<Tab label=\"Dos\">\n\n# Dos heading\n\ndos body\n\n</Tab>\n</Tabs>\n";
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let salt = doc_id_salt(&base);
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::build(&events);
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut frame = |pending: &mut Option<String>| {
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut on_link = |_: &str| {};
                let mut links = Vec::new();
                show_markdown(
                    ui,
                    &events,
                    &anchors,
                    &dests,
                    pending,
                    salt,
                    false,
                    false,
                    1100.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
    };
    // The target exists (in the unselected panel): keep it for the frame
    // that actually renders it.
    let mut pending = Some("dos-heading".to_string());
    frame(&mut pending);
    assert_eq!(
        pending.as_deref(),
        Some("dos-heading"),
        "a known target that was not rendered this pass stays pending"
    );
    // An unknown target is not retried forever.
    let mut unknown = Some("does-not-exist".to_string());
    frame(&mut unknown);
    assert_eq!(unknown, None, "an unknown target is dropped");
}

/// `<Card>` draws its title line and body inside the box.
#[test]
fn card_renders_title_and_body() {
    let (texts, _) = render_probe("<Card title=\"Card title\">\nCard body text\n</Card>\n");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Card title")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Card body text")),
        "{texts:?}"
    );
}

/// `<Steps>` numbers each step and shows its title and body.
#[test]
fn steps_render_numbers_and_content() {
    let md = "<Steps>\n<Step title=\"Install\">\nRun it\n</Step>\n<Step title=\"Use\">\nEnjoy\n</Step>\n</Steps>\n";
    let (texts, _) = render_probe(md);
    assert!(texts.iter().any(|(t, _, _)| t.as_str() == "1"), "{texts:?}");
    assert!(texts.iter().any(|(t, _, _)| t.as_str() == "2"), "{texts:?}");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Install")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Run it")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Enjoy")),
        "{texts:?}"
    );
}

/// `<Badge>` paints a pill (a filled rounded rect) with its text.
#[test]
fn badge_renders_a_pill() {
    let shapes = render_shapes("Status: <Badge type=\"info\">New</Badge>\n", 750.0, 400.0);
    assert!(
        shapes
            .iter()
            .any(|s| matches!(s, egui::epaint::Shape::Rect(_))),
        "badge pill painted"
    );
    let (texts, _) = probe_shapes(&shapes);
    assert!(texts.iter().any(|(t, _, _)| t.contains("New")), "{texts:?}");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Status:")),
        "{texts:?}"
    );
}

/// A `<Badge>` inside a heading paints the same pill: it used to flatten
/// into plain heading text (no pill, no accent).
#[test]
fn badge_in_a_heading_renders_the_pill() {
    let shapes = render_shapes("# Status <Badge type=\"tip\">New</Badge>\n", 750.0, 400.0);
    assert!(
        shapes
            .iter()
            .any(|s| matches!(s, egui::epaint::Shape::Rect(_))),
        "heading badge pill painted"
    );
    let (texts, _) = probe_shapes(&shapes);
    assert!(texts.iter().any(|(t, _, _)| t.contains("New")), "{texts:?}");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Status")),
        "{texts:?}"
    );
}

/// `<CodeGroup>` renders its fenced children as code tabs: labels from the
/// fence `title=`, only the first block visible.
#[test]
fn codegroup_renders_code_tabs() {
    let md = "<CodeGroup>\n```js title=\"app.js\"\nconsole.log(1)\n```\n\n```py title=\"app.py\"\nprint(2)\n```\n</CodeGroup>\n";
    let (texts, _) = render_probe(md);
    assert!(texts.iter().any(|(t, _, _)| t == "app.js"), "{texts:?}");
    assert!(texts.iter().any(|(t, _, _)| t == "app.py"), "{texts:?}");
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("console.log(1)")),
        "first block is shown: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("print(2)")),
        "second block is hidden: {texts:?}"
    );
}

/// Clicking a tab switches the visible panel: the selection lives in egui's
/// temp store (document salt + labels) and survives across frames.
#[test]
fn clicking_a_tab_switches_the_panel() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let md = "<Tabs>\n<Tab label=\"First\">\nPanel one\n</Tab>\n<Tab label=\"Second\">\nPanel two\n</Tab>\n</Tabs>\n";
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut links = Vec::new();
    let mut on_link = |_: &str| {};

    let mut run = |events_in: Vec<egui::Event>| -> Vec<egui::epaint::Shape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(750.0, 750.0),
            )),
            events: events_in,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                show_markdown(
                    ui,
                    &events,
                    &anchors,
                    &dests,
                    &mut None,
                    doc_id_salt(&base),
                    false,
                    false,
                    750.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
        out.shapes.iter().map(|c| c.shape.clone()).collect()
    };

    let first = probe_shapes(&run(Vec::new())).0;
    let (x, y) = first
        .iter()
        .find(|(t, _, _)| t == "Second")
        .map(|(_, x, y)| (*x, *y))
        .expect("second tab is painted");
    let pos = egui::pos2(x, y + 8.0);
    let press = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = run(vec![egui::Event::PointerMoved(pos)]);
    let _ = run(vec![press(true)]);
    let texts = probe_shapes(&run(vec![press(false)])).0;
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Panel two")),
        "clicked panel is shown: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Panel one")),
        "the old panel is hidden: {texts:?}"
    );
}

/// Two `<Tabs>` sets with the SAME labels keep independent selections: the
/// per-instance state is keyed by the marker's event index, so clicking a
/// tab in the first list never flips a "ghost" selection in the second (a
/// reported docs-site behavior).
#[test]
fn identical_tabs_sets_keep_independent_selections() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let md = "<Tabs>\n<Tab label=\"First\">\nPanel one A\n</Tab>\n<Tab label=\"Second\">\nPanel two A\n</Tab>\n</Tabs>\n\n<Tabs>\n<Tab label=\"First\">\nPanel one B\n</Tab>\n<Tab label=\"Second\">\nPanel two B\n</Tab>\n</Tabs>\n";
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut links = Vec::new();
    let mut on_link = |_: &str| {};

    let mut run = |events_in: Vec<egui::Event>| -> Vec<egui::epaint::Shape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(750.0, 750.0),
            )),
            events: events_in,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                show_markdown(
                    ui,
                    &events,
                    &anchors,
                    &dests,
                    &mut None,
                    doc_id_salt(&base),
                    false,
                    false,
                    750.0,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
            });
        });
        out.textures_delta.clear();
        out.shapes.iter().map(|c| c.shape.clone()).collect()
    };

    let first = probe_shapes(&run(Vec::new())).0;
    // The FIRST set's "Second" label (painted first).
    let (x, y) = first
        .iter()
        .find(|(t, _, _)| t == "Second")
        .map(|(_, x, y)| (*x, *y))
        .expect("a Second label is painted");
    let pos = egui::pos2(x, y + 8.0);
    let press = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = run(vec![egui::Event::PointerMoved(pos)]);
    let _ = run(vec![press(true)]);
    let texts = probe_shapes(&run(vec![press(false)])).0;
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Panel two A")),
        "the clicked set switched: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Panel one A")),
        "the clicked set hides its old panel: {texts:?}"
    );
    assert!(
        texts.iter().any(|(t, _, _)| t.contains("Panel one B")),
        "the second set keeps its own selection: {texts:?}"
    );
    assert!(
        !texts.iter().any(|(t, _, _)| t.contains("Panel two B")),
        "no ghost switch in the second set: {texts:?}"
    );
}
