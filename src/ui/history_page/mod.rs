// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::history_page`: the internal `about:history` page.
//!
//! Rendered in the central content panel exactly like a document sheet: the
//! same reading column, spacing and palette, so the local history opens as a
//! page INSIDE the browser instead of a floating window.
//!
//! - Searchable list of the local visits (newest first).
//! - Clicking a row navigates the SAME tab showing the page to that visit
//!   (`App` opens the page in a new tab, so the previous page is never
//!   replaced and Back returns to the list).
//! - The X removes one visit; the footer can delete everything (two clicks).
//! - State that must survive frames (the search text, the filtered-index
//!   cache and the armed delete deadline) lives in egui memory under fixed
//!   ids, so nothing has to be threaded through `App`.
//!
//! Nothing here touches the network or the encrypted store format: the page
//! only reads `HistoryStore` and mutates it through its public API. Every
//! remote string is already sanitized/bounded by the store.

use crate::browser::tab::Tab;
use crate::history::{format_age, HistoryStore, Visit};
use crate::ui::{icons, theme};
use crate::util::{strip_scheme_prefix, truncate_chars};
use eframe::egui;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// History row height (two text lines).
const ROW_H: f32 = 46.0;
/// Space reserved at the right of a row for the remove button.
const ROW_ACTION_W: f32 = 30.0;
/// Visual gap between the right-aligned age label and the remove button.
const AGE_GAP: f32 = 6.0;
/// Max chars of a row title before the ellipsis (display bound).
const ROW_TITLE_CHARS: usize = crate::config::ROW_TITLE_CHARS;
/// Max chars of a row URL before the ellipsis (display bound).
const ROW_URL_CHARS: usize = crate::config::ROW_URL_CHARS;
/// Height reserved below the list for the footer (count + delete + gutter
/// + bottom breathing room).
const FOOTER_H: f32 = 60.0;
/// Fixed width of the footer's destructive button: both labels
/// ("Delete history now" / "Confirm delete") fit, so arming the two-click
/// confirmation never resizes it (or nudges the footer). Sized against the
/// bundled proportional primary (Noto Sans): the narrower
/// 164px value was tuned for epaint's default face and left "Delete history
/// now" ~0.7px over, which grew the button. The page's own test pins this.
const DELETE_W: f32 = 168.0;
/// Seconds the "Delete history now" button stays armed.
const CLEAR_CONFIRM_SECS: u64 = crate::config::DELETE_CONFIRM_SECS;

/// Cached filtered indices, newest-first: `(store generation, RAW filter
/// text, lowercase needle, rows)`. Rebuilt only when the query or the store
/// changes; the 20000-entry scan used to re-lowercase every URL and title
/// each frame, and even a hit paid a `to_lowercase()` before comparing.
/// Stored behind an `Arc` so a cache HIT clones the pointer (a bare clone
/// copied the whole index, ~160KB at the entry cap, every frame).
type FilterCache = (u64, String, String, Vec<usize>);

/// Drops the per-tab egui-memory state a closed `about:history` tab parked
/// (search text, filter cache, armed delete, and the search field's own
/// edit state, which lives in the persisted store). Ids are never reused, so
/// without this every closed history tab would retain its cache forever.
pub(crate) fn forget_tab_data(ctx: &egui::Context, tab_id: usize) {
    ctx.data_mut(|d| {
        d.remove::<String>(egui::Id::new(("history-page-filter", tab_id)));
        d.remove::<std::sync::Arc<FilterCache>>(egui::Id::new(("history-page-cache", tab_id)));
        d.remove::<Option<Instant>>(egui::Id::new(("history-page-clear", tab_id)));
        d.remove::<egui::text_edit::TextEditState>(egui::Id::new(("history-page-search", tab_id)));
    });
    crate::ui::forget_scroll_state(ctx, crate::ui::HISTORY_SCROLL_ID, tab_id);
}

