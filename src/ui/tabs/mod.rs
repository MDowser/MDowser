// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::tabs`: vertical tab rail.
//!
//! LAYOUT (stable by construction):
//! - Collapsed: a fixed 44px rail below the toolbar with one favicon per
//!   tab, drawn at the toolbar's icon size (~8px of breathing room each
//!   side). The page keeps almost the full window width.
//! - Hover: the rail expands to 248px as a floating overlay (no content
//!   reflow) showing favicon + title + close, plus a pinned `+ New Tab`.
//!   It collapses only when the pointer moves into the document area:
//!   moving up to the toolbar or leaving the window keeps it open.
//! - The expansion is ANIMATED (a short width tween clipped to the growing
//!   edge, labels fading AND drifting in behind it) instead of popping the
//!   full-width panel in one frame, which read as a flash. The curve is an
//!   ease-out with a longer tail than `cubic_out` (`ease_out`): it snaps open
//!   and settles gently. egui mirrors the easing when closing, so both
//!   directions settle the same way. At t == 0 the overlay is exactly the
//!   collapsed rail, so both directions start from the state the user already
//!   sees and nothing jumps. Labels hold back until the panel has some width,
//!   then cascade top to bottom with a short per-row delay.
//! - Rows have the SAME x/y/size in both states (same paddings, row height
//!   and gap): expanding only grows the row background to the right and
//!   reveals the title/close. Nothing jumps: labels drift into their shared
//!   column while fading, then rest there (see `label_slide`/`row_reveal`).
//! - Chrome icon size is shared with the toolbar (`icons::BUTTON` /
//!   `icons::GLYPH`): favicons and the rail's vector icons draw at the same
//!   size as the toolbar buttons, so the chrome reads as one set. The
//!   selected tab's accent bar lives in the rail's left gutter, clear of
//!   the row-filling favicon.
//! - The overlay shares the toolbar's tone and square top corners, so the
//!   two read as ONE chrome surface (no shadow above: it painted a dirty
//!   seam under the toolbar). Only the right edge is elevated: a soft
//!   penumbra in light mode, a hairline in dark mode (black blur over dark
//!   chrome reads as dirt). That separator is drawn in BOTH rail states —
//!   collapsed it is the smaller, fainter version — and it morphs with the
//!   open tween, so opening or closing never pops the shadow on or off.
//!   Action rows are the same borderless rows in both states.
//! - While the overlay covers the rail, the collapsed content is NOT drawn —
//!   pointer inside the overlay OR above it (the toolbar area, which keeps
//!   the overlay open): otherwise the old rows show through below/behind it,
//!   the two lists disagree on positions, and the full list was rebuilt under
//!   an opaque overlay for nothing.
//! - Two actions are pinned to the bottom in both states (same size, same
//!   offsets), so they never move: `History` at the very bottom with
//!   `+ New Tab` right above it (plus a separator in the expanded state).
//! - Favicons come from `media::favicon` (`config::FAVICON_PATHS`:
//!   `/favicon.ico`, `/favicon.png`, `/favicon.svg`); while unknown, the tab
//!   shows an hourglass icon (loading) or a home/globe icon.
//!
//! Selection/close/new-tab are returned via `TabsResponse` (one struct out,
//! mirrors `OmniboxResponse`) so `App` can mutate the `Vec<Tab>` without
//! fighting the borrow checker. The last expanded overlay rect is kept by
//! the caller to retain hover while the pointer is over the overlay (which
//! covers the collapsed rail's widgets).

use crate::browser::tab::{Tab, TabStateKind};
use crate::media::favicon::FaviconCache;
use crate::ui::{icons, theme};
use eframe::egui;
use url::Url;

