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
//! knob, the tab bar paints each tab into a fixed slot, and each destructive
//! action's confirmation note only takes space while armed (fit content).

use super::*;

/// Dialog header: settings icon + title on the left, close X on
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
                        theme::strong(
                            egui::RichText::new("Preferences")
                                .size(theme::typography::DIALOG_TITLE),
                        )
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
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::stroke::HAIRLINE),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
}

/// A grouped surface (an outlined card): subtle fill, hairline
/// border and rounded corners, holding one section.
/// Shares the fill/stroke recipe with [`theme::card_with`]; the dialog uses
/// the SHEET radius (12px, extra-large) while document cards stay
/// at CARD, so the radius is overridden after reuse.
pub(super) fn card(ui: &mut egui::Ui, s: &theme::Scheme, add: impl FnOnce(&mut egui::Ui)) {
    theme::card_with(s.container_low, s.outline_variant, CARD_PAD as i8)
        .corner_radius(egui::CornerRadius::same(theme::radius::SHEET))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Small uppercase section label.
///
/// `text` is expected ALREADY uppercase (the labels are static: a per-frame
/// `to_uppercase()` allocation bought nothing).
pub(super) fn section_title(ui: &mut egui::Ui, s: &theme::Scheme, text: &str) {
    ui.label(
        theme::strong(egui::RichText::new(text).size(theme::typography::EYEBROW)).color(s.primary),
    );
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
        egui::FontId::proportional(theme::typography::UI_LINK),
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
            egui::Stroke::new(theme::stroke::HAIRLINE, s.primary),
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
            ui.label(
                egui::RichText::new(label)
                    .size(theme::typography::ROW)
                    .color(s.on_surface),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                control(ui);
            });
        },
    );
}

/// Hairline separator between rows inside a card.
pub(super) fn divider(ui: &mut egui::Ui, s: &theme::Scheme) {
    ui.add_space(theme::spacing::XXS);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::stroke::HAIRLINE),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
    ui.add_space(theme::spacing::XXS);
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
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::typography::UI_SMALL)
                .color(color),
        )
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

/// Caption under the CACHE card title: global budget usage, or why the
/// cache is off (forget-on-exit / no data directory). Bounded: three small
/// integers, no disk walk.
pub(super) fn cache_caption(ui: &mut egui::Ui, s: &theme::Scheme) {
    let text = match crate::cache::global().map(|c| c.stats()) {
        Some(stats) if stats.enabled => {
            let total = stats
                .doc_bytes
                .saturating_add(stats.image_bytes)
                .saturating_add(stats.favicon_bytes)
                .saturating_add(stats.index_bytes);
            let entries = stats
                .doc_entries
                .saturating_add(stats.image_entries)
                .saturating_add(stats.favicon_entries);
            format!(
                "Encrypted on disk · {} of {} · {} entries",
                human_bytes(total),
                human_bytes(crate::config::MAX_CACHE_BYTES as u64),
                entries
            )
        }
        _ => "Cache off (forget on exit, or no data directory)".to_string(),
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::typography::UI_SMALL)
                .color(s.on_surface_variant),
        )
        .truncate()
        .selectable(false),
    );
}

/// Short human size (`512 B`, `12.3 KiB`, `4.1 MiB`, `1.0 GiB`): one decimal
/// past bytes, never wider than a caption row.
pub(super) fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", value / KIB)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", value / (KIB * KIB))
    } else {
        format!("{:.1} GiB", value / (KIB * KIB * KIB))
    }
}

/// Description of one cache class in the Preferences CACHE card.
pub(super) struct CacheClearSpec {
    /// Row noun (`documents`, `images`, `favicons`).
    pub noun: &'static str,
    /// Counted unit (`pages`, `files`, `icons`).
    pub unit: &'static str,
    /// Bytes currently used by the class.
    pub size: fn(&crate::cache::CacheStats) -> u64,
    /// Entry count for the class.
    pub count: fn(&crate::cache::CacheStats) -> usize,
    /// Matching `CacheManager` clear method.
    pub clear: fn(&crate::cache::CacheManager),
}

