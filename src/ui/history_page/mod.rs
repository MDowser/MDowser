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
//! - Grouped list of the local visits (newest first): rolling date sections
//!   (Today / Yesterday / Previous 7 days / Previous 30 days / Older), one
//!   favicon per row (the shared `FaviconCache`, like the Home tiles; a
//!   globe or file glyph while it loads or when there is none) and each
//!   row's relative age plus a remove X.
//! - Capsule search field (magnifier + clear X) filtering URL and title; the
//!   footer reports the filtered count ("N of M pages").
//! - Clicking a row navigates the SAME tab showing the page to that visit
//!   (`App` opens the page in a new tab, so the previous page is never
//!   replaced and Back returns to the list).
//! - The X removes one visit; the footer can delete everything (two clicks).
//! - State that must survive frames (the search text, the filtered-index
//!   cache and the armed delete deadline) lives in egui memory under fixed
//!   ids, so nothing has to be threaded through `App`.
//!
//! Nothing here touches the network or the encrypted store format: the page
//! only reads `HistoryStore` and mutates it through its public API, and
//! favicons resolve through the shared session cache (which owns the probe
//! lifecycle). Every remote string is already sanitized/bounded by the store.

use crate::browser::tab::Tab;
use crate::history::{format_age, HistoryStore, Retention, Visit};
use crate::media::favicon::FaviconCache;
use crate::ui::{icons, theme};
use crate::util::{strip_scheme_prefix, truncate_chars};
use eframe::egui;
use std::time::{Duration, Instant};

/// History row height (favicon + two text lines).
const ROW_H: f32 = 52.0;
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
/// Height of the capsule search field (room for the magnifier, one text
/// line and the clear button, at the chrome's control measure).
const SEARCH_H: f32 = 36.0;
/// Inner padding of the search capsule (left of the magnifier, right of the
/// clear button).
const CAPSULE_PAD: f32 = 6.0;
/// Slot side of the capsule's magnifier.
const CAPSULE_ICON: f32 = 24.0;
/// Slot side of the capsule's clear button. ALWAYS reserved, so typing the
/// first character never shifts the text. The shared chrome icon box.
const CAPSULE_CLEAR: f32 = crate::ui::theme::ICON_BOX;
/// Side of the favicon art inside a row (the toolbar glyph would dominate
/// the two text lines; the slot below keeps the text clear of it).
const FAVICON_ART: f32 = 20.0;
/// Width of the favicon slot at the left of a row (art plus its lane).
const FAVICON_SLOT: f32 = 28.0;
/// Gap between the favicon slot and the text column.
const FAVICON_GAP: f32 = 8.0;
/// Reserve the age column so title and age never collide.
const AGE_W: f32 = 82.0;
/// Left inset of a row's favicon slot (and of the section header label).
const ROW_INSET: f32 = 10.0;
/// Seconds of age granularity shared by the date sections and the row age
/// labels: both are derived from the same tick, so a row can never read
/// "yesterday" inside the Today section.
const AGE_TICK_SECS: u64 = 60;

/// Cached filtered rows + their date sections, one per `about:history` tab.
///
/// Rebuilt only when the query, the store revision or the age tick changes;
/// the 20000-entry scan used to re-lowercase every URL and title each frame,
/// and even a hit paid a `to_lowercase()` before comparing. Stored behind an
/// `Arc` so a cache HIT clones the pointer (a bare clone copied the whole
/// index, ~160KB at the entry cap, every frame).
struct FilterCache {
    /// Store revision the cache was built on.
    generation: u64,
    /// RAW filter text: compared on a hit, so a settled frame builds no
    /// `String`.
    raw: String,
    /// Trimmed, lowercased needle the rows were filtered with.
    needle: String,
    /// Age tick (`now / AGE_TICK_SECS`) the sections and labels belong to.
    tick: u64,
    /// Filtered store indices, newest first.
    rows: Vec<usize>,
    /// Contiguous date sections of `rows`, in order.
    sections: Vec<Section>,
    /// Flat slot of each section's header inside the virtualized list.
    offsets: Vec<usize>,
    /// Slots the virtualized list owns (headers + rows).
    flat_total: usize,
}

