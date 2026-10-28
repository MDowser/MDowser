// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::tests::*;
use super::{doc_id_salt, show_markdown};
use eframe::egui;

/// An image inside a heading renders as an image (an inert placeholder
/// here, `interactive = false`), never as leaked alt text: the old code
/// fell through to the plain-text branch and the image was never
/// registered for fetching.
#[test]
fn heading_image_renders_as_image_not_alt_text() {
    let (texts, _) = render_probe("# ![Rust Logo](https://ex.com/logo.png) Title\n");
    let joined: String = texts
        .iter()
        .map(|(t, _, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(joined.contains("Title"), "heading text kept: {joined}");
    assert!(
        !joined.contains("Rust Logo"),
        "alt text must not leak as text in a heading: {texts:?}"
    );
}

/// Bounding rect of every shape painted with `tex_id` (textured rects
/// and meshes), nested shape groups included.
fn textured_bounds(shape: &egui::epaint::Shape, tex_id: egui::TextureId) -> Option<egui::Rect> {
    match shape {
        egui::epaint::Shape::Rect(r)
            if r.brush
                .as_ref()
                .is_some_and(|b| b.fill_texture_id == tex_id) =>
        {
            Some(r.rect)
        }
        egui::epaint::Shape::Mesh(m) if m.texture_id == tex_id => {
            let mut min = egui::pos2(f32::MAX, f32::MAX);
            let mut max = egui::pos2(f32::MIN, f32::MIN);
            for v in &m.vertices {
                min = min.min(v.pos);
                max = max.max(v.pos);
            }
            Some(egui::Rect::from_min_max(min, max))
        }
        egui::epaint::Shape::Vec(v) => v
            .iter()
            .filter_map(|s| textured_bounds(s, tex_id))
            .reduce(egui::Rect::union),
        _ => None,
    }
}

/// Renders `md` with one pre-uploaded `image_size` texture registered
/// for `url` and returns the size actually painted for it (no network).
fn rendered_image_size(md: &str, url: &str, image_size: [usize; 2]) -> egui::Vec2 {
    rendered_image_bounds(md, url, image_size, false).size()
}

/// Like [`rendered_image_size`] but returns the painted rect, so
/// position (centered block vs inline flow) can be asserted.
fn rendered_image_rect(md: &str, url: &str, image_size: [usize; 2]) -> egui::Rect {
    rendered_image_bounds(md, url, image_size, false)
}

/// Like [`rendered_image_rect`] with a vector (SVG) source flag.
fn rendered_vector_rect(md: &str, url: &str, image_size: [usize; 2]) -> egui::Rect {
    rendered_image_bounds(md, url, image_size, true)
}

fn rendered_image_bounds(md: &str, url: &str, image_size: [usize; 2], vector: bool) -> egui::Rect {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let tex = ctx.load_texture(
        "probe-image",
        egui::ColorImage::filled(image_size, egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    );
    let tex_id = tex.id();
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    if vector {
        images.insert_ready_vector(url, tex);
    } else {
        images.insert_ready(url, tex);
    }
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut rect = egui::Rect::NOTHING;
    for clipped in &out.shapes {
        if let Some(r) = textured_bounds(&clipped.shape, tex_id) {
            rect = if rect.is_negative() { r } else { rect.union(r) };
        }
    }
    out.textures_delta.clear();
    assert!(!rect.is_negative(), "image must be painted: {md:?}");
    rect
}

/// SIZE POLICY: an inline image (sharing a line with text) paints at its
/// natural pixel size, not the old 24px text-height box.
#[test]
fn inline_image_renders_at_natural_size() {
    let url = "https://ex.com/pic.png";
    let size = rendered_image_size(&format!("before ![pic]({url}) after\n"), url, [100, 40]);
    assert!(
        (size.x - 100.0).abs() < 0.5 && (size.y - 40.0).abs() < 0.5,
        "natural size expected 100x40: {size:?}"
    );
}

/// POSITION POLICY: an inline image too tall for a text line is promoted
/// to a centered block (it used to dominate the row, vertically centering
/// the neighboring words next to a huge picture).
#[test]
fn large_inline_image_is_promoted_to_a_centered_block() {
    let url = "https://ex.com/big.png";
    let rect = rendered_image_rect(&format!("before ![big]({url}) after\n"), url, [300, 220]);
    assert!(
        (rect.width() - 300.0).abs() < 0.5 && (rect.height() - 220.0).abs() < 0.5,
        "natural size kept: {rect:?}"
    );
    assert!(
        (rect.center().x - 550.0).abs() < 1.5,
        "promoted image must be centered in the panel: {rect:?}"
    );
}

/// POSITION POLICY: a run of consecutive large images renders as ONE
/// centered gallery row in source order, not one centered block per
/// image (which stacked a photo strip vertically and let entries move
/// while their textures resolved).
#[test]
fn consecutive_large_images_render_as_a_centered_gallery_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let tex_a = ctx.load_texture(
        "gallery-a",
        egui::ColorImage::filled([200, 100], egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    );
    let tex_b = ctx.load_texture(
        "gallery-b",
        egui::ColorImage::filled([160, 80], egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    );
    let (id_a, id_b) = (tex_a.id(), tex_b.id());
    let (a_url, b_url) = ("https://ex.com/a.png", "https://ex.com/b.png");
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let events = crate::markdown::document::parse_events(&format!("![a]({a_url})![b]({b_url})\n"))
        .expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    images.insert_ready(a_url, tex_a);
    images.insert_ready(b_url, tex_b);
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut rect_a = egui::Rect::NOTHING;
    let mut rect_b = egui::Rect::NOTHING;
    for clipped in &out.shapes {
        if let Some(r) = textured_bounds(&clipped.shape, id_a) {
            rect_a = if rect_a.is_negative() {
                r
            } else {
                rect_a.union(r)
            };
        }
        if let Some(r) = textured_bounds(&clipped.shape, id_b) {
            rect_b = if rect_b.is_negative() {
                r
            } else {
                rect_b.union(r)
            };
        }
    }
    out.textures_delta.clear();
    assert!(
        !rect_a.is_negative() && !rect_b.is_negative(),
        "both painted"
    );
    assert!(
        (rect_a.center().y - rect_b.center().y).abs() < 2.0,
        "both images share one gallery row: a={rect_a:?} b={rect_b:?}"
    );
    assert!(
        rect_a.max.x <= rect_b.min.x + 1.0,
        "gallery keeps source order left to right: a={rect_a:?} b={rect_b:?}"
    );
    let union = rect_a.union(rect_b);
    assert!(
        (union.center().x - 550.0).abs() < 5.0,
        "gallery row is centered in the panel: {union:?}"
    );
}

/// POSITION POLICY: a small image sharing a line stays inline (not every
/// image becomes a block).
#[test]
fn small_inline_image_stays_in_the_text_flow() {
    let url = "https://ex.com/small.png";
    let rect = rendered_image_rect(&format!("before ![s]({url}) after\n"), url, [24, 24]);
    assert!(
        rect.center().x < 300.0,
        "small inline image stays at the line start: {rect:?}"
    );
}

/// Inline images get horizontal breathing room so neighboring glyphs
/// never touch the bitmap.
#[test]
fn inline_image_has_horizontal_breathing_room() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let url = "https://ex.com/pad.png";
    let tex = ctx.load_texture(
        "pad-image",
        egui::ColorImage::filled([20, 20], egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    );
    let tex_id = tex.id();
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let events =
        crate::markdown::document::parse_events(&format!("x ![i]({url}) y\n")).expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    images.insert_ready(url, tex);
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut img = egui::Rect::NOTHING;
    let mut x_end = 0.0_f32;
    let mut y_x = None;
    for clipped in &out.shapes {
        if let Some(r) = textured_bounds(&clipped.shape, tex_id) {
            img = r;
        }
        if let egui::epaint::Shape::Text(t) = &clipped.shape {
            // Glyphs can be offset inside the galley (the row origin is
            // captured before the image): add the first glyph offset.
            let gx = t
                .galley
                .rows
                .first()
                .and_then(|r| r.glyphs.first())
                .map_or(0.0, |g| g.pos.x);
            match t.galley.text().trim() {
                "x" => x_end = t.pos.x + gx + t.galley.rect.width(),
                "y" => y_x = Some(t.pos.x + gx),
                _ => {}
            }
        }
    }
    out.textures_delta.clear();
    let y_x = y_x.expect("trailing text rendered");
    assert!(
        img.min.x - x_end >= 2.0,
        "left breathing room: image {img:?} after text ending at {x_end}"
    );
    assert!(
        y_x - img.max.x >= 2.0,
        "right breathing room: text at {y_x} after image {img:?}"
    );
}

/// Inline SVG (vector) sources are clamped to an icon-sized box; block
/// SVG keeps its natural size.
#[test]
fn inline_vector_is_icon_sized_and_block_vector_keeps_natural_size() {
    let inline = rendered_vector_rect(
        "before ![v](https://ex.com/v.svg) after\n",
        "https://ex.com/v.svg",
        [512, 512],
    );
    assert!(
        (inline.height() - 32.0).abs() < 0.5,
        "inline vector clamped to 32px tall: {inline:?}"
    );
    let block = rendered_vector_rect(
        "![v](https://ex.com/v.svg)\n",
        "https://ex.com/v.svg",
        [300, 150],
    );
    assert!(
        (block.width() - 300.0).abs() < 0.5 && (block.height() - 150.0).abs() < 0.5,
        "block vector keeps its natural size: {block:?}"
    );
}

/// ICON STRIPS: consecutive inline SVGs share the tallest height in the
/// strip (aspect preserved), like the source site's CSS. Without it, an
/// asset declaring `1em` rendered at 16px next to 256px logos clamped
/// to 32px.
#[test]
fn consecutive_inline_vectors_share_one_strip_height() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    // Three intrinsic sizes: 256x257, `1em` -> 16x16, viewBox-only (467:533).
    let sizes = [[256usize, 257], [16, 16], [467, 533]];
    let urls = [
        "https://ex.com/wide.svg",
        "https://ex.com/em.svg",
        "https://ex.com/tall.svg",
    ];
    let mut ids = Vec::new();
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    for (n, size) in sizes.iter().enumerate() {
        let tex = ctx.load_texture(
            format!("strip-{n}"),
            egui::ColorImage::filled(*size, egui::Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        ids.push(tex.id());
        images.insert_ready_vector(urls[n], tex);
    }
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let md = format!(
        "Get started ![]({})![]({})![]({}) Copy\n",
        urls[0], urls[1], urls[2]
    );
    let events = crate::markdown::document::parse_events(&md).expect("parse");
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                // Tests use no anchors and no pending scroll target.
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut rects = vec![egui::Rect::NOTHING; ids.len()];
    for clipped in &out.shapes {
        for (n, id) in ids.iter().enumerate() {
            if let Some(r) = textured_bounds(&clipped.shape, *id) {
                rects[n] = if rects[n].is_negative() {
                    r
                } else {
                    rects[n].union(r)
                };
            }
        }
    }
    out.textures_delta.clear();
    for (n, r) in rects.iter().enumerate() {
        assert!(!r.is_negative(), "icon {n} painted");
        assert!(
            (r.height() - 32.0).abs() < 0.6,
            "strip icon {n} must share the 32px height: {r:?}"
        );
    }
    // Aspect ratios are preserved: the `1em` icon grows to 32 wide, the
    // viewBox-only one keeps its 467:533 ratio.
    assert!((rects[0].width() - 31.9).abs() < 0.6, "{:?}", rects[0]);
    assert!((rects[1].width() - 32.0).abs() < 0.6, "{:?}", rects[1]);
    assert!((rects[2].width() - 28.0).abs() < 0.6, "{:?}", rects[2]);
    assert!(
        (rects[0].center().y - rects[1].center().y).abs() < 1.0
            && (rects[1].center().y - rects[2].center().y).abs() < 1.0,
        "strip icons share one row: {rects:?}"
    );
}

/// Renders `md` with each `(url, size)` registered as a decoded SVG and
/// returns the painted rect per URL (`NOTHING` when it was not painted).
fn render_vector_set(md: &str, texs: &[(&str, [usize; 2])]) -> Vec<egui::Rect> {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 750.0),
        )),
        ..Default::default()
    };
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut ids = Vec::new();
    for (n, (url, size)) in texs.iter().enumerate() {
        let tex = ctx.load_texture(
            format!("probe-{n}"),
            egui::ColorImage::filled(*size, egui::Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        ids.push(tex.id());
        images.insert_ready_vector(url, tex);
    }
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            show_markdown(
                ui,
                &events,
                &crate::markdown::anchors::AnchorIndex::default(),
                &crate::protocol::rewrite::DestCache::build(
                    &base,
                    &crate::markdown::anchors::AnchorIndex::default(),
                    &events,
                ),
                &mut None,
                doc_id_salt(&base),
                false,
                false,
                1100.0,
                1.0,
                &mut images,
                &mut emoji,
                &mut on_link,
                &mut links,
            );
        });
    });
    let mut rects = vec![egui::Rect::NOTHING; texs.len()];
    for clipped in &out.shapes {
        for (n, id) in ids.iter().enumerate() {
            if let Some(r) = textured_bounds(&clipped.shape, *id) {
                rects[n] = if rects[n].is_negative() {
                    r
                } else {
                    rects[n].union(r)
                };
            }
        }
    }
    out.textures_delta.clear();
    rects
}

