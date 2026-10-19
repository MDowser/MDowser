// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{anchor_from_url, Tab, TabState};
use crate::error::BrowserError;
use crate::protocol::client::Fetched;
use eframe::egui;
use poll_promise::Promise;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

pub(super) fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

/// A tab in `Loading` whose promise is already resolved (never fetched).
pub(super) fn loading_tab(
    asked: &str,
    current: Option<Url>,
    back: Vec<Url>,
    forward: Vec<Url>,
) -> Tab {
    let mut t = Tab::new(1);
    t.current = current;
    t.back = back;
    t.forward = forward;
    let promise: Promise<Result<Fetched, BrowserError>> =
        Promise::from_ready(Err(BrowserError::Cancelled));
    t.state = TabState::Loading {
        promise,
        asked: asked.to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: t.back.clone(),
        saved_forward: t.forward.clone(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    t
}

#[test]
fn back_commits_the_pending_navigation() {
    // A is loaded, B is loading (Back target = A). Back must move to A
    // and leave B (the pending page) in forward, not push the stale A.
    let mut t = loading_tab(
        "https://ex.com/b",
        Some(u("https://ex.com/a/")),
        vec![u("https://ex.com/a/")],
        vec![],
    );
    let prev = t.prepare_back().expect("back target");
    assert_eq!(prev.as_str(), "https://ex.com/a/");
    assert!(t.back.is_empty());
    assert_eq!(t.forward, vec![u("https://ex.com/b/")]);
    assert_eq!(t.current, Some(u("https://ex.com/b/")));
}

#[test]
fn forward_commits_the_pending_navigation() {
    // B is loaded, C is loading, forward target is A (different): the
    // pending C becomes the current position and goes to `back`.
    let mut t = loading_tab(
        "https://ex.com/c",
        Some(u("https://ex.com/b/")),
        vec![],
        vec![u("https://ex.com/a/")],
    );
    let next = t.prepare_forward().expect("forward target");
    assert_eq!(next.as_str(), "https://ex.com/a/");
    assert!(t.forward.is_empty());
    assert_eq!(t.back, vec![u("https://ex.com/c/")]);
    assert_eq!(t.current, Some(u("https://ex.com/c/")));
}

#[test]
fn local_rejection_while_loading_restores_history_snapshots() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/b"));
    t.back = vec![u("https://ex.com/a/")];
    t.forward = Vec::new(); // cleared by the new navigation
    let saved_forward = vec![u("https://ex.com/c/")];
    let promise: Promise<Result<Fetched, BrowserError>> =
        Promise::from_ready(Err(BrowserError::Cancelled));
    t.state = TabState::Loading {
        promise,
        asked: "https://ex.com/b".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: vec![u("https://ex.com/a/")],
        saved_forward: saved_forward.clone(),
        pushed_history: true,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    t.fail_locally("mailto:x".to_string(), vec!["not https".to_string()]);
    // Same guarantees as a failed network load: the page pushed by the
    // pending navigation stays on `back`, Forward is restored.
    assert_eq!(t.back, vec![u("https://ex.com/a/")]);
    assert_eq!(t.forward, saved_forward, "Forward entries are not lost");
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert!(t.last_navigate.is_some(), "cooldown stamped");
}

#[test]
fn local_rejection_restores_the_original_back_stack() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/b"));
    t.back = vec![u("https://ex.com/old/")];
    t.forward = vec![u("https://ex.com/c/")];
    let promise: Promise<Result<Fetched, BrowserError>> =
        Promise::from_ready(Err(BrowserError::Cancelled));
    t.state = TabState::Loading {
        promise,
        asked: "https://ex.com/b".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: vec![u("https://ex.com/previous/")],
        saved_forward: vec![u("https://ex.com/c/")],
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    t.fail_locally("bad input".to_string(), vec!["bad".to_string()]);
    // Not a pushed navigation: the pre-navigation back stack comes back.
    assert_eq!(t.back, vec![u("https://ex.com/previous/")]);
    assert_eq!(t.forward, vec![u("https://ex.com/c/")]);
}

/// A locally rejected input while a page is already on screen (a
/// `Loading` backdrop) must keep that page and its in-flight fetch: the
/// rejection becomes a toast, never a destructive error log.
#[test]
fn local_rejection_with_a_backdrop_keeps_the_page_and_the_load() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    let cancel = Arc::new(AtomicBool::new(false));
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::Cancelled)),
        asked: "https://ex.com/pending/".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: Some(Box::new(fetched_for("https://ex.com/a/"))),
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: cancel.clone(),
    };
    let before_address = t.address.clone();
    t.navigate_to_text("mailto:x", true);
    assert!(
        matches!(t.state, TabState::Loading { .. }),
        "the pending load continues"
    );
    assert!(!cancel.load(Ordering::Relaxed), "fetch not cancelled");
    assert!(t.toast_text.is_some(), "rejection explained");
    assert_ne!(t.address, before_address, "address shows the rejection");
}

