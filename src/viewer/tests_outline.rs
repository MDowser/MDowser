// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer` outline end-to-end tests: the always-visible sidebar drawn
//! through the real `show_content` entry point (the reading sheet's own
//! `ScrollArea`), wrapped rows, the scrollspy cursor, page-end selection and
//! the responsive width.
//!
//! Unlike `tests_find.rs`, the tabs here carry a REAL anchor index and
//! outline, built exactly like `protocol::client::fetch` does: the sidebar
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

/// Whether the outline cursor names a heading the last walk rendered (the
/// sidebar only lights that row).
fn current_outline_row_lit(tab: &Tab) -> bool {
    match &tab.state {
        TabState::Loaded { fetched } => tab.outline.current_is_rendered(&fetched.outline),
        _ => false,
    }
}

/// Left edge of the sidebar on `screen`.
fn panel_left(screen: egui::Rect) -> f32 {
    screen.right() - super::outline_panel::panel_width(screen.width())
}

/// One frame through `show_content` at `screen`, returning each painted text
/// with its top-left position and size (document galleys and the sidebar's
/// own rows alike).
fn frame(
    ctx: &egui::Context,
    screen: egui::Rect,
    tab: &mut Tab,
    store: &mut HistoryStore,
    bookmarks: &mut BookmarkStore,
    favicons: &mut FaviconCache,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2, egui::Vec2)> {
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
            egui::epaint::Shape::Text(t) => {
                Some((t.galley.text().to_string(), t.pos, t.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// Position of the LAST sidebar text CONTAINING `needle`. The sidebar is
/// drawn BEFORE the sheet, so a title that also appears in the document must
/// be filtered by x: the sheet never reaches the sidebar's columns.
fn panel_text_at(
    texts: &[(String, egui::Pos2, egui::Vec2)],
    needle: &str,
    screen: egui::Rect,
) -> Option<egui::Pos2> {
    let left = panel_left(screen);
    texts
        .iter()
        .filter(|(t, p, _)| p.x >= left && t.contains(needle))
        .map(|(_, p, _)| *p)
        .next_back()
}

/// Position of the first document text CONTAINING `needle` (left of the
/// sidebar).
fn document_text_at(
    texts: &[(String, egui::Pos2, egui::Vec2)],
    needle: &str,
    screen: egui::Rect,
) -> Option<egui::Pos2> {
    let left = panel_left(screen);
    texts
        .iter()
        .filter(|(t, p, _)| p.x < left && t.contains(needle))
        .map(|(_, p, _)| *p)
        .next()
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

fn wheel(delta_y: f32) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, delta_y),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The sidebar is present with no toggle and no context menu, stays on the
/// right, and wraps a long title over several lines instead of eliding it.
#[test]
fn the_sidebar_is_always_visible_and_wraps_titles() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# Top\n\nbody\n\n\
        ### A deliberately long heading that has to wrap several times in a narrow sidebar because it keeps going\n\n\
        body\n\n## Tail\n\nbody\n";
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
    let texts = run(&mut tab, Vec::new());
    let left = panel_left(screen());
    let header = panel_text_at(&texts, "Outline", screen()).expect("sidebar header painted");
    assert!(header.x >= left, "header at the sidebar's left edge");
    assert!(header.y < 80.0, "the sidebar starts at the top");
    // No close button: the body has no dismissible-surface chrome.
    assert!(
        panel_text_at(&texts, "Close", screen()).is_none(),
        "the permanent sidebar has no close control"
    );
    // The long title is painted by the sidebar (the doc heading is below the
    // fold) and its row is taller than one line: the next row starts more
    // than a single line below it.
    let long = panel_text_at(&texts, "deliberately long", screen())
        .expect("the long title row is painted");
    let tail = panel_text_at(&texts, "Tail", screen()).expect("the next row is painted");
    assert!(
        tail.y - long.y > 34.0,
        "the long title takes several wrapped lines: long {long:?}, tail {tail:?}"
    );
    assert!(
        long.x > tail.x,
        "the deeper heading keeps its indent step: long {long:?}, tail {tail:?}"
    );
    // The document keeps its own column left of the sidebar.
    for (text, pos, size) in &texts {
        if text.contains("body") {
            assert!(
                pos.x + size.x <= left + 0.5,
                "document text {text:?} runs under the sidebar: {pos:?}+{size:?}"
            );
        }
    }
}

/// Clicking a sidebar row scrolls the matching heading through the real
/// document walk: the sidebar only sets `tab.pending_anchor`, the same
/// request a `#id` link makes, and the heading's own render consumes it.
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
    // The marker paragraph sits below the fold, its heading too (the sidebar
    // row above is the ONLY painted "Deepmark").
    let before = run(&mut tab, Vec::new());
    assert!(
        document_text_at(&before, "deepmark marker", screen()).is_none(),
        "the marker starts below the fold"
    );
    let row = panel_text_at(&before, "Deepmark", screen()).expect("the outline row is painted");
    // Click inside the row: the galley starts at the row's text inset, so a
    // small offset right/down of it stays inside the row.
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(
        tab.outline.current, 1,
        "the clicked row is selected at once"
    );
    // The sidebar sets the same anchor request as a `#id` link and this
    // frame's document walk consumes it; the sheet's scroll animation settles
    // over the following ones.
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    let y = document_text_at(&texts, "deepmark marker", screen())
        .expect("the marker is scrolled into view")
        .y;
    assert!(
        (0.0..700.0).contains(&y),
        "the heading must sit at the top of the viewport: y={y}"
    );
    assert_eq!(tab.pending_anchor, None, "the walk consumed the request");
}

/// A collapsed section still contributes its headings to the outline. The
/// first such heading must not take the cursor while it is hidden: the reader
/// is above every rendered heading, and that row can never be reached by
/// scrolling.
#[test]
fn a_collapsed_heading_never_takes_the_cursor() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "<Accordion title=\"More\">\n\n# Hidden section\n\nhidden body\n\n</Accordion>\n\n# Visible section\n\nvisible body\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab| {
        frame(
            &ctx,
            screen(),
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            Vec::new(),
        )
    };
    for _ in 0..3 {
        let _ = run(&mut tab);
    }
    assert_eq!(
        tab.outline.current, 1,
        "the cursor points at the first section that actually renders"
    );
    assert!(
        current_outline_row_lit(&tab),
        "the rendered section's row is the one lit"
    );
}

