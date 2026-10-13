// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{decrypt, encrypt, load_or_create_key};
use std::path::PathBuf;

/// Fresh, unique scratch directory per test.
fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdowser-crypto-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn aead_roundtrips_and_rejects_wrong_key_short_and_wrong_magic() {
    let key = [7u8; 32];
    let plain = b"https://ex.com/\tHello\n";
    let blob = encrypt(&key, b"MDHS", 1, plain).expect("encrypt");
    assert_eq!(
        decrypt(&key, b"MDHS", 1, &blob).as_deref(),
        Some(plain.as_slice())
    );

    let mut wrong_key = key;
    wrong_key[0] ^= 0xFF;
    assert_eq!(
        decrypt(&wrong_key, b"MDHS", 1, &blob),
        None,
        "wrong key fails closed"
    );
    assert_eq!(
        decrypt(&key, b"MDHS", 1, &blob[..10]),
        None,
        "short blob fails"
    );
    // The magic is authenticated as AAD: a blob from another store (or
    // a tampered header) never opens.
    assert_eq!(
        decrypt(&key, b"MDBM", 1, &blob),
        None,
        "wrong magic fails closed"
    );
    assert_eq!(
        decrypt(&key, b"MDHS", 2, &blob),
        None,
        "wrong version fails closed"
    );
}

