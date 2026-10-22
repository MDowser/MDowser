// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).
//!
//! Every fixture uses `file://` URLs: the page resolves row favicons through
//! the shared session cache, and an unknown `https:` origin would start a
//! network probe inside the test run (the Home tile tests avoid the same
//! thing the same way).

use super::show_history_page;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// A local-file URL: recorded like any visit (`file://` is recordable) but
/// never probed by the favicon cache, so tests stay off the network.
fn file_url(tag: &str) -> String {
    format!("file:///tmp/mdowser-history-{tag}.md")
}

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    }
}

/// One page frame with the given pointer events.
fn render_page(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    tab: &mut Tab,
    store: &mut HistoryStore,
    favicons: &mut FaviconCache,
) -> Vec<egui::epaint::ClippedShape> {
    let input = egui::RawInput { events, ..input() };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, tab, store, favicons, 8.0);
        });
    });
    out.textures_delta.clear();
    out.shapes
}

/// Typing into the search field is a memory write (the page reads its own
/// per-tab query back every frame): tests seed it the same way.
fn store_query(ctx: &egui::Context, tab: &Tab, query: &str) {
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::unique(("history-page-filter", tab.id)),
            query.to_string(),
        );
    });
}

/// Every text string painted this frame.
fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Rows occupy the full `ROW_H` in the virtualized list: the two inset text
/// scopes used to leave the parent cursor 6px short per row, so consecutive
/// rows overlapped and the page could scroll ~6px per row past the last one
/// into an empty tail.
#[test]
fn rows_keep_the_full_row_stride() {
    use super::ROW_H;
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = HistoryStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.record(&file_url("a"), Some("Alpha"));
    store.record(&file_url("b"), Some("Beta"));

    let _ = render_page(&ctx, vec![], &mut tab, &mut store, &mut favicons);
    let shapes = render_page(&ctx, vec![], &mut tab, &mut store, &mut favicons);
    let pos_of = |text: &str| {
        shapes.iter().find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos),
            _ => None,
        })
    };
    let a = pos_of("Alpha").expect("first row painted");
    let b = pos_of("Beta").expect("second row painted");
    let stride = (b.y - a.y).abs();
    assert!(
        (stride - ROW_H).abs() < 0.5,
        "rows must sit ROW_H ({ROW_H}) apart, got {stride}"
    );
}

/// The search capsule's painted pill: the only rect of the capsule's height
/// wide enough to be the column (row washes are `ROW_H` tall).
fn capsule_rect(shapes: &[egui::epaint::ClippedShape]) -> egui::Rect {
    use super::SEARCH_H;
    shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r)
                if (r.rect.height() - SEARCH_H).abs() < 0.5 && r.rect.width() > 200.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("the search capsule is painted")
}

/// A full pointer click at `pos` (move, press, release).
fn click(
    ctx: &egui::Context,
    pos: egui::Pos2,
    tab: &mut Tab,
    store: &mut HistoryStore,
    favicons: &mut FaviconCache,
) {
    render_page(
        ctx,
        vec![egui::Event::PointerMoved(pos)],
        tab,
        store,
        favicons,
    );
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    render_page(
        ctx,
        vec![egui::Event::PointerMoved(pos), button(true)],
        tab,
        store,
        favicons,
    );
    render_page(ctx, vec![button(false)], tab, store, favicons);
}

/// The page renders headlessly with and without entries, an empty store
/// shows the "No history yet" state instead of panicking, and a populated
/// one paints its date section header.
#[test]
fn history_page_renders() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);
    assert!(matches!(tab.state, TabState::History));

    // Empty store first.
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, &mut favicons, 8.0);
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "empty page painted");

    // More entries than the visible window: exercises the virtualized
    // list row-range indexing (sections + rows).
    for i in 0..120 {
        store.record(&file_url(&format!("{i}")), Some(&format!("Page {i}")));
    }
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, &mut favicons, 8.0);
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "populated page painted");
    let painted: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        painted.iter().any(|text| text == "Today"),
        "the newest visits paint under the Today section"
    );
}