/// A page whose only headings live in a closed section has no row to light:
/// the internal cursor stays at the first item, but the sidebar must not
/// claim a section that is not on the page. Opening the section gives the
/// cursor a rendered heading and the row lights again.
#[test]
fn a_page_with_only_hidden_headings_lights_no_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "<Accordion title=\"More\">\n\n# Hidden section\n\nhidden body\n\n</Accordion>\n";
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
    let before = run(&mut tab, Vec::new());
    assert!(
        !current_outline_row_lit(&tab),
        "a hidden heading lights no row"
    );
    let header = document_text_at(&before, "More", screen()).expect("accordion header");
    let _ = run(&mut tab, click(header + egui::vec2(20.0, 8.0)));
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    assert!(
        current_outline_row_lit(&tab),
        "the now-rendered heading gives the cursor a row to light"
    );
}

/// Clicking an outline row whose heading lives in a closed `<Accordion>` must
/// reveal it: the click opens the section and the same anchor request scrolls
/// the heading into view. It used to select the row, leave the content
/// collapsed and keep the request pending forever.
#[test]
fn a_row_click_opens_a_closed_accordion_and_scrolls_to_its_heading() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let filler: String = (0..140).map(|i| format!("filler {i:03}\n\n")).collect();
    let md = format!(
        "# Top\n\n{filler}<Accordion title=\"More\">\n\n## Hidden section\n\nhiddenmark paragraph\n\n</Accordion>\n\n{filler}"
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
    let before = run(&mut tab, Vec::new());
    assert!(
        document_text_at(&before, "hiddenmark", screen()).is_none(),
        "the collapsed section hides its body"
    );
    let row = panel_text_at(&before, "Hidden section", screen()).expect("outline row");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    let y = document_text_at(&texts, "hiddenmark", screen())
        .expect("the click opened the accordion and scrolled its heading into view")
        .y;
    assert!(
        (0.0..700.0).contains(&y),
        "the revealed heading stays in the viewport: y={y}"
    );
    assert_eq!(
        tab.pending_anchor, None,
        "the revealed heading consumed the anchor request"
    );
}

