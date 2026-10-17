// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache::favicons`: encrypted single-file favicon disk store.
//!
//! - All probed favicon bytes live in ONE encrypted file (`favicons.bin`,
//!   magic `MDCF`): icons are tiny (`MAX_FAVICON_BYTES` each), few in number
//!   and change rarely, so one file stays small and simple. Keyed by probe
//!   URL with full HTTP freshness, exactly like images (same record layout).
//! - The whole map is held in memory (bounded by
//!   `MAX_CACHE_FAVICON_BYTES` + `MAX_CACHE_FAVICON_ENTRIES`) and rewritten
//!   on the background flusher thread when dirty: probes land rarely (once
//!   per origin), so the rewrite cost is amortized.
//! - The session texture cache (`media::favicon`) is unchanged and stays the
//!   first lookup: disk is the second level, network the last.

use super::format::{now_secs, read_string, read_u64, read_u8, write_string, write_u64};
use super::images::{parse_image, serialize_image, ImageHit, ImageRecord};
use super::policy::CapturedHeaders;
use crate::config;
use crate::store::crypto::{decrypt, encrypt};
use std::collections::HashMap;
use std::path::PathBuf;

/// Magic/version of the encrypted favicon file.
pub(crate) const FAVICON_MAGIC: &[u8; 4] = b"MDCF";
const FAVICON_VERSION: u8 = 1;
/// On-disk map format version (inside the AEAD plaintext).
const RECORD_FORMAT: u8 = 1;

pub(crate) struct FaviconStore {
    dir: PathBuf,
    key: Option<[u8; 32]>,
    map: HashMap<String, ImageRecord>,
    bytes: u64,
    pub file_dirty: bool,
}

impl FaviconStore {
    pub(crate) fn open(dir: PathBuf, key: Option<[u8; 32]>) -> Self {
        let mut store = Self {
            dir,
            key,
            map: HashMap::new(),
            bytes: 0,
            file_dirty: false,
        };
        // Keyless (disabled): never touch the filesystem (see `DocsStore::open`).
        if key.is_some() {
            store.load();
        }
        store
    }

    fn path(&self) -> PathBuf {
        self.dir.join("favicons.bin")
    }

    fn load(&mut self) {
        let Some(key) = self.key else { return };
        let max =
            config::MAX_CACHE_FAVICON_BYTES as u64 + crate::store::crypto::OVERHEAD_BYTES as u64;
        let Some(blob) = crate::settings::read_regular_file(&self.path(), max) else {
            return;
        };
        let Some(plain) = decrypt(&key, FAVICON_MAGIC, FAVICON_VERSION, &blob) else {
            crate::settings::preserve_blob_bytes(self.dir.as_path(), "favicons.bin", &blob);
            return;
        };
        let mut data = plain.as_slice();
        if read_u8(&mut data) != Some(RECORD_FORMAT) {
            return;
        }
        let Some(count) = read_u64(&mut data) else {
            return;
        };
        if count > config::MAX_CACHE_FAVICON_ENTRIES as u64 {
            return;
        }
        let mut map = HashMap::with_capacity(count.min(256) as usize);
        let mut bytes = 0u64;
        for _ in 0..count {
            let Some(k) = read_string(&mut data, config::MAX_URL_BYTES) else {
                return;
            };
            let Some(len) = read_u64(&mut data) else {
                return;
            };
            if len > config::MAX_FAVICON_BYTES as u64 + 2048 || (data.len() as u64) < len {
                return;
            }
            let (rec_bytes, rest) = data.split_at(len as usize);
            data = rest;
            let Some(record) = parse_image(rec_bytes) else {
                return;
            };
            if record.key != k {
                continue;
            }
            if record.body.len() > config::MAX_FAVICON_BYTES {
                continue;
            }
            bytes = bytes.saturating_add(record_size(&record));
            if let Some(previous) = map.insert(k, record) {
                bytes = bytes.saturating_sub(record_size(&previous));
            }
        }
        self.map = map;
        self.bytes = bytes;
    }

    /// Serializes the whole map for the flusher thread (`None` = disabled).
    pub(crate) fn serialize(&self) -> Option<Vec<u8>> {
        let key = self.key?;
        if self.map.len() > config::MAX_CACHE_FAVICON_ENTRIES {
            return None;
        }
        let mut out = Vec::with_capacity(self.bytes.min(8 * 1024 * 1024) as usize + 64);
        out.push(RECORD_FORMAT);
        write_u64(&mut out, self.map.len() as u64);
        let mut keys: Vec<&String> = self.map.keys().collect();
        keys.sort();
        for k in keys {
            let record = &self.map[k];
            let rec = serialize_image(record);
            write_string(&mut out, k);
            write_u64(&mut out, rec.len() as u64);
            out.extend_from_slice(&rec);
        }
        if out.len() > config::MAX_CACHE_FAVICON_BYTES {
            return None;
        }
        encrypt(&key, FAVICON_MAGIC, FAVICON_VERSION, &out)
    }