/// The age label and the remove button must never overlap: the 24px
/// button box used to sit 6px beyond the reserved column, landing on the
/// right-aligned age text (and its hover fill).
#[test]
fn age_and_remove_do_not_overlap() {
    use super::{age_anchor, remove_button_rect, AGE_GAP, ROW_ACTION_W, ROW_H};
    for width in [320.0, 760.0] {
        // The floating scrollbar lane the row reserves on the right.
        let lane = 10.0;
        let row = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, ROW_H));
        let remove = remove_button_rect(row, lane);
        let age = age_anchor(row, lane);
        assert!(
            remove.min.x - age.x >= AGE_GAP,
            "age anchor {age:?} runs into the remove button {remove:?}"
        );
        assert!(
            remove.max.x <= row.max.x - lane,
            "button stays clear of the scrollbar"
        );
        assert!(
            remove.min.x > row.max.x - ROW_ACTION_W - lane,
            "button stays inside the action column"
        );
    }
}

/// The search filter matches URL and title, case-insensitively, and an
/// empty needle keeps every visit.
#[test]
fn filter_matches_url_and_title() {
    use super::matches_filter;
    use crate::history::Visit;
    let visit = Visit {
        url: "https://Example.com/Docs/".to_string(),
        title: Some("The Docs".to_string()),
        visited_at: 0,
    };
    assert!(matches_filter(&visit, ""));
    assert!(matches_filter(&visit, "example"));
    assert!(matches_filter(&visit, "docs"));
    assert!(matches_filter(&visit, "the docs"));
    assert!(!matches_filter(&visit, "missing"));
}

/// Regression: the virtualized list must use the whole sheet height.
/// The column once lived inside a plain `ui.horizontal` (whose nested
/// `ui.vertical` only receives the row's initial height), so only the
/// first row was visible and the footer was pushed off the sheet.
#[test]
fn list_uses_the_whole_sheet_height() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    for i in 0..60 {
        store.record(&file_url(&format!("{i}")), Some(&format!("Page {i}")));
    }
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, &mut favicons, 8.0);
        });
    });
    out.textures_delta.clear();
    // A ~700px sheet with 52px rows must show many rows, not one.
    let visible_rows = out
        .shapes
        .iter()
        .filter(|c| {
            matches!(
                &c.shape,
                egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Page ")
            )
        })
        .count();
    assert!(
        visible_rows >= 5,
        "only {visible_rows} history rows visible; the list is height-starved"
    );
}

/// Position of the search field's hint text, if painted.
fn hint_pos(shapes: &[egui::epaint::ClippedShape]) -> Option<egui::Pos2> {
    shapes.iter().find_map(|c| match &c.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == "Search history" => Some(t.pos),
        _ => None,
    })
}

/// Focusing the search capsule must not move its text: the frameless
/// `TextEdit` lives inside a fixed capsule whose fill/stroke never change
/// width, so the hint/typed text keeps its position when the field gains
/// focus (a regression here means the text shifts by a pixel on focus).
#[test]
fn search_field_geometry_is_stable_on_focus() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    store.record(&file_url("a"), Some("A"));
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);

    // Settle, then locate the field from its hint text.
    let mut shapes = Vec::new();
    for _ in 0..3 {
        shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store, &mut favicons);
    }
    let before = hint_pos(&shapes).expect("the search hint is visible");

    // Hover, press and release on the field: it takes keyboard focus.
    render_page(
        &ctx,
        vec![egui::Event::PointerMoved(before)],
        &mut tab,
        &mut store,
        &mut favicons,
    );
    let button = |pressed| egui::Event::PointerButton {
        pos: before,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    render_page(
        &ctx,
        vec![egui::Event::PointerMoved(before), button(true)],
        &mut tab,
        &mut store,
        &mut favicons,
    );
    render_page(
        &ctx,
        vec![button(false)],
        &mut tab,
        &mut store,
        &mut favicons,
    );
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(("history-page-search", tab.id)))),
        "the click must focus the search field"
    );

    // Settled frame with the field focused.
    let after_shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store, &mut favicons);
    let after = hint_pos(&after_shapes).expect("the search hint stayed visible");
    assert_eq!(before, after, "focusing the search field moved its text");
}

/// Arming the two-click delete must not resize the footer button: both
/// labels fit the fixed width the page gives it, so the swap never moves
/// the footer.
#[test]
fn delete_button_labels_fit_the_fixed_width() {
    use super::{delete_button, DELETE_W};
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut widths = Vec::new();
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            for label in ["Delete history now", "Confirm delete"] {
                widths.push(delete_button(ui, label).rect.width());
            }
        });
    });
    out.textures_delta.clear();
    assert_eq!(widths.len(), 2);
    for w in widths {
        assert!(
            (w - DELETE_W).abs() < 0.01,
            "label must fit the fixed width {DELETE_W} (got {w})"
        );
    }
}

