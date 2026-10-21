// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{address_id, show_omnibox, BookmarkState};
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::Tab;
use crate::history::HistoryStore;
use eframe::egui;

/// One omnibox frame at a 900x600 viewport; returns `(submitted, painted
/// shapes)`.
fn frame(
    ctx: &egui::Context,
    tab: &mut Tab,
    history: &HistoryStore,
    bookmarks: &BookmarkStore,
    focus: bool,
    events: Vec<egui::Event>,
) -> (bool, Vec<egui::epaint::ClippedShape>) {
    frame_sized(ctx, tab, history, bookmarks, focus, events, 600.0)
}

/// One omnibox frame at a custom viewport height.
fn frame_sized(
    ctx: &egui::Context,
    tab: &mut Tab,
    history: &HistoryStore,
    bookmarks: &BookmarkStore,
    focus: bool,
    events: Vec<egui::Event>,
    screen_h: f32,
) -> (bool, Vec<egui::epaint::ClippedShape>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, screen_h),
        )),
        events,
        ..Default::default()
    };
    let mut submitted = false;
    let mut out = ctx.run_ui(input, |root| {
        submitted = show_omnibox(
            root,
            tab,
            400.0,
            34.0,
            focus,
            "address-bar",
            Some(BookmarkState::Unavailable),
            history,
            bookmarks,
        )
        .submitted;
    });
    out.textures_delta.clear();
    (submitted, out.shapes)
}

/// The framed popup rect stored by the last frame.
fn popup_rect(ctx: &egui::Context) -> egui::Rect {
    let key = address_id(1, "address-bar")
        .with("suggest")
        .with("popup-rect");
    ctx.data_mut(|d| d.get_temp(key))
        .expect("the popup was recorded")
}

/// Every painted text run.
fn painted(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Number of filled (non-transparent) rectangles that intersect `region`.
///
/// Compares counts, not colors: an `Area` fades in and multiplies every
/// shape by its opacity, and the test harness does not advance time, so the
/// painted fill is the theme color at a fraction of its alpha.
fn filled_rects_in(shapes: &[egui::epaint::ClippedShape], region: egui::Rect) -> usize {
    shapes
        .iter()
        .filter(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) => r.fill.a() > 0 && region.intersects(r.rect),
            _ => false,
        })
        .count()
}

/// Position of the first painted text run equal to `text`.
fn text_pos(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
    shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos),
            _ => None,
        })
        .unwrap_or_else(|| panic!("text {text:?} was painted"))
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Two bookmarks (toggle order: Alpha oldest, Beta newest) + one visit.
fn stores() -> (HistoryStore, BookmarkStore) {
    let mut bookmarks = BookmarkStore::in_memory();
    bookmarks.toggle("https://ex.com/alpha/", Some("Alpha"));
    bookmarks.toggle("https://ex.com/beta/", Some("Beta"));
    let mut history = HistoryStore::in_memory();
    history.record("https://history.example/page/", Some("Past Page"));
    (history, bookmarks)
}

/// A focused, empty field offers recent bookmarks (newest first) with
/// their schemeless URL; an unfocused field paints no popup.
#[test]
fn focused_empty_field_offers_recent_bookmarks() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (submitted, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    assert!(!submitted, "opening the popup never navigates");
    let texts = painted(&shapes);
    assert!(
        texts.iter().any(|t| t == "Beta"),
        "newest bookmark first: {texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Alpha"));
    assert!(
        texts.iter().any(|t| t == "ex.com/beta/"),
        "rows carry the schemeless URL: {texts:?}"
    );
    // Not focused: same stores, no popup.
    let mut tab = Tab::new(2);
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    assert!(!painted(&shapes).iter().any(|t| t == "Beta"));
}

/// Typing filters by URL/title, history included, and rows keep only
/// their own text (no stale list from the previous query).
#[test]
fn typing_filters_the_suggestions() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    tab.address = "past".to_string();
    // First frame: the popup area runs its (invisible) sizing pass.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let texts = painted(&shapes);
    assert!(
        texts.iter().any(|t| t == "Past Page"),
        "history matches the query: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Beta"),
        "no stale rows: {texts:?}"
    );
}