/// ICON BLOCK: the shared height now spans the whole paragraph, not one
/// line, so a card list's ten logos match even though each card is its
/// own row.
#[test]
fn non_consecutive_inline_vectors_share_a_block_height() {
    let big = "https://ex.com/big.svg";
    let small = "https://ex.com/small.svg";
    let md = format!("before ![]({big}) middle ![]({small}) after\n");
    let rects = render_vector_set(&md, &[(big, [512, 512]), (small, [16, 16])]);
    for (n, r) in rects.iter().enumerate() {
        assert!(!r.is_negative(), "icon {n} painted: {rects:?}");
        assert!(
            (r.height() - 32.0).abs() < 0.6,
            "icon {n} must share the block height, not its own: {r:?}"
        );
    }
}

/// THEME TWINS: the light/dark variant of the same logo collapses to ONE
/// icon (the one matching the theme), so a card shows one logo, not two.
#[test]
fn theme_variant_logo_pair_collapses_to_one() {
    let light = "https://ex.com/logo/light.svg";
    let dark = "https://ex.com/logo/dark.svg";
    let md = format!("[![]({light})![]({dark}) Logo](https://ex.com/x)\n");
    let rects = render_vector_set(&md, &[(light, [64, 64]), (dark, [64, 64])]);
    let painted = rects.iter().filter(|r| !r.is_negative()).count();
    assert_eq!(painted, 1, "exactly one variant painted: {rects:?}");
}

