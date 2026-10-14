// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::tests::{fetched_for, loading_tab, settling_tab, u};
use super::{Tab, TabState};
use crate::error::BrowserError;
use crate::protocol::client::Fetched;
use poll_promise::Promise;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

/// While settling, the chrome still names the pending load (titles,
/// favicon origin, file indicator, action target) and refresh stays
/// disabled; bookmarks wait for a shown page.
#[test]
fn settling_counts_as_loading_everywhere() {
    let t = settling_tab();
    assert!(t.is_loading());
    assert!(t.title().contains("ex.com/new/"));
    assert!(t.window_title().contains("ex.com/new/"));
    assert!(!t.can_refresh());
    assert!(!t.can_retry());
    assert!(t.bookmark_target().is_none());
    assert!(t.loaded_url().is_none());
    assert!(!t.is_local_file());
    assert_eq!(
        t.action_target().map(|x| x.as_str().to_string()),
        Some("https://ex.com/new/".to_string())
    );
    assert_eq!(
        t.favicon_origin().map(|x| x.as_str().to_string()),
        Some("https://ex.com/".to_string())
    );
    // Backdrop on screen: a local rejection would be a toast, so the
    // internal `shows_document` gate holds.
    assert!(t.shows_document());
    let mut first = Tab::new(9);
    first.state = TabState::Settling {
        asked: "https://ex.com/new/".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        pending: Box::new(fetched_for("https://ex.com/new/")),
        settling: Box::new(crate::media::cache::ImageCache::new()),
        gate_started: Instant::now(),
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
    };
    assert!(!first.shows_document(), "no backdrop on a first load");
}

/// Ctrl/Cmd+D target: only a LOADED document offers a bookmark; the raw
/// page title travels with it (the store sanitizes it).
#[test]
fn bookmark_target_only_exists_for_loaded_documents() {
    assert!(Tab::new(1).bookmark_target().is_none(), "Home");
    let mut failed = Tab::new(2);
    failed.state = TabState::Failed {
        asked: "x".to_string(),
        lines: Vec::new(),
    };
    assert!(failed.bookmark_target().is_none(), "Failed");
    let mut history = Tab::new(3);
    history.open_history(true);
    assert!(history.bookmark_target().is_none(), "History");
    let mut licenses = Tab::new(5);
    licenses.open_licenses(true);
    assert!(licenses.bookmark_target().is_none(), "Licenses");
    let mut loaded = Tab::new(4);
    let mut fetched = fetched_for("https://ex.com/a/");
    fetched.meta.title = Some("Page A".to_string());
    loaded.state = TabState::Loaded { fetched };
    let (url, title) = loaded.bookmark_target().expect("loaded target");
    assert_eq!(url.as_str(), "https://ex.com/a/");
    assert_eq!(title.as_deref(), Some("Page A"));
}

/// The internal history page opens from the address text without touching
/// the network, records no visit and lands on `about:history`. The fresh
/// Home tab left behind is a real entry (`about:home` moves to `back`),
/// so Back returns to it.
#[test]
fn history_input_opens_the_internal_page() {
    let mut t = Tab::new(1);
    t.navigate_to_text("about:history", true);
    assert!(matches!(t.state, TabState::History));
    assert_eq!(t.address, "about:history");
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:history"));
    assert!(t.pending_visit.is_none(), "internal page records no visit");
    assert!(!t.is_loading());
    assert_eq!(
        t.back.iter().map(Url::as_str).collect::<Vec<_>>(),
        vec!["about:home"],
        "the Home tab is Back's target"
    );
    // Case-insensitive and repeat-safe (a second open is a no-op).
    t.navigate_to_text("ABOUT:HISTORY", true);
    assert!(matches!(t.state, TabState::History));
    assert_eq!(
        t.back.iter().map(Url::as_str).collect::<Vec<_>>(),
        vec!["about:home"],
        "no duplicate push"
    );
}

/// Opening the history page is a real navigation for back/forward: the
/// current page moves to `back` and Back returns to it.
#[test]
fn history_page_pushes_the_previous_page_on_back() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.navigate_to_text("about:history", true);
    assert!(matches!(t.state, TabState::History));
    assert_eq!(t.back.last().map(Url::as_str), Some("https://ex.com/a/"));
    // Back leaves the history page and returns to the stored page.
    let back = t.prepare_back();
    assert_eq!(back.as_ref().map(Url::as_str), Some("https://ex.com/a/"));
}

