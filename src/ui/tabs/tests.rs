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
    // Render with the SHIPPED font stack, as production does (epaint's
    // bundled default faces are never registered).
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

    // The rail is CHROME: its labels (tab titles, `+ New Tab`, `History`)
    // stay in the regular face. `theme::strong` (`wght = 700`) is for the
    // document surfaces and the titled pages, never the rail.
    let out = frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        Some(egui::pos2(20.0, 300.0)),
    );
    let strong = crate::ui::theme::strong_family();
    let mut labels = 0;
    for clipped in &out.shapes {
        let egui::Shape::Text(t) = &clipped.shape else {
            continue;
        };
        let Some(section) = t.galley.job.sections.first() else {
            continue;
        };
        labels += 1;
        assert_ne!(
            section.format.font_id.family,
            strong,
            "rail label {:?} must not use the bold family",
            t.galley.text()
        );
    }
    assert!(labels >= 2, "expanded rail paints its labels: {labels}");

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
    // Render with the SHIPPED font stack, as production does (see above).
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
                super::paint_edge_separator(
                    ui.painter(),
                    ui.visuals().dark_mode,
                    rect,
                    &scheme,
                    1.0,
                );
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

/// The separator is drawn in BOTH rail states: collapsed it is smaller and
/// fainter (light) and a dimmer hairline (dark), so opening/closing morphs
/// the shadow instead of popping it on or off.
#[test]
fn collapsed_separator_is_smaller_and_softer() {
    use crate::ui::theme;

    // The collapsed rail's edge: the same x as the real rail.
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 48.0), egui::pos2(44.0, 300.0));

    /// Paint one separator; return the black penumbra strips as `(reach past
    /// the edge, alpha)` plus the hairline's alpha (`0` when no hairline).
    fn paint(
        ctx: &egui::Context,
        rect: egui::Rect,
        scheme: &theme::Scheme,
        progress: f32,
    ) -> (Vec<(f32, u8)>, u8) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let mut strips = Vec::new();
        let mut hairline = 0;
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                super::paint_edge_separator(
                    ui.painter(),
                    ui.visuals().dark_mode,
                    rect,
                    scheme,
                    progress,
                );
            });
        });
        for clipped in &out.shapes {
            let egui::Shape::Rect(r) = &clipped.shape else {
                continue;
            };
            if r.rect.max.x == rect.max.x && r.rect.min.x == rect.max.x - 1.0 {
                hairline = r.fill.a();
            } else if r.fill.a() > 0 && r.fill.r() == 0 && r.fill.g() == 0 && r.fill.b() == 0 {
                strips.push((r.rect.max.x - rect.max.x, r.fill.a()));
            }
        }
        out.textures_delta.clear();
        (strips, hairline)
    }

    let ctx = egui::Context::default();
    let reach = |v: &[(f32, u8)]| v.iter().map(|(x, _)| *x).fold(0.0_f32, f32::max);
    let peak = |v: &[(f32, u8)]| v.iter().map(|(_, a)| *a).max().unwrap_or(0);

    // Light: a penumbra in both states, narrower and fainter when collapsed.
    ctx.set_theme(egui::Theme::Light);
    let scheme = theme::light();
    let (open_strips, _) = paint(&ctx, rect, &scheme, 1.0);
    let (closed_strips, _) = paint(&ctx, rect, &scheme, 0.0);
    assert!(
        !open_strips.is_empty() && !closed_strips.is_empty(),
        "light: the penumbra paints in both states"
    );
    assert!(
        peak(&closed_strips) > 0 && peak(&closed_strips) < peak(&open_strips),
        "collapsed penumbra is fainter: {} vs {}",
        peak(&closed_strips),
        peak(&open_strips)
    );
    assert!(
        reach(&closed_strips) < reach(&open_strips),
        "collapsed penumbra is narrower: {} vs {}",
        reach(&closed_strips),
        reach(&open_strips)
    );

    // Dark: one hairline in both states, dimmer while collapsed.
    ctx.set_theme(egui::Theme::Dark);
    let scheme = theme::dark();
    let (open_strips, open_line) = paint(&ctx, rect, &scheme, 1.0);
    let (_, closed_line) = paint(&ctx, rect, &scheme, 0.0);
    assert!(open_strips.is_empty(), "dark: no black blur");
    assert!(
        closed_line > 0 && closed_line < open_line,
        "collapsed hairline is dimmer but present: {closed_line} vs {open_line}"
    );
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

