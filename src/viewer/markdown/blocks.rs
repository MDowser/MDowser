// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::blocks`: paragraph rendering.
//!
//! Paragraphs (with inline-image promotion and galleries), the inline-run
//! helpers and the block-shaped paragraph/image rules. Headings live in
//! `headings`, tables in `tables` and raw HTML blocks in `html`; every
//! rendered block spends the per-frame block budget. The recursive walk
//! itself lives in `walk`.

use super::*;

/// Renders a paragraph (inline buffer) with wrapping and modern air.
pub(super) fn render_paragraph(ui: &mut egui::Ui, mut frags: Vec<Frag<'_>>, ctx: &mut RenderCtx) {
    // Empty paragraph: only whitespace text and no image.
    if frags.is_empty()
        || frags
            .iter()
            .all(|f| f.image.is_none() && f.text.trim().is_empty())
    {
        return;
    }
    // Bound the label BEFORE any per-frame work on it (skip/menu scans and
    // the final layout): a single multi-MB paragraph is one giant galley.
    frags = cap_block_text(frags);
    // "Skip to content" style links are accessibility chrome for the HTML
    // page; the `.md` variant of many sites leaks them as the FIRST
    // paragraph. Only that first block is eligible, and only when it is a
    // single link to a fragment: a plain paragraph that happens to read
    // "Skip navigation" is never dropped.
    if ctx.allow_skip && is_skip_link(&frags) {
        return;
    }
    // Pipe/dot menus (`Home | Docs | About`, `Uno · Dos · Tres`): a single
    // line that reads as navigation, never a table (delimited tables arrive
    // as `Tag::Table`, not paragraphs).
    if ctx.quote == 0 && ctx.item.is_none() {
        if let Some(menu) = menu_items(&frags) {
            flush_menu(ui, &menu, ctx);
            return;
        }
    }
    // Card lists flattened by a site's markdown exporter: a paragraph made
    // only of links where at least one link carries body text reads as a
    // wall of blue when kept inline. Short link runs ("Docs | Chat | Forum")
    // stay inline; long-link paragraphs stack one entry per line.
    if ctx.quote == 0 && ctx.item.is_none() && link_stack_candidate(&frags) {
        render_link_stack(ui, &frags, ctx);
        return;
    }
    // POSITION POLICY: a paragraph that holds a single image renders it as
    // a centered block WHEREVER the markdown places it (top level, list
    // item or quote; the indent/quote wrapper already surrounds it). Only
    // images sharing a line with text stay inline at natural size.
    if let Some(f) = image_only(&frags) {
        // `image_only` guarantees an image frag; the `if let` keeps the
        // invariant local instead of panicking the UI thread on a future
        // regression.
        if let Some(img) = f.image.clone() {
            let alt = f.text.clone();
            let link = f.link.clone();
            let selected = ctx.selected;
            let interactive = ctx.interactive;
            let pad = begin_item_block(ui, ctx);
            render_indented(ui, pad, |ui| {
                crate::viewer::image::render_image_block(
                    ui,
                    &img,
                    &alt,
                    link.as_ref(),
                    selected,
                    interactive,
                    ctx.on_link,
                    ctx.images,
                    ctx.links,
                );
            });
            return;
        }
    }
    // Callouts (`> [!NOTE]`): badge + marker stripped.
    if ctx.quote > 0 {
        if let Some(label) = detect_callout(&frags) {
            frags = strip_callout(&frags);
            render_callout_badge(ui, label, ctx.selected);
        }
    }
    // LARGE INLINE IMAGES: a photo sharing a line with text would push the
    // neighboring words to its vertical center and open a huge blank line
    // around it. Promote those to centered blocks and let the remaining
    // text flow as its own paragraph(s) (small icons/badges stay inline).
    if frags.iter().any(|f| {
        f.image
            .as_ref()
            .is_some_and(|img| image_needs_own_block(ctx, img))
    }) {
        render_paragraph_around_blocks(ui, &frags, ctx);
        return;
    }
    // Paragraph breath: top-level prose keeps its air. Inside a quote the
    // box supplies the outer padding, so the air goes BETWEEN paragraphs
    // (before the second and later ones) instead of after the last one,
    // which used to leave the callout's bottom edge visibly loose.
    if ctx.item.is_none() && ctx.quote > 0 {
        if ctx.quote_blocks > 0 {
            ui.add_space(theme::spacing::M);
        }
        ctx.quote_blocks += 1;
    }
    render_wrapped_lines(ui, &frags, ctx);
    // Inside a list item the implicit item spacing separates blocks, so the
    // list reads as one compact block instead of a loose, airy stack.
    if ctx.item.is_none() && ctx.quote == 0 {
        ui.add_space(theme::spacing::M);
    }
}

