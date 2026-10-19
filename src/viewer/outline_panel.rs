// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::outline_panel`: the always-visible document-outline sidebar.
//!
//! A reading aid that is part of the layout, never an overlay and never a
//! context-menu entry:
//! - It docks to the right of the reading sheet on every document page, even
//!   one without headings (a short empty state replaces the rows). Keeping
//!   the slot on every page means navigation never reflows the reading
//!   column, and the page that is still visible while the next one loads
//!   keeps the slot with a short loading state. It is intentionally narrow (176-240px depending on
//!   the window) and titles WRAP: a long heading takes as many lines as it
//!   needs instead of being elided.
//! - Rows come from the document's own outline (`Fetched.outline`, built
//!   once per fetch), the current row from the scrollspy the caller ran
//!   right after this frame's document walk (`viewer::outline`).
//! - A row click requests a scroll through the SAME mechanism as a `#id`
//!   link click (`Tab::pending_anchor`): this frame's document walk (the
//!   sidebar is drawn before the sheet) consumes it, never a second
//!   scrolling implementation.
//! - The list virtualizes itself: measured row heights are cached per panel
//!   width/document, and only the visible rows are laid out and painted.
//!   Heights are measured with the same wrapping job the rows paint, so the
//!   scrollbar can never drift from the content.
//! - The panel publishes its screen rect in [`OutlineState::panel_rect`] so
//!   the find bar docks just left of it instead of covering it.

use crate::markdown::outline::{Outline, OutlineItem};
use crate::ui::theme;
use crate::viewer::outline::OutlineState;
use eframe::egui;

/// Widest the sidebar gets: narrow enough that the reading sheet keeps the
/// page's measure on a desktop window.
const PANEL_MAX_W: f32 = 240.0;
/// Narrowest usable width: one indent step plus a two-word title.
const PANEL_MIN_W: f32 = 176.0;
/// Space the document always keeps when the window is unusually narrow.
const DOC_MIN_W: f32 = 320.0;
/// Sidebar horizontal padding (inside the panel frame).
const PAD_X: f32 = 12.0;
/// Row vertical padding, above and below the wrapped title.
const ROW_PAD_Y: f32 = 6.0;
/// Shortest row (a one-line title with breathing room).
const ROW_MIN_H: f32 = 28.0;
/// Left inset of the title before its indent steps.
const ROW_PAD_X: f32 = 8.0;
/// One indent step (px) per heading level below the shallowest one.
const INDENT_STEP: f32 = 12.0;
/// Width of the accent bar on the current row.
const ACCENT_W: f32 = 3.0;
/// Extra rows rendered above/below the viewport, so a partial row is never
/// missing while the scroll position settles.
const OVERSCAN_ROWS: usize = 2;

/// What the panel asks the viewer to do this frame.
#[derive(Debug, Default)]
pub(crate) struct OutlinePanelResponse {
    /// Anchor id of a clicked row (the viewer stores it in
    /// `Tab::pending_anchor`, exactly like a `#id` link click).
    pub clicked: Option<String>,
}

/// Width the sidebar takes from `available` points: proportional on ordinary
/// windows, clamped so it never eats the document.
pub(crate) fn panel_width(available: f32) -> f32 {
    if !available.is_finite() || available <= 0.0 {
        return 0.0;
    }
    let desired = (available * 0.28).clamp(PANEL_MIN_W, PANEL_MAX_W);
    desired.min((available - DOC_MIN_W).max(0.0))
}

