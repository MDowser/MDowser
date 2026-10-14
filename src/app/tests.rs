// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{first_run_target, stored_target, MdowserApp, WindowGeometryState};
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use crate::settings::{Prefs, WindowGeometry};
use crate::ui::preferences::PreferencesState;

fn app_with(tabs: Vec<Tab>, active: usize) -> MdowserApp {
    MdowserApp {
        tabs,
        active,
        next_id: 99,
        last_window_title: "MDowser".to_string(),
        zoom: 1.0,
        favicons: FaviconCache::new(),
        tabs_overlay_rect: None,
        focus_address: false,
        history: HistoryStore::in_memory(),
        bookmarks: BookmarkStore::in_memory(),
        prefs: Prefs::default(),
        session_saved: Vec::new(),
        session_saved_active: active,
        session_save_retry_at: None,
        session_restore_enabled: false,
        window_geometry: WindowGeometryState::new(None),
        windows: PreferencesState::new(),
        // The real bar is installed at startup (AppKit owns it); tests never
        // touch it, so the field stays `None` here.
        #[cfg(target_os = "macos")]
        mac_menu: None,
        #[cfg(target_os = "macos")]
        pending_menu_events: Vec::new(),
    }
}

#[test]
fn active_tab_heals_empty_vec_without_panic() {
    let mut app = app_with(Vec::new(), 0);
    let id = app.active_tab_mut().id;
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active, 0);
    assert_eq!(id, 99);
}

#[test]
fn active_tab_clamps_oob_index() {
    let mut app = app_with(vec![Tab::new(1), Tab::new(2)], 7);
    let id = app.active_tab_mut().id;
    assert_eq!(app.active, 1);
    assert_eq!(id, 2);
}

#[test]
fn close_last_tab_resets_instead_of_emptying() {
    let mut app = app_with(vec![Tab::new(5)], 0);
    app.close_tab(&eframe::egui::Context::default(), 0);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active, 0);
}

/// Focus ownership: neither launching the app (`MdowserApp::new` leaves
/// `focus_address` false, so no suggestion popup opens over a fresh
/// window) nor opening a new tab focuses an address field; only an
/// explicit `Ctrl/Cmd+L` does (`open_history` focuses it nowhere
/// either, covered above).
#[test]
fn new_tab_leaves_the_address_field_unfocused() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.new_tab();
    assert!(!app.focus_address, "Ctrl/Cmd+T must not focus the field");
}

#[test]
fn new_tab_respects_the_cap() {
    let mut app = app_with(Vec::new(), 0);
    for _ in 0..(crate::config::MAX_TABS + 5) {
        app.new_tab();
    }
    assert_eq!(app.tabs.len(), crate::config::MAX_TABS);
    // The refused attempt leaves a visible toast instead of silence.
    assert!(app.active_tab_mut().toast_text.is_some());
}

/// A loaded tab for Ctrl/Cmd+D tests (no fetch, no network).
fn loaded_tab(id: usize, url: &str, title: Option<&str>) -> Tab {
    let mut fetched = crate::protocol::client::Fetched {
        display_url: url::Url::parse(url).unwrap(),
        meta: crate::markdown::frontmatter::Meta::default(),
        h1_title: None,
        plain_text: String::new(),
        events: Vec::new(),
        anchors: crate::markdown::anchors::AnchorIndex::default(),
        dests: crate::protocol::rewrite::DestCache::default(),
    };
    fetched.meta.title = title.map(str::to_string);
    let mut tab = Tab::new(id);
    tab.state = TabState::Loaded { fetched };
    tab
}

/// Ctrl/Cmd+D toggles the loaded page in the encrypted store, with a
/// toast confirmation (bookmarks are listed only on Home).
#[test]
fn toggle_bookmark_adds_then_removes_the_loaded_page() {
    let mut app = app_with(vec![loaded_tab(1, "https://ex.com/a/", Some("Page A"))], 0);
    app.toggle_bookmark();
    assert_eq!(app.bookmarks.entries().len(), 1);
    assert_eq!(app.bookmarks.entries()[0].url, "https://ex.com/a/");
    assert_eq!(app.bookmarks.entries()[0].title.as_deref(), Some("Page A"));
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Bookmark added")
    );
    app.toggle_bookmark();
    assert!(app.bookmarks.entries().is_empty(), "second press removes");
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Bookmark removed")
    );
}

