// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::bookmarks`: the bookmark "quick access" grid on the Home page.
//!
//! The ONLY place bookmarks are listed (by design): they are not in the
//! toolbar, a menu or a page, so the new tab page stays the single home for
//! saved pages.
//!
//! - A responsive tile grid (newest first): each tile shows a stored custom
//!   glyph, else the site favicon when one resolves, else the first letter
//!   of the title on a stable colored circle.
//! - Clicking a tile (its icon or its name) navigates the SAME tab;
//!   Cmd/Ctrl+click asks `App` for a new tab (like a link).
//! - Editing happens ON the tile, with no extra window or page: the hover
//!   pencil renames it (the check saves; Enter or a click elsewhere commits,
//!   Esc drops the draft), dragging a tile reorders the grid and the hover X
//!   removes it. Every change goes straight to the encrypted store
//!   (debounced save).
//!
//! Nothing here touches the network or the store format directly: the
//! favicons come from the shared session cache (`media::favicon`), and the
//! store sanitizes every field on the way in.

use crate::bookmarks::{Bookmark, BookmarkStore};
use crate::browser::tab::Tab;
use crate::config::BOOKMARK_GRID_MAX_VISIBLE_ROWS;
use crate::media::favicon::FaviconCache;
use crate::ui::{icons, theme};
use crate::util::{strip_scheme_prefix, truncate_chars};
use eframe::egui;

/// Tile height (icon above a two-line title).
///
/// Sized so the name's TWO rows really fit under the icon: at
/// `TITLE_SIZE` the bundled Noto Sans metric is 1.362em per row
/// (~17.03px, `2 * 17.03 = 34.06px` for two rows). The old 92px tile
/// left the name only 30px, so the second row spilled ~2px past the
/// tile's bottom edge and the clip rect shaved its descenders while the
/// hover highlight (= this same rect) cut through the line.
const TILE_H: f32 = 96.0;
/// Section header: the title line's font size and the ribbon slot beside
/// it. The block reservation measures the real title line from this size,
/// so it can never undershoot the header and clip the grid below (see the
/// `allocate_ui_with_layout` in [`show_bookmarks`]).
/// Both rest on the shared type scale (`typography::ROW`/`HEADING`).
const HEADER_TITLE_SIZE: f32 = theme::typography::ROW;
const HEADER_ICON: f32 = theme::typography::HEADING;
/// Vertical nudge for the header ribbon. The line box centers the 18px slot
/// on the row, but the ribbon's ink is a touch taller than the 15pt caps
/// (~12.5px vs 11px), so it reads a hair high next to "Bookmarks" (measured
/// ink centers 0.75px apart). One point down lines the two optical centers
/// up (within a quarter pixel) and stays on the device-pixel grid, so the
/// strokes stay crisp at 1x and 2x.
const HEADER_ICON_DY: f32 = 1.0;
/// Target tile width the column count is computed from.
const TILE_TARGET_W: f32 = 104.0;
/// Gap between tiles, horizontally and vertically.
const GRID_GAP: f32 = 10.0;
/// Smallest / largest column count for any content width.
const MIN_COLS: usize = 2;
const MAX_COLS: usize = 6;
/// Floor for one tile's width: below `MIN_COLS * MIN_TILE_W + GRID_GAP` the
/// grid drops to a single column instead of overflowing the section.
const MIN_TILE_W: f32 = 40.0;
/// Icon circle diameter and its top offset inside the tile.
const ICON_SIZE: f32 = 48.0;
const ICON_TOP: f32 = 6.0;
/// Site favicon art side inside the circle: the app's one favicon size
/// (`icons::GLYPH`, the toolbar's painted glyph), so every favicon in the
/// UI reads the same.
const FAVICON: f32 = icons::GLYPH;
/// Top of the title area inside the tile: a 2px gap under the icon
/// (which ends at `ICON_TOP + ICON_SIZE` = 54) and enough room for the
/// two 17.03px rows with a bottom air margin (see `TILE_H`).
const TITLE_TOP: f32 = 58.0;
/// Title font size and display cap.
const TITLE_SIZE: f32 = 12.5;
const TITLE_CHARS: usize = 34;
/// Bottom guard of the title box: the text clip rect stops this far above
/// the tile's bottom edge, so a line a hair taller than the two measured
/// rows can never paint over the next grid row. Both title rows (and the
/// inline field) still fit inside it (see `TILE_H`).
const TITLE_BOTTOM_GUARD: f32 = 2.0;
/// Inline rename field: one text line plus this much air above and below,
/// centered in the name slot. The field used to fill the whole two-line
/// slot, which left a wide band of empty input around the single line.
const FIELD_PAD: f32 = 5.0;
/// Tile action buttons: 22px boxes STACKED at the top-right corner (the X
/// on top, the pencil/check below), inset from the tile edge.
const ACTION_SIZE: f32 = 22.0;
const ACTION_INSET: f32 = 2.0;
/// Vertical gap between the two stacked action buttons.
const ACTION_GAP: f32 = 2.0;