#[test]
fn key_is_created_reused_and_regenerated_when_corrupt() {
    let dir = tmp_dir("key");
    let path = dir.join("store.key");
    let first = load_or_create_key(&path).expect("create");
    assert_eq!(std::fs::read(&path).expect("key file").len(), 32);
    assert_eq!(load_or_create_key(&path), Some(first), "reused as-is");
    std::fs::write(&path, b"short").unwrap();
    let fresh = load_or_create_key(&path).expect("regenerated");
    assert_ne!(fresh, first, "a wrong-size key is discarded");
    assert_eq!(std::fs::read(&path).expect("key file").len(), 32);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two instances creating the key at the same moment must agree on one
/// key: the atomic `hard_link` installation means both read the SAME
/// complete 32 bytes (the old `create_new` + write path could expose an
/// empty file that the loser then deleted, leaving the winner with an
/// unlinked key).
#[test]
fn concurrent_creators_agree_on_one_key() {
    for round in 0..8 {
        let dir = tmp_dir(&format!("keyrace{round}"));
        let path = dir.join("store.key");
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || load_or_create_key(&path))
            })
            .collect();
        let keys: Vec<_> = threads
            .into_iter()
            .map(|t| t.join().expect("thread"))
            .collect();
        assert!(keys.iter().all(|k| k.is_some()), "every creator gets a key");
        assert!(
            keys.iter().all(|k| *k == keys[0]),
            "all creators agree on the winner's key"
        );
        assert_eq!(std::fs::read(&path).expect("key file").len(), 32);
        // The winner's key file is the one every caller read.
        assert_eq!(load_or_create_key(&path), keys[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A key copied in with wider permissions is tightened before use: the
/// key is the only thing protecting the encrypted store.
#[cfg(unix)]
#[test]
fn wider_key_permissions_are_tightened() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("keymode");
    let path = dir.join("store.key");
    let key = load_or_create_key(&path).expect("create");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(load_or_create_key(&path), Some(key), "still loads");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "key tightened on load");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A symlinked key is refused fail-closed and its TARGET is never touched:
/// the tightening runs on the opened handle (`O_NOFOLLOW`), so a symlink
/// planted at `store.key` can neither be followed nor have an arbitrary file
/// chmod'ed through it.
#[cfg(unix)]
#[test]
fn symlinked_key_is_refused_and_target_permissions_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("keysymlink");
    let target = dir.join("victim.bin");
    std::fs::write(&target, [7u8; 32]).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    let link = dir.join("store.key");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(load_or_create_key(&link), None, "refused, never followed");
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o644,
        "the symlink target was not chmod'ed"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        [7u8; 32],
        "the target file is untouched"
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the key path still holds the symlink (refused, never deleted)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The corrupt-key repair lock serializes replacement across instances
/// and is released on drop; a second holder waits (bounded) and then
/// gives up instead of blocking forever.
#[test]
fn repair_lock_serializes_and_releases() {
    use super::{acquire_repair_lock, REPAIR_LOCK_WAIT};
    let dir = tmp_dir("repairlock");
    let key = dir.join("store.key");
    let first = acquire_repair_lock(&key).expect("first lock wins");
    let started = std::time::Instant::now();
    assert!(
        acquire_repair_lock(&key).is_none(),
        "second holder gives up after the bounded wait"
    );
    assert!(started.elapsed() >= REPAIR_LOCK_WAIT);
    drop(first);
    assert!(acquire_repair_lock(&key).is_some(), "lock released on drop");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stale repair lock that CANNOT be removed (a directory at the lock
/// path stands in for an immutable file / permission quirk) must make
/// the helper give up at once instead of spinning forever on the UI
/// thread: `remove_file` failing used to re-observe the same stale lock
/// with no sleep and no wait check.
#[test]
fn undeletable_stale_repair_lock_does_not_spin() {
    use super::acquire_repair_lock;
    let dir = tmp_dir("repairstuck");
    let key = dir.join("store.key");
    // A DIRECTORY at `<key>.repair`: `create_new` fails with
    // AlreadyExists and `remove_file` fails with EISDIR/EPERM, so the
    // stale branch can never succeed. Backdate it past the stale
    // window so the stale takeover path is the one taken.
    let stubborn = dir.join("store.key.repair");
    std::fs::create_dir(&stubborn).expect("stand-in lock");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    let stamp: std::fs::FileTimes = std::fs::FileTimes::new().set_modified(old);
    // A `File` handle (not a symlink) backdates the stand-in reliably on
    // every platform the app ships on.
    let handle = std::fs::File::open(&stubborn).expect("open dir handle");
    handle.set_times(stamp).expect("backdate the lock");
    drop(handle);
    let started = std::time::Instant::now();
    assert!(
        acquire_repair_lock(&key).is_none(),
        "an undeletable stale lock must fall back to a plain read"
    );
    assert!(
        started.elapsed() < super::REPAIR_LOCK_WAIT,
        "the failed takeover must not burn the wait budget: {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A corrupt key is regenerated and the repair lock cleaned up, so the
/// next launch is not stuck behind a leftover.
#[test]
fn corrupt_key_regeneration_cleans_the_repair_lock() {
    let dir = tmp_dir("repairclean");
    let path = dir.join("store.key");
    std::fs::write(&path, b"short").expect("write corrupt key");
    assert!(load_or_create_key(&path).is_some(), "regenerated");
    assert!(!dir.join("store.key.repair").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// While another instance holds the repair lock, a corrupt key is NOT
/// deleted: the file on disk may already be that instance's complete
/// fresh key. The losing launch fails closed (no key); once the lock is
/// released the repair runs normally.
#[test]
fn corrupt_key_is_left_intact_while_another_repair_holds_the_lock() {
    use super::acquire_repair_lock;
    let dir = tmp_dir("repairhold");
    let path = dir.join("store.key");
    std::fs::write(&path, b"short").expect("write corrupt key");
    let held = acquire_repair_lock(&path).expect("hold the repair lock");
    assert_eq!(
        load_or_create_key(&path),
        None,
        "fails closed while another repair holds the lock"
    );
    assert_eq!(
        std::fs::read(&path).expect("key file"),
        b"short",
        "the held file is never deleted"
    );
    drop(held);
    let key = load_or_create_key(&path).expect("regenerated after release");
    assert_eq!(std::fs::read(&path).expect("key file").len(), 32);
    assert_eq!(load_or_create_key(&path), Some(key), "stable afterwards");
    let _ = std::fs::remove_dir_all(&dir);
}