/// One visual line of an inline flow: the fragments laid out together,
/// wrapping at the column width.
pub(super) fn render_inline_line(
    ui: &mut egui::Ui,
    line: &[Frag<'_>],
    target: Option<f32>,
    ctx: &mut RenderCtx,
) {
    // A footnote back-reference handed to this flow (see
    // `RenderCtx::pending_back_arrow`) is drawn AFTER the line's fragments,
    // so it wraps as the flow's last inline box and lands at the end of the
    // note's text on its own line instead of on a separate row.
    let back_arrow = ctx.pending_back_arrow.take();
    inline_flow(ui, line, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let selected = ctx.selected;
        let interactive = ctx.interactive;
        for f in line.iter() {
            match (target, f.image.as_ref()) {
                (Some(height), Some(img)) => crate::viewer::image::render_inline_image_at_height(
                    ui,
                    img,
                    &f.text,
                    f.link.as_ref(),
                    selected,
                    interactive,
                    ctx.on_link,
                    ctx.images,
                    ctx.links,
                    height,
                ),
                _ => render_frag(
                    ui,
                    f,
                    selected,
                    interactive,
                    ctx.on_link,
                    ctx.images,
                    ctx.emoji,
                    ctx.links,
                ),
            }
        }
        if let Some(anchor_id) = back_arrow {
            ui.add(BackArrow { anchor_id });
        }
    });
}

/// Renders inline fragments line by line (hard breaks split them), wrapping
/// at the column width. The list-item marker sits in the gutter of the FIRST
/// text row; the text column is a real left offset for the whole line, so
/// every wrapped row (and every later fragment that moves to its own row)
/// keeps the same hanging indent.
pub(super) fn render_wrapped_lines(ui: &mut egui::Ui, frags: &[Frag<'_>], ctx: &mut RenderCtx) {
    let lines = visual_lines(frags);
    let multi = lines.len() > 1;
    // Common icon height for the WHOLE block, not just one line: a card list
    // renders each card in its own row, so a per-line run never sees that all
    // its logos should match (see `block_vector_target`).
    let target = block_vector_target(ui, frags, ctx);
    for line in lines.iter() {
        if line.is_empty() {
            if multi {
                ui.add_space(theme::spacing::XS);
            }
            continue;
        }
        // First line of an item (non-checkbox AND checkbox markers): draw the
        // marker in a gutter column aligned with this first row, then render
        // the line in the remaining width. A marker drawn inside the flow as
        // a widget would center against the whole wrapped block, and later
        // rows of the flow would fall back to the item's left edge.
        let pending = ctx.item.as_ref().is_some_and(|it| !it.marker_rendered);
        if pending {
            let check = ctx.item.as_ref().and_then(|it| it.check);
            let (text, size, align) = match ctx.item.as_ref() {
                Some(it) if it.check.is_none() => marker_geom(ui, it),
                _ => (std::borrow::Cow::Borrowed(""), 0.0, 0.0),
            };
            let bullet = ctx.item.as_ref().is_some_and(|it| it.bullet);
            let note = ctx.item.as_ref().is_some_and(|it| it.note);
            let selected = ctx.selected;
            let rtl = line_is_rtl(line);
            let layout = if rtl {
                egui::Layout::right_to_left(egui::Align::Min)
            } else {
                egui::Layout::left_to_right(egui::Align::Min)
            };
            let resolved = ui
                .with_layout(layout, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let align = if let Some(done) = check {
                        draw_task_checkbox(ui, done)
                    } else {
                        // Marker cell exactly one body line tall, sharing
                        // the first text row's top so its ink lands on that
                        // row's baseline/optical center.
                        let (cell, _) = ui.allocate_exact_size(
                            egui::vec2(align, theme::BODY_LINE_HEIGHT),
                            egui::Sense::hover(),
                        );
                        // The dot hugs the text column, on the flow's own
                        // side (right of the text for RTL items).
                        let center_x = if rtl {
                            cell.left() + BULLET_TEXT_GAP
                        } else {
                            cell.right() - BULLET_TEXT_GAP
                        };
                        paint_item_marker(
                            ui,
                            &text,
                            size,
                            bullet,
                            note,
                            selected,
                            cell,
                            Some(center_x),
                        );
                        align
                    };
                    // The text owns the remaining width; every wrapped row
                    // stays inside this child, indented by construction.
                    ui.vertical(|ui| {
                        render_inline_line(ui, line, target, ctx);
                    });
                    align
                })
                .inner;
            if let Some(it) = ctx.item.as_mut() {
                it.align = resolved;
                it.marker_rendered = true;
            }
            continue;
        }
        // Continuation line: same hanging indent as the item's first line,
        // on the line's own side (left for LTR, right for RTL).
        let pad = ctx.item.as_ref().map_or(0.0, item_pad);
        if pad <= 0.0 {
            render_inline_line(ui, line, target, ctx);
            continue;
        }
        let rtl = line_is_rtl(line);
        let layout = if rtl {
            egui::Layout::right_to_left(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        };
        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(pad);
            ui.vertical(|ui| {
                render_inline_line(ui, line, target, ctx);
            });
        });
    }
}