/// The reveal curve is exact at both ends (the final-width geometry depends
/// on it), front-loaded (fast start, gentle tail) and the label cascade is
/// monotonic but always settles flush.
#[test]
fn reveal_curves_are_exact_and_front_loaded() {
    use super::{ease_out, label_reveal, row_reveal, STAGGER, STAGGER_MAX, TEXT_START};

    assert_eq!(ease_out(0.0), 0.0);
    assert_eq!(ease_out(1.0), 1.0);
    // Front-loaded: quintic beats the old cubic (0.875) at the midpoint.
    assert!(
        ease_out(0.5) > 0.9,
        "front-loaded ease-out at the midpoint: {}",
        ease_out(0.5)
    );

    // Labels hold back until the panel already has some width.
    assert_eq!(label_reveal(TEXT_START), 0.0);
    assert_eq!(label_reveal(1.0), 1.0);

    // Rows cascade: a lower row is never ahead of the one above it, and
    // every row lands exactly at full reveal once the panel settles.
    for i in 0..6 {
        assert_eq!(row_reveal(1.0, i), 1.0, "row {i} settles flush");
        assert!(
            row_reveal(0.5, i) <= row_reveal(0.5, i.saturating_sub(1)) + f32::EPSILON,
            "row {i} must not lead the one above it"
        );
    }
    assert!(
        row_reveal(0.6, 0) > row_reveal(0.6, 2),
        "the stagger is visible mid-reveal"
    );
    // The cap bounds the wait: rows past it share the last capped timing.
    let cap_rows = (STAGGER_MAX / STAGGER).ceil() as usize;
    assert_eq!(row_reveal(0.6, cap_rows + 3), row_reveal(0.6, cap_rows));
}

/// Mid-expansion the pinned action labels sit toward the growing edge and
/// drift to their final column as the panel settles: the reveal is a slide,
/// not a bare alpha pop. The settled frame is the zero-offset reference.
#[test]
fn labels_drift_into_place_while_expanding() {
    use super::{EXPANDED_W, RAIL_W, TITLE_SLIDE};
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

    /// Painted right edge of the overlay (`None` while not drawn).
    fn painted_edge(out: &egui::FullOutput) -> Option<f32> {
        out.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) if r.rect.max.x > RAIL_W + 1.0 && r.rect.min.y > 0.0 => {
                Some(clipped.clip_rect.max.x)
            }
            _ => None,
        })
    }

    /// X origin of the painted label whose text is `text`.
    fn label_x(out: &egui::FullOutput, text: &str) -> Option<f32> {
        out.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos.x),
            _ => None,
        })
    }

    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let mut tabs = vec![Tab::new(1)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<Rect> = None;
    let hover = Some(egui::pos2(20.0, 300.0));

    // Prime the tween collapsed (see the gradual-expansion test), then take
    // the first frame that is mid-tween AND already paints the labels.
    frame(&ctx, &mut tabs, &mut favicons, &mut overlay, None);
    let mut mid: Option<(f32, f32)> = None;
    for _ in 0..40 {
        let out = frame(&ctx, &mut tabs, &mut favicons, &mut overlay, hover);
        let edge = painted_edge(&out).unwrap_or(0.0);
        if edge > RAIL_W && edge < EXPANDED_W {
            if let Some(x) = label_x(&out, "New Tab") {
                mid = Some((x, edge));
                break;
            }
        }
    }
    let (mid_x, mid_edge) = mid.expect("a mid-tween frame paints the label");

    // Let it settle fully: zero offset is the reference.
    let mut settled_x = None;
    for _ in 0..40 {
        let out = frame(&ctx, &mut tabs, &mut favicons, &mut overlay, hover);
        if painted_edge(&out).is_some_and(|e| e >= EXPANDED_W) {
            settled_x = label_x(&out, "New Tab");
            if settled_x.is_some() {
                break;
            }
        }
    }
    let settled_x = settled_x.expect("the settled overlay paints the label");

    let drift = mid_x - settled_x;
    assert!(
        drift > 1.0 && drift <= TITLE_SLIDE + 0.5,
        "label drifts into place while expanding: {mid_x} -> {settled_x} (edge {mid_edge})"
    );
}

