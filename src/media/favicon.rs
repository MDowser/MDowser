// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::favicon`: session-wide per-origin favicon textures.
//!
//! - One probe per origin (`scheme://host[:port]`), shared by every tab on
//!   that origin: `config::FAVICON_PATHS` = `/favicon.ico`, `/favicon.png`
//!   and `/favicon.svg` (the conventional paths a markdown-only site can
//!   serve; there is no HTML `<link>` scan).
//! - Bounded (`MAX_FAVICON_HOSTS`); icons are tiny and stay for the session,
//!   which is not a document cache: no page body is ever reused.
//! - Decoded at `MAX_FAVICON_DIM` (drawn at the toolbar icon size, ~27 px)
//!   and counted in the process-wide texture budget, so a session cannot add
//!   up to unbounded VRAM outside the documented bound.
//! - Background jobs share the global image slot limiter.

use super::decode::DecodedImage;
use super::fetch;
use super::try_acquire_image_slot;
use crate::config;
use eframe::egui;
use poll_promise::Promise;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;

enum Entry {
    Loading {
        promise: Promise<Result<DecodedImage, String>>,
        cancel: Arc<AtomicBool>,
        /// Registration instant (job deadline clock).
        since: std::time::Instant,
    },
    Ready(egui::TextureHandle),
    /// The process-wide texture budget refused the decoded icon (VRAM was
    /// tight, the icon itself is fine). `request` retries once `until`
    /// passes; the fallback glyph is drawn meanwhile.
    BudgetWait {
        /// Do not re-probe before this instant.
        until: std::time::Instant,
    },
    Failed,
}

/// Bounded origin -> favicon texture cache.
#[derive(Default)]
pub struct FaviconCache {
    entries: HashMap<String, Entry>,
    /// Share of the process-wide texture budget held by uploaded icons.
    /// Released on `cancel_all`/drop so images can reuse it.
    reserved_total: usize,
    /// Budget-backoff attempts per origin (capped, so a permanently full
    /// budget cannot re-probe+re-decode an icon every cooldown forever).
    budget_attempts: HashMap<String, u8>,
    /// `Entry::Loading` slots in `entries`: `poll` and `has_pending` gate on
    /// this instead of walking up to `MAX_FAVICON_HOSTS` entries every frame.
    loading_count: usize,
    /// `Entry::BudgetWait` slots and the earliest deadline among them: the
    /// due-deadline walk only runs once the earliest one passes.
    backoff_count: usize,
    next_backoff: Option<std::time::Instant>,
    /// `Entry::Failed` slots: with `entries.len()` this answers
    /// `at_capacity` in O(1) (it used to be recomputed by walking every
    /// entry once per `poll`).
    failed_count: usize,
    /// The origin cap is reached with nothing evictable (`Failed`): set by
    /// `request` when it refuses at the cap, cleared whenever a `Failed` slot
    /// exists or the cap has room, so a saturated cache answers an unknown
    /// origin without a `Url::parse` + key allocation per tile per frame.
    at_capacity: bool,
}