/// A locally rejected URL must never echo pasted credentials into the
/// omnibox, the error view or "Copy log" (docs/SECURITY.md contract).
#[test]
fn local_rejection_never_echoes_credentials() {
    let mut t = Tab::new(1);
    t.navigate_to_text("user:pass@example.com:bad/", true);
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert!(!t.address.contains("pass"), "address: {}", t.address);
    assert!(
        t.address.contains("example.com"),
        "host context kept: {}",
        t.address
    );
    if let TabState::Failed { asked, lines } = &t.state {
        assert!(!asked.contains("pass"), "asked: {asked}");
        assert!(
            !lines.iter().any(|l| l.contains("pass")),
            "log lines: {lines:?}"
        );
    } else {
        unreachable!("state checked above");
    }
}

#[test]
fn forward_to_the_pending_url_never_mutates_the_stacks() {
    // Loading A while forward.top is also A: already going there, no-op.
    let mut t = loading_tab(
        "https://ex.com/a",
        Some(u("https://ex.com/b/")),
        vec![],
        vec![u("https://ex.com/a/")],
    );
    assert!(t.prepare_forward().is_none());
    assert_eq!(t.forward, vec![u("https://ex.com/a/")], "stacks untouched");
    assert!(t.back.is_empty(), "stacks untouched");
}

#[test]
fn back_to_the_pending_url_never_mutates_the_stacks() {
    // Loading A while back.top is also A: must be a no-op (the old code
    // popped the back entry and pushed it into forward without loading).
    let mut t = loading_tab(
        "https://ex.com/a",
        Some(u("https://ex.com/a/")),
        vec![u("https://ex.com/a/")],
        vec![],
    );
    assert!(t.prepare_back().is_none());
    assert_eq!(t.back, vec![u("https://ex.com/a/")], "stacks untouched");
    assert!(t.forward.is_empty(), "stacks untouched");
}

#[test]
fn self_duplicate_is_not_pushed_forward() {
    // Corrupted stacks (back.top == current): Back must not create a
    // Forward entry pointing at the page already shown.
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.back = vec![u("https://ex.com/a/")];
    let prev = t.prepare_back().expect("back target");
    assert_eq!(prev.as_str(), "https://ex.com/a/");
    assert!(t.back.is_empty());
    assert!(t.forward.is_empty(), "self-duplicate healed");
}

#[test]
fn clear_overlay_resets_everything() {
    let mut t = Tab::new(1);
    t.selected_all = true;
    t.context_open = true;
    t.context_pos = Some(egui::pos2(1.0, 2.0));
    t.context_viewport = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(10.0, 10.0),
    ));
    t.doc_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(10.0, 10.0),
    ));
    t.context_link = Some("https://ex.com/x".to_string());
    t.link_hits.push(crate::viewer::LinkHit {
        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(4.0, 4.0)),
        url: std::sync::Arc::from("https://ex.com/x"),
    });
    t.site_info = Some("address-bar");
    t.clear_overlay();
    assert!(!t.selected_all && !t.context_open);
    assert!(t.context_pos.is_none() && t.context_viewport.is_none());
    assert!(t.context_link.is_none() && t.link_hits.is_empty());
    assert!(t.doc_rect.is_none());
    // The page-info card describes the page on screen: every navigation
    // and tab switch drops its owner, so it can never linger over a
    // different page.
    assert!(!t.site_info_open());
}

