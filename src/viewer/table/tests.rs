// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::render::distribute_columns;
use super::*;
fn frag(text: &str) -> Frag<'static> {
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

/// Renders one `#`/Domain/Score/Grade table and returns the grid width
/// plus the `(text, left, width)` of every Text shape of the LAST frame
/// (`Grid` settles on frame 2).
fn render_leaderboard_like() -> (f32, Vec<(String, f32, f32)>) {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 600.0),
        )),
        ..Default::default()
    };
    let header = vec![
        vec![frag("#")],
        vec![frag("Domain")],
        vec![frag("Score")],
        vec![frag("Grade")],
    ];
    let row = vec![
        vec![frag("1")],
        vec![frag("www.celticos.com")],
        vec![frag("96")],
        vec![frag("A")],
    ];
    let tb = TableCtx {
        aligns: vec![
            Alignment::Right,
            Alignment::None,
            Alignment::Right,
            Alignment::Center,
        ],
        headers: Some(header),
        rows: vec![row],
        in_head: false,
    };
    let mut total = 0.0;
    let mut texts = Vec::new();
    for frame in 0..3 {
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut no_link = |_: &str, _: crate::viewer::LinkActivation| {};
                let mut images = ImageCache::new();
                let mut emoji = crate::media::emoji::EmojiCache::new();
                let mut links = Vec::new();
                total = flush_table(
                    ui,
                    &tb,
                    0,
                    0x5A17,
                    false,
                    false,
                    1000.0,
                    &mut no_link,
                    &mut images,
                    &mut emoji,
                    &mut links,
                )
                .width();
            });
        });
        if frame == 2 {
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::epaint::Shape::Text(t) => {
                        // Wrapped labels keep a leading space inside the
                        // galley; the visible rect is the row rect
                        // without it (same math the Label uses).
                        let row = t.galley.rows[0]
                            .rect_without_leading_space()
                            .translate(t.pos.to_vec2());
                        Some((t.galley.text().to_string(), row.left(), row.width()))
                    }
                    _ => None,
                })
                .collect();
        }
        out.textures_delta.clear();
    }
    (total, texts)
}

fn shape<'a>(texts: &'a [(String, f32, f32)], needle: &str) -> &'a (String, f32, f32) {
    texts
        .iter()
        .find(|(t, _, _)| t == needle)
        .unwrap_or_else(|| panic!("{needle:?} not painted: {texts:?}"))
}

/// Regression: right/center cells used to be allocated at the full
/// `max_col_width` (340px), so `#`, Score and Grade columns each became
/// 340px wide and the real columns were pushed off the card. Columns are
/// now content-sized first, then grown to fill the card (a short table
/// leaves no blank strip) and the alignment must actually align.
#[test]
fn aligned_columns_size_to_content_and_align() {
    let (total, texts) = render_leaderboard_like();
    // The grid fills the card's inner width (~981px at this viewport):
    // it neither hugs the content (blank strip) nor overflows it.
    assert!(
        total > 900.0,
        "grid must fill the card's inner width: {total}px wide"
    );
    assert!(
        total < 990.0,
        "grid must not exceed the card: {total}px wide"
    );
    // Right: `#` header and `1` cell share a right edge.
    let (_, hx, hw) = shape(&texts, "#");
    let (_, rx, rw) = shape(&texts, "1");
    assert!(
        ((hx + hw) - (rx + rw)).abs() < 1.5,
        "`#` header must be right-aligned with `1`: {hx}+{hw} vs {rx}+{rw}"
    );
    // Right: `Score` header and `96` share a right edge.
    let (_, sx, sw) = shape(&texts, "Score");
    let (_, vx, vw) = shape(&texts, "96");
    assert!(
        ((sx + sw) - (vx + vw)).abs() < 1.5,
        "`Score` must be right-aligned with `96`: {sx}+{sw} vs {vx}+{vw}"
    );
    // Center: `Grade` header and `A` share a center.
    let (_, gx, gw) = shape(&texts, "Grade");
    let (_, ax, aw) = shape(&texts, "A");
    assert!(
        ((gx + gw / 2.0) - (ax + aw / 2.0)).abs() < 1.5,
        "`Grade` must be centered over `A`: {} vs {}",
        gx + gw / 2.0,
        ax + aw / 2.0
    );
    // Left: Domain header and value share a left edge.
    let (_, dx, _) = shape(&texts, "Domain");
    let (_, vx2, _) = shape(&texts, "www.celticos.com");
    assert!(
        (dx - vx2).abs() < 1.5,
        "`Domain` must be left-aligned with the value: {dx} vs {vx2}"
    );
}

