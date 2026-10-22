// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::components::tabs_state_id;
use super::{doc_id_salt, menu_items, quote_indent_id, show_markdown, Frag};
use eframe::egui;
use std::sync::Arc;

pub(super) fn plain(text: &str) -> Frag<'static> {
    Frag {
        text: text.to_string().into(),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: false,
        link: None,
        image: None,
        badge: None,
    }
}

/// The incremental labels hash must stay identical to the old
/// `join("\x1f")` key: changing it would silently forget the tab a reader
/// selected before updating the app.
#[test]
fn tabs_state_id_matches_the_joined_labels_key() {
    let labels = vec!["First".to_string(), String::new(), "Third".to_string()];
    let joined = labels.join("\u{1f}");
    let old = egui::Id::unique((7u64, "mdx-tabs", 3usize, crate::media::fnv1a(&joined)));
    assert_eq!(tabs_state_id(7, 3, &labels), old);
}

/// A fragment that WAS a link but whose destination was dropped (`""`).
pub(super) fn inert_link(text: &str) -> Frag<'static> {
    Frag {
        text: text.to_string().into(),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: false,
        link: Some(Arc::from("")),
        image: None,
        badge: None,
    }
}

pub(super) fn linked(text: &str) -> Frag<'static> {
    Frag {
        text: text.to_string().into(),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: false,
        link: Some(Arc::from("https://ex.com/x")),
        image: None,
        badge: None,
    }
}

#[test]
fn detects_simple_menu() {
    let frags = vec![plain("Home | Docs | About")];
    let menu = menu_items(&frags).expect("menu");
    assert_eq!(menu.sep, '|');
    assert_eq!(menu.items.len(), 3);
}

#[test]
fn detects_dot_menus() {
    // Middle dot, bullet and math dots read as menus when they look like
    // navigation (uppercase start); the dot family uses the same guard
    // as pipes, so all-lowercase prose stays a paragraph.
    for (text, sep, n) in [
        ("Uno · Dos · Tres", '·', 3),
        ("Home • Docs • About", '•', 3),
        ("Home ∙ Docs", '∙', 2),
        ("Home ⋅ Docs", '⋅', 2),
    ] {
        let frags = vec![plain(text)];
        let menu = menu_items(&frags).unwrap_or_else(|| panic!("{text:?} is a menu"));
        assert_eq!(menu.sep, sep, "{text:?}");
        assert_eq!(menu.items.len(), n, "{text:?}");
    }
}

#[test]
fn rejects_lowercase_dot_prose() {
    // Navigation guard for dots too: no link, no capitalized label.
    let frags = vec![plain("inicio · contacto · ayuda")];
    assert!(menu_items(&frags).is_none());
    // A link makes it a menu again even when lowercase.
    let frags = vec![linked("inicio"), plain(" · "), linked("contacto")];
    assert!(menu_items(&frags).is_some());
}

#[test]
fn dot_inside_a_word_stays_a_word() {
    // Catalan `l·l` is not a menu separator.
    let frags = vec![plain("El col·legi de Barcelona")];
    assert!(menu_items(&frags).is_none());
    let frags = vec![plain("col·legi")];
    assert!(menu_items(&frags).is_none());
}

#[test]
fn rejects_lowercase_prose_pipes() {
    // "pan | vino | queso" is prose, not navigation: no links and no
    // capitalized start. Must stay a paragraph.
    let frags = vec![plain("pan | vino | queso")];
    assert!(menu_items(&frags).is_none());
}

#[test]
fn keeps_mixed_case_menus() {
    // One capitalized item is enough: `Home | docs` is a menu.
    let frags = vec![plain("Home | docs")];
    let menu = menu_items(&frags).expect("menu");
    assert_eq!(menu.items.len(), 2);
}

#[test]
fn detects_linked_menu() {
    let frags = vec![linked("Home"), plain(" | "), linked("Docs")];
    let menu = menu_items(&frags).expect("menu");
    assert_eq!(menu.items.len(), 2);
    assert!(menu.items[0][0].link.is_some());
}

#[test]
fn detects_linked_dot_menu() {
    let frags = vec![linked("Inicio"), plain(" · "), linked("Guía")];
    let menu = menu_items(&frags).expect("menu");
    assert_eq!(menu.sep, '·');
    assert_eq!(menu.items.len(), 2);
    assert!(menu.items[1][0].link.is_some());
}