/// Minimal `Fetched` for title/state tests (no text, no events).
pub(super) fn fetched_for(url: &str) -> Fetched {
    Fetched {
        display_url: u(url),
        meta: crate::markdown::frontmatter::Meta::default(),
        h1_title: None,
        plain_text: String::new(),
        source: String::new(),
        events: Vec::new(),
        anchors: crate::markdown::anchors::AnchorIndex::default(),
        outline: crate::markdown::outline::Outline::default(),
        dests: crate::protocol::rewrite::DestCache::default(),
        stale_copy: None,
    }
}

#[test]
fn anchor_from_url_reads_the_fragment() {
    assert_eq!(
        anchor_from_url(&u("https://ex.com/a/#section")),
        Some("section".to_string())
    );
    assert_eq!(anchor_from_url(&u("https://ex.com/a/")), None);
    // A bare `#` has no target.
    assert_eq!(anchor_from_url(&u("https://ex.com/a/#")), None);
}

/// A successfully resolved page is handed to `App` (once) for the local
/// history, carrying the raw page title (frontmatter, else first H1);
/// failures never do.
#[test]
fn resolved_page_is_handed_to_history_once() {
    let url = "https://ex.com/a/";
    let mut fetched = fetched_for(url);
    fetched.meta.title = Some("Page A".to_string());
    let promise: Promise<Result<Fetched, BrowserError>> = Promise::from_ready(Ok(fetched));
    let mut t = Tab::new(1);
    t.state = TabState::Loading {
        promise,
        asked: url.to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state());
    assert_eq!(t.pending_visit, Some((u(url), Some("Page A".to_string()))));
    // `App` takes it; nothing is handed twice.
    assert!(t.pending_visit.take().is_some());
    assert!(t.pending_visit.is_none());
}

/// A `Fetched` built from markdown (no fetch, no network), so image-gate
/// tests can carry real image events.
pub(super) fn fetched_from_md(url: &str, md: &str) -> Fetched {
    let base = u(url);
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let anchors = crate::markdown::anchors::build(&events);
    let outline = crate::markdown::outline::build(&events, &anchors);
    let dests = crate::protocol::rewrite::DestCache::build(&base, &anchors, &events);
    Fetched {
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
    }
}

/// A `Settling` tab whose gate cache is empty (settled): the next poll
/// swaps without touching the network.
pub(super) fn settling_tab() -> Tab {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    t.address = "ex.com/new/".to_string();
    t.state = TabState::Settling {
        asked: "https://ex.com/new/".to_string(),
        address_at_start: "ex.com/new/".to_string(),
        started: Instant::now(),
        backdrop: Some(Box::new(fetched_for("https://ex.com/a/"))),
        pending: Box::new(fetched_for("https://ex.com/new/")),
        settling: Box::new(crate::media::cache::ImageCache::new()),
        gate_started: Instant::now(),
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
    };
    t
}

/// The image gate is bounded: it waits long enough for local/small
/// images but never pins the old page anywhere near the per-image job
/// deadline.
#[test]
#[allow(clippy::assertions_on_constants)]
fn doc_image_gate_is_bounded() {
    assert!(crate::config::DOC_IMAGE_GATE_SECS >= 1);
    assert!(crate::config::DOC_IMAGE_GATE_SECS <= 30);
    assert!(crate::config::DOC_IMAGE_GATE_SECS < crate::config::IMAGE_JOB_TIMEOUT_SECS);
}

/// Text-only pages skip the gate: a fetch with no images resolves
/// straight to `Loaded` on the same poll (no extra frame of backdrop).
#[test]
fn text_only_fetch_shows_loaded_without_settling() {
    let url = "https://ex.com/a/";
    let promise: Promise<Result<Fetched, BrowserError>> = Promise::from_ready(Ok(fetched_for(url)));
    let mut t = Tab::new(1);
    t.state = TabState::Loading {
        promise,
        asked: url.to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state());
    assert!(
        matches!(t.state, TabState::Loaded { .. }),
        "no gate without images"
    );
    assert_eq!(t.current, Some(u(url)));
    assert!(t.pending_visit.is_some(), "visit recorded at the swap");
}

