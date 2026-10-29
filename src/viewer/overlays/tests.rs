// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{content_menu, hovered_link, link_at, show_link_status};
use crate::browser::tab::{Tab, TabState};
use crate::protocol::client::Fetched;
use crate::viewer::LinkHit;
use eframe::egui;

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))
}

fn fetched_for(url: &str) -> Fetched {
    Fetched {
        display_url: url::Url::parse(url).unwrap(),
        meta: crate::markdown::frontmatter::Meta::default(),
        h1_title: None,
        plain_text: "# Hi\n\nbody text\n".to_string(),
        source: "# Hi\n\nbody text\n".to_string().into(),
        events: Vec::new(),
        anchors: crate::markdown::anchors::AnchorIndex::default(),
        outline: crate::markdown::outline::Outline::default(),
        dests: crate::protocol::rewrite::DestCache::default(),
        stale_copy: None,
    }
}

/// A loaded tab with the menu forced open (and pinned to this viewport).
fn loaded_tab(context_link: Option<&str>) -> Tab {
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    tab.doc_rect = Some(screen());
    tab.context_open = true;
    tab.context_pos = Some(egui::pos2(200.0, 180.0));
    tab.context_viewport = Some(screen());
    tab.context_link = context_link.map(str::to_string);
    tab
}

/// Screen position of the painted menu row labelled `label`.
fn row_position(ctx: &egui::Context, tab: &mut Tab, label: &str) -> egui::Pos2 {
    render_menu_positions(ctx, tab, Vec::new())
        .into_iter()
        .find(|(t, _)| t == label)
        .map_or_else(|| panic!("row {label:?} painted"), |(_, pos)| pos)
}

/// One completed primary click at `pos` (press + release, like a real
/// gesture) in one menu frame.
fn click_row(ctx: &egui::Context, tab: &mut Tab, pos: egui::Pos2) {
    menu_frame(
        ctx,
        tab,
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, egui::PointerButton::Primary, true),
            press(pos, egui::PointerButton::Primary, false),
        ],
    );
}

#[test]
fn link_at_finds_the_hitbox_containing_the_pointer() {
    let hits = vec![
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(60.0, 30.0)),
            url: std::sync::Arc::from("https://ex.com/a"),
            hovered: false,
        },
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(70.0, 10.0), egui::pos2(120.0, 30.0)),
            url: std::sync::Arc::from("https://ex.com/b"),
            hovered: false,
        },
    ];
    assert_eq!(
        link_at(&hits, egui::pos2(20.0, 20.0)),
        Some("https://ex.com/a")
    );
    assert_eq!(
        link_at(&hits, egui::pos2(100.0, 20.0)),
        Some("https://ex.com/b")
    );
    assert_eq!(link_at(&hits, egui::pos2(65.0, 20.0)), None, "gap");
    assert_eq!(link_at(&hits, egui::pos2(20.0, 50.0)), None, "below");
}

/// ONE menu frame with the given pointer events, returning each painted
/// text with its position. TWO passes on purpose: the first area frame is
/// an INVISIBLE sizing pass (egui measures the popup).
fn render_menu_positions(
    ctx: &egui::Context,
    tab: &mut Tab,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        events,
        ..Default::default()
    };
    let mut first = ctx.run_ui(input.clone(), |root| {
        let url = content_menu(root, tab);
        assert!(url.is_none(), "rendering alone clicks nothing");
    });
    first.textures_delta.clear();
    let mut out = ctx.run_ui(input, |root| {
        let url = content_menu(root, tab);
        assert!(url.is_none(), "rendering alone clicks nothing");
    });
    let mut texts = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
            _ => None,
        })
        .collect::<Vec<_>>();
    out.textures_delta.clear();
    texts.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.y.total_cmp(&b.1.y)));
    texts
}

