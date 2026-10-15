// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    grid_geometry, show_bookmarks, ACTION_GAP, ACTION_INSET, ACTION_SIZE, FAVICON, FIELD_PAD,
    ICON_SIZE, ICON_TOP, TILE_H, TITLE_BOTTOM_GUARD, TITLE_SIZE, TITLE_TOP,
};
use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// Home column width in the 900px test viewport.
const SECTION_W: f32 = 560.0;

/// Avatar-slot center of one tile, derived from its painted title.
fn icon_center(title_pos: egui::Pos2) -> egui::Pos2 {
    egui::pos2(
        title_pos.x,
        title_pos.y - TITLE_TOP + ICON_TOP + ICON_SIZE * 0.5,
    )
}

fn base_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    }
}

/// One rendered frame with the given events, laid out like the Home
/// page (the section runs inside the centered column); returns shapes.
fn render(
    ctx: &egui::Context,
    input: egui::RawInput,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) -> Vec<egui::epaint::ClippedShape> {
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            ui.vertical_centered(|ui| {
                let _ = show_bookmarks(ui, tab, store, favicons, SECTION_W);
            });
        });
    });
    out.textures_delta.clear();
    out.shapes
}

/// Position of the text shape exactly matching `text`, if painted.
fn text_pos(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<egui::Pos2> {
    shapes.iter().find_map(|c| match &c.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos),
        _ => None,
    })
}

/// Position and galley of the text shape exactly matching `text`.
fn text_galley(
    shapes: &[egui::epaint::ClippedShape],
    text: &str,
) -> Option<(egui::Pos2, std::sync::Arc<egui::epaint::Galley>)> {
    shapes.iter().find_map(|c| match &c.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some((t.pos, t.galley.clone())),
        _ => None,
    })
}

/// Center of one tile's action button, derived from the painted title
/// (centered text reports the tile center in `TextShape.pos.x`). The
/// buttons are stacked: X on top, pencil/check below.
fn action_center(title_pos: egui::Pos2, remove: bool) -> egui::Pos2 {
    let (_, tile_w) = grid_geometry(SECTION_W);
    let tile_left = title_pos.x - tile_w * 0.5;
    let x = tile_left + tile_w - ACTION_INSET - ACTION_SIZE * 0.5;
    let top = title_pos.y - TITLE_TOP + ACTION_INSET + ACTION_SIZE * 0.5;
    let y = if remove {
        top
    } else {
        top + ACTION_SIZE + ACTION_GAP
    };
    egui::pos2(x, y)
}

/// Full pointer click at `pos` (move, press, release).
fn click(
    ctx: &egui::Context,
    pos: egui::Pos2,
    modifiers: egui::Modifiers,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    };
    let moved = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::PointerMoved(pos),
        ],
        ..base_input()
    };
    let _ = render(ctx, moved, tab, store, favicons);
    let pressed = egui::RawInput {
        events: vec![egui::Event::ModifiersChanged(modifiers), button(true)],
        ..base_input()
    };
    let _ = render(ctx, pressed, tab, store, favicons);
    let released = egui::RawInput {
        events: vec![egui::Event::ModifiersChanged(modifiers), button(false)],
        ..base_input()
    };
    let _ = render(ctx, released, tab, store, favicons);
}

/// Drags from `from` to `to` in steps, so egui starts and tracks the
/// drag (press, move, release).
fn drag(
    ctx: &egui::Context,
    from: egui::Pos2,
    to: egui::Pos2,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) {
    let button = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let frame = |events: Vec<egui::Event>,
                 tab: &mut Tab,
                 store: &mut BookmarkStore,
                 favicons: &mut FaviconCache| {
        let input = egui::RawInput {
            events,
            ..base_input()
        };
        let _ = render(ctx, input, tab, store, favicons);
    };
    frame(vec![egui::Event::PointerMoved(from)], tab, store, favicons);
    frame(vec![button(from, true)], tab, store, favicons);
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let p = egui::pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        frame(vec![egui::Event::PointerMoved(p)], tab, store, favicons);
    }
    frame(vec![button(to, false)], tab, store, favicons);
    frame(Vec::new(), tab, store, favicons);
}

/// Types `text` into the focused field.
fn type_text(
    ctx: &egui::Context,
    text: &str,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) {
    let input = egui::RawInput {
        events: vec![egui::Event::Text(text.to_string())],
        ..base_input()
    };
    let _ = render(ctx, input, tab, store, favicons);
}

