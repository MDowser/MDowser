// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{BookmarkStore, Toggle, BOOKMARKS_FILE, FILE_MAGIC, FILE_VERSION, KEY_FILE};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Fresh, unique scratch directory per test.
fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdowser-bookmarks-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn encrypted_roundtrip_keeps_bookmarks_and_hides_plaintext() {
    let dir = tmp_dir("roundtrip");
    let mut store = BookmarkStore::load_from(dir.clone());
    assert_eq!(
        store.toggle("https://ex.com/a/", Some("Page A")),
        Toggle::Added
    );
    assert_eq!(store.toggle("file:///tmp/notes.md", None), Toggle::Added);
    // Force the write (the normal path is debounced).
    store.dirty = true;
    store.save();

    let blob = std::fs::read(dir.join(BOOKMARKS_FILE)).expect("bookmark file");
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

    let reloaded = BookmarkStore::load_from(dir.clone());
    assert_eq!(reloaded.entries().len(), 2);
    assert_eq!(reloaded.entries()[0].url, "https://ex.com/a/");
    assert_eq!(reloaded.entries()[0].title.as_deref(), Some("Page A"));
    assert!(reloaded.entries()[0].added_at > 0);
    assert_eq!(reloaded.entries()[1].url, "file:///tmp/notes.md");
    // The URL index is rebuilt on load: the toolbar ribbon must work on
    // the first frame after a restart.
    assert!(reloaded.is_bookmarked("https://ex.com/a/"));
    assert!(reloaded.is_bookmarked("file:///tmp/notes.md"));
    assert!(!reloaded.is_bookmarked("https://ex.com/b/"));
    cleanup(&dir);
}

#[test]
fn toggle_adds_removes_and_is_idempotent() {
    let dir = tmp_dir("toggle");
    let mut store = BookmarkStore::load_from(dir.clone());
    assert_eq!(store.toggle("https://ex.com/a/", Some("A")), Toggle::Added);
    assert_eq!(store.toggle("https://ex.com/b/", None), Toggle::Added);
    assert_eq!(store.entries().len(), 2);
    // Same URL (even with a new title): the toggle removes it instead
    // of adding a duplicate.
    assert_eq!(
        store.toggle("https://ex.com/a/", Some("A renamed")),
        Toggle::Removed
    );
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries()[0].url, "https://ex.com/b/");
    cleanup(&dir);
}

/// The byte budget is enforced on the SERIALIZED body. The store keeps
/// the caller's spelling for credential-free URLs (no re-encoding), but
/// a URL can still serialize larger than its char count, and a store
/// that ever exceeds the read cap loses every bookmark on the next
/// launch. Whatever the exact ratio, an add must never push the body
/// past `MAX_BOOKMARK_STORE_BYTES`: it is refused instead.
#[test]
fn byte_budget_refuses_an_over_long_entry() {
    let mut store = BookmarkStore::in_memory();
    // Char-capped URLs made of 4-byte UTF-8 chars: ~24 KiB each, so the
    // byte budget binds long before the 2000-entry cap.
    let filler_url = |index: usize| format!("https://ex.com/{index}/{}", "😀".repeat(8_000));
    let huge = format!("https://ex.com/huge/{}", "😀".repeat(8_000));
    assert!(huge.chars().count() <= crate::config::MAX_URL_INPUT_CHARS);
    let huge_bytes = super::entry_bytes(&super::Bookmark {
        url: huge.clone(),
        title: None,
        added_at: 1,
        icon: None,
        color: None,
    });
    assert!(
        huge_bytes > 16 * 1024,
        "the worst-case URL must serialize to more than 16 KiB: {huge_bytes}"
    );
    // Pre-fill until only less than one URL's worth of budget remains.
    let mut index = 0usize;
    while crate::config::MAX_BOOKMARK_STORE_BYTES - store.plain_bytes >= huge_bytes {
        let url = filler_url(index);
        index += 1;
        let bookmark = super::Bookmark {
            url: url.clone(),
            title: None,
            added_at: 1,
            icon: None,
            color: None,
        };
        store.urls.insert(url);
        store.plain_bytes += super::entry_bytes(&bookmark);
        store.entries.push(bookmark);
    }
    assert!(!store.entries.is_empty(), "the filler must add entries");
    assert!(
        store.entries.len() < crate::config::MAX_BOOKMARK_STORE_ENTRIES,
        "the byte budget must bind before the entry cap"
    );
    assert!(store.plain_bytes < crate::config::MAX_BOOKMARK_STORE_BYTES);
    // The URL cannot fit the remaining budget: refused untouched.
    let before = store.plain_bytes;
    let entries_before = store.entries.len();
    assert_eq!(store.toggle(&huge, None), Toggle::LimitReached);
    assert_eq!(
        store.plain_bytes, before,
        "a refused toggle changes nothing"
    );
    assert_eq!(store.entries.len(), entries_before);
    // A bounded entry dropped back in still fits.
    store.remove_entry(0);
    assert_eq!(store.toggle("https://ok.example/", None), Toggle::Added);
    assert!(store.plain_bytes <= crate::config::MAX_BOOKMARK_STORE_BYTES);
}