/// Menu rows keep their geometry in every pointer state: the labels are
/// painted into fixed rects, so hovering or pressing a row never moves
/// the popup (the old `selectable_label`-style rows grew their frame on
/// hover and shifted the list).
#[test]
fn menu_rows_keep_their_geometry_on_hover() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(Some("https://ex.com/other/"));
    let mut baseline = Vec::new();
    for _ in 0..3 {
        baseline = render_menu_positions(&ctx, &mut tab, Vec::new());
    }
    assert!(baseline.len() >= 3, "menu painted its rows: {baseline:?}");

    let anchor = |label: &str| {
        baseline
            .iter()
            .find(|(t, _)| t == label)
            .unwrap_or_else(|| panic!("row {label:?} painted"))
            .1
    };
    for label in [
        "Open link in new tab",
        "Copy link address",
        "Copy",
        "Select all",
    ] {
        let pos = anchor(label);
        let hovered = render_menu_positions(&ctx, &mut tab, vec![egui::Event::PointerMoved(pos)]);
        assert_eq!(hovered, baseline, "hovering {label:?} moved the menu");
        // Press (the active state) without releasing: the row must not
        // change shape, and a press alone never triggers the action.
        let pressed = render_menu_positions(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(pressed, baseline, "pressing {label:?} moved the menu");
    }
}

#[test]
fn link_at_prefers_the_last_hit_on_overlap() {
    let hits = vec![
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(50.0, 50.0)),
            url: std::sync::Arc::from("https://ex.com/first"),
            hovered: false,
        },
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(40.0, 40.0)),
            url: std::sync::Arc::from("https://ex.com/last"),
            hovered: false,
        },
    ];
    assert_eq!(
        link_at(&hits, egui::pos2(20.0, 20.0)),
        Some("https://ex.com/last"),
        "later (more specific) hits win"
    );
}

#[test]
fn hovered_link_ignores_idle_fragments_and_prefers_the_last() {
    let hits = vec![
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(50.0, 50.0)),
            url: std::sync::Arc::from("https://ex.com/idle"),
            hovered: false,
        },
        LinkHit {
            rect: egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(40.0, 40.0)),
            url: std::sync::Arc::from("https://ex.com/hovered"),
            hovered: true,
        },
    ];
    assert_eq!(hovered_link(&hits), Some("https://ex.com/hovered"));
    assert_eq!(hovered_link(&hits[..1]), None, "no fragment hovered");
}

/// Text runs painted by one `show_link_status` call. Two passes: the first
/// `Area` frame is an invisible sizing pass, exactly like the menu/toast.
fn status_texts(ctx: &egui::Context, tab: &Tab, events: Vec<egui::Event>) -> Vec<String> {
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        events,
        ..Default::default()
    };
    let mut first = ctx.run_ui(input.clone(), |root| show_link_status(root, tab));
    first.textures_delta.clear();
    let mut out = ctx.run_ui(input, |root| show_link_status(root, tab));
    let texts = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    out.textures_delta.clear();
    texts
}

fn tab_with_hovered_link(url: &str) -> Tab {
    let mut tab = Tab::new(1);
    tab.doc_rect = Some(screen());
    tab.link_hits.push(LinkHit {
        rect: egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(60.0, 30.0)),
        url: std::sync::Arc::from(url),
        hovered: true,
    });
    tab
}

/// Hovering a link surfaces its resolved destination before the click, so a
/// link whose text hides its target can be checked first.
#[test]
fn hovering_a_link_shows_its_destination() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let tab = tab_with_hovered_link("https://ex.com/real/target?q=1");
    let texts = status_texts(&ctx, &tab, Vec::new());
    assert!(
        texts.iter().any(|t| t == "https://ex.com/real/target?q=1"),
        "destination chip missing: {texts:?}"
    );
}

/// The chip belongs to a clean reading surface: an open context menu or a
/// pressed pointer gesture (starting a drag selection) hides it.
#[test]
fn link_status_yields_to_the_context_menu_and_pointer_gestures() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = tab_with_hovered_link("https://ex.com/real/target");
    tab.context_open = true;
    assert!(
        status_texts(&ctx, &tab, Vec::new()).is_empty(),
        "an open context menu owns the overlay"
    );
    tab.context_open = false;
    let press = egui::Event::PointerButton {
        pos: egui::pos2(20.0, 20.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    assert!(
        status_texts(&ctx, &tab, vec![press]).is_empty(),
        "a pressed pointer is a selection gesture, not a hover"
    );
}

/// Right-clicking a link offers its address; the row copies exactly the
/// resolved URL and confirms with a toast, without navigating.
#[test]
fn copy_link_address_copies_the_destination() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let url = "https://ex.com/other/?a=1";
    let mut tab = loaded_tab(Some(url));
    settle_menu(&ctx, &mut tab);
    let pos = row_position(&ctx, &mut tab, "Copy link address");
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events: vec![
                egui::Event::PointerMoved(pos),
                press(pos, egui::PointerButton::Primary, true),
                press(pos, egui::PointerButton::Primary, false),
            ],
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, &mut tab);
        },
    );
    out.textures_delta.clear();
    let copied = out
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(copied, Some(url), "the resolved address copied verbatim");
    assert!(!tab.context_open, "the action closes the menu");
    assert_eq!(tab.toast_text.as_deref(), Some("Link address copied"));
}

