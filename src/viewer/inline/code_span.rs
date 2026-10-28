// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::inline::code_span`: inline code spans (the tight painted pill).
//!
//! `super::body_line_baseline` caches the body font baseline (the line every
//! inline run is translated onto); `render_code_span` lays the span out with
//! that baseline and paints the per-row background pills
//! (`code_pill_rows`).

use super::*;

/// Draws one inline code span as a tight pill hugging its glyphs.
///
/// epaint places a smaller font's baseline at its own ascent, above the
/// surrounding body text, and `RichText::code()` sizes its background from
/// the glyph's `line_height` (pinned to [`theme::BODY_LINE_HEIGHT`]), so the
/// plain label showed a wide gray band below the words. Both are fixed here:
///
/// - The galley is laid out by hand with a corrected `line_height`, so the
///   code baseline sits exactly on the body baseline, and it is placed
///   inside a rect widened by [`CODE_PILL_PAD_X`] on both sides: the
///   neighboring words keep their distance from the pill instead of the
///   padding being painted over them.
/// - ONE pill is painted per visual row, hugging that row's glyph run, so a
///   soft-wrapped span shows a pill per row instead of one giant box over
///   the whole two-row label.
/// - A span that does not fit after the previous word moves to the next
///   row as a unit (an atomic inline box), instead of breaking mid-word.
/// - The text stays a selectable [`egui::Label`] — or an [`egui::Link`] for
///   code inside links — so native selection and clicks keep working.
///
/// `selected` (select-all highlight) and multi-line code spans keep the
/// plain full-row `RichText::code()` background: the highlight must cover
/// the whole line box, and a `\n`-separated span is a block, not a pill.
/// `link` is `Some` only when the span is clickable; `color` is the text
/// color the caller picked (link blue, muted backdrop, body color).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_code_span(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    text: &str,
    selected: bool,
    color: egui::Color32,
    link: Option<&Arc<str>>,
    on_link: &mut dyn FnMut(&str, LinkActivation),
    links: &mut Vec<LinkHit>,
    find: &mut Option<crate::viewer::find::FindRender>,
    find_hl: Option<bool>,
    find_scroll: bool,
) {
    let styled = |mut rt: egui::RichText| -> egui::RichText {
        rt = rt.monospace();
        if f.strong {
            rt = theme::strong(rt);
        }
        if f.em {
            rt = rt.italics();
        }
        if f.strike {
            rt = rt.strikethrough();
        }
        rt
    };
    if text.is_empty() {
        // An empty code span (rare malformed input) has no glyph run to
        // hug: keep the zero-width body text, never a stray empty pill.
        body_text(
            ui,
            styled(egui::RichText::new(text).color(color))
                .line_height(Some(theme::BODY_LINE_HEIGHT)),
        );
        return;
    }
    if selected || text.contains('\n') {
        // Select-all: the highlight must cover the full line box.
        // Multi-line code spans: the correction below is per row and the
        // golden background keeps every row readable.
        // (A find match deliberately stays on the TIGHT pill path below:
        // this branch's `.code()` layout differs from the pill's, so
        // switching to it made a searched span re-wrap the paragraph.)
        let mut rt = styled(egui::RichText::new(text).color(color).code())
            .line_height(Some(theme::BODY_LINE_HEIGHT));
        if let Some(current) = find_hl {
            rt = theme::find_text(ui, rt, current);
        } else if selected {
            rt = theme::selected_text(ui, rt);
        }
        if let Some(dest) = link {
            let resp = ui.link(rt);
            if let Some(current) = find_hl {
                if current {
                    if let Some(fr) = find.as_mut() {
                        fr.current_rect = Some(resp.rect);
                    }
                    if find_scroll {
                        ui.scroll_to_rect(resp.rect, Some(egui::Align::Center));
                    }
                }
            }
            if !dest.starts_with('#') {
                push_link_hit(links, resp.rect, dest, resp.hovered());
            }
            if let Some(activation) = link_activation(&resp) {
                on_link(dest, activation);
            }
        } else {
            let resp = body_text(ui, rt);
            if let Some(current) = find_hl {
                if current {
                    if let Some(fr) = find.as_mut() {
                        fr.current_rect = Some(resp.rect);
                    }
                    if find_scroll {
                        ui.scroll_to_rect(resp.rect, Some(egui::Align::Center));
                    }
                }
            }
        }
        return;
    }

    // epaint lays a smaller face's baseline at ITS ascent (plus its internal
    // leading centering), not the body's: with one 16px monospace face the
    // code baseline lands ~2px below the surrounding text. Position, don't
    // re-metric: the galley is laid out naturally and translated so its first
    // glyph's baseline shares the body row's.
    //
    // A find match rides on the SAME tight pill (the amber comes from
    // `pill_fill` below), only the ink is forced readable on it: taking the
    // full-row `.code()` branch instead changed the span's geometry and
    // re-wrapped paragraphs whose code spans matched (e.g. a space query).
    let find_color = find_hl.map(|_| theme::scheme_ref(ui).on_surface);
    let color = find_color.unwrap_or(color);
    let body_baseline = super::body_line_baseline(ui);
    let mut job = std::sync::Arc::unwrap_or_clone(
        egui::WidgetText::from(styled(egui::RichText::new(text).color(color))).into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::BOTTOM,
        ),
    );

    // Placement: reserve the glyph run PLUS the pill padding as one widget,
    // so the padding is real layout air (never painted over the neighbors).
    let full = ui.available_width();
    let remaining = ui.available_size_before_wrap().x;
    // The wrap probe (a second full layout per span per frame) only matters
    // mid-row: at a fresh row (`remaining == full`) the pill never wraps,
    // and without wrapping there is nothing to decide.
    let mid_row = remaining + 0.5 < full && ui.layout().main_wrap();
    let intrinsic = if mid_row {
        // Single-row width of the run: decides whether the pill fits here.
        let mut measure = job.clone();
        measure.wrap.max_width = f32::INFINITY;
        ui.fonts_mut(|fonts| fonts.layout_job(measure)).size().x
    } else {
        0.0
    };
    if mid_row && intrinsic + 2.0 * CODE_PILL_PAD_X > remaining {
        // The pill is atomic: fill the rest of this row so the allocation
        // below wraps it whole. (A wrapped label starting after content used
        // to break mid-word and reserve an empty first row.) On a fresh row
        // there is nothing to fill: the span wraps inside the column.
        ui.add_space(remaining.max(0.0));
    }
    job.wrap.max_width = (full - 2.0 * CODE_PILL_PAD_X).max(1.0);
    job.first_row_min_height = theme::BODY_LINE_HEIGHT;
    job.halign = egui::Align::LEFT;
    job.justify = false;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    // Baseline correction: shift the run so its first glyph's baseline sits
    // where a body glyph's would in the same row (whole pixels, like
    // epaint's own rounding, so the run stays crisp).
    let delta_y = galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(0.0, |g| body_baseline - g.pos.y)
        .round();
    let (outer, _) = ui.allocate_exact_size(
        galley.size() + egui::vec2(2.0 * CODE_PILL_PAD_X, 0.0),
        egui::Sense::hover(),
    );
    let content = egui::Rect::from_min_size(
        outer.min + egui::vec2(CODE_PILL_PAD_X, delta_y),
        galley.size(),
    );

    // One pill per visual row, hugging that row's glyph run. A find match
    // tints the pill amber (stronger for the current one) instead of the
    // code background, so inline code highlights without losing its shape.
    let pill_fill = match find_hl {
        Some(true) => theme::find_current_bg(ui),
        Some(false) => theme::find_match_bg(ui),
        None => ui.visuals().code_bg_color,
    };
    for pill in code_pill_rows(&galley, content) {
        ui.painter()
            .rect_filled(pill, egui::CornerRadius::same(CODE_PILL_RADIUS), pill_fill);
    }
    if let Some(current) = find_hl {
        if current {
            if let Some(fr) = find.as_mut() {
                fr.current_rect = Some(outer);
            }
            if find_scroll {
                ui.scroll_to_rect(outer, Some(egui::Align::Center));
            }
        }
    }

    // Text (selectable) or link, drawn ON TOP of the pill: `put` places the
    // widget inside the reserved content rect without consuming more space.
    if let Some(dest) = link {
        let resp = ui.put(content, egui::Link::new(galley));
        // In-document anchors are jump targets, not pages: never a hitbox
        // for the menu or the hover destination chip.
        if !dest.starts_with('#') {
            push_link_hit(links, outer, dest, resp.hovered());
        }
        if let Some(activation) = link_activation(&resp) {
            on_link(dest, activation);
        }
    } else {
        ui.put(content, egui::Label::new(galley).selectable(true));
    }
    // `put` places the text in a child scope, and egui advances the parent
    // cursor to that child's (inner) rect: re-assert the reserved outer rect
    // or the following word would sit on the pill's right edge, losing the
    // right half of the padding.
    let _ = ui.advance_cursor_after_rect(outer);
}

