// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::items`: list item markers and indentation.
//!
//! Bullets (`·`), ordered markers and footnote-definition numbers are
//! measured and painted so continuation lines align; task checkboxes are
//! native egui widgets with accessible names.

use super::*;

/// Content offset of a continuation line inside an item: the content column
/// measured when the marker was drawn ([`BULLET_GUTTER`] for a bullet, the
/// padded marker width for an ordered item). Nesting is STRUCTURAL (the whole
/// nested list block is indented once per level), so adding
/// `depth * ITEM_INDENT` here would push continuation lines past their own
/// first line.
pub(super) fn item_pad(it: &ItemState) -> f32 {
    it.align
}

/// Text, font size and content offset of a non-checkbox item marker.
///
/// - Unordered items: the document bullet (`·`), larger than the body text.
/// - Ordered items: `N. ` at body size; the offset pads the number up to the
///   same content column as a bullet (with a minimum gap for wide numbers).
/// - Footnote definitions (`note`): `N. ` small, like the reference marker it
///   numbers, in the same gutter so a note reads like an ordered entry.
pub(super) fn marker_geom(
    ui: &egui::Ui,
    it: &ItemState,
) -> (std::borrow::Cow<'static, str>, f32, f32) {
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    if it.bullet {
        (it.marker.clone(), body * BULLET_SCALE, BULLET_GUTTER)
    } else {
        let size = if it.note {
            egui::TextStyle::Small.resolve(ui.style()).size
        } else {
            body
        };
        // Cache ordered-marker widths per frame burst: `layout_no_wrap`
        // shapes text, so measuring `10. ` on every scroll frame for every
        // item burns CPU on long lists. Widths only depend on the ordinal
        // (the marker text is exactly `"{ordinal}. "`), the size and the
        // face (a footnote number draws from the bold family), so the key
        // is two integers and a flag: no `String` per item per frame.
        use std::cell::RefCell;
        use std::collections::HashMap;
        thread_local! {
            static MARKER_W: RefCell<HashMap<(u64, u32, bool), f32>> =
                RefCell::new(HashMap::new());
        }
        let key = (it.ordinal, (size * 16.0).round() as u32, it.note);
        let drawn = MARKER_W.with(|c| {
            if let Some(w) = c.borrow().get(&key) {
                return *w;
            }
            let color = theme::scheme_ref(ui).on_surface;
            let w = ui
                .painter()
                .layout_no_wrap(
                    it.marker.clone().into_owned(),
                    marker_font(size, it.note),
                    color,
                )
                .size()
                .x;
            c.borrow_mut().insert(key, w);
            // Bound the cache: markers are tiny, but a hostile doc could
            // use thousands of distinct ordinals (`1. `..`9999. `).
            if c.borrow().len() > 512 {
                c.borrow_mut().clear();
            }
            w
        });
        let align = drawn + (BULLET_GUTTER - drawn).max(MARKER_GAP);
        (it.marker.clone(), size, align)
    }
}

/// Font of a non-bullet item marker: the bold family for a footnote number
/// (it echoes the number the raised reference shows), plain proportional for
/// a list ordinal.
fn marker_font(size: f32, note: bool) -> egui::FontId {
    if note {
        egui::FontId::new(size, theme::strong_family())
    } else {
        egui::FontId::proportional(size)
    }
}

/// Draws a read-only task checkbox and returns its content
/// offset: a 14px rounded square (1px gray border when pending; accent-blue
/// fill with a white check when done), centered on a body line so the item's
/// first text row stays aligned. Read-only, with a hover hint.
pub(super) fn draw_task_checkbox(ui: &mut egui::Ui, done: bool) -> f32 {
    /// Box side (px): the checkbox's visual size (the button type size).
    const BOX: f32 = theme::typography::BUTTON;
    let s = theme::scheme_ref(ui);
    let (row_rect, resp) = ui.allocate_exact_size(
        egui::vec2(BOX, theme::BODY_LINE_HEIGHT),
        egui::Sense::hover(),
    );
    let box_rect = egui::Rect::from_center_size(row_rect.center(), egui::Vec2::splat(BOX));
    let radius = egui::CornerRadius::same(crate::ui::theme::radius::SMALL);
    let painter = ui.painter();
    if done {
        painter.rect_filled(box_rect, radius, s.primary);
        // White check mark: two strokes, like the native control.
        let at = |x: f32, y: f32| box_rect.lerp_inside(egui::vec2(x, y));
        painter.add(egui::Shape::line(
            vec![at(0.25, 0.55), at(0.43, 0.73), at(0.76, 0.30)],
            egui::Stroke::new(crate::ui::theme::stroke::EMPHASIS, egui::Color32::WHITE),
        ));
    } else {
        painter.rect(
            box_rect,
            radius,
            s.surface,
            egui::Stroke::new(crate::ui::theme::stroke::HAIRLINE, s.on_surface_variant),
            egui::StrokeKind::Inside,
        );
    }
    // A screen reader otherwise sees only the item text and cannot tell
    // `[x]` from `[ ]`.
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, false, done, "Task list item")
    });
    crate::ui::hover_tooltip(resp, "Read-only");
    ui.add_space(theme::spacing::XS);
    BOX + 4.0
}