/// Every rolling bucket boundary lands exactly where the labels claim.
#[test]
fn age_group_covers_every_bucket() {
    use super::{age_group, AgeGroup};
    const DAY: u64 = 86_400;
    let now = 10_000_000_u64;
    assert_eq!(age_group(now, now), AgeGroup::Today);
    assert_eq!(age_group(now, now - (DAY - 1)), AgeGroup::Today);
    assert_eq!(age_group(now, now - DAY), AgeGroup::Yesterday);
    assert_eq!(age_group(now, now - (2 * DAY - 1)), AgeGroup::Yesterday);
    assert_eq!(age_group(now, now - 2 * DAY), AgeGroup::PreviousWeek);
    assert_eq!(age_group(now, now - (7 * DAY - 1)), AgeGroup::PreviousWeek);
    assert_eq!(age_group(now, now - 7 * DAY), AgeGroup::PreviousMonth);
    assert_eq!(
        age_group(now, now - (30 * DAY - 1)),
        AgeGroup::PreviousMonth
    );
    assert_eq!(age_group(now, now - 30 * DAY), AgeGroup::Older);
    // A future/clock-skewed timestamp reads as "just now", never older.
    assert_eq!(age_group(now, now + 5_000), AgeGroup::Today);
}

/// The filtered rows fold into contiguous newest-first sections, skipping
/// the buckets that have no visits, and stay bound to `rows` offsets.
#[test]
fn build_sections_groups_contiguous_rows() {
    use super::{build_sections, AgeGroup};
    use crate::history::Visit;
    const DAY: u64 = 86_400;
    let now = 10_000_000_u64;
    let visit = |age_days: u64| Visit {
        url: file_url("s"),
        title: None,
        visited_at: now - age_days * DAY,
    };
    // Oldest first, like the store.
    let entries = vec![visit(40), visit(10), visit(3), visit(1), visit(0)];
    // Newest first, like the filter cache: 0d, 1d, 3d, 10d, 40d.
    let rows: Vec<usize> = (0..entries.len()).rev().collect();
    let sections = build_sections(&entries, &rows, now);
    let shape: Vec<(AgeGroup, usize)> = sections.iter().map(|s| (s.group, s.len)).collect();
    assert_eq!(
        shape,
        vec![
            (AgeGroup::Today, 1),
            (AgeGroup::Yesterday, 1),
            (AgeGroup::PreviousWeek, 1),
            (AgeGroup::PreviousMonth, 1),
            (AgeGroup::Older, 1),
        ]
    );

    // Runs collapse: three visits in the same bucket share one section
    // (store order is oldest first, so the 9-day-old visit leads).
    let entries = vec![visit(9), visit(0), visit(0)];
    let rows: Vec<usize> = (0..entries.len()).rev().collect();
    let sections = build_sections(&entries, &rows, now);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].group, AgeGroup::Today);
    assert_eq!(sections[0].len, 2);
    assert_eq!(sections[1].group, AgeGroup::PreviousMonth);
    assert_eq!(sections[1].len, 1);

    // No rows: no sections (the empty state owns that frame).
    assert!(build_sections(&entries, &[], now).is_empty());
    // A stale index never panics (defensive `entries.get`).
    assert_eq!(build_sections(&entries, &[99], now).len(), 1);
}

/// The flattened virtualized slots map back to the right header/row: each
/// header owns one slot and every row follows its section's header.
#[test]
fn item_at_maps_flat_slots() {
    use super::{build_sections, item_at, ListItem};
    use crate::history::Visit;
    const DAY: u64 = 86_400;
    let now = 10_000_000_u64;
    let entries = vec![
        Visit {
            url: file_url("old"),
            title: None,
            visited_at: now - 2 * DAY,
        },
        Visit {
            url: file_url("a"),
            title: None,
            visited_at: now - 60,
        },
        Visit {
            url: file_url("b"),
            title: None,
            visited_at: now - 30,
        },
    ];
    // Newest first: b, a (same bucket) then old.
    let rows = vec![2_usize, 1, 0];
    let sections = build_sections(&entries, &rows, now);
    let mut offsets = Vec::new();
    let mut total = 0;
    for section in &sections {
        offsets.push(total);
        total += 1 + section.len;
    }
    assert_eq!(total, 5, "two headers + three rows");
    let flat: Vec<ListItem> = (0..total)
        .map(|f| item_at(f, &sections, &offsets))
        .collect();
    match (&flat[0], &flat[1], &flat[2], &flat[3], &flat[4]) {
        (
            ListItem::Header(0),
            ListItem::Row(0),
            ListItem::Row(1),
            ListItem::Header(1),
            ListItem::Row(2),
        ) => {}
        _ => panic!("unexpected flat mapping"),
    }
}

