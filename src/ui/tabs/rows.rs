// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::tabs::rows`: one rail row (collapsed favicon or expanded
//! favicon+title+close), the pinned action rows and the edge separator.
//!
//! Geometry is shared by both rail states (same x/y/size), so expansion
//! only grows the background to the right and nothing jumps.

use super::*;

/// Row background shared by both states (same geometry, same colors).
pub(super) fn draw_row_background(
    ui: &egui::Ui,
    rect: egui::Rect,
    selected: bool,
    hovered: bool,
    focused: bool,
    s: &theme::Scheme,
) {
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(ROW_RADIUS), s.surface);
        // Accent bar: unmistakable active tab in icon mode. Rows start at
        // `PAD_X`, so it takes the rail's left gutter: the favicon now
        // fills the row (toolbar-sized) and an in-row bar would be covered.
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.min.x - PAD_X + 2.0, rect.min.y + 8.0),
            egui::vec2(3.0, rect.height() - 16.0),
        );
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius::same(theme::radius::HAIRLINE),
            s.primary,
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(ROW_RADIUS), s.container_high);
    }
    if focused {
        theme::focus_ring_with(ui, rect, ROW_RADIUS, s);
    }
}

/// Draws one pinned bottom action row (`+ New Tab` / `History`), shared by
/// both rail states, and reports whether it is hovered (the collapsed rail
/// keeps the overlay open while the pointer is over a row).
#[allow(clippy::too_many_arguments)]
pub(super) fn action_row(
    ui: &mut egui::Ui,
    row_w: f32,
    s: &theme::Scheme,
    icon: icons::Icon,
    label: Option<&str>,
    label_alpha: f32,
    name: &str,
    tooltip: impl FnOnce() -> String,
    want: &mut bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(row_w, ACTION_ROW_H), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    draw_action_row(ui, rect, &resp, s, icon, label, label_alpha);
    // Accessible name + tooltip: the collapsed rail shows no label. The
    // tooltip string is built on hover frames only.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let hovered = resp.hovered();
    if crate::ui::hover_tooltip_lazy(resp, tooltip).clicked() {
        *want = true;
    }
    hovered
}

/// Paints one action row: icon at the favicon column, optional label at the
/// title column, hover wash across the row. The caller owns the `Response`
/// (click, tooltip, accessibility): the collapsed rail passes no label.
/// `label_alpha` fades the label during the overlay's open animation (the
/// icon stays solid: it is painted with its full tint).
pub(super) fn draw_action_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    resp: &egui::Response,
    s: &theme::Scheme,
    icon: icons::Icon,
    label: Option<&str>,
    label_alpha: f32,
) {
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(ROW_RADIUS), s.container_high);
    } else if resp.has_focus() {
        // Shared focus ring (same token as every other hand-painted row).
        theme::focus_ring_with(ui, rect, ROW_RADIUS, s);
    }
    let color = if resp.hovered() {
        s.primary
    } else {
        s.on_surface
    };
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + ICON_CX, rect.center().y),
        egui::vec2(ICON_BOX, ICON_BOX),
    );
    icons::paint(ui, icon_rect, icon, color);
    if let Some(label) = label {
        // Aligned with the tab titles (same icon column + gap).
        let title_left = rect.min.x + ICON_CX + FAVICON / 2.0 + 8.0;
        let label_rect = egui::Rect::from_min_max(
            egui::pos2(title_left, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        );
        let color = s.on_surface.gamma_multiply(label_alpha);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(label_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::Label::new(theme::strong(egui::RichText::new(label).color(color)))
                        .truncate()
                        .selectable(false),
                );
            },
        );
    }
}

/// Right-edge separation for the expanded overlay.
///
/// The toolbar, rail and overlay share one chrome tone and must read as a
/// single surface, so there is deliberately NO shadow above (the frame
/// shadow's top half painted a gray seam under the toolbar). The right edge
/// floats over the page and needs its own separation:
/// - light: a soft gradient penumbra (26 -> 0 alpha over ~12px);
/// - dark: a crisp 1px hairline (black blur over dark chrome reads as dirt,
///   same rationale as `theme::soft_shadow`).
pub(super) fn paint_edge_separator(ui: &egui::Ui, rect: egui::Rect, s: &theme::Scheme) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let painter = ui.painter();
    if ui.visuals().dark_mode {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.max.x - 1.0, rect.min.y), rect.max),
            egui::CornerRadius::ZERO,
            s.outline_variant,
        );
        return;
    }
    const WIDTH: f32 = 12.0;
    const STEPS: usize = 12;
    let step_w = WIDTH / STEPS as f32;
    for i in 0..STEPS {
        let t = i as f32 / STEPS as f32;
        // Quadratic falloff: darkest at the panel edge, gone at WIDTH.
        let alpha = (26.0 * (1.0 - t) * (1.0 - t)).round() as u8;
        if alpha == 0 {
            continue;
        }
        let x0 = rect.max.x + t * WIDTH;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0, rect.min.y),
                egui::pos2(x0 + step_w, rect.max.y),
            ),
            egui::CornerRadius::ZERO,
            egui::Color32::from_black_alpha(alpha),
        );
    }
}

