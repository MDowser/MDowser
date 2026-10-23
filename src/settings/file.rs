// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `settings::file`: encrypted `settings.bin` read/write (key by key) and
//! its dedicated background writer.
//!
//! Every key is written independently (decrypt + read-modify-write +
//! encrypt + atomic rename) against a per-directory plaintext cache, so the
//! history module and the Preferences window never clobber each other; the
//! file write + `fsync` run on a background worker.

use super::*;

// ---------------------------------------------------------------------------
// Encrypted low-level file helpers (shared with `history`/`bookmarks`)
// ---------------------------------------------------------------------------

/// Reads + decrypts `dir/file` with the shared store key. `None` on any
/// failure (missing, non-regular, oversized, unreadable key, decrypt or UTF-8
/// failure). A present-but-undecryptable blob is preserved as `.bak` once
/// (like history/bookmarks) before being treated as missing, so restoring
/// the matching key can still recover it. Legacy plaintext files are never
/// read here.
pub(crate) fn read_encrypted_file(
    dir: &Path,
    file: &str,
    magic: &[u8; 4],
    version: u8,
    max_plain: u64,
) -> Option<Vec<u8>> {
    let path = dir.join(file);
    // Encrypted size = plaintext cap + header/nonce/tag. `read_regular_file`
    // opens with `O_NONBLOCK` and re-checks the opened handle, so a FIFO
    // swapped in after the metadata check can neither block the UI thread
    // nor be read.
    let bytes = read_regular_file(
        &path,
        max_plain + crate::store::crypto::OVERHEAD_BYTES as u64,
    )?;
    let key = settings_writer().store_key(dir)?;
    match decrypt(&key, magic, version, &bytes) {
        Some(plain) => Some(plain),
        None => {
            // Keep the first unreadable blob for recovery; the current file
            // stays until a later save replaces it. Atomic `create_new`
            // copy: never clobbers, no exists-then-rename TOCTOU.
            preserve_blob_bytes(dir, file, &bytes);
            None
        }
    }
}

/// Decrypted settings plaintext, when present and valid.
pub(crate) fn read_settings_plaintext(dir: &Path) -> Option<String> {
    let plain = read_encrypted_file(
        dir,
        SETTINGS_FILE,
        SETTINGS_MAGIC,
        SETTINGS_VERSION,
        MAX_SETTINGS_BYTES,
    )?;
    std::str::from_utf8(&plain).ok().map(str::to_string)
}

/// Reads one `key=value` line from the encrypted `settings.bin`, when present.
/// Legacy `settings.conf` is never read (clean break).
///
/// A successfully read body is cached for the session (`settings_writer`'s
/// per-directory cache), so loading four keys costs one decrypt, not four.
/// A missing or unreadable file is NOT cached: the next call must re-check
/// the disk instead of resurrecting an empty body.
pub(crate) fn read_setting(dir: &Path, key: &str) -> Option<String> {
    cleanup_legacy_files(dir);
    let text = {
        let writer = settings_writer();
        let mut cache = writer.body.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(dir) {
            Some(body) => body.clone(),
            None => {
                let body = read_settings_plaintext(dir)?;
                cache.insert(dir.to_path_buf(), body.clone());
                body
            }
        }
    };
    let prefix = format!("{key}=");
    // Scan every line (the plaintext is already capped by `MAX_SETTINGS_BYTES`):
    // `write_setting` preserves an arbitrary number of keys, so a short line
    // read cap could silently hide keys this app itself wrote.
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix.as_str())
            .map(|value| value.trim().to_string())
    })
}