/// The favicon fallback glyph follows the visit kind: local files get the
/// file page, web pages the globe (the tab rail's fallback pair).
#[test]
fn fallback_icon_follows_the_visit_kind() {
    use super::fallback_icon;
    use crate::ui::icons::Icon;
    assert_eq!(fallback_icon("file:///tmp/notes.md"), Icon::File);
    assert_eq!(fallback_icon("https://ex.com/a"), Icon::Globe);
}

/// The header subtitle names the active retention window, reusing the
/// Preferences wording for the presets a user can actually pick.
#[test]
fn retention_caption_covers_every_preset() {
    use super::retention_caption;
    use crate::history::Retention;
    assert_eq!(retention_caption(Retention::Session), "Forget on exit");
    assert_eq!(retention_caption(Retention::Forever), "Keep forever");
    assert_eq!(retention_caption(Retention::Days(30)), "Keep 1 month");
    assert_eq!(retention_caption(Retention::Days(90)), "Keep 3 months");
    assert_eq!(retention_caption(Retention::Days(180)), "Keep 6 months");
    assert_eq!(retention_caption(Retention::Days(365)), "Keep 1 year");
    // A day count from an older/hand-edited setting still reads sensibly.
    assert_eq!(retention_caption(Retention::Days(7)), "Keep 7 days");
}

/// The row columns never collide: the favicon clears the text column, the
/// title box stops before the age column, and neither line runs under the
/// remove button or the scrollbar lane, at every supported width.
#[test]
fn row_columns_never_collide() {
    use super::{age_anchor, row_slots, ROW_H};
    let lane = 10.0;
    for width in [320.0, 760.0] {
        let row = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, ROW_H));
        let slots = row_slots(row, lane);
        assert!(
            slots.favicon.min.x >= row.min.x,
            "favicon {slots:?} starts inside the row"
        );
        assert!(
            slots.favicon.max.x < slots.content_left,
            "favicon {slots:?} must clear the text column"
        );
        assert!(
            slots.content_left + 40.0 <= slots.title_max_x,
            "the title must keep a usable box {slots:?}"
        );
        assert!(
            slots.title_max_x < age_anchor(row, lane).x,
            "the title box {slots:?} must stop before the age label"
        );
        assert!(
            slots.action_left <= slots.remove.min.x,
            "the URL line must stop before the remove button {slots:?}"
        );
        assert!(
            slots.remove.max.x <= row.max.x - lane,
            "the button must stay clear of the scrollbar {slots:?}"
        );
    }
}

/// The section header paints inside the sheet: a local-file visit puts the
/// "Today" caption inside its own clip rect (never clipped by the list) and
/// left of the group rule's right end.
#[test]
fn section_header_paints_inside_the_sheet() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    store.record(&file_url("header"), Some("Header Page"));
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store, &mut favicons);
    }
    let (pos, galley, clip) = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == "Today" => {
                Some((t.pos, t.galley.clone(), c.clip_rect))
            }
            _ => None,
        })
        .expect("the section caption is painted");
    let rect = egui::Rect::from_min_size(pos, galley.size());
    assert!(
        clip.contains_rect(rect),
        "section caption {rect:?} clipped by {clip:?}"
    );
    assert!(
        pos.x > clip.min.x && pos.x < clip.max.x,
        "section caption starts inside the sheet: {pos:?}"
    );
}

