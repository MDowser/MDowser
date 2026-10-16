// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown::menu`: inline navigation menus.
//!
//! A one-line paragraph made of pipe/dot-separated entries renders as
//! a centered nav row. Deliberately strict (measure-first, bounded
//! entries, navigation guard) so sentences and tables never qualify.

use super::*;

/// Separators recognized in an inline menu line, in preference order:
/// pipes first, then the dot family ("`Uno · Dos · Tres`", "`Home • Docs`").
pub(super) const MENU_SEPARATORS: [char; 5] = ['|', '·', '•', '∙', '⋅'];

/// One recognized inline navigation row (`Home | Docs | About`).
pub(super) struct Menu<'a> {
    /// Separator found in the source, rendered muted between entries.
    pub(super) sep: char,
    /// Entries with their per-frag styles/links preserved.
    pub(super) items: Vec<Vec<Frag<'a>>>,
}

/// Splits a pipe/dot menu paragraph into entries, or `None` when it is not
/// a menu.
///
/// Strict on purpose (never steals tables or sentences):
/// - Single visual line only (no hard breaks); tables never reach here as
///   paragraphs (pulldown-cmark emits `Tag::Table`), but delimiter-like
///   items (`---`, `:--:`) still reject.
/// - No code frags (pipes inside `` `code` `` stay literal) and no images.
/// - Pipe splits anywhere; dot separators (`·`, `•`, `∙`, `⋅`) split only
///   as their own token (whitespace on one side) so `col·legi` stays a word.
/// - `2..=8` entries (`config::MENU_*`); each: max chars, max words, no
///   sentence punctuation (`.!?;,`), bounded total length.
/// - Navigation guard (BOTH separator families): a bare `pan | vino | queso`
///   or `término · definición` sentence stays a paragraph; a menu needs at
///   least one link or one label starting uppercase/digit.
/// - Preserves per-frag styles/links so `[Docs](/d) | Home` keeps its link.
pub(super) fn menu_items<'a>(frags: &[Frag<'a>]) -> Option<Menu<'a>> {
    // Code pipes stay literal; an image is never a menu.
    if frags.iter().any(|f| f.code || f.image.is_some()) {
        return None;
    }
    // Cheap pre-reject BEFORE `visual_lines` and the joined allocation: a
    // fragment without any supported separator can never split into a menu,
    // and this runs for every top-level paragraph every frame.
    if !frags.iter().any(|f| {
        MENU_SEPARATORS
            .iter()
            .any(|separator| f.text.contains(*separator))
    }) {
        return None;
    }
    if visual_lines(frags).len() != 1 {
        return None;
    }
    // Bound the join itself: `joined.chars().count()` below rejects menus
    // past `max_items * (ITEM_MAX + 3)` chars, and each char is at most 4
    // bytes in UTF-8, so a byte sum past that times 4 can never become a
    // menu. This keeps a hostile `a|`-repeated paragraph from allocating
    // a body-sized `String` every frame just to be rejected.
    let max_items = crate::config::MENU_MAX_ITEMS + 2;
    let max_chars = max_items * (crate::config::MENU_ITEM_MAX_CHARS + 3);
    let total_bytes: usize = frags.iter().map(|f| f.text.len()).sum();
    if total_bytes > max_chars.saturating_mul(4) {
        return None;
    }
    let joined: String = frags.iter().map(|f| f.text.as_ref()).collect();
    // Cap the scan itself: a valid menu has at most `MENU_MAX_ITEMS` items
    // plus leading/trailing empties, so a paragraph with thousands of pipes
    // (`a|` repeated) is rejected on the joined LENGTH before any per-
    // segment `Vec`/`String` is built. The old split-then-check order
    // allocated one Vec per separator per frame and could hang the UI.
    if joined.chars().count() > max_chars {
        return None;
    }
    // First supported separator that actually splits this line.
    let spans = MENU_SEPARATORS
        .into_iter()
        .map(|sep| (sep, menu_spans(&joined, sep, max_items + 1)))
        .find(|(_, spans)| !spans.is_empty())?;
    let (sep, spans) = spans;
    // Re-check on the SPANS before splitting fragments: `spans.len() + 1`
    // segments can only drop to `MENU_MAX_ITEMS` after at most two outer
    // empties, so anything larger can never become a menu.
    if spans.len() + 1 > max_items {
        return None;
    }
    let items = split_frags_at(frags, &spans);
    // Drop one outer empty from leading/trailing `|`.
    let mut items = items;
    if items
        .first()
        .is_some_and(|v| v.iter().all(|f| f.text.trim().is_empty()))
    {
        items.remove(0);
    }
    if items
        .last()
        .is_some_and(|v| v.iter().all(|f| f.text.trim().is_empty()))
    {
        items.pop();
    }
    if items.len() < crate::config::MENU_MIN_ITEMS || items.len() > crate::config::MENU_MAX_ITEMS {
        return None;
    }
    let mut total = 0usize;
    for item in &items {
        let text: String = item.iter().map(|f| f.text.as_ref()).collect();
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        // Table delimiter lookalike (`---`, `:---:`) is not a menu item.
        let no_space: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        let is_delim = !no_space.is_empty()
            && no_space.chars().all(|c| c == '-' || c == ':' || c == '|')
            && no_space.contains('-');
        if is_delim {
            return None;
        }
        if t.chars().count() > crate::config::MENU_ITEM_MAX_CHARS {
            return None;
        }
        if t.contains(['.', '!', '?', ';', ',']) {
            return None;
        }
        if t.split_whitespace().count() > crate::config::MENU_ITEM_MAX_WORDS {
            return None;
        }
        total += t.chars().count();
    }
    if total > crate::config::MENU_TOTAL_MAX_CHARS {
        return None;
    }
    // Navigation guard (BOTH separator families): a bare `pan | vino |
    // queso` or `término · definición` sentence is not navigation. Require at
    // least one real link, or at least one item that looks like a nav label
    // (starts uppercase / digit). All-lowercase prose stays a paragraph;
    // mixed menus like `Home | docs` still render as menus.
    let has_link = items
        .iter()
        .flatten()
        .any(|f| f.link.as_deref().is_some_and(|l| !l.is_empty()));
    let looks_like_nav = items.iter().any(|item| {
        let text: String = item.iter().map(|f| f.text.as_ref()).collect();
        let t = text.trim();
        t.chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    });
    if !has_link && !looks_like_nav {
        return None;
    }
    // Trim leading/trailing whitespace-only edges for clean rendering.
    let mut out = items;
    for item in &mut out {
        while item.first().is_some_and(|f| f.text.trim().is_empty()) {
            item.remove(0);
        }
        while item.last().is_some_and(|f| f.text.trim().is_empty()) {
            item.pop();
        }
        if item.is_empty() {
            return None;
        }
        if let Some(first) = item.first_mut() {
            let trimmed = first.text.trim_start().to_string();
            first.text = trimmed.into();
        }
        if let Some(last) = item.last_mut() {
            let trimmed = last.text.trim_end().to_string();
            last.text = trimmed.into();
        }
        if item.iter().all(|f| f.text.is_empty()) {
            return None;
        }
    }
    Some(Menu { sep, items: out })
}

