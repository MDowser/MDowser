// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::emoji`: color emoji rendered from the embedded Noto Color Emoji.
//!
//! epaint's text stack rasterizes outline glyphs into a monochrome coverage
//! atlas, so Noto Color Emoji (COLRv1 + SVG-in-OpenType, no usable outlines)
//! cannot go through the normal font fallback chains. Instead the viewer and
//! the user glyphs of `ui::icons::paint_text` (bookmark tiles) draw emoji
//! as textures:
//!
//! 1. A grapheme cluster is recognized as emoji (`unicode-segmentation` +
//!    the `emojis` crate data, which covers ZWJ sequences, flags, keycaps
//!    and skin-tone modifiers).
//! 2. The cluster is shaped with harfrust against the embedded font, so
//!    ligature glyphs (a whole family, a flag) resolve to one glyph id.
//! 3. The glyph's description is isolated from the font's SVG table
//!    (read-fonts): Noto shares one giant document between thousands of
//!    glyphs, so the element `id="glyph<gid>"` is extracted together with
//!    the transitive closure of its `#id` references (gradients, `<use>`
//!    components), and the em-square viewBox the OpenType SVG spec defines
//!    is injected when the document has none.
//! 4. The document is rasterized with the same hardened `resvg` pipeline
//!    used for SVG images and cached as an egui texture per
//!    (cluster, device-px size) under `config::EMOJI_CACHE_*`, reserved from
//!    the process-wide texture budget (the viewer buckets text sizes). When
//!    anything fails the caller falls back to the monochrome text path, so
//!    a document never breaks because of an emoji.
//!
//! Copy text is unaffected: `plain_text` still carries the emoji characters.