/// One per-class cache clear row: fixed geometry, live size, two-click
/// confirmation. Returning `armed` lets the caller request repaints while
/// any row counts down.
pub(super) fn cache_clear_row(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    stats: Option<&crate::cache::CacheStats>,
    confirm_until: &mut Option<Instant>,
    spec: &CacheClearSpec,
) -> bool {
    let now = Instant::now();
    let confirming = confirm_until.is_some_and(|until| until > now);
    let label = if confirming {
        format!("Confirm clear {}", spec.noun)
    } else {
        match stats {
            Some(st) if st.enabled => format!(
                "Clear {} ({} · {} {})",
                spec.noun,
                human_bytes((spec.size)(st)),
                (spec.count)(st),
                spec.unit
            ),
            _ => format!("Clear {} (cache off)", spec.noun),
        }
    };
    if danger_row(ui, s, &label, icons::Icon::Delete).clicked() {
        if confirming {
            if let Some(cache) = crate::cache::global() {
                (spec.clear)(cache);
            }
            *confirm_until = None;
        } else {
            *confirm_until = Some(crate::config::deadline_after(CLEAR_CONFIRM_SECS));
        }
    }
    let armed = confirm_until.is_some_and(|until| until > Instant::now());
    confirm_note(
        ui,
        armed,
        &format!("Click again to delete cached {}.", spec.noun),
    );
    armed
}

/// `N pages stored`, singular-aware. Delegates the singular/plural rule to
/// the history page's memoized caption (single source; the `" stored"`
/// suffix stays here, where the persistence meaning lives).
pub(super) fn count_label(count: usize) -> String {
    crate::ui::history_page::with_count_label(count, |base| format!("{base} stored"))
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
        egui::FontId::proportional(theme::typography::ROW),
        s.error,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Toggle switch. Returns the response; `changed()` is set on click.
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
    // Hover feedback on the track, like every other hand-painted control:
    // one step up the neutral ramp while off (the on track is already the
    // accent, so the mix below leaves it at `primary` as before).
    let base = if response.hovered() {
        s.container_hover
    } else {
        s.container_high
    };
    let fill = theme::mix(base, s.primary, t);
    let stroke = if t < 0.5 {
        egui::Stroke::new(theme::stroke::HAIRLINE, s.outline_variant)
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
            egui::Stroke::new(theme::stroke::HAIRLINE, s.primary),
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

/// Tab bar height (fixed, so switching tabs never moves the content below).
const TAB_H: f32 = 36.0;
/// One shortcut reference row (action + keys, fixed height).
const SHORTCUT_ROW_H: f32 = 28.0;

/// Preferences tab bar: three equal tabs with stable geometry.
///
/// The selected tab carries an accent underline + accent label; hover paints
/// the neutral wash behind a label whose position never changes. Returns the
/// newly selected tab, if any.
pub(super) fn tab_bar(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    current: PreferencesTab,
) -> Option<PreferencesTab> {
    let mut chosen = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD as i8,
            right: PAD as i8,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            let total = ui.available_width();
            let gap = theme::spacing::XS;
            // Equal tabs across the row (derived from the tab list, so a new
            // tab cannot silently overflow the bar).
            let count = PreferencesTab::ALL.len() as f32;
            let w = ((total - gap * (count - 1.0)) / count).max(80.0);
            ui.allocate_ui_with_layout(
                egui::vec2(total, TAB_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for tab in PreferencesTab::ALL {
                        let selected = tab == current;
                        if tab_button(ui, s, tab.label(), selected, w).clicked() && !selected {
                            chosen = Some(tab);
                        }
                    }
                },
            );
        });
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::stroke::HAIRLINE),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
    chosen
}

/// One tab button: fixed-size, centered label, accent underline when selected.
fn tab_button(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    label: &str,
    selected: bool,
    w: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, TAB_H), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() && !selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::MENU),
            s.container_hover,
        );
    } else if response.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::MENU);
    }
    if selected {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 12.0, rect.max.y - 3.0),
            egui::vec2(rect.width() - 24.0, 2.0),
        );
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius::same(theme::radius::HAIRLINE),
            s.primary,
        );
    }
    let color = if selected {
        s.primary
    } else {
        s.on_surface_variant
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(
            theme::typography::BUTTON,
            if selected {
                theme::strong_family()
            } else {
                egui::FontFamily::Proportional
            },
        ),
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    response
}

