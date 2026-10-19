// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{content_margin, content_max_width, dark, light, tonal_button_sized, type_scale};
use eframe::egui;

/// sRGB relative luminance (WCAG) of one color.
fn rel_luminance(c: egui::Color32) -> f32 {
    fn ch(v: u8) -> f32 {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio between two opaque colors.
fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    let (la, lb) = (rel_luminance(a), rel_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Native text selection must stay READABLE: egui paints selected
/// glyphs with `selection.stroke.color` (a `Stroke::NONE` color made
/// them transparent), on top of `selection.bg_fill`.
#[test]
fn selection_colors_stay_readable_in_both_themes() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    for (theme, scheme) in [(egui::Theme::Light, light()), (egui::Theme::Dark, dark())] {
        let visuals = ctx.style_of(theme).visuals.clone();
        let sel = visuals.selection;
        assert_eq!(
            sel.bg_fill, scheme.primary_container,
            "{theme:?}: selection must use the container tone"
        );
        assert!(
            sel.stroke.color.a() == 255,
            "{theme:?}: selected text must not be transparent"
        );
        assert_eq!(
            sel.stroke.color, scheme.on_surface,
            "{theme:?}: selected text must keep the readable on-surface color"
        );
        assert!(
            sel.stroke.width <= 0.0,
            "{theme:?}: no stroke border around the selection"
        );
        let ratio = contrast(sel.stroke.color, sel.bg_fill);
        assert!(
            ratio >= 4.5,
            "{theme:?}: selected text contrast {ratio:.2} below WCAG AA"
        );
    }
}

/// Body text, muted text and links meet WCAG AA against the page in
/// both themes: a palette regression that darkens the page or lightens
/// the text fails here instead of shipping low-contrast reading text.
#[test]
fn body_and_link_contrast_meet_wcag_aa() {
    for (name, scheme) in [("light", light()), ("dark", dark())] {
        let body = contrast(scheme.on_surface, scheme.surface);
        assert!(
            body >= 7.0,
            "{name}: body contrast {body:.2} below WCAG AAA"
        );
        let muted = contrast(scheme.on_surface_variant, scheme.surface);
        assert!(
            muted >= 4.5,
            "{name}: muted contrast {muted:.2} below WCAG AA"
        );
        let link = contrast(scheme.primary, scheme.surface);
        assert!(link >= 4.5, "{name}: link contrast {link:.2} below WCAG AA");
    }
}

/// In-page find washes sit BEHIND `on_surface` text (see `find_text`), so
/// both the ordinary and the current match must keep that text readable.
/// The current match stays a distinct, stronger wash than the ordinary one.
#[test]
fn find_washes_keep_reading_text_legible() {
    for (name, scheme) in [("light", light()), ("dark", dark())] {
        for (kind, wash) in [
            ("match", scheme.find_match),
            ("current", scheme.find_current),
        ] {
            let ratio = contrast(scheme.on_surface, wash);
            assert!(
                ratio >= 4.5,
                "{name}/{kind}: find text contrast {ratio:.2} below WCAG AA"
            );
        }
        let step = contrast(scheme.find_current, scheme.find_match);
        assert!(
            step >= 1.2,
            "{name}: the current find wash must stand out from a match ({step:.2})"
        );
    }
}

/// The floating document scrollbar must be VISIBLE: the handle paints
/// `widgets.inactive.fg_stroke` (with `foreground_color = true`) at the
/// dormant opacity over the page, and it used to paint `container_high` —
/// the same gray as the chrome — at ~1.1:1. There is no WCAG minimum for a
/// decorative component, so the assertion pins only that the handle is a
/// real, visible step above the old chrome-gray value (which measured
/// ~1.09 light / ~1.13 dark); a regression back to the chrome tone fails
/// here.
#[test]
fn scrollbar_handle_stays_visible_in_both_themes() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    for (theme, scheme) in [(egui::Theme::Light, light()), (egui::Theme::Dark, dark())] {
        let style = ctx.style_of(theme);
        let scroll = style.spacing.scroll;
        let opacity = scroll.dormant_handle_opacity;
        assert!(scroll.foreground_color, "{theme:?}: handle color");
        assert!(opacity > 0.0, "{theme:?}: a dormant handle must paint");
        // Blend the handle over the page it floats on (the sheet surface)
        // and require a visible contrast step.
        let handle = blend(
            style.visuals.widgets.inactive.fg_stroke.color,
            scheme.surface,
            opacity,
        );
        let ratio = contrast(handle, scheme.surface);
        assert!(
            ratio >= 1.2,
            "{theme:?}: dormant scrollbar handle contrast {ratio:.2} is invisible on the page"
        );
    }
}

/// Linear blend of `top` over `bottom` at `opacity` (the scrollbar paints
/// `handle_color.gamma_multiply(opacity)` over the page).
fn blend(top: egui::Color32, bottom: egui::Color32, opacity: f32) -> egui::Color32 {
    let mix = |t: u8, b: u8| (t as f32 * opacity + b as f32 * (1.0 - opacity)).round() as u8;
    egui::Color32::from_rgb(
        mix(top.r(), bottom.r()),
        mix(top.g(), bottom.g()),
        mix(top.b(), bottom.b()),
    )
}

/// The font stack must be exactly the bundled Noto faces, with a bundled
/// primary in each family (Noto Sans proportional, Noto Sans Mono CJK
/// monospace): the complete emoji face and the rest follow as glyph
/// fallbacks, and epaint's bundled faces (Ubuntu Light, Hack, its old Noto
/// Emoji subset, emoji-icon-font) must not be registered at all: nothing
/// outside `assets/` may paint.
#[test]
fn complete_emoji_font_is_registered() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(200.0, 100.0),
        )),
        ..Default::default()
    };
    // Fonts are built during a pass: the definitions are readable in it.
    let mut out = ctx.run_ui(input, |root| {
        root.ctx().fonts_mut(|f| {
            let defs = f.definitions();
            for name in ["noto-emoji-full", "noto-cjk", "noto-cjk-mono", "noto-sans"] {
                assert!(defs.font_data.contains_key(name), "{name} missing");
            }
            for name in [
                "Ubuntu-Light",
                "Hack",
                "NotoEmoji-Regular",
                "emoji-icon-font",
            ] {
                assert!(
                    !defs.font_data.contains_key(name),
                    "epaint-bundled {name} must not be registered"
                );
            }
            for (family, primary, third) in [
                // Proportional: Noto Sans (Latin-ext/Vietnamese) leads.
                (egui::FontFamily::Proportional, "noto-sans", "noto-cjk"),
                // Monospace: the bundled mono CUT leads (face 5 of the CJK
                // collection, true monospace Latin), so code keeps its
                // alignment; Noto Sans follows as a fallback.
                (egui::FontFamily::Monospace, "noto-cjk-mono", "noto-sans"),
            ] {
                let list = defs
                    .families
                    .get(&family)
                    .unwrap_or_else(|| panic!("{family:?} has no fonts"));
                assert_eq!(
                    list.first().map(String::as_str),
                    Some(primary),
                    "{family:?} primary face"
                );
                assert_eq!(
                    list.get(1).map(String::as_str),
                    Some("noto-emoji-full"),
                    "{family:?} emoji fallback order"
                );
                assert_eq!(
                    list.get(2).map(String::as_str),
                    Some(third),
                    "{family:?} third fallback order"
                );
                for name in [
                    "Ubuntu-Light",
                    "Hack",
                    "NotoEmoji-Regular",
                    "emoji-icon-font",
                ] {
                    assert!(
                        !list.iter().any(|n| n == name),
                        "{family:?} must not fall back to epaint-bundled {name}"
                    );
                }
            }
        });
    });
    out.textures_delta.clear();
}