/// Enter navigates to the typed address, never to the default highlight:
/// the popup preselects its first row only visually. Presetting the default
/// row used to replace the typed URL, so editing an address (e.g. trimming
/// `ex.com/alpha/beta/` to `ex.com/alpha/b`) silently "reloaded" the page
/// already shown.
#[test]
fn enter_keeps_the_typed_address_when_nothing_was_picked() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    // Matches the Beta bookmark; the typed text must win anyway.
    tab.address = "ex.com/be".to_string();
    // Ctrl/Cmd+L equivalent, then the popup builds.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted, "Enter submits even with the popup open");
    assert_eq!(
        tab.address, "ex.com/be",
        "the typed address is submitted as-is"
    );
}

/// `Down`/`Up` (or hovering a row) is an EXPLICIT pick: only then is the
/// highlighted row painted and only then does Enter navigate to it instead
/// of the typed address. The FIRST `Down` picks the first row.
#[test]
fn enter_uses_an_explicitly_selected_suggestion() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    // The empty field offers [Beta, Alpha]; `Down` picks the first, Beta.
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::ArrowDown)],
    );
    assert!(!submitted, "moving the selection never navigates");
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted, "Enter must submit");
    assert_eq!(
        tab.address, "ex.com/beta/",
        "the explicitly selected bookmark is preset"
    );
    assert!(tab.address_dirty, "the caller navigates from the field");
}

/// The first `Up` mirrors the first `Down`: it picks the LAST row (no row is
/// selected until the user picks one).
#[test]
fn first_up_picks_the_last_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::ArrowUp)],
    );
    assert!(!submitted, "moving the selection never navigates");
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted, "Enter must submit");
    assert_eq!(tab.address, "ex.com/alpha/", "the last row was picked");
}

/// An untouched popup paints NO highlighted row: the first row used to look
/// selected while Enter submitted the typed text. The first `Down` is a real
/// pick and highlights exactly one row.
#[test]
fn the_first_row_is_not_highlighted_until_picked() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    // Settle the empty-field popup ([Beta, Alpha]).
    for _ in 0..2 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    }
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let popup: egui::Rect = ctx
        .data_mut(|d| {
            d.get_temp(
                address_id(1, "address-bar")
                    .with("suggest")
                    .with("popup-rect"),
            )
        })
        .expect("the popup was recorded");
    // The popup chrome (frame/shadow) is identical frame to frame; only the
    // row highlight can add a filled rect inside it.
    let untouched = filled_rects_in(&shapes, popup);
    let (_, shapes) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::ArrowDown)],
    );
    assert_eq!(
        filled_rects_in(&shapes, popup),
        untouched + 1,
        "the first `Down` highlights exactly the picked row"
    );
}

/// `Down` twice then `Tab` completes the second row into the field without
/// submitting (standard completion).
#[test]
fn down_and_tab_complete_without_navigating() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::ArrowDown)],
    );
    assert!(!submitted);
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::ArrowDown)],
    );
    assert!(!submitted, "moving the selection never navigates");
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::Tab)],
    );
    assert!(!submitted, "Tab completes, it never navigates");
    assert_eq!(tab.address, "ex.com/alpha/", "second row completed");
}

/// Esc closes the popup first (field keeps its text); a second Esc
/// reverts to the shown page like before.
#[test]
fn escape_closes_the_popup_before_reverting() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    tab.address = "ex.com/be".to_string();
    // First frame: the popup area runs its (invisible) sizing pass.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    assert!(painted(&shapes).iter().any(|t| t == "Beta"), "popup open");
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::Escape)],
    );
    assert_eq!(tab.address, "ex.com/be", "first Esc only closes the popup");
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    assert!(
        !painted(&shapes).iter().any(|t| t == "Beta"),
        "popup stays closed while editing"
    );
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![key(egui::Key::Escape)],
    );
    assert_eq!(tab.address, "", "second Esc reverts (Home has no page)");
}

/// Clicking a row navigates to it (mouse path, not just keyboard).
#[test]
fn clicking_a_suggestion_navigates_to_it() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    // Click the "Beta" row: the title text position sits inside it.
    let pos = text_pos(&shapes, "Beta") + egui::vec2(30.0, 8.0);
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![egui::Event::PointerMoved(pos), button(true)],
    );
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![button(false)],
    );
    assert!(submitted, "the row click navigates");
    assert_eq!(tab.address, "ex.com/beta/");
}

