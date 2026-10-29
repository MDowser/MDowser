// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::show_navbar;
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::ui::omnibox::{address_id, BookmarkState};
use eframe::egui;

fn test_stores() -> (HistoryStore, BookmarkStore) {
    (HistoryStore::in_memory(), BookmarkStore::in_memory())
}

/// The toolbar renders headlessly (all icons, omnibox and zoom pill)
/// without panicking or triggering a navigation on its own.
#[test]
fn toolbar_renders_without_navigating() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };
    let mut tab = Tab::new(1);
    let mut zoom = 1.0_f32;
    let mut prefs = false;
    let (history, bookmarks) = test_stores();
    let mut out = ctx.run_ui(input, |root| {
        let _ = show_navbar(
            root,
            &mut tab,
            &history,
            &bookmarks,
            &mut zoom,
            true,
            &mut prefs,
            BookmarkState::Unavailable,
        );
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "toolbar painted something");
    assert!(matches!(tab.state, crate::browser::tab::TabState::Home));
    assert_eq!(zoom, 1.0);
    assert!(!prefs, "preferences stay closed until clicked");
    // Narrow window: the zoom pill hides and nothing panics.
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(420.0, 600.0),
        )),
        ..Default::default()
    };
    let mut tab = Tab::new(2);
    let mut out = ctx.run_ui(input, |root| {
        let _ = show_navbar(
            root,
            &mut tab,
            &history,
            &bookmarks,
            &mut zoom,
            false,
            &mut prefs,
            BookmarkState::Unavailable,
        );
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "narrow toolbar painted");
}

/// Every glyph anchor the toolbar painted: text positions plus the
/// CENTER of every vector icon shape (paths, circles, meshes, line
/// segments). Hover fills and focus rings are rectangles and are
/// ignored; the hover/press effect scales a glyph around its center,
/// so anchors must stay put in every pointer state.
fn toolbar_anchors(ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(i64, i64)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let mut tab = Tab::new(1);
    let mut zoom = 1.0_f32;
    let mut prefs = false;
    let (history, bookmarks) = test_stores();
    let mut out = ctx.run_ui(input, |root| {
        let _ = show_navbar(
            root,
            &mut tab,
            &history,
            &bookmarks,
            &mut zoom,
            false,
            &mut prefs,
            BookmarkState::Unavailable,
        );
    });
    out.textures_delta.clear();
    let mut anchors: Vec<(i64, i64)> = Vec::new();
    for clipped in &out.shapes {
        let center = match &clipped.shape {
            egui::epaint::Shape::Text(t) => t.pos,
            egui::epaint::Shape::Path(p) => egui::Rect::from_points(&p.points).center(),
            egui::epaint::Shape::Circle(c) => c.center,
            egui::epaint::Shape::Mesh(m) => m.calc_bounds().center(),
            egui::epaint::Shape::LineSegment { points, .. } => egui::pos2(
                (points[0].x + points[1].x) / 2.0,
                (points[0].y + points[1].y) / 2.0,
            ),
            _ => continue,
        };
        anchors.push((center.x.round() as i64, center.y.round() as i64));
    }
    anchors.sort_unstable();
    anchors
}

/// The toolbar never moves a widget between pointer states (idle, hover,
/// press, click): the chrome uses fixed-size allocations, hover only
/// restyles and the icon effect scales the glyph around its own center.
/// Locks the "bars must not shift" contract.
#[test]
fn toolbar_geometry_is_stable_in_every_pointer_state() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    // Settle the hover/press animations.
    let mut baseline = Vec::new();
    for _ in 0..3 {
        baseline = toolbar_anchors(&ctx, Vec::new());
    }
    assert!(!baseline.is_empty(), "toolbar anchors collected");

    // The leftmost and rightmost anchors are Back and Preferences.
    let back = baseline.iter().min_by_key(|a| a.0).copied().unwrap();
    let prefs_btn = baseline.iter().max_by_key(|a| a.0).copied().unwrap();
    let center = |a: (i64, i64)| egui::pos2(a.0 as f32, a.1 as f32);
    let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };

    for pos in [center(back), center(prefs_btn)] {
        // Hover.
        let hovered = toolbar_anchors(&ctx, vec![egui::Event::PointerMoved(pos)]);
        assert_eq!(hovered, baseline, "hover moved a toolbar widget at {pos:?}");
        // Press (active) and release (clicked): still no movement.
        let pressed = toolbar_anchors(&ctx, vec![egui::Event::PointerMoved(pos), press(pos, true)]);
        assert_eq!(pressed, baseline, "press moved a toolbar widget at {pos:?}");
        let released = toolbar_anchors(&ctx, vec![press(pos, false)]);
        assert_eq!(
            released, baseline,
            "release moved a toolbar widget at {pos:?}"
        );
    }
}

