// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::headings`: heading rendering (sized by level,
//! with anchor ids and the inline-image rule).

use super::*;

/// Renders a heading sized by level with extra air.
///
/// Returns the rect the heading's own content painted (text and the H1/H2
/// rule, item indentation included): the outline scrollspy records it by the
/// heading's `Start` event index. The air above (`spacing::XL`) is NOT part
/// of it, so a heading scrolled exactly to the viewport top still reads as
/// the current section.
pub(super) fn render_heading(
    ui: &mut egui::Ui,
    mut frags: Vec<Frag<'_>>,
    level: HeadingLevel,
    anchor_id: Option<&str>,
    ctx: &mut RenderCtx,
) -> egui::Rect {
    // A hostile ATX heading can carry megabytes of text: bound the label.
    frags = cap_block_text(frags);
    // Heading scale: 2em / 1.5em / 1.25em / 1em / .875em / .85em of the
    // 16px body size (`TextStyle::Body` in `ui::theme`).
    // The cap bounds the BASE size only: the toolbar zoom is applied by egui
    // at paint time (it multiplies every size equally, this cap included).
    let size = match level {
        HeadingLevel::H1 => 32.0 * ctx.type_scale,
        HeadingLevel::H2 => 24.0 * ctx.type_scale,
        HeadingLevel::H3 => 20.0 * ctx.type_scale,
        HeadingLevel::H4 => 16.0 * ctx.type_scale,
        HeadingLevel::H5 => 14.0 * ctx.type_scale,
        HeadingLevel::H6 => 13.6 * ctx.type_scale,
    }
    .min(40.0);
    // Heading margins: one `XL` step of air BEFORE every heading (plus the
    // 6px before an H1/H2 rule) and 8px after it, then the normal block
    // spacing. The old H1/H2 literals (20/18px) drifted from the spacing
    // scale; `XL` (16px) keeps every heading on it.
    ui.add_space(theme::spacing::XL);
    // Anchor click / cross-page fragment: scroll this heading to the top.
    // The fragment may be percent-encoded (`#caf%C3%A9`) or mixed case.
    if let (Some(id), Some(pending)) = (anchor_id, ctx.pending_anchor.as_deref()) {
        if !id.is_empty() && crate::markdown::anchors::fragment_matches(id, pending) {
            scroll_to_here(ui);
            *ctx.pending_anchor = None;
        }
    }
    if ctx.quote > 0 {
        if let Some(label) = detect_callout(&frags) {
            frags = strip_callout(&frags);
            render_callout_badge(ui, label, ctx.selected);
        }
    }
    // H6 renders muted; every other level uses the body color. Select-all
    // forces the high-contrast foreground (matches `theme::selected_text`):
    // muted H6 on the pale selection bg is unreadable otherwise.
    // One palette lookup per heading, not one per branch.
    let scheme = theme::scheme_ref(ui);
    let title_color = if ctx.selected || !matches!(level, HeadingLevel::H6) {
        scheme.on_surface
    } else {
        scheme.on_surface_variant
    };
    let pad = begin_item_block(ui, ctx);
    let lines = visual_lines(&frags);
    let h_rule = matches!(level, HeadingLevel::H1 | HeadingLevel::H2);
    // The heading's own content top, measured BEFORE layout: `render_indented`
    // runs the content directly on this `Ui` when there is no item
    // indentation, and `Ui::min_rect` would then start at the previous
    // block's top instead of the heading's.
    let top = ui.cursor().top();
    render_indented(ui, pad, |ui| {
        for line in lines {
            if line.is_empty() {
                ui.add_space(theme::spacing::XS);
                continue;
            }
            inline_flow(ui, line, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for f in line {
                    render_heading_frag(ui, f, size, title_color, ctx);
                }
            });
        }
        // Inside the indent so a heading in a list item keeps the rule
        // aligned with its text instead of spanning from the item edge.
        // `padding-bottom: .3em` equivalent: a little air first.
        if h_rule {
            ui.add_space(theme::spacing::S);
            ui.separator();
        }
    });
    // Cursor after the content (rule included), before the trailing
    // `spacing::M` air.
    let bottom = ui.cursor().top();
    ui.add_space(theme::spacing::M);
    let left = ui.cursor().left();
    egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left, bottom.max(top)))
}

