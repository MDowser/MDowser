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
        closed_tabs: Vec::new(),
        last_window_title: "MDowser".to_string(),
        zoom: 1.0,
        favicons: FaviconCache::new(),
        tabs_overlay_rect: None,
        focus_address: false,
        pending_drops: Vec::new(),
        drop_hover: false,
        history: HistoryStore::in_memory(),
        bookmarks: BookmarkStore::in_memory(),
        prefs: Prefs::default(),
        session_saved: Vec::new(),
        session_saved_active: active,
        session_save_retry_at: None,
        session_restore_enabled: false,
        window_geometry: WindowGeometryState::new(None),
        windows: PreferencesState::new(),
        save_dir: None,
        // The real bar is installed at startup (AppKit owns it); tests never
        // touch it, so the field stays `None` here.
        #[cfg(target_os = "macos")]
        mac_menu: None,
        #[cfg(target_os = "macos")]
        pending_menu_events: Vec::new(),
    }
}

/// A native dropped-file handle for `raw_input_hook` tests. eframe's
/// production handles come from egui-winit; the hook only reads the path.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct TestDroppedFile(std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl eframe::egui::DroppedFile for TestDroppedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
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

/// OS drag state is read before the label-selection early return: hovering
/// paints the target, and dropped handles are drained into the app queue
/// (`logic` owns tab creation).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn raw_input_tracks_hover_and_queues_dropped_paths() {
    use eframe::App as _;

    let ctx = eframe::egui::Context::default();
    let mut app = app_with(vec![Tab::new(1)], 0);
    let path = std::env::temp_dir().join("mdowser-dropped-test.md");

    let mut hover = eframe::egui::RawInput {
        hovered_files: vec![eframe::egui::HoveredFile {
            path: Some(path.clone()),
            mime: "text/markdown".to_string(),
        }],
        ..Default::default()
    };
    app.raw_input_hook(&ctx, &mut hover);
    assert!(app.drop_hover, "hovering shows the drop target");
    assert!(app.pending_drops.is_empty(), "nothing dropped yet");

    let mut drop = eframe::egui::RawInput {
        dropped_files: vec![std::sync::Arc::new(TestDroppedFile(path.clone()))],
        ..Default::default()
    };
    app.raw_input_hook(&ctx, &mut drop);
    assert!(!app.drop_hover, "a completed drop clears the target");
    assert_eq!(app.pending_drops, vec![path]);
    assert!(
        drop.dropped_files.is_empty(),
        "the hook owns the drop list after queueing it"
    );
}

/// An oversized drag cannot grow the queue without bound: keep just enough
/// paths for `open_dropped_files` to open the remaining slots and toast the
/// overflow, and drain the raw list either way.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn raw_input_bounds_the_drop_queue() {
    use eframe::App as _;

    let ctx = eframe::egui::Context::default();
    let mut app = app_with(vec![Tab::new(1)], 0);
    let mut drop = eframe::egui::RawInput::default();
    for i in 0..(crate::config::MAX_TABS + 10) {
        let path = std::env::temp_dir().join(format!("mdowser-drop-{i}.md"));
        drop.dropped_files
            .push(std::sync::Arc::new(TestDroppedFile(path)));
    }

    app.raw_input_hook(&ctx, &mut drop);

    assert_eq!(
        app.pending_drops.len(),
        crate::config::MAX_TABS + 1,
        "one path beyond the cap is enough to report the overflow"
    );
    assert!(drop.dropped_files.is_empty(), "the hook owns the drop list");
}

/// Two drops delivered before the next `logic` pass share one bounded queue;
/// the second cannot grow it past the cap (and its handles are still drained).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn raw_input_bounds_the_drop_queue_across_calls() {
    use eframe::App as _;

    let ctx = eframe::egui::Context::default();
    let mut app = app_with(vec![Tab::new(1)], 0);
    for round in 0..2 {
        let mut drop = eframe::egui::RawInput::default();
        for i in 0..(crate::config::MAX_TABS + 10) {
            let path = std::env::temp_dir().join(format!("mdowser-drop-{round}-{i}.md"));
            drop.dropped_files
                .push(std::sync::Arc::new(TestDroppedFile(path)));
        }
        app.raw_input_hook(&ctx, &mut drop);
        assert!(drop.dropped_files.is_empty(), "the hook owns the drop list");
    }

    assert_eq!(
        app.pending_drops.len(),
        crate::config::MAX_TABS + 1,
        "the queue stays capped across hook calls"
    );
}