use crate::config;
use eframe::egui;
use poll_promise::Promise;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// One piece of a text fragment: plain text or one emoji cluster.
pub(crate) enum Segment<'a> {
    Text(&'a str),
    Emoji(&'a str),
}

/// True when `text` holds any codepoint that can begin or join a COLOR emoji
/// cluster recognized below: a pictograph at/above U+1F000, a variation
/// selector (VS16), a ZWJ joiner or the keycap combining mark. Every cluster
/// `is_color_emoji` accepts contains at least one of them (lower single
/// codepoints are rejected there by design), so this is a safe pre-filter:
/// CJK, Cyrillic, Arabic and other emoji-free text skips the per-grapheme
/// segmentation and table lookups entirely.
fn has_color_emoji_candidate(text: &str) -> bool {
    text.chars().any(|c| {
        let u = c as u32;
        u >= 0x1F000 || matches!(u, 0xFE0F | 0x200D | 0x20E3)
    })
}

/// True when `text` can contain a color emoji cluster (the cheap scalar
/// pre-filter of [`for_each_segment`], exposed for the table width cache: a
/// table with emoji is not cacheable because the measured width depends on
/// whether the emoji texture has resolved).
pub(crate) fn may_contain_emoji(text: &str) -> bool {
    has_color_emoji_candidate(text)
}

/// True when `text` holds at least one color-emoji cluster. Allocation-free
/// (the scalar pre-filter first, then one grapheme walk), so the viewer can
/// probe before drawing: an emoji-free fragment takes its normal text path
/// without building a segment list.
pub(crate) fn has_color_emoji(text: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation as _;
    if text.is_ascii() || !has_color_emoji_candidate(text) {
        return false;
    }
    text.graphemes(true).any(is_color_emoji)
}

/// Calls `f` for each text/emoji piece of `text`, in order, WITHOUT
/// allocating: the viewer draws straight from this walk, while the old
/// `Vec`-building split allocated one vector per fragment per repaint.
pub(crate) fn for_each_segment<'a>(text: &'a str, mut f: impl FnMut(Segment<'a>)) {
    use unicode_segmentation::UnicodeSegmentation as _;
    // ASCII can never hold (or combine into) a color-emoji cluster: skip the
    // grapheme segmentation + emoji-table lookup for the common case. The
    // scalar pre-scan covers non-ASCII text without a single candidate
    // (CJK, Cyrillic, accented Latin, RTL), which used to pay the full
    // segmentation per fragment per repaint for nothing.
    if text.is_ascii() || !has_color_emoji_candidate(text) {
        f(Segment::Text(text));
        return;
    }
    let mut start = 0;
    for (i, grapheme) in text.grapheme_indices(true) {
        if is_color_emoji(grapheme) {
            if start < i {
                f(Segment::Text(&text[start..i]));
            }
            f(Segment::Emoji(grapheme));
            start = i + grapheme.len();
        }
    }
    if start < text.len() {
        f(Segment::Text(&text[start..]));
    }
}

/// Splits `text` into text and emoji clusters, in order (test helper; the
/// viewer walks [`for_each_segment`] to avoid the `Vec`).
#[cfg(test)]
pub(crate) fn split(text: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    for_each_segment(text, |segment| out.push(segment));
    out
}

/// Color-emoji heuristic: known emoji, with an explicit presentation
/// selector or a multi-codepoint sequence, or a pictograph codepoint.
fn is_color_emoji(grapheme: &str) -> bool {
    if emojis::get(grapheme).is_none() {
        return false;
    }
    let mut chars = grapheme.chars();
    let first = chars.next().unwrap_or(' ');
    if chars.next().is_some() {
        return true;
    }
    (first as u32) >= 0x1F000
}

/// Device-pixel side for one emoji texture drawn by the VIEWER at `size`
/// points: scaled by the pixels-per-point, rounded up to
/// `config::EMOJI_TEXTURE_PX_BUCKET` and clamped, so zoom levels share
/// cached textures across text of nearby sizes. `ui::icons` keeps its own
/// finer grid: chrome glyphs must land 1:1 to stay crisp.
pub(crate) fn device_px(size: f32, ppp: f32) -> u32 {
    let raw = (size * ppp).ceil() as u32;
    let bucket = config::EMOJI_TEXTURE_PX_BUCKET.max(1);
    raw.div_ceil(bucket)
        .saturating_mul(bucket)
        .clamp(config::EMOJI_TEXTURE_PX_MIN, config::EMOJI_TEXTURE_PX_MAX)
}

/// One cache slot for a `(cluster, device px)` pair.
enum Cached {
    /// Uploaded texture, the bytes it reserved in the process-wide pool and
    /// its last-use tick (LRU eviction).
    Texture(egui::TextureHandle, usize, u64),
    /// Rasterizing on a background worker. The second field is the number of
    /// process-budget refusals already spent on this cluster, so a saturated
    /// texture pool still gives up after `MAX_IMAGE_BUDGET_RETRIES`.
    Loading(Promise<Option<egui::ColorImage>>, u8),
    /// The cluster has no color document: a permanent miss.
    NoDocument,
    /// The process-wide texture budget was full: retried after
    /// `config::EMOJI_BUDGET_RETRY_SECS` so a transient saturation does not
    /// disable color emoji for the session, up to
    /// `config::MAX_IMAGE_BUDGET_RETRIES` attempts (then a permanent miss,
    /// so a saturated pool cannot keep re-rasterizing every frame).
    BudgetMiss(std::time::Instant, u8),
}

/// Process-wide count of in-flight emoji raster jobs, capped at
/// [`config::MAX_CONCURRENT_EMOJI`]. A cluster miss past the cap stays
/// UNREGISTERED (no slot inserted): a later frame retries it, which keeps a
/// page full of new emoji from spawning one thread per cluster.
#[cfg_attr(test, allow(dead_code))]
static EMOJI_JOBS: AtomicUsize = AtomicUsize::new(0);

/// Releases one job slot when the worker returns, panic included.
#[cfg_attr(test, allow(dead_code))]
struct EmojiJobGuard;

impl Drop for EmojiJobGuard {
    fn drop(&mut self) {
        EMOJI_JOBS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Bounded texture cache for color emoji, keyed by (cluster, device px).
///
/// The px level is the outer map so a hit can be looked up with the caller's
/// `&str` cluster: no key `String` is allocated per lookup (this runs for
/// every emoji cluster in the document, every frame).
pub(crate) struct EmojiCache {
    textures: HashMap<u32, HashMap<String, Cached>>,
    order: VecDeque<(u32, String)>,
    /// Keys of the slots currently rasterizing (`Cached::Loading`), in spawn
    /// order. Bounded by [`config::MAX_CONCURRENT_EMOJI`], so `poll` uploads
    /// only finished jobs instead of scanning every entry per frame, and
    /// `has_pending` answers in O(1).
    loading: Vec<(u32, String)>,
    bytes: usize,
    /// Monotonic use counter for LRU eviction.
    clock: u64,
    /// Bump when a cluster gains or loses its color texture (a table width
    /// measured against the old state is stale): the table width cache keys
    /// on it so media tables re-measure exactly once per transition instead
    /// of every frame. `NoDocument`/`BudgetMiss` traffic renders the same
    /// monochrome fallback and does not bump.
    resolved: u64,
}

impl Default for EmojiCache {
    fn default() -> Self {
        Self::new()
    }
}

impl EmojiCache {
    pub(crate) fn new() -> Self {
        Self {
            textures: HashMap::new(),
            order: VecDeque::new(),
            loading: Vec::new(),
            bytes: 0,
            clock: 0,
            resolved: 0,
        }
    }

    /// Geometry epoch for the table width cache (see the field docs).
    pub(crate) fn resolved_epoch(&self) -> u64 {
        self.resolved
    }

    /// Texture for one emoji cluster at `px` device pixels on its longest
    /// side, or `None` while it has no usable color texture.
    ///
    /// A miss rasters on a background worker (`MAX_CONCURRENT_EMOJI`
    /// process-wide): the caller keeps the monochrome fallback this frame and
    /// [`Self::poll`] uploads the texture later, so a burst of new emoji (or
    /// a zoom/DPI change re-bucketing every visible glyph) never rasterizes
    /// inside the frame.
    ///
    /// `None` also covers the permanent cases: no color document for the
    /// cluster, the texture budget exhausted (`BudgetMiss`, retried), or the
    /// worker cap reached (the miss stays UNREGISTERED, so a later frame
    /// retries it instead of spawning one thread per cluster).
    pub(crate) fn texture(
        &mut self,
        ctx: &egui::Context,
        cluster: &str,
        px: u32,
    ) -> Option<egui::TextureHandle> {
        self.clock = self.clock.wrapping_add(1);
        let now = self.clock;
        let mut budget_attempts = 0u8;
        if let Some(slot) = self.textures.get_mut(&px).and_then(|m| m.get_mut(cluster)) {
            match slot {
                Cached::Texture(texture, _, used) => {
                    *used = now;
                    return Some(texture.clone());
                }
                // Rasterizing on a worker: the fallback holds the line.
                Cached::Loading(_, _) | Cached::NoDocument => return None,
                // A fresh budget miss: wait out the retry delay, then retry.
                Cached::BudgetMiss(at, _)
                    if at.elapsed()
                        < std::time::Duration::from_secs(config::EMOJI_BUDGET_RETRY_SECS) =>
                {
                    return None;
                }
                Cached::BudgetMiss(_, attempts) => budget_attempts = *attempts,
            }
        }
        // Tests run synchronously: their assertions cover layout and the
        // resolution epochs, not the worker driver.
        #[cfg(test)]
        return self.render_sync(ctx, cluster, px, now, budget_attempts);
        #[cfg(not(test))]
        {
            // Uploads and repaints are driven by `poll`/`has_pending`; the
            // context is only used by the test-only synchronous path.
            let _ = ctx;
            // Past the process-wide job cap the miss stays unregistered; the
            // next frame (scheduled while any job is in flight) retries it.
            if EMOJI_JOBS.load(Ordering::SeqCst) >= config::MAX_CONCURRENT_EMOJI {
                return None;
            }
            EMOJI_JOBS.fetch_add(1, Ordering::SeqCst);
            let job_cluster = cluster.to_owned();
            let promise = Promise::spawn_thread("emoji-raster", move || {
                let _job = EmojiJobGuard;
                render_cluster(&job_cluster, px)
            });
            self.insert(px, cluster, Cached::Loading(promise, budget_attempts));
            None
        }
    }

    /// Test-only synchronous miss path: rasterize, reserve, upload, cache
    /// (the old inline behavior; production off-loads it to a worker).
    #[cfg(test)]
    fn render_sync(
        &mut self,
        ctx: &egui::Context,
        cluster: &str,
        px: u32,
        now: u64,
        previous_attempts: u8,
    ) -> Option<egui::TextureHandle> {
        let Some(image) = render_cluster(cluster, px) else {
            // Cache the miss: no document for this cluster, never retry.
            self.insert(px, cluster, Cached::NoDocument);
            return None;
        };
        let bytes = image.pixels.len() * 4;
        if !super::try_reserve_texture_bytes(bytes) {
            let attempts = previous_attempts;
            if attempts >= config::MAX_IMAGE_BUDGET_RETRIES {
                self.insert(px, cluster, Cached::NoDocument);
                return None;
            }
            self.insert(
                px,
                cluster,
                Cached::BudgetMiss(std::time::Instant::now(), attempts + 1),
            );
            ctx.request_repaint_after(std::time::Duration::from_secs(
                config::EMOJI_BUDGET_RETRY_SECS,
            ));
            return None;
        }
        let texture = ctx.load_texture(
            format!("emoji-{}", crate::media::fnv1a(cluster)),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.insert(px, cluster, Cached::Texture(texture.clone(), bytes, now));
        Some(texture)
    }

    /// Uploads every finished worker raster and re-arms budget retries.
    /// Returns `true` when a texture (or a terminal failure) changed what the
    /// next frame draws.
    ///
    /// Only the in-flight keys are visited (`self.loading`, bounded by the
    /// job cap): a settled cache never walks its entries.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        if self.loading.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut i = 0;
        while i < self.loading.len() {
            let ready = {
                let key = &self.loading[i];
                matches!(
                    self.textures.get(&key.0).and_then(|m| m.get(&key.1)),
                    Some(Cached::Loading(promise, _)) if promise.ready().is_some()
                )
            };
            if !ready {
                i += 1;
                continue;
            }
            let key = self.loading.remove(i);
            // Leave a placeholder while taking the promise, so `insert`
            // keeps treating this key as replaced (no duplicate order item).
            let taken = self
                .textures
                .get_mut(&key.0)
                .and_then(|m| m.get_mut(&key.1))
                .map(|slot| std::mem::replace(slot, Cached::NoDocument));
            let Some(Cached::Loading(promise, attempts)) = taken else {
                continue;
            };
            match promise.try_take() {
                Ok(Some(image)) => {
                    let bytes = image.pixels.len() * 4;
                    if super::try_reserve_texture_bytes(bytes) {
                        let texture = ctx.load_texture(
                            format!("emoji-{}", crate::media::fnv1a(&key.1)),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.insert(key.0, &key.1, Cached::Texture(texture, bytes, self.clock));
                    } else if attempts >= config::MAX_IMAGE_BUDGET_RETRIES {
                        // Give up for good: keep the monochrome fallback.
                        self.insert(key.0, &key.1, Cached::NoDocument);
                    } else {
                        self.insert(
                            key.0,
                            &key.1,
                            Cached::BudgetMiss(std::time::Instant::now(), attempts + 1),
                        );
                        ctx.request_repaint_after(std::time::Duration::from_secs(
                            config::EMOJI_BUDGET_RETRY_SECS,
                        ));
                    }
                    changed = true;
                }
                Ok(None) => {
                    // No color document for this cluster: permanent miss.
                    self.insert(key.0, &key.1, Cached::NoDocument);
                    changed = true;
                }
                Err(promise) => {
                    // Race: `ready()` said yes, `try_take` disagreed. Keep the
                    // slot loading and its key queued.
                    self.insert(key.0, &key.1, Cached::Loading(promise, attempts));
                    self.loading.push(key);
                }
            }
        }
        changed
    }

    /// `true` while any raster job is running (repaint scheduler).
    pub(crate) fn has_pending(&self) -> bool {
        !self.loading.is_empty()
    }

    /// Inserts or replaces one slot, then evicts past the bounds. The order
    /// queue holds each key at most once (replacing a slot keeps its
    /// position), so eviction never releases one reservation twice.
    fn insert(&mut self, px: u32, cluster: &str, entry: Cached) {
        let incoming_texture = matches!(entry, Cached::Texture(..));
        if let Cached::Texture(_, reserved, _) = &entry {
            self.bytes = self.bytes.saturating_add(*reserved);
        }
        let replaced = self
            .textures
            .entry(px)
            .or_default()
            .insert(cluster.to_owned(), entry);
        match replaced {
            // Defensive: replacing a live texture under the same key must
            // release its reservation even though today's call sites only
            // replace NoDocument/BudgetMiss slots.
            Some(Cached::Texture(_, reserved, _)) => {
                self.bytes = self.bytes.saturating_sub(reserved);
                super::release_texture_bytes(reserved);
            }
            // A cluster that gains its color texture renders at a different
            // width: table measurements against the fallback are stale.
            // (Texture-over-texture keeps the same raster size; fallback
            // traffic renders the same monochrome text either way.)
            Some(_) if incoming_texture => {
                self.resolved = self.resolved.wrapping_add(1);
            }
            Some(_) => {}
            None => self.order.push_back((px, cluster.to_owned())),
        }
        self.evict();
    }

    /// Drops entries until both the count and byte bounds hold, least
    /// recently used first (a cluster used every frame survives a burst of
    /// one-shot emoji), releasing each evicted texture's reservation.
    ///
    /// One O(n) pass per victim (the old code scanned the queue once to pick
    /// and again to remove, ~1M comparisons per insert at the 1024-entry cap
    /// on the UI thread).
    fn evict(&mut self) {
        while self.order.len() > config::EMOJI_CACHE_ENTRIES
            || self.bytes > config::EMOJI_CACHE_BYTES
        {
            let Some(victim) = self.oldest_entry() else {
                break;
            };
            self.drop_entry(&victim);
        }
    }

    /// The least-recently-used key: a non-texture slot (no last-use tick) is
    /// always the better victim, else the oldest texture tick.
    fn oldest_entry(&self) -> Option<(u32, String)> {
        let mut best: Option<((u32, String), u64)> = None;
        for (px, cluster) in &self.order {
            let used = match self.textures.get(px).and_then(|m| m.get(cluster)) {
                Some(Cached::Texture(_, _, used)) => *used,
                // Non-texture slots carry no last use: always the first
                // victims (tick 0 is older than any live texture's).
                _ => 0,
            };
            if best.as_ref().is_none_or(|(_, at)| used < *at) {
                best = Some(((px.to_owned(), cluster.clone()), used));
            }
        }
        best.map(|(key, _)| key)
    }

    /// Removes one key from both the order queue and the texture map,
    /// releasing its reservation (and invalidating table measurements when
    /// a color texture falls back to monochrome text).
    fn drop_entry(&mut self, victim: &(u32, String)) {
        if let Some(pos) = self.order.iter().position(|key| key == victim) {
            self.order.remove(pos);
        }
        let removed = self
            .textures
            .get_mut(&victim.0)
            .and_then(|m| m.remove(&victim.1));
        match removed {
            Some(Cached::Texture(_, reserved, _)) => {
                self.bytes = self.bytes.saturating_sub(reserved);
                super::release_texture_bytes(reserved);
                // The cluster falls back to monochrome text, which lays out
                // at a different width: stale table measurements must miss.
                self.resolved = self.resolved.wrapping_add(1);
            }
            // An evicted in-flight job is no longer uploadable: forget its
            // key so `has_pending`/`poll` never track a slot that is gone
            // (the worker finishes on its own and releases its job slot).
            Some(Cached::Loading(..)) => {
                self.loading.retain(|key| key != victim);
            }
            _ => {}
        }
        // Drop the now-empty px level so the map stays lean.
        if self
            .textures
            .get(&victim.0)
            .is_some_and(std::collections::HashMap::is_empty)
        {
            self.textures.remove(&victim.0);
        }
    }
}

impl Drop for EmojiCache {
    fn drop(&mut self) {
        if self.bytes > 0 {
            super::release_texture_bytes(self.bytes);
        }
    }
}

/// Warms the color font, its shared SVG document index and the hardened
/// rasterizer on a background thread (`App::new`): the first emoji (document
/// text or a chrome icon) would otherwise parse the font's shared XML
/// mid-frame on the UI thread. Failures are irrelevant (the monochrome
/// fallback still works).
pub(crate) fn prewarm() {
    let _ = render_cluster("😀", 32);
}

/// The parsed color font, its shaper caches and vertical metrics.
struct EmojiFont {
    font: skrifa::FontRef<'static>,
    shaper: harfrust::ShaperData,
    upem: f32,
    ascent: f32,
    descent: f32,
}

fn emoji_font() -> Option<&'static EmojiFont> {
    static FONT: OnceLock<Option<EmojiFont>> = OnceLock::new();
    FONT.get_or_init(|| {
        use skrifa::raw::TableProvider as _;
        use skrifa::MetadataProvider as _;
        let font = skrifa::FontRef::new(crate::assets::noto_color_emoji()).ok()?;
        // No `svg()` table means this is not a color font we can use.
        font.svg().ok()?;
        let metrics = font.metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        );
        let shaper = harfrust::ShaperData::new(&font);
        Some(EmojiFont {
            font,
            shaper,
            upem: metrics.units_per_em as f32,
            ascent: metrics.ascent,
            descent: metrics.descent,
        })
    })
    .as_ref()
}

/// Rasterizes one emoji cluster into a color image.
fn render_cluster(cluster: &str, px: u32) -> Option<egui::ColorImage> {
    let font = emoji_font()?;
    let glyphs = shape_cluster(font, cluster);
    match glyphs.as_slice() {
        [] => None,
        [(gid, advance)] => rasterize_glyph(font, *gid, *advance, px),
        // A sequence without a ligature glyph: compose the pieces left to
        // right at their shaped advances.
        _ => compose_glyphs(font, &glyphs, px),
    }
}

/// Shapes the cluster to `(glyph id, x advance)` pairs (font units).
fn shape_cluster(font: &EmojiFont, cluster: &str) -> Vec<(u32, i32)> {
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str(cluster);
    buffer.guess_segment_properties();
    let shaper = font.shaper.shaper(&font.font).build();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
    shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .filter(|(info, _)| info.glyph_id != 0)
        .map(|(info, pos)| (info.glyph_id, pos.x_advance))
        .collect()
}

/// Index of one SVG document from the font: element byte ranges by id and
/// by glyph id, plus the document's own viewBox (when it declares one).
struct DocIndex {
    ids: HashMap<String, Range<usize>>,
    glyphs: HashMap<u32, Range<usize>>,
    root_range: Range<usize>,
    viewbox: Option<String>,
}

/// Parsed document indexes. `cache_key` is the document slice identity for
/// slices borrowed from the embedded font (the giant shared document is
/// parsed once, not per glyph); owned (gzip-decoded) documents parse fresh.
fn doc_index(doc: &str, cache_key: Option<(usize, usize)>) -> Option<Arc<DocIndex>> {
    type DocIndexes = Mutex<HashMap<(usize, usize), Arc<DocIndex>>>;
    static INDEXES: OnceLock<DocIndexes> = OnceLock::new();
    let indexes = INDEXES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(key) = cache_key {
        if let Some(index) = indexes.lock().ok()?.get(&key) {
            return Some(index.clone());
        }
    }
    let parsed = roxmltree::Document::parse(doc).ok()?;
    let root = parsed.root_element();
    let mut ids = HashMap::new();
    let mut glyphs = HashMap::new();
    for node in parsed.descendants().filter(|n| n.is_element()) {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let range = node.range();
        if let Some(gid) = id.strip_prefix("glyph").and_then(|n| n.parse::<u32>().ok()) {
            glyphs.entry(gid).or_insert_with(|| range.clone());
        }
        ids.entry(id.to_owned()).or_insert(range);
    }
    let index = Arc::new(DocIndex {
        ids,
        glyphs,
        root_range: root.range(),
        viewbox: root.attribute("viewBox").map(str::to_owned),
    });
    if let Some(key) = cache_key {
        indexes.lock().ok()?.insert(key, index.clone());
    }
    Some(index)
}

/// Rasterizes one glyph's description at `px` on its longest side.
fn rasterize_glyph(font: &EmojiFont, gid: u32, advance: i32, px: u32) -> Option<egui::ColorImage> {
    let document = glyph_document(font, gid, advance)?;
    super::decode::decode_svg_pixels(&document, px).ok()
}

/// Isolates one glyph's SVG document: the element `id="glyph<gid>"` plus the
/// transitive closure of the `#id` references it uses, inside a root that
/// carries the em-square viewBox (or the document's own viewBox).
fn glyph_document(font: &EmojiFont, gid: u32, advance: i32) -> Option<Vec<u8>> {
    use skrifa::raw::TableProvider as _;
    let raw = font
        .font
        .svg()
        .ok()?
        .glyph_data(skrifa::GlyphId::new(gid))
        .ok()
        .flatten()?;
    // Uncompressed documents borrow the embedded font data (cacheable by
    // slice identity); gzip documents decode into an owned buffer.
    let owned;
    let (bytes, cache_key): (&[u8], Option<(usize, usize)>) = if raw.starts_with(&[0x1F, 0x8B]) {
        owned = decompress(raw)?;
        (&owned, None)
    } else {
        (raw, Some((raw.as_ptr() as usize, raw.len())))
    };
    let doc = std::str::from_utf8(bytes).ok()?;
    let index = doc_index(doc, cache_key)?;
    let target = index.glyphs.get(&gid)?.clone();

    // A document whose root IS the glyph: keep it whole, just add the em
    // viewBox when it has none.
    if target == index.root_range {
        let mut out = doc.to_owned();
        if index.viewbox.is_none() {
            out = inject_viewbox(&out, &em_viewbox(font, advance))?;
        }
        return Some(out.into_bytes());
    }

    // Otherwise wrap the glyph element with the closure of the elements its
    // `#id` references need (gradients, components, ...).
    let mut defs: Vec<&str> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = referenced_ids(&doc[target.clone()])
        .map(str::to_owned)
        .collect();
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(range) = index.ids.get(&id) else {
            continue;
        };
        if *range == index.root_range || *range == target {
            continue;
        }
        let source = &doc[range.clone()];
        defs.push(source);
        queue.extend(referenced_ids(source).map(str::to_owned));
    }

    let viewbox = index
        .viewbox
        .clone()
        .unwrap_or_else(|| em_viewbox(font, advance));
    let mut out = String::with_capacity(doc.len().min(64 * 1024));
    out.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" viewBox=\"",
    );
    out.push_str(&viewbox);
    out.push_str("\"><defs>");
    for def in defs {
        out.push_str(def);
    }
    out.push_str("</defs>");
    out.push_str(&doc[target]);
    out.push_str("</svg>");
    Some(out.into_bytes())
}