/// The popup is an overlay: the address field rect is identical with and
/// without it ("bars must not shift" contract, same as the toolbar).
#[test]
fn popup_never_moves_the_field() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let with_popup = ctx
        .read_response(address_id(1, "address-bar"))
        .expect("field registered")
        .rect;
    let empty = (HistoryStore::in_memory(), BookmarkStore::in_memory());
    let mut bare = Tab::new(2);
    let _ = frame(&ctx, &mut bare, &empty.0, &empty.1, true, Vec::new());
    let without_popup = ctx
        .read_response(address_id(2, "address-bar"))
        .expect("field registered")
        .rect;
    assert_eq!(with_popup, without_popup, "the popup shifted the field");
}

/// The framed popup is exactly as wide as the capsule: `Frame::popup`
/// pads its content with the theme menu margin, so sizing the content to
/// the capsule width overshoots ~12px on the right.
#[test]
fn popup_matches_the_capsule_width() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    // Settle: focus, then the popup Area past its sizing pass.
    for _ in 0..3 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    }
    let popup = popup_rect(&ctx);
    assert!(
        (popup.width() - 400.0).abs() < 0.5,
        "popup {w:.1}px wide for a 400px capsule",
        w = popup.width()
    );
}

/// The popup follows its content (three suggestions do not reserve the
/// eight-row maximum) and is capped by the viewport, with the rows living in
/// a scroll area instead of being clipped by the Area.
#[test]
fn popup_shrinks_to_its_rows_and_caps_to_the_viewport() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    for _ in 0..3 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    }
    let three_rows = popup_rect(&ctx).height();

    let mut many = BookmarkStore::in_memory();
    for i in 0..8 {
        let url = format!("https://ex.com/site-{i}/");
        let title = format!("Site {i}");
        many.toggle(&url, Some(&title));
    }
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    for _ in 0..3 {
        let _ = frame(&ctx, &mut tab, &history, &many, true, Vec::new());
    }
    let eight_rows = popup_rect(&ctx).height();
    assert!(
        three_rows + 20.0 < eight_rows,
        "a short list must not reserve the full popup: {three_rows:.1}px vs {eight_rows:.1}px"
    );

    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    for _ in 0..3 {
        let _ = frame_sized(&ctx, &mut tab, &history, &many, true, Vec::new(), 220.0);
    }
    let capped = popup_rect(&ctx).height();
    assert!(
        capped < eight_rows,
        "a short viewport must cap the popup: {capped:.1}px vs {eight_rows:.1}px"
    );
    assert!(
        capped <= 220.0,
        "the capped popup must stay inside the viewport: {capped:.1}px"
    );
}

/// A primary-button press/release event at `pos`.
fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The address field's stored cursor/selection range.
fn cursor_range(ctx: &egui::Context, id: egui::Id) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(ctx, id)
        .unwrap_or_default()
        .cursor
        .char_range()
        .expect("the field stored a cursor range")
}

/// Simulate one full click (press frame + release frame) at `pos`.
fn click(
    ctx: &egui::Context,
    tab: &mut Tab,
    history: &HistoryStore,
    bookmarks: &BookmarkStore,
    pos: egui::Pos2,
) {
    let _ = frame(
        ctx,
        tab,
        history,
        bookmarks,
        false,
        vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
    );
    let _ = frame(
        ctx,
        tab,
        history,
        bookmarks,
        false,
        vec![pointer_button(pos, false)],
    );
}

/// The first click on an unfocused field selects the whole address (typing
/// replaces it); a later click places the cursor at the click point.
#[test]
fn click_selects_all_then_positions_the_cursor() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    tab.address = "https://ex.com/alpha/beta/gamma".to_string();
    let id = address_id(1, "address-bar");
    for _ in 0..2 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    }
    let rect = ctx.read_response(id).unwrap().rect;
    // Click 1: focuses and selects all.
    click(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        egui::pos2(rect.left() + 60.0, rect.center().y),
    );
    let range = cursor_range(&ctx, id);
    let selected = range.as_sorted_char_range();
    assert_eq!(
        (selected.start.0, selected.end.0),
        (0, tab.address.chars().count()),
        "the focus click selects the whole address"
    );
    // Click 2: the field is already focused; the cursor lands on the click.
    click(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        egui::pos2(rect.left() + 130.0, rect.center().y),
    );
    let range = cursor_range(&ctx, id);
    assert!(
        range.is_empty(),
        "the second click places a collapsed cursor, not a selection"
    );
    assert!(
        range.primary.index.0 > 0,
        "the cursor is at the click point: {range:?}"
    );
}

