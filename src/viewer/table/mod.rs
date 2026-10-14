// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::table`: Markdown tables in a bordered card.
//!
//! - Collapsed interior grid: 1px row and column separators painted
//!   BETWEEN cells (`table th, td { border: 1px solid }`, collapsed), while
//!   the card frame stays the outer border.
//! - Cells are adjacent (no Grid spacing) and carry a tight 5px/11px
//!   padding INSIDE (slightly tighter than the common 6px/13px): the grid
//!   reaches the card border edge to edge and the text never touches a
//!   line.
//! - Subtle surface, 1px `#d1d9e0` border, 6px corners, `#f6f8fa` stripes.
//! - Respects per-column alignment. Columns are measured from content
//!   (wrapped at the per-column cap), then grown proportionally to fill the
//!   card or shrunk with a `min_col` floor so wide cells wrap instead of
//!   forcing a horizontal scrollbar (see `render::distribute_columns`). When even
//!   the floor does not fit and the table scrolls, every column keeps its
//!   measured width up to a wider scroll cap instead of collapsing to the
//!   floor (which broke words like "Header" mid-word). The
//!   measurement is cached across frames (content hash + caps + selection);
//!   tables holding images or emoji bypass the cache because their widths
//!   depend on resolved textures.
//! - The reading column is fluid; wide tables scroll horizontally and ragged
//!   rows are padded.
//! - Grid ids are bounded (`md-table-{idx}-{salt}`) so egui memory stays
//!   bounded.

use super::inline::{render_frag, visual_lines, Frag};
use super::LinkHit;
use crate::media::cache::ImageCache;
use crate::ui::theme;
use eframe::egui;
use pulldown_cmark::Alignment;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Cell padding (px), slightly tighter than the common 6px/13px inset:
/// the text is inset from the collapsed cell border, while the
/// grid itself reaches the card's outer border (the card frame IS the outer
/// cell border).
const CELL_PAD_X: f32 = 11.0;
const CELL_PAD_Y: f32 = 5.0;

/// Table buffer while events arrive.
pub(crate) struct TableCtx<'a> {
    pub(crate) aligns: Vec<Alignment>,
    pub(crate) headers: Option<Vec<Vec<Frag<'a>>>>,
    pub(crate) rows: Vec<Vec<Vec<Frag<'a>>>>,
    pub(crate) in_head: bool,
}

/// Measured widths of one table, in render order (see [`flush_table`]).
struct TableWidths {
    /// `rows[r][c]`: width of every visual line of that cell, in order
    /// (0.0 for empty lines). Rows that lack a cell simply lack the entry.
    rows: Vec<Vec<Vec<f32>>>,
    /// Final width of every column, clamped to `[min, max]`.
    cols: Vec<f32>,
}

thread_local! {
    /// Bounded cache of measured table widths. A text table's measurement
    /// is a pure function of its content, the column caps and the theme;
    /// re-measuring every table on every repaint (the invisible sizing pass
    /// runs the full `render_frag` flow per cell line) was the single
    /// most expensive per-frame layout on table-heavy documents.
    static WIDTHS: RefCell<HashMap<TableKey, Rc<CachedWidths>>> =
        RefCell::new(HashMap::new());
    /// Approximate bytes currently held by `WIDTHS`.
    static WIDTHS_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Monotonic use clock for the LRU eviction order.
    static WIDTHS_TICK: Cell<u64> = const { Cell::new(0) };
    /// Content fingerprints per `(doc_salt, table event index)`: the full
    /// fingerprint is computed once per document and reused, so a settled
    /// large table costs no byte scan per frame.
    static WIDTH_HASH: RefCell<HashMap<(u64, usize), u64>> = RefCell::new(HashMap::new());
}

/// Cache key: document salt + table index + measurement inputs (column caps
/// and flags) + a content hash + the texture-resolution epochs. The hash
/// makes a salt collision harmless: changed text misses and is re-measured.
/// Tables holding images or emoji ARE cached: their widths depend on
/// resolved textures, and the epochs change on every width-affecting
/// transition (image upload/failure, emoji raster/evict), so a stale entry
/// can only ever miss, never pin an old width.
type TableKey = (u64, usize, u32, u32, bool, bool, u64, u64, u64);

