// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::preferences::widgets`: the fixed-geometry widget kit of the
//! Preferences dialog.
//!
//! Every interactive row keeps the SAME geometry in all visual states (the
//! egui default `selectable_label` grew its frame on hover and shifted the
//! list), the dropdowns paint into a fixed rect, the switch animates its
//! knob, and each destructive action's confirmation note only takes space
//! while armed (fit content).

use super::*;

/// Material dialog header: settings icon + title on the left, close X on
/// the right, followed by a full-bleed divider.
pub(super) fn dialog_header(ui: &mut egui::Ui, s: &theme::Scheme, open: &mut bool) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD as i8,
            right: 12,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            // Fixed 32px lane, vertically centered: the close button sets
            // the height, so the icon + title center on it instead of
            // top-aligning (which left extra air under the title).
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    icons::paint(ui, icon_rect, icons::Icon::Settings, s.primary);
                    ui.add_space(10.0);
                    ui.label(
                        theme::strong(egui::RichText::new("Preferences").size(21.0))
                            .color(s.on_surface),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if close_button(ui, s).clicked() {
                            *open = false;
                        }
                    });
                },
            );
        });
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
}

/// A grouped surface (Material "outlined card"): subtle fill, hairline
/// border and rounded corners, holding one section.
/// Shares the fill/stroke recipe with [`theme::card_with`]; the dialog uses
/// the SHEET radius (12px, Material extra-large) while document cards stay
/// at CARD, so the radius is overridden after reuse.
pub(super) fn card(ui: &mut egui::Ui, s: &theme::Scheme, add: impl FnOnce(&mut egui::Ui)) {
    theme::card_with(s.container_low, s.outline_variant, CARD_PAD as i8)
        .corner_radius(egui::CornerRadius::same(theme::radius::SHEET))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Small uppercase section label (Material "label" style).
///
/// `text` is expected ALREADY uppercase (the labels are static: a per-frame
/// `to_uppercase()` allocation bought nothing).
pub(super) fn section_title(ui: &mut egui::Ui, s: &theme::Scheme, text: &str) {
    ui.label(theme::strong(egui::RichText::new(text).size(11.0)).color(s.primary));
}

/// Small text link: content-sized, clickable, with a hover underline. Painted
/// into an explicitly allocated rect (like the dialog's other controls), so
/// its geometry is stable and it cannot drift outside the fixed dialog.
pub(super) fn text_link(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    text: &str,
    tooltip: &str,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(13.0),
        s.primary,
    );
    let size = galley.size();
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let response = crate::ui::hover_tooltip(response, tooltip);
    if response.hovered() || response.has_focus() {
        // Underline on hover AND keyboard focus: every other hand-painted
        // control shows focus, this link used to be reachable but invisible.
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x, rect.max.y),
                egui::pos2(rect.max.x, rect.max.y),
            ],
            egui::Stroke::new(1.0, s.primary),
        );
    }
    ui.painter().galley(rect.min, galley, s.primary);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, text));
    response
}

/// The "Delete history" confirmation note: only takes space while `armed`,
/// so both cards fit their rows when disarmed.
pub(super) fn confirm_note(ui: &mut egui::Ui, armed: bool, text: &str) {
    if armed {
        ui.label(theme::weak(ui, text));
    }
}

/// One setting row: label on the left, control on the right, fixed height.
pub(super) fn setting_row(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    label: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new(label).size(15.0).color(s.on_surface));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                control(ui);
            });
        },
    );
}