/// The fallback chain must really draw CJK, extended Latin/Vietnamese
/// and emoji glyphs: a missing registration or a broken `.ttc` face
/// index fails here instead of showing tofu in the viewer.
///
/// `has_glyph` cannot be used: it reports a false negative for ANY char
/// owned by the family's replacement face (chosen from '◻', which the
/// complete emoji face supplies). Every char is rasterized
/// instead and compared with the replacement glyph a private-use
/// codepoint produces; identical `uv_rect`s mean tofu.
#[test]
fn fallbacks_cover_cjk_latin_and_emoji() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(200.0, 100.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        root.ctx().fonts_mut(|f| {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let id = egui::FontId::new(16.0, family.clone());
                let missing = f
                    .layout_no_wrap('\u{10FFFD}'.to_string(), id.clone(), egui::Color32::WHITE)
                    .rows[0]
                    .glyphs[0]
                    .uv_rect;
                for (text, label) in [
                    // Chinese, Japanese, Korean (kana + Hangul).
                    ("中文 日本語 한국어", "CJK"),
                    // Latin Extended-A (Polish/Czech/Turkish/Romanian).
                    ("Zażółć gęślą jaźń Čeština", "Latin Extended-A"),
                    // Vietnamese (Latin Extended Additional).
                    ("Tiếng Việt", "Vietnamese"),
                    // Complete monochrome emoji.
                    ("🚀🤩🫶", "emoji"),
                    // One base letter per bundled script face: every
                    // one must paint a real glyph, never the
                    // replacement box.
                    ("ن", "Arabic"),
                    ("ש", "Hebrew"),
                    ("न", "Devanagari"),
                    ("ব", "Bengali"),
                    ("ਪ", "Gurmukhi"),
                    ("ગ", "Gujarati"),
                    ("କ", "Odia"),
                    ("த", "Tamil"),
                    ("త", "Telugu"),
                    ("ಕ", "Kannada"),
                    ("മ", "Malayalam"),
                    ("ස", "Sinhala"),
                    ("ท", "Thai"),
                    ("ລ", "Lao"),
                    ("ក", "Khmer"),
                    ("မ", "Myanmar"),
                    ("ሀ", "Ethiopic"),
                    ("Հ", "Armenian"),
                    ("ქ", "Georgian"),
                    ("ހ", "Thaana"),
                    ("ߊ", "N'Ko"),
                    ("Ꭰ", "Cherokee"),
                    ("ᐊ", "Canadian Aboriginal"),
                    ("⠿", "Symbols 2"),
                ] {
                    for c in text.chars().filter(|c| !c.is_whitespace()) {
                        let real = f
                            .layout_no_wrap(c.to_string(), id.clone(), egui::Color32::WHITE)
                            .rows[0]
                            .glyphs[0]
                            .uv_rect;
                        assert_ne!(real, missing, "{family:?}: {label} glyph {c:?}");
                    }
                }
            }
        });
    });
    out.textures_delta.clear();
}

