// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `bookmarks`: encrypted, persistent local bookmarks.
//!
//! STORAGE (all local, the same per-OS application data directory as the
//! encrypted history; see `settings::default_data_dir`). Files (created
//! lazily on the first save):
//! - `bookmarks.bin`: the bookmarks, AEAD-encrypted in the shared format
//!   (`crate::store::crypto`): magic `MDBM`, version `1`, a 12-byte random nonce,
//!   then the ChaCha20-Poly1305 ciphertext. The 5 header bytes are
//!   authenticated as additional data, so magic/version tampering fails
//!   the open too. Plaintext: one `timestamp\turl\ttitle\ticon\tcolor`
//!   line per bookmark; `icon`/`color` (the Home tile customization) are
//!   empty when automatic, and a version-1 line written before they
//!   existed (three fields) parses with both unset.
//! - `store.key`: single shared 32-byte key for history, bookmarks,
//!   settings and session, created mode `0600` on Unix; the
//!   data directory is created mode `0700`. Same threat model as before
//!   (other users, backups and casual inspection, NOT
//!   same-user malware); documented in `docs/SECURITY.md`.
//!
//! LIFETIME: bookmarks have no retention window: they are kept until the
//! user removes them and survive restarts (that is the point of a
//! bookmark). A first run (no store file yet) starts with a curated default
//! set (`DEFAULT_BOOKMARKS`); an existing store is never reseeded. The store
//! is bounded by `MAX_BOOKMARK_STORE_ENTRIES`; an add
//! at the cap is REFUSED (the caller shows a toast) instead of silently
//! dropping the oldest bookmark, unlike history's disposable visits.
//!
//! FAILURE POLICY: same as history: bookmarks are never worth crashing or
//! blocking over. Any IO/crypto error degrades to an empty in-memory store
//! and is surfaced as a note on the Home page (the only place bookmarks
//! are listed); the app keeps working.
//!
//! SAVING: the ciphertext is built on the UI thread (bounded) and the file
//! write + `fsync` run on the shared background writer
//! (`crate::store::writer`), debounced like history; `on_exit` flushes.

use crate::config;
use crate::settings::{atomic_write, default_data_dir};
use crate::store::crypto::{decrypt, encrypt, load_or_create_key};
use crate::store::writer::{StoreWriter, WriteJob};
use crate::util::{normalize_stored_url, now_secs, sanitize_title};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use url::Url;

/// Magic bytes at the start of the encrypted bookmark store.
const FILE_MAGIC: &[u8; 4] = b"MDBM";
/// Store format version (authenticated as AAD together with the magic).
const FILE_VERSION: u8 = 1;
/// Encrypted store file name inside the data directory.
const BOOKMARKS_FILE: &str = "bookmarks.bin";
/// Shared key file for every encrypted store (see `settings::STORE_KEY_FILE`).
const KEY_FILE: &str = crate::settings::STORE_KEY_FILE;

/// Curated bookmarks seeded on a first run (no store file yet), display
/// order: the Home grid shows newest first, so they are inserted reversed.
/// Titles are editable inline like any other bookmark; deleting them never
/// brings them back (seeding happens once, never recurrently).
///
/// Vercel/Sentry/Supabase docs live ONLY in the omnibox builtin suggestions
/// (`builtin_sites::builtin_suggestions()`, `assets/builtin_sites.txt`), not
/// here: the first-run grid stays at
/// five tiles while the address popup still offers the full builtin list.
pub(crate) const DEFAULT_BOOKMARKS: [(&str, &str); 5] = [
    ("https://acceptmarkdown.com/", "Accept Markdown"),
    (
        "https://agentready.md/leaderboard",
        "AgentReady Leaderboard",
    ),
    ("https://www.cloudflare.com/", "Cloudflare"),
    ("https://docs.stripe.com/", "Stripe Docs"),
    ("https://isitagentready.com/", "Is It Agent Ready"),
];

