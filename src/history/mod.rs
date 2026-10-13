// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `history`: encrypted, configurable local browsing history.
//!
//! STORAGE (all local, standard per-OS application data directory):
//! - macOS: `~/Library/Application Support/MDowser/`
//! - Windows: `%LOCALAPPDATA%\MDowser\` (falls back to `%APPDATA%`)
//! - Linux/BSD: `$XDG_DATA_HOME/mdowser/` (falls back to
//!   `~/.local/share/mdowser/`)
//!
//! Files (created lazily on the first save):
//! - `history.bin`: the visits, AEAD-encrypted in the shared format
//!   (`crate::store::crypto`): magic `MDHS`, version `1`, a 12-byte random nonce,
//!   then the ChaCha20-Poly1305 ciphertext. The 5 header bytes are
//!   authenticated as additional data, so magic/version tampering fails
//!   the open too.
//! - `store.key`: 32 random bytes, created mode `0600` on Unix; the data
//!   directory is created mode `0700`. Single shared key for history,
//!   bookmarks, settings and session. The key lives beside the data by
//!   design (the usual threat model for a local key store): it protects
//!   the files against other users, backups and casual inspection,
//!   NOT against malware running as the same user. Documented in
//!   `docs/SECURITY.md`. Key handling and the debounced background writer are
//!   shared with `bookmarks` (`crate::store::crypto`, `crate::store::writer`).
//! - `settings.bin`: encrypted preferences shared with the `settings` module
//!   (`retention=...`, theme, zoom, session restore). A corrupt value falls
//!   back to its default.
//!
//! RETENTION:
//! - [`Retention::Days`] (default 90 = 3 months), [`Retention::Forever`],
//!   or [`Retention::Session`] (forget on exit: nothing is ever read from
//!   or written to disk; leftovers are wiped when that policy loads and
//!   again on a clean exit).
//! - Entries older than the window are pruned on load, on record and when
//!   the preference changes.
//! - [`HistoryStore::clear`] deletes the visits (the shared `store.key` is
//!   kept: bookmarks/settings/session still need it).
//!
//! FAILURE POLICY: history is never worth crashing (or blocking) over.
//! Any IO/crypto error degrades to an empty in-memory history and is
//! surfaced as a note in Preferences; the app keeps working.
//!
//! BOUNDS: entries and serialized bytes are capped in `config`; loaded
//! files larger than the byte cap + a small header are refused unread.

use crate::config;
use crate::settings::{atomic_write, default_data_dir, read_setting};
use crate::store::crypto::{decrypt, encrypt, load_or_create_key};
use crate::store::writer::{StoreWriter, WriteJob};
use crate::util::{normalize_stored_url, now_secs, sanitize_title};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Magic bytes at the start of the encrypted store.
const FILE_MAGIC: &[u8; 4] = b"MDHS";
/// Store format version (authenticated as AAD together with the magic).
const FILE_VERSION: u8 = 1;
/// Encrypted store file name inside the data directory.
const HISTORY_FILE: &str = "history.bin";
/// Shared key file for every encrypted store (see `settings::STORE_KEY_FILE`).
const KEY_FILE: &str = crate::settings::STORE_KEY_FILE;

/// How long visits are kept on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// In-memory only: nothing is read or written; leftovers are wiped when
    /// this policy loads and again on a clean exit.
    Session,
    /// Keep visits for at most this many days.
    Days(u32),
    /// Keep visits until the user deletes them.
    Forever,
}

impl Default for Retention {
    /// 3 months, the documented default.
    fn default() -> Self {
        Retention::Days(config::HISTORY_DEFAULT_RETENTION_DAYS)
    }
}

