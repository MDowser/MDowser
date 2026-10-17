// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::cache`: one document's decoded image textures.
//!
//! - Keyed by absolute URL string. `ensure()` registers a URL and starts a
//!   background fetch+decode as soon as a global image slot is free.
//! - `poll()` (UI thread) collects finished jobs and uploads textures.
//! - Entries are dropped together with the document on navigation: decoded
//!   textures are never shared across pages (the encrypted DISK cache in
//!   `cache` is what avoids re-downloading them).
//! - Bounded: at most `MAX_IMAGES_PER_DOC` entries; beyond that the renderer
//!   shows a placeholder instead of queueing unbounded work.
//! - Transient failures (network error, stall timeout, truncated transfer
//!   that surfaces as a decode error) get `config::MAX_IMAGE_RETRIES`
//!   automatic retries, re-queued behind the pending jobs; content-type
//!   rejections and cancellations are final (the bytes will not change).
//! - Every entry has a deadline (`config::IMAGE_JOB_TIMEOUT_SECS`): a job
//!   stuck in a decoder it cannot interrupt still stops counting as pending
//!   and its placeholder becomes a failure state.

use super::decode::DecodedImage;
use super::fetch;
use super::try_acquire_image_slot;
use crate::config;
use eframe::egui;
use poll_promise::Promise;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Is this failure worth one automatic retry?
///
/// The reason strings come from `protocol::client::get_bytes` (and the
/// decoders) and are classified by content, never by bare prefix:
/// - Content-type rejections, 4xx (except 408/429), size caps, https
///   downgrades and cancellations are deterministic: retrying cannot help.
/// - Network errors, stalls, truncated transfers (often surfacing as a
///   decoder error) and 5xx/429 are transient.
fn is_transient(reason: &str) -> bool {
    if reason == "cancelled" || reason.starts_with("internal error") {
        return false;
    }
    if reason.starts_with("access to ") {
        // Deterministic verdicts: the same bytes would come back.
        const FINAL_MARKERS: &[&str] = &[
            "cancelled",
            "redirected off https",
            "redirected with credentials",
            "redirected to an over-long URL",
            "with credentials (blocked)",
            "does not exist",
            "exceeds ",
            "(expected an image)",
            // Local-file verdicts from `protocol::local`: the path will not
            // become a readable regular file by retrying.
            "is a directory",
            "is not a regular file",
            "is not a local path",
        ];
        if FINAL_MARKERS.iter().any(|m| reason.contains(m)) {
            return false;
        }
        // Local open/read IO errors (`protocol::local` formats the io::Error
        // after the `file:` URL: `access to file:///x/y.png: Permission denied
        // (os error 13)` or `… error reading file: …`). The same path and
        // bytes come back, so a retry only wastes a read. Scoped to LOCAL
        // targets on purpose: a NETWORK failure also carries an
        // `(os error N)` suffix for a reset socket, and that IS transient.
        if (reason.contains(" (os error ") || reason.contains(" error reading file:"))
            && reason.starts_with("access to file:")
        {
            return false;
        }
        // `returned HTTP 4xx`: client errors are final (408/429 retry).
        if let Some(code) = http_status(reason) {
            if (400..500).contains(&code) && code != 408 && code != 429 {
                return false;
            }
        }
        return true;
    }
    if reason.starts_with("no runtime") {
        return false;
    }
    // Decoder verdicts that are deterministic for the same bytes: retrying
    // cannot change a format/cap rejection. A truncated transfer surfaces as
    // a generic `image:`/`svg:` parser error, which stays transient below.
    //
    // `svg: {roxmltree error}` is deliberately NOT listed: a truncated or
    // corrupted SVG download reaches the same parser error as a malformed
    // document, so one retry is worth it (bounded by `MAX_IMAGE_RETRIES`).
    const FINAL_DECODE_MARKERS: &[&str] = &[
        "unrecognized image format",
        "image has zero size",
        "image is too large",
        "image has too many pixels",
        "svg: source is not UTF-8",
        "svg: more than",
        "svg: nesting exceeds",
        "svg: CSS filter declarations are not allowed",
        "svg: duplicate element ids are not allowed",
        // Covers the structural rejections: `<script>/<foreignObject>/
        // <marker>/<pattern>` alike.
        "elements are not allowed",
        "svg: degenerate size",
        "svg: raster buffer refused",
        "decoded frame exceeds the allocation budget",
        "frame side too large",
        "inconsistent frame buffer",
    ];
    if FINAL_DECODE_MARKERS.iter().any(|m| reason.contains(m)) {
        return false;
    }
    // Decoder errors ("image: ...", "svg: ...", "avif: ..."): a truncated
    // transfer often surfaces as one, and one retry can fix it.
    true
}