/// A store body that exceeds the byte budget (written by a different
/// version, or overflowing through percent-encoding) is trimmed
/// oldest-first on the next load so the serialization stays inside the
/// read cap forever after.
#[test]
fn oversized_stored_body_is_trimmed_on_load() {
    let mut store = BookmarkStore::in_memory();
    store.entries = vec![
        super::Bookmark {
            // Wider than the whole byte budget (the memory is transient:
            // a store like this can only arrive from disk).
            url: format!("https://big.example/{}", "x".repeat(64 * 1024 * 1024)),
            title: None,
            added_at: 1,
            icon: None,
            color: None,
        },
        super::Bookmark {
            url: "https://small.example/".to_string(),
            title: None,
            added_at: 2,
            icon: None,
            color: None,
        },
    ];
    store.rebuild_url_index();
    store.rebuild_plain_bytes();
    assert!(store.plain_bytes > crate::config::MAX_BOOKMARK_STORE_BYTES);
    store.trim();
    assert_eq!(
        store.entries.len(),
        1,
        "the oversized oldest entry is dropped"
    );
    assert_eq!(store.entries[0].url, "https://small.example/");
    assert!(store.plain_bytes <= crate::config::MAX_BOOKMARK_STORE_BYTES);
    // The serialized body matches the counter and stays inside the cap.
    let plain = store.serialize();
    assert_eq!(plain.len(), store.plain_bytes);
    assert!(plain.len() <= crate::config::MAX_BOOKMARK_STORE_BYTES);
}

/// Trimming to the ENTRY cap must not lose extra bookmarks to the byte
/// loop: the drained entries used to stay in `plain_bytes`, so a body over
/// both caps dropped additional live entries to compensate for phantom
/// bytes.
#[test]
fn entry_cap_trim_does_not_over_drop_against_the_byte_budget() {
    let mut store = BookmarkStore::in_memory();
    let cap = crate::config::MAX_BOOKMARK_STORE_ENTRIES;
    for index in 0..cap + 3 {
        store.entries.push(super::Bookmark {
            url: format!("https://e{index}.example/"),
            title: None,
            added_at: index as u64 + 1,
            icon: None,
            color: None,
        });
    }
    store.rebuild_url_index();
    store.rebuild_plain_bytes();
    assert!(
        store.plain_bytes < crate::config::MAX_BOOKMARK_STORE_BYTES,
        "the byte budget is not the limiter in this test"
    );
    store.trim();
    assert_eq!(store.entries.len(), cap, "only the excess is dropped");
    assert!(store.plain_bytes <= crate::config::MAX_BOOKMARK_STORE_BYTES);
    let plain = store.serialize();
    assert_eq!(plain.len(), store.plain_bytes);
}