/// Inert image destinations never start a fetch: a page whose only
/// image cannot load still resolves on the same poll.
#[test]
fn preload_ignores_inert_images() {
    let fetched = fetched_from_md("https://ex.com/a/", "![a](mailto:x@y.z)\n");
    let mut cache = crate::media::cache::ImageCache::new();
    Tab::preload_doc_images(&mut cache, &fetched);
    assert!(!cache.has_pending(), "nothing to wait for");
}

/// A settled gate swaps atomically: the pending page shows, history and
/// the visit commit at the swap (never at fetch time), and the gate
/// reports the change so the new page paints immediately.
#[test]
fn settling_swap_shows_pending_and_records_visit() {
    let ctx = egui::Context::default();
    let mut t = settling_tab();
    assert!(t.is_loading(), "the indicator stays on during the gate");
    assert!(t.pending_visit.is_none(), "no visit before the swap");
    assert!(t.poll(&ctx), "the swap reports a change");
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert!(!t.is_loading());
    assert_eq!(t.current, Some(u("https://ex.com/new/")));
    assert_eq!(
        t.pending_visit,
        Some((u("https://ex.com/new/"), None)),
        "visit recorded at the swap"
    );
    assert!(t.last_navigate.is_some(), "cooldown stamped at the swap");
}

/// `Stop` during the gate drops the pending page and brings the
/// backdrop back, exactly like stopping a fetch.
#[test]
fn stop_during_settling_restores_backdrop() {
    let mut t = settling_tab();
    t.stop();
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert_eq!(t.current, Some(u("https://ex.com/a/")));
    assert!(t.pending_visit.is_none(), "abandoned page records nothing");
    assert!(!t.is_loading());
}

/// Navigating away mid-gate abandons the pending document but keeps
/// the ORIGINAL backdrop on screen (a local file is used so the new
/// fetch never touches the network).
#[test]
fn navigating_away_from_settling_keeps_original_backdrop() {
    let mut t = settling_tab();
    let target = url::Url::parse("file:///nonexistent-mdowser-test/c.md").unwrap();
    t.navigate_to_url(&target, true);
    match &t.state {
        TabState::Loading {
            backdrop, asked, ..
        } => {
            assert_eq!(
                backdrop.as_ref().map(|b| b.display_url.as_str()),
                Some("https://ex.com/a/"),
                "the first page stays visible, not the pending one"
            );
            assert!(asked.contains("nonexistent-mdowser-test"));
        }
        other => panic!("expected a fresh Loading, got {}", state_name(other)),
    }
}

/// Small helper so the navigation test above reports the state name
/// instead of a bare `TabState` debug dump.
fn state_name(state: &TabState) -> &'static str {
    match state {
        TabState::Home => "Home",
        TabState::History => "History",
        TabState::Licenses => "Licenses",
        TabState::Loading { .. } => "Loading",
        TabState::Settling { .. } => "Settling",
        TabState::Loaded { .. } => "Loaded",
        TabState::Failed { .. } => "Failed",
    }
}

/// A promise that is NOT ready (never resolves while the test runs):
/// the timeout path must be exercised for real, because a READY result
/// now always wins over an expired deadline.
pub(super) fn hung_promise() -> Promise<Result<Fetched, BrowserError>> {
    Promise::spawn_thread("mdowser-test-hung-fetch", || {
        std::thread::sleep(Duration::from_secs(600));
        Err(BrowserError::Cancelled)
    })
}

/// A visual timeout with a page on screen must bring that page back
/// (history + address) and explain the timeout, instead of replacing a
/// readable document with an error log.
#[test]
fn timeout_restores_the_backdrop_instead_of_failing() {
    let mut t = Tab::new(1);
    t.current = Some(u("https://ex.com/a/"));
    // State right after the timed-out navigation pushed A (the page now
    // restored as the backdrop): back = [hist, A].
    t.back = vec![u("https://ex.com/hist/"), u("https://ex.com/a/")];
    t.address = "ex.com/pending/".to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    t.state = TabState::Loading {
        promise: hung_promise(),
        asked: "https://ex.com/pending/".to_string(),
        address_at_start: t.address.clone(),
        started: Instant::now() - Duration::from_secs(crate::config::TOTAL_FETCH_TIMEOUT_SECS + 1),
        backdrop: Some(Box::new(fetched_for("https://ex.com/a/"))),
        saved_back: vec![u("https://ex.com/hist/")],
        saved_forward: vec![u("https://ex.com/fwd/")],
        pushed_history: true,
        cancel: cancel.clone(),
    };
    assert!(t.poll_state(), "state changed");
    assert!(cancel.load(Ordering::Relaxed), "hung fetch cancelled");
    assert!(matches!(t.state, TabState::Loaded { .. }), "page restored");
    assert_eq!(t.current, Some(u("https://ex.com/a/")));
    assert_eq!(t.address, "ex.com/a/", "address follows the restored page");
    // `pushed_history` keeps the stack the navigation pushed, and the
    // restored backdrop heals its own entry off it (an entry equal to the
    // page shown would only reload it): the OTHER pre-navigation entry,
    // here the page before the backdrop, stays reachable.
    assert_eq!(t.back, vec![u("https://ex.com/hist/")]);
    assert_eq!(t.forward, vec![u("https://ex.com/fwd/")]);
    assert!(t.toast_text.is_some(), "timeout explained to the user");
}