/// Presses one key (no modifiers).
fn press_key(
    ctx: &egui::Context,
    key: egui::Key,
    tab: &mut Tab,
    store: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) {
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..base_input()
    };
    let _ = render(ctx, input, tab, store, favicons);
}

/// The palette length and the store's color bound must stay in sync
/// (stored indexes are validated against `MAX_BOOKMARK_COLORS`).
#[test]
fn palette_matches_the_stored_color_bound() {
    assert_eq!(
        super::PALETTE.len(),
        crate::config::MAX_BOOKMARK_COLORS as usize
    );
}

/// The tile favicon must draw at the app's one favicon size (the
/// toolbar glyph side, shared with the tab rail), while the avatar
/// circle keeps a margin around it.
#[test]
fn tile_favicons_share_the_app_favicon_size() {
    assert_eq!(FAVICON, crate::ui::icons::GLYPH);
    // Const block (not a plain runtime `assert!`): both sides are
    // compile-time constants, so clippy wants the check evaluated then.
    const {
        assert!(
            FAVICON < ICON_SIZE,
            "the avatar circle must keep a margin around the favicon"
        );
    }
}

/// The section renders empty and populated; a long list is virtualized
/// and newest-first order is kept in reading order.
#[test]
fn bookmark_grid_renders() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();

    let mut out = ctx.run_ui(base_input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            ui.vertical_centered(|ui| {
                assert!(
                    show_bookmarks(ui, &mut tab, &mut store, &mut favicons, SECTION_W).is_none()
                );
            });
        });
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "empty section painted");

    for i in 0..40 {
        // Local files: no favicon probes (the test must not touch the
        // network); the titles are what the grid test asserts on.
        store.toggle(
            &format!("file:///tmp/page-{i}.md"),
            Some(&format!("Page {i}")),
        );
    }
    let mut out = ctx.run_ui(base_input(), |root| {
        egui::CentralPanel::default().show(root, |ui| {
            ui.vertical_centered(|ui| {
                assert!(
                    show_bookmarks(ui, &mut tab, &mut store, &mut favicons, SECTION_W).is_none()
                );
            });
        });
    });
    out.textures_delta.clear();
    let tiles = out
        .shapes
        .iter()
        .filter(|c| {
            matches!(
                &c.shape,
                egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Page ")
            )
        })
        .count();
    assert!(tiles >= 4, "virtualized tiles painted: {tiles}");
    // Newest first: with 5 columns, "Page 39" sits left of "Page 38"
    // on the first row (same y).
    let newest = text_pos(&out.shapes, "Page 39").expect("newest tile painted");
    let previous = text_pos(&out.shapes, "Page 38").expect("previous tile painted");
    assert!(
        (newest.x, newest.y) < (previous.x, previous.y),
        "bookmarks must list newest first ({newest:?} vs {previous:?})"
    );
    // Centered under the Home column: the first tile's center is half a
    // tile right of the section's left edge (never hugging the window).
    let (_, tile_w) = grid_geometry(SECTION_W);
    let section_left = (900.0 - SECTION_W) / 2.0;
    assert!(
        (newest.x - (section_left + tile_w * 0.5)).abs() < 1.0,
        "the grid must be centered under the address capsule: {newest:?}"
    );
}

/// A renamed tile renders without panicking and paints the custom name.
#[test]
fn customized_tiles_render() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/custom.md", Some("Site"));
    assert!(store.set_title(0, "Renamed"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(
        text_pos(&shapes, "Renamed").is_some(),
        "the custom name is painted"
    );
}

/// A title wider/taller than the tile's two-line name slot is ELIDED by
/// the layout (`…` on the second row) instead of being cut mid-glyph by
/// the clip rect (the char cap alone cannot know the real line count).
#[test]
fn long_titles_elide_instead_of_clipping() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    let long = "Un titulo de marcador extremadamente largo que ocupa muchas lineas de texto";
    store.toggle("file:///tmp/long.md", Some(long));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let galley = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Un titulo") => {
                Some(t.galley.clone())
            }
            _ => None,
        })
        .expect("the long title is painted");
    assert_eq!(galley.rows.len(), 2, "the title stays within two rows");
    assert!(
        galley.elided,
        "the over-long title is elided with an ellipsis"
    );
}