/// A drop opens one tab per path right after the active tab, in drop order,
/// and selects only the first new tab (the current page is not replaced).
#[test]
fn dropped_paths_open_after_active_and_select_the_first() {
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 1);
    let first = std::env::temp_dir().join("mdowser-drop-first.md");
    let second = std::env::temp_dir().join("mdowser-drop-second.md");
    app.drop_hover = true;

    app.open_dropped_files(vec![first.clone(), second.clone()]);

    assert_eq!(app.tabs.len(), 5);
    assert_eq!(app.active, 2, "the first dropped tab is visible");
    assert!(!app.drop_hover);
    assert_eq!(app.tabs[2].id, 99);
    assert_eq!(app.tabs[3].id, 100);
    assert_eq!(app.tabs[4].id, 3, "the original page follows the batch");
    let first_url = url::Url::from_file_path(&first).unwrap();
    let second_url = url::Url::from_file_path(&second).unwrap();
    assert_eq!(app.tabs[2].session_url(), Some(&first_url));
    assert_eq!(app.tabs[3].session_url(), Some(&second_url));
}

/// More than two paths keep their drop order even when tabs already exist to
/// the right of the active tab (the batch must not interleave with them).
#[test]
fn dropped_paths_keep_their_order_ahead_of_later_tabs() {
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 1);
    let paths: Vec<_> = (0..3)
        .map(|i| std::env::temp_dir().join(format!("mdowser-drop-order-{i}.md")))
        .collect();

    app.open_dropped_files(paths.clone());

    assert_eq!(app.tabs.len(), 6);
    assert_eq!(app.active, 2, "the first dropped tab is visible");
    assert_eq!(
        app.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![1, 2, 99, 100, 101, 3],
        "the whole batch sits between the active tab and its right neighbor"
    );
    for (offset, path) in paths.iter().enumerate() {
        let expected = url::Url::from_file_path(path).unwrap();
        assert_eq!(
            app.tabs[2 + offset].session_url(),
            Some(&expected),
            "tab {offset} keeps its drop position"
        );
    }
}

/// At the cap, a drop still opens the tabs that fit and reports the rest
/// with the shared toast instead of silently dropping them.
#[test]
fn a_drop_at_the_tab_cap_opens_what_fits_and_toasts() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    while app.tabs.len() < crate::config::MAX_TABS - 1 {
        let id = app.tabs.len() + 2;
        app.tabs.push(Tab::new(id));
    }
    let first = std::env::temp_dir().join("mdowser-drop-at-cap.md");
    let second = std::env::temp_dir().join("mdowser-drop-over-cap.md");
    // The fixture seeds `next_id` at 99, which the synthetic rail above also
    // uses; move it past the fixture ids so the dropped tab stays unique.
    app.next_id = 1000;

    app.open_dropped_files(vec![first.clone(), second]);

    assert_eq!(app.tabs.len(), crate::config::MAX_TABS);
    assert_eq!(app.active, 1);
    assert_eq!(app.tabs[1].id, 1000);
    let first_url = url::Url::from_file_path(&first).unwrap();
    assert_eq!(app.tabs[1].session_url(), Some(&first_url));
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Tab limit reached")
    );
}

/// A dropped path the URL type cannot represent becomes a visible local
/// error, never an HTTPS host lookup (the address parser would read a bare
/// relative name as a web host).
#[test]
fn a_relative_drop_becomes_a_local_error_not_a_host_lookup() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.open_dropped_files(vec![std::path::PathBuf::from("relative.md")]);
    assert_eq!(app.tabs.len(), 2);
    assert!(matches!(app.tabs[1].state, TabState::Failed { .. }));
    assert!(app.tabs[1].address.contains("relative.md"));
}

