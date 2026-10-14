// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::components`: native MDX component blocks.
//!
//! `markdown::document::normalize_markup` leaves canonical markers in the
//! event stream (`<mdowser-cmp data="…" />`, one HTML block each) around the
//! content of structural components. This module draws them:
//!
//! - `<Tabs>`/`<Tab>`: a clickable tab bar; only the selected tab's content
//!   is laid out. Selection lives in egui's temp store, keyed by the document
//!   salt, the marker's event index and the tab labels, so it survives
//!   scroll/zoom, stays per-tab and never leaks between two sets that share
//!   the same labels.
//! - `<Accordion>`: a collapsible section (`▸`/`▾` header); open state in the
//!   same temp store.
//! - `<Card>`: a bordered box (title emitted as its first content line by the
//!   normalizer, so links stay real links).
//! - `<Steps>`/`<Step>`: numbered rows.
//!
//! `Copy` walks every event, so unselected tabs and collapsed sections still
//! reach the clipboard (nothing the author wrote is hidden from Copy).

use super::*;

/// Padding inside a card box.
const CARD_PAD: i8 = 10;
/// Diameter of a step's number badge.
const STEP_BADGE: f32 = 24.0;

/// Reads the marker fields of the HTML block at `j`, with the index just past
/// the block. `None` when the block is not a component marker.
pub(super) fn marker_at(events: &[Event<'static>], j: usize) -> Option<(Vec<String>, usize)> {
    if !matches!(events.get(j), Some(Event::Start(Tag::HtmlBlock))) {
        return None;
    }
    let mut buf = String::new();
    let mut k = j + 1;
    while k < events.len() {
        match &events[k] {
            Event::End(TagEnd::HtmlBlock) => {
                k += 1;
                break;
            }
            Event::Html(h) | Event::Text(h) => buf.push_str(h),
            _ => {}
        }
        k += 1;
    }
    let fields = crate::markdown::document::component_fields(&buf)?;
    Some((fields, k))
}

/// State key of one `<Tabs>` instance: document salt + the marker's own
/// event index + the tab labels. The event index keeps two tab sets with
/// identical labels (e.g. two `<Tabs items={[…​]}>` lists on one page, or
/// two label-less sets that both fall back to `Tab 1…N`) from sharing one
/// selection; the labels keep the key content-sensitive, so a document edit
/// that changes them starts a fresh selection instead of applying the old
/// one to different tabs. Shared with the tests, which select a tab by
/// writing the state directly.
pub(super) fn tabs_state_id(doc_salt: u64, at: usize, labels: &[String]) -> egui::Id {
    let key = labels.join("\u{1f}");
    egui::Id::new((doc_salt, "mdx-tabs", at, crate::media::fnv1a(&key)))
}

/// State key of one `<Accordion>` instance: document salt + the marker's own
/// event index + the title. The event index keeps two sections with the same
/// title (or two untitled ones, which both fall back to `Details`) from
/// sharing one open/closed state — the same rule as [`tabs_state_id`].
pub(super) fn section_state_id(doc_salt: u64, at: usize, label: &str) -> egui::Id {
    egui::Id::new((doc_salt, "mdx-sec", at, crate::media::fnv1a(label)))
}

fn get_usize(ui: &egui::Ui, id: egui::Id) -> usize {
    ui.ctx()
        .data_mut(|d| *d.get_temp_mut_or_insert_with(id, || 0usize))
}

fn get_bool(ui: &egui::Ui, id: egui::Id) -> bool {
    ui.ctx()
        .data_mut(|d| *d.get_temp_mut_or_insert_with(id, || false))
}

/// Renders the content events in `[start, stop)` as blocks.
fn render_range(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    start: usize,
    stop: usize,
    ctx: &mut RenderCtx,
) {
    let mut k = start;
    while k < stop && k < events.len() {
        render_block(ui, events, &mut k, ctx);
    }
}

/// True when a decoded marker field is the close tag for `kind` (`/kind`).
fn is_close_tag(tag: &str, kind: &str) -> bool {
    // Allocation-free `format!("/{kind}")` comparison: the decoded field is
    // `/kind` exactly. Keeps the per-walk scan off the allocator.
    tag.len() == kind.len() + 1 && tag.as_bytes().first() == Some(&b'/') && &tag[1..] == kind
}

/// Finds the close marker of `kind` starting at `start`, returning
/// `(close_index, after_close)` or `None` when there is none.
fn find_close(events: &[Event<'static>], start: usize, kind: &str) -> Option<(usize, usize)> {
    let mut depth = 1usize;
    let mut j = start;
    while j < events.len() {
        if let Some((fields, after)) = marker_at(events, j) {
            let tag = fields[0].as_str();
            if tag == kind {
                depth += 1;
            } else if is_close_tag(tag, kind) {
                depth -= 1;
                if depth == 0 {
                    return Some((j, after));
                }
            }
            j = after;
            continue;
        }
        j += 1;
    }
    None
}

/// Renders a component marker at the current walk position (the marker block
/// itself is already consumed by the caller; `*i` is past it). `at` is the
/// marker's own `Start(HtmlBlock)` event index: stable per document and
/// unique per component instance, used to key per-instance state.
pub(super) fn render_component(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    fields: &[String],
    at: usize,
) {
    match fields[0].as_str() {
        "tabs" => render_tabs(ui, events, i, ctx, at),
        "tab" => render_transparent(ui, events, i, ctx, "tab"),
        "sec" => render_section(ui, events, i, ctx, fields, at),
        "card" => render_card(ui, events, i, ctx),
        "steps" => render_steps(ui, events, i, ctx),
        "step" => render_transparent(ui, events, i, ctx, "step"),
        // Close markers are consumed by their parent; an orphan does nothing.
        _ => {}
    }
}

/// Renders the children of a container whose markers are otherwise unused
/// (a `Tab`/`Step` seen without its parent).
fn render_transparent(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    kind: &str,
) {
    if let Some((close, after)) = find_close(events, *i, kind) {
        render_range(ui, events, *i, close, ctx);
        *i = after;
    }
}

/// One tab label: text centered in a fixed-size cell. Returns the cell rect,
/// whether it is hovered, and whether it was clicked. Its geometry depends
/// only on the label, so hover/selection never shift the bar.
fn tab_label(
    ui: &mut egui::Ui,
    text: &str,
    selected: bool,
    max_w: f32,
) -> (egui::Rect, bool, bool) {
    let scheme = theme::scheme_ref(ui);
    let font = egui::TextStyle::Body.resolve(ui.style());
    // A very long label must not push the strip outside the sheet: measure a
    // capped spelling (the full text stays in the hover tooltip and Copy),
    // and fit it to `max_w` (the strip's per-cell share) when needed.
    let pad_x = theme::spacing::L;
    let text_w = (max_w - pad_x * 2.0).max(24.0);
    let mut shown = crate::util::truncate_chars(text, crate::config::MAX_COMPONENT_LABEL_CHARS);
    let mut galley =
        ui.painter()
            .layout_no_wrap(shown.as_ref().to_owned(), font.clone(), scheme.on_surface);
    if galley.size().x > text_w {
        let per_char = (galley.size().x / shown.chars().count().max(1) as f32).max(1.0);
        let cap = (text_w / per_char).floor().max(1.0) as usize;
        shown = crate::util::truncate_chars(text, cap);
        galley = ui
            .painter()
            .layout_no_wrap(shown.as_ref().to_owned(), font, scheme.on_surface);
    }
    let truncated = shown.len() < text.len();
    let desired = egui::vec2(galley.size().x + pad_x * 2.0, TAB_H);
    let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if truncated {
        resp.clone().on_hover_text(text);
    }
    let hovered = resp.hovered();
    // A screen reader must hear the tab (and whether it is selected): the
    // cells are hand-painted, so nothing is announced otherwise.
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, text.to_owned())
    });
    let ink = if selected {
        scheme.primary
    } else if hovered {
        scheme.on_surface
    } else {
        scheme.on_surface_variant
    };
    if resp.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::SMALL);
    }
    ui.painter().galley(
        egui::pos2(rect.min.x + pad_x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    (rect, hovered, resp.clicked())
}

/// Height of the tab strip cell (px).
const TAB_H: f32 = 30.0;

/// A full-width clickable header (accordion): fixed height, chevron + title.
/// Returns `true` when clicked.
fn section_header(ui: &mut egui::Ui, title: &str, open: bool) -> bool {
    let scheme = theme::scheme_ref(ui);
    let width = ui.available_width().max(1.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::SMALL),
            scheme.container_hover,
        );
    }
    let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
    let label = if title.is_empty() { "Details" } else { title };
    let text = format!("{arrow}  {label}");
    let shown = crate::util::truncate_chars(&text, crate::config::MAX_COMPONENT_LABEL_CHARS);
    let truncated = shown.len() < text.len();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(shown.into_owned(), font, scheme.on_surface);
    if truncated {
        resp.clone().on_hover_text(label);
    }
    // Hand-painted control: announce the header (with its expanded state) and
    // show keyboard focus like every other painted control in the app.
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, label.to_owned())
    });
    if resp.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::SMALL);
    }
    ui.painter().galley(
        egui::pos2(rect.min.x + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        scheme.on_surface,
    );
    resp.clicked()
}

