// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    format_age, HistoryStore, Retention, Visit, FILE_MAGIC, FILE_VERSION, HISTORY_FILE, KEY_FILE,
};
use crate::settings::SETTINGS_FILE;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Fresh, unique scratch directory per test.
fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdowser-history-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retention_settings_roundtrip_and_reject_garbage() {
    for retention in [
        Retention::Session,
        Retention::Days(1),
        Retention::Days(90),
        Retention::Forever,
    ] {
        assert_eq!(
            Retention::from_setting(&retention.as_setting()),
            Some(retention)
        );
    }
    assert_eq!(Retention::from_setting(" 90 "), Some(Retention::Days(90)));
    assert_eq!(Retention::from_setting("0"), None);
    assert_eq!(Retention::from_setting("-5"), None);
    assert_eq!(Retention::from_setting("999999"), None);
    assert_eq!(Retention::from_setting("nope"), None);
    assert_eq!(Retention::default(), Retention::Days(90));
}

#[test]
fn encrypted_roundtrip_keeps_entries_and_hides_plaintext() {
    let dir = tmp_dir("roundtrip");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", Some("Page A"));
    store.record("https://ex.com/b/", None);
    // Force the write (the normal path is debounced).
    store.dirty = true;
    store.save();

    let blob = std::fs::read(dir.join(HISTORY_FILE)).expect("history file");
    assert_eq!(&blob[..4], FILE_MAGIC);
    assert_eq!(blob[4], FILE_VERSION);
    assert!(
        !blob.windows(8).any(|w| w == b"https://"),
        "ciphertext must not leak URLs"
    );
    assert!(
        !blob.windows(6).any(|w| w == b"Page A"),
        "ciphertext must not leak titles"
    );

    let reloaded = HistoryStore::load_from(dir.clone());
    assert_eq!(reloaded.entries().len(), 2);
    assert_eq!(reloaded.entries()[0].url, "https://ex.com/a/");
    assert_eq!(reloaded.entries()[0].title.as_deref(), Some("Page A"));
    assert_eq!(reloaded.entries()[1].url, "https://ex.com/b/");
    cleanup(&dir);
}

#[test]
fn tampered_or_truncated_store_loads_empty_without_panicking() {
    let dir = tmp_dir("tamper");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();

    // Flip one ciphertext byte: authentication must fail closed.
    let path = dir.join(HISTORY_FILE);
    let mut blob = std::fs::read(&path).unwrap();
    let last = blob.len() - 1;
    blob[last] ^= 0x01;
    std::fs::write(&path, &blob).unwrap();
    assert!(HistoryStore::load_from(dir.clone()).entries().is_empty());

    // Truncated file: also empty, never a panic.
    std::fs::write(&path, &blob[..blob.len() / 2]).unwrap();
    assert!(HistoryStore::load_from(dir.clone()).entries().is_empty());

    // Bogus magic/version is rejected before decryption.
    std::fs::write(&path, b"XXXX\x09short").unwrap();
    assert!(HistoryStore::load_from(dir.clone()).entries().is_empty());
    cleanup(&dir);
}

#[test]
fn corrupt_key_is_regenerated_and_history_starts_fresh() {
    let dir = tmp_dir("badkey");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    std::fs::write(dir.join(KEY_FILE), b"short").unwrap();
    assert!(HistoryStore::load_from(dir.clone()).entries().is_empty());
    // The next save works again with a fresh key.
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/b/", None);
    store.dirty = true;
    store.save();
    assert!(store.save_error().is_none());
    assert_eq!(
        std::fs::read(dir.join(KEY_FILE)).unwrap().len(),
        32,
        "key regenerated"
    );
    cleanup(&dir);
}

/// A key copied in with wider permissions is tightened before use: the
/// key is the only thing protecting the encrypted store.
#[cfg(unix)]
#[test]
fn wider_key_permissions_are_tightened_on_load() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("keymode");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    let key_path = dir.join(KEY_FILE);
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let reloaded = HistoryStore::load_from(dir.clone());
    assert_eq!(reloaded.entries().len(), 1, "history still decrypts");
    let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "key tightened on load");
    cleanup(&dir);
}

