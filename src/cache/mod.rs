// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache`: encrypted HTTP disk cache for every resource the browser fetches.
//!
//! - Three physical stores, one per resource class: documents (one encrypted
//!   file per host, `docs/`), images (one encrypted file per URL, `img/`),
//!   favicons (one encrypted file for all probes, `favicons.bin`). All three
//!   share the single `store.key` (ChaCha20-Poly1305, same recipe as
//!   history/bookmarks/settings) and honor `Cache-Control` (`policy`).
//! - Fast because hits are RAM-indexed: startup decrypts only the small
//!   index files, bodies decrypt lazily per hit, and a bounded RAM fast path
//!   covers Back/Forward without touching the disk at all.
//! - Bounded and silent: 512 MiB / 50 000 entries globally, with per-class
//!   quotas so one gallery cannot evict every document. Past any bound the
//!   OLDEST entries are evicted FIFO — a store never fails and never toasts.
//! - Freshness is HTTP: `max-age`/`Expires` (+`Date`/`Age`), `no-store`,
//!   `no-cache`, `must-revalidate`, `immutable` inside the absolute 1-year
//!   ceiling, `ETag`/`Last-Modified` revalidation (304 refreshes the entry).
//!   Expired + unreachable is never served as CURRENT: the client's fallback
//!   (`protocol::client::fetch`) serves a stored copy MARKED offline/saved
//!   when one exists; without one the error view stands. `Refresh` forces a
//!   revalidation (or a full fetch when there is nothing to validate with).
//! - Document keys are canonical (`policy::doc_key`): every spelling of one
//!   logical page (`/love`, `/love/`, `/love.md`, indexes…) hits the same
//!   record. Local files and internal pages are never cached.
//! - "Forget on exit" disables the cache entirely (neither read nor written,
//!   leftovers wiped), like history. "Delete all data" removes the whole
//!   data directory, cache included. Each class can also be cleared
//!   separately from Preferences (in-memory now, files via atomic
//!   rename + background delete, so a racing fetch can never lose its file
//!   from under it).

pub(crate) mod docs;
pub(crate) mod favicons;
pub(crate) mod format;
pub(crate) mod images;
pub(crate) mod policy;

use docs::{DocHit, DocRecord, DocsStore};
use favicons::FaviconStore;
use format::now_secs;
use images::{ImageHit, ImageRecord, ImageStore};
use policy::CapturedHeaders;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Per-class quotas inside the global 512 MiB budget (128 + 352 + 32).
/// A gallery of 16 MiB photos cannot evict every cached document.
const DOCS_QUOTA_BYTES: u64 = 128 * 1024 * 1024;
/// Test quota: small enough that a handful of 1 KiB images exercises the
/// REAL eviction path (oldest file deleted from disk, accounting updated)
/// without writing megabytes.
#[cfg(not(test))]
const IMAGES_QUOTA_BYTES: u64 = 352 * 1024 * 1024;
#[cfg(test)]
const IMAGES_QUOTA_BYTES: u64 = 8 * 1024;
const FAVICONS_QUOTA_BYTES: u64 =
    (crate::config::MAX_CACHE_FAVICON_BYTES as u64).saturating_sub(favicons::FILE_HEADER_BYTES);

/// Resource class (stable `u8` for the order map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Class {
    Docs,
    Images,
    Favicons,
}

/// Snapshot of cache usage for Preferences.
#[derive(Debug, Clone, Default)]
pub(crate) struct CacheStats {
    pub enabled: bool,
    pub doc_bytes: u64,
    pub doc_entries: usize,
    pub image_bytes: u64,
    pub image_entries: usize,
    pub favicon_bytes: u64,
    pub favicon_entries: usize,
    /// On-disk bytes of the two index files (counted in the global budget).
    pub index_bytes: u64,
}

struct Inner {
    dir: Option<PathBuf>,
    docs: DocsStore,
    images: ImageStore,
    favicons: FaviconStore,
    /// Global FIFO: sequence -> (class, key). The oldest entry evicts first.
    order: BTreeMap<u64, (Class, String)>,
    /// Reverse map for O(1) re-insert/drop of the old sequence, carrying the
    /// entry's metadata estimate (key + stored URL + row framing) so the
    /// global budget covers index memory too (see `meta_bytes`).
    seq_of: HashMap<(Class, String), (u64, u64)>,
    next_seq: u64,
    total_bytes: u64,
    /// Live estimate of the in-memory index rows (keys + `fetched_url`s +
    /// framing) of documents and images. Bounded by construction: without
    /// it, a hostile session could hold tens of thousands of long URLs'
    /// worth of index metadata that the 512 MiB body budget never sees, and
    /// the flush would serialize that whole index while holding the lock.
    /// Replaced exactly on drop/re-store, so it tracks inserts, refreshes,
    /// clears and FIFO eviction. (Favicons charge their metadata through the
    /// record size: their single file carries bodies, not index rows.)
    meta_bytes: u64,
    /// Mutation generation (flush clears dirty flags only when it flushed
    /// the latest generation: a racing store is never silently dropped).
    generation: u64,
    flushed_generation: u64,
    last_flush: Instant,
    flush_running: bool,
    /// Bumped by every explicit clear (`clear_*`). The background flusher
    /// captures it with its serialization and refuses to write when it
    /// changed: a flush that serialized BEFORE a clear must never land
    /// AFTER the clear's own synchronous empty write (that would resurrect
    /// deleted entries on the next start after a crash in the window).
    flush_epoch: u64,
    /// Set by [`CacheManager::suppress`] ("Delete all data" teardown): every
    /// later store/refresh/flush-spawn is a no-op, so a racing background job
    /// can never recreate the wiped directory. Synchronous reads still serve.
    stopped: bool,
    /// On-disk bytes of the two persisted index files (`docs-index.bin` +
    /// `img-index.bin`), re-measured after every successful flush. Feeds the
    /// Preferences stats; the BUDGET uses `meta_bytes` (the live estimate),
    /// so entries added since the last flush are never invisible to it.
    index_bytes: u64,
}