/// The internal licenses page opens from the address text without
/// touching the network, records no visit, lands on `about:licenses` and
/// behaves like a real navigation for Back.
#[test]
fn licenses_input_opens_the_internal_page() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.navigate_to_text("about:licenses", true);
    assert!(matches!(t.state, TabState::Licenses));
    assert_eq!(t.address, "about:licenses");
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:licenses"));
    assert!(t.pending_visit.is_none(), "internal page records no visit");
    assert!(!t.is_loading());
    assert_eq!(t.title(), "Licenses");
    assert_eq!(t.window_title(), "Licenses");
    // Real navigation: the page left behind is Back's target, and Back
    // returns the tab to it.
    assert_eq!(t.back.last().map(Url::as_str), Some("https://ex.com/a/"));
    let back = t.prepare_back();
    assert_eq!(back.as_ref().map(Url::as_str), Some("https://ex.com/a/"));

    // Case-insensitive and repeat-safe (a second open is a no-op), and
    // Esc restores the internal URL in the address field.
    let mut t = Tab::new(2);
    t.navigate_to_text("ABOUT:LICENSES", true);
    assert!(matches!(t.state, TabState::Licenses));
    assert_eq!(
        t.back.iter().map(Url::as_str).collect::<Vec<_>>(),
        vec!["about:home"],
        "no duplicate push beyond the Home entry"
    );
    t.address = "typing".to_string();
    t.revert_address();
    assert_eq!(t.address, "about:licenses");
}

/// "Home" from the internal licenses page returns to the landing page
/// (`about:home` as `current`, empty field), pushing the licenses page
/// on `back`. Refresh stays put (Home never reloads).
#[test]
fn home_from_licenses_returns_to_the_landing_page() {
    let mut t = Tab::new(1);
    t.navigate_to_text("about:licenses", true);
    assert!(matches!(t.state, TabState::Licenses));
    t.go_home();
    assert!(matches!(t.state, TabState::Home));
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    assert!(t.address.is_empty());
    assert_eq!(
        t.back.last().map(Url::as_str),
        Some("about:licenses"),
        "Back returns to the licenses page"
    );
    t.refresh();
    assert!(matches!(t.state, TabState::Home));
}

/// "Home" from the internal history page returns to the landing page
/// (`about:home` as `current`, empty field), pushing the history page
/// on `back`. Refresh stays put (Home never reloads).
#[test]
fn home_from_history_returns_to_the_landing_page() {
    let mut t = Tab::new(1);
    t.navigate_to_text("about:history", true);
    assert!(matches!(t.state, TabState::History));
    t.go_home();
    assert!(matches!(t.state, TabState::Home));
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    assert!(t.address.is_empty());
    assert_eq!(
        t.back.last().map(Url::as_str),
        Some("about:history"),
        "Back returns to the history page"
    );
    // Refresh on the landing page has no target: nothing reopens.
    t.refresh();
    assert!(matches!(t.state, TabState::Home));
}

/// "Home" from a loaded page lands on the new tab page (`about:home`,
/// empty field) while the page left behind stays reachable with Back —
/// and the landing page itself stays reachable with Forward.
#[test]
fn home_from_a_loaded_page_returns_to_the_new_tab_page() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.address = "ex.com/a/".to_string();
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    t.go_home();
    assert!(matches!(t.state, TabState::Home));
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    assert!(t.address.is_empty(), "address field cleared");
    assert_eq!(
        t.back.last().map(Url::as_str),
        Some("https://ex.com/a/"),
        "Back returns to the page left behind"
    );
    assert_eq!(
        t.prepare_back().map(|x| x.as_str().to_string()),
        Some("https://ex.com/a/".to_string())
    );
    assert_eq!(
        t.forward.iter().map(Url::as_str).collect::<Vec<_>>(),
        vec!["about:home"],
        "Forward returns to the landing page"
    );
}

/// Typing `about:home` opens the landing page from anywhere without
/// touching the network: empty field, `about:home` as `current`.
/// Case-insensitive and repeat-safe.
#[test]
fn home_input_opens_the_landing_page() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    t.navigate_to_text("about:home", true);
    assert!(matches!(t.state, TabState::Home));
    assert!(t.address.is_empty(), "the capsule keeps its hint");
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    assert!(t.pending_visit.is_none(), "internal page records no visit");
    assert_eq!(
        t.back.last().map(Url::as_str),
        Some("https://ex.com/a/"),
        "Back returns to the page left behind"
    );
    // Repeat-safe (a second open clears a typed draft but pushes nothing).
    t.address = "typing".to_string();
    t.navigate_to_text("ABOUT:HOME", true);
    assert!(matches!(t.state, TabState::Home));
    assert!(t.address.is_empty());
    assert_eq!(
        t.back.iter().map(Url::as_str).collect::<Vec<_>>(),
        vec!["https://ex.com/a/"],
        "no duplicate push"
    );
}

