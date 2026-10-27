// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `store::crypto`: shared AEAD file format and key handling for the encrypted
//! local stores (`history`, `bookmarks`, `settings`, `session`).
//!
//! FORMAT: `magic (4B) | version (1B) | nonce (12B) | ciphertext + tag`,
//! ChaCha20-Poly1305 (`aws-lc-rs`, the same crypto backend rustls links)
//! with the 5 header bytes authenticated as additional data,
//! so magic/version tampering fails the open too. The magic and version are
//! per store: only a reader holding the matching constants can open a file,
//! even though all four share the single `store.key`.
//!
//! KEY: 32 random bytes (256-bit) in the single shared `store.key` file, created mode
//! `0600` on Unix; the data directory is created mode `0700`. The key lives
//! beside the data by design (the usual threat model for a local key
//! store): it protects the store against other users, backups and casual
//! inspection, NOT against malware running as the same user. Documented in
//! `docs/SECURITY.md`. A key file of the wrong size is discarded and regenerated
//! (the store it belonged to can no longer be decrypted anyway); a
//! pre-existing key with wider-than-user mode is tightened, or refused when
//! it cannot be (fail closed).

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use std::io::{self, Write};
use std::path::Path;

/// ChaCha20-Poly1305 authentication tag length (bytes).
const TAG_LEN: usize = 16;

/// Length of the `magic` prefix in bytes (store-format magic is 4 bytes).
const MAGIC_LEN: usize = 4;

/// Fixed byte overhead [`encrypt`] adds around the plaintext: the
/// `magic | version` header, the nonce and the authentication tag. Storage
/// readers cap the WHOLE blob, so they derive their file cap from the
/// plaintext budget plus this instead of hardcoding `+ 64`.
pub(crate) const OVERHEAD_BYTES: usize = MAGIC_LEN + 1 + NONCE_LEN + TAG_LEN;

/// Encrypts `plaintext` into `magic | version | nonce | ciphertext+tag`.
pub(crate) fn encrypt(
    key: &[u8; 32],
    magic: &[u8; 4],
    version: u8,
    plaintext: &[u8],
) -> Option<Vec<u8>> {
    let mut header = [0u8; 5];
    header[..magic.len()].copy_from_slice(magic);
    header[magic.len()] = version;
    let mut nonce = [0u8; NONCE_LEN];
    aws_lc_rs::rand::fill(&mut nonce).ok()?;
    let unbound = UnboundKey::new(&CHACHA20_POLY1305, key).ok()?;
    let cipher = LessSafeKey::new(unbound);
    // Build the final `header | nonce | ciphertext | tag` layout up front
    // and seal the plaintext tail IN PLACE: the plaintext is copied exactly
    // once (into `out`), instead of into a separate body buffer and then
    // again into the result.
    let body_at = header.len() + nonce.len();
    let mut out = Vec::with_capacity(body_at + plaintext.len() + TAG_LEN);
    out.extend_from_slice(&header);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(plaintext);
    let tag = cipher
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(header),
            &mut out[body_at..],
        )
        .ok()?;
    out.extend_from_slice(tag.as_ref());
    Some(out)
}

/// Decrypts a store blob produced by [`encrypt`]; `None` on any structural
/// or authentication failure (wrong key, wrong magic/version, tampering,
/// truncation).
pub(crate) fn decrypt(
    key: &[u8; 32],
    magic: &[u8; 4],
    version: u8,
    data: &[u8],
) -> Option<Vec<u8>> {
    let header_len = magic.len() + 1;
    let minimum = header_len + NONCE_LEN + TAG_LEN;
    if data.len() < minimum {
        return None;
    }
    let header = data.get(..header_len)?;
    if header.get(..magic.len())? != magic || header[magic.len()] != version {
        return None;
    }
    let nonce: [u8; NONCE_LEN] = data
        .get(header_len..header_len + NONCE_LEN)?
        .try_into()
        .ok()?;
    let mut body = data.get(header_len + NONCE_LEN..)?.to_vec();
    let unbound = UnboundKey::new(&CHACHA20_POLY1305, key).ok()?;
    let cipher = LessSafeKey::new(unbound);
    // `open_in_place` authenticates and decrypts into `body`; keep that
    // buffer as the result (truncating the tag away) instead of copying the
    // whole plaintext a second time.
    let plain_len = match cipher.open_in_place(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(header),
        &mut body,
    ) {
        Ok(plain) => plain.len(),
        Err(_) => return None,
    };
    body.truncate(plain_len);
    Some(body)
}

