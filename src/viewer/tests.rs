// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::show_content;
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// A tab in `History` renders the internal page through the normal
/// central-panel entry point and, with no click, performs no navigation.
#[test]
fn history_state_renders_the_internal_page() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    };
    let mut tab = Tab::new(1);
    tab.open_history(true);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.record("https://ex.com/a/", Some("A"));
    let mut out = ctx.run_ui(input, |root| {
        assert!(
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons).is_none(),
            "no new-tab URL from the history page"
        );
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "history page painted");
    assert!(
        matches!(tab.state, TabState::History),
        "the page itself did not navigate"
    );
}

/// A tab in `Licenses` renders the embedded third-party notices through
/// the normal central-panel entry point and, with no click, performs no
/// navigation. The painted sheet must contain the notices' H1.
#[test]
fn licenses_state_renders_the_embedded_notices() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    };
    let mut tab = Tab::new(1);
    tab.open_licenses(true);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut out = ctx.run_ui(input, |root| {
        assert!(
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons).is_none(),
            "no new-tab URL from the licenses page"
        );
    });
    out.textures_delta.clear();
    assert!(
        matches!(tab.state, TabState::Licenses),
        "the page itself did not navigate"
    );
    let painted: String = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        painted.contains("Third-party notices"),
        "the embedded notices render (painted text: {painted:?})"
    );
}

/// Build a `Loaded` tab rendering `md` (no fetch, no network).
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
            source: md.to_string(),
            events,
            anchors,
            outline: crate::markdown::outline::Outline::default(),
            dests,
            stale_copy: None,
        },
    };
    tab
}

/// Regression: a code block's horizontal bar must sit BELOW its lines.
/// The floating bar used to be painted at the bottom of the text
/// viewport, so the expanded handle covered the last line while
/// scrolling; the bar now owns a lane under the code.
#[test]
fn code_bar_sits_below_the_code_in_the_sheet() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let long = "a".repeat(300);
    let mut tab = loaded_tab(&format!("```rust\nfn f() {{ \"{long}\" }}\n```\n"));
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |pointer: Option<egui::Pos2>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons);
        });
        out.textures_delta.clear();
        out.shapes
    };

    // The bar's background strip: a thin, wide rect. The sheet's top
    // fade also paints thin strips, so take the lowest one.
    let mut shapes = Vec::new();
    for _ in 0..3 {
        shapes = run(None);
    }
    let strip = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.rect.height() <= 3.0 && r.rect.width() >= 100.0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .expect("the code bar is painted");

    // Hover it: the handle expands to its full height.
    let mut handle = None;
    for _ in 0..30 {
        let shapes = run(Some(strip.center()));
        if let Some(rect) = shapes.iter().find_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r)
                if r.rect.height() >= 8.0 && r.rect.height() <= 11.0 && r.rect.width() >= 30.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        }) {
            handle = Some(rect);
        }
    }
    let handle = handle.expect("the hovered handle expands");
    let text_bottom = run(None)
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().contains("aaaa") => {
                Some(t.pos.y + t.galley.rect.max.y)
            }
            _ => None,
        })
        .fold(f32::MIN, f32::max);
    assert!(text_bottom > f32::MIN, "the code line was laid out");
    assert!(
        handle.min.y >= text_bottom,
        "the bar covers the code: strip {strip:?} handle {handle:?} text bottom {text_bottom:.1}"
    );
}

/// Regression: a wide table's horizontal bar must sit BELOW its last
/// row. The floating handle used to be painted over the row's bottom
/// (covering text while scrolling); a scrolling table now reserves the
/// bar's lane under the grid.
#[test]
fn table_bar_sits_below_the_last_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    // 14 columns at the 64px floor do not fit the reading column: the
    // floors win and the table scrolls horizontally.
    let cols = 14;
    let header: Vec<String> = (1..=cols).map(|i| format!("Header {i}")).collect();
    let sep: Vec<&str> = (1..=cols).map(|_| "---").collect();
    let cells: Vec<&str> = (1..=cols).map(|_| "Somewhat long value here").collect();
    let md = format!(
        "| {} |\n| {} |\n| {} |\n| {} |\n",
        header.join(" | "),
        sep.join(" | "),
        cells.join(" | "),
        cells.join(" | "),
    );
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |pointer: Option<egui::Pos2>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons);
        });
        out.textures_delta.clear();
        out.shapes
    };

    // The bar's background strip: the lowest thin, wide rect (the sheet's
    // top fade also paints thin strips).
    let mut shapes = Vec::new();
    for _ in 0..3 {
        shapes = run(None);
    }
    let strip = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.rect.height() <= 3.0 && r.rect.width() >= 100.0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .expect("the table bar is painted");

    // Hover it: the handle expands to its full height.
    let mut handle = None;
    for _ in 0..30 {
        let shapes = run(Some(strip.center()));
        if let Some(rect) = shapes.iter().find_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r)
                if r.rect.height() >= 8.0 && r.rect.height() <= 11.0 && r.rect.width() >= 30.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        }) {
            handle = Some(rect);
        }
    }
    let handle = handle.expect("the hovered handle expands");
    let text_bottom = run(None)
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.pos.y + t.galley.rect.max.y),
            _ => None,
        })
        .fold(f32::MIN, f32::max);
    // The lane must leave a real gap: without it the expanded handle sat
    // 3px under the text box (grazing descenders) instead of below the
    // grid, so require a minimum clearance.
    assert!(
        handle.min.y >= text_bottom + 4.0,
        "the bar hangs over the last row: strip {strip:?} handle {handle:?} text bottom {text_bottom:.1}"
    );
}