// Tile avatar hues live in `theme::AVATAR_PALETTE` (single source with the
// documented fixed-hue exception); `config::MAX_BOOKMARK_COLORS` keeps
// stored indexes in range (a test pins the two together).

/// Open inline rename, kept in egui memory so it survives frames without
/// being threaded through `App`: which tile, the draft and the pending-focus
/// flag. The commit is applied to the store by `show_bookmarks`; the draft
/// only drives the field.
#[derive(Clone)]
struct TileEdit {
    /// URL of the bookmark being renamed (the identity: indexes shift).
    url: String,
    /// Field draft.
    draft: String,
    /// Take keyboard focus (and select the draft) on the opening frame.
    focus: bool,
}

/// Draws the Home bookmark grid at `width` (the Home column's capsule
/// width), so the tiles line up with the address field above them.
///
/// Returns the URL of a tile the user Cmd/Ctrl+clicked ("open in a new
/// tab"): only `App` may create tabs, so the request travels up through
/// `home` and `viewer`. A plain click navigates this tab in place.
/// Drops the per-tab egui-memory state a closed Home tab parked (an open
/// inline rename, an in-flight tile drag). Ids are never reused: without
/// this the drafts would outlive every closed tab.
pub(crate) fn forget_tab_data(ctx: &egui::Context, tab_id: usize) {
    ctx.data_mut(|d| {
        d.remove::<Option<TileEdit>>(egui::Id::new(("bookmark-edit", tab_id)));
        d.remove::<Option<String>>(egui::Id::new(("bookmark-drag", tab_id)));
    });
    crate::ui::forget_scroll_state(ctx, crate::ui::BOOKMARKS_SCROLL_ID, tab_id);
}