impl FilterCache {
    /// The one O(n) scan this page ever runs: filters `entries` and folds
    /// the result into date sections.
    fn build(entries: &[Visit], generation: u64, filter: &str, tick: u64) -> Self {
        let needle = filter.trim().to_lowercase();
        let rows: Vec<usize> = entries
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, visit)| matches_filter(visit, &needle))
            .map(|(index, _)| index)
            .collect();
        let sections = build_sections(entries, &rows, tick * AGE_TICK_SECS);
        let mut offsets = Vec::with_capacity(sections.len());
        let mut flat_total = 0usize;
        for section in &sections {
            offsets.push(flat_total);
            flat_total += 1 + section.len;
        }
        Self {
            generation,
            raw: filter.to_string(),
            needle,
            tick,
            rows,
            sections,
            offsets,
            flat_total,
        }
    }

    /// Unix seconds the sections and the row labels are built for
    /// (tick-aligned, so both agree).
    fn now(&self) -> u64 {
        self.tick * AGE_TICK_SECS
    }
}

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
///
/// `favicons` is the shared session cache (the probe lifecycle stays there):
/// ready textures paint per visible row, anything else falls back to a
/// globe/file glyph. Safe to call every frame: settled origins cost one hash
/// probe per visible row.
pub fn show_history_page(
    ui: &mut egui::Ui,
    tab: &mut Tab,
    store: &mut HistoryStore,
    favicons: &mut FaviconCache,
    gutter: f32,
) {
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
            // Page title, styled like a document H1, with a live subtitle
            // (total count + retention window) underneath.
            ui.label(theme::strong(
                egui::RichText::new("History")
                    .size(theme::typography::PAGE_TITLE)
                    .color(s.on_surface),
            ));
            let total = store.entries().len();
            with_header_label(total, store.retention(), |subtitle| {
                ui.label(theme::weak(ui, subtitle));
            });
            ui.add_space(theme::spacing::M);

            // Search capsule (kept across frames in egui memory, per tab so
            // two open history tabs never share a query).
            let filter_id = egui::Id::new(("history-page-filter", tab.id));
            let field_id = egui::Id::new(("history-page-search", tab.id));
            let mut filter = ui.data_mut(|d| d.get_temp::<String>(filter_id).unwrap_or_default());
            let cap_w = ui.available_width().max(80.0);
            let (cap_rect, cap_resp) =
                ui.allocate_exact_size(egui::vec2(cap_w, SEARCH_H), egui::Sense::click());
            // Same contract as the omnibox: the whole capsule is the field,
            // so the padding shows the text cursor too. The clear button on
            // top keeps its own `PointingHand`.
            let cap_resp = cap_resp.on_hover_cursor(egui::CursorIcon::Text);
            // Painted after the field below (fresh focus state, same frame).
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(
                    cap_rect.min.x + CAPSULE_PAD + CAPSULE_ICON * 0.5,
                    cap_rect.center().y,
                ),
                egui::vec2(CAPSULE_ICON, CAPSULE_ICON),
            );
            let clear_rect = egui::Rect::from_center_size(
                egui::pos2(
                    cap_rect.max.x - CAPSULE_PAD - CAPSULE_CLEAR * 0.5,
                    cap_rect.center().y,
                ),
                egui::vec2(CAPSULE_CLEAR, CAPSULE_CLEAR),
            );
            icons::paint(
                ui,
                icon_rect.shrink(4.0),
                icons::Icon::Search,
                s.on_surface_variant,
            );
            let field_left = icon_rect.max.x + 2.0;
            // Never invert the field box on an ultra-narrow sheet: the text
            // keeps a 16px floor and the clear slot stays reserved.
            let field_right = (clear_rect.min.x - 2.0).max(field_left + 16.0);
            let field_rect = egui::Rect::from_min_max(
                egui::pos2(field_left, cap_rect.min.y + 3.0),
                egui::pos2(field_right, cap_rect.max.y - 3.0),
            );
            let field = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(field_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut filter)
                                .id(field_id)
                                // Frameless: the capsule is the frame. A framed
                                // field would double the border and its
                                // state-dependent stroke could shift the text
                                // on focus (the page's test pins the position).
                                .frame(egui::Frame::new())
                                .margin(egui::Margin::ZERO)
                                .desired_width(field_rect.width().max(16.0))
                                .char_limit(crate::config::MAX_HISTORY_FILTER_CHARS)
                                .hint_text("Search history")
                                .vertical_align(egui::Align::Center),
                        )
                    },
                )
                .inner;
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search history")
            });
            if field.changed() {
                ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            }
            if !filter.is_empty() {
                let clear = icons::icon_button_at(
                    ui,
                    clear_rect,
                    ("history-search-clear", tab.id),
                    icons::Icon::Close,
                    "Clear search",
                    true,
                );
                if clear.clicked() {
                    filter.clear();
                    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
                    ui.memory_mut(|m| m.request_focus(field_id));
                }
            }
            // Clicking the capsule padding (or the decorative magnifier)
            // focuses the field; the clear slot handles its own pointer.
            if cap_resp.clicked() {
                let in_clear = !filter.is_empty()
                    && cap_resp
                        .interact_pointer_pos()
                        .is_some_and(|pos| clear_rect.contains(pos));
                if !in_clear {
                    ui.memory_mut(|m| m.request_focus(field_id));
                }
            }
            // Fresh-state capsule fill + ring: constant 1px stroke width in
            // both states, so focusing never moves the text (see the theme's
            // stroke-width invariant). The field sits ON TOP of the capsule
            // widget, so its own hover must light the capsule too.
            let focused = field.has_focus() || ui.memory(|m| m.has_focus(field_id));
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
                    theme::pill_radius(SEARCH_H),
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
            ui.add_space(theme::spacing::M);
            theme::rule(ui);
            ui.add_space(theme::spacing::S);

            // Rebuild the filtered view only when the query, the store or
            // the age tick changed (re-lowercasing every URL/title per frame
            // made scrolling a full list stutter, and the section walk is
            // the same O(n) scan).
            let generation = store.generation();
            let tick = crate::util::now_secs() / AGE_TICK_SECS;
            let cache_id = egui::Id::new(("history-page-cache", tab.id));
            let cached = ui.data_mut(|d| d.get_temp::<std::sync::Arc<FilterCache>>(cache_id));
            let cache: std::sync::Arc<FilterCache> = match cached {
                // Compare the RAW filter on a hit (no per-frame String).
                Some(cache)
                    if cache.generation == generation
                        && cache.raw == filter
                        && cache.tick == tick =>
                {
                    cache
                }
                _ => {
                    let cache = std::sync::Arc::new(FilterCache::build(
                        store.entries(),
                        generation,
                        &filter,
                        tick,
                    ));
                    ui.data_mut(|d| {
                        d.insert_temp(cache_id, cache.clone());
                    });
                    cache
                }
            };
            let needle: &str = &cache.needle;
            let rows: &[usize] = &cache.rows;
            // One age clock for the sections AND the rows: the same tick
            // built both, so a label and its section never disagree.
            let now = cache.now();

            // The list owns the height left below the header and above the
            // footer (header/footer stay put; only the list scrolls).
            let list_h = (ui.available_height() - FOOTER_H - gutter).max(ROW_H);
            if rows.is_empty() {
                ui.add_space((list_h * 0.30).min(56.0));
                ui.vertical_centered(|ui| {
                    let (title, hint) = if needle.is_empty() {
                        ("No history yet", "Pages you open will appear here.")
                    } else {
                        ("No matches", "Try a different search.")
                    };
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                    icons::paint(ui, icon_rect, icons::Icon::History, s.on_surface_variant);
                    ui.label(theme::strong(
                        egui::RichText::new(title).color(s.on_surface),
                    ));
                    ui.label(theme::weak(ui, hint));
                });
                ui.add_space((list_h * 0.30).min(56.0));
            } else {
                // Virtualized list over headers + rows: only the visible
                // slots are built each frame. Headers share the row stride
                // (`show_rows` is uniform), painting their caption in a
                // full-height slot, so grouping costs no custom virtualizer.
                // `show_rows` reads `item_spacing.y` for its row stride, so
                // the scoped spacing must be zero.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let list_out = egui::ScrollArea::vertical()
                        .id_salt((crate::ui::HISTORY_SCROLL_ID, tab.id))
                        .max_height(list_h)
                        .auto_shrink([false, true])
                        .show_rows(ui, ROW_H, cache.flat_total, |ui, range| {
                            for flat in range {
                                match item_at(flat, &cache.sections, &cache.offsets) {
                                    ListItem::Header(section) => {
                                        if let Some(section) = cache.sections.get(section) {
                                            group_header(ui, section.group);
                                        }
                                    }
                                    ListItem::Row(pos) => {
                                        // Defensive (`get`, like
                                        // `build_sections`): a stale index
                                        // must skip a slot, never panic the
                                        // UI thread.
                                        let Some(index) = rows.get(pos).copied() else {
                                            continue;
                                        };
                                        let Some(visit) = store.entries().get(index) else {
                                            continue;
                                        };
                                        // Owned handle (one Arc bump): the
                                        // borrow of `favicons` ends here, so
                                        // the row painter stays `&`-only.
                                        let favicon = favicons.texture_for_url(&visit.url);
                                        match history_row(ui, index, visit, now, favicon.as_ref()) {
                                            RowAction::Open => {
                                                navigate = Some(visit.url.clone());
                                            }
                                            RowAction::Remove => {
                                                remove = Some(index);
                                            }
                                            RowAction::None => {}
                                        }
                                    }
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
                let shown = (!needle.is_empty()).then_some(rows.len());
                with_footer_label(total, shown, |label| {
                    ui.label(theme::weak(ui, label));
                });
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
                        // "Delete history" removes the visits, never the shared
                        // `store.key` (bookmarks/settings/session need it):
                        // the note must not promise a key deletion that does
                        // not happen (Preferences says the same).
                        ui.label(theme::weak(ui, "Click again to delete every visit."));
                        // Keep the armed label from going stale without input.
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                    }
                });
            });
            // A failed save/delete must never look silent on the page that
            // owns the action (Preferences shows it too).
            if let Some(error) = store.save_error() {
                ui.add_space(theme::spacing::XS);
                ui.label(egui::RichText::new(error).small().color(s.error));
            }
            ui.add_space(gutter + theme::spacing::M);
        },
    );

    if let Some(index) = remove {
        store.remove_entry(index);
    }
    if let Some(url) = navigate {
        tab.navigate_to_text(&url, true);
    }
    // Keep the relative ages fresh while the page is visible: wake at the
    // NEXT age tick, the moment the cached sections and the labels can
    // change (a flat 60s wait could land just after a tick and show the old
    // labels for almost another minute).
    let now = crate::util::now_secs();
    ui.ctx()
        .request_repaint_after(Duration::from_secs(AGE_TICK_SECS - (now % AGE_TICK_SECS)));
}

