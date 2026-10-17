// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `settings::platform`: per-OS data-directory resolution and the shared
//! file helpers (atomic write with unique temp, permission tightening,
//! legacy-file cleanup, wipe-all-data, capped regular-file reads).
//!
//! Shared with `history` and `bookmarks`.

use super::*;

/// Deletes legacy plaintext / per-store-key files from the pre-unified
/// layout. Clean break: never read, just removed (best-effort).
/// - `settings.conf` (+ orphaned `.tmp.*`)
/// - `session.conf` (+ `.bak`, `.tmp.*`)
/// - `history.key` / `bookmarks.key` (+ `.tmp.*`, `.repair`)
///
/// Runs at most once per directory per process: the sweep is startup work
/// (legacy files are never written again), while its callers sit on the
/// settings-hot paths — every `read_setting` used to re-scan the directory
/// and, off Linux, spawn `ps` per crash-orphaned temp on the UI thread.
pub(crate) fn cleanup_legacy_files(dir: &Path) {
    static SWEPT: std::sync::OnceLock<Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    {
        let mut swept = SWEPT
            .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !swept.insert(dir.to_path_buf()) {
            return;
        }
    }
    let remove = |path: PathBuf| {
        let _ = std::fs::remove_file(&path);
    };
    remove(dir.join(LEGACY_SETTINGS_FILE));
    remove(dir.join(LEGACY_SESSION_FILE));
    remove(dir.join(format!("{}.bak", LEGACY_SESSION_FILE)));
    for key in [LEGACY_HISTORY_KEY, LEGACY_BOOKMARKS_KEY] {
        remove(dir.join(key));
        remove(dir.join(format!("{key}.repair")));
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&format!("{}.tmp.", LEGACY_SETTINGS_FILE))
                || name.starts_with(&format!("{}.tmp.", LEGACY_SESSION_FILE))
                || name.starts_with(&format!("{}.tmp.", LEGACY_HISTORY_KEY))
                || name.starts_with(&format!("{}.tmp.", LEGACY_BOOKMARKS_KEY))
                || name.starts_with(&format!("{SETTINGS_FILE}.tmp."))
                || name.starts_with(&format!("{SESSION_FILE}.tmp."))
                || name.starts_with("history.bin.tmp.")
                || name.starts_with("bookmarks.bin.tmp.")
                // Orphaned key-creation temps hold the raw key bytes: a crash
                // between `open` and `hard_link` (`store::crypto::link_fresh_key`)
                // would otherwise leave key material on disk forever.
                || name.starts_with(&format!("{STORE_KEY_FILE}.tmp."))
            {
                // A SECOND instance's live temp file matches the same
                // prefix: deleting it mid-write makes that instance's atomic
                // rename fail (a spurious "could not save ..."). A temp
                // whose writer process is still running is not an orphan.
                if temp_owner_is_alive(&name) {
                    continue;
                }
                remove(entry.path());
            }
        }
    }
}