/// One cached measurement.
struct CachedWidths {
    widths: TableWidths,
    /// Approximate retained bytes (accounting for the cache budget).
    bytes: usize,
    /// Last-use stamp ([`WIDTHS_TICK`]); the smallest is evicted first.
    used: Cell<u64>,
}

/// Max cached tables (tied to the per-document render budget, like the
/// code-block cache, so a sequential scan never evicts what it will ask for
/// again next frame). The byte budget is the real memory bound.
const WIDTHS_CACHE_MAX: usize = crate::config::MAX_RENDERED_BLOCKS;

/// Max approximate bytes retained across all cached tables.
const WIDTHS_CACHE_BYTES_MAX: usize = 16 * 1024 * 1024;

/// Bumps and returns the LRU use clock.
fn widths_tick() -> u64 {
    WIDTHS_TICK.with(|c| {
        let value = c.get().wrapping_add(1);
        c.set(value);
        value
    })
}

/// Approximate heap bytes of one measurement.
fn widths_bytes(w: &TableWidths) -> usize {
    let rows: usize = w
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|line| {
                    line.len() * std::mem::size_of::<f32>() + std::mem::size_of::<Vec<f32>>()
                })
                .sum::<usize>()
                + std::mem::size_of::<Vec<Vec<f32>>>()
        })
        .sum();
    rows + w.cols.len() * std::mem::size_of::<f32>() + std::mem::size_of::<TableWidths>()
}