/// Rolling date bucket of one visit, from its age in seconds.
///
/// Calendar-day grouping would need a timezone database the app does not
/// carry; rolling 24h/7d/30d windows stay correct on any clock and match the
/// relative-age labels (`format_age`) the rows already show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgeGroup {
    /// Visited less than a day ago.
    Today,
    /// Visited one to two days ago.
    Yesterday,
    /// Visited two to seven days ago.
    PreviousWeek,
    /// Visited seven to thirty days ago.
    PreviousMonth,
    /// Anything older.
    Older,
}

/// Bucket of one visit for the grouped list.
fn age_group(now: u64, visited_at: u64) -> AgeGroup {
    const DAY: u64 = 86_400;
    match now.saturating_sub(visited_at) {
        age if age < DAY => AgeGroup::Today,
        age if age < 2 * DAY => AgeGroup::Yesterday,
        age if age < 7 * DAY => AgeGroup::PreviousWeek,
        age if age < 30 * DAY => AgeGroup::PreviousMonth,
        _ => AgeGroup::Older,
    }
}

/// Section header caption (static: no per-frame allocation).
fn group_label(group: AgeGroup) -> &'static str {
    match group {
        AgeGroup::Today => "Today",
        AgeGroup::Yesterday => "Yesterday",
        AgeGroup::PreviousWeek => "Previous 7 days",
        AgeGroup::PreviousMonth => "Previous 30 days",
        AgeGroup::Older => "Older",
    }
}