/// Hairline separator between rows inside a card.
pub(super) fn divider(ui: &mut egui::Ui, s: &theme::Scheme) {
    ui.add_space(2.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
    ui.add_space(2.0);
}

/// Subtle caption under the retention row: how much is stored (or the last
/// save error). The data directory shows as a hover tooltip instead of a
/// full path line, keeping the card clean. The tooltip content (and its
/// path `String`) is built lazily only while hovered.
pub(super) fn history_caption(ui: &mut egui::Ui, s: &theme::Scheme, store: &HistoryStore) {
    let (text, color) = match store.save_error() {
        Some(error) => (error.to_string(), s.error),
        None => (count_label(store.entries().len()), s.on_surface_variant),
    };
    let response = ui.add(
        egui::Label::new(egui::RichText::new(text).size(12.0).color(color))
            .truncate()
            .selectable(false),
    );
    response.on_hover_ui(|ui| {
        if matches!(store.retention(), Retention::Session) {
            ui.label("Not stored on disk (forget on exit)");
        } else if let Some(dir) = store.storage_dir() {
            ui.label(dir.display().to_string());
        } else {
            ui.label("No data directory: kept in memory for this session");
        }
    });
}

/// `N pages stored`, singular-aware.
pub(super) fn count_label(count: usize) -> String {
    if count == 1 {
        "1 page stored".to_string()
    } else {
        format!("{count} pages stored")
    }
}

/// Full-width destructive action row (trash + label, error-tinted hover).
pub(super) fn danger_row(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    label: &str,
    icon: icons::Icon,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::MENU),
            theme::tint(s.error, 24),
        );
    } else if response.has_focus() {
        theme::focus_ring_stroke(ui, rect, theme::radius::MENU, s.error);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + 14.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    icons::paint(ui, icon_rect, icon, s.error);
    ui.painter().text(
        egui::pos2(rect.min.x + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.0),
        s.error,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Material toggle switch. Returns the response; `changed()` is set on click.
/// `label` is the accessible name: the visible row label is a separate
/// `Label`, so without it a screen reader gets an unnamed checkbox.
pub(super) fn switch(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    label: &str,
    on: &mut bool,
) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(SWITCH_W, SWITCH_H), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let track = egui::Rect::from_min_size(rect.min, egui::vec2(SWITCH_W, SWITCH_H));
    let radius = theme::pill_radius(SWITCH_H);
    let fill = theme::mix(s.container_high, s.primary, t);
    let stroke = if t < 0.5 {
        egui::Stroke::new(1.0, s.outline_variant)
    } else {
        egui::Stroke::NONE
    };
    ui.painter()
        .rect(track, radius, fill, stroke, egui::StrokeKind::Inside);
    if response.has_focus() {
        let ring_radius =
            egui::CornerRadius::same(theme::pill_radius(SWITCH_H).nw.saturating_add(2));
        ui.painter().rect_stroke(
            track.expand(2.0),
            ring_radius,
            egui::Stroke::new(1.0, s.primary),
            egui::StrokeKind::Inside,
        );
    }
    let knob_r = SWITCH_H * 0.5 - 3.0;
    let knob_x = egui::lerp(
        (track.min.x + SWITCH_H * 0.5)..=(track.max.x - SWITCH_H * 0.5),
        t,
    );
    let knob = if t < 0.5 {
        s.on_surface_variant
    } else {
        egui::Color32::WHITE
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, track.center().y), knob_r, knob);
    let checked = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, label)
    });
    response
}

/// Borderless close button: a circled hover fill and a thin X.
pub(super) fn close_button(ui: &mut egui::Ui, s: &theme::Scheme) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::FULL),
            s.omnibox_hover,
        );
    } else if response.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::FULL);
    }
    let c = rect.center();
    let d = 5.0;
    let stroke = egui::Stroke::new(1.8, s.on_surface_variant);
    ui.painter()
        .line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
    ui.painter()
        .line_segment([c + egui::vec2(d, -d), c + egui::vec2(-d, d)], stroke);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    crate::ui::hover_tooltip(response, "Close")
}

/// One fixed-geometry dropdown popup row: hover paints behind a label whose
/// position never changes; the selected row shows a leading accent and a
/// trailing check mark. Returns `true` when clicked.
pub(super) fn dropdown_row(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    selected: bool,
    label: &str,
) -> bool {
    let width = ui.available_width().max(140.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, DROPDOWN_ROW_H), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::row::ROW_RADIUS),
            s.container_high,
        );
    } else if response.has_focus() {
        theme::focus_ring(ui, rect, crate::ui::row::ROW_RADIUS);
    }
    let color = if selected { s.primary } else { s.on_surface };
    ui.painter().text(
        egui::pos2(rect.min.x + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.5),
        color,
    );
    if selected {
        let c = egui::pos2(rect.max.x - 14.0, rect.center().y);
        let stroke = egui::Stroke::new(1.8, s.primary);
        ui.painter().line_segment(
            [c + egui::vec2(-4.5, 0.5), c + egui::vec2(-1.5, 3.5)],
            stroke,
        );
        ui.painter().line_segment(
            [c + egui::vec2(-1.5, 3.5), c + egui::vec2(4.5, -3.5)],
            stroke,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    response.clicked()
}

/// Dropdown built on [`dropdown_row`]: returns the chosen value, if any.
/// `name` is the accessible name: egui's `ComboBox` exposes only the current
/// value, so without it a screen reader announces "Keep 3 months (default),
/// combo box" with no idea what it sets.
pub(super) fn dropdown<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    id_salt: &str,
    name: &str,
    selected_text: &str,
    options: &[(T, &'static str)],
    current: T,
) -> Option<T> {
    let mut chosen = None;
    // Cap the fixed width by the row's remaining space: at high zoom (or a
    // very narrow window) the 168px combo overran the dialog's right edge
    // and clipped the control.
    let width = COMBO_W.min((ui.available_width() - 12.0).max(96.0));
    let combo = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text)
        .width(width)
        .show_ui(ui, |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (value, label) in options {
                if dropdown_row(ui, s, *value == current, label) {
                    chosen = Some(*value);
                }
            }
        });
    combo
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
    chosen
}