/// Back and Forward traverse the landing page like any other entry: no
/// stack is lost by passing through Home. Internal pages only, so the
/// test never touches the network.
#[test]
fn back_and_forward_traverse_the_landing_page() {
    let mut t = Tab::new(1);
    t.navigate_to_text("about:history", true);
    t.navigate_to_text("about:licenses", true);
    // Back: licenses -> history -> Home (empty capsule back).
    t.go_back();
    assert!(matches!(t.state, TabState::History));
    t.go_back();
    assert!(matches!(t.state, TabState::Home));
    assert!(t.address.is_empty(), "Back lands on an empty capsule");
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    // Forward: Home -> history -> licenses (nothing lost).
    t.go_forward();
    assert!(matches!(t.state, TabState::History));
    t.go_forward();
    assert!(matches!(t.state, TabState::Licenses));
    assert!(t.forward.is_empty() && t.back.len() == 2);
}

/// "Home" while a load is pending commits the pending URL as the page
/// left behind and cancels the fetch (no orphan thread result lands on
/// the landing page).
#[test]
fn home_abandons_a_pending_load() {
    let mut t = loading_tab(
        "https://ex.com/pending/",
        Some(u("https://ex.com/old/")),
        vec![],
        vec![],
    );
    t.go_home();
    assert!(matches!(t.state, TabState::Home));
    assert_eq!(t.current.as_ref().map(Url::as_str), Some("about:home"));
    assert!(t.address.is_empty());
    assert_eq!(
        t.back.last().map(Url::as_str),
        Some("https://ex.com/pending/")
    );
}

/// A failed load (even a cancellation) reports no history visit.
#[test]
fn failed_load_reports_no_history_visit() {
    let promise: Promise<Result<Fetched, BrowserError>> =
        Promise::from_ready(Err(BrowserError::Cancelled));
    let mut t = Tab::new(1);
    t.state = TabState::Loading {
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
    assert!(t.poll_state());
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert!(t.pending_visit.is_none());
}

#[test]
fn cooldown_pending_only_while_counting_down() {
    let mut t = Tab::new(1);
    // Home: no navigation yet -> nothing to tick (must stay idle).
    assert!(!t.cooldown_pending());
    // Fresh stamp -> countdown running.
    t.last_navigate = Some(Instant::now());
    assert!(t.cooldown_pending());
    // Old stamp -> cooldown over.
    t.last_navigate =
        Some(Instant::now() - Duration::from_secs(crate::config::REFRESH_COOLDOWN_SECS + 1));
    assert!(!t.cooldown_pending());
}

#[test]
fn action_target_prefers_the_failed_url() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/old/"));
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/old/"),
    };
    assert_eq!(
        t.action_target().map(|x| x.as_str().to_string()),
        Some("https://ex.com/old/".to_string())
    );
    // Failed: the failed `asked` wins over the stale `current`.
    t.state = TabState::Failed {
        asked: "https://ex.com/new/".to_string(),
        lines: vec![],
    };
    assert_eq!(
        t.action_target().map(|x| x.as_str().to_string()),
        Some("https://ex.com/new/".to_string())
    );
    // Failed with an unparseable `asked`: fall back to `current`.
    t.state = TabState::Failed {
        asked: "mailto:a@b.com".to_string(),
        lines: vec![],
    };
    assert_eq!(
        t.action_target().map(|x| x.as_str().to_string()),
        Some("https://ex.com/old/".to_string())
    );

    // Loading: the pending URL wins over the stale `current` too.
    let t = loading_tab(
        "https://ex.com/pending/",
        Some(u("https://ex.com/old/")),
        vec![],
        vec![],
    );
    assert_eq!(
        t.action_target().map(|x| x.as_str().to_string()),
        Some("https://ex.com/pending/".to_string())
    );
}

#[test]
fn heal_self_duplicates_drops_matching_top_entries() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.back = vec![u("https://ex.com/b/"), u("https://ex.com/a/")];
    t.forward = vec![u("https://ex.com/a/")];
    t.heal_self_duplicates(&u("https://ex.com/a/"));
    assert_eq!(t.back, vec![u("https://ex.com/b/")], "back healed");
    assert!(t.forward.is_empty(), "forward healed");
}