/// One contiguous run of `rows` (filtered store indices, newest first)
/// sharing a bucket. `len` addresses `rows`, not the store; the section's
/// starting offset lives in the caller's flat-offset table.
#[derive(Debug, Clone, Copy)]
struct Section {
    /// The bucket every row in this run belongs to.
    group: AgeGroup,
    /// Number of rows in this run.
    len: usize,
}

/// Folds the filtered newest-first `rows` into their date sections. The rows
/// arrive age-ordered (the store is oldest-first, the filter reverses it),
/// so each bucket is one contiguous run and empty buckets simply never
/// appear. Defensive on the index (`entries.get`): the rows were built from
/// the same generation, but a corrupt index must not panic a frame.
fn build_sections(entries: &[Visit], rows: &[usize], now: u64) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for &index in rows {
        let visited_at = entries
            .get(index)
            .map(|visit| visit.visited_at)
            .unwrap_or(0);
        let group = age_group(now, visited_at);
        match sections.last_mut() {
            Some(section) if section.group == group => section.len += 1,
            _ => sections.push(Section { group, len: 1 }),
        }
    }
    sections
}

/// One slot of the flattened virtualized list: either a section header or
/// one filtered row (`rows` offset, not a store index).
enum ListItem {
    /// Header of `sections[i]`.
    Header(usize),
    /// `rows[i]`.
    Row(usize),
}