pub fn show_bookmarks(
    ui: &mut egui::Ui,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
    width: f32,
) -> Option<String> {
    let count = store.entries().len();
    let edit_id = egui::Id::new(("bookmark-edit", tab.id));
    let drag_id = egui::Id::new(("bookmark-drag", tab.id));
    // The inline rename survives frames in egui memory; this local copy is
    // written back at the end of the pass. Stored as `Option<_>` in BOTH
    // directions: temp data is keyed by TYPE, so writing an `Option` while
    // reading a bare `TileEdit` would leave the old value in place (the
    // field reopened forever after a commit).
    let mut edit = ui
        .data_mut(|d| d.get_temp::<Option<TileEdit>>(edit_id))
        .flatten();
    // A tile removed outside the open field (its X, a reload) drops the
    // stale edit instead of applying it to a different bookmark.
    if edit
        .as_ref()
        .is_some_and(|state| !store.entries().iter().any(|b| b.url == state.url))
    {
        edit = None;
    }
    // The drag in flight (the dragged URL): also in egui memory, because it
    // spans frames. `Option` in BOTH directions (same type-keyed reason as
    // the edit above): a bare-`String` read of an `Option` write would pin
    // the drag forever.
    let mut drag_url = ui
        .data_mut(|d| d.get_temp::<Option<String>>(drag_id))
        .flatten();
    let mut drag_started: Option<String> = None;
    let mut drag_stopped = false;
    let mut drop_target: Option<usize> = None;
    let mut reorder: Option<(usize, usize)> = None;
    // Filled inside the widgets, applied after the store borrows end.
    let mut navigate: Option<String> = None;
    let mut open_new_tab: Option<String> = None;
    let mut remove: Option<usize> = None;
    let mut begin: Option<TileEdit> = None;
    let mut finish: Option<bool> = None;
    // Whether a tile (or one of its action buttons) owns the keyboard this
    // pass; reported on the tab so `App` leaves Alt+Left/Right to the grid.
    let mut tile_focused = false;
    // Real id of the grid's `ScrollArea`, recorded for `forget_tab` (see
    // `ui::remember_scroll`).
    let mut grid_scroll_id: Option<egui::Id> = None;

    let s = theme::scheme_ref(ui);
    // Top air on the spacing scale (24px = XL + M): the old bare 26.0 was
    // the one distance on the Home page that did not come from a token.
    ui.add_space(theme::spacing::XL + theme::spacing::M);
    // One fixed-width, centered block for the whole section: the Home
    // column centers a KNOWN-size child, so the header, the grid and the
    // empty hint line up with the address capsule above (a bare `scope`
    // spans the full column and hugs the left edge).
    let (cols, tile_w) = grid_geometry(width);
    // The floating scrollbar overlays the right edge when the grid scrolls:
    // the rightmost column's action buttons keep this lane clear so the bar
    // can neither cover nor steal their clicks.
    let lane = theme::scrollbar_width(ui);
    let rows = count.div_ceil(cols);
    // The header row's height (the taller of the ribbon slot and the title
    // line) PLUS both gaps that follow it: the item spacing after the row
    // and the explicit `spacing::S` air. They are part of the reservation
    // below, because the block must hand the grid its full height: a
    // 4px-short block left the ScrollArea's viewport 4px shorter than the
    // tile, which clipped the tile's bottom — the hover wash IS the tile
    // rect, so it showed as a cut hover highlight, and cut the rename
    // field's bottom edge.
    let header_h = ui
        .fonts_mut(|f| f.row_height(&egui::FontId::proportional(HEADER_TITLE_SIZE)))
        .max(HEADER_ICON);
    let header_block_h = header_h + ui.spacing().item_spacing.y + theme::spacing::S;
    // What the grid (or the empty hint line) really occupies.
    let content_h = if count == 0 {
        ui.text_style_height(&egui::TextStyle::Body)
    } else {
        let visible_rows = rows.min(BOOKMARK_GRID_MAX_VISIBLE_ROWS);
        TILE_H * visible_rows as f32 + GRID_GAP * visible_rows.saturating_sub(1) as f32
    };
    // The grid only scrolls past BOOKMARK_GRID_MAX_VISIBLE_ROWS rows. Below that the
    // scroll area still reports its trailing item gap as overflow, which
    // drew a full-height floating bar next to the last tile (and lit it up
    // on hover) with nothing to scroll: keep it hidden until it is needed.
    let grid_scrolls = rows > BOOKMARK_GRID_MAX_VISIBLE_ROWS;
    ui.allocate_ui_with_layout(
        egui::vec2(width, header_block_h + content_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            // Header: ribbon + section title + count.
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(HEADER_ICON, HEADER_ICON),
                    egui::Sense::hover(),
                );
                // Nudged onto the title's optical center (see
                // `HEADER_ICON_DY`): the slot stays in the layout where it
                // was, only the painted glyph moves.
                icons::paint(
                    ui,
                    rect.translate(egui::vec2(0.0, HEADER_ICON_DY)),
                    icons::Icon::Bookmark,
                    s.primary,
                );
                ui.label(theme::strong(
                    egui::RichText::new("Bookmarks")
                        .size(HEADER_TITLE_SIZE)
                        .color(s.on_surface),
                ));
                if count > 0 {
                    ui.label(theme::weak(ui, &count.to_string()));
                }
            });
            ui.add_space(theme::spacing::S);

            if count == 0 {
                ui.label(theme::weak(
                    ui,
                    &format!(
                        "No bookmarks yet - {} bookmarks the current page.",
                        crate::ui::BOOKMARK_SHORTCUT
                    ),
                ));
            } else {
                // Responsive, virtualized grid: `show_rows` lays out one
                // TILE row at a time, so a long list never builds every
                // tile each frame.
                // `show_rows` reads `item_spacing.y` for its row stride,
                // so the spacing here must be the grid gap.
                ui.spacing_mut().item_spacing = egui::vec2(GRID_GAP, GRID_GAP);
                let grid_out = egui::ScrollArea::vertical()
                    .id_salt((crate::ui::BOOKMARKS_SCROLL_ID, tab.id))
                    .max_width(width)
                    .max_height(content_h)
                    .auto_shrink([true, true])
                    .scroll_bar_visibility(if grid_scrolls {
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                    } else {
                        egui::scroll_area::ScrollBarVisibility::AlwaysHidden
                    })
                    .show_rows(ui, TILE_H, rows, |ui, range| {
                        for row in range {
                            ui.horizontal(|ui| {
                                for col in 0..cols {
                                    // Newest first: display position 0 is
                                    // the last entry of the insertion-
                                    // ordered store.
                                    let position = row * cols + col;
                                    if position >= count {
                                        break;
                                    }
                                    let index = count - 1 - position;
                                    // Defensive: a store mutation mid-frame
                                    // (a save landing, a rename) must never
                                    // panic a frame; skip the stale slot.
                                    let Some(bookmark) = store.entries().get(index) else {
                                        break;
                                    };
                                    let tile_edit =
                                        edit.as_mut().filter(|state| state.url == bookmark.url);
                                    let dragging =
                                        drag_url.as_deref() == Some(bookmark.url.as_str());
                                    match bookmark_tile(
                                        ui,
                                        &TileRef {
                                            bookmark,
                                            tile_w,
                                            dragging,
                                            drag_active: drag_url.is_some(),
                                            // Only the rightmost column can
                                            // sit under the floating bar.
                                            right_inset: if col + 1 == cols { lane } else { 0.0 },
                                            generation: store.generation(),
                                        },
                                        edit_id,
                                        tile_edit,
                                        favicons,
                                        &mut tile_focused,
                                    ) {
                                        TileAction::None => {}
                                        TileAction::Open { new_tab } => {
                                            if new_tab {
                                                open_new_tab = Some(bookmark.url.clone());
                                            } else {
                                                navigate = Some(bookmark.url.clone());
                                            }
                                        }
                                        TileAction::Remove => remove = Some(index),
                                        TileAction::Rename => {
                                            begin = Some(TileEdit {
                                                url: bookmark.url.clone(),
                                                draft: bookmark.title.clone().unwrap_or_default(),
                                                focus: true,
                                            });
                                        }
                                        TileAction::Save => finish = Some(true),
                                        TileAction::Move { delta } => {
                                            if let Some((from, to)) =
                                                move_indices(count, position, delta)
                                            {
                                                reorder = Some((from, to));
                                            }
                                        }
                                        TileAction::DragStart => {
                                            drag_started = Some(bookmark.url.clone());
                                        }
                                        TileAction::DragOver => drop_target = Some(position),
                                        TileAction::DragStop => drag_stopped = true,
                                        TileAction::Finish { commit } => finish = Some(commit),
                                    }
                                }
                            });
                        }
                    });
                grid_scroll_id = Some(grid_out.id);
            }
        },
    );

    // A failed background save must be visible where the bookmarks live.
    if let Some(error) = store.save_error() {
        ui.add_space(theme::spacing::XS);
        ui.label(egui::RichText::new(error).small().color(s.error));
    }

    // Apply the rename that just closed (Enter, the save check or a click
    // elsewhere commits; Esc drops the draft) BEFORE any removal: a
    // same-frame X keeps the index it was drawn with.
    if let Some(commit) = finish {
        if commit {
            if let Some(state) = &edit {
                if let Some(index) = store.entries().iter().position(|b| b.url == state.url) {
                    // A refusal (the rename would push the store past its
                    // serialized budget) leaves the old name: the store never
                    // silently drops a bookmark to make room, and a full
                    // bookmark store is a deliberate user state.
                    store.set_title(index, &state.draft);
                }
            }
        }
        edit = None;
    }
    // The pencil of another tile opens a fresh field (the previous draft was
    // applied above); the new edit wins over a finished one.
    // Write back while a field is open (the draft changes as the user types)
    // or when it just opened/closed; skip the idle closed write per frame.
    let edit_changed = finish.is_some() || begin.is_some();
    if let Some(state) = begin {
        edit = Some(state);
    }
    if edit.is_some() || edit_changed {
        ui.data_mut(|d| d.insert_temp(edit_id, edit.clone()));
    }

    // Drag reorder, applied LIVE: the tile moves while the pointer moves, so
    // the grid follows the drag instead of jumping on release. Display
    // positions are newest-first while the store is insertion-ordered, so
    // both ends are mirrored. Persist only on change (start/stop), not
    // every frame while hovering.
    let had_drag = drag_url.is_some();
    let drag_began = drag_started.is_some();
    if let Some(url) = drag_started {
        drag_url = Some(url);
    }
    if let (Some(target), Some(url)) = (drop_target, drag_url.as_deref()) {
        if let Some(from) = (0..count).find(|p| {
            store
                .entries()
                .get(count - 1 - p)
                .is_some_and(|b| b.url == url)
        }) {
            if from != target {
                reorder = Some((count - 1 - from, count - 1 - target));
            }
        }
    }
    if drag_stopped || !ui.input(|i| i.pointer.any_down()) {
        drag_url = None;
    }
    if drag_began || drag_stopped || had_drag != drag_url.is_some() {
        ui.data_mut(|d| d.insert_temp(drag_id, drag_url.clone()));
    }

    // Remember for the next frame whether a field is open: the shortcut
    // handler must leave Esc to it (same race as the omnibox address
    // field). Set AFTER the finish/begin handling, so the frame that closes
    // a field already reports it closed.
    tab.tile_editing = edit.is_some();
    // The grid reuses Alt+Left/Right on a focused tile; App's Back/Forward
    // shortcuts check this last-frame flag and leave the keys alone.
    tab.tile_focused = tile_focused;

    if let Some((from, to)) = reorder {
        store.move_entry(from, to);
    }
    if let Some(index) = remove {
        store.remove_entry(index);
    }
    if let Some(id) = grid_scroll_id {
        crate::ui::remember_scroll(ui.ctx(), crate::ui::BOOKMARKS_SCROLL_ID, tab.id, id);
    }

    if let Some(url) = navigate {
        tab.navigate_to_text(&url, true);
    }
    open_new_tab
}