/// The reading sheet is flush with ALL FOUR window edges (the rail
/// included: those pixels belong to the content) while keeping the
/// rounded rail-side corner and NO frame margin: the gutter and the
/// always-visible scrollbar travel inside the scroll content, so the bar
/// hugs the sheet's right edge.
#[test]
fn content_frame_is_flush_on_all_sides() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let frame = super::content_frame(ui);
            assert_eq!(frame.outer_margin.top, 0, "flush with the top edge");
            assert_eq!(frame.outer_margin.right, 0, "flush with the right edge");
            assert_eq!(frame.outer_margin.bottom, 0, "flush with the bottom edge");
            assert_eq!(frame.outer_margin.left, 0, "flush against the rail");
            assert_eq!(frame.inner_margin.top, 0, "no frame margin");
            assert_eq!(frame.inner_margin.right, 0, "the bar hugs the edge");
            assert_eq!(frame.inner_margin.bottom, 0, "no frame margin");
            assert_eq!(frame.inner_margin.left, 0, "no frame margin");
            // Only the rail-side corner stays rounded: flush sides
            // must not reveal chrome triangles through cutouts.
            assert!(frame.corner_radius.nw > 0);
            assert_eq!(frame.corner_radius.ne, 0);
            assert_eq!(frame.corner_radius.sw, 0);
            assert_eq!(frame.corner_radius.se, 0);
        });
    });
    out.textures_delta.clear();
}

/// The responsive gutter travels INSIDE the scroll content: the frame
/// keeps no margin at all (the sheet -- and its bar -- stay flush with
/// the window edges) while the pad stays bounded and matches
/// `content_margin` for every viewport class.
#[test]
fn content_gutter_is_responsive_inside_the_scroll() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 700.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let avail = ui.available_width();
            let frame = super::content_frame(ui);
            let pad = super::content_gutter(ui);
            assert_eq!(frame.inner_margin.top, 0, "vertical clip at the edge");
            assert_eq!(frame.inner_margin.bottom, 0, "vertical clip at the edge");
            assert_eq!(frame.inner_margin.left, 0, "the content owns the gutters");
            assert_eq!(frame.inner_margin.right, 0, "the bar hugs the edge");
            assert!(pad > 0.0 && pad <= 16.0, "responsive gutter: {pad}");
            assert_eq!(pad, super::content_margin(avail) as f32);
        });
    });
    out.textures_delta.clear();
}