/// Agent-card lists: each card ships a light/dark logo pair of wildly
/// different intrinsic sizes (a 256px logo, a `1em` icon). One logo per
/// card and one common height for the whole list.
#[test]
fn card_logo_pairs_render_uniform() {
    let cl = "https://ex.com/icons/alpha/light.svg";
    let cd = "https://ex.com/icons/alpha/dark.svg";
    let xl = "https://ex.com/icons/beta/light.svg";
    let xd = "https://ex.com/icons/beta/dark.svg";
    let md = format!(
        "[![]({cl})![]({cd}) Alpha Foundation<h3>Agent One</h3>A description long enough to pass the forty character gate](https://ex.com/a) \
         [![]({xl})![]({xd}) Beta Cooperative<h3>Agent Two</h3>Another description long enough to pass the forty char gate](https://ex.com/b)\n"
    );
    let rects = render_vector_set(
        &md,
        &[
            (cl, [256, 257]),
            (cd, [256, 257]),
            (xl, [16, 16]),
            (xd, [16, 16]),
        ],
    );
    let painted: Vec<egui::Rect> = rects.iter().copied().filter(|r| !r.is_negative()).collect();
    assert_eq!(painted.len(), 2, "one logo per card: {rects:?}");
    for r in &painted {
        assert!(
            (r.height() - 32.0).abs() < 0.6,
            "all card logos share one height: {r:?}"
        );
    }
}

