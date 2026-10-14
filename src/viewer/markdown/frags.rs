// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::frags`: inline fragment collection.
//!
//! Walks the owned event stream into [`Frag`]s: styles, (absolutized)
//! links, images, coalesced text, block-text caps and the light/dark
//! theme-twin logo collapse. Code spans and images keep their own
//! event forms; everything else lands here.

use super::*;

/// Collects inline content until the next `End` event (left unconsumed).
///
/// - Recurses for emphasis / strong / strikethrough / links / images.
/// - Returns the fragments in order, with style flags and (non-empty)
///   absolute link destinations.
/// - `in_cell`: table cells render task markers as `[x]` text (no widget).
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_inline<'e>(
    events: &'e [Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    strong: bool,
    em: bool,
    strike: bool,
    code: bool,
    link: Option<&Arc<str>>,
    in_cell: bool,
) -> Vec<Frag<'e>> {
    let mut out: Vec<Frag<'e>> = Vec::new();
    while *i < events.len() {
        match &events[*i] {
            Event::End(_) => break,
            Event::Start(Tag::Emphasis) => {
                *i += 1;
                let inner = collect_inline_nested(
                    events, i, ctx, strong, true, strike, code, link, in_cell,
                );
                if *i < events.len() {
                    *i += 1;
                }
                out.extend(inner);
            }
            Event::Start(Tag::Strong) => {
                *i += 1;
                let inner =
                    collect_inline_nested(events, i, ctx, true, em, strike, code, link, in_cell);
                if *i < events.len() {
                    *i += 1;
                }
                out.extend(inner);
            }
            Event::Start(Tag::Strikethrough) => {
                *i += 1;
                let inner =
                    collect_inline_nested(events, i, ctx, strong, em, true, code, link, in_cell);
                if *i < events.len() {
                    *i += 1;
                }
                out.extend(inner);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                // Pre-resolved at load time (`DestCache`): an unknown anchor
                // or a non-navigable scheme arrives as the inert `""`. A
                // destination past the navigation cap was made inert there
                // too, so nothing is parsed per frame. The handle is shared:
                // every fragment of this link clones the `Arc`, never the
                // URL string.
                let abs = ctx.dests.link_rc(dest_url).unwrap_or_else(empty_link);
                *i += 1;
                let inner = collect_inline_nested(
                    events,
                    i,
                    ctx,
                    strong,
                    em,
                    strike,
                    code,
                    Some(&abs),
                    in_cell,
                );
                if *i < events.len() {
                    *i += 1;
                }
                out.extend(inner);
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                // Pre-resolved at load time: an over-long source and every
                // `#frag` (an image never navigates) arrive as `None`.
                let abs = ctx.dests.image_rc(dest_url);
                // Bound the tooltip at capture: the title arrives unbounded
                // from the parser and a multi-MB one used to be cloned per
                // frame before the display-side truncation.
                let title = truncate_chars(title, MAX_IMAGE_TITLE_CHARS).into_owned();
                *i += 1;
                let alt_frags =
                    collect_inline_nested(events, i, ctx, strong, em, strike, false, None, in_cell);
                if *i < events.len() {
                    *i += 1;
                }
                // Bound the alt while collecting (same reason): stop once the
                // display cap is reached instead of concatenating megabytes.
                // Each fragment is cut to the remaining char budget with the
                // shared ellipsis rule before appending, so one oversized
                // `Text` event (up to the whole 10 MiB body) is never copied
                // or rescanned per frame.
                let mut alt = String::new();
                for f in &alt_frags {
                    let room = MAX_IMAGE_ALT_CHARS.saturating_sub(alt.chars().count());
                    if room == 0 {
                        break;
                    }
                    let text = if f.text.as_ref() == "\n" {
                        " "
                    } else {
                        f.text.as_ref()
                    };
                    alt.push_str(&truncate_chars(text, room));
                }
                let alt = alt.trim().to_string();
                let Some(abs) = abs else {
                    // Unfetchable destination (non-https / inert): keep the
                    // textual placeholder; inside a link it stays clickable.
                    let placeholder = if alt.is_empty() {
                        "[image]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    };
                    out.push(make_frag(placeholder, strong, em, strike, false, link));
                    continue;
                };
                // Theme twins (`logo-light.svg` immediately followed by
                // `logo-dark.svg`) are ONE logo in two variants: the
                // site's CSS shows only the one matching the theme, so
                // collapse the pair. The dark twin is also never
                // registered for fetching.
                let prev_url = out
                    .last()
                    .and_then(|f| f.image.as_ref())
                    .map(|img| img.url.clone());
                let keep_prev = prev_url
                    .as_deref()
                    .and_then(|prev| theme_twin_choice(prev, &abs, ctx.dark));
                match keep_prev {
                    Some(true) => {} // keep the twin already collected
                    Some(false) => {
                        out.pop(); // the new variant wins: drop the twin
                        out.push(real_image_frag(alt, strong, em, strike, link, abs, title));
                    }
                    None => {
                        out.push(real_image_frag(alt, strong, em, strike, link, abs, title));
                    }
                }
            }
            Event::Text(t) => {
                out.push(make_frag(t.as_ref(), strong, em, strike, code, link));
                *i += 1;
            }
            Event::Code(c) => {
                out.push(make_frag(c.as_ref(), strong, em, strike, true, link));
                *i += 1;
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                if is_html_comment(h) {
                    // An HTML comment is invisible (browser semantics): it
                    // leaves NO fragment, not even a space, so
                    // `foo<!-- x -->bar` reads `foobar`.
                    *i += 1;
                    continue;
                }
                if let Some(ty) = crate::markdown::document::badge_open(h) {
                    *i += 1;
                    out.push(collect_badge(events, i, badge_kind(&ty)));
                    continue;
                }
                if crate::markdown::document::is_badge_close(h) {
                    // A stray close tag: nothing to draw.
                    *i += 1;
                    continue;
                }
                if is_html_break(h) {
                    out.push(newline_frag());
                } else {
                    out.push(space_frag());
                }
                *i += 1;
            }
            Event::SoftBreak => {
                out.push(space_frag());
                *i += 1;
            }
            Event::HardBreak => {
                out.push(newline_frag());
                *i += 1;
            }
            Event::FootnoteReference(label) => {
                // Raised small marker showing the reference NUMBER (a
                // superscript), clickable when the definition exists.
                // `footnote_id` returns the RESERVED anchor (`fn-<label>` or
                // its deduped form), so a colliding heading id never hijacks
                // the jump; `footnote_number` is the 1-based ordinal in
                // first-reference order.
                let label =
                    crate::util::truncate_chars(label, crate::config::MAX_FOOTNOTE_LABEL_CHARS);
                let link = ctx
                    .anchors
                    .footnote_id(&label)
                    .map(|anchor| Arc::<str>::from(format!("#{anchor}")));
                let text = ctx
                    .anchors
                    .footnote_number(&label)
                    .map_or_else(|| label.to_string(), |n| n.to_string());
                out.push(sup_frag(text, link.as_ref()));
                *i += 1;
            }
            Event::TaskListMarker(checked) => {
                if in_cell {
                    // Inside tables there is no room for a widget: text fallback.
                    out.push(make_frag(
                        if *checked { "[x] " } else { "[ ] " }.to_string(),
                        strong,
                        em,
                        strike,
                        false,
                        link,
                    ));
                } else if let Some(it) = ctx.item.as_mut() {
                    if it.check.is_none() {
                        it.check = Some(*checked);
                    }
                }
                *i += 1;
            }
            // Blocks inside inline context: stop (callers handle blocks).
            Event::Start(_) => break,
            _ => *i += 1,
        }
    }
    coalesce_text(out)
}