/// One frame of `content_menu` with the given pointer events.
fn menu_frame(ctx: &egui::Context, tab: &mut Tab, events: Vec<egui::Event>) {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, tab);
        },
    );
    out.textures_delta.clear();
}

/// A realistic open → Copy → reopen sequence with the real pointer paths.
#[test]
fn realistic_open_copy_reopen() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    tab.doc_rect = Some(screen());
    let click = |pos: egui::Pos2, button: egui::PointerButton| {
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, button, true),
            press(pos, button, false),
        ]
    };
    // 1. Open with a secondary click.
    menu_frame(
        &ctx,
        &mut tab,
        click(egui::pos2(200.0, 180.0), egui::PointerButton::Secondary),
    );
    settle_menu(&ctx, &mut tab);
    assert!(tab.context_open, "opened");
    // 2. Click the Copy row.
    let positions = render_menu_positions(&ctx, &mut tab, Vec::new());
    let copy_pos = positions
        .iter()
        .find(|(t, _)| t == "Copy")
        .expect("Copy row painted")
        .1;
    menu_frame(
        &ctx,
        &mut tab,
        click(copy_pos, egui::PointerButton::Primary),
    );
    assert!(!tab.context_open, "Copy closes the menu");
    assert_eq!(tab.toast_text.as_deref(), Some("Text copied"));
    // 3. Right-click elsewhere: the menu must open fresh at the new spot.
    let second = egui::pos2(500.0, 400.0);
    menu_frame(
        &ctx,
        &mut tab,
        click(second, egui::PointerButton::Secondary),
    );
    assert!(tab.context_open, "right-click reopens the menu");
    assert_eq!(tab.context_pos, Some(second), "anchored at the new click");
}

/// A right-click away from the popup re-anchors it at the new position
/// (standard browser behavior): the old rows vanish and new ones appear
/// under the cursor.
#[test]
fn right_click_elsewhere_reanchors_the_menu() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    let before = render_menu_positions(&ctx, &mut tab, Vec::new());
    let copy_before = before
        .iter()
        .find(|(t, _)| t == "Copy")
        .expect("Copy row painted")
        .1;
    let target = egui::pos2(600.0, 450.0);
    let after = render_menu_positions(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(target),
            press(target, egui::PointerButton::Secondary, true),
            press(target, egui::PointerButton::Secondary, false),
        ],
    );
    assert!(tab.context_open, "the menu stays open (re-anchored)");
    assert_eq!(tab.context_pos, Some(target), "anchored at the new click");
    let copy_after = after
        .iter()
        .find(|(t, _)| t == "Copy")
        .expect("Copy row painted after the move")
        .1;
    assert!(
        copy_after.y > copy_before.y + 100.0 && copy_after.x > copy_before.x + 100.0,
        "rows must move to the new anchor: {copy_before:?} -> {copy_after:?}"
    );
}

/// A right-click whose press was intercepted before egui (a text selection
/// was alive, see `MdowserApp::raw_input_hook`) opens the menu from
/// `Tab::pending_context_click`, not from `secondary_clicked`.
#[test]
fn pending_context_click_opens_the_menu() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    tab.context_open = false;
    let at = egui::pos2(300.0, 220.0);
    tab.pending_context_click = Some(at);
    menu_frame(&ctx, &mut tab, Vec::new());
    assert!(tab.context_open, "the pending click opens the menu");
    assert_eq!(tab.context_pos, Some(at), "anchored at the pending click");
    assert!(
        tab.pending_context_click.is_none(),
        "the pending click is consumed"
    );
}