/// Maps a virtualized slot to its header/row. `offsets[i]` is the flat slot
/// of `sections[i]`'s header (built alongside the sections above). A row's
/// GLOBAL offset into `rows` is `flat - index - 1`: every section before it
/// contributed `1 + len` slots, i.e. `index` headers plus `offsets[index] - index`
/// rows, so the local and global offsets differ by exactly the header count.
fn item_at(flat: usize, sections: &[Section], offsets: &[usize]) -> ListItem {
    let mut index = 0;
    while index < sections.len() {
        let start = offsets[index];
        let end = start + 1 + sections[index].len;
        if flat == start {
            return ListItem::Header(index);
        }
        if flat < end {
            return ListItem::Row(flat - index - 1);
        }
        index += 1;
    }
    // Unreachable while `flat` is inside the list (`show_rows` bounds it);
    // a header is always safe to paint, a row index might not be.
    debug_assert!(false, "history slot {flat} is past the grouped list");
    ListItem::Header(sections.len().saturating_sub(1))
}

/// One date-group label. It owns a full [`ROW_H`] slot like a visit row:
/// `show_rows` strides uniformly, so a shorter header would need a custom
/// virtualizer. The caption is the app's section-label language (bold
/// `UI_SMALL` in the muted tone, like the Shortcuts tab's group labels) and
/// the slot's own air above/below separates the sections. A real `Label`
/// (never painter text) so the section names reach the accessibility tree,
/// exactly like every row's title.
fn group_header(ui: &mut egui::Ui, group: AgeGroup) {
    let s = theme::scheme_ref(ui);
    let width = ui.available_width().max(80.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add_space(ROW_INSET);
            ui.add(
                egui::Label::new(theme::strong(
                    egui::RichText::new(group_label(group))
                        .size(theme::typography::UI_SMALL)
                        .color(s.on_surface_variant),
                ))
                .selectable(false),
            );
        },
    );
}

/// Footer/header count caption ("1 page" / "N pages"), memoized on `count`:
/// the page paints every frame while open, and the `format!` otherwise
/// allocated per frame. The borrow lives only for the `with` closure.
/// `pub(crate)` so Preferences reuses the same singular/plural rule for
/// its `"N pages stored"` caption instead of duplicating it.
pub(crate) fn with_count_label<R>(count: usize, f: impl FnOnce(&str) -> R) -> R {
    use std::cell::RefCell;
    thread_local! {
        static LABEL: RefCell<(usize, String)> = const { RefCell::new((usize::MAX, String::new())) };
    }
    LABEL.with(|cell| {
        let mut cached = cell.borrow_mut();
        if cached.0 != count {
            cached.0 = count;
            cached.1 = if count == 1 {
                "1 page".to_string()
            } else {
                format!("{count} pages")
            };
        }
        f(&cached.1)
    })
}

