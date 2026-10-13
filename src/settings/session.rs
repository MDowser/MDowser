// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `settings::session`: encrypted open-tabs memory ("restore tabs on
//! startup").
//!
//! `save_session*` write `session.bin` (magic `MDSS`) only while the
//! preference is enabled; `load_session*` reopen the stored tabs and active
//! index. `delete_session_from` removes the file (and its `.tmp.*`/`.bak`
//! siblings) when the preference is turned off.

use super::*;

/// Saves the open tabs through the background writer. `Err` when the
/// snapshot could not even be built/encrypted (no data directory, key
/// unavailable) so the caller keeps its "needs save" state and surfaces the
/// failure instead of marking the snapshot as stored.
pub(crate) fn save_session(tabs: &[SessionTab], active: usize) -> io::Result<()> {
    let Some(dir) = default_data_dir() else {
        return Err(io::Error::other("no per-user data directory is available"));
    };
    save_session_async(&dir, tabs, active)
}

/// [`save_session_to`] through the background writer. `Err` when the blob
/// could not be built/encrypted OR when the job could not be queued: the
/// caller must keep its "needs save" state, so a full queue is never
/// mistaken for a stored snapshot.
pub(crate) fn save_session_async(dir: &Path, tabs: &[SessionTab], active: usize) -> io::Result<()> {
    let blob = session_blob(dir, tabs, active)?;
    if !settings_writer().enqueue(dir.to_path_buf(), SESSION_FILE, blob) {
        return Err(io::Error::other("could not queue the session save"));
    }
    Ok(())
}

/// Writes `session.bin` atomically on the CALLER thread, encrypted with the
/// shared store key (test-only: the app path is [`save_session_async`], so
/// the UI thread never `fsync`s).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn save_session_to(dir: &Path, tabs: &[SessionTab], active: usize) -> io::Result<()> {
    let blob = session_blob(dir, tabs, active)?;
    atomic_write(dir, SESSION_FILE, &blob)
}

/// Builds the encrypted session blob (shared by both writers).
fn session_blob(dir: &Path, tabs: &[SessionTab], active: usize) -> io::Result<Vec<u8>> {
    // No legacy sweep here: this runs on every tab change, while
    // `load_session_from` (once per start) already removed the legacy files.
    let mut body = format!("{SESSION_MAGIC} {SESSION_VERSION}\nactive={active}\n");
    for tab in tabs.iter().take(config::MAX_TABS) {
        match tab {
            // A `Home` tab has no URL: a bare `-` keeps the tab COUNT (and
            // therefore the active index) stable across a restore.
            None => body.push_str("-\n"),
            Some(url) => {
                // Fail closed per tab: an over-long URL is stored as Home
                // rather than written unbounded (the in-memory tabs should
                // already be capped, but the writer must not trust that).
                if url.len() <= MAX_SESSION_URL_BYTES {
                    body.push_str(url);
                } else {
                    body.push('-');
                }
                body.push('\n');
            }
        }
    }
    let key = super::settings_writer()
        .store_key(dir)
        .ok_or_else(|| io::Error::other("store key unavailable"))?;
    encrypt(
        &key,
        SESSION_MAGIC_BIN,
        SESSION_VERSION_BIN,
        body.as_bytes(),
    )
    .ok_or_else(|| io::Error::other("session encryption failed"))
}

/// Loads the remembered session, when a valid one exists.
pub(crate) fn load_session() -> Option<(Vec<SessionTab>, usize)> {
    load_session_from(default_data_dir()?.as_path())
}

/// Reads `session.bin` from an explicit directory (tests + [`load_session`]).
/// Legacy plaintext `session.conf` is never read (clean break).
pub(crate) fn load_session_from(dir: &Path) -> Option<(Vec<SessionTab>, usize)> {
    cleanup_legacy_files(dir);
    let plain = read_encrypted_file(
        dir,
        SESSION_FILE,
        SESSION_MAGIC_BIN,
        SESSION_VERSION_BIN,
        MAX_SESSION_BYTES,
    )?;
    let text = std::str::from_utf8(&plain).ok()?;
    let mut lines = text.lines();
    // Header: `<magic> <version>`, both required. A version this build does
    // not know is ignored (never parsed with the wrong line semantics).
    let mut header = lines.next()?.split_whitespace();
    if header.next() != Some(SESSION_MAGIC) {
        return None;
    }
    match header.next().map(str::parse::<u8>) {
        Some(Ok(version)) if version == SESSION_VERSION => {}
        _ => return None,
    }
    let mut active = 0usize;
    let mut tabs: Vec<SessionTab> = Vec::new();
    for line in lines.take(config::MAX_TABS + 1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("active=") {
            active = value.trim().parse().unwrap_or(0);
            continue;
        }
        if line == "-" {
            tabs.push(None);
            continue;
        }
        // Only URLs this browser could ever have opened (https pages, local
        // files and the internal `about:` pages) and within the serialized
        // cap; anything else is a corrupt line and is skipped. A remote
        // `file://host/` share, any URL with credentials or a `file:` URL
        // with a port, and any other `about:` string are rejected (mirrors
        // `protocol::local::parse_file_url`). Credentials are rejected on
        // EVERY scheme: a tampered session file must not seed a
        // credential-bearing URL into the restore path.
        let accepted = line.len() <= MAX_SESSION_URL_BYTES
            && url::Url::parse(line)
                .ok()
                .is_some_and(|url| match url.scheme() {
                    "https" => url.username().is_empty() && url.password().is_none(),
                    "file" => {
                        crate::protocol::local::is_local_host(&url)
                            && url.username().is_empty()
                            && url.password().is_none()
                            && url.port().is_none()
                    }
                    _ => crate::config::INTERNAL_PAGES
                        .iter()
                        .any(|page| url.as_str().eq_ignore_ascii_case(page)),
                });
        if accepted {
            tabs.push(Some(line.to_string()));
        }
        if tabs.len() >= config::MAX_TABS {
            break;
        }
    }
    if tabs.is_empty() {
        return None;
    }
    let active = active.min(tabs.len() - 1);
    Some((tabs, active))
}

/// Removes `session.bin` (plus the `.bak` and `.tmp.*` siblings a crash
/// can leave: all of them hold visited URLs) from `dir`. A missing file is
/// a success; any other failure is returned so the UI can warn instead of
/// leaving URLs behind silently.
pub(super) fn delete_session_from(dir: Option<&Path>) -> io::Result<()> {
    let Some(dir) = dir else {
        return Ok(());
    };
    let mut first_error: Option<io::Error> = None;
    let mut remove = |path: PathBuf| match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            if first_error.is_none() {
                first_error = Some(e);
            }
        }
    };
    remove(dir.join(SESSION_FILE));
    remove(dir.join(format!("{SESSION_FILE}.bak")));
    // Orphaned temp files from an interrupted `atomic_write` (crash, full
    // disk) hold the same ciphertext: remove every one this module could
    // ever have created.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&format!("{SESSION_FILE}.tmp.")) {
                remove(entry.path());
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