/// Ink center of a galley's first glyph, relative to the galley's top-left
/// corner.
///
/// `Glyph::uv_rect` is the glyph's INKED rectangle, measured from its
/// baseline origin, so the returned point is the real center of the painted
/// dot/digit, not the center of the (much taller) line box.
pub(super) fn glyph_ink_center(galley: &egui::Galley) -> Option<egui::Vec2> {
    let placed = galley.rows.first()?;
    let glyph = placed.row.glyphs.first()?;
    if glyph.uv_rect.is_nothing() {
        return None;
    }
    Some(
        placed.pos.to_vec2()
            + glyph.pos.to_vec2()
            + glyph.uv_rect.offset
            + glyph.uv_rect.size / 2.0,
    )
}

/// Ink bottom of a galley's first glyph, relative to the galley's top-left
/// corner. A digit's ink bottom is its BASELINE, so two markers can share a
/// baseline by aligning their ink bottoms.
pub(super) fn glyph_ink_bottom(galley: &egui::Galley) -> Option<f32> {
    let placed = galley.rows.first()?;
    let glyph = placed.row.glyphs.first()?;
    if glyph.uv_rect.is_nothing() {
        return None;
    }
    Some(placed.pos.y + glyph.pos.y + glyph.uv_rect.offset.y + glyph.uv_rect.size.y)
}

/// Paints one item marker into its gutter `cell`, which is exactly one body
/// line tall and sits on the FIRST text row (a wrapped item must not center
/// its bullet against the whole block).
///
/// Alignment is ink-based, not box-based:
///
/// - Ordered markers (`N. `, body size) share the first text row's baseline:
///   viewer text jobs have the row top as their baseline offset (TOP valign,
///   `line_height` pinned to [`theme::BODY_LINE_HEIGHT`]), so painting the
///   body-size galley at the cell top puts the number exactly on the text
///   line. Centering the glyph box used to hang it ~3px low.
/// - Footnote numbers draw at the small size, so the box-top rule does not
///   apply: their ink bottom is aligned with the ink bottom of the same
///   marker at body size (i.e. with where a list ordinal's baseline lies).
/// - The scaled document bullet keeps the OPTICAL center of a body-size
///   middot: its ink center is scaled back to body size and pinned to where
///   a `·` typed in that row would sit. The old box centering let the
///   oversized glyph descend to the baseline instead.
/// - Horizontally the bullet's INK is centered on `bullet_center_x`, which
///   the caller places next to the text column (the dot hugs its text); with
///   `None` the ink centers in the cell instead. Ink (not advance) centering
///   keeps asymmetric side bearings from shifting the dot.
/// - A selected (`selected`) note marker paints the select-all highlight
///   behind its glyphs, like a reference marker does in the text.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_item_marker(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    bullet: bool,
    note: bool,
    selected: bool,
    cell: egui::Rect,
    bullet_center_x: Option<f32>,
) {
    // Honor the enclosing scope's text override: a plain blockquote sets
    // `override_text_color` to the muted tone for its body, and the marker
    // must not pop back to full contrast inside it. The theme keeps the
    // override at `on_surface` everywhere else, so this is the normal
    // text color outside quotes.
    let color = ui
        .visuals()
        .override_text_color
        .unwrap_or_else(|| theme::scheme_ref(ui).on_surface);
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text.to_owned(), marker_font(size, note), color);
    let Some(ink) = glyph_ink_center(&galley) else {
        // No readable ink (should not happen for `·` / `N. `): keep the old
        // box-centered fallback so the marker is never lost.
        painter.galley(cell.center() - galley.size() / 2.0, galley, color);
        return;
    };
    let pos = if bullet {
        let body = egui::TextStyle::Body.resolve(ui.style()).size;
        let body_ink = glyph_ink_center(&painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(body),
            color,
        ))
        // The ink offset scales linearly with the font size, so this is the
        // body-size value even when the sample above has no atlas entry.
        .unwrap_or(ink / BULLET_SCALE);
        let cx = bullet_center_x.unwrap_or(cell.center().x);
        egui::pos2(cx - ink.x, cell.top() + body_ink.y - ink.y)
    } else if note {
        // The small number shares the baseline a body-size marker would
        // have at the cell top (see the ordered rule above).
        let body = egui::TextStyle::Body.resolve(ui.style()).size;
        let body_bottom = glyph_ink_bottom(&painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(body),
            color,
        ))
        .unwrap_or(body * 0.75);
        let bottom = glyph_ink_bottom(&galley).unwrap_or(0.0);
        egui::pos2(
            cell.center().x - galley.size().x / 2.0,
            cell.top() + body_bottom - bottom,
        )
    } else {
        egui::pos2(cell.center().x - galley.size().x / 2.0, cell.top())
    };
    if note && selected {
        painter.rect_filled(
            egui::Rect::from_min_size(pos, galley.size()),
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::select_bg(ui),
        );
    }
    painter.galley(pos, galley, color);
}