/// A failed background write must surface in Preferences AND leave the
/// store dirty so the next tick retries; a later success clears the
/// warning (the old code dropped `dirty` at enqueue time, so the visit
/// was silently lost).
#[test]
fn failed_background_write_is_retried_and_the_warning_clears() {
    let dir = tmp_dir("save-retry");
    let mut store = HistoryStore::load_from(dir.clone());
    // Block the destination with a directory: `rename` onto it fails
    // on every platform.
    std::fs::create_dir_all(dir.join(HISTORY_FILE)).unwrap();
    store.record("https://ex.com/retry/", None);
    let expedite = |s: &mut HistoryStore| {
        s.last_save =
            Instant::now() - Duration::from_secs(crate::config::HISTORY_SAVE_DEBOUNCE_SECS + 1);
    };
    expedite(&mut store);
    let _ = store.tick();
    // Do NOT expedite while waiting: `tick` must surface the failure and
    // leave it visible during the debounce window.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let _ = store.tick();
        if store.save_error().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "write failure must surface");
    }
    assert!(store.pending_save(), "failed write stays pending for retry");
    // Unblock and force the retry; the warning must clear once it lands.
    std::fs::remove_dir_all(dir.join(HISTORY_FILE)).unwrap();
    expedite(&mut store);
    let _ = store.tick();
    let deadline = Instant::now() + Duration::from_secs(5);
    while store
        .writer
        .as_ref()
        .is_some_and(crate::store::writer::StoreWriter::busy)
    {
        assert!(Instant::now() < deadline, "retry must finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = store.tick();
    assert!(store.save_error().is_none(), "warning cleared");
    assert!(!store.pending_save(), "store is clean again");
    assert!(dir.join(HISTORY_FILE).is_file());
    assert_eq!(
        HistoryStore::load_from(dir.clone()).entries().len(),
        1,
        "visit persisted by the retry"
    );
    cleanup(&dir);
}

#[test]
fn consecutive_duplicate_updates_instead_of_duplicating() {
    let dir = tmp_dir("dedup");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", Some("First"));
    store.record("https://ex.com/a/", Some("Updated"));
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries()[0].title.as_deref(), Some("Updated"));
    store.record("https://ex.com/b/", None);
    store.record("https://ex.com/a/", None);
    assert_eq!(store.entries().len(), 3, "non-consecutive visits stay");
    cleanup(&dir);
}

#[test]
fn non_https_and_empty_input_is_never_recorded() {
    let dir = tmp_dir("input");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("http://ex.com/x", None);
    store.record("javascript:alert(1)", None);
    store.record("file://server/share/x.md", None);
    store.record("   ", None);
    assert!(store.entries().is_empty());
    cleanup(&dir);
}

/// Local files are recorded (the user can reopen them from History),
/// while a remote file share never is.
#[test]
fn local_files_are_recorded() {
    let dir = tmp_dir("local");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("file:///tmp/notes.md", Some("Notes"));
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries()[0].url, "file:///tmp/notes.md");
    assert_eq!(store.entries()[0].title.as_deref(), Some("Notes"));
    cleanup(&dir);
}

/// A URL past the BYTE cap is skipped, never truncated into a
/// different page (`...` used to be stored, so the row 404'd). The cap is
/// byte-based because serialized URLs are percent-encoded: a valid
/// under-cap address whose encoding outgrew its char count used to be
/// silently dropped.
#[test]
fn oversized_urls_are_skipped_not_truncated() {
    let dir = tmp_dir("longurl");
    let mut store = HistoryStore::load_from(dir.clone());
    let over = format!(
        "https://ex.com/{}",
        "a".repeat(crate::config::MAX_URL_BYTES)
    );
    store.record(&over, None);
    assert!(store.entries().is_empty(), "over-cap URL skipped");
    // Exactly at the byte cap: stored whole.
    let prefix = "https://ex.com/";
    let at_cap = format!(
        "{prefix}{}",
        "a".repeat(crate::config::MAX_URL_BYTES - prefix.len())
    );
    assert_eq!(at_cap.len(), crate::config::MAX_URL_BYTES);
    store.record(&at_cap, None);
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries()[0].url, at_cap, "stored whole");
    cleanup(&dir);
}

#[test]
fn retention_prunes_old_visits() {
    let dir = tmp_dir("prune");
    let mut store = HistoryStore::load_from(dir.clone());
    let now = 1_800_000_000u64;
    store.entries.push(Visit {
        url: "https://old.example/".to_string(),
        title: None,
        visited_at: now - 200 * 86_400,
    });
    store.entries.push(Visit {
        url: "https://new.example/".to_string(),
        title: None,
        visited_at: now - 10 * 86_400,
    });
    store.retention = Retention::Days(90);
    store.prune(now);
    assert_eq!(store.entries.len(), 1);
    assert_eq!(store.entries[0].url, "https://new.example/");

    // Forever keeps both windows untouched.
    store.retention = Retention::Forever;
    store.entries.push(Visit {
        url: "https://ancient.example/".to_string(),
        title: None,
        visited_at: 0,
    });
    store.prune(now);
    assert_eq!(store.entries.len(), 2);
    cleanup(&dir);
}

