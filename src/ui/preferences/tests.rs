// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{PreferencesState, PreferencesTab};
use crate::history::HistoryStore;
use crate::settings::Prefs;
use eframe::egui;

fn input_at(pointer: Option<egui::Pos2>) -> egui::RawInput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            // Tall enough for the fixed dialog (720px) at 90%:
            // a shorter viewport would scroll and clip the footer link.
            egui::vec2(900.0, 1000.0),
        )),
        ..Default::default()
    };
    if let Some(pos) = pointer {
        input.events.push(egui::Event::PointerMoved(pos));
    }
    input
}

fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
            _ => None,
        })
        .collect()
}

/// Ink rect of one painted text shape.
///
/// `Shape::Text`'s `pos` is the galley's ALIGNMENT pivot, not its top-left:
/// `Galley::rect` is relative to it, and with `LayoutJob::halign` a
/// right-aligned galley spans `-width..=0`. The ink is therefore
/// `galley.rect` translated to `pos` — measuring `pos..pos + size` made a
/// correctly right-aligned Preferences label (the shortcut keys column) look
/// like it extended past its card.
fn ink_rect(t: &egui::epaint::TextShape) -> egui::Rect {
    t.galley.rect.translate(t.pos.to_vec2())
}

/// The Preferences dialog renders and its controls keep their exact
/// positions when the pointer hovers the retention dropdown: the old
/// `selectable_label` grew a frame on hover and shifted the list.
#[test]
fn preferences_geometry_is_stable_on_hover() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    for i in 0..8 {
        store.record(&format!("https://ex.com/{i}/"), Some(&format!("Page {i}")));
    }
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;

    // Let the window settle at its anchored position, then take the
    // baseline (no pointer).
    let mut baseline = Vec::new();
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        baseline = texts(&out.shapes);
    }
    assert!(!baseline.is_empty(), "something rendered");
    // Hover the selected retention value (the dropdown's own text).
    let anchor = baseline
        .iter()
        .find(|(text, _)| text.contains("Keep 3 months"))
        .map(|(_, pos)| *pos)
        .expect("retention dropdown visible");

    let mut hovered = Vec::new();
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(Some(anchor)), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        hovered = texts(&out.shapes);
    }
    let sort = |v: &mut Vec<(String, egui::Pos2)>| {
        v.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.y.total_cmp(&b.1.y))
                .then_with(|| a.1.x.total_cmp(&b.1.x))
        });
    };
    let mut base = baseline;
    let mut hov = hovered;
    sort(&mut base);
    sort(&mut hov);
    assert_eq!(
        base, hov,
        "hovering must not move any text in the Preferences dialog"
    );
}

/// Arming "Delete history" shows its confirmation note as fit content:
/// the note only takes space while armed, pushing the cards below it
/// (the inner scroll covers viewports that cannot fit the armed stack).
#[test]
fn delete_confirmation_fits_content_and_pushes_below() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    for i in 0..8 {
        store.record(&format!("https://ex.com/{i}/"), Some(&format!("Page {i}")));
    }
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;

    // Settle, then take the disarmed geometry.
    let mut base = None;
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        base = Some(out);
    }
    let base = base.expect("dialog rendered");
    let base_texts = texts(&base.shapes);
    let anchor = base_texts
        .iter()
        .find(|(text, _)| text == "Delete history")
        .map(|(_, pos)| *pos + egui::vec2(1.0, 1.0))
        .expect("delete row visible");

    // A single click arms the confirmation; later frames only hover.
    let click = |pressed| egui::Event::PointerButton {
        pos: anchor,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut armed = None;
    for frame in 0..4 {
        let mut input = input_at(Some(anchor));
        if frame == 0 {
            input.events.push(click(true));
            input.events.push(click(false));
        }
        let mut out = ctx.run_ui(input, |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        armed = Some(out);
    }
    let armed = armed.expect("dialog rendered");
    let armed_texts = texts(&armed.shapes);
    assert!(
        armed_texts.iter().any(|(text, _)| text == "Confirm delete"),
        "the first click must arm the confirmation"
    );
    assert!(
        armed_texts
            .iter()
            .any(|(text, _)| text.starts_with("Click again")),
        "the confirmation note must be visible"
    );

    // Rows above the note keep their exact position...
    {
        let label = "Keep visits";
        let find = |v: &[(String, egui::Pos2)]| {
            v.iter()
                .find(|(text, _)| text == label)
                .map_or_else(|| panic!("{label} visible"), |(_, pos)| *pos)
        };
        assert_eq!(
            find(&base_texts),
            find(&armed_texts),
            "arming the delete confirmation must not move {label}"
        );
    }
    // ...rows below move down (or stay when the scroll covers them),
    // never up over the note.
    {
        let label = "Delete all data and close";
        let find = |v: &[(String, egui::Pos2)]| {
            v.iter()
                .find(|(text, _)| text == label)
                .map_or_else(|| panic!("{label} visible"), |(_, pos)| *pos)
        };
        assert!(
            find(&armed_texts).y >= find(&base_texts).y,
            "arming the delete confirmation must not pull {label} up"
        );
    }
    // ...the disarmed dialog clips nothing, and the armed note itself
    // stays inside the scroll viewport.
    for c in &base.shapes {
        if let egui::epaint::Shape::Text(t) = &c.shape {
            let rect = ink_rect(t);
            assert!(
                c.clip_rect.contains_rect(rect),
                "{:?} is clipped: {rect:?} outside {:?}",
                t.galley.text(),
                c.clip_rect
            );
        }
    }
    for c in &armed.shapes {
        if let egui::epaint::Shape::Text(t) = &c.shape {
            if t.galley.text().starts_with("Click again") {
                let rect = ink_rect(t);
                assert!(
                    c.clip_rect.contains_rect(rect),
                    "armed note is clipped: {rect:?} outside {:?}",
                    c.clip_rect
                );
            }
        }
    }
}

/// The destructive action lives in the dialog (no panic on click) and the
/// count caption reflects the store.
#[test]
fn preferences_renders_without_entries() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    for _ in 0..2 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty(), "dialog painted");
    }
}