/// A drag inside an unfocused field selects from the PRESS POINT: the
/// select-all-on-focus must not overwrite the press cursor at the end of the
/// press frame (it used to re-anchor every drag at character 0, so a partial
/// selection was impossible).
#[test]
fn drag_selects_from_the_press_point() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = Tab::new(1);
    tab.address = "https://ex.com/alpha/beta/gamma".to_string();
    let id = address_id(1, "address-bar");
    for _ in 0..2 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    }
    let rect = ctx.read_response(id).unwrap().rect;
    let press = egui::pos2(rect.left() + 150.0, rect.center().y);
    let drag_to = egui::pos2(rect.left() + 60.0, rect.center().y);
    // Press on the right, drag left, release.
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![
            egui::Event::PointerMoved(press),
            pointer_button(press, true),
        ],
    );
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![egui::Event::PointerMoved(drag_to)],
    );
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![pointer_button(drag_to, false)],
    );
    let range = cursor_range(&ctx, id);
    let selected = range.as_sorted_char_range();
    assert!(
        selected.start.0 > 0,
        "the selection is anchored at the press point, not at 0: {range:?}"
    );
    assert!(
        selected.end.0 < tab.address.chars().count(),
        "the drag stops at the release point: {range:?}"
    );
    // A click after the drag places the caret: the released arm must not
    // linger and re-select the whole address.
    click(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        egui::pos2(rect.left() + 130.0, rect.center().y),
    );
    let after = cursor_range(&ctx, id);
    assert!(
        after.is_empty(),
        "a click after a drag places a collapsed cursor: {after:?}"
    );
}

/// A pointer that merely SITS over the popup (it was already there when the
/// popup opened, or the list rebuilt under it while typing) must not count
/// as an explicit pick: pressing Enter after trimming the path used to
/// navigate to the highlighted full-path suggestion instead of the typed
/// intermediate address.
#[test]
fn stationary_pointer_over_the_popup_never_picks_a_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut bookmarks = BookmarkStore::in_memory();
    bookmarks.toggle("https://ex.com/alpha/", Some("Alpha"));
    let mut history = HistoryStore::in_memory();
    history.record("https://ex.com/alpha/beta/", Some("The Page"));
    let mut tab = Tab::new(1);
    tab.address = "ex.com/alpha/beta/".to_string();
    // The pointer already sits where row 1 of the popup will open, before
    // any popup exists (the user is about to press Ctrl/Cmd+L).
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![egui::Event::PointerMoved(egui::pos2(100.0, 109.0))],
    );
    // Ctrl/Cmd+L: focus; the popup opens under the stationary pointer.
    for _ in 0..3 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    }
    // Trim the path: the query now matches both the Alpha bookmark (row 0)
    // and the full-path history visit (row 1, under the pointer).
    tab.address = "ex.com/alpha".to_string();
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    // One more frame so the two-row popup is in the hit-test snapshot.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted, "Enter must submit");
    assert_eq!(
        tab.address, "ex.com/alpha",
        "the typed intermediate address wins over the hovered full-path row"
    );
}

/// Moving the pointer ONTO a row is still an explicit pick: Enter navigates
/// to it (the hover pick the popup documents).
#[test]
fn moving_the_pointer_onto_a_row_picks_it() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut bookmarks = BookmarkStore::in_memory();
    bookmarks.toggle("https://ex.com/alpha/", Some("Alpha"));
    let mut history = HistoryStore::in_memory();
    history.record("https://ex.com/alpha/beta/", Some("The Page"));
    let mut tab = Tab::new(1);
    tab.address = "ex.com/alpha".to_string();
    // Focus and settle the two-row popup with no pointer over it.
    for _ in 0..3 {
        let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    }
    // The pointer moves onto the second row (the full-path history visit).
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        true,
        vec![egui::Event::PointerMoved(egui::pos2(100.0, 109.0))],
    );
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted, "Enter must submit");
    assert_eq!(
        tab.address, "ex.com/alpha/beta/",
        "the row the pointer moved onto is the pick"
    );
}