/// Common height for the inline SVG icons of one BLOCK (paragraph or link
/// stack), or `None` when the block has fewer than two decoded vectors (a
/// lone icon keeps its intrinsic/clamped size).
///
/// Generalizes the old "consecutive icons in one line share a height" rule to
/// the whole block: a card list renders each card in its own row, so per-line
/// runs never saw that the ten logos should match. Assets declaring wildly
/// different intrinsic sizes (`width="1em"` next to a 512px viewport) then
/// render at one icon size, like a single CSS class does on the source site.
pub(super) fn block_vector_target(
    ui: &egui::Ui,
    frags: &[Frag<'_>],
    ctx: &RenderCtx,
) -> Option<f32> {
    let max_w = ui.max_rect().width().max(4.0);
    let mut count = 0usize;
    let mut max = f32::MIN;
    for f in frags {
        if let Some(h) = f
            .image
            .as_ref()
            .and_then(|img| crate::viewer::image::inline_vector_height(ctx.images, img, max_w))
        {
            count += 1;
            max = max.max(h);
        }
    }
    (count >= 2).then_some(max)
}

/// Is this inline image too large to share a line with text?
///
/// Only decidable once the texture exists: while loading, the compact
/// placeholder stays inline and the image may be promoted on the frame it
/// resolves (the layout shift is inherent to async image loading).
pub(super) fn image_needs_own_block(ctx: &RenderCtx, img: &ImageFrag) -> bool {
    let Status::Ready { tex, vector } = ctx.images.status(&img.url) else {
        return false;
    };
    let col_w = theme::content_max_width(ctx.viewport_w);
    let size = crate::viewer::image::inline_image_size(tex.size_vec2(), vector, col_w);
    // Two body lines tall, or wider than most of the reading column.
    size.y > crate::viewer::image::MAX_INLINE_IMAGE_H || (col_w > 0.0 && size.x > col_w * 0.8)
}

/// Renders a paragraph that contains at least one oversize image: pending
/// text runs render as wrapped lines, oversize images as centered blocks.
///
/// A run of 2+ CONSECUTIVE images renders as ONE centered gallery row (see
/// [`render_image_gallery`]) instead of one block per image: a photo strip
/// (`![](a)![](b)![](c)`) is a single line in the source, and per-image
/// promotion stacked it vertically and moved entries around as their
/// textures resolved one by one.
pub(super) fn render_paragraph_around_blocks(
    ui: &mut egui::Ui,
    frags: &[Frag<'_>],
    ctx: &mut RenderCtx,
) {
    let mut pending: Vec<Frag<'_>> = Vec::new();
    let flush = |ui: &mut egui::Ui, pending: &mut Vec<Frag<'_>>, ctx: &mut RenderCtx| {
        if !pending.is_empty() {
            render_wrapped_lines(ui, pending, ctx);
            pending.clear();
        }
    };
    let mut i = 0usize;
    while i < frags.len() {
        if frags[i].image.is_some() {
            let start = i;
            while i < frags.len() && frags[i].image.is_some() {
                i += 1;
            }
            let run = &frags[start..i];
            // The run is a gallery only once at least one of its images is
            // known to be block-sized: small runs (icon strips) keep the
            // normal inline flow.
            let gallery = run.len() >= 2
                && run.iter().any(|f| {
                    f.image
                        .as_ref()
                        .is_some_and(|img| image_needs_own_block(ctx, img))
                });
            if gallery {
                flush(ui, &mut pending, ctx);
                let pad = begin_item_block(ui, ctx);
                render_indented(ui, pad, |ui| render_image_gallery(ui, run, ctx));
                continue;
            }
            for f in run {
                let block_img = f
                    .image
                    .as_ref()
                    .filter(|img| image_needs_own_block(ctx, img));
                match block_img {
                    Some(img) => {
                        flush(ui, &mut pending, ctx);
                        let alt = f.text.clone();
                        let link = f.link.clone();
                        let selected = ctx.selected;
                        let interactive = ctx.interactive;
                        let pad = begin_item_block(ui, ctx);
                        render_indented(ui, pad, |ui| {
                            crate::viewer::image::render_image_block(
                                ui,
                                img,
                                &alt,
                                link.as_ref(),
                                selected,
                                interactive,
                                ctx.on_link,
                                ctx.images,
                                ctx.links,
                            );
                        });
                    }
                    None => pending.push(f.clone()),
                }
            }
        } else {
            pending.push(frags[i].clone());
            i += 1;
        }
    }
    flush(ui, &mut pending, ctx);
    // Trailing air only where the surrounding block supplies no separation:
    // in a quote the box owns the outer padding (the paragraph path gives
    // air BETWEEN blocks), and inside a list item the item spacing already
    // separates, so an unconditional 8px left the quote's bottom loose.
    if ctx.quote == 0 && ctx.item.is_none() {
        ui.add_space(theme::spacing::M);
    }
}

/// Renders a run of consecutive images as a centered gallery.
///
/// Images keep their natural (never upscaled, column-clamped) inline size,
/// flow left-to-right in source order and wrap into extra centered rows when
/// they do not fit. Items still loading/failed keep the compact placeholder
/// box, so the document order never changes while textures arrive.
pub(super) fn render_image_gallery(ui: &mut egui::Ui, run: &[Frag<'_>], ctx: &mut RenderCtx) {
    /// Horizontal gap between gallery items (px).
    const GALLERY_GAP: f32 = 8.0;
    let avail = ui.available_width().max(0.0);
    let widths: Vec<f32> = run
        .iter()
        .map(|f| match &f.image {
            Some(img) => crate::viewer::image::inline_allocation_width(ctx.images, img, avail),
            None => 0.0,
        })
        .collect();
    // Greedy rows: a single wider-than-column item stays alone and
    // `render_inline_image` clamps it to the available width.
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut row_w = 0.0_f32;
    for (idx, w) in widths.iter().copied().enumerate() {
        let extra = if row.is_empty() { w } else { GALLERY_GAP + w };
        if !row.is_empty() && row_w + extra > avail {
            rows.push(std::mem::take(&mut row));
            row_w = w;
        } else {
            row_w += extra;
        }
        row.push(idx);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    for row in rows {
        let row_w = row.iter().map(|&i| widths[i]).sum::<f32>()
            + GALLERY_GAP * row.len().saturating_sub(1) as f32;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(((avail - row_w) / 2.0).max(0.0));
            for (n, &i) in row.iter().enumerate() {
                if n > 0 {
                    ui.add_space(GALLERY_GAP);
                }
                let f = &run[i];
                if let Some(img) = &f.image {
                    crate::viewer::image::render_inline_image(
                        ui,
                        img,
                        &f.text,
                        f.link.as_ref(),
                        ctx.selected,
                        ctx.interactive,
                        ctx.on_link,
                        ctx.images,
                        ctx.links,
                    );
                }
            }
        });
    }
}

/// Scrolls the enclosing `ScrollArea` so the current cursor row sits at the
/// top of the viewport. Used by anchor navigation (headings, footnotes).
pub(super) fn scroll_to_here(ui: &egui::Ui) {
    let rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width().max(1.0), 1.0),
    );
    ui.scroll_to_rect(rect, Some(egui::Align::TOP));
}