/// "Delete all data and close" needs two clicks: the first arms the
/// confirmation, the second sets `wipe_all` for `App` to delete the data
/// directory and `process::exit` without further saves.
#[test]
fn wipe_all_needs_two_clicks() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;
    // Settle, then locate the wipe row.
    let mut anchor = None;
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        anchor = texts(&out.shapes)
            .iter()
            .find(|(text, _)| text == "Delete all data and close")
            .map(|(_, pos)| *pos + egui::vec2(1.0, 1.0));
    }
    let anchor = anchor.expect("wipe row visible");
    let click = |pressed| egui::Event::PointerButton {
        pos: anchor,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    // First click arms, does not wipe.
    let mut input = input_at(Some(anchor));
    input.events.push(click(true));
    input.events.push(click(false));
    let mut out = ctx.run_ui(input, |root| {
        let actions = windows.show(root.ctx(), &mut store, &mut prefs);
        assert!(!actions.wipe_all, "first click only arms");
    });
    out.textures_delta.clear();
    // Second click (still armed) requests the wipe.
    let mut wiped = false;
    for frame in 0..3 {
        let mut input = input_at(Some(anchor));
        if frame == 0 {
            input.events.push(click(true));
            input.events.push(click(false));
        }
        let mut out = ctx.run_ui(input, |root| {
            if windows.show(root.ctx(), &mut store, &mut prefs).wipe_all {
                wiped = true;
            }
        });
        out.textures_delta.clear();
    }
    assert!(wiped, "second click must request wipe-all");
}

/// The About card links to the internal licenses page: clicking the small
/// text sets the action flag `App` turns into a new tab (the dialog
/// itself never navigates).
#[test]
fn licenses_link_requests_the_internal_page() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;

    // Settle the dialog, then locate the link's text.
    let mut anchor = None;
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        anchor = texts(&out.shapes)
            .iter()
            .find(|(text, _)| text == "View third-party licenses")
            .map(|(_, pos)| *pos);
    }
    let anchor = anchor.expect("licenses link visible") + egui::vec2(1.0, 1.0);
    let click = |pressed| egui::Event::PointerButton {
        pos: anchor,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut requested = false;
    for frame in 0..3 {
        let mut input = input_at(Some(anchor));
        if frame == 0 {
            input.events.push(click(true));
            input.events.push(click(false));
        }
        let mut out = ctx.run_ui(input, |root| {
            if windows
                .show(root.ctx(), &mut store, &mut prefs)
                .open_licenses
            {
                requested = true;
            }
        });
        out.textures_delta.clear();
    }
    assert!(
        requested,
        "clicking the link must ask App to open the licenses page"
    );
}

