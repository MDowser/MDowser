// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{visual_lines, Badge, Frag, LinkHit, MAX_PILL_H};
use eframe::egui;

/// Relative luminance (WCAG 2.1).
fn luminance(c: egui::Color32) -> f32 {
    fn ch(v: u8) -> f32 {
        let s = v as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// Composites a translucent fill over an opaque background.
fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
    let a = fg.a() as f32 / 255.0;
    let m = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)).round() as u8;
    egui::Color32::from_rgb(m(fg.r(), bg.r()), m(fg.g(), bg.g()), m(fg.b(), bg.b()))
}

fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Badge ink must stay readable over its tint in BOTH themes (the old solid
/// accent fill with body-colored text read dark-on-dark in light mode and
/// light-on-light in dark mode).
#[test]
fn badge_ink_is_readable_in_both_schemes() {
    for scheme in [crate::ui::theme::light(), crate::ui::theme::dark()] {
        for badge in [
            Badge::Neutral,
            Badge::Note,
            Badge::Tip,
            Badge::Warning,
            Badge::Caution,
        ] {
            let (fill, ink, _stroke) = super::badge_colors(&scheme, badge);
            let bg = over(fill, scheme.surface);
            let ratio = contrast(ink, bg);
            assert!(ratio >= 3.5, "{badge:?} contrast {ratio:.2} too low");
        }
    }
}

fn plain(text: &str) -> Frag<'static> {
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

/// Renders one inline flow at `width` px and returns the context, the
/// frame output and the link hitboxes collected by the renderer.
fn run_flow(
    frags: &[Frag<'_>],
    width: f32,
    interactive: bool,
) -> (egui::Context, egui::FullOutput, Vec<LinkHit>) {
    run_flow_selected(frags, width, interactive, false)
}

/// [`run_flow`] with the select-all highlight flag.
fn run_flow_selected(
    frags: &[Frag<'_>],
    width: f32,
    interactive: bool,
    selected: bool,
) -> (egui::Context, egui::FullOutput, Vec<LinkHit>) {
    use crate::media::cache::ImageCache;
    use eframe::egui;

    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 200.0),
        )),
        ..Default::default()
    };
    let mut links = Vec::new();
    let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
    let out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut images = ImageCache::new();
            let mut emoji = crate::media::emoji::EmojiCache::new();
            super::inline_flow(ui, frags, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for f in frags {
                    super::render_frag(
                        ui,
                        f,
                        selected,
                        interactive,
                        &mut on_link,
                        &mut images,
                        &mut emoji,
                        &mut links,
                        &mut None,
                    );
                }
            });
        });
    });
    (ctx, out, links)
}

/// The one drawn text shape whose content is exactly `text`.
fn text_shape<'a>(out: &'a egui::FullOutput, text: &str) -> Option<&'a egui::epaint::TextShape> {
    out.shapes.iter().find_map(|clipped| match &clipped.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t),
        _ => None,
    })
}

/// First-glyph x of a shape. A wrapping `Label` bakes a `leading_space`
/// into the galley, so `shape.pos.x` may sit left of the actual ink.
fn text_left(t: &egui::epaint::TextShape) -> f32 {
    let leading = t
        .galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(0.0, |g| g.pos.x);
    t.pos.x + leading
}

/// Right edge of the run: the galley's widest row (which includes the
/// baked-in leading space of its first row).
fn text_right(t: &egui::epaint::TextShape) -> f32 {
    t.pos.x + t.galley.size().x
}

/// Bounding box of every vertex painted with the code-pill background.
fn painted_pill(ctx: &egui::Context, out: &egui::FullOutput) -> Option<egui::Rect> {
    let bg = crate::ui::theme::light().code_bg;
    let prims = ctx.tessellate(out.shapes.clone(), 1.0);
    let mut bounds: Option<egui::Rect> = None;
    for p in &prims {
        if let egui::epaint::Primitive::Mesh(mesh) = &p.primitive {
            for v in &mesh.vertices {
                if v.color == bg {
                    bounds = Some(match bounds {
                        Some(r) => r.union(egui::Rect::from_min_max(v.pos, v.pos)),
                        None => egui::Rect::from_min_max(v.pos, v.pos),
                    });
                }
            }
        }
    }
    bounds
}

#[test]
fn splits_on_hard_breaks() {
    let frags = vec![plain("a"), plain("\n"), plain("b")];
    let lines = visual_lines(&frags);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].len(), 1);
    assert_eq!(lines[1].len(), 1);
}

#[test]
fn no_break_is_one_line() {
    let frags = vec![plain("a"), plain("b")];
    assert_eq!(visual_lines(&frags).len(), 1);
}