/// One rail frame with the given events, laid out like production (toolbar
/// above the rail) and simulating `App`: a reorder the rail reports is
/// applied to `tabs` right away (the same remove+insert `App::move_tab`
/// performs), so the live-reorder tests exercise the real loop. Returns the
/// frame output and the response the rail produced.
fn rail_frame(
    ctx: &egui::Context,
    tabs: &mut Vec<Tab>,
    favicons: &mut FaviconCache,
    overlay: &mut Option<egui::Rect>,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, super::TabsResponse) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let mut reported = super::TabsResponse::default();
    let mut out = ctx.run_ui(input, |root| {
        egui::Panel::top("test-top").show(root, |ui| {
            ui.label("toolbar");
        });
        let resp = show_tabs(root, tabs, 0, favicons, overlay);
        if let Some((from, to)) = resp.move_tab {
            if from < tabs.len() && to < tabs.len() && from != to {
                let tab = tabs.remove(from);
                tabs.insert(to, tab);
            }
        }
        reported = resp;
    });
    out.textures_delta.clear();
    (out, reported)
}

/// Painted right edge of the expanded overlay (`None` while not drawn).
fn rail_overlay_edge(out: &egui::FullOutput) -> Option<f32> {
    use super::RAIL_W;
    out.shapes.iter().find_map(|clipped| match &clipped.shape {
        egui::Shape::Rect(r) if r.rect.max.x > RAIL_W + 1.0 && r.rect.min.y > 0.0 => {
            Some(clipped.clip_rect.max.x)
        }
        _ => None,
    })
}

/// Hovers the rail until the overlay is fully expanded and returns the rail
/// top (`None` if it never expanded).
fn expand_overlay(
    ctx: &egui::Context,
    tabs: &mut Vec<Tab>,
    favicons: &mut FaviconCache,
    overlay: &mut Option<egui::Rect>,
) -> Option<f32> {
    use super::EXPANDED_W;
    let _ = rail_frame(ctx, tabs, favicons, overlay, Vec::new());
    for _ in 0..40 {
        let (out, _) = rail_frame(
            ctx,
            tabs,
            favicons,
            overlay,
            vec![egui::Event::PointerMoved(egui::pos2(20.0, 300.0))],
        );
        if rail_overlay_edge(&out).is_some_and(|e| e >= EXPANDED_W) {
            return overlay.map(|r| r.min.y);
        }
    }
    None
}