pub(crate) struct CacheManager {
    inner: Mutex<Inner>,
}

static CACHE: OnceLock<CacheManager> = OnceLock::new();

/// Stops the global cache's write paths (see
/// [`CacheManager::suppress`]); reads still serve. No-op before [`init`].
pub(crate) fn suppress() {
    if let Some(cache) = global() {
        cache.suppress();
    }
}

/// Re-enables the write paths after a "Delete all data" wipe that FAILED
/// (the app keeps running and must keep caching; see
/// [`CacheManager::resume`]).
pub(crate) fn resume() {
    if let Some(cache) = global() {
        cache.resume();
    }
}

/// Global cache (initialized once by `App::new`; uninitialized = disabled).
pub(crate) fn global() -> Option<&'static CacheManager> {
    CACHE.get()
}

/// Initializes the global cache. `persistent = false` ("Forget on exit")
/// wipes leftovers and disables the cache entirely.
pub(crate) fn init(dir: Option<PathBuf>, persistent: bool) {
    if CACHE.get().is_some() {
        return;
    }
    let manager = match dir {
        Some(dir) if persistent => CacheManager::open(dir),
        Some(dir) => {
            // Forget on exit: never read or write; sweep leftovers now.
            let cache_dir = dir.join("cache");
            let _ = std::fs::remove_dir_all(&cache_dir);
            CacheManager::disabled()
        }
        None => CacheManager::disabled(),
    };
    let _ = CACHE.set(manager);
}

impl CacheManager {
    fn disabled() -> Self {
        let dir = PathBuf::new();
        Self {
            inner: Mutex::new(Inner {
                dir: None,
                docs: DocsStore::open(dir.clone(), None),
                images: ImageStore::open(dir.clone(), None),
                favicons: FaviconStore::open(dir, None),
                order: BTreeMap::new(),
                seq_of: HashMap::new(),
                next_seq: 1,
                total_bytes: 0,
                meta_bytes: 0,
                generation: 0,
                flushed_generation: 0,
                last_flush: Instant::now(),
                flush_running: false,
                stopped: false,
                flush_epoch: 0,
                index_bytes: 0,
            }),
        }
    }