#[test]
fn rejects_table_delimiter() {
    let frags = vec![plain("a | b"), plain("\n"), plain("--- | ---")];
    assert!(menu_items(&frags).is_none(), "multiline is not a menu");
    let single = vec![plain("--- | ---")];
    assert!(
        menu_items(&single).is_none(),
        "delimiter alone is not a menu"
    );
}

#[test]
fn rejects_sentences_and_code() {
    let sentence = vec![plain("See foo | bar for details.")];
    assert!(
        menu_items(&sentence).is_none(),
        "sentence with period is not a menu"
    );
    let sentence = vec![plain("Uno · Dos · Tres. Punto")];
    assert!(
        menu_items(&sentence).is_none(),
        "dot sentence with period is not a menu"
    );
    let code = vec![Frag {
        text: "a | b".to_string().into(),
        strong: false,
        em: false,
        strike: false,
        code: true,
        sup: false,
        link: None,
        image: None,
        badge: None,
    }];
    assert!(menu_items(&code).is_none(), "code pipes are not a menu");
}

#[test]
fn rejects_single_item() {
    let frags = vec![plain("Just text")];
    assert!(menu_items(&frags).is_none());
}

/// A menu row is centered in the reading column (it used to render as a
/// left-aligned paragraph with small separators).
#[test]
fn menu_row_is_centered() {
    let (texts, _) = render_probe("Home | Docs | About\n");
    let home = texts
        .iter()
        .find(|(t, _, _)| t.contains("Home"))
        .expect("Home rendered");
    assert!(
        home.1 > 100.0,
        "menu must be centered, not at the paragraph edge: {texts:?}"
    );
    // The menu line still reads in source order.
    let docs = texts
        .iter()
        .find(|(t, _, _)| t.contains("Docs"))
        .expect("Docs rendered");
    let about = texts
        .iter()
        .find(|(t, _, _)| t.contains("About"))
        .expect("About rendered");
    assert!(docs.1 > home.1 && about.1 > docs.1, "{texts:?}");
}

/// `Uno | Dos | Tres` is a menu (a single non-table line), including the
/// mixed `Home | docs` case; the dot family renders too.
#[test]
fn single_line_pipe_and_dot_menus_render() {
    let (texts, _) = render_probe("Uno | Dos | Tres\n\nUno · Dos · Tres\n");
    for item in ["Uno", "Dos", "Tres"] {
        // Rendered once per menu line (pipe + dot).
        assert_eq!(
            texts.iter().filter(|(t, _, _)| t.as_str() == item).count(),
            2,
            "{item} in both menus: {texts:?}"
        );
    }
}

#[test]
fn doc_salt_is_stable_and_distinct() {
    let a = url::Url::parse("https://ex.com/a/").unwrap();
    let b = url::Url::parse("https://ex.com/b/").unwrap();
    assert_eq!(doc_id_salt(&a), doc_id_salt(&a));
    assert_ne!(doc_id_salt(&a), doc_id_salt(&b));
}

/// A quote's indent id follows the BLOCK (its event index), not the order a
/// frame happens to render blocks in: two quotes keep their own nested
/// widget ids even when a `<Tabs>` panel switch makes one take the other's
/// render slot.
#[test]
fn quote_indent_ids_are_keyed_by_the_block() {
    let salt = doc_id_salt(&url::Url::parse("https://ex.com/a/").unwrap());
    // Stable for the same block ...
    assert_eq!(quote_indent_id(salt, 1, 10), quote_indent_id(salt, 1, 10));
    // ... distinct across blocks, whatever order they render in ...
    assert_ne!(quote_indent_id(salt, 1, 10), quote_indent_id(salt, 1, 42));
    // ... and across nesting levels and documents.
    assert_ne!(quote_indent_id(salt, 1, 10), quote_indent_id(salt, 2, 10));
    let other = doc_id_salt(&url::Url::parse("https://ex.com/b/").unwrap());
    assert_ne!(quote_indent_id(salt, 1, 10), quote_indent_id(other, 1, 10));
}

/// One painted text run: `(text, x, y)` with x including the glyph
/// offset (the real painted position).
pub(super) type TextRun = (String, f32, f32);
/// One horizontal rule: `(x_min, x_max)`.
pub(super) type HLine = (f32, f32);

