// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab::nav`: navigation entry points and stack moves.
//!
//! Every method here is `impl Tab`; the split is only for readability.
//! `pub(super)` lets the parent module's poll loop and the tab tests
//! reach the private helpers. Back/Forward/Refresh/Stop live in the
//! sibling `nav_moves` module.

use super::*;

impl Tab {
    /// Navigates to the `address` field text (Go button / Enter).
    /// - Invalid address text shows an immediate error without network,
    ///   but KEEPS `current` and history (previously lost: bug).
    pub fn navigate_from_address(&mut self) {
        let input = self.address.trim().to_string();
        self.navigate_to_text(&input, true);
    }

    /// Navigates to arbitrary text (field, clicked link, home...).
    /// - `push_history`: true for new navigation, false for back/forward/refresh
    ///   (the caller already moved the stacks).
    /// - Empty (rewrite ignore signal for an unfollowable scheme / `#frag`) is ignored.
    /// - Overlong input (> `MAX_URL_INPUT_CHARS`) is rejected locally with
    ///   `BadUrl`, keeping `current` and history.
    /// - A new different URL while `Loading` abandons the previous fetch
    ///   (cancels cascade) instead of dead-locking for the whole fallback
    ///   budget (`config::TOTAL_FETCH_TIMEOUT_SECS`).
    /// - Same URL while `Loading` is ignored (double Enter/click).
    pub fn navigate_to_text(&mut self, input: &str, push_history: bool) {
        let input = input.trim();
        if input.is_empty() {
            return;
        }
        // Internal page: intercepted BEFORE `parse_navigable` (the `about:`
        // scheme is otherwise a blocked scheme). No network.
        if is_history_input(input) {
            self.open_history(push_history);
            return;
        }
        if is_licenses_input(input) {
            self.open_licenses(push_history);
            return;
        }
        if is_home_input(input) {
            self.open_home(push_history);
            return;
        }
        // Bound the stored `asked`/`address` text even before parsing.
        if input.chars().count() > config::MAX_URL_INPUT_CHARS {
            let line =
                BrowserError::BadUrl(format!("URL exceeds {} chars", config::MAX_URL_INPUT_CHARS))
                    .to_string();
            // Never echo pasted credentials into the field, the error view
            // or "Copy log" (docs/SECURITY.md: credentials are never logged).
            let safe = client::sanitize_for_display(input);
            self.address = truncate_chars(
                &strip_scheme_prefix(&safe),
                config::MAX_ADDRESS_DISPLAY_CHARS,
            )
            .into_owned();
            if self.shows_document() {
                // Keep the document; make the silent rejection visible.
                self.set_toast(&line);
                return;
            }
            self.fail_locally(
                truncate_chars(&safe, config::MAX_ASKED_DISPLAY_CHARS).into_owned(),
                vec![line],
            );
            return;
        }
        match client::parse_navigable(input) {
            Ok(url) => self.navigate_to_url(&url, push_history),
            Err(e) => {
                // Local error (blocked scheme, remote file host, bad path): no network.
                // Keep `current` and history. If a document is shown, keep it
                // (clicking `mailto:` must not destroy the page); only show
                // `Failed` when there is nothing to keep.
                // Both stored strings are bounded like every other branch and
                // sanitized: a local rejection must never echo `user:pass@`
                // (the parser's own error text can embed the input for
                // `BadUrl` cases).
                let line = client::sanitize_for_display(&e.to_string());
                let safe = client::sanitize_for_display(input);
                self.address = truncate_chars(
                    &strip_scheme_prefix(&safe),
                    config::MAX_ADDRESS_DISPLAY_CHARS,
                )
                .into_owned();
                if self.shows_document() {
                    // Keep the document; explain why nothing happened
                    // (e.g. clicking `mailto:` used to be silent).
                    self.set_toast(&line);
                    return;
                }
                // If already Loading a different URL, abandon it for an
                // explicit error (cancel cooperatively so the old thread
                // stops early). History snapshots are restored exactly like
                // a failed network load.
                self.fail_locally(
                    truncate_chars(&safe, config::MAX_ASKED_DISPLAY_CHARS).into_owned(),
                    vec![line],
                );
            }
        }
    }

    /// True when a document is visibly on screen: a loaded page, or a page
    /// kept as the backdrop of a pending load (stale-while-revalidate). A
    /// rejected input must NOT destroy either one.
    pub(super) fn shows_document(&self) -> bool {
        match &self.state {
            TabState::Loaded { .. } => true,
            TabState::Loading { backdrop, .. } | TabState::Settling { backdrop, .. } => {
                backdrop.is_some()
            }
            _ => false,
        }
    }