/// The Preferences title keeps equal air above and below: the header
/// lane is a fixed 32px vertically-centered row inside symmetric
/// 12px padding, stacked flush on the divider (no theme item gaps).
#[test]
fn preferences_header_has_equal_air_above_and_below() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    let mut out = None;
    for _ in 0..5 {
        let mut o = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        o.textures_delta.clear();
        out = Some(o);
    }
    let out = out.expect("dialog rendered");
    let mut title = None;
    let mut dialog_top = None;
    let mut divider_top: Option<f32> = None;
    for c in &out.shapes {
        match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == "Preferences" => {
                title = Some((t.pos, t.galley.size()));
            }
            egui::epaint::Shape::Rect(r) => {
                if r.corner_radius.nw == 14 {
                    dialog_top = Some(r.rect.min.y);
                }
                if (r.rect.height() - 1.0).abs() < 0.01 && r.rect.width() > 400.0 {
                    divider_top = match divider_top {
                        Some(y) if y < r.rect.min.y => Some(y),
                        Some(y) => Some(y.min(r.rect.min.y)),
                        None => Some(r.rect.min.y),
                    };
                }
            }
            _ => {}
        }
    }
    let (pos, size) = title.expect("title rendered");
    let top = dialog_top.expect("dialog frame rendered");
    let divider = divider_top.expect("header divider rendered");
    let above = pos.y - top;
    let below = divider - (pos.y + size.y);
    assert!(
        (above - below).abs() <= 1.0,
        "title air must be symmetric: {above:.1}px above vs {below:.1}px below"
    );
}

/// Each tab shows its own content: General holds the appearance rows plus
/// the version footer, Data the retention and destructive rows, Shortcuts
/// the keyboard reference.
#[test]
fn each_tab_shows_its_own_content() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;

    let mut rendered = |windows: &mut PreferencesState, store: &mut HistoryStore| {
        let mut last = None;
        for _ in 0..4 {
            let mut out = ctx.run_ui(input_at(None), |root| {
                windows.show(root.ctx(), store, &mut prefs);
            });
            out.textures_delta.clear();
            last = Some(out);
        }
        let out = last.expect("dialog rendered");
        texts(&out.shapes)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
    };

    windows.selected = PreferencesTab::General;
    let general = rendered(&mut windows, &mut store);
    assert!(
        general.iter().any(|t| t == "Theme"),
        "General must hold the appearance rows"
    );
    assert!(
        general.iter().any(|t| t == "View third-party licenses"),
        "General must hold the version/licenses footer"
    );
    assert!(
        !general.iter().any(|t| t == "Keep visits"),
        "General must not hold the Data rows"
    );

    windows.selected = PreferencesTab::Data;
    let data = rendered(&mut windows, &mut store);
    assert!(
        data.iter().any(|t| t == "Keep visits"),
        "Data must hold the retention row"
    );
    assert!(
        data.iter().any(|t| t == "Delete all data and close"),
        "Data must hold the wipe row"
    );
    assert!(
        !data.iter().any(|t| t == "Theme"),
        "Data must not hold the appearance rows"
    );

    windows.selected = PreferencesTab::Shortcuts;
    let shortcuts = rendered(&mut windows, &mut store);
    assert!(
        shortcuts.iter().any(|t| t == "KEYBOARD SHORTCUTS"),
        "Shortcuts must hold the reference table"
    );
    assert!(
        !shortcuts.iter().any(|t| t == "Theme"),
        "Shortcuts must not hold the appearance rows"
    );
}

