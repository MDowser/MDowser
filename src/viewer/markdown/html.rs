// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::html`: the HTML blocks that reach the viewer.
//!
//! Most tags never get here: `markdown::document::normalize_markup` strips
//! them (transparent wrappers) before parsing. What remains is `<pre>` /
//! `<textarea>` content (inert verbatim, shown as muted monospace), malformed
//! or unbalanced markup, and HTML comments (`<!-- ... -->`), which are the
//! one invisible case: they paint nothing at all.

use super::*;

/// HTML block: shown as muted monospace text (never interpreted); an HTML
/// comment paints nothing (browser semantics).
pub(super) fn render_html_block(ui: &mut egui::Ui, mut buf: String, ctx: &mut RenderCtx) {
    if buf.trim().is_empty() {
        return;
    }
    let pad = begin_item_block(ui, ctx);
    if is_html_comment(&buf) {
        // Invisible: no text and no gap. Running after `begin_item_block`
        // keeps a comment-only list item visible as an empty marker row,
        // exactly like an empty `<li>` in a browser.
        return;
    }
    // Bound the label: one multi-MB HTML block is one giant galley.
    //
    // Byte fast path first: a buffer at or under the cap in BYTES has at
    // most that many chars (a UTF-8 char is 1..=4 bytes), so the char-exact
    // scan only runs for a block that really is oversized. The truncated
    // text is assembled in one String (the old `format!` of a truncation
    // allocated twice per frame).
    let cap = crate::config::MAX_RENDERED_BLOCK_CHARS;
    if buf.len() > cap && buf.chars().count() > cap {
        let mut out = truncate_chars(&buf, cap).into_owned();
        out.push_str(" … [truncated]");
        buf = out;
    }
    let selected = ctx.selected;
    render_indented(ui, pad, |ui| {
        // HTML stays inert text; "Select all" must still cover it. The
        // explicit muted token keeps it visibly de-emphasized: `.weak()`
        // scales whatever the default color is, which is now on_surface.
        let mut rt = egui::RichText::new(buf)
            .small()
            .monospace()
            .color(theme::scheme_ref(ui).on_surface_variant);
        if selected {
            rt = theme::selected_text(ui, rt);
        }
        body_text(ui, rt);
    });
}