/// A multi-run line keeps source order even in a RIGHT column: egui's
/// wrapping flow flips to RTL inside a right-aligned scope, and the old
/// code used it directly (`**a** b` rendered as `b a`). The explicit
/// left-to-right flow that replaced it must paint the runs in order.
#[test]
fn right_column_keeps_run_order() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 600.0),
        )),
        ..Default::default()
    };
    let mut strong_a = frag("alpha");
    strong_a.strong = true;
    let tb = TableCtx {
        aligns: vec![Alignment::Right],
        headers: None,
        rows: vec![vec![vec![strong_a, frag(" omega")]]],
        in_head: false,
    };
    let mut out = None;
    for _ in 0..3 {
        let mut o = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut no_link = |_: &str, _: crate::viewer::LinkActivation| {};
                let mut images = ImageCache::new();
                let mut emoji = crate::media::emoji::EmojiCache::new();
                let mut links = Vec::new();
                flush_table(
                    ui,
                    &tb,
                    0,
                    0x5A17,
                    false,
                    false,
                    1000.0,
                    &mut no_link,
                    &mut images,
                    &mut emoji,
                    &mut links,
                );
            });
        });
        o.textures_delta.clear();
        out = Some(o);
    }
    let out = out.expect("frame");
    let runs: Vec<(String, f32)> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) => {
                let row = t.galley.rows[0]
                    .rect_without_leading_space()
                    .translate(t.pos.to_vec2());
                Some((t.galley.text().to_string(), row.left()))
            }
            _ => None,
        })
        .collect();
    let alpha = runs
        .iter()
        .find(|(t, _)| t == "alpha")
        .expect("alpha painted");
    let omega = runs
        .iter()
        .find(|(t, _)| t.contains("omega"))
        .expect("omega painted");
    assert!(
        alpha.1 < omega.1,
        "right column reversed the runs: {runs:?}"
    );
}

/// The collapsed interior grid: a 2x2 table (one header row, one
/// data row) paints exactly one horizontal and one vertical separator
/// BETWEEN its cells, in the border color. The card frame (RectShape)
/// stays the outer border, and no interior line is drawn twice.
#[test]
fn interior_grid_lines_are_painted_between_cells() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let outline = crate::ui::theme::light().outline_variant;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 600.0),
        )),
        ..Default::default()
    };
    let tb = TableCtx {
        aligns: vec![Alignment::None, Alignment::None],
        headers: Some(vec![vec![frag("a")], vec![frag("b")]]),
        rows: vec![vec![vec![frag("1")], vec![frag("2")]]],
        in_head: false,
    };
    let mut out = None;
    for _ in 0..3 {
        let mut o = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut no_link = |_: &str, _: crate::viewer::LinkActivation| {};
                let mut images = ImageCache::new();
                let mut emoji = crate::media::emoji::EmojiCache::new();
                let mut links = Vec::new();
                flush_table(
                    ui,
                    &tb,
                    0,
                    0x5A17,
                    false,
                    false,
                    1000.0,
                    &mut no_link,
                    &mut images,
                    &mut emoji,
                    &mut links,
                );
            });
        });
        o.textures_delta.clear();
        out = Some(o);
    }
    let out = out.expect("frame");
    let (mut horizontal, mut vertical) = (0usize, 0usize);
    for clipped in &out.shapes {
        if let egui::epaint::Shape::LineSegment { points, stroke } = &clipped.shape {
            if stroke.color != outline {
                continue;
            }
            if (points[0].y - points[1].y).abs() < 0.01 {
                horizontal += 1;
            } else {
                vertical += 1;
            }
        }
    }
    assert_eq!(horizontal, 1, "one row separator between header and data");
    assert_eq!(vertical, 1, "one column separator between the two columns");
}