/// Every top-bar button owns a fixed slot whatever it renders, so
/// swapping an icon never shifts its neighbours: Reload (enabled) /
/// Reload on cooldown (disabled) / Stop (loading) share one `BUTTON`
/// slot, the outline/filled/disabled ribbon shares the 26px right slot
/// with the clear X, and the zoom value never resizes the pill at the
/// same breakpoint. The address field rect encodes both sides (its min
/// follows the nav cluster, its max precedes the reserved zoom/prefs),
/// so it must be identical in every content state.
#[test]
fn toolbar_slots_are_fixed_across_content_states() {
    use std::sync::{atomic::AtomicBool, Arc};
    use std::time::Instant;

    /// One navbar frame; returns the address field rect (rounded to px).
    fn field_rect(
        ctx: &egui::Context,
        tab: &mut Tab,
        state: BookmarkState,
        zoom_value: f32,
    ) -> (i64, i64, i64, i64) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let mut zoom = zoom_value;
        let mut prefs = false;
        let (history, bookmarks) = test_stores();
        let mut out = ctx.run_ui(input, |root| {
            let _ = show_navbar(
                root, tab, &history, &bookmarks, &mut zoom, false, &mut prefs, state,
            );
        });
        out.textures_delta.clear();
        let rect = ctx
            .read_response(address_id(tab.id, "address-bar"))
            .expect("the field is registered")
            .rect;
        (
            rect.min.x.round() as i64,
            rect.min.y.round() as i64,
            rect.max.x.round() as i64,
            rect.max.y.round() as i64,
        )
    }

    /// Loaded + refreshable (a current document, no cooldown stamp).
    fn refreshable_tab(id: usize) -> Tab {
        let mut tab = Tab::new(id);
        tab.current = Some(url::Url::parse("https://ex.com/a/").unwrap());
        tab.state = TabState::Loaded {
            fetched: crate::protocol::client::Fetched {
                display_url: url::Url::parse("https://ex.com/a/").unwrap(),
                meta: crate::markdown::frontmatter::Meta::default(),
                h1_title: None,
                plain_text: String::new(),
                source: String::new().into(),
                events: Vec::new(),
                anchors: crate::markdown::anchors::AnchorIndex::default(),
                outline: crate::markdown::outline::Outline::default(),
                dests: crate::protocol::rewrite::DestCache::default(),
                stale_copy: None,
            },
        };
        tab
    }

    /// Loading (the Reload slot shows Stop).
    fn loading_tab(id: usize) -> Tab {
        let mut tab = Tab::new(id);
        let promise: poll_promise::Promise<
            Result<crate::protocol::client::Fetched, crate::error::BrowserError>,
        > = poll_promise::Promise::from_ready(Err(crate::error::BrowserError::Cancelled));
        tab.state = TabState::Loading {
            promise,
            asked: "https://ex.com/a/".to_string(),
            address_at_start: String::new(),
            started: Instant::now(),
            backdrop: None,
            saved_back: Vec::new(),
            saved_forward: Vec::new(),
            pushed_history: false,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        tab
    }

    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut base = refreshable_tab(21);
    let baseline = field_rect(&ctx, &mut base, BookmarkState::NotBookmarked, 1.0);

    // Home: Reload on cooldown (disabled), nothing to bookmark.
    let mut home = Tab::new(22);
    assert_eq!(
        field_rect(&ctx, &mut home, BookmarkState::Unavailable, 1.0),
        baseline,
        "cooldown/disabled reload moved the omnibox"
    );
    // Filled ribbon: same right slot as the outline one.
    let mut marked = refreshable_tab(23);
    assert_eq!(
        field_rect(&ctx, &mut marked, BookmarkState::Bookmarked, 1.0),
        baseline,
        "filled ribbon moved the omnibox"
    );
    // Loading: Stop replaces Reload in the same slot.
    let mut loading = loading_tab(24);
    assert_eq!(
        field_rect(&ctx, &mut loading, BookmarkState::Unavailable, 1.0),
        baseline,
        "Stop (loading) moved the omnibox"
    );
    // The zoom value never resizes the pill at the same breakpoint.
    for zoom_value in [0.8, 1.5] {
        let mut tab = refreshable_tab(25);
        assert_eq!(
            field_rect(&ctx, &mut tab, BookmarkState::NotBookmarked, zoom_value),
            baseline,
            "zoom {zoom_value} moved the omnibox"
        );
    }
}