/// The name's TWO rows must fit inside the tile: the second row used to
/// spill past the tile's bottom edge, where the text clip rect shaved its
/// descenders and the hover highlight (the tile rect itself) cut through
/// the line. Measured against the real font metrics, not assumed: the
/// bundled Noto Sans row at `TITLE_SIZE` is ~1.362em.
#[test]
fn two_line_titles_fit_inside_the_tile() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle(
        "file:///tmp/two-lines.md",
        Some("Un titulo de marcador extremadamente largo que ocupa dos lineas"),
    );

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    // Pin the arithmetic the constants encode: two rows + the top offset
    // stay inside the tile, with the clip guard on top of that. Measured
    // AFTER a frame has run (the fonts only exist once a frame painted).
    let row_h = ctx.fonts_mut(|f| f.row_height(&egui::FontId::proportional(TITLE_SIZE)));
    assert!(
        TITLE_TOP + 2.0 * row_h <= TILE_H - TITLE_BOTTOM_GUARD,
        "two {TITLE_SIZE}pt rows ({row_h}px each) must fit the title box \
         (TITLE_TOP {TITLE_TOP} + 2 rows <= TILE_H {TILE_H} - guard {TITLE_BOTTOM_GUARD})"
    );

    // The painted title is the char-capped/elided string, so match the
    // layout by prefix (as the elide test does).
    let (pos, galley) = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Un titulo") => {
                Some((t.pos, t.galley.clone()))
            }
            _ => None,
        })
        .expect("the two-line title is painted");
    assert_eq!(galley.rows.len(), 2, "the title really uses two rows");
    // `pos.y` is the galley's top (the tile's top + `TITLE_TOP`), so the
    // text's bottom is `pos.y + height` and the clip box's bottom is
    // `tile top + TILE_H - guard`.
    let text_bottom = pos.y + galley.size().y;
    let clip_bottom = (pos.y - TITLE_TOP) + TILE_H - TITLE_BOTTOM_GUARD;
    assert!(
        text_bottom <= clip_bottom,
        "both rows must stay inside the tile's text box (text bottom {text_bottom} > clip bottom {clip_bottom})"
    );
}

/// Clicking a tile's ICON navigates the tab showing Home to the
/// bookmark (the avatar is a normal part of the tile, never an editor).
/// A local file URL keeps the test off the network (the load fails on
/// its own thread; only the state transition is asserted).
#[test]
fn tile_click_opens_the_bookmark() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///nonexistent-mdowser-test/notes.md", Some("Notes"));

    // Settle, then locate the title from its painted text.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Notes").expect("the tile title is painted");
    click(
        &ctx,
        icon_center(pos),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    tab.cancel_inflight();

    assert!(
        matches!(tab.state, TabState::Loading { .. }),
        "the icon click must navigate the tab"
    );
    assert!(
        tab.address.contains("notes.md"),
        "the tab navigates to the bookmark: {}",
        tab.address
    );
}

/// Cmd/Ctrl+click asks for a NEW tab instead of replacing Home: the
/// request travels up as the returned URL (only `App` creates tabs).
#[test]
fn command_click_returns_a_new_tab_request() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/cmd.md", Some("Cmd Page"));
    store.toggle("file:///tmp/plain.md", Some("Plain Page"));

    // Newest first: "Plain Page" is the first tile.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Plain Page").expect("tile title painted");
    let click_pos = egui::pos2(pos.x + 4.0, pos.y + 6.0);
    let command = egui::Modifiers::COMMAND;
    let button = |pressed| egui::Event::PointerButton {
        pos: click_pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: command,
    };
    for events in [
        vec![
            egui::Event::ModifiersChanged(command),
            egui::Event::PointerMoved(click_pos),
        ],
        vec![egui::Event::ModifiersChanged(command), button(true)],
    ] {
        let input = egui::RawInput {
            events,
            ..base_input()
        };
        let _ = render(&ctx, input, &mut tab, &mut store, &mut favicons);
    }
    // The release frame is the one that RETURNS the new-tab request:
    // capture it instead of discarding it (a later frame returns None).
    let released = egui::RawInput {
        events: vec![egui::Event::ModifiersChanged(command), button(false)],
        ..base_input()
    };
    let mut requested = None;
    let mut out = ctx.run_ui(released, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            ui.vertical_centered(|ui| {
                requested = show_bookmarks(ui, &mut tab, &mut store, &mut favicons, SECTION_W);
            });
        });
    });
    out.textures_delta.clear();

    assert_eq!(
        requested.as_deref(),
        Some("file:///tmp/plain.md"),
        "command click asks App for a new tab"
    );
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "the Home tab itself must not navigate"
    );
}

