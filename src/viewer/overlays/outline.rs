// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::overlays::outline`: the floating document-outline panel.
//!
//! A reading aid, never a layout participant:
//! - Overlay only (`Area`, `Order::Middle`): the document is never reflowed
//!   and the panel stays BELOW the context menu, the find bar and the
//!   toasts (all `Foreground`), so a menu opened over it is never covered.
//! - Docked to the reading sheet's right edge, just under the find bar, on
//!   ordinary windows; below `SHEET_W` it becomes a bottom sheet (same rows,
//!   one column), so a narrow window keeps a usable table of contents.
//! - Rows come from the document's own outline (`Fetched.outline`, built
//!   once per fetch), the current row from the scrollspy the caller ran
//!   right after this frame's document walk (`viewer::outline`).
//! - A row click requests a scroll through the SAME mechanism as a `#id`
//!   link click (`Tab::pending_anchor`): consumed by the matching heading on
//!   the next document walk, never a second scrolling implementation.
//! - Levels indent one step per heading level below the document's
//!   shallowest one, titles are elided to the row's width (the full title
//!   lives in the hover tooltip and the accessible name), and the list
//!   scrolls on its own with the current row revealed.
//!
//! `Esc` (unless another surface owns the press, see
//! `OutlineState::esc_elsewhere`) and a primary press outside the panel both
//! close it. The panel reads the key itself, like the find bar, so direct
//! rendering (tests) keeps the close behavior.

use crate::browser::tab::{Tab, TabState};
use crate::markdown::outline::{Outline, OutlineItem};
use crate::ui::{icons, theme};
use eframe::egui;

/// Panel width on ordinary windows: fits a two- or three-word heading with
/// its indent steps.
const PANEL_W: f32 = 264.0;
/// Row height. `item_spacing.y` is zeroed before `show_rows`, so rows advance
/// exactly by this (the reveal math assumes it).
const ROW_H: f32 = 26.0;
/// Left inset of the row label, before the indent steps.
const ROW_PAD: f32 = 10.0;
/// One indent step (px) per heading level below the shallowest one.
const INDENT_STEP: f32 = 14.0;
/// Width of the accent bar on the current row.
const ACCENT_W: f32 = 3.0;
/// Distance from the panel's right edge to the sheet's (matches the find
/// bar's `-12`).
const SIDE_OFFSET: f32 = 12.0;
/// Distance from the viewport top to the panel's top: the find bar sits at
/// `-12/48` (34px capsule + padding), so `90` clears it.
const TOP_OFFSET: f32 = 90.0;
/// Window width below which the panel docks to the bottom instead of the
/// right edge (phone/tablet-width windows: a right-docked panel would cover
/// the reading column whole).
const SHEET_W: f32 = 640.0;
/// Tallest the panel gets, as a fraction of the window height (a long
/// outline scrolls inside it instead of covering the page).
const MAX_H_FRAC: f32 = 0.6;
/// Breathing room kept around the bottom sheet.
const SHEET_MARGIN: f32 = 12.0;

