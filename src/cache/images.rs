// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache::images`: encrypted one-file-per-image store.
//!
//! - Every cached image is its own encrypted file
//!   (`img/<shard>/<hash>.bin`, magic `MDCI`): a 512 MiB cache of many
//!   small files stays manageable, and one corrupt file can never take down
//!   its neighbors. Sharded two levels so no directory holds 50k entries.
//! - The [`ImageIndexEntry`] map (URL -> file, sizes, expiry, validators,
//!   sequence) is persisted separately (`img-index.bin`, magic `MDIX`) and flushed
//!   debounced; body files are written synchronously by the background job
//!   thread that fetched them.
//! - Filenames are hashes of the normalized URL (never the URL itself), and
//!   the record carries its key + URL, both verified on read: a collision
//!   can only miss, never serve the wrong bytes.
//! - The same layout backs the favicon disk file through
//!   `super::favicons` (one map file instead of many single files).

use super::format::{
    is_stem, now_secs, read_bytes, read_string, read_u64, read_u8, shard_for, stem_for,
    write_bytes, write_string, write_u64,
};
use super::policy::CapturedHeaders;
use crate::config;
use crate::store::crypto::{decrypt, encrypt, OVERHEAD_BYTES};
use std::collections::HashMap;
use std::path::PathBuf;

/// Magic/version of one encrypted image file.
pub(crate) const IMAGE_MAGIC: &[u8; 4] = b"MDCI";
const IMAGE_VERSION: u8 = 1;
/// Magic/version of the encrypted image index file.
pub(crate) const IMAGE_INDEX_MAGIC: &[u8; 4] = b"MDIX";
const IMAGE_INDEX_VERSION: u8 = 1;
/// On-disk record format version (inside the AEAD plaintext).
const RECORD_FORMAT: u8 = 1;

/// One cached image (raw bytes as delivered, pre-decode).
#[derive(Debug, Clone)]
pub(crate) struct ImageRecord {
    /// Normalized URL key (`policy::resource_key`).
    pub key: String,
    /// Exact fetched URL (conditional revalidation target).
    pub fetched_url: String,
    /// Validated `Content-Type` (gate re-checked on a hit).
    pub content_type: String,
    pub stored_at: u64,
    pub fresh_until: u64,
    pub etag: String,
    pub last_modified: String,
    /// Raw response bytes (magic-sniffed + decoded on every hit, so a
    /// decoder upgrade never serves stale pixels from a new container).
    pub body: Vec<u8>,
}

/// Index entry: everything for expiry/eviction/revalidation without
/// touching the body file.
#[derive(Debug, Clone)]
pub(crate) struct ImageIndexEntry {
    /// File stem (hash of the key).
    pub stem: String,
    pub fetched_url: String,
    pub size: u64,
    pub stored_at: u64,
    pub fresh_until: u64,
    pub seq: u64,
    pub has_validator: bool,
}

/// Lookup outcome.
#[derive(Debug)]
pub(crate) enum ImageHit {
    Fresh(ImageRecord),
    Stale(ImageRecord),
}

/// One-file-per-image store. File IO runs on background job threads; the
/// caller (manager) holds the lock for index bookkeeping.
pub(crate) struct ImageStore {
    dir: PathBuf,
    key: Option<[u8; 32]>,
    index: HashMap<String, ImageIndexEntry>,
    bytes: u64,
    pub index_dirty: bool,
}

impl ImageStore {
    pub(crate) fn open(dir: PathBuf, key: Option<[u8; 32]>) -> Self {
        let mut store = Self {
            dir,
            key,
            index: HashMap::new(),
            bytes: 0,
            index_dirty: false,
        };
        // Keyless (disabled): never touch the filesystem (see `DocsStore::open`).
        if key.is_some() {
            store.load_index();
            store.sweep();
        }
        store
    }

    fn img_dir(&self) -> PathBuf {
        self.dir.join("img")
    }