/// The same for an unselected `<Tabs>` panel: the row click selects the tab
/// that holds the heading before scrolling, instead of silently doing nothing
/// while the request stays pending.
#[test]
fn a_row_click_selects_the_tab_that_holds_the_heading() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let filler: String = (0..140).map(|i| format!("filler {i:03}\n\n")).collect();
    let md = format!(
        "# Top\n\n{filler}<Tabs>\n<Tab label=\"First\">\n\n## First section\n\nfirstmark\n\n</Tab>\n<Tab label=\"Second\">\n\n{filler}## Second section\n\nsecondmark paragraph\n\n</Tab>\n</Tabs>\n\n{filler}"
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
    let before = run(&mut tab, Vec::new());
    assert!(
        document_text_at(&before, "secondmark", screen()).is_none(),
        "the unselected tab hides its body"
    );
    let row = panel_text_at(&before, "Second section", screen()).expect("outline row");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    let y = document_text_at(&texts, "secondmark", screen())
        .expect("the click selected the tab and scrolled its heading into view")
        .y;
    assert!(
        (0.0..700.0).contains(&y),
        "the revealed heading stays in the viewport: y={y}"
    );
    assert_eq!(
        tab.pending_anchor, None,
        "the revealed heading consumed the anchor request"
    );
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
    assert_eq!(
        tab.outline.current, 0,
        "the sidebar opens on the first section"
    );
    // The wheel scrolls the HOVERED area: hover the sheet, away from the
    // sidebar (a press or wheel over the sidebar belongs to its own list).
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 350.0))],
    );
    // One wheel step per frame until the last section is reached: the
    // sections are several thousand px apart and one step moves a fraction
    // of that, so the cursor must PASS THROUGH the middle one (each heading
    // crosses the viewport top on its own frame).
    let mut seen = vec![tab.outline.current];
    while tab.outline.current < 2 && seen.len() < 500 {
        let _ = run(&mut tab, vec![wheel(-200.0)]);
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

/// A cluster of headings with little text between them must still be walked
/// row by row while scrolling: the spy may not leap from the first to the
/// last just because the sections are short.
#[test]
fn dense_headings_are_selected_one_by_one_while_scrolling() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let mut md = format!("# Top\n\n{}", body(80));
    for i in 0..6 {
        md.push_str(&format!("## Close {i}\n\nline {i}\n\n"));
    }
    md.push_str(&format!("{}tail\n", body(80)));
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
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 350.0))],
    );
    let mut seen = vec![tab.outline.current];
    for _ in 0..600 {
        let _ = run(&mut tab, vec![wheel(-30.0)]);
        if *seen.last().unwrap() != tab.outline.current {
            seen.push(tab.outline.current);
        }
        if tab.outline.current == 6 {
            break;
        }
    }
    assert_eq!(
        seen,
        (0..=6).collect::<Vec<_>>(),
        "every dense heading became current in order"
    );
}

/// A heading at the very end of the document cannot scroll to the top (there
/// is no content left below it). Clicking its row must still select it, and
/// the page end must keep it current instead of falling back to an earlier
/// section.
#[test]
fn a_heading_at_the_page_end_stays_selected() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body: String = (0..120).map(|i| format!("body {i:03}\n\n")).collect();
    let md = format!("# One\n\n{body}## Two\n\n{body}## Three\n\nlastmark tail paragraph\n");
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
    let before = run(&mut tab, Vec::new());
    let row = panel_text_at(&before, "Three", screen()).expect("the last row is painted");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(tab.outline.current, 2, "the clicked row is current at once");
    for _ in 0..60 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    assert_eq!(
        tab.outline.current, 2,
        "the page end keeps the selected last section"
    );
    assert!(
        document_text_at(&texts, "lastmark", screen()).is_some(),
        "the heading near the page end is visible"
    );
    // A manual scroll releases the clicked row to the scrollspy. Move the
    // pointer off the sidebar first: the wheel belongs to the hovered area.
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 350.0))],
    );
    let _ = run(&mut tab, vec![wheel(200.0)]);
    let _ = run(&mut tab, vec![wheel(200.0)]);
    assert!(
        tab.outline.current < 2,
        "scrolling up moves the cursor back to an earlier section"
    );
}