/// One stored bookmark (a page the user chose to keep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// Normalized absolute `https://` URL, or a local `file://` path.
    pub url: String,
    /// Sanitized frontmatter title, when the page had one. Editable from
    /// the Home tile editor.
    pub title: Option<String>,
    /// Unix timestamp (seconds) of when the page was bookmarked.
    pub added_at: u64,
    /// Custom tile glyph (a letter, digit or short emoji cluster); `None` =
    /// automatic (site favicon, else the first letter of the title).
    pub icon: Option<String>,
    /// Tile color palette index (`< config::MAX_BOOKMARK_COLORS`); `None` =
    /// automatic (stable hash of the URL). Only meaningful for the letter
    /// and custom-glyph tiles.
    pub color: Option<u8>,
}

/// Outcome of [`BookmarkStore::toggle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// The page was not bookmarked and now is.
    Added,
    /// The page was bookmarked and is now removed.
    Removed,
    /// The store is at its entry cap: nothing changed.
    LimitReached,
    /// The URL is not a bookmarkable page: nothing changed.
    Unchanged,
}

/// Encrypted local bookmarks.
pub struct BookmarkStore {
    /// Bookmarks, OLDEST first (insertion order). The Home page iterates
    /// in reverse (newest first).
    entries: Vec<Bookmark>,
    /// Exact set of the `entries` URLs, kept in lockstep with the vector:
    /// the toolbar ribbon asks "is this page bookmarked?" EVERY frame, and a
    /// linear scan of up to `MAX_BOOKMARK_STORE_ENTRIES` was the only
    /// per-frame O(n) left in the chrome.
    urls: HashSet<String>,
    /// Data directory (`~/.local/share/mdowser`, ...).
    dir: PathBuf,
    /// False when no per-user data directory could be resolved: bookmarks
    /// stay in memory for the session.
    available: bool,
    /// Monotonic revision of `entries`, bumped on every mutation. The
    /// omnibox caches its suggestion list on it (like the History window
    /// does): re-filtering bookmarks + history every frame is wasted work.
    generation: u64,
    /// Cached 32-byte key (loaded/created lazily on the first save).
    key: Option<[u8; 32]>,
    /// True when `entries` changed since the last successful write.
    dirty: bool,
    /// Last write attempt (debounce anchor).
    last_save: Instant,
    /// Last persistence error, shown on the Home page; `None` = healthy.
    save_error: Option<String>,
    /// Serialized plaintext bytes of `entries` (the exact body
    /// [`Self::serialize`] would produce). Kept in lockstep with every
    /// mutation so `toggle` can refuse an entry that would push the store
    /// past `MAX_BOOKMARK_STORE_BYTES`: URLs are stored PERCENT-ENCODED, so
    /// the char-based input caps cannot bound the serialized bytes (a
    /// 4-byte char becomes up to 12 encoded bytes).
    plain_bytes: usize,
    /// Serialized background writer for the debounced saves: the encrypted
    /// blob is built here (fast, bounded) while the file write + `fsync`
    /// run off the UI thread. `None` until needed.
    writer: Option<StoreWriter>,
}

impl BookmarkStore {
    /// Loads the store from the per-OS data directory (or degrades to an
    /// in-memory store when no home directory can be resolved). A first run
    /// (no store file yet) starts with the curated [`DEFAULT_BOOKMARKS`];
    /// an existing store — even one the user emptied — is never reseeded.
    pub fn load() -> Self {
        match default_data_dir() {
            Some(dir) => {
                let fresh = !dir.join(BOOKMARKS_FILE).is_file();
                let mut store = Self::load_from(dir);
                if fresh {
                    store.seed_defaults();
                }
                store
            }
            None => Self::in_memory(),
        }
    }

    /// Loads from an explicit directory. Never fails: a missing/corrupt
    /// file simply starts empty; the old file is left untouched until the
    /// next successful save replaces it.
    pub(crate) fn load_from(dir: PathBuf) -> Self {
        let mut store = Self {
            entries: Vec::new(),
            urls: HashSet::new(),
            dir,
            available: true,
            generation: 0,
            key: None,
            dirty: false,
            last_save: Instant::now(),
            save_error: None,
            plain_bytes: 0,
            writer: None,
        };
        store.load_entries();
        store
    }

    /// In-memory store used when persistence is unavailable and by tests.
    pub(crate) fn in_memory() -> Self {
        Self {
            entries: Vec::new(),
            urls: HashSet::new(),
            dir: PathBuf::new(),
            available: false,
            generation: 0,
            key: None,
            dirty: false,
            last_save: Instant::now(),
            save_error: None,
            plain_bytes: 0,
            writer: None,
        }
    }