    fn file_path(&self, stem: &str) -> PathBuf {
        self.img_dir()
            .join(shard_for(stem))
            .join(format!("{stem}.bin"))
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("img-index.bin")
    }

    fn load_index(&mut self) {
        let Some(key) = self.key else { return };
        let max = image_index_cap();
        let path = self.index_path();
        let Some(blob) = crate::settings::read_regular_file(&path, max) else {
            return;
        };
        let Some(plain) = decrypt(&key, IMAGE_INDEX_MAGIC, IMAGE_INDEX_VERSION, &blob) else {
            crate::settings::preserve_blob_bytes(self.dir.as_path(), "img-index.bin", &blob);
            return;
        };
        let mut data = plain.as_slice();
        if read_u8(&mut data) != Some(RECORD_FORMAT) {
            return;
        }
        let Some(count) = read_u64(&mut data) else {
            return;
        };
        if count > config::MAX_CACHE_ENTRIES as u64 {
            return;
        }
        let mut index = HashMap::with_capacity(count.min(4096) as usize);
        let mut bytes = 0u64;
        for _ in 0..count {
            let Some(k) = read_string(&mut data, config::MAX_URL_BYTES) else {
                return;
            };
            let Some(stem) = read_string(&mut data, 64) else {
                return;
            };
            let Some(fetched_url) = read_string(&mut data, config::MAX_URL_BYTES) else {
                return;
            };
            let Some(size) = read_u64(&mut data) else {
                return;
            };
            let Some(stored_at) = read_u64(&mut data) else {
                return;
            };
            let Some(fresh_until) = read_u64(&mut data) else {
                return;
            };
            let Some(seq) = read_u64(&mut data) else {
                return;
            };
            let Some(flags) = read_u8(&mut data) else {
                return;
            };
            // A stem becomes a path component: reject anything but the
            // exact 16-hex shape this cache writes.
            if !is_stem(&stem) {
                continue;
            }
            let size = size.min(config::MAX_CACHE_IMAGE_BYTES as u64 + 4096);
            bytes = bytes.saturating_add(size);
            if let Some(previous) = index.insert(
                k,
                ImageIndexEntry {
                    stem,
                    fetched_url,
                    size,
                    stored_at,
                    fresh_until,
                    seq,
                    has_validator: flags & 1 == 1,
                },
            ) {
                bytes = bytes.saturating_sub(previous.size);
            }
        }
        self.index = index;
        self.bytes = bytes;
    }

    pub(crate) fn serialize_index(&self) -> Option<Vec<u8>> {
        let key = self.key?;
        if self.index.len() > config::MAX_CACHE_ENTRIES {
            return None;
        }
        let mut out = Vec::with_capacity(self.index.len() * 200);
        out.push(RECORD_FORMAT);
        write_u64(&mut out, self.index.len() as u64);
        let mut keys: Vec<&String> = self.index.keys().collect();
        keys.sort();
        for k in keys {
            let e = &self.index[k];
            write_string(&mut out, k);
            write_string(&mut out, &e.stem);
            write_string(&mut out, &e.fetched_url);
            write_u64(&mut out, e.size);
            write_u64(&mut out, e.stored_at);
            write_u64(&mut out, e.fresh_until);
            write_u64(&mut out, e.seq);
            out.push(u8::from(e.has_validator));
        }
        encrypt(&key, IMAGE_INDEX_MAGIC, IMAGE_INDEX_VERSION, &out)
    }

    pub(crate) fn write_index_blob(&self, blob: &[u8]) -> bool {
        crate::settings::atomic_write(&self.dir, "img-index.bin", blob).is_ok()
    }