/// One pill rect per galley row, hugging that row's glyph run.
///
/// `content` is where the galley is drawn (already inset by
/// [`CODE_PILL_PAD_X`]). Each row's rect spans its first-to-last non-space
/// glyph, expanded by the pill padding, and vertically reaches from the
/// tallest glyph's box top to the deepest descender: a wrapped span gets a
/// rounded pill per row instead of one box over the whole label.
pub(super) fn code_pill_rows(galley: &egui::Galley, content: egui::Rect) -> Vec<egui::Rect> {
    let mut pills = Vec::with_capacity(galley.rows.len());
    for row in &galley.rows {
        let Some(first) = row.glyphs.iter().find(|g| !g.chr.is_whitespace()) else {
            continue;
        };
        let Some(last) = row.glyphs.iter().rev().find(|g| !g.chr.is_whitespace()) else {
            continue;
        };
        let ascent = row.glyphs.iter().map(|g| g.font_ascent).fold(0.0, f32::max);
        let descent = row
            .glyphs
            .iter()
            .map(|g| g.font_height - g.font_ascent)
            .fold(0.0, f32::max);
        let baseline = content.top() + row.pos.y + first.pos.y;
        let mut top = baseline - ascent - CODE_PILL_PAD_Y;
        let mut bottom = baseline + descent + CODE_PILL_PAD_Y;
        // Shrink a taller-than-cap box symmetrically about its own center,
        // so the glyphs stay optically centered in the pill.
        if bottom - top > super::MAX_PILL_H {
            let center = (top + bottom) * 0.5;
            top = center - super::MAX_PILL_H * 0.5;
            bottom = center + super::MAX_PILL_H * 0.5;
        }
        pills.push(egui::Rect::from_min_max(
            egui::pos2(
                content.left() + row.pos.x + first.pos.x - CODE_PILL_PAD_X,
                top,
            ),
            egui::pos2(
                content.left() + row.pos.x + last.max_x() + CODE_PILL_PAD_X,
                bottom,
            ),
        ));
    }
    pills
}