/// One shortcut reference row: action on the left, keys on the right.
///
/// The keys column is right-aligned by the inner right-to-left layout: egui
/// anchors that label's galley at the RIGHT edge of its rect (`LayoutJob`
/// `halign` follows the placing layout), so the shape's `pos` sits at the
/// column's right edge while the ink extends left of it (the geometry test
/// measures ink through `galley.rect` for exactly this reason).
pub(super) fn shortcut_row(ui: &mut egui::Ui, s: &theme::Scheme, action: &str, keys: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), SHORTCUT_ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(action)
                    .size(theme::typography::BUTTON)
                    .color(s.on_surface),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(keys)
                        .monospace()
                        .size(theme::typography::UI_SMALL)
                        .color(s.on_surface_variant),
                );
            });
        },
    );
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
    let stroke = egui::Stroke::new(theme::stroke::ICON, s.on_surface_variant);
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
            s.container_hover,
        );
    } else if response.has_focus() {
        theme::focus_ring(ui, rect, crate::ui::row::ROW_RADIUS);
    }
    let color = if selected { s.primary } else { s.on_surface };
    ui.painter().text(
        egui::pos2(rect.min.x + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme::typography::DROPDOWN),
        color,
    );
    if selected {
        let c = egui::pos2(rect.max.x - 14.0, rect.center().y);
        let stroke = egui::Stroke::new(theme::stroke::ICON, s.primary);
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
/// `name` is the accessible name: the field exposes only the current value,
/// so without it a screen reader announces "Keep 3 months (default), combo
/// box" with no idea what it sets.
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
    // very narrow window) the 168px field overran the dialog's right edge
    // and clipped the control.
    let width = COMBO_W.min((ui.available_width() - 12.0).max(96.0));
    let (response, popup_id) = dropdown_field(ui, s, id_salt, name, selected_text, width);
    egui::Popup::menu(&response)
        .id(popup_id)
        .width(width)
        // The menu closes on the first click (row selection included), like
        // every other popup in the app.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (value, label) in options {
                if dropdown_row(ui, s, *value == current, label) {
                    chosen = Some(*value);
                }
            }
        });
    chosen
}

/// The dropdown's closed state: a fixed-height field with the selected value
/// on the left and a chevron on the right, painted into an exact rect like
/// every other control in this dialog.
///
/// Deliberately NOT egui's `ComboBox`: its button is a `Frame`, and a frame
/// inside a right-to-left row is laid out from the parent cursor's
/// `next_widget_position` (for a right-to-left layout that seed sits at the
/// row's right-CENTER), which parked the box ~7px below the row's middle
/// while the zoom pill, the switch and the tab buttons — exact rects — stayed
/// centered. Returns the button response and the id of its menu popup.
fn dropdown_field(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    id_salt: &str,
    name: &str,
    selected_text: &str,
    width: f32,
) -> (egui::Response, egui::Id) {
    // The shared button metrics: padding + one line of the button text, so
    // the field matches the theme's buttons instead of a hardcoded height.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let height =
        2.0 * ui.spacing().button_padding.y + ui.fonts_mut(|fonts| fonts.row_height(&font));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let button_id = ui.make_persistent_id(("prefs-dropdown", id_salt));
    let popup_id = button_id.with("menu");
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let response = ui
        .interact(rect, button_id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // Enter/`Space` on the focused field opens the menu like a click would.
    if !open
        && response.has_focus()
        && ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
        })
    {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    // Surface: the weak-button recipe (resting container, hover wash, the
    // open state on top) with the shared hairline.
    let (fill, stroke) = if open {
        (s.container_high, s.on_surface_variant)
    } else if response.hovered() {
        (s.container_hover, s.outline_variant)
    } else {
        (s.container_low, s.outline_variant)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(theme::radius::CARD),
        fill,
        egui::Stroke::new(theme::stroke::HAIRLINE, stroke),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::CARD);
    }
    // Selected value: elided to its own lane, so an overlong value can never
    // run under the chevron (the shared tile/table recipe).
    let pad = ui.spacing().button_padding.x;
    let icon_slot = egui::vec2(16.0, 16.0);
    let lane_left = rect.min.x + pad;
    let lane_w = (rect.max.x - pad - icon_slot.x - lane_left).max(1.0);
    let mut job =
        egui::text::LayoutJob::simple(selected_text.to_owned(), font.clone(), s.on_surface, lane_w);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(
        egui::pos2(lane_left, rect.center().y - galley.size().y * 0.5),
        galley.clone(),
        s.on_surface,
    );
    // An elided value keeps its full text in a hover tooltip (the app's rule
    // for every truncated label).
    let response = if galley.elided {
        crate::ui::hover_tooltip(response, selected_text)
    } else {
        response
    };
    // Chevron: the shared vector icon kit, muted like the trailing glyphs of
    // the other fields.
    icons::paint(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.max.x - pad - icon_slot.x * 0.5, rect.center().y),
            icon_slot,
        ),
        icons::Icon::ChevronDown,
        s.on_surface_variant,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ComboBox,
            true,
            format!("{name}: {selected_text}"),
        )
    });
    (response, popup_id)
}