/// `Some(reason)` when `settings.bin` exists but cannot be trusted: not a
/// regular file, over the cap, unreadable or undecryptable. A MISSING file
/// is not an error (defaults apply).
///
/// The distinction matters to [`Prefs::load_from`]: it must not delete the
/// encrypted `session.bin` because a transient read failure looked like
/// "restore tabs off".
pub(crate) fn settings_file_error(dir: &Path) -> Option<String> {
    let path = dir.join(SETTINGS_FILE);
    let meta = match std::fs::metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => return Some(format!("could not read {SETTINGS_FILE}: {e}")),
        Ok(meta) => meta,
    };
    if !meta.is_file() {
        return Some(format!("{SETTINGS_FILE} is not a regular file"));
    }
    if meta.len() > MAX_SETTINGS_BYTES + crate::store::crypto::OVERHEAD_BYTES as u64 {
        return Some(format!("{SETTINGS_FILE} is too large; defaults are used"));
    }
    // `read_regular_file` re-checks the opened handle (`O_NONBLOCK`), so a
    // FIFO swapped in after the metadata check cannot block this path either.
    let Some(bytes) = read_regular_file(
        &path,
        MAX_SETTINGS_BYTES + crate::store::crypto::OVERHEAD_BYTES as u64,
    ) else {
        return Some(format!("could not read {SETTINGS_FILE}"));
    };
    match settings_writer().store_key(dir) {
        None => Some(format!("could not read {SETTINGS_FILE}: key unavailable")),
        Some(key) => match decrypt(&key, SETTINGS_MAGIC, SETTINGS_VERSION, &bytes) {
            None => Some(format!(
                "could not decrypt {SETTINGS_FILE}; defaults are used"
            )),
            Some(plain) => match String::from_utf8(plain) {
                Ok(text) => {
                    // Seed the shared body cache: the caller reads the keys
                    // through it, so a valid file is read + decrypted ONCE
                    // per process at startup instead of twice.
                    let writer = settings_writer();
                    let mut cache = writer.body.lock().unwrap_or_else(|e| e.into_inner());
                    cache.entry(dir.to_path_buf()).or_insert(text);
                    None
                }
                Err(e) => Some(format!("could not read {SETTINGS_FILE}: {e}")),
            },
        },
    }
}

/// Writes one key to `settings.bin` (encrypted), preserving every OTHER key.
///
/// The read-modify-write runs against the shared in-memory body cache (the
/// same one [`write_setting_async`] uses), so a burst of preference changes
/// neither re-decrypts the file per key nor loses another key to a stale
/// on-disk copy. The file write + `fsync` happen on the CALLER thread: this
/// variant is test-only (the app path is [`write_setting_async`], so the UI
/// thread never `fsync`s).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_setting(dir: &Path, key: &str, value: &str) -> io::Result<()> {
    let body = settings_body_with(dir, key, value)?;
    let store_key = settings_writer()
        .store_key(dir)
        .ok_or_else(|| io::Error::other("store key unavailable"))?;
    let blob = encrypt(
        &store_key,
        SETTINGS_MAGIC,
        SETTINGS_VERSION,
        body.as_bytes(),
    )
    .ok_or_else(|| io::Error::other("settings encryption failed"))?;
    atomic_write(dir, SETTINGS_FILE, &blob)
}

/// [`write_setting`] through the process-wide background writer: the body
/// update (bounded, in memory) stays on the caller, the file write +
/// `fsync` do not. A dedicated worker serializes every `settings.bin` write
/// (a per-call thread could reorder two quick preference changes);
/// [`flush_settings_writes`] makes them durable (exit paths, tests).
pub(crate) fn write_setting_async(dir: &Path, key: &str, value: &str) -> io::Result<()> {
    let body = settings_body_with(dir, key, value)?;
    let key_bytes = settings_writer()
        .store_key(dir)
        .ok_or_else(|| io::Error::other("store key unavailable"))?;
    let blob = encrypt(
        &key_bytes,
        SETTINGS_MAGIC,
        SETTINGS_VERSION,
        body.as_bytes(),
    )
    .ok_or_else(|| io::Error::other("settings encryption failed"))?;
    // A fresh worker per call would lose write ordering: use the shared one.
    // A refused queue is a real failure the caller must see (it used to be
    // reported as a success).
    if !settings_writer().enqueue(dir.to_path_buf(), SETTINGS_FILE, blob) {
        return Err(io::Error::other("could not queue the settings save"));
    }
    Ok(())
}