#[test]
fn entry_cap_keeps_the_newest() {
    let dir = tmp_dir("cap");
    let mut store = HistoryStore::load_from(dir.clone());
    let cap = crate::config::MAX_HISTORY_STORE_ENTRIES;
    for i in 0..(cap + 10) {
        store.entries.push(Visit {
            url: format!("https://ex.com/{i}/"),
            title: None,
            visited_at: i as u64 + 1,
        });
    }
    store.trim();
    assert_eq!(store.entries.len(), cap);
    assert_eq!(
        store.entries.last().unwrap().url,
        format!("https://ex.com/{}/", cap + 9)
    );
    assert_eq!(store.entries[0].url, "https://ex.com/10/");
    cleanup(&dir);
}

#[test]
fn byte_cap_drops_only_the_oldest() {
    let dir = tmp_dir("bytes");
    let mut store = HistoryStore::load_from(dir.clone());
    // Bigger than the whole plaintext budget on its own.
    let big =
        "https://ex.com/".to_string() + &"a".repeat(crate::config::MAX_HISTORY_STORE_BYTES + 1024);
    store.entries.push(Visit {
        url: big.clone(),
        title: None,
        visited_at: 2,
    });
    store.entries.push(Visit {
        url: "https://new.example/".to_string(),
        title: None,
        visited_at: 1,
    });
    let before = store.entries.len();
    // The test pokes `entries` directly (a URL past the input cap cannot go
    // through `record`), so the derived byte total must be refreshed the way
    // `load_entries` does before trim can trust its O(1) fast path.
    store.recompute_plain_bytes();
    store.trim();
    // The giant entry cannot fit the plaintext budget; the newest stays.
    assert!(store.entries.len() < before);
    assert_eq!(store.entries.last().unwrap().url, "https://new.example/");
    cleanup(&dir);
}

#[test]
fn session_mode_never_reads_nor_writes() {
    let dir = tmp_dir("session");
    // A normal run leaves history behind...
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://persisted.example/", Some("Old"));
    store.dirty = true;
    store.save();
    assert!(dir.join(HISTORY_FILE).exists());

    // ...a session-mode run starts empty and never persists.
    // Switching to forget-on-exit persists only the preference and
    // wipes the encrypted leftovers immediately (the shared key stays:
    // bookmarks/settings/session still need it).
    let mut chooser = HistoryStore::load_from(dir.clone());
    chooser.set_retention(Retention::Session);
    // The preference write is queued on the background settings worker.
    crate::settings::flush_settings_writes();
    assert!(
        !dir.join(HISTORY_FILE).exists(),
        "switch wipes the store at once"
    );
    assert!(dir.join(KEY_FILE).exists(), "shared key is kept");

    // The next run starts empty and wipes whatever was left behind.
    let mut session = HistoryStore::load_from(dir.clone());
    assert_eq!(session.retention(), Retention::Session);
    assert!(session.entries().is_empty(), "nothing is read");
    assert!(!dir.join(HISTORY_FILE).exists(), "leftovers wiped on load");

    // It stays usable in memory and never writes.
    session.record("https://private.example/", Some("Secret"));
    assert_eq!(session.entries().len(), 1, "usable during the session");
    session.dirty = true;
    session.save();
    assert!(session.save_error().is_none());
    assert!(
        !dir.join(HISTORY_FILE).exists(),
        "session data never reaches disk"
    );

    // Exit wipes (nothing left) and clears memory.
    session.on_exit();
    assert!(!dir.join(HISTORY_FILE).exists());
    assert!(session.entries().is_empty());
    // The retention preference itself survives (settings are not history).
    assert!(dir.join(SETTINGS_FILE).exists());
    assert_eq!(
        HistoryStore::load_from(dir.clone()).retention(),
        Retention::Session
    );
    cleanup(&dir);
}

#[test]
fn clear_removes_memory_and_file_but_keeps_shared_key() {
    let dir = tmp_dir("clear");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    store.set_retention(Retention::Days(30));
    // Preference writes are queued on the background settings worker.
    crate::settings::flush_settings_writes();
    assert!(dir.join(HISTORY_FILE).exists());
    assert!(dir.join(SETTINGS_FILE).exists());
    store.clear();
    assert!(store.entries().is_empty());
    assert!(!dir.join(HISTORY_FILE).exists());
    assert!(dir.join(KEY_FILE).exists(), "shared key is kept");
    // The retention preference survives a history wipe.
    assert!(dir.join(SETTINGS_FILE).exists());
    cleanup(&dir);
}