#[test]
fn layout_helpers_stay_bounded() {
    // Non-finite / degenerate viewports must not propagate NaN/Inf.
    assert_eq!(content_max_width(f32::NAN), 760.0);
    assert_eq!(content_max_width(f32::INFINITY), 760.0);
    assert_eq!(content_max_width(0.0), 0.0);
    assert_eq!(content_max_width(-10.0), 0.0);
    assert!(content_max_width(1100.0) <= 760.0);
    assert_eq!(content_margin(320.0), 8);
    assert_eq!(content_margin(800.0), 12);
    assert_eq!(content_margin(1400.0), 16);
    assert_eq!(type_scale(320.0), 0.95);
    assert_eq!(type_scale(600.0), 1.0);
    assert_eq!(type_scale(900.0), 1.06);
}

/// Every widget state keeps one `bg_stroke.width` (and zero `expansion`):
/// a `small` button subtracts that width from its zeroed vertical
/// padding, so a state with a thinner stroke (the old transparent
/// `active`) allocated 2px less and shifted its neighbors on press/focus.
#[test]
fn widget_states_share_one_stroke_width() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let v = ctx.style_of(theme).visuals.clone();
        let states = [
            ("noninteractive", &v.widgets.noninteractive),
            ("inactive", &v.widgets.inactive),
            ("hovered", &v.widgets.hovered),
            ("active", &v.widgets.active),
            ("open", &v.widgets.open),
        ];
        let width = v.widgets.inactive.bg_stroke.width;
        assert!(width > 0.0, "{theme:?}: inactive needs a real stroke");
        for (name, state) in states {
            assert_eq!(state.expansion, 0.0, "{theme:?}/{name}: expansion");
            assert_eq!(
                state.bg_stroke.width, width,
                "{theme:?}/{name}: stroke width must match every other state"
            );
        }
    }
}

/// A `small` `Button` (the rendered code `Copy` / error `Copy log`) must
/// keep its rect in EVERY pointer state: idle, hover, press and click.
/// The active state used to lose its 1px `bg_stroke` width, which the
/// small-button padding math turns into 1px less per side: the pressed
/// button shrank 2px and the label after it moved up 1px.
#[test]
fn button_rect_is_stable_in_every_pointer_state() {
    fn run(ctx: &egui::Context, input: egui::RawInput, rects: &mut Vec<(egui::Rect, egui::Rect)>) {
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                ui.horizontal(|ui| {
                    let button = ui.small_button("Copy");
                    let after = ui.label("after");
                    rects.push((button.rect, after.rect));
                });
            });
        });
        out.textures_delta.clear();
    }

    let ctx = egui::Context::default();
    super::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
    let base = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(screen),
        events,
        ..Default::default()
    };
    let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };

    let mut rects = Vec::new();
    run(&ctx, base(Vec::new()), &mut rects);
    let pos = rects[0].0.center();
    // Hover.
    run(&ctx, base(vec![egui::Event::PointerMoved(pos)]), &mut rects);
    // Press: the active state.
    run(
        &ctx,
        base(vec![egui::Event::PointerMoved(pos), press(pos, true)]),
        &mut rects,
    );
    // Release: the click frame is active too.
    run(&ctx, base(vec![press(pos, false)]), &mut rects);

    assert_eq!(rects.len(), 4, "four frames captured");
    for (i, pair) in rects.iter().enumerate() {
        assert_eq!(
            *pair, rects[0],
            "frame {i}: the button or its neighbor moved"
        );
    }
}

/// Disabled tonal buttons must use the muted full-opacity label color,
/// not the enabled on-container color faded by egui's `disabled_alpha`
/// (dark-on-dark, nearly invisible).
#[test]
fn disabled_buttons_keep_a_readable_text_color() {
    let ctx = egui::Context::default();
    // The tonal label is typeset in the bundled bold face: a bare default
    // context has that family unbound and epaint panics on lookup.
    crate::ui::theme::install_test_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let resp = tonal_button_sized(ui, "Retry", egui::vec2(60.0, 32.0), false);
            assert!(!resp.clicked(), "disabled button must not be clickable");
        });
    });
    let s = dark();
    let mut seen = 0;
    for clipped in &out.shapes {
        if let egui::epaint::Shape::Text(t) = &clipped.shape {
            if t.galley.text() == "Retry" {
                seen += 1;
                let color = t
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|sec| sec.format.color)
                    .expect("text section");
                assert_eq!(color, s.on_surface_variant, "disabled text color");
                assert_ne!(color, s.on_secondary_container);
            }
        }
    }
    out.textures_delta.clear();
    assert_eq!(seen, 1, "the disabled label rendered");
}