#[test]
fn commit_pending_navigation_moves_current_to_the_pending_url() {
    let mut t = loading_tab(
        "https://ex.com/b",
        Some(u("https://ex.com/a/")),
        vec![u("https://ex.com/a/")],
        vec![],
    );
    t.commit_pending_navigation();
    assert_eq!(t.current, Some(u("https://ex.com/b/")));
}

/// Stop restores the backdrop page, its history snapshots and the
/// normalized address, and flags the abandoned background fetch.
#[test]
fn stop_restores_the_backdrop_and_cancels() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    // State right after the abandoned navigation pushed A (the page now
    // restored as the backdrop): back = [hist, A].
    t.back = vec![u("https://ex.com/hist/"), u("https://ex.com/a/")];
    t.address = "ex.com/pending/".to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::Cancelled)),
        asked: "https://ex.com/pending/".to_string(),
        address_at_start: t.address.clone(),
        started: Instant::now(),
        backdrop: Some(Box::new(fetched_for("https://ex.com/a/"))),
        saved_back: vec![u("https://ex.com/hist/")],
        saved_forward: vec![u("https://ex.com/fwd/")],
        pushed_history: true,
        cancel: cancel.clone(),
    };
    t.stop();
    assert!(cancel.load(Ordering::Relaxed), "background fetch cancelled");
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert_eq!(t.current, Some(u("https://ex.com/a/")));
    assert_eq!(t.address, "ex.com/a/", "address follows the restored page");
    // Same rule as a failed fetch: the stack the abandoned navigation
    // pushed is kept, and the restored backdrop heals its own entry off it
    // (a self-entry would only reload the page shown), so the remaining
    // pre-navigation entries stay reachable with Back.
    assert_eq!(t.back, vec![u("https://ex.com/hist/")]);
    assert_eq!(t.forward, vec![u("https://ex.com/fwd/")]);
}

/// A failed NEW navigation keeps the page the user came from reachable:
/// the pushed entry must survive the failure restore.
#[test]
fn failed_new_navigation_keeps_the_previous_page_on_back() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    // `navigate_to_url(B)` pushed A: back = [X, A].
    t.back = vec![u("https://ex.com/x/"), u("https://ex.com/a/")];
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::NoCandidateWorked {
            asked: "https://ex.com/b/".to_string(),
            lines: vec!["boom".to_string()],
        })),
        asked: "https://ex.com/b/".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        // Pre-push snapshots: restoring `back` would lose A.
        saved_back: vec![u("https://ex.com/x/")],
        saved_forward: vec![],
        pushed_history: true,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state(), "state changed");
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert_eq!(
        t.back.last(),
        Some(&u("https://ex.com/a/")),
        "previous page stays reachable with Back"
    );
    assert_eq!(t.current, Some(u("https://ex.com/a/")));
}

/// A failed Back/Forward navigation restores the ORIGINAL stacks (the
/// target was already moved out; losing it would corrupt history).
#[test]
fn failed_back_navigation_restores_the_original_stacks() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.back = vec![u("https://ex.com/x/")];
    t.forward = vec![u("https://ex.com/b/")];
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::NoCandidateWorked {
            asked: "https://ex.com/a/".to_string(),
            lines: vec!["boom".to_string()],
        })),
        asked: "https://ex.com/a/".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: vec![u("https://ex.com/x/")],
        saved_forward: vec![u("https://ex.com/b/")],
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    t.poll_state();
    assert_eq!(t.back, vec![u("https://ex.com/x/")]);
    assert_eq!(t.forward, vec![u("https://ex.com/b/")]);
}

/// Stop on a first load (no backdrop) settles into a quiet Failed log
/// and keeps the requested address for context.
#[test]
fn stop_without_backdrop_settles_into_failure() {
    let mut t = Tab::new(1);
    t.address = "ex.com/slow/".to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::Cancelled)),
        asked: "https://ex.com/slow/".to_string(),
        address_at_start: t.address.clone(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: cancel.clone(),
    };
    t.stop();
    assert!(cancel.load(Ordering::Relaxed), "background fetch cancelled");
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert_eq!(t.address, "ex.com/slow/");
}

/// Stop on a tab that is not loading is a real no-op: the document and
/// history must survive (the `mem::replace` parking trick must never
/// reach a non-loading state).
#[test]
fn stop_is_a_noop_when_not_loading() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    t.stop();
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert_eq!(t.current, Some(u("https://ex.com/a/")));
}