/// The Shortcuts tab lists every group with its actions and keys.
#[test]
fn shortcuts_tab_lists_every_group() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Shortcuts;
    let mut last = None;
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        last = Some(out);
    }
    let last = last.expect("dialog rendered");
    let rendered = texts(&last.shapes)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    for group in ["TABS", "NAVIGATION", "PAGE", "ZOOM"] {
        assert!(
            rendered.iter().any(|t| t == group),
            "shortcuts must list the {group} group"
        );
    }
    for (action, keys) in [
        ("New tab", "Ctrl/Cmd + T"),
        ("Close tab", "Ctrl/Cmd + W"),
        ("Reopen closed tab", "Ctrl/Cmd + Shift + T"),
        ("Next / previous tab", "Ctrl + Tab / Ctrl + Shift + Tab"),
        ("Back / forward", "Alt + Left / Right"),
        ("Reload page", "Ctrl/Cmd + R / F5"),
        ("Open history", "Ctrl + H / Cmd + Y"),
        ("Find in page", "Ctrl/Cmd + F"),
        ("Zoom in", "Ctrl/Cmd + + / ="),
        ("Reset zoom to 100%", "Ctrl/Cmd + 0"),
    ] {
        assert!(
            rendered.iter().any(|t| t == action),
            "shortcuts must list {action}"
        );
        assert!(
            rendered.iter().any(|t| t == keys),
            "shortcuts must list {keys}"
        );
    }
    // The reference is curated, not the full key map: the bare `Esc` rows
    // (stop/close) and the keyboard reorders stay out.
    for gone in [
        "Stop loading",
        "Close dialog, find bar or menu",
        "Move focused tab one slot",
        "Move focused bookmark tile",
    ] {
        assert!(
            !rendered.iter().any(|t| t == gone),
            "{gone:?} must stay out of the quick reference"
        );
    }
    assert!(
        !rendered.iter().any(|t| t == "Esc"),
        "no bare Esc row belongs in the quick reference"
    );
    // Every reference row must fit the dialog width — no column may be
    // clipped by the scroll viewport or the card edge (measured through the
    // galley's own rect: the keys column is right-aligned) — and the two
    // columns of a row must never overlap each other.
    let shapes = &last.shapes;
    let find_ink = |needle: &str| {
        shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == needle => Some(ink_rect(t)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} rendered"))
    };
    // The title keeps a beat of air above the first group: that gap must be
    // clearly wider than the rhythm inside a group (label to its own first
    // row), where the title is the card's heading and the group label binds
    // to the rows below it.
    let title_to_group = find_ink("TABS").min.y - find_ink("KEYBOARD SHORTCUTS").max.y;
    let group_to_row = find_ink("New tab").min.y - find_ink("TABS").max.y;
    assert!(
        title_to_group >= 14.0 && title_to_group > group_to_row + 4.0,
        "the title needs air above the first group: {title_to_group} vs {group_to_row}"
    );
    for (action, keys) in super::SHORTCUT_GROUPS
        .iter()
        .flat_map(|(_, rows)| rows.iter())
    {
        let a = find_ink(action);
        let k = find_ink(keys);
        assert!(
            a.max.x + 8.0 <= k.min.x,
            "{action:?} and {keys:?} must not overlap: {a:?} vs {k:?}"
        );
    }
    for c in &last.shapes {
        if let egui::epaint::Shape::Text(t) = &c.shape {
            let rect = ink_rect(t);
            assert!(
                c.clip_rect.contains_rect(rect),
                "shortcut text {:?} is clipped: {rect:?} outside {:?}",
                t.galley.text(),
                c.clip_rect
            );
        }
    }
}

/// Clicking a tab bar label switches the visible tab.
#[test]
fn clicking_the_tab_bar_switches_tabs() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::General;

    // Settle on General, then locate the Data tab label.
    let mut anchor = None;
    for _ in 0..4 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        anchor = texts(&out.shapes)
            .iter()
            .find(|(text, _)| text == "Data")
            .map(|(_, pos)| *pos + egui::vec2(1.0, 1.0));
    }
    let anchor = anchor.expect("Data tab visible");
    let click = |pressed| egui::Event::PointerButton {
        pos: anchor,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut data_visible = false;
    for frame in 0..4 {
        let mut input = input_at(Some(anchor));
        if frame == 0 {
            input.events.push(click(true));
            input.events.push(click(false));
        }
        let mut out = ctx.run_ui(input, |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        data_visible = texts(&out.shapes)
            .iter()
            .any(|(text, _)| text == "Keep visits");
    }
    assert!(
        data_visible,
        "clicking the Data tab must show the Data content"
    );
}

/// The selected tab carries the accent underline, under its own label.
#[test]
fn selected_tab_carries_a_moving_underline() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    let s = crate::ui::theme::scheme_ref_ctx(&ctx);
    for tab in PreferencesTab::ALL {
        windows.selected = tab;
        let mut last = None;
        // The floating window FADES IN on open (egui scales the painted
        // colors for the first frames), so settle until the fade is over.
        for _ in 0..30 {
            let mut out = ctx.run_ui(input_at(None), |root| {
                windows.show(root.ctx(), &mut store, &mut prefs);
            });
            out.textures_delta.clear();
            last = Some(out);
        }
        let out = last.expect("dialog rendered");
        let label = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == tab.label() => Some(ink_rect(t)),
                _ => None,
            })
            .expect("tab label visible");
        let underline = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Rect(r)
                    if (r.rect.height() - 2.0).abs() < 0.5 && r.fill == s.primary =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .expect("selected tab underline");
        assert!(
            underline.center().x >= label.min.x && underline.center().x <= label.max.x,
            "{tab:?}: underline {underline:?} must sit under its label {label:?}"
        );
    }
}