/// Applies one `key=value` to the CURRENT settings plaintext and returns the
/// new body. The plaintext is cached per directory in the shared writer, so
/// a burst of preference changes neither re-decrypts the file per key nor
/// loses the other keys to a stale on-disk copy.
///
/// A MISSING file starts from an empty body (defaults). A file that is
/// PRESENT but cannot be read or decrypted is an error, never an empty body:
/// rewriting from "" would silently drop every other stored key.
fn settings_body_with(dir: &Path, key: &str, value: &str) -> io::Result<String> {
    let writer = settings_writer();
    let mut cache = writer.body.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(body) = cache.get_mut(dir) {
        // The cache is authoritative for this process: the first read
        // populated it (or the file was absent then). The update runs on a
        // COPY: an over-budget body is refused WITHOUT poisoning the cached
        // body (a later, smaller change must still be able to write).
        let mut updated = body.clone();
        set_key_in_body(&mut updated, key, value);
        check_settings_budget(&updated)?;
        *body = updated.clone();
        return Ok(updated);
    }
    let body = match read_settings_plaintext(dir) {
        Some(text) => text,
        None => match std::fs::metadata(dir.join(SETTINGS_FILE)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(io::Error::other(format!(
                    "{SETTINGS_FILE} is unreadable ({e}); refusing to overwrite it"
                )))
            }
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{SETTINGS_FILE} is unreadable; refusing to overwrite it"
                )))
            }
        },
    };
    let mut body = body;
    set_key_in_body(&mut body, key, value);
    check_settings_budget(&body)?;
    // Create/cache the body for the rest of the process. `cleanup_legacy_files`
    // runs once per directory here instead of on every read/write.
    cleanup_legacy_files(dir);
    cache.insert(dir.to_path_buf(), body.clone());
    Ok(body)
}

/// Refuses a body the READER would reject, so a change can never produce a
/// file that silently falls back to defaults on the next launch (the reader
/// caps the whole blob at `MAX_SETTINGS_BYTES + OVERHEAD_BYTES`).
fn check_settings_budget(body: &str) -> io::Result<()> {
    if body.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(io::Error::other(format!(
            "the change would push {SETTINGS_FILE} past its {MAX_SETTINGS_BYTES}-byte budget"
        )));
    }
    Ok(())
}

/// Process-wide background writers for `settings.bin`/`session.bin`, plus
/// the per-directory settings plaintext cache the read-modify-write runs
/// against.
///
/// Each file has its own serialized worker: two quick preference changes
/// cannot reorder, and one file's newest-wins replacement can never drop the
/// other file's queued blob. The cache
/// keeps the body the UI thread mutates, so no key is lost and no decrypt
/// happens per change.
pub(crate) struct SettingsWrites {
    writer: Mutex<Option<crate::store::writer::StoreWriter>>,
    /// Separate worker for `session.bin`: the two files are independent
    /// (each is replaced atomically), and one bounded queue cannot lose the
    /// other file's newest blob to newest-wins replacement.
    session_writer: Mutex<Option<crate::store::writer::StoreWriter>>,
    /// Current plaintext per data directory (`write_setting` keys are
    /// read-modify-written against it; bounded by the app's one directory
    /// plus each test's scratch dir).
    body: Mutex<std::collections::HashMap<PathBuf, String>>,
    /// The shared `store.key` bytes per data directory, loaded once per
    /// process. The key is created once and never rotated, so re-reading +
    /// re-validating (and potentially repairing) the key file on every
    /// preference and session write blocked the UI thread on disk for
    /// nothing; history/bookmarks already hold the same key in memory.
    /// Keyed by directory because tests use many scratch directories.
    store_key: Mutex<std::collections::HashMap<PathBuf, [u8; 32]>>,
    /// A blob that could not even be queued (two worker spawns refused it):
    /// surfaced through [`Self::take_outcome`] like a write failure. Keyed by
    /// the file it belongs to, so the Preferences warning lands on the row
    /// it names (restore-tabs vs the Appearance card) and the session
    /// bookkeeping is not told a session save was accepted.
    queue_error: Mutex<Option<(bool, String)>>,
    /// Outcome of a worker that a `flush` RETIRED: dropping it would take a
    /// failed write's warning with it (the "restore tabs off" path flushes
    /// mid-session), so it is parked here and reported by `take_outcome`
    /// exactly like a live worker's.
    drained: Mutex<WriteOutcomes>,
}