/// The reported reload case: the shown page is in history, and the typed
/// address is a substring of it (trimming the URL by hand). Enter must
/// navigate to the typed text — it used to be replaced by the highlighted
/// history row, "reloading" the page already shown.
#[test]
fn enter_keeps_the_typed_address_over_its_history_match() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut bookmarks = BookmarkStore::in_memory();
    bookmarks.toggle("https://ex.com/alpha/", Some("Alpha"));
    let mut history = HistoryStore::in_memory();
    history.record("https://ex.com/alpha/beta/", Some("The Page"));
    let mut tab = Tab::new(1);
    tab.address = "ex.com/alpha/b".to_string();
    // Ctrl/Cmd+L focuses once, then the popup builds.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let (submitted, _) = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![key(egui::Key::Enter)],
    );
    assert!(submitted);
    assert_eq!(
        tab.address, "ex.com/alpha/b",
        "the typed address wins over its history match"
    );
}

/// A tab showing a LOADED document (no fetch, no network): the page-info
/// card reads `Tab::site_info`, so its tests need a real page state.
fn loaded_tab(id: usize, url: &str, title: Option<&str>) -> Tab {
    let mut fetched = crate::protocol::client::Fetched {
        display_url: url::Url::parse(url).expect("test URL"),
        meta: crate::markdown::frontmatter::Meta::default(),
        h1_title: None,
        plain_text: String::new(),
        source: String::new(),
        events: Vec::new(),
        anchors: crate::markdown::anchors::AnchorIndex::default(),
        outline: crate::markdown::outline::Outline::default(),
        dests: crate::protocol::rewrite::DestCache::default(),
        stale_copy: None,
    };
    fetched.meta.title = title.map(str::to_string);
    let mut tab = Tab::new(id);
    tab.address = crate::protocol::resolver::display_string(&fetched.display_url);
    tab.state = crate::browser::tab::TabState::Loaded { fetched };
    tab
}

/// The indicator center, after a settle frame registered its response.
fn indicator_pos(ctx: &egui::Context, tab_id: usize) -> egui::Pos2 {
    ctx.read_response(address_id(tab_id, "address-bar").with("indicator"))
        .expect("indicator registered")
        .rect
        .center()
}

/// Clicking the security indicator toggles a card with the REAL page
/// facts: the kind, the sanitized page title and the exact address
/// (scheme included). Clicking it again closes the card.
#[test]
fn indicator_click_toggles_the_page_info_card() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", Some("My Page"));
    // Settle: the indicator response exists after one frame.
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let pos = indicator_pos(&ctx, 1);
    // Closed at rest: none of the card's text is painted.
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    assert!(
        !painted(&shapes).iter().any(|t| t == "Secure connection"),
        "the card must not paint before the click"
    );
    // A half-edited address in the field must not change what the card
    // reports: it describes the page actually rendered, not the draft.
    tab.address = "ex.com/pa".to_string();
    // Open: kind + title + exact address.
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(tab.site_info_open(), "the click opened the card");
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let texts = painted(&shapes);
    assert!(
        texts.iter().any(|t| t == "Secure connection"),
        "card headline: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "My Page"),
        "the page's own title: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "https://ex.com/page/"),
        "the exact address keeps its scheme: {texts:?}"
    );
    // Second click: closed again.
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(!tab.site_info_open(), "the second click closed the card");
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    assert!(!painted(&shapes).iter().any(|t| t == "Secure connection"));
}

/// A page served from the cache's stale record never shows the padlock of a
/// current one: the glyph and the tooltip name the copy (cloud-off offline /
/// archive saved), and the page-info card's wording matches (`Tab::site_info`
/// is what the card paints).
#[test]
fn stale_copies_change_the_indicator_glyph_and_tip() {
    use crate::browser::tab::TabState;
    use crate::protocol::client::StaleCopy;
    use crate::ui::icons::Icon;

    fn set_stale(tab: &mut Tab, cause: StaleCopy) {
        if let TabState::Loaded { fetched } = &mut tab.state {
            fetched.stale_copy = Some(cause);
        } else {
            panic!("the fixture must be a loaded tab");
        }
    }

    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    assert_eq!(super::indicator_icon(&tab), Icon::Lock);
    assert_eq!(super::indicator_tip(&tab), "Secure connection (HTTPS only)");

    set_stale(&mut tab, StaleCopy::Offline);
    assert_eq!(super::indicator_icon(&tab), Icon::CloudOff);
    assert_eq!(super::indicator_tip(&tab), "Offline copy (on this device)");

    set_stale(&mut tab, StaleCopy::Saved);
    assert_eq!(super::indicator_icon(&tab), Icon::Archive);
    assert_eq!(super::indicator_tip(&tab), "Saved copy (on this device)");
}

