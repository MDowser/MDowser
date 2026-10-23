// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache::docs`: encrypted per-domain document store.
//!
//! - One encrypted file per host (`docs/<shard>/<hash>.bin`, magic `MDCD`):
//!   every canonical spelling of a page (`/love`, `/love/`, `/love.md`,
//!   ...) shares the canonical [`doc_key`](super::policy::doc_key), so one
//!   record matches them all. A host file never exceeds
//!   `MAX_CACHE_DOMAIN_BYTES` (oldest entries of that host go first).
//! - The [`DocIndexEntry`] map (key -> host file, sizes, expiry,
//!   validators, sequence) is persisted separately (`docs-index.bin`,
//!   magic `MDDX`) and flushed debounced; body files are written
//!   synchronously by the background job thread that fetched them.
//! - Filenames are hashes: a cleartext hostname would leak history to
//!   anyone listing the directory. The record carries its key + host, both
//!   verified on read, so a hash collision can only miss, never mix pages.
//! - FIFO eviction is driven by the manager (sequence order); this store
//!   only inserts, reads and removes.

use super::format::{
    is_stem, now_secs, read_string, read_u64, read_u8, shard_for, stem_for, write_string, write_u64,
};
use super::policy::{doc_host_key, CapturedHeaders};
use crate::config;
use crate::store::crypto::{decrypt, encrypt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// Magic/version of one encrypted per-host document file.
pub(crate) const DOCS_MAGIC: &[u8; 4] = b"MDCD";
const DOCS_VERSION: u8 = 1;
/// Magic/version of the encrypted document index file.
pub(crate) const DOCS_INDEX_MAGIC: &[u8; 4] = b"MDDX";
const DOCS_INDEX_VERSION: u8 = 1;
/// On-disk record format version (inside the AEAD plaintext).
const RECORD_FORMAT: u8 = 1;
/// Hard cap on entries per host file (parse bomb guard; the byte cap trips
/// far earlier on real bodies).
const MAX_ENTRIES_PER_HOST_FILE: usize = 20_000;

/// One cached document.
#[derive(Debug, Clone)]
pub(crate) struct DocRecord {
    /// Canonical key (`policy::doc_key`): matches every URL spelling.
    pub key: String,
    /// Exact candidate URL that produced the body (conditional revalidation
    /// goes here, not to an arbitrary spelling).
    pub fetched_url: String,
    /// Landed display URL (what history/address show; rebuilt via `finish_document`).
    pub final_url: String,
    /// Validated `Content-Type` (kept, so a revalidation mismatch is caught).
    pub content_type: String,
    /// Unix seconds of the store/revalidation.
    pub stored_at: u64,
    /// Unix seconds until which the entry serves without revalidation.
    pub fresh_until: u64,
    /// Validators for conditional requests (empty = absent).
    pub etag: String,
    pub last_modified: String,
    /// Markdown source (already UTF-8 validated at fetch time).
    pub body: String,
}

/// Index entry: everything needed for expiry/eviction/revalidation WITHOUT
/// decrypting the host file.
#[derive(Debug, Clone)]
pub(crate) struct DocIndexEntry {
    /// Host-file stem (hash of the host key).
    pub host_stem: String,
    /// Exact candidate URL for conditional revalidation.
    pub fetched_url: String,
    /// Approximate entry bytes (serialized record length).
    pub size: u64,
    pub stored_at: u64,
    pub fresh_until: u64,
    /// Global FIFO sequence (assigned by the manager).
    pub seq: u64,
    pub has_validator: bool,
}

/// Lookup outcome.
#[derive(Debug)]
pub(crate) enum DocHit {
    /// Fresh (or `no-cache`-free): serve immediately.
    Fresh(DocRecord),
    /// Expired: needs a conditional revalidation (`etag`/`last_modified`
    /// may both be empty when the response carried no validators).
    Stale(DocRecord),
}

/// Per-domain document store (index + host files). All methods assume the
/// caller holds the manager lock except file IO, which runs on background
/// job threads.
pub(crate) struct DocsStore {
    dir: PathBuf,
    key: Option<[u8; 32]>,
    index: HashMap<String, DocIndexEntry>,
    /// Recently used bodies (fast path for Back/Forward without disk IO).
    ram: HashMap<String, DocRecord>,
    ram_bytes: usize,
    ram_order: std::collections::VecDeque<String>,
    /// Class byte total (serialized record bytes, conservative).
    bytes: u64,
    /// Host files rewritten since the last index flush (the index flush
    /// rewrites the index file, not the host files: those are synchronous).
    pub index_dirty: bool,
}

impl DocsStore {
    pub(crate) fn open(dir: PathBuf, key: Option<[u8; 32]>) -> Self {
        let mut store = Self {
            dir,
            key,
            index: HashMap::new(),
            ram: HashMap::new(),
            ram_bytes: 0,
            ram_order: std::collections::VecDeque::new(),
            bytes: 0,
            index_dirty: false,
        };
        // A keyless (disabled) store never touches the filesystem: with an
        // empty dir the host path would resolve relative to the process CWD,
        // and a sweep there could delete unrelated files. All lookups miss
        // and all stores are no-ops without a key anyway.
        if key.is_some() {
            store.load_index();
            store.sweep();
        }
        store
    }

    fn host_dir(&self) -> PathBuf {
        self.dir.join("docs")
    }

    fn host_path(&self, stem: &str) -> PathBuf {
        self.host_dir()
            .join(shard_for(stem))
            .join(format!("{stem}.bin"))
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("docs-index.bin")
    }

    /// Loads the encrypted index (bounded; corrupt/missing = empty).
    fn load_index(&mut self) {
        let Some(key) = self.key else { return };
        let max = docs_index_cap();
        let path = self.index_path();
        let Some(blob) = crate::settings::read_regular_file(&path, max) else {
            return;
        };
        let Some(plain) = decrypt(&key, DOCS_INDEX_MAGIC, DOCS_INDEX_VERSION, &blob) else {
            crate::settings::preserve_blob_bytes(self.dir.as_path(), "docs-index.bin", &blob);
            return;
        };
        let mut data = plain.as_slice();
        if read_u8(&mut data) != Some(RECORD_FORMAT) {
            return;
        }
        let Some(next) = read_u64(&mut data) else {
            return;
        };
        let _ = next; // manager owns the sequence counter; see `CacheManager`.
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
            let Some(host_stem) = read_string(&mut data, 64) else {
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
            // A stem becomes a path component: anything but the exact
            // 16-hex shape this cache writes is a poisoned index entry.
            if !is_stem(&host_stem) {
                continue;
            }
            // Sizes are advisory accounting: clamp absurd values instead of
            // trusting a (decrypted, but possibly hand-crafted) file.
            let size = size.min(config::MAX_BODY_BYTES as u64 + 4096);
            bytes = bytes.saturating_add(size);
            if let Some(previous) = index.insert(
                k,
                DocIndexEntry {
                    host_stem,
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

    /// Serializes the index for the debounced flush (`None` = disabled).
    pub(crate) fn serialize_index(&self) -> Option<Vec<u8>> {
        let key = self.key?;
        if self.index.len() > config::MAX_CACHE_ENTRIES {
            return None;
        }
        let mut out = Vec::with_capacity(self.index.len() * 200);
        out.push(RECORD_FORMAT);
        write_u64(&mut out, 0); // reserved: the manager owns `next_seq`.
        write_u64(&mut out, self.index.len() as u64);
        // Deterministic order (by key) so flushes are reproducible.
        let mut keys: Vec<&String> = self.index.keys().collect();
        keys.sort();
        for k in keys {
            let e = &self.index[k];
            write_string(&mut out, k);
            write_string(&mut out, &e.host_stem);
            write_string(&mut out, &e.fetched_url);
            write_u64(&mut out, e.size);
            write_u64(&mut out, e.stored_at);
            write_u64(&mut out, e.fresh_until);
            write_u64(&mut out, e.seq);
            out.push(u8::from(e.has_validator));
        }
        encrypt(&key, DOCS_INDEX_MAGIC, DOCS_INDEX_VERSION, &out)
    }

    /// Writes the serialized index to disk (flusher thread).
    pub(crate) fn write_index_blob(&self, blob: &[u8]) -> bool {
        crate::settings::atomic_write(&self.dir, "docs-index.bin", blob).is_ok()
    }

    /// Removes crash-orphaned temp files and unreferenced host files, and
    /// drops index entries whose host file is gone (crash between body
    /// write and index flush, or a manual delete).
    fn sweep(&mut self) {
        let mut referenced = std::collections::HashSet::new();
        for e in self.index.values() {
            referenced.insert(e.host_stem.clone());
        }
        let mut missing_hosts = std::collections::HashSet::new();
        if let Ok(top) = std::fs::read_dir(self.host_dir()) {
            for shard in top.flatten() {
                let shard_path = shard.path();
                // Symlinks are removed, never followed: a planted link must
                // not let the startup sweep walk (or delete) outside cache/.
                if !is_real_dir(&shard_path) {
                    let _ = std::fs::remove_file(&shard_path);
                    continue;
                }
                let Ok(files) = std::fs::read_dir(&shard_path) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if !is_regular_file(&path) {
                        // A symlink/FIFO/directory in a shard is not ours:
                        // remove the entry itself (remove_file never follows).
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    let name = file.file_name().to_string_lossy().into_owned();
                    if name.contains(".tmp.") {
                        if !recently_modified(&path, orphan_grace()) {
                            let _ = std::fs::remove_file(&path);
                        }
                        continue;
                    }
                    let Some(stem) = name.strip_suffix(".bin") else {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    };
                    // Another instance may have written the body but not yet
                    // flushed the debounced index; an orphan must be old
                    // enough to be provably dead before it is deleted.
                    if (!is_stem(stem) || !referenced.contains(stem))
                        && !recently_modified(&path, orphan_grace())
                    {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
        // Drop entries whose host file vanished (and their RAM copies).
        let host_dir = self.host_dir();
        self.index.retain(|_, e| {
            let keep = is_regular_file(&host_path_for(&host_dir, &e.host_stem));
            if !keep {
                missing_hosts.insert(e.host_stem.clone());
            }
            keep
        });
        if !missing_hosts.is_empty() {
            self.index_dirty = true;
            self.bytes = self.index.values().map(|e| e.size).sum();
            self.ram.retain(|k, _| self.index.contains_key(k));
            self.recount_ram();
        }
    }

    /// Looks up a canonical key. Returns `Fresh` (serve) or `Stale`
    /// (revalidate). File mismatches (hash collision, corrupt entry) are
    /// silent misses that also drop the poisoned index entry.
    pub(crate) fn lookup(&mut self, key: &str, now: u64) -> Option<DocHit> {
        self.key?;
        let host_stem = self.index.get(key)?.host_stem.clone();
        // Fast path: RAM body with a live index entry.
        if let Some(record) = self.ram.get(key) {
            if record.key == key {
                let record = record.clone();
                self.ram_order.retain(|k| k != key);
                self.ram_order.push_back(key.to_string());
                return Some(classify(record, now));
            }
            self.ram_remove(key);
        }
        let Some(record) = self.read_host_entry(&host_stem, key) else {
            // A missing/corrupt host file (or a hash collision) can never
            // serve: drop the poisoned index entry instead of retrying it
            // on every navigation.
            self.drop_entry(key);
            return None;
        };
        self.ram_insert(record.clone());
        Some(classify(record, now))
    }

    /// Inserts or replaces a record (writes the host file synchronously;
    /// the caller runs on a background job thread). Returns the NET byte
    /// delta for the global accounting — the new size minus the replaced
    /// size minus every byte this insert evicted from the host file — plus
    /// the keys the host cap evicted (`None` = not stored): the manager
    /// drops evicted keys from the global FIFO too, so the order never holds
    /// phantoms. Over-cap hosts evict their own oldest first.
    pub(crate) fn insert(&mut self, record: DocRecord, seq: u64) -> Option<(i64, Vec<String>)> {
        let key_bytes = self.key?;
        let host_key = record
            .fetched_url
            .parse::<Url>()
            .ok()
            .and_then(|u| doc_host_key(&u))?;
        if record.body.len() > config::MAX_BODY_BYTES {
            return None;
        }
        let stem = stem_for(&host_key);
        // Read-modify-write the host file (bounded; corrupt = start fresh).
        let mut entries = self.read_host_file(&stem).unwrap_or_default();
        entries.retain(|r| r.key != record.key);
        let size = serialized_doc_len(&record) as u64;
        entries.push(record.clone());
        // Oldest-first while the host file is over its own cap. Index
        // removals apply only AFTER the rewritten file lands: a failed
        // write below must leave the previous file + index consistent.
        entries.sort_by_key(|r| r.stored_at);
        let (evicted, evicted_index_bytes) = evict_over_host_cap(
            &mut entries,
            &self.index,
            &stem,
            config::MAX_CACHE_DOMAIN_BYTES as u64,
        );
        let blob = serialize_host_file(&entries);
        let encrypted = encrypt(&key_bytes, DOCS_MAGIC, DOCS_VERSION, &blob)?;
        let path = self.host_path(&stem);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            // Runtime re-check (defense in depth): a shard directory swapped
            // for a symlink between open and this write must not redirect the
            // body outside the cache directory.
            if !is_real_dir(parent) {
                return None;
            }
        }
        if crate::settings::atomic_write(
            path.parent().unwrap_or(&self.dir),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("docs.bin"),
            &encrypted,
        )
        .is_err()
        {
            return None;
        }
        // Commit the index entry (replaces any previous size accounting),
        // then the host-cap evictions: the file already landed, so the index
        // follows it atomically from the reader's point of view.
        let old_size = self.index.get(&record.key).map(|e| e.size).unwrap_or(0);
        self.index.insert(
            record.key.clone(),
            DocIndexEntry {
                host_stem: stem,
                fetched_url: record.fetched_url.clone(),
                size,
                stored_at: record.stored_at,
                fresh_until: record.fresh_until,
                seq,
                has_validator: !record.etag.is_empty() || !record.last_modified.is_empty(),
            },
        );
        self.bytes = self.bytes.saturating_add(size).saturating_sub(old_size);
        for old_key in &evicted {
            if let Some(e) = self.index.remove(old_key) {
                self.bytes = self.bytes.saturating_sub(e.size);
            }
            self.ram_remove(old_key);
        }
        self.index_dirty = true;
        self.ram_insert(record);
        Some((
            size as i64 - old_size as i64 - evicted_index_bytes as i64,
            evicted,
        ))
    }

    /// Removes one key (FIFO eviction / clear). Returns freed bytes.
    pub(crate) fn remove(&mut self, key: &str) -> u64 {
        let Some(entry) = self.index.remove(key) else {
            return 0;
        };
        self.bytes = self.bytes.saturating_sub(entry.size);
        self.ram_remove(key);
        // Rewrite the host file without the entry (best-effort: a failure
        // leaves the bytes on disk, but the index no longer references them
        // and the next sweep drops the orphan).
        if let Some(mut entries) = self.read_host_file(&entry.host_stem) {
            let before = entries.len();
            entries.retain(|r| r.key != key);
            if entries.len() != before {
                if entries.is_empty() {
                    let _ = std::fs::remove_file(self.host_path(&entry.host_stem));
                } else if let Some(key_bytes) = self.key {
                    let blob = serialize_host_file(&entries);
                    if let Some(encrypted) = encrypt(&key_bytes, DOCS_MAGIC, DOCS_VERSION, &blob) {
                        let path = self.host_path(&entry.host_stem);
                        let _ = crate::settings::atomic_write(
                            path.parent().unwrap_or(&self.dir),
                            path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("docs.bin"),
                            &encrypted,
                        );
                    }
                }
            }
        }
        self.index_dirty = true;
        entry.size
    }

    /// Drops everything (Preferences "clear documents"): in-memory state now,
    /// files via atomic rename + background delete (a racing job thread can
    /// never have its fresh file deleted from under it).
    pub(crate) fn clear(&mut self) {
        // Keyless (disabled): in-memory only, never touch the filesystem.
        if self.key.is_none() {
            self.index.clear();
            self.ram.clear();
            self.ram_bytes = 0;
            self.ram_order.clear();
            self.bytes = 0;
            return;
        }
        self.index.clear();
        self.ram.clear();
        self.ram_bytes = 0;
        self.ram_order.clear();
        self.bytes = 0;
        self.index_dirty = true;
        // The index file itself is rewritten on the next flush (now empty);
        // host files move away atomically.
        rename_away_and_delete(self.host_dir());
        // Write the empty index synchronously: a crash right after "clear"
        // must not resurrect entries.
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

    /// All index keys with their sequence (for the manager's global FIFO).
    pub(crate) fn ordered_keys(&self) -> Vec<(u64, String)> {
        self.index.iter().map(|(k, e)| (e.seq, k.clone())).collect()
    }

    /// Refreshes an entry's expiry/validators after a 304 (rewrites the host
    /// file; the body is unchanged). Returns the NET byte delta for the
    /// global accounting plus the host-cap-evicted keys (`None` = no such
    /// entry; see [`Self::insert`]).
    pub(crate) fn refresh(
        &mut self,
        key: &str,
        fresh_until: u64,
        headers: &CapturedHeaders,
    ) -> Option<(i64, Vec<String>)> {
        let entry = self.index.get(key).cloned()?;
        let mut record = self.read_host_entry(&entry.host_stem, key)?;
        let now = now_secs();
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
        self.insert(record, seq)
    }

    /// Drops one index entry whose body can no longer be read (corrupt host
    /// file, hash collision, deleted shard). The file itself is left for the
    /// next sweep: `remove` would rewrite the same unreadable host file.
    fn drop_entry(&mut self, key: &str) {
        if let Some(entry) = self.index.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.size);
        }
        self.ram_remove(key);
        self.index_dirty = true;
    }

    fn read_host_file(&self, stem: &str) -> Option<Vec<DocRecord>> {
        let key = self.key?;
        let path = self.host_path(stem);
        let max =
            config::MAX_CACHE_DOMAIN_BYTES as u64 + crate::store::crypto::OVERHEAD_BYTES as u64;
        let blob = crate::settings::read_regular_file(&path, max)?;
        let plain = decrypt(&key, DOCS_MAGIC, DOCS_VERSION, &blob)?;
        parse_host_file(&plain, stem)
    }

    fn read_host_entry(&self, stem: &str, key: &str) -> Option<DocRecord> {
        let entries = self.read_host_file(stem)?;
        entries.into_iter().find(|r| r.key == key)
    }

    fn ram_insert(&mut self, record: DocRecord) {
        // The RAM fast path is bounded (`CACHE_RAM_DOC_BYTES`), FIFO: a
        // gallery of huge documents cannot grow it without bound.
        self.ram_remove(&record.key);
        let size = record.body.len().saturating_add(512);
        while self.ram_bytes.saturating_add(size) > config::CACHE_RAM_DOC_BYTES
            && !self.ram_order.is_empty()
        {
            if let Some(old) = self.ram_order.pop_front() {
                if let Some(r) = self.ram.remove(&old) {
                    self.ram_bytes = self
                        .ram_bytes
                        .saturating_sub(r.body.len().saturating_add(512));
                }
            } else {
                break;
            }
        }
        self.ram_bytes = self.ram_bytes.saturating_add(size);
        self.ram_order.push_back(record.key.clone());
        self.ram.insert(record.key.clone(), record);
    }

    fn ram_remove(&mut self, key: &str) {
        if let Some(r) = self.ram.remove(key) {
            self.ram_bytes = self
                .ram_bytes
                .saturating_sub(r.body.len().saturating_add(512));
            self.ram_order.retain(|k| k != key);
        }
    }

    fn recount_ram(&mut self) {
        self.ram_bytes = self
            .ram
            .values()
            .map(|r| r.body.len().saturating_add(512))
            .sum();
        self.ram_order.retain(|k| self.ram.contains_key(k));
    }
}

fn classify(record: DocRecord, now: u64) -> DocHit {
    if record.fresh_until > now {
        DocHit::Fresh(record)
    } else {
        DocHit::Stale(record)
    }
}

fn host_path_for(host_dir: &Path, stem: &str) -> PathBuf {
    host_dir.join(shard_for(stem)).join(format!("{stem}.bin"))
}

/// `true` only for a real directory (never a symlink to one).
pub(crate) fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

/// `true` only for a real regular file (never a symlink/FIFO/device).
pub(crate) fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

/// Age after which an unreferenced body/temp file is provably dead. Covers
/// the debounced index flush window of a concurrently running instance.
pub(crate) fn orphan_grace() -> Duration {
    Duration::from_secs(
        config::CACHE_INDEX_FLUSH_DEBOUNCE_SECS
            .saturating_mul(2)
            .max(60),
    )
}

/// `true` when the file was modified less than `max_age` ago (a live writer
/// of another instance). Missing metadata counts as old.
pub(crate) fn recently_modified(path: &Path, max_age: Duration) -> bool {
    std::fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < max_age)
}

fn docs_index_cap() -> u64 {
    // 50k entries x ~300 B worst case, plus headroom.
    (config::MAX_CACHE_ENTRIES as u64)
        .saturating_mul(512)
        .saturating_add(1024)
}

/// Removes crash-orphaned `*.tmp.*` files directly inside `dir` (no
/// recursion: each store sweeps its own shards). Symlinks are removed as
/// entries and never followed (a planted link must not make startup walk
/// outside `cache/`). This is the cheap root sweep for the index/favicon
/// files; document and image shards are handled by their store sweeps.
pub(crate) fn sweep_temps(dir: &Path) {
    let Ok(top) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in top.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(".tmp.") && !recently_modified(&path, orphan_grace()) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Atomically renames a cache directory away and deletes it on a background
/// thread (the "clear" path never blocks the UI on 50k files, and a racing
/// writer recreates the live directory instead of losing its file).
pub(crate) fn rename_away_and_delete(dir: PathBuf) {
    let Some(parent) = dir.parent().map(Path::to_path_buf) else {
        return;
    };
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let away = parent.join(format!(
        "{name}.clear.{}.{}",
        std::process::id(),
        now_secs()
    ));
    // `rename` fails when the directory does not exist: nothing to delete.
    if std::fs::rename(&dir, &away).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("mdowser-cache-clear".into())
        .spawn(move || {
            let _ = std::fs::remove_dir_all(&away);
        })
        .ok();
}

/// Oldest-first host-cap eviction (pure, unit-testable): drops entries from
/// the front of `entries` while their serialized total exceeds `cap` or the
/// file would exceed `MAX_ENTRIES_PER_HOST_FILE`, always keeping the newest
/// record. Returns the INDEX-charged keys removed and the bytes they were
/// charged, so the manager can shrink the global total by them (entries of
/// another stem are poisoned records: dropped from the rewritten file,
/// never charged).
fn evict_over_host_cap(
    entries: &mut Vec<DocRecord>,
    index: &HashMap<String, DocIndexEntry>,
    stem: &str,
    cap: u64,
) -> (Vec<String>, u64) {
    let mut evicted: Vec<String> = Vec::new();
    let mut evicted_index_bytes: u64 = 0;
    let mut host_bytes: u64 = entries.iter().map(|r| serialized_doc_len(r) as u64).sum();
    // Find the prefix to drop first: `Vec::remove(0)` in the loop made an
    // over-cap host file quadratic in its entry count.
    let mut drop_count = 0usize;
    while (host_bytes > cap || entries.len() - drop_count > MAX_ENTRIES_PER_HOST_FILE)
        && entries.len() - drop_count > 1
    {
        host_bytes = host_bytes.saturating_sub(serialized_doc_len(&entries[drop_count]) as u64);
        drop_count += 1;
    }
    for old in entries.drain(..drop_count) {
        if let Some(entry) = index.get(&old.key).filter(|e| e.host_stem == stem) {
            evicted_index_bytes = evicted_index_bytes.saturating_add(entry.size);
            evicted.push(old.key);
        }
    }
    (evicted, evicted_index_bytes)
}

fn serialized_doc_len(record: &DocRecord) -> usize {
    record.key.len()
        + record.fetched_url.len()
        + record.final_url.len()
        + record.content_type.len()
        + record.etag.len()
        + record.last_modified.len()
        + record.body.len()
        + 64
}

fn serialize_host_file(entries: &[DocRecord]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(RECORD_FORMAT);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for r in entries {
        write_string(&mut out, &r.key);
        write_string(&mut out, &r.fetched_url);
        write_string(&mut out, &r.final_url);
        write_string(&mut out, &r.content_type);
        write_u64(&mut out, r.stored_at);
        write_u64(&mut out, r.fresh_until);
        write_string(&mut out, &r.etag);
        write_string(&mut out, &r.last_modified);
        write_string(&mut out, &r.body);
    }
    out
}

fn parse_host_file(plain: &[u8], stem: &str) -> Option<Vec<DocRecord>> {
    let mut data = plain;
    if read_u8(&mut data)? != RECORD_FORMAT {
        return None;
    }
    if data.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    data = &data[4..];
    if count > MAX_ENTRIES_PER_HOST_FILE {
        return None;
    }
    let mut entries = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let key = read_string(&mut data, config::MAX_URL_BYTES)?;
        let fetched_url = read_string(&mut data, config::MAX_URL_BYTES)?;
        let final_url = read_string(&mut data, config::MAX_URL_BYTES)?;
        let content_type = read_string(&mut data, 256)?;
        let stored_at = read_u64(&mut data)?;
        let fresh_until = read_u64(&mut data)?;
        let etag = read_string(&mut data, 1024)?;
        let last_modified = read_string(&mut data, 256)?;
        let body = read_string(&mut data, config::MAX_BODY_BYTES)?;
        // The record must belong to this host file: a swapped/colliding
        // file can never serve another host's page.
        let expected = fetched_url
            .parse::<Url>()
            .ok()
            .and_then(|u| doc_host_key(&u))
            .map(|h| stem_for(&h));
        if expected.as_deref() != Some(stem) {
            continue;
        }
        // The stored final URL must still parse (it is re-fetched through
        // `finish_document` on a hit).
        if final_url.parse::<Url>().is_err() {
            continue;
        }
        entries.push(DocRecord {
            key,
            fetched_url,
            final_url,
            content_type,
            stored_at,
            fresh_until,
            etag,
            last_modified,
            body,
        });
    }
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: &str, host: &str) -> DocRecord {
        DocRecord {
            key: key.to_string(),
            fetched_url: format!("https://{host}/love.md"),
            final_url: format!("https://{host}/love/"),
            content_type: "text/markdown".to_string(),
            stored_at: 1_700_000_000,
            fresh_until: 1_700_000_000 + 300,
            etag: String::new(),
            last_modified: String::new(),
            body: "# Love\n".to_string(),
        }
    }

    #[test]
    fn host_file_roundtrip_with_stem_check() {
        let entries = vec![record("https://ex.com/love", "ex.com")];
        let blob = serialize_host_file(&entries);
        let stem = stem_for("ex.com");
        let parsed = parse_host_file(&blob, &stem).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].body, "# Love\n");
        // A record smuggled into another host's file is skipped.
        let parsed = parse_host_file(&blob, &stem_for("evil.com")).unwrap();
        assert!(parsed.is_empty());
        // Bad version rejected.
        let mut bad = blob.clone();
        bad[0] = 99;
        assert!(parse_host_file(&bad, &stem).is_none());
    }

    /// Regression (global accounting): the host-cap eviction must report the
    /// index-charged bytes it drops, so `CacheManager` can subtract them
    /// from the global total. Before this, every over-cap insert left
    /// phantom bytes behind, which later evicted unrelated entries.
    #[test]
    fn host_cap_eviction_reports_charged_bytes() {
        let stem = stem_for("ex.com");
        let mut entries: Vec<DocRecord> = ["one", "two", "three"]
            .into_iter()
            .map(|n| record(&format!("https://ex.com/{n}"), "ex.com"))
            .collect();
        for (i, r) in entries.iter_mut().enumerate() {
            r.stored_at = i as u64 + 1;
        }
        let sizes: Vec<u64> = entries
            .iter()
            .map(|r| serialized_doc_len(r) as u64)
            .collect();
        let mut index: HashMap<String, DocIndexEntry> = HashMap::new();
        for (i, r) in entries.iter().enumerate() {
            index.insert(
                r.key.clone(),
                DocIndexEntry {
                    host_stem: stem.clone(),
                    fetched_url: r.fetched_url.clone(),
                    size: sizes[i],
                    stored_at: r.stored_at,
                    fresh_until: 0,
                    seq: i as u64,
                    has_validator: false,
                },
            );
        }
        // A cap that fits exactly the two newest records: the oldest one
        // (whose full index-charged size is reported) is evicted.
        let cap = sizes[1] + sizes[2];
        let (evicted, bytes) = evict_over_host_cap(&mut entries, &index, &stem, cap);
        assert_eq!(evicted, vec!["https://ex.com/one".to_string()]);
        assert_eq!(bytes, sizes[0]);
        assert_eq!(entries.len(), 2);
        // A poisoned record of ANOTHER stem is dropped from the file but
        // never reported as freed: it was never part of the global total.
        let mut foreign = record("https://evil.com/x", "evil.com");
        foreign.stored_at = 0;
        let mut entries = vec![foreign, entries.remove(0)];
        let (evicted, bytes) = evict_over_host_cap(&mut entries, &index, &stem, 0);
        assert!(evicted.is_empty());
        assert_eq!(bytes, 0);
        assert_eq!(entries.len(), 1);
    }

    /// Regression: the writer must enforce the same entry cap the READER
    /// enforces. A host file over `MAX_ENTRIES_PER_HOST_FILE` failed to
    /// parse, so the next insert silently restarted the file with one record
    /// and every cached document for that host was lost.
    #[test]
    fn host_cap_eviction_enforces_the_entry_limit() {
        let stem = stem_for("ex.com");
        let mut entries: Vec<DocRecord> = (0..=MAX_ENTRIES_PER_HOST_FILE)
            .map(|i| record(&format!("https://ex.com/page-{i}"), "ex.com"))
            .collect();
        for (i, r) in entries.iter_mut().enumerate() {
            r.stored_at = i as u64 + 1;
        }
        let mut index: HashMap<String, DocIndexEntry> = HashMap::new();
        for (i, r) in entries.iter().enumerate() {
            index.insert(
                r.key.clone(),
                DocIndexEntry {
                    host_stem: stem.clone(),
                    fetched_url: r.fetched_url.clone(),
                    size: serialized_doc_len(r) as u64,
                    stored_at: r.stored_at,
                    fresh_until: 0,
                    seq: i as u64,
                    has_validator: false,
                },
            );
        }
        let (evicted, _) = evict_over_host_cap(&mut entries, &index, &stem, u64::MAX);
        assert_eq!(evicted, vec!["https://ex.com/page-0".to_string()]);
        assert_eq!(entries.len(), MAX_ENTRIES_PER_HOST_FILE);
        assert_eq!(
            entries.first().map(|r| r.key.as_str()),
            Some("https://ex.com/page-1")
        );
        // The rewritten file must be accepted by its own reader.
        assert_eq!(
            parse_host_file(&serialize_host_file(&entries), &stem).map(|parsed| parsed.len()),
            Some(MAX_ENTRIES_PER_HOST_FILE)
        );
    }

    #[test]
    fn insert_lookup_refresh_remove_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-docs-test-{}.{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let key = [7u8; 32];
        let mut store = DocsStore::open(dir.clone(), Some(key));
        let rec = record("https://ex.com/love", "ex.com");
        let (delta, evicted) = store.insert(rec, 1).expect("stored");
        assert!(delta > 0);
        assert!(evicted.is_empty());
        assert_eq!(store.entry_count(), 1);
        let hit = store.lookup("https://ex.com/love", 1_700_000_100).unwrap();
        assert!(matches!(hit, DocHit::Fresh(_)));
        let hit = store.lookup("https://ex.com/love", 1_800_000_000).unwrap();
        assert!(matches!(hit, DocHit::Stale(_)));
        assert!(store
            .lookup("https://ex.com/other", 1_700_000_100)
            .is_none());
        let freed = store.remove("https://ex.com/love");
        assert!(freed > 0);
        assert_eq!(store.entry_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regression: a keyless (disabled) store must never touch the
    /// filesystem — no creation, no reads, no writes, no deletes. (A sweep
    /// over a relative host dir once deleted unrelated files.)
    #[test]
    fn disabled_store_never_touches_the_filesystem() {
        let dir = std::env::temp_dir().join(format!(
            "mdowser-cache-docs-disabled-{}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = DocsStore::open(dir.clone(), None);
        assert!(!dir.exists());
        assert!(store.lookup("https://ex.com/love", 1_700_000_100).is_none());
        let rec = record("https://ex.com/love", "ex.com");
        assert!(store.insert(rec, 1).is_none());
        assert_eq!(store.remove("https://ex.com/love"), 0);
        store.clear();
        assert!(!dir.exists());
        assert!(store.serialize_index().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