/// The hover X removes the bookmark (and only it), without navigating.
#[test]
fn remove_button_deletes_the_bookmark() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/keep.md", Some("Keep Me"));
    store.toggle("file:///tmp/remove.md", Some("Remove Me"));

    // Newest first: "Remove Me" is the first tile.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Remove Me").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, true),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    assert_eq!(store.entries().len(), 1, "the X removed one bookmark");
    assert_eq!(store.entries()[0].title.as_deref(), Some("Keep Me"));
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "removal must not navigate"
    );
}

/// Keyboard chain through a tile's action buttons: with a tile focused,
/// `Tab` moves into the pencil and then onto the X (the hover-only buttons
/// stay allocated on the frame that moves focus, so the traversal can land
/// on them), and `Enter` on the focused X removes the bookmark. Without the
/// chain the first Tab after the tile jumped straight to the NEXT tile and
/// Enter opened its page instead.
#[test]
fn tab_reaches_the_tile_action_buttons() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/one.md", Some("One"));
    store.toggle("file:///tmp/two.md", Some("Two"));
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    let key = |k: egui::Key| egui::Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let press = |ctx: &egui::Context,
                 event: egui::Event,
                 tab: &mut Tab,
                 store: &mut BookmarkStore,
                 favicons: &mut FaviconCache| {
        let mut input = base_input();
        input.events = vec![event];
        let _ = render(ctx, input, tab, store, favicons);
    };
    // Tab 1: focus the first (newest) tile; tab 2: its pencil; tab 3: its X.
    for _ in 0..3 {
        press(
            &ctx,
            key(egui::Key::Tab),
            &mut tab,
            &mut store,
            &mut favicons,
        );
    }
    assert_eq!(
        store.entries().len(),
        2,
        "Tab alone must never activate a tile"
    );
    // Enter on the focused X removes that tile (keyboard activation).
    press(
        &ctx,
        key(egui::Key::Enter),
        &mut tab,
        &mut store,
        &mut favicons,
    );
    assert_eq!(
        store.entries().len(),
        1,
        "Enter on the focused X must remove the bookmark"
    );
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "the Enter must not have opened the tile instead"
    );
}

/// The pencil opens an inline rename field ON the tile (no window); the
/// tab flag keeps Esc for the field, and Enter commits the draft.
#[test]
fn pencil_opens_the_inline_rename_and_enter_commits() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    // The field is open on the tile, prefilled with the current title
    // (typing replaces it: the opening frame selects the whole draft),
    // owns the keyboard and never navigates.
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(
        text_pos(&shapes, "Editable").is_some(),
        "the inline rename draft is painted on the tile"
    );
    assert!(tab.tile_editing, "the field owns Esc while open");
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "editing must not navigate"
    );

    type_text(&ctx, "Mis notas", &mut tab, &mut store, &mut favicons);
    press_key(&ctx, egui::Key::Enter, &mut tab, &mut store, &mut favicons);

    // The exact result (not "EditableMis notas") proves the opening
    // frame selected the whole draft.
    assert_eq!(store.entries()[0].title.as_deref(), Some("Mis notas"));
    assert!(!tab.tile_editing, "Enter closes the field");
    // The closed field must STAY closed: the egui-memory write is
    // keyed by TYPE, and an `Option`/bare mismatch used to resurrect
    // the old draft on the next frame.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(!tab.tile_editing, "the field must not reopen");
}

/// The inline rename draft must paint at the tile name's own size: the
/// field fell back to `TextStyle::Body` (16px here), so opening the
/// editor read as a much bigger font than the name it replaces.
#[test]
fn inline_rename_paints_at_the_tile_name_size() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    // While the field is open the draft is the tile's only "Editable"
    // text (the title is replaced by the field).
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let (_, galley) = text_galley(&shapes, "Editable").expect("the draft is painted on the tile");
    assert_eq!(
        galley.job.sections[0].format.font_id.size, TITLE_SIZE,
        "the rename draft keeps the name's size, not the 16px Body default"
    );
}

