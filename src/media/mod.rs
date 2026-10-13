// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media`: bounded, pure-Rust image and favicon pipeline.
//!
//! - `decode`: magic-byte sniffing + decoders (PNG/JPEG/GIF/WebP/ICO via
//!   `image`, JPEG XL via `jxl-oxide`, AVIF via `avif-decode`/`rav1d`,
//!   static SVG via `resvg`/`usvg` with scripts and external refs off).
//! - `fetch`: background download (HTTPS) / local read (file) with the same
//!   anti-abuse caps
//!   as documents (capped streaming, stall timeout, cooperative cancel).
//! - `cache`: one document's decoded images, keyed by absolute URL. Entries
//!   live as long as the document; navigation drops them (the no-cache rule
//!   applies to documents and their subresources alike).
//! - `emoji`: color emoji from the embedded Noto Color Emoji: grapheme
//!   clusters are shaped with harfrust, their SVG-in-OpenType documents
//!   rasterized through `decode` and cached as bounded inline textures.
//! - `favicon`: session-wide per-origin icon cache for the tab strip.
//!
//! IMAGES ARE SUBRESOURCES, NEVER DOCUMENTS:
//! - Page acceptance still requires a markdown Content-Type (never HTML).
//! - Image responses are only read when their `Content-Type` is an accepted
//!   image type (or generic `application/octet-stream`/missing), and the
//!   bytes must then decode as one of the supported formats.
//! - All work happens on background threads with a process-wide slot limit,
//!   so a gallery of images never blocks document navigation.
//! - Textures are budgeted three ways: per document
//!   (`MAX_TEXTURE_BYTES`), process-wide (`MAX_TEXTURE_BYTES_TOTAL`, shared
//!   with the favicon cache), and favicons additionally decode at
//!   `MAX_FAVICON_DIM` (they draw at the toolbar icon size, ~27 px), so many
//!   loaded tabs cannot add up to unbounded VRAM.

pub mod cache;
pub mod decode;
pub mod emoji;
pub mod favicon;
pub mod fetch;

use std::sync::atomic::{AtomicUsize, Ordering};

/// Process-wide concurrent image/favicon jobs (download + decode).
static IMAGE_SLOTS: AtomicUsize = AtomicUsize::new(0);

/// RAII reservation of one image job slot. Dropping frees the slot; the
/// background closure owns it so a panicking thread still releases it.
pub(crate) struct ImageSlot;

impl Drop for ImageSlot {
    fn drop(&mut self) {
        IMAGE_SLOTS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Tries to reserve one image slot without ever waiting.
/// Returns `None` when the limit is reached; callers retry on a later frame.
pub(crate) fn try_acquire_image_slot() -> Option<ImageSlot> {
    let mut cur = IMAGE_SLOTS.load(Ordering::SeqCst);
    loop {
        if cur >= crate::config::MAX_CONCURRENT_IMAGES {
            return None;
        }
        match IMAGE_SLOTS.compare_exchange_weak(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(ImageSlot),
            Err(actual) => cur = actual,
        }
    }
}

/// FNV-1a hash used to name textures and cache keys (deterministic, tiny).
pub(crate) fn fnv1a(value: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for b in value.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Process-wide bytes currently reserved for uploaded image textures.
/// Bounds total VRAM regardless of how many tabs keep documents alive.
static TEXTURE_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Serializes the tests that touch [`TEXTURE_BYTES`] (they run in parallel
/// with each other by default; `media::tests` and `media::emoji::tests`).
#[cfg(test)]
pub(crate) static TEXTURE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tries to reserve `bytes` of the process-wide texture budget.
/// Returns `false` when the reservation would exceed
/// `config::MAX_TEXTURE_BYTES_TOTAL` (the caller shows a placeholder).
pub(crate) fn try_reserve_texture_bytes(bytes: usize) -> bool {
    let mut cur = TEXTURE_BYTES.load(Ordering::SeqCst);
    loop {
        let next = match cur.checked_add(bytes) {
            Some(n) if n <= crate::config::MAX_TEXTURE_BYTES_TOTAL => n,
            _ => return false,
        };
        match TEXTURE_BYTES.compare_exchange_weak(cur, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(actual) => cur = actual,
        }
    }
}

/// Releases a reservation made by `try_reserve_texture_bytes`.
/// Saturates at zero so a double release can never wrap the counter.
pub(crate) fn release_texture_bytes(bytes: usize) {
    let _ = TEXTURE_BYTES.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
        Some(cur.saturating_sub(bytes))
    });
}

#[cfg(test)]
mod tests {
    use super::{release_texture_bytes, try_reserve_texture_bytes};
    use crate::config;

    // NOTE: this test shares the process-wide counter with the emoji-cache
    // eviction test; `TEXTURE_TEST_LOCK` serializes them so the global is
    // stable while each runs.
    #[test]
    fn texture_budget_is_global_and_saturating() {
        let _guard = super::TEXTURE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Reserve exactly the budget: allowed once.
        assert!(try_reserve_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL));
        // Any further byte must be rejected (per-document caps are checked
        // separately by `ImageCache`; this is the process-wide bound).
        assert!(!try_reserve_texture_bytes(1));
        assert!(!try_reserve_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL));
        // Partial release frees exactly that much.
        release_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL);
        assert!(try_reserve_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL));
        release_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL);
        // Over-release saturates at zero instead of wrapping.
        release_texture_bytes(usize::MAX);
        assert!(try_reserve_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL));
        release_texture_bytes(config::MAX_TEXTURE_BYTES_TOTAL);
    }
}