/// `<Tabs>`/`<Tab>`: a clickable tab bar over the selected tab's content.
fn render_tabs(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    at: usize,
) {
    let Some(Scan {
        ranges: tabs,
        after: end,
        labels,
    }) = scan_pairs(events, *i, "tabs", "tab")
    else {
        return;
    };
    if tabs.is_empty() {
        *i = end;
        return;
    }
    let id = tabs_state_id(ctx.doc_salt, at, &labels);
    let mut sel = get_usize(ui, id).min(tabs.len() - 1);
    let scheme = theme::scheme_ref(ui);
    let full_w = ui.available_width().max(1.0);
    // Cap each cell at an equal share of the strip: a long label (or many
    // tabs) must elide inside the sheet, never push later tabs past its edge.
    let cell_cap = (full_w / labels.len().max(1) as f32).max(48.0);
    let mut rects: Vec<egui::Rect> = Vec::with_capacity(labels.len());
    let mut hovers: Vec<bool> = Vec::with_capacity(labels.len());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (idx, label) in labels.iter().enumerate() {
            let text = if label.is_empty() {
                format!("Tab {}", idx + 1)
            } else {
                label.clone()
            };
            let (rect, hovered, clicked) = tab_label(ui, &text, idx == sel, cell_cap);
            if clicked {
                sel = idx;
            }
            rects.push(rect);
            hovers.push(hovered);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, sel));
    // The strip's bottom border, full reading column, with the active tab's
    // underline sitting ON it (a browser-tab look, not buttons).
    let (line, _) = ui.allocate_exact_size(egui::vec2(full_w, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(line, egui::CornerRadius::ZERO, scheme.outline_variant);
    for (idx, rect) in rects.iter().enumerate() {
        if idx == sel {
            let u = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, line.max.y - 2.0),
                egui::pos2(rect.max.x, line.max.y),
            );
            ui.painter()
                .rect_filled(u, egui::CornerRadius::ZERO, scheme.primary);
        } else if hovers.get(idx).copied().unwrap_or(false) {
            let u = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, line.max.y - 1.0),
                egui::pos2(rect.max.x, line.max.y),
            );
            ui.painter()
                .rect_filled(u, egui::CornerRadius::ZERO, scheme.on_surface_variant);
        }
    }
    ui.add_space(theme::spacing::M);
    let (start, stop) = tabs[sel];
    render_range(ui, events, start, stop, ctx);
    *i = end;
}

