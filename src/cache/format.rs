// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache::format`: binary record encoding, deterministic filenames and
//! wall-clock helpers shared by the three disk stores.
//!
//! - Records are length-prefixed little-endian fields (`u32` byte length +
//!   bytes; `u64` for times/sizes; `u8` version per file). No text parsing,
//!   no allocator bombs: every length is checked against the caller's cap
//!   before allocating.
//! - Filenames are `fnv1a` hashes (deterministic across runs, unlike
//!   `RandomState`): a hostname or URL in cleartext inside `cache/` would
//!   leak the browsing history to anyone listing the directory, backups
//!   included. Every record ALSO carries its key string, verified on load,
//!   so a (cryptographically negligible) hash collision can only cause a
//!   miss, never serve the wrong body. [`is_stem`] rejects any stem that is
//!   not the exact 16-lowercase-hex shape this cache writes, so a loaded
//!   index can never turn into a path component (or a panicking slice).
//! - Times are Unix seconds (`SystemTime`): persistable across restarts,
//!   unlike `Instant`. Clock jumps are tolerated: expiry is advisory (a
//!   wildly future clock only delays revalidation; a past clock only causes
//!   an early revalidation, never stale serving past `stored_at` sanity).

use std::time::{SystemTime, UNIX_EPOCH};

/// Deterministic 64-bit FNV-1a (same recipe as `media::fnv1a`, local so
/// this module has no media dependency).
pub(crate) fn fnv1a64(value: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for b in value.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Hex filename stem for a cache key (16 lowercase hex chars).
pub(crate) fn stem_for(key: &str) -> String {
    format!("{:016x}", fnv1a64(key))
}

/// `true` when `stem` is exactly the 16 lowercase hex chars this cache
/// writes. Loaded indexes are untrusted input (a hand-edited encrypted file
/// or a disk swap): a stem is used as a path component and as a `&str`
/// slice, so anything else is rejected before it reaches the filesystem.
pub(crate) fn is_stem(stem: &str) -> bool {
    stem.len() == 16
        && stem
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Two-level shard directory for a stem (`ab/cd...` -> `ab/`): keeps any
/// single directory under a few hundred entries at the 50k cap.
///
/// Invalid stems (never produced by [`stem_for`]) map to the `00` shard
/// instead of panicking on a non-char-boundary slice; callers that build
/// paths still validate with [`is_stem`] and refuse them.
pub(crate) fn shard_for(stem: &str) -> &str {
    if is_stem(stem) {
        &stem[..2]
    } else {
        "00"
    }
}

/// Current Unix time in seconds (saturates at 0 before the epoch, which
/// cannot happen on a real clock).
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Bounded `u32`-prefixed byte string reader: `None` on truncation or when
/// the declared length exceeds `cap` (fail closed, never pre-allocate).
pub(crate) fn read_bytes<'a>(data: &mut &'a [u8], cap: usize) -> Option<&'a [u8]> {
    if data.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    *data = &data[4..];
    if len > cap || data.len() < len {
        return None;
    }
    let (out, rest) = data.split_at(len);
    *data = rest;
    Some(out)
}

/// Bounded `u32`-prefixed UTF-8 string reader.
pub(crate) fn read_string(data: &mut &[u8], cap: usize) -> Option<String> {
    let bytes = read_bytes(data, cap)?;
    String::from_utf8(bytes.to_vec()).ok()
}

/// `u64` little-endian reader.
pub(crate) fn read_u64(data: &mut &[u8]) -> Option<u64> {
    if data.len() < 8 {
        return None;
    }
    let value = u64::from_le_bytes(data[..8].try_into().ok()?);
    *data = &data[8..];
    Some(value)
}

/// `u8` reader.
pub(crate) fn read_u8(data: &mut &[u8]) -> Option<u8> {
    let (first, rest) = data.split_first()?;
    *data = rest;
    Some(*first)
}

/// Appends a `u32`-prefixed byte string.
pub(crate) fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Appends a `u32`-prefixed string.
pub(crate) fn write_string(out: &mut Vec<u8>, value: &str) {
    write_bytes(out, value.as_bytes());
}

/// Appends a `u64` little-endian.
pub(crate) fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_deterministic_and_sharded() {
        assert_eq!(
            stem_for("https://ex.com/love/"),
            stem_for("https://ex.com/love/")
        );
        assert_ne!(stem_for("https://a.com/"), stem_for("https://b.com/"));
        let stem = stem_for("https://ex.com/");
        assert_eq!(stem.len(), 16);
        assert!(is_stem(&stem));
        assert_eq!(shard_for(&stem), &stem[..2]);
    }

    #[test]
    fn invalid_stems_never_panic_or_escape_the_shard() {
        for bad in [
            "",
            "..",
            "abc",
            "................",
            "ABCDEF0123456789",
            "éé",
        ] {
            assert!(!is_stem(bad), "{bad:?}");
            // Never a panic on a non-char-boundary slice.
            assert_eq!(shard_for(bad), "00");
        }
    }

    #[test]
    fn roundtrip_helpers() {
        let mut out = Vec::new();
        write_string(&mut out, "https://ex.com/love/?q=1");
        write_u64(&mut out, 1_700_000_000);
        out.push(7);
        let mut data = out.as_slice();
        assert_eq!(
            read_string(&mut data, 1024).as_deref(),
            Some("https://ex.com/love/?q=1")
        );
        assert_eq!(read_u64(&mut data), Some(1_700_000_000));
        assert_eq!(read_u8(&mut data), Some(7));
        assert!(data.is_empty());
    }

    #[test]
    fn overlong_lengths_fail_closed() {
        let mut bad = 10_000u32.to_le_bytes().to_vec();
        bad.extend_from_slice(&[0u8; 4]);
        let mut data = bad.as_slice();
        assert!(read_bytes(&mut data, 8).is_none());
        let mut trunc = [0u8; 3].as_slice();
        assert!(read_bytes(&mut trunc, 1024).is_none());
        let mut empty: &[u8] = &[];
        assert!(read_u64(&mut empty).is_none());
        assert!(read_u8(&mut empty).is_none());
    }
}