/// Loads the 32-byte key, creating it (mode 0600) on first use. A key of
/// the wrong size is discarded and regenerated: the store it belongs to
/// can no longer be decrypted anyway. Creation writes a fully synced
/// private temp file and links it into place with `hard_link` (atomic and
/// never replacing an existing file), so a second app instance can only
/// ever read the winner's complete key — it can never observe (or delete)
/// a half-written one. A pre-existing key with wider-than-user mode is
/// tightened on its OPENED handle (or refused, see [`KeyRead::Refused`]):
/// the key is the only thing protecting the encrypted store, so a
/// copied/restored store must not keep it readable by other local users.
pub(crate) fn load_or_create_key(path: &Path) -> Option<[u8; 32]> {
    // A non-regular file (FIFO/device/directory) is never read: it could
    // block forever on the UI thread. A missing file falls through to
    // creation below.
    if std::fs::metadata(path).is_ok_and(|meta| !meta.is_file()) {
        return None;
    }
    // Lives until the end of the function: when a corrupt key is being
    // repaired, the replacement below is installed while this guard still
    // holds the repair lock.
    let _repair_guard = match read_key_problem(path) {
        Ok(key) => return Some(key),
        // Present, 32 bytes, but wider-than-user and not tightenable: fail
        // closed WITHOUT deleting the key (the store may still be the
        // user's, and a future run may be able to tighten it).
        Err(KeyProblem::Refused) => return None,
        // A wrong-size file is not a key: discard it and regenerate. The
        // replacement is serialized across instances (and re-checked under
        // the lock).
        Err(KeyProblem::WrongSize) => {
            let guard = acquire_repair_lock(path);
            match read_key_problem(path) {
                Ok(key) => return Some(key),
                Err(KeyProblem::Refused) => return None,
                Err(_) => {
                    // Without the lock this launch must NOT delete the
                    // file: another instance may be mid-repair and that
                    // path may already hold its complete fresh key.
                    // Report the key as unavailable instead; the next
                    // launch retries the repair.
                    guard.as_ref()?;
                    let _ = std::fs::remove_file(path);
                }
            }
            guard
        }
        Err(KeyProblem::Missing) => None,
    };
    let mut key = [0u8; 32];
    aws_lc_rs::rand::fill(&mut key).ok()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
        #[cfg(unix)]
        {
            // Tighten only: never widen an admin-hardened dir.
            use std::os::unix::fs::PermissionsExt as _;
            if let Ok(meta) = std::fs::metadata(parent) {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    let _ = std::fs::set_permissions(
                        parent,
                        std::fs::Permissions::from_mode(mode & !0o077),
                    );
                }
            }
        }
    }
    match link_fresh_key(path, &key) {
        KeyLink::Won => Some(key),
        KeyLink::Lost => read_key(path),
        // No hard links on this filesystem (some FAT/network mounts): fall
        // back to `create_new` + direct write. The read-side retries cover
        // the tiny partially-written window that reintroduces.
        KeyLink::Unsupported => create_key_direct(path, &key),
    }
}

/// A wrong-size key file is repaired under this best-effort cross-process
/// lock, so two instances starting at once cannot delete each other's
/// freshly installed key. The lock is a sibling `<key>.repair` file created
/// with `create_new`; a stale one (holder crashed) is taken over after
/// `REPAIR_LOCK_STALE`, and a waiter gives up after `REPAIR_LOCK_WAIT` and
/// falls back to a plain read, so a hung holder can only make THIS launch
/// report the key as unavailable, never block it. A stale lock that cannot
/// be removed at all (immutable file, directory squatting the name) gives
/// up immediately for the same reason.
const REPAIR_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(500);
/// Age past which a held repair lock is considered abandoned (crash).
const REPAIR_LOCK_STALE: std::time::Duration = std::time::Duration::from_secs(5);
/// Poll interval while waiting for the repair lock.
const REPAIR_LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// RAII guard for the repair lock: removes the lock file on drop (a
/// leftover is harmless — [`acquire_repair_lock`] takes over stale locks).
#[derive(Debug)]
struct RepairLock {
    path: std::path::PathBuf,
}