/// Collapsed rail width: one toolbar-sized favicon + ~8px of breathing
/// room per side.
pub(crate) const RAIL_W: f32 = 44.0;
/// Expanded overlay width (hover).
const EXPANDED_W: f32 = 248.0;
/// Row height in both states.
const ROW_H: f32 = 36.0;
/// Vertical gap between rows (both states).
const ROW_GAP: f32 = 2.0;
/// Favicon (image) side: the toolbar's painted glyph size, so the rail's
/// favicons and the toolbar icons read as one set.
const FAVICON: f32 = icons::GLYPH;
/// Box the rail's VECTOR icons are painted in (`icons::paint` applies its
/// line-icon art margin inside): a toolbar button box, so their visible
/// glyph is exactly `FAVICON`, one toolbar glyph.
const ICON_BOX: f32 = icons::BUTTON;
/// Close button hit box in the expanded rows: the omnibox slot's box, so
/// the in-row X matches the in-field clear X.
const CLOSE_W: f32 = 26.0;
/// Shared horizontal inner margin (rail and overlay).
const PAD_X: f32 = 6.0;
/// Favicon center offset from the row's left edge (same in both states):
/// rows are `RAIL_W - 2 * PAD_X` wide, the icon is centered in them.
const ICON_CX: f32 = (RAIL_W - 2.0 * PAD_X) / 2.0;
/// Shared top/bottom inner margin.
const PAD_TOP: f32 = 8.0;
const PAD_BOTTOM: f32 = 8.0;
/// Corner radius of tab/action rows in both states.
const ROW_RADIUS: u8 = crate::ui::theme::radius::MENU;
/// Height of one pinned action row: the toolbar's row rhythm, so the
/// toolbar-sized icon breathes (the old 30px row left it nearly touching).
const ACTION_ROW_H: f32 = crate::config::CHROME_ROW_H;
/// Vertical gap between the two pinned action rows (explicit; the layout
/// zeroes egui's item spacing so this constant is the real gap).
const ACTION_GAP: f32 = 8.0;
/// Gap the expanded overlay adds between `+ New Tab` and the separator.
const ACTION_SEP_GAP: f32 = 6.0;
/// egui's fixed separator widget height (`SeparatorStyle::spacing`).
const ACTION_SEP_H: f32 = 6.0;
/// Height reserved at the bottom for BOTH pinned actions (both states):
/// `History` (bottom) plus `+ New Tab`, their explicit gap, and the expanded
/// overlay's gap + separator. Derived from the same constants the layout
/// uses, so the reserved height can never under-shoot the painted block
/// (under-reserving let the separator cut across the list).
const ACTION_H: f32 = 2.0 * ACTION_ROW_H + ACTION_GAP + ACTION_SEP_GAP + ACTION_SEP_H;
/// Open/close animation time of the hover overlay, in seconds. Slow enough
/// to read as a deliberate motion, fast enough to still feel like hover.
/// The easing (`ease_out`, quintic) spends most of this time on the gentle
/// tail, so the panel is close to its width early and settles soft.
const EXPAND_ANIM_SECS: f32 = 0.22;
/// Progress point (in the eased width `t`) where the labels start to appear:
/// they hold back until the panel already has some width, so the text never
/// shows in a strip too thin to read.
const TEXT_START: f32 = 0.25;
/// How far (px) a label drifts in while it fades: it starts this much toward
/// the growing edge and settles at its final column, so the reveal reads as
/// content flowing out of the rail instead of a bare alpha pop. Always at
/// its column when the panel is at rest (no leftover offset).
const TITLE_SLIDE: f32 = 8.0;
/// Extra per-row delay of the label reveal (in `t` units), and its cap: a
/// short cascade top-to-bottom, bounded so a long list never leaves the last
/// tab waiting. Every row still reaches full reveal exactly at `t == 1`.
const STAGGER: f32 = 0.05;
const STAGGER_MAX: f32 = 0.25;

/// `easeOutQuint` (<https://easings.net/#easeOutQuint>): a fast start with a
/// long, gentle tail, the shape a native sidebar's open motion has. Exact at
/// both ends (`0 -> 0`, `1 -> 1`), which the final-width geometry relies on
/// (an asymptotic curve such as `exponential_out` never quite reaches
/// `EXPANDED_W`). Used for the width tween; egui derives the close motion
/// from it too (`1 - ease(1 - v)`), so collapsing settles the same way.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

/// Shared label reveal curve: `0` until [`TEXT_START`], then `0..1`.
fn label_reveal(t: f32) -> f32 {
    ((t - TEXT_START) / (1.0 - TEXT_START)).clamp(0.0, 1.0)
}

/// [`label_reveal`] delayed by `index * STAGGER` (capped at [`STAGGER_MAX`]),
/// so titles cascade top-to-bottom instead of switching on together. Every
/// index reaches `1.0` at `t == 1`, so the settled layout is identical for
/// every row.
fn row_reveal(t: f32, index: usize) -> f32 {
    let start = TEXT_START + (index as f32 * STAGGER).min(STAGGER_MAX);
    ((t - start) / (1.0 - start)).clamp(0.0, 1.0)
}

/// What the tab rail reported this frame (mirrors `OmniboxResponse`:
/// one struct out instead of one out-param per action).
#[derive(Debug, Default)]
pub struct TabsResponse {
    /// A tab row was clicked (select it).
    pub select: Option<usize>,
    /// A close box or middle-click asked to close a tab.
    pub close: Option<usize>,
    /// The "+ New Tab" action row was clicked.
    pub new: bool,
    /// The "History" action row was clicked.
    pub history: bool,
}

