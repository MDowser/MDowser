// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer` outline end-to-end tests: the panel drawn through the real
//! `show_content` entry point (the reading sheet's own `ScrollArea`), the
//! scrollspy cursor and the responsive placement.
//!
//! Unlike `tests_find.rs`, the tabs here carry a REAL anchor index and
//! outline, built exactly like `protocol::client::fetch` does: the panel
//! lists the fetched outline and jumps through the same reserved heading ids
//! a `#id` link uses, so a default-constructed index would test nothing.

use super::show_content;
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// A loaded tab from a body carrying real anchors + outline (the same build
/// order `protocol::client::fetch` uses).
fn loaded_tab(md: &str) -> Tab {
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::build(&events);
    let outline = crate::markdown::outline::build(&events, &anchors);
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: crate::protocol::client::Fetched {
            display_url: base,
            meta: crate::markdown::frontmatter::Meta::default(),
            h1_title: crate::markdown::document::first_h1_title(&events),
            plain_text: String::new(),
            source: md.to_string(),
            events,
            anchors,
            outline,
            dests,
            stale_copy: None,
        },
    };
    tab
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))
}

/// One frame through `show_content` at `screen`, returning each painted text
/// with its top-left position (document galleys and the panel's own rows
/// alike).
fn frame(
    ctx: &egui::Context,
    screen: egui::Rect,
    tab: &mut Tab,
    store: &mut HistoryStore,
    bookmarks: &mut BookmarkStore,
    favicons: &mut FaviconCache,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(screen),
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
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
            _ => None,
        })
        .collect()
}

/// Position of the LAST painted text CONTAINING `needle`.
///
/// The panel is drawn (and paints) AFTER the document, so a title that exists
/// both as the document's heading and as a panel row resolves to the panel's
/// copy; off-screen document text is culled from the frame's shapes anyway.
fn text_at(texts: &[(String, egui::Pos2)], needle: &str) -> Option<egui::Pos2> {
    texts
        .iter()
        .filter(|(t, _)| t.contains(needle))
        .map(|(_, p)| *p)
        .next_back()
}

/// A primary press+release at `pos`, both events in one frame (a real click).
fn click(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

/// Clicking a row scrolls the matching heading to the top through the real
/// document walk: the panel only sets `tab.pending_anchor`, the same request
/// a `#id` link makes, and the heading's own render consumes it.
#[test]
fn a_row_click_scrolls_its_heading_into_view() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let lines: String = (0..200).map(|i| format!("line {i:03}\n\n")).collect();
    let md = format!("# Top\n\n{lines}## Deepmark\n\ndeepmark marker paragraph\n");
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen(),
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            events,
        )
    };
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    // Open the panel and give the `Area` its settling frames (the first is an
    // invisible disabled sizing pass).
    tab.outline.show();
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    // The marker paragraph sits below the fold, its heading too (the panel
    // row above is the ONLY painted "Deepmark").
    let before = run(&mut tab, Vec::new());
    assert!(
        text_at(&before, "deepmark marker").is_none(),
        "the marker starts below the fold"
    );
    let row = text_at(&before, "Deepmark").expect("the outline row is painted");
    // Click inside the row: the galley starts at the row's text inset, so a
    // small offset right/down of it stays inside the 26px row.
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert!(tab.outline.open, "a row click must not close the panel");
    assert!(
        tab.pending_anchor.as_deref() == Some("deepmark"),
        "the row asks for the heading's own anchor id: {:?}",
        tab.pending_anchor
    );
    // The walk consumes the request on the next frame and the sheet's scroll
    // animation settles over the following ones.
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    let y = text_at(&texts, "deepmark marker")
        .expect("the marker is scrolled into view")
        .y;
    assert!(
        (0.0..700.0).contains(&y),
        "the heading must sit at the top of the viewport: y={y}"
    );
    assert_eq!(tab.pending_anchor, None, "the walk consumed the request");
}

/// Wheel-scrolling the document moves the outline cursor section by section:
/// the spy picks the last heading that scrolled past the viewport top from
/// the rects this frame's walk recorded.
#[test]
fn the_cursor_follows_the_reader_scrolling() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let md = format!(
        "# One\n\n{}## Two\n\n{}## Three\n\n{}tail\n",
        body(150),
        body(150),
        body(150)
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen(),
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            events,
        )
    };
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    tab.outline.show();
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(
        (tab.outline.open, tab.outline.current),
        (true, 0),
        "the panel opens on the first section"
    );
    // The wheel scrolls the HOVERED area: hover the sheet, away from the
    // panel (a press or wheel over the panel belongs to its own list).
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
    // One wheel step per frame until the last section is reached: the
    // sections are several thousand px apart and one step moves a fraction
    // of that, so the cursor must PASS THROUGH the middle one (each heading
    // crosses the viewport top on its own frame).
    let mut seen = vec![tab.outline.current];
    while tab.outline.current < 2 && seen.len() < 500 {
        let _ = run(&mut tab, vec![wheel.clone()]);
        seen.push(tab.outline.current);
    }
    assert!(
        seen.contains(&1),
        "the second section became current on the way down: {seen:?}"
    );
    assert_eq!(
        tab.outline.current, 2,
        "the last section is current at the end: {seen:?}"
    );
    assert!(
        seen.windows(2).all(|w| w[0] <= w[1]),
        "scrolling down never moves the cursor back up: {seen:?}"
    );
}