impl Retention {
    /// The radio options shown in Preferences, in menu order.
    pub fn options() -> [(Retention, &'static str); 6] {
        [
            (Retention::Session, "Forget on exit"),
            (Retention::Days(30), "Keep 1 month"),
            (Retention::Days(90), "Keep 3 months (default)"),
            (Retention::Days(180), "Keep 6 months"),
            (Retention::Days(365), "Keep 1 year"),
            (Retention::Forever, "Keep forever"),
        ]
    }

    /// Setting-file spelling: `session`, `forever` or a day count.
    fn as_setting(self) -> String {
        match self {
            Retention::Session => "session".to_string(),
            Retention::Forever => "forever".to_string(),
            Retention::Days(days) => days.to_string(),
        }
    }

    /// Parses a setting value; unknown values return `None` (caller keeps
    /// its default instead of inventing a window).
    fn from_setting(value: &str) -> Option<Self> {
        match value.trim() {
            "session" => Some(Retention::Session),
            "forever" => Some(Retention::Forever),
            days => days
                .parse::<u32>()
                .ok()
                // 10 years is "effectively forever" but still a day window;
                // absurd values are rejected as corrupt settings.
                .filter(|d| (1..=3_650).contains(d))
                .map(Retention::Days),
        }
    }

    /// Age after which visits are pruned; `None` = no age-based pruning.
    fn keep_for(self) -> Option<Duration> {
        match self {
            Retention::Session | Retention::Forever => None,
            Retention::Days(days) => Some(Duration::from_secs(u64::from(days) * 86_400)),
        }
    }
}

/// One recorded visit (a successfully loaded page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    /// Normalized absolute `https://` URL, or a local `file://` path.
    pub url: String,
    /// Sanitized frontmatter title, when the page had one.
    pub title: Option<String>,
    /// Unix timestamp (seconds) of the visit.
    pub visited_at: u64,
}

/// Encrypted local history + retention preference.
pub struct HistoryStore {
    /// Visits, OLDEST first. The UI iterates in reverse.
    entries: Vec<Visit>,
    /// Sum of [`persist::entry_bytes`] over `entries`, maintained on every
    /// mutation so `trim` (which runs per recorded visit AND per save) can
    /// answer "under both caps" in O(1) instead of walking up to 20 000
    /// entries every time.
    plain_bytes: usize,
    /// Current retention window.
    retention: Retention,
    /// Data directory (`~/.local/share/mdowser`, ...).
    dir: PathBuf,
    /// False when no per-user data directory could be resolved: history
    /// stays in memory for the session (like `Retention::Session`).
    available: bool,
    /// Cached 32-byte key (loaded/created lazily on the first save).
    key: Option<[u8; 32]>,
    /// True when `entries` changed since the last successful write.
    dirty: bool,
    /// Monotonic revision of `entries`, bumped on every mutation. The
    /// History window caches its filtered index list on it: re-lowercasing
    /// up to 20000 URLs+titles per frame made scrolling the open list stutter.
    generation: u64,
    /// Last write attempt (debounce anchor).
    last_save: Instant,
    /// Last persistence error, shown in Preferences; `None` = healthy.
    save_error: Option<String>,
    /// Serialized background writer used by the debounced UI-path saves:
    /// the encrypted blob is built here (fast, bounded) while the file
    /// write + `fsync` run off the UI thread. `None` until needed.
    writer: Option<StoreWriter>,
    /// A wipe (`clear` or `Session` retention) waiting for the writer to
    /// drain: files are deleted only once no queued rename can land after
    /// the delete, or the stale blob would resurrect the store.
    wipe_pending: bool,
}

impl HistoryStore {
    /// Loads the store from the per-OS data directory (or degrades to an
    /// in-memory store when no home directory can be resolved).
    pub fn load() -> Self {
        match default_data_dir() {
            Some(dir) => Self::load_from(dir),
            None => Self::in_memory(),
        }
    }

    /// Loads from an explicit directory. Never fails: a missing/corrupt
    /// file simply starts empty; the old file is left untouched until the
    /// next successful save replaces it.
    pub(crate) fn load_from(dir: PathBuf) -> Self {
        let mut store = Self {
            entries: Vec::new(),
            plain_bytes: 0,
            retention: Retention::default(),
            dir,
            available: true,
            key: None,
            dirty: false,
            generation: 0,
            last_save: Instant::now(),
            save_error: None,
            writer: None,
            wipe_pending: false,
        };
        store.retention = load_retention(&store.dir);
        if matches!(store.retention, Retention::Session) {
            // Forget-on-exit never reads persisted data: start empty and
            // remove anything a previous non-session run left behind.
            store.wipe_files();
            return store;
        }
        store.load_entries();
        store
    }