/// Exit must complete an armed wipe: `clear` defers deletion while the
/// background writer is busy, and `on_exit` used to flush/save without
/// ever deleting, so the "deleted" ciphertext survived and
/// the visits reappeared on the next launch. The shared key stays.
#[test]
fn exit_completes_a_pending_wipe() {
    let dir = tmp_dir("clear-exit");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    store.set_retention(Retention::Days(30));
    assert!(dir.join(HISTORY_FILE).exists());
    assert!(dir.join(KEY_FILE).exists());
    // Simulate the deferred arm exactly as `clear` leaves it when the
    // worker was busy (the disk state is what matters here).
    store.entries.clear();
    store.dirty = false;
    store.key = None;
    store.wipe_pending = true;
    store.on_exit();
    assert!(!dir.join(HISTORY_FILE).exists(), "ciphertext must be gone");
    assert!(dir.join(KEY_FILE).exists(), "shared key is kept");
    // A fresh store starts empty instead of resurrecting the visit.
    assert!(HistoryStore::load_from(dir.clone()).entries().is_empty());
    cleanup(&dir);
}

/// "Delete history now" must never report success while the ciphertext
/// survives: a failed `remove_file` surfaces through `save_error` (seen
/// in Preferences) instead of an empty, healthy-looking list.
#[test]
fn clear_reports_delete_failures_instead_of_lying() {
    let dir = tmp_dir("clear-fail");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    // Make the history path unremovable: a non-empty directory.
    let blocked = dir.join(HISTORY_FILE);
    std::fs::create_dir_all(&blocked).expect("mkdir");
    std::fs::write(blocked.join("blocker"), b"x").expect("write");
    store.clear();
    assert!(store.entries().is_empty());
    let error = store.save_error().unwrap_or_default().to_string();
    assert!(
        error.contains("could not delete"),
        "delete failure must surface: {error:?}"
    );
    // Cleanup: the store dir now holds a directory where the file was.
    let _ = std::fs::remove_file(blocked.join("blocker"));
    let _ = std::fs::remove_dir(&blocked);
    cleanup(&dir);
}

#[test]
fn retention_change_persists_and_prunes() {
    let dir = tmp_dir("retention");
    let mut store = HistoryStore::load_from(dir.clone());
    let now = now_for_test();
    store.entries.push(Visit {
        url: "https://old.example/".to_string(),
        title: None,
        visited_at: now - 60 * 86_400,
    });
    store.entries.push(Visit {
        url: "https://fresh.example/".to_string(),
        title: None,
        visited_at: now,
    });
    store.set_retention(Retention::Days(30));
    // Preference writes are queued on the background settings worker.
    crate::settings::flush_settings_writes();
    assert_eq!(store.entries().len(), 1, "60-day visit pruned to 30 days");
    // Retention changes mark the store dirty; persist like `tick` would.
    store.save();
    let reloaded = HistoryStore::load_from(dir.clone());
    assert_eq!(reloaded.retention(), Retention::Days(30));
    assert_eq!(reloaded.entries().len(), 1);
    cleanup(&dir);
}

#[test]
fn remove_entry_by_index() {
    let dir = tmp_dir("remove");
    let mut store = HistoryStore::load_from(dir.clone());
    store.entries.push(Visit {
        url: "https://a.example/".to_string(),
        title: None,
        visited_at: 1,
    });
    store.entries.push(Visit {
        url: "https://b.example/".to_string(),
        title: None,
        visited_at: 2,
    });
    store.remove_entry(0);
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries[0].url, "https://b.example/");
    store.remove_entry(9); // out of bounds: no-op, no panic.
    assert_eq!(store.entries().len(), 1);
    cleanup(&dir);
}