/// Draws the tab rail on the root `Ui` (must run after the top toolbar).
///
/// `overlay_rect` is the previous frame's expanded-overlay rect (`None` when
/// collapsed); updated in place.
///
/// Takes `&mut [Tab]` because the per-frame display data (title, favicon
/// origin) is memoized on the tab (`Tab::display`); nothing else is mutated.
pub fn show_tabs(
    ui: &mut egui::Ui,
    tabs: &mut [Tab],
    active: usize,
    favicons: &mut FaviconCache,
    overlay_rect: &mut Option<egui::Rect>,
) -> TabsResponse {
    let s = theme::scheme_ref(ui);
    let mut out = TabsResponse::default();
    // Ensure every visible tab's origin has a favicon probe in flight (or a
    // cached texture). Cheap no-op after the first request per origin, and
    // borrowed from the per-tab display memo: no per-frame `Url` clone or
    // `Vec` allocation (the memo is rebuilt only when a tab changes).
    for tab in tabs.iter_mut() {
        if let Some(origin) = tab.display().0.origin.as_ref() {
            favicons.request(origin);
        }
    }

    // Overlay state from the previous frame. While the pointer is inside the
    // overlay, the collapsed content is skipped so nothing shows behind it.
    let pointer = ui.ctx().pointer_hover_pos();
    let overlay_was_open = overlay_rect.is_some();
    let inside_overlay = overlay_rect
        .as_ref()
        .is_some_and(|r| pointer.is_some_and(|p| r.contains(p)));
    let skip_content = overlay_was_open && inside_overlay;
    // The overlay covers the whole rail whenever it is open (and while it
    // closes), and a pointer ABOVE the overlay top keeps it open (the
    // toolbar shares its surface): in both cases the full collapsed list
    // would be rebuilt under an opaque overlay for nothing.
    let covered = skip_content
        || (overlay_was_open
            && pointer
                .zip(*overlay_rect)
                .is_some_and(|(p, r)| p.y < r.min.y));

    let mut rail_hovered = false;
    let panel = egui::Panel::left("tabs_rail")
        .resizable(false)
        .exact_size(RAIL_W)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: PAD_X as i8,
                    right: PAD_X as i8,
                    top: PAD_TOP as i8,
                    bottom: PAD_BOTTOM as i8,
                })
                .fill(s.container_low)
                .stroke(egui::Stroke::NONE),
        )
        .show(ui, |ui| {
            if covered {
                return;
            }
            let row_w = RAIL_W - 2.0 * PAD_X;
            // Same list/action split as the overlay: identical row geometry.
            let list_h = (ui.available_height() - ACTION_H).max(ROW_H);
            // Offset shared with the expanded overlay explicitly (the two
            // ScrollAreas have different parents, so `id_salt` alone does
            // NOT share state): expanding must not jump to a different row.
            let shared_id = egui::Id::new("tabs_shared_scroll");
            let shared_y = ui.data_mut(|d| d.get_temp::<f32>(shared_id).unwrap_or(0.0));
            let rail_scroll = egui::ScrollArea::vertical()
                .id_salt("tabs_scroll")
                .max_height(list_h)
                .auto_shrink([false, false])
                .vertical_scroll_offset(shared_y)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    // Rows carry their own ROW_GAP: egui's default item
                    // spacing (8px) is appended after every allocation and
                    // doubled the intended 2px gap, so fewer tabs fit.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, tab) in tabs.iter_mut().enumerate() {
                        let selected = i == active;
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::click());
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        rail_hovered |= resp.hovered();
                        draw_row_background(
                            ui,
                            rect,
                            selected,
                            resp.hovered(),
                            resp.has_focus(),
                            s,
                        );
                        let (disp, _rebuilt) = tab.display();
                        // Borrow the memo: the tab is not touched again in
                        // this iteration, so no per-frame title/origin clone
                        // is needed (the display cache is rebuilt only when
                        // the tab's state actually changes).
                        let title = disp.title.as_str();
                        let origin = disp.origin.as_ref();
                        draw_favicon(
                            ui,
                            rect,
                            disp.loading,
                            disp.kind,
                            origin,
                            favicons,
                            selected,
                        );
                        // Accessible name + selected state for screen readers
                        // (the row is a custom-painted widget).
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                selected,
                                title,
                            )
                        });
                        let resp = crate::ui::hover_tooltip(resp, title);
                        if resp.clicked() {
                            out.select = Some(i);
                        }
                        // Middle-click closes the tab.
                        if resp.clicked_by(egui::PointerButton::Middle) {
                            out.close = Some(i);
                        }
                        ui.add_space(ROW_GAP);
                    }
                });
            ui.data_mut(|d| d.insert_temp(shared_id, rail_scroll.state.offset.y));
            // Pinned action rows (same geometry as the overlay's). `History`
            // is the very bottom row; `+ New Tab` sits right above it.
            // Bottom_up layout stacks in insertion order, so History goes
            // in first.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                // Zero item spacing: the gaps below are explicit constants
                // (and `ACTION_H` is derived from exactly them).
                ui.spacing_mut().item_spacing.y = 0.0;
                rail_hovered |= action_row(
                    ui,
                    row_w,
                    s,
                    icons::Icon::History,
                    None,
                    1.0,
                    "History",
                    || format!("History ({})", crate::ui::HISTORY_SHORTCUT),
                    &mut out.history,
                );
                ui.add_space(ACTION_GAP);
                rail_hovered |= action_row(
                    ui,
                    row_w,
                    s,
                    icons::Icon::Plus,
                    None,
                    1.0,
                    "New tab",
                    || "New tab (Ctrl/Cmd+T)".to_string(),
                    &mut out.new,
                );
            });
        });

    // The panel covers its gutters, not just the rows.
    rail_hovered |= panel.response.hovered();
    let rail_rect = panel.response.rect;
    // Height available below the rail top. `viewport_rect()` is the LOCAL
    // viewport rect (origin 0,0): `viewport().inner_rect` is in global
    // screen coordinates and must not be mixed with local layout rects.
    let viewport = ui.ctx().input(|i| i.viewport_rect());
    let total_h =
        (viewport.max.y - rail_rect.min.y).max(PAD_TOP + PAD_BOTTOM + ROW_H + ACTION_H + 4.0);

    // Close only when the pointer is in the document area. Above the rail
    // top is the toolbar (same chrome surface as the overlay) and `None`
    // means the pointer left the window: neither collapses the overlay.
    // Gated on `overlay_was_open` so a collapsed rail never opens by itself.
    let keep_open =
        overlay_was_open && (pointer.is_none() || pointer.is_some_and(|p| p.y < rail_rect.min.y));
    let open = skip_content || rail_hovered || keep_open;
    // Expansion animation. Queried EVERY frame (collapsed ones included) so
    // the id is already primed at 0 when the pointer first reaches the rail:
    // the animation manager returns the target on its first call, which
    // would make that first hover pop.
    let t = ui.ctx().animate_bool_with_time_and_easing(
        egui::Id::new("tabs_rail_expand"),
        open,
        EXPAND_ANIM_SECS,
        ease_out,
    );
    if !open && t <= 0.0 {
        *overlay_rect = None;
        // Collapsed: the separator STAYS — smaller and fainter — so closing
        // the overlay never pops the shadow off. It goes on its own Middle
        // layer (a plain layer painter, NOT an `Area`: a fresh area runs a
        // sizing pass, hidden for one frame, and this edge must not blink).
        let edge = egui::Rect::from_min_max(rail_rect.min, rail_rect.max);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("tabs_edge_separator"),
        ));
        paint_edge_separator(&painter, ui.visuals().dark_mode, edge, s, 0.0);
        // Return the COLLAPSED content's actions, not a fresh default: a
        // keyboard user can focus a tab with Tab and activate it with
        // Enter/Space (egui synthesizes a click, and no hover ever set
        // `open`), and that click must not be discarded here.
        return out;
    }
    // Animated width and label reveal. At `t == 0` the overlay is exactly the
    // collapsed rail (`RAIL_W`), so the motion starts from the layout the
    // user already sees. The labels of the pinned actions hold back until the
    // panel has some width, then fade in while drifting to their column; the
    // tab rows do the same one beat after the row above them (see
    // `row_reveal`), so the list cascades instead of switching on at once.
    let width = egui::lerp(RAIL_W..=EXPANDED_W, t);
    let text_t = label_reveal(t);
    // Curtain clip: everything the overlay paints (background included) is
    // clipped to `width`, so the panel grows out of the rail instead of
    // flashing its final size in one frame.
    let clip = egui::Rect::from_min_max(
        rail_rect.min,
        egui::pos2(rail_rect.min.x + width, rail_rect.min.y + total_h),
    );

    // The overlay must read as an EXTENSION of the toolbar, not a floating
    // card: same tone, square corners, and no shadow above (the frame
    // shadow's top half painted a gray seam under the toolbar). Only the
    // right edge is elevated (`paint_edge_separator`).
    let area = egui::Area::new(egui::Id::new("tabs_overlay"))
        .order(egui::Order::Middle)
        .fixed_pos(rail_rect.min)
        // The built-in area fade (opacity over `animation_time`) would stack
        // under the width tween and briefly ghost the rail's icons (the
        // overlay is semi-transparent over an already-skipped rail content).
        // The width tween is the animation.
        .fade_in(false)
        .show(ui.ctx(), |ui| {
            let screen_clip = ui.clip_rect();
            ui.set_clip_rect(clip.intersect(screen_clip));
            egui::Frame::new()
                .fill(s.container_low)
                // Square corners on all four sides: the panel is flush with
                // the toolbar (top) and the window edges (left/bottom); any
                // radius would open a notch against the page.
                .corner_radius(egui::CornerRadius::ZERO)
                // No stroke: a hard outline here looked like a black frame.
                .stroke(egui::Stroke::NONE)
                .inner_margin(egui::Margin {
                    left: PAD_X as i8,
                    right: PAD_X as i8,
                    top: PAD_TOP as i8,
                    bottom: PAD_BOTTOM as i8,
                })
                .show(ui, |ui| {
                    let row_w = EXPANDED_W - 2.0 * PAD_X;
                    ui.set_min_width(row_w);
                    ui.set_max_width(row_w);
                    // Fixed height: rows start at the top and the action row
                    // stays glued to the bottom, like the collapsed rail.
                    let content_h = total_h - PAD_TOP - PAD_BOTTOM;
                    ui.set_min_height(content_h);
                    ui.set_max_height(content_h);
                    let list_h = (content_h - ACTION_H).max(ROW_H);
                    // Shared offset written by the rail just above this
                    // frame (see there): expanding keeps the same rows in
                    // view. Written back after user scrolling the overlay.
                    let shared_id = egui::Id::new("tabs_shared_scroll");
                    let shared_y = ui.data_mut(|d| d.get_temp::<f32>(shared_id).unwrap_or(0.0));
                    let overlay_scroll = egui::ScrollArea::vertical()
                        .id_salt("tabs_scroll")
                        .max_height(list_h)
                        .auto_shrink([false, false])
                        .vertical_scroll_offset(shared_y)
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        )
                        .show(ui, |ui| {
                            // Same zeroed spacing as the collapsed rail:
                            // both states must show identical row density.
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (i, tab) in tabs.iter_mut().enumerate() {
                                expanded_row(
                                    ui,
                                    i,
                                    tab,
                                    i == active,
                                    row_w,
                                    row_reveal(t, i),
                                    favicons,
                                    &mut out,
                                );
                                ui.add_space(ROW_GAP);
                            }
                        });
                    ui.data_mut(|d| d.insert_temp(shared_id, overlay_scroll.state.offset.y));
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        // Same borderless action rows as the collapsed rail
                        // (plus their labels), so both states agree. History
                        // is the bottom row, New Tab right above it. Item
                        // spacing is zeroed and the gaps are the shared
                        // constants `ACTION_H` is derived from.
                        ui.spacing_mut().item_spacing.y = 0.0;
                        action_row(
                            ui,
                            row_w,
                            s,
                            icons::Icon::History,
                            Some("History"),
                            text_t,
                            "History",
                            || format!("History ({})", crate::ui::HISTORY_SHORTCUT),
                            &mut out.history,
                        );
                        ui.add_space(ACTION_GAP);
                        action_row(
                            ui,
                            row_w,
                            s,
                            icons::Icon::Plus,
                            Some("New Tab"),
                            text_t,
                            "New tab",
                            || "New tab (Ctrl/Cmd+T)".to_string(),
                            &mut out.new,
                        );
                        ui.add_space(ACTION_SEP_GAP);
                        ui.separator();
                    });
                });
            // The right-edge separator spills a few px over the page, so it
            // is painted OUTSIDE the curtain clip, at the ANIMATED edge (the
            // frame's own rect is full width from the first frame). Same
            // rect the collapsed state paints, so the hand-off at t == 0 is
            // seamless.
            ui.set_clip_rect(screen_clip);
            let edge = egui::Rect::from_min_max(
                rail_rect.min,
                egui::pos2(rail_rect.min.x + width, rail_rect.max.y),
            );
            paint_edge_separator(ui.painter(), ui.visuals().dark_mode, edge, s, t);
        });
    *overlay_rect = Some(area.response.rect);
    out
}

mod rows;

use rows::*;

#[cfg(test)]
mod tests;