/// Draws the sidebar and returns a clicked row, if any. The caller draws it
/// BEFORE its central panel, so the document lays out in the remaining
/// width; the title rows never join the document's text selection.
///
/// `empty_label` is shown when the page has no headings, so the slot never
/// disappears (and the reading column never widens) between pages.
pub(crate) fn show(
    ui: &mut egui::Ui,
    outline: &Outline,
    state: &mut OutlineState,
    tab_id: usize,
    doc_generation: u64,
    empty_label: &str,
) -> OutlinePanelResponse {
    let available = ui.available_width();
    let width = panel_width(available);
    if width < PANEL_MIN_W * 0.5 {
        // No room for a usable sidebar (a window shrunk far below the
        // supported minimum): the document wins.
        state.panel_rect = None;
        return OutlinePanelResponse::default();
    }
    let s = theme::scheme_ref(ui);
    let frame = egui::Frame::new()
        .fill(s.container_low)
        .inner_margin(egui::Margin {
            left: PAD_X as i8,
            right: PAD_X as i8,
            top: 14,
            bottom: 10,
        });
    let mut clicked = None;
    let response = egui::Panel::right(egui::Id::new(("outline-panel", tab_id)))
        .resizable(false)
        .exact_size(width)
        .frame(frame)
        .show(ui, |ui| {
            header(ui);
            if outline.is_empty() {
                empty_state(ui, empty_label);
            } else {
                clicked = list(ui, outline, state, tab_id, doc_generation);
            }
        });
    let rect = response.response.rect;
    state.panel_rect = Some(rect);
    if let Some(index) = clicked {
        let id = outline.items.get(index).map(|item| item.id.clone());
        state.select(index);
        return OutlinePanelResponse { clicked: id };
    }
    OutlinePanelResponse::default()
}

/// The panel heading. No close button: the sidebar is part of the document
/// page, not a dismissible popup.
fn header(ui: &mut egui::Ui) {
    let s = theme::scheme_ref(ui);
    ui.label(
        theme::strong(egui::RichText::new("Outline").size(theme::typography::EYEBROW))
            .color(s.on_surface_variant),
    );
    ui.add_space(theme::spacing::XS);
}

/// Placeholder for a page with no headings (or for the previous page while
/// the next one loads): the panel keeps its width so the sheet never reflows.
fn empty_state(ui: &mut egui::Ui, label: &str) {
    let s = theme::scheme_ref(ui);
    ui.label(
        egui::RichText::new(label)
            .size(theme::typography::UI_SMALL)
            .color(s.on_surface_variant),
    );
}

/// The virtualized row list. Returns the clicked row index, if any.
fn list(
    ui: &mut egui::Ui,
    outline: &Outline,
    state: &mut OutlineState,
    tab_id: usize,
    doc_generation: u64,
) -> Option<usize> {
    let rows = outline.items.len();
    let width = ui.available_width().max(1.0);
    let key = (width.to_bits(), doc_generation, rows);
    if state.row_heights_key != Some(key) || state.row_heights.len() != rows {
        state.row_heights.clear();
        state.row_heights.reserve(rows);
        for item in &outline.items {
            state
                .row_heights
                .push(measure_row(ui, item, outline.indent_steps(item), width));
        }
        state.row_heights_key = Some(key);
    }
    let heights = std::mem::take(&mut state.row_heights);
    let total_h: f32 = heights.iter().sum();
    let current = state.current.min(rows.saturating_sub(1));
    let view_h = ui.available_height().max(ROW_MIN_H);
    let reveal = std::mem::take(&mut state.reveal);
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(("outline-scroll", tab_id))
        .auto_shrink([false, false])
        .max_height(view_h);
    if reveal {
        scroll = scroll.vertical_scroll_offset(crate::viewer::outline::reveal_offset(
            current, &heights, view_h,
        ));
    }
    let mut clicked = None;
    let out = scroll.show_viewport(ui, |ui, viewport| {
        ui.set_width(width);
        ui.set_height(total_h);
        let (start, top) = first_visible(&heights, viewport.min.y);
        let mut end = start;
        let mut bottom = top;
        while end < rows && bottom < viewport.max.y + ROW_MIN_H {
            bottom += heights[end];
            end += 1;
        }
        let extra = (end + OVERSCAN_ROWS).min(rows);
        for height in &heights[end..extra] {
            bottom += height;
        }
        end = extra;
        let y_min = ui.max_rect().top() + top;
        let y_max = ui.max_rect().top() + bottom;
        let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), y_min..=y_max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            for (index, height) in heights.iter().enumerate().take(end).skip(start) {
                let item = &outline.items[index];
                let indent = outline.indent_steps(item) as f32 * INDENT_STEP;
                let id = egui::Id::new(("outline-row", tab_id, index));
                if row(ui, id, item, indent, index == current, width, *height).clicked() {
                    clicked = Some(index);
                }
            }
        });
    });
    // Registered so closing the tab drops the offset (the real id is derived
    // from the parent `Ui`, see `ui::remember_scroll`).
    crate::ui::remember_scroll(ui.ctx(), crate::ui::OUTLINE_SCROLL_ID, tab_id, out.id);
    state.row_heights = heights;
    clicked
}