/// `distribute_columns` fills the card when there is room and shrinks
/// with wrapping when there is not, never below `min_col`.
#[test]
fn distributes_leftover_and_shrinks_with_wrap() {
    let natural = [64.0, 264.0, 64.0, 64.0];
    // Room to spare: proportional fill; the wide column grows most.
    let cols = distribute_columns(&natural, 64.0, 660.0);
    let sum: f32 = cols.iter().sum();
    assert!(
        (sum - 660.0).abs() < 0.5,
        "must fill the usable width: {cols:?}"
    );
    assert!(cols[1] > 300.0, "the wide column grows most: {cols:?}");
    assert!(cols[0] > natural[0], "small columns grow too: {cols:?}");
    // Not enough room: proportional shrink; floors pin the small
    // columns and the wide one absorbs the rest (cells wrap).
    let cols = distribute_columns(&natural, 64.0, 308.0);
    let sum: f32 = cols.iter().sum();
    assert!(sum <= 308.5, "must shrink to the usable width: {cols:?}");
    assert!(
        cols.iter().all(|w| *w >= 63.5),
        "floors respected: {cols:?}"
    );
    assert!(cols[1] < 264.0, "the wide column shrinks: {cols:?}");
    // Even the floors do not fit: the floors win (table scrolls).
    assert_eq!(
        distribute_columns(&[500.0, 500.0], 64.0, 100.0),
        vec![64.0, 64.0]
    );
}

/// Regression: f32 rounding in the grow branch summed past `usable` by an
/// ulp, and `flush_table` reads that as "even the floors do not fit" — it
/// re-laid every column out for scrolling, capped at `scroll_max`. On a
/// real-world pricing table (`86.25 + 340` grown to fill a 757px
/// card summed to `757.00006 > 757`) the second column collapsed from
/// ~604px to 128px and wrapped every couple of words while the card stayed
/// full width. A fitting layout must never report an overflow.
#[test]
fn grown_columns_never_overshoot_by_rounding() {
    let cols = distribute_columns(&[86.25, 340.0], 64.0, 757.0);
    let sum: f32 = cols.iter().sum();
    assert!(
        sum <= 757.0,
        "a fitting table must not report an overflow: {cols:?} sum {sum}"
    );
    assert!(
        cols[1] > 500.0,
        "the wide column keeps the card's width: {cols:?}"
    );
    // The real overflow path (the floors do not fit) still wins: the
    // widest column sits at its floor, so there is no residual to take.
    assert_eq!(
        distribute_columns(&[500.0, 500.0], 64.0, 100.0),
        vec![64.0, 64.0]
    );
}

/// Sweep: no fitting layout may sum past `usable`, at any column count or
/// width mix, and a real overflow (the floors do not fit) must keep the
/// floors. Guards the rounding settle against future layout tweaks.
#[test]
fn distributed_columns_never_exceed_usable_without_a_real_overflow() {
    let naturals: [[f32; 2]; 5] = [
        [86.25, 340.0],
        [74.0, 340.0],
        [48.5, 123.25],
        [10.0, 10.0],
        [1000.0, 1000.0],
    ];
    let usable_values = [
        64.0f32, 100.0, 127.0, 128.0, 200.0, 511.0, 512.0, 757.0, 1078.0,
    ];
    for natural in naturals {
        for usable in usable_values {
            let cols = distribute_columns(&natural, 64.0, usable);
            let sum: f32 = cols.iter().sum();
            let floors_fit = natural.len() as f32 * 64.0 <= usable;
            assert!(
                sum <= usable || !floors_fit,
                "natural={natural:?} usable={usable} -> {cols:?} (sum {sum})"
            );
        }
    }
    let four = [64.0f32, 264.0, 64.0, 64.0];
    for usable in usable_values {
        let cols = distribute_columns(&four, 64.0, usable);
        let sum: f32 = cols.iter().sum();
        let floors_fit = four.len() as f32 * 64.0 <= usable;
        assert!(
            sum <= usable || !floors_fit,
            "natural={four:?} usable={usable} -> {cols:?} (sum {sum})"
        );
    }
}