/// A loaded tab for Ctrl/Cmd+D tests (no fetch, no network).
fn loaded_tab(id: usize, url: &str, title: Option<&str>) -> Tab {
    let mut fetched = crate::protocol::client::Fetched {
        display_url: url::Url::parse(url).unwrap(),
        meta: crate::markdown::frontmatter::Meta::default(),
        h1_title: None,
        plain_text: String::new(),
        source: String::new(),
        events: Vec::new(),
        anchors: crate::markdown::anchors::AnchorIndex::default(),
        outline: crate::markdown::outline::Outline::default(),
        dests: crate::protocol::rewrite::DestCache::default(),
        stale_copy: None,
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

/// The rail's reorder (`move_tab`) moves the entry and keeps the ACTIVE tab
/// pointing at the same `Tab`: moving it keeps it selected, and moving
/// another tab across it keeps the shown page.
#[test]
fn move_tab_keeps_the_active_tab() {
    let order = |app: &MdowserApp| app.tabs.iter().map(|t| t.id).collect::<Vec<_>>();

    // Moving the ACTIVE tab: it stays selected, now at the target slot.
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 0);
    app.move_tab(0, 2);
    assert_eq!(order(&app), vec![2, 3, 1]);
    assert_eq!(app.tabs[app.active].id, 1, "the moved tab is still shown");

    // Moving another tab ACROSS the active one: the shown page is unchanged.
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 2);
    app.move_tab(0, 2);
    assert_eq!(order(&app), vec![2, 3, 1]);
    assert_eq!(app.tabs[app.active].id, 3, "the shown tab is the same one");

    // A move before the active tab shifts its index, never its identity.
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 2);
    app.move_tab(1, 0);
    assert_eq!(order(&app), vec![2, 1, 3]);
    assert_eq!(app.tabs[app.active].id, 3);

    // No-op and out-of-range pairs are ignored (defensive: a stale pair can
    // arrive from a frame that also closed a tab).
    let mut app = app_with(vec![Tab::new(1), Tab::new(2)], 1);
    app.move_tab(0, 0);
    app.move_tab(1, 5);
    app.move_tab(5, 0);
    assert_eq!(order(&app), vec![1, 2]);
    assert_eq!(app.active, 1);
}

/// A reorder changes the URLs the session snapshot remembers per index, so
/// "restore tabs on startup" persists the new order with no format change.
#[test]
fn move_tab_marks_the_session_for_save() {
    let mut a = Tab::new(1);
    a.current = Some(url::Url::parse("https://ex.com/a/").unwrap());
    let mut b = Tab::new(2);
    b.current = Some(url::Url::parse("https://ex.com/b/").unwrap());
    let mut app = app_with(vec![a, b], 0);
    app.session_restore_enabled = true;
    app.session_saved = super::session_snapshot(&app.tabs);
    app.session_saved_active = app.active;
    assert!(!super::session_needs_save(&app), "the start state is saved");
    app.move_tab(0, 1);
    assert!(
        super::session_needs_save(&app),
        "the reordered session must be written"
    );
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

/// A stored window on a secondary monitor is clamped to THAT monitor, not to
/// the primary one the native window was created on. This used to shrink a
/// window remembered on a larger display to the primary monitor's bounds.
#[test]
fn stored_window_geometry_clamps_to_the_monitor_it_overlaps() {
    use eframe::egui;
    let primary = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let secondary = egui::Rect::from_min_size(egui::pos2(1920.0, 0.0), egui::vec2(2560.0, 1440.0));
    let stored = WindowGeometry {
        pos: egui::pos2(2000.0, 100.0),
        size: egui::vec2(2400.0, 1300.0),
        maximized: false,
    };
    let target = stored_target(
        stored,
        &[primary, secondary],
        Some(primary),
        egui::Vec2::ZERO,
        1.0,
    )
    .unwrap();
    assert_eq!(
        target, stored,
        "a window on the larger secondary monitor keeps its size"
    );

    // A rectangle straddling both displays belongs to the one it mostly
    // occupies; the old code always used the creation (primary) monitor.
    let straddling = WindowGeometry {
        pos: egui::pos2(1800.0, 100.0),
        size: egui::vec2(2500.0, 1000.0),
        maximized: false,
    };
    let target = stored_target(
        straddling,
        &[primary, secondary],
        Some(primary),
        egui::Vec2::ZERO,
        1.0,
    )
    .unwrap();
    assert_eq!(
        target.size,
        egui::vec2(2500.0, 1000.0),
        "the secondary monitor has room for the full window"
    );
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

/// Esc ownership: when another surface claims the key (a focused address
/// field, the inline tile editor, a floating window, the page-info card or a
/// popup), the find bar must NOT close on the same press — one Esc used to
/// close the bar AND act on the surface behind it.
#[test]
fn find_bar_leaves_esc_to_the_surface_that_owns_it() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(vec![loaded_tab(1, "https://ex.com/a/", Some("A"))], 0);
    let escape = || eframe::egui::Event::Key {
        key: eframe::egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::NONE,
    };
    let cmd_key = eframe::egui::Event::Key {
        key: eframe::egui::Key::F,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::COMMAND,
    };
    let frame = |ctx: &eframe::egui::Context,
                 app: &mut super::MdowserApp,
                 events: Vec<eframe::egui::Event>| {
        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            app.handle_shortcuts(ctx);
            let active = app.active;
            crate::viewer::overlays::find::show_find_bar(root, &mut app.tabs[active]);
        });
        out.textures_delta.clear();
    };
    frame(&ctx, &mut app, vec![cmd_key]);
    frame(&ctx, &mut app, vec![]);
    assert!(app.tabs[0].find.open, "Ctrl/Cmd+F opened the bar");
    // The address field owned the keyboard last frame: the bar must leave
    // Esc alone (and so must Stop).
    app.tabs[0].address_focused = true;
    frame(&ctx, &mut app, vec![escape()]);
    assert!(
        app.tabs[0].find.open,
        "a focused address field keeps Esc; the bar stays open"
    );
    // With nothing claiming the key, the next press closes the bar.
    frame(&ctx, &mut app, vec![escape()]);
    assert!(!app.tabs[0].find.open, "Esc closed the bar");
}