    /// Settles a LOCAL rejection (`BadUrl`, blocked scheme, remote file
    /// host, oversized input) into
    /// `Failed` while preserving history, exactly like a failed fetch:
    /// a pending navigation's snapshots are restored (a failed new
    /// navigation keeps the page it pushed on `back`, so Forward entries
    /// are not silently lost), and the refresh cooldown is stamped.
    ///
    /// Callers must check [`Self::shows_document`] first: a visible page
    /// (including a `Loading`/`Settling` backdrop) becomes a toast instead.
    pub(super) fn fail_locally(&mut self, asked: String, lines: Vec<String>) {
        self.cancel_inflight();
        self.cancel = None;
        self.clear_overlay();
        self.pending_anchor = None;
        self.scroll_top_requested = false;
        let previous = std::mem::replace(&mut self.state, TabState::Home);
        if let TabState::Loading {
            saved_back,
            saved_forward,
            pushed_history,
            ..
        }
        | TabState::Settling {
            saved_back,
            saved_forward,
            pushed_history,
            ..
        } = previous
        {
            if !pushed_history {
                self.back = saved_back;
            }
            self.forward = saved_forward;
            self.last_navigate = Some(Instant::now());
            self.images.cancel_all();
        }
        self.state = TabState::Failed { asked, lines };
        // The display memo is keyed on (state kind, doc_generation): two
        // consecutive failures share the kind, so the generation must bump
        // here or the tab would keep the previous error's title.
        self.doc_generation = self.doc_generation.wrapping_add(1);
    }