/// A narrow card: the same 4-column table shrinks and wraps instead of
/// forcing a horizontal scrollbar, and it still uses the card's width.
#[test]
fn narrow_cards_shrink_instead_of_scrolling() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(420.0, 600.0),
        )),
        ..Default::default()
    };
    let tb = TableCtx {
        aligns: vec![
            Alignment::Right,
            Alignment::None,
            Alignment::Right,
            Alignment::Center,
        ],
        headers: Some(vec![
            vec![frag("#")],
            vec![frag("Domain")],
            vec![frag("Score")],
            vec![frag("Grade")],
        ]),
        rows: vec![vec![
            vec![frag("1")],
            vec![frag("www.poderosamentesimple.com")],
            vec![frag("94")],
            vec![frag("A")],
        ]],
        in_head: false,
    };
    let mut total = 0.0;
    let mut avail = 0.0;
    for _ in 0..3 {
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                avail = ui.available_width();
                let mut no_link = |_: &str, _: crate::viewer::LinkActivation| {};
                let mut images = ImageCache::new();
                let mut emoji = crate::media::emoji::EmojiCache::new();
                let mut links = Vec::new();
                total = flush_table(
                    ui,
                    &tb,
                    0,
                    0x5A17,
                    false,
                    false,
                    420.0,
                    &mut no_link,
                    &mut images,
                    &mut emoji,
                    &mut links,
                )
                .width();
            });
        });
        out.textures_delta.clear();
    }
    // `flush_table` reserves `avail - 3` for the card's inner width.
    assert!(
        total <= avail - 2.0,
        "narrow table must not overflow the card: {total} vs {avail}"
    );
    assert!(
        total > avail * 0.6,
        "narrow table still uses the width: {total} vs {avail}"
    );
}

/// A table that cannot fit and scrolls keeps its measured columns up to
/// the wider `scroll_max` cap instead of collapsing every column to
/// `min_col` (the old 64px floor wrapped "Header" and "Somewhat" in the
/// middle of the word).
#[test]
fn scrolling_tables_keep_readable_columns() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 700.0),
        )),
        ..Default::default()
    };
    // 14 columns: the 64px floors (896) do not fit the ~880px card, so
    // the table scrolls.
    const COLS: usize = 14;
    let headers: Vec<Vec<Frag<'static>>> = (1..=COLS)
        .map(|i| vec![frag(&format!("Header {i}"))])
        .collect();
    let row: Vec<Vec<Frag<'static>>> = (1..=COLS)
        .map(|_| vec![frag("Somewhat long value here")])
        .collect();
    let tb = TableCtx {
        aligns: vec![Alignment::None; COLS],
        headers: Some(headers),
        rows: vec![row],
        in_head: false,
    };
    let mut total = 0.0;
    for _ in 0..3 {
        let mut out = ctx.run_ui(input.clone(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut no_link = |_: &str, _: crate::viewer::LinkActivation| {};
                let mut images = ImageCache::new();
                let mut emoji = crate::media::emoji::EmojiCache::new();
                let mut links = Vec::new();
                total = flush_table(
                    ui,
                    &tb,
                    0,
                    0x5A17,
                    false,
                    false,
                    900.0,
                    &mut no_link,
                    &mut images,
                    &mut emoji,
                    &mut links,
                )
                .width();
            });
        });
        out.textures_delta.clear();
    }
    // Old behavior: all 14 columns at the 64px floor = 896px.
    assert!(
        total > COLS as f32 * 100.0,
        "scrolling columns stayed at the fit floor: {total}"
    );
}

