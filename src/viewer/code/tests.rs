// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{block_for, line_count, render_body, visible_body};
use crate::markdown::highlight as hl;
use eframe::egui;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn trailing_newline_is_not_a_line() {
    // pulldown-cmark includes the fenced block's trailing `\n`.
    assert_eq!(visible_body("let a = 1;\n"), "let a = 1;");
    assert_eq!(visible_body("a\n\nb\n"), "a\n\nb");
    // No trailing newline: unchanged.
    assert_eq!(visible_body("a\n\nb"), "a\n\nb");
    // Blank lines in the middle are real lines.
    assert_eq!(line_count(visible_body("a\n\nb\n")), 3);
    assert_eq!(line_count(visible_body("")), 1);
}

/// The per-document cache is keyed by (document, index, language): a hit
/// still consumes the event stream and returns the same block.
#[test]
fn block_for_caches_and_still_consumes_events() {
    let events: Vec<Event<'static>> = Parser::new("```rust\nlet a = 1;\n```\n")
        .map(|e| e.into_static())
        .collect();
    let start = events
        .iter()
        .position(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
        .expect("code block");
    let mut first_i = start + 1;
    let first = block_for(&events, &mut first_i, hl::Lang::Rust, 7, 0, true);
    assert!(
        matches!(events[first_i], Event::End(TagEnd::CodeBlock)),
        "must stop ON the closing End"
    );
    assert_eq!(first.body, "let a = 1;");
    assert!(
        first.lines.borrow().is_some(),
        "rust block is tokenized when its rows are shown"
    );

    let mut second_i = start + 1;
    let second = block_for(&events, &mut second_i, hl::Lang::Rust, 7, 0, true);
    assert_eq!(second_i, first_i, "cache hit advances the same way");
    assert!(
        Rc::ptr_eq(&first, &second),
        "second call must hit the cache"
    );
}

/// Regression: the old fixed 64-entry LRU evicted each block before the
/// next frame asked for it again (sequential-scan thrash), so a document
/// with more blocks re-copied and re-tokenized everything every frame.
/// Every block a document can render now stays cached while the byte
/// budget holds.
#[test]
fn many_blocks_stay_cached_between_frames() {
    const N: usize = 200;
    let salt = 0xfeed_0001_u64;
    let parse = |_idx: usize| -> Vec<Event<'static>> {
        Parser::new("```rust\nlet a = 1;\n```\n")
            .map(|e| e.into_static())
            .collect()
    };
    let mut first = Vec::with_capacity(N);
    for idx in 0..N {
        let events = parse(idx);
        let start = events
            .iter()
            .position(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
            .expect("code block");
        let mut i = start + 1;
        first.push(block_for(&events, &mut i, hl::Lang::Rust, salt, idx, true));
    }
    for (idx, cached) in first.iter().enumerate() {
        let events = parse(idx);
        let start = events
            .iter()
            .position(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
            .expect("code block");
        let mut i = start + 1;
        let again = block_for(&events, &mut i, hl::Lang::Rust, salt, idx, true);
        assert!(
            Rc::ptr_eq(cached, &again),
            "block {idx} must stay cached across frames"
        );
    }
}

/// Past the per-document line budget nothing renders: the body is still
/// collected (to consume the event stream) but tokenization is skipped.
#[test]
fn past_budget_blocks_skip_tokenization() {
    let events: Vec<Event<'static>> = Parser::new("```rust\nlet a = 1;\n```\n")
        .map(|e| e.into_static())
        .collect();
    let start = events
        .iter()
        .position(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
        .expect("code block");
    let mut i = start + 1;
    let block = block_for(&events, &mut i, hl::Lang::Rust, 0xfeed_0002, 0, false);
    assert_eq!(block.body, "let a = 1;");
    assert!(
        block.lines.borrow().is_none(),
        "no spans built past the budget"
    );
    // A later frame that can show the block fills them lazily: the block
    // used to render monochrome forever after one budget-starved frame.
    assert!(
        block.ensure_spans(hl::Lang::Rust, true).is_some(),
        "a frame with budget left tokenizes the cached body"
    );
}

/// The highlighted lines are laid out ONCE and reused across frames; a
/// selection (or theme) change rebuilds them. Regression guard for the
/// per-frame `LayoutJob` construction that copied every span.
#[test]
fn code_galleys_are_reused_and_rebuilt_on_selection() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let events: Vec<Event<'static>> = Parser::new("```rust\nlet a = 1;\nlet b = 2;\n```\n")
        .map(|e| e.into_static())
        .collect();
    let start = events
        .iter()
        .position(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
        .expect("code block");
    let mut i = start + 1;
    let block = block_for(&events, &mut i, hl::Lang::Rust, 0xfeed_0003, 0, true);
    let mut first: Option<Arc<egui::Galley>> = None;
    let mut second: Option<Arc<egui::Galley>> = None;
    let mut selected_first: Option<Arc<egui::Galley>> = None;
    for frame in 0..3 {
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut budget = 100;
                render_body(ui, &block, hl::Lang::Rust, frame == 2, &mut budget);
                let galley = block
                    .galleys
                    .borrow()
                    .lines
                    .first()
                    .cloned()
                    .expect("line laid out");
                match frame {
                    0 => first = Some(galley),
                    1 => second = Some(galley),
                    _ => selected_first = Some(galley),
                }
            });
        });
        out.textures_delta.clear();
    }
    let first = first.expect("frame 1 laid out");
    let second = second.expect("frame 2 laid out");
    let selected_first = selected_first.expect("selected frame laid out");
    assert!(
        Arc::ptr_eq(&first, &second),
        "same theme/selection must reuse the laid-out galley"
    );
    assert!(
        !Arc::ptr_eq(&first, &selected_first),
        "select-all changes the formats: the galley must rebuild"
    );
}