    /// Navigates to an already validated `Url` (enters `Loading`).
    ///
    /// A fresh encrypted disk-cache record serves without network; Refresh
    /// revalidates instead (see `force_revalidate`). Neither errors nor
    /// failed fetches are reused; a same-document reload still navigates
    /// (history untouched, like refresh) instead of returning early.
    /// Single-flight per tab: any previous in-flight fetch is cancelled
    /// cooperatively before the new thread spawns. The old thread exits
    /// promptly (cancel flag + per-chunk stall timeout in `fetch_one`), so
    /// rapid navigations cannot accumulate unbounded background threads.
    pub fn navigate_to_url(&mut self, url: &Url, push_history: bool) {
        // Refresh consumes the forced-revalidation flag up front (even when
        // this navigation turns out to be a duplicate-in-flight no-op
        // below): a stale flag must never leak into the next navigation.
        let force_revalidate = std::mem::replace(&mut self.force_revalidate, false);
        // Internal page reached through back/forward/refresh (the address
        // text path is intercepted in `navigate_to_text`).
        if is_history_url(url) {
            self.open_history(push_history);
            return;
        }
        if is_licenses_url(url) {
            self.open_licenses(push_history);
            return;
        }
        if is_home_url(url) {
            self.open_home(push_history);
            return;
        }
        // The pending URL is the real current position while `Loading` (the
        // same rule back/forward use). Commit it BEFORE deciding whether
        // this is a reload: comparing against the stale page could misread
        // navigating back to the pre-load page as a reload, skip the push
        // and leave a self-duplicate in `back` (Back then only reloaded the
        // page already shown).
        self.commit_pending_navigation();
        // The real target this tab is about (also while the load is in
        // flight or has failed): the session snapshot remembers it instead
        // of serializing the not-yet-resolved tab as Home.
        self.session_target = Some(url.clone());
        // A `#fragment` on the target is a post-load scroll request; the
        // route cascade still fetches without it (resolver strips fragments).
        self.pending_anchor = anchor_from_url(url);
        // No result/error cache: explicit re-navigation to the same logical
        // document (`/love` == `/love/` == `/love.md`) reloads instead of
        // returning early. History is left untouched in that reload case
        // (like refresh) so back/forward are not corrupted with duplicates.
        let is_reload_same = push_history
            .then_some(self.current.as_ref())
            .flatten()
            .is_some_and(|cur| resolver::is_same_logical(cur, url));
        // Duplicate while loading/settling the same logical URL: ignore
        // (double Enter/click).
        if let TabState::Loading { asked, .. } | TabState::Settling { asked, .. } = &self.state {
            if let Ok(prev_url) = client::parse_navigable(asked) {
                if resolver::is_same_logical(&prev_url, url) {
                    return;
                }
            } else if asked == url.as_str() {
                return;
            }
        }
        // Snapshot history for failure restore (cheap: URLs only).
        let saved_back = self.back.clone();
        let saved_forward = self.forward.clone();
        // History: current moves to `back`; `forward` clears on new navigation.
        // Both stacks are bounded (`MAX_HISTORY_ENTRIES`): oldest dropped.
        // Same-document reload (Go to the same URL, error retry of the same
        // page) leaves both stacks alone, like refresh: it revalidates (or
        // refetches) without pushing duplicates or dropping forward.
        let pushed_history = push_history && !is_reload_same;
        // A genuinely new page starts at the top; Back/Forward/Refresh and
        // same-document reloads keep the flag clear so egui restores the
        // saved offset. Consumed by the viewer's first `Loaded` frame.
        self.scroll_top_requested = pushed_history;
        if pushed_history {
            // Borrowed: the old `self.current.clone()` cloned the `Url`
            // only to move it into `back` right after (same cost, plus an
            // `Option` wrapper). Borrow and clone once on push.
            if let Some(cur) = self.current.as_ref() {
                // Avoid pushing duplicate when `back.last == current`
                // (can happen after abandon + retry). Also skip when the
                // current and target are the same logical document.
                if self.back.last() != Some(cur) && !resolver::is_same_logical(cur, url) {
                    self.back.push(cur.clone());
                    if self.back.len() > config::MAX_HISTORY_ENTRIES {
                        let excess = self.back.len() - config::MAX_HISTORY_ENTRIES;
                        self.back.drain(0..excess);
                    }
                }
            }
            self.forward.clear();
        }
        let asked = url.as_str().to_string();
        // Minimal UI: schemeless field.
        self.address = resolver::display_string(url);
        self.address_dirty = false;
        let address_at_start = self.address.clone();
        // No `last_navigate` stamp here: cooldown starts when the load
        // RESOLVES (see `poll`), not when it starts. Stamping both extended
        // the lock well beyond 5s on slow loads.
        // A new navigation clears any "select all" highlight and the stale
        // context-menu rect (it belongs to the previous document).
        self.clear_overlay();
        // Image textures stay while the old document is still visible as a
        // backdrop; they are dropped at the atomic swap to the new document
        // (or when the load fails) so no cross-page subresource cache survives.
        // Cancel any previous in-flight fetch cooperatively before
        // replacing the state (the old thread checks the flag).
        self.cancel_inflight();
        // Keep the current document as backdrop so the old page stays
        // visible until the new one arrives (no white flash). Moved (no clone):
        // take previous state, extract `Loaded` or keep previous backdrop
        // when abandoning a `Loading`/`Settling`. A settling gate's pending
        // document and its image jobs are dropped here (their `Drop`
        // cancels the jobs and frees the budget).
        let prev_state = std::mem::replace(&mut self.state, TabState::Home);
        // Superseding an in-flight load (`Loading` -> `Loading`) keeps the
        // same state KIND, and the display memo is keyed on (kind,
        // generation): without an explicit bump the tab rail keeps showing
        // the OLD url's title and favicon origin for the whole second load.
        let superseded_load = matches!(prev_state, TabState::Loading { .. });
        let backdrop = match prev_state {
            TabState::Loaded { fetched } => Some(Box::new(fetched)),
            TabState::Loading { backdrop, .. } | TabState::Settling { backdrop, .. } => backdrop,
            TabState::Failed { .. } | TabState::Home | TabState::History | TabState::Licenses => {
                None
            }
        };
        if superseded_load {
            self.doc_generation = self.doc_generation.wrapping_add(1);
        }

        // Launch the fetch on a background thread (shared network runtime,
        // never a per-job one: the HTTP pool must stay attached to the
        // process-wide executor; see `protocol::client`). The UI never
        // blocks; `poll()` collects the result.
        // Refresh consumes the forced-revalidation flag; every other
        // navigation serves fresh disk-cache records without network.
        // The cancel flag is shared: superseding navigation sets it.
        let start = url.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let job_cancel = cancel.clone();
        let mode = if force_revalidate {
            client::FetchMode::ForceRevalidate
        } else {
            client::FetchMode::Normal
        };
        let promise = Promise::spawn_thread("fetch-markdown", move || {
            client::fetch_blocking_with_mode(&start, job_cancel, mode)
        });
        self.state = TabState::Loading {
            promise,
            asked,
            address_at_start,
            started: Instant::now(),
            backdrop,
            saved_back,
            saved_forward,
            pushed_history,
            cancel,
        };
    }

    /// Opens the internal local-history page (`about:history`).
    ///
    /// Behaves like a real navigation for the history stacks -- the current
    /// page moves to `back`, `forward` clears and the refresh cooldown is
    /// stamped -- but it never touches the network, never records a visit and
    /// never fetches an image. `push_history` is false for back/forward and
    /// refresh. A repeat request while already on the page is a no-op (like
    /// reloading the same logical document); refresh/back still re-render it
    /// against the current store.
    pub fn open_history(&mut self, push_history: bool) {
        if matches!(self.state, TabState::History) && push_history {
            return;
        }
        self.open_internal_page(HISTORY_URL, TabState::History, push_history);
    }