/// The sidebar is part of the layout on narrow windows too (always on the
/// right, never a bottom sheet). It only shrinks proportionally.
#[test]
fn narrow_windows_keep_the_right_sidebar() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# One\n\nbody\n\n## Two\n\nbody\n\n## Three\n\nbody\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let narrow = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 700.0));
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
    let texts = run(&mut tab, narrow);
    let header = panel_text_at(&texts, "Outline", narrow).expect("the sidebar header is painted");
    assert!(
        header.x > narrow.width() * 0.5,
        "a narrow window still docks the sidebar to the right: {header:?}"
    );
    assert!(
        header.y < 80.0,
        "and starts at the top, never as a bottom sheet: {header:?}"
    );
}

/// Escape and presses outside the sidebar do not close it: it is permanent
/// layout, not a dismissible surface.
#[test]
fn escape_and_outside_presses_leave_the_sidebar_open() {
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
    assert!(
        panel_text_at(&run(&mut tab, Vec::new()), "Outline", screen()).is_some(),
        "Escape leaves the permanent sidebar alone"
    );
    let _ = run(&mut tab, click(egui::pos2(300.0, 300.0)));
    assert!(
        panel_text_at(&run(&mut tab, Vec::new()), "Outline", screen()).is_some(),
        "a press in the sheet leaves the sidebar alone"
    );
}

/// A document without headings keeps the sidebar slot (with a short empty
/// state) so navigating to it from a page with headings does not widen the
/// reading column.
#[test]
fn a_headingless_document_keeps_an_empty_sidebar() {
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
    let texts = frame(
        &ctx,
        screen(),
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        Vec::new(),
    );
    assert!(
        panel_text_at(&texts, "Outline", screen()).is_some(),
        "the headingless page keeps the sidebar header"
    );
    assert!(
        panel_text_at(&texts, "No headings", screen()).is_some(),
        "the empty panel explains why it has no rows"
    );
    let left = panel_left(screen());
    let body = document_text_at(&texts, "Just one paragraph", screen())
        .expect("the document still paints its paragraph");
    assert!(
        body.x < left,
        "the reading column stays left of the reserved slot: {body:?}"
    );
    assert!(
        tab.outline.panel_rect.is_some(),
        "the empty panel still publishes its rect (the find bar docks left of it)"
    );
}

/// Navigating away keeps the sidebar slot over the stale backdrop while the
/// next page loads: the reader clicked a link, not a narrower layout.
#[test]
fn a_loading_page_keeps_the_sidebar_slot() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab("# One\n\nbody\n\n## Two\n\nbody\n");
    let lines: String = (0..200).map(|i| format!("line {i:03}\n\n")).collect();
    let backdrop = loaded_tab(&format!("# Old\n\n{lines}"));
    let backdrop = match backdrop.state {
        TabState::Loaded { fetched } => fetched,
        _ => unreachable!(),
    };
    tab.state = TabState::Loading {
        promise: poll_promise::Promise::from_ready(Err(crate::error::BrowserError::Cancelled)),
        asked: "https://ex.com/b/".to_string(),
        address_at_start: String::new(),
        started: std::time::Instant::now(),
        backdrop: Some(Box::new(backdrop)),
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
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
    let texts = frame(
        &ctx,
        screen(),
        &mut tab,
        &mut store,
        &mut bookmarks,
        &mut favicons,
        Vec::new(),
    );
    assert!(
        panel_text_at(&texts, "Outline", screen()).is_some(),
        "the loading page keeps the sidebar"
    );
    assert!(
        panel_text_at(&texts, "Loading...", screen()).is_some(),
        "the stale outline is replaced by a loading placeholder"
    );
    let left = panel_left(screen());
    let body = document_text_at(&texts, "line 000", screen())
        .expect("the stale backdrop is still visible");
    assert!(
        body.x < left,
        "the backdrop keeps the narrower reading column: {body:?}"
    );
    assert!(
        tab.outline.panel_rect.is_some(),
        "the loading panel still publishes its rect"
    );
}