    fn open(dir: PathBuf) -> Self {
        let cache_dir = dir.join("cache");
        // Never write through a planted symlink or a non-directory at the
        // cache path: disable the cache instead of escaping the data dir.
        if !ensure_cache_dir(&cache_dir) {
            return Self::disabled();
        }
        // The data directory is already owner-only; tighten the cache
        // subdirectory too (a permissive umask must not make hashed entries
        // world-readable metadata) and sweep leftovers from interrupted
        // atomic writes / clears before loading the indexes.
        crate::settings::tighten_dir_perms(&cache_dir);
        sweep_stale_clear_dirs(&cache_dir);
        docs::sweep_temps(&cache_dir);
        let key =
            crate::store::crypto::load_or_create_key(&dir.join(crate::settings::STORE_KEY_FILE));
        // The cache cannot be encrypted without the shared store key: stay
        // disabled (never fall back to plaintext) when it is unavailable.
        let Some(key) = key else {
            return Self::disabled();
        };
        let docs = DocsStore::open(cache_dir.clone(), Some(key));
        let images = ImageStore::open(cache_dir.clone(), Some(key));
        let favicons = FaviconStore::open(cache_dir.clone(), Some(key));
        // Rebuild the global FIFO from the persisted sequences (documents +
        // images carry theirs; favicons are ordered by stored time).
        let mut order: BTreeMap<u64, (Class, String)> = BTreeMap::new();
        let mut seq_of: HashMap<(Class, String), (u64, u64)> = HashMap::new();
        let mut next_seq = 1u64;
        for (seq, k) in docs.ordered_keys() {
            next_seq = next_seq.max(seq.saturating_add(1));
            // A seq collision (hand-edited index) moves to a fresh sequence
            // instead of dropping an entry.
            let seq = if order.contains_key(&seq) {
                let fresh = next_seq;
                next_seq = next_seq.saturating_add(1);
                fresh
            } else {
                seq
            };
            order.insert(seq, (Class::Docs, k.clone()));
            seq_of.insert((Class::Docs, k.clone()), (seq, open_meta_estimate(&k)));
        }
        for (seq, k) in images.ordered_keys() {
            next_seq = next_seq.max(seq.saturating_add(1));
            // A seq collision across classes (hand-edited index) moves to a
            // fresh sequence instead of dropping an entry.
            let seq = if order.contains_key(&seq) {
                let fresh = next_seq;
                next_seq = next_seq.saturating_add(1);
                fresh
            } else {
                seq
            };
            order.insert(seq, (Class::Images, k.clone()));
            seq_of.insert((Class::Images, k.clone()), (seq, open_meta_estimate(&k)));
        }
        // Favicons persist `stored_at` only: assign fresh sequences oldest
        // first so FIFO order survives restarts. Their metadata rides in the
        // record size already (see `meta_bytes`), so the estimate is 0.
        let mut fav: Vec<(u64, String, u64)> = favicons.ordered_keys();
        fav.sort();
        for (_, k, _) in fav {
            let seq = next_seq;
            next_seq = next_seq.saturating_add(1);
            order.insert(seq, (Class::Favicons, k.clone()));
            seq_of.insert((Class::Favicons, k), (seq, 0));
        }
        // `index_dirty` is NOT reset here: the stores' open-time sweeps set
        // it when they pruned entries the files no longer match, and that
        // pruned index must reach the disk (the first debounced tick does).
        let total_bytes = docs
            .byte_total()
            .saturating_add(images.byte_total())
            .saturating_add(favicons.byte_total());
        let meta_bytes: u64 = seq_of.values().map(|(_, meta)| *meta).sum();
        // Index files on disk count against the same budget from the first
        // byte (measured before the directory moves into the store).
        let index_bytes = file_len(&cache_dir.join("docs-index.bin"))
            .saturating_add(file_len(&cache_dir.join("img-index.bin")));
        Self {
            inner: Mutex::new(Inner {
                dir: Some(cache_dir),
                docs,
                images,
                favicons,
                order,
                seq_of,
                next_seq,
                total_bytes,
                meta_bytes,
                generation: 0,
                flushed_generation: 0,
                last_flush: Instant::now(),
                flush_running: false,
                stopped: false,
                flush_epoch: 0,
                index_bytes,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn enabled(&self) -> bool {
        self.lock().dir.is_some()
    }

    // -- documents -------------------------------------------------------

    pub(crate) fn doc_lookup(&self, key: &str) -> Option<DocHit> {
        let now = now_secs();
        let mut inner = self.lock();
        let before = inner.docs.byte_total();
        let hit = inner.docs.lookup(key, now);
        // A poisoned body makes the store drop its index entry; the global
        // total and FIFO must follow, or the phantom keeps consuming budget
        // and can evict unrelated entries for the rest of the session.
        let after = inner.docs.byte_total();
        if after < before {
            inner.total_bytes = inner.total_bytes.saturating_sub(before - after);
            inner.drop_seq(Class::Docs, key);
            inner.touched();
        }
        hit
    }

    pub(crate) fn doc_store(&self, record: DocRecord) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let key = record.key.clone();
        // Index row this entry will occupy (key + stored URL + framing); the
        // manager charges it so index memory counts against the budget.
        let meta = meta_estimate(&record.key, &record.fetched_url);
        let seq = inner.next_seq();
        // `drop_seq` only on success: a failed write must leave the previous
        // order membership (and its bytes) untouched.
        if let Some((delta, evicted)) = inner.docs.insert(record, seq) {
            inner.drop_seq(Class::Docs, &key);
            for old in evicted {
                inner.drop_seq(Class::Docs, &old);
            }
            inner.account(Class::Docs, key, seq, meta, Some(delta));
            inner.evict_while_over();
        }
    }

    pub(crate) fn doc_refresh(&self, key: &str, fresh_until: u64, headers: &CapturedHeaders) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        if let Some((delta, evicted)) = inner.docs.refresh(key, fresh_until, headers) {
            for old in evicted {
                inner.drop_seq(Class::Docs, &old);
            }
            inner.adjust_bytes(delta);
            inner.evict_while_over();
            inner.touched();
        }
    }

    // -- images ----------------------------------------------------------

    pub(crate) fn image_lookup(&self, key: &str) -> Option<ImageHit> {
        let now = now_secs();
        let mut inner = self.lock();
        let before = inner.images.byte_total();
        let hit = inner.images.lookup(key, now);
        let after = inner.images.byte_total();
        if after < before {
            inner.total_bytes = inner.total_bytes.saturating_sub(before - after);
            inner.drop_seq(Class::Images, key);
            inner.touched();
        }
        hit
    }

    pub(crate) fn image_store(&self, record: ImageRecord) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let key = record.key.clone();
        let meta = meta_estimate(&record.key, &record.fetched_url);
        let seq = inner.next_seq();
        if let Some(delta) = inner.images.insert(record, seq) {
            inner.drop_seq(Class::Images, &key);
            inner.account(Class::Images, key, seq, meta, Some(delta));
            inner.evict_while_over();
        }
    }

    pub(crate) fn image_refresh(&self, key: &str, fresh_until: u64, headers: &CapturedHeaders) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        if let Some(delta) = inner.images.refresh(key, fresh_until, headers) {
            inner.adjust_bytes(delta);
            inner.evict_while_over();
            inner.touched();
        }
    }

    // -- favicons --------------------------------------------------------

    pub(crate) fn favicon_lookup(&self, key: &str) -> Option<ImageHit> {
        let now = now_secs();
        self.lock().favicons.lookup(key, now)
    }