    /// Opens the internal third-party licenses page (`about:licenses`).
    ///
    /// Same navigation contract as [`Self::open_history`]: real stack moves,
    /// no network, no visit, no image fetch. The page renders the embedded
    /// `THIRD_PARTY_NOTICES.md` (see `licenses::document`).
    pub fn open_licenses(&mut self, push_history: bool) {
        if matches!(self.state, TabState::Licenses) && push_history {
            return;
        }
        self.open_internal_page(LICENSES_URL, TabState::Licenses, push_history);
    }

    /// Opens the internal new-tab page (`about:home`).
    ///
    /// Same navigation contract as [`Self::open_history`]: real stack moves
    /// (the current page moves to `back`, `forward` clears), no network, no
    /// visit, no image fetch. The only difference with the other internal
    /// pages is the address field: it stays empty on Home (the large capsule
    /// shows its hint), so it is cleared after the shared bookkeeping. A
    /// repeat request while already on the page is a no-op.
    pub fn open_home(&mut self, push_history: bool) {
        if matches!(self.state, TabState::Home) && push_history {
            // Repeat request (toolbar Home button on Home): no stack move,
            // but drop a stale typed draft like the old `go_home` did, so no
            // hidden text survives behind the landing page.
            self.address.clear();
            self.address_dirty = false;
            return;
        }
        self.open_internal_page(HOME_URL, TabState::Home, push_history);
        self.address.clear();
    }

    /// Shared bookkeeping for the internal `about:` pages (`about:home`,
    /// `about:history`, `about:licenses`): a real navigation for the stacks,
    /// nothing else.
    ///
    /// The current page moves to `back`, `forward` clears and the refresh
    /// cooldown is stamped, but no network is touched, no visit is recorded
    /// and no image is fetched. `push_history` is false for back/forward and
    /// refresh; a repeat request while already on the page is a no-op (like
    /// reloading the same logical document), while refresh/back re-render it.
    pub(super) fn open_internal_page(
        &mut self,
        url_str: &'static str,
        state: TabState,
        push_history: bool,
    ) {
        // Parsed lazily and defensively: never panic the UI thread if the
        // internal URL ever stops parsing.
        let Ok(url) = Url::parse(url_str) else {
            return;
        };
        // Commit any in-flight navigation as the current position before the
        // stack move (same rule as `navigate_to_url`), so Back returns to the
        // page being loaded rather than the one it replaced.
        self.commit_pending_navigation();
        let pushed_history = push_history && self.current.as_ref() != Some(&url);
        if pushed_history {
            if let Some(cur) = self.current.clone() {
                if self.back.last() != Some(&cur) && cur != url {
                    Self::push_bounded(&mut self.back, cur);
                }
            }
            self.forward.clear();
        }
        self.cancel_inflight();
        self.cancel = None;
        self.clear_overlay();
        self.images.cancel_all();
        // A fresh page: the previous document's code cache must never serve
        // a body on the page we leave behind (same rule as a resolved fetch).
        // The generation bump only invalidates THIS tab's keys.
        self.doc_generation = self.doc_generation.wrapping_add(1);
        // Internal pages are session `None` markers: forget the last real
        // target so a later navigate-and-quit cannot resurrect it.
        self.session_target = None;
        self.address = url_str.to_string();
        self.address_dirty = false;
        self.pending_anchor = None;
        // Internal surfaces have their own per-tab scroll areas: the
        // document flag must not leak into a later Back-to-document (which
        // would wrongly jump that page to the top).
        self.scroll_top_requested = false;
        self.pending_visit = None;
        self.selected_all = false;
        self.current = Some(url);
        // The pages read live state every frame (History) or embedded data
        // (Licenses), so they need no reload cooldown; leaving `last_navigate`
        // clear also spares the idle window a 5s countdown of repaints it
        // would get from a resolved load.
        self.last_navigate = None;
        self.state = state;
    }

    /// "Home" button: goes to the new tab page (`about:home`: the landing
    /// page with the large omnibox and the bookmark grid), like a fresh tab.
    ///
    /// The page left behind moves to `back`, so Back returns to it, and any
    /// in-flight load is abandoned (cancelled). Works from every state:
    /// a loaded page, the internal history page, a pending load or an
    /// error view all land back on the landing page. The address field stays
    /// empty on Home (unlike the other internal pages, which echo their URL).
    pub fn go_home(&mut self) {
        self.open_home(true);
    }