/// Below `SHEET_W` the panel becomes a bottom sheet: the header jumps from
/// the top band (right-docked below the find bar) to the bottom band, where a
/// right-docked panel would cover the whole reading column.
#[test]
fn narrow_windows_get_the_bottom_sheet() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# One\n\nbody\n\n## Two\n\nbody\n\n## Three\n\nbody\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let narrow = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 700.0));
    let mut run = |tab: &mut Tab, screen: egui::Rect| {
        frame(
            &ctx,
            screen,
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            Vec::new(),
        )
    };
    for _ in 0..3 {
        let _ = run(&mut tab, narrow);
    }
    tab.outline.show();
    for _ in 0..3 {
        let _ = run(&mut tab, narrow);
    }
    let texts = run(&mut tab, narrow);
    let y = text_at(&texts, "Outline")
        .expect("the panel header is painted")
        .y;
    assert!(
        y > 350.0,
        "a narrow window docks the panel to the bottom: y={y}"
    );
    // Widening the same window docks it back under the find bar: the panel
    // is an overlay, so nothing about the document had to be re-laid out.
    for _ in 0..3 {
        let _ = run(&mut tab, screen());
    }
    let texts = run(&mut tab, screen());
    let y = text_at(&texts, "Outline")
        .expect("the panel header is painted")
        .y;
    assert!(
        y < 300.0,
        "an ordinary window docks the panel to the top-right: y={y}"
    );
}

/// Opening the panel never reflows the document: the outline is an overlay
/// (a `Middle` area), so every document shape keeps its text, position and
/// size, and only the panel's own shapes are added (mirrors
/// `tests_find.rs::find_keeps_the_document_layout_identical`).
#[test]
fn the_open_panel_never_moves_the_document() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    const MD: &str = "# Alpha\n\n\
        A first paragraph with a [link](https://ex.com/x) and `code` in it.\n\n\
        ## Beta\n\n\
        The quick brown fox jumps over the lazy dog while the clever fox watches the other animals, and a cat sat on a mat.\n\n\
        ## Gamma\n\n\
        tail paragraph\n";
    let mut tab = loaded_tab(MD);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    // Tall enough that the whole document paints at once (nothing culled).
    let tall = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 6000.0));
    let mut run = |tab: &mut Tab| -> Vec<(String, egui::Pos2, egui::Vec2)> {
        let input = egui::RawInput {
            screen_rect: Some(tall),
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
                    Some((t.galley.text().to_string(), t.pos, t.galley.size()))
                }
                _ => None,
            })
            .collect()
    };
    for _ in 0..3 {
        let _ = run(&mut tab);
    }
    let before = run(&mut tab);
    let has_outline = |texts: &[(String, egui::Pos2, egui::Vec2)]| {
        texts.iter().any(|(t, _, _)| t.contains("Outline"))
    };
    assert!(!has_outline(&before), "the closed panel paints nothing");
    tab.outline.show();
    let _ = run(&mut tab);
    let after = run(&mut tab);
    assert!(has_outline(&after), "the open panel paints its header");
    assert!(
        after.len() > before.len(),
        "the panel adds text shapes of its own"
    );
    // Every document shape pairs up with the same text, position and size:
    // the open panel only APPENDED its own shapes (drawn after the sheet).
    for (i, (bt, bp, bs)) in before.iter().enumerate() {
        let (at, ap, as_) = &after[i];
        assert_eq!(at, bt, "shape {i} text changed");
        assert!(
            (ap.x - bp.x).abs() <= 0.5 && (ap.y - bp.y).abs() <= 0.5,
            "shape {i} ({bt:?}) moved: {:?} -> {:?}",
            bp,
            ap
        );
        assert!(
            (as_.x - bs.x).abs() <= 0.5 && (as_.y - bs.y).abs() <= 0.5,
            "shape {i} ({bt:?}) resized: {bs:?} -> {as_:?}"
        );
    }
}

/// Escape and a primary press outside close the panel: the panel reads both
/// itself, so direct rendering (without the app's key router) keeps the
/// close behavior.
#[test]
fn escape_and_a_click_outside_close_the_panel() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# T\n\nshort body\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen(),
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            events,
        )
    };
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    tab.outline.show();
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    assert!(tab.outline.open);
    let _ = run(
        &mut tab,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!tab.outline.open, "Escape closes the panel");
    // Reopen, then press in the reading area (outside the panel rect).
    tab.outline.show();
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    assert!(tab.outline.open);
    let _ = run(&mut tab, click(egui::pos2(300.0, 300.0)));
    assert!(!tab.outline.open, "a press outside closes the panel");
}

/// A document without headings never keeps the panel open: the open flag can
/// outlive its list (a live re-fetch replaces the document), and an empty
/// card would be worse than nothing.
#[test]
fn a_headingless_document_closes_the_panel() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab("Just one paragraph, no headings at all.\n");
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for _ in 0..3 {
        let _ = frame(
            &ctx,
            screen(),
            &mut tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            Vec::new(),
        );
    }
    tab.outline.show();
    let texts = frame(
        &ctx,
        screen(),
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        Vec::new(),
    );
    assert!(!tab.outline.open, "the empty panel closes itself");
    assert!(
        text_at(&texts, "Outline").is_none(),
        "and paints nothing for the frame it was open"
    );
}