/// `Ctrl/Cmd+F` on a loaded page opens the find bar, and the bar's next
/// frame gives the query field the keyboard with the previous query
/// SELECTED (typing replaces it). Esc closes; the query survives for the
/// next open.
#[test]
fn find_shortcut_focuses_and_selects_the_query() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(vec![loaded_tab(1, "https://ex.com/a/", Some("A"))], 0);
    let tab_id = app.tabs[0].id;
    let edit_id = eframe::egui::Id::unique(("find-query", tab_id));
    let key = |modifiers: eframe::egui::Modifiers| eframe::egui::Event::Key {
        key: eframe::egui::Key::F,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    // One frame with Ctrl/Cmd+F: `handle_shortcuts` sets the open+focus
    // request, the bar (drawn later in the same pass) consumes it on its
    // first REAL frame.
    let frame = |ctx: &eframe::egui::Context,
                 app: &mut super::MdowserApp,
                 events: Vec<eframe::egui::Event>| {
        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            app.handle_shortcuts(ctx);
            let active = app.active;
            crate::viewer::overlays::find::show_find_bar(root, &mut app.tabs[active]);
        });
        out.textures_delta.clear();
    };
    frame(&ctx, &mut app, vec![key(eframe::egui::Modifiers::COMMAND)]);
    assert!(app.tabs[0].find.open, "Ctrl/Cmd+F opened the bar");
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(
        ctx.memory(|m| m.has_focus(edit_id)),
        "the query field owns the keyboard"
    );
    // Type a query the usual way, then close with Esc and reopen with the
    // shortcut: the query comes back selected.
    for ch in ["a", "b"] {
        frame(
            &ctx,
            &mut app,
            vec![eframe::egui::Event::Text(ch.to_string())],
        );
    }
    assert_eq!(app.tabs[0].find.query, "ab");
    frame(
        &ctx,
        &mut app,
        vec![eframe::egui::Event::Key {
            key: eframe::egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: eframe::egui::Modifiers::NONE,
        }],
    );
    assert!(!app.tabs[0].find.open, "Esc closed the bar");
    frame(&ctx, &mut app, vec![key(eframe::egui::Modifiers::COMMAND)]);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(ctx.memory(|m| m.has_focus(edit_id)), "focused again");
    let state = eframe::egui::TextEdit::load_state(&ctx, edit_id).expect("field state");
    let range = state.cursor.char_range().expect("query selected");
    let (a, b) = (range.primary.index.0, range.secondary.index.0);
    assert_eq!(
        (a.min(b), a.max(b)),
        (0, 2),
        "the kept query comes up selected"
    );
}