/// `(start, end)` byte spans of `sep` where it splits a menu line. Pipes
/// split anywhere; dot-family separators only when they stand as their own
/// token (whitespace or edge on one side). Stops after `limit` spans: the
/// caller only needs to know there are too many to ever form a menu, so a
/// separator-dense paragraph never allocates one tuple per character.
pub(super) fn menu_spans(joined: &str, sep: char, limit: usize) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for (i, c) in joined.char_indices() {
        if c != sep {
            continue;
        }
        let token = sep == '|'
            || joined[..i]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace)
            || joined[i + c.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace);
        if token {
            spans.push((i, i + c.len_utf8()));
            if spans.len() >= limit {
                break;
            }
        }
    }
    spans
}

/// Splits fragments into the segments between separator spans, preserving
/// per-frag styles. Separators are dropped (they are re-rendered muted).
pub(super) fn split_frags_at<'a>(
    frags: &[Frag<'a>],
    spans: &[(usize, usize)],
) -> Vec<Vec<Frag<'a>>> {
    let total: usize = frags.iter().map(|f| f.text.len()).sum();
    // Segment ranges between separator spans.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for &(span_start, span_end) in spans {
        if span_start > start {
            ranges.push((start, span_start));
        }
        start = span_end;
    }
    if start < total {
        ranges.push((start, total));
    }
    let mut items: Vec<Vec<Frag<'a>>> = Vec::with_capacity(ranges.len());
    let (mut fi, mut base) = (0usize, 0usize);
    for (range_start, range_end) in ranges {
        // Advance to the fragment holding `range_start`.
        while fi < frags.len() && base + frags[fi].text.len() <= range_start {
            base += frags[fi].text.len();
            fi += 1;
        }
        let mut item = Vec::new();
        let (mut f, mut fb) = (fi, base);
        while f < frags.len() && fb < range_end {
            let text = frags[f].text.as_ref();
            let from = range_start.saturating_sub(fb).min(text.len());
            let to = (range_end - fb).min(text.len());
            // `get` instead of indexing: the offsets derive from the joined
            // line and should always land on char boundaries, but a mixed-up
            // fragment must never panic the UI thread over a decorative menu.
            if to > from {
                if let Some(slice) = text.get(from..to) {
                    let mut piece = frags[f].clone();
                    piece.text = slice.to_string().into();
                    item.push(piece);
                }
            }
            fb += text.len();
            f += 1;
        }
        items.push(item);
    }
    items
}