/// A loaded tab with a bookmarkable HTTPS URL (no fetch, no network).
fn loaded_tab() -> Tab {
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: crate::protocol::client::Fetched {
            display_url: url::Url::parse("https://ex.com/a/").unwrap(),
            meta: crate::markdown::frontmatter::Meta::default(),
            h1_title: None,
            plain_text: String::new(),
            source: String::new().into(),
            events: Vec::new(),
            anchors: crate::markdown::anchors::AnchorIndex::default(),
            outline: crate::markdown::outline::Outline::default(),
            dests: crate::protocol::rewrite::DestCache::default(),
            stale_copy: None,
        },
    };
    tab
}

/// One navbar frame with the given events; returns the toggle intent.
fn frame(
    ctx: &egui::Context,
    tab: &mut Tab,
    state: BookmarkState,
    events: Vec<egui::Event>,
) -> bool {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let mut zoom = 1.0_f32;
    let mut prefs = false;
    let mut toggled = false;
    let (history, bookmarks) = test_stores();
    let mut out = ctx.run_ui(input, |root| {
        toggled = show_navbar(
            root, tab, &history, &bookmarks, &mut zoom, false, &mut prefs, state,
        );
    });
    out.textures_delta.clear();
    toggled
}

/// The toolbar ribbon reports the toggle intent without navigating. Its
/// position is derived from the address field: the ribbon owns the same
/// 26px right slot inside the capsule (one slot, so nothing shifts).
#[test]
fn bookmark_button_reports_the_toggle() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab();
    let field_id = address_id(tab.id, "address-bar");
    // Settle, then read the field's rect from the interaction snapshot.
    let _ = frame(&ctx, &mut tab, BookmarkState::NotBookmarked, Vec::new());
    let field = ctx
        .read_response(field_id)
        .expect("the field is registered");
    // Slot geometry: 7px pad + a 26px box; the field ends before it.
    let pos = egui::pos2(field.rect.max.x + 13.0, field.rect.center().y);
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut toggled = false;
    for events in [
        vec![egui::Event::PointerMoved(pos)],
        vec![button(true)],
        vec![button(false)],
    ] {
        toggled |= frame(&ctx, &mut tab, BookmarkState::NotBookmarked, events);
    }
    assert!(toggled, "the ribbon click must report the toggle");
    assert!(
        matches!(tab.state, TabState::Loaded { .. }),
        "the ribbon must not navigate"
    );

    // Nothing loaded: the ribbon is disabled and a click is ignored.
    let mut home = Tab::new(2);
    let home_field = address_id(home.id, "address-bar");
    let _ = frame(&ctx, &mut home, BookmarkState::Unavailable, Vec::new());
    let field = ctx
        .read_response(home_field)
        .expect("the home field is registered");
    let pos = egui::pos2(field.rect.max.x + 13.0, field.rect.center().y);
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut toggled = false;
    for events in [
        vec![egui::Event::PointerMoved(pos)],
        vec![button(true)],
        vec![button(false)],
    ] {
        toggled |= frame(&ctx, &mut home, BookmarkState::Unavailable, events);
    }
    assert!(!toggled, "a disabled ribbon must ignore clicks");
}