/// Each tab owns its scroll position: scrolling the Shortcuts list down,
/// visiting General and coming back keeps the list where it was, and General
/// opens at its own top (one shared ScrollArea used to hand its offset to
/// whichever tab came next — it is clamped per frame, so leaving the scrolled
/// tab simply reset it).
///
/// The probes stay inside the viewport on purpose: egui skips painting text
/// fully outside the clip rect, so a scrolled-out row cannot be measured.
#[test]
fn each_tab_keeps_its_own_scroll_offset() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    // A short viewport: the Shortcuts reference scrolls, General does not.
    let input = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 560.0),
        )),
        events,
        ..Default::default()
    };
    // Run `frames` frames and return the last one (the very first frame only
    // sizes the window, so it carries no shapes).
    let mut run = |windows: &mut PreferencesState,
                   store: &mut HistoryStore,
                   events: Vec<egui::Event>,
                   frames: usize| {
        let mut last = None;
        for _ in 0..frames {
            let mut out = ctx.run_ui(input(events.clone()), |root| {
                windows.show(root.ctx(), store, &mut prefs);
            });
            out.textures_delta.clear();
            last = Some(out);
        }
        last.expect("frames run")
    };
    let y_of = |out: &egui::FullOutput, needle: &str| {
        out.shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == needle => {
                    Some(ink_rect(t).min.y)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} visible"))
    };
    let tab_center = |out: &egui::FullOutput, label: &str| {
        out.shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    Some(ink_rect(t).center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} tab visible"))
    };
    let click = |pos: egui::Pos2| {
        vec![
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ]
    };
    // A middle row of the reference: visible unscrolled AND scrolled down.
    const PROBE: &str = "Bookmark this page";

    // Shortcuts starts at its top (its bottom note is out of view).
    windows.selected = PreferencesTab::Shortcuts;
    let out = run(&mut windows, &mut store, vec![], 6);
    let open_y = y_of(&out, PROBE);
    assert!(
        out.shapes.iter().all(|c| !matches!(
            &c.shape,
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Ctrl/Cmd means")
        )),
        "the bottom note starts below the fold"
    );

    // Wheel over the dialog scrolls the reference up. The wheel repeats on
    // every frame (egui feeds a scroll area a SMOOTHED delta), so the
    // scrolled-to-the-end position is deterministic.
    let over = egui::pos2(450.0, 320.0);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    let mut events = vec![egui::Event::PointerMoved(over)];
    events.extend(std::iter::repeat_n(wheel, 2));
    let out = run(&mut windows, &mut store, events, 4);
    let scrolled_y = y_of(&out, PROBE);
    assert!(
        scrolled_y < open_y - 40.0,
        "the wheel must scroll the reference (from {open_y} to {scrolled_y})"
    );
    // The bottom note comes into view only when scrolled down.
    let note_y = |out: &egui::FullOutput| {
        out.shapes.iter().find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Ctrl/Cmd means") => {
                Some(ink_rect(t).min.y)
            }
            _ => None,
        })
    };
    assert!(
        note_y(&out).is_some(),
        "the bottom note shows once scrolled"
    );

    // Switch to General: its own top, no inherited offset.
    let general = tab_center(&out, "General");
    let out = run(&mut windows, &mut store, click(general), 4);
    let theme_y = y_of(&out, "Theme");
    let mut control = PreferencesState::new();
    control.prefs_open = true;
    control.selected = PreferencesTab::General;
    let control_y = y_of(&run(&mut control, &mut store, vec![], 6), "Theme");
    assert!(
        (theme_y - control_y).abs() < 1.0,
        "General must open at its own top: Theme at {theme_y}, control {control_y}"
    );

    // Back to Shortcuts: still scrolled away from its top (the shared-scroll
    // bug reset it, which would park the probe at its unscrolled position)
    // and still showing the end of the list.
    let shortcuts = tab_center(&out, "Shortcuts");
    let out = run(&mut windows, &mut store, click(shortcuts), 4);
    let back_y = y_of(&out, PROBE);
    assert!(
        back_y < open_y - 40.0,
        "coming back must keep the Shortcuts scroll (open {open_y}, scrolled {scrolled_y}, back {back_y})"
    );
    assert!(
        note_y(&out).is_some(),
        "the bottom note is still in view when coming back"
    );
}