/// First visible row and its top offset inside the content, from the panel
/// viewport top.
fn first_visible(heights: &[f32], viewport_top: f32) -> (usize, f32) {
    let mut top = 0.0f32;
    for (index, height) in heights.iter().enumerate() {
        if top + height > viewport_top {
            return (index, top);
        }
        top += height;
    }
    (heights.len(), top)
}

/// Height of one wrapped title row at `width`, measured with the exact job
/// [`row`] paints.
fn measure_row(ui: &egui::Ui, item: &OutlineItem, steps: u8, width: f32) -> f32 {
    let indent = steps as f32 * INDENT_STEP;
    let text_w = (width - ROW_PAD_X - indent - ROW_PAD_X).max(24.0);
    let galley = ui
        .painter()
        .layout_job(title_job(item, text_w, egui::Color32::PLACEHOLDER));
    (galley.size().y + 2.0 * ROW_PAD_Y).max(ROW_MIN_H)
}

/// Layout job shared by measurement and painting: the full title wraps at
/// `max_width`; no row limit and no elision character.
fn title_job(item: &OutlineItem, max_width: f32, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width;
    job.append(
        &item.title,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(theme::typography::UI_SMALL),
            color,
            ..Default::default()
        },
    );
    job
}

/// One outline row: wrapped label, indent, current/hover state, keyboard
/// focus ring and the click target. `height` is the measured height, so the
/// row and the scrollbar agree exactly.
fn row(
    ui: &mut egui::Ui,
    id: egui::Id,
    item: &OutlineItem,
    indent: f32,
    current: bool,
    width: f32,
    height: f32,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let sense = egui::Sense::click();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let resp = ui.interact(rect, id, sense);
    if current {
        // One step darker than the panel fill (`container_low`): the current
        // row must read as a surface, not disappear into the sidebar.
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::row::ROW_RADIUS),
            s.container,
        );
        // Accent bar: the reading-aid equivalent of the rail's active bar.
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x, rect.min.y + 4.0),
            egui::pos2(rect.min.x + ACCENT_W, rect.max.y - 4.0),
        );
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius::same(theme::radius::HAIRLINE),
            s.primary,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::row::ROW_RADIUS),
            s.container_hover,
        );
    }
    if resp.has_focus() {
        crate::ui::theme::focus_ring(ui, rect, crate::ui::row::ROW_RADIUS);
    }
    let color = if current {
        s.on_surface
    } else {
        s.on_surface_variant
    };
    let text_left = ROW_PAD_X + indent;
    let text_w = (rect.width() - text_left - ROW_PAD_X).max(24.0);
    let galley = ui.painter().layout_job(title_job(item, text_w, color));
    ui.painter().galley(
        egui::pos2(
            rect.min.x + text_left,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // Accessible name: the row is custom-painted, so screen readers cannot
    // infer it from the shapes (the full title, never a wrapped fragment).
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.title.as_str())
    });
    crate::ui::hover_tooltip(resp, &item.title)
}