/// A text table is measured ONCE: an identical table hits the cache (the
/// very same `Rc`) and changed text re-measures. Media tables (image,
/// color emoji) are cached too, keyed by the texture-resolution epochs:
/// settled frames hit, while resolving the image misses exactly once
/// and re-measures instead of pinning the placeholder width.
#[test]
fn width_cache_hits_settled_tables_and_remeasures_on_resolve() {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };
    let table = |body: &str| TableCtx {
        aligns: Vec::new(),
        headers: Some(vec![vec![frag("Domain")]]),
        rows: vec![vec![vec![frag(body)]]],
        in_head: false,
    };
    let mut img_frag = frag("");
    img_frag.image = Some(crate::viewer::inline::ImageFrag {
        url: "https://ex.com/a.png".to_string().into(),
        title: None,
    });
    let media = TableCtx {
        aligns: Vec::new(),
        headers: None,
        rows: vec![vec![vec![img_frag]]],
        in_head: false,
    };
    let emoji = TableCtx {
        aligns: Vec::new(),
        headers: None,
        rows: vec![vec![vec![frag("hola 🚀")]]],
        in_head: false,
    };

    let mut hits = Vec::new();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let mut images = ImageCache::new();
            let mut emoji_cache = crate::media::emoji::EmojiCache::new();
            let mut measure = |tb: &TableCtx<'_>, idx: usize| {
                table_widths(
                    ui,
                    tb,
                    1,
                    64.0,
                    340.0,
                    false,
                    false,
                    &mut images,
                    &mut emoji_cache,
                    0x7E57,
                    idx,
                )
            };
            // The walk keys a table by the event index of its `Start`: the
            // SAME index must reuse the cached measurement on every frame,
            // and a table at another index (different content under the same
            // document salt) must not.
            hits.push(measure(&table("www.celticos.com"), 0));
            hits.push(measure(&table("www.celticos.com"), 0));
            hits.push(measure(&table("agentready.md"), 1));
            hits.push(measure(&media, 2));
            hits.push(measure(&media, 2));
            hits.push(measure(&emoji, 3));
            // The fingerprint memo is filled by the frames above: a settled
            // table costs no byte scan per frame (it used to re-hash the
            // whole table text to validate the memo).
            assert_eq!(
                cached_fingerprint(0x7E57, 0),
                Some(table_fingerprint(&table("www.celticos.com"))),
                "the fingerprint memo is reused, not recomputed"
            );
            // Resolving the cell image bumps the geometry epoch: the
            // next measure must re-measure (fresh `Rc`), then settle
            // again (same `Rc`).
            let tex = ui.ctx().load_texture(
                "epoch-test",
                egui::ColorImage::example(),
                egui::TextureOptions::NEAREST,
            );
            images.insert_ready("https://ex.com/a.png", tex);
            hits.push(table_widths(
                ui,
                &media,
                1,
                64.0,
                340.0,
                false,
                false,
                &mut images,
                &mut emoji_cache,
                0x7E57,
                2,
            ));
            hits.push(table_widths(
                ui,
                &media,
                1,
                64.0,
                340.0,
                false,
                false,
                &mut images,
                &mut emoji_cache,
                0x7E57,
                2,
            ));
        });
    });
    out.textures_delta.clear();
    assert!(
        Rc::ptr_eq(&hits[0], &hits[1]),
        "identical text must reuse the cached measurement"
    );
    assert!(
        !Rc::ptr_eq(&hits[0], &hits[2]),
        "a table at another index must not reuse the measurement"
    );
    assert!(
        table_fingerprint(&table("www.celticos.com")) != table_fingerprint(&table("agentready.md")),
        "changed text must produce a different fingerprint"
    );
    assert!(
        !Rc::ptr_eq(&hits[0], &hits[3]),
        "an image cell must not reuse a text measurement"
    );
    assert!(
        Rc::ptr_eq(&hits[3], &hits[4]),
        "a settled media table must reuse the cached measurement"
    );
    assert!(
        !Rc::ptr_eq(&hits[0], &hits[5]),
        "an emoji cell must not reuse a text measurement"
    );
    assert!(
        !Rc::ptr_eq(&hits[4], &hits[6]),
        "resolving the cell image must re-measure once"
    );
    assert!(
        Rc::ptr_eq(&hits[6], &hits[7]),
        "the re-measured media table must settle again"
    );
}
