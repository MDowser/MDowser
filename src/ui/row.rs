// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::row`: one shared recipe for custom-painted full-width rows.
//!
//! - [`painted_row_at`] paints every chrome row that needs an explicit
//!   widget id (context menu, Preferences dropdowns, history rows,
//!   suggestion rows, tab rows), so keyboard focus and exact geometry on
//!   hover stay identical across surfaces.
//! - Centralizing the allocate + hover + paint trims duplication and keeps
//!   the rows visually identical.

use eframe::egui;

/// Corner radius shared by every hand-painted row (context menus and the
/// Preferences dropdown rows used to differ: 6 vs 8).
/// Single source: [`crate::ui::theme::radius::MENU`].
pub(crate) const ROW_RADIUS: u8 = crate::ui::theme::radius::MENU;

/// Options for [`painted_row_at`]: keeps the helper under
/// the `too_many_arguments` budget while the row call sites share it.
pub struct RowSpec<'a> {
    /// Row height.
    pub height: f32,
    /// Left label.
    pub label: &'a str,
    /// Optional right-side hint.
    pub suffix: Option<&'a str>,
    /// Clickable or hover-only.
    pub enabled: bool,
    /// Hover background.
    pub hover_fill: egui::Color32,
    /// Label color.
    pub fg: egui::Color32,
    /// Right-hint color (defaults to `fg` when not set by caller).
    pub suffix_fg: Option<egui::Color32>,
    /// Horizontal text inset.
    pub pad_x: f32,
}

/// Paints one full-width row with an explicit widget id: keyboard-navigated
/// lists (the context menu) move focus between rows by id, which the auto id
/// cannot do.
///
/// Text is painted (not a `Button` widget) so it never joins a document
/// text selection and rows keep exact geometry on hover.
pub fn painted_row_at(ui: &mut egui::Ui, id: egui::Id, spec: &RowSpec<'_>) -> egui::Response {
    let width = ui.available_width().max(120.0);
    painted_row_impl(ui, id, spec, width)
}

/// Shared allocate + hover/press/focus paint for the row entry point.
fn painted_row_impl(
    ui: &mut egui::Ui,
    id: egui::Id,
    spec: &RowSpec<'_>,
    width: f32,
) -> egui::Response {
    let sense = if spec.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    // Explicit id (keyboard-navigated lists): register the interaction with
    // THAT id instead of the auto id `allocate_exact_size` made. The
    // allocation already moved the layout cursor.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, spec.height), sense);
    let resp = ui.interact(rect, id, sense);
    // Pressed state is one step past hover (lighten in dark mode, darken in
    // light): a press-and-hold used to look identical to hover.
    let pressed = spec.enabled && resp.is_pointer_button_down_on();
    if pressed {
        let factor = if ui.visuals().dark_mode { 1.35 } else { 0.9 };
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(ROW_RADIUS),
            crate::ui::theme::shade(spec.hover_fill, factor),
        );
    } else if spec.enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(ROW_RADIUS), spec.hover_fill);
    }
    if spec.enabled && resp.has_focus() {
        // Keyboard focus ring paints IN ADDITION to the hover wash (not
        // instead of it): hiding the ring while hovered lost keyboard
        // position as soon as the pointer rested on the row.
        crate::ui::theme::focus_ring(ui, rect, ROW_RADIUS);
    }
    ui.painter().text(
        egui::pos2(rect.min.x + spec.pad_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        spec.label,
        egui::FontId::proportional(crate::ui::theme::typography::DROPDOWN),
        spec.fg,
    );
    if let Some(suffix) = spec.suffix {
        ui.painter().text(
            egui::pos2(rect.max.x - spec.pad_x, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            suffix,
            egui::FontId::proportional(crate::ui::theme::typography::UI_COMPACT),
            spec.suffix_fg.unwrap_or(spec.fg),
        );
    }
    resp
}