/// Extracts the status code from `… returned HTTP 503`.
fn http_status(reason: &str) -> Option<u16> {
    reason
        .rsplit_once("HTTP ")?
        .1
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// One image lifecycle entry.
enum Entry {
    /// Registered; waiting for a process-wide image slot (or for a global
    /// texture-budget backoff).
    Queued {
        /// When the entry was registered (deadline clock).
        since: std::time::Instant,
        /// Do not start the fetch before this instant. Set by the
        /// global-budget backoff: the bytes may already have decoded fine,
        /// only VRAM was tight.
        not_before: Option<std::time::Instant>,
    },
    /// Downloading + decoding on a background thread.
    Loading {
        promise: Promise<Result<DecodedImage, String>>,
        cancel: Arc<AtomicBool>,
        /// When the entry was registered (not when the job started): the
        /// queue wait counts against the job deadline.
        since: std::time::Instant,
    },
    /// Decoded and uploaded; handle kept for instant redraws. `vector`
    /// marks SVG sources (inline icons get an icon-sized clamp).
    Ready {
        tex: egui::TextureHandle,
        vector: bool,
    },
    /// Network/type/decode failure; the reason is shown in a tooltip.
    Failed(String),
}

/// Read-only view of one entry for the renderer.
pub enum Status<'a> {
    /// Fetch/decode in progress (or queued).
    Loading,
    /// Texture ready to draw; `vector` marks SVG sources.
    Ready {
        tex: &'a egui::TextureHandle,
        vector: bool,
    },
    /// Failed with a human-readable reason.
    Failed(&'a str),
    /// Not registered (image count cap reached).
    Unavailable,
}

/// Per-document image cache.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, Entry>,
    queue: VecDeque<String>,
    /// Automatic retry counter per URL (transient failures only).
    retries: HashMap<String, u8>,
    /// Global-texture-budget backoff counter per URL (separate from the
    /// network retries: the decoded bytes are fine, only VRAM was tight).
    budget_retries: HashMap<String, u8>,
    /// Bytes of texture already uploaded for this document.
    texture_bytes: usize,
    /// Share of the process-wide texture budget this cache holds.
    /// Released on `cancel_all`/drop so other documents can use it.
    reserved_total: usize,
    /// Bump on every transition that changes rendered geometry
    /// (new registration, texture upload, failure placeholder): the table
    /// width cache keys on it so media tables re-measure exactly once per
    /// transition instead of every frame. Queued↔Loading moves render the
    /// same placeholder and do not bump.
    resolved_epoch: u64,
    /// Cached number of `Entry::Loading` entries, so `pump` does not rescan
    /// every entry per frame while the queue still has work (a 512-image
    /// document used to cost ~0.26 M matches per frame at 60 fps).
    /// Recomputed from scratch at every `poll` (which already walks the
    /// entries), so a missed transition cannot wedge the per-document job
    /// cap for more than one frame.
    loading_count: usize,
    /// Earliest instant a budget-backoff entry in `queue` may be started.
    /// `pump` returns at once while the clock is before it, instead of
    /// re-walking the whole queue for every visible image every frame (a
    /// VRAM-tight gallery called `ensure` per visible image with hundreds of
    /// backed-off entries). `None` = no backoff pending.
    next_budget_retry: Option<std::time::Instant>,
}