/// Header subtitle (`"12 · Keep 3 months"`), memoized on `(count,
/// retention)`: the page repaints every frame while open, and the old
/// inline `format!` allocated per frame (same contract as
/// [`with_count_label`]).
fn with_header_label<R>(count: usize, retention: Retention, f: impl FnOnce(&str) -> R) -> R {
    use std::cell::RefCell;
    thread_local! {
        static LABEL: RefCell<((usize, Retention), String)> =
            const { RefCell::new(((usize::MAX, Retention::Session), String::new())) };
    }
    LABEL.with(|cell| {
        let mut cached = cell.borrow_mut();
        let key = (count, retention);
        if cached.0 != key {
            cached.1 = format!(
                "{} · {}",
                with_count_label(count, str::to_string),
                retention_caption(retention)
            );
            cached.0 = key;
        }
        f(&cached.1)
    })
}

/// Footer count (`"12 pages"` unfiltered, `"3 of 12 pages"` while
/// filtering), memoized on `(total, shown)`: same per-frame contract as
/// [`with_count_label`].
fn with_footer_label<R>(total: usize, shown: Option<usize>, f: impl FnOnce(&str) -> R) -> R {
    use std::cell::RefCell;
    thread_local! {
        static LABEL: RefCell<((usize, Option<usize>), String)> =
            const { RefCell::new(((usize::MAX, None), String::new())) };
    }
    LABEL.with(|cell| {
        let mut cached = cell.borrow_mut();
        let key = (total, shown);
        if cached.0 != key {
            cached.0 = key;
            cached.1 = match shown {
                Some(n) => {
                    let total_label = if total == 1 {
                        "1 page".to_string()
                    } else {
                        format!("{total} pages")
                    };
                    format!("{n} of {total_label}")
                }
                None => {
                    if total == 1 {
                        "1 page".to_string()
                    } else {
                        format!("{total} pages")
                    }
                }
            };
        }
        f(&cached.1)
    })
}

/// Retention window caption for the header subtitle. Known presets reuse the
/// exact Preferences wording; anything else (a day count from an older or
/// hand-edited setting) falls back to "Keep N days".
fn retention_caption(retention: Retention) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    match retention {
        Retention::Session => Cow::Borrowed("Forget on exit"),
        Retention::Forever => Cow::Borrowed("Keep forever"),
        Retention::Days(30) => Cow::Borrowed("Keep 1 month"),
        Retention::Days(90) => Cow::Borrowed("Keep 3 months"),
        Retention::Days(180) => Cow::Borrowed("Keep 6 months"),
        Retention::Days(365) => Cow::Borrowed("Keep 1 year"),
        Retention::Days(days) => Cow::Owned(format!("Keep {days} days")),
    }
}

/// Glyph behind a row without a ready favicon: local files get the file
/// page, web pages the globe (the tab rail's fallback).
fn fallback_icon(url: &str) -> icons::Icon {
    if url.starts_with("file://") {
        icons::Icon::File
    } else {
        icons::Icon::Globe
    }
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
        // First-line center: the title occupies `min.y + 6 .. min.y + 26`
        // of the 52px row (see `history_row`).
        rect.min.y + 16.0,
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

/// Horizontal slots one row lays its content into: favicon, text column,
/// title/age boundary and remove button. Pure geometry (no `Ui`), so the
/// page's tests can pin the columns without driving a frame.
#[derive(Debug, Clone, Copy)]
struct RowSlots {
    /// Favicon art box (fitted inside the slot at the row's left).
    favicon: egui::Rect,
    /// First x of the text column (title and URL).
    content_left: f32,
    /// Right bound of the URL line and left edge of the action column.
    action_left: f32,
    /// Right bound of the title box: the age column owns what follows.
    title_max_x: f32,
    /// Remove button hit/paint box.
    remove: egui::Rect,
}

/// [`RowSlots`] for one row of `rect` width, reserving `lane` for the
/// floating scrollbar.
fn row_slots(rect: egui::Rect, lane: f32) -> RowSlots {
    let text_left = rect.min.x + ROW_INSET;
    let action_left = rect.max.x - ROW_ACTION_W - lane;
    let content_left = text_left + FAVICON_SLOT + FAVICON_GAP;
    let fav_center = egui::pos2(text_left + FAVICON_SLOT * 0.5, rect.center().y);
    RowSlots {
        favicon: egui::Rect::from_center_size(fav_center, egui::vec2(FAVICON_ART, FAVICON_ART)),
        content_left,
        action_left,
        // The title keeps at least 40px even when the window is too narrow
        // for the age column.
        title_max_x: (action_left - AGE_W).max(content_left + 40.0),
        remove: remove_button_rect(rect, lane),
    }
}

/// The footer's delete button at its fixed width: the shared tonal button
/// (themed hover/press/disabled ramp, accent focus handling) at a fixed
/// height so the two-click confirmation never resizes the footer. The focus
/// ring paints in the error tone: this is the destructive action.
fn delete_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let resp = theme::tonal_button_sized(ui, label, egui::vec2(DELETE_W, 28.0), true);
    if resp.has_focus() {
        theme::focus_ring_stroke(
            ui,
            resp.rect,
            theme::radius::CARD,
            theme::scheme_ref(ui).error,
        );
    }
    resp
}