/// Draws the internal history page. `gutter` is the sheet's responsive
/// content gutter (`theme::content_gutter`), applied inside the content like
/// a document's so the sheet stays flush with the rail and the window edge.
pub fn show_history_page(ui: &mut egui::Ui, tab: &mut Tab, store: &mut HistoryStore, gutter: f32) {
    let s = theme::scheme_ref(ui);
    // The reading column is an EXPLICITLY sized child Ui. Inside a plain
    // `ui.horizontal` the nested `ui.vertical` only gets the row's initial
    // height (`interact_size.y`), so `ui.available_height()` collapsed to 0:
    // the virtualized list was squeezed to one visible row and the footer
    // fell off the sheet. `max_rect` here is the full sheet, so the list can
    // own everything between header and footer.
    let page = ui.max_rect();
    let inner = (page.width() - 2.0 * gutter).max(0.0);
    let max_w = theme::content_max_width(inner);
    let col_w = max_w.min(inner).max(0.0);
    let pad = ((inner - col_w) / 2.0).max(0.0);
    let col_rect = egui::Rect::from_min_max(
        egui::pos2(page.min.x + gutter + pad, page.min.y),
        egui::pos2(page.min.x + gutter + pad + col_w, page.max.y),
    );

    // Filled inside the closure, applied after the borrows end.
    let mut navigate: Option<String> = None;
    let mut remove: Option<usize> = None;

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(col_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.add_space(gutter);
            // Page title, styled like a document H1.
            ui.label(theme::strong(
                egui::RichText::new("History")
                    .size(30.0)
                    .color(s.on_surface),
            ));
            ui.add_space(10.0);

            // Search field (kept across frames in egui memory, per tab so
            // two open history tabs never share a query).
            let filter_id = egui::Id::new(("history-page-filter", tab.id));
            let mut filter = ui.data_mut(|d| d.get_temp::<String>(filter_id).unwrap_or_default());
            let search = ui.add_sized(
                [ui.available_width().max(80.0), 30.0],
                egui::TextEdit::singleline(&mut filter)
                    .id(egui::Id::new(("history-page-search", tab.id)))
                    .char_limit(crate::config::MAX_HISTORY_FILTER_CHARS)
                    .hint_text("Search history")
                    // TextEdit defaults to LEFT_TOP: a fixed 30pt box would
                    // sit the text at its top edge (every other single-line
                    // field in the app centers explicitly).
                    .vertical_align(egui::Align::Center)
                    .margin(egui::Margin::symmetric(8, 5)),
            );
            search.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search history")
            });
            if search.changed() {
                ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            }
            ui.add_space(10.0);
            theme::rule(ui);
            ui.add_space(theme::spacing::S);

            // Rebuild the filtered index list only when the query or the
            // store changed (re-lowercasing every URL/title per frame made
            // scrolling a full list stutter).
            let generation = store.generation();
            let cache_id = egui::Id::new(("history-page-cache", tab.id));
            let cached = ui.data_mut(|d| d.get_temp::<std::sync::Arc<FilterCache>>(cache_id));
            let cache: std::sync::Arc<FilterCache> = match cached {
                // Compare the RAW filter on a hit (no per-frame String).
                Some(cache) if cache.0 == generation && cache.1 == filter => cache,
                _ => {
                    let needle = filter.trim().to_lowercase();
                    let rows: Vec<usize> = store
                        .entries()
                        .iter()
                        .enumerate()
                        .rev()
                        .filter(|(_, visit)| matches_filter(visit, &needle))
                        .map(|(index, _)| index)
                        .collect();
                    let cache = std::sync::Arc::new((generation, filter.clone(), needle, rows));
                    ui.data_mut(|d| {
                        d.insert_temp(cache_id, cache.clone());
                    });
                    cache
                }
            };
            let needle: &str = &cache.2;
            let rows: &[usize] = &cache.3;

            // The list owns the height left below the header and above the
            // footer (header/footer stay put; only the list scrolls).
            let list_h = (ui.available_height() - FOOTER_H - gutter).max(ROW_H);
            if rows.is_empty() {
                ui.add_space((list_h * 0.35).min(60.0));
                ui.vertical_centered(|ui| {
                    let message = if needle.is_empty() {
                        "No history yet"
                    } else {
                        "No matches"
                    };
                    ui.label(theme::weak(ui, message));
                });
                ui.add_space((list_h * 0.35).min(60.0));
            } else {
                // Virtualized list: only the visible rows are built each
                // frame. `show_rows` reads `item_spacing.y` for its row
                // stride, so the scoped spacing must be zero.
                let now = now_unix();
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let list_out = egui::ScrollArea::vertical()
                        .id_salt((crate::ui::HISTORY_SCROLL_ID, tab.id))
                        .max_height(list_h)
                        .auto_shrink([false, true])
                        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                            for row in range {
                                let index = rows[row];
                                let visit = &store.entries()[index];
                                match history_row(ui, index, visit, now) {
                                    RowAction::Open => navigate = Some(visit.url.clone()),
                                    RowAction::Remove => remove = Some(index),
                                    RowAction::None => {}
                                }
                            }
                        });
                    crate::ui::remember_scroll(
                        ui.ctx(),
                        crate::ui::HISTORY_SCROLL_ID,
                        tab.id,
                        list_out.id,
                    );
                });
            }

            // Footer: entry count on the left, two-click delete on the right.
            ui.add_space(theme::spacing::S);
            ui.separator();
            ui.add_space(theme::spacing::XS);
            ui.horizontal(|ui| {
                let count = store.entries().len();
                let label = if count == 1 {
                    "1 page".to_string()
                } else {
                    format!("{count} pages")
                };
                ui.label(theme::weak(ui, &label));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Salted by tab id: with two about:history pages open,
                    // arming "Delete history now" in one must not show the
                    // confirmation (and accept a second click) in the other.
                    let clear_id = egui::Id::new(("history-page-clear", tab.id));
                    let mut confirm_until =
                        ui.data_mut(|d| d.get_temp::<Option<Instant>>(clear_id).flatten());
                    let confirming = confirm_until.is_some_and(|until| until > Instant::now());
                    let label = if confirming {
                        "Confirm delete"
                    } else {
                        "Delete history now"
                    };
                    if delete_button(ui, label).clicked() {
                        if confirming {
                            store.clear();
                            confirm_until = None;
                        } else {
                            confirm_until = Some(crate::config::deadline_after(CLEAR_CONFIRM_SECS));
                        }
                    }
                    ui.data_mut(|d| d.insert_temp(clear_id, confirm_until));
                    if confirm_until.is_some_and(|until| until > Instant::now()) {
                        ui.label(theme::weak(
                            ui,
                            "Click again to delete every visit and the key.",
                        ));
                        // Keep the armed label from going stale without input.
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                    }
                });
            });
            ui.add_space(gutter + 8.0);
        },
    );

    if let Some(index) = remove {
        store.remove_entry(index);
    }
    if let Some(url) = navigate {
        tab.navigate_to_text(&url, true);
    }
    // Keep the relative ages fresh while the page is visible.
    ui.ctx().request_repaint_after(Duration::from_secs(60));
}