/// The find bar docks left of the permanent sidebar instead of covering it.
#[test]
fn the_find_bar_docks_left_of_the_sidebar() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let md = "# T\n\nterm one\n\n## Two\n\nbody\n";
    let mut tab = loaded_tab(md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab| {
        frame(
            &ctx,
            screen(),
            tab,
            &mut store,
            &mut bookmarks,
            &mut favicons,
            Vec::new(),
        )
    };
    for _ in 0..2 {
        let _ = run(&mut tab);
    }
    tab.find.open = true;
    tab.find.query = "term".to_string();
    let _ = run(&mut tab);
    let texts = run(&mut tab);
    let count = texts
        .iter()
        .find(|(t, p, _)| t.contains("/1") && p.y < 130.0)
        .expect("the find bar paints its count");
    assert!(
        count.1.x < panel_left(screen()),
        "the find bar stays left of the sidebar: {count:?}"
    );
}

/// A page whose last headings can never reach the viewport top (the sheet
/// runs out of content first) still walks them in order as the reader scrolls
/// to the bottom, and scrolling back up walks them in reverse. This is the
/// www.cloudflare.com case: its final three sections share the last
/// screenful, so a spy that only marks headings crossing the top line left
/// the last rows unreachable.
#[test]
fn the_cursor_walks_the_page_end_and_follows_a_manual_scroll() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let md = format!(
        "# One\n\n{}## Two\n\n{}## Three\n\n{}## Four\n\n{}## Five\n\ntail\n",
        body(80),
        body(40),
        body(20),
        body(10)
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen,
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
    assert_eq!(
        tab.outline.current, 0,
        "the page opens on its first section"
    );
    // The wheel scrolls the hovered area: keep it over the sheet, away from
    // the sidebar's own scrollable list.
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 450.0))],
    );
    let mut seen = Vec::new();
    let mut last = tab.outline.current;
    for _ in 0..500 {
        let _ = run(&mut tab, vec![wheel(-30.0)]);
        if tab.outline.current != last {
            seen.push(tab.outline.current);
            last = tab.outline.current;
        }
        if tab.outline.current == 4 {
            break;
        }
    }
    assert_eq!(
        seen,
        (1..=4).collect::<Vec<_>>(),
        "every tail heading became current in order"
    );
    assert_eq!(
        tab.outline.current, 4,
        "the last section is current at the page end"
    );
    // Clicking the last row must select it, then a real scroll up must hand
    // the cursor back to the spy instead of leaving the click pinned.
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    let texts = run(&mut tab, Vec::new());
    let row = panel_text_at(&texts, "Five", screen).expect("the last outline row is painted");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(
        tab.outline.current, 4,
        "the clicked row is selected at once"
    );
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(tab.outline.current, 4, "the page end keeps it selected");
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 450.0))],
    );
    let mut seen = Vec::new();
    let mut last = tab.outline.current;
    for _ in 0..800 {
        let _ = run(&mut tab, vec![wheel(30.0)]);
        if tab.outline.current != last {
            seen.push(tab.outline.current);
            last = tab.outline.current;
        }
        if tab.outline.current == 0 {
            break;
        }
    }
    assert_eq!(
        seen,
        vec![3, 2, 1, 0],
        "scrolling up walks the tail back and reaches the first section"
    );
}

