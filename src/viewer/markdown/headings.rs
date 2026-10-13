// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::headings`: heading rendering (sized by level,
//! with anchor ids and the inline-image rule).

use super::*;

/// Renders a heading sized by level with extra air.
pub(super) fn render_heading(
    ui: &mut egui::Ui,
    mut frags: Vec<Frag<'_>>,
    level: HeadingLevel,
    anchor_id: Option<&str>,
    ctx: &mut RenderCtx,
) {
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
    // Heading margins: extra air BEFORE the heading (20/18/16px by level,
    // plus the 6px before an H1/H2 rule) and 8px after it, then the normal
    // block spacing.
    match level {
        HeadingLevel::H1 => ui.add_space(20.0),
        HeadingLevel::H2 => ui.add_space(18.0),
        _ => ui.add_space(theme::spacing::XL),
    }
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
    ui.add_space(theme::spacing::M);
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
    // RTL display order (epaint has no bidi), same preparation as
    // `render_frag`: code spans keep the literal source.
    let display = crate::viewer::bidi::visual_text(f.text.as_ref());
    let text = if f.code {
        f.text.as_ref()
    } else {
        display.as_ref()
    };
    // Images in headings (`# ![Logo](logo.png) Title`) render like any
    // other inline image: the old code fell through to the alt text and
    // the image was never even registered for fetching.
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
    let sel_bg = ctx.selected.then(|| theme::select_bg(ui));
    // Color emoji for plain heading text; links and code keep the text path.
    // The scalar pre-check avoids the grapheme split for emoji-free text.
    if f.link.is_none() && !f.code && crate::media::emoji::may_contain_emoji(text) {
        let rendered = render_emoji_text(ui, text, size, ctx.emoji, ctx.selected, &|_ui, piece| {
            let mut rt = theme::strong(egui::RichText::new(piece).size(size)).color(title_color);
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            if let Some(bg) = sel_bg {
                rt = rt.background_color(bg);
            }
            rt
        });
        if rendered {
            return;
        }
    }
    if let Some(dest) = f.link.as_ref() {
        if dest.is_empty() {
            // Ignored destination: plain text, no hand, no nav.
            let mut rt = theme::strong(egui::RichText::new(text).size(size));
            if f.code {
                rt = rt.monospace();
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            if let Some(bg) = sel_bg {
                rt = rt.background_color(bg).color(title_color);
            }
            body_text(ui, rt);
        } else if !ctx.interactive {
            // Backdrop: blue like a link, but inert.
            let mut rt = theme::strong(egui::RichText::new(text).size(size)).color(link_color);
            if f.code {
                rt = rt.monospace();
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            if let Some(bg) = sel_bg {
                rt = rt.background_color(bg);
            }
            body_text(ui, rt);
        } else {
            let mut rt = theme::strong(egui::RichText::new(text).size(size)).color(link_color);
            if f.code {
                rt = rt.monospace();
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            if let Some(bg) = sel_bg {
                rt = rt.background_color(bg);
            }
            let resp = ui.link(rt);
            // In-document anchors are jump targets, never new tabs.
            if !dest.starts_with('#') {
                push_link_hit(ctx.links, resp.rect, dest);
            }
            if resp.clicked() {
                (ctx.on_link)(dest);
            }
        }
    } else if f.code {
        let mut rt = theme::strong(egui::RichText::new(text).size(size))
            .monospace()
            .color(title_color);
        if let Some(bg) = sel_bg {
            rt = rt.background_color(bg);
        }
        body_text(ui, rt);
    } else {
        let mut rt = theme::strong(egui::RichText::new(text).size(size)).color(title_color);
        if f.em {
            rt = rt.italics();
        }
        if f.strike {
            rt = rt.strikethrough();
        }
        if let Some(bg) = sel_bg {
            rt = rt.background_color(bg);
        }
        body_text(ui, rt);
    }
}