/// A first load (no backdrop) still settles into a quiet `Failed` log on
/// timeout: there is no page to bring back.
#[test]
fn timeout_without_backdrop_still_fails() {
    let mut t = Tab::new(1);
    let cancel = Arc::new(AtomicBool::new(false));
    t.state = TabState::Loading {
        promise: hung_promise(),
        asked: "https://ex.com/slow/".to_string(),
        address_at_start: String::new(),
        started: Instant::now() - Duration::from_secs(crate::config::TOTAL_FETCH_TIMEOUT_SECS + 1),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: cancel.clone(),
    };
    assert!(t.poll_state(), "state changed");
    assert!(cancel.load(Ordering::Relaxed));
    assert!(matches!(t.state, TabState::Failed { .. }));
}

/// A resolved load bumps the tab's document generation, re-keying its
/// code-block cache without touching other tabs (the old global
/// `clear_cache` flushed every tab's blocks on any resolve).
#[test]
fn resolving_bumps_the_document_generation() {
    let mut t = Tab::new(1);
    let before = t.doc_generation;
    t.state = TabState::Loading {
        promise: Promise::from_ready(Ok(fetched_for("https://ex.com/a/"))),
        asked: "https://ex.com/a".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state());
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert_ne!(t.doc_generation, before, "generation re-keys the cache");
}

/// A READY result that lands after the deadline must still be delivered
/// (a UI stall must not throw away a finished download).
#[test]
fn ready_result_wins_over_an_expired_deadline() {
    let mut t = Tab::new(1);
    t.state = TabState::Loading {
        promise: Promise::from_ready(Ok(fetched_for("https://ex.com/late/"))),
        asked: "https://ex.com/late/".to_string(),
        address_at_start: String::new(),
        started: Instant::now() - Duration::from_secs(crate::config::TOTAL_FETCH_TIMEOUT_SECS + 1),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state(), "state changed");
    assert!(
        matches!(t.state, TabState::Loaded { .. }),
        "the finished download must be shown"
    );
    assert_eq!(t.current, Some(u("https://ex.com/late/")));
}

/// Esc in the field restores the page actually shown and clears the
/// dirty flag so the next resolve may fill the field again.
#[test]
fn revert_address_restores_the_shown_page() {
    let mut t = Tab::new(1);
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/love/"),
    };
    t.address = "typed garbage".to_string();
    t.address_dirty = true;
    t.revert_address();
    assert_eq!(t.address, "ex.com/love/");
    assert!(!t.address_dirty);
    // Home has nothing to restore: the field clears.
    let mut home = Tab::new(2);
    home.address = "typed".to_string();
    home.revert_address();
    assert!(home.address.is_empty());
}

#[test]
fn window_title_url_fallback_is_bounded() {
    let long = format!("https://ex.com/{}/", "a".repeat(300));
    let mut t = Tab::new(1);
    t.state = TabState::Loaded {
        fetched: fetched_for(&long),
    };
    let title = t.window_title();
    assert!(
        title.chars().count() < 120,
        "window title must be bounded: {} chars",
        title.chars().count()
    );
    assert!(title.starts_with("ex.com/"));
}

