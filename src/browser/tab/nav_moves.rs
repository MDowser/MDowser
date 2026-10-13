// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab::nav_moves`: Back/Forward/Refresh/Stop and the address
//! revert.
//!
//! Every method is `impl Tab`; the navigation and internal-page entry
//! points live in `nav`.

use super::*;

impl Tab {
    /// "Back" button. Abandons in-flight load to navigate (cancels cascade).
    /// Snapshots history BEFORE mutating so a failed back restores the
    /// original stacks instead of losing the target.
    pub fn go_back(&mut self) {
        // Snapshot before mutation (see `poll` restore).
        let orig_back = self.back.clone();
        let orig_forward = self.forward.clone();
        if let Some(prev) = self.prepare_back() {
            self.navigate_to_url(&prev, false);
            // `navigate_to_url` snapshotted after our mutation; overwrite
            // with the pre-mutation snapshot so failure restores correctly.
            // A fresh navigation always lands in `Loading` (never
            // `Settling`: the gate only starts after a fetch resolves).
            if let TabState::Loading {
                saved_back,
                saved_forward,
                ..
            } = &mut self.state
            {
                *saved_back = orig_back;
                *saved_forward = orig_forward;
            }
        }
    }

    /// "Forward" button. Abandons in-flight load to navigate.
    pub fn go_forward(&mut self) {
        let orig_back = self.back.clone();
        let orig_forward = self.forward.clone();
        if let Some(next) = self.prepare_forward() {
            self.navigate_to_url(&next, false);
            // `navigate_to_url` always lands in `Loading` (see `go_back`).
            if let TabState::Loading {
                saved_back,
                saved_forward,
                ..
            } = &mut self.state
            {
                *saved_back = orig_back;
                *saved_forward = orig_forward;
            }
        }
    }

    /// "Refresh" button (honors the 5s cooldown, never duplicates in Loading).
    pub fn refresh(&mut self) {
        if !self.can_refresh() {
            return;
        }
        // A `Failed` tab reloads its failed target (`action_target`), not
        // the previous page that `current` still points at.
        if let Some(cur) = self.action_target() {
            // Refresh leaves history alone (same page).
            self.navigate_to_url(&cur, false);
        }
    }

    /// Stop button (shown while loading/settling) / `Esc` on the toolbar:
    /// cancels the in-flight fetch (or the image gate) and restores the
    /// page that was on screen before it started, keeping its decoded
    /// images. A first load with no page behind it settles into a quiet
    /// `Failed` log. History snapshots are restored like a failed load, so
    /// a stopped navigation never corrupts back/forward.
    pub fn stop(&mut self) {
        // Guard FIRST: the `mem::replace` below parks `Home` in the slot
        // while the fields are moved out, so a non-loading tab would lose
        // its document if this ran anyway.
        if !self.is_loading() {
            return;
        }
        match std::mem::replace(&mut self.state, TabState::Home) {
            TabState::Loading {
                backdrop,
                asked,
                address_at_start,
                saved_back,
                saved_forward,
                pushed_history,
                cancel,
                ..
            } => {
                // Cancel cooperatively: the background thread stops between
                // candidates/chunks instead of finishing the abandoned fetch.
                cancel.store(true, Ordering::Relaxed);
                self.cancel = None;
                self.settle_abandoned(
                    backdrop,
                    asked,
                    &address_at_start,
                    saved_back,
                    saved_forward,
                    pushed_history,
                    "Loading stopped".to_string(),
                    None,
                );
            }
            TabState::Settling {
                backdrop,
                asked,
                address_at_start,
                saved_back,
                saved_forward,
                pushed_history,
                ..
            } => {
                // The document already arrived; only the image gate is
                // abandoned. The settling cache drops here (its `Drop`
                // cancels the jobs and frees the budget) while `self.images`
                // still holds the backdrop's textures.
                self.cancel = None;
                self.settle_abandoned(
                    backdrop,
                    asked,
                    &address_at_start,
                    saved_back,
                    saved_forward,
                    pushed_history,
                    "Loading stopped".to_string(),
                    None,
                );
            }
            _ => {
                // Unreachable after the `is_loading` guard; never panic the UI.
            }
        }
    }

    /// Settles an abandoned load (`stop` or visual timeout) back into a
    /// stable state.
    ///
    /// - A backdrop (the page that was on screen) comes back as `Loaded`,
    ///   keeping its decoded images, its history snapshots and the
    ///   normalized address; `toast` can explain why the load ended.
    /// - A first load without a backdrop falls into a quiet `Failed` log
    ///   carrying `failure_line`.
    /// - History is restored like a failed fetch (`pushed_history`): the
    ///   entry the abandoned navigation pushed stays reachable with Back
    ///   when it pushed one, exactly as a fetch error behaves.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn settle_abandoned(
        &mut self,
        backdrop: Option<Box<Fetched>>,
        asked: String,
        address_at_start: &str,
        saved_back: Vec<Url>,
        saved_forward: Vec<Url>,
        pushed_history: bool,
        failure_line: String,
        toast: Option<String>,
    ) {
        // The abandoned navigation's anchor target no longer applies.
        self.pending_anchor = None;
        // Same for its top-reset: the restored backdrop keeps its own
        // reading position instead of jumping to the top.
        self.scroll_top_requested = false;
        // History snapshots are restored like a failed load: an abandoned
        // navigation never corrupts back/forward, and the pushed entry is
        // kept when this navigation pushed one (same rule as
        // `restore_failed_navigation`).
        if !pushed_history {
            self.back = saved_back;
        }
        self.forward = saved_forward;
        self.last_navigate = Some(Instant::now());
        // Never clobber typing started after the navigation (same rule as
        // `poll`): only refresh the field when the user left it untouched.
        let untouched = !self.address_dirty && self.address == address_at_start;
        self.address_dirty = false;
        match backdrop {
            Some(fetched) => {
                // The page is still on screen: commit it as current again.
                self.current = Some(fetched.display_url.clone());
                // Entries equal to the restored page would only reload it.
                self.heal_self_duplicates(&fetched.display_url);
                if untouched {
                    self.address = resolver::display_string(&fetched.display_url);
                }
                if let Some(text) = toast {
                    self.set_toast(&text);
                }
                self.state = TabState::Loaded { fetched: *fetched };
            }
            None => {
                if untouched {
                    self.address = strip_scheme_prefix(&asked).into_owned();
                }
                self.state = TabState::Failed {
                    asked,
                    lines: vec![failure_line],
                };
            }
        }
    }

    /// `Esc` in the address field: restore the field to the page actually
    /// shown (or clear it on Home). Never touches history or the network.
    pub fn revert_address(&mut self) {
        let shown = match &self.state {
            TabState::Loaded { fetched } => Some(resolver::display_string(&fetched.display_url)),
            TabState::Loading { asked, .. }
            | TabState::Settling { asked, .. }
            | TabState::Failed { asked, .. } => Some(strip_scheme_prefix(asked).into_owned()),
            TabState::History => Some(HISTORY_URL.to_string()),
            TabState::Licenses => Some(LICENSES_URL.to_string()),
            TabState::Home => None,
        };
        match shown {
            Some(text) => {
                self.address =
                    truncate_chars(&text, config::MAX_ADDRESS_DISPLAY_CHARS).into_owned();
            }
            None => self.address.clear(),
        }
        self.address_dirty = false;
    }
}