/// One expanded row: favicon + title + close, click selects. `text_t` fades
/// the title during the overlay's open animation (the favicon itself stays
/// solid).
#[allow(clippy::too_many_arguments)]
pub(super) fn expanded_row(
    ui: &mut egui::Ui,
    index: usize,
    tab: &mut Tab,
    selected: bool,
    row_w: f32,
    text_t: f32,
    favicons: &mut FaviconCache,
    out: &mut TabsResponse,
) {
    let s = theme::scheme_ref(ui);
    let (row_rect, row_resp) =
        ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::click());
    let row_resp = row_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // `contains_pointer` (not `hovered`): the close box is a separate widget
    // on top, so egui's hover set dropped the row the moment the pointer
    // reached the X and the highlight blinked off under the pointer.
    draw_row_background(
        ui,
        row_rect,
        selected,
        row_resp.contains_pointer(),
        row_resp.has_focus(),
        s,
    );
    let (disp, _rebuilt) = tab.display();
    let origin = disp.origin.as_ref();
    draw_favicon(
        ui,
        row_rect,
        disp.loading,
        disp.kind,
        origin,
        favicons,
        selected,
    );
    let title = disp.title.as_str();
    row_resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, title)
    });
    // Title (non-interactive label: the row itself handles selection).
    // The right edge reserves the floating scrollbar's lane: the bar senses
    // the full bar width at the overlay's right edge even while invisible,
    // and would otherwise steal clicks from the outer part of the close box.
    let lane = theme::scrollbar_width(ui);
    let title_left = row_rect.min.x + ICON_CX + FAVICON / 2.0 + 8.0;
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(title_left, row_rect.min.y),
        egui::pos2(row_rect.max.x - CLOSE_W - 6.0 - lane, row_rect.max.y),
    );
    let title_color = if selected {
        s.on_surface
    } else {
        s.on_surface_variant
    };
    let title_color = title_color.gamma_multiply(text_t);
    // Child UI with a left-to-right centered layout: the label is vertically
    // centered and truncates inside the reserved width.
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(theme::strong(egui::RichText::new(title).color(title_color)))
                    .truncate()
                    .selectable(false),
            );
        },
    );
    // Close button (only in the expanded state: the rail itself never
    // crosses the title, so it cannot shift anything). Kept clear of the
    // floating scrollbar lane by the same margin as the title. The shared
    // chrome icon-button recipe is used (hover/press tween, pill-radius hover
    // fill, focus ring, accessible name, tooltip): the hand-rolled copy had
    // drifted from the other X buttons (CARD-radius fill, no pressed state).
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(
            row_rect.max.x - CLOSE_W / 2.0 - 2.0 - lane,
            row_rect.center().y,
        ),
        egui::vec2(CLOSE_W, CLOSE_W),
    );
    // The accessible name carries the tab title: with many tabs, dozens of
    // identical "Close tab" nodes are unusable with a screen reader. Built
    // lazily inside the closure (accesskit may be off: no per-frame String).
    let close_resp = icons::icon_button_at_named_lazy(
        ui,
        close_rect,
        ("tab-close", index),
        icons::Icon::Close,
        || format!("Close tab: {title}"),
        || "Close tab (Ctrl/Cmd+W)".to_string(),
        true,
    );
    if close_resp.clicked() {
        out.close = Some(index);
        return;
    }
    // Middle-click closes the tab.
    if row_resp.clicked_by(egui::PointerButton::Middle) {
        out.close = Some(index);
        return;
    }
    if row_resp.clicked() {
        out.select = Some(index);
    }
}

/// Draws the tab favicon (or a placeholder) at the shared icon column.
/// `origin` is the tab's server origin (one lookup per frame, shared);
/// `loading` / `state_kind` come from the display memo so this never needs
/// the whole `Tab` back.
pub(super) fn draw_favicon(
    ui: &mut egui::Ui,
    row_rect: egui::Rect,
    loading: bool,
    state_kind: TabStateKind,
    origin: Option<&Url>,
    favicons: &FaviconCache,
    selected: bool,
) {
    let s = theme::scheme_ref(ui);
    let center = egui::pos2(row_rect.min.x + ICON_CX, row_rect.center().y);
    // Images draw at the toolbar glyph size; vector placeholders paint in a
    // full toolbar button box, whose visible glyph is that same size.
    let art_rect = egui::Rect::from_center_size(center, egui::vec2(FAVICON, FAVICON));
    let glyph_rect = egui::Rect::from_center_size(center, egui::vec2(ICON_BOX, ICON_BOX));
    // Borrowed handle: painting does not need to clone the texture.
    let tex = origin.and_then(|origin| favicons.texture(origin));
    if let Some(tex) = tex {
        // Fit, never stretch: wide/tall favicons keep their aspect.
        crate::ui::paint_texture_fit(ui, art_rect, tex, 4);
        // Tiny "loading" dot over a stale favicon while a new page loads.
        if loading {
            ui.painter().circle_filled(
                egui::pos2(art_rect.max.x - 2.0, art_rect.max.y - 2.0),
                2.0,
                s.primary,
            );
        }
        return;
    }
    if loading {
        icons::paint(ui, glyph_rect, icons::Icon::Loading, s.on_surface_variant);
        return;
    }
    let (icon, color) = match state_kind {
        TabStateKind::Home => (icons::Icon::Home, s.on_surface_variant),
        _ => (
            icons::Icon::Globe,
            if selected {
                s.on_surface
            } else {
                s.on_surface_variant
            },
        ),
    };
    icons::paint(ui, glyph_rect, icon, color);
}
