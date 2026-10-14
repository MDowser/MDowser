// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::budget`: block-budget helpers for the renderer.
//!
//! - [`skip_block`]: consumes one block without layout once the per-frame
//!   block budget is spent (Copy keeps the full text).
//! - [`render_truncation_notice`]: the one-line notice explaining the cut.
//! - [`render_block_flat`]: depth-guard fallback that flattens a subtree to
//!   one plain paragraph instead of recursing (no stack overflow).
//!
//! Pure moves from `mod.rs`: no behavior change. All three operate on the
//! shared [`RenderCtx`] through `super::*`.

use super::*;

/// Consumes exactly one block starting at `*i` without rendering it.
/// Used past the block budget so a crafted document cannot queue hundreds
/// of thousands of widgets per frame.
pub(super) fn skip_block(events: &[Event<'static>], i: &mut usize) {
    if !matches!(events[*i], Event::Start(_)) {
        *i += 1;
        return;
    }
    let mut depth = 1usize;
    *i += 1;
    while *i < events.len() && depth > 0 {
        match &events[*i] {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        *i += 1;
    }
}

/// Consumes one table CELL's contents without collecting fragments, leaving
/// `*i` ON its `End(TableCell)` (the table loop's shared step consumes it).
/// Used past [`crate::config::MAX_RENDERED_TABLE_CELLS`], so a wide table
/// cannot queue one widget subtree per cell per frame.
pub(super) fn skip_cell(events: &[Event<'static>], i: &mut usize) {
    let mut depth = 0usize;
    while *i < events.len() {
        match &events[*i] {
            Event::Start(_) => depth += 1,
            Event::End(TagEnd::TableCell) if depth == 0 => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        *i += 1;
    }
}

/// One-line notice once the per-document block budget is spent. The
/// document text itself stays complete: `Copy` uses `Fetched.plain_text`.
pub(super) fn render_truncation_notice(ui: &mut egui::Ui) {
    let s = theme::scheme_ref(ui);
    ui.add_space(theme::spacing::S);
    ui.label(
        egui::RichText::new("... more content hidden (the full text is still copied)")
            .small()
            .italics()
            .color(s.on_surface_variant),
    );
    ui.add_space(theme::spacing::S);
}

/// Appends `part` to `text` without letting the buffer grow past the
/// per-block render cap: the flattened subtree (depth-guard fallback) can
/// hold up to the whole 10 MiB body, and `render_paragraph` would only
/// truncate it after the per-frame copy. Characters past the cap are
/// dropped here exactly as `cap_block_text` would. `remaining` is the char
/// budget left; the appended slice is bounded by it, so the total work
/// across one flatten pass stays O(cap).
fn push_capped(text: &mut String, part: &str, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    let cut = part
        .char_indices()
        .nth(*remaining)
        .map_or(part.len(), |(i, _)| i);
    let piece = part.get(..cut).unwrap_or_default();
    text.push_str(piece);
    *remaining = remaining.saturating_sub(piece.chars().count());
}

/// Fallback when the render depth guard is reached: consumes the whole subtree
/// (balanced `Start`/`End` counting) WITHOUT recursing and shows its text as
/// one plain paragraph. Hostile nesting stays readable and, above all,
/// cannot overflow the UI thread's stack.
pub(super) fn render_block_flat(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
) {
    let mut text = String::new();
    let mut remaining = crate::config::MAX_RENDERED_BLOCK_CHARS;
    let mut balance = 0usize;
    let mut meta = 0usize;
    let mut saw_start = false;
    while *i < events.len() {
        match &events[*i] {
            Event::Start(Tag::MetadataBlock(_)) => {
                meta += 1;
                balance += 1;
                saw_start = true;
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                meta = meta.saturating_sub(1);
                balance = balance.saturating_sub(1);
            }
            Event::Start(Tag::Item | Tag::List(_)) => {
                // Item/list boundaries get a separator so deeply nested item
                // text never glues together (`ab`) at max depth.
                push_capped(&mut text, "\n", &mut remaining);
                balance += 1;
                saw_start = true;
            }
            Event::Start(Tag::Heading { .. }) => {
                // The heading's id comes from the anchor index BY EVENT
                // INDEX (see `AnchorIndex::heading_id_at`), so a flattened
                // subtree (depth guard) or a skipped one (budget, unselected
                // tab) can never shift a later heading's target.
                balance += 1;
                saw_start = true;
            }
            Event::Start(_) => {
                balance += 1;
                saw_start = true;
            }
            Event::End(_) => balance = balance.saturating_sub(1),
            Event::Text(t) if meta == 0 => push_capped(&mut text, t, &mut remaining),
            Event::Code(c) if meta == 0 => push_capped(&mut text, c, &mut remaining),
            Event::SoftBreak if meta == 0 => push_capped(&mut text, " ", &mut remaining),
            Event::HardBreak if meta == 0 => push_capped(&mut text, "\n", &mut remaining),
            Event::Rule if meta == 0 => {
                push_capped(&mut text, "\n---", &mut remaining);
            }
            _ => {}
        }
        *i += 1;
        if saw_start && balance == 0 {
            break;
        }
        if !saw_start {
            // Self-contained event (Rule, stray text): one event is the block.
            break;
        }
    }
    let frags = vec![make_frag(text, false, false, false, false, None)];
    render_paragraph(ui, frags, ctx);
}