/// The visible-row range must be computed in WINDOW-LOCAL coordinates:
/// `ViewportInfo::inner_rect` is in MONITOR space (winit reports the window
/// position there), so intersecting it with the local clip rect shifted the
/// bound by the window's position on screen. As a code block scrolled up,
/// that culled the rows near the top earlier than the clip edge, growing a
/// blank band at the top of the block.
#[test]
fn code_viewport_ignores_the_monitor_space_window_rect() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let mut raw = egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    // A window sitting at monitor position (100, 300): what egui-winit
    // reports as `inner_rect`, in monitor space.
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 300.0),
                screen.size(),
            )),
            ..Default::default()
        },
    );
    let mut range = 0..0;
    let mut out = ctx.run_ui(raw, |root| {
        // The code body scrolled 2000px up: its content origin is above the
        // window, 200 rows of 20px.
        let content =
            egui::Rect::from_min_size(egui::pos2(70.0, -2000.0), egui::vec2(760.0, 200.0 * 20.0));
        root.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            range = super::code_viewport(ui, 200, 20.0);
        });
    });
    out.textures_delta.clear();
    // The viewport starts at local y=0 (the window top), so the first
    // visible row is floor((0 - (-2000)) / 20) = 100. The monitor-space
    // window rect would make it 115 (a 15-row blank band).
    assert_eq!(range.start, 100, "row culling must use local coordinates");
    // The range must cover the visible height: 100 + ceil(700/20) + 1.
    assert!(range.end >= 135, "visible range too short: {range:?}");
}

/// Plain (huge/unsupported) code rows are painted at their ABSOLUTE row
/// offset: the content origin already carries the scroll translation, so a
/// relative offset (`row - visible.start`) painted the visible rows at the
/// block's TOP and left a scrolled viewport blank.
#[test]
fn plain_lines_paint_at_their_absolute_rows() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body: String = (0..200).map(|i| format!("line {i:03}\n")).collect();
    let body = body.trim_end();
    let block = plain_test_block(body);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            // Scrolled 2000px: the content origin is above the window.
            let content = egui::Rect::from_min_size(
                egui::pos2(70.0, -2000.0),
                egui::vec2(760.0, 200.0 * 20.0),
            );
            root.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                super::render_plain_lines(ui, &block, false, 100..110, 20.0);
            });
        },
    );
    let rows: Vec<(String, f32)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("line ") => {
                Some((t.galley.text().to_string(), t.pos.y))
            }
            _ => None,
        })
        .collect();
    out.textures_delta.clear();
    let y_of = |name: &str| {
        rows.iter()
            .find(|(t, _)| t == name)
            .map(|(_, y)| *y)
            .unwrap_or_else(|| panic!("{name} not painted: {rows:?}"))
    };
    assert_eq!(y_of("line 100"), 0.0, "row 100 at its absolute offset");
    assert_eq!(y_of("line 109"), 180.0, "row 109 at its absolute offset");
    assert_eq!(rows.len(), 10, "only the visible rows are built");
}

/// The plain path's content width is MONOTONIC: once a long row was
/// measured, scrolling it out of view must not shrink the returned width
/// (egui clamps the horizontal offset to the content width, which snapped
/// a scrolled-right log back to column 0).
#[test]
fn plain_lines_keep_the_widest_row_width() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let short: String = (0..20).map(|i| format!("line {i:03}\n")).collect();
    let body = format!(
        "{}\na very long line {}\n",
        short.trim_end(),
        "x".repeat(200)
    );
    let block = plain_test_block(&body);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            let content =
                egui::Rect::from_min_size(egui::pos2(70.0, 0.0), egui::vec2(760.0, 21.0 * 20.0));
            root.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                // Only the short rows are visible first.
                let before = super::render_plain_lines(ui, &block, false, 0..10, 20.0);
                // The long row (the last one) becomes visible once...
                let long = super::render_plain_lines(ui, &block, false, 20..21, 20.0);
                assert!(long > before, "the long row widens the content");
                // ...and scrolling it away keeps the width.
                let after = super::render_plain_lines(ui, &block, false, 0..10, 20.0);
                assert_eq!(after, long, "the widest-row memory must be monotonic");
            });
        },
    );
    out.textures_delta.clear();
}

/// A bare [`super::CachedBlock`] wrapping `body`, for the plain-path tests.
fn plain_test_block(body: &str) -> super::CachedBlock {
    super::CachedBlock {
        body: body.to_string(),
        line_count: super::line_count(body),
        tokenizable: false,
        lines: std::cell::RefCell::new(None),
        galleys: std::cell::RefCell::new(super::Galleys::default()),
        plain_max_w: std::cell::Cell::new(0.0),
        galley_bytes: std::cell::Cell::new(0),
        bytes: body.len(),
        used: std::cell::Cell::new(0),
    }
}