/// A store whose body carries two entries with the same normalized URL (a
/// file written before normalization, or hand-edited) must not report "not
/// bookmarked" after one toggle while a tile for the page is still in the
/// grid: the URL index only forgets a URL no entry carries any more.
#[test]
fn toggling_a_duplicated_url_keeps_the_remaining_entry_bookmarked() {
    let mut store = BookmarkStore::in_memory();
    store.entries = vec![
        super::Bookmark {
            url: "https://dup.example/".to_string(),
            title: Some("older".to_string()),
            added_at: 1,
            icon: None,
            color: None,
        },
        super::Bookmark {
            url: "https://dup.example/".to_string(),
            title: Some("newer".to_string()),
            added_at: 2,
            icon: None,
            color: None,
        },
    ];
    store.rebuild_url_index();
    store.rebuild_plain_bytes();
    let url = url::Url::parse("https://dup.example/").expect("url");
    assert!(store.is_bookmarked_url(&url), "the index sees the page");
    assert_eq!(store.toggle("https://dup.example/", None), Toggle::Removed);
    assert_eq!(store.entries.len(), 1, "one duplicate remains");
    assert!(
        store.is_bookmarked_url(&url),
        "the ribbon must still report the remaining tile as bookmarked"
    );
    assert_eq!(store.toggle("https://dup.example/", None), Toggle::Removed);
    assert!(!store.is_bookmarked_url(&url), "now the page is gone");
}

/// `remove_entry` (the tile's X) follows the same duplicate-URL rule as
/// `toggle`: removing one of two duplicated entries must not clear the
/// index while the other's tile is still on screen.
#[test]
fn removing_a_duplicated_entry_keeps_the_remaining_one_bookmarked() {
    let mut store = BookmarkStore::in_memory();
    store.entries = vec![
        super::Bookmark {
            url: "https://dup.example/".to_string(),
            title: Some("older".to_string()),
            added_at: 1,
            icon: None,
            color: None,
        },
        super::Bookmark {
            url: "https://dup.example/".to_string(),
            title: Some("newer".to_string()),
            added_at: 2,
            icon: None,
            color: None,
        },
    ];
    store.rebuild_url_index();
    store.rebuild_plain_bytes();
    let url = url::Url::parse("https://dup.example/").expect("url");
    store.remove_entry(0);
    assert_eq!(store.entries.len(), 1, "one duplicate remains");
    assert!(
        store.is_bookmarked_url(&url),
        "the ribbon must still report the remaining tile as bookmarked"
    );
    store.remove_entry(0);
    assert!(!store.is_bookmarked_url(&url), "now the page is gone");
}

/// `plain_bytes` is the exact size of what `serialize` writes: the two
/// cannot drift (the byte budget check depends on it).
#[test]
fn plain_bytes_matches_the_serialized_body() {
    let mut store = BookmarkStore::in_memory();
    store.toggle("https://ex.com/a/", Some("Title A"));
    store.toggle("file:///tmp/notes.md", None);
    store.set_title(1, "Renamed");
    assert_eq!(store.serialize().len(), store.plain_bytes);
    store.remove_entry(0);
    assert_eq!(store.serialize().len(), store.plain_bytes);
    assert_eq!(store.plain_bytes, super::entry_bytes(&store.entries[0]));
}

/// First-run seeding inserts the curated defaults in listed order (Home
/// shows newest first), marked dirty for the debounced save. Seeding an
/// existing store is a no-op, and `load_from` never seeds: only `load`
/// seeds, gated on the store file being absent, so deleted defaults
/// never come back.
#[test]
fn seed_defaults_inserts_the_curated_set_once() {
    use super::DEFAULT_BOOKMARKS;
    let dir = tmp_dir("seed");
    let mut store = BookmarkStore::load_from(dir.clone());
    assert!(store.entries().is_empty(), "fresh dir starts empty");
    store.seed_defaults();
    assert_eq!(store.entries().len(), DEFAULT_BOOKMARKS.len());
    // Display order (newest first) matches the curated list order.
    let displayed: Vec<&str> = store
        .entries()
        .iter()
        .rev()
        .map(|b| b.url.as_str())
        .collect();
    let expected: Vec<&str> = DEFAULT_BOOKMARKS.iter().map(|(url, _)| *url).collect();
    assert_eq!(displayed, expected);
    for bookmark in store.entries() {
        assert!(bookmark.title.as_deref().is_some_and(|t| !t.is_empty()));
        assert!(bookmark.added_at > 0);
    }
    // Seeds persist through the encrypted save.
    store.save();
    assert!(dir.join(BOOKMARKS_FILE).is_file(), "seeds must reach disk");
    assert_eq!(
        BookmarkStore::load_from(dir.clone()).entries().len(),
        DEFAULT_BOOKMARKS.len()
    );
    // Seeding again changes nothing.
    store.seed_defaults();
    assert_eq!(store.entries().len(), DEFAULT_BOOKMARKS.len());
    cleanup(&dir);

    // A store file on disk — even an emptied one — is never reseeded by
    // `load_from` (the `load` gate is the store file being absent).
    let dir = tmp_dir("seed-empty");
    let mut saved = BookmarkStore::load_from(dir.clone());
    saved.toggle("https://ex.com/a/", None);
    saved.dirty = true;
    saved.save();
    saved.remove_entry(0);
    saved.save();
    assert!(dir.join(BOOKMARKS_FILE).is_file());
    assert!(BookmarkStore::load_from(dir.clone()).entries().is_empty());
    cleanup(&dir);
}