    /// URL a "Refresh" / "Retry" action must act on.
    ///
    /// While `Failed` or `Loading`, `current` may still point at the
    /// previous page, but the user is looking at (and the bar shows) the
    /// failed/pending `asked`: acting on the stale page would silently load
    /// a different document. Mirrors the error view Retry. Falls back to
    /// `current` when `asked` does not parse (local BadUrl rejection with a
    /// previous page).
    pub(super) fn action_target(&self) -> Option<Url> {
        let asked = match &self.state {
            TabState::Failed { asked, .. }
            | TabState::Loading { asked, .. }
            | TabState::Settling { asked, .. } => Some(asked),
            _ => None,
        };
        asked
            .and_then(|a| client::parse_navigable(a).ok())
            .or_else(|| self.current.clone())
    }

    /// Commits the in-flight navigation's target as `current`.
    ///
    /// While `Loading`, `current` still points at the previous page. History
    /// operations (back/forward) must treat the pending URL as the current
    /// position; otherwise they push the stale page again and create
    /// self-duplicates (Back/Forward pointing at the page already shown).
    pub(super) fn commit_pending_navigation(&mut self) {
        let asked = match &self.state {
            TabState::Loading { asked, .. } | TabState::Settling { asked, .. } => {
                Some(asked.clone())
            }
            _ => None,
        };
        if let Some(asked) = asked {
            if let Ok(url) = client::parse_navigable(&asked) {
                self.current = Some(resolver::normalized_display(&url));
            }
        }
    }

    /// Is the tab currently loading/settling `url` (same logical document)?
    pub(super) fn is_loading_logical(&self, url: &Url) -> bool {
        let asked = match &self.state {
            TabState::Loading { asked, .. } | TabState::Settling { asked, .. } => {
                Some(asked.clone())
            }
            _ => None,
        };
        if let Some(asked) = asked {
            if let Ok(pending) = client::parse_navigable(&asked) {
                return resolver::is_same_logical(&pending, url);
            }
        }
        false
    }

    /// Pushes `url` onto `stack`, bounded by `MAX_HISTORY_ENTRIES`.
    pub(super) fn push_bounded(stack: &mut Vec<Url>, url: Url) {
        stack.push(url);
        if stack.len() > config::MAX_HISTORY_ENTRIES {
            let excess = stack.len() - config::MAX_HISTORY_ENTRIES;
            stack.drain(0..excess);
        }
    }

    /// Drops stack entries that point at `current` itself.
    ///
    /// A `back`/`forward` entry equal to the page just loaded can only
    /// reload the page already shown: pressing the button would look like
    /// a no-op. Heals stacks left by interrupted navigations (navigating
    /// away while another page was still loading) or older versions.
    pub(super) fn heal_self_duplicates(&mut self, current: &Url) {
        while self
            .back
            .last()
            .is_some_and(|b| resolver::is_same_logical(b, current))
        {
            self.back.pop();
        }
        while self
            .forward
            .last()
            .is_some_and(|f| resolver::is_same_logical(f, current))
        {
            self.forward.pop();
        }
    }

    /// Mutates the stacks for a Back navigation and returns the target.
    /// No network here: `go_back` then navigates (keeps this unit-testable).
    ///
    /// - Commits any pending navigation first (see `commit_pending_navigation`).
    /// - A no-op when the tab is already loading the same logical target
    ///   (never mutates the stacks without navigating).
    /// - Skips pushing a self-duplicate into `forward` (heals stacks
    ///   corrupted by older versions).
    pub(super) fn prepare_back(&mut self) -> Option<Url> {
        self.commit_pending_navigation();
        let prev = self.back.last()?.clone();
        if self.is_loading_logical(&prev) {
            return None;
        }
        self.back.pop();
        if let Some(cur) = self.current.clone() {
            if !resolver::is_same_logical(&cur, &prev) {
                Self::push_bounded(&mut self.forward, cur);
            }
        }
        Some(prev)
    }

    /// Mutates the stacks for a Forward navigation and returns the target.
    /// Mirror of `prepare_back`.
    pub(super) fn prepare_forward(&mut self) -> Option<Url> {
        self.commit_pending_navigation();
        let next = self.forward.last()?.clone();
        if self.is_loading_logical(&next) {
            return None;
        }
        self.forward.pop();
        if let Some(cur) = self.current.clone() {
            if !resolver::is_same_logical(&cur, &next) {
                Self::push_bounded(&mut self.back, cur);
            }
        }
        Some(next)
    }
}
