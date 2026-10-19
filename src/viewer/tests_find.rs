// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer` find end-to-end tests: opening the bar, stepping matches and
//! scrolling the current one into view through the real `show_content`
//! entry point (the reading sheet's own `ScrollArea`).

use super::show_content;
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// A loaded tab from a body (same shape as `tests.rs`'s `loaded_tab`).
fn loaded_tab(md: &str) -> Tab {
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: crate::protocol::client::Fetched {
            display_url: base,
            meta: crate::markdown::frontmatter::Meta::default(),
            h1_title: crate::markdown::document::first_h1_title(&events),
            plain_text: String::new(),
            events,
            anchors,
            dests,
            stale_copy: None,
        },
    };
    tab
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))
}

/// One frame through `show_content`, returning each painted text with its y.
/// Held modifiers arrive as `Event::ModifiersChanged` (the platform sends
/// one when the state changes; `RawInput` carries no modifier field).
fn frame(
    ctx: &egui::Context,
    tab: &mut Tab,
    store: &mut HistoryStore,
    bookmarks: &mut BookmarkStore,
    favicons: &mut FaviconCache,
    events: Vec<egui::Event>,
) -> Vec<(String, f32)> {
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        show_content(root, tab, store, bookmarks, favicons);
    });
    out.textures_delta.clear();
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.y)),
            _ => None,
        })
        .collect()
}

fn enter(modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// The y of the first painted text CONTAINING `needle`, when visible.
///
/// With find active a matching paragraph is split into pieces (the match
/// and the gap around it), so the search matches by containment;
/// off-screen text is CULLED from the frame's shapes, which is what the
/// scroll tests assert around.
fn text_y(texts: &[(String, f32)], needle: &str) -> Option<f32> {
    texts
        .iter()
        .find(|(t, _)| t.contains(needle))
        .map(|(_, y)| *y)
}

/// Typing a query whose match sits BELOW the fold scrolls the sheet down to
/// the match (the current one must end up painted on screen).
#[test]
fn a_match_below_the_fold_scrolls_into_view() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let lines: String = (0..200).map(|i| format!("line {i:03}\n\n")).collect();
    let md = format!("# Title\n\n{lines}tailmark paragraph\n");
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(&ctx, tab, &mut store, &mut bookmarks, &mut favicons, events)
    };
    // Settle: the sheet starts at the top, the tail mark is below the fold
    // (culled from the frame's shapes).
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    let before = run(&mut tab, Vec::new());
    assert!(
        text_y(&before, "tailmark paragraph").is_none(),
        "the mark starts below the fold"
    );
    // Type the query (frame N: it changed -> scroll requested; the sheet's
    // animation settles over the following frames).
    tab.find.show(tab.doc_generation);
    tab.find.query = "tailmark".to_string();
    tab.find.scroll_to_current = true;
    let mut texts = run(&mut tab, Vec::new());
    for _ in 0..40 {
        texts = run(&mut tab, Vec::new());
    }
    // The GAP piece after the match (" paragraph") can only come from the
    // document's own paragraph: the bar's field paints the query alone, so
    // this cannot be satisfied by the bar itself.
    let y = text_y(&texts, " paragraph").expect("the match is scrolled into view");
    assert!(
        (0.0..700.0).contains(&y),
        "the match must be scrolled into view: y={y}"
    );
}

/// A match ABOVE the fold also scrolls into view (upward).
#[test]
fn a_match_above_the_fold_scrolls_into_view() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let lines: String = (0..200).map(|i| format!("line {i:03}\n\n")).collect();
    let md = format!("# Headmark title\n\n{lines}");
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(&ctx, tab, &mut store, &mut bookmarks, &mut favicons, events)
    };
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    // Wheel-scroll down until the heading is far above the top edge. The
    // pointer must hover the sheet first (the wheel scrolls the hovered
    // area), like the app's own scroll test does.
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(450.0, 350.0))],
    );
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -200.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..30 {
        let _ = run(&mut tab, vec![wheel.clone()]);
    }
    assert!(
        text_y(&run(&mut tab, Vec::new()), "Headmark title").is_none(),
        "the heading must be scrolled off the top after the wheel"
    );
    tab.find.show(tab.doc_generation);
    tab.find.query = "headmark".to_string();
    tab.find.scroll_to_current = true;
    let mut texts = run(&mut tab, Vec::new());
    for _ in 0..40 {
        texts = run(&mut tab, Vec::new());
    }
    // Same trick as the below-the-fold test: the GAP piece " title" proves
    // the document's own heading is painted (the bar's field shows only the
    // query).
    let y = text_y(&texts, " title").expect("the match is scrolled back into view");
    assert!(
        (0.0..700.0).contains(&y),
        "the match must be scrolled into view upward: y={y}"
    );
}