/// The theme-twins rule is narrow: only a single differing `light`/`dark`
/// token pairs two URLs.
#[test]
fn theme_twin_choice_pairs_light_and_dark_only() {
    let l = "https://ex.com/logo/light.svg";
    let d = "https://ex.com/logo/dark.svg";
    assert_eq!(super::theme_twin_choice(l, d, false), Some(true));
    assert_eq!(super::theme_twin_choice(l, d, true), Some(false));
    assert_eq!(super::theme_twin_choice(d, l, true), Some(true));
    assert_eq!(super::theme_twin_choice(d, l, false), Some(false));
    // Not a pair: other token, identical URLs, or a second difference.
    assert_eq!(
        super::theme_twin_choice(l, "https://ex.com/logo/blue.svg", true),
        None
    );
    assert_eq!(super::theme_twin_choice(l, l, true), None);
    assert_eq!(
        super::theme_twin_choice("https://a/light/x.svg", "https://b/dark/x.svg", true),
        None
    );
    assert_eq!(
        super::theme_twin_choice("https://a/lightning.svg", "https://a/dark.svg", true),
        None
    );
    // Raster variants (a real preview gallery) never collapse.
    assert_eq!(
        super::theme_twin_choice("https://ex.com/light.png", "https://ex.com/dark.png", true),
        None
    );
}