/// Every General-tab control is centered in its row and shares one right
/// edge.
///
/// Regression: the dropdown field sat ~7px below the row's middle (egui's
/// `ComboBox` button is a `Frame`, and a frame inside a right-to-left row is
/// anchored from the parent cursor's right-CENTER seed) while the zoom pill
/// and the switch — exact rects — stayed centered.
#[test]
fn general_controls_are_centered_and_right_aligned() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::General;
    let mut last = None;
    for _ in 0..6 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        last = Some(out);
    }
    let out = last.expect("dialog rendered");
    let rects: Vec<(egui::Rect, u8)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) => Some((r.rect, r.corner_radius.nw)),
            _ => None,
        })
        .collect();
    // The three control surfaces, identified by their fixed geometry: the
    // dropdown field (`COMBO_W` wide), the pill (fully rounded) and the
    // switch (its own fixed size).
    let field = rects
        .iter()
        .find(|(r, _)| (r.width() - super::COMBO_W).abs() < 0.5 && r.height() > 24.0)
        .map(|(r, _)| *r)
        .expect("dropdown field painted");
    let pill = rects
        .iter()
        .find(|(r, radius)| *radius >= 14 && r.width() < 200.0 && r.height() < 40.0)
        .map(|(r, _)| *r)
        .expect("zoom pill painted");
    let switch_rect = rects
        .iter()
        .find(|(r, _)| {
            (r.width() - super::SWITCH_W).abs() < 0.5 && (r.height() - super::SWITCH_H).abs() < 0.5
        })
        .map(|(r, _)| *r)
        .expect("switch painted");
    let row_center = |needle: &str| {
        out.shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == needle => {
                    Some(ink_rect(t).center().y)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} visible"))
    };
    for (label, control) in [
        ("Theme", field),
        ("Default zoom", pill),
        ("Restore tabs on startup", switch_rect),
    ] {
        let center = row_center(label);
        assert!(
            (control.center().y - center).abs() <= 1.0,
            "{label}: {control:?} must be centered on its row (row center {center})"
        );
    }
    for control in [pill, switch_rect] {
        assert!(
            (control.max.x - field.max.x).abs() <= 0.5,
            "controls must share one right edge: {control:?} vs {field:?}"
        );
    }
}

/// The dropdown field opens its menu on a click and a row click selects the
/// value (the field and its `Popup` replaced egui's `ComboBox`).
#[test]
fn dropdown_field_opens_and_selects() {
    use crate::history::Retention;
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;
    let input = |events: Vec<egui::Event>| {
        let mut input = input_at(None);
        input.events = events;
        input
    };
    let mut run =
        |windows: &mut PreferencesState, store: &mut HistoryStore, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(input(events), |root| {
                windows.show(root.ctx(), store, &mut prefs);
            });
            out.textures_delta.clear();
            out
        };
    let find_center = |out: &egui::FullOutput, needle: &str| {
        out.shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == needle => {
                    Some(ink_rect(t).center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} visible"))
    };
    let click = |pos: egui::Pos2| {
        vec![
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ]
    };

    // Settle on the Data tab, then click the field (the selected value's
    // line; the field elides a long value, so match its prefix).
    let mut settled = None;
    for _ in 0..6 {
        settled = Some(run(&mut windows, &mut store, vec![]));
    }
    let settled = settled.expect("frames run");
    let field = settled
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("Keep 3 months") => {
                Some(ink_rect(t).center())
            }
            _ => None,
        })
        .expect("the selected value is on the field");
    // The click toggles the menu open (settle: the popup state is read at
    // the start of the next pass).
    let _ = run(&mut windows, &mut store, click(field));
    let mut opened = None;
    for _ in 0..3 {
        opened = Some(run(&mut windows, &mut store, vec![]));
    }
    let opened = opened.expect("frames run");
    assert!(
        texts(&opened.shapes)
            .iter()
            .any(|(t, _)| t == "Keep 1 month"),
        "clicking the field must open the menu with its options"
    );
    // Pick the second option: the store takes the new retention.
    let row = find_center(&opened, "Keep 1 month");
    let _ = run(&mut windows, &mut store, click(row));
    for _ in 0..3 {
        let _ = run(&mut windows, &mut store, vec![]);
    }
    assert_eq!(
        store.retention(),
        Retention::Days(30),
        "picking a row must apply the value"
    );
    let out = run(&mut windows, &mut store, vec![]);
    assert!(
        texts(&out.shapes).iter().any(|(t, _)| t == "Keep 1 month"),
        "the field must show the chosen value"
    );
    assert!(
        !texts(&out.shapes).iter().any(|(t, _)| t == "Keep 1 year"),
        "the menu must close after picking a row"
    );
}