/// Home (or any non-loaded state) has no bookmark target: the shortcut
/// explains itself with a toast and the store stays clean.
#[test]
fn toggle_bookmark_on_home_changes_nothing() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.toggle_bookmark();
    assert!(app.bookmarks.entries().is_empty());
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Nothing to bookmark")
    );
}

/// The toolbar ribbon state follows the loaded page and the store: an
/// unbookmarked page offers the outline ribbon, a bookmarked one the
/// filled ribbon, and Home has nothing to bookmark.
#[test]
fn bookmark_button_state_follows_the_store() {
    use crate::ui::omnibox::BookmarkState;
    let mut app = app_with(vec![loaded_tab(1, "https://ex.com/a/", Some("Page A"))], 0);
    assert_eq!(app.bookmark_state(0), BookmarkState::NotBookmarked);
    app.toggle_bookmark();
    assert_eq!(app.bookmark_state(0), BookmarkState::Bookmarked);
    app.toggle_bookmark();
    assert_eq!(app.bookmark_state(0), BookmarkState::NotBookmarked);
    let home = app_with(vec![Tab::new(1)], 0);
    assert_eq!(home.bookmark_state(0), BookmarkState::Unavailable);
}

/// "Open link in new tab": the new tab goes right AFTER the active one,
/// the current tab stays active (background open). The URL is invalid
/// on purpose: `navigate_to_text` then fails locally, so the test never
/// touches the network.
#[test]
fn open_url_in_new_tab_inserts_after_the_active_tab() {
    let mut app = app_with(vec![Tab::new(1), Tab::new(2)], 0);
    app.open_url_in_new_tab("not a valid url");
    assert_eq!(app.tabs.len(), 3);
    assert_eq!(app.active, 0, "current tab keeps the focus");
    assert!(matches!(app.tabs[1].state, TabState::Failed { .. }));
    assert_eq!(app.tabs[1].id, 99, "fresh id assigned");
}

/// At `MAX_TABS`, "Open link in new tab" refuses with a toast instead
/// of allocating (and never navigates the current page).
#[test]
fn open_url_in_new_tab_respects_the_cap() {
    let mut app = app_with(Vec::new(), 0);
    for _ in 0..crate::config::MAX_TABS {
        app.new_tab();
    }
    let before = app.tabs.len();
    app.open_url_in_new_tab("not a valid url");
    assert_eq!(app.tabs.len(), before, "no tab beyond the cap");
    assert!(app.active_tab_mut().toast_text.is_some(), "toast shown");
}

/// The History button / Ctrl+H opens `about:history` in a NEW tab
/// (inserted after the active one and activated) instead of replacing
/// the page being read.
#[test]
fn open_history_opens_a_new_tab_without_replacing_the_current_page() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.open_history();
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active, 1, "the history tab is the visible one");
    assert!(
        matches!(app.tabs[0].state, TabState::Home),
        "the previous page is untouched"
    );
    assert!(matches!(app.tabs[1].state, TabState::History));
    assert_eq!(app.tabs[1].id, 99, "fresh id assigned");
    assert_eq!(app.tabs[1].address, "about:history");
    assert!(
        !app.focus_address,
        "history never focuses the address field"
    );
}

/// A repeat request while the active tab already shows the history page
/// is a no-op: no second copy of the same internal page.
#[test]
fn open_history_is_a_noop_while_already_showing_it() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.open_history();
    let before = app.tabs.len();
    app.open_history();
    assert_eq!(app.tabs.len(), before, "no duplicate history tab");
    assert_eq!(app.active, before - 1, "still the history tab");
}

