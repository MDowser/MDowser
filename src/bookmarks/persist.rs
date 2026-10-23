// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `bookmarks::persist`: the encrypted on-disk format and the debounced
//! background writer for [`BookmarkStore`].
//!
//! - `serialize`/`deserialize` + `write_entry`/`entry_bytes`/
//!   `sanitize_glyph`: the `timestamp\turl\ttitle\ticon\tcolor` plaintext.
//! - `key_or_create`, `load_entries`, `preserve_unreadable_blob`: the
//!   `bookmarks.bin` + shared `store.key` files.
//! - `trim`, `rebuild_url_index`, `rebuild_plain_bytes`: in-memory caps and
//!   derived data.
//! - `save`, `save_async`, `encrypt_pending`, `flush_writer`: the debounced
//!   background write (shared with `history`, `store::writer`).

use super::*;

/// Serializes a bookmark SNAPSHOT (oldest-first lines), honouring the
/// reader's byte budget. Shared by the synchronous `save` path and the
/// worker-side builder (which owns a snapshot and never touches the store).
pub(super) fn serialize_entries(entries: &[Bookmark]) -> Vec<u8> {
    let mut out = Vec::with_capacity(config::MAX_BOOKMARK_STORE_BYTES.min(64 * 1024));
    for bookmark in entries {
        if out.len().saturating_add(entry_bytes(bookmark)) > config::MAX_BOOKMARK_STORE_BYTES {
            break;
        }
        write_entry(&mut out, bookmark);
    }
    out
}

impl BookmarkStore {
    /// Reads and decrypts the store file. Any failure leaves the store
    /// empty; the file is kept until a later save replaces it.
    pub(super) fn load_entries(&mut self) {
        let path = self.dir.join(BOOKMARKS_FILE);
        // Refuse absurd files before allocating: a valid file is bounded by
        // the plaintext cap plus header/nonce/tag. Compare in `u64`: the
        // 32-bit `usize` cast could wrap and pass a multi-gigabyte file.
        // `read_regular_file` opens with `O_NONBLOCK` and re-checks the
        // opened handle, so a FIFO swapped in after the metadata check can
        // neither block the UI thread nor be read.
        let max_file = config::MAX_BOOKMARK_STORE_FILE_BYTES as u64;
        let Some(bytes) = crate::settings::read_regular_file(&path, max_file) else {
            return;
        };
        let Some(key) = load_or_create_key(&self.dir.join(KEY_FILE)) else {
            return;
        };
        self.key = Some(key);
        let Some(plain) = decrypt(&key, FILE_MAGIC, FILE_VERSION, &bytes) else {
            self.preserve_unreadable_blob(&bytes);
            // The store loads empty; without this note the user would see a
            // blank Home with no explanation (the doc contract surfaces
            // unreadable stores through `save_error`).
            self.save_error =
                Some("bookmarks file could not be read; a backup copy was kept".to_string());
            return;
        };
        self.entries = deserialize(&plain);
        self.rebuild_url_index();
        self.rebuild_plain_bytes();
        self.trim();
    }

    /// The store file is present but could not be decrypted (missing or
    /// regenerated key, corruption, tampering). Keep a copy as `.bak` before
    /// a future save replaces it: restoring the right key can still recover
    /// the bookmarks. Atomic `create_new` copy via
    /// [`crate::settings::preserve_blob_bytes`]: never clobbers, no
    /// exists-then-rename TOCTOU, and the current file stays in place.
    pub(super) fn preserve_unreadable_blob(&self, bytes: &[u8]) {
        crate::settings::preserve_blob_bytes(&self.dir, BOOKMARKS_FILE, bytes);
    }

    /// Serializes bookmarks oldest-first as
    /// `timestamp\turl\ttitle\ticon\tcolor` lines (empty fields when unset).
    ///
    /// The body carries the SAME budget the reader enforces
    /// (`MAX_BOOKMARK_STORE_BYTES`), checked BEFORE each entry so the body
    /// can never be one entry over it.
    pub(super) fn serialize(&self) -> Vec<u8> {
        serialize_entries(&self.entries)
    }