/// Dragging a rail row reorders the tabs live (the same live reorder the
/// bookmark grid applies) and a drag never selects or closes anything.
#[test]
fn dragging_a_row_reorders_the_rail() {
    use super::{PAD_TOP, ROW_GAP, ROW_H};

    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let mut tabs = vec![Tab::new(1), Tab::new(2), Tab::new(3)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<egui::Rect> = None;
    let order = |tabs: &[Tab]| tabs.iter().map(|t| t.id).collect::<Vec<_>>();
    let base = order(&tabs);

    let top = expand_overlay(&ctx, &mut tabs, &mut favicons, &mut overlay)
        .expect("the rail expands on hover");
    // Row slots have fixed geometry: PAD_TOP from the rail top, ROW_H rows
    // ROW_GAP apart. The expanded rows span the overlay's width, so x = 60
    // lands on the row (and clear of the close box).
    let slot = |i: usize| {
        egui::pos2(
            60.0,
            top + PAD_TOP + i as f32 * (ROW_H + ROW_GAP) + ROW_H * 0.5,
        )
    };
    let button = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };

    // Drag the first row down to the third slot.
    let from = slot(0);
    let to = slot(2);
    let _ = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        vec![egui::Event::PointerMoved(from)],
    );
    let _ = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        vec![button(from, true)],
    );
    let mut moves = 0;
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let p = egui::pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        let (_, resp) = rail_frame(
            &ctx,
            &mut tabs,
            &mut favicons,
            &mut overlay,
            vec![egui::Event::PointerMoved(p)],
        );
        if resp.move_tab.is_some() {
            moves += 1;
        }
        assert!(resp.select.is_none(), "a drag must never select a tab");
        assert!(resp.close.is_none(), "a drag must never close a tab");
    }
    let (_, resp) = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        vec![button(to, false)],
    );
    assert!(
        resp.select.is_none() && resp.close.is_none(),
        "the release must not activate the row"
    );
    assert!(moves >= 1, "the drag reported the live reorder");
    assert_eq!(
        order(&tabs),
        vec![base[1], base[2], base[0]],
        "the dragged first tab landed in the third slot"
    );

    // A stale drag (the type-keyed egui-memory trap) would keep reordering
    // while the pointer moves AFTER the release.
    let settled = order(&tabs);
    let _ = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        vec![egui::Event::PointerMoved(slot(0))],
    );
    let _ = rail_frame(&ctx, &mut tabs, &mut favicons, &mut overlay, Vec::new());
    assert_eq!(
        order(&tabs),
        settled,
        "the drag state must clear on release"
    );
}

/// `Alt+Up`/`Alt+Down` move the FOCUSED row one slot, the vertical
/// equivalent of the bookmark grid's `Alt+Left`/`Alt+Right`; the id is keyed
/// by the TAB, so a second press moves the SAME tab again (a slot-keyed id
/// would walk the focus onto the tab that arrived at the slot).
#[test]
fn alt_down_moves_the_focused_row_one_slot() {
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let mut tabs = vec![Tab::new(1), Tab::new(2), Tab::new(3)];
    let mut favicons = FaviconCache::new();
    let mut overlay: Option<egui::Rect> = None;
    let order = |tabs: &[Tab]| tabs.iter().map(|t| t.id).collect::<Vec<_>>();

    let key = |key: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ]
    };

    // Two frames so the rows are in egui's focus order, then Tab focuses the
    // first row (slot 0).
    let _ = rail_frame(&ctx, &mut tabs, &mut favicons, &mut overlay, Vec::new());
    let _ = rail_frame(&ctx, &mut tabs, &mut favicons, &mut overlay, Vec::new());
    let _ = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        key(egui::Key::Tab, egui::Modifiers::NONE),
    );

    let (_, resp) = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        key(egui::Key::ArrowDown, egui::Modifiers::ALT),
    );
    assert_eq!(
        order(&tabs),
        vec![2, 1, 3],
        "Alt+Down moves the focused first tab down one slot"
    );
    assert!(resp.move_tab.is_some(), "the keyboard move was reported");
    assert!(
        resp.select.is_none() && resp.close.is_none(),
        "a keyboard move must not activate or close anything"
    );

    // Focus follows the moved tab, not the slot.
    let _ = rail_frame(
        &ctx,
        &mut tabs,
        &mut favicons,
        &mut overlay,
        key(egui::Key::ArrowDown, egui::Modifiers::ALT),
    );
    assert_eq!(
        order(&tabs),
        vec![2, 3, 1],
        "the SAME tab keeps moving (focus is keyed by the tab)"
    );
}