/// `<Accordion>`: a collapsible section with its title as the header. `at`
/// is the marker's own event index (see [`section_state_id`]).
fn render_section(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    fields: &[String],
    at: usize,
) {
    let Some((close, after)) = find_close(events, *i, "sec") else {
        return;
    };
    let title = fields.get(1).cloned().unwrap_or_default();
    let label = if title.is_empty() {
        "Details".to_string()
    } else {
        title.clone()
    };
    let id = section_state_id(ctx.doc_salt, at, &label);
    let mut open = get_bool(ui, id);
    if section_header(ui, &label, open) {
        open = !open;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, open));
    if open {
        render_indented(ui, Some(ITEM_INDENT), |ui| {
            render_range(ui, events, *i, close, ctx);
        });
    }
    *i = after;
}

/// `<Card>`: a bordered box around its content.
fn render_card(ui: &mut egui::Ui, events: &[Event<'static>], i: &mut usize, ctx: &mut RenderCtx) {
    let Some((close, after)) = find_close(events, *i, "card") else {
        return;
    };
    let scheme = theme::scheme_ref(ui);
    egui::Frame::new()
        .fill(scheme.container_low)
        .stroke(egui::Stroke::new(
            theme::stroke::HAIRLINE,
            scheme.outline_variant,
        ))
        .corner_radius(egui::CornerRadius::same(theme::radius::CARD))
        .inner_margin(egui::Margin::same(CARD_PAD))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(1.0));
            render_range(ui, events, *i, close, ctx);
        });
    ui.add_space(2.0);
    *i = after;
}