/// The keyboard path must not leave a row focused after activating it:
/// otherwise the next pointer open would ring that row again.
#[test]
fn keyboard_activation_does_not_re_ring_on_reopen() {
    use egui::Key;
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    let tab_id = tab.id;
    let row_id = |index: usize| egui::Id::unique(("doc-menu-row", tab_id, index));
    settle_menu(&ctx, &mut tab);
    let key = |key: Key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    // Down focuses Copy, Enter activates it (whole-page copy; closes).
    menu_frame(&ctx, &mut tab, vec![key(Key::ArrowDown)]);
    assert!(ctx.memory(|m| m.has_focus(row_id(0))), "Down focuses Copy");
    menu_frame(&ctx, &mut tab, vec![key(Key::Enter)]);
    assert!(!tab.context_open, "Enter closes the menu");
    // The closed menu surrenders the row when it is next visited (the same
    // frame kept the row alive to process the Enter).
    menu_frame(&ctx, &mut tab, Vec::new());
    assert!(
        !ctx.memory(|m| (0..2).any(|i| m.has_focus(row_id(i)))),
        "a closed menu must surrender its focused row"
    );
    // Pointer reopen: no row may be ringed again.
    let pos = egui::pos2(200.0, 180.0);
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, egui::PointerButton::Secondary, true),
            press(pos, egui::PointerButton::Secondary, false),
        ],
    );
    settle_menu(&ctx, &mut tab);
    assert!(tab.context_open, "the menu reopened");
    assert!(
        !ctx.memory(|m| (0..2).any(|i| m.has_focus(row_id(i)))),
        "the reopened menu must not ring a remembered row"
    );
}

/// Lays out the popup (sizing pass + one painted frame).
fn settle_menu(ctx: &egui::Context, tab: &mut Tab) {
    for _ in 0..2 {
        menu_frame(ctx, tab, Vec::new());
    }
}

fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A plain primary click outside the popup closes the open menu.
#[test]
fn outside_primary_click_closes_the_menu() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    let pos = egui::pos2(700.0, 500.0);
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, egui::PointerButton::Primary, true),
            press(pos, egui::PointerButton::Primary, false),
        ],
    );
    assert!(
        !tab.context_open,
        "an outside primary click must close the menu"
    );
}

/// A press-drag-release outside the popup (starting a text selection) also
/// closes the menu: the old code only dismissed on a completed CLICK, so
/// dragging never closed it.
#[test]
fn outside_drag_closes_the_menu() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    let from = egui::pos2(600.0, 500.0);
    let to = egui::pos2(300.0, 420.0);
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(from),
            press(from, egui::PointerButton::Primary, true),
        ],
    );
    menu_frame(&ctx, &mut tab, vec![egui::Event::PointerMoved(to)]);
    menu_frame(
        &ctx,
        &mut tab,
        vec![press(to, egui::PointerButton::Primary, false)],
    );
    assert!(
        !tab.context_open,
        "a drag starting outside the menu must close it"
    );
}

/// Opening the menu with the pointer must not leave a row ringed: the old
/// code focused the first row (Copy, without a link under the cursor), so
/// the menu looked like it had a "selected by default" entry.
#[test]
fn opening_with_the_pointer_leaves_no_row_focused() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    tab.doc_rect = Some(screen());
    let tab_id = tab.id;
    let row_id = |index: usize| egui::Id::unique(("doc-menu-row", tab_id, index));
    let pos = egui::pos2(200.0, 180.0);
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, egui::PointerButton::Secondary, true),
            press(pos, egui::PointerButton::Secondary, false),
        ],
    );
    settle_menu(&ctx, &mut tab);
    assert!(tab.context_open, "the secondary click opens the menu");
    assert!(
        !ctx.memory(|m| m.has_focus(row_id(0))),
        "no row may be focused when the menu opens with the pointer"
    );
}