/// Draws the item marker first when this is the item's first block, then
/// returns the content indentation to use. `None` outside list items.
pub(super) fn begin_item_block(ui: &mut egui::Ui, ctx: &mut RenderCtx) -> Option<f32> {
    let selected = ctx.selected;
    let it = ctx.item.as_mut()?;
    if !it.marker_rendered {
        if let Some(done) = it.check {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                it.align = draw_task_checkbox(ui, done);
            });
        } else {
            let (text, size, align) = marker_geom(ui, it);
            let bullet = it.bullet;
            let note = it.note;
            // The block starts on its own row below the marker: a standard
            // body line cell with the marker in its gutter position.
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(align, theme::BODY_LINE_HEIGHT),
                egui::Sense::hover(),
            );
            // Same hug-the-text x as the inline first-line path
            // (`blocks::render_wrapped_lines`): a mixed list whose items
            // start with different block kinds used to show TWO bullet
            // columns, one centered in the gutter and one hugging the text.
            paint_item_marker(
                ui,
                &text,
                size,
                bullet,
                note,
                selected,
                rect,
                Some(rect.right() - BULLET_TEXT_GAP),
            );
            it.align = align;
        }
        it.marker_rendered = true;
    }
    Some(item_pad(it))
}

/// Runs `content` at an optional extra left padding (item indentation).
/// Returns the closure's value so callers can keep the inner response
/// (e.g. the blockquote bar needs the indent rect).
pub(super) fn render_indented<R>(
    ui: &mut egui::Ui,
    pad: Option<f32>,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    match pad {
        Some(pad) if pad > 0.0 => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(pad);
                ui.vertical(|ui| content(ui)).inner
            })
            .inner
        }
        _ => content(ui),
    }
}

/// Renders every item of one list, up to (but not consuming) its
/// `End(List)`. `depth` is the nesting depth of the items (0 = top level).
/// Extracted so the caller can wrap the whole list in a structural indent
/// when it is nested.
///
/// Pure move from `mod.rs`: no behavior change.
pub(super) fn render_list_items(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    ordered: bool,
    first_ordinal: u64,
    depth: usize,
) {
    let mut ordinal = first_ordinal;
    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::List(_))) {
        if matches!(events[*i], Event::Start(Tag::Item)) {
            *i += 1;
            // Unordered items always draw the same document bullet `·`,
            // whatever the source marker was (`-`, `*` or `+`): the parser
            // does not preserve it, and a printed list does not alternate
            // markers by depth. Ordered items keep their number.
            // The bullet is borrowed (zero allocation per item per frame);
            // only the ordered prefix allocates.
            let (marker, bullet) = if ordered {
                (std::borrow::Cow::Owned(format!("{ordinal}. ")), false)
            } else {
                (std::borrow::Cow::Borrowed("\u{00B7}"), true)
            };
            let saved = ctx.item.replace(ItemState {
                depth,
                marker,
                ordinal: if ordered { ordinal } else { 0 },
                bullet,
                note: false,
                check: None,
                marker_rendered: false,
                align: 0.0,
            });
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Item)) {
                render_block(ui, events, i, ctx);
            }
            if *i < events.len() {
                *i += 1; // End(Item)
            }
            // Empty item (no child block): show the marker alone.
            let pending = ctx.item.as_ref().is_some_and(|it| !it.marker_rendered);
            if pending {
                // Same marker path as any other first block (row + paint).
                let _ = begin_item_block(ui, ctx);
            }
            ctx.item = saved;
            ordinal += 1;
            // No extra gap after an item: the implicit item spacing plus the
            // compact paragraph air inside items keep the list tight.
        } else {
            *i += 1;
        }
    }
}