/// Renders a pipe/dot menu as a centered nav row with muted separators.
///
/// The row is centered by measuring the rendered width (egui has no
/// "shrink-wrap then center" layout); when it cannot fit the column it
/// falls back to a left-aligned wrapping row.
pub(super) fn flush_menu(ui: &mut egui::Ui, menu: &Menu<'_>, ctx: &mut RenderCtx) {
    ui.add_space(theme::spacing::XS);
    let sep = menu.sep.to_string();
    let sep_w = {
        let font = egui::TextStyle::Small.resolve(ui.style());
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(sep.clone(), font, ui.visuals().text_color())
                .size()
                .x
        })
    };
    let gap = 6.0;
    let item_w: Vec<f32> = menu
        .items
        .iter()
        .map(|item| item.iter().map(|f| frag_width(ui, f)).sum::<f32>())
        .collect();
    let total = item_w.iter().sum::<f32>()
        + (menu.items.len().saturating_sub(1)) as f32 * (2.0 * gap + sep_w);
    let avail = ui.available_width();
    if total <= avail {
        let pad = ((avail - total) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(pad);
            for (i, item) in menu.items.iter().enumerate() {
                if i > 0 {
                    ui.add_space(gap);
                    ui.label(theme::weak(ui, &sep));
                    ui.add_space(gap);
                }
                for f in item {
                    render_frag(
                        ui,
                        f,
                        ctx.selected,
                        ctx.interactive,
                        ctx.on_link,
                        ctx.images,
                        ctx.emoji,
                        ctx.links,
                        &mut ctx.find,
                    );
                }
            }
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, item) in menu.items.iter().enumerate() {
                if i > 0 {
                    ui.add_space(gap);
                    ui.label(theme::weak(ui, &sep));
                    ui.add_space(gap);
                }
                // One zero-spacing group per entry: the wrapped row's `gap`
                // used to be inserted BETWEEN the fragments of an entry
                // (`**Home** page` rendered as `Home  page`).
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for f in item {
                        render_frag(
                            ui,
                            f,
                            ctx.selected,
                            ctx.interactive,
                            ctx.on_link,
                            ctx.images,
                            ctx.emoji,
                            ctx.links,
                            &mut ctx.find,
                        );
                    }
                });
            }
        });
    }
    ui.add_space(theme::spacing::M);
}

/// Approximate rendered width of one inline fragment (body font): used to
/// center a menu row before drawing it.
pub(super) fn frag_width(ui: &egui::Ui, f: &Frag<'_>) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(f.text.to_string(), font, ui.visuals().text_color())
            .size()
            .x
    })
}