/// Esc drops the draft: the previous title survives and the field
/// closes (App leaves Esc to the field via `Tab::tile_editing`).
#[test]
fn escape_cancels_the_inline_rename() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    type_text(&ctx, "Changed", &mut tab, &mut store, &mut favicons);
    press_key(&ctx, egui::Key::Escape, &mut tab, &mut store, &mut favicons);

    assert_eq!(
        store.entries()[0].title.as_deref(),
        Some("Editable"),
        "Esc must not commit the draft"
    );
    assert!(!tab.tile_editing, "Esc closes the field");
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(!tab.tile_editing, "the field must not reopen");
}

/// A click anywhere else commits the draft (blur = save).
#[test]
fn clicking_away_commits_the_inline_rename() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    type_text(&ctx, "Guardada", &mut tab, &mut store, &mut favicons);
    // Far below the grid: empty page space.
    click(
        &ctx,
        egui::pos2(450.0, 650.0),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    assert_eq!(store.entries()[0].title.as_deref(), Some("Guardada"));
    assert!(!tab.tile_editing, "the click away closed the field");
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(!tab.tile_editing, "the field must not reopen");
}

/// Clicking the pencil while the field is open (the save check)
/// commits too: "Enter or the pencil again", per the contract.
#[test]
fn pencil_again_saves_the_inline_rename() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    let pencil = action_center(pos, false);
    click(
        &ctx,
        pencil,
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    type_text(&ctx, "Con lapiz", &mut tab, &mut store, &mut favicons);

    // The same spot now holds the save check: click it again.
    click(
        &ctx,
        pencil,
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    assert_eq!(store.entries()[0].title.as_deref(), Some("Con lapiz"));
    assert!(!tab.tile_editing, "the save check closed the field");
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    assert!(!tab.tile_editing, "the field must not reopen");
}

/// Dragging a tile reorders the grid (and never opens the page).
#[test]
fn dragging_a_tile_reorders_the_grid() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for (i, name) in ["Uno", "Dos", "Tres"].iter().enumerate() {
        store.toggle(&format!("file:///tmp/page-{i}.md"), Some(name));
    }

    // Newest first: Tres, Dos, Uno.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let tres = icon_center(text_pos(&shapes, "Tres").expect("first tile painted"));
    let uno = icon_center(text_pos(&shapes, "Uno").expect("last tile painted"));
    drag(&ctx, tres, uno, &mut tab, &mut store, &mut favicons);

    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "a drag must never open the page"
    );
    // Store order is oldest first: the dragged first display item lands
    // last on screen, so it becomes FIRST in the store.
    let titles: Vec<String> = store
        .entries()
        .iter()
        .map(|b| b.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles, vec!["Tres", "Uno", "Dos"]);

    // A stale drag (the same type-keyed egui-memory trap as the field)
    // would keep reordering while the pointer moves AFTER the release.
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let target = icon_center(text_pos(&shapes, "Uno").expect("tile painted"));
    let moved = egui::RawInput {
        events: vec![egui::Event::PointerMoved(target)],
        ..base_input()
    };
    let _ = render(&ctx, moved, &mut tab, &mut store, &mut favicons);
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let after: Vec<String> = store
        .entries()
        .iter()
        .map(|b| b.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(after, titles, "the drag state must clear on release");
}

/// Alt+Left/Right reorder the FOCUSED tile one display slot: the keyboard
/// equivalent of the drag reorder (the grid used to be pointer-only). The
/// mapping mirrors the newest-first display order onto the
/// insertion-ordered store.
#[test]
fn keyboard_move_maps_display_slots_to_store_indices() {
    // Store [A,B,C] shows as [C,B,A].
    assert_eq!(
        super::move_indices(3, 0, 1),
        Some((2, 1)),
        "C one slot right lands at store 1 -> [A,C,B]"
    );
    assert_eq!(
        super::move_indices(3, 1, -1),
        Some((1, 2)),
        "B one slot left lands at store 2 -> [A,C,B]"
    );
    // The ends do not wrap: a move past the first/last slot is a no-op.
    assert_eq!(super::move_indices(3, 0, -1), None);
    assert_eq!(super::move_indices(3, 2, 1), None);
    assert_eq!(super::move_indices(1, 0, 1), None);
}