/// Fixed-size flat buttons ([`tonal_button_sized`] / [`primary_button_sized`])
/// center their label in the frame. `Button::min_size` alone aligned the
/// label to the PARENT's cross axis (centered under a centered/right-to-left
/// parent, left-parked in the error view's left-aligned column), so the
/// shared body uses `add_sized` (a `centered_and_justified` child).
#[test]
fn sized_buttons_center_their_label() {
    let ctx = egui::Context::default();
    super::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 200.0),
        )),
        ..Default::default()
    };
    let mut rects = Vec::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            // Plain top-down column: the layout that exposed the left-parked
            // label (no centering to inherit from the parent).
            rects.push(super::primary_button_sized(ui, "Open", egui::vec2(240.0, 44.0)).rect);
            rects.push(super::tonal_button_sized(ui, "Retry", egui::vec2(160.0, 36.0), true).rect);
        });
    });
    out.textures_delta.clear();
    let labels: Vec<(String, egui::Pos2, egui::Vec2)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => {
                Some((t.galley.text().to_string(), t.pos, t.galley.size()))
            }
            _ => None,
        })
        .collect();
    for (label, rect) in [("Open", rects[0]), ("Retry", rects[1])] {
        let (_, pos, size) = labels
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("label {label:?} painted"));
        let center = *pos + *size / 2.0;
        assert!(
            (center.x - rect.center().x).abs() <= 0.5,
            "{label:?}: label x {} vs frame center {}",
            center.x,
            rect.center().x
        );
        assert!(
            (center.y - rect.center().y).abs() <= 0.5,
            "{label:?}: label y {} vs frame center {}",
            center.y,
            rect.center().y
        );
    }
}

/// The accent-filled button (the error view's "open in your browser" escape
/// hatch) keeps its white label at WCAG AA in both themes and EVERY pointer
/// state: the raw dark accent is a light blue (white on it measured ~3.1:1),
/// so dark mode fills with a darker shade and LIGHTENS through hover/press.
/// The test reads the ACTUAL painted fill under the label per state instead
/// of re-deriving it, so a palette or shade regression fails here.
#[test]
fn primary_button_label_stays_readable_in_both_themes() {
    /// `(label color, the smallest opaque fill under the label)` of one frame.
    fn ink_and_fill(out: &egui::FullOutput) -> (egui::Color32, egui::Color32) {
        let (pos, size, label_color) = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "Open" => Some((
                    t.pos,
                    t.galley.size(),
                    t.galley
                        .job
                        .sections
                        .first()
                        .map(|s| s.format.color)
                        .expect("text section"),
                )),
                _ => None,
            })
            .expect("the label painted");
        let center = pos + size / 2.0;
        let fill = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Rect(r) if r.rect.contains(center) && r.fill.a() == 255 => {
                    Some((r.rect.area(), r.fill))
                }
                _ => None,
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, fill)| fill)
            .expect("the button fill painted");
        (label_color, fill)
    }

    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let ctx = egui::Context::default();
        super::apply(&ctx);
        ctx.set_theme(theme);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = super::primary_button_sized(ui, "Open", egui::vec2(240.0, 44.0));
                });
            },
        );
        out.textures_delta.clear();
        let (label_color, idle_fill) = ink_and_fill(&out);
        assert_eq!(label_color, egui::Color32::WHITE, "{theme:?}: white label");
        // The pointer then targets the button: hover, press and release.
        let pos = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "Open" => {
                    Some(t.pos + t.galley.size() / 2.0)
                }
                _ => None,
            })
            .expect("the label painted");
        let press = |pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut fills = vec![idle_fill];
        for events in [
            vec![egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerMoved(pos), press(true)],
            vec![press(false)],
        ] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let _ = super::primary_button_sized(ui, "Open", egui::vec2(240.0, 44.0));
                    });
                },
            );
            out.textures_delta.clear();
            fills.push(ink_and_fill(&out).1);
        }
        for (state, fill) in ["idle", "hover", "pressed", "released"].iter().zip(fills) {
            let ratio = contrast(label_color, fill);
            assert!(
                ratio >= 4.5,
                "{theme:?}/{state}: white label contrast {ratio:.2} below WCAG AA"
            );
        }
    }
}
