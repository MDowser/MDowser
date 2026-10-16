// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::overlays::find`: floating in-page find bar.
//!
//! - Overlay only (never shifts the document): a popup `Area` anchored to
//!   the reading sheet's top-right, built from the app's own chrome kit so
//!   it reads as one piece with the toolbar: an omnibox-style capsule (soft
//!   chrome fill, magnifier, frameless field, focus ring in the accent),
//!   a muted `n/m` count and circular icon buttons (`ui::icons`) for
//!   previous/next/close, with the same hover fill, tooltips and lazy
//!   accessible names as every other chrome control.
//! - `Enter` steps forward (`Shift+Enter` back), `Esc` closes; the buttons
//!   do the same for pointer users. The query is capped at
//!   `MAX_FIND_QUERY_CHARS` and the count comes from the last rendered
//!   frame (at most one frame stale while typing).

use crate::browser::tab::Tab;
use crate::ui::{icons, theme};
use crate::viewer::find::truncate_query;
use eframe::egui;

/// Capsule height: the toolbar's control rhythm (30-34px chrome rows).
const BAR_H: f32 = 34.0;
/// Capsule side padding before/after the icon and the field.
const CAPSULE_PAD: f32 = 6.0;
/// Magnifier slot inside the capsule.
const ICON: f32 = 22.0;
/// Width of the query field (one line of text, like every other cap).
const FIELD_W: f32 = 220.0;
/// Fixed width of the count column (`12/34`, `120/340+`): a fixed slot keeps
/// the buttons from shifting as the numbers change while typing.
const COUNT_W: f32 = 72.0;

