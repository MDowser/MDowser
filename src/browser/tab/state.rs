// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab::state`: visible tab state and its discriminant.
//!
//! Split out of `tab/mod.rs` for size only: `TabState` carries the
//! background promise, the stale-while-revalidate backdrop, the settling
//! document and the image gate; `TabStateKind` is the cheap key the
//! display memo and the chrome use. `Tab` methods live in the parent.

use super::*;

/// Visible tab state.
pub enum TabState {
    /// Landing page (`about:home`): the new-tab page with the large
    /// omnibox and the bookmark grid. A real back/forward entry; the
    /// address field stays empty on it.
    Home,
    /// Internal local-history page (`about:history`): rendered from the
    /// encrypted store, never fetched from the network.
    History,
    /// Internal third-party licenses page (`about:licenses`): the embedded
    /// `THIRD_PARTY_NOTICES.md` rendered like a document, never fetched.
    Licenses,
    /// Downloading in the background.
    Loading {
        /// Background-thread promise.
        promise: Promise<FetchResult>,
        /// User-requested text (shown as "Loading ...").
        asked: String,
        /// Address field value when the load started. Used to detect
        /// user edits during load so `poll` never clobbers typing.
        address_at_start: String,
        /// When it started (for the loading placeholder / visual timeout).
        started: Instant,
        /// Previous document, kept on screen until the new one arrives
        /// (stale-while-revalidate: no white flash when following a link).
        /// Moved into the `Settling` image gate when the load resolves and
        /// dropped at the atomic swap to `Loaded` (or restored on failure).
        /// Boxed: the document is big
        /// and would otherwise trip `large_enum_variant` on `TabState`.
        backdrop: Option<Box<Fetched>>,
        /// History snapshots before this navigation (to restore on failure).
        /// Cloned once per navigation (URLs are tiny); guarantees back/forward
        /// never corrupt on failed loads.
        saved_back: Vec<Url>,
        saved_forward: Vec<Url>,
        /// True when this navigation PUSHED the previous page onto `back`.
        /// On failure the pushed entry is kept (the previous page stays
        /// reachable with Back); without this, a failed first navigation
        /// silently dropped the page the user came from.
        pushed_history: bool,
        /// Cooperative cancel flag shared with the background thread.
        /// Set on new navigation / tab close / visual timeout so the
        /// abandoned fetch stops between candidates/chunks.
        cancel: Arc<AtomicBool>,
    },
    /// Document fetched, images still settling.
    ///
    /// The previous page (when there is one) stays on screen with the
    /// loading indicator while the NEW document's images resolve in
    /// `settling`: the swap to `Loaded` is atomic once every image is
    /// `Ready`/`Failed`/`Unavailable` or `DOC_IMAGE_GATE_SECS` passes, so
    /// the reader never sees placeholder-to-bitmap reflow. A first load
    /// without a backdrop keeps the stable empty panel meanwhile.
    Settling {
        /// User-requested text (shown as "Loading ...").
        asked: String,
        /// Address field value when the load started. Used to detect
        /// user edits during load so `poll` never clobbers typing.
        address_at_start: String,
        /// When the document fetch started (for the visual timeout log).
        started: Instant,
        /// Previous document, kept on screen until the swap (same
        /// stale-while-revalidate contract as `Loading::backdrop`).
        /// Boxed: the document is big.
        backdrop: Option<Box<Fetched>>,
        /// New document waiting for its images.
        /// Boxed: the document is big.
        pending: Box<Fetched>,
        /// New document's images, resolving in the background.
        /// Boxed: the cache is big and would otherwise trip
        /// `large_enum_variant` on `TabState`.
        settling: Box<ImageCache>,
        /// When the image gate started (bounds the extra wait).
        gate_started: Instant,
        /// History snapshots before this navigation (to restore on stop).
        saved_back: Vec<Url>,
        saved_forward: Vec<Url>,
        /// True when this navigation PUSHED the previous page onto `back`.
        pushed_history: bool,
    },
    /// Document ready.
    Loaded {
        /// Document + metadata + URLs.
        fetched: Fetched,
    },
    /// No candidate worked.
    Failed {
        /// Requested text.
        asked: String,
        /// Error lines (one per attempt + header).
        lines: Vec<String>,
    },
}

/// Discriminant of [`TabState`]: the cheap half of the display-cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStateKind {
    Home,
    History,
    Licenses,
    Loading,
    Settling,
    Loaded,
    Failed,
}

impl TabState {
    /// Which variant this is (see [`TabStateKind`]).
    pub(super) fn kind(&self) -> TabStateKind {
        match self {
            Self::Home => TabStateKind::Home,
            Self::History => TabStateKind::History,
            Self::Licenses => TabStateKind::Licenses,
            Self::Loading { .. } => TabStateKind::Loading,
            Self::Settling { .. } => TabStateKind::Settling,
            Self::Loaded { .. } => TabStateKind::Loaded,
            Self::Failed { .. } => TabStateKind::Failed,
        }
    }
}
