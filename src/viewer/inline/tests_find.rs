// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::inline` find tests: the single-galley precise split, the wash
//! rectangles it paints behind the glyphs, the emoji refusal and the
//! ordinals both paths assign.

use super::*;
use crate::viewer::find::FindRender;
use eframe::egui;

fn plain(text: &str) -> Frag<'static> {
    Frag {
        text: text.to_string().into(),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: false,
        link: None,
        image: None,
        badge: None,
    }
}

/// What one `render_plain_split` call painted: the flag it returned, the
/// find cursor, the galley (its job must stay UNSplit) and the fills of the
/// rectangles painted behind it.
struct SplitResult {
    ok: bool,
    fr: FindRender,
    galley: Option<std::sync::Arc<egui::Galley>>,
    wash_fills: Vec<egui::Color32>,
}

fn split_galley(text: &str, query: &str, current: usize) -> SplitResult {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let frag = plain(text);
    let mut fr = FindRender::new(query, current, false);
    let mut ok = false;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 200.0),
            )),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                ok = render_plain_split(ui, &frag, text, false, &mut fr);
            });
        },
    );
    out.textures_delta.clear();
    let mut galley = None;
    let mut wash_fills = Vec::new();
    for c in &out.shapes {
        match &c.shape {
            egui::epaint::Shape::Text(t) => {
                galley = Some(std::sync::Arc::clone(&t.galley));
            }
            // Only the find washes; the panel frame paints its own fill.
            egui::epaint::Shape::Rect(r)
                if r.fill == crate::ui::theme::light().find_match
                    || r.fill == crate::ui::theme::light().find_current =>
            {
                wash_fills.push(r.fill)
            }
            _ => {}
        }
    }
    SplitResult {
        ok,
        fr,
        galley,
        wash_fills,
    }
}

/// The precise split paints ONE galley with the whole fragment (so shaping
/// and wrapping match the unsearched render) whose washes are separate
/// rectangles: one per match, in the match/current amber.
#[test]
fn one_unsplit_galley_carries_the_washes() {
    let light = crate::ui::theme::light();
    let r = split_galley("hello world, hello there", "hello", 0);
    assert!(r.ok, "the split rendered");
    let galley = r.galley.expect("a text shape was painted");
    assert_eq!(
        galley.text(),
        "hello world, hello there",
        "the whole fragment lives in ONE galley"
    );
    assert_eq!(
        galley.job.sections.len(),
        1,
        "the job stays UNSplit: per-match sections shaped each piece on its \
         own and drifted the metrics"
    );
    assert_eq!(r.fr.next_ordinal, 2, "both occurrences claimed");
    assert_eq!(
        r.wash_fills,
        vec![light.find_current, light.find_match],
        "one wash rectangle per match, the first (current) in the stronger amber"
    );
}

/// The current ordinal selects which wash gets the stronger amber.
#[test]
fn the_current_ordinal_picks_the_wash() {
    let light = crate::ui::theme::light();
    let r = split_galley("hello world, hello there", "hello", 1);
    assert!(r.ok);
    assert_eq!(r.wash_fills, vec![light.find_match, light.find_current]);
}

/// Case-insensitive matching on the rendered fragment: the stored query is
/// raw, `FindRender` carries the lowered form.
#[test]
fn match_is_case_insensitive_and_counts_every_occurrence() {
    let r = split_galley("Hello HELLO", "hello", 0);
    assert!(r.ok);
    assert_eq!(r.galley.expect("painted").text(), "Hello HELLO");
    assert_eq!(r.fr.next_ordinal, 2);
    assert_eq!(r.wash_fills.len(), 2, "two washes");
}

/// No occurrence: the precise path refuses (false) and claims nothing, so
/// the caller renders the fragment normally.
#[test]
fn no_match_refuses_without_claiming() {
    let r = split_galley("plain text", "zzz", 0);
    assert!(!r.ok, "no match: the caller owns the fragment");
    assert!(r.galley.is_none(), "nothing painted by the split");
    assert_eq!(r.fr.next_ordinal, 0);
}

/// An emoji fragment refuses the precise split even when the match does NOT
/// touch the cluster: a galley cannot draw a texture, so the caller's
/// whole-fragment path takes over (color textures + the wash behind them).
/// Refusing must claim NOTHING, or the fallback would double-count.
#[test]
fn emoji_fragment_refuses_the_precise_split() {
    let r = split_galley("rust ❤️ rust", "rust", 0);
    assert!(!r.ok, "an emoji fragment must not take the galley split");
    assert!(r.galley.is_none(), "the split painted nothing");
    assert_eq!(r.fr.next_ordinal, 0, "a refused split claims nothing");
}

/// A match spanning a WRAPPED line gets one wash rectangle per row (the
/// crossover row band, not one box across both).
#[test]
fn a_wrap_spanning_match_gets_one_rect_per_row() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    // Narrow column so the phrase wraps.
    let r = {
        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let frag = plain(text);
        let mut fr = FindRender::new("epsilon zeta", 0, false);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(140.0, 200.0),
                )),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let ok = render_plain_split(ui, &frag, text, false, &mut fr);
                    assert!(ok, "the phrase matched");
                });
            },
        );
        out.textures_delta.clear();
        let galley = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some(std::sync::Arc::clone(&t.galley)),
                _ => None,
            })
            .expect("painted");
        let rows = galley.rows.len();
        let rects = out
            .shapes
            .iter()
            .filter(|c| matches!(&c.shape, egui::epaint::Shape::Rect(_)))
            .count();
        (rows, rects)
    };
    assert!(
        r.0 >= 2,
        "the narrow column must wrap the phrase: rows={}",
        r.0
    );
    assert!(
        r.1 >= 2,
        "a wrap-spanning match paints one band per row: rects={}",
        r.1
    );
}
