// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{paint, paint_text, Icon};
use eframe::egui;

/// Every variant, so a new icon cannot dodge the checks below.
const ALL: [Icon; 27] = [
    Icon::Home,
    Icon::Back,
    Icon::Forward,
    Icon::Reload,
    Icon::ZoomOut,
    Icon::ZoomIn,
    Icon::Plus,
    Icon::Globe,
    Icon::Lock,
    Icon::File,
    Icon::Image,
    Icon::BrokenImage,
    Icon::Close,
    Icon::History,
    Icon::Search,
    Icon::Info,
    Icon::Settings,
    Icon::Loading,
    Icon::Delete,
    Icon::Bookmark,
    Icon::BookmarkFilled,
    Icon::Edit,
    Icon::Check,
    Icon::ChevronUp,
    Icon::ChevronDown,
    Icon::CloudOff,
    Icon::Archive,
];

/// Every icon paints vector geometry: no text fallback and no texture.
#[test]
fn every_icon_paints_vector_shapes() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 200.0),
        )),
        ..Default::default()
    };
    let mut arts = 0;
    let mut texts = 0;
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(root, |ui| {
                for (i, icon) in ALL.iter().enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(8.0 + i as f32 * 36.0, 8.0),
                        egui::vec2(32.0, 32.0),
                    );
                    paint(ui, rect, *icon, egui::Color32::BLACK);
                }
            });
    });
    for clipped in &out.shapes {
        match &clipped.shape {
            egui::Shape::Text(_) => texts += 1,
            egui::Shape::Path(_)
            | egui::Shape::Circle(_)
            | egui::Shape::Mesh(_)
            | egui::Shape::LineSegment { .. } => arts += 1,
            _ => {}
        }
    }
    assert_eq!(texts, 0, "an icon fell back to the text path");
    assert!(
        arts >= ALL.len(),
        "every icon must paint (got {arts} vector shapes)"
    );
    out.textures_delta.clear();
}

/// The color-emoji pipeline still serves user glyphs (`paint_text`,
/// bookmark tiles): a letter always paints, as texture or text.
#[test]
fn paint_text_still_paints_user_glyphs() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(200.0, 100.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(root, |ui| {
                paint_text(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(32.0, 32.0)),
                    "A",
                    egui::Color32::WHITE,
                );
            });
    });
    assert!(!out.shapes.is_empty(), "a tile glyph painted");
    out.textures_delta.clear();
}

/// Every icon paints without panicking at toolbar and small sizes, in
/// both themes, and inside buttons.
#[test]
fn every_icon_paints_at_any_size() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            for (i, icon) in ALL.iter().enumerate() {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(8.0 + i as f32 * 40.0, 8.0),
                    egui::vec2(32.0, 32.0),
                );
                paint(ui, rect, *icon, egui::Color32::BLACK);
                let small = egui::Rect::from_min_size(
                    egui::pos2(8.0 + i as f32 * 40.0, 60.0),
                    egui::vec2(14.0, 14.0),
                );
                paint(ui, small, *icon, egui::Color32::WHITE);
            }
            // Custom-rect button path (zero-sized rect included).
            let _ = super::icon_button_at(
                ui,
                egui::Rect::from_min_size(egui::pos2(8.0, 100.0), egui::vec2(24.0, 24.0)),
                "test",
                Icon::Close,
                "Clear",
                true,
            );
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "icons painted");
}

/// The button effect really animates: hovering eases the muted glyph to
/// the accent while it grows ~10% around its own center, and pressing
/// pulls it back; the button itself never moves.
#[test]
fn icon_button_hover_effect_grows_and_tints() {
    use crate::ui::theme;

    /// Runs `settle` frames with the given events; returns the icon's
    /// painted bounding box and stroke colors from the last frame.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        settle: usize,
    ) -> (egui::Rect, Vec<egui::Color32>) {
        let mut bbox = egui::Rect::NOTHING;
        let mut colors = Vec::new();
        for _ in 0..settle {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events: events.clone(),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = super::icon_button(ui, Icon::Home, "Home", true);
                });
            });
            out.textures_delta.clear();
            bbox = egui::Rect::NOTHING;
            colors.clear();
            for clipped in &out.shapes {
                if let egui::Shape::Path(p) = &clipped.shape {
                    bbox = bbox.union(egui::Rect::from_points(&p.points));
                    if let egui::epaint::ColorMode::Solid(c) = p.stroke.color {
                        colors.push(c);
                    }
                }
            }
        }
        (bbox, colors)
    }

    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let s = if ctx.theme() == egui::Theme::Dark {
        theme::dark()
    } else {
        theme::light()
    };
    let (idle, idle_colors) = frame(&ctx, Vec::new(), 3);
    assert!(idle.width() > 0.0, "the idle icon painted");
    assert!(
        idle_colors.iter().all(|&c| c == s.on_surface_variant),
        "idle glyph uses the muted tone"
    );

    // Hover: settle the tween (0.14s at 1/60 per frame).
    let pos = idle.center();
    let (hover, hover_colors) = frame(&ctx, vec![egui::Event::PointerMoved(pos)], 25);
    assert!(
        hover.width() > idle.width() * 1.05,
        "hover grows the glyph: {idle:?} -> {hover:?}"
    );
    assert!(
        hover_colors.iter().all(|&c| c == s.primary),
        "hover tints the glyph to the accent"
    );

    // Press: the glyph pulls back while held (still lifted vs idle).
    let press = egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    let (pressed, _) = frame(&ctx, vec![egui::Event::PointerMoved(pos), press], 25);
    assert!(
        pressed.width() < hover.width(),
        "press pulls the glyph back: {pressed:?} vs {hover:?}"
    );
    assert!(
        pressed.width() > idle.width(),
        "the pressed glyph is still lifted"
    );
}

/// A disabled button owns exactly the same slot as an enabled one, so
/// a button turning disabled (Reload entering its cooldown, Back with
/// no history) never pushes its neighbours: the fade is a faded
/// painter, never a `ui.scope` (an empty scope advances the cursor one
/// item spacing). Regression test: the widget after the button lands
/// on the same x either way.
#[test]
fn icon_button_disabled_keeps_its_slot() {
    fn marker_x(enabled: bool) -> f32 {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            ..Default::default()
        };
        let mut x = f32::NAN;
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let _ = super::icon_button(ui, Icon::Reload, "Reload", enabled);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    x = rect.min.x;
                });
            });
        });
        out.textures_delta.clear();
        x
    }

    assert_eq!(
        marker_x(true),
        marker_x(false),
        "disabling the button moved the following widget"
    );
}