/// The pill's padding is real layout air: words glued to the code span
/// (`foo`code`bar`, no spaces in the source) keep a full
/// [`super::CODE_PILL_PAD_X`] on each side, and the painted pill never
/// reaches under the neighboring glyphs. Regression guard: the old pill
/// painted its padding OVER the neighbors (no layout space reserved).
#[test]
fn code_pill_keeps_margin_with_the_neighbors() {
    let pad_x = super::CODE_PILL_PAD_X;
    let mut code = plain("code");
    code.code = true;
    let frags = vec![plain("foo"), code, plain("bar")];
    let (ctx, mut out, _links) = run_flow(&frags, 400.0, true);

    let foo = text_shape(&out, "foo").expect("body run before the code");
    let code = text_shape(&out, "code").expect("code run");
    let bar = text_shape(&out, "bar").expect("body run after the code");
    let foo_right = text_right(foo);
    let code_left = text_left(code);
    let code_right = text_right(code);
    let bar_left = text_left(bar);

    assert!(
        (code_left - foo_right - pad_x).abs() < 1.0,
        "code starts {pad_x}px after the previous word (got {})",
        code_left - foo_right
    );
    assert!(
        (bar_left - code_right - pad_x).abs() < 1.0,
        "the next word starts {pad_x}px after the code (got {})",
        bar_left - code_right
    );
    let pill = painted_pill(&ctx, &out).expect("pill painted");
    assert!(
        pill.left() >= foo_right - 0.5,
        "pill left {} must not overlap the previous word (ends {foo_right})",
        pill.left()
    );
    assert!(
        pill.right() <= bar_left + 0.5,
        "pill right {} must not overlap the next word (starts {bar_left})",
        pill.right()
    );
    out.textures_delta.clear();
}

/// A soft-wrapped span paints one tight pill PER ROW: the second row's
/// pill hugs that row's shorter glyph run instead of a single box
/// spanning the whole two-row label (the old full-width gray bar).
#[test]
fn wrapped_code_span_paints_a_pill_per_row() {
    let long = "this-is-a-very-long-inline-code-span-that-should-wrap-across-two-visible-rows";
    let mut code = plain(long);
    code.code = true;
    let frags = vec![code];
    let (_ctx, mut out, _links) = run_flow(&frags, 420.0, false);

    let t = text_shape(&out, long).expect("code run");
    assert!(
        t.galley.rows.len() >= 2,
        "the span must soft-wrap for this test (got {} rows)",
        t.galley.rows.len()
    );
    // A span starting at the column's left edge must stay on this row
    // (no empty row above it).
    assert!(
        t.pos.y < 10.0,
        "the wrapped span must not reserve an empty first row (y {})",
        t.pos.y
    );
    let content = egui::Rect::from_min_size(t.pos, t.galley.size());
    let pills = super::code_pill_rows(&t.galley, content);
    assert_eq!(
        pills.len(),
        t.galley.rows.len(),
        "one pill per visual row, none skipped"
    );
    for (i, pill) in pills.iter().enumerate() {
        assert!(
            pill.left() >= content.left() - super::CODE_PILL_PAD_X - 0.5,
            "row {i}: pill must stay inside the reserved content box"
        );
        assert!(
            pill.right() <= content.right() + super::CODE_PILL_PAD_X + 0.5,
            "row {i}: pill must stay inside the reserved content box"
        );
        assert!(
            pill.height() <= crate::ui::theme::BODY_LINE_HEIGHT,
            "row {i}: pill must hug the glyphs, not the whole line box"
        );
    }
    // The rows hug DIFFERENT runs: a single box over the whole label
    // (the old bug) could not have a higher top and a wider bottom edge.
    assert!(pills[0].top() < pills[1].top(), "rows at different tops");
    assert!(
        pills[1].right() < pills[0].right(),
        "the second row's pill hugs its own (shorter) run: {} vs {}",
        pills[1].right(),
        pills[0].right()
    );
    out.textures_delta.clear();
}