/// `Enter` in the focused query field steps to the next match (the field
/// keeps the keyboard, like a browser's find box) and `Shift+Enter` steps
/// back to it.
#[test]
fn enter_steps_forward_and_shift_enter_back() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# T\n\nterm one\n\nmiddle\n\nterm two\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for _ in 0..2 {
        let _ = frame(
            &ctx,
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            vec![],
        );
    }
    // Open the bar and type: on the bar's first frame (the `Area` sizing
    // pass) the focus request is deferred, so give it a frame to land, then
    // one more for the count.
    tab.find.show(tab.doc_generation);
    tab.find.query = "term".to_string();
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![],
    );
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![],
    );
    assert_eq!(tab.find.total, 2, "both occurrences counted");
    assert_eq!(tab.find.current, 0, "the first match is current");
    // Enter steps forward.
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![enter(egui::Modifiers::NONE)],
    );
    assert_eq!(tab.find.current, 1, "Enter steps to the second match");
    // Shift+Enter steps back: the held Shift arrives as a modifier-state
    // event (the platform sends one when the key state changes).
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![
            egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
            enter(egui::Modifiers::SHIFT),
        ],
    );
    assert_eq!(tab.find.current, 0, "Shift+Enter steps back");
    // The keyboard stays in the field: a second Enter without re-clicking
    // steps again.
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![
            egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            enter(egui::Modifiers::NONE),
        ],
    );
    assert_eq!(tab.find.current, 1, "the field keeps the keyboard");
}

/// The bar's chevron buttons step matches too (pointer path), and the
/// count/current match stay consistent.
#[test]
fn chevron_buttons_step_matches() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# T\n\nterm one\n\nmiddle\n\nterm two\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(&ctx, tab, &mut store, &mut bookmarks, &mut favicons, events)
    };
    for _ in 0..2 {
        let _ = run(&mut tab, Vec::new());
    }
    tab.find.show(tab.doc_generation);
    tab.find.query = "term".to_string();
    let _ = run(&mut tab, Vec::new());
    let texts = run(&mut tab, Vec::new());
    assert_eq!(tab.find.total, 2);
    // Click the down chevron: find its rect from the painted glyphs is not
    // possible (vectors), so click the position the layout gives it: the
    // buttons sit right of the count, which follows the capsule.
    // The count text `1/2` is painted: use its position to locate the row.
    let (_, count_y) = texts
        .iter()
        .find(|(t, _)| t.contains("/2"))
        .cloned()
        .unwrap_or_else(|| panic!("count painted: {texts:?}"));
    // The bar is anchored to the sheet's right edge: the row is capsule,
    // count, previous, next, close, so the NEXT chevron's center sits a
    // fixed offset left of the right edge (12 anchor + 6 frame margin +
    // 32 close + 4 spacing + half of 32).
    let down_x = screen().width() - 12.0 - 6.0 - 32.0 - 4.0 - 16.0;
    let down = egui::Event::PointerButton {
        pos: egui::pos2(down_x, count_y + 8.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    let up = egui::Event::PointerButton {
        pos: egui::pos2(down_x, count_y + 8.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = run(&mut tab, vec![down, up]);
    let _ = run(&mut tab, Vec::new());
    assert_eq!(tab.find.current, 1, "the chevron steps to the next match");
}

/// Searching must not change the LAYOUT, for ANY query shape: the split
/// paints one unsplit galley with the washes behind the glyphs. A query of a
/// single SPACE matches almost everywhere (the worst case: every space was
/// its own widget before, and its own shaping SECTION after) and a phrase
/// query wraps across matches.
#[test]
fn find_keeps_the_document_layout_identical() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    // The real-world shape that moved: paragraphs with inline code spans,
    // links and long wrapping lines (acceptmarkdown.com/guides).
    const MD: &str = "# Title\n\n\
        - [Set the Vary: Accept header](https://ex.com/vary) — Without `Vary: Accept`, CDNs and browser caches will happily serve the wrong representation to the wrong audience.\n\
        - [Parsing Accept & quality values](https://ex.com/parse) — Ranking types by q-value, breaking ties by specificity, and respecting `q=0`. The parsing rules every content-negotiating server needs to get right.\n\
        - [Returning 406 Not Acceptable](https://ex.com/406) — When an `Accept` header genuinely can't be satisfied, `406` is the right response. The common mistake is returning it too eagerly.\n\n\
        The quick brown fox jumps over the lazy dog while the clever fox watches the other animals. A cat and a dog sat on a mat; a nap followed a while later.\n\n\
        tailmark paragraph here\n";
    for (width, query) in [
        (900.0_f32, "e"),
        (700.0, " "),
        (700.0, "a "),
        (900.0, "the"),
    ] {
        let mut tab = loaded_tab(MD);
        let mut store = HistoryStore::in_memory();
        let mut bookmarks = BookmarkStore::in_memory();
        let mut favicons = FaviconCache::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 6000.0));
        let mut run = |tab: &mut Tab| -> Vec<(String, f32, f32)> {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                show_content(root, tab, &mut store, &mut bookmarks, &mut favicons);
            });
            out.textures_delta.clear();
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::epaint::Shape::Text(t) => {
                        Some((t.galley.text().to_string(), t.pos.y, t.galley.size().y))
                    }
                    _ => None,
                })
                .collect()
        };
        for _ in 0..3 {
            let _ = run(&mut tab);
        }
        let before = run(&mut tab);
        tab.find.open = true;
        tab.find.query = query.to_string();
        tab.find.scroll_to_current = false;
        let _ = run(&mut tab);
        let after = run(&mut tab);
        assert!(
            tab.find.total >= 4,
            "{query:?}: the query must match several times: {}",
            tab.find.total
        );
        // Every shape pairs up with the same text, y and height: the layout
        // is byte-identical (the searched render only adds the bar's own
        // shapes).
        // The bar's own two texts (query field + count) are in the
        // Foreground layer, so they come AFTER the document's shapes.
        assert_eq!(
            after.len(),
            before.len() + 2,
            "{query:?}: only the bar's two texts may be added"
        );
        for (i, (bt, by, bh)) in before.iter().enumerate() {
            let (at, ay, ah) = &after[i];
            assert_eq!(at, bt, "{query:?}: shape {i} text changed");
            assert!(
                (ay - by).abs() <= 0.5 && (ah - bh).abs() <= 0.5,
                "{query:?}: shape {i} ({bt:?}) moved/resized: y {by} -> {ay}, h {bh} -> {ah}"
            );
        }
    }
}

