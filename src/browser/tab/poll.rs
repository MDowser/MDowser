// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab::poll`: the per-frame poll loop and the document/image
//! handoff states (Loading -> Settling -> Loaded).
//!
//! Every method here is `impl Tab`; the split from `mod.rs` is only for
//! readability. `App::logic` calls `Tab::poll` once per frame per tab.
//!
//! - `preload_doc_images` starts the new document's images in their own
//!   cache while the backdrop stays on screen.
//! - `swap_to_loaded` performs the atomic swap once the image gate lifts.
//! - `poll_settling`/`poll_state` drive both states and the timeouts.

use super::*;

impl Tab {
    /// Call every frame from `App::logic`.
    /// - Polls the document image cache (textures upload on the UI thread).
    /// - A ready `Loading` becomes `Settling` (or `Loaded` for text-only
    ///   pages) or `Failed`.
    /// - A `Settling` gate keeps the backdrop on screen until the new
    ///   document's images are all `Ready`/`Failed`/`Unavailable` or
    ///   `DOC_IMAGE_GATE_SECS` passes, then swaps atomically to `Loaded`.
    /// - A `Loading` older than `TOTAL_FETCH_TIMEOUT_SECS` is cancelled and
    ///   settles back to its backdrop when one is shown (toast: timed out),
    ///   else becomes `Failed` (hung thread / panic guard; previously hung
    ///   forever).
    /// - Returns `true` when anything changed (to request extra repaint).
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let emoji_changed = self.emoji.poll(ctx);
        let images_changed = self.images.poll(ctx);
        if matches!(self.state, TabState::Settling { .. }) {
            return self.poll_settling(ctx) || images_changed || emoji_changed;
        }
        let state_changed = self.poll_state();
        images_changed || state_changed || emoji_changed
    }

    /// Preloads every image of `fetched` into `cache` (document order,
    /// deduplicated, capped): the settling gate waits on this cache while
    /// the backdrop stays visible, so the swap never shows placeholders
    /// turning into bitmaps one by one.
    pub(super) fn preload_doc_images(cache: &mut ImageCache, fetched: &Fetched) {
        use pulldown_cmark::{Event, Tag};
        use std::collections::HashSet;
        let mut seen_raw: HashSet<&str> = HashSet::new();
        for event in &fetched.events {
            let raw = match event {
                Event::Start(Tag::Image { dest_url, .. }) => dest_url.as_ref(),
                _ => continue,
            };
            if !seen_raw.insert(raw) {
                continue;
            }
            let Some(rc) = fetched.dests.image_rc(raw) else {
                continue;
            };
            if !cache.ensure(&rc) {
                // Image-count cap reached: the rest render as
                // `Unavailable` placeholders (settled by definition).
                break;
            }
        }
    }

    /// Swaps a settled document in: drops the backdrop's images, adopts the
    /// preloaded ones, commits history/address state and shows `Loaded`.
    /// The address field was already normalized when the gate started, so
    /// user typing during the gate is never clobbered here.
    pub(super) fn swap_to_loaded(&mut self, fetched: Fetched, settling: ImageCache) {
        self.cancel = None;
        self.current = Some(fetched.display_url.clone());
        // Hand the shown page to `App::logic` for the local history: only
        // pages that actually reach the screen are recorded.
        self.pending_visit = Some((fetched.display_url.clone(), fetched.meta.title.clone()));
        // A stack entry equal to the page just loaded would only reload
        // it: heal those self-duplicates so Back/Forward never look like
        // no-ops.
        self.heal_self_duplicates(&fetched.display_url);
        self.last_navigate = Some(Instant::now());
        // New document: drop the backdrop's images (textures + in-flight
        // jobs) and adopt the preloaded ones.
        self.images.cancel_all();
        self.images = settling;
        // Fresh document: the block cache is keyed by (url salt, index,
        // language) and the same URL may have changed server-side
        // (refresh/reload), so it must never serve the previous body. The
        // generation bump re-keys only THIS tab: resolving a page in one
        // tab used to flush every other tab's cached blocks.
        self.doc_generation = self.doc_generation.wrapping_add(1);
        // The whole-page highlight belonged to the old page.
        self.selected_all = false;
        self.state = TabState::Loaded { fetched };
    }

    /// Polls a `Settling` gate (needs egui access for texture uploads).
    /// Returns `true` when the gate swapped to `Loaded`.
    pub(super) fn poll_settling(&mut self, ctx: &egui::Context) -> bool {
        // Take the gate out so `swap_to_loaded` (which replaces `state`)
        // works without borrow checker fights; put it back when still
        // waiting.
        let mut tmp = TabState::Home;
        std::mem::swap(&mut self.state, &mut tmp);
        let TabState::Settling {
            asked,
            address_at_start,
            started,
            backdrop,
            pending,
            mut settling,
            gate_started,
            saved_back,
            saved_forward,
            pushed_history,
        } = tmp
        else {
            self.state = tmp;
            return false;
        };
        settling.poll(ctx);
        let settled = !settling.has_pending()
            || gate_started.elapsed() > Duration::from_secs(config::DOC_IMAGE_GATE_SECS);
        if !settled {
            self.state = TabState::Settling {
                asked,
                address_at_start,
                started,
                backdrop,
                pending,
                settling,
                gate_started,
                saved_back,
                saved_forward,
                pushed_history,
            };
            return false;
        }
        // Gate passed: swap atomically. History snapshots need no restore
        // on success.
        let _ = (saved_back, saved_forward, pushed_history);
        self.swap_to_loaded(*pending, *settling);
        true
    }

    /// Document-state poll (no egui access): see `poll`.
    pub(super) fn poll_state(&mut self) -> bool {
        // Only the Loading case matters; the rest never changes alone.
        // Early-return BEFORE the swap: moving the whole `TabState` enum
        // (up to ~0.5 KB, `Loading` holds the promise inline) for every
        // settled tab, every frame, was pure cache traffic.
        if !matches!(self.state, TabState::Loading { .. }) {
            return false;
        }
        // Trick: pull the state out with `replace` so `try_take()`
        // (which consumes the `Promise`) works without borrow checker fights.
        let mut tmp = TabState::Home;
        std::mem::swap(&mut self.state, &mut tmp);
        match tmp {
            TabState::Loading {
                promise,
                asked,
                address_at_start,
                started,
                backdrop,
                saved_back,
                saved_forward,
                pushed_history,
                cancel,
            } => {
                // Visual timeout: a hung fetch must not pin the tab forever.
                // The hung thread is cancelled AND, when the previous page is
                // still on screen, it comes back (history restored, images
                // kept) with a toast instead of being lost to an error log.
                // A first load without a backdrop falls into `Failed`.
                // Readiness is checked FIRST: a promise that already holds a
                // complete document must be delivered even if the deadline
                // passed between two polls (UI stall/suspend), never expired
                // (same rule as the media caches).
                if promise.ready().is_none()
                    && started.elapsed() > Duration::from_secs(config::TOTAL_FETCH_TIMEOUT_SECS)
                {
                    cancel.store(true, Ordering::Relaxed);
                    self.cancel = None;
                    self.settle_abandoned(
                        backdrop,
                        asked,
                        &address_at_start,
                        saved_back,
                        saved_forward,
                        pushed_history,
                        format!(
                            "access timeout after {}s (cancelled, retry or go back)",
                            config::TOTAL_FETCH_TIMEOUT_SECS
                        ),
                        Some("Load timed out; showing the previous page".to_string()),
                    );
                    return true;
                }
                if promise.ready().is_none() {
                    // Still downloading: put the state back intact.
                    self.state = TabState::Loading {
                        promise,
                        asked,
                        address_at_start,
                        started,
                        backdrop,
                        saved_back,
                        saved_forward,
                        pushed_history,
                        cancel,
                    };
                    return false;
                }
                // Ready: consume the promise.
                match promise.try_take() {
                    Ok(Ok(fetched)) => {
                        self.cancel = None;
                        // Minimal schemeless field, always normalized `/.../`.
                        // Preserve user typing via dirty flag (robust to
                        // type-and-revert); empty field is always filled.
                        // Done at gate start (not at the swap) so the
                        // omnibox already names the landed URL while the
                        // backdrop is on screen.
                        let normalized = resolver::display_string(&fetched.display_url);
                        let untouched = !self.address_dirty && self.address == address_at_start;
                        if self.address.trim().is_empty() || untouched {
                            self.address = normalized;
                            self.address_dirty = false;
                        }
                        // Preload the new document's images in a separate
                        // cache while the backdrop stays visible: the swap
                        // below is atomic, so no placeholder-to-bitmap
                        // reflow ever reaches the screen. `current`,
                        // history, `last_navigate` and the block-cache
                        // generation all commit at the swap, never here.
                        let mut settling = Box::new(ImageCache::new());
                        Self::preload_doc_images(&mut settling, &fetched);
                        if !settling.has_pending() {
                            // Text-only (or fully inert) page: nothing to
                            // wait for, show it on this same poll.
                            self.swap_to_loaded(fetched, *settling);
                            return true;
                        }
                        self.state = TabState::Settling {
                            asked,
                            address_at_start,
                            started,
                            backdrop,
                            pending: Box::new(fetched),
                            settling,
                            gate_started: Instant::now(),
                            saved_back,
                            saved_forward,
                            pushed_history,
                        };
                        true
                    }
                    Ok(Err(e)) => {
                        // Superseded loads report `Cancelled`: abandoned by a
                        // newer navigation, so there is nothing to show (the
                        // newer Loading already replaced state). This arm only
                        // runs when the cancelled load is still the current
                        // state (e.g. tab close corner), so map it to a quiet
                        // Failed instead of a scary log.
                        if e.is_cancelled() {
                            self.restore_failed_navigation(
                                saved_back,
                                saved_forward,
                                pushed_history,
                                &asked,
                                &address_at_start,
                            );
                            self.state = TabState::Failed {
                                asked,
                                lines: vec![e.to_string()],
                            };
                            return true;
                        }
                        // `NoCandidateWorked` already carries lines; other errors get one line.
                        let lines = match e {
                            BrowserError::NoCandidateWorked { lines, .. } => lines,
                            other => vec![other.to_string()],
                        };
                        // Restore history snapshots: failed back/forward/new-nav
                        // never corrupts the opposite stack. A page pushed by
                        // this navigation stays on `back`, so a failed load
                        // keeps the page the user came from reachable.
                        // Keep the previous `current` so back still works.
                        // `address` shows the request schemeless for context,
                        // unless the user already typed something else.
                        // The fragment belonged to the failed request: never
                        // apply it to a document restored later (back/retry).
                        self.restore_failed_navigation(
                            saved_back,
                            saved_forward,
                            pushed_history,
                            &asked,
                            &address_at_start,
                        );
                        self.state = TabState::Failed { asked, lines };
                        true
                    }
                    Err(promise) => {
                        // Race: `ready()` said ready but `try_take` disagrees.
                        // Put it back and retry next frame.
                        self.state = TabState::Loading {
                            promise,
                            asked,
                            address_at_start,
                            started,
                            backdrop,
                            saved_back,
                            saved_forward,
                            pushed_history,
                            cancel,
                        };
                        false
                    }
                }
            }
            other => {
                self.state = other;
                false
            }
        }
    }
}