/// Content fingerprint of one table in ONE walk over its fragments.
///
/// The fingerprint is over text bytes and style; link destinations are
/// hashed too (a style swapped between two links is a different layout in
/// principle, and hashing them is cheap next to a layout pass).
/// Texture state is NOT fingerprinted: image/emoji resolution is tracked by
/// the caches' geometry epochs, which join the cache key in
/// [`table_widths`].
fn table_fingerprint(tb: &TableCtx<'_>) -> u64 {
    fn hash_cells(hash: &mut u64, cells: &[Vec<Frag<'_>>]) {
        for cell in cells {
            for frag in cell {
                for b in frag.text.as_bytes() {
                    *hash ^= u64::from(*b);
                    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
                let flags = u64::from(frag.strong)
                    | u64::from(frag.em) << 1
                    | u64::from(frag.strike) << 2
                    | u64::from(frag.code) << 3;
                *hash = hash.wrapping_mul(0x0000_0100_0000_01b3) ^ flags;
                if let Some(link) = &frag.link {
                    for b in link.as_bytes() {
                        *hash ^= u64::from(*b);
                        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
                if frag.image.is_some() {
                    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3) ^ 0x9e37;
                }
                // The resolved image size feeds the measured width, so the
                // URL joins the fingerprint (alt text is already hashed
                // above as `frag.text`).
                if let Some(img) = frag.image.as_ref() {
                    for b in img.url.as_bytes() {
                        *hash ^= u64::from(*b);
                        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
            }
        }
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    if let Some(headers) = tb.headers.as_ref() {
        hash_cells(&mut hash, headers);
    }
    for row in &tb.rows {
        hash_cells(&mut hash, row);
    }
    hash
}

/// The memoized [`table_fingerprint`] for one `(doc_salt, table event
/// index)`.
///
/// The events of one document are immutable for the lifetime of its
/// `doc_salt` (which mixes the tab id and the per-tab document generation:
/// a replaced document or a reload re-keys), and the walk passes the
/// table's `Start` EVENT index — stable whatever the frame renders — so the
/// fingerprint can be trusted without re-hashing the text every frame.
fn cached_fingerprint(doc_salt: u64, idx: usize) -> Option<u64> {
    WIDTH_HASH.with(|cell| cell.borrow().get(&(doc_salt, idx)).copied())
}

/// Stores a freshly computed fingerprint for `(doc_salt, idx)`, bounded like
/// the width cache: a document with more tables than the cache can hold
/// cannot grow the memo without bound.
fn memoize_fingerprint(doc_salt: u64, idx: usize, hash: u64) {
    WIDTH_HASH.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() >= WIDTHS_CACHE_MAX {
            map.clear();
        }
        map.insert((doc_salt, idx), hash);
    });
}

/// One table's measured widths from the cache, or a fresh measurement.
///
/// Media tables (images, color emoji) are cached like text ones: the key
/// carries both geometry epochs, so every resolution/evict/registration
/// transition misses exactly once and re-measures, while settled frames
/// hit. The entry is stored under the POST-measure epochs: measuring
/// itself can register new images (`ensure`) or rasterize new emoji, and
/// storing under the pre-measure epochs would pin widths measured against
/// placeholders the new state no longer shows.
#[allow(clippy::too_many_arguments)]
fn table_widths(
    ui: &mut egui::Ui,
    tb: &TableCtx<'_>,
    ncols: usize,
    min_col: f32,
    max_col: f32,
    selected: bool,
    interactive: bool,
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    doc_salt: u64,
    idx: usize,
) -> Rc<CachedWidths> {
    // The fingerprint is memoized per `(doc_salt, event index)` (see
    // `cached_fingerprint`): re-hashing up to `MAX_TABLE_ROWS` rows of text
    // on every frame kept a settled large table at O(text) per frame even
    // though its measured widths were already cached.
    let hash = match cached_fingerprint(doc_salt, idx) {
        Some(hash) => hash,
        None => {
            let hash = table_fingerprint(tb);
            memoize_fingerprint(doc_salt, idx, hash);
            hash
        }
    };
    let key = (
        doc_salt,
        idx,
        min_col.to_bits(),
        max_col.to_bits(),
        selected,
        interactive,
        hash,
        images.resolved_epoch(),
        emoji.resolved_epoch(),
    );
    let hit = WIDTHS.with(|cell| {
        let map = cell.borrow();
        map.get(&key).map(|entry| {
            entry.used.set(widths_tick());
            entry.clone()
        })
    });
    if let Some(hit) = hit {
        return hit;
    }
    let measured = measure_table(
        ui,
        tb,
        ncols,
        min_col,
        max_col,
        selected,
        interactive,
        images,
        emoji,
    );
    // Re-key after the pass: the measurement above may have registered new
    // images or rasterized new emoji, which bumped the epochs mid-pass.
    let key = (
        doc_salt,
        idx,
        min_col.to_bits(),
        max_col.to_bits(),
        selected,
        interactive,
        hash,
        images.resolved_epoch(),
        emoji.resolved_epoch(),
    );
    let bytes = widths_bytes(&measured);
    WIDTHS.with(|cell| {
        let mut map = cell.borrow_mut();
        // Evict least-recently-used entries in ONE pass (sorted by age), not
        // one `min_by_key` rescan per victim: a document with more tables
        // than the cache can hold would otherwise re-measure every table on
        // every frame, at O(cap²) inside a single frame. The pass is shared
        // with the code-block cache (`viewer::cache::evict_oldest`).
        let slots_needed = map.len().saturating_add(1).saturating_sub(WIDTHS_CACHE_MAX);
        let bytes_deficit = WIDTHS_BYTES
            .with(Cell::get)
            .saturating_add(bytes)
            .saturating_sub(WIDTHS_CACHE_BYTES_MAX);
        let aged: Vec<(u64, TableKey)> = map
            .iter()
            .map(|(key, entry)| (entry.used.get(), *key))
            .collect();
        super::cache::evict_oldest(aged, slots_needed, bytes_deficit, |key| {
            map.remove(&key).map(|old| {
                WIDTHS_BYTES.with(|total| total.set(total.get().saturating_sub(old.bytes)));
                old.bytes
            })
        });
        WIDTHS_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
        let entry = Rc::new(CachedWidths {
            widths: measured,
            bytes,
            used: Cell::new(widths_tick()),
        });
        map.insert(key, entry.clone());
        entry
    })
}

mod render;

pub(crate) use render::flush_table;
use render::measure_table;

#[cfg(test)]
mod tests;