/// The em-square viewport the OpenType SVG spec defines when a document has
/// no viewBox: x from 0 to the glyph advance, y from -ascent to -descent
/// (the SVG y axis points down).
fn em_viewbox(font: &EmojiFont, advance: i32) -> String {
    let width = if advance > 0 {
        advance as f32
    } else {
        font.upem
    };
    format!(
        "0 {} {} {}",
        -font.ascent,
        width,
        font.ascent - font.descent
    )
}

/// Adds a viewBox to an `<svg>` root that lacks one.
fn inject_viewbox(doc: &str, viewbox: &str) -> Option<String> {
    let root = doc.find("<svg")?;
    let end = doc[root..].find('>')? + root;
    let mut out = String::with_capacity(doc.len() + viewbox.len() + 12);
    out.push_str(&doc[..end]);
    out.push_str(" viewBox=\"");
    out.push_str(viewbox);
    out.push('"');
    out.push_str(&doc[end..]);
    Some(out)
}

/// Every `#id` reference in one element source (`url(#x)`, `href="#x"`,
/// `style="fill:url(#x)"`, ...). Ids not present in the document are
/// ignored by the caller, so over-collecting (e.g. hex colors) is safe.
fn referenced_ids(source: &str) -> impl Iterator<Item = &str> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(hash) = source[i..].find('#') {
        let start = i + hash + 1;
        let mut end = start;
        while end < bytes.len()
            && matches!(bytes[end], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b':')
        {
            end += 1;
        }
        if end > start {
            out.push(&source[start..end]);
        }
        i = end.max(start);
    }
    out.into_iter()
}