/// Closing the dialog disarms deletes but keeps the selected tab.
#[test]
fn closing_keeps_the_selected_tab() {
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Shortcuts;
    windows.close_windows();
    assert!(!windows.prefs_open, "close must hide the dialog");
    assert_eq!(
        windows.selected,
        PreferencesTab::Shortcuts,
        "close must keep the selected tab"
    );
}

/// The Data tab holds the CACHE card: one caption plus one two-click clear
/// row per class, in documents/images/favicons order, with nothing clipped
/// by the dialog or the scroll viewport.
#[test]
fn cache_card_lists_three_clears() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;
    let mut last = None;
    for _ in 0..6 {
        let mut out = ctx.run_ui(input_at(None), |root| {
            windows.show(root.ctx(), &mut store, &mut prefs);
        });
        out.textures_delta.clear();
        last = Some(out);
    }
    let out = last.expect("dialog rendered");
    let rendered = texts(&out.shapes);
    let y_of = |needle: &str| {
        rendered
            .iter()
            .find(|(text, _)| text == needle)
            .map_or_else(|| panic!("{needle:?} visible"), |(_, pos)| pos.y)
    };
    // Section first, then one row per class in a stable order.
    let caption = y_of("CACHE");
    let docs = rendered
        .iter()
        .find(|(text, _)| text.starts_with("Clear documents ("))
        .map(|(_, pos)| pos.y)
        .expect("documents clear row visible");
    let images = rendered
        .iter()
        .find(|(text, _)| text.starts_with("Clear images ("))
        .map(|(_, pos)| pos.y)
        .expect("images clear row visible");
    let favicons = rendered
        .iter()
        .find(|(text, _)| text.starts_with("Clear favicons ("))
        .map(|(_, pos)| pos.y)
        .expect("favicons clear row visible");
    assert!(
        caption < docs && docs < images && images < favicons,
        "cache rows must stack under their caption in class order"
    );
    // The rows sit between the history and the wipe cards, never overlapping
    // them.
    let history = y_of("Delete history");
    let wipe = y_of("Delete all data and close");
    assert!(
        history < docs && favicons < wipe,
        "the cache card must sit between the history and wipe cards"
    );
    // Nothing the dialog paints may be clipped.
    for c in &out.shapes {
        if let egui::epaint::Shape::Text(t) = &c.shape {
            let rect = ink_rect(t);
            assert!(
                c.clip_rect.contains_rect(rect),
                "{:?} is clipped: {rect:?} outside {:?}",
                t.galley.text(),
                c.clip_rect
            );
        }
    }
}