    /// In-memory store used when persistence is unavailable and by tests.
    pub(crate) fn in_memory() -> Self {
        Self {
            entries: Vec::new(),
            plain_bytes: 0,
            retention: Retention::default(),
            dir: PathBuf::new(),
            available: false,
            key: None,
            dirty: false,
            generation: 0,
            last_save: Instant::now(),
            save_error: None,
            writer: None,
            wipe_pending: false,
        }
    }

    /// Visits, OLDEST first.
    pub fn entries(&self) -> &[Visit] {
        &self.entries
    }

    /// Monotonic revision of [`Self::entries`], bumped on every mutation.
    /// Consumers cache derived views (the History window filter) on it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Current retention preference.
    pub fn retention(&self) -> Retention {
        self.retention
    }

    /// True under [`Retention::Session`] ("Forget on exit"): visited URLs
    /// must never hit the disk — including the encrypted `session.bin`
    /// snapshot, which is why "restore tabs" is turned off while this mode
    /// is active.
    pub fn forgets_on_exit(&self) -> bool {
        matches!(self.retention, Retention::Session)
    }

    /// Data directory, when persistence is available (Preferences shows it).
    pub fn storage_dir(&self) -> Option<&Path> {
        self.available.then_some(self.dir.as_path())
    }

    /// Last save error, if any (Preferences shows it as a warning).
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Records a successfully loaded page. Consecutive visits to the same
    /// URL update the previous entry instead of duplicating it (a refresh
    /// is not a new page). Titles are sanitized on the way in, the URL must
    /// be a recordable page (an `https` URL or a local `file:` path) within
    /// the input cap (longer ones are skipped, never truncated into a
    /// different page), and the retention/bounds are applied at once.
    pub fn record(&mut self, url: &str, title: Option<&str>) {
        // Defensive: the store only ever holds pages that actually loaded
        // (HTTPS or local files). An over-long URL is skipped: truncating it
        // with `...` would record a DIFFERENT page that a later click would
        // fail to open. Authority credentials are stripped from the URL
        // itself (path/query bytes are preserved) even though callers
        // already sanitize: defense in depth if a future caller passes raw
        // input.
        let Some(url) = normalize_stored_url(url) else {
            return;
        };
        // New visit or updated newest entry: derived views must rebuild.
        self.generation = self.generation.wrapping_add(1);
        let title = title.map(sanitize_title).filter(|title| !title.is_empty());
        let now = now_secs();
        if let Some(last) = self.entries.last_mut() {
            if last.url == url {
                let before = entry_bytes(last);
                last.visited_at = now;
                if title.is_some() {
                    last.title = title;
                }
                self.plain_bytes = self
                    .plain_bytes
                    .saturating_sub(before)
                    .saturating_add(entry_bytes(last));
                self.dirty = true;
                self.prune(now);
                self.trim();
                return;
            }
        }
        let visit = Visit {
            url,
            title,
            visited_at: now,
        };
        self.plain_bytes = self.plain_bytes.saturating_add(entry_bytes(&visit));
        self.entries.push(visit);
        self.dirty = true;
        self.prune(now);
        self.trim();
    }