impl FaviconCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Icon texture for `origin` (`scheme://host[:port]`) when already loaded.
    /// The tab rail holds a parsed `Url` per tab, so it uses this form; the
    /// string-keyed lookup is [`Self::texture_for_origin`].
    ///
    /// Allocation-free on the common path: the key is a borrowed slice of
    /// the origin's serialization (`entries` is keyed by `String`, and
    /// `String: Borrow<str>`), so a settled frame costs one hash probe per
    /// tab instead of a `String` per tab per frame.
    pub fn texture(&self, origin: &Url) -> Option<&egui::TextureHandle> {
        let owned;
        let key: &str = match origin_key(origin.as_str()) {
            Some(key) => key,
            None => {
                // `:443`/userinfo spellings: the owned key is still correct.
                owned = Self::key(origin);
                &owned
            }
        };
        self.texture_for_origin(key)
    }

    /// [`Self::texture`] keyed by the serialized origin string (`https://host[:port]`).
    fn texture_for_origin(&self, key: &str) -> Option<&egui::TextureHandle> {
        match self.entries.get(key) {
            Some(Entry::Ready(tex)) => Some(tex),
            _ => None,
        }
    }

    /// Icon texture for a bookmarked URL string, requesting the probe on the
    /// first call: the ready texture, else `None` (loading / failed /
    /// backing off / not an https page).
    ///
    /// The origin key is derived from the URL STRING and the cache is keyed
    /// by it, so a settled tile costs one hash probe per frame instead of an
    /// allocation-heavy `Url::parse` per tile per frame on the Home grid.
    /// The URL is parsed at most once per unknown origin, on the frame that
    /// starts the probe.
    pub fn texture_for_url(&mut self, url: &str) -> Option<egui::TextureHandle> {
        if !url.starts_with("https://") {
            return None;
        }
        // Borrowed key when the URL is in normalized form (always, for a
        // stored bookmark): the cache answers or reports "probe already
        // known" without allocating or parsing.
        if let Some(key) = origin_key(url) {
            match self.entries.get(key) {
                Some(Entry::Ready(tex)) => return Some(tex.clone()),
                Some(Entry::Loading { .. } | Entry::Failed) => return None,
                Some(Entry::BudgetWait { until }) if *until > std::time::Instant::now() => {
                    return None;
                }
                // Unknown origin, or a budget backoff whose cooldown
                // elapsed: start/retry the probe below.
                Some(Entry::BudgetWait { .. }) | None => {}
            }
        }
        // The cache is saturated with icons that cannot be evicted:
        // answering with the fallback glyph is the only possible outcome,
        // so skip the parse/allocation the unknown-origin path would do.
        if self.at_capacity {
            return None;
        }
        let parsed = Url::parse(url).ok()?;
        self.request(&parsed);
        let key = Self::key(&parsed);
        match self.entries.get(&key) {
            Some(Entry::Ready(tex)) => Some(tex.clone()),
            _ => None,
        }
    }

    /// Requests the favicon for `origin` unless already known/queued.
    /// Safe to call every frame: it is a no-op after the first attempt
    /// (except for a budget backoff whose cooldown elapsed: VRAM was tight,
    /// the decoded icon must not be lost for the whole session).
    pub fn request(&mut self, origin: &Url) {
        if origin.scheme() != "https" {
            return;
        }
        // Borrowed key on the normalized spelling: a settled frame performs
        // one hash probe instead of allocating a `String` per tab per frame.
        let owned;
        let key: &str = match origin_key(origin.as_str()) {
            Some(key) => key,
            None => {
                owned = Self::key(origin);
                &owned
            }
        };
        let retry_after_budget = match self.entries.get(key) {
            None => false,
            Some(Entry::BudgetWait { until }) if *until <= std::time::Instant::now() => true,
            Some(_) => return,
        };
        if retry_after_budget {
            self.entries.remove(key);
            self.backoff_count = self.backoff_count.saturating_sub(1);
        }
        // Entries are never evicted except for failed probes: a transient
        // failure must not permanently exhaust the origin cap. A session
        // that fills every slot with READY icons simply stops probing new
        // origins (512 of them is far past a tab rail's working set); the
        // cache is session-scoped and dropped with the app.
        if self.entries.len() >= config::MAX_FAVICON_HOSTS {
            let failed = self.entries.iter().find_map(|(k, e)| match e {
                Entry::Failed => Some(k.clone()),
                _ => None,
            });
            match failed {
                Some(k) => {
                    self.entries.remove(&k);
                    self.failed_count = self.failed_count.saturating_sub(1);
                }
                None => {
                    // Nothing evictable: remember it so every later unknown
                    // origin answers without parsing (cleared as soon as a
                    // slot frees or a probe fails).
                    self.at_capacity = true;
                    return;
                }
            }
        }
        // Wait-free slot: if all image jobs are busy, remember the origin
        // with a short backoff instead of recording a failure, so the next
        // frames answer without re-parsing the URL and `poll` retries the
        // probe when the cooldown passes.
        let Some(slot) = try_acquire_image_slot() else {
            let until = config::deadline_after(config::IMAGE_BUDGET_RETRY_SECS);
            self.entries
                .insert(key.to_owned(), Entry::BudgetWait { until });
            self.backoff_count += 1;
            self.next_backoff = Some(self.next_backoff.map_or(until, |cur| cur.min(until)));
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let job_cancel = cancel.clone();
        let probe = origin.clone();
        let promise = Promise::spawn_thread("fetch-favicon", move || {
            let _slot = slot;
            fetch::fetch_favicon_blocking(&probe, job_cancel)
        });
        self.entries.insert(
            key.to_owned(),
            Entry::Loading {
                promise,
                cancel,
                since: std::time::Instant::now(),
            },
        );
        self.loading_count += 1;
    }

    /// Collects finished probes and uploads textures. `true` on change.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        // O(1) settled gate: with no probe in flight and no backoff pending
        // there is nothing to upload, expire or retry. Without it, a settled
        // cache walked every entry up to four times per frame (poll runs for
        // the whole session, per frame, settled included).
        if self.loading_count == 0 && self.backoff_count == 0 {
            return false;
        }
        let mut changed = false;
        if self.loading_count > 0 {
            // Ready first: a resolved probe must be uploaded, never expired, even
            // when it resolves between the readiness and deadline snapshots.
            let ready_keys: Vec<String> = self
                .entries
                .iter()
                .filter_map(|(key, entry)| match entry {
                    Entry::Loading { promise, .. } if promise.ready().is_some() => {
                        Some(key.clone())
                    }
                    _ => None,
                })
                .collect();
            for key in ready_keys {
                let Some(Entry::Loading {
                    promise,
                    cancel,
                    since,
                }) = self.entries.remove(&key)
                else {
                    continue;
                };
                self.loading_count = self.loading_count.saturating_sub(1);
                match promise.try_take() {
                    Ok(Ok(decoded)) => {
                        // Icons share the process-wide texture budget like every
                        // other uploaded image: `MAX_FAVICON_HOSTS` icons must not
                        // add up outside the documented VRAM bound. An exhausted
                        // budget is TRANSIENT (other documents release their share
                        // on navigation), so back off and retry instead of
                        // dropping the decoded icon for the whole session.
                        let bytes = decoded
                            .pixels
                            .size
                            .iter()
                            .copied()
                            .product::<usize>()
                            .saturating_mul(4);
                        if !super::try_reserve_texture_bytes(bytes) {
                            let attempts = self.budget_attempts.get(&key).copied().unwrap_or(0);
                            if attempts < config::MAX_IMAGE_BUDGET_RETRIES {
                                self.budget_attempts.insert(key.clone(), attempts + 1);
                                let until = config::deadline_after(config::IMAGE_BUDGET_RETRY_SECS);
                                self.entries.insert(key, Entry::BudgetWait { until });
                                self.backoff_count += 1;
                                self.next_backoff =
                                    Some(self.next_backoff.map_or(until, |cur| cur.min(until)));
                            } else {
                                self.budget_attempts.remove(&key);
                                self.entries.insert(key, Entry::Failed);
                                self.failed_count += 1;
                            }
                            changed = true;
                            continue;
                        }
                        self.budget_attempts.remove(&key);
                        self.reserved_total = self.reserved_total.saturating_add(bytes);
                        let name = format!("favicon-{:016x}", super::fnv1a(&key));
                        let tex =
                            ctx.load_texture(name, decoded.pixels, egui::TextureOptions::LINEAR);
                        self.entries.insert(key, Entry::Ready(tex));
                        changed = true;
                    }
                    Ok(Err(_)) => {
                        self.budget_attempts.remove(&key);
                        self.entries.insert(key, Entry::Failed);
                        self.failed_count += 1;
                        changed = true;
                    }
                    Err(promise) => {
                        // Race: keep the ORIGINAL deadline (re-anchoring `since`
                        // would postpone the timeout on every flap).
                        self.entries.insert(
                            key,
                            Entry::Loading {
                                promise,
                                cancel,
                                since,
                            },
                        );
                        self.loading_count += 1;
                    }
                }
            }
            // Deadline: an undecodable/probe-stuck icon must not keep the repaint
            // scheduler alive forever (mirror of the image cache rule).
            let timeout = std::time::Duration::from_secs(config::IMAGE_JOB_TIMEOUT_SECS);
            let expired: Vec<String> = self
                .entries
                .iter()
                .filter(|(_, entry)| match entry {
                    Entry::Loading { promise, since, .. } => {
                        promise.ready().is_none() && since.elapsed() > timeout
                    }
                    _ => false,
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in expired {
                // Take the entry so the promise can be inspected without a race:
                // if it resolved between the readiness snapshot and here,
                // `try_take` hands back the result and it is re-armed as a ready
                // entry (the next poll uploads it) instead of being discarded as
                // a timeout. A still-pending probe is cancelled and failed.
                let Some(entry) = self.entries.remove(&key) else {
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
                                key,
                                Entry::Loading {
                                    promise: poll_promise::Promise::from_ready(result),
                                    cancel,
                                    since,
                                },
                            );
                            continue;
                        }
                        cancel.store(true, Ordering::Relaxed);
                        self.entries.insert(key, Entry::Failed);
                        self.failed_count += 1;
                        changed = true;
                    }
                    other => {
                        self.entries.insert(key, other);
                    }
                }
            }
        }
        // Budget backoffs whose cooldown elapsed are RETRIED HERE, not only
        // from `request`: the repaint the backoff schedules is the only
        // frame an origin nobody renders any more (a closed tab, a tile
        // scrolled out) would ever get, so waiting for a later `request`
        // left `has_pending` true forever (an idle 8 fps loop) and the icon
        // unresolved. Bounded by `MAX_IMAGE_BUDGET_RETRIES`; past the cap
        // the origin FAILS and stops counting as pending. The walk only runs
        // once the EARLIEST deadline passes (inserts keep it updated), never
        // per frame while a cooldown runs.
        let now = std::time::Instant::now();
        if self.backoff_count > 0 && self.next_backoff.is_some_and(|at| at <= now) {
            let mut due: Vec<String> = Vec::new();
            let mut next: Option<std::time::Instant> = None;
            for (key, entry) in &self.entries {
                if let Entry::BudgetWait { until } = entry {
                    if *until <= now {
                        due.push(key.clone());
                    } else {
                        next = Some(next.map_or(*until, |cur| cur.min(*until)));
                    }
                }
            }
            self.next_backoff = next;
            for key in due {
                let attempts = self.budget_attempts.get(&key).copied().unwrap_or(0);
                if attempts >= config::MAX_IMAGE_BUDGET_RETRIES {
                    self.budget_attempts.remove(&key);
                    self.backoff_count = self.backoff_count.saturating_sub(1);
                    self.entries.insert(key, Entry::Failed);
                    self.failed_count += 1;
                    changed = true;
                    continue;
                }
                if let Ok(origin) = Url::parse(&key) {
                    // `request` removes the due BudgetWait (counting down the
                    // backoff) and starts the probe, or re-arms a fresh
                    // backoff when the slot limiter is still saturated.
                    self.request(&origin);
                }
            }
        }
        // A slot may have freed (another tab's navigation released budget):
        // the saturated-cache flag is O(1) from the counters above, so an
        // unknown origin resumes trying without a per-frame walk.
        self.at_capacity =
            self.entries.len() >= config::MAX_FAVICON_HOSTS && self.failed_count == 0;
        changed
    }

    /// `true` while any favicon probe is in flight (repaint scheduler), or
    /// while one is backing off a full texture budget: the backoff is a
    /// deadline `poll` retries from (the repaint keeps those polls coming,
    /// or an idle window would leave the icon unresolved until the next
    /// input event). O(1): the counters are maintained on every transition.
    pub fn has_pending(&self) -> bool {
        self.loading_count > 0 || self.backoff_count > 0
    }

    /// Cancels in-flight probes and forgets every icon.
    pub fn cancel_all(&mut self) {
        for (_, entry) in self.entries.drain() {
            if let Entry::Loading { cancel, .. } = entry {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.budget_attempts.clear();
        self.loading_count = 0;
        self.backoff_count = 0;
        self.next_backoff = None;
        self.failed_count = 0;
        self.at_capacity = false;
        // Textures are dropped with the entries; release their share of the
        // process-wide budget so images can reuse it.
        if self.reserved_total > 0 {
            super::release_texture_bytes(self.reserved_total);
            self.reserved_total = 0;
        }
    }

    /// Origin key: no path, no credentials (`https://ex.com:8443`).
    fn key(origin: &Url) -> String {
        origin.origin().ascii_serialization()
    }
}

/// Origin key (`https://host[:port]`) derived from a URL STRING without
/// parsing or allocating it: the bytes `Url::origin().ascii_serialization()`
/// would produce for a normalized `https://` URL.
///
/// Path/query/fragment are cut, a default `:443` port and any
/// credentials-bearing authority make the helper refuse (`None`) so the
/// caller falls back to the parser instead of keying a different origin.
/// A non-lowercase authority is refused too: the parser lowercases the host,
/// so keying `https://EX.com` here would split one origin into two cache
/// entries (and two probes). The returned slice borrows `url`, which is what
/// makes the per-frame lookups (`FaviconCache::texture`/`texture_for_url`)
/// allocation-free.
fn origin_key(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..authority_end)?;
    if authority.contains('@') {
        return None;
    }
    if authority.is_empty() || authority.ends_with(":443") {
        return None;
    }
    if authority.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    url.get(.."https://".len() + authority_end)
}