    /// Bookmarks, OLDEST first.
    pub fn entries(&self) -> &[Bookmark] {
        &self.entries
    }

    /// Monotonic revision of [`Self::entries`], bumped on every mutation.
    /// Consumers cache derived views (the omnibox suggestions) on it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Last save error, if any (the Home page shows it as a warning).
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// True when `url` is already in the store (the toolbar ribbon state).
    /// Normalizes like [`Self::toggle`], so the ribbon matches the stored
    /// (credential-free) URL. Exact index lookup, no scan.
    pub fn is_bookmarked(&self, url: &str) -> bool {
        normalize_stored_url(url).is_some_and(|url| self.urls.contains(&url))
    }

    /// [`Self::is_bookmarked`] for a URL that is ALREADY parsed (the loaded
    /// page's `display_url`): no re-parse and no allocation on the common
    /// credential-free path. Parsing only happens for a page URL that still
    /// carries credentials, which the fetch layer strips anyway.
    pub fn is_bookmarked_url(&self, url: &Url) -> bool {
        if url.username().is_empty() && url.password().is_none() {
            return self.urls.contains(url.as_str());
        }
        self.is_bookmarked(url.as_str())
    }

    /// Adds the page when it is not bookmarked, removes it when it is:
    /// the Ctrl/Cmd+D toggle. URLs are normalized by the caller (a loaded
    /// page) and validated here anyway; authority credentials are stripped
    /// from the URL itself (path/query bytes are preserved) and the title is
    /// sanitized on the way in. At the entry cap — or when the entry would
    /// push the serialized store past `MAX_BOOKMARK_STORE_BYTES` — a new
    /// bookmark is refused ([`Toggle::LimitReached`]), never silently
    /// trimmed: the caller surfaces it as a toast.
    pub fn toggle(&mut self, url: &str, title: Option<&str>) -> Toggle {
        // Defensive: the store only ever holds pages this browser can open
        // (HTTPS or local files). An over-long URL is refused instead of
        // truncated into a different page.
        let Some(url) = normalize_stored_url(url) else {
            return Toggle::Unchanged;
        };
        if let Some(index) = self.entries.iter().position(|bookmark| bookmark.url == url) {
            let removed = self.entries.remove(index);
            // A body loaded from disk can hold two entries with the same
            // normalized URL (a store written before normalization, or hand-
            // edited): the `urls` index may only forget the URL when NO
            // entry carries it any more, or the ribbon reported "not
            // bookmarked" while a tile for the page was still on screen.
            if !self.entries.iter().any(|bookmark| bookmark.url == url) {
                self.urls.remove(&url);
            }
            self.plain_bytes = self.plain_bytes.saturating_sub(entry_bytes(&removed));
            self.dirty = true;
            self.generation = self.generation.wrapping_add(1);
            return Toggle::Removed;
        }
        let title = title.map(sanitize_title).filter(|title| !title.is_empty());
        let entry = Bookmark {
            url,
            title,
            added_at: now_secs(),
            icon: None,
            color: None,
        };
        let entry_bytes = entry_bytes(&entry);
        if self.entries.len() >= config::MAX_BOOKMARK_STORE_ENTRIES
            || self.plain_bytes.saturating_add(entry_bytes) > config::MAX_BOOKMARK_STORE_BYTES
        {
            return Toggle::LimitReached;
        }
        self.urls.insert(entry.url.clone());
        self.plain_bytes += entry_bytes;
        self.entries.push(entry);
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
        Toggle::Added
    }

