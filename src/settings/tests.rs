// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    load_session_from, read_setting, save_session_to, write_setting, Prefs, ThemePref,
    WindowGeometry, MAX_SETTINGS_BYTES, SETTINGS_FILE,
};
use eframe::egui;
use std::path::PathBuf;

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdowser-settings-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn theme_settings_roundtrip_and_reject_garbage() {
    for theme in [ThemePref::System, ThemePref::Light, ThemePref::Dark] {
        assert_eq!(ThemePref::from_setting(theme.as_setting()), Some(theme));
    }
    assert_eq!(ThemePref::from_setting(" DARK "), Some(ThemePref::Dark));
    assert_eq!(ThemePref::from_setting("sepia"), None);
    assert_eq!(ThemePref::default(), ThemePref::System);
}

/// The store reader must refuse missing, non-regular and over-cap files
/// without allocating, and return the bytes for a valid regular file.
#[test]
fn read_regular_file_is_bounded_and_type_checked() {
    let dir = tmp_dir("read-regular");
    // Missing file: `None`, never an error path that panics.
    assert_eq!(super::read_regular_file(&dir.join("nope.bin"), 1024), None);
    // A directory is not a regular file.
    assert_eq!(super::read_regular_file(&dir, 1024), None);
    // Over the cap: refused before reading.
    let big = dir.join("big.bin");
    std::fs::write(&big, vec![0u8; 2048]).unwrap();
    assert_eq!(super::read_regular_file(&big, 1024), None);
    // A valid file comes back whole.
    let ok = dir.join("ok.bin");
    std::fs::write(&ok, b"hello").unwrap();
    assert_eq!(
        super::read_regular_file(&ok, 1024).as_deref(),
        Some(b"hello".as_slice())
    );
    cleanup(&dir);
}

/// A store file is never a symlink: a planted link must not make the reader
/// follow it out of the data directory (nor tighten the target's mode).
#[cfg(unix)]
#[test]
fn read_regular_file_rejects_symlinks() {
    let dir = tmp_dir("read-regular-link");
    let target = dir.join("target.bin");
    std::fs::write(&target, b"secret").unwrap();
    let link = dir.join("link.bin");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(super::read_regular_file(&link, 1024), None);
    cleanup(&dir);
}

/// Writing one key must never clobber another (history retention and the
/// Preferences window update different keys in the same file).
#[test]
fn write_setting_preserves_other_keys() {
    use super::{SETTINGS_MAGIC, SETTINGS_VERSION, STORE_KEY_FILE};
    use crate::store::crypto::{decrypt, load_or_create_key};
    let dir = tmp_dir("preserve");
    write_setting(&dir, "retention", "90").unwrap();
    write_setting(&dir, "theme", "dark").unwrap();
    write_setting(&dir, "retention", "30").unwrap();
    assert_eq!(read_setting(&dir, "retention").as_deref(), Some("30"));
    assert_eq!(read_setting(&dir, "theme").as_deref(), Some("dark"));
    // A rewrite keeps exactly the two keys, no duplicates (decrypt first:
    // the file on disk is ciphertext).
    let blob = std::fs::read(dir.join(SETTINGS_FILE)).unwrap();
    let key = load_or_create_key(&dir.join(STORE_KEY_FILE)).unwrap();
    let plain = decrypt(&key, SETTINGS_MAGIC, SETTINGS_VERSION, &blob).unwrap();
    let body = std::str::from_utf8(&plain).unwrap();
    assert_eq!(body.matches("retention=").count(), 1);
    assert_eq!(body.matches("theme=").count(), 1);
    // Ciphertext must not leak plaintext prefs.
    assert!(
        !blob.windows(9).any(|w| w == b"retention"),
        "ciphertext must not leak settings"
    );
    cleanup(&dir);
}

#[test]
fn prefs_roundtrip_and_clamp_zoom() {
    let dir = tmp_dir("prefs");
    let mut prefs = Prefs::load_from(dir.clone());
    assert_eq!(prefs.theme(), ThemePref::System);
    assert_eq!(prefs.default_zoom(), 1.0);
    // Restore is ON by default; exercise the off -> on roundtrip.
    assert!(prefs.restore_tabs());
    prefs.set_restore_tabs(false);
    // Writes are queued on the background settings worker: drain before
    // reading the file back (the app flushes at exit).
    super::flush_settings_writes();
    assert!(!Prefs::load_from(dir.clone()).restore_tabs());
    prefs.set_theme(ThemePref::Dark);
    prefs.set_default_zoom(9.0); // clamped to ZOOM_MAX
    prefs.set_restore_tabs(true);
    super::flush_settings_writes();
    let reloaded = Prefs::load_from(dir.clone());
    assert_eq!(reloaded.theme(), ThemePref::Dark);
    assert_eq!(reloaded.default_zoom(), crate::config::ZOOM_MAX);
    assert!(reloaded.restore_tabs());
    cleanup(&dir);
}