/// Gzip-decodes one document, bounded (the asset is trusted, but a corrupt
/// table must never expand without limit).
fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read as _;
    if !data.starts_with(&[0x1F, 0x8B]) {
        return Some(data.to_vec());
    }
    let mut out = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(data).take(64 * 1024 * 1024);
    decoder.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Composes a multi-glyph cluster into one image, left to right.
///
/// The composed box is capped at `MAX_TEXTURE_DIM` per side: glyph
/// advances come from the font and an absurd face could otherwise make
/// `width * side` arbitrarily large. A cluster past the cap renders
/// monochrome (the caller's fallback), never as a giant allocation.
fn compose_glyphs(font: &EmojiFont, glyphs: &[(u32, i32)], px: u32) -> Option<egui::ColorImage> {
    let scale = px as f32 / font.upem;
    let total: f32 = glyphs.iter().map(|(_, adv)| *adv as f32 * scale).sum();
    let width = total.ceil().max(1.0) as usize;
    let side = px as usize;
    let cap = crate::config::MAX_TEXTURE_DIM as usize;
    if width > cap || side > cap {
        return None;
    }
    let mut pixels = vec![egui::Color32::TRANSPARENT; width * side];
    let mut x = 0.0f32;
    for (gid, advance) in glyphs {
        if let Some(piece) = rasterize_glyph(font, *gid, *advance, px) {
            let ox = x.round() as usize;
            for y in 0..side.min(piece.size[1]) {
                for sx in 0..piece.size[0] {
                    let dx = ox + sx;
                    if dx < width {
                        pixels[y * width + dx] = piece.pixels[y * piece.size[0] + sx];
                    }
                }
            }
        }
        x += *advance as f32 * scale;
    }
    Some(egui::ColorImage::new([width, side], pixels))
}

#[cfg(test)]
mod tests;