/// SIZE POLICY: an image wider than the reading column is scaled down to
/// the column width (aspect preserved), never left overflowing.
#[test]
fn oversized_image_is_clamped_to_the_column_width() {
    // 2000 px is within egui's test texture-side limit (2048) and much
    // wider than the ~1100 px test panel: the painted rect must be
    // clamped, not natural.
    let url = "https://ex.com/wide.png";
    let size = rendered_image_size(&format!("![wide]({url})\n"), url, [2000, 100]);
    assert!(
        size.x > 700.0 && size.x < 2000.0,
        "clamped to the column, not natural/overflowing: {size:?}"
    );
    assert!(
        (size.y / size.x - 100.0 / 2000.0).abs() < 0.01,
        "aspect ratio preserved: {size:?}"
    );
}

/// POSITION POLICY: a standalone image inside a blockquote renders as a
/// centered block (the placeholder card shows the alt text), not as a
/// tiny inline icon: the markdown position decides.
#[test]
fn standalone_image_inside_quote_renders_as_block() {
    let (texts, _) = render_probe("> ![shot](https://ex.com/shot.png)\n");
    let joined: String = texts
        .iter()
        .map(|(t, _, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        joined.contains("shot"),
        "block placeholder must show the alt text: {texts:?}"
    );
}

/// An oversized image alt is capped while collecting (one huge `Text`
/// event must not be copied whole per frame): the placeholder shows the
/// truncated label with the shared ellipsis, never the megabyte tail.
#[test]
fn oversized_image_alt_is_capped_at_collection() {
    let alt = "A".repeat(600_000);
    let md = format!("![{alt}](https://ex.com/pic.png)\n");
    let (texts, _) = render_probe(&md);
    let joined: String = texts
        .iter()
        .map(|(t, _, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("|");
    // The placeholder label is `<message>: <alt>`; with the cap the joined
    // runs hold at most ~2x `MAX_IMAGE_ALT_CHARS` A's (label + caption),
    // while the uncapped variant would carry all 600 000.
    let a_count = joined.chars().filter(|c| *c == 'A').count();
    assert!(
        a_count <= 2 * 512 + 16,
        "alt must be capped at MAX_IMAGE_ALT_CHARS, got {a_count} chars"
    );
    assert!(
        joined.contains("..."),
        "the shared ellipsis marks the cut: {}",
        &joined[..joined.len().min(80)]
    );
}