/// Precise find split for a plain heading fragment (at the heading size).
/// Returns true when painted (at least one valid match claimed).
fn render_heading_split(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    text: &str,
    size: f32,
    title_color: egui::Color32,
    selected: bool,
    fr: &mut crate::viewer::find::FindRender,
) -> bool {
    // Pre-gate like the prose path: skips the lowercase + `Vec` for a
    // non-matching heading.
    if !crate::viewer::find::contains_match(text, &fr.query_lower) {
        return false;
    }
    let (ranges, fragment_capped) = crate::viewer::find::find_ranges_capped(text, &fr.query_lower);
    if ranges.is_empty() {
        return false;
    }
    if !crate::viewer::find::ranges_resolve(text, &ranges, &fr.query_lower) {
        return false;
    }
    // Color emoji: the emoji path paints the textures with the wash behind
    // them (a galley cannot draw a texture), same as body text.
    if crate::media::emoji::may_contain_emoji(text) {
        return false;
    }
    // Select-all washes the whole page: the caller's fallback paints the
    // heading uniformly (see `render_plain_split`).
    if selected {
        return false;
    }
    let Some((start, claimed)) =
        crate::viewer::inline::claim_fragment(fr, ranges.len(), fragment_capped)
    else {
        return false;
    };
    // ONE unsplit galley with the washes painted behind the glyphs (see
    // `paint_find_wash`): separate widgets or per-match sections both moved
    // the text.
    crate::viewer::inline::paint_find_wash(
        ui,
        heading_rt(f, text, size, title_color),
        text,
        &ranges,
        start,
        claimed,
        fr,
    );
    true
}

/// The heading's own inline styling: strong at the heading size, in
/// `color`, with the monospace/emphasis/strike the fragment carries. Shared
/// by every heading path (find split, emoji, links, plain runs) so a searched
/// or selected heading lays out exactly like an untouched one.
fn heading_rt(f: &Frag<'_>, text: &str, size: f32, color: egui::Color32) -> egui::RichText {
    let mut rt = theme::strong(egui::RichText::new(text).size(size)).color(color);
    if f.code {
        rt = rt.monospace();
    }
    if f.em {
        rt = rt.italics();
    }
    if f.strike {
        rt = rt.strikethrough();
    }
    rt
}