/// A code span that does not fit after the previous word moves to the
/// next row AS A UNIT: one row, starting at the column's left edge, with
/// the following text right after it. The old label broke mid-word and
/// reserved an empty first row.
#[test]
fn code_span_that_does_not_fit_moves_to_the_next_row() {
    let mut code = plain("Content-Type");
    code.code = true;
    let frags = vec![plain("xxxxxxxxxxxxxxxxxxxx"), code, plain(": sets Vary.")];
    let (_ctx, mut out, _links) = run_flow(&frags, 240.0, true);

    let body = text_shape(&out, "xxxxxxxxxxxxxxxxxxxx").expect("body run");
    let code = text_shape(&out, "Content-Type").expect("code run");
    let after = text_shape(&out, ": sets Vary.").expect("trailing text");
    assert_eq!(
        code.galley.rows.len(),
        1,
        "the pill must never break mid-word"
    );
    assert!(
        code.pos.y >= body.pos.y + 20.0,
        "the pill must move below the body row ({} vs {})",
        code.pos.y,
        body.pos.y
    );
    assert!(
        (code.pos.x - (body.pos.x + super::CODE_PILL_PAD_X)).abs() < 1.0,
        "the moved pill starts at the column edge (x {} vs column {})",
        code.pos.x,
        body.pos.x
    );
    assert!(
        (after.pos.y - code.pos.y).abs() < 0.01,
        "the trailing text stays on the pill's row"
    );
    assert!(
        text_left(after) >= code.pos.x + code.galley.size().x + super::CODE_PILL_PAD_X - 1.0,
        "the trailing text starts after the pill's right padding"
    );
    out.textures_delta.clear();
}

/// Code inside a link uses the same tight pill (the old `.code()`
/// background filled the whole line box and left a gray band below the
/// words), and the whole pill is offered as the link's hitbox.
#[test]
fn linked_code_span_uses_the_tight_pill() {
    let mut code = plain("README.md");
    code.code = true;
    code.link = Some("https://example.com/README.md".to_string().into());
    let frags = vec![plain("see "), code, plain(" now")];
    let (ctx, mut out, links) = run_flow(&frags, 400.0, true);

    assert_eq!(links.len(), 1, "the linked code span is one link");
    let t = text_shape(&out, "README.md").expect("linked code run");
    assert!(
        links[0].rect.width() >= t.galley.size().x + 2.0 * super::CODE_PILL_PAD_X - 1.0,
        "the link hitbox covers the whole pill including its padding"
    );
    let pill = painted_pill(&ctx, &out).expect("pill painted");
    assert!(
        pill.height() < crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "linked code must keep the tight pill (height {})",
        pill.height()
    );
    out.textures_delta.clear();
}

/// A code link with an inert destination (unknown anchor, rejected
/// scheme) paints the tight pill but never registers a link hitbox.
#[test]
fn inert_code_link_paints_a_pill_without_a_hit() {
    let mut code = plain("broken");
    code.code = true;
    code.link = Some(String::new().into());
    let frags = vec![code];
    let (ctx, mut out, links) = run_flow(&frags, 400.0, true);

    assert!(links.is_empty(), "an inert destination must not be a link");
    let pill = painted_pill(&ctx, &out).expect("pill painted");
    assert!(
        pill.height() < crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "inert code keeps the tight pill (height {})",
        pill.height()
    );
    out.textures_delta.clear();
}

/// Loading-backdrop code links stay inert (no hitbox, no navigation) but
/// keep the link color and the tight pill.
#[test]
fn backdrop_code_link_paints_a_pill_without_a_hit() {
    let mut code = plain("live-blue");
    code.code = true;
    code.link = Some("https://example.com/".to_string().into());
    let frags = vec![code];
    let (ctx, mut out, links) = run_flow(&frags, 400.0, false);

    assert!(links.is_empty(), "the backdrop must not register links");
    let pill = painted_pill(&ctx, &out).expect("pill painted");
    assert!(
        pill.height() < crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "backdrop code keeps the tight pill (height {})",
        pill.height()
    );
    out.textures_delta.clear();
}

/// Select-all keeps the full-row `RichText::code()` background: the
/// highlight must cover the whole line box, so the tight pill is not
/// used while `selected`.
#[test]
fn selected_code_span_keeps_the_full_row_background() {
    let mut code = plain("selected");
    code.code = true;
    let frags = vec![plain("a "), code, plain(" b")];
    let (ctx, mut out, _links) = run_flow_selected(&frags, 400.0, false, true);

    let pill = painted_pill(&ctx, &out).expect("background painted");
    assert!(
        pill.height() > crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "select-all must cover the full line box (height {})",
        pill.height()
    );
    out.textures_delta.clear();
}

/// RTL flows get the same pill (the placement helpers are
/// direction-agnostic): the span must render without breaking the line
/// or losing its background.
#[test]
fn rtl_flow_still_paints_the_code_pill() {
    let mut code = plain("code");
    code.code = true;
    let frags = vec![plain("שלום "), code, plain(" עולם")];
    let (ctx, mut out, _links) = run_flow(&frags, 400.0, true);

    assert!(
        text_shape(&out, "code").is_some(),
        "the RTL flow draws the code run"
    );
    let pill = painted_pill(&ctx, &out).expect("pill painted");
    assert!(
        pill.height() < crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "RTL code keeps the tight pill (height {})",
        pill.height()
    );
    out.textures_delta.clear();
}