/// A corrupt/unknown `restore_tabs` value keeps the DEFAULT (restore ON)
/// instead of being read as "off": the off path deletes the encrypted
/// session, so a garbage value must never destroy the visited-URL memory.
#[test]
fn corrupt_restore_tabs_keeps_restore_on_and_session_intact() {
    let dir = tmp_dir("corrupt-restore");
    save_session_to(&dir, &[Some("https://example.com/".into())], 0).expect("seed session");
    write_setting(&dir, "restore_tabs", "yes").unwrap();
    super::flush_settings_writes();
    let prefs = Prefs::load_from(dir.clone());
    assert!(
        prefs.restore_tabs(),
        "an unknown spelling keeps the default"
    );
    assert!(
        dir.join(super::SESSION_FILE).exists(),
        "the session must survive a corrupt preference value"
    );
    // An explicit false still turns it off and deletes the session.
    write_setting(&dir, "restore_tabs", "false").unwrap();
    super::flush_settings_writes();
    let prefs = Prefs::load_from(dir.clone());
    assert!(!prefs.restore_tabs(), "explicit false is honored");
    assert!(
        !dir.join(super::SESSION_FILE).exists(),
        "turning restore off deletes the session"
    );
    cleanup(&dir);
}

/// Preference writes are queued on the background worker: the in-memory
/// state changes immediately, the file lands after a flush, and the
/// queued body carries every key (no stale read-modify-write).
#[test]
fn async_setting_writes_are_queued_and_drained_by_flush() {
    let dir = tmp_dir("async-prefs");
    let mut prefs = Prefs::load_from(dir.clone());
    prefs.set_theme(ThemePref::Dark);
    prefs.set_default_zoom(1.25);
    // The in-memory state changes synchronously (the UI reads it back
    // immediately); the file write is what is queued.
    assert_eq!(prefs.theme(), ThemePref::Dark);
    assert_eq!(prefs.default_zoom(), 1.25);
    // (The file may already have landed on a fast worker: the flush
    // below is what guarantees it, so no "not yet written" assertion.)
    super::flush_settings_writes();
    let body = super::read_settings_plaintext(&dir).expect("flushed");
    assert!(body.contains("theme=dark"), "{body}");
    assert!(body.contains("zoom=1.25"), "{body}");
    // A second burst keeps every key (the read-modify-write runs against
    // the shared body, not a stale file).
    prefs.set_theme(ThemePref::Light);
    super::flush_settings_writes();
    let body = super::read_settings_plaintext(&dir).expect("flushed");
    assert!(body.contains("theme=light"), "{body}");
    assert!(body.contains("zoom=1.25"), "{body}");
    assert_eq!(body.matches("theme=").count(), 1, "no duplicates: {body}");
    cleanup(&dir);
}

#[test]
fn session_roundtrip_keeps_home_tabs_and_active() {
    let dir = tmp_dir("session");
    let tabs = vec![
        Some("https://ex.com/a/".to_string()),
        None,
        Some("about:history".to_string()),
        Some("about:licenses".to_string()),
        Some("file:///tmp/notes.md".to_string()),
    ];
    save_session_to(&dir, &tabs, 2).unwrap();
    assert_eq!(
        load_session_from(&dir),
        Some((tabs, 2)),
        "session survives including the bare Home tab, the internal pages and local files"
    );
    cleanup(&dir);
}

#[test]
fn session_rejects_bad_and_non_http_input() {
    use super::SESSION_FILE;
    let dir = tmp_dir("session-bad");
    // Undecryptable blob is refused.
    std::fs::write(dir.join(SESSION_FILE), b"not-encrypted").unwrap();
    assert_eq!(load_session_from(&dir), None, "undecryptable is refused");

    // Encrypted but only non-recordable URLs: filtered to empty -> no session.
    let bad = vec![
        Some("http://plain.example/".to_string()),
        Some("javascript:alert(1)".to_string()),
        Some("file://server/share/x.md".to_string()),
    ];
    save_session_to(&dir, &bad, 0).unwrap();
    assert_eq!(
        load_session_from(&dir),
        None,
        "only https/about/local files survive; an empty result is no session"
    );
    cleanup(&dir);
}

