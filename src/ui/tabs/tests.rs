// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::show_tabs;
use crate::browser::tab::Tab;
use crate::media::favicon::FaviconCache;
use eframe::egui;

/// The collapsed rail must render headlessly without panicking, start
/// no favicon probes for Home tabs and stay collapsed with no pointer.
#[test]
fn rail_renders_collapsed_without_network() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };
    let mut tabs = vec![Tab::new(1), Tab::new(2)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<egui::Rect> = None;
    let mut out = ctx.run_ui(input, |root| {
        egui::Panel::top("test-top").show(root, |ui| {
            ui.label("toolbar");
        });
        let resp = show_tabs(root, &mut tabs, 0, &mut favicons, &mut overlay);
        assert!(resp.select.is_none() && resp.close.is_none() && !resp.new && !resp.history);
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "rail painted something");
    assert!(overlay.is_none(), "no hover = collapsed");
}

/// The hover overlay must grow out of the rail over several frames
/// instead of popping to full width in one (which read as a flash), and
/// shrink back the same way.
#[test]
fn overlay_expands_and_collapses_gradually() {
    use super::{EXPANDED_W, RAIL_W};
    use egui::{Event, RawInput, Rect};

    fn frame(
        ctx: &egui::Context,
        tabs: &mut [Tab],
        favicons: &mut FaviconCache,
        overlay: &mut Option<Rect>,
        pos: Option<egui::Pos2>,
    ) -> egui::FullOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            events: pos.map_or_else(Vec::new, |p| vec![Event::PointerMoved(p)]),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::Panel::top("test-top").show(root, |ui| {
                ui.label("toolbar");
            });
            let _ = show_tabs(root, tabs, 0, favicons, overlay);
        });
        out.textures_delta.clear();
        out
    }

    /// Painted right edge of the overlay (its background spans the full
    /// 248px the first frame; the clip rect carries the animated width).
    /// `None` while the overlay is not drawn.
    fn painted_edge(out: &egui::FullOutput) -> Option<f32> {
        out.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) if r.rect.max.x > RAIL_W + 1.0 && r.rect.min.y > 0.0 => {
                Some(clipped.clip_rect.max.x)
            }
            _ => None,
        })
    }

    let ctx = egui::Context::default();
    // Expanded rows typeset titles in the bundled bold face: bind it or
    // epaint panics on the family lookup.
    crate::ui::theme::install_test_fonts(&ctx);
    let mut tabs = vec![Tab::new(1), Tab::new(2)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<Rect> = None;

    // Prime the animation while collapsed (the animation manager returns
    // the target on its first call, so without a collapsed frame the very
    // first hover would pop), then hover the rail.
    let out = frame(&ctx, &mut tabs, &mut favicons, &mut overlay, None);
    assert_eq!(painted_edge(&out), None, "collapsed: no overlay drawn");

    // egui hides a brand-new `Area` for one sizing pass, so the first
    // hovered frame paints nothing; the tween is checked from the first
    // frame that actually shows the overlay.
    let _ = frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        Some(egui::pos2(20.0, 300.0)),
    );
    let out = frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        Some(egui::pos2(20.0, 300.0)),
    );
    let first = painted_edge(&out).expect("hover: the overlay is visible");
    assert!(
        first > RAIL_W && first < EXPANDED_W,
        "first hovered frame is mid-tween, not full width: {first}"
    );

    // Keep hovering: the tween finishes at the exact final width.
    let mut last = first;
    for _ in 0..30 {
        let out = frame(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            Some(egui::pos2(20.0, 300.0)),
        );
        last = painted_edge(&out).unwrap_or(last);
        if last >= EXPANDED_W {
            break;
        }
    }
    assert!(last >= EXPANDED_W, "overlay finishes expanding: {last}");

    // Leaving the overlay shrinks it back out over several frames.
    let out = frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        Some(egui::pos2(500.0, 300.0)),
    );
    let closing = painted_edge(&out).expect("closing frame still draws the overlay");
    assert!(
        closing > RAIL_W && closing < EXPANDED_W,
        "leaving mid-tween, not an instant snap: {closing}"
    );
    let mut collapsed = false;
    for _ in 0..30 {
        let out = frame(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            Some(egui::pos2(500.0, 300.0)),
        );
        if painted_edge(&out).is_none() {
            collapsed = true;
            break;
        }
    }
    assert!(collapsed, "overlay collapses fully after the tween");
    assert!(overlay.is_none(), "collapsed: hover carry cleared");
}