    /// Removes one entry by its index into [`Self::entries`].
    pub fn remove_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            let removed = self.entries.remove(index);
            self.plain_bytes = self.plain_bytes.saturating_sub(entry_bytes(&removed));
            self.dirty = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Deletes every visit on disk and in memory. Used by the
    /// "Delete history now" action. The shared `store.key` is kept:
    /// bookmarks/settings/session still need it.
    /// A queued/background write is drained first: a blob renamed after the
    /// delete would resurrect the store. When a stalled disk refuses to
    /// drain within the flush timeout the wipe stays armed and `tick`
    /// completes it as soon as the writer is idle.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.plain_bytes = 0;
        self.dirty = false;
        self.save_error = None;
        self.generation = self.generation.wrapping_add(1);
        self.wipe_pending = true;
        self.finish_wipe();
    }

    /// Applies a new retention policy: writes the preference immediately
    /// and prunes entries that fall outside the new window.
    pub fn set_retention(&mut self, retention: Retention) {
        if self.retention == retention {
            return;
        }
        self.retention = retention;
        if self.available {
            // Read-modify-write of just this key: the Preferences window
            // stores theme/zoom/session in the same file (see `settings`),
            // and a whole-file rewrite here used to clobber them. The file
            // write + `fsync` go to the shared settings worker (no UI stall);
            // `App::on_exit` flushes them.
            if let Err(e) = crate::settings::write_setting_async(
                &self.dir,
                "retention",
                &retention.as_setting(),
            ) {
                self.save_error = Some(format!("could not save settings: {e}"));
            }
        }
        if matches!(retention, Retention::Session) {
            // "Forget on exit": stop the queued writer first (an encrypted
            // blob already headed to disk must not land after the policy
            // changed), then wipe what a previous policy left. Nothing new
            // is persisted from here on. See `finish_wipe` for the
            // timed-out-drain case.
            self.dirty = false;
            self.wipe_pending = true;
            self.finish_wipe();
        } else {
            self.prune(now_secs());
            self.dirty = true;
        }
    }

    /// Debounced save from the UI loop: writes at most every
    /// `HISTORY_SAVE_DEBOUNCE_SECS` while dirty. Returns `true` while a
    /// write is still owed (queued/debouncing), so the caller can schedule a
    /// repaint and an idle window never leaves the last visit unsaved.
    /// The encrypted blob is built here; the file write + fsync run on the
    /// worker thread (see [`StoreWriter`]).
    pub fn tick(&mut self) -> bool {
        // Complete a wipe that had to wait for a background write to drain.
        self.finish_wipe();
        // Surface a finished background write's outcome. A failure is kept
        // visible AND marks the store dirty again (a failed write must be
        // retried, not silently lost: `encrypt_pending` had already cleared
        // `dirty` at enqueue time). A confirmed success clears a previous
        // warning; clearing only on success avoids a warning that flickers
        // while the disk keeps rejecting writes.
        let worker_ok = self.writer.as_ref().is_some_and(StoreWriter::take_success);
        if let Some(error) = self.writer.as_ref().and_then(StoreWriter::take_error) {
            self.save_error = Some(error);
            // A wipe in progress supersedes the retry: the data is being
            // deleted, so a failed write must not re-arm the save path and
            // recreate the files with an empty store.
            if !self.wipe_pending {
                self.dirty = true;
            }
        } else if worker_ok {
            self.save_error = None;
        }
        if self.pending_save()
            && self.last_save.elapsed() >= Duration::from_secs(config::HISTORY_SAVE_DEBOUNCE_SECS)
        {
            self.save_async();
        }
        self.pending_save()
            || self.wipe_pending
            || self.writer.as_ref().is_some_and(StoreWriter::busy)
    }

    /// Is a debounced write still pending? (Never under `Session`, which
    /// only keeps visits in memory, without a data directory, or while a
    /// wipe is still being completed.)
    pub fn pending_save(&self) -> bool {
        self.dirty
            && self.available
            && !matches!(self.retention, Retention::Session)
            && !self.wipe_pending
    }

    /// Clean exit: flushes a dirty store, or wipes the files under the
    /// `Session` policy. A clean store is left untouched (no empty files).
    /// The shared `store.key` is never deleted here: only "Delete all data
    /// and close" removes the whole data directory.
    pub fn on_exit(&mut self) {
        self.flush_writer();
        // A pending wipe must complete BEFORE any save: otherwise a blob
        // the worker drained during exit would survive on disk and the
        // "deleted" history would reappear on the next launch
        // (`clear` only schedules the wipe when the writer is busy).
        if self.wipe_pending {
            self.finish_wipe();
            if self.wipe_pending {
                // A stalled disk kept the writer busy past the flush
                // timeout: exit is the last chance. The delete is queued
                // through the writer itself so it can never be overtaken by
                // the in-flight rename (a direct `wipe_files` could lose that
                // race and resurrect the ciphertext); the separate discard
                // writer removes the target once the stalled rename lands.
                self.queue_discard();
            }
        }
        if matches!(self.retention, Retention::Session) {
            self.entries.clear();
            self.plain_bytes = 0;
            self.dirty = false;
            self.wipe_files();
            return;
        }
        self.save();
    }
}

mod persist;

use persist::entry_bytes;
use persist::load_retention;

pub(crate) use persist::format_age;

#[cfg(test)]
mod tests;