/// Fresh scratch directory for the save tests (the stores' own recipe:
/// `pid`-tagged, so parallel test processes never share one).
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mdowser-app-save-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A loaded page whose Markdown source is non-trivial (the save tests read
/// the bytes) and whose outline is real, so the outline panel can draw it.
fn loaded_tab_with_outline(id: usize, url: &str, title: &str) -> Tab {
    let mut tab = loaded_tab(id, url, Some(title));
    let TabState::Loaded { fetched } = &mut tab.state else {
        panic!("loaded_tab builds a Loaded tab");
    };
    fetched.source = "# Title\n\nBody\n".to_string();
    fetched.plain_text = "Title\nBody\n".to_string();
    let events = crate::markdown::document::parse_events("# Title\n").expect("parse");
    fetched.outline =
        crate::markdown::outline::build(&events, &crate::markdown::anchors::build(&events));
    tab
}

/// `Ctrl/Cmd+S` (and the context menu's `Save page`, which shares
/// `save_page`) writes the captured Markdown straight into the downloads
/// folder under the page's own name; a repeat save picks the browser-style
/// copy name and never overwrites the file already there.
#[test]
fn save_page_writes_markdown_named_after_the_page() {
    let dir = scratch("named");
    let mut app = app_with(
        vec![loaded_tab_with_outline(
            1,
            "https://ex.com/guide/",
            "Guía rápida",
        )],
        0,
    );
    app.save_page_to(Some(&dir));
    let target = dir.join("guía-rápida.md");
    assert_eq!(
        std::fs::read_to_string(&target).expect("file written"),
        "# Title\n\nBody\n",
        "the Markdown source, exactly as captured"
    );
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Saved guía-rápida.md")
    );
    app.save_page_to(Some(&dir));
    assert_eq!(
        std::fs::read_to_string(dir.join("guía-rápida (1).md")).expect("copy written"),
        "# Title\n\nBody\n"
    );
    assert_eq!(
        std::fs::read_to_string(&target).expect("first kept"),
        "# Title\n\nBody\n",
        "the file already on disk stays untouched"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Home (or any page that renders no document) has nothing to capture:
/// `save_page` says so with a toast and writes nothing.
#[test]
fn save_page_without_a_document_toasts() {
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.save_page();
    assert_eq!(
        app.active_tab_mut().toast_text.as_deref(),
        Some("Nothing to save")
    );
}

/// `Ctrl/Cmd+S` is the keyboard entry point of the same write (the context
/// menu's `Save page` row is the pointer one), and a document page is its
/// only target: on Home the key is not claimed and nothing is written.
#[test]
fn save_shortcut_writes_on_document_pages_only() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let key = || eframe::egui::Event::Key {
        key: eframe::egui::Key::S,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::COMMAND,
    };
    let frame = |ctx: &eframe::egui::Context, app: &mut MdowserApp| {
        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(900.0, 700.0),
            )),
            events: vec![key()],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |_root| {
            app.handle_shortcuts(ctx);
        });
        out.textures_delta.clear();
    };
    let dir = scratch("shortcut");
    let mut page = app_with(
        vec![loaded_tab_with_outline(1, "https://ex.com/a/", "A")],
        0,
    );
    page.save_dir = Some(dir.clone());
    frame(&ctx, &mut page);
    assert_eq!(
        std::fs::read_to_string(dir.join("a.md")).expect("shortcut saved"),
        "# Title\n\nBody\n"
    );
    assert_eq!(
        page.active_tab_mut().toast_text.as_deref(),
        Some("Saved a.md")
    );
    let mut home = app_with(vec![Tab::new(1)], 0);
    frame(&ctx, &mut home);
    assert_eq!(
        home.active_tab_mut().toast_text,
        None,
        "Home does not claim the key (the menu row explains it instead)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Esc ownership of the reading surfaces: the find bar owns its press. The
/// permanent outline sidebar is layout, not a dismissible overlay, so it
/// never competes for the key.
#[test]
fn esc_closes_the_find_bar() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(
        vec![loaded_tab_with_outline(1, "https://ex.com/a/", "A")],
        0,
    );
    let generation = app.tabs[0].doc_generation;
    app.tabs[0].find.show(generation);
    let escape = || eframe::egui::Event::Key {
        key: eframe::egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::NONE,
    };
    let input = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(900.0, 700.0),
        )),
        events: vec![escape()],
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        app.handle_shortcuts(&ctx);
        let active = app.active;
        crate::viewer::overlays::find::show_find_bar(root, &mut app.tabs[active]);
    });
    out.textures_delta.clear();
    assert!(!app.tabs[0].find.open, "the bar owned the press");
}