/// Inline code must paint a pill that hugs the glyphs and sit on the BODY
/// text baseline.
///
/// epaint sizes a `RichText::code()` background from the glyph's
/// `line_height` (the viewer pins it to `BODY_LINE_HEIGHT`), so the pill
/// covered the whole 24px line box and left a gray band below the text;
/// and a smaller font's baseline lands at its own ascent, above the
/// surrounding words. Regression guard for both.
#[test]
fn code_pill_hugs_the_glyphs_and_keeps_the_body_baseline() {
    use super::{inline_flow, render_frag, CODE_PILL_PAD_Y};
    use crate::media::cache::ImageCache;
    use eframe::egui;

    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let scheme = crate::ui::theme::light();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(700.0, 200.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut code = plain("Accept: text/markdown");
            code.code = true;
            let frags = vec![plain("serves Markdown for "), code, plain(", sets Vary.")];
            let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
            let mut links = Vec::new();
            let mut images = ImageCache::new();
            let mut emoji = crate::media::emoji::EmojiCache::new();
            inline_flow(ui, &frags, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for f in &frags {
                    render_frag(
                        ui,
                        f,
                        false,
                        false,
                        &mut on_link,
                        &mut images,
                        &mut emoji,
                        &mut links,
                        &mut None,
                    );
                }
            });
        });
    });
    // Baseline of the first glyph: galley position + glyph position.
    let baseline = |t: &egui::epaint::TextShape| t.pos.y + t.galley.rows[0].glyphs[0].pos.y;
    let mut body = None;
    let mut code = None;
    for clipped in &out.shapes {
        if let egui::epaint::Shape::Text(t) = &clipped.shape {
            match t.galley.text() {
                "serves Markdown for " => body = Some(baseline(t)),
                "Accept: text/markdown" => {
                    let g = &t.galley.rows[0].glyphs[0];
                    code = Some((baseline(t), g.font_ascent, g.font_height, t.pos.y));
                }
                _ => {}
            }
        }
    }
    let (code_baseline, code_ascent, code_height, _code_top) = code.expect("code span");
    let body_baseline = body.expect("body run");
    assert!(
        (code_baseline - body_baseline).abs() < 0.51,
        "code baseline {code_baseline} must match body baseline {body_baseline}"
    );
    // The pill: the only vertices painted with the code background color.
    let prims = ctx.tessellate(out.shapes.clone(), 1.0);
    let mut pill: Option<(f32, f32)> = None;
    for p in &prims {
        if let egui::epaint::Primitive::Mesh(mesh) = &p.primitive {
            for v in &mesh.vertices {
                if v.color == scheme.code_bg {
                    let e = pill.get_or_insert((v.pos.y, v.pos.y));
                    e.0 = e.0.min(v.pos.y);
                    e.1 = e.1.max(v.pos.y);
                }
            }
        }
    }
    let (pill_top, pill_bottom) = pill.expect("pill painted");
    let pill_h = pill_bottom - pill_top;
    assert!(
        pill_h <= code_height + 2.0 * CODE_PILL_PAD_Y + 0.6,
        "pill height {pill_h} must hug the {code_height}px code font"
    );
    assert!(
        pill_h < crate::ui::theme::BODY_LINE_HEIGHT - 2.0,
        "pill must not fill the whole {}px line box (it was {pill_h})",
        crate::ui::theme::BODY_LINE_HEIGHT
    );
    let glyph_top = code_baseline - code_ascent - CODE_PILL_PAD_Y;
    let glyph_bottom = code_baseline + (code_height - code_ascent) + CODE_PILL_PAD_Y;
    // The pill hugs the face box, CAPPED at `MAX_PILL_H` (the bundled mono
    // cut's em box is taller than a normal UI face, and the pill must not
    // become a second line box).
    let (expected_top, expected_bottom) = if glyph_bottom - glyph_top > MAX_PILL_H {
        let center = (glyph_top + glyph_bottom) * 0.5;
        (center - MAX_PILL_H * 0.5, center + MAX_PILL_H * 0.5)
    } else {
        (glyph_top, glyph_bottom)
    };
    // Tolerance 1px: epaint rounds a rect to whole physical pixels and
    // insets the pure-color vertices by half the feather, so the painted
    // top can sit up to ~1px below the exact glyph-box top. The guard
    // still catches the old full-line-box background (a ~24px band).
    assert!(
        (pill_top - expected_top).abs() < 1.0,
        "pill top {pill_top} must sit at the glyph box top {expected_top} (face box {glyph_top}..{glyph_bottom}, cap {MAX_PILL_H})"
    );
    assert!(
        (pill_bottom - expected_bottom).abs() < 1.0,
        "pill bottom {pill_bottom} must sit at {expected_bottom}"
    );
    out.textures_delta.clear();
}