/// A session the app itself wrote must always load: URL serialization
/// percent-encodes, so one 4-byte UTF-8 char becomes 12 ASCII chars and
/// a char-derived byte cap used to reject the file (tabs silently lost
/// after a restart).
#[test]
fn session_roundtrips_a_percent_encoded_url() {
    let dir = tmp_dir("session-encoded");
    let emoji = "\u{1f600}".repeat(4000);
    let url = url::Url::parse(&format!("https://ex.com/{emoji}")).unwrap();
    let tabs = vec![Some(url.to_string())];
    assert!(tabs[0].as_ref().unwrap().len() > 4096, "encoded, not raw");
    save_session_to(&dir, &tabs, 0).unwrap();
    assert_eq!(load_session_from(&dir), Some((tabs, 0)));
    cleanup(&dir);
}

/// A change that would push the settings plaintext past `MAX_SETTINGS_BYTES`
/// is REFUSED: the writer must never produce a file the reader rejects at
/// the next launch (defaults would silently replace every preference), and
/// the refusal must not poison the cached body for a later, smaller change.
#[test]
fn settings_writes_refuse_an_over_budget_body() {
    let dir = tmp_dir("settings-budget");
    let huge = "v".repeat(MAX_SETTINGS_BYTES as usize + 1);
    let err = write_setting(&dir, "theme", &huge).expect_err("over-budget write is refused");
    assert!(err.to_string().contains("budget"), "{err}");
    assert!(
        !dir.join(SETTINGS_FILE).exists(),
        "nothing was written for the refused change"
    );
    // The cache still holds a usable (pre-change) body: a normal write and
    // read work right after the refusal.
    write_setting(&dir, "theme", "dark").expect("a small write still works");
    assert_eq!(read_setting(&dir, "theme").as_deref(), Some("dark"));
    cleanup(&dir);
}

/// The window geometry is one `settings.bin` key: it must round-trip
/// (including a negative position and the maximized flag) and refuse every
/// malformed spelling, so a corrupt value falls back to the first-run
/// geometry instead of opening a bogus window.
#[test]
fn window_geometry_roundtrips_and_rejects_garbage() {
    let geometry = WindowGeometry {
        pos: egui::pos2(-12.5, 80.0),
        size: egui::vec2(1536.0, 864.0),
        maximized: true,
    };
    assert_eq!(
        WindowGeometry::from_setting(&geometry.as_setting()),
        Some(geometry)
    );
    for bad in [
        "",
        "1,2,3",         // missing fields
        "1,2,3,4",       // missing maximized
        "a,2,3,4,0",     // non-numeric position
        "1,2,-3,4,0",    // negative size
        "1,2,3,0,0",     // zero size
        "1,2,3,4,maybe", // unknown maximized spelling
        "1,2,3,4,0,9",   // trailing field
        "1,2,inf,4,0",   // non-finite size
        "1,2,nan,4,0",   // non-finite size
    ] {
        assert_eq!(WindowGeometry::from_setting(bad), None, "{bad:?} refused");
    }
}

/// `Prefs` reads the window geometry from the encrypted settings and ignores
/// a corrupt value (first-run behavior). Seeded through the synchronous
/// helper so the test does not race the app-wide settings worker.
#[test]
fn prefs_window_geometry_roundtrip() {
    let dir = tmp_dir("window-geometry");
    let geometry = WindowGeometry {
        pos: egui::pos2(120.0, 80.0),
        size: egui::vec2(1536.0, 864.0),
        maximized: false,
    };
    let prefs = Prefs::load_from(dir.clone());
    assert_eq!(prefs.window_geometry(), None, "no geometry on a first run");
    write_setting(&dir, "window", &geometry.as_setting()).unwrap();
    assert_eq!(
        Prefs::load_from(dir.clone()).window_geometry(),
        Some(geometry)
    );
    write_setting(&dir, "window", "garbage").unwrap();
    assert_eq!(
        Prefs::load_from(dir.clone()).window_geometry(),
        None,
        "a corrupt value is ignored, never an error"
    );
    cleanup(&dir);
}