/// Regression: the always-visible bar must hug the reading sheet's right
/// edge. The frame used to keep the responsive gutter as a horizontal
/// inner margin, so the bar floated that gutter inside the sheet (a white
/// strip between the bar and the sheet edge). The failed-load sheet has
/// the same frame + bar and needs no fetch.
#[test]
fn scrollbar_hugs_the_sheet_edge() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let input = egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let mut tab = Tab::new(1);
    tab.state = TabState::Failed {
        asked: "https://ex.com/docs/".to_string(),
        lines: (0..80)
            .map(|i| format!("access to https://ex.com/{i} failed"))
            .collect(),
    };
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    // Several passes: the floating bar fades in over the first frames.
    let mut shapes = Vec::new();
    for _ in 0..10 {
        let mut out = ctx.run_ui(input.clone(), |root| {
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons);
        });
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    // The vertical handle is the narrow, tall rounded rect painted at the
    // window's right edge (the sheet spans the whole panel; the error
    // view's inner log bar sits one gutter inside it).
    let handle = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r)
                if r.rect.width() <= 12.0
                    && r.rect.height() >= 24.0
                    && r.rect.max.x >= screen.max.x - 1.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .next_back()
        .expect("the always-visible bar is painted at the right edge");
    assert!(
        (handle.max.x - screen.max.x).abs() < 0.5,
        "bar floats {:.1}px inside the sheet: {handle:?}",
        screen.max.x - handle.max.x
    );
    assert!(handle.width() <= 12.0, "handle is a bar: {handle:?}");
}

/// End-to-end guard for the code virtualization: after the page scrolls a
/// long PLAIN code block up to (and past) the window top, the visible rows
/// must fill the viewport from its top edge. Regression: the visible-rect
/// bound mixed monitor-space and local coordinates (culling rows early), and
/// the plain renderer painted rows at a relative offset (parking them at the
/// block top), so a scrolled block went blank.
#[test]
fn scrolled_plain_code_block_fills_the_viewport_from_the_top() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let lines: String = (0..200).map(|i| format!("line {i:03}\n")).collect();
    let md = format!("# Title\n\n```txt\n{}```\n", lines);
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |events: Vec<egui::Event>| {
        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            events,
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
        let mut out = ctx.run_ui(raw, |root| {
            show_content(root, &mut tab, &mut store, &mut bookmarks, &mut favicons);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text().starts_with("line ") => {
                    Some((t.galley.text().to_string(), t.pos.y))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for _ in 0..3 {
        let _ = run(Vec::new());
    }
    // Wheel-scroll the page until the code block is far above the top edge.
    let wheel = |dy: f32| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = run(vec![egui::Event::PointerMoved(egui::pos2(450.0, 350.0))]);
    for _ in 0..30 {
        let _ = run(vec![wheel(-200.0)]);
    }
    let mut visible: Vec<f32> = run(Vec::new())
        .into_iter()
        .filter_map(|(_, y)| (y > -20.0 && y < 700.0).then_some(y))
        .collect();
    visible.sort_by(f32::total_cmp);
    assert!(!visible.is_empty(), "the scrolled block must show rows");
    let top = visible[0];
    assert!(
        top < 20.0,
        "the first visible row must reach the top edge, not stop at {top}"
    );
}

/// A fresh navigation to an already-seen document starts at the top:
/// only Back/Forward/Refresh reuse the saved offset. End-to-end: scroll
/// a long page down (the title leaves the viewport), request the reset a
/// new navigation sets, and the next frame shows the title back at the
/// top edge while consuming the flag.
#[test]
fn fresh_navigation_starts_a_seen_page_at_the_top() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let lines: String = (0..200).map(|i| format!("line {i:03}\n\n")).collect();
    let md = format!("# Title\n\n{lines}");
    let mut tab = loaded_tab(&md);
    let mut store = HistoryStore::in_memory();
    let mut bookmarks = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let mut run = |tab: &mut Tab, events: Vec<egui::Event>| {
        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
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
        let mut out = ctx.run_ui(raw, |root| {
            show_content(root, tab, &mut store, &mut bookmarks, &mut favicons);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.y)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let title_ys = |texts: &[(String, f32)]| {
        texts
            .iter()
            .filter_map(|(t, y)| (t == "Title").then_some(*y))
            .collect::<Vec<_>>()
    };
    for _ in 0..3 {
        let _ = run(&mut tab, Vec::new());
    }
    // Wheel-scroll the page until the title is far above the top edge.
    let wheel = |dy: f32| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = run(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(450.0, 350.0))],
    );
    for _ in 0..30 {
        let _ = run(&mut tab, vec![wheel(-200.0)]);
    }
    let texts = run(&mut tab, Vec::new());
    let title_y = title_ys(&texts);
    assert!(
        title_y.iter().all(|&y| y < 0.0),
        "the title must be above the viewport after scrolling: {title_y:?}"
    );
    // What a fresh navigation to this already-seen page sets: the next
    // frame starts at the top instead of reusing the saved offset.
    tab.scroll_top_requested = true;
    let title_y = title_ys(&run(&mut tab, Vec::new()));
    assert!(
        !tab.scroll_top_requested,
        "the reset applies to the first frame only"
    );
    assert!(
        !title_y.is_empty(),
        "the title is back on screen after the reset"
    );
    assert!(
        title_y.iter().all(|&y| y > -20.0 && y < 60.0),
        "the title must sit at the top edge, not mid-page: {title_y:?}"
    );
}