/// The Home tile customization (name, glyph, color) survives a save +
/// reload through the encrypted file.
#[test]
fn tile_customization_roundtrips() {
    let dir = tmp_dir("custom");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", Some("A"));
    assert!(store.set_title(0, "Rust news"));
    // The glyph and color editors are gone from the UI, but stored
    // values still round-trip (written directly: no setters remain).
    store.entries[0].icon = Some("🦀".to_string());
    store.entries[0].color = Some(3);
    store.dirty = true;
    store.save();

    let reloaded = BookmarkStore::load_from(dir.clone());
    let bookmark = &reloaded.entries()[0];
    assert_eq!(bookmark.title.as_deref(), Some("Rust news"));
    assert_eq!(bookmark.icon.as_deref(), Some("🦀"));
    assert_eq!(bookmark.color, Some(3));
    cleanup(&dir);
}

/// Load-time glyph sanitization: whitespace and control characters are
/// dropped, ZWJ clusters survive and over-long glyphs are capped.
#[test]
fn stored_glyphs_are_sanitized_on_load() {
    let family = "👨‍👩‍👧";
    let long = "a".repeat(crate::config::MAX_BOOKMARK_ICON_CHARS + 4);
    let capped = "a".repeat(crate::config::MAX_BOOKMARK_ICON_CHARS);
    let plain = format!(
        "1700000000\thttps://ex.com/a/\tA\t A b \n\
         1700000001\thttps://ex.com/b/\tB\t{family}\n\
         1700000002\thttps://ex.com/c/\tC\t{long}\n\
         1700000003\thttps://ex.com/d/\tD\t \n"
    );
    let parsed = super::deserialize(plain.as_bytes());
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].icon.as_deref(), Some("Ab"), "whitespace dropped");
    assert_eq!(
        parsed[1].icon.as_deref(),
        Some(family),
        "ZWJ cluster survives"
    );
    assert_eq!(
        parsed[2].icon.as_deref(),
        Some(capped.as_str()),
        "over-long glyph capped"
    );
    assert_eq!(parsed[3].icon, None, "whitespace-only clears");
}

/// `move_entry` reorders in place, bounds-checks and reports no-ops.
#[test]
fn move_entry_reorders_within_bounds() {
    let dir = tmp_dir("move");
    let mut store = BookmarkStore::load_from(dir.clone());
    for i in 0..4 {
        store.toggle(&format!("https://ex.com/{i}/"), None);
    }
    let urls = |store: &BookmarkStore| -> Vec<String> {
        store.entries().iter().map(|b| b.url.clone()).collect()
    };
    let base = urls(&store);
    // Later -> earlier.
    assert!(store.move_entry(3, 1));
    assert_eq!(
        urls(&store),
        vec![
            base[0].clone(),
            base[3].clone(),
            base[1].clone(),
            base[2].clone()
        ]
    );
    // Earlier -> later.
    assert!(store.move_entry(0, 2));
    assert_eq!(
        urls(&store),
        vec![
            base[3].clone(),
            base[1].clone(),
            base[0].clone(),
            base[2].clone()
        ]
    );
    // No-ops and out-of-range are ignored, never a panic.
    assert!(!store.move_entry(1, 1));
    assert!(!store.move_entry(9, 0));
    assert!(!store.move_entry(0, 9));
    assert_eq!(store.entries().len(), 4, "nothing was dropped");
    cleanup(&dir);
}