    pub(crate) fn favicon_store(&self, record: ImageRecord) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let key = record.key.clone();
        let seq = inner.next_seq();
        // Favicon metadata is charged through the record size (`favicons.bin`
        // carries the bodies), so the index-row estimate is 0.
        if let Some(delta) = inner.favicons.insert(record, seq) {
            inner.drop_seq(Class::Favicons, &key);
            inner.account(Class::Favicons, key, seq, 0, Some(delta));
            inner.evict_while_over();
        }
    }

    pub(crate) fn favicon_refresh(&self, key: &str, fresh_until: u64, headers: &CapturedHeaders) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        if let Some(delta) = inner.favicons.refresh(key, fresh_until, headers) {
            inner.adjust_bytes(delta);
            inner.touched();
        }
    }

    // -- maintenance -----------------------------------------------------

    /// Stops every later write path (stores, refreshes, flush spawns) for
    /// the "Delete all data" teardown: a racing background fetch can neither
    /// recreate the wiped directory nor queue work nobody drains. Reads still
    /// serve (the process exits immediately after).
    pub(crate) fn suppress(&self) {
        self.lock().stopped = true;
    }

    /// Re-enables every write path after a FAILED "Delete all data" wipe:
    /// the directory is still there, so the cache must keep working instead
    /// of staying silently read-only for the rest of the session.
    pub(crate) fn resume(&self) {
        self.lock().stopped = false;
    }

    pub(crate) fn stats(&self) -> CacheStats {
        let inner = self.lock();
        CacheStats {
            enabled: inner.dir.is_some(),
            doc_bytes: inner.docs.byte_total(),
            doc_entries: inner.docs.entry_count(),
            image_bytes: inner.images.byte_total(),
            image_entries: inner.images.entry_count(),
            favicon_bytes: inner.favicons.byte_total(),
            favicon_entries: inner.favicons.entry_count(),
            index_bytes: inner.index_bytes,
        }
    }

    pub(crate) fn clear_docs(&self) {
        let mut inner = self.lock();
        if inner.dir.is_none() {
            return;
        }
        let class_bytes = inner.docs.byte_total();
        inner.docs.clear();
        inner.total_bytes = inner.total_bytes.saturating_sub(class_bytes);
        inner.drop_class(Class::Docs);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    /// Drops one poisoned document record (stored body that no longer
    /// parses, or an origin that turned to `no-store` on revalidation).
    /// The FIFO sequence drops even when the store reports zero freed
    /// bytes: a zero-size index row must not leave a phantom in `order`.
    pub(crate) fn clear_docs_for_key(&self, key: &str) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let freed = inner.docs.remove(key);
        inner.total_bytes = inner.total_bytes.saturating_sub(freed);
        inner.drop_seq(Class::Docs, key);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    /// Drops one poisoned image record (stored body that no longer decodes).
    pub(crate) fn clear_images_for_key(&self, key: &str) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let freed = inner.images.remove(key);
        inner.total_bytes = inner.total_bytes.saturating_sub(freed);
        inner.drop_seq(Class::Images, key);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    /// Drops one poisoned favicon record.
    pub(crate) fn clear_favicons_for_key(&self, key: &str) {
        let mut inner = self.lock();
        if inner.dir.is_none() || inner.stopped {
            return;
        }
        let freed = inner.favicons.remove(key);
        inner.total_bytes = inner.total_bytes.saturating_sub(freed);
        inner.drop_seq(Class::Favicons, key);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    pub(crate) fn clear_images(&self) {
        let mut inner = self.lock();
        if inner.dir.is_none() {
            return;
        }
        let class_bytes = inner.images.byte_total();
        inner.images.clear();
        inner.total_bytes = inner.total_bytes.saturating_sub(class_bytes);
        inner.drop_class(Class::Images);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    pub(crate) fn clear_favicons(&self) {
        let mut inner = self.lock();
        if inner.dir.is_none() {
            return;
        }
        let class_bytes = inner.favicons.byte_total();
        inner.favicons.clear();
        inner.total_bytes = inner.total_bytes.saturating_sub(class_bytes);
        inner.drop_class(Class::Favicons);
        inner.flush_epoch = inner.flush_epoch.wrapping_add(1);
        inner.touched();
    }

    /// Per-frame flush check (cheap: one lock + timestamp compare). Spawns
    /// the background flusher at most every
    /// `CACHE_INDEX_FLUSH_DEBOUNCE_SECS` while dirty.
    ///
    /// Never waits for the lock: a background fetch or flush may be writing
    /// a multi-megabyte body while holding it, and blocking the UI thread on
    /// that would drop frames. A skipped check is harmless — the next frame
    /// retries, and the exit path flushes synchronously.
    pub(crate) fn tick(&self) {
        let debounce = Duration::from_secs(crate::config::CACHE_INDEX_FLUSH_DEBOUNCE_SECS);
        let inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        let should_flush = inner.dir.is_some()
            && !inner.stopped
            && !inner.flush_running
            && inner.flushed_generation != inner.generation
            && inner.last_flush.elapsed() >= debounce;
        drop(inner);
        if should_flush {
            self.spawn_flush();
        }
    }

    /// Synchronous bounded flush (exit path). Waits for a running flusher,
    /// then flushes directly when still dirty.
    pub(crate) fn flush_blocking(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            {
                let inner = self.lock();
                if !inner.flush_running {
                    break;
                }
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let dirty = {
            let inner = self.lock();
            inner.dir.is_some() && inner.flushed_generation != inner.generation
        };
        if dirty {
            self.flush_now();
        }
        true
    }

    fn spawn_flush(&self) {
        {
            let mut inner = self.lock();
            if inner.flush_running || inner.dir.is_none() || inner.stopped {
                return;
            }
            inner.flush_running = true;
        }
        let spawned = std::thread::Builder::new()
            .name("mdowser-cache-flush".into())
            .spawn(|| {
                if let Some(cache) = global() {
                    cache.flush_now();
                    cache.lock().flush_running = false;
                }
            });
        // A refused thread start must not wedge flushing forever: the next
        // tick (or the exit flush) retries.
        if spawned.is_err() {
            self.lock().flush_running = false;
        }
    }

    /// Serializes + writes the indexes and favicon file (flusher thread or
    /// the exit path). Body files are already on disk: only metadata lands
    /// here. The writes hold the manager lock: an index write is one bounded
    /// blob (encrypt + `fsync`), and holding the lock makes the flush atomic
    /// with clears — a flush that serialized BEFORE an explicit clear refuses
    /// to land AFTER it (checked via `flush_epoch`), so deleted entries can
    /// never be resurrected by a racing write.
    fn flush_now(&self) {
        struct Pending {
            dir: PathBuf,
            docs: Option<Vec<u8>>,
            images: Option<Vec<u8>>,
            favicons: Option<Vec<u8>>,
            docs_dirty: bool,
            images_dirty: bool,
            favicons_dirty: bool,
            generation: u64,
            epoch: u64,
        }
        let pending = {
            let inner = self.lock();
            inner.dir.clone().map(|dir| Pending {
                dir,
                docs: inner.docs.serialize_index(),
                images: inner.images.serialize_index(),
                favicons: inner.favicons.serialize(),
                docs_dirty: inner.docs.index_dirty,
                images_dirty: inner.images.index_dirty,
                favicons_dirty: inner.favicons.file_dirty,
                generation: inner.generation,
                epoch: inner.flush_epoch,
            })
        };
        let Some(pending) = pending else {
            return;
        };
        // A store that failed to serialize (over-cap index, no key) simply
        // skips its file: the in-memory state stays authoritative.
        let mut ok = true;
        let mut inner = self.lock();
        // An explicit clear landed while serializing: its own synchronous
        // empty write is the truth — drop this stale serialization instead
        // of landing it afterwards. The next tick persists the current state.
        if inner.flush_epoch != pending.epoch {
            return;
        }
        if let Some(blob) = pending.docs.as_deref() {
            ok &= crate::settings::atomic_write(&pending.dir, "docs-index.bin", blob).is_ok();
        } else if pending.docs_dirty {
            ok = false;
        }
        if let Some(blob) = pending.images.as_deref() {
            ok &= crate::settings::atomic_write(&pending.dir, "img-index.bin", blob).is_ok();
        } else if pending.images_dirty {
            ok = false;
        }
        if pending.favicons_dirty {
            match pending.favicons.as_deref() {
                Some(blob) => {
                    ok &= crate::settings::atomic_write(&pending.dir, "favicons.bin", blob).is_ok();
                }
                None => ok = false,
            }
        }
        // Only clear what this flush actually wrote: a racing store bumped
        // `generation` past what was serialized and needs another flush.
        // Index sizes are re-measured from what landed (bodies shrank only
        // via eviction, which already updated `total_bytes`).
        //
        // `last_flush` moves on BOTH paths: a failed flush (read-only data
        // dir, full disk) must retry at the next debounce, not on every
        // frame — `tick` consults it before spawning another flusher.
        inner.last_flush = Instant::now();
        if ok && inner.generation == pending.generation {
            inner.docs.index_dirty = false;
            inner.images.index_dirty = false;
            inner.favicons.file_dirty = false;
            inner.flushed_generation = pending.generation;
            inner.index_bytes = file_len(&pending.dir.join("docs-index.bin"))
                .saturating_add(file_len(&pending.dir.join("img-index.bin")));
        }
    }
}

impl Inner {
    fn next_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1).max(1);
        seq
    }

    fn drop_seq(&mut self, class: Class, key: &str) {
        if let Some((seq, meta)) = self.seq_of.remove(&(class, key.to_string())) {
            self.order.remove(&seq);
            self.meta_bytes = self.meta_bytes.saturating_sub(meta);
        }
    }

    fn drop_class(&mut self, class: Class) {
        let mut freed = 0u64;
        self.seq_of.retain(|(c, _), (_, meta)| {
            if *c == class {
                freed = freed.saturating_add(*meta);
                false
            } else {
                true
            }
        });
        self.order.retain(|_, (c, _)| *c != class);
        self.meta_bytes = self.meta_bytes.saturating_sub(freed);
    }

    fn touched(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Applies a signed byte delta to the global total (never underflows).
    fn adjust_bytes(&mut self, delta: i64) {
        if delta >= 0 {
            self.total_bytes = self.total_bytes.saturating_add(delta as u64);
        } else {
            self.total_bytes = self.total_bytes.saturating_sub(delta.unsigned_abs());
        }
    }

    fn account(&mut self, class: Class, key: String, seq: u64, meta: u64, stored: Option<i64>) {
        // `None` = the write failed (no key, oversize, IO): nothing to track.
        let Some(delta) = stored else {
            return;
        };
        self.touched();
        self.adjust_bytes(delta);
        // The new sequence replaces the old one (a re-store goes to the back
        // of the FIFO, like any fresh write); the metadata estimate follows
        // the entry exactly (keys are per-class unique).
        if let Some((old_seq, old_meta)) = self.seq_of.insert((class, key.clone()), (seq, meta)) {
            self.order.remove(&old_seq);
            self.meta_bytes = self.meta_bytes.saturating_sub(old_meta);
        }
        self.meta_bytes = self.meta_bytes.saturating_add(meta);
        self.order.insert(seq, (class, key));
    }

    fn total_entries(&self) -> usize {
        self.order.len()
    }

    fn class_usage(&self, class: Class) -> (u64, usize) {
        match class {
            Class::Docs => (self.docs.byte_total(), self.docs.entry_count()),
            Class::Images => (self.images.byte_total(), self.images.entry_count()),
            Class::Favicons => (self.favicons.byte_total(), self.favicons.entry_count()),
        }
    }

    fn class_over(&self, class: Class) -> bool {
        let (bytes, entries) = self.class_usage(class);
        match class {
            Class::Docs => bytes > DOCS_QUOTA_BYTES,
            Class::Images => bytes > IMAGES_QUOTA_BYTES,
            Class::Favicons => {
                bytes > FAVICONS_QUOTA_BYTES || entries > crate::config::MAX_CACHE_FAVICON_ENTRIES
            }
        }
    }

    fn global_over(&self) -> bool {
        // Bodies + the live index-metadata estimate: `index_bytes` (measured
        // at flush time) is display-only — using it here left entries added
        // since the last flush invisible to the budget, so a hostile
        // URL-heavy session could hold an unbounded index until the next
        // flush measured it and one eviction pass drained the whole FIFO.
        self.total_bytes.saturating_add(self.meta_bytes) > crate::config::MAX_CACHE_BYTES as u64
            || self.total_entries() > crate::config::MAX_CACHE_ENTRIES
    }

    /// FIFO eviction: per-class quotas first (a gallery never starves the
    /// documents), then the global budget. Never fails, never reports: it
    /// just makes room.
    ///
    /// Each pass removes exactly one entry from `order`, so the loop always
    /// makes progress; a loaded pair of full indexes can briefly hold more
    /// than `MAX_CACHE_ENTRIES` together and must be allowed to drain all
    /// the way down, not just for a fixed iteration budget.
    fn evict_while_over(&mut self) {
        loop {
            let over: Vec<Class> = [Class::Docs, Class::Images, Class::Favicons]
                .into_iter()
                .filter(|c| self.class_over(*c))
                .collect();
            let Some((seq, class, key)) = pick_victim(&self.order, &over, self.global_over())
            else {
                break;
            };
            self.order.remove(&seq);
            if let Some((_, meta)) = self.seq_of.remove(&(class, key.clone())) {
                self.meta_bytes = self.meta_bytes.saturating_sub(meta);
            }
            let freed = match class {
                Class::Docs => self.docs.remove(&key),
                Class::Images => self.images.remove(&key),
                Class::Favicons => self.favicons.remove(&key),
            };
            self.total_bytes = self.total_bytes.saturating_sub(freed);
            self.touched();
        }
    }
}

/// Ensures `dir` exists as a REAL directory (never a symlink to one, never a
/// file/FIFO). Returns false when the path is unsafe or cannot be created.
fn ensure_cache_dir(dir: &Path) -> bool {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta.file_type().is_dir(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
            // Re-check after creation: a racing symlink swap must fail
            // closed rather than redirect every later write.
            std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.file_type().is_dir())
        }
        Err(_) => false,
    }
}