/// Shows the outline panel when `tab.outline.open` (the caller gates the
/// state on document pages).
pub(crate) fn show_outline_panel(root: &mut egui::Ui, tab: &mut Tab) {
    if !tab.outline.open {
        return;
    }
    // The outline of the page on screen. Field access (not
    // `Tab::document_outline`) on purpose: the borrow of `tab.state` below
    // stays disjoint from the panel's own state (`tab.outline`) and the
    // pending anchor a row sets.
    let outline: &Outline = match &tab.state {
        TabState::Loaded { fetched } => &fetched.outline,
        TabState::Licenses => match crate::licenses::document() {
            Some(doc) => &doc.outline,
            None => return,
        },
        // A document page is the only place the panel opens; anything else
        // is a stale state one frame from its reset.
        _ => return,
    };
    if outline.is_empty() {
        // Nothing to list (an all-image or heading-less document): close
        // instead of showing an empty card.
        tab.outline.close();
        return;
    }
    let ctx = root.ctx().clone();
    // Esc closes unless another surface owns the press this frame.
    if !tab.outline.esc_elsewhere && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.outline.close();
        return;
    }
    // Copies for the closure: it must not capture `tab` (the state borrow
    // above stays live for the whole draw).
    let tab_id = tab.id;
    let rows = outline.items.len();
    let current = tab.outline.current.min(rows - 1);
    let reveal = tab.outline.reveal;
    // Panel geometry: right-docked below the find bar on ordinary windows,
    // a bottom sheet on narrow ones.
    let screen = ctx.content_rect();
    let sheet = screen.width() < SHEET_W;
    let panel_w = if sheet {
        PANEL_W.min((screen.width() - 2.0 * SHEET_MARGIN).max(160.0))
    } else {
        PANEL_W
    };
    // Viewport of the row list: the rows that fit the window cap; the list
    // itself scrolls past it.
    let cap = (screen.height() * MAX_H_FRAC).max(ROW_H * 3.0);
    let view_h = (rows as f32 * ROW_H).min(cap);
    let mut clicked: Option<String> = None;
    let mut close = false;
    let area = egui::Area::new(egui::Id::new(("outline-panel", tab_id)))
        .anchor(
            if sheet {
                egui::Align2::CENTER_BOTTOM
            } else {
                egui::Align2::RIGHT_TOP
            },
            if sheet {
                egui::vec2(0.0, -SHEET_MARGIN)
            } else {
                egui::vec2(-SIDE_OFFSET, TOP_OFFSET)
            },
        )
        // Below the find bar, the context menu and the toasts (all
        // `Foreground`): a menu opened over the panel is never covered.
        .order(egui::Order::Middle)
        .show(&ctx, |ui| {
            // Same elevation recipe as the find bar (lighter than the
            // menu-weight popup shadow).
            egui::Frame::popup(ui.style())
                .corner_radius(egui::CornerRadius::same(theme::radius::CARD))
                .shadow(theme::soft_shadow(ui, [0, 1], 8, 14))
                .inner_margin(egui::Margin::same(theme::spacing::S as i8))
                .show(ui, |ui| {
                    ui.set_width(panel_w);
                    let s = theme::scheme_ref(ui);
                    ui.horizontal(|ui| {
                        ui.label(
                            theme::strong(
                                egui::RichText::new("Outline").size(theme::typography::EYEBROW),
                            )
                            .color(s.on_surface_variant),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::icon_button(ui, icons::Icon::Close, "Close (Esc)", true)
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(theme::spacing::XXS);
                    // Rows advance exactly by `ROW_H`: `show_rows` reads the
                    // spacing from THIS `Ui` (`reveal_offset` assumes it).
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt(("outline-scroll", tab_id))
                        .max_height(view_h)
                        .auto_shrink([false, false]);
                    if reveal {
                        // Computed, not `scroll_to_rect`ed: `show_rows` only
                        // runs the visible rows, so an off-screen row could
                        // never be scrolled to through its own rect.
                        scroll = scroll.vertical_scroll_offset(
                            crate::viewer::outline::reveal_offset(current, rows, view_h, ROW_H),
                        );
                    }
                    let out = scroll.show_rows(ui, ROW_H, rows, |ui, range| {
                        for index in range {
                            let item = &outline.items[index];
                            let indent = outline.indent_steps(item) as f32 * INDENT_STEP;
                            let id = egui::Id::new(("outline-row", tab_id, index));
                            let resp = row(ui, id, item, indent, index == current);
                            if resp.clicked() {
                                clicked = Some(item.id.clone());
                            }
                        }
                    });
                    // Registered so closing the tab drops the offset (the
                    // real id is derived from the parent `Ui`, see
                    // `ui::remember_scroll`).
                    crate::ui::remember_scroll(
                        ui.ctx(),
                        crate::ui::OUTLINE_SCROLL_ID,
                        tab_id,
                        out.id,
                    );
                });
        });
    if close {
        tab.outline.close();
        return;
    }
    tab.outline.reveal = false;
    if let Some(id) = clicked {
        // The same request a `#id` link click makes: the next document walk
        // scrolls the matching heading to the top.
        tab.pending_anchor = Some(id);
    }
    // A primary PRESS outside closes (a drag selection counts: egui reports
    // it as a press, never as a click). The rows above already claimed
    // their own clicks, which sit inside the panel rect.
    let outside = ctx.input(|i| {
        i.pointer.primary_pressed()
            && i.pointer
                .hover_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos))
    });
    if outside {
        tab.outline.close();
    }
}

/// One outline row: label, indent, current/hover state, keyboard focus ring
/// and the click target.
fn row(
    ui: &mut egui::Ui,
    id: egui::Id,
    item: &OutlineItem,
    indent: f32,
    current: bool,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let width = ui.available_width().max(80.0);
    let sense = egui::Sense::click();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), sense);
    let resp = ui.interact(rect, id, sense);
    if current {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::row::ROW_RADIUS),
            s.container_low,
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
    let text_left = ROW_PAD + indent;
    let text_w = (rect.width() - text_left - ROW_PAD).max(8.0);
    // One-row elided galley (`…` past the row's width): the full title stays
    // in the tooltip and the accessible name.
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_rows = 1;
    job.wrap.max_width = text_w;
    job.wrap.overflow_character = Some('…');
    job.append(
        &item.title,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(theme::typography::UI_SMALL),
            color,
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);
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
    // infer it from the shapes (the full title, never the elided galley).
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item.title.as_str())
    });
    // Tooltip last: it consumes the response and returns it.
    crate::ui::hover_tooltip(resp, &item.title)
}