/// Row outcome for the history list.
enum RowAction {
    /// Nothing happened.
    None,
    /// Navigate the tab showing the page to this visit.
    Open,
    /// Remove this visit from the store.
    Remove,
}

/// Case-insensitive match against URL and title.
///
/// Delegates to the omnibox's shared comparator: ASCII candidates compare
/// in place (the old unconditional `to_lowercase()` allocated up to two
/// Strings per row, i.e. tens of thousands per keystroke on a full store),
/// non-ASCII keeps the exact Unicode path.
fn matches_filter(visit: &Visit, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    crate::suggest::matches_candidate(&visit.url, visit.title.as_deref(), needle)
}

/// Right-aligned anchor of one row's age label. Kept at least [`AGE_GAP`]
/// away from the remove button: the two used to overlap because the 24px
/// button box overflowed the reserved `ROW_ACTION_W` column by 6px. `lane`
/// is the floating scrollbar's width, reserved so the bar never covers the
/// button or steals its clicks.
fn age_anchor(rect: egui::Rect, lane: f32) -> egui::Pos2 {
    egui::pos2(
        rect.max.x - ROW_ACTION_W - AGE_GAP - lane,
        rect.min.y + 14.5,
    )
}

/// Hit/paint box of one row's remove button, centered inside the reserved
/// action column (plus the scrollbar lane) so it never reaches into the
/// title/age/URL area and the floating bar never overlaps it.
fn remove_button_rect(rect: egui::Rect, lane: f32) -> egui::Rect {
    egui::Rect::from_center_size(
        egui::pos2(rect.max.x - ROW_ACTION_W * 0.5 - lane, rect.center().y),
        egui::vec2(24.0, 24.0),
    )
}