/// Removes directories left behind by an interrupted per-class clear
/// (`docs.clear.<pid>.<time>` / `img.clear...`). They are cache-internal,
/// never referenced, and would otherwise hold their bytes forever.
fn sweep_stale_clear_dirs(cache_dir: &Path) {
    let Ok(top) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for entry in top.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = name.starts_with("docs.clear.") || name.starts_with("img.clear.");
        if !ours {
            continue;
        }
        let path = entry.path();
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_dir());
        if is_dir {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// File size in bytes (`None`/non-regular/errors = 0): bounded metadata
/// probe, never a read.
fn file_len(path: &PathBuf) -> u64 {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map_or(0, |m| m.len())
}

/// Metadata estimate for one docs/images index row: the key, the stored
/// `fetched_url` and the row framing. Charged into the global budget so
/// index memory is bounded exactly like bodies (see `Inner::meta_bytes`).
fn meta_estimate(key: &str, fetched_url: &str) -> u64 {
    (key.len() as u64)
        .saturating_add(fetched_url.len() as u64)
        .saturating_add(64)
}

/// Conservative estimate for an entry loaded from a persisted index (the
/// stored `fetched_url` is not passed around here; it is bounded by
/// `MAX_URL_BYTES` and is close to the key's own length in practice).
fn open_meta_estimate(key: &str) -> u64 {
    (key.len() as u64).saturating_mul(2).saturating_add(160)
}

/// Pure victim selection for FIFO eviction (testable without 512 MiB of
/// fixtures): the oldest entry of an over-quota class first, else the
/// globally oldest entry. `None` when nothing is over budget.
fn pick_victim(
    order: &BTreeMap<u64, (Class, String)>,
    classes_over: &[Class],
    global_over: bool,
) -> Option<(u64, Class, String)> {
    for class in classes_over {
        if let Some((seq, key)) = order
            .iter()
            .find(|(_, (c, _))| c == class)
            .map(|(s, (_, k))| (*s, k.clone()))
        {
            return Some((seq, *class, key));
        }
    }
    if global_over {
        order
            .iter()
            .next()
            .map(|(seq, (c, k))| (*seq, *c, k.clone()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::policy::CapturedHeaders;

    fn manager_in(dir: PathBuf) -> CacheManager {
        CacheManager::open(dir)
    }

    fn doc(key: &str, host: &str, body_len: usize) -> DocRecord {
        DocRecord {
            key: key.to_string(),
            fetched_url: format!("https://{host}/a.md"),
            final_url: format!("https://{host}/a/"),
            content_type: "text/markdown".to_string(),
            stored_at: now_secs(),
            fresh_until: now_secs() + 300,
            etag: String::new(),
            last_modified: String::new(),
            body: "x".repeat(body_len),
        }
    }

    fn img(key: &str, len: usize) -> ImageRecord {
        ImageRecord {
            key: key.to_string(),
            fetched_url: key.to_string(),
            content_type: "image/png".to_string(),
            stored_at: now_secs(),
            fresh_until: now_secs() + 3600,
            etag: String::new(),
            last_modified: String::new(),
            body: vec![7u8; len],
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdowser-cache-mgr-{name}-{}", std::process::id(),));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn fifo_eviction_never_errors() {
        let dir = tmp("fifo").join("cache");
        let mgr = manager_in(dir);
        // Fill past tiny effective pressure by shrinking? Use the real caps
        // but verify accounting instead of writing 512 MiB: 300 docs x 1 KiB.
        for i in 0..300 {
            mgr.doc_store(doc(&format!("https://ex.com/p{i}"), "ex.com", 1024));
        }
        let stats = mgr.stats();
        assert_eq!(stats.doc_entries, 300);
        assert!(stats.doc_bytes > 300 * 1024);
        // Re-storing an existing key does not duplicate the order entry.
        mgr.doc_store(doc("https://ex.com/p0", "ex.com", 1024));
        assert_eq!(mgr.stats().doc_entries, 300);
        // Lookup hits the canonical key.
        assert!(mgr.doc_lookup("https://ex.com/p0").is_some());
        // Images + favicons flow through the same manager.
        mgr.image_store(img("https://ex.com/a.png", 512));
        mgr.favicon_store(img("https://ex.com/favicon.ico", 64));
        assert!(mgr.image_lookup("https://ex.com/a.png").is_some());
        assert!(mgr.favicon_lookup("https://ex.com/favicon.ico").is_some());
        // Per-class clear drops only its class.
        mgr.clear_docs();
        let stats = mgr.stats();
        assert_eq!(stats.doc_entries, 0);
        assert_eq!(stats.image_entries, 1);
        assert_eq!(stats.favicon_entries, 1);
        mgr.clear_images();
        mgr.clear_favicons();
        assert_eq!(mgr.stats().image_entries, 0);
        assert_eq!(mgr.stats().favicon_entries, 0);
        let _ = std::fs::remove_dir_all(tmp("fifo"));
    }

    #[test]
    fn doc_store_roundtrip_through_files() {
        let dir = tmp("roundtrip").join("cache");
        let mgr = manager_in(dir.clone());
        mgr.doc_store(doc("https://ex.com/love", "ex.com", 64));
        mgr.image_store(img("https://ex.com/a.png", 128));
        // Indexes flush debounced: force a synchronous flush before drop so
        // the new manager re-reads them from disk like a fresh process.
        assert!(mgr.flush_blocking(std::time::Duration::from_secs(5)));
        // Drop the manager (new one re-reads the indexes from disk).
        drop(mgr);
        let mgr2 = manager_in(dir);
        assert!(mgr2.doc_lookup("https://ex.com/love").is_some());
        assert!(mgr2.image_lookup("https://ex.com/a.png").is_some());
        let _ = std::fs::remove_dir_all(tmp("roundtrip"));
    }

    #[test]
    fn disabled_manager_misses_everything() {
        let mgr = CacheManager::disabled();
        assert!(!mgr.enabled());
        assert!(mgr.doc_lookup("https://ex.com/").is_none());
        mgr.doc_store(doc("https://ex.com/", "ex.com", 8));
        assert_eq!(mgr.stats().doc_entries, 0);
        let _ = CapturedHeaders::default();
    }

    /// A failed "Delete all data" wipe re-enables the cache: writes stopped
    /// by `suppress` must resume instead of leaving the session read-only.
    #[test]
    fn suppress_then_resume_restores_writes() {
        let dir = tmp("suppress").join("cache");
        let mgr = manager_in(dir);
        mgr.suppress();
        mgr.doc_store(doc("https://ex.com/a", "ex.com", 8));
        assert_eq!(mgr.stats().doc_entries, 0);
        mgr.resume();
        mgr.doc_store(doc("https://ex.com/a", "ex.com", 8));
        assert_eq!(mgr.stats().doc_entries, 1);
        let _ = std::fs::remove_dir_all(tmp("suppress"));
    }

    /// A corrupt host file makes `DocsStore::lookup` drop the index entry.
    /// The manager's global total and FIFO must drop it too: the phantom
    /// used to keep consuming the budget and could evict unrelated entries.
    #[test]
    fn poisoned_lookup_releases_global_accounting() {
        let dir = tmp("poisoned").join("cache");
        let mgr = manager_in(dir.clone());
        mgr.doc_store(doc("https://ex.com/a", "ex.com", 64));
        assert_eq!(mgr.stats().doc_entries, 1);
        // Reopen so the lookup reads the host file instead of the RAM fast
        // path, like a fresh process with a poisoned file on disk.
        assert!(mgr.flush_blocking(std::time::Duration::from_secs(5)));
        drop(mgr);
        let mgr = manager_in(dir);
        let cache_dir = mgr.lock().dir.clone().expect("enabled cache");
        let mut host_file = None;
        if let Ok(shards) = std::fs::read_dir(cache_dir.join("docs")) {
            for shard in shards.flatten() {
                if let Ok(files) = std::fs::read_dir(shard.path()) {
                    if let Some(file) = files
                        .flatten()
                        .find(|f| f.path().extension().and_then(|e| e.to_str()) == Some("bin"))
                    {
                        host_file = Some(file.path());
                        break;
                    }
                }
            }
        }
        let host_file = host_file.expect("host file written");
        std::fs::write(&host_file, b"not a valid encrypted host file").expect("corrupt");
        assert!(mgr.doc_lookup("https://ex.com/a").is_none());
        let stats = mgr.stats();
        assert_eq!(stats.doc_entries, 0);
        assert_eq!(stats.doc_bytes, 0);
        assert_eq!(mgr.lock().total_bytes, 0);
        let _ = std::fs::remove_dir_all(tmp("poisoned"));
    }

    /// A cache path swapped for a symlink must disable the cache, never
    /// redirect writes outside the data directory.
    #[cfg(unix)]
    #[test]
    fn symlinked_cache_dir_disables_the_cache() {
        let root = tmp("symlink");
        let target = tmp("symlink-target");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, root.join("cache")).unwrap();
        let mgr = manager_in(root.clone());
        assert!(!mgr.enabled());
        mgr.doc_store(doc("https://ex.com/", "ex.com", 8));
        assert_eq!(mgr.stats().doc_entries, 0);
        assert!(!target.join("docs").exists());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(target);
    }

    /// Revalidation rewrites metadata (validators/content-type) and can
    /// change a record's serialized size. The global budget must follow that
    /// delta, not just the per-class total.
    #[test]
    fn refresh_keeps_global_byte_accounting_exact() {
        let dir = tmp("refresh").join("cache");
        let mgr = manager_in(dir);
        let headers = CapturedHeaders {
            etag: Some(format!("\"{}\"", "v".repeat(200))),
            content_type: Some("text/markdown; charset=utf-8".to_string()),
            ..Default::default()
        };
        mgr.doc_store(doc("https://ex.com/love", "ex.com", 64));
        let docs_before = mgr.stats().doc_bytes;
        assert_eq!(mgr.lock().total_bytes, docs_before);
        mgr.doc_refresh("https://ex.com/love", now_secs() + 300, &headers);
        let docs_after = mgr.stats().doc_bytes;
        assert!(docs_after > docs_before);
        let total_after_docs = mgr.lock().total_bytes;
        assert_eq!(total_after_docs, docs_after);

        mgr.image_store(img("https://ex.com/a.png", 128));
        let images_before = mgr.stats().image_bytes;
        let total_before_images = mgr.lock().total_bytes;
        assert_eq!(total_before_images, docs_after + images_before);
        mgr.image_refresh("https://ex.com/a.png", now_secs() + 3600, &headers);
        let images_after = mgr.stats().image_bytes;
        assert!(images_after > images_before);
        let total_after_images = mgr.lock().total_bytes;
        assert_eq!(total_after_images, docs_after + images_after);
        let _ = std::fs::remove_dir_all(tmp("refresh"));
    }

    /// Regression (index-metadata budget): every stored entry charges its
    /// index row into the budget, and every drop (clear, poison removal,
    /// FIFO eviction) releases it exactly. Before this, index metadata was
    /// only counted after a flush, so a URL-heavy session could grow an
    /// unbounded in-memory index whose serialization held the lock.
    #[test]
    fn index_metadata_is_charged_and_released() {
        let dir = tmp("meta").join("cache");
        let mgr = manager_in(dir);
        assert_eq!(mgr.lock().meta_bytes, 0);
        mgr.doc_store(doc("https://ex.com/love", "ex.com", 8));
        let one = mgr.lock().meta_bytes;
        assert!(one > 0, "a stored document charges its index row");
        mgr.doc_store(doc("https://ex.com/other", "ex.com", 8));
        assert!(mgr.lock().meta_bytes > one);
        mgr.clear_docs();
        assert_eq!(mgr.lock().meta_bytes, 0);
        // A poisoned-record drop releases the entry's share too.
        mgr.image_store(img("https://ex.com/a.png", 128));
        assert!(mgr.lock().meta_bytes > 0);
        mgr.clear_images_for_key("https://ex.com/a.png");
        assert_eq!(mgr.lock().meta_bytes, 0);
        let _ = std::fs::remove_dir_all(tmp("meta"));
    }

    #[test]
    fn victim_selection_is_fifo_per_class_then_global() {
        let mut order: BTreeMap<u64, (Class, String)> = BTreeMap::new();
        order.insert(1, (Class::Images, "img-a".into()));
        order.insert(2, (Class::Docs, "doc-a".into()));
        order.insert(3, (Class::Images, "img-b".into()));
        // Over-quota images evict the OLDEST image, not the oldest entry.
        let victim = pick_victim(&order, &[Class::Images], false).unwrap();
        assert_eq!(victim, (1, Class::Images, "img-a".to_string()));
        // No class over but global over: the globally oldest entry.
        let victim = pick_victim(&order, &[], true).unwrap();
        assert_eq!(victim.0, 1);
        // Nothing over: no victim.
        assert!(pick_victim(&order, &[], false).is_none());
        // An over-quota class with no entries falls through to global.
        let victim = pick_victim(&order, &[Class::Favicons], true).unwrap();
        assert_eq!(victim.0, 1);
    }

    /// End-to-end FIFO eviction under the (test-shrunk) image quota: the
    /// oldest images are deleted FROM DISK, the newest survive, and the
    /// byte/entry accounting stays exact. Never an error, never a toast.
    #[test]
    fn quota_eviction_deletes_oldest_files_first() {
        let dir = tmp("evict").join("cache");
        let mgr = manager_in(dir.clone());
        // 20 images x ~1 KiB against an 8 KiB test quota.
        for i in 0..20 {
            mgr.image_store(img(&format!("https://ex.com/i{i:02}.png"), 1024));
        }
        let stats = mgr.stats();
        // Over-quota classes drain to at most quota + one fresh entry: the
        // newest images survive, the oldest are gone (from memory AND disk).
        assert!(
            stats.image_bytes <= IMAGES_QUOTA_BYTES + 2048,
            "bytes={}",
            stats.image_bytes
        );
        assert!(stats.image_entries < 20, "entries={}", stats.image_entries);
        assert!(mgr.image_lookup("https://ex.com/i19.png").is_some());
        assert!(mgr.image_lookup("https://ex.com/i18.png").is_some());
        assert!(mgr.image_lookup("https://ex.com/i00.png").is_none());
        assert!(mgr.image_lookup("https://ex.com/i01.png").is_none());
        // The evicted files are really gone from disk (no silent litter).
        // (The manager nests one more `cache/` under the given dir.)
        let img_dir = dir.join("cache").join("img");
        let files_on_disk: usize = walk_files(&img_dir);
        assert_eq!(files_on_disk, stats.image_entries, "orphan files on disk");
        // Totals still reconcile: summing the index matches the manager.
        let _ = std::fs::remove_dir_all(tmp("evict"));
    }

    /// Counts regular files under a directory (test-only disk reconciliation).
    fn walk_files(dir: &PathBuf) -> usize {
        let mut count = 0;
        let Ok(top) = std::fs::read_dir(dir) else {
            return 0;
        };
        for entry in top.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += walk_files(&path);
            } else if path.is_file() {
                count += 1;
            }
        }
        count
    }
}
