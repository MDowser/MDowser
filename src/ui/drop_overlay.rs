// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::drop_overlay`: the full-window target shown during an OS file drag.
//!
//! It is a layer painter, not an `Area` or a widget: the OS owns the drag
//! gesture, so the overlay must never take pointer input, run a sizing pass
//! (which would hide it for one frame) or reflow the page under it.

use super::theme;
use eframe::egui;

/// Widest card on a large window; narrower viewports keep a 24px margin.
const CARD_MAX_W: f32 = 440.0;
/// Card height: icon, title and one wrapped subtitle.
const CARD_H: f32 = 184.0;
/// Smallest gap between the card and the window edge.
const SCREEN_MARGIN: f32 = 24.0;

/// Paints the drop target over the whole window (Foreground layer).
///
/// Called only while `raw_input.hovered_files` is non-empty, so the settled
/// window never pays for it. The card uses the active theme's surface, text
/// and accent tones; the scrim is darker in dark mode so the card keeps its
/// contrast over an already dark page.
pub(crate) fn show(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    if screen.width() <= 0.0 || screen.height() <= 0.0 {
        return;
    }
    let s = theme::scheme_ref_ctx(ctx);
    let dark = ctx.theme() == egui::Theme::Dark;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop-overlay"),
    ));

    let scrim = if dark {
        egui::Color32::from_black_alpha(120)
    } else {
        egui::Color32::from_black_alpha(36)
    };
    painter.rect_filled(screen, egui::CornerRadius::ZERO, scrim);

    let card_w = CARD_MAX_W.min((screen.width() - 2.0 * SCREEN_MARGIN).max(0.0));
    let card_h = CARD_H.min((screen.height() - 2.0 * SCREEN_MARGIN).max(0.0));
    if card_w <= 0.0 || card_h <= 0.0 {
        return;
    }
    let card = egui::Rect::from_center_size(screen.center(), egui::vec2(card_w, card_h));
    let radius = egui::CornerRadius::same(theme::radius::CARD);
    painter.rect_filled(card, radius, s.surface);
    painter.rect_stroke(
        card,
        radius,
        egui::Stroke::new(2.0, s.primary),
        egui::StrokeKind::Inside,
    );

    // Download-style arrow in a soft accent disc: a file-drop affordance that
    // stays legible at every zoom (vector, no raster asset).
    let icon = egui::pos2(card.center().x, card.top() + 46.0);
    painter.circle_filled(
        icon,
        22.0,
        theme::tint(s.primary, if dark { 48 } else { 22 }),
    );
    let ink = egui::Stroke::new(2.2, s.primary);
    painter.line_segment(
        [icon + egui::vec2(0.0, -9.0), icon + egui::vec2(0.0, 5.0)],
        ink,
    );
    painter.line_segment(
        [icon + egui::vec2(-7.0, -1.0), icon + egui::vec2(0.0, 6.0)],
        ink,
    );
    painter.line_segment(
        [icon + egui::vec2(7.0, -1.0), icon + egui::vec2(0.0, 6.0)],
        ink,
    );
    painter.line_segment(
        [
            icon + egui::vec2(-10.0, 11.0),
            icon + egui::vec2(10.0, 11.0),
        ],
        ink,
    );

    let title = painter.layout_no_wrap(
        "Drop to open".to_owned(),
        egui::FontId::new(22.0, theme::strong_family()),
        s.on_surface,
    );
    painter.galley(
        egui::pos2(card.center().x - title.size().x / 2.0, card.top() + 82.0),
        title,
        s.on_surface,
    );

    let subtitle = painter.layout(
        "Files and folders open in new tabs".to_owned(),
        egui::FontId::proportional(14.0),
        s.on_surface_variant,
        (card.width() - 48.0).max(1.0),
    );
    painter.galley(
        egui::pos2(
            card.center().x - subtitle.size().x / 2.0,
            card.top() + 120.0,
        ),
        subtitle,
        s.on_surface_variant,
    );
}

#[cfg(test)]
mod tests;