/// The Preferences "View third-party licenses" action opens
/// `about:licenses` in a NEW tab (inserted after the active one and
/// activated) instead of replacing the page being read; a repeat request
/// while it is active is a no-op.
#[test]
fn open_licenses_opens_a_new_tab_without_replacing_the_current_page() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.open_licenses();
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active, 1, "the licenses tab is the visible one");
    assert!(
        matches!(app.tabs[0].state, TabState::Home),
        "the previous page is untouched"
    );
    assert!(matches!(app.tabs[1].state, TabState::Licenses));
    assert_eq!(app.tabs[1].id, 99, "fresh id assigned");
    assert_eq!(app.tabs[1].address, "about:licenses");
    app.open_licenses();
    assert_eq!(app.tabs.len(), 2, "no duplicate licenses tab");
}

/// At `MAX_TABS`, opening History refuses with a toast instead of
/// allocating (and never navigates the current page).
#[test]
fn open_history_respects_the_cap() {
    let mut app = app_with(Vec::new(), 0);
    for _ in 0..crate::config::MAX_TABS {
        app.new_tab();
    }
    let before = app.tabs.len();
    app.open_history();
    assert_eq!(app.tabs.len(), before, "no tab beyond the cap");
    assert!(app.active_tab_mut().toast_text.is_some(), "toast shown");
}

#[test]
fn select_tab_keeps_overlay_state_scoped_per_page() {
    let mut app = app_with(vec![Tab::new(1), Tab::new(2)], 0);
    app.tabs[0].selected_all = true;
    app.tabs[1].selected_all = true;
    app.select_tab(1);
    assert_eq!(app.active, 1);
    assert!(!app.tabs[0].selected_all, "old page highlight cleared");
    assert!(app.tabs[1].selected_all, "new page untouched");
    // Out-of-range selection is a no-op.
    app.select_tab(9);
    assert_eq!(app.active, 1);
}

/// One session entry per tab, `None` for a `Home` tab, so the tab count
/// and active index survive a restore.
#[test]
fn session_snapshot_marks_home_tabs() {
    let home = Tab::new(1);
    let mut loaded = Tab::new(2);
    loaded.current = Some(url::Url::parse("https://ex.com/a/").unwrap());
    assert_eq!(
        super::session_snapshot(&[home, loaded]),
        vec![None, Some("https://ex.com/a/".to_string())]
    );
}

/// A restored tab whose load is still in flight (or already failed) keeps
/// the URL the user asked for in the snapshot instead of collapsing to the
/// `None` Home marker: the first save of the session must not erase a
/// remembered URL just because its fetch has not resolved yet.
#[test]
fn session_snapshot_keeps_an_in_flight_restored_url() {
    let mut pending = Tab::new(1);
    // No network: `navigate_to_url` only spawns the fetch (the tab is
    // dropped before it can resolve).
    pending.navigate_to_url(&url::Url::parse("https://ex.com/slow/").unwrap(), false);
    assert!(
        matches!(pending.state, TabState::Loading { .. }),
        "still loading"
    );
    assert_eq!(
        super::session_snapshot(std::slice::from_ref(&pending)),
        vec![Some("https://ex.com/slow/".to_string())]
    );
    // Opening an internal page clears the remembered target (Home marker).
    pending.open_home(true);
    assert_eq!(
        super::session_snapshot(std::slice::from_ref(&pending)),
        vec![None]
    );
}

/// OS/CLI targets: one tab each, in order, sequential ids. Local paths
/// are used so the test never touches the network (the files do not
/// exist and each tab fails fast on its own).
#[test]
fn initial_targets_open_one_tab_each_in_order() {
    let mut next_id = 42;
    let tabs = super::tabs_from_initial(
        vec![
            "/nonexistent-mdowser-test/a.md".to_string(),
            "/nonexistent-mdowser-test/b.md".to_string(),
        ],
        &mut next_id,
    );
    assert_eq!(tabs.len(), 2);
    assert_eq!(next_id, 44, "ids stay sequential");
    assert_eq!(tabs[0].id, 42);
    assert!(matches!(tabs[0].state, TabState::Loading { .. }));
}