impl Drop for RepairLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Best-effort cross-process mutex around corrupt-key replacement;
/// `None` when the caller should proceed WITHOUT the lock (timeout/open
/// failure: a plain re-read decides, never a block).
fn acquire_repair_lock(key_path: &Path) -> Option<RepairLock> {
    let name = key_path.file_name().map_or_else(
        || "store.key".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let lock_path = key_path.with_file_name(format!("{name}.repair"));
    let start = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_file) => return Some(RepairLock { path: lock_path }),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Stale (holder crashed mid-repair): take it over.
                let stale = std::fs::metadata(&lock_path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > REPAIR_LOCK_STALE);
                if stale {
                    // A failed removal must NOT loop back into the same
                    // stale observation: `remove_file` on an undeletable
                    // path (directory, immutable flag, permission quirk)
                    // would spin hot forever on the UI thread. Give up
                    // without the lock instead (best-effort by contract).
                    if std::fs::remove_file(&lock_path).is_err() {
                        return None;
                    }
                    continue;
                }
                if start.elapsed() >= REPAIR_LOCK_WAIT {
                    return None;
                }
                std::thread::sleep(REPAIR_LOCK_POLL);
            }
            Err(_) => return None,
        }
    }
}

/// Outcome of the atomic `hard_link` key installation.
enum KeyLink {
    /// This call installed the key.
    Won,
    /// Another writer won; its (complete) key is at `path`.
    Lost,
    /// The filesystem rejected hard links; use the direct-write fallback.
    Unsupported,
}

/// Writes the key to a unique private sibling and hard-links it into place.
/// The link is atomic and never replaces an existing file, so concurrent
/// creators deterministically get `Won`/`Lost` with a complete key either
/// way. The temp file is removed on every path.
fn link_fresh_key(path: &Path, key: &[u8; 32]) -> KeyLink {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map_or_else(
        || "store.key".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let tmp = path.with_file_name(format!(
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
    let written = (|| -> Option<()> {
        let mut file = options.open(&tmp).ok()?;
        file.write_all(key).ok()?;
        file.sync_all().ok()?;
        Some(())
    })();
    if written.is_none() {
        let _ = std::fs::remove_file(&tmp);
        // Distinguish "temp could not be created" from "link unsupported":
        // without a temp file there is nothing to link, so go direct.
        return KeyLink::Unsupported;
    }
    let link = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match link {
        Ok(()) => KeyLink::Won,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => KeyLink::Lost,
        Err(_) => KeyLink::Unsupported,
    }
}

/// Fallback key creation on filesystems without hard links: `create_new`
/// plus a direct write. `create_new` keeps two creators from clobbering
/// each other; the loser reads the winner's file (retried by [`read_key`]
/// until the 32 bytes are there). The value returned is always the key
/// READ BACK from `path`, never the buffer just written: a concurrent
/// launcher can remove a still-partial file through the wrong-size repair
/// path and install its own key, leaving the written buffer attached to an
/// unlinked inode.
fn create_key_direct(path: &Path, key: &[u8; 32]) -> Option<[u8; 32]> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            if file.write_all(key).is_err() {
                let _ = std::fs::remove_file(path);
                return None;
            }
            let _ = file.sync_all();
            drop(file);
            read_key(path)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read_key(path),
        Err(_) => None,
    }
}

/// Why a key file could not be used.
enum KeyProblem {
    /// No file at the path (or it stayed unreadable after the retry window).
    Missing,
    /// The file exists but never held 32 bytes (after a short retry window).
    WrongSize,
    /// 32 bytes, but wider-than-user permissions that could not be
    /// tightened: refused fail-closed, never deleted.
    Refused,
}

/// Best-effort read for callers that only need "the key or nothing".
fn read_key(path: &Path) -> Option<[u8; 32]> {
    read_key_problem(path).ok()
}

/// Reads a 32-byte private key, retrying briefly while a concurrent
/// creator may still be filling the file (only possible on the direct
/// fallback path; the `hard_link` path never exposes a partial key).
fn read_key_problem(path: &Path) -> Result<[u8; 32], KeyProblem> {
    for attempt in 0..4u32 {
        // A key is exactly 32 bytes. Never `read` a larger file whole (a
        // restored backup, a symlink to some big file, user error): open
        // with `O_NONBLOCK` and re-check the OPENED handle, then read at
        // most 33 bytes. The path may be swapped between the first metadata
        // probe and the open, so only the handle is trusted; the streaming
        // read enforces the cap even if the file grows afterwards. A FIFO
        // swapped in cannot block the caller, and a symlinked key is
        // refused (never followed, never tightened).
        match read_key_bytes(path) {
            KeyRead::Bytes(bytes) => {
                if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
                    return Ok(key);
                }
                if attempt < 3 {
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    continue;
                }
                return Err(KeyProblem::WrongSize);
            }
            // Present, but not (and not made) user-only: fail closed. The
            // key may still be the user's, so it is never deleted.
            KeyRead::Refused => return Err(KeyProblem::Refused),
            KeyRead::None if attempt < 3 => {
                // Missing, non-regular, over the cap, or a transient IO
                // error: a concurrent creator may still be filling the file
                // (direct fallback path), so retry briefly before deciding
                // between Missing and WrongSize below.
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            KeyRead::None => {
                // Distinguish "no key" from "a file that can never be a key":
                // an oversized regular file is WrongSize (regenerate path),
                // anything else is Missing.
                if std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 32) {
                    return Err(KeyProblem::WrongSize);
                }
                return Err(KeyProblem::Missing);
            }
        }
    }
    Err(KeyProblem::Missing)
}