/// One visit row: favicon, title + age on the first line, URL below,
/// remove X on the right. Clicking the row body opens the page.
///
/// `favicon` is the ready session texture for this visit's origin (resolved
/// by the caller, one hash probe per visible row); `None` paints the
/// globe/file fallback glyph instead.
fn history_row(
    ui: &mut egui::Ui,
    index: usize,
    visit: &Visit,
    now: u64,
    favicon: Option<&egui::TextureHandle>,
) -> RowAction {
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
    }
    if response.has_focus() {
        // Focus ring IN ADDITION to the hover wash (see omnibox rows):
        // hiding it while hovered lost keyboard position under the pointer.
        theme::focus_ring_with(ui, rect, theme::radius::MENU, s);
    }

    // First line: title (or the URL when the page had none).
    let title = match &visit.title {
        Some(title) => std::borrow::Cow::Borrowed(title.as_str()),
        None => strip_scheme_prefix(&visit.url),
    };
    let title = truncate_chars(&title, ROW_TITLE_CHARS);
    // Accessible name for the custom-painted row (the labels are not
    // interactive themselves, and the age caption below is painter text).
    // URL and age belong in it: a screen reader must tell two visits to the
    // same title apart and hear the relative age. Built inside the closure
    // by accesskit, never per frame.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "{title} — {} — {}",
                strip_scheme_prefix(&visit.url),
                crate::history::format_age(now, visit.visited_at)
            ),
        )
    });
    let lane = theme::scrollbar_width(ui);
    let slots = row_slots(rect, lane);
    // Favicon slot: the ready texture fitted (never stretched, like every
    // other favicon painter), else the globe/file fallback glyph.
    if let Some(texture) = favicon {
        crate::ui::paint_texture_fit(ui, slots.favicon, texture, 4);
    } else {
        icons::paint(
            ui,
            slots.favicon,
            fallback_icon(&visit.url),
            s.on_surface_variant,
        );
    }
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(slots.content_left, rect.min.y + 6.0),
        egui::pos2(slots.title_max_x, rect.min.y + 26.0),
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
        egui::pos2(slots.content_left, rect.min.y + 26.0),
        egui::pos2(slots.action_left, rect.min.y + 46.0),
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

    let remove_rect = slots.remove;
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
        ui.advance_cursor_after_rect(rect);
        return RowAction::Remove;
    }
    if response.clicked() {
        // Defensive: if the remove hit test lost to the row (overlapping
        // widgets), a click on the X must never open the page.
        if response
            .interact_pointer_pos()
            .is_some_and(|pos| remove_rect.contains(pos))
        {
            ui.advance_cursor_after_rect(rect);
            return RowAction::None;
        }
        ui.advance_cursor_after_rect(rect);
        return RowAction::Open;
    }
    // Full title + URL on hover: both lines are truncated, and sighted
    // hover could not recover them (only accesskit saw the full text).
    crate::ui::hover_tooltip_lazy(response, || {
        let t = visit.title.as_deref().unwrap_or(visit.url.as_str());
        format!("{t} — {}", strip_scheme_prefix(&visit.url))
    });
    // The two inset text scopes leave the parent cursor at the URL line's
    // bottom (46px into a 52px row): the next row then overlapped this one
    // by 6px and the virtualized list reported 52px strides while painting
    // 46px, so the page could scroll into a large empty tail. Re-own the
    // whole row.
    ui.advance_cursor_after_rect(rect);
    RowAction::None
}

/// Current Unix time in seconds (0 on a pre-epoch clock).
#[cfg(test)]
mod tests;
