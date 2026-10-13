// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{render_cluster, split, Segment};
use eframe::egui;

/// True when `text` contains at least one cluster that renders as a
/// color emoji (the production viewer decides this from `split`).
fn contains_emoji(text: &str) -> bool {
    split(text)
        .iter()
        .any(|segment| matches!(segment, Segment::Emoji(_)))
}

/// A pictograph renders as a real color image (more than two colors).
#[test]
fn renders_a_color_pictograph() {
    let image = render_cluster("🚀", 64).expect("rocket renders");
    assert!(image.size[0] > 0 && image.size[1] > 0);
    let colors: std::collections::HashSet<[u8; 4]> =
        image.pixels.iter().map(|p| p.to_array()).collect();
    assert!(
        colors.len() > 4,
        "rocket must be colored: {} colors",
        colors.len()
    );
}

/// A glyph from the giant shared document (the common case) extracts
/// only its element and the referenced defs.
#[test]
fn renders_a_shared_document_glyph() {
    // U+1F600 grinning face: part of the shared 14 MB document.
    let image = render_cluster("😀", 64).expect("grinning face renders");
    assert!(image.size[0] > 0 && image.size[1] > 0);
}

/// ZWJ sequences, flags and keycaps resolve through the shaper.
#[test]
fn renders_sequences_as_one_glyph() {
    for cluster in ["👨‍👩‍👧", "🇪🇸", "🏳️‍🌈", "👍🏽", "3️⃣"] {
        let image = render_cluster(cluster, 48).expect("sequence renders");
        assert!(image.size[0] > 0 && image.size[1] > 0, "{cluster}");
    }
}

/// Unknown clusters fall back instead of panicking.
#[test]
fn unknown_cluster_is_none() {
    assert!(render_cluster("A", 32).is_none());
    assert!(render_cluster("", 32).is_none());
}

#[test]
fn splits_text_and_emoji_clusters() {
    let segments = split("Hola 🚀 fin 👨‍👩‍👧!");
    let rendered: Vec<String> = segments
        .iter()
        .map(|s| match s {
            Segment::Text(t) => format!("T:{t}"),
            Segment::Emoji(e) => format!("E:{e}"),
        })
        .collect();
    assert_eq!(rendered, vec!["T:Hola ", "E:🚀", "T: fin ", "E:👨‍👩‍👧", "T:!"]);
}

/// Emoji-free non-ASCII text takes the scalar fast path: the segments
/// stay one plain-text piece (no grapheme segmentation), in scripts
/// where that used to run for every fragment of every repaint.
#[test]
fn non_ascii_without_candidates_is_plain_text() {
    for text in [
        "日本語のテキスト",
        "Привет, мир",
        "مرحبا بالعالم",
        "café con leche",
    ] {
        let segments = split(text);
        assert!(
            matches!(segments.as_slice(), [Segment::Text(t)] if *t == text),
            "{text} must stay one text segment"
        );
    }
    // A lone candidate scalar that is not a known emoji is still text.
    assert!(!contains_emoji("a\u{1F000}b"));
}

/// Text-presentation symbols stay text unless the emoji selector is
/// present; pictographs are color by default.
#[test]
fn presentation_heuristic() {
    assert!(!contains_emoji("© 2026"));
    assert!(contains_emoji("©️"));
    assert!(contains_emoji("❤️"));
    assert!(!contains_emoji("❤")); // text presentation without VS16
    assert!(contains_emoji("🚀"));
    assert!(contains_emoji("🇪🇸"));
    assert!(!contains_emoji("plain text"));
}

/// Evicting a texture must release its process-wide reservation
/// immediately (`config.rs` documents release on eviction): before, the
/// bytes leaked and could permanently exhaust the global texture pool.
#[test]
fn eviction_releases_the_global_texture_budget() {
    use std::sync::atomic::Ordering;
    // Both this test and `media::tests` touch the process-wide counter.
    let _guard = crate::media::TEXTURE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = crate::media::TEXTURE_BYTES.load(Ordering::SeqCst);
    let ctx = egui::Context::default();
    let mut cache = super::EmojiCache::new();
    // One more entry than the count bound: the first must be evicted.
    for i in 0..=crate::config::EMOJI_CACHE_ENTRIES {
        let reserved = 4; // one 1x1 RGBA pixel
        let texture = ctx.load_texture(
            format!("test-{i}"),
            egui::ColorImage::new([1, 1], vec![egui::Color32::RED]),
            egui::TextureOptions::LINEAR,
        );
        assert!(crate::media::try_reserve_texture_bytes(reserved));
        cache.insert(
            16,
            &format!("cluster-{i}"),
            super::Cached::Texture(texture, reserved, i as u64 + 1),
        );
    }
    assert_eq!(
        crate::media::TEXTURE_BYTES.load(Ordering::SeqCst),
        before + crate::config::EMOJI_CACHE_ENTRIES * 4,
        "the evicted texture's reservation must be released at eviction"
    );
    drop(cache);
    assert_eq!(
        crate::media::TEXTURE_BYTES.load(Ordering::SeqCst),
        before,
        "dropping the cache releases every remaining reservation"
    );
}