/// Is the `<file>.tmp.<pid>.<seq>` suffix owned by a process still running?
///
/// The atomic-write temp name carries its creator's pid ([`crate::settings`]
/// `atomic_write`). Only a pid still present in the process table is
/// treated as live: pid reuse after a crash is possible but the sweep then
/// merely leaves one more crash-orphaned temp behind, which the next
/// startup without that pid removes. Unparsable names are treated as
/// orphans (the pre-existing behavior).
fn temp_owner_is_alive(name: &str) -> bool {
    let Some(pid) = name.rsplit('.').nth(1).and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    if pid == std::process::id() {
        // Our own temps are live while this runs: only a flush/drop may
        // remove them.
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        // `/proc/<pid>` exists exactly while the process does; no signal
        // permission games and no `unsafe`.
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No procfs: consult the process table without signalling
        // (`ps -o pid= -p <pid>` prints one line only while it exists).
        std::process::Command::new("ps")
            .args(["-o", "pid=", "-p", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output()
            .map(|out| !out.stdout.iter().all(u8::is_ascii_whitespace))
            .unwrap_or(false)
    }
}

/// Removes one encrypted store file plus its `.bak` (an unreadable-blob
/// preservation) and crash-orphaned `<name>.tmp.*` siblings (an interrupted
/// `atomic_write`; they hold the same ciphertext), best-effort: `NotFound` is
/// success. Used by [`crate::store::writer::StoreWriter`]'s discard job,
/// which serializes the delete behind any in-flight write on the same
/// worker.
pub(crate) fn remove_store_file(dir: &Path, name: &str) -> std::io::Result<()> {
    let mut first_error: Option<std::io::Error> = None;
    let mut remove = |path: std::path::PathBuf| match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            if first_error.is_none() {
                first_error = Some(e);
            }
        }
    };
    remove(dir.join(name));
    remove(dir.join(format!("{name}.bak")));
    if let Ok(entries) = std::fs::read_dir(dir) {
        let prefix = format!("{name}.tmp.");
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                remove(entry.path());
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Best-effort copy of an undecryptable store blob to `<file>.bak`.
/// - Uses `create_new` (O_EXCL): atomic, never clobbers an earlier preserved
///   copy, and closes the `exists()`-then-`rename()` TOCTOU where a planted
///   `.bak` could suppress preservation.
/// - Keeps the current file in place (a later save replaces it, the backup
///   survives): the old `rename`-away left the store missing until the next
///   save, contradicting the documented "stays in place" contract.
/// - Mode `0600` on Unix; failures are ignored (recovery is best-effort).
pub(crate) fn preserve_blob_bytes(dir: &Path, file: &str, bytes: &[u8]) {
    let bak = dir.join(format!("{file}.bak"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let Ok(mut f) = options.open(&bak) else {
        return;
    };
    use std::io::Write as _;
    let _ = f.write_all(bytes);
    let _ = f.sync_all();
}

/// Tightening of an OPENED file to owner-only on Unix: strips group/other
/// bits, never widens owner bits. A restored `0644` backup must not stay
/// world-readable until the next save; a stricter mode (e.g. `0400`) is left
/// alone.
///
/// Operates on the already-opened handle (`fchmod`), never a fresh path
/// lookup: a path swapped for a symlink between the read and a path-based
/// `chmod` could tighten an arbitrary file, and a symlink must never be
/// followed (`read_regular_file` reads the handle it already has).
///
/// `true` when the handle is user-only afterwards (already was, or was just
/// tightened): callers that must fail closed (the store key) can distinguish
/// "private" from "the filesystem refused the change".
#[cfg(unix)]
pub(crate) fn tighten_opened_perms(file: &std::fs::File) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = file.metadata() else {
        return false;
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 == 0 {
        return true;
    }
    if file
        .set_permissions(std::fs::Permissions::from_mode(mode & !0o077))
        .is_err()
    {
        return false;
    }
    // Verify on the same handle: the call "succeeding" is not proof.
    file.metadata()
        .map(|meta| meta.permissions().mode() & 0o077 == 0)
        .unwrap_or(false)
}

/// Non-Unix: POSIX modes do not exist; the per-user data directory and the
/// `create_new` creation carry the protection (mirrors the store key's
/// non-Unix policy).
#[cfg(not(unix))]
pub(crate) fn tighten_opened_perms(_file: &std::fs::File) -> bool {
    true
}
/// Best-effort removal of the whole per-user data directory
/// (history + bookmarks + settings + session + key). Used by
/// "Delete all data and close". [`suppress_writes`] is set first by the
/// caller, so no further save can recreate it; the caller re-enables writes
/// (and reports the failure) when the removal does not go through, instead
/// of exiting as if the data were gone.
pub(crate) fn wipe_all_data() -> io::Result<()> {
    let Some(dir) = default_data_dir() else {
        // No directory resolved means there is nothing this app ever wrote.
        return Ok(());
    };
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        // Nothing persisted yet: the wipe is already true.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Reads a store file without ever blocking on a FIFO swap.
///
/// `metadata` rejects non-regular files, but a local attacker can swap one
/// in between that check and `open`: opening a FIFO with no writer blocks
/// forever (no timeout can interrupt it) and would hang the calling thread
/// (the UI thread for history/bookmarks/settings loads). `O_NONBLOCK` makes
/// that open return immediately and the opened-handle recheck rejects the
/// FIFO before any byte is read — the same recipe as
/// [`crate::protocol::local`] document reads.
///
/// `max_len` bounds the allocation even if the file grows after the metadata
/// check. Any failure (missing file, non-regular, over the cap, IO error)
/// is `None`: every store treats that as "empty in-memory store".
pub(crate) fn read_regular_file(path: &Path, max_len: u64) -> Option<Vec<u8>> {
    use std::io::Read as _;
    // `symlink_metadata` inspects the directory entry itself: a store file is
    // never a symlink, and following one would let a planted link read (and
    // tighten permissions on) an arbitrary file outside the data directory.
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    if meta.len() > max_len {
        return None;
    }
    #[cfg(unix)]
    let opened = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let opened = std::fs::File::open(path).ok()?;
    let mut file = opened;
    // Re-check the OPENED handle: the path may have been swapped between
    // `symlink_metadata` and `open`; the handle is what is actually read.
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut out = Vec::with_capacity(meta.len().min(max_len) as usize);
    // 64 KiB chunks: an 8 KiB buffer made a capped 48 MiB store pay ~6 000
    // `read(2)` syscalls (and an extend per chunk) on the load path.
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
        if out.len() as u64 > max_len {
            return None;
        }
    }
    // A restored world-readable store must not stay that way until the next
    // save: tighten group/other bits best-effort on the OPENED handle (never
    // widens owner bits, never follows a symlink swapped in after the open).
    #[cfg(unix)]
    tighten_opened_perms(&file);
    Some(out)
}

/// Unique temp-file sequence for [`atomic_write`] (module scope so the
/// counter survives every call).
static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Tightens a directory to owner-only on Unix, without following a symlink
/// and without ever widening an admin-hardened mode. Best-effort: a platform
/// or filesystem that refuses the change leaves it as is.
pub(crate) fn tighten_dir_perms(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        // Open the directory itself with `O_NOFOLLOW` and `fchmod` the
        // handle: a path swapped for a symlink between check and chmod must
        // never retarget the operation.
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(dir)
        else {
            return;
        };
        let Ok(meta) = file.metadata() else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            let _ = file.set_permissions(std::fs::Permissions::from_mode(mode & !0o077));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

/// Once set by [`suppress_writes`], every store write fails instead of
/// recreating the data directory (the "Delete all data and close" teardown).
static WRITES_SUPPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Permanently refuses every later store write (`wipe_all_and_exit`).
pub(crate) fn suppress_writes() {
    WRITES_SUPPRESSED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Re-enables store writes after a wipe that did NOT go through: the user's
/// data is still on disk, so refusing every save would lose new data with
/// no visible reason. Only the failed-wipe path may call this.
pub(crate) fn allow_writes() {
    WRITES_SUPPRESSED.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Atomically writes `bytes` to `dir/name` (temp file + rename), creating
/// the directory with user-only permissions on Unix. The rename replaces the
/// previous file on every platform (no move-aside, so a crash cannot leave
/// the target missing).
///
/// The temp name is unique per process and write: two running instances (no
/// single-instance guard) sharing one fixed `.tmp` path could interleave
/// their writes byte-by-byte and leave a file that fails to parse.
///
/// Once [`suppress_writes`] was called, every later write fails instead of
/// recreating the data directory: a background worker still draining its
/// queue must not resurrect the wiped store.
pub(crate) fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::Ordering;
    if WRITES_SUPPRESSED.load(Ordering::SeqCst) {
        return Err(io::Error::other("store writes suppressed"));
    }
    std::fs::create_dir_all(dir)?;
    tighten_dir_perms(dir);
    let target = dir.join(name);
    // Any failure while creating/writing/syncing the temp file removes it:
    // a full disk (the history store retries every few seconds) must not
    // litter a partial `.tmp` per attempt.
    //
    // `create_new` (O_EXCL) instead of `create`: a symlink pre-planted at a
    // predictable temp path can never redirect the write, and a leftover
    // temp file from a crashed process (pid reuse) is skipped with a fresh
    // sequence instead of being truncated-and-followed.
    let mut tmp: Option<PathBuf> = None;
    let write_result = (|| -> io::Result<()> {
        let mut opened: Option<std::fs::File> = None;
        for _ in 0..16 {
            let candidate = dir.join(format!(
                "{name}.tmp.{}.{}",
                std::process::id(),
                TMP_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&candidate) {
                Ok(file) => {
                    tmp = Some(candidate);
                    opened = Some(file);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        let mut file = opened.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not create a unique temp file",
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // `mode` only applies at creation: a leftover `.tmp` from a
            // crash must not keep wider permissions.
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(bytes)?;
        file.sync_all()
    })();
    let Some(tmp) = tmp else {
        return write_result;
    };
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Replace the destination in ONE atomic rename. On every supported
    // platform `std::fs::rename` replaces an existing target (`rename(2)`,
    // Windows `MoveFileExW` with replace / `FileRenameInfoEx`), so a crash
    // can never leave the store missing or split between a `.bak` and a
    // temp file the way the old move-aside dance could.
    std::fs::rename(&tmp, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    // Durability: persist the directory entry too, so a power loss
    // right after a confirmed save cannot lose the rename itself.
    // Best-effort: a platform that refuses `fsync` on a directory
    // (Windows cannot open one this way) still has the (synced) contents.
    if let Ok(dir_file) = std::fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

/// Standard per-user data directory for the platform.
///
/// Every candidate base must be ABSOLUTE: a relative `HOME`/`APPDATA`
/// would resolve the data directory (and `wipe_all_data`'s recursive
/// delete) under the current working directory.
pub(crate) fn default_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return crate::util::home_dir().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("MDowser")
        });
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var_os("APPDATA").filter(|value| !value.is_empty()))
            .map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .map(|base| base.join("MDowser"));
    }
    // Linux and other Unix: XDG Base Directory specification. A relative
    // `XDG_DATA_HOME` is ignored (the spec requires an absolute path): the
    // stores (and `wipe_all_data`'s `remove_dir_all`) must never resolve
    // under the current working directory. The `HOME` fallback carries the
    // same absolute-only rule (`util::home_dir`).
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| crate::util::home_dir().map(|home| home.join(".local").join("share")))
        .map(|base| base.join("mdowser"))
}