static SETTINGS_WRITES: std::sync::OnceLock<SettingsWrites> = std::sync::OnceLock::new();

/// The shared settings writer/cache (lazily initialized).
pub(crate) fn settings_writer() -> &'static SettingsWrites {
    SETTINGS_WRITES.get_or_init(|| SettingsWrites {
        writer: Mutex::new(None),
        session_writer: Mutex::new(None),
        body: Mutex::new(std::collections::HashMap::new()),
        store_key: Mutex::new(std::collections::HashMap::new()),
        queue_error: Mutex::new(None),
        drained: Mutex::new(WriteOutcomes::default()),
    })
}

impl SettingsWrites {
    /// The shared store key, loaded + validated ONCE per process.
    ///
    /// Returns `None` when the key cannot be loaded/created (the caller
    /// surfaces the failure like before). The cache is only populated on
    /// success, so a transiently unavailable key is retried on the next call.
    pub(super) fn store_key(&self, dir: &Path) -> Option<[u8; 32]> {
        let mut cache = self.store_key.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(key) = cache.get(dir) {
            return Some(*key);
        }
        let key = load_or_create_key(&dir.join(STORE_KEY_FILE))?;
        cache.insert(dir.to_path_buf(), key);
        Some(key)
    }

    /// Queues one encrypted blob (replacing a queued-but-unwritten one:
    /// newest wins) and respawns the worker when the previous one had
    /// already exited. Ordering is preserved: a fresh worker is only
    /// created for a job the old worker could not take.
    ///
    /// `false` when the job was NOT queued (a real resource failure): the
    /// caller must not mark the data as stored.
    pub(super) fn enqueue(&self, dir: PathBuf, file: &'static str, blob: Vec<u8>) -> bool {
        let is_session = file == SESSION_FILE;
        let slot = if is_session {
            &self.session_writer
        } else {
            &self.writer
        };
        let label = if is_session { "session" } else { "settings" };
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        let job = crate::store::writer::WriteJob {
            dir,
            bytes: blob,
            discard: false,
            build: None,
        };
        // `queue_or_respawn` reports a worker that exited before taking the
        // job, so the newest blob can never be handed to a queue nobody
        // reads (a stopped-and-drained worker used to swallow it silently).
        if !crate::store::writer::queue_or_respawn(&mut guard, file, label, job) {
            // Two spawns in a row refused the job: a real resource failure.
            // It reaches Preferences through `take_outcome`, and the next
            // change (or `tick`) retries through the same path.
            self.note_queue_error(is_session, format!("could not queue the {label} save"));
            return false;
        }
        true
    }

    /// Records a queue failure against the file it belongs to.
    fn note_queue_error(&self, session: bool, message: String) {
        *self.queue_error.lock().unwrap_or_else(|e| e.into_inner()) = Some((session, message));
    }

    /// Queues a serialized delete of `session.bin` on the session worker
    /// (see [`crate::store::writer::WriteJob::discard`]): a queued or
    /// in-flight snapshot can no longer be renamed into place after the
    /// delete, which is the exact promise "restore tabs off" makes about the
    /// visited URLs.
    pub(super) fn discard_session(&self, dir: PathBuf) {
        let mut guard = self
            .session_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let job = crate::store::writer::WriteJob {
            dir,
            bytes: Vec::new(),
            discard: true,
            build: None,
        };
        if !crate::store::writer::queue_or_respawn(&mut guard, SESSION_FILE, "session", job) {
            self.note_queue_error(true, "could not queue the session delete".into());
        }
    }

