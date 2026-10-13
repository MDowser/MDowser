// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::show_history_page;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use eframe::egui;

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    }
}

/// The page renders headlessly with and without entries, and an empty
/// store shows the "No history yet" state instead of panicking.
#[test]
fn history_page_renders() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut tab = Tab::new(1);
    tab.open_history(true);
    assert!(matches!(tab.state, TabState::History));

    // Empty store first.
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, 8.0);
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "empty page painted");

    // More entries than the visible window: exercises the virtualized
    // list row-range indexing.
    for i in 0..120 {
        store.record(&format!("https://ex.com/{i}/"), Some(&format!("Page {i}")));
    }
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, 8.0);
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "populated page painted");
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
        store.record(&format!("https://ex.com/{i}/"), Some(&format!("Page {i}")));
    }
    let mut tab = Tab::new(1);
    tab.open_history(true);
    let mut out = ctx.run_ui(input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, &mut tab, &mut store, 8.0);
        });
    });
    out.textures_delta.clear();
    // A ~700px sheet with 46px rows must show many rows, not one.
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

/// One page frame with the given pointer events.
fn render_page(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    tab: &mut Tab,
    store: &mut HistoryStore,
) -> Vec<egui::epaint::ClippedShape> {
    let input = egui::RawInput { events, ..input() };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            show_history_page(ui, tab, store, 8.0);
        });
    });
    out.textures_delta.clear();
    out.shapes
}

/// Focusing the search field must not move its text. The default-framed
/// `TextEdit` balances its state-dependent inner margin against the state
/// stroke width (7+1 resting, 8+0 focused), so the contract holds as long
/// as the themed `selection.stroke` and widget strokes stay compatible;
/// this guards the field's text position (a regression here means the
/// hint/typed text shifts by a pixel on focus).
#[test]
fn search_field_geometry_is_stable_on_focus() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    store.record("https://ex.com/a/", Some("A"));
    let mut tab = Tab::new(1);
    tab.open_history(true);

    // Settle, then locate the field from its hint text.
    let mut shapes = Vec::new();
    for _ in 0..3 {
        shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store);
    }
    let before = hint_pos(&shapes).expect("the search hint is visible");

    // Hover, press and release on the field: it takes keyboard focus.
    render_page(
        &ctx,
        vec![egui::Event::PointerMoved(before)],
        &mut tab,
        &mut store,
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
    );
    render_page(&ctx, vec![button(false)], &mut tab, &mut store);
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::new(("history-page-search", tab.id)))),
        "the click must focus the search field"
    );

    // Settled frame with the field focused.
    let after_shapes = render_page(&ctx, Vec::new(), &mut tab, &mut store);
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