/// Renders `md` headlessly and returns every text run and horizontal
/// line segment (separators).
pub(super) fn render_probe(md: &str) -> (Vec<TextRun>, Vec<HLine>) {
    render_probe_sized(md, 750.0)
}

/// Like [`render_probe`] with a custom viewport height: hostile nesting
/// stacks rows (one per level), so those probes need more vertical room
/// or egui culls the fully clipped content.
pub(super) fn render_probe_sized(md: &str, screen_h: f32) -> (Vec<TextRun>, Vec<HLine>) {
    render_probe_sized_wh(md, 1100.0, screen_h)
}

/// Like [`render_probe_sized`] with an explicit width too: deep list
/// nesting pushes content right one indentation step per level, so the
/// hostile-list probe needs horizontal room as well.
pub(super) fn render_probe_sized_wh(
    md: &str,
    screen_w: f32,
    screen_h: f32,
) -> (Vec<TextRun>, Vec<HLine>) {
    probe_shapes(&render_shapes(md, screen_w, screen_h))
}

/// Every painted shape for `md`, rendered headlessly at `screen_w` by
/// `screen_h`, with no anchor index (links to `#anchors` are inert).
pub(super) fn render_shapes(md: &str, screen_w: f32, screen_h: f32) -> Vec<egui::epaint::Shape> {
    render_shapes_with_anchors(
        md,
        screen_w,
        screen_h,
        &crate::markdown::anchors::AnchorIndex::default(),
    )
}

/// Like [`render_shapes`] with a real anchor index (footnote numbering and
/// `#fragment` link resolution depend on it).
pub(super) fn render_shapes_with_anchors(
    md: &str,
    screen_w: f32,
    screen_h: f32,
    anchors: &crate::markdown::anchors::AnchorIndex,
) -> Vec<egui::epaint::Shape> {
    render_shapes_opts(md, screen_w, screen_h, anchors, false).0
}

/// Like [`render_shapes_with_anchors`], plus the content column's bottom
/// edge (the parent `Ui`'s `min_rect`): the trailing `spacing::PAGE_END` pad
/// paints no shape, so a test that measures the blank end margin needs the
/// laid-out bottom, not the shape bounding box.
pub(super) fn render_shapes_with_bottom(
    md: &str,
    screen_w: f32,
    screen_h: f32,
    anchors: &crate::markdown::anchors::AnchorIndex,
) -> (Vec<egui::epaint::Shape>, f32) {
    render_shapes_opts(md, screen_w, screen_h, anchors, false)
}

/// Like [`render_shapes_with_anchors`] with the "Select all" highlight on.
pub(super) fn render_shapes_selected(
    md: &str,
    screen_w: f32,
    screen_h: f32,
    anchors: &crate::markdown::anchors::AnchorIndex,
) -> Vec<egui::epaint::Shape> {
    render_shapes_opts(md, screen_w, screen_h, anchors, true).0
}

fn render_shapes_opts(
    md: &str,
    screen_w: f32,
    screen_h: f32,
    anchors: &crate::markdown::anchors::AnchorIndex,
    selected: bool,
) -> (Vec<egui::epaint::Shape>, f32) {
    let ctx = egui::Context::default();
    // Bold text renders through a named family: a bare context must bind
    // the embedded fonts or epaint panics on the lookup.
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen_w, screen_h),
        )),
        ..Default::default()
    };
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    let normalized = crate::markdown::document::normalize_body(md);
    let events = crate::markdown::document::parse_events(&normalized).expect("parse");
    let mut images = crate::media::cache::ImageCache::new();
    let mut emoji = crate::media::emoji::EmojiCache::new();
    let mut bottom = 0.0f32;
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            // A vertical child, like the real reading column: the panel
            // `Ui`'s own rect fills the window, so the column's `min_rect`
            // is what reports the content's laid-out bottom.
            ui.vertical(|ui| {
                let mut on_link = |_: &str| {};
                let mut links = Vec::new();
                // `interactive = false`: links are inert and no image fetch
                // starts (the cache would otherwise hit the network).
                show_markdown(
                    ui,
                    &events,
                    anchors,
                    &crate::protocol::rewrite::DestCache::build(&base, anchors, &events),
                    &mut None,
                    doc_id_salt(&base),
                    selected,
                    false,
                    screen_w,
                    1.0,
                    &mut images,
                    &mut emoji,
                    &mut on_link,
                    &mut links,
                );
                // Laid-out bottom of the column, pad included (it paints no
                // shape, so the shape list alone cannot measure it).
                bottom = ui.min_rect().bottom();
            });
        });
    });
    let shapes = out.shapes.iter().map(|c| c.shape.clone()).collect();
    out.textures_delta.clear();
    (shapes, bottom)
}

