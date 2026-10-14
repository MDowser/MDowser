// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::table::render`: the measuring/layout pass and the paint pass
//! for Markdown tables.
//!
//! `flush_table` measures every cell line, caches the widths (`super`) and
//! renders the card, the interior grid and the rows; the helpers here are
//! private to the table module except `distribute_columns`, which the
//! width tests exercise directly.

use super::*;

/// Renders a table in a card with a striped `Grid` and unique id.
/// - Respects per-column `Alignment` (left/center/right).
/// - Tighter spacing on narrow windows so tables fit without scrolling sooner.
/// - Column widths come from ONE invisible measurement pass over every cell
///   line (wrapped at `max_col_width`, clamped to
///   `[min_col_width, max_col_width]`); the render pass then allocates
///   exactly that width per cell. This is required, not an optimization:
///   egui's `Grid` reports `max_col_width` as the available width of every
///   non-last column and sizes a column from the widgets inside it, so a
///   centered/right cell that used the available width would both align
///   against the cap and inflate its column to it.
/// - A table that cannot fit shrinks toward `min_col`; when even that does
///   not fit it scrolls, and the columns keep their measured width up to the
///   wider `scroll_max` instead of collapsing to `min_col`.
/// - Ids are bounded (`md-table-{idx}-{salt}`): document salt prevents
///   cross-page state leaks while keeping egui memory bounded.
///
/// Returns the Grid's own rect (its width is the sum of the column widths);
/// the card around it still fills the reading column.
#[allow(clippy::too_many_arguments)]
pub(crate) fn flush_table(
    ui: &mut egui::Ui,
    tb: &TableCtx<'_>,
    idx: usize,
    doc_salt: u64,
    selected: bool,
    interactive: bool,
    viewport_w: f32,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
) -> egui::Rect {
    ui.add_space(theme::spacing::S);
    let narrow = viewport_w < 600.0;
    // Column count from the widest row (headers or body).
    let mut ncols = tb.aligns.len();
    if let Some(h) = tb.headers.as_ref() {
        ncols = ncols.max(h.len());
    }
    for r in &tb.rows {
        ncols = ncols.max(r.len());
    }
    let ncols = ncols.max(1);
    // Card fills the reading column; Grid columns size to content inside it.
    // - `min`: keeps tiny cells (e.g. `#`, `x`) from collapsing to 0.
    // - `scroll_max`: a table that cannot fit and scrolls keeps each column
    //   at its measured width up to this cap; squeezing every column to
    //   `min_col` (the fit floor) broke words like "Header" mid-word.
    // - `max`: long prose cells wrap instead of forcing the whole table into
    //   a far horizontal scroll.
    let avail = ui.available_width().max(0.0);
    // The card frame is the table's outer border, so the grid spans its
    // inner area edge to edge (a collapsed border grid); the 5px/11px
    // cell padding lives INSIDE each cell (`render_table_cell`). The card's
    // 1px stroke narrows the inner `ScrollArea` by 2px, and 1px more of
    // slack keeps rounding from triggering the horizontal scrollbar.
    // `inner` is the width that inner ScrollArea really sees: both the
    // scroll decision below and the reserved bar lane compare against it,
    // or a table overflowing in the 1-2px window showed its bar over the
    // last row without a lane.
    let inner = (avail - 3.0).max(0.0);
    let (min_col, scroll_max, max_col) = if narrow {
        (
            48.0_f32,
            112.0_f32,
            220.0_f32.min(inner.max(160.0)).min((inner * 0.6).max(48.0)),
        )
    } else {
        (
            64.0_f32,
            128.0_f32,
            340.0_f32.min(inner.max(200.0)).min((inner * 0.6).max(64.0)),
        )
    };
    let measured = table_widths(
        ui,
        tb,
        ncols,
        min_col,
        max_col,
        selected,
        interactive,
        images,
        emoji,
        doc_salt,
        idx,
    );
    // Final columns: the measured (content + padding) widths are grown to
    // fill the card's inner width, or shrunk proportionally so a wide table
    // wraps instead of forcing a horizontal scrollbar (see
    // `distribute_columns`). Cells are adjacent (spacing 0): the collapsed
    // border sits exactly on the shared edge.
    let mut cols = distribute_columns(&measured.widths.cols, min_col, inner);
    // Even the fit floors do not fit: the table scrolls. Re-layout keeping
    // every column at its measured width up to `scroll_max` (same overflow
    // condition egui uses to show the bar), so the columns the user scrolls
    // through stay readable instead of 48/64px slivers of wrapped words.
    // Compared against `inner`, the width the inner `ScrollArea` really
    // sees (the card frame's 1px stroke narrows it): comparing against
    // `avail` left a 1-2px window where the bar appeared for a table that
    // barely overflowed while `bar_lane` (same condition) was not reserved.
    // `distribute_columns` settles its rounding, so this strict test only
    // fires on a REAL overflow: an f32 residual must never send a fitting
    // table down the scrolling path (see `settle_rounding`).
    if cols.iter().sum::<f32>() > inner {
        cols = measured
            .widths
            .cols
            .iter()
            .map(|w| w.min(scroll_max))
            .collect();
    }
    // The Grid must allow the grown widths (its `max_col_width` caps every
    // non-last column's available width).
    let grid_max = cols.iter().copied().fold(max_col, f32::max);
    let rows = &measured.widths.rows;
    ui.set_min_width(avail.max(0.0));
    // A table wider than the card scrolls horizontally: reserve the bar's
    // lane under the grid (same condition egui uses to show the bar, so the
    // card only grows when it actually scrolls; a table that fits keeps its
    // grid flush with the card border). Without the lane the floating handle
    // covered the last row while scrolling.
    let grid_w: f32 = cols.iter().sum();
    let bar_lane = (grid_w > inner).then(|| theme::scrollbar_lane(ui));
    // The Grid's own rect: its width is the sum of the measured columns
    // (the card around it fills the reading column separately). Returned so
    // tests can assert column proportions without the card's min width.
    let mut grid_rect = egui::Rect::NOTHING;
    theme::table_card(ui).show(ui, |ui| {
        ui.set_min_width(inner.max(0.0));
        // Horizontal scroll for wide tables (code already had it).
        // Id salted by document: same index on another page never shares state.
        let mut scroll = egui::ScrollArea::horizontal()
            .id_salt(("table-scroll", idx, doc_salt))
            .auto_shrink([false, false]);
        if let Some(lane) = bar_lane {
            scroll = scroll.content_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 0,
                bottom: lane,
            });
        }
        scroll.show(ui, |ui| {
            // Cell rects per row: the interior grid is painted from them
            // after the Grid lays the table out (see `paint_table_grid`).
            let mut cell_rows: Vec<Vec<egui::Rect>> = Vec::new();
            grid_rect = egui::Grid::new(("md-table", idx, doc_salt))
                .num_columns(ncols)
                .striped(true)
                .spacing([0.0, 0.0])
                .min_col_width(min_col)
                .max_col_width(grid_max)
                .show(ui, |ui| {
                    let mut row_idx = 0usize;
                    if let Some(headers) = tb.headers.as_ref() {
                        let rect = render_table_row(
                            ui,
                            headers,
                            rows.get(row_idx).map(Vec::as_slice),
                            ncols,
                            &tb.aligns,
                            &cols,
                            selected,
                            interactive,
                            on_link,
                            images,
                            emoji,
                            links,
                        );
                        cell_rows.push(rect);
                        row_idx += 1;
                    }
                    for row in &tb.rows {
                        let rect = render_table_row(
                            ui,
                            row,
                            rows.get(row_idx).map(Vec::as_slice),
                            ncols,
                            &tb.aligns,
                            &cols,
                            selected,
                            interactive,
                            on_link,
                            images,
                            emoji,
                            links,
                        );
                        cell_rows.push(rect);
                        row_idx += 1;
                    }
                })
                .response
                .rect;
            paint_table_grid(ui, &cell_rows);
        });
    });
    ui.add_space(theme::spacing::M);
    grid_rect
}