/// Runs one frame with a single key chord, exactly as a shortcut press
/// reaches `handle_shortcuts`.
fn press_shortcut(
    ctx: &eframe::egui::Context,
    app: &mut super::MdowserApp,
    modifiers: eframe::egui::Modifiers,
    key: eframe::egui::Key,
) {
    let input = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(900.0, 700.0),
        )),
        events: vec![eframe::egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |_root| app.handle_shortcuts(ctx));
    out.textures_delta.clear();
}

/// Browser Back/Forward: Alt+Left/Right traverse the page history, while a
/// focused bookmark tile keeps Alt+Left/Right for its own grid reorder.
#[test]
fn back_and_forward_shortcuts_leave_a_focused_tile_alone() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(vec![Tab::new(1)], 0);
    app.tabs[0].open_history(true);
    assert!(matches!(app.tabs[0].state, TabState::History));

    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::ALT,
        eframe::egui::Key::ArrowLeft,
    );
    assert!(
        matches!(app.tabs[0].state, TabState::Home),
        "Alt+Left went back to the new-tab page"
    );
    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::ALT,
        eframe::egui::Key::ArrowRight,
    );
    assert!(
        matches!(app.tabs[0].state, TabState::History),
        "Alt+Right went forward again"
    );

    app.tabs[0].tile_focused = true;
    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::ALT,
        eframe::egui::Key::ArrowLeft,
    );
    assert!(
        matches!(app.tabs[0].state, TabState::History),
        "a focused bookmark tile keeps Alt+Left for its reorder"
    );
    app.tabs[0].tile_focused = false;
    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::ALT,
        eframe::egui::Key::ArrowLeft,
    );
    assert!(
        matches!(app.tabs[0].state, TabState::Home),
        "with the grid unfocused, Back works again"
    );
}

/// Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PageDown/PageUp cycle the tab rail
/// and wrap at both ends.
#[test]
fn tab_cycle_shortcuts_wrap() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(vec![Tab::new(1), Tab::new(2), Tab::new(3)], 0);
    let ctrl = eframe::egui::Modifiers::CTRL;

    press_shortcut(&ctx, &mut app, ctrl, eframe::egui::Key::Tab);
    assert_eq!(app.active, 1);
    press_shortcut(
        &ctx,
        &mut app,
        ctrl | eframe::egui::Modifiers::SHIFT,
        eframe::egui::Key::Tab,
    );
    assert_eq!(app.active, 0);
    press_shortcut(&ctx, &mut app, ctrl, eframe::egui::Key::PageDown);
    assert_eq!(app.active, 1);
    press_shortcut(&ctx, &mut app, ctrl, eframe::egui::Key::PageUp);
    assert_eq!(app.active, 0);
    // One step back from the first tab wraps to the last.
    press_shortcut(
        &ctx,
        &mut app,
        ctrl | eframe::egui::Modifiers::SHIFT,
        eframe::egui::Key::Tab,
    );
    assert_eq!(app.active, 2);
}

/// Ctrl/Cmd+Shift+T reopens the last closed tab (and is not mistaken for
/// Ctrl/Cmd+T: the reopened tab keeps its page, rather than becoming Home).
#[test]
fn reopen_closed_tab_restores_the_page() {
    let ctx = eframe::egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut app = app_with(vec![Tab::new(1), Tab::new(2)], 0);
    app.tabs[1].open_history(true);
    app.close_tab(&ctx, 1);
    assert_eq!(app.tabs.len(), 1, "the history tab was closed");

    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::COMMAND | eframe::egui::Modifiers::SHIFT,
        eframe::egui::Key::T,
    );
    assert_eq!(app.tabs.len(), 2, "the closed tab came back");
    assert_eq!(app.active, 1, "and is active");
    assert!(
        matches!(app.tabs[1].state, TabState::History),
        "the remembered URL was reopened, not a blank tab"
    );
    // Nothing left on the stack: another press is a no-op.
    press_shortcut(
        &ctx,
        &mut app,
        eframe::egui::Modifiers::COMMAND | eframe::egui::Modifiers::SHIFT,
        eframe::egui::Key::T,
    );
    assert_eq!(app.tabs.len(), 2);
}