impl Drop for FaviconCache {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::TEXTURE_TEST_LOCK;

    fn loading(entry_age_secs: u64, promise: Promise<Result<DecodedImage, String>>) -> Entry {
        Entry::Loading {
            promise,
            cancel: Arc::new(AtomicBool::new(false)),
            since: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(entry_age_secs))
                .expect("instant arithmetic"),
        }
    }

    fn resolved(pixels: usize) -> Promise<Result<DecodedImage, String>> {
        Promise::from_ready(Ok(DecodedImage {
            pixels: egui::ColorImage::new(
                [pixels, pixels],
                vec![egui::Color32::WHITE; pixels * pixels],
            ),
            vector: false,
        }))
    }

    fn resolves_later() -> Result<DecodedImage, String> {
        std::thread::sleep(std::time::Duration::from_secs(2));
        Err("still running".to_string())
    }

    /// Test-only: stages one entry and keeps the O(1) counters in step with
    /// it (tests build states directly instead of driving `request`/`poll`
    /// transitions).
    fn set_entry(cache: &mut FaviconCache, key: &str, entry: Entry) {
        if let Some(old) = cache.entries.insert(key.to_string(), entry) {
            match old {
                Entry::Loading { .. } => {
                    cache.loading_count = cache.loading_count.saturating_sub(1);
                }
                Entry::BudgetWait { .. } => {
                    cache.backoff_count = cache.backoff_count.saturating_sub(1);
                }
                Entry::Failed => cache.failed_count = cache.failed_count.saturating_sub(1),
                Entry::Ready(_) => {}
            }
        }
        match cache.entries.get(key) {
            Some(Entry::Loading { .. }) => cache.loading_count += 1,
            Some(Entry::BudgetWait { until }) => {
                cache.backoff_count += 1;
                cache.next_backoff = Some(cache.next_backoff.map_or(*until, |cur| cur.min(*until)));
            }
            Some(Entry::Failed) => cache.failed_count += 1,
            Some(Entry::Ready(_)) | None => {}
        }
    }

    /// A probe that resolved between the deadline snapshot and the failure
    /// insertion must be uploaded, never replaced by `Failed`: the old code
    /// dropped the decoded pixels and showed a permanent failure icon.
    #[test]
    fn resolved_past_deadline_is_uploaded_not_failed() {
        let _lock = TEXTURE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = egui::Context::default();
        let mut cache = FaviconCache::default();
        let expired = config::IMAGE_JOB_TIMEOUT_SECS + 5;
        set_entry(&mut cache, "https://ex.com", loading(expired, resolved(8)));
        assert!(cache.poll(&ctx), "upload must report a change");
        assert!(matches!(
            cache.entries.get("https://ex.com"),
            Some(Entry::Ready(_))
        ));
        assert!(cache.reserved_total > 0, "upload must reserve its bytes");
    }

    /// A budget backoff is not a running job, but it IS a deadline the app
    /// must wake up for (its retry runs on a later frame); `request` only
    /// re-probes once the cooldown elapsed.
    #[test]
    fn budget_backoff_is_retried_only_after_the_cooldown() {
        let mut cache = FaviconCache::default();
        let origin = Url::parse("https://ex.com/a").expect("url");
        let key = "https://ex.com".to_string();
        // Cooldown still running: every `request` is a no-op.
        set_entry(
            &mut cache,
            &key,
            Entry::BudgetWait {
                until: std::time::Instant::now() + std::time::Duration::from_secs(60),
            },
        );
        assert!(
            cache.has_pending(),
            "a backoff deadline must keep the repaint scheduler awake"
        );
        assert!(cache.texture(&origin).is_none(), "no texture yet");
        cache.request(&origin);
        assert!(matches!(
            cache.entries.get(&key),
            Some(Entry::BudgetWait { .. })
        ));
        // Cooldown elapsed: the wait entry is dropped and a fresh probe is
        // allowed (the global slot limiter may still defer it, so only the
        // transition away from `BudgetWait` is asserted).
        set_entry(
            &mut cache,
            &key,
            Entry::BudgetWait {
                until: std::time::Instant::now(),
            },
        );
        cache.request(&origin);
        assert!(!matches!(
            cache.entries.get(&key),
            Some(Entry::BudgetWait { .. })
        ));
    }

    /// An expired budget backoff is retried by `poll` ITSELF, not only by a
    /// later `request`: the repaint the backoff schedules is the only frame a
    /// closed tab's (or scrolled-out tile's) origin ever gets, so waiting for a
    /// `request` left `has_pending` true forever (an idle 8 fps loop) with the
    /// icon unresolved.
    #[test]
    fn expired_budget_backoff_is_retried_by_poll() {
        let _lock = TEXTURE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = egui::Context::default();
        let mut cache = FaviconCache::default();
        let key = "https://ex.com".to_string();
        set_entry(
            &mut cache,
            &key,
            Entry::BudgetWait {
                until: std::time::Instant::now(),
            },
        );
        cache.poll(&ctx);
        assert!(
            !matches!(cache.entries.get(&key), Some(Entry::BudgetWait { .. })),
            "the due backoff is retried without another request"
        );
    }

    /// A budget backoff past the attempt cap fails from `poll` as well, so an
    /// origin nobody requests any more stops counting as pending.
    #[test]
    fn exhausted_budget_backoff_fails_from_poll() {
        let ctx = egui::Context::default();
        let mut cache = FaviconCache::default();
        let key = "https://ex.com".to_string();
        set_entry(
            &mut cache,
            &key,
            Entry::BudgetWait {
                until: std::time::Instant::now(),
            },
        );
        cache
            .budget_attempts
            .insert(key.clone(), config::MAX_IMAGE_BUDGET_RETRIES);
        cache.poll(&ctx);
        assert!(matches!(cache.entries.get(&key), Some(Entry::Failed)));
        assert!(!cache.has_pending(), "a failed origin is not pending");
    }

    /// `texture_for_url` (the Home bookmark-tile path) answers from the
    /// cache without a `Url::parse` per frame once the origin is known, and
    /// a `cancel_all` (navigation) drops the probe so the tile re-requests
    /// it instead of staying iconless for the session.
    #[test]
    fn texture_for_url_skips_known_origins_and_survives_cancel_all() {
        let _lock = TEXTURE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = egui::Context::default();
        let mut cache = FaviconCache::default();
        // Unknown origin: the first call requests a probe (no texture yet).
        assert!(cache.texture_for_url("https://ex.com/a").is_none());
        assert!(
            matches!(
                cache.entries.get("https://ex.com"),
                Some(Entry::Loading { .. })
            ),
            "the first call must start a probe"
        );
        // Second call: still no texture, but the probe is not restarted.
        let before = cache.entries.len();
        assert!(cache.texture_for_url("https://ex.com/b").is_none());
        assert_eq!(cache.entries.len(), before, "no duplicate probe");
        // Non-https pages never probe.
        assert!(cache.texture_for_url("file:///tmp/notes.md").is_none());
        assert!(!cache.entries.contains_key("file://"));
        // A resolved entry answers the texture.
        set_entry(
            &mut cache,
            "https://ex.com",
            Entry::Ready(ctx.load_texture(
                "test-favicon",
                egui::ColorImage::new([2, 2], vec![egui::Color32::WHITE; 4]),
                egui::TextureOptions::LINEAR,
            )),
        );
        assert!(cache.texture_for_url("https://ex.com/c").is_some());
        // cancel_all drops it; the next call re-probes.
        cache.cancel_all();
        assert!(cache.entries.is_empty());
        assert!(cache.texture_for_url("https://ex.com/a").is_none());
        assert!(
            matches!(
                cache.entries.get("https://ex.com"),
                Some(Entry::Loading { .. })
            ),
            "a cancelled origin must be re-requested"
        );
    }

    /// The string-side origin key must agree with the parsed-side key for
    /// every normal URL (the fast path's whole contract), including ports
    /// and subdomains. Credentials and default ports make the helper refuse
    /// so the parsed fallback decides (never a different key).
    #[test]
    fn origin_key_matches_the_parsed_origin() {
        for url in [
            "https://ex.com/a",
            "https://ex.com",
            "https://ex.com:8443/path?q=1#frag",
            "https://sub.domain.example.org/x/y",
        ] {
            let parsed = Url::parse(url).expect("url");
            assert_eq!(
                origin_key(url),
                Some(FaviconCache::key(&parsed).as_str()),
                "string key must match the parsed key for {url}"
            );
        }
        // Credentials are never keyed by slice: the caller falls back to the
        // parser, which drops userinfo before building the key.
        assert_eq!(origin_key("https://user:pass@ex.com/page"), None);
        // The default port serializes away; the helper refuses so the
        // parsed fallback decides (never a different key).
        assert_eq!(origin_key("https://ex.com:443/a"), None);
        assert_eq!(origin_key("http://ex.com/a"), None);
        assert_eq!(origin_key("file:///tmp/x.md"), None);
        // A non-lowercase authority would key a DIFFERENT origin than the
        // parser's lowercase one (two cache entries, two probes for one
        // site), so the helper refuses it too.
        assert_eq!(origin_key("https://EX.com/a"), None);
    }

    /// A probe still pending past the deadline becomes `Failed` and its
    /// cancel flag is set (the deadline cannot keep the repaint loop alive).
    #[test]
    fn pending_past_deadline_fails_and_cancels() {
        let ctx = egui::Context::default();
        let mut cache = FaviconCache::default();
        let expired = config::IMAGE_JOB_TIMEOUT_SECS + 5;
        let entry = loading(
            expired,
            Promise::spawn_thread("test-favicon", resolves_later),
        );
        set_entry(&mut cache, "https://ex.com", entry);
        assert!(cache.poll(&ctx), "timeout must report a change");
        assert!(matches!(
            cache.entries.get("https://ex.com"),
            Some(Entry::Failed)
        ));
    }
}