/// Build a `Loaded` tab that renders `md` (no fetch, no network).
fn markdown_tab(id: usize, md: &str) -> Tab {
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::AnchorIndex::default();
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    let mut tab = Tab::new(id);
    tab.state = TabState::Loaded {
        fetched: crate::protocol::client::Fetched {
            display_url: base,
            meta: crate::markdown::frontmatter::Meta::default(),
            h1_title: crate::markdown::document::first_h1_title(&events),
            plain_text: md.to_string(),
            events,
            anchors,
            dests,
        },
    };
    tab
}

/// Right-clicking a text selection keeps it: the secondary PRESS is
/// intercepted before egui (`raw_input_hook`) so the label-selection plugin
/// cannot collapse it, and its release opens the menu as a pending context
/// click. Copying from that menu copies the SELECTED text, not the page.
#[test]
fn right_click_keeps_the_text_selection_for_copy() {
    use eframe::egui;
    use eframe::App as _;

    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let md = "The quick brown fox jumps over the lazy dog near the river bank at dawn.\n";
    let mut app = app_with(vec![markdown_tab(1, md)], 0);
    let mut frame =
        |events: Vec<egui::Event>| -> (Vec<egui::epaint::ClippedShape>, Vec<String>, bool) {
            let mut input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            // Exactly what eframe does: intercept before egui processes the input.
            app.raw_input_hook(&ctx, &mut input);
            let mut out = ctx.run_ui(input, |root| {
                let tab = &mut app.tabs[0];
                let _ = crate::viewer::show_content(
                    root,
                    tab,
                    &mut app.history,
                    &mut app.bookmarks,
                    &mut app.favicons,
                );
            });
            out.textures_delta.clear();
            let copied: Vec<String> = out
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            (out.shapes, copied, app.tabs[0].context_open)
        };
    let text_rect = |shapes: &[egui::epaint::ClippedShape]| -> egui::Rect {
        shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text().contains("quick brown") => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()))
                }
                _ => None,
            })
            .expect("the paragraph label is painted")
    };
    let press =
        |pos: egui::Pos2, button: egui::PointerButton, pressed: bool| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

    let mut rect = egui::Rect::NOTHING;
    for _ in 0..3 {
        rect = text_rect(&frame(Vec::new()).0);
    }
    let y = rect.center().y;
    let from = egui::pos2(rect.left() + 6.0, y);
    let to = egui::pos2(rect.left() + 180.0, y);
    let _ = frame(vec![
        egui::Event::PointerMoved(from),
        press(from, egui::PointerButton::Primary, true),
    ]);
    let _ = frame(vec![egui::Event::PointerMoved(to)]);
    let _ = frame(vec![press(to, egui::PointerButton::Primary, false)]);
    assert!(
        crate::viewer::has_label_selection(&ctx),
        "the drag created a selection"
    );
    let (_, selected, _) = frame(vec![egui::Event::Copy]);
    let selected = selected.first().cloned().expect("the selection copies");
    assert!(
        selected.contains("quick brown") && !selected.contains("dawn"),
        "the drag selected a substring: {selected:?}"
    );

    // Right-click inside the selection: press frame, then release frame.
    let rc = egui::pos2(rect.left() + 60.0, y);
    let _ = frame(vec![
        egui::Event::PointerMoved(rc),
        press(rc, egui::PointerButton::Secondary, true),
    ]);
    assert!(
        crate::viewer::has_label_selection(&ctx),
        "the intercepted press must keep the selection"
    );
    let (_, _, open) = frame(vec![press(rc, egui::PointerButton::Secondary, false)]);
    assert!(open, "the intercepted release opens the context menu");
    assert!(
        crate::viewer::has_label_selection(&ctx),
        "the selection survives the whole right-click"
    );

    // Settle the menu, then click its Copy row: the clipboard must get the
    // SELECTED text (the label-selection plugin copies it).
    for _ in 0..2 {
        let _ = frame(Vec::new());
    }
    let shapes = frame(Vec::new()).0;
    let copy_pos = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == "Copy" => Some(t.pos),
            _ => None,
        })
        .expect("the Copy row is painted");
    let (_, copied, open) = frame(vec![
        egui::Event::PointerMoved(copy_pos),
        press(copy_pos, egui::PointerButton::Primary, true),
        press(copy_pos, egui::PointerButton::Primary, false),
    ]);
    assert!(!open, "Copy closes the menu");
    assert_eq!(
        copied.first().map(String::as_str),
        Some(selected.as_str()),
        "Copy must copy the selected text"
    );
}