/// Final column widths, browser-like (`table { width: 100% }` auto layout):
///
/// - When the measured widths fit, the leftover card width is distributed
///   proportionally to content, so a short table fills the drawn border
///   instead of leaving a blank strip on the right.
/// - When they do not fit, widths shrink proportionally down to `min_col`
///   and the cells WRAP; columns pinned at the floor leave the remaining
///   space to the others (iterated), so a narrow window does not produce a
///   horizontal scrollbar while the table can still fit.
/// - If even the floors do not fit, the floors win and the table scrolls.
///
/// A FITTING layout never sums past `usable`, not even by a rounding ulp:
/// callers read `sum > usable` as "the table does not fit" and re-lay every
/// column out for horizontal scrolling (see [`settle_rounding`]).
pub(super) fn distribute_columns(natural: &[f32], min_col: f32, usable: f32) -> Vec<f32> {
    let n = natural.len();
    if n == 0 {
        return Vec::new();
    }
    let total: f32 = natural.iter().copied().sum();
    if total <= 0.0 {
        return vec![min_col; n];
    }
    if total <= usable {
        // Grow: proportional to content, like a browser's auto table layout.
        let extra = usable - total;
        let mut cols: Vec<f32> = natural.iter().map(|w| w + extra * (w / total)).collect();
        settle_rounding(&mut cols, usable, min_col);
        return cols;
    }
    // Shrink with a `min_col` floor; pin the columns that would fall below
    // it and let the others absorb the rest.
    let mut fixed = vec![false; n];
    loop {
        let free_total: f32 = natural
            .iter()
            .enumerate()
            .filter(|(i, _)| !fixed[*i])
            .map(|(_, w)| *w)
            .sum();
        if free_total <= 0.0 {
            break;
        }
        let fixed_sum = fixed.iter().filter(|f| **f).count() as f32 * min_col;
        let free_usable = usable - fixed_sum;
        let mut changed = false;
        for (i, w) in natural.iter().enumerate() {
            if !fixed[i] && free_usable * w / free_total < min_col {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let fixed_sum = fixed.iter().filter(|f| **f).count() as f32 * min_col;
    let free_usable = (usable - fixed_sum).max(0.0);
    let free_total: f32 = natural
        .iter()
        .enumerate()
        .filter(|(i, _)| !fixed[*i])
        .map(|(_, w)| *w)
        .sum();
    let mut cols: Vec<f32> = natural
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if fixed[i] || free_total <= 0.0 {
                min_col
            } else {
                // `min_col.min(*w)`: `f32::clamp` PANICS when the lower
                // bound is above the upper one (a column measured narrower
                // than the floor must never take the UI thread down).
                (free_usable * w / free_total).clamp(min_col.min(*w), *w)
            }
        })
        .collect();
    settle_rounding(&mut cols, usable, min_col);
    cols
}

/// Removes a floating-point residual from the widest column, so a FITTING
/// layout never sums past `usable` by a rounding ulp.
///
/// The caller (`flush_table`) reads `sum > usable` as "even the floors do
/// not fit" and re-lays every column out for horizontal scrolling, capped
/// at `scroll_max`; without this guard a fitting two-column table
/// (`86.25 + 340` grown to fill 757 summed to `757.00006 > 757`) collapsed
/// to `[86.25, 128]` — the second column wrapped every couple of words
/// while the card stayed full width.
///
/// A REAL overflow — even the `min_col` floors do not fit — is left alone:
/// there the widest column already sits at its floor and `take` is 0, so
/// the caller still scrolls.
fn settle_rounding(cols: &mut [f32], usable: f32, min_col: f32) {
    let over = cols.iter().sum::<f32>() - usable;
    if over <= 0.0 {
        return;
    }
    if let Some(widest) = cols
        .iter_mut()
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    {
        let take = over.min((*widest - min_col).max(0.0));
        *widest -= take;
    }
}

/// Paints the collapsed interior cell grid: 1px separators BETWEEN
/// rows and columns, at the midpoints of the Grid spacing. The rects come
/// from the same render pass that drew the cells, so wrapped/multi-line
/// rows get their real heights; the card frame remains the outer border.
fn paint_table_grid(ui: &egui::Ui, rows: &[Vec<egui::Rect>]) {
    // Union rect per rendered row (a ragged/empty row is skipped).
    let bands: Vec<egui::Rect> = rows
        .iter()
        .filter_map(|cells| {
            let mut rect = egui::Rect::NOTHING;
            for c in cells {
                rect = rect.union(*c);
            }
            rect.is_positive().then_some(rect)
        })
        .collect();
    let (Some(first), Some(last)) = (bands.first(), bands.last()) else {
        return;
    };
    // Full table width: ragged rows are padded, so span the union of bands.
    let left = bands
        .iter()
        .map(egui::Rect::left)
        .fold(f32::INFINITY, f32::min);
    let right = bands
        .iter()
        .map(egui::Rect::right)
        .fold(f32::NEG_INFINITY, f32::max);
    let stroke = egui::Stroke::new(
        theme::stroke::HAIRLINE,
        theme::scheme_ref(ui).outline_variant,
    );
    let painter = ui.painter();
    // Row separators: midpoint between consecutive row bands.
    for pair in bands.windows(2) {
        let y = (pair[0].bottom() + pair[1].top()) / 2.0;
        painter.hline(left..=right, y, stroke);
    }
    // Column separators: from the row that declares the most cells, so a
    // ragged first row cannot hide a boundary; midpoints between them.
    let Some(widest) = rows.iter().max_by_key(|cells| cells.len()) else {
        return;
    };
    for pair in widest.windows(2) {
        let x = (pair[0].right() + pair[1].left()) / 2.0;
        painter.vline(x, first.top()..=last.bottom(), stroke);
    }
}

/// ONE invisible layout pass over every cell line (see [`flush_table`]).
///
/// Lines are measured at `max_col - 2*CELL_PAD_X` (the content area inside
/// the cell padding), so a column is as wide as its content plus padding
/// (or `max_col` when the content wraps). The measurement goes through the
/// same `render_frag` path as the real pass, so text, links, emoji, code
/// pills and images (whose size comes from the per-document cache) measure
/// exactly what they draw.
#[allow(clippy::too_many_arguments)]
pub(super) fn measure_table(
    ui: &mut egui::Ui,
    tb: &TableCtx<'_>,
    ncols: usize,
    min_col: f32,
    max_col: f32,
    selected: bool,
    interactive: bool,
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
) -> TableWidths {
    // Header first, then body: the same order `flush_table` renders.
    let mut order: Vec<&[Vec<Frag<'_>>]> = Vec::with_capacity(1 + tb.rows.len());
    if let Some(h) = tb.headers.as_ref() {
        order.push(h);
    }
    for row in &tb.rows {
        order.push(row);
    }
    let content_max = (max_col - 2.0 * CELL_PAD_X).max(1.0);
    let mut rows: Vec<Vec<Vec<f32>>> = Vec::with_capacity(order.len());
    let mut cols = vec![0.0f32; ncols];
    for row in order {
        let mut row_widths: Vec<Vec<f32>> = Vec::with_capacity(row.len());
        for (col, cell) in row.iter().enumerate() {
            let mut line_widths: Vec<f32> = Vec::new();
            for line in visual_lines(cell) {
                let w = if line.is_empty() {
                    0.0
                } else {
                    measure_line_width(ui, line, content_max, selected, interactive, images, emoji)
                };
                line_widths.push(w);
                if let Some(slot) = cols.get_mut(col) {
                    *slot = slot.max(w);
                }
            }
            row_widths.push(line_widths);
        }
        rows.push(row_widths);
    }
    // Measured content + the cell's own padding; the clamp keeps tiny cells
    // legible and long prose wrapping.
    for w in &mut cols {
        *w = (*w + 2.0 * CELL_PAD_X).clamp(min_col, max_col);
    }
    TableWidths { rows, cols }
}

/// Renders one row: its cells at the measured column widths, then padding
/// and `end_row`. Shared by the header and body rows. Returns the union of
/// the rendered cell rects (used to paint the interior grid).
#[allow(clippy::too_many_arguments)]
fn render_table_row(
    ui: &mut egui::Ui,
    row: &[Vec<Frag<'_>>],
    row_widths: Option<&[Vec<f32>]>,
    ncols: usize,
    aligns: &[Alignment],
    cols: &[f32],
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
) -> Vec<egui::Rect> {
    let mut rects: Vec<egui::Rect> = Vec::with_capacity(row.len());
    for (col, cell) in row.iter().enumerate() {
        rects.push(render_table_cell(
            ui,
            cell,
            col,
            aligns,
            cols.get(col).copied().unwrap_or(0.0),
            row_widths.and_then(|r| r.get(col)).map(Vec::as_slice),
            selected,
            interactive,
            on_link,
            images,
            emoji,
            links,
        ));
    }
    // Pad ragged rows so Grid keeps N columns.
    for _ in row.len()..ncols {
        ui.label("");
    }
    ui.end_row();
    rects
}

/// One table cell, rendered at its column's measured width.
///
/// Returns the scope's rect (the full column at the cell's real height), so
/// `flush_table` can paint the interior grid from real geometry.
///
/// The scope is the widget the `Grid` measures, so its width IS the column
/// width. Lines then align against that real width; using the Grid's
/// `available_rect` would align them against `max_col_width` and report the
/// cap back as the column width.
///
/// All flows stay left-to-right regardless of the alignment: egui's
/// `horizontal_wrapped` flips to right-to-left inside a right-aligned scope,
/// which used to reverse the visual order of a multi-run line (`**a** b`).
/// Header cells arrive already marked `strong` (baked once when the table is
/// built), so no per-frame clone is needed to style them.
#[allow(clippy::too_many_arguments)]
fn render_table_cell(
    ui: &mut egui::Ui,
    cell: &[Frag<'_>],
    col: usize,
    aligns: &[Alignment],
    col_w: f32,
    line_widths: Option<&[f32]>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
) -> egui::Rect {
    let align = match aligns.get(col) {
        Some(Alignment::Center) => egui::Align::Center,
        Some(Alignment::Right) => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let col_w = col_w.max(1.0);
    // Content area inside the cell padding: lines wrap and align against
    // this width, so the text never touches the border.
    let content_w = (col_w - 2.0 * CELL_PAD_X).max(1.0);
    let mut rect = ui.available_rect_before_wrap();
    rect.max.x = rect.min.x + col_w;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            // Reserve the full column: the Grid sizes a column from the max
            // widget width, so a short cell must not shrink its column.
            ui.set_min_width(col_w);
            // Vertical padding as REAL allocations: the returned cell rect
            // (used for the interior grid) must include it.
            let _ = ui.allocate_exact_size(egui::vec2(1.0, CELL_PAD_Y), egui::Sense::hover());
            // Inset content area; the line's own alignment happens inside.
            let mut content = ui.available_rect_before_wrap();
            content.min.x += CELL_PAD_X;
            content.max.x -= CELL_PAD_X;
            content.max.y = content.min.y;
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(content)
                    .layout(egui::Layout::top_down(align)),
                |ui| {
                    for (line_idx, line) in visual_lines(cell).into_iter().enumerate() {
                        if line.is_empty() {
                            continue;
                        }
                        let line_w = line_widths
                            .and_then(|ws| ws.get(line_idx))
                            .copied()
                            .unwrap_or(0.0);
                        render_table_line(
                            ui,
                            line,
                            align,
                            content_w,
                            line_w,
                            selected,
                            interactive,
                            on_link,
                            images,
                            emoji,
                            links,
                        );
                    }
                },
            );
            let _ = ui.allocate_exact_size(egui::vec2(1.0, CELL_PAD_Y), egui::Sense::hover());
        },
    )
    .response
    .rect
}

/// Renders one visual line of a table cell (see [`render_table_cell`]).
#[allow(clippy::too_many_arguments)]
fn render_table_line(
    ui: &mut egui::Ui,
    line: &[Frag<'_>],
    align: egui::Align,
    col_w: f32,
    line_w: f32,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
) {
    let avail = ui.available_width().max(1.0).min(col_w).max(1.0);
    let height = ui.spacing().interact_size.y;
    let width = if align == egui::Align::Min {
        avail
    } else {
        // `+ 0.5`: the line renders at this exact width, so glyph rounding
        // can never push the last run into a phantom wrap.
        (line_w + 0.5).min(avail).max(1.0)
    };
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for f in line {
                render_frag(ui, f, selected, interactive, on_link, images, emoji, links);
            }
        },
    );
}

/// Natural (wrapped-to-`avail`) width of one cell line, in px.
///
/// The line is laid out in a throwaway sizing-pass child: invisible,
/// non-interactive and outside the cell flow, so probing cannot paint,
/// navigate or shift the layout. The real pass reuses egui's glyph layout
/// cache, so the same text is not shaped twice.
fn measure_line_width(
    ui: &mut egui::Ui,
    line: &[Frag<'_>],
    avail: f32,
    selected: bool,
    interactive: bool,
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
) -> f32 {
    let cell = ui.available_rect_before_wrap();
    let mut probe = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cell)
            .sizing_pass()
            .invisible(),
    );
    // Probe-only sinks: the invisible pass must not register link hitboxes
    // (they would double the right-click menu) nor navigate anywhere.
    let mut ignored_links: Vec<LinkHit> = Vec::new();
    let mut ignored_nav = |_: &str| {};
    let inner = probe.allocate_ui_with_layout(
        egui::vec2(avail.max(1.0), ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for f in line {
                render_frag(
                    ui,
                    f,
                    selected,
                    interactive,
                    &mut ignored_nav,
                    images,
                    emoji,
                    &mut ignored_links,
                );
            }
        },
    );
    inner.response.rect.width()
}
