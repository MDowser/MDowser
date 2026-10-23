// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `history::persist`: pruning, the encrypted file format and the
//! background-writer plumbing for [`HistoryStore`].
//!
//! - `prune`/`trim`: retention and caps (entries, bytes) applied in memory.
//! - `serialize`/`deserialize`: the `timestamp\turl\ttitle` plaintext body.
//! - `key_or_create`, `load_entries`, `preserve_unreadable_blob`: the
//!   AEAD-encrypted `history.bin` + shared `store.key` on disk.
//! - `save_async`/`encrypt_pending`/`flush_writer`/`finish_wipe`/
//!   `wipe_files`: the debounced background write and the delete paths.
//! - `format_age`: the human "x ago" label (also used by `ui::history_page`).
//!
//! All methods here are `impl HistoryStore`; the parent owns the public API
//! and the recording policy.

use super::*;

impl HistoryStore {
    /// Prunes visits older than the retention window.
    pub(super) fn prune(&mut self, now: u64) {
        let Some(keep) = self.retention.keep_for() else {
            return;
        };
        let cutoff = now.saturating_sub(keep.as_secs());
        let before = self.entries.len();
        self.entries.retain(|visit| visit.visited_at >= cutoff);
        if self.entries.len() != before {
            self.dirty = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Recomputes the derived plaintext byte total from `entries`. Used on
    /// load (and by tests that poke `entries` directly); every mutation of
    /// the public API maintains it incrementally instead.
    pub(super) fn recompute_plain_bytes(&mut self) {
        self.plain_bytes = self.entries.iter().map(entry_bytes).sum();
    }

    /// Drops oldest entries until the entry and byte caps hold. Keeping the
    /// newest is what matters for a history menu.
    ///
    /// O(1) on the settled path: the derived byte total is maintained
    /// incrementally (see `HistoryStore::plain_bytes`), so a recorded visit
    /// under both caps no longer walks up to 20 000 entries. The byte walk
    /// still runs when the byte cap is actually crossed.
    pub(super) fn trim(&mut self) {
        if self.entries.len() <= config::MAX_HISTORY_STORE_ENTRIES
            && self.plain_bytes <= config::MAX_HISTORY_STORE_BYTES
        {
            return;
        }
        let before = self.entries.len();
        if self.entries.len() > config::MAX_HISTORY_STORE_ENTRIES {
            let excess = self.entries.len() - config::MAX_HISTORY_STORE_ENTRIES;
            let drained: usize = self.entries[..excess].iter().map(entry_bytes).sum();
            self.entries.drain(0..excess);
            self.plain_bytes = self.plain_bytes.saturating_sub(drained);
        }
        if self.plain_bytes > config::MAX_HISTORY_STORE_BYTES {
            let mut bytes = 0usize;
            let mut first_keep = self.entries.len();
            for (index, visit) in self.entries.iter().enumerate().rev() {
                let size = entry_bytes(visit);
                if bytes.saturating_add(size) > config::MAX_HISTORY_STORE_BYTES {
                    break;
                }
                bytes += size;
                first_keep = index;
            }
            if first_keep > 0 {
                let drained: usize = self.entries[..first_keep].iter().map(entry_bytes).sum();
                self.entries.drain(0..first_keep);
                self.plain_bytes = self.plain_bytes.saturating_sub(drained);
            }
        }
        if self.entries.len() != before {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Reads and decrypts the store file. Any failure leaves the store
    /// empty; the file is kept until a later save replaces it.
    pub(super) fn load_entries(&mut self) {
        let path = self.dir.join(HISTORY_FILE);
        // Refuse absurd files before allocating: a valid file is bounded by
        // the plaintext cap plus header/nonce/tag. Compare in `u64`: the
        // 32-bit `usize` cast could wrap and pass a multi-gigabyte file.
        // `read_regular_file` opens with `O_NONBLOCK` and re-checks the
        // opened handle, so a FIFO swapped in after the metadata check can
        // neither block the UI thread nor be read.
        let max_file =
            (config::MAX_HISTORY_STORE_BYTES + crate::store::crypto::OVERHEAD_BYTES) as u64;
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
            // blank History with no explanation (the doc contract surfaces
            // unreadable stores through `save_error`).
            self.save_error =
                Some("history file could not be read; a backup copy was kept".to_string());
            return;
        };
        self.entries = deserialize(&plain);
        // Rebuild the derived byte total once for the loaded store; every
        // later mutation maintains it incrementally.
        self.recompute_plain_bytes();
        self.trim();
        self.prune(now_secs());
    }

    /// The store file is present but could not be decrypted (missing or
    /// regenerated key, corruption, tampering). Keep a copy as `.bak` before
    /// a future save replaces it: restoring the right key can still recover
    /// the data. [`Self::wipe_files`] removes the backup too, so an explicit
    /// delete really deletes. Atomic `create_new` copy via
    /// [`crate::settings::preserve_blob_bytes`]: never clobbers, no
    /// exists-then-rename TOCTOU, and the current file stays in place.
    pub(super) fn preserve_unreadable_blob(&self, bytes: &[u8]) {
        crate::settings::preserve_blob_bytes(&self.dir, HISTORY_FILE, bytes);
    }

    /// Serializes entries oldest-first as `timestamp\turl\ttitle` lines.
    pub(super) fn serialize(&self) -> Vec<u8> {
        // Rough per-entry budget (timestamp + two tabs + newline + a short
        // URL/title); avoids repeated growth on a large store.
        let mut out = Vec::with_capacity(self.entries.len().saturating_mul(96));
        for visit in &self.entries {
            out.extend_from_slice(visit.visited_at.to_string().as_bytes());
            out.push(b'\t');
            out.extend_from_slice(visit.url.as_bytes());
            out.push(b'\t');
            if let Some(title) = &visit.title {
                out.extend_from_slice(title.as_bytes());
            }
            out.push(b'\n');
        }
        out
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

    /// Writes the encrypted store atomically on the UI thread. Used by
    /// tests, `on_exit` and any caller that must be durable before
    /// returning; waits for queued background writes first so a stale blob
    /// can never be renamed over a newer one. Errors are kept for the
    /// Preferences warning and retried on the next tick.
    pub fn save(&mut self) {
        // A timed-out background write keeps the worker alive: writing a
        // second blob here could land BEFORE the in-flight one and then be
        // clobbered by it. Keep the store dirty and let the next tick
        // re-enqueue through the same worker instead.
        if !self.flush_writer() {
            return;
        }
        if let Some(blob) = self.encrypt_pending() {
            match atomic_write(&self.dir, HISTORY_FILE, &blob) {
                Ok(()) => {
                    self.dirty = false;
                    self.save_error = None;
                }
                Err(e) => self.save_error = Some(format!("could not save history: {e}")),
            }
        }
    }

    /// Queues the debounced UI-path write on the background writer: the
    /// serialization + AEAD encryption stay here (bounded by
    /// `MAX_HISTORY_STORE_BYTES`, a few ms), the file write + `fsync` do not
    /// block a frame.
    pub(super) fn save_async(&mut self) {
        let Some(blob) = self.encrypt_pending() else {
            return;
        };
        let job = WriteJob {
            dir: self.dir.clone(),
            bytes: blob,
            discard: false,
            build: None,
        };
        // `queue_or_respawn` replaces a worker that a flush stopped (or that
        // died) and reports one that exited before taking the job, so the
        // newest blob is never handed to a queue nobody reads. Write order
        // is preserved: a fresh worker only receives a job the old one
        // could not take.
        if crate::store::writer::queue_or_respawn(&mut self.writer, HISTORY_FILE, "history", job) {
            self.dirty = false;
        } else {
            self.dirty = true;
            self.save_error = Some("could not save history: writer unavailable".to_string());
        }
    }

    /// Serializes + encrypts the dirty store. `None` when there is nothing
    /// to write or a step failed (the reason is stored in `save_error`).
    pub(super) fn encrypt_pending(&mut self) -> Option<Vec<u8>> {
        self.last_save = Instant::now();
        if !self.available || !self.dirty || matches!(self.retention, Retention::Session) {
            self.dirty = false;
            return None;
        }
        self.trim();
        let Some(key) = self.key_or_create() else {
            self.save_error = Some("history key unavailable".to_string());
            return None;
        };
        let plain = self.serialize();
        match encrypt(&key, FILE_MAGIC, FILE_VERSION, &plain) {
            Some(blob) => Some(blob),
            None => {
                self.save_error = Some("history encryption failed".to_string());
                None
            }
        }
    }

    /// Waits (bounded) for every queued background write to land. Returns
    /// `true` when the worker drained and exited; on timeout the writer is
    /// KEPT (never joined), so a stuck `fsync` on a hung disk cannot freeze a
    /// frame, "Delete history" or exit forever. Further saves go through the
    /// same worker (or a fresh one when it has stopped), preserving write
    /// order.
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
        let drained = worker.flush(Duration::from_secs(config::HISTORY_SAVE_FLUSH_TIMEOUT_SECS));
        if let Some(error) = worker.take_error() {
            self.save_error = Some(error);
            // A failed background write must be retried synchronously by
            // `save` (the caller), not only by the next `tick`: `save_async`
            // cleared `dirty` at enqueue time, so a lost write would
            // otherwise look clean and the data would never reach disk.
            if !self.wipe_pending {
                self.dirty = true;
            }
        }
        if drained {
            self.writer = None;
            true
        } else {
            if self.save_error.is_none() {
                self.save_error = Some("history write still in progress; will retry".to_string());
            }
            false
        }
    }

    /// Completes an armed wipe once the background writer can no longer
    /// rename a stale blob over the deleted files. `flush_writer` drains and
    /// joins the worker; when a stalled disk runs out the flush timeout the
    /// wipe stays armed for the next `tick` (never a permanent-freeze wait).
    pub(super) fn finish_wipe(&mut self) {
        if !self.wipe_pending {
            return;
        }
        // Cheap poll first: while the worker is still draining an in-flight
        // write, never block a frame waiting for it; the next tick retries.
        if self.writer.as_ref().is_some_and(StoreWriter::busy) {
            return;
        }
        // Queue is empty and the worker is idle: the join below is fast.
        if !self.flush_writer() {
            if self.save_error.is_none() {
                self.save_error = Some("history is still being deleted; will retry".to_string());
            }
            return;
        }
        self.wipe_pending = false;
        // The wipe supersedes any earlier background-write warning; a
        // delete failure below is the only one that must survive.
        self.save_error = None;
        self.wipe_files();
    }

    /// Queues the wipe as a serialized delete job (exit-time fallback for a
    /// writer that would not drain: see [`HistoryStore::on_exit`]).
    ///
    /// The job replaces any queued-but-unwritten blob and runs AFTER the
    /// in-flight write on the worker that queued it, so a stalled rename can
    /// never land after the delete and resurrect a deleted store. A worker
    /// that already exited is replaced; `push` reports the exit race under
    /// the same lock the worker takes, so the discard can never be left in a
    /// queue nobody reads. `wipe_pending` clears only once the delete is
    /// really the writer's responsibility.
    pub(super) fn queue_discard(&mut self) {
        if !self.available {
            self.wipe_pending = false;
            return;
        }
        if crate::store::writer::queue_or_respawn(
            &mut self.writer,
            HISTORY_FILE,
            "history",
            crate::store::writer::WriteJob {
                dir: self.dir.clone(),
                bytes: Vec::new(),
                discard: true,
                build: None,
            },
        ) {
            self.wipe_pending = false;
        } else if self.save_error.is_none() {
            // The wipe stays ARMED: a later tick retries through `finish_wipe`
            // instead of looking successful while the ciphertext survives.
            self.save_error = Some("could not delete history: writer unavailable".to_string());
        }
    }

    /// Removes the history file (settings stay, shared key stays). Failures
    /// are reported through `save_error`: "Delete history now" must not look
    /// successful while the ciphertext survives on disk.
    pub(super) fn wipe_files(&mut self) {
        if !self.available {
            return;
        }
        let backup = format!("{HISTORY_FILE}.bak");
        for name in [HISTORY_FILE, backup.as_str()] {
            match std::fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.save_error = Some(format!("could not delete {name}: {e}"));
                }
            }
        }
        // Orphaned temp files from an interrupted `atomic_write` (crash, full
        // disk) are ciphertext: remove every one this module could ever have
        // created. The shared `store.key` (+ its tmp/repair siblings) is
        // never touched here.
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            let cipher_prefix = format!("{HISTORY_FILE}.tmp.");
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(&cipher_prefix) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Plaintext bytes one entry costs in the serialized file (upper bound).
pub(super) fn entry_bytes(visit: &Visit) -> usize {
    // 20 timestamp digits (u64 max, so the bound holds for a forged file
    // too) + two tabs + newline + fields.
    24 + visit.url.len() + visit.title.as_deref().map(str::len).unwrap_or(0)
}

/// Parses the serialized plaintext. Invalid lines are skipped: a partial
/// or hand-edited (impossible without the key, but belt-and-braces) body
/// must never poison the store.
pub(super) fn deserialize(bytes: &[u8]) -> Vec<Visit> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(
        text.len()
            .div_ceil(64)
            .min(config::MAX_HISTORY_STORE_ENTRIES),
    );
    for line in text.lines().take(config::MAX_HISTORY_STORE_ENTRIES + 1) {
        let mut parts = line.splitn(3, '\t');
        let (Some(timestamp), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(visited_at) = timestamp.parse::<u64>() else {
            continue;
        };
        // Normalized on load too: a store written by an older build (or a
        // hand-edited body) cannot smuggle credentials back into the UI.
        let Some(url) = normalize_stored_url(url) else {
            continue;
        };
        let title = parts.next().map(sanitize_title).filter(|t| !t.is_empty());
        out.push(Visit {
            url,
            title,
            visited_at,
        });
    }
    out
}

/// Reads the retention preference; a missing/corrupt value yields the
/// default (3 months), never an error.
pub(super) fn load_retention(dir: &Path) -> Retention {
    read_setting(dir, "retention")
        .and_then(|value| Retention::from_setting(&value))
        .unwrap_or_default()
}

/// Human age of a visit, relative to `now` (both Unix seconds).
///
/// Borrowed for the fixed buckets ("just now", "yesterday", "1 week ago",
/// ...): the history page renders one per visible row per frame, and a
/// `String` per row was pure churn.
pub(crate) fn format_age(now: u64, visited_at: u64) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    let age = now.saturating_sub(visited_at);
    if age < 60 {
        return Cow::Borrowed("just now");
    }
    if age < 3_600 {
        return Cow::Owned(format!("{} min ago", age / 60));
    }
    if age < 86_400 {
        return Cow::Owned(format!("{} h ago", age / 3_600));
    }
    if age < 172_800 {
        return Cow::Borrowed("yesterday");
    }
    if age < 604_800 {
        return Cow::Owned(format!("{} days ago", age / 86_400));
    }
    if age < 2_592_000 {
        let weeks = age / 604_800;
        return if weeks == 1 {
            Cow::Borrowed("1 week ago")
        } else {
            Cow::Owned(format!("{weeks} weeks ago"))
        };
    }
    if age < 31_536_000 {
        let months = age / 2_592_000;
        return if months == 1 {
            Cow::Borrowed("1 month ago")
        } else {
            Cow::Owned(format!("{months} months ago"))
        };
    }
    let years = age / 31_536_000;
    if years == 1 {
        Cow::Borrowed("1 year ago")
    } else {
        Cow::Owned(format!("{years} years ago"))
    }
}
