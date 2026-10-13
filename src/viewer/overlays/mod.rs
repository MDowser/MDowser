// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::overlays`: floating UI over the document.
//!
//! - `content_menu`: minimal right-click menu over the page: `Copy` and
//!   `Select all`, plus `Open link in new tab` when the click landed on a
//!   link. Uses a global secondary-click detector + a foreground `Area`, so
//!   right-click works on text too without ever eating link clicks.
//!   A gesture whose press was intercepted before egui (a text selection
//!   was alive: see `MdowserApp::raw_input_hook`) opens the menu from
//!   `Tab::pending_context_click` instead of `secondary_clicked`.
//! - `show_toast`: transient confirmation (copy), auto-dismissed.
//!
//! ORDER MATTERS: the document calls `content_menu` BEFORE rendering the
//! document. A `Copy` click pushes `Event::Copy` into this frame's input,
//! so the labels rendered right after -- which still hold the native text
//! selection -- perform the actual clipboard copy.
//!
//! The menu anchor is in screen coordinates, so a window move / scroll /
//! resize invalidates it: the menu closes instead of drifting (the page
//! highlight stays, it belongs to the document). The `Area` constrains
//! itself to the window, so the popup never spawns off-screen.

use super::{has_label_selection, LinkHit};
use crate::browser::tab::{Tab, TabState};
use crate::ui::theme;
use eframe::egui;

/// Menu width: every row shares it, so the popup has clean straight edges.
const MENU_W: f32 = 212.0;
/// Menu row height (compact: no chunky default button padding).
const ROW_H: f32 = 30.0;
/// Horizontal padding inside a row (label and shortcut hint).
const ROW_PAD: f32 = 10.0;

/// Action chosen from the right-click menu (applied to the current tab).
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    /// Copy the native text selection, else the whole page text.
    Copy,
    /// Highlight the whole document (drops any native selection).
    SelectAll,
}