/// Load-time color sanitization: an out-of-range or non-numeric fifth
/// field falls back to automatic instead of keeping a bad index.
#[test]
fn parsed_colors_are_range_checked() {
    let plain = b"1700000000\thttps://ex.com/a/\tPage A\tx\t99\n1700000001\thttps://ex.com/b/\tPage B\tx\tnope\n1700000002\thttps://ex.com/c/\tPage C\tx\t2\n";
    let parsed = super::deserialize(plain);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].color, None, "out-of-range color dropped");
    assert_eq!(parsed[1].color, None, "non-numeric color dropped");
    assert_eq!(parsed[2].color, Some(2));
}

/// A version-1 line written before the tile customization existed has
/// only three fields: it parses, with no icon/color.
#[test]
fn legacy_three_field_lines_parse_without_customization() {
    let plain = b"1700000000\thttps://ex.com/a/\tPage A\n1700000001\thttps://ex.com/b/\t\n";
    let parsed = super::deserialize(plain);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].title.as_deref(), Some("Page A"));
    assert_eq!(parsed[0].icon, None);
    assert_eq!(parsed[0].color, None);
    assert_eq!(parsed[1].title, None);
}

#[test]
fn non_recordable_urls_are_never_stored() {
    let dir = tmp_dir("input");
    let mut store = BookmarkStore::load_from(dir.clone());
    for url in [
        "http://ex.com/x",
        "javascript:alert(1)",
        "file://server/share/x.md",
        "   ",
    ] {
        assert_eq!(store.toggle(url, None), Toggle::Unchanged, "{url}");
    }
    assert!(store.entries().is_empty());
    cleanup(&dir);
}

/// A URL past the input cap is refused, never truncated into a
/// different page (`...` would store a page the row could not open).
#[test]
fn oversized_urls_are_refused_not_truncated() {
    let dir = tmp_dir("longurl");
    let mut store = BookmarkStore::load_from(dir.clone());
    // Over the BYTE cap of a serialized URL (percent-encoding is counted,
    // see `util::is_recordable_url`).
    let over = format!(
        "https://ex.com/{}",
        "a".repeat(crate::config::MAX_URL_BYTES)
    );
    assert_eq!(store.toggle(&over, None), Toggle::Unchanged);
    assert!(store.entries().is_empty(), "over-cap URL refused");
    cleanup(&dir);
}

/// At the cap, an add is REFUSED (never silently dropping the oldest
/// bookmark) and the existing entries stay untouched; removing one
/// makes room again.
#[test]
fn the_entry_cap_refuses_new_adds() {
    let dir = tmp_dir("cap");
    let mut store = BookmarkStore::load_from(dir.clone());
    let cap = crate::config::MAX_BOOKMARK_STORE_ENTRIES;
    for i in 0..cap {
        assert_eq!(
            store.toggle(&format!("https://ex.com/{i}/"), None),
            Toggle::Added
        );
    }
    assert_eq!(
        store.toggle("https://ex.com/one-more/", None),
        Toggle::LimitReached
    );
    assert_eq!(store.entries().len(), cap);
    assert_eq!(
        store.entries()[0].url,
        "https://ex.com/0/",
        "the oldest bookmark was kept"
    );
    // An EXISTING bookmark can still be toggled off at the cap.
    assert_eq!(store.toggle("https://ex.com/7/", None), Toggle::Removed);
    assert_eq!(
        store.toggle("https://ex.com/one-more/", None),
        Toggle::Added
    );
    cleanup(&dir);
}

#[test]
fn remove_entry_by_index_is_bounds_checked() {
    let dir = tmp_dir("remove");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", None);
    store.toggle("https://ex.com/b/", None);
    store.remove_entry(0);
    assert_eq!(store.entries().len(), 1);
    assert_eq!(store.entries()[0].url, "https://ex.com/b/");
    store.remove_entry(9); // out of bounds: no-op, no panic.
    assert_eq!(store.entries().len(), 1);
    cleanup(&dir);
}

/// `is_bookmarked` (the toolbar ribbon state) tracks add/remove.
#[test]
fn is_bookmarked_reflects_the_store() {
    let dir = tmp_dir("contains");
    let mut store = BookmarkStore::load_from(dir.clone());
    assert!(!store.is_bookmarked("https://ex.com/a/"));
    store.toggle("https://ex.com/a/", None);
    assert!(store.is_bookmarked("https://ex.com/a/"));
    assert!(!store.is_bookmarked("https://ex.com/b/"));
    // The exact index answers the parsed-URL form too (the toolbar's
    // per-frame path), including a stored-then-removed entry.
    let parsed = url::Url::parse("https://ex.com/a/").expect("parse");
    assert!(store.is_bookmarked_url(&parsed));
    store.remove_entry(0);
    assert!(!store.is_bookmarked("https://ex.com/a/"));
    assert!(!store.is_bookmarked_url(&parsed));
    cleanup(&dir);
}

