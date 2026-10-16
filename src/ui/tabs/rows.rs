// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::tabs::rows`: one rail row (collapsed favicon or expanded
//! favicon+title+close), the pinned action rows and the edge separator.
//!
//! Geometry is shared by both rail states (same x/y/size), so expansion
//! only grows the background to the right and nothing jumps. Labels are the
//! only moving parts: they drift a few px while fading in and then rest at
//! that same shared column (`label_slide` is exactly 0 at rest).

use super::*;

/// Horizontal drift of a label during the overlay's open animation: it
/// starts `TITLE_SLIDE` px toward the growing edge and settles at its final
/// column as `t` reaches 1 (and at exactly 0 once at rest). The favicon and
/// the vector icons never move: only the text drifts.
fn label_slide(t: f32) -> f32 {
    TITLE_SLIDE * (1.0 - t)
}

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
/// `label_alpha` fades the label during the overlay's open animation and
/// drives its drift (the icon stays solid: it is painted with its full tint).
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
    }
    if resp.has_focus() {
        // Shared focus ring IN ADDITION to the hover wash (same rule as
        // every other hand-painted row): hiding it while hovered lost
        // keyboard position as soon as the pointer rested on the row.
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
        // Aligned with the tab titles (same icon column + gap), drifting in
        // with them during the reveal.
        let title_left = rect.min.x + ICON_CX + FAVICON / 2.0 + 8.0 + label_slide(label_alpha);
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
                // Regular chrome text: the rail is not document emphasis, so
                // it deliberately does NOT go through `theme::strong` (the
                // `wght = 700` face the reading surfaces use).
                ui.add(
                    egui::Label::new(egui::RichText::new(label).color(color))
                        .truncate()
                        .selectable(false),
                );
            },
        );
    }
}