/// The footer's delete button at its fixed width: `min_size` (not
/// `add_sized`, which would also fix the height) centers the label in a frame
/// that never changes size when the two-click confirmation swaps the text.
fn delete_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).min_size(egui::vec2(DELETE_W, 0.0)))
}

/// One visit row: title + age on the first line, URL below, remove X on the
/// right. Clicking the row body opens the page.
fn history_row(ui: &mut egui::Ui, index: usize, visit: &Visit, now: u64) -> RowAction {
    let s = theme::scheme_ref(ui);
    let width = ui.available_width().max(80.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    // `contains_pointer` (not `hovered`): the remove button is a separate
    // widget ON TOP, and egui's hover set holds only the topmost one, so the
    // row wash used to blink off the moment the pointer reached the X (the
    // bookmark tile already keeps its highlight the same way).
    if response.contains_pointer() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::MENU),
            s.container_high,
        );
    } else if response.has_focus() {
        theme::focus_ring_with(ui, rect, theme::radius::MENU, s);
    }

    // First line: title (or the URL when the page had none).
    let title = match &visit.title {
        Some(title) => std::borrow::Cow::Borrowed(title.as_str()),
        None => strip_scheme_prefix(&visit.url),
    };
    let title = truncate_chars(&title, ROW_TITLE_CHARS);
    // Accessible name for the custom-painted row (the labels are not
    // interactive themselves). The URL belongs in it: a screen reader must
    // be able to tell two visits to the same title apart. Built inside the
    // closure by accesskit, never per frame.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{title} — {}", strip_scheme_prefix(&visit.url)),
        )
    });
    let text_left = rect.min.x + 10.0;
    let lane = theme::scrollbar_width(ui);
    let action_left = rect.max.x - ROW_ACTION_W - lane;
    // Reserve the age column so title and age never collide.
    let age_w = 82.0;
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.min.y + 5.0),
        egui::pos2(
            (action_left - age_w).max(text_left + 40.0),
            rect.min.y + 24.0,
        ),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(theme::strong(
                    egui::RichText::new(title).color(s.on_surface),
                ))
                .truncate()
                .selectable(false),
            );
        },
    );
    ui.painter().text(
        age_anchor(rect, lane),
        egui::Align2::RIGHT_CENTER,
        format_age(now, visit.visited_at),
        egui::FontId::proportional(theme::typography::EYEBROW),
        s.on_surface_variant,
    );

    // Second line: the schemeless URL.
    let stripped = strip_scheme_prefix(&visit.url);
    let url = truncate_chars(&stripped, ROW_URL_CHARS);
    let url_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.min.y + 24.0),
        egui::pos2(action_left, rect.min.y + 43.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(url_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(url)
                        .size(theme::typography::UI_COMPACT)
                        .color(s.on_surface_variant),
                )
                .truncate()
                .selectable(false),
            );
        },
    );

    let remove_rect = remove_button_rect(rect, lane);
    // Per-row accessible name: every row's X used the same static label, so
    // a screen reader listed N identical "Remove from history" buttons. The
    // name is rebuilt from the visit (the display title was already moved
    // into its label) and only when accesskit asks.
    let remove_name = || {
        let name = visit.title.as_deref().unwrap_or(visit.url.as_str());
        format!("Remove {name} from history")
    };
    let remove = icons::icon_button_at_named_lazy(
        ui,
        remove_rect,
        ("history-remove", index),
        icons::Icon::Close,
        remove_name,
        || "Remove from history".to_string(),
        true,
    );
    if remove.clicked() {
        return RowAction::Remove;
    }
    if response.clicked() {
        // Defensive: if the remove hit test lost to the row (overlapping
        // widgets), a click on the X must never open the page.
        if response
            .interact_pointer_pos()
            .is_some_and(|pos| remove_rect.contains(pos))
        {
            return RowAction::None;
        }
        return RowAction::Open;
    }
    RowAction::None
}

/// Current Unix time in seconds (0 on a pre-epoch clock).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