/// Reopening the bar (or pressing Ctrl/Cmd+F while it is open) focuses the
/// query field and SELECTS the previous query, so typing replaces it.
#[test]
fn reopening_selects_the_previous_query() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# T\n\nterm one\n\nterm two\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for _ in 0..2 {
        let _ = frame(
            &ctx,
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            vec![],
        );
    }
    let edit_id = egui::Id::new(("find-query", tab.id));
    // Open + type, close, reopen.
    tab.find.show(tab.doc_generation);
    tab.find.query = "term".to_string();
    for _ in 0..3 {
        let _ = frame(
            &ctx,
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            vec![],
        );
    }
    // Esc closes (the bar consumes it).
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!tab.find.open, "Esc closed the bar");
    // Reopen: the field takes focus and the kept query comes up selected.
    tab.find.show(tab.doc_generation);
    for _ in 0..3 {
        let _ = frame(
            &ctx,
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            vec![],
        );
    }
    let state = egui::TextEdit::load_state(&ctx, edit_id).expect("field state");
    let range = state
        .cursor
        .char_range()
        .expect("the query is selected on reopen");
    let (a, b) = (range.primary.index.0, range.secondary.index.0);
    assert_eq!(
        (a.min(b), a.max(b)),
        (0, 4),
        "the whole previous query is selected"
    );
    assert!(
        ctx.memory(|m| m.has_focus(edit_id)),
        "the field owns the keyboard"
    );
    // Typing one character REPLACES the selection (the user-visible effect).
    let _ = frame(
        &ctx,
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        vec![egui::Event::Text("x".to_string())],
    );
    assert_eq!(tab.find.query, "x", "typing replaced the selected query");
    // Pressing Ctrl/Cmd+F while the bar is ALREADY open selects the query
    // again (the same `show` the shortcut calls).
    tab.find.show(tab.doc_generation);
    for _ in 0..3 {
        let _ = frame(
            &ctx,
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            vec![],
        );
    }
    let state = egui::TextEdit::load_state(&ctx, edit_id).expect("field state");
    let range = state.cursor.char_range().expect("re-selected");
    let (a, b) = (range.primary.index.0, range.secondary.index.0);
    assert_eq!(
        (a.min(b), a.max(b)),
        (0, 1),
        "the query comes up selected again"
    );
}