/// Right-edge separation for the rail/expanded overlay.
///
/// The toolbar, rail and overlay share one chrome tone and must read as a
/// single surface, so there is deliberately NO shadow above (the frame
/// shadow's top half painted a gray seam under the toolbar). The right edge
/// floats over the page and needs its own separation:
/// - light: a soft gradient penumbra (darkest at the panel edge, gone at its
///   reach);
/// - dark: a crisp 1px hairline (black blur over dark chrome reads as dirt,
///   same rationale as `theme::soft_shadow`).
///
/// `progress` is the rail's open tween (0 collapsed, 1 fully open). The
/// separator is drawn in BOTH states — collapsed it is the smaller, fainter
/// version — and it morphs with the panel, so opening or closing never pops
/// the shadow on or off (the collapsed rail used to show nothing at all).
pub(super) fn paint_edge_separator(
    painter: &egui::Painter,
    dark: bool,
    rect: egui::Rect,
    s: &theme::Scheme,
    progress: f32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let progress = progress.clamp(0.0, 1.0);
    if dark {
        // One hairline at both ends of the tween, dimmed while collapsed so
        // the boundary strengthens with the panel instead of snapping on.
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.max.x - 1.0, rect.min.y), rect.max),
            egui::CornerRadius::ZERO,
            s.outline_variant
                .gamma_multiply(egui::lerp(0.55..=1.0, progress)),
        );
        return;
    }
    // Reach (px) and peak alpha of the penumbra: a hint of boundary when
    // collapsed, the floating page shadow when open.
    const COLLAPSED_WIDTH: f32 = 5.0;
    const COLLAPSED_ALPHA: f32 = 12.0;
    const OPEN_WIDTH: f32 = 12.0;
    const OPEN_ALPHA: f32 = 26.0;
    const STEPS: usize = 12;
    let width = egui::lerp(COLLAPSED_WIDTH..=OPEN_WIDTH, progress);
    let peak = egui::lerp(COLLAPSED_ALPHA..=OPEN_ALPHA, progress);
    let step_w = width / STEPS as f32;
    for i in 0..STEPS {
        let t = i as f32 / STEPS as f32;
        // Quadratic falloff: darkest at the panel edge, gone at `width`.
        let alpha = (peak * (1.0 - t) * (1.0 - t)).round() as u8;
        if alpha == 0 {
            continue;
        }
        let x0 = rect.max.x + t * width;
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

/// Drag-reorder signal one row reported this pass (see `show_tabs`).
#[derive(Clone, Copy, Debug)]
pub(super) enum RowDrag {
    /// This row just started being dragged.
    Start,
    /// The pointer is over this row while another one is dragged: it is the
    /// drop target.
    Over,
    /// The dragged row was released.
    Stop,
}

/// One row's drag state for this pass, bundled so the row's signature stays
/// readable (the same shape `ui::bookmarks` gives its `TileRef`).
#[derive(Clone, Copy, Default)]
pub(super) struct RowDragState {
    /// This row is the one being dragged.
    pub dragging: bool,
    /// Some row is being dragged right now (drop targets are live).
    pub active: bool,
}

/// The egui id of one rail row's interaction.
///
/// Keyed by the TAB id, not by the row's slot: focus, hover, tooltip and the
/// in-flight drag follow the tab when the rail reorders (a slot-keyed id made
/// `Alt+Up`/`Alt+Down` walk the focus onto a different tab after every move).
/// `state` separates the collapsed rail's row from the expanded overlay's,
/// which are both drawn in the overlay's opening frame.
pub(super) fn row_id(state: &'static str, tab_id: usize) -> egui::Id {
    egui::Id::new(("tab-row", state, tab_id))
}

/// Keyboard reorder of the FOCUSED row: `Alt+Up`/`Alt+Down` move it one slot,
/// the vertical equivalent of the bookmark grid's `Alt+Left`/`Alt+Right`.
/// One slot per press; `None` when the row is not focused or the move would
/// leave the list. Returns `(from, to)` in the current list order.
pub(super) fn move_keys(
    ui: &egui::Ui,
    resp: &egui::Response,
    index: usize,
    count: usize,
) -> Option<(usize, usize)> {
    if !resp.has_focus() {
        return None;
    }
    let delta = ui.input(|i| {
        if !i.modifiers.alt {
            0
        } else if i.key_pressed(egui::Key::ArrowUp) {
            -1
        } else if i.key_pressed(egui::Key::ArrowDown) {
            1
        } else {
            0
        }
    });
    if delta == 0 {
        return None;
    }
    let last = count.saturating_sub(1);
    let to = (index as i64 + i64::from(delta)).clamp(0, last as i64) as usize;
    (to != index).then_some((index, to))
}

/// One expanded row: favicon + title + close, click selects and dragging it
/// reorders the rail. `text_t` fades the title during the overlay's open
/// animation and drives its drift (the favicon itself stays solid).
#[allow(clippy::too_many_arguments)]
pub(super) fn expanded_row(
    ui: &mut egui::Ui,
    index: usize,
    tab: &mut Tab,
    selected: bool,
    row_w: f32,
    text_t: f32,
    favicons: &mut FaviconCache,
    drag: RowDragState,
    count: usize,
    out: &mut TabsResponse,
) -> Option<RowDrag> {
    let s = theme::scheme_ref(ui);
    // Geometry and interaction are separate: the widget id must be keyed by
    // the TAB (see `row_id`), which `allocate_exact_size` cannot express.
    // `cross_justify` is false in this layout, so the rect is exactly the one
    // requested (and the intrinsic size keeps `allocate_exact_size`'s
    // contract for any layout consumer).
    let (_, row_rect) = ui.allocate_space(egui::vec2(row_w, ROW_H));
    let mut row_resp = ui.interact(
        row_rect,
        row_id("open", tab.id),
        egui::Sense::click_and_drag(),
    );
    row_resp.set_intrinsic_size(egui::vec2(row_w, ROW_H));
    // Hover and drop target are read GEOMETRICALLY, from THIS frame's rect and
    // pointer, while still honoring what is on top of the row (a floating
    // window must win the pointer). `Response::contains_pointer` cannot do
    // this here: its snapshot is computed at frame start from the PREVIOUS
    // frame's rects, and a live reorder moves the rows, so the drag kept
    // targeting the slot a row no longer occupied (it bounced between two
    // slots instead of advancing).
    let pointer_over = ui.ctx().rect_contains_pointer(ui.layer_id(), row_rect);
    // While the row is dragged the pointer is captured, so a hover cursor no
    // longer applies: set the grab cursor directly, like the grid tile.
    let row_resp = if drag.dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        row_resp
    } else {
        row_resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    // Deferred: a drag event must not skip painting the row this frame.
    let mut drag_signal: Option<RowDrag> = None;
    if row_resp.drag_started() {
        drag_signal = Some(RowDrag::Start);
    } else if row_resp.drag_stopped() {
        drag_signal = Some(RowDrag::Stop);
    } else if drag.active && !drag.dragging && pointer_over {
        drag_signal = Some(RowDrag::Over);
    }
    // The wash stays while the pointer sits on any part of the row, the close
    // box included (it is a separate widget inside this rect).
    draw_row_background(
        ui,
        row_rect,
        selected,
        pointer_over,
        row_resp.has_focus(),
        s,
    );
    if drag.dragging {
        // The dragged row carries the same primary ring as a dragged tile.
        ui.painter().rect_stroke(
            row_rect.shrink(0.5),
            egui::CornerRadius::same(ROW_RADIUS),
            egui::Stroke::new(1.0, s.primary),
            egui::StrokeKind::Inside,
        );
    }
    // Captured before `display()` mutably borrows the tab: the close button
    // is keyed by the TAB (never its slot), so focus and hover follow the
    // page through a reorder (`Alt+Up/Down`, a drag).
    let tab_id = tab.id;
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
    // Sighted hover: the title truncates, and the collapsed rail already
    // tooltips while the expanded row did not. Gated (not lazy): the text
    // is borrowed, so hovered frames alone allocate.
    let row_resp = crate::ui::hover_tooltip(row_resp, title);
    // Title (non-interactive label: the row itself handles selection).
    // The right edge reserves the floating scrollbar's lane: the bar senses
    // the full bar width at the overlay's right edge even while invisible,
    // and would otherwise steal clicks from the outer part of the close box.
    let lane = theme::scrollbar_width(ui);
    let title_left = row_rect.min.x + ICON_CX + FAVICON / 2.0 + 8.0 + label_slide(text_t);
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
            // Regular chrome text (see `draw_action_row`): tab titles carry
            // the tab's identity, not emphasis, so they stay off the bold
            // family.
            ui.add(
                egui::Label::new(egui::RichText::new(title).color(title_color))
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
        ("tab-close", tab_id),
        icons::Icon::Close,
        || format!("Close tab: {title}"),
        || "Close tab (Ctrl/Cmd+W)".to_string(),
        true,
    );
    // Drag and keyboard-reorder signals win over click/close: egui reports
    // no click for a gesture it classified as a drag, so this can never
    // swallow a real activation.
    if let Some(signal) = drag_signal {
        return Some(signal);
    }
    if !drag.dragging {
        if let Some(mv) = move_keys(ui, &row_resp, index, count) {
            out.move_tab = Some(mv);
            return None;
        }
    }
    if close_resp.clicked() {
        out.close = Some(index);
        return None;
    }
    // Middle-click closes the tab.
    if row_resp.clicked_by(egui::PointerButton::Middle) {
        out.close = Some(index);
        return None;
    }
    if row_resp.clicked() {
        out.select = Some(index);
    }
    None
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
