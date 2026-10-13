// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::tables`: the block-level table entry point
//! (delegates to `viewer::table::flush_table` with the document salt).

use super::*;

/// Renders a table card (indented inside list items). `table_at` is the
/// event index of the table's `Start` (see [`super::walk::render_block`]).
pub(super) fn render_table(
    ui: &mut egui::Ui,
    tb: TableCtx<'_>,
    ctx: &mut RenderCtx,
    table_at: usize,
) {
    let doc_salt = ctx.doc_salt;
    let selected = ctx.selected;
    let interactive = ctx.interactive;
    let viewport_w = ctx.viewport_w;
    let pad = begin_item_block(ui, ctx);
    match pad {
        Some(pad) if pad > 0.0 => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(pad);
                ui.vertical(|ui| {
                    flush_table(
                        ui,
                        &tb,
                        table_at,
                        doc_salt,
                        selected,
                        interactive,
                        viewport_w,
                        ctx.on_link,
                        ctx.images,
                        ctx.emoji,
                        ctx.links,
                    );
                });
            });
        }
        _ => {
            // The table's own rect is only useful to tests.
            flush_table(
                ui,
                &tb,
                table_at,
                doc_salt,
                selected,
                interactive,
                viewport_w,
                ctx.on_link,
                ctx.images,
                ctx.emoji,
                ctx.links,
            );
        }
    }
}