/// The cache is the page's ONE store scan: rows come out newest-first and
/// filtered, the sections tile them contiguously, and every virtualized slot
/// maps to exactly one header or row.
#[test]
fn filter_cache_builds_a_consistent_view() {
    use super::{item_at, FilterCache, ListItem, AGE_TICK_SECS};
    use crate::history::Visit;
    const DAY: u64 = 86_400;
    // Build the fixture ON the tick the cache uses: ages that land exactly
    // on a bucket boundary keep their bucket (a raw `now` would drift up to
    // `AGE_TICK_SECS` younger and flip them).
    let tick = 10_000_000_u64 / AGE_TICK_SECS;
    let now = tick * AGE_TICK_SECS;
    let visit = |age_days: u64, title: &str| Visit {
        url: file_url(title),
        title: Some(title.to_string()),
        visited_at: now - age_days * DAY,
    };
    // Oldest first, like the store.
    let entries = vec![
        visit(9, "Old Page"),
        visit(1, "Middle Page"),
        visit(0, "Fresh Page"),
        visit(0, "Fresh Note"),
    ];
    let cache = FilterCache::build(&entries, 7, "page", tick);
    assert_eq!(cache.generation, 7);
    assert_eq!(cache.raw, "page");
    // Filtered (the note is out) and newest first.
    let titles: Vec<&str> = cache
        .rows
        .iter()
        .map(|&index| entries[index].title.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(titles, vec!["Fresh Page", "Middle Page", "Old Page"]);
    // One header per non-empty bucket, one slot per header + row.
    assert_eq!(cache.sections.len(), 3);
    assert_eq!(cache.flat_total, cache.rows.len() + cache.sections.len());
    let mut headers = 0;
    let mut rows_seen = 0;
    for flat in 0..cache.flat_total {
        match item_at(flat, &cache.sections, &cache.offsets) {
            ListItem::Header(index) => {
                assert_eq!(cache.offsets[index], flat, "header owns its offset slot");
                headers += 1;
            }
            ListItem::Row(pos) => {
                assert!(pos < cache.rows.len(), "row offset {pos} is in range");
                rows_seen += 1;
            }
        }
    }
    assert_eq!(headers, cache.sections.len());
    assert_eq!(rows_seen, cache.rows.len());
    // The tick is the age clock the labels and sections share.
    assert_eq!(cache.now() / AGE_TICK_SECS, tick);
}

/// A stored query (the capsule field's own per-tab state) filters the
/// rendered list end to end: the matching row stays, the other drops out,
/// the footer reports the filter and the hint gives way to the query. The
/// second frame exercises the cache HIT path.
#[test]
fn stored_query_filters_the_rendered_list() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    store.record(&file_url("keep"), Some("Keep Page"));
    store.record(&file_url("drop"), Some("Drop Page"));
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);
    store_query(&ctx, &tab, "keep");
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store, &mut favicons);
    }
    let painted = painted_texts(&shapes);
    assert!(
        painted.iter().any(|t| t == "Keep Page"),
        "the matching row stays listed"
    );
    assert!(
        !painted.iter().any(|t| t == "Drop Page"),
        "a non-matching row is filtered out"
    );
    assert!(
        painted.iter().any(|t| t == "1 of 2 pages"),
        "the footer reports the filtered count"
    );
    assert!(
        !painted.iter().any(|t| t == "Search history"),
        "the typed query replaces the hint"
    );
}

/// The capsule's clear button empties the query and brings the full list
/// back (and the hint with it), without stealing the field's focus.
#[test]
fn clear_button_empties_the_query() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    store.record(&file_url("keep"), Some("Keep Page"));
    store.record(&file_url("drop"), Some("Drop Page"));
    let mut tab = Tab::new(1);
    let mut favicons = FaviconCache::new();
    tab.open_history(true);
    store_query(&ctx, &tab, "keep");
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store, &mut favicons);
    }
    // The clear slot sits at the capsule's right end (the page reserves it
    // on every frame, typed query or not).
    let capsule = capsule_rect(&shapes);
    let clear = egui::pos2(
        capsule.max.x - super::CAPSULE_PAD - super::CAPSULE_CLEAR * 0.5,
        capsule.center().y,
    );
    click(&ctx, clear, &mut tab, &mut store, &mut favicons);
    let painted = painted_texts(&render_page(
        &ctx,
        Vec::new(),
        &mut tab,
        &mut store,
        &mut favicons,
    ));
    assert!(
        painted.iter().any(|t| t == "Drop Page"),
        "clearing the query brings the filtered-out row back"
    );
    assert!(
        painted.iter().any(|t| t == "2 pages"),
        "the footer is back to the unfiltered count"
    );
    assert!(
        painted.iter().any(|t| t == "Search history"),
        "the hint returns with the empty field"
    );
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(("history-page-search", tab.id)))),
        "clearing keeps the keyboard in the search field"
    );
}

/// The count caption is singular only for exactly one page.
#[test]
fn count_label_is_singular_only_for_one() {
    use super::with_count_label;
    for (count, want) in [(0, "0 pages"), (1, "1 page"), (200, "200 pages")] {
        with_count_label(count, |got| assert_eq!(got, want));
    }
}