/// After an action closes the menu, the next right-click opens it again:
/// the menu must never stay latched shut.
#[test]
fn menu_reopens_after_an_action() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    // Click "Select all" (a completed click inside the popup).
    let baseline = render_menu_positions(&ctx, &mut tab, Vec::new());
    let all_pos = baseline
        .iter()
        .find(|(t, _)| t.contains("Select all"))
        .expect("Select all row painted")
        .1;
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(all_pos),
            press(all_pos, egui::PointerButton::Primary, true),
            press(all_pos, egui::PointerButton::Primary, false),
        ],
    );
    assert!(!tab.context_open, "the action closes the menu");
    assert!(tab.selected_all, "Select all applied");
    // A closed menu must not leave its rows focused: reopening with the
    // pointer would ring the remembered row ("Copy" selected again).
    let tab_id = tab.id;
    let row_id = |index: usize| egui::Id::unique(("doc-menu-row", tab_id, index));
    assert!(
        !ctx.memory(|m| (0..2).any(|i| m.has_focus(row_id(i)))),
        "closing the menu must not keep a row focused"
    );
    // Right-click again: the menu must come back.
    let pos = egui::pos2(200.0, 180.0);
    menu_frame(
        &ctx,
        &mut tab,
        vec![
            egui::Event::PointerMoved(pos),
            press(pos, egui::PointerButton::Secondary, true),
            press(pos, egui::PointerButton::Secondary, false),
        ],
    );
    assert!(tab.context_open, "right-click reopens the menu");
}

/// Every text run painted by the forced-open menu. TWO passes on
/// purpose: the first area frame is an INVISIBLE sizing pass (egui
/// measures the popup), the second paints the rows.
fn render_menu_texts(ctx: &egui::Context, tab: &mut Tab) -> Vec<String> {
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        ..Default::default()
    };
    let mut first = ctx.run_ui(input.clone(), |root| {
        let url = content_menu(root, tab);
        assert!(url.is_none(), "rendering alone clicks nothing");
    });
    first.textures_delta.clear();
    let mut out = ctx.run_ui(input, |root| {
        let url = content_menu(root, tab);
        assert!(url.is_none(), "rendering alone clicks nothing");
    });
    let texts = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    out.textures_delta.clear();
    texts
}

/// The minimal menu renders `Open link in new tab` (only over a link), the
/// two clipboard actions and the document actions (`Find in page`,
/// `Save page`), each with its shortcut hint; no navigation items remain.
#[test]
fn menu_renders_the_minimal_actions() {
    let ctx = egui::Context::default();
    let mut tab = loaded_tab(Some("https://ex.com/other/"));
    let texts = render_menu_texts(&ctx, &mut tab);
    let joined = texts.join("|");
    for item in [
        "Open link in new tab",
        "Copy link address",
        "Copy",
        "Select all",
        "Find in page",
        "Save page",
    ] {
        assert!(joined.contains(item), "{item:?} missing: {joined}");
    }
    // The hints are painted next to the labels, so a reader learns the
    // shortcut without opening the menu bar.
    let hints = if cfg!(target_os = "macos") {
        ["Cmd+C", "Cmd+A", "Cmd+F", "Cmd+S"]
    } else {
        ["Ctrl+C", "Ctrl+A", "Ctrl+F", "Ctrl+S"]
    };
    for hint in hints {
        assert!(joined.contains(hint), "{hint:?} missing: {joined}");
    }
    for gone in [
        "Back",
        "Forward",
        "Refresh",
        "Server home",
        "Share",
        "Document outline",
    ] {
        assert!(
            !texts.iter().any(|t| t == gone),
            "removed item {gone:?} still rendered: {joined}"
        );
    }
}

/// The `Save page` row only ASKS for the write (`Tab::save_asked`): the
/// viewer owns no document payload, so `App` reads it right after the draw
/// and writes the file — the same path as `Ctrl/Cmd+S`.
#[test]
fn save_row_asks_the_app_for_the_write() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    assert!(!tab.save_asked, "nothing asked before the click");
    let pos = row_position(&ctx, &mut tab, "Save page");
    click_row(&ctx, &mut tab, pos);
    assert!(!tab.context_open, "the action closes the menu");
    assert!(tab.save_asked, "the viewer flagged the request");
}

/// Without a link under the cursor the menu has no "Open link" row.
#[test]
fn menu_hides_the_link_row_off_links() {
    let ctx = egui::Context::default();
    let mut tab = loaded_tab(None);
    let joined = render_menu_texts(&ctx, &mut tab).join("|");
    assert!(!joined.contains("Open link in new tab"), "{joined}");
    assert!(!joined.contains("Copy link address"), "{joined}");
    assert!(
        joined.contains("Copy") && joined.contains("Select all"),
        "{joined}"
    );
}