/// First run (no stored geometry): the window opens at the
/// monitor-proportional size, centered on the current monitor.
#[test]
fn first_run_window_geometry_is_proportional_and_centered() {
    use eframe::egui;
    let monitor = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let target = first_run_target(Some(monitor), egui::Vec2::ZERO, 1.0).expect("a monitor");
    assert_eq!(
        target.size,
        egui::vec2(1536.0, 864.0),
        "80 % of the monitor"
    );
    assert_eq!(target.pos, egui::pos2(192.0, 108.0), "centered");
    assert!(!target.maximized);
    // The frame chrome is accounted for: the OUTER rect is centered.
    let target = first_run_target(Some(monitor), egui::vec2(0.0, 40.0), 1.0).unwrap();
    assert_eq!(target.pos.y, 88.0);
    // A 2x monitor: the size is physical, so the ui-point bounds scale with
    // it (the window is still 80 % of the monitor).
    let hidpi = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(3840.0, 2160.0));
    let target = first_run_target(Some(hidpi), egui::Vec2::ZERO, 2.0).unwrap();
    assert_eq!(target.size, egui::vec2(3072.0, 1728.0));
    assert_eq!(target.pos, egui::pos2(384.0, 216.0));
    // No monitor information at all: nothing to place.
    assert!(first_run_target(None, egui::Vec2::ZERO, 1.0).is_none());
}

/// A stored geometry is restored when its position still lands on an
/// attached monitor; the size is clamped to that monitor and the maximized
/// flag survives.
#[test]
fn stored_window_geometry_is_restored_and_clamped() {
    use eframe::egui;
    let monitor = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let stored = WindowGeometry {
        pos: egui::pos2(300.0, 200.0),
        size: egui::vec2(1000.0, 700.0),
        maximized: true,
    };
    let target = stored_target(stored, &[monitor], Some(monitor), egui::Vec2::ZERO, 1.0).unwrap();
    assert_eq!(target, stored, "an on-screen geometry is kept verbatim");
    // A size from a larger screen is clamped to this monitor.
    let huge = WindowGeometry {
        size: egui::vec2(3000.0, 2000.0),
        ..stored
    };
    let target = stored_target(huge, &[monitor], Some(monitor), egui::Vec2::ZERO, 1.0).unwrap();
    assert_eq!(target.size, egui::vec2(1920.0, 1080.0));
    assert!(target.maximized, "the maximized flag survives the clamp");
}

/// The monitor a stored window was on can be gone: the geometry then falls
/// back to centering on the current monitor instead of opening off-screen.
#[test]
fn stored_window_geometry_falls_back_to_centering_without_its_monitor() {
    use eframe::egui;
    let monitor = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let stored = WindowGeometry {
        pos: egui::pos2(5000.0, 5000.0), // was on a monitor that is gone
        size: egui::vec2(1000.0, 700.0),
        maximized: false,
    };
    let target = stored_target(stored, &[monitor], Some(monitor), egui::Vec2::ZERO, 1.0).unwrap();
    assert_eq!(
        target.pos,
        egui::pos2(460.0, 190.0),
        "centered on the current monitor"
    );
    assert_eq!(target.size, stored.size);
    // No monitor list to validate against: trust the stored geometry.
    assert_eq!(
        stored_target(stored, &[], Some(monitor), egui::Vec2::ZERO, 1.0),
        Some(stored)
    );
}