/// The focused tile reorders with Alt+Right exactly like a drag (and never
/// navigates).
#[test]
fn alt_right_moves_the_focused_tile_one_slot() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for (i, name) in ["Uno", "Dos", "Tres"].iter().enumerate() {
        store.toggle(&format!("file:///tmp/page-{i}.md"), Some(name));
    }
    // Two frames so the tiles are in egui's focus order, then Tab focuses
    // the first tile drawn (the newest one, "Tres").
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    press_key(&ctx, egui::Key::Tab, &mut tab, &mut store, &mut favicons);
    // `InputState::modifiers` only follows `Event::ModifiersChanged` (the
    // modifiers carried BY a key event are ignored), exactly as winit
    // reports a real Alt press.
    let input = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers::ALT),
            egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::ALT,
            },
        ],
        ..base_input()
    };
    let _ = render(&ctx, input, &mut tab, &mut store, &mut favicons);
    let titles: Vec<String> = store
        .entries()
        .iter()
        .map(|b| b.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles, vec!["Uno", "Tres", "Dos"], "Tres moved one slot");
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "a keyboard move must never open the page"
    );
}

/// Dragging DOWN into the second row reorders across rows (the drop
/// target is the tile under the pointer, whatever its row).
#[test]
fn dragging_across_rows_reorders_the_grid() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    for i in 0..7 {
        store.toggle(&format!("file:///tmp/row-{i}.md"), Some(&format!("P{i}")));
    }

    // Display, newest first: P6 P5 P4 P3 P2 / P1 P0 (two rows of five).
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let newest = icon_center(text_pos(&shapes, "P6").expect("first tile painted"));
    let below = icon_center(text_pos(&shapes, "P1").expect("second-row tile painted"));
    assert!(below.y > newest.y, "the second row really is below");
    drag(&ctx, newest, below, &mut tab, &mut store, &mut favicons);

    // P6 leaves display 0 and lands where P1 was (display 5).
    let titles: Vec<String> = store
        .entries()
        .iter()
        .map(|b| b.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        titles,
        vec!["P0", "P6", "P1", "P2", "P3", "P4", "P5"],
        "cross-row drag must keep every entry exactly once"
    );
    assert!(
        !matches!(tab.state, TabState::Loading { .. }),
        "a drag must never open the page"
    );
}

/// The grid's viewport must contain the WHOLE tile: the pre-reserved
/// section block used to be 4px shorter than its own header, so the
/// ScrollArea's viewport clipped the tile's bottom. The hover wash IS the
/// tile rect, so a short viewport showed as a cut hover highlight (and cut
/// the inline rename field's bottom edge).
#[test]
fn the_grid_viewport_contains_the_whole_tile() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/hovered.md", Some("Notes"));

    // Settle, then put the pointer on the tile: the wash is painted.
    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Notes").expect("the tile title is painted");
    let hover = egui::pos2(pos.x, pos.y - TITLE_TOP + ICON_TOP + ICON_SIZE * 0.5);
    let input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(hover)],
        ..base_input()
    };
    let shapes = render(&ctx, input, &mut tab, &mut store, &mut favicons);

    let (tile, clip) = tile_wash(&shapes).expect("the hovered tile paints its wash");
    assert!(
        clip.contains_rect(tile),
        "the hover wash must not be clipped (wash {tile:?}, viewport {clip:?})"
    );
}

/// The inline rename field is a compact one-line pill inside the name
/// slot: it used to fill the whole two-line slot, which left a wide band
/// of empty input above and below the single line of text.
#[test]
fn the_inline_field_hugs_the_draft_inside_the_tile() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    let pencil = action_center(pos, false);
    click(
        &ctx,
        pencil,
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );

    // The pointer stays on the edited tile, so the wash (the tile rect) is
    // painted: use it as the tile reference. The field's own surface is the
    // only other rect as wide as the name slot (tile - 2 * 4px inset).
    let input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(pencil)],
        ..base_input()
    };
    let shapes = render(&ctx, input, &mut tab, &mut store, &mut favicons);
    let (tile, clip) = tile_wash(&shapes).expect("the edited tile paints its wash");
    assert!(clip.contains_rect(tile), "the tile must not be clipped");
    let field = inline_field_surface(&shapes).expect("the inline field paints its input surface");
    assert!(
        tile.contains_rect(field),
        "the field must sit inside the tile (field {field:?}, tile {tile:?})"
    );
    assert!(
        field.height() < TILE_H - TITLE_TOP - TITLE_BOTTOM_GUARD,
        "the field hugs one line instead of filling the two-line slot: {}px",
        field.height()
    );
}

