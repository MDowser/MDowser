// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::show;
use eframe::egui;

/// The drag target covers the whole window (the scrim) and explains the
/// action. It is painted on the Foreground layer, so it also sits above the
/// toolbar, the tab rail and Preferences.
#[test]
fn drop_overlay_paints_a_full_window_target() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let input = egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| show(root.ctx()));
    out.textures_delta.clear();

    assert!(
        out.shapes.iter().any(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect) => rect.rect == screen,
            _ => false,
        }),
        "the scrim must cover the whole window"
    );
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|text| text == "Drop to open"),
        "the target names the action: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("new tabs")),
        "the target explains where the documents go: {texts:?}"
    );
}