/// The display memo (title + favicon origin) must refresh on every
/// state transition that can change it, including a same-kind
/// `Loading → Loading` navigation and a `Failed → Failed` one
/// (`fail_locally` bumps `doc_generation` for exactly this).
#[test]
fn display_memo_refreshes_on_state_changes() {
    let mut t = Tab::new(1);
    assert_eq!(t.display().0.title, "New Tab");
    assert!(t.display().0.origin.is_none(), "Home has no origin");

    // A same-kind Loading → Loading transition (second navigation while
    // the first is still in flight).
    t.state = TabState::Loading {
        promise: poll_promise::Promise::from_ready(Err(crate::error::BrowserError::Cancelled)),
        asked: "https://first.example/a".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.display().0.title.contains("first.example"));
    t.doc_generation = t.doc_generation.wrapping_add(1);
    t.state = TabState::Loading {
        promise: poll_promise::Promise::from_ready(Err(crate::error::BrowserError::Cancelled)),
        asked: "https://second.example/b".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let (disp, rebuilt) = t.display();
    assert!(rebuilt, "the generation bump must rebuild the memo");
    assert!(
        disp.title.contains("second.example"),
        "the memo must show the new URL: {}",
        disp.title
    );
    assert_eq!(
        disp.origin.as_ref().map(Url::as_str),
        Some("https://second.example/")
    );

    // A settled frame hits the memo (no rebuild) and answers identically.
    let (disp, rebuilt) = t.display();
    assert!(!rebuilt, "an unchanged state must not rebuild");
    assert!(disp.title.contains("second.example"));
}

/// A fresh navigation to a new page requests the top: revisiting an
/// already-seen document must not reuse its saved offset (only
/// Back/Forward/Refresh restore it). A local file target keeps the test
/// off the network.
#[test]
fn fresh_navigation_requests_scroll_top() {
    let mut t = Tab::new(1);
    let target = url::Url::parse("file:///nonexistent-mdowser-test/fresh.md").unwrap();
    t.navigate_to_url(&target, true);
    assert!(
        t.scroll_top_requested,
        "a new page starts at the top, even when seen before"
    );
    assert!(matches!(t.state, TabState::Loading { .. }));
}

/// Reloading the same logical document restores the offset like a
/// refresh: no top reset, no history push.
#[test]
fn same_document_reload_keeps_scroll() {
    let mut t = Tab::new(1);
    let target = url::Url::parse("file:///nonexistent-mdowser-test/same.md").unwrap();
    t.current = Some(target.clone());
    t.navigate_to_url(&target, true);
    assert!(
        !t.scroll_top_requested,
        "same-document reload keeps the reading position"
    );
}

/// Back, Forward and Refresh never request the top: egui restores the
/// offset each entry saved.
#[test]
fn back_forward_and_refresh_keep_scroll() {
    let mut t = Tab::new(1);
    t.current = Some(u("file:///b.md"));
    t.back = vec![u("file:///a.md")];
    t.go_back();
    assert!(!t.scroll_top_requested, "Back restores the saved offset");
    t.go_forward();
    assert!(!t.scroll_top_requested, "Forward restores the saved offset");
    // Resolve the pending load so Refresh has a document, then age the
    // cooldown out: Refresh re-downloads in place and keeps the offset.
    t.state = TabState::Loaded {
        fetched: fetched_for("file:///b.md"),
    };
    t.current = Some(u("file:///b.md"));
    t.scroll_top_requested = true;
    t.last_navigate = Some(Instant::now() - Duration::from_secs(60));
    t.refresh();
    assert!(
        !t.scroll_top_requested,
        "Refresh keeps the reading position"
    );
}

/// A locally rejected navigation drops a pending top reset: nothing new
/// renders, so the flag must not leak into the next page.
#[test]
fn local_rejection_drops_scroll_top() {
    let mut t = Tab::new(1);
    t.scroll_top_requested = true;
    t.fail_locally("bad input".to_string(), vec!["bad".to_string()]);
    assert!(!t.scroll_top_requested);
    assert!(matches!(t.state, TabState::Failed { .. }));
}

/// A failed load drops the pending top reset with the history restore,
/// so going Back afterwards still finds its saved offset.
#[test]
fn failed_load_drops_scroll_top() {
    let mut t = Tab::new(1);
    t.scroll_top_requested = true;
    t.state = TabState::Loading {
        promise: Promise::from_ready(Err(BrowserError::Cancelled)),
        asked: "file:///nonexistent-mdowser-test/gone.md".to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: true,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state());
    assert!(matches!(t.state, TabState::Failed { .. }));
    assert!(!t.scroll_top_requested);
}

/// Stopping a load brings the backdrop back with its own position: the
/// abandoned navigation's top reset must not jump the restored page.
#[test]
fn stop_drops_scroll_top() {
    let mut t = settling_tab();
    t.scroll_top_requested = true;
    t.stop();
    assert!(matches!(t.state, TabState::Loaded { .. }));
    assert!(!t.scroll_top_requested);
}

/// Internal pages own per-tab scroll areas, so opening one clears the
/// document flag: it must never leak into a later Back-to-document.
#[test]
fn internal_pages_drop_scroll_top() {
    let mut t = Tab::new(1);
    t.scroll_top_requested = true;
    t.open_history(true);
    assert!(!t.scroll_top_requested);
    assert!(matches!(t.state, TabState::History));
}

/// The OS title carries only the open page's own title; `MDowser` is
/// reserved for Home (nothing open), and no app name is ever appended.
#[test]
fn window_title_is_bare_page_title_or_app_name() {
    assert_eq!(Tab::new(1).window_title(), "MDowser", "Home");
    let mut t = Tab::new(2);
    t.state = TabState::History;
    assert_eq!(t.window_title(), "History");
    t.state = TabState::Licenses;
    assert_eq!(t.window_title(), "Licenses");

    let mut fetched = fetched_for("https://ex.com/a/");
    fetched.meta.title = Some("Page A".to_string());
    t.state = TabState::Loaded { fetched };
    assert_eq!(t.window_title(), "Page A");
    // No frontmatter title: the schemeless URL, still bare.
    t.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    assert_eq!(t.window_title(), "ex.com/a/");
}

/// Without a frontmatter title, the document's first `#` heading names
/// the tab, the window, the page-info card and the bookmark target; the
/// URL stays the last resort.
#[test]
fn first_h1_names_the_page_when_metadata_is_missing() {
    let md = "Intro line.\n\n# Carla's Site\n\nBody.\n";
    let mut t = Tab::new(1);
    t.state = TabState::Loaded {
        fetched: fetched_from_md("https://ex.com/a/", md),
    };
    assert_eq!(t.title(), "Carla's Site");
    assert_eq!(t.window_title(), "Carla's Site");
    assert_eq!(t.site_info().page_title.as_deref(), Some("Carla's Site"));
    let (url, title) = t.bookmark_target().expect("bookmark target");
    assert_eq!(url.as_str(), "https://ex.com/a/");
    assert_eq!(title.as_deref(), Some("Carla's Site"));

    // A frontmatter `title` still wins over the heading.
    let mut fetched = fetched_from_md("https://ex.com/b/", md);
    fetched.meta.title = Some("Metadata Title".to_string());
    t.state = TabState::Loaded { fetched };
    assert_eq!(t.title(), "Metadata Title");
    assert_eq!(t.window_title(), "Metadata Title");

    // Only H2s: still the URL fallback, no invented title.
    t.state = TabState::Loaded {
        fetched: fetched_from_md("https://ex.com/c/", "## Section\n\nBody.\n"),
    };
    assert_eq!(t.title(), "ex.com/c/");
    assert_eq!(t.window_title(), "ex.com/c/");
}

/// The H1 fallback also travels to the local history (the store
/// sanitizes it like a frontmatter title).
#[test]
fn first_h1_title_is_recorded_in_history() {
    let url = "https://ex.com/a/";
    let fetched = fetched_from_md(url, "# Page H1\n\nBody.");
    let promise: Promise<Result<Fetched, BrowserError>> = Promise::from_ready(Ok(fetched));
    let mut t = Tab::new(1);
    t.state = TabState::Loading {
        promise,
        asked: url.to_string(),
        address_at_start: String::new(),
        started: Instant::now(),
        backdrop: None,
        saved_back: Vec::new(),
        saved_forward: Vec::new(),
        pushed_history: false,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(t.poll_state());
    assert_eq!(t.pending_visit, Some((u(url), Some("Page H1".to_string()))));
}