impl ImageCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Geometry epoch: changes whenever an entry moves to a state that
    /// renders at a different size (see the field docs). The table width
    /// cache mixes this into its key.
    pub fn resolved_epoch(&self) -> u64 {
        self.resolved_epoch
    }

    /// Bump after a width-affecting transition (single place so no insert
    /// site can forget it).
    fn bump_epoch(&mut self) {
        self.resolved_epoch = self.resolved_epoch.wrapping_add(1);
    }

    /// Registers `url` for background loading when possible.
    ///
    /// Returns `false` when the document already hit the image-count cap;
    /// the caller renders an "image limit" placeholder in that case.
    pub fn ensure(&mut self, url: &str) -> bool {
        if self.entries.contains_key(url) {
            self.pump();
            return true;
        }
        if self.entries.len() >= config::MAX_IMAGES_PER_DOC {
            return false;
        }
        self.entries.insert(
            url.to_string(),
            Entry::Queued {
                since: std::time::Instant::now(),
                not_before: None,
            },
        );
        // Unavailable -> Queued renders a different placeholder: widths
        // measured before this registration are stale.
        self.bump_epoch();
        self.queue.push_back(url.to_string());
        self.pump();
        true
    }

    /// Status of `url` for rendering (never triggers work).
    pub fn status(&self, url: &str) -> Status<'_> {
        match self.entries.get(url) {
            Some(Entry::Ready { tex, vector }) => Status::Ready {
                tex,
                vector: *vector,
            },
            Some(Entry::Failed(reason)) => Status::Failed(reason),
            Some(Entry::Queued { .. } | Entry::Loading { .. }) => Status::Loading,
            None => Status::Unavailable,
        }
    }

    /// Starts queued jobs while process-wide slots are free.
    ///
    /// At most `MAX_CONCURRENT_IMAGES_PER_DOC` jobs run for THIS document:
    /// a single gallery can queue `MAX_IMAGES_PER_DOC` URLs, and without the
    /// per-document cap it would hold every global slot until it drained
    /// (starving other tabs' images and favicons). The remaining global
    /// slots stay available to other caches.
    fn pump(&mut self) {
        // Cheap gate: with nothing queued there is nothing to start, and the
        // renderer calls `ensure` for every visible image every frame. The
        // in-flight count below is cached (`loading_count`), so a page full
        // of settled images never rescans the map per image per frame.
        if self.queue.is_empty() {
            self.next_budget_retry = None;
            return;
        }
        // Every queued entry is waiting for its budget deadline: re-walking
        // the queue for each visible image (`ensure` runs per image per
        // frame) is pure waste until the earliest one passes.
        if self
            .next_budget_retry
            .is_some_and(|at| std::time::Instant::now() < at)
        {
            return;
        }
        self.next_budget_retry = None;
        let mut this_loading = self.loading_count;
        // Budget-backoff entries skipped this pass: they go back behind the
        // ready work once the queue drains, never spin inside this loop.
        let mut retry_later: VecDeque<String> = VecDeque::new();
        // Earliest backoff deadline seen while the queue is drained.
        let mut next_retry: Option<std::time::Instant> = None;
        while self.queue.front().is_some() {
            if this_loading >= config::MAX_CONCURRENT_IMAGES_PER_DOC {
                break;
            }
            let Some(slot) = try_acquire_image_slot() else {
                break;
            };
            let Some(url) = self.queue.pop_front() else {
                break;
            };
            // The entry can be gone if the cache was reset while queued.
            let Some(entry) = self.entries.get(&url) else {
                drop(slot);
                continue;
            };
            // A settled entry behind a stale queue item must not restart.
            let (since, not_before, budget_wait) = match entry {
                Entry::Queued { since, not_before } => {
                    let at = *not_before;
                    (
                        *since,
                        at,
                        at.is_some_and(|at| at > std::time::Instant::now()),
                    )
                }
                _ => {
                    drop(slot);
                    continue;
                }
            };
            if budget_wait {
                drop(slot);
                if let Some(at) = not_before {
                    next_retry = Some(next_retry.map_or(at, |cur| cur.min(at)));
                }
                retry_later.push_back(url);
                continue;
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let job_url = url.clone();
            let job_cancel = cancel.clone();
            let promise = Promise::spawn_thread("fetch-image", move || {
                let _slot = slot; // released when the thread ends
                fetch::fetch_image_blocking(&job_url, job_cancel)
            });
            self.entries.insert(
                url,
                Entry::Loading {
                    promise,
                    cancel,
                    since,
                },
            );
            this_loading += 1;
        }
        // Only a walk that drained the queue saw EVERY backoff deadline: a
        // break for slots leaves later entries unseen, so the gate stays off
        // and the next pump re-walks.
        let drained = self.queue.is_empty();
        self.queue.extend(retry_later);
        self.loading_count = this_loading;
        self.next_budget_retry = if drained { next_retry } else { None };
    }

    /// Collects finished jobs, uploads textures and retries queued work.
    /// Returns `true` when a texture/state changed (needs a repaint).
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        // O(1) settled gate: `queue` holds exactly the `Queued` entries (see
        // `has_pending`), so with an empty queue and no loading entry there
        // is nothing to upload, expire or retry. Without this, `poll` ran
        // three full scans of a 512-image document's entries on EVERY frame,
        // for EVERY tab (poll is called per tab per frame).
        if self.queue.is_empty() && self.loading_count == 0 {
            return false;
        }
        let mut changed = false;
        // Refresh the cached in-flight count in this pass (it already walks
        // the entries below), so a transition that missed an incremental
        // update can never wedge the per-document job cap.
        self.loading_count = self
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Loading { .. }))
            .count();
        // Ready jobs FIRST: a job that already produced a value must be
        // uploaded, never expired. The two snapshots cannot be atomic, so
        // an entry that resolved between them used to be marked Failed and
        // its decoded pixels were dropped.
        let ready_keys: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(url, entry)| match entry {
                Entry::Loading { promise, .. } if promise.ready().is_some() => Some(url.clone()),
                _ => None,
            })
            .collect();
        for url in ready_keys {
            let Some(Entry::Loading {
                promise,
                cancel,
                since,
            }) = self.entries.remove(&url)
            else {
                continue;
            };
            match promise.try_take() {
                Ok(Ok(decoded)) => {
                    self.retries.remove(&url);
                    let bytes = decoded
                        .pixels
                        .size
                        .iter()
                        .copied()
                        .product::<usize>()
                        .saturating_mul(4);
                    if self.texture_bytes.saturating_add(bytes) > config::MAX_TEXTURE_BYTES {
                        // Budget reached: keep the page usable, show a
                        // placeholder instead of uploading unbounded VRAM.
                        self.budget_retries.remove(&url);
                        self.entries
                            .insert(url, Entry::Failed("image memory budget reached".into()));
                        self.bump_epoch();
                        changed = true;
                        continue;
                    }
                    // Process-wide budget: many loaded tabs must not add up
                    // to unbounded VRAM (see `config::MAX_TEXTURE_BYTES_TOTAL`).
                    if !super::try_reserve_texture_bytes(bytes) {
                        // TRANSIENT (other documents release their share on
                        // navigation): back off and retry later instead of
                        // failing the image for the page's whole life or
                        // burning the NETWORK retry (the bytes are fine, only
                        // VRAM is tight). The ORIGINAL `since` is kept, so
                        // the total wait stays bounded by the same
                        // `IMAGE_JOB_TIMEOUT_SECS` deadline as every entry.
                        let attempts = self.budget_retries.get(&url).copied().unwrap_or(0);
                        if attempts < config::MAX_IMAGE_BUDGET_RETRIES {
                            self.budget_retries.insert(url.clone(), attempts + 1);
                            self.entries.insert(
                                url.clone(),
                                Entry::Queued {
                                    since,
                                    not_before: Some(config::deadline_after(
                                        config::IMAGE_BUDGET_RETRY_SECS,
                                    )),
                                },
                            );
                            self.queue.push_back(url);
                        } else {
                            self.budget_retries.remove(&url);
                            self.entries
                                .insert(url, Entry::Failed("global image budget reached".into()));
                            self.bump_epoch();
                        }
                        changed = true;
                        continue;
                    }
                    self.texture_bytes += bytes;
                    self.reserved_total += bytes;
                    // The upload succeeded: the backoff counter (if any) is
                    // consumed and must not count against a later, separate
                    // budget squeeze for the same URL.
                    self.budget_retries.remove(&url);
                    // Texture names are diagnostic only; handles are unique.
                    let name = format!("img-{:016x}", super::fnv1a(&url));
                    let vector = decoded.vector;
                    let tex = ctx.load_texture(name, decoded.pixels, egui::TextureOptions::LINEAR);
                    self.entries.insert(url, Entry::Ready { tex, vector });
                    self.bump_epoch();
                    changed = true;
                }
                Ok(Err(reason)) => {
                    // One automatic retry for TRANSIENT failures (network
                    // error, stall timeout, truncated transfer surfacing as
                    // a decode error): flaky CDNs otherwise leave half the
                    // page as failure placeholders. Content-type
                    // rejections and cancellations never retry.
                    let attempts = self.retries.get(&url).copied().unwrap_or(0);
                    if attempts < config::MAX_IMAGE_RETRIES && is_transient(&reason) {
                        self.retries.insert(url.clone(), attempts + 1);
                        self.entries.insert(
                            url.clone(),
                            Entry::Queued {
                                since: std::time::Instant::now(),
                                not_before: None,
                            },
                        );
                        self.queue.push_back(url);
                    } else {
                        self.retries.remove(&url);
                        self.budget_retries.remove(&url);
                        self.entries.insert(url, Entry::Failed(reason));
                        self.bump_epoch();
                    }
                    changed = true;
                }
                Err(promise) => {
                    // Race: `ready()` said yes, `try_take` disagreed. Keep it
                    // with its ORIGINAL deadline: re-anchoring `since` here
                    // would let a flapping promise postpone the timeout
                    // forever.
                    self.entries.insert(
                        url,
                        Entry::Loading {
                            promise,
                            cancel,
                            since,
                        },
                    );
                }
            }
        }
        // Deadline after readiness: a job stuck inside a decoder cannot be
        // interrupted, but it must not spin forever either (or keep the
        // repaint loop alive). Expired entries become `Failed` and their
        // cancel flag is set so an in-flight fetch aborts too.
        let timeout = std::time::Duration::from_secs(config::IMAGE_JOB_TIMEOUT_SECS);
        // Collect expired keys without cloning the whole map each tick:
        // only entries past their deadline allocate, and ready jobs were
        // already uploaded above so this snapshot stays small.
        let mut expired: Vec<String> = Vec::new();
        for (url, entry) in self.entries.iter() {
            let timed_out = match entry {
                Entry::Queued { since, .. } => since.elapsed() > timeout,
                Entry::Loading { since, promise, .. } => {
                    promise.ready().is_none() && since.elapsed() > timeout
                }
                _ => false,
            };
            if timed_out {
                expired.push(url.clone());
            }
        }
        for url in &expired {
            // Take the entry so the promise can be inspected without a race:
            // if it resolved between the readiness snapshot and here,
            // `try_take` hands back the result and it is re-armed as a ready
            // entry (the next poll uploads it) instead of being discarded as
            // a timeout. A still-pending job is cancelled and failed.
            let Some(entry) = self.entries.remove(url) else {
                continue;
            };
            match entry {
                Entry::Loading {
                    promise,
                    cancel,
                    since,
                } => {
                    if let Ok(result) = promise.try_take() {
                        self.entries.insert(
                            url.clone(),
                            Entry::Loading {
                                promise: Promise::from_ready(result),
                                cancel,
                                since,
                            },
                        );
                        continue;
                    }
                    cancel.store(true, Ordering::Relaxed);
                    self.budget_retries.remove(url);
                    self.entries
                        .insert(url.clone(), Entry::Failed("image timed out".into()));
                    self.bump_epoch();
                    changed = true;
                }
                // A queued entry past its deadline must FAIL, not be
                // reinserted as-is: the old `other` arm kept it pending
                // forever when no image slot ever freed up, so the 120ms
                // repaint loop ran for the whole session.
                Entry::Queued { .. } => {
                    self.budget_retries.remove(url);
                    self.entries
                        .insert(url.clone(), Entry::Failed("image timed out".into()));
                    self.bump_epoch();
                    changed = true;
                }
                other => {
                    self.entries.insert(url.clone(), other);
                }
            }
        }
        // One HashSet instead of `Vec::contains` per queued URL: the queue
        // can hold `MAX_IMAGES_PER_DOC` entries and the expired list can
        // hold as many, so the old scan was O(n²) on a page-wide timeout.
        let failed: std::collections::HashSet<&str> = expired
            .iter()
            .filter(|url| matches!(self.entries.get(*url), Some(Entry::Failed(_))))
            .map(String::as_str)
            .collect();
        if !failed.is_empty() {
            self.queue.retain(|u| !failed.contains(u.as_str()));
        }
        self.pump();
        changed
    }

    /// `true` while any image is queued or downloading (repaint scheduler).
    ///
    /// O(1): `queue` holds exactly the entries in `Queued` state (a queue
    /// item that no longer maps to a queued entry is dropped by `pump`, which
    /// runs at the end of every `poll`), so the old full `entries` scan per
    /// tab per frame only recomputed what these two fields already say.
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty() || self.loading_count > 0
    }

    /// Cancels every in-flight job and frees every texture (navigation /
    /// tab reset).
    pub fn cancel_all(&mut self) {
        for (_, entry) in self.entries.drain() {
            if let Entry::Loading { cancel, .. } = entry {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        // Every entry returns to Unavailable: geometry measured against the
        // old states is stale even if a caller reuses its key.
        self.bump_epoch();
        self.queue.clear();
        self.loading_count = 0;
        self.next_budget_retry = None;
        self.retries.clear();
        self.budget_retries.clear();
        // Textures are dropped with the entries (handles above); release
        // their share of the process-wide budget so other tabs can reuse it.
        if self.reserved_total > 0 {
            super::release_texture_bytes(self.reserved_total);
            self.reserved_total = 0;
        }
        self.texture_bytes = 0;
    }
}

impl Drop for ImageCache {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
impl ImageCache {
    /// Test-only: register an already-uploaded texture (no network, no
    /// promise) so renderers can be tested with a known natural size.
    pub(crate) fn insert_ready(&mut self, url: &str, tex: egui::TextureHandle) {
        self.entries
            .insert(url.to_string(), Entry::Ready { tex, vector: false });
        self.bump_epoch();
    }

    /// Test-only: like [`Self::insert_ready`] but marks the texture as an
    /// SVG (vector) source, so inline clamping can be tested.
    pub(crate) fn insert_ready_vector(&mut self, url: &str, tex: egui::TextureHandle) {
        self.entries
            .insert(url.to_string(), Entry::Ready { tex, vector: true });
        self.bump_epoch();
    }
}

#[cfg(test)]
mod tests;