/// Maps an `<Badge type="…">` value to its pill accent.
fn badge_kind(ty: &str) -> crate::viewer::inline::Badge {
    use crate::viewer::inline::Badge;
    match ty.to_ascii_lowercase().as_str() {
        "note" | "info" | "default" | "blue" => Badge::Note,
        "tip" | "success" | "green" => Badge::Tip,
        "warning" | "warn" | "yellow" => Badge::Warning,
        "caution" | "danger" | "error" | "red" => Badge::Caution,
        _ => Badge::Neutral,
    }
}

/// Consumes a badge's content (until its close marker) into ONE pill
/// fragment. An unclosed badge stops at the block end, so it never eats the
/// rest of the document.
fn collect_badge<'e>(
    events: &[Event<'static>],
    i: &mut usize,
    badge: crate::viewer::inline::Badge,
) -> Frag<'e> {
    let mut buf = String::new();
    let mut depth = 1usize;
    while *i < events.len() {
        match &events[*i] {
            Event::End(_) => break,
            Event::Html(h) | Event::InlineHtml(h) => {
                if crate::markdown::document::badge_open(h).is_some() {
                    depth += 1;
                } else if crate::markdown::document::is_badge_close(h) {
                    depth -= 1;
                    if depth == 0 {
                        *i += 1;
                        break;
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => buf.push_str(t),
            Event::SoftBreak | Event::HardBreak => buf.push(' '),
            _ => {}
        }
        *i += 1;
    }
    badge_frag(buf, badge)
}

/// Merges adjacent text fragments that share the same style and link.
///
/// `pulldown-cmark` splits runs like `[!NOTE]` into several `Text` events
/// (`[`, `!NOTE`, `]`); callout detection, marker stripping and the
/// skip-link label all need the whole run. Hard-break fragments (`"\n"`)
/// are never merged, so `visual_lines` still splits correctly.
pub(super) fn coalesce_text(frags: Vec<Frag<'_>>) -> Vec<Frag<'_>> {
    let mut out: Vec<Frag<'_>> = Vec::with_capacity(frags.len());
    for f in frags {
        // A superscript footnote marker is its own widget even when the
        // neighbours share its style/link: never merge it into a text run.
        // A badge is its own pill for the same reason.
        let mergeable = f.image.is_none() && !f.sup && f.badge.is_none() && !f.text.contains('\n');
        if mergeable {
            if let Some(last) = out.last_mut() {
                let same = last.image.is_none()
                    && !last.sup
                    && last.badge.is_none()
                    && !last.text.contains('\n')
                    && last.strong == f.strong
                    && last.em == f.em
                    && last.strike == f.strike
                    && last.code == f.code
                    && last.link == f.link;
                if same {
                    // Append in place: the old `format!` reallocated and
                    // copied the whole accumulated run on every merge, O(n²)
                    // for a paragraph of hundreds of thousands of same-style
                    // fragments (one label per frame).
                    last.text.to_mut().push_str(f.text.as_ref());
                    continue;
                }
            }
        }
        out.push(f);
    }
    out
}

/// Counts the chars of `text`, stopping once more than `limit` are seen.
/// `limit + 1` means "over the limit" without scanning a multi-MB fragment:
/// the char-aware truncation pass runs per frame and a plain `chars().count()`
/// on one giant unfenced paragraph would be O(body) every repaint.
fn count_chars_up_to(text: &str, limit: usize) -> usize {
    text.chars().take(limit.saturating_add(1)).count()
}

/// Caps one block's rendered text
/// ([`crate::config::MAX_RENDERED_BLOCK_CHARS`]).
///
/// Returns the fragments unchanged when they fit; otherwise truncates the
/// overflowing fragment and appends a bounded notice. Image fragments keep
/// their alt text (already capped at capture time) and do not consume the
/// budget. Runs on every frame like the rest of the viewer, so the fit check
/// must be allocation-free.
pub(super) fn cap_block_text<'a>(frags: Vec<Frag<'a>>) -> Vec<Frag<'a>> {
    let cap = crate::config::MAX_RENDERED_BLOCK_CHARS;
    // Fast path (the common case): char count never exceeds byte count, so
    // a block under `cap` BYTES cannot need truncation. Only pathological
    // blocks pay for the char-aware pass: a repaint must not rescan every
    // paragraph of the document.
    let mut bytes = 0usize;
    let mut over = false;
    for f in &frags {
        if f.image.is_none() {
            bytes = bytes.saturating_add(f.text.len());
            if bytes > cap {
                over = true;
                break;
            }
        }
    }
    if !over {
        return frags;
    }
    let mut out: Vec<Frag<'a>> = Vec::with_capacity(frags.len() + 1);
    let mut used = 0usize;
    let mut truncated = false;
    for mut f in frags {
        // Image fragments never consume the budget and are never dropped:
        // they stay in place even after the text cut-off (a paragraph past
        // the cap used to lose every image that followed the truncation).
        if f.image.is_some() {
            out.push(f);
            continue;
        }
        if truncated {
            continue;
        }
        let remaining = cap.saturating_sub(used);
        let chars = count_chars_up_to(f.text.as_ref(), remaining);
        if chars <= remaining {
            used += chars;
            out.push(f);
            continue;
        }
        f.text =
            std::borrow::Cow::Owned(truncate_chars(f.text.as_ref(), remaining.max(1)).into_owned());
        out.push(f);
        truncated = true;
    }
    if truncated {
        out.push(make_frag(
            " … [truncated]",
            false,
            false,
            false,
            false,
            None,
        ));
    }
    out
}

/// Recurses into one nested inline context unless [`MAX_RENDER_DEPTH`] is
/// reached: then the inner content is flattened (text/code/breaks, no nested
/// styling) without recursing, so hostile `***…`/`[…]` nesting cannot
/// overflow the stack. `*i` is left ON the matching `End`, like
/// [`collect_inline`].
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_inline_nested<'e>(
    events: &'e [Event<'static>],
    i: &mut usize,
    ctx: &mut RenderCtx,
    strong: bool,
    em: bool,
    strike: bool,
    code: bool,
    link: Option<&Arc<str>>,
    in_cell: bool,
) -> Vec<Frag<'e>> {
    if ctx.depth >= MAX_RENDER_DEPTH {
        let mut out = Vec::new();
        collect_inline_flat(events, i, &mut out);
        return out;
    }
    ctx.depth += 1;
    let out = collect_inline(events, i, ctx, strong, em, strike, code, link, in_cell);
    ctx.depth -= 1;
    out
}

