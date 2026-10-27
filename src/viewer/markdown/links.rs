// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::links`: link runs and callouts.
//!
//! Whole-paragraph link runs (site card lists), leaked skip links and
//! `[!NOTE]`-style callouts (badge color + stripped marker).

use super::*;

/// Returns the single image fragment when a paragraph holds exactly one
/// image and nothing else (whitespace-only text is ignored).
pub(super) fn image_only<'a, 'b>(frags: &'b [Frag<'a>]) -> Option<&'b Frag<'a>> {
    let mut found: Option<&'b Frag<'a>> = None;
    for f in frags {
        if f.image.is_some() {
            if found.is_some() {
                return None;
            }
            found = Some(f);
            continue;
        }
        if !f.text.trim().is_empty() {
            return None;
        }
    }
    found
}

/// Accessibility skip links leaked by `.md` variants ("Skip to content",
/// "Saltar al contenido", ...). Strict on purpose so a legitimate sentence
/// is never swallowed:
/// - Every non-empty fragment is a link to a fragment (`""` after rewrite
///   when the target is missing, or `#id` when it exists): the exporter
///   leaks the anchor, so plain text with the same words does not qualify.
/// - The whole-paragraph label (normalized, arrows trimmed) matches a known
///   accessibility phrase; a longer sentence containing the words never
///   disappears.
/// - Callers additionally require this to be the document's FIRST block.
pub(super) fn is_skip_link(frags: &[Frag<'_>]) -> bool {
    if frags.iter().any(|f| f.image.is_some()) {
        return false;
    }
    let mut saw_text = false;
    for f in frags {
        if f.text.trim().is_empty() {
            continue;
        }
        saw_text = true;
        let is_fragment_link = f
            .link
            .as_deref()
            .is_some_and(|l| l.is_empty() || l.starts_with('#'));
        if !is_fragment_link {
            return false;
        }
    }
    if !saw_text {
        return false;
    }
    // Reject oversized candidates BEFORE building the label: the first
    // block can be a single multi-MB link, and normalizing is several
    // full-size allocations + scans per frame for a paragraph the cap
    // discards anyway.
    let mut chars = 0usize;
    for f in frags {
        chars = chars.saturating_add(f.text.chars().count());
        if chars > crate::config::SKIP_LINK_MAX_CHARS {
            return false;
        }
    }
    let text: String = frags.iter().map(|f| f.text.as_ref()).collect();
    // Shared with `plain_text_normalized`, so the clipboard drops exactly
    // the block the viewer drops (see `markdown::document::is_skip_link_label`).
    crate::markdown::document::is_skip_link_label(&text)
}

/// A paragraph that is just link entries with body text (site card lists
/// flattened to links by the markdown exporter). Gated on length so short
/// inline runs ("Docs Chat Forum") keep their natural flow.
///
/// Images are allowed when they are INSIDE a link (card logos): a joined card
/// link is `[![logo] Vendor<h3>Name</h3>Description](url)`, and stacking it
/// one card per row is what makes the export readable. A bare image run
/// (`![](a)![](b)`) has no link and is still rejected, and at least TWO
/// distinct destinations are required so a single linked image with a long
/// caption keeps its normal (promoted) block layout.
pub(super) fn link_stack_candidate(frags: &[Frag<'_>]) -> bool {
    let mut first: Option<&str> = None;
    let mut two_links = false;
    let mut long_text = false;
    for f in frags {
        if f.image.is_none() && f.text.trim().is_empty() {
            continue;
        }
        let Some(link) = f.link.as_deref() else {
            return false;
        };
        if link.is_empty() {
            return false;
        }
        match first {
            None => first = Some(link),
            Some(f0) if f0 != link => two_links = true,
            _ => {}
        }
        if f.text.trim().chars().count() > crate::config::LINK_STACK_MIN_TEXT_CHARS {
            long_text = true;
        }
    }
    first.is_some() && two_links && long_text
}

/// Renders a link-only paragraph as stacked entries (one wrapped entry per
/// LINK) with `16px` block spacing between them.
///
/// A single link is often split into several frags by inline HTML
/// (`[WAF<h4>Title</h4>Body](url)`) and by inserted spaces; those frags
/// must stay on the same wrapped line, so entries are runs of frags that
/// share a destination. Whitespace-only text joins the current run; a
/// leading logo (`![](icon)`) carries the link and starts the run too.
pub(super) fn render_link_stack(ui: &mut egui::Ui, frags: &[Frag<'_>], ctx: &mut RenderCtx) {
    let pad = begin_item_block(ui, ctx);
    render_indented(ui, pad, |ui| {
        // One icon height for the whole stack: every card logo matches.
        let target = block_vector_target(ui, frags, ctx);
        let mut first = true;
        let mut i = 0usize;
        while i < frags.len() {
            // A leading link image has empty text: never skip it, or the
            // card would lose its logo.
            if frags[i].image.is_none() && frags[i].text.trim().is_empty() {
                i += 1;
                continue;
            }
            if !first {
                ui.add_space(theme::spacing::S);
            }
            first = false;
            let link = frags[i].link.clone();
            let start = i;
            let mut end = i + 1;
            while end < frags.len()
                && (frags[end].link == link
                    || (frags[end].image.is_none() && frags[end].text.trim().is_empty()))
            {
                end += 1;
            }
            i = end;
            inline_flow(ui, &frags[start..end], |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for f in &frags[start..end] {
                    match (target, f.image.as_ref()) {
                        (Some(height), Some(img)) => {
                            crate::viewer::image::render_inline_image_at_height(
                                ui,
                                img,
                                &f.text,
                                f.link.as_ref(),
                                ctx.img_ctx(),
                                height,
                            )
                        }
                        _ => render_frag(
                            ui,
                            f,
                            ctx.selected,
                            ctx.interactive,
                            ctx.on_link,
                            ctx.images,
                            ctx.emoji,
                            ctx.links,
                            &mut ctx.find,
                        ),
                    }
                }
            });
        }
    });
    ui.add_space(theme::spacing::M);
}

/// Callout markers (`> [!NOTE]`, `> [!WARNING]`...).
/// Returns the badge label when the first text starts with a marker.
pub(super) fn detect_callout(frags: &[Frag<'_>]) -> Option<&'static str> {
    let first = frags.iter().find(|f| !f.text.trim().is_empty())?;
    let t = first.text.trim_start();
    // Marker is usually its own frag (`[!NOTE]`) or a prefix (`[!NOTE] text`).
    // Only the longest marker's worth of bytes is scanned/copied: this runs
    // per quoted paragraph per frame, and the text can be up to
    // `MAX_RENDERED_BLOCK_CHARS`. The cut is moved back to a char boundary:
    // `str::get` returns `None` mid-character, and the old fallback uppercased
    // the WHOLE paragraph — e.g. `> ¿Qué tal?` (byte 12 splits `é`).
    let mut n = "[!IMPORTANT]".len().min(t.len());
    while n > 0 && !t.is_char_boundary(n) {
        n -= 1;
    }
    let upper = t[..n].to_ascii_uppercase();
    for (marker, label) in [
        ("[!NOTE]", "NOTE"),
        ("[!TIP]", "TIP"),
        ("[!IMPORTANT]", "IMPORTANT"),
        ("[!WARNING]", "WARNING"),
        ("[!CAUTION]", "CAUTION"),
    ] {
        if upper.starts_with(marker) {
            return Some(label);
        }
    }
    None
}

/// Looks ahead inside a blockquote (bounded scan) for an alert
/// marker, so the quote bar can take the alert color before the inner
/// paragraph renders its badge. `pulldown-cmark` splits `[!NOTE]` across
/// several `Text` events (`[`, `!NOTE`, `]`), so consecutive text is
/// concatenated before matching.
pub(super) fn detect_callout_ahead(
    events: &[Event<'static>],
    mut i: usize,
) -> Option<&'static str> {
    let end = i.saturating_add(64).min(events.len());
    const MARKERS: [(&str, &str); 5] = [
        ("[!NOTE]", "NOTE"),
        ("[!TIP]", "TIP"),
        ("[!IMPORTANT]", "IMPORTANT"),
        ("[!WARNING]", "WARNING"),
        ("[!CAUTION]", "CAUTION"),
    ];
    let mut probe = String::new();
    while i < end {
        match &events[i] {
            Event::End(TagEnd::BlockQuote(_) | TagEnd::Paragraph)
            | Event::SoftBreak
            | Event::HardBreak => return None,
            Event::Text(t) => {
                // A single `Text` event can be the whole 10 MiB body (a quote
                // holding one giant paragraph). Only the marker prefix
                // matters, so never copy/uppercase more than the longest
                // marker: appending the full text and then building its
                // uppercase copy used to allocate twice the body per frame,
                // defeating every render cap.
                const MARKER_LEN: usize = "[!IMPORTANT]".len();
                let remaining = MARKER_LEN.saturating_sub(probe.len());
                let cut = remaining.min(t.len());
                // The byte cap splitting `t` mid-character means no marker.
                probe.push_str(t.get(..cut)?);
                let upper = probe.trim_start().to_ascii_uppercase();
                for (marker, label) in MARKERS {
                    if upper.starts_with(marker) {
                        return Some(label);
                    }
                }
                // Longer than the longest marker without a match: this is
                // ordinary quoted prose, not an alert.
                if probe.len() >= MARKER_LEN {
                    return None;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Alert accent color for a callout label.
pub(super) fn callout_color(scheme: &theme::Scheme, label: &str) -> egui::Color32 {
    match label {
        "TIP" => scheme.alert_tip,
        "IMPORTANT" => scheme.alert_important,
        "WARNING" => scheme.alert_warning,
        "CAUTION" => scheme.alert_caution,
        _ => scheme.alert_note,
    }
}

/// Removes the `[!XXX]` prefix from the first non-empty frag (clone-on-write).
pub(super) fn strip_callout<'a>(frags: &[Frag<'a>]) -> Vec<Frag<'a>> {
    let mut out = frags.to_vec();
    for f in &mut out {
        if f.text.trim().is_empty() {
            continue;
        }
        let trimmed = f.text.trim_start();
        if let Some(pos) = trimmed.find(']') {
            let marker = &trimmed[..=pos];
            let upper = marker.to_ascii_uppercase();
            if [
                "[!NOTE]",
                "[!TIP]",
                "[!IMPORTANT]",
                "[!WARNING]",
                "[!CAUTION]",
            ]
            .contains(&upper.as_str())
            {
                let cut = f.text.len() - trimmed.len() + pos + 1;
                let rest = f.text[cut..].trim_start_matches([' ', '\n']).to_string();
                f.text = rest.into();
            }
        }
        break;
    }
    // Drop a now-empty leading frag so no blank gap remains.
    if out.first().is_some_and(|f| f.text.is_empty()) {
        out.remove(0);
    }
    out
}

/// Callout badge above the quoted content (alert accent).
pub(super) fn render_callout_badge(ui: &mut egui::Ui, label: &'static str, selected: bool) {
    let scheme = theme::scheme_ref(ui);
    let badge = callout_color(scheme, label);
    // Callout title: body size, medium weight, title
    // case ("Note", not "NOTE").
    let title = match label {
        "NOTE" => "Note",
        "TIP" => "Tip",
        "IMPORTANT" => "Important",
        "WARNING" => "Warning",
        _ => "Caution",
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut rt = theme::strong(egui::RichText::new(title)).color(badge);
        if selected {
            rt = theme::selected_text(ui, rt);
        }
        ui.label(rt);
    });
}