/// Store indices for a keyboard reorder (`Alt+Left`/`Alt+Right`): `delta` is
/// in DISPLAY slots while the grid shows newest-first and the store is
/// insertion-ordered, so the two ends mirror (`store = last - display`).
/// `None` when the move would land on the tile itself or off the grid.
fn move_indices(count: usize, position: usize, delta: i32) -> Option<(usize, usize)> {
    let last = count.saturating_sub(1);
    let target = (position as i64 + i64::from(delta)).clamp(0, last as i64) as usize;
    let from = last - position.min(last);
    let to = last - target;
    (from != to).then_some((from, to))
}

/// Column count and tile width for a content width: targets
/// `TILE_TARGET_W` per tile, clamped so narrow windows keep two columns and
/// wide ones never grow hairline tiles. Below even two floor-width tiles
/// plus the gap it drops to ONE column instead of overflowing the section.
fn grid_geometry(width: f32) -> (usize, f32) {
    let width = if width.is_finite() {
        width.max(0.0)
    } else {
        0.0
    };
    let min_cols = if width < MIN_COLS as f32 * MIN_TILE_W + GRID_GAP {
        1
    } else {
        MIN_COLS
    };
    let cols = ((width + GRID_GAP) / (TILE_TARGET_W + GRID_GAP)).floor() as usize;
    let cols = cols.clamp(min_cols, MAX_COLS);
    let tile_w = ((width - GRID_GAP * (cols as f32 - 1.0)) / cols as f32).max(MIN_TILE_W);
    (cols, tile_w)
}

mod tile;

use tile::*;

#[cfg(test)]
mod tests;