    /// Inserts the curated [`DEFAULT_BOOKMARKS`] into an empty store (first
    /// run only; the caller gates on the store file being absent, so a user
    /// who emptied the grid is never reseeded). Reversed on the way in so
    /// the Home grid (newest first) shows them in the listed order. Their
    /// favicons load through the usual tile pipeline: each tile requests
    /// its origin's icon on first display, and `FaviconCache::request` is a
    /// no-op afterwards, so every icon is probed once per session at most.
    fn seed_defaults(&mut self) {
        if !self.entries.is_empty() {
            return;
        }
        let now = now_secs();
        for (url, title) in DEFAULT_BOOKMARKS.into_iter().rev() {
            let Some(url) = normalize_stored_url(url) else {
                continue;
            };
            if self.urls.contains(&url) {
                continue;
            }
            let title = sanitize_title(title);
            let bookmark = Bookmark {
                url,
                title: (!title.is_empty()).then_some(title),
                added_at: now,
                icon: None,
                color: None,
            };
            // Keep the byte accounting exact from the start: a seeded store
            // that forgot it would look empty and the next save would
            // silently drop the OLDEST bookmark during `trim`.
            self.plain_bytes = self.plain_bytes.saturating_add(entry_bytes(&bookmark));
            self.urls.insert(bookmark.url.clone());
            self.entries.push(bookmark);
        }
        if !self.entries.is_empty() {
            self.dirty = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Removes one bookmark by its index into [`Self::entries`].
    pub fn remove_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            let removed = self.entries.remove(index);
            self.urls.remove(&removed.url);
            self.plain_bytes = self.plain_bytes.saturating_sub(entry_bytes(&removed));
            self.dirty = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Renames a bookmark (the Home tile editor). Empty input clears the
    /// title back to the URL fallback. Returns `true` when it changed.
    ///
    /// A rename that would push the serialized store past
    /// `MAX_BOOKMARK_STORE_BYTES` is REFUSED (the caller keeps the old
    /// name), exactly like [`Self::toggle`] refuses an add: the store must
    /// never have to silently drop the oldest bookmark to make room.
    pub fn set_title(&mut self, index: usize, title: &str) -> bool {
        let title = sanitize_title(title);
        let title = (!title.is_empty()).then_some(title);
        let Some(bookmark) = self.entries.get_mut(index) else {
            return false;
        };
        if bookmark.title == title {
            return false;
        }
        let before = entry_bytes(bookmark);
        let after = before
            .saturating_sub(bookmark.title.as_deref().map_or(0, str::len))
            .saturating_add(title.as_deref().map_or(0, str::len));
        if self
            .plain_bytes
            .saturating_sub(before)
            .saturating_add(after)
            > config::MAX_BOOKMARK_STORE_BYTES
        {
            return false;
        }
        bookmark.title = title;
        self.plain_bytes = self
            .plain_bytes
            .saturating_sub(before)
            .saturating_add(after);
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Moves one bookmark from index `from` to final index `to` (drag
    /// reorder on the Home grid). Out-of-range indexes and no-ops are
    /// ignored. Returns `true` when the order changed.
    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return false;
        }
        let bookmark = self.entries.remove(from);
        self.entries.insert(to, bookmark);
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Debounced save from the UI loop: writes at most every
    /// `BOOKMARKS_SAVE_DEBOUNCE_SECS` while dirty. Returns `true` while a
    /// write is still owed (queued/debouncing), so the caller can schedule
    /// a repaint and an idle window never leaves the last change unsaved.
    pub fn tick(&mut self) -> bool {
        // Surface a finished background write's outcome. A failure is kept
        // visible AND marks the store dirty again (a failed write must be
        // retried, not silently lost). A confirmed success clears a
        // previous warning; clearing only on success avoids a warning that
        // flickers while the disk keeps rejecting writes.
        let worker_ok = self.writer.as_ref().is_some_and(StoreWriter::take_success);
        if let Some(error) = self.writer.as_ref().and_then(StoreWriter::take_error) {
            self.save_error = Some(error);
            self.dirty = true;
        } else if worker_ok {
            self.save_error = None;
        }
        if self.pending_save()
            && self.last_save.elapsed() >= Duration::from_secs(config::BOOKMARKS_SAVE_DEBOUNCE_SECS)
        {
            self.save_async();
        }
        self.pending_save() || self.writer.as_ref().is_some_and(StoreWriter::busy)
    }

    /// Is a debounced write still pending? (Never when no data directory
    /// could be resolved.)
    pub fn pending_save(&self) -> bool {
        self.dirty && self.available
    }

    /// Clean exit: flushes a dirty store. A clean store is left untouched
    /// (no empty file).
    pub fn on_exit(&mut self) {
        self.flush_writer();
        self.save();
    }
}

mod persist;

#[cfg(test)]
pub(crate) use persist::deserialize;
pub(crate) use persist::entry_bytes;

#[cfg(test)]
mod tests;