/// Flattens one inline context without recursing: text, code and breaks are
/// kept (styles dropped). Stops ON the matching `End`, which the caller
/// consumes exactly like [`collect_inline`] does.
pub(super) fn collect_inline_flat<'e>(
    events: &'e [Event<'static>],
    i: &mut usize,
    out: &mut Vec<Frag<'e>>,
) {
    let mut balance = 1usize;
    while *i < events.len() {
        match &events[*i] {
            Event::Start(_) => {
                balance += 1;
                *i += 1;
            }
            Event::End(_) => {
                balance -= 1;
                if balance == 0 {
                    break;
                }
                *i += 1;
            }
            Event::Text(t) => {
                out.push(make_frag(t.as_ref(), false, false, false, false, None));
                *i += 1;
            }
            Event::Code(c) => {
                out.push(make_frag(c.as_ref(), false, false, true, false, None));
                *i += 1;
            }
            Event::SoftBreak => {
                out.push(space_frag());
                *i += 1;
            }
            Event::HardBreak => {
                out.push(newline_frag());
                *i += 1;
            }
            Event::FootnoteReference(label) => {
                out.push(make_frag(
                    format!(
                        "[^{}]",
                        crate::util::truncate_chars(label, crate::config::MAX_FOOTNOTE_LABEL_CHARS)
                    ),
                    true,
                    false,
                    false,
                    false,
                    None,
                ));
                *i += 1;
            }
            _ => *i += 1,
        }
    }
}