/// The menu is keyboard-navigable after a pointer open too: no row starts
/// focused, the first `Down` moves focus to the first row (a second `Down`
/// to the next one) and `Enter` activates it without any pointer — the menu
/// used to be pointer-only.
#[test]
fn menu_is_keyboard_navigable() {
    use egui::Key;
    let ctx = egui::Context::default();
    let mut tab = loaded_tab(None);
    // Copy the id: the closures below borrow `tab` mutably.
    let tab_id = tab.id;
    let row_id = |index: usize| egui::Id::unique(("doc-menu-row", tab_id, index));
    // Open the menu (a secondary click inside the document rect).
    tab.doc_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(600.0, 400.0),
    ));
    let press = |key: Key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    // A click is press + release (a lone press never reports `clicked`).
    let button = |pressed: bool| egui::Event::PointerButton {
        pos: egui::pos2(100.0, 100.0),
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    // Frame 1: the secondary press+release opens the menu.
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events: vec![button(true), button(false)],
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, &mut tab);
        },
    );
    out.textures_delta.clear();
    assert!(tab.context_open, "the secondary click opens the menu");
    // The sizing pass is invisible; run one more frame so the rows exist.
    // Opening with the POINTER leaves no row focused (a ringed row read as a
    // pre-selected entry); the keys move focus in on the first press.
    for _ in 0..2 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen()),
                ..Default::default()
            },
            |root| {
                let _ = content_menu(root, &mut tab);
            },
        );
        out.textures_delta.clear();
    }
    assert!(
        !ctx.memory(|m| m.has_focus(row_id(0))),
        "the pointer must not leave a row focused on open"
    );
    // First Down focuses the first row (Copy: no link under the cursor).
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events: vec![press(Key::ArrowDown)],
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, &mut tab);
        },
    );
    out.textures_delta.clear();
    assert!(
        ctx.memory(|m| m.has_focus(row_id(0))),
        "the first Down must focus the first row"
    );
    // Second Down moves to Select all; Enter activates it: the tab gets the
    // select-all highlight.
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events: vec![press(Key::ArrowDown)],
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, &mut tab);
        },
    );
    out.textures_delta.clear();
    assert!(
        ctx.memory(|m| m.has_focus(row_id(1))),
        "Down must move focus to the next row"
    );
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            events: vec![press(Key::Enter)],
            ..Default::default()
        },
        |root| {
            let _ = content_menu(root, &mut tab);
        },
    );
    out.textures_delta.clear();
    assert!(!tab.context_open, "Enter closes the menu");
    assert!(
        tab.selected_all,
        "Enter on Select all highlighted the whole page"
    );
}

/// The find bar paints the app's own chrome kit: an omnibox-style capsule
/// (rounded pill behind the field), the muted `n/m` count and the three
/// circular icon buttons, all in ONE row — and typing a query never moves
/// any of it (the count keeps a fixed slot while the digits change).
#[test]
fn find_bar_paints_the_chrome_capsule_and_keeps_its_geometry() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    tab.find.show(tab.doc_generation);
    tab.find.query = "markdown".to_string();
    tab.find.total = 12;
    tab.find.current = 2;
    // Two passes: the first popup frame is an invisible sizing pass.
    let render = |tab: &mut Tab| -> Vec<(String, f32)> {
        let input = egui::RawInput {
            screen_rect: Some(screen()),
            ..Default::default()
        };
        let mut first = ctx.run_ui(input.clone(), |root| {
            super::find::show_find_bar(root, tab);
        });
        first.textures_delta.clear();
        let mut out = ctx.run_ui(input, |root| {
            super::find::show_find_bar(root, tab);
        });
        let mut texts = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.x)),
                _ => None,
            })
            .collect::<Vec<_>>();
        out.textures_delta.clear();
        texts.sort_by(|a, b| a.1.total_cmp(&b.1));
        texts
    };
    let baseline = render(&mut tab);
    assert!(
        baseline.iter().any(|(t, _)| t == "markdown"),
        "query painted: {baseline:?}"
    );
    assert!(
        baseline.iter().any(|(t, _)| t == "3/12"),
        "count painted: {baseline:?}"
    );
    // The capsule (painted by `Shape::Rect`, not text) sits in the popup:
    // a tall rounded rect whose radius is the pill of BAR_H.
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        ..Default::default()
    };
    let mut first = ctx.run_ui(input.clone(), |root| {
        super::find::show_find_bar(root, &mut tab);
    });
    first.textures_delta.clear();
    let mut out = ctx.run_ui(input, |root| {
        super::find::show_find_bar(root, &mut tab);
    });
    let pill = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) => Some(r.clone()),
            _ => None,
        })
        .find(|r| {
            (r.rect.height() - 34.0).abs() < 0.5
                && r.corner_radius.nw == crate::ui::theme::pill_radius(34.0).nw
                && r.rect.width() > 200.0
        });
    out.textures_delta.clear();
    assert!(pill.is_some(), "capsule pill painted");
    // Longer digits in the same fixed slot: the field (and therefore the
    // buttons, which follow it in the row) must not move. The count's own
    // ink is centered in its fixed slot, so only its x may shift.
    tab.find.total = 340;
    tab.find.current = 119;
    let after = render(&mut tab);
    let x_of = |texts: &[(String, f32)], needle: &str| -> f32 {
        texts
            .iter()
            .find(|(t, _)| t == needle)
            .map_or_else(|| panic!("{needle:?} not painted: {texts:?}"), |(_, x)| *x)
    };
    assert_eq!(
        x_of(&after, "markdown"),
        x_of(&baseline, "markdown"),
        "the query field must not move when the count grows"
    );
    assert!(
        after.iter().any(|(t, _)| t == "120/340"),
        "the new count painted: {after:?}"
    );
}