/// The overlay only collapses into the document: moving up to the
/// toolbar or leaving the window keeps it open.
#[test]
fn overlay_stays_open_on_toolbar_and_outside_window() {
    use super::{EXPANDED_W, RAIL_W};
    use egui::{Event, RawInput, Rect};

    fn frame_events(
        ctx: &egui::Context,
        tabs: &mut [Tab],
        favicons: &mut FaviconCache,
        overlay: &mut Option<Rect>,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::Panel::top("test-top").show(root, |ui| {
                ui.label("toolbar");
            });
            let _ = show_tabs(root, tabs, 0, favicons, overlay);
        });
        out.textures_delta.clear();
        out
    }

    fn moved(p: egui::Pos2) -> Vec<Event> {
        vec![Event::PointerMoved(p)]
    }

    /// Painted right edge of the overlay (`None` while not drawn).
    fn painted_edge(out: &egui::FullOutput) -> Option<f32> {
        out.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) if r.rect.max.x > RAIL_W + 1.0 && r.rect.min.y > 0.0 => {
                Some(clipped.clip_rect.max.x)
            }
            _ => None,
        })
    }

    let ctx = egui::Context::default();
    // Expanded rows typeset titles in the bundled bold face (see above).
    crate::ui::theme::install_test_fonts(&ctx);
    let mut tabs = vec![Tab::new(1), Tab::new(2)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<Rect> = None;

    // Prime collapsed, then hover the rail until fully expanded.
    frame_events(&ctx, &mut tabs, &mut favicons, &mut overlay, Vec::new());
    let mut expanded = false;
    for _ in 0..40 {
        let out = frame_events(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            moved(egui::pos2(20.0, 300.0)),
        );
        if painted_edge(&out).is_some_and(|e| e >= EXPANDED_W) {
            expanded = true;
            break;
        }
    }
    assert!(expanded, "rail hover expands the overlay");
    assert!(overlay.is_some());

    // Up to the toolbar (above the rail top): stays open.
    for _ in 0..10 {
        let out = frame_events(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            moved(egui::pos2(450.0, 10.0)),
        );
        assert!(
            painted_edge(&out).is_some(),
            "toolbar must not collapse the overlay"
        );
    }
    assert!(overlay.is_some(), "toolbar keeps the hover carry");

    // Back to the rail, then outside the window: stays open.
    frame_events(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        moved(egui::pos2(20.0, 300.0)),
    );
    for _ in 0..10 {
        let out = frame_events(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            vec![Event::PointerGone],
        );
        assert!(
            painted_edge(&out).is_some(),
            "leaving the window must not collapse the overlay"
        );
    }
    assert!(overlay.is_some(), "outside keeps the hover carry");

    // Into the document: collapses back out over several frames.
    let mut collapsed = false;
    for _ in 0..40 {
        let out = frame_events(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            moved(egui::pos2(500.0, 300.0)),
        );
        if painted_edge(&out).is_none() {
            collapsed = true;
            break;
        }
    }
    assert!(collapsed, "document collapses the overlay");
    assert!(overlay.is_none(), "collapsed: hover carry cleared");
}

