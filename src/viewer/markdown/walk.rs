// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::walk`: the recursive block walk.
//!
//! `render_block` maps one parser event (and its nested content) to the
//! screen, spending the per-frame block budget and enforcing the render
//! depth guard; `bake_table_header` fixes up pulldown-cmark table heads
//! (which close without a `TableRow`).

use super::*;

pub(super) fn bake_table_header<'a>(row: Vec<Vec<Frag<'a>>>) -> Vec<Vec<Frag<'a>>> {
    row.into_iter()
        .map(|cell| {
            cell.into_iter()
                .map(|mut f| {
                    f.strong = true;
                    f
                })
                .collect()
        })
        .collect()
}

/// Consumes exactly one block starting at `*i` and renders it.
pub(super) fn render_block(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
) {
    // Per-document block budget (see `RenderCtx::blocks_left`): past it the
    // remaining blocks are consumed WITHOUT layout or widgets, so a crafted
    // document cannot queue hundreds of thousands of widgets per frame.
    // `Copy` keeps the full text; one notice explains the truncation.
    if ctx.blocks_left == 0 {
        skip_block(events, i);
        ctx.truncated = true;
        return;
    }
    ctx.blocks_left -= 1;
    // ---------- Metadata blocks (never rendered) ---------- //
    // YAML `---` metadata is disabled at the parser (frontmatter is
    // handled by `frontmatter::split`); only `+++` blocks reach here.
    if matches!(events[*i], Event::Start(Tag::MetadataBlock(_))) {
        *i += 1;
        let mut depth = 1usize;
        while *i < events.len() && depth > 0 {
            match &events[*i] {
                Event::Start(Tag::MetadataBlock(_)) => depth += 1,
                Event::End(TagEnd::MetadataBlock(_)) => depth -= 1,
                _ => {}
            }
            *i += 1;
        }
        return;
    }
    // Depth guard: hostile nesting (`> > > …`, `- - - …`, `***…`) would
    // otherwise recurse without bound and overflow the UI thread's stack
    // (uncatchable). Deeper content renders flattened instead.
    if ctx.depth >= MAX_RENDER_DEPTH {
        render_block_flat(ui, events, i, ctx);
        return;
    }
    ctx.depth += 1;
    if ctx.depth == 1 {
        // Only the very first top-level block can be a leaked skip link.
        ctx.allow_skip = ctx.first_block;
        ctx.first_block = false;
    } else {
        ctx.allow_skip = false;
    }
    match &events[*i] {
        // ---------- Blocks ---------- //
        Event::Start(Tag::Paragraph) => {
            *i += 1;
            let frags = collect_inline(events, i, ctx, false, false, false, false, None, false);
            if *i < events.len() {
                *i += 1; // End(Paragraph)
            }
            render_paragraph(ui, frags, ctx);
        }
        Event::Start(Tag::Heading { level, .. }) => {
            let level = *level;
            // Heading ids are looked up by the heading's OWN event index
            // (`AnchorIndex::heading_id_at`): the walk skips content
            // (unselected `<Tab>` panels, closed accordions, the block
            // budget), and a render-order counter handed every later heading
            // the wrong target.
            //
            // The id is only CONSUMED to satisfy a pending anchor (see
            // `render_heading`), so settled frames clone nothing: the
            // per-heading `String` clone used to run on every frame.
            let heading_id = ctx
                .pending_anchor
                .is_some()
                .then(|| ctx.anchors.heading_id_at(*i).map(str::to_string))
                .flatten();
            *i += 1;
            let frags = collect_inline(events, i, ctx, false, false, false, false, None, false);
            if *i < events.len() {
                *i += 1; // End(Heading)
            }
            render_heading(ui, frags, level, heading_id.as_deref(), ctx);
        }
        Event::Start(Tag::BlockQuote(_)) => {
            // The block's OWN event index keys its inner indent (see
            // `quote_indent_id`), exactly like code blocks and component
            // markers do: a per-pass counter let two different quotes alias
            // onto one set of nested widget ids whenever a frame rendered a
            // different subset of blocks (an unselected `<Tabs>` panel, a
            // closed accordion, the block budget).
            let quote_at = *i;
            *i += 1;
            ctx.quote += 1;
            let level = ctx.quote;
            let id = quote_indent_id(ctx.doc_salt, level, quote_at);
            let scheme = theme::scheme_ref(ui);
            // Alert callouts (`> [!NOTE]`) tint bar and background with the
            // alert accent; plain quotes keep the neutral gray.
            let callout = detect_callout_ahead(events, *i);
            let accent = callout.map(|label| callout_color(scheme, label));
            // The quote box background is reserved BEFORE the content, so it
            // paints behind it (same trick as the inline code pill); its
            // final rect is only known once the quoted blocks laid out.
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            // Quote as an item's first block: the item marker belongs at
            // the ITEM's indentation, before the quote's own indent. The
            // lazy marker draw used to fire inside the quote, painting
            // the bullet next to the quoted text.
            //
            // `BOX_PAD` is the box's padding around the content; `BAR_W` is
            // the single thick left bar; `QUOTE_GAP` separates consecutive
            // quote boxes (their backgrounds expand BOX_PAD beyond the
            // content, so without it the next box would touch).
            const BAR_W: f32 = 5.0;
            const BOX_PAD: f32 = 6.0;
            const QUOTE_GAP: f32 = 12.0;
            let pad = begin_item_block(ui, ctx);
            // The box spans the full quote column (callouts are block
            // elements), not just the widest line: record the indented
            // region's right edge and reserve the box's right padding inside
            // the layout, so a full-width line never touches the background.
            let mut box_right = 0.0f32;
            let resp = render_indented(ui, pad, |ui| {
                ui.indent(id, |ui| {
                    box_right = ui.max_rect().max.x;
                    ui.set_max_width((ui.available_width() - BOX_PAD).max(1.0));
                    // Plain quotes render muted `#59636e` text; alert bodies
                    // keep the normal color (callout bodies inherit the
                    // normal text color). Explicit colors (links, code,
                    // headings) override this.
                    if accent.is_none() {
                        ui.visuals_mut().override_text_color = Some(scheme.on_surface_variant);
                    }
                    let saved_quote_blocks = ctx.quote_blocks;
                    ctx.quote_blocks = 0;
                    while *i < events.len()
                        && !matches!(events[*i], Event::End(TagEnd::BlockQuote(_)))
                    {
                        render_block(ui, events, i, ctx);
                    }
                    ctx.quote_blocks = saved_quote_blocks;
                })
            });
            if *i < events.len() {
                *i += 1; // End(BlockQuote)
            }
            ctx.quote -= 1;
            // `indent` response covers the whole quoted block.
            let rect = resp.response.rect;
            if rect.height() > 1.0 {
                // ONE thick bar (egui's indent guide line is disabled in
                // `theme::apply`) with the whole quote box on a soft
                // background: a neutral wash for plain quotes, the alert
                // accent at ~9% for callouts. The left gutter comes from the
                // active style instead of a hardcoded `-18`, so zoom/font
                // changes cannot move the bar.
                let indent_w = ui.style().spacing.indent.max(8.0);
                let right = if box_right > rect.min.x {
                    box_right
                } else {
                    rect.max.x + BOX_PAD
                };
                let box_rect = egui::Rect {
                    min: egui::pos2(rect.min.x - indent_w + 2.0, rect.min.y - BOX_PAD),
                    max: egui::pos2(right, rect.max.y + BOX_PAD),
                };
                let fill = match accent {
                    Some(c) => egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 24),
                    None => scheme.container_low,
                };
                ui.painter().set(
                    bg_idx,
                    egui::Shape::rect_filled(
                        box_rect,
                        egui::CornerRadius::same(theme::radius::CARD),
                        fill,
                    ),
                );
                let bar = egui::Rect {
                    min: box_rect.min,
                    max: egui::pos2(box_rect.min.x + BAR_W, box_rect.max.y),
                };
                ui.painter().rect(
                    bar,
                    egui::CornerRadius {
                        nw: theme::radius::CARD,
                        sw: theme::radius::CARD,
                        ne: 0,
                        se: 0,
                    },
                    accent.unwrap_or(scheme.outline_variant),
                    egui::Stroke::NONE,
                    egui::StrokeKind::Inside,
                );
            }
            // Air after the box: the natural item spacing alone is smaller
            // than the two boxes' paddings, which made them touch.
            ui.add_space(QUOTE_GAP);
        }
        Event::Start(Tag::CodeBlock(kind)) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) => {
                    // Server-controlled: bound what is stored/displayed (the
                    // chip draws it; the copy body comes from the events).
                    // Only the language token is shown, so docs-generator
                    // `title=` metadata never leaks into the chip.
                    let label = crate::markdown::highlight::info_lang(l);
                    if label.trim().is_empty() {
                        None
                    } else {
                        Some(crate::util::truncate_chars(
                            label,
                            crate::config::MAX_LANG_CHARS,
                        ))
                    }
                }
                CodeBlockKind::Indented => None,
            };
            *i += 1;
            // The block's EVENT index keys its cache entry and scroll state
            // (see `viewer::code`): stable per document, independent of what
            // the walk renders this frame, so switching a `<Tabs>` panel can
            // never make one block inherit another's cached body.
            let block_at = *i - 1;
            let doc_salt = ctx.doc_salt;
            let selected = ctx.selected;
            let pad = begin_item_block(ui, ctx);
            // Borrowed fallback: an unlabeled fence must not allocate per
            // frame just to name the chip.
            let lang = lang.as_deref().unwrap_or("code");
            // `code::render_code_block` consumes the block's events (copying
            // the text only on the first frame; later frames hit the cache)
            // and draws the card inside the item/quote indentation.
            let budget = &mut ctx.code_lines_left;
            render_indented(ui, pad, |ui| {
                render_code_block(ui, events, i, lang, selected, doc_salt, block_at, budget);
            });
        }
        Event::Start(Tag::HtmlBlock) => {
            // The marker's own event index: keys per-instance state (e.g. the
            // `<Tabs>` selection) so two instances with the same labels can
            // never share it.
            let marker_at = *i;
            *i += 1;
            // Bound the buffer WHILE collecting: a multi-MB HTML block used
            // to be concatenated per frame before `render_html_block` trimmed
            // it. One char PAST the render cap is collected (never more), so
            // the renderer still sees an oversized block and shows its
            // `… [truncated]` notice while the per-frame allocation stays at
            // the cap instead of 4x it.
            let collect_cap = crate::config::MAX_RENDERED_BLOCK_CHARS.saturating_add(1);
            let mut chars = 0usize;
            let mut buf = String::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::End(TagEnd::HtmlBlock) => {
                        *i += 1;
                        break;
                    }
                    Event::Html(h) | Event::Text(h) => {
                        if chars < collect_cap {
                            for ch in h.chars().take(collect_cap - chars) {
                                buf.push(ch);
                                chars += 1;
                            }
                        }
                        *i += 1;
                    }
                    _ => *i += 1,
                }
            }
            if let Some(fields) = crate::markdown::document::component_fields(&buf) {
                render_component(ui, events, i, ctx, &fields, marker_at);
            } else {
                render_html_block(ui, buf, ctx);
            }
        }
        Event::Start(Tag::List(start)) => {
            let ordered = start.is_some();
            let first_ordinal = start.unwrap_or(1);
            *i += 1;
            // Nested lists indent one level deeper than their parent item.
            let depth = ctx.item.as_ref().map_or(0, |it| it.depth + 1);
            // A nested list as the item's first block: draw the parent
            // marker first, at the item's own indentation. Drawing it at
            // the end of the item left the bullet AFTER the nested content.
            // The returned pad is the parent's measured content offset:
            // nested content starts at least at the parent's text column
            // (`max`), so a wide marker (`10. `, `[x] `) never lets the
            // nested list hang out to the LEFT of the parent's text; narrow
            // markers keep the normal one-step indent.
            let parent_pad = begin_item_block(ui, ctx).unwrap_or(0.0);
            if depth == 0 {
                render_list_items(ui, events, i, ctx, ordered, first_ordinal, depth);
            } else {
                render_indented(ui, Some(parent_pad.max(ITEM_INDENT)), |ui| {
                    render_list_items(ui, events, i, ctx, ordered, first_ordinal, depth);
                });
            }
            if *i < events.len() {
                *i += 1; // End(List)
            }
        }
        Event::Start(Tag::FootnoteDefinition(label)) => {
            // Server-controlled label: bound what is rendered per frame.
            let label = crate::util::truncate_chars(label, crate::config::MAX_FOOTNOTE_LABEL_CHARS);
            // `[^label]` references navigate to the RESERVED id (`fn-<label>`
            // or its deduped form when a heading claimed the plain id); the
            // fragment may arrive percent-encoded or mixed case.
            let reserved_id = ctx.anchors.footnote_id(&label);
            if let Some(anchor_id) = reserved_id {
                let hit = ctx.pending_anchor.as_deref().is_some_and(|pending| {
                    crate::markdown::anchors::fragment_matches(anchor_id, pending)
                });
                if hit {
                    scroll_to_here(ui);
                    *ctx.pending_anchor = None;
                }
            }
            // A definition the document never references has no reference
            // number and no place to scroll back to; it still renders (never
            // dropped) and continues the numbering.
            let referenced = ctx.anchors.footnote_number(&label);
            *i += 1;
            // Footnote as an item's first block: item marker first. The
            // returned pad is the item's content column, reused below so
            // the note starts where the item's own text would.
            let enclosing_pad = begin_item_block(ui, ctx);
            if ctx.footnotes_started {
                ui.add_space(crate::ui::theme::spacing::M);
            } else {
                // ONE rule before the first definition (the footnotes-section
                // border, 1px like the H1/H2 rules): the old code drew a
                // full rule before EVERY definition.
                ctx.footnotes_started = true;
                ui.add_space(crate::ui::theme::spacing::L);
                ui.separator();
                ui.add_space(crate::ui::theme::spacing::XS);
            }
            let number = referenced.unwrap_or_else(|| {
                ctx.unreferenced_footnotes += 1;
                ctx.anchors.footnote_count() + ctx.unreferenced_footnotes
            });
            // The definition renders like a numbered entry: the marker is the
            // reference's number (`N. `, small and bold) sharing the FIRST
            // text line with the note body, and wrapped lines/continuation
            // blocks hang under the text column (`ItemState::note`).
            // `render_wrapped_lines` draws the marker inline on the first
            // row; a first block that cannot flow inline (code fence, list,
            // quote, rule, image block) falls back through `begin_item_block`
            // to the marker on its own row above the indented block, like a
            // list item does.
            let marker = std::borrow::Cow::Owned(format!("{number}. "));
            let saved_item = ctx.item.replace(ItemState {
                depth: 0,
                marker,
                ordinal: number as u64,
                bullet: false,
                note: true,
                check: None,
                marker_rendered: false,
                align: 0.0,
            });
            render_indented(ui, enclosing_pad, |ui| {
                // Back-reference (referenced definitions only): a small muted
                // vector arrow (the chrome's own `Icon::Back`) that scrolls
                // to the reference the marker was painted at this frame
                // (`render_superscript_ref` remembers the rect under the
                // reserved anchor id). A vector glyph, NOT the `↩`
                // character: U+21A9 is emoji-capable, so the emoji fallback
                // face drew its boxed "return key" symbol instead of a text
                // arrow.
                //
                // It belongs at the END of the note's text on the SAME line,
                // so it is handed to the note's own inline flow as its last
                // inline box (see `pending_back_arrow`) instead of being
                // allocated as a separate row under the note.
                ctx.pending_back_arrow = if referenced.is_some() {
                    reserved_id
                } else {
                    None
                };
                while *i < events.len()
                    && !matches!(events[*i], Event::End(TagEnd::FootnoteDefinition))
                {
                    render_block(ui, events, i, ctx);
                }
                // Empty definition (no child block): show the marker alone,
                // like an empty list item.
                if ctx.item.as_ref().is_some_and(|it| !it.marker_rendered) {
                    let _ = begin_item_block(ui, ctx);
                }
                // No inline flow ran (a note ending in a code fence, a table,
                // a list or nothing): fall back to a trailing row, so the
                // back-reference is never lost.
                if let Some(anchor_id) = ctx.pending_back_arrow.take() {
                    let anchor_id = anchor_id.to_string();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), BACK_ARROW_W),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add(BackArrow {
                                anchor_id: &anchor_id,
                            })
                        },
                    );
                }
            });
            ctx.item = saved_item;
            if *i < events.len() {
                *i += 1; // End(FootnoteDefinition)
            }
            ui.add_space(crate::ui::theme::spacing::XS);
        }
        // ---------- tables ---------- //
        Event::Start(Tag::Table(aligns)) => {
            // The table's EVENT index keys its width cache and scroll id
            // (see `viewer::table`): stable per document, so a `<Tabs>`
            // selection can never make one table inherit another's state.
            let table_at = *i;
            let aligns = aligns.clone();
            *i += 1;
            let mut tb = TableCtx {
                aligns,
                headers: None,
                rows: Vec::new(),
                in_head: false,
            };
            let mut cur_row: Vec<Vec<Frag<'_>>> = Vec::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::End(TagEnd::Table) => {
                        *i += 1;
                        break;
                    }
                    Event::Start(Tag::TableHead) => {
                        tb.in_head = true;
                        *i += 1;
                    }
                    // pulldown-cmark closes the header with `End(TableHead)`
                    // and does NOT emit `End(TableRow)` for it: commit the
                    // accumulated cells here or the header would be silently
                    // dropped by the next `Start(TableRow)` clear.
                    Event::End(TagEnd::TableHead) => {
                        tb.in_head = false;
                        let row = std::mem::take(&mut cur_row);
                        if tb.headers.is_none() && !row.is_empty() {
                            tb.headers = Some(bake_table_header(row));
                        }
                        *i += 1;
                    }
                    Event::Start(Tag::TableRow) => {
                        if tb.rows.len() >= crate::config::MAX_TABLE_ROWS
                            || ctx.blocks_left == 0
                            || ctx.table_cells_left == 0
                        {
                            // Row or cell budget spent (or the document's
                            // whole block budget): consume the row without
                            // collecting cells (a table can hold ~100k rows
                            // under the event cap as ONE block, and one row
                            // can hold thousands of cells).
                            ctx.truncated = true;
                            *i += 1;
                            while *i < events.len() {
                                match &events[*i] {
                                    Event::End(TagEnd::TableRow) => {
                                        *i += 1;
                                        break;
                                    }
                                    _ => *i += 1,
                                }
                            }
                        } else {
                            // A row lays out like a block: charge the
                            // per-frame budget so many-row tables cannot
                            // bypass `MAX_RENDERED_BLOCKS`.
                            ctx.blocks_left -= 1;
                            cur_row.clear();
                            *i += 1;
                        }
                    }
                    Event::End(TagEnd::TableRow) => {
                        let row = std::mem::take(&mut cur_row);
                        if tb.in_head && tb.headers.is_none() {
                            tb.headers = Some(bake_table_header(row));
                        } else {
                            tb.rows.push(row);
                        }
                        *i += 1;
                    }
                    Event::Start(Tag::TableCell) => {
                        *i += 1;
                        if ctx.table_cells_left == 0 {
                            // Cell budget spent: consume the cell without
                            // collecting fragments (no layout, no widgets).
                            ctx.truncated = true;
                            skip_cell(events, i);
                        } else {
                            ctx.table_cells_left -= 1;
                            let cell = collect_inline(
                                events, i, ctx, false, false, false, false, None, true,
                            );
                            // A single huge cell is one giant label too.
                            cur_row.push(cap_block_text(cell));
                        }
                        if *i < events.len() {
                            *i += 1; // End(TableCell)
                        }
                    }
                    _ => *i += 1,
                }
            }
            render_table(ui, tb, ctx, table_at);
        }
        Event::Rule => {
            *i += 1;
            let pad = begin_item_block(ui, ctx);
            render_indented(ui, pad, |ui| {
                ui.add_space(crate::ui::theme::spacing::M);
                theme::rule(ui);
                ui.add_space(crate::ui::theme::spacing::M);
            });
        }
        // ---------- Inline runs (tight list items, stray inline) ---------- //
        Event::Text(_)
        | Event::Code(_)
        | Event::Html(_)
        | Event::InlineHtml(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::FootnoteReference(_)
        | Event::TaskListMarker(_)
        | Event::Start(
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. },
        ) => {
            let frags = collect_inline(events, i, ctx, false, false, false, false, None, false);
            render_paragraph(ui, frags, ctx);
        }
        // Anything else (End markers at top level, unsupported starts): skip.
        _ => *i += 1,
    }
    ctx.depth -= 1;
}