    /// Drops orphaned temps + unreferenced body files, and index entries
    /// whose body file is gone.
    fn sweep(&mut self) {
        let mut referenced = std::collections::HashSet::new();
        for e in self.index.values() {
            referenced.insert(e.stem.clone());
        }
        if let Ok(top) = std::fs::read_dir(self.img_dir()) {
            for shard in top.flatten() {
                let shard_path = shard.path();
                // Symlinks are removed, never followed (same rule as docs).
                if !super::docs::is_real_dir(&shard_path) {
                    let _ = std::fs::remove_file(&shard_path);
                    continue;
                }
                let Ok(files) = std::fs::read_dir(&shard_path) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if !super::docs::is_regular_file(&path) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    let name = file.file_name().to_string_lossy().into_owned();
                    if name.contains(".tmp.") {
                        if !super::docs::recently_modified(&path, super::docs::orphan_grace()) {
                            let _ = std::fs::remove_file(&path);
                        }
                        continue;
                    }
                    let Some(stem) = name.strip_suffix(".bin") else {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    };
                    // Same grace as documents: another instance may not have
                    // flushed its index yet.
                    if (!is_stem(stem) || !referenced.contains(stem))
                        && !super::docs::recently_modified(&path, super::docs::orphan_grace())
                    {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
        let img_dir = self.img_dir();
        let before = self.index.len();
        self.index.retain(|_, e| {
            super::docs::is_regular_file(
                &img_dir
                    .join(shard_for(&e.stem))
                    .join(format!("{}.bin", e.stem)),
            )
        });
        if self.index.len() != before {
            self.index_dirty = true;
            self.bytes = self.index.values().map(|e| e.size).sum();
        }
    }

    pub(crate) fn lookup(&mut self, key: &str, now: u64) -> Option<ImageHit> {
        self.key?;
        let entry = self.index.get(key)?.clone();
        let Some(mut record) = self.read_file(&entry.stem, key) else {
            // Missing/corrupt body or a hash collision: drop the poisoned
            // index entry instead of retrying it on every image render.
            self.remove(key);
            return None;
        };
        // The index is authoritative for freshness: a 304 updates it
        // without rewriting a multi-megabyte body (see `refresh`).
        record.stored_at = entry.stored_at;
        record.fresh_until = entry.fresh_until;
        if record.fresh_until > now {
            Some(ImageHit::Fresh(record))
        } else {
            Some(ImageHit::Stale(record))
        }
    }

    /// Inserts or replaces one image (synchronous body write on the job
    /// thread). Returns the byte delta for global accounting (`None` = not
    /// stored).
    pub(crate) fn insert(&mut self, record: ImageRecord, seq: u64) -> Option<i64> {
        let key_bytes = self.key?;
        if record.body.len() > config::MAX_CACHE_IMAGE_BYTES {
            return None;
        }
        if record.key.len() > config::MAX_URL_BYTES
            || record.fetched_url.len() > config::MAX_URL_BYTES
        {
            return None;
        }
        let stem = stem_for(&record.key);
        let blob = serialize_image(&record);
        // `read_file` reads at most `MAX_CACHE_IMAGE_BYTES + OVERHEAD + 4096`
        // encrypted bytes: a body at the cap plus long URL/validator fields
        // would be written here but rejected on every read, causing a
        // re-fetch and a full rewrite each time. Reject it instead.
        if blob.len() > config::MAX_CACHE_IMAGE_BYTES.saturating_add(4096) {
            return None;
        }
        let encrypted = encrypt(&key_bytes, IMAGE_MAGIC, IMAGE_VERSION, &blob)?;
        let path = self.file_path(&stem);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            // Runtime re-check (defense in depth): a shard directory swapped
            // for a symlink between open and this write must not redirect the
            // body outside the cache directory.
            if !super::docs::is_real_dir(parent) {
                return None;
            }
        }
        if crate::settings::atomic_write(
            path.parent().unwrap_or(&self.dir),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("img.bin"),
            &encrypted,
        )
        .is_err()
        {
            return None;
        }
        let size = blob.len() as u64 + OVERHEAD_BYTES as u64;
        let old_size = self.index.get(&record.key).map(|e| e.size).unwrap_or(0);
        self.index.insert(
            record.key.clone(),
            ImageIndexEntry {
                stem,
                fetched_url: record.fetched_url.clone(),
                size,
                stored_at: record.stored_at,
                fresh_until: record.fresh_until,
                seq,
                has_validator: !record.etag.is_empty() || !record.last_modified.is_empty(),
            },
        );
        self.bytes = self.bytes.saturating_add(size).saturating_sub(old_size);
        self.index_dirty = true;
        Some(size as i64 - old_size as i64)
    }

    /// Removes one key (FIFO eviction / clear). Returns freed bytes.
    pub(crate) fn remove(&mut self, key: &str) -> u64 {
        let Some(entry) = self.index.remove(key) else {
            return 0;
        };
        self.bytes = self.bytes.saturating_sub(entry.size);
        let _ = std::fs::remove_file(self.file_path(&entry.stem));
        self.index_dirty = true;
        entry.size
    }

    pub(crate) fn clear(&mut self) {
        // Keyless (disabled): in-memory only, never touch the filesystem.
        if self.key.is_none() {
            self.index.clear();
            self.bytes = 0;
            return;
        }
        self.index.clear();
        self.bytes = 0;
        self.index_dirty = true;
        super::docs::rename_away_and_delete(self.img_dir());
        if let Some(blob) = self.serialize_index() {
            let _ = self.write_index_blob(&blob);
            self.index_dirty = false;
        }
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.index.len()
    }

    pub(crate) fn byte_total(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn ordered_keys(&self) -> Vec<(u64, String)> {
        self.index.iter().map(|(k, e)| (e.seq, k.clone())).collect()
    }

    /// Refreshes expiry/validators after a 304. When the response repeats the
    /// stored validators and content type, only the index is updated: the
    /// body (up to `MAX_CACHE_IMAGE_BYTES`) is NOT read-modify-written. A
    /// metadata change still rewrites it, because the body carries the
    /// validators used by the next conditional request.
    pub(crate) fn refresh(
        &mut self,
        key: &str,
        fresh_until: u64,
        headers: &CapturedHeaders,
    ) -> Option<i64> {
        let now = now_secs();
        let entry = self.index.get(key)?.clone();
        let Some(mut record) = self.read_file(&entry.stem, key) else {
            self.remove(key);
            return None;
        };
        let metadata_changed = headers.etag.as_deref().is_some_and(|v| v != record.etag)
            || headers
                .last_modified
                .as_deref()
                .is_some_and(|v| v != record.last_modified)
            || headers
                .content_type
                .as_deref()
                .is_some_and(|v| v != record.content_type);
        if !metadata_changed {
            if let Some(entry) = self.index.get_mut(key) {
                entry.stored_at = now;
                entry.fresh_until = fresh_until;
            }
            self.index_dirty = true;
            return Some(0);
        }
        record.stored_at = now;
        record.fresh_until = fresh_until;
        if let Some(etag) = headers.etag.as_deref() {
            record.etag = etag.to_string();
        }
        if let Some(lm) = headers.last_modified.as_deref() {
            record.last_modified = lm.to_string();
        }
        if let Some(ct) = headers.content_type.as_deref() {
            record.content_type = ct.to_string();
        }
        let seq = entry.seq;
        if let Some(delta) = self.insert(record, seq) {
            return Some(delta);
        }
        // The body rewrite failed (disk full, read-only data dir). The
        // in-memory index still carries the refreshed expiry, exactly like
        // the old implementation; the next lookup revalidates with the old
        // validators instead of serving the entry as fresh forever.
        if let Some(entry) = self.index.get_mut(key) {
            entry.stored_at = now;
            entry.fresh_until = fresh_until;
        }
        self.index_dirty = true;
        Some(0)
    }

    fn read_file(&self, stem: &str, key: &str) -> Option<ImageRecord> {
        let key_bytes = self.key?;
        let path = self.file_path(stem);
        let max = config::MAX_CACHE_IMAGE_BYTES as u64 + OVERHEAD_BYTES as u64 + 4096;
        let blob = crate::settings::read_regular_file(&path, max)?;
        let plain = decrypt(&key_bytes, IMAGE_MAGIC, IMAGE_VERSION, &blob)?;
        let record = parse_image(&plain)?;
        // Key + URL verification: a hash collision or swapped file can only
        // miss, never serve the wrong bytes.
        if record.key != key || stem_for(&record.key) != stem {
            return None;
        }
        if record.fetched_url.parse::<url::Url>().is_err() {
            return None;
        }
        Some(record)
    }
}

pub(crate) fn serialize_image(record: &ImageRecord) -> Vec<u8> {
    let mut out = Vec::with_capacity(serialized_image_len(record));
    out.push(RECORD_FORMAT);
    write_string(&mut out, &record.key);
    write_string(&mut out, &record.fetched_url);
    write_string(&mut out, &record.content_type);
    write_u64(&mut out, record.stored_at);
    write_u64(&mut out, record.fresh_until);
    write_string(&mut out, &record.etag);
    write_string(&mut out, &record.last_modified);
    write_bytes(&mut out, &record.body);
    out
}

/// Exact byte length of [`serialize_image`]: one format byte, each field's
/// `u32` length prefix plus bytes, and the two `u64` timestamps. Shared with
/// the favicon store's quota accounting so its estimate matches the file the
/// serializer actually produces.
pub(crate) fn serialized_image_len(record: &ImageRecord) -> usize {
    const PREFIX: usize = 4;
    let string = |value: &str| PREFIX + value.len();
    1 + string(&record.key)
        + string(&record.fetched_url)
        + string(&record.content_type)
        + 8
        + 8
        + string(&record.etag)
        + string(&record.last_modified)
        + PREFIX
        + record.body.len()
}

pub(crate) fn parse_image(plain: &[u8]) -> Option<ImageRecord> {
    let mut data = plain;
    if read_u8(&mut data)? != RECORD_FORMAT {
        return None;
    }
    let key = read_string(&mut data, config::MAX_URL_BYTES)?;
    let fetched_url = read_string(&mut data, config::MAX_URL_BYTES)?;
    let content_type = read_string(&mut data, config::MAX_CACHE_RECORD_HEADER_CHARS)?;
    let stored_at = read_u64(&mut data)?;
    let fresh_until = read_u64(&mut data)?;
    let etag = read_string(&mut data, 1024)?;
    let last_modified = read_string(&mut data, config::MAX_CACHE_RECORD_HEADER_CHARS)?;
    let body = read_bytes(&mut data, config::MAX_CACHE_IMAGE_BYTES)?.to_vec();
    Some(ImageRecord {
        key,
        fetched_url,
        content_type,
        stored_at,
        fresh_until,
        etag,
        last_modified,
        body,
    })
}

fn image_index_cap() -> u64 {
    (config::MAX_CACHE_ENTRIES as u64)
        .saturating_mul(512)
        .saturating_add(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: &str) -> ImageRecord {
        ImageRecord {
            key: key.to_string(),
            fetched_url: key.to_string(),
            content_type: "image/png".to_string(),
            stored_at: 1_700_000_000,
            fresh_until: 1_700_000_000 + 3600,
            etag: "\"v1\"".to_string(),
            last_modified: String::new(),
            body: vec![137, 80, 78, 71],
        }
    }

    #[test]
    fn image_file_roundtrip() {
        let rec = record("https://ex.com/a.png");
        let blob = serialize_image(&rec);
        let parsed = parse_image(&blob).unwrap();
        assert_eq!(parsed.body, vec![137, 80, 78, 71]);
        assert_eq!(parsed.etag, "\"v1\"");
        let mut bad = blob.clone();
        bad[0] = 42;
        assert!(parse_image(&bad).is_none());
    }

    /// The record reader enforces the same header caps the capture path
    /// writes; a maximum-length value must survive a round trip.
    #[test]
    fn persisted_headers_at_the_cap_roundtrip() {
        let mut rec = record("https://ex.com/a.png");
        rec.content_type = "t".repeat(config::MAX_CACHE_RECORD_HEADER_CHARS);
        rec.last_modified = "l".repeat(config::MAX_CACHE_RECORD_HEADER_CHARS);
        let blob = serialize_image(&rec);
        let parsed = parse_image(&blob).expect("parses");
        assert_eq!(parsed.content_type, rec.content_type);
        assert_eq!(parsed.last_modified, rec.last_modified);
        assert_eq!(blob.len(), serialized_image_len(&rec));
    }

    /// A file whose decrypted key does not match its filename (hash
    /// collision, swapped file, targeted tampering) is a silent miss — it
    /// can never serve another URL's bytes.
    #[test]
    fn key_mismatch_is_a_miss_never_a_mixup() {
        use super::super::format::stem_for;
        use crate::store::crypto::encrypt;

        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-img-collision-{}.{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let key = [11u8; 32];
        let mut store = ImageStore::open(dir.clone(), Some(key));
        store.insert(record("https://ex.com/a.png"), 1);
        assert!(store
            .lookup("https://ex.com/a.png", 1_700_000_100)
            .is_some());
        // Overwrite the file at a.png's slot with a VALIDLY encrypted record
        // for a different URL (the worst case a collision can produce).
        let hostile = ImageRecord {
            key: "https://evil.test/x.png".to_string(),
            ..record("https://evil.test/x.png")
        };
        let blob = serialize_image(&hostile);
        let encrypted = encrypt(&key, IMAGE_MAGIC, IMAGE_VERSION, &blob).unwrap();
        let stem = stem_for("https://ex.com/a.png");
        let path = store.file_path(&stem);
        crate::settings::atomic_write(
            path.parent().unwrap(),
            path.file_name().and_then(|n| n.to_str()).unwrap(),
            &encrypted,
        )
        .unwrap();
        assert!(store
            .lookup("https://ex.com/a.png", 1_700_000_100)
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn insert_lookup_evict_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-img-test-{}.{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = ImageStore::open(dir.clone(), Some([9u8; 32]));
        let delta = store.insert(record("https://ex.com/a.png"), 1);
        assert!(delta.is_some_and(|d| d > 0));
        let hit = store.lookup("https://ex.com/a.png", 1_700_000_100).unwrap();
        assert!(matches!(hit, ImageHit::Fresh(_)));
        let stale = store.lookup("https://ex.com/a.png", 1_800_000_000).unwrap();
        assert!(matches!(stale, ImageHit::Stale(_)));
        assert!(store
            .lookup("https://ex.com/b.png", 1_700_000_100)
            .is_none());
        let freed = store.remove("https://ex.com/a.png");
        assert!(freed > 0);
        assert_eq!(store.entry_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A 304 that repeats the stored validators must refresh the index only:
    /// the body file (up to 16 MiB) stays byte-for-byte identical, and the
    /// index is authoritative for the new expiry.
    #[test]
    fn unchanged_validators_refresh_without_rewriting_the_body() {
        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-img-refresh-{}.{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = ImageStore::open(dir.clone(), Some([9u8; 32]));
        let rec = record("https://ex.com/a.png");
        store.insert(rec, 1).expect("stored");
        let path = store.file_path(&stem_for("https://ex.com/a.png"));
        let before = std::fs::read(&path).expect("body");
        let headers = CapturedHeaders {
            etag: Some("\"v1\"".to_string()),
            content_type: Some("image/png".to_string()),
            ..Default::default()
        };
        let fresh_until = 1_700_003_900;
        assert_eq!(
            store.refresh("https://ex.com/a.png", fresh_until, &headers),
            Some(0)
        );
        let after = std::fs::read(&path).expect("body");
        assert_eq!(before, after, "the body must not be rewritten");
        match store
            .lookup("https://ex.com/a.png", 1_700_003_700)
            .expect("hit")
        {
            ImageHit::Fresh(hit) => assert_eq!(hit.fresh_until, fresh_until),
            ImageHit::Stale(_) => panic!("index refresh must make the entry fresh"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