/// Outcome of one [`read_key_bytes`] attempt.
enum KeyRead {
    /// 32-ish bytes read from an OPENED, user-only regular file.
    Bytes(Vec<u8>),
    /// The file exists but is (or would remain) readable by other local
    /// users, or is a symlink: refused fail-closed, never deleted.
    Refused,
    /// Missing, non-regular, over the cap, or an IO error.
    None,
}

/// Reads at most 33 bytes from `path` through an `O_NONBLOCK` open whose
/// handle is re-checked: only regular files are read, a FIFO swapped in
/// cannot block, and a file grown after the check cannot force a large
/// allocation. On Unix the OPENED handle is also tightened to owner-only
/// (`fchmod`, never a path-based `chmod`): a symlink swapped in after the
/// permission check can neither be followed (`O_NOFOLLOW` refuses it
/// outright) nor made the target of a chmod the process did not intend.
/// The 33rd byte exists only to detect oversize without allocating it.
fn read_key_bytes(path: &Path) -> KeyRead {
    use std::io::Read as _;
    if std::fs::metadata(path).is_ok_and(|meta| !meta.is_file() || meta.len() > 32) {
        return KeyRead::None;
    }
    #[cfg(unix)]
    let opened = {
        use std::os::unix::fs::OpenOptionsExt as _;
        match std::fs::OpenOptions::new()
            .read(true)
            // `O_NOFOLLOW`: a symlink planted between the metadata check
            // and the open is refused instead of read (and never chmod'ed
            // through).
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            // A symlinked key is a refusal (fail closed), not "missing":
            // regenerating over it would silently discard the store.
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return KeyRead::Refused,
            Err(_) => return KeyRead::None,
        }
    };
    #[cfg(not(unix))]
    let opened = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return KeyRead::None,
    };
    let mut file = opened;
    match file.metadata() {
        Ok(meta) if meta.is_file() => {}
        _ => return KeyRead::None,
    }
    // The key is the only thing protecting the encrypted store: a restored
    // or copied key with group/other bits is tightened ON THE HANDLE; when
    // that cannot be done the key is refused (fail closed), never trusted.
    if !crate::settings::tighten_opened_perms(&file) {
        return KeyRead::Refused;
    }
    let mut out = Vec::with_capacity(32);
    let mut chunk = [0u8; 33];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return KeyRead::None,
        };
        out.extend_from_slice(&chunk[..n]);
        if out.len() > 32 {
            return KeyRead::None;
        }
    }
    KeyRead::Bytes(out)
}

#[cfg(test)]
mod tests;