/// Builds one inline fragment. An empty link destination is PRESERVED (it
/// marks an inert link: the source had a link, but the destination was
/// dropped), which the skip-link filter needs; `render_frag` still renders
/// it as plain text and never navigates.
/// `text` accepts anything convertible to `Cow`: borrowed event text stays
/// borrowed (no per-frame copy), rewritten fragments own their String.
pub(super) fn make_frag<'a>(
    text: impl Into<Cow<'a, str>>,
    strong: bool,
    em: bool,
    strike: bool,
    code: bool,
    link: Option<&Arc<str>>,
) -> Frag<'a> {
    Frag {
        text: text.into(),
        strong,
        em,
        strike,
        code,
        sup: false,
        link: link.cloned(),
        image: None,
        badge: None,
    }
}

/// [`make_frag`] for a footnote reference: the raised small marker that
/// links to the definition. `text` is the reference NUMBER (never the source
/// `[^label]` spelling).
pub(super) fn sup_frag(number: String, link: Option<&Arc<str>>) -> Frag<'static> {
    Frag {
        text: Cow::Owned(number),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: true,
        link: link.cloned(),
        image: None,
        badge: None,
    }
}

/// An inline `<Badge>`: a small pill (drawn as `viewer::inline::render_badge`).
pub(super) fn badge_frag(text: String, badge: crate::viewer::inline::Badge) -> Frag<'static> {
    Frag {
        text: Cow::Owned(text),
        strong: false,
        em: false,
        strike: false,
        code: false,
        sup: false,
        link: None,
        image: None,
        badge: Some(badge),
    }
}

