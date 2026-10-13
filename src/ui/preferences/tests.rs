// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::PreferencesState;
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
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("{label} visible"))
        };
        assert_eq!(
            find(&base_texts),
            find(&armed_texts),
            "arming the delete confirmation must not move {label}"
        );
    }
    // ...rows below move down (or stay when the scroll covers them),
    // never up over the note.
    for label in [
        "Theme",
        "Default zoom",
        "Restore tabs on startup",
        "Delete all data and close",
    ] {
        let find = |v: &[(String, egui::Pos2)]| {
            v.iter()
                .find(|(text, _)| text == label)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("{label} visible"))
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
            let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
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
                let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
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