    pub(crate) fn lookup(&self, key: &str, now: u64) -> Option<ImageHit> {
        self.key?;
        let record = self.map.get(key)?.clone();
        if record.fetched_url.parse::<url::Url>().is_err() {
            return None;
        }
        if record.fresh_until > now {
            Some(ImageHit::Fresh(record))
        } else {
            Some(ImageHit::Stale(record))
        }
    }

    /// Inserts or replaces one favicon (in-memory; flushed debounced).
    /// Returns the byte delta for global accounting (`None` = not stored).
    pub(crate) fn insert(&mut self, record: ImageRecord, _seq: u64) -> Option<i64> {
        self.key?;
        if record.body.len() > config::MAX_FAVICON_BYTES || record.key.len() > config::MAX_URL_BYTES
        {
            return None;
        }
        let size = record_size(&record);
        let old = self.map.get(&record.key).map(record_size).unwrap_or(0);
        self.map.insert(record.key.clone(), record);
        self.bytes = self.bytes.saturating_add(size).saturating_sub(old);
        self.file_dirty = true;
        Some(size as i64 - old as i64)
    }

    /// Removes one probe URL (FIFO eviction / clear). Returns freed bytes.
    pub(crate) fn remove(&mut self, key: &str) -> u64 {
        let Some(record) = self.map.remove(key) else {
            return 0;
        };
        let size = record_size(&record);
        self.bytes = self.bytes.saturating_sub(size);
        self.file_dirty = true;
        size
    }

    /// Drops everything (Preferences "clear favicons").
    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
        self.file_dirty = false;
        // Keyless (disabled): in-memory only, never touch the filesystem.
        if self.key.is_none() {
            return;
        }
        // Single file: delete directly (no rename dance needed for one
        // atomic file; a racing probe rewrite recreates it and the next
        // flush reconciles, worst case one stale icon until then).
        let _ = std::fs::remove_file(self.path());
        let _ = std::fs::remove_file(self.dir.join("favicons.bin.bak"));
        super::docs::sweep_temps(&self.dir);
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.map.len()
    }

    pub(crate) fn byte_total(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn ordered_keys(&self) -> Vec<(u64, String, u64)> {
        self.map
            .iter()
            .map(|(k, r)| (r.stored_at, k.clone(), record_size(r)))
            .collect()
    }

    /// Refreshes a stored probe in place after a 304. Returns the byte delta
    /// for the global accounting (`None` = no such entry): the validator
    /// fields are replaced, so the record's size (and the class total) can
    /// change without any body write.
    pub(crate) fn refresh(
        &mut self,
        key: &str,
        fresh_until: u64,
        headers: &CapturedHeaders,
    ) -> Option<i64> {
        let now = now_secs();
        let record = self.map.get_mut(key)?;
        let old = record_size(record);
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
        let new = record_size(record);
        self.bytes = self.bytes.saturating_add(new).saturating_sub(old);
        self.file_dirty = true;
        Some(new as i64 - old as i64)
    }
}

/// Serialized-size estimate for one favicon record (body + key + fixed
/// overhead), matching what [`FaviconStore::serialize`] writes.
fn record_size(record: &ImageRecord) -> u64 {
    record.body.len() as u64 + record.key.len() as u64 + 512
}

#[cfg(test)]
mod tests {
    use super::super::format::now_secs;
    use super::*;

    fn record(key: &str) -> ImageRecord {
        ImageRecord {
            key: key.to_string(),
            fetched_url: key.to_string(),
            content_type: "image/x-icon".to_string(),
            stored_at: 1_700_000_000,
            fresh_until: 1_700_000_000 + 3600,
            etag: String::new(),
            last_modified: String::new(),
            body: vec![0, 1, 2, 3],
        }
    }

    #[test]
    fn favicon_roundtrip_and_clear() {
        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-fav-test-{}.{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = FaviconStore::open(dir.clone(), Some([3u8; 32]));
        assert!(store
            .insert(record("https://ex.com/favicon.ico"), 1)
            .is_some_and(|d| d > 0));
        let hit = store
            .lookup("https://ex.com/favicon.ico", 1_700_000_100)
            .unwrap();
        assert!(matches!(hit, ImageHit::Fresh(_)));
        let blob = store.serialize().unwrap();
        assert!(crate::settings::atomic_write(&dir, "favicons.bin", &blob).is_ok());
        let reloaded = FaviconStore::open(dir.clone(), Some([3u8; 32]));
        assert_eq!(reloaded.entry_count(), 1);
        store.clear();
        assert_eq!(store.entry_count(), 0);
        assert!(!store.path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