/// Horizontal slot reserved for the footnote back-arrow button on the
/// fallback row (px).
const BACK_ARROW_W: f32 = 18.0;

/// The footnote definition's back-reference button: a small muted vector
/// arrow that scrolls to the reference the note's marker was painted at
/// (the rect `render_superscript_ref` remembered this frame under the
/// reserved `#fn-…` anchor id).
///
/// A vector glyph, NOT the `↩` character: U+21A9 is emoji-capable, so the
/// emoji fallback face drew its boxed "return key" symbol instead of a text
/// arrow. Placed by the footnote branch into the gutter it reserved beside
/// the note's text column, so the button sits on the note's LAST text line;
/// only a REFERENCED definition places it (an unreferenced one has nowhere
/// to go).
pub(super) struct BackArrow<'a> {
    pub(super) anchor_id: &'a str,
}

impl egui::Widget for BackArrow<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        /// Clickable side of the arrow button (px); the glyph is inset inside.
        const BOX: f32 = 18.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(BOX, theme::BODY_LINE_HEIGHT),
            egui::Sense::click(),
        );
        let box_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(BOX));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let s = theme::scheme_ref(ui);
        if resp.hovered() {
            ui.painter().rect_filled(
                box_rect,
                egui::CornerRadius::same(theme::radius::SMALL),
                s.container_high,
            );
        } else if resp.has_focus() {
            theme::focus_ring(ui, box_rect, theme::radius::SMALL);
        }
        crate::ui::icons::paint(
            ui,
            box_rect.shrink(4.0),
            crate::ui::icons::Icon::Back,
            if resp.hovered() {
                s.primary
            } else {
                s.on_surface_variant
            },
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back to the reference")
        });
        let resp = crate::ui::hover_tooltip(resp, "Back to the reference");
        if resp.clicked() {
            let dest = format!("#{}", self.anchor_id);
            if let Some(rect) = crate::viewer::inline::fnref_rect_for(ui, &dest) {
                ui.scroll_to_rect(rect, Some(egui::Align::BOTTOM));
            }
        }
        resp
    }
}