#[test]
fn tampered_or_truncated_store_loads_empty_without_panicking() {
    let dir = tmp_dir("tamper");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", None);
    store.dirty = true;
    store.save();

    // Flip one ciphertext byte: authentication must fail closed.
    let path = dir.join(BOOKMARKS_FILE);
    let mut blob = std::fs::read(&path).unwrap();
    let last = blob.len() - 1;
    blob[last] ^= 0x01;
    std::fs::write(&path, &blob).unwrap();
    assert!(BookmarkStore::load_from(dir.clone()).entries().is_empty());

    // Truncated file: also empty, never a panic.
    std::fs::write(&path, &blob[..blob.len() / 2]).unwrap();
    assert!(BookmarkStore::load_from(dir.clone()).entries().is_empty());

    // History's magic (wrong store) is rejected before decryption.
    std::fs::write(&path, b"MDHS\x01short").unwrap();
    assert!(BookmarkStore::load_from(dir.clone()).entries().is_empty());
    cleanup(&dir);
}

/// A failed background write must surface on Home AND leave the store
/// dirty so the next tick retries; a later success clears the warning.
#[test]
fn failed_background_write_is_retried_and_the_warning_clears() {
    let dir = tmp_dir("save-retry");
    let mut store = BookmarkStore::load_from(dir.clone());
    // Block the destination with a directory: `rename` onto it fails
    // on every platform.
    std::fs::create_dir_all(dir.join(BOOKMARKS_FILE)).unwrap();
    store.toggle("https://ex.com/retry/", None);
    let expedite = |s: &mut BookmarkStore| {
        s.last_save =
            Instant::now() - Duration::from_secs(crate::config::BOOKMARKS_SAVE_DEBOUNCE_SECS + 1);
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
    // Unblock and force the retry; the warning clears once it lands.
    std::fs::remove_dir_all(dir.join(BOOKMARKS_FILE)).unwrap();
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
    assert!(dir.join(BOOKMARKS_FILE).is_file());
    assert_eq!(
        BookmarkStore::load_from(dir.clone()).entries().len(),
        1,
        "bookmark persisted by the retry"
    );
    cleanup(&dir);
}

#[test]
fn on_exit_flushes_pending_bookmarks() {
    let dir = tmp_dir("exit");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", Some("A"));
    assert!(store.pending_save(), "debounced write still pending");
    store.on_exit();
    assert!(!store.pending_save());
    assert!(dir.join(BOOKMARKS_FILE).exists());
    let reloaded = BookmarkStore::load_from(dir.clone());
    assert_eq!(reloaded.entries().len(), 1);
    assert_eq!(reloaded.entries()[0].title.as_deref(), Some("A"));
    cleanup(&dir);
}

#[test]
fn in_memory_store_never_touches_disk() {
    let mut store = BookmarkStore::in_memory();
    store.toggle("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    assert!(store.save_error().is_none());
    assert_eq!(store.entries().len(), 1);
    store.on_exit();
    assert_eq!(store.entries().len(), 1);
}

#[test]
fn tick_reports_pending_until_the_debounce_elapses() {
    let dir = tmp_dir("tick");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", None);
    assert!(store.pending_save());
    assert!(store.tick());
    store.last_save =
        Instant::now() - Duration::from_secs(crate::config::BOOKMARKS_SAVE_DEBOUNCE_SECS + 1);
    assert!(store.tick());
    assert!(!store.pending_save());
    for _ in 0..200 {
        if !store.tick() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!store.tick(), "background write settled");
    assert!(dir.join(BOOKMARKS_FILE).exists());
    cleanup(&dir);
}

#[cfg(unix)]
#[test]
fn key_file_is_user_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("perms");
    let mut store = BookmarkStore::load_from(dir.clone());
    store.toggle("https://ex.com/a/", None);
    store.dirty = true;
    store.save();
    let path = dir.join(KEY_FILE);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "key must be user-only (got {mode:o})");
    cleanup(&dir);
}