/// Shared empty destination for an inert link: keeps the `Some(empty)`
/// marker per fragment (the skip-link filter needs it) without allocating
/// one `Arc` per inert link event.
pub(super) fn empty_link() -> Arc<str> {
    static EMPTY: std::sync::OnceLock<Arc<str>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::from("")).clone()
}

pub(super) fn space_frag() -> Frag<'static> {
    make_frag(" ", false, false, false, false, None)
}

pub(super) fn newline_frag() -> Frag<'static> {
    make_frag("\n", false, false, false, false, None)
}

/// One real (fetchable) image fragment. `title` is already display-bounded.
pub(super) fn real_image_frag<'a>(
    alt: String,
    strong: bool,
    em: bool,
    strike: bool,
    link: Option<&Arc<str>>,
    url: Arc<str>,
    title: String,
) -> Frag<'a> {
    Frag {
        text: alt.into(),
        strong,
        em,
        strike,
        code: false,
        sup: false,
        // Inside a link the image stays clickable.
        link: link.filter(|l| !l.is_empty()).cloned(),
        image: Some(ImageFrag {
            url,
            // Interned: `Frag` clones share the tooltip instead of copying
            // its bytes per frame.
            title: (!title.trim().is_empty()).then(|| Arc::<str>::from(title.trim())),
        }),
        badge: None,
    }
}

/// Which of two ADJACENT image URLs is the theme twin to KEEP.
///
/// Docs sites ship one logo as `logo-light.svg` + `logo-dark.svg` and let CSS
/// show the variant matching the theme; without CSS both render, so a card
/// showed two logos. A pair here means the two URLs are identical except for
/// exactly one token that is `light` vs `dark`, so unrelated icon runs are
/// never touched.
///
/// Returns `Some(keep_a)`, preferring the variant matching `dark`, or `None`
/// when the two URLs are not a theme pair. Only `.svg` URLs pair: a docs logo
/// is a vector, and this keeps a raster "light/dark preview" gallery (a real
/// pair of pictures, not one logo) untouched.
pub(super) fn theme_twin_choice(a: &str, b: &str, dark: bool) -> Option<bool> {
    if !is_svg_url(a) || !is_svg_url(b) {
        return None;
    }
    // Token-by-token comparison without allocating two `Vec<String>` per
    // adjacent image pair per frame: catalog pages used to pay O(n*m)
    // `String` allocs on every repaint.
    let mut ta = theme_token_iter(a);
    let mut tb = theme_token_iter(b);
    let mut diff: Option<(&str, &str)> = None;
    loop {
        match (ta.next(), tb.next()) {
            (Some(x), Some(y)) => {
                if !x.eq_ignore_ascii_case(y) {
                    if diff.is_some() {
                        return None; // more than one token differs: not a pair
                    }
                    diff = Some((x, y));
                }
            }
            (None, None) => break,
            _ => return None, // different token counts: not a pair
        }
    }
    let (x, y) = diff?; // identical URLs are not a pair
    let a_is_dark = if x.eq_ignore_ascii_case("light") && y.eq_ignore_ascii_case("dark") {
        false
    } else if x.eq_ignore_ascii_case("dark") && y.eq_ignore_ascii_case("light") {
        true
    } else {
        return None;
    };
    Some(a_is_dark == dark)
}

/// URL tokens split on `/ - _ .`, compared case-insensitively by the caller.
/// Returns borrowed slices (no per-token `String` per frame): catalog pages
/// with adjacent image pairs used to pay O(n*m) allocs on every repaint.
pub(super) fn theme_token_iter(url: &str) -> impl Iterator<Item = &str> + '_ {
    url.split(['/', '-', '_', '.']).filter(|s| !s.is_empty())
}

/// `true` when the URL path (query/fragment ignored) ends in `.svg`.
pub(super) fn is_svg_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path.len().checked_sub(4) {
        // `get` keeps a multi-byte character straddling the slice a miss
        // instead of a panic (URLs are remote input).
        Some(start) => path
            .get(start..)
            .is_some_and(|end| end.eq_ignore_ascii_case(".svg")),
        None => false,
    }
}