/// Shows the find bar when `tab.find.open` (Loaded/Licenses only; the caller
/// gates the state). Mutates the query/current/scroll flags; the document
/// render below highlights and scrolls from them the same frame.
pub(crate) fn show_find_bar(root: &mut egui::Ui, tab: &mut Tab) {
    if !tab.find.open {
        return;
    }
    let ctx = root.ctx().clone();
    // Esc closes the bar unless ANOTHER surface owns the press this frame (a
    // focused field, the page-info card, a popup/menu or a floating window);
    // `app::shortcuts` records that in `tab.find.esc_elsewhere` before the
    // viewer draws, so one press never closes the bar AND reverts the
    // omnibox (or the inline tile field) behind it. The bar reads the key
    // itself so direct rendering (tests, any future surface) keeps the
    // close behavior.
    if !tab.find.esc_elsewhere && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.find.close();
        return;
    }
    let tab_id = tab.id;
    let edit_id = egui::Id::new(("find-query", tab_id));
    let mut step: Option<i8> = None;
    let mut close = false;
    egui::Area::new(egui::Id::new(("find-bar", tab_id)))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 48.0))
        .order(egui::Order::Foreground)
        .show(&ctx, |ui| {
            // An `Area`'s FIRST frame runs as an invisible sizing pass with a
            // DISABLED Ui: a focus request made there is swallowed (the field
            // cannot take it), so the flag stays set and the request is
            // retried on the next, real frame.
            if tab.find.request_focus && ui.is_enabled() {
                tab.find.request_focus = false;
                ui.memory_mut(|m| m.request_focus(edit_id));
                // Reopening with a kept query selects it all (like the
                // omnibox on `Ctrl/Cmd+L`), so typing replaces it instead of
                // appending.
                crate::ui::select_all_in(&ctx, edit_id, &tab.find.query);
            }
            // Same elevation recipe as every floating surface (`Frame::popup`
            // via `theme::apply_visuals`), with a LIGHTER shadow than the
            // default popup: the bar is a small, always-on-top chrome strip
            // and the menu-weight penumbra read as too heavy over the page.
            // Dark mode stays flat like every other surface.
            egui::Frame::popup(ui.style())
                .corner_radius(egui::CornerRadius::same(theme::radius::TILE))
                .shadow(theme::soft_shadow(ui, [0, 1], 8, 14))
                .inner_margin(egui::Margin::same(theme::spacing::S as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme::spacing::XS;
                        // Capsule: painted behind (fresh focus state read
                        // after the field, like the omnibox).
                        let can_step = !tab.find.query.is_empty() && tab.find.total > 0;
                        let s = theme::scheme_ref(ui);
                        let cap_w = CAPSULE_PAD + ICON + 2.0 + FIELD_W + CAPSULE_PAD;
                        let (cap_rect, cap_resp) =
                            ui.allocate_exact_size(egui::vec2(cap_w, BAR_H), egui::Sense::click());
                        let bg_idx = ui.painter().add(egui::Shape::Noop);
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(
                                cap_rect.min.x + CAPSULE_PAD + ICON * 0.5,
                                cap_rect.center().y,
                            ),
                            egui::vec2(ICON, ICON),
                        );
                        icons::paint(
                            ui,
                            icon_rect.shrink(2.0),
                            icons::Icon::Search,
                            s.on_surface_variant,
                        );
                        let field_left = icon_rect.max.x + 2.0;
                        let field_rect = egui::Rect::from_min_max(
                            egui::pos2(field_left, cap_rect.min.y + 3.0),
                            egui::pos2(cap_rect.max.x - CAPSULE_PAD, cap_rect.max.y - 3.0),
                        );
                        let field = ui
                            .scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(field_rect)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut tab.find.query)
                                            .id(edit_id)
                                            // Frameless: the capsule is the
                                            // frame (same contract as the
                                            // omnibox and the history search).
                                            .frame(egui::Frame::new())
                                            .margin(egui::Margin::ZERO)
                                            .desired_width(field_rect.width().max(16.0))
                                            .char_limit(crate::config::MAX_FIND_QUERY_CHARS)
                                            .text_color(s.on_surface)
                                            .vertical_align(egui::Align::Center)
                                            .hint_text(
                                                egui::RichText::new("Find in page")
                                                    .color(s.on_surface_variant),
                                            ),
                                    )
                                },
                            )
                            .inner;
                        field.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::TextEdit,
                                true,
                                "Find in page",
                            )
                        });
                        if field.changed() {
                            tab.find.query = truncate_query(&tab.find.query);
                            tab.find.current = 0;
                            tab.find.scroll_to_current = true;
                        }
                        // Clicking the capsule padding (or the decorative
                        // magnifier) focuses the field.
                        if cap_resp.clicked() {
                            ui.memory_mut(|m| m.request_focus(edit_id));
                        }
                        // Fresh-state capsule fill + ring (constant 1px
                        // stroke, never a resizing 2px one: the shared
                        // invariant, the history search does the same).
                        let focused = field.has_focus() || ui.memory(|m| m.has_focus(edit_id));
                        let hovered = cap_resp.hovered() || field.hovered();
                        let fill = if focused {
                            s.input_bg
                        } else if hovered {
                            s.omnibox_hover
                        } else {
                            s.omnibox_bg
                        };
                        ui.painter().set(
                            bg_idx,
                            egui::Shape::Rect(egui::epaint::RectShape::new(
                                cap_rect,
                                theme::pill_radius(BAR_H),
                                fill,
                                egui::Stroke::new(
                                    theme::stroke::HAIRLINE,
                                    if focused {
                                        s.primary
                                    } else {
                                        s.outline_variant
                                    },
                                ),
                                egui::StrokeKind::Inside,
                            )),
                        );
                        // Count: muted small text in a fixed slot, so the
                        // buttons never move while the digits change. The
                        // accessible name spells the same fact out ("Match 3
                        // of 12"; a bare `3/12` reads as text); it is built
                        // INSIDE the accesskit closure, so a settled frame
                        // allocates one String for the label and nothing else.
                        let (count_rect, count_resp) = ui
                            .allocate_exact_size(egui::vec2(COUNT_W, BAR_H), egui::Sense::hover());
                        if !tab.find.query.is_empty() {
                            let color = if tab.find.total == 0 {
                                s.on_surface_variant
                            } else {
                                s.on_surface
                            };
                            let (current, total, capped) =
                                (tab.find.current, tab.find.total, tab.find.capped);
                            let text = tab.find.count_text(current, total, capped);
                            ui.painter().text(
                                count_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                text,
                                egui::FontId::proportional(theme::typography::UI_SMALL),
                                color,
                            );
                        }
                        count_resp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Label,
                                true,
                                count_a11y(
                                    &tab.find.query,
                                    tab.find.current,
                                    tab.find.total,
                                    tab.find.capped,
                                ),
                            )
                        });
                        // Prev / next: the shared circular icon buttons (same
                        // hover fill, glyph animation, focus ring and lazy
                        // tooltips as the toolbar).
                        if icons::icon_button(
                            ui,
                            icons::Icon::ChevronUp,
                            "Previous match (Shift+Enter)",
                            can_step,
                        )
                        .clicked()
                        {
                            step = Some(-1);
                        }
                        if icons::icon_button(
                            ui,
                            icons::Icon::ChevronDown,
                            "Next match (Enter)",
                            can_step,
                        )
                        .clicked()
                        {
                            step = Some(1);
                        }
                        // Enter steps matches (Shift+Enter back). A singleline
                        // `TextEdit` SURRENDERS focus when Enter is pressed, so
                        // by this point `has_focus` is already false: the
                        // documented `lost_focus() && Enter` pair is the
                        // reliable read. The keyboard is handed straight back
                        // so the next Enter steps again, like a browser's find
                        // box (the focus must not walk out of the field on the
                        // first press).
                        if field.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if can_step {
                                step = Some(if ctx.input(|i| i.modifiers.shift) {
                                    -1
                                } else {
                                    1
                                });
                            }
                            ui.memory_mut(|m| m.request_focus(edit_id));
                        }
                        if icons::icon_button(ui, icons::Icon::Close, "Close (Esc)", true).clicked()
                        {
                            close = true;
                        }
                    });
                });
        });
    if close {
        tab.find.close();
        return;
    }
    if let Some(dir) = step {
        let total = tab.find.total;
        if total > 0 {
            tab.find.current = if dir > 0 {
                (tab.find.current + 1) % total
            } else {
                (tab.find.current + total - 1) % total
            };
            tab.find.scroll_to_current = true;
        }
    }
}

/// Accessible name of the count slot: the visible `3/12` reads as raw text
/// to a screen reader, so the label states the fact in words. Built only
/// when accesskit asks (inside `widget_info`'s closure).
fn count_a11y(query: &str, current: usize, total: usize, capped: bool) -> String {
    if query.is_empty() {
        "No search".to_string()
    } else if total == 0 {
        "No matches".to_string()
    } else if capped {
        format!("Match {} of {} or more", current + 1, total)
    } else {
        format!("Match {} of {}", current + 1, total)
    }
}