/// The `Find in page` row opens the find bar through the same state the
/// shortcut uses (the field takes focus on the bar's next frame, and the
/// menu never keeps the keyboard).
#[test]
fn find_row_opens_the_find_bar() {
    use egui::Key;
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = loaded_tab(None);
    settle_menu(&ctx, &mut tab);
    assert!(!tab.find.open, "closed before the menu action");
    let key = |key: Key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    // Rows without a link under the cursor: Copy, Select all, Find in page.
    for _ in 0..3 {
        menu_frame(&ctx, &mut tab, vec![key(Key::ArrowDown)]);
    }
    menu_frame(&ctx, &mut tab, vec![key(Key::Enter)]);
    assert!(!tab.context_open, "the menu closes on the action");
    assert!(tab.find.open, "Find in page opened the bar");
    assert!(
        tab.find.request_focus,
        "the bar takes the keyboard on its next frame"
    );
}

#[test]
fn find_no_matches_is_centered_in_its_slot() {
    use egui::epaint::Shape;
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut tab = Tab::new(1);
    tab.state = TabState::Loaded {
        fetched: fetched_for("https://ex.com/a/"),
    };
    tab.find.show(tab.doc_generation);
    tab.find.query = "zzz-no-hit".to_string();
    tab.find.total = 0;
    tab.find.current = 0;
    // Two passes: the first popup frame is an invisible sizing pass.
    let input = egui::RawInput {
        screen_rect: Some(screen()),
        ..Default::default()
    };
    let mut first = ctx.run_ui(input.clone(), |root| {
        super::find::show_find_bar(root, &mut tab);
    });
    first.textures_delta.clear();
    let mut out = ctx.run_ui(input, |root| {
        super::find::show_find_bar(root, &mut tab);
    });
    // Capsule pill (256x34) and the "No matches" ink.
    let capsule = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Rect(r) if (r.rect.height() - 34.0).abs() < 0.5 && r.rect.width() > 200.0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .next()
        .expect("capsule pill painted");
    let (pos, size) = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) if t.galley.text() == "No matches" => Some((t.pos, t.galley.size())),
            _ => None,
        })
        .next()
        .expect("No matches painted");
    out.textures_delta.clear();
    // The field's inner scope used to leave the parent cursor at the field's
    // right edge (`capsule.max.x - CAPSULE_PAD`), pulling the count slot 6px
    // left into the capsule. The slot must start after a full item spacing.
    let slot_min_x = capsule.max.x + crate::ui::theme::spacing::XS;
    let slot_center_x = slot_min_x + 72.0 * 0.5;
    let text_center_x = pos.x + size.x * 0.5;
    assert!(
        pos.x >= slot_min_x - 0.5,
        "No matches overlaps the capsule: ink at {pos:?} vs slot starting at {slot_min_x}"
    );
    assert!(
        (text_center_x - slot_center_x).abs() < 1.0,
        "No matches not centered: center at {text_center_x:.2} vs slot center {slot_center_x:.2}"
    );
}