/// Each cache class clears with two clicks: the first arms its own
/// confirmation (sibling rows keep their exact positions), the second
/// dismisses it. The actual per-class store clearing is covered by the
/// `cache` module tests, which use isolated managers instead of the
/// process-wide test cache.
#[test]
fn cache_clear_needs_two_clicks_per_class() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;
    // Events run ONLY on the first frame (replaying a click every frame
    // would arm/clear/arm again); the following frames settle hovers.
    // Like the existing click tests, the pointer moves to the target first:
    // without it egui never hovers the row the click lands on.
    let mut run =
        |windows: &mut PreferencesState, at: Option<egui::Pos2>, events: Vec<egui::Event>| {
            let mut last = None;
            for frame in 0..3 {
                let mut input = input_at(at);
                if frame == 0 {
                    input.events.extend(events.clone());
                }
                let mut out = ctx.run_ui(input, |root| {
                    windows.show(root.ctx(), &mut store, &mut prefs);
                });
                out.textures_delta.clear();
                last = Some(out);
            }
            last.expect("frames run")
        };
    let row_center = |out: &egui::FullOutput, prefix: &str| {
        texts(&out.shapes)
            .iter()
            .find(|(text, _)| text.starts_with(prefix))
            .map_or_else(
                || panic!("{prefix:?} row visible"),
                |(_, pos)| *pos + egui::vec2(1.0, 1.0),
            )
    };
    let click = |pos: egui::Pos2| {
        vec![
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ]
    };
    // With no initialized cache the rows still render, with their off label.
    let settled = run(&mut windows, None, vec![]);
    assert!(
        texts(&settled.shapes)
            .iter()
            .any(|(text, _)| text.starts_with("Clear documents (cache off)")),
        "the documents row must render with the cache-off label"
    );

    // First click on "Clear images" arms ONLY its confirmation: sibling rows
    // keep their exact positions, like the history delete.
    let before: Vec<(String, egui::Pos2)> = texts(&settled.shapes);
    let at = |v: &[(String, egui::Pos2)], needle: &str| {
        v.iter()
            .find(|(text, _)| text.starts_with(needle))
            .map_or_else(|| panic!("{needle:?} visible"), |(_, pos)| *pos)
    };
    let images_at = row_center(&settled, "Clear images (");
    let _ = run(&mut windows, Some(images_at), click(images_at));
    let armed = run(&mut windows, Some(images_at), vec![]);
    let armed_texts = texts(&armed.shapes);
    assert!(
        armed_texts
            .iter()
            .any(|(text, _)| text == "Confirm clear images"),
        "first click must arm the images confirmation"
    );
    assert!(
        armed_texts
            .iter()
            .any(|(text, _)| text == "Click again to delete cached images."),
        "the armed note must be visible"
    );
    assert_eq!(
        at(&before, "Clear documents ("),
        at(&armed_texts, "Clear documents ("),
        "arming must not move the documents row above"
    );
    // The row BELOW the armed one moves down (or stays when the scroll
    // covers it), never up over the note — the same fit-content rule as
    // the history delete.
    assert!(
        at(&armed_texts, "Clear favicons (").y >= at(&before, "Clear favicons (").y,
        "arming must not pull the favicons row up"
    );

    // Second click dismisses the confirmation without touching siblings.
    let _ = run(&mut windows, Some(images_at), click(images_at));
    let cleared = run(&mut windows, None, vec![]);
    let cleared_texts = texts(&cleared.shapes);
    assert!(
        cleared_texts
            .iter()
            .any(|(text, _)| text.starts_with("Clear images (cache off)")),
        "second click must dismiss the confirmation"
    );
    assert!(
        cleared_texts
            .iter()
            .all(|(text, _)| text != "Click again to delete cached images."),
        "the armed note must disappear"
    );
}

/// On a short viewport the taller Data tab scrolls instead of breaking:
/// the whole CACHE card fits the first screen (no scroll needed to see all
/// three clears) and the wipe card below stays reachable by scrolling.
#[test]
fn cache_card_fits_first_screen_and_rest_stays_reachable() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let mut store = HistoryStore::in_memory();
    let mut prefs = Prefs::default();
    let mut windows = PreferencesState::new();
    windows.prefs_open = true;
    windows.selected = PreferencesTab::Data;
    // Short window: the dialog maxes out and the tab scrolls.
    let input = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 560.0),
        )),
        events,
        ..Default::default()
    };
    let mut run = |windows: &mut PreferencesState, events: Vec<egui::Event>| {
        let mut last = None;
        for frame in 0..4 {
            let mut frame_input = input(vec![]);
            if frame == 0 {
                frame_input.events = events.clone();
            }
            let mut out = ctx.run_ui(frame_input, |root| {
                windows.show(root.ctx(), &mut store, &mut prefs);
            });
            out.textures_delta.clear();
            last = Some(out);
        }
        last.expect("frames run")
    };
    let painted = |out: &egui::FullOutput| {
        texts(&out.shapes)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
    };
    // The full CACHE card is above the fold: caption plus all three rows
    // paint with no scrolling (egui culls fully-outside text, so painted =
    // visible).
    let first = painted(&run(&mut windows, vec![]));
    for needle in [
        "CACHE",
        "Clear documents (",
        "Clear images (",
        "Clear favicons (",
    ] {
        assert!(
            first.iter().any(|t| t.starts_with(needle)),
            "{needle:?} must be visible without scrolling"
        );
    }
    // The wipe card below the fold is one scroll away (same wheel pattern
    // as the tab-scroll test: egui smooths the delta, so the end position
    // is deterministic).
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    let mut events = vec![egui::Event::PointerMoved(egui::pos2(450.0, 300.0))];
    events.extend(std::iter::repeat_n(wheel, 4));
    let scrolled = painted(&run(&mut windows, events));
    assert!(
        scrolled.iter().any(|t| t == "Delete all data and close"),
        "the wipe card must be reachable by scrolling"
    );
}