/// `<Steps>`/`<Step>`: numbered rows.
fn render_steps(ui: &mut egui::Ui, events: &[Event<'static>], i: &mut usize, ctx: &mut RenderCtx) {
    let Some(Scan {
        ranges: steps,
        after: end,
        labels: titles,
    }) = scan_pairs(events, *i, "steps", "step")
    else {
        return;
    };
    if steps.is_empty() {
        *i = end;
        return;
    }
    let scheme = theme::scheme_ref(ui);
    for (n, (start, stop)) in steps.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(STEP_BADGE, STEP_BADGE), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), STEP_BADGE / 2.0, scheme.secondary_container);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                (n + 1).to_string(),
                egui::FontId::proportional(12.0),
                scheme.on_secondary_container,
            );
            ui.add_space(10.0);
            ui.vertical(|ui| {
                if let Some(title) = titles.get(n).filter(|t| !t.is_empty()) {
                    let shown = crate::util::truncate_chars(
                        title,
                        crate::config::MAX_COMPONENT_LABEL_CHARS,
                    );
                    let truncated = shown.len() < title.len();
                    let label = ui.label(egui::RichText::new(shown.into_owned()).strong());
                    if truncated {
                        label.on_hover_text(title.as_str());
                    }
                }
                render_range(ui, events, *start, *stop, ctx);
            });
        });
        ui.add_space(theme::spacing::XS);
    }
    *i = end;
}

/// A scanned container: each child's content range, the index after the
/// container's close marker, and each child's label.
struct Scan {
    ranges: Vec<(usize, usize)>,
    after: usize,
    labels: Vec<String>,
}

/// Scans a container (`tabs`/`steps`) into its child (`tab`/`step`) content
/// ranges plus each child's label, stopping after the container's close.
/// Nested same-name containers are skipped by depth.
fn scan_pairs(events: &[Event<'static>], start: usize, outer: &str, inner: &str) -> Option<Scan> {
    let close_outer = format!("/{outer}");
    let close_inner = format!("/{inner}");
    let mut depth = 1usize;
    let mut labels = Vec::new();
    let mut ranges = Vec::new();
    let mut open: Option<usize> = None;
    let mut j = start;
    while j < events.len() {
        if let Some((fields, after)) = marker_at(events, j) {
            match fields[0].as_str() {
                k if k == outer => depth += 1,
                k if k == close_outer => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = open.take() {
                            ranges.push((s, j));
                        }
                        return Some(Scan {
                            ranges,
                            after,
                            labels,
                        });
                    }
                }
                k if k == inner && depth == 1 => {
                    labels.push(fields.get(1).cloned().unwrap_or_default());
                    open = Some(after);
                }
                k if k == close_inner && depth == 1 => {
                    if let Some(s) = open.take() {
                        ranges.push((s, j));
                    }
                }
                _ => {}
            }
            j = after;
            continue;
        }
        j += 1;
    }
    None
}