    /// The cached key, created on first use.
    pub(super) fn key_or_create(&mut self) -> Option<[u8; 32]> {
        if let Some(key) = self.key {
            return Some(key);
        }
        if !self.available {
            return None;
        }
        let key = load_or_create_key(&self.dir.join(KEY_FILE))?;
        self.key = Some(key);
        Some(key)
    }

    /// Drops oldest bookmarks until BOTH caps hold: the entry count and the
    /// serialized byte budget (URLs are percent-encoded on the way in, so a
    /// store written by a different version could exceed either). Adds at
    /// the cap are refused in [`Self::toggle`]; this only repairs a body
    /// that arrived from disk.
    ///
    /// The derived data (`urls`, `plain_bytes`, `generation`) is rebuilt
    /// only when a bookmark was actually dropped: `trim` also runs on every
    /// save, and cloning the whole URL index per save was pure waste.
    pub(super) fn trim(&mut self) {
        let before = self.entries.len();
        if self.entries.len() > config::MAX_BOOKMARK_STORE_ENTRIES {
            let excess = self.entries.len() - config::MAX_BOOKMARK_STORE_ENTRIES;
            // Charge the drained entries to the byte count BEFORE the byte
            // loop below: `plain_bytes` still counted them, so the loop
            // would drop additional live bookmarks to compensate for
            // phantom bytes (silent data loss at the entry cap).
            let drained_bytes = self.entries[..excess]
                .iter()
                .map(entry_bytes)
                .fold(0usize, usize::saturating_add);
            self.entries.drain(0..excess);
            self.plain_bytes = self.plain_bytes.saturating_sub(drained_bytes);
        }
        if self.plain_bytes > config::MAX_BOOKMARK_STORE_BYTES && !self.entries.is_empty() {
            // Find the cut first and drain ONCE: `remove(0)` in a loop shifted
            // every remaining entry per victim (O(n²) on a body that arrived
            // from disk over budget).
            let mut cut = 0usize;
            let mut freed = 0usize;
            while cut < self.entries.len()
                && self.plain_bytes.saturating_sub(freed) > config::MAX_BOOKMARK_STORE_BYTES
            {
                freed = freed.saturating_add(entry_bytes(&self.entries[cut]));
                cut += 1;
            }
            self.entries.drain(0..cut);
            self.plain_bytes = self.plain_bytes.saturating_sub(freed);
        }
        if self.entries.len() != before {
            self.rebuild_url_index();
            self.rebuild_plain_bytes();
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Rebuilds the exact URL index from `entries` (bulk paths: load/trim).
    pub(super) fn rebuild_url_index(&mut self) {
        self.urls.clear();
        self.urls
            .extend(self.entries.iter().map(|bookmark| bookmark.url.clone()));
    }

    /// Recomputes [`Self::plain_bytes`] from `entries` (bulk paths:
    /// load/trim). The single mutation paths keep it exact incrementally.
    pub(super) fn rebuild_plain_bytes(&mut self) {
        self.plain_bytes = self
            .entries
            .iter()
            .map(entry_bytes)
            .fold(0usize, usize::saturating_add);
    }

    /// Writes the encrypted store atomically on the UI thread. Used by
    /// tests, `on_exit` and any caller that must be durable before
    /// returning; waits for queued background writes first so a stale blob
    /// can never be renamed over a newer one. Errors are kept for the Home
    /// warning and retried on the next tick.
    pub fn save(&mut self) {
        // A timed-out background write keeps the worker alive: writing a
        // second blob here could land BEFORE the in-flight one and then be
        // clobbered by it. Keep the store dirty and let the next tick
        // re-enqueue through the same worker instead.
        if !self.flush_writer() {
            return;
        }
        if let Some(blob) = self.encrypt_pending() {
            match atomic_write(&self.dir, BOOKMARKS_FILE, &blob) {
                Ok(()) => {
                    self.dirty = false;
                    self.save_error = None;
                }
                Err(e) => self.save_error = Some(format!("could not save bookmarks: {e}")),
            }
        }
    }

    /// Queues the debounced UI-path write on the background writer.
    ///
    /// The WORKER serializes + encrypts: at the 48 MiB cap the old
    /// UI-thread build copied and encrypted megabytes inside a frame, and a
    /// failing disk repeated that every debounce interval. The UI thread
    /// only clones the entries snapshot (small in practice: URLs and
    /// titles) and queues the job; the file write + `fsync` never blocked a
    /// frame either way.
    pub(super) fn save_async(&mut self) {
        if !self.available || !self.dirty {
            self.dirty = false;
            return;
        }
        self.last_save = Instant::now();
        self.trim();
        let entries = self.entries.clone();
        let build = move |dir: &std::path::Path| -> Result<Vec<u8>, String> {
            let key = load_or_create_key(&dir.join(KEY_FILE))
                .ok_or_else(|| "bookmark key unavailable".to_string())?;
            let plain = serialize_entries(&entries);
            encrypt(&key, FILE_MAGIC, FILE_VERSION, &plain)
                .ok_or_else(|| "bookmark encryption failed".to_string())
        };
        let job = WriteJob {
            dir: self.dir.clone(),
            bytes: Vec::new(),
            discard: false,
            build: Some(Box::new(build)),
        };
        // `queue_or_respawn` replaces a worker that a flush stopped (or that
        // died) and reports one that exited before taking the job, so the
        // newest blob is never handed to a queue nobody reads. Write order
        // is preserved: a fresh worker only receives a job the old one
        // could not take.
        if crate::store::writer::queue_or_respawn(
            &mut self.writer,
            BOOKMARKS_FILE,
            "bookmarks",
            job,
        ) {
            self.dirty = false;
        } else {
            self.dirty = true;
            self.save_error = Some("could not save bookmarks: writer unavailable".to_string());
        }
    }

    /// Serializes + encrypts the dirty store. `None` when there is nothing
    /// to write or a step failed (the reason is stored in `save_error`).
    pub(super) fn encrypt_pending(&mut self) -> Option<Vec<u8>> {
        self.last_save = Instant::now();
        if !self.available || !self.dirty {
            self.dirty = false;
            return None;
        }
        self.trim();
        let Some(key) = self.key_or_create() else {
            self.save_error = Some("bookmark key unavailable".to_string());
            return None;
        };
        let plain = self.serialize();
        match encrypt(&key, FILE_MAGIC, FILE_VERSION, &plain) {
            Some(blob) => Some(blob),
            None => {
                self.save_error = Some("bookmark encryption failed".to_string());
                None
            }
        }
    }

    /// Waits (bounded) for every queued background write to land. Returns
    /// `true` when the worker drained and exited; on timeout the writer is
    /// KEPT (never joined), so a stuck `fsync` on a hung disk cannot freeze
    /// a frame or exit forever. Further saves go through the same worker
    /// (or a fresh one when it has stopped), preserving write order.
    pub(super) fn flush_writer(&mut self) -> bool {
        let Some(worker) = self.writer.as_mut() else {
            return true;
        };
        // A dead worker can never drain: drop it and let the synchronous
        // path write (the caller retries through `save`/`save_async`, which
        // respawns a fresh worker).
        if worker.worker_dead() {
            self.writer = None;
            return true;
        }
        let drained = worker.flush(Duration::from_secs(
            config::BOOKMARKS_SAVE_FLUSH_TIMEOUT_SECS,
        ));
        if let Some(error) = worker.take_error() {
            self.save_error = Some(error);
            // Same contract as history: a failed background write re-arms
            // `dirty` so `save` retries synchronously instead of trusting a
            // clean flag that was set at enqueue time.
            self.dirty = true;
        }
        if drained {
            self.writer = None;
            true
        } else {
            if self.save_error.is_none() {
                self.save_error = Some("bookmark write still in progress; will retry".to_string());
            }
            false
        }
    }
}

/// Appends one serialized bookmark line (including the trailing `\n`). The
/// single writer for the store format: `entry_bytes` needs no separate size
/// math to stay in sync.
pub(super) fn write_entry(out: &mut Vec<u8>, bookmark: &Bookmark) {
    out.extend_from_slice(bookmark.added_at.to_string().as_bytes());
    out.push(b'\t');
    out.extend_from_slice(bookmark.url.as_bytes());
    out.push(b'\t');
    if let Some(title) = &bookmark.title {
        out.extend_from_slice(title.as_bytes());
    }
    out.push(b'\t');
    if let Some(icon) = &bookmark.icon {
        out.extend_from_slice(icon.as_bytes());
    }
    out.push(b'\t');
    if let Some(color) = bookmark.color {
        out.extend_from_slice(color.to_string().as_bytes());
    }
    out.push(b'\n');
}

/// Serialized size of one bookmark line, computed arithmetically in the
/// same shape [`write_entry`] emits (no throwaway buffer per call: the
/// per-entry accounting runs on every mutation and every trim).
pub(crate) fn entry_bytes(bookmark: &Bookmark) -> usize {
    /// Decimal digits of `value` (at least one).
    fn digits(mut value: u64) -> usize {
        let mut count = 1;
        while value >= 10 {
            value /= 10;
            count += 1;
        }
        count
    }
    // `timestamp \t url \t title \t icon \t color \n`
    5 + digits(bookmark.added_at)
        + bookmark.url.len()
        + bookmark.title.as_deref().map_or(0, str::len)
        + bookmark.icon.as_deref().map_or(0, str::len)
        + bookmark.color.map_or(0, |color| digits(u64::from(color)))
}

/// Parses the serialized plaintext. Invalid lines are skipped: a partial
/// or hand-edited (impossible without the key, but belt-and-braces) body
/// must never poison the store. A version-1 line written before the tile
/// customization existed has only three fields; `icon`/`color` stay unset.
pub(crate) fn deserialize(bytes: &[u8]) -> Vec<Bookmark> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(
        text.len()
            .div_ceil(64)
            .min(config::MAX_BOOKMARK_STORE_ENTRIES),
    );
    for line in text.lines().take(config::MAX_BOOKMARK_STORE_ENTRIES + 1) {
        let mut parts = line.splitn(5, '\t');
        let (Some(timestamp), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(added_at) = timestamp.parse::<u64>() else {
            continue;
        };
        // Normalized on load too: a store written by an older build (or a
        // hand-edited body) cannot smuggle credentials back into the UI.
        let Some(url) = normalize_stored_url(url) else {
            continue;
        };
        let title = parts.next().map(sanitize_title).filter(|t| !t.is_empty());
        let icon = parts.next().and_then(sanitize_glyph);
        let color = parts
            .next()
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|index| *index < config::MAX_BOOKMARK_COLORS);
        out.push(Bookmark {
            url,
            title,
            added_at,
            icon,
            color,
        });
    }
    out
}

/// Sanitizes a custom tile glyph: drops control characters (a pasted
/// newline/tab must not break the serialized line or the layout), trims
/// and drops whitespace, and caps the result at
/// `MAX_BOOKMARK_ICON_CHARS`. Format characters are KEPT here (unlike
/// `sanitize_title`): zero-width joiners and variation selectors are part
/// of standard emoji clusters. `None` when nothing printable remains.
pub(super) fn sanitize_glyph(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len().min(MAX_GLYPH_BYTES));
    for c in raw
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .take(config::MAX_BOOKMARK_ICON_CHARS)
    {
        out.push(c);
    }
    (!out.is_empty()).then_some(out)
}

/// Byte budget for [`sanitize_glyph`]'s allocation: the char cap times the
/// worst-case UTF-8 width (4 bytes per char).
const MAX_GLYPH_BYTES: usize = config::MAX_BOOKMARK_ICON_CHARS * 4;