/// Edge treatment contract for the expanded overlay: NOTHING is painted
/// above the panel top (that shadow half painted a gray seam under the
/// toolbar and made the panel look detached) and the right edge keeps a
/// separator in both themes: gradient strips in light, one hairline in
/// dark (a black blur over dark chrome reads as dirt).
#[test]
fn edge_separator_paints_the_right_edge_only() {
    use crate::ui::theme;
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 48.0), egui::pos2(248.0, 300.0));
    for dark in [false, true] {
        let scheme = if dark { theme::dark() } else { theme::light() };
        let ctx = egui::Context::default();
        ctx.set_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let mut strips: Vec<egui::Rect> = Vec::new();
        let mut hairlines = 0;
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                super::paint_edge_separator(ui, rect, &scheme);
            });
        });
        for clipped in &out.shapes {
            if let egui::Shape::Rect(r) = &clipped.shape {
                if dark {
                    if r.fill == scheme.outline_variant {
                        hairlines += 1;
                        assert_eq!(r.rect.max.x, rect.max.x, "hairline on the edge");
                        assert_eq!(r.rect.max.y, rect.max.y, "hairline reaches the bottom");
                    }
                } else if r.fill.a() > 0 && r.fill.r() == 0 && r.fill.g() == 0 && r.fill.b() == 0 {
                    strips.push(r.rect);
                }
            }
        }
        out.textures_delta.clear();
        if dark {
            assert_eq!(hairlines, 1, "dark mode: exactly one right-edge hairline");
            assert!(strips.is_empty(), "dark mode: no black blur");
        } else {
            assert!(!strips.is_empty(), "light mode: gradient strips");
            for r in &strips {
                assert!(
                    r.min.x >= rect.max.x,
                    "strip starts at the right edge, never over the panel"
                );
                assert_eq!(r.min.y, rect.min.y, "strip starts at the panel top");
                assert_eq!(r.max.y, rect.max.y, "strip reaches the bottom");
            }
        }
    }
}

/// The chrome shares ONE icon size: the rail's favicon column and its
/// vector icons paint exactly like a toolbar glyph (`icons::paint` in an
/// `icons::BUTTON` box). The old 20px favicon column painted them
/// visibly smaller than the toolbar.
#[test]
fn rail_icons_match_the_toolbar_glyph_size() {
    use crate::ui::icons;
    use egui::{RawInput, Rect};

    /// Union bounding box of every Path shape whose center is above
    /// `y_limit` (the rail's pinned action rows sit at the bottom).
    fn top_path_bounds(out: &egui::FullOutput, y_limit: f32) -> Rect {
        out.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Path(p) => {
                    let b = Rect::from_points(&p.points);
                    (b.center().y < y_limit).then_some(b)
                }
                _ => None,
            })
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::NOTHING)
    }

    let ctx = egui::Context::default();
    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0));

    // Reference: the Home placeholder painted the toolbar way.
    let mut out = ctx.run_ui(
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let rect = Rect::from_min_size(
                    egui::pos2(40.0, 40.0),
                    egui::vec2(icons::BUTTON, icons::BUTTON),
                );
                icons::paint(ui, rect, icons::Icon::Home, egui::Color32::BLACK);
            });
        },
    );
    let reference = top_path_bounds(&out, f32::INFINITY);
    out.textures_delta.clear();
    assert!(reference.width() > 0.0, "the reference glyph painted");

    // The collapsed rail draws that same placeholder for a Home tab
    // (History and New Tab paint their own icons further down).
    let mut tabs = vec![Tab::new(1)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<Rect> = None;
    let mut out = ctx.run_ui(
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::Panel::top("test-top").show(root, |ui| {
                ui.label("toolbar");
            });
            let _ = show_tabs(root, &mut tabs, 0, &mut favicons, &mut overlay);
        },
    );
    out.textures_delta.clear();
    let rail_icon = top_path_bounds(&out, 200.0);
    assert!(rail_icon.width() > 0.0, "the rail placeholder painted");
    assert!(
        (rail_icon.width() - reference.width()).abs() < 0.05
            && (rail_icon.height() - reference.height()).abs() < 0.05,
        "the rail icon must match the toolbar glyph: {rail_icon:?} vs {reference:?}"
    );
    // Sanity: clearly not the old 20px favicon column.
    assert!(
        rail_icon.width() > icons::GLYPH * 0.6,
        "the rail icon must not shrink back to the old small column: {rail_icon:?}"
    );
}