/// Clicking a page-end heading that cannot reach the top keeps THAT row
/// selected once the sheet settles at the bottom. The page-end reading line
/// must not hand the cursor to a deeper section after the click's scroll
/// finishes: the reader asked for this row, not for the document tail.
#[test]
fn a_clicked_page_end_heading_stays_selected_after_the_scroll_settles() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let md = format!(
        "# One\n\n{}## Two\n\n{}## Three\n\n{}## Four\n\n{}## Five\n\ntail\n",
        body(80),
        body(40),
        body(20),
        body(10)
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen,
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
    let texts = run(&mut tab, Vec::new());
    let row = panel_text_at(&texts, "Four", screen).expect("the row is painted");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(
        tab.outline.current, 3,
        "the clicked row is selected at once"
    );
    // Let the requested scroll run to the end of the document: `Five` becomes
    // the deepest visible section, but the clicked `Four` stays current.
    for _ in 0..80 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(
        tab.outline.current, 3,
        "the clicked page-end row stays selected after the scroll settles"
    );
}

/// Clicking a tail row that is ALREADY the current section must keep it
/// selected too: the click's scroll to the clamped page end moves the
/// reading line on to the deeper row, but not the reader's choice.
#[test]
fn a_clicked_tail_row_that_is_already_current_stays_current() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let md = format!(
        "# One\n\n{}## Two\n\n{}## Three\n\n{}## Four\n\n{}## Five\n\ntail\n",
        body(80),
        body(40),
        body(20),
        body(10)
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen,
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
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 450.0))],
    );
    // Reach the bottom, then step back up until `Four` (the second-to-last
    // heading) is the current section at rest. Its own scroll target is the
    // clamped page end, so clicking it must not let `Five` take over.
    for _ in 0..500 {
        let _ = run(&mut tab, vec![wheel(-30.0)]);
        if tab.outline.current == 4 {
            break;
        }
    }
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(tab.outline.current, 4, "the page reached its bottom");
    for _ in 0..500 {
        let _ = run(&mut tab, vec![wheel(30.0)]);
        if tab.outline.current == 3 {
            break;
        }
    }
    for _ in 0..40 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(tab.outline.current, 3, "the test rests on the clicked row");
    let texts = run(&mut tab, Vec::new());
    let row = panel_text_at(&texts, "Four", screen).expect("the row is painted");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(
        tab.outline.current, 3,
        "the clicked row is selected at once"
    );
    for _ in 0..80 {
        let _ = run(&mut tab, Vec::new());
    }
    assert_eq!(
        tab.outline.current, 3,
        "the clicked tail row stays selected after its scroll settles"
    );
}

/// A row click requests a smooth scroll and pins the row while it settles.
/// A manual scroll in the meantime must release the pin: the wheel delta is
/// read BEFORE the document walk, because the sheet's `ScrollArea` consumes
/// and zeroes it. Reading it after the walk leaves the clicked row pinned
/// even though the spy already points at the section on screen.
#[test]
fn a_manual_scroll_releases_a_row_click_still_settling() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let body = |n: usize| -> String { (0..n).map(|i| format!("body {i:03}\n\n")).collect() };
    let md = format!(
        "# One\n\n{}## Two\n\n{}## Three\n\ntail\n",
        body(80),
        body(80)
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        frame(
            &ctx,
            screen,
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
    assert_eq!(
        tab.outline.current, 0,
        "the page opens on its first section"
    );
    // Click `## Two` far below: the sheet starts a smooth scroll toward it
    // and the click pin keeps the row current while it settles.
    let texts = run(&mut tab, Vec::new());
    let row = panel_text_at(&texts, "Two", screen).expect("the row is painted");
    let _ = run(&mut tab, click(row + egui::vec2(20.0, 8.0)));
    assert_eq!(tab.outline.current, 1, "the click pins the requested row");
    // Before that scroll reaches the heading, the reader wheels back up over
    // the sheet. The spy still points at the first section; only the manual
    // scroll can hand the cursor back to it.
    let _ = run(
        &mut tab,
        vec![
            egui::Event::PointerMoved(egui::pos2(300.0, 450.0)),
            wheel(30.0),
        ],
    );
    assert_eq!(
        tab.outline.current, 0,
        "the manual scroll releases the pin to the section on screen"
    );
}