/// The generation counter must advance on EVERY entry mutation: the
/// History window caches its filtered rows on it, so a missed bump would
/// show a stale list (removed rows still there, new visits missing).
#[test]
fn generation_revisions_every_entry_mutation() {
    let dir = tmp_dir("generation");
    let mut store = HistoryStore::load_from(dir.clone());
    let mut last = store.generation();
    store.record("https://ex.com/a/", None);
    assert!(store.generation() > last, "new visit");
    last = store.generation();
    // Consecutive duplicate: updated in place, still a change.
    store.record("https://ex.com/a/", Some("Updated"));
    assert!(store.generation() > last, "updated visit");
    last = store.generation();
    // Retention pruning inside `record` drops the ancient entry.
    store.entries.push(Visit {
        url: "https://ancient.example/".to_string(),
        title: None,
        visited_at: 0,
    });
    store.retention = Retention::Days(1);
    store.record("https://ex.com/b/", None);
    assert_eq!(store.entries().len(), 2, "ancient entry pruned");
    assert!(store.generation() > last, "pruned");
    last = store.generation();
    store.remove_entry(0);
    assert!(store.generation() > last, "removed");
    last = store.generation();
    store.clear();
    assert!(store.generation() > last, "cleared");
    cleanup(&dir);
}

#[test]
fn timed_out_clock_never_panics_formatting() {
    assert_eq!(format_age(100, 200), "just now");
    assert_eq!(format_age(1_000_000, 999_999), "just now");
    assert_eq!(format_age(200, 100), "1 min ago");
    assert_eq!(format_age(90_000, 0), "yesterday");
}

#[test]
fn format_age_covers_every_bucket() {
    let now = 2_000_000_000u64;
    assert_eq!(format_age(now, now), "just now");
    assert_eq!(format_age(now, now - 120), "2 min ago");
    assert_eq!(format_age(now, now - 7_200), "2 h ago");
    assert_eq!(format_age(now, now - 86_400), "yesterday");
    assert_eq!(format_age(now, now - 3 * 86_400), "3 days ago");
    assert_eq!(format_age(now, now - 10 * 86_400), "1 week ago");
    assert_eq!(format_age(now, now - 20 * 86_400), "2 weeks ago");
    assert_eq!(format_age(now, now - 60 * 86_400), "2 months ago");
    assert_eq!(format_age(now, now - 400 * 86_400), "1 year ago");
    assert_eq!(format_age(now, now - 900 * 86_400), "2 years ago");
}

#[test]
fn on_exit_flushes_pending_visits() {
    let dir = tmp_dir("exit");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", Some("A"));
    assert!(store.pending_save(), "debounced write still pending");
    store.on_exit();
    assert!(!store.pending_save());
    assert!(dir.join(HISTORY_FILE).exists());
    let reloaded = HistoryStore::load_from(dir.clone());
    assert_eq!(reloaded.entries().len(), 1);
    assert_eq!(reloaded.entries()[0].title.as_deref(), Some("A"));
    cleanup(&dir);
}

#[test]
fn tick_reports_pending_until_the_debounce_elapses() {
    let dir = tmp_dir("tick");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    // The debounce window is still open: the caller must repaint later.
    assert!(store.pending_save());
    assert!(store.tick());
    // Force the anchor into the past: the tick QUEUES the write on the
    // background worker and keeps reporting pending until it lands.
    store.last_save =
        Instant::now() - Duration::from_secs(crate::config::HISTORY_SAVE_DEBOUNCE_SECS + 1);
    assert!(store.tick());
    assert!(!store.pending_save());
    // Drain the worker (bounded wait: this is one tiny local file).
    for _ in 0..200 {
        if !store.tick() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!store.tick(), "background write settled");
    assert!(dir.join(HISTORY_FILE).exists());
    cleanup(&dir);
}

#[test]
fn in_memory_store_never_touches_disk() {
    let mut store = HistoryStore::in_memory();
    assert!(store.storage_dir().is_none());
    store.record("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    assert!(store.save_error().is_none());
    assert_eq!(store.entries().len(), 1);
    store.on_exit();
    // Forever/Days would try to save; in-memory stays healthy either way.
    assert_eq!(store.entries().len(), 1);
}

#[cfg(unix)]
#[test]
fn files_and_directory_are_user_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("perms");
    let mut store = HistoryStore::load_from(dir.clone());
    store.record("https://ex.com/a/", None);
    store.set_retention(Retention::Days(30));
    // Preference writes are queued on the background settings worker:
    // flush before asserting the files on disk.
    crate::settings::flush_settings_writes();
    store.dirty = true;
    store.save();
    let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        dir_mode, 0o700,
        "data dir must be user-only (got {dir_mode:o})"
    );
    for name in [HISTORY_FILE, KEY_FILE, SETTINGS_FILE] {
        let path = dir.join(name);
        assert!(path.exists(), "{name} must exist for the permission check");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{name} must be user-only (got {mode:o})");
    }
    cleanup(&dir);
}

/// Current Unix time for tests that need real "now" alignment.
fn now_for_test() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