/// Esc closes the card; the address field keeps its text (the key is
/// spent on the card, never on a field revert or a Stop).
#[test]
fn escape_closes_the_page_info_card_without_reverting_the_field() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let pos = indicator_pos(&ctx, 1);
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(tab.site_info_open());
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![key(egui::Key::Escape)],
    );
    assert!(!tab.site_info_open(), "Esc closed the card");
    assert_eq!(tab.address, "ex.com/page/", "the field was left alone");
    let (_, shapes) = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    assert!(!painted(&shapes).iter().any(|t| t == "Secure connection"));
}

/// Ctrl/Cmd+L (an explicit focus request) closes the card: the user moved
/// on to editing the address.
#[test]
fn focusing_the_address_closes_the_page_info_card() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let pos = indicator_pos(&ctx, 1);
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(tab.site_info_open());
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, true, Vec::new());
    assert!(
        !tab.site_info_open(),
        "focusing the address closed the card"
    );
}

/// A press anywhere outside the card (the field included) closes it in
/// the same frame, before it would paint again.
#[test]
fn a_press_outside_closes_the_page_info_card() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let pos = indicator_pos(&ctx, 1);
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(tab.site_info_open());
    // Press on the field (inside the capsule, outside the card).
    let field = ctx
        .read_response(address_id(1, "address-bar"))
        .expect("field registered")
        .rect;
    let outside = egui::pos2(field.left() + 80.0, field.center().y);
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![
            egui::Event::PointerMoved(outside),
            pointer_button(outside, true),
        ],
    );
    assert!(!tab.site_info_open(), "the outside press closed the card");
}

/// A press ON the card is swallowed (the card stays open and the click
/// never reaches the document behind the floating area).
#[test]
fn a_press_on_the_page_info_card_keeps_it_open() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let pos = indicator_pos(&ctx, 1);
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    assert!(tab.site_info_open());
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let card: egui::Rect = ctx
        .data_mut(|d| d.get_temp(address_id(1, "address-bar").with("site-info").with("rect")))
        .expect("the card rect was recorded");
    let inside = card.center();
    let _ = frame(
        &ctx,
        &mut tab,
        &history,
        &bookmarks,
        false,
        vec![
            egui::Event::PointerMoved(inside),
            pointer_button(inside, true),
        ],
    );
    assert!(tab.site_info_open(), "a press on the card closed it");
}

/// The card sits under the capsule (below the indicator) at its clamped
/// width, anchored to the capsule's left edge like a browser site panel.
#[test]
fn page_info_card_anchors_under_the_capsule() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let indicator = ctx
        .read_response(address_id(1, "address-bar").with("indicator"))
        .expect("indicator registered")
        .rect;
    let pos = indicator.center();
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let card: egui::Rect = ctx
        .data_mut(|d| d.get_temp(address_id(1, "address-bar").with("site-info").with("rect")))
        .expect("the card rect was recorded");
    assert!(
        card.min.y >= indicator.max.y,
        "the card hangs below the indicator: {card:?} vs {indicator:?}"
    );
    assert!(
        (card.min.x - 0.0).abs() < 1.0,
        "the card starts at the capsule's left edge: {card:?}"
    );
    assert!(
        (card.width() - 320.0).abs() < 1.5,
        "the card keeps its clamped width: {card:?}"
    );
}

/// The card is an overlay: it never shifts the capsule or the field (the
/// same "bars must not shift" contract as the suggestion popup).
#[test]
fn page_info_card_never_moves_the_field() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let (history, bookmarks) = stores();
    let mut tab = loaded_tab(1, "https://ex.com/page/", None);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let before = ctx
        .read_response(address_id(1, "address-bar"))
        .expect("field registered")
        .rect;
    let pos = indicator_pos(&ctx, 1);
    click(&ctx, &mut tab, &history, &bookmarks, pos);
    let _ = frame(&ctx, &mut tab, &history, &bookmarks, false, Vec::new());
    let after = ctx
        .read_response(address_id(1, "address-bar"))
        .expect("field registered")
        .rect;
    assert_eq!(before, after, "the card shifted the field");
}