    /// Drains the outcome of the finished writes: the settings worker's last
    /// error (for the Preferences warning) and whether a settings write
    /// landed since the last check (clears that warning only on a CONFIRMED
    /// success, never on enqueue), plus the SESSION worker's outcome in its
    /// own report.
    ///
    /// Both workers are drained, in separate reports: a successful per-tab
    /// session save (which happens far more often) must not clear a
    /// settings-write failure the user still needs to see, and a session
    /// save that fails on the worker (full disk, revoked directory) must not
    /// pass silently — `save_session` only sees the enqueue. A worker
    /// retired by [`Self::flush`] left its outcome in `drained`, reported
    /// here with the same semantics.
    pub(super) fn take_outcome(&self) -> WriteOutcomes {
        let mut outcomes =
            std::mem::take(&mut *self.drained.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some((session, error)) = self
            .queue_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let slot = if session {
                &mut outcomes.session_error
            } else {
                &mut outcomes.settings_error
            };
            *slot = Some(error);
        }
        if let Some(worker) = self
            .writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if let Some(error) = worker.take_error() {
                outcomes.settings_error = Some(error);
            } else if worker.take_success() {
                outcomes.settings_succeeded = true;
            }
        }
        if let Some(worker) = self
            .session_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if let Some(error) = worker.take_error() {
                outcomes.session_error = Some(error);
            } else if worker.take_success() {
                outcomes.session_succeeded = true;
            }
        }
        outcomes
    }

    /// Drains queued settings/session writes, waiting at most `timeout` in
    /// TOTAL across both workers. `true` when every queued write landed and
    /// the workers were joined.
    ///
    /// A worker that finished is retired (`None`), but its last outcome is
    /// first parked in `drained`: dropping it would silently discard a
    /// failure the Preferences warning exists to show (this runs mid-session
    /// on the "restore tabs off" path, not only at exit).
    fn flush(&self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        let mut drained = true;
        for (slot, session) in [(&self.writer, false), (&self.session_writer, true)] {
            // One deadline for the whole flush: giving each worker the full
            // timeout doubled the documented bound (`set_restore_tabs`
            // promises a brief, bounded wait).
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                return false;
            };
            let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_mut() {
                Some(worker) if worker.worker_dead() => {
                    self.park_outcome(worker, session);
                    *guard = None;
                }
                Some(worker) => {
                    drained &= worker.flush(remaining);
                    if !worker.busy() {
                        self.park_outcome(worker, session);
                        *guard = None;
                    }
                }
                None => {}
            }
        }
        drained
    }

    /// Moves a retired worker's last error / confirmed success into
    /// `drained` (reported by [`Self::take_outcome`] like a live worker's).
    fn park_outcome(&self, worker: &crate::store::writer::StoreWriter, session: bool) {
        let error = worker.take_error();
        let succeeded = error.is_none() && worker.take_success();
        let mut drained = self.drained.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = error {
            if session {
                drained.session_error = Some(error);
            } else {
                drained.settings_error = Some(error);
            }
        } else if succeeded {
            if session {
                drained.session_succeeded = true;
            } else {
                drained.settings_succeeded = true;
            }
        }
    }
}

/// One frame's drained writer outcomes (see
/// [`SettingsWrites::take_outcome`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct WriteOutcomes {
    /// Last settings-write failure (shown in Preferences).
    pub(super) settings_error: Option<String>,
    /// A settings write landed on disk (clears a previous warning).
    pub(super) settings_succeeded: bool,
    /// Last session-write failure: surfaced separately, so a settings
    /// success cannot hide it (and vice versa).
    pub(super) session_error: Option<String>,
    /// A session write landed on disk.
    pub(super) session_succeeded: bool,
}

/// Drains every queued settings/session write (bounded wait). Called by the
/// exit path and by tests that must read the file back immediately.
pub(crate) fn flush_settings_writes() {
    let _ = settings_writer().flush(std::time::Duration::from_secs(
        config::HISTORY_SAVE_FLUSH_TIMEOUT_SECS,
    ));
}

/// Replaces (or appends) one `key=value` line in `body`, dropping duplicate
/// occurrences of the key and blank lines.
fn set_key_in_body(body: &mut String, key: &str, value: &str) {
    let prefix = format!("{key}=");
    let existing = std::mem::take(body);
    let mut replaced = false;
    for line in existing.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with(prefix.as_str()) {
            // Keep the first occurrence (with the new value), drop duplicates.
            if !replaced {
                body.push_str(&prefix);
                body.push_str(value);
                body.push('\n');
                replaced = true;
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !replaced {
        body.push_str(&prefix);
        body.push_str(value);
        body.push('\n');
    }
}