/// Text runs and horizontal rules of a painted shape list (the extraction
/// [`render_probe_sized_wh`] does).
pub(super) fn probe_shapes(shapes: &[egui::epaint::Shape]) -> (Vec<TextRun>, Vec<HLine>) {
    let mut texts = Vec::new();
    let mut lines = Vec::new();
    for shape in shapes {
        match shape {
            egui::epaint::Shape::Text(t) => {
                let gx = t
                    .galley
                    .rows
                    .first()
                    .and_then(|r| r.glyphs.first())
                    .map(|g| g.pos.x)
                    .unwrap_or(0.0);
                texts.push((t.galley.text().to_string(), t.pos.x + gx, t.pos.y));
            }
            egui::epaint::Shape::LineSegment { points, .. }
                if (points[0].y - points[1].y).abs() < 0.5 =>
            {
                lines.push((points[0].x.min(points[1].x), points[0].x.max(points[1].x)));
            }
            _ => {}
        }
    }
    (texts, lines)
}

/// Ink center and baseline of a painted galley's first glyph, relative
/// to the galley's top-left corner. `Glyph::uv_rect` is the INKED
/// rectangle, measured from the glyph's baseline origin, so this is what
/// the eye sees, not the line box.
pub(super) fn glyph_ink(galley: &egui::Galley) -> (egui::Vec2, f32) {
    let placed = galley.rows.first().expect("a text row");
    let glyph = placed.row.glyphs.first().expect("a glyph");
    (
        placed.pos.to_vec2()
            + glyph.pos.to_vec2()
            + glyph.uv_rect.offset
            + glyph.uv_rect.size / 2.0,
        placed.pos.y + glyph.pos.y,
    )
}

/// First painted galley whose text matches `needle` exactly, with its
/// paint position.
pub(super) fn galley_at<'a>(
    shapes: &'a [egui::epaint::Shape],
    needle: &str,
) -> (egui::Pos2, &'a egui::Galley) {
    shapes
        .iter()
        .find_map(|s| match s {
            egui::epaint::Shape::Text(t) if t.galley.text() == needle => {
                Some((t.pos, t.galley.as_ref()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("text {needle:?} not painted"))
}

pub(super) fn x_of(texts: &[(String, f32, f32)], needle: &str) -> f32 {
    texts
        .iter()
        .find(|(t, _, _)| t == needle)
        .map(|(_, x, _)| *x)
        .unwrap_or_else(|| panic!("text {needle:?} not found in {texts:?}"))
}

pub(super) fn y_of(texts: &[(String, f32, f32)], needle: &str) -> f32 {
    texts
        .iter()
        .find(|(t, _, _)| t == needle)
        .map(|(_, _, y)| *y)
        .unwrap_or_else(|| panic!("text {needle:?} not found in {texts:?}"))
}

/// Y of the first painted text that CONTAINS `needle`: fragment boundaries
/// are not stable once inline tags are stripped, so tests match substrings.
pub(super) fn y_of_contains(texts: &[(String, f32, f32)], needle: &str) -> f32 {
    texts
        .iter()
        .find(|(t, _, _)| t.contains(needle))
        .map(|(_, _, y)| *y)
        .unwrap_or_else(|| panic!("text containing {needle:?} not found in {texts:?}"))
}

/// Position of the nth document bullet (`·`) in paint order.
pub(super) fn bullet_at(texts: &[(String, f32, f32)], n: usize) -> (f32, f32) {
    texts
        .iter()
        .filter(|(t, _, _)| t == "\u{00B7}")
        .nth(n)
        .map(|(_, x, y)| (*x, *y))
        .unwrap_or_else(|| panic!("bullet {n} not found in {texts:?}"))
}