/// Right-click context menu over the document.
///
/// Returns the URL of a link the user asked to open in a new tab (`App`
/// owns the tab list; the viewer never creates tabs by itself).
///
/// Intentionally minimal:
/// - `Open link in new tab` (only when the right-click landed on a link).
/// - `Copy`: the drag selection when there is one, else the whole page text.
/// - `Select all`: whole-page highlight; `Ctrl/Cmd+C` then copies it.
///
/// egui has no cross-widget selection model that can be driven from the
/// outside, so "Select all" highlights the whole page (`Tab::selected_all`)
/// and `Copy` computes the plain text from the body. A REAL (drag)
/// selection is copied through egui itself: the menu pushes `Event::Copy`
/// before the labels render, so the label selection plugin copies it (see
/// the module docs).
pub(crate) fn content_menu(root: &mut egui::Ui, tab: &mut Tab) -> Option<String> {
    let ctx = root.ctx().clone();
    // A closed menu owns nothing: if a row still holds keyboard focus in
    // egui memory (the keyboard path closes the menu after `Enter`), drop it
    // now — otherwise the next pointer open would rebuild that row with its
    // focus ring lit and it would read as a pre-selected entry again. Run
    // BEFORE the open detection so a reopen in the same frame is clean.
    if !tab.context_open {
        surrender_menu_focus(&ctx, tab.id);
    }
    // Window move: the anchor lives in screen coords, so any viewport move
    // invalidates it; close instead of drifting.
    let screen = ctx
        .input(|i| i.viewport().inner_rect)
        .unwrap_or_else(|| ctx.input(|i| i.viewport_rect()));
    if tab.context_open {
        if let Some(open_at) = tab.context_viewport {
            if open_at != screen {
                tab.context_open = false;
                tab.context_viewport = None;
                return None;
            }
        }
    }
    // Esc always closes an open menu. Scroll/resize also closes: the anchor
    // is in screen coords and goes stale after either.
    if tab.context_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.context_open = false;
    }
    if tab.context_open
        && ctx.input(|i| i.smooth_scroll_delta.y.abs() > 0.0 || i.zoom_delta() != 1.0)
    {
        tab.context_open = false;
        return None;
    }
    // Global pointer detection against the last viewport rect (the document
    // renders after this call, so `doc_rect` / `link_hits` are the previous
    // frame's: stable unless the page scrolled or navigated).
    // `reopened`: a right-click that (re)anchors the menu this frame is NOT
    // an "outside click", even when the Area had to shift the popup away
    // from the window edge.
    let mut reopened = false;
    // A gesture whose PRESS was intercepted before egui (`raw_input_hook`,
    // when a text selection existed) arrives as a pending click: egui never
    // saw the press, so `secondary_clicked` stays false for it.
    let pending_click = tab.pending_context_click.take();
    if let Some(rect) = tab.doc_rect {
        let secondary_pos = pending_click.or_else(|| {
            ctx.input(|i| i.pointer.secondary_clicked())
                .then(|| ctx.input(|i| i.pointer.hover_pos()))
                .flatten()
        });
        if let Some(pos) = secondary_pos {
            if rect.contains(pos) {
                tab.context_open = true;
                tab.context_pos = Some(pos);
                tab.context_viewport = Some(screen);
                // Link under the cursor, if any: the menu will offer
                // "Open link in new tab" above Copy / Select all.
                tab.context_link = link_at(&tab.link_hits, pos).map(str::to_string);
                reopened = true;
            }
        } else if ctx.input(|i| i.pointer.primary_pressed()) && !tab.context_open {
            // Press (not click) so starting a drag selection replaces the
            // whole-page highlight with the real selection immediately.
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                if rect.contains(pos) {
                    tab.selected_all = false;
                }
            }
        }
    }
    if !tab.context_open {
        return None;
    }
    // Cheap: selected-all flag only. Body NOT cloned per frame (10 MiB jank).
    let was_selected = match &tab.state {
        TabState::Loaded { .. } | TabState::Licenses => tab.selected_all,
        _ => {
            tab.context_open = false;
            return None;
        }
    };
    let has_selection = has_label_selection(&ctx);
    let anchor = tab.context_pos.unwrap_or_else(|| {
        tab.doc_rect
            .map(|r| r.center())
            .unwrap_or_else(|| egui::pos2(200.0, 200.0))
    });
    let copy_hint: &str = if cfg!(target_os = "macos") {
        "Cmd+C"
    } else {
        "Ctrl+C"
    };
    let all_hint: &str = if cfg!(target_os = "macos") {
        "Cmd+A"
    } else {
        "Ctrl+A"
    };
    let copy_tip = if has_selection && !was_selected {
        "Copy the selected text"
    } else {
        "Copy the whole page text"
    };
    let mut action: Option<MenuAction> = None;
    let mut open_new: Option<String> = None;
    // Keyboard navigation: Up/Down move focus between the rows, Enter
    // activates the focused one. Opening the menu with the pointer leaves NO
    // row focused: the ring lit "Copy" (without a link under the cursor) on
    // every open, which read as a pre-selected entry. The rows are real
    // widgets with explicit ids, so the keys still reach them.
    let row_count = if tab.context_link.is_some() { 3 } else { 2 };
    let row_id = |index: usize| egui::Id::new(("doc-menu-row", tab.id, index));
    let (menu_up, menu_down, menu_enter) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
        )
    });
    if !reopened && (menu_up || menu_down) {
        let focused = ctx.memory(|m| (0..row_count).find(|&i| m.has_focus(row_id(i))));
        let next = match (focused, menu_up) {
            (Some(0), true) | (None, true) => row_count - 1,
            (Some(i), true) => i - 1,
            (Some(i), false) if i + 1 < row_count => i + 1,
            _ => 0,
        };
        ctx.memory_mut(|m| {
            // Cancel egui's own arrow-key focus traversal for this frame: it
            // would move PAST the row this code just requested (the old menu
            // relied on that traversal, which only worked because a row was
            // already focused at open).
            m.move_focus(egui::FocusDirection::None);
            m.request_focus(row_id(next));
        });
    }
    // `fixed_pos` + the default `constrain(true)`: the Area clamps itself
    // to the window, so the popup never spawns off-screen near an edge.
    // Tuple id salt: no per-frame allocation (the old `format!` built one
    // `String` per frame while the menu was open).
    let area = egui::Area::new(egui::Id::new(("doc-menu", tab.id)))
        .fixed_pos(anchor)
        .order(egui::Order::Foreground)
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(MENU_W);
                ui.set_max_width(MENU_W);
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut index = 0usize;
                let mut row =
                    |ui: &mut egui::Ui, label: &str, hint: Option<&str>| -> egui::Response {
                        let resp = menu_row(ui, row_id(index), label, hint, true);
                        index += 1;
                        resp
                    };
                if let Some(url) = tab.context_link.as_deref() {
                    let resp = row(ui, "Open link in new tab", None);
                    if resp.clicked() || (menu_enter && resp.has_focus()) {
                        open_new = Some(url.to_string());
                    }
                    menu_separator(ui);
                }
                let copy_row = row(ui, "Copy", Some(copy_hint));
                let copy_enter = menu_enter && copy_row.has_focus();
                if crate::ui::hover_tooltip(copy_row, copy_tip).clicked() || copy_enter {
                    action = Some(MenuAction::Copy);
                }
                let all_row = row(ui, "Select all", Some(all_hint));
                if all_row.clicked() || (menu_enter && all_row.has_focus()) {
                    action = Some(MenuAction::SelectAll);
                }
            });
        });
    // Click outside the popup closes it (rows above set `action`/`open_new`).
    // Hover is at the click pos, so this is a true outside-click test.
    // A PRESS that starts outside closes it too: egui reports a drag (press,
    // move, release) as `primary_pressed` + movement and never as a click, so
    // the click-only check left the menu floating over the whole drag
    // selection gesture.
    let outside = !reopened
        && ctx.input(|i| {
            (i.pointer.primary_clicked()
                || i.pointer.secondary_clicked()
                || i.pointer.primary_pressed())
                && i.pointer
                    .hover_pos()
                    .is_some_and(|p| !area.response.rect.contains(p))
        });
    if outside && action.is_none() && open_new.is_none() {
        tab.context_open = false;
        return None;
    }
    if let Some(url) = open_new {
        tab.context_open = false;
        return Some(url);
    }
    match action {
        Some(MenuAction::Copy) => {
            tab.context_open = false;
            if has_selection && !was_selected {
                // Real selection: egui owns the text. The labels below read
                // this event this very frame and copy the selection.
                ctx.input_mut(|i| i.events.push(egui::Event::Copy));
                tab.set_toast("Selection copied");
            } else {
                // Whole page (select-all highlight or nothing selected).
                // Only here pay the copy cost (explicit action); the text was
                // derived once at fetch time, no UI-thread reparse.
                let full = match &tab.state {
                    TabState::Loaded { fetched } => fetched.plain_text.clone(),
                    TabState::Licenses => crate::licenses::document()
                        .map(|doc| doc.plain_text.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                ctx.copy_text(full);
                tab.set_toast("Text copied");
            }
            None
        }
        Some(MenuAction::SelectAll) => {
            tab.context_open = false;
            super::select_all(&ctx, tab);
            None
        }
        None => None,
    }
}

/// Drops any menu-row keyboard focus left in egui memory when the menu is
/// closed, so a rebuilt row cannot ring itself on the next open.
/// Max 3 rows (`Open link in new tab` + `Copy` + `Select all`).
fn surrender_menu_focus(ctx: &egui::Context, tab_id: usize) {
    let row_id = |index: usize| egui::Id::new(("doc-menu-row", tab_id, index));
    if let Some(id) = ctx
        .memory(|m| m.focused())
        .filter(|id| (0..3).any(|index| row_id(index) == *id))
    {
        ctx.memory_mut(|m| m.surrender_focus(id));
    }
}

/// URL of the link whose hitbox contains `pos`.
/// Last hit wins: a link split into several inline fragments pushes one
/// hitbox per fragment, and they sit next to each other (never overlapping).
fn link_at(hits: &[LinkHit], pos: egui::Pos2) -> Option<&str> {
    hits.iter()
        .rev()
        .find(|h| h.rect.contains(pos))
        .map(|h| h.url.as_ref())
}

/// One compact, full-width menu row: label left, optional shortcut hint
/// right, subtle hover fill. Text is PAINTED (menu text must never join a
/// document text selection, and egui buttons cannot be full-width flat).
fn menu_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    hint: Option<&str>,
    enabled: bool,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let fg = if enabled {
        s.on_surface
    } else {
        // Muted but full-opacity: egui's disabled alpha left rows unreadable.
        s.on_surface_variant
    };
    let resp = crate::ui::row::painted_row_at(
        ui,
        id,
        &crate::ui::row::RowSpec {
            height: ROW_H,
            label,
            suffix: hint,
            enabled,
            hover_fill: s.container_high,
            fg,
            suffix_fg: Some(s.on_surface_variant),
            pad_x: ROW_PAD,
        },
    );
    // Accessible name: the row is custom-painted, so screen readers cannot
    // infer it from the shapes.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Thin grouped separator between menu sections (dimmer than `ui.separator`).
fn menu_separator(ui: &mut egui::Ui) {
    let s = theme::scheme_ref(ui);
    ui.add_space(5.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(0.0), 1.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, s.outline_variant);
    ui.add_space(5.0);
}

/// Transient confirmation (copy) at the bottom of the screen.
/// Auto-dismisses after ~2s (and requests a repaint so it vanishes on time).
pub(crate) fn show_toast(root: &mut egui::Ui, tab: &mut Tab) {
    let (text, at) = match (&tab.toast_text, &tab.toast_at) {
        (Some(t), Some(a)) => (t.clone(), *a),
        _ => return,
    };
    if at.elapsed() > std::time::Duration::from_secs(2) {
        tab.toast_text = None;
        tab.toast_at = None;
        return;
    }
    let ctx = root.ctx().clone();
    let tab_id = tab.id;
    egui::Area::new(egui::Id::new(("toast", tab_id)))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
        .order(egui::Order::Foreground)
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(text).strong());
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(300));
}

#[cfg(test)]
mod tests;