/// The hover wash: the painted rect as wide as a tile and as tall as
/// `TILE_H`, with the viewport clip it was painted under.
fn tile_wash(shapes: &[egui::epaint::ClippedShape]) -> Option<(egui::Rect, egui::Rect)> {
    let (_, tile_w) = grid_geometry(SECTION_W);
    shapes.iter().find_map(|c| match &c.shape {
        egui::epaint::Shape::Rect(r)
            if (r.rect.width() - tile_w).abs() < 1.0 && (r.rect.height() - TILE_H).abs() < 1.0 =>
        {
            Some((r.rect, c.clip_rect))
        }
        _ => None,
    })
}

/// A draft wider than the field must scroll so the CARET (the end being
/// typed) stays in view: centered text that no longer fit sat still and
/// showed a cut-off middle, so the end was invisible while typing.
#[test]
fn a_long_draft_scrolls_so_the_caret_stays_in_view() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    // One event: the whole draft is replaced, the caret lands at its END.
    let long = "Un nombre de marcador bastante largo";
    type_text(&ctx, long, &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    let field = inline_field_surface(&shapes).expect("the field paints its input surface");
    let (text_pos, galley) = text_galley(&shapes, long).expect("the draft is painted");
    // Left-aligned once it overflows, scrolled so its right edge sits on
    // the field's inner right edge: the caret is on screen.
    let right = text_pos.x + galley.size().x;
    assert!(
        (right - (field.right() - FIELD_PAD)).abs() < 1.0,
        "the draft must scroll to its end (right edge {right}, field inner right {})",
        field.right() - FIELD_PAD
    );
    assert!(
        text_pos.x < field.left(),
        "the draft really scrolled left (draft {text_pos:?}, field {field:?})"
    );
}

/// While the draft fits, it stays centered: the field opens on the centered
/// name it replaces, so only an overflowing draft may jump to the left.
#[test]
fn a_fitting_draft_stays_centered() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    let mut store = BookmarkStore::in_memory();
    let mut favicons = FaviconCache::new();
    store.toggle("file:///tmp/editable.md", Some("Editable"));

    let _ = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);
    let pos = text_pos(&shapes, "Editable").expect("tile title painted");
    click(
        &ctx,
        action_center(pos, false),
        egui::Modifiers::NONE,
        &mut tab,
        &mut store,
        &mut favicons,
    );
    let shapes = render(&ctx, base_input(), &mut tab, &mut store, &mut favicons);

    let field = inline_field_surface(&shapes).expect("the field paints its input surface");
    let (text_pos, _) = text_galley(&shapes, "Editable").expect("the draft is painted");
    // Centered galleys report their CENTER in the shape position.
    assert!(
        (text_pos.x - field.center().x).abs() < 1.0,
        "a fitting draft stays centered (draft center {text_pos:?}, field center {:?})",
        field.center()
    );
}

/// The editing alignment decision: centered until the draft overflows,
/// left-aligned (egui's caret-follow path) once it does.
#[test]
fn draft_alignment_centers_until_the_text_overflows() {
    assert_eq!(super::tile::draft_align(50.0, 86.0), egui::Align::Center);
    assert_eq!(super::tile::draft_align(86.0, 86.0), egui::Align::Center);
    assert_eq!(super::tile::draft_align(86.5, 86.0), egui::Align::Min);
}

/// The painted input surface of the inline rename field (as wide as the
/// name slot and shorter than the tile).
fn inline_field_surface(shapes: &[egui::epaint::ClippedShape]) -> Option<egui::Rect> {
    let (_, tile_w) = grid_geometry(SECTION_W);
    shapes.iter().find_map(|c| match &c.shape {
        egui::epaint::Shape::Rect(r)
            if (r.rect.width() - (tile_w - 8.0)).abs() < 1.0 && r.rect.height() < 40.0 =>
        {
            Some(r.rect)
        }
        _ => None,
    })
}