/// One heading fragment: `title_color`, strong, links accent-blue (inert
/// when not interactive), select-all forces readable colors.
pub(super) fn render_heading_frag(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    size: f32,
    title_color: egui::Color32,
    ctx: &mut RenderCtx,
) {
    // Images in headings (`# ![Logo](logo.png) Title`) render like any
    // other inline image: the old code fell through to the alt text and
    // the image was never even registered for fetching.
    if let Some(img) = &f.image {
        crate::viewer::image::render_inline_image(ui, img, &f.text, f.link.as_ref(), ctx.img_ctx());
        return;
    }
    let scheme = theme::scheme_ref(ui);
    let link_color = if ctx.selected {
        scheme.on_surface
    } else {
        scheme.primary
    };
    // A footnote reference is a raised marker PROPORTIONAL to the heading
    // size (the body-sized marker a heading would otherwise get looks lost
    // in a 32px H1).
    if f.sup {
        crate::viewer::inline::render_superscript_ref(
            ui,
            f,
            size,
            ctx.selected,
            ctx.interactive,
            ctx.on_link,
        );
        return;
    }
    if let Some(badge) = f.badge {
        // A badge in a heading (`# Status <Badge type="tip">New</Badge>`)
        // draws the same pill as in a paragraph, scaled with the heading:
        // it used to flatten into plain heading text.
        crate::viewer::inline::render_badge(ui, f.text.as_ref(), badge, ctx.selected, size);
        return;
    }
    // RTL display order (epaint has no bidi), same preparation as
    // `render_frag`: code spans keep the literal source. Computed only for
    // text fragments, after the image/superscript/badge early returns.
    let display = crate::viewer::bidi::visual_text(f.text.as_ref());
    let text = if f.code {
        f.text.as_ref()
    } else {
        display.as_ref()
    };
    let sel_bg = ctx.selected.then(|| theme::select_bg(ui));
    // In-page find: plain heading fragments get the same precise split as
    // body text (at the heading size); links/code/RTL fall back to a
    // whole-fragment highlight below.
    if let Some(fr) = ctx.find.as_mut() {
        if !text.is_empty()
            && f.link.is_none()
            && !f.code
            && !crate::viewer::bidi::contains_rtl(text)
            && render_heading_split(ui, f, text, size, title_color, ctx.selected, fr)
        {
            return;
        }
    }
    let (find_hl, find_scroll): (Option<bool>, bool) = match ctx.find.as_mut() {
        Some(fr) if !text.is_empty() => {
            // Reached for links, code, RTL runs and emoji headings (the wash
            // goes behind their textures) or when the precise split bailed;
            // a successful split returns above, so nothing is claimed twice.
            // Pre-gate like the prose path.
            if !crate::viewer::find::contains_match(text, &fr.query_lower) {
                (None, false)
            } else {
                let (ranges, fragment_capped) =
                    crate::viewer::find::find_ranges_capped(text, &fr.query_lower);
                let n = ranges.len();
                if n == 0 {
                    (None, false)
                } else {
                    let start = fr.claim(n);
                    let claimed = fr.next_ordinal - start;
                    if fragment_capped && claimed == n {
                        fr.capped = true;
                    }
                    if claimed == 0 {
                        (None, false)
                    } else {
                        (
                            Some((start..start + claimed).contains(&fr.current)),
                            fr.scroll,
                        )
                    }
                }
            }
        }
        _ => (None, false),
    };
    let scroll_to = |ui: &egui::Ui, rect: egui::Rect| {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    };
    // Color emoji for plain heading text; links and code keep the text path.
    // The scalar pre-check avoids the grapheme split for emoji-free text.
    // An emoji heading takes the whole-fragment wash here (color preserved),
    // never the precise split: a galley cannot draw a texture.
    if f.link.is_none() && !f.code && crate::media::emoji::may_contain_emoji(text) {
        let emoji_bg = find_hl
            .map(|current| {
                if current {
                    theme::find_current_bg(ui)
                } else {
                    theme::find_match_bg(ui)
                }
            })
            .or(sel_bg);
        let rendered = render_emoji_text(ui, text, size, ctx.emoji, emoji_bg, &|ui, piece| {
            let mut rt = heading_rt(f, piece, size, title_color);
            if let Some(current) = find_hl {
                rt = theme::find_text(ui, rt, current);
            } else if let Some(bg) = sel_bg {
                rt = rt.background_color(bg);
            }
            rt
        });
        if let Some(rect) = rendered {
            if find_hl == Some(true) {
                if let Some(fr) = ctx.find.as_mut() {
                    fr.current_rect = Some(rect);
                }
                if find_scroll {
                    scroll_to(ui, rect);
                }
            }
            return;
        }
    }
    // Interactive link: accent, real `ui.link` (hand cursor, click).
    if let Some(dest) = f.link.as_ref().filter(|d| !d.is_empty() && ctx.interactive) {
        let mut rt = heading_rt(f, text, size, link_color);
        if let Some(current) = find_hl {
            rt = theme::find_text(ui, rt, current);
        } else if let Some(bg) = sel_bg {
            rt = rt.background_color(bg);
        }
        let resp = ui.link(rt);
        if find_hl == Some(true) && find_scroll {
            scroll_to(ui, resp.rect);
        }
        // In-document anchors are jump targets, never new tabs.
        if !dest.starts_with('#') {
            push_link_hit(ctx.links, resp.rect, dest);
        }
        if resp.clicked() {
            (ctx.on_link)(dest);
        }
        return;
    }
    // Everything else is inert text: a backdrop link stays accent-blue, an
    // ignored destination (`[x]()`) and plain/code runs take the heading
    // color (the ignored-link run used to drop it, so an H6 or a quoted
    // heading lost its tone there).
    let color = match f.link.as_deref() {
        Some(dest) if !dest.is_empty() => link_color,
        _ => title_color,
    };
    let rt = heading_rt(f, text, size, color);
    if let Some(current) = find_hl {
        let resp = body_text(ui, theme::find_text(ui, rt, current));
        if current && find_scroll {
            scroll_to(ui, resp.rect);
        }
    } else {
        body_text(
            ui,
            match sel_bg {
                Some(bg) => rt.background_color(bg),
                None => rt,
            },
        );
    }
}
