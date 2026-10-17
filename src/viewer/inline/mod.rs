// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::inline`: inline fragments and single-line widgets.
//!
//! - `Frag` is the styled inline unit produced by the Markdown parser:
//!   text (optionally linked/styled) or an image reference.
//! - `render_frag` draws one fragment (text / code / link / image).
//! - `visual_lines` splits on hard breaks.
//! - `body_text` is the selectable label used for every document string.

use super::push_link_hit;
use super::LinkHit;
use crate::media::cache::ImageCache;
use crate::ui::theme;
use eframe::egui;
use std::borrow::Cow;
use std::sync::Arc;

/// Horizontal padding of the inline code pill beyond the glyph run (px):
/// `.4em` at the code size, which is the body size (16px), so 6.4px.
const CODE_PILL_PAD_X: f32 = 6.4;
/// Vertical padding of the inline code pill beyond the glyph box (px):
/// `.2em` at the 16px code size.
const CODE_PILL_PAD_Y: f32 = 3.2;
/// Corner radius of the inline code pill (px): the shared card radius (6px).
const CODE_PILL_RADIUS: u8 = crate::ui::theme::radius::CARD;
/// Height cap for one tight code pill (`code_pill_rows`): the body line
/// minus 3px. A face whose em box is tall (the bundled CJK mono cut used
/// for code measures ~1.4em) would otherwise inflate the pill into a whole
/// line box, losing the tight-tag look.
pub(super) const MAX_PILL_H: f32 = crate::ui::theme::BODY_LINE_HEIGHT - 3.0;

/// Image reference inside an inline fragment (absolute https URL).
#[derive(Debug, Clone)]
pub(crate) struct ImageFrag {
    /// Absolute https source. Interned (`DestCache`): cloning one is a
    /// refcount bump, so a document repeating one URL never copies it.
    pub(crate) url: Arc<str>,
    /// Markdown `title` attribute (tooltip), when present. Interned on an
    /// `Arc`: `Frag` is cloned per frame in mixed text/image paragraphs, and
    /// a `String` here would deep-copy per image per repaint.
    pub(crate) title: Option<Arc<str>>,
}

/// Inline fragment (styled text + optional link + optional image).
///
/// `text` borrows from the document's event list when unchanged: a repaint
/// (scroll, selection) must not copy the whole document every frame. Only
/// fragments that are actually rewritten (menu splitting, callout badge,
/// alt-text concatenation) own their text.
#[derive(Debug, Clone)]
pub(crate) struct Frag<'a> {
    pub(crate) text: Cow<'a, str>,
    pub(crate) strong: bool,
    pub(crate) em: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    /// Footnote reference: a raised small marker (`1`) linked to its
    /// definition, never the source spelling. `text` then carries the
    /// reference NUMBER (the anchor index assigns it).
    pub(crate) sup: bool,
    /// Interned destination (`DestCache` handle): one `Arc<str>` shared by
    /// every fragment of one link, never a string per fragment.
    pub(crate) link: Option<Arc<str>>,
    /// Set when this fragment is an image; `text` then carries the alt text.
    pub(crate) image: Option<ImageFrag>,
    /// Inline `<Badge>text</Badge>` (MDX): a small colored pill. `None` for
    /// every other fragment.
    pub(crate) badge: Option<Badge>,
}

/// Accent of an inline badge (`<Badge type="…">`), mapped to the alert
/// palette; the default is a neutral chrome pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Badge {
    Neutral,
    Note,
    Tip,
    Warning,
    Caution,
}

/// Splits frags on hard-break (`"\n"`) frags into visual lines.
/// Each `"\n"` frag starts a new line; other frags keep their style.
pub(crate) fn visual_lines<'a, 'b>(frags: &'b [Frag<'a>]) -> Vec<&'b [Frag<'a>]> {
    let mut lines: Vec<&'b [Frag<'a>]> = Vec::new();
    let mut start = 0usize;
    for (i, f) in frags.iter().enumerate() {
        if f.text.as_ref() == "\n" {
            lines.push(&frags[start..i]);
            start = i + 1;
        }
    }
    lines.push(&frags[start..]);
    lines
}

/// Selectable body text: drag-select and Cmd/Ctrl+C work natively.
/// Used for every document string so the page feels selectable.
/// Returns the `Response` so callers can measure rendered widths (list
/// markers use it for continuation alignment).
pub(crate) fn body_text(ui: &mut egui::Ui, rt: egui::RichText) -> egui::Response {
    ui.add(egui::Label::new(rt).selectable(true))
}

/// Base direction of one inline line: true when the first strong character
/// of its text is right-to-left (Unicode P2/P3). Image alt text does not
/// participate (it is metadata, not content).
pub(crate) fn line_is_rtl(frags: &[Frag<'_>]) -> bool {
    // Fast path: pure-ASCII lines are always LTR. Avoids the grapheme scan
    // plus a `String` allocation per paragraph per frame on large LTR docs.
    if frags.iter().all(|f| {
        f.image.is_some() || {
            let t = f.text.as_ref();
            t.is_ascii() || !super::bidi::contains_rtl(t)
        }
    }) {
        return false;
    }
    let text: String = frags
        .iter()
        .filter(|f| f.image.is_none())
        .map(|f| f.text.as_ref())
        .collect();
    super::bidi::is_rtl(&text)
}

/// Runs an inline fragment flow with its line direction: left-to-right by
/// default, right-to-left (wrapped, aligned from the right) when the base
/// direction is RTL, like a browser would lay the line out. Mirrors
/// `egui::Ui::horizontal_wrapped` exactly so LTR lines are untouched.
///
/// Returns the full response so callers can read the flow's rect (list
/// markers paint into the gutter using the first row's position).
pub(crate) fn inline_flow<R>(
    ui: &mut egui::Ui,
    frags: &[Frag<'_>],
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let dir = if line_is_rtl(frags) {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let initial_size = egui::vec2(
        ui.available_size_before_wrap().x,
        ui.spacing().interact_size.y,
    );
    ui.allocate_ui_with_layout(initial_size, dir.with_main_wrap(true), add)
}

/// Device-pixel side for one emoji texture: the drawn size times
/// `EMOJI_SCALE` (emoji draw slightly larger than the surrounding text),
/// bucketed by `media::emoji::device_px` so zoom levels share textures.
fn emoji_px(size: f32, ppp: f32) -> u32 {
    crate::media::emoji::device_px(size * crate::config::EMOJI_SCALE, ppp)
}

/// Renders `text` with color emoji inline images where possible, styling the
/// text pieces with `style`. Emoji without a color document (or over the
/// texture budget) fall back to the styled monochrome text, so this never
/// loses content.
///
/// `bg` is the wash painted BEHIND an emoji texture (select-all or the find
/// highlight): the surrounding pieces carry it, so a bare image would chop
/// the band. `None` draws the textures bare.
///
/// Returns the painted bounds (`None` when `text` has no color emoji, so the
/// caller keeps its normal one-piece text path). The probe is
/// allocation-free; when it passes, the draw walks the same segments
/// without building a list.
pub(crate) fn render_emoji_text(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    emoji: &mut crate::media::emoji::EmojiCache,
    bg: Option<egui::Color32>,
    style: &dyn Fn(&mut egui::Ui, &str) -> egui::RichText,
) -> Option<egui::Rect> {
    // Probe allocation-free BEFORE drawing: the caller's normal path must
    // render the text when there is no emoji. Then draw straight from the
    // no-`Vec` segment walk (the old `split` built one vector per fragment
    // per repaint and walked it twice).
    if !crate::media::emoji::has_color_emoji(text) {
        return None;
    }
    let mut bounds: Option<egui::Rect> = None;
    let mut extend = |rect: egui::Rect| {
        bounds = Some(match bounds {
            Some(b) => b.union(rect),
            None => rect,
        });
    };
    let px = emoji_px(size, ui.ctx().pixels_per_point());
    let side = size * crate::config::EMOJI_SCALE;
    crate::media::emoji::for_each_segment(text, |segment| match segment {
        crate::media::emoji::Segment::Text(piece) => {
            let rt = style(ui, piece);
            extend(body_text(ui, rt).rect);
        }
        crate::media::emoji::Segment::Emoji(cluster) => {
            match emoji.texture(ui.ctx(), cluster, px) {
                Some(texture) => {
                    let tex = texture.size_vec2();
                    let height = if tex.x > 0.0 {
                        side * tex.y / tex.x
                    } else {
                        side
                    };
                    let art = egui::vec2(side, height);
                    if let Some(bg) = bg {
                        // The wash must cover the emoji too: the surrounding
                        // labels get a background, so a bare image chopped
                        // the band.
                        let (rect, _) = ui.allocate_exact_size(art, egui::Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(crate::ui::theme::radius::SMALL),
                            bg,
                        );
                        egui::Image::new(&texture).paint_at(ui, rect);
                        extend(rect);
                    } else {
                        extend(
                            ui.add(egui::Image::new(&texture).fit_to_exact_size(art))
                                .rect,
                        );
                    }
                }
                None => {
                    let rt = style(ui, cluster);
                    extend(body_text(ui, rt).rect);
                }
            }
        }
    });
    Some(bounds.unwrap_or(egui::Rect::NOTHING))
}
/// Precise find split for a plain fragment (no link/code, LTR).
///
/// The fragment is painted as ONE galley whose washes are RECTANGLES painted
/// behind the glyphs. Two earlier shapes both moved the page:
///
/// - one WIDGET per piece: each piece was measured and wrapped on its own,
///   so a searched paragraph re-wrapped;
/// - one JOB with per-match SECTIONS: epaint shapes every section as its own
///   run, so the kerning across boundaries was lost and the text measured
///   ~1px wider over a long line, enough to push a borderline word to the
///   next row (acceptmarkdown.com's paragraphs moved on a space query).
///
/// Keeping the unsplit job (single section, single shaping run) keeps the
/// metrics byte-identical to the unsearched render; only the rectangles are
/// added.
///
/// A fragment holding color emoji never comes here: an emoji is a texture, so
/// the job could not draw it (and its pieces would be monochrome). Those go
/// through the whole-fragment emoji path with the wash painted behind the
/// textures.
///
/// Returns true when the fragment held at least one claimed match (painted
/// here); false renders nothing (no match, exotic folding, emoji, select-all
/// or past the global cap) and lets the caller decide.
fn render_plain_split(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    text: &str,
    selected: bool,
    fr: &mut crate::viewer::find::FindRender,
) -> bool {
    // Pre-gate: most fragments do not match, and the range walk below
    // lowercases + collects per fragment per frame. `contains_match` stops
    // at the first hit allocation-free on the ASCII path.
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
    // Color emoji: the caller's emoji path paints the textures (with the
    // wash behind them); a galley cannot.
    if crate::media::emoji::may_contain_emoji(text) {
        return false;
    }
    // Select-all washes the whole page anyway: the caller's fallback paints
    // the fragment uniformly (its selection background and forced colors are
    // the established look for that state).
    if selected {
        return false;
    }
    let Some((start, claimed)) = claim_fragment(fr, ranges.len(), fragment_capped) else {
        return false;
    };
    paint_find_wash(ui, style_frag(f, text), text, &ranges, start, claimed, fr);
    true
}

/// Claims a fragment's occurrences for `fr`: the start ordinal and how many
/// were taken, or `None` when nothing could be claimed (past the global cap).
/// The per-fragment cap (`fragment_capped`) is propagated to the bar's `N+`.
pub(crate) fn claim_fragment(
    fr: &mut crate::viewer::find::FindRender,
    n: usize,
    fragment_capped: bool,
) -> Option<(usize, usize)> {
    let start = fr.claim(n);
    let claimed = fr.next_ordinal - start;
    if fragment_capped && claimed == n {
        fr.capped = true;
    }
    (claimed > 0).then_some((start, claimed))
}

/// Paints the amber washes for `ranges[..claimed]` and then the text itself,
/// as one galley: `base` carries the fragment's own styling, the job is the
/// single-section one the unsearched render builds (identical shaping and
/// wrapping), and each match becomes one rectangle per galley row painted
/// BEHIND the glyphs (the label paints right after, on top). The current
/// match's rectangle is recorded for the caller's visibility retry and
/// scrolled into view; `start` is the ordinal of `ranges[0]` and the
/// ordinals are already claimed.
pub(crate) fn paint_find_wash(
    ui: &mut egui::Ui,
    base: egui::RichText,
    text: &str,
    ranges: &[(usize, usize)],
    start: usize,
    claimed: usize,
    fr: &mut crate::viewer::find::FindRender,
) {
    let job = std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(base).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        ui.text_valign(),
    ));
    // Mirror `Label::layout_in_ui`'s wrapping inputs (the whole row width and
    // the first row's indentation): the galley measured here is exactly the
    // one the label lays out below, so the rectangles land on the glyphs.
    let pos = egui::pos2(ui.max_rect().left(), ui.cursor().top());
    let avail = ui.available_width();
    let first_indent = avail - ui.available_size_before_wrap().x;
    let cursor_height = ui.cursor().height();
    let mut layout = job.clone();
    layout.wrap.max_width = avail;
    layout.first_row_min_height = cursor_height;
    layout.halign = egui::Align::Min;
    layout.justify = false;
    if let Some(first) = layout.sections.first_mut() {
        first.leading_space = first_indent;
    }
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(layout));
    let mut current_rect: Option<egui::Rect> = None;
    for (k, (s, e)) in ranges.iter().enumerate().take(claimed) {
        let current = start + k == fr.current;
        let bg = if current {
            theme::find_current_bg(ui)
        } else {
            theme::find_match_bg(ui)
        };
        for local in range_row_rects(&galley, text, *s, *e) {
            let screen = local.translate(pos.to_vec2());
            ui.painter().rect_filled(screen, 0.0, bg);
            if current {
                current_rect = Some(match current_rect {
                    Some(r) => r.union(screen),
                    None => screen,
                });
            }
        }
    }
    if let Some(rect) = current_rect {
        fr.current_rect = Some(rect);
        if fr.scroll {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }
    }
    // The text itself: the same selectable label the unsearched path adds,
    // on the same job (the layout above was a cache hit), so it paints on
    // top of the washes with byte-identical metrics.
    ui.add(egui::Label::new(job).selectable(true));
}

/// The fragment's own inline styling (strong/emphasis/strike at the body
/// line height): the recipe the normal text path and the find split share, so
/// a searched fragment lays out exactly like an unsearched one.
fn style_frag(f: &Frag<'_>, text: &str) -> egui::RichText {
    let mut rt = egui::RichText::new(text);
    if f.strong {
        rt = theme::strong(rt);
    }
    if f.em {
        rt = rt.italics();
    }
    if f.strike {
        rt = rt.strikethrough();
    }
    rt.line_height(Some(theme::BODY_LINE_HEIGHT))
}

/// One galley-local rectangle per row the byte range `[start, end)` touches
/// (the same cursor math egui's own selection painting uses), so a match
/// spanning a wrap gets one band per row instead of one box across both.
pub(crate) fn range_row_rects(
    galley: &egui::Galley,
    text: &str,
    start: usize,
    end: usize,
) -> Vec<egui::Rect> {
    let start_char = text[..start.min(text.len())].chars().count();
    let end_char = text[..end.min(text.len())].chars().count();
    let min = galley.layout_from_cursor(egui::text::CCursor::new(start_char));
    let max = galley.layout_from_cursor(egui::text::CCursor::new(end_char));
    let mut rects = Vec::new();
    for ri in min.row..=max.row {
        let Some(placed) = galley.rows.get(ri) else {
            break;
        };
        let row = &placed.row;
        let (left, right) = if min.row == max.row {
            (row.x_offset(min.column), row.x_offset(max.column))
        } else if ri == min.row {
            (row.x_offset(min.column), row.size.x)
        } else if ri == max.row {
            (0.0, row.x_offset(max.column))
        } else {
            (0.0, row.size.x)
        };
        rects.push(egui::Rect::from_min_max(
            egui::pos2(left, placed.pos.y),
            egui::pos2(right.max(left), placed.pos.y + row.size.y),
        ));
    }
    rects
}

/// Renders an inline fragment (text / code / link / image).
/// - Plain inline code goes through [`render_code_span`] (tight painted pill,
///   body baseline; select-all keeps the full-row background).
/// - A footnote reference goes through [`render_superscript_ref`]: a raised
///   small marker linked to its definition.
/// - Links use `ui.link` but navigate INTERNALLY (never open a browser).
/// - Real links push their screen rect into `links`, so the right-click
///   menu can offer "Open link in new tab" for the link under the cursor.
/// - Inert (`interactive=false`, loading backdrop): links render as plain text.
/// - When `selected` (select-all highlight), the foreground is forced to
///   `on_surface`: keeping link-blue / code-token colors on the pale
///   selection bg made the highlight unreadable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_frag(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
    find: &mut Option<crate::viewer::find::FindRender>,
) {
    if let Some(img) = &f.image {
        // Images own their whole fragment: alt text is the tooltip/caption.
        super::image::render_inline_image(
            ui,
            img,
            &f.text,
            f.link.as_ref(),
            super::image::ImgCtx {
                selected,
                interactive,
                on_link,
                images,
                links,
            },
        );
        return;
    }

    if f.sup {
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        render_superscript_ref(ui, f, size, selected, interactive, on_link);
        return;
    }

    if let Some(badge) = f.badge {
        let base = egui::TextStyle::Body.resolve(ui.style()).size;
        render_badge(ui, f.text.as_ref(), badge, selected, base);
        return;
    }

    // Right-to-left display order: epaint paints glyphs left-to-right and
    // applies no bidi, so RTL runs are reordered here (`viewer::bidi`).
    // Code spans keep the source byte-for-byte (program text is LTR).
    let display = if f.code {
        Cow::Borrowed(f.text.as_ref())
    } else {
        super::bidi::visual_text(f.text.as_ref())
    };
    let text = display.as_ref();

    // In-page find: plain fragments (no link/code, LTR) get a precise
    // per-occurrence split (wrapping still works: the pieces are separate
    // widgets in the same wrapped flow, monochrome while open so emoji
    // needs no special path); links, code and RTL fall back to a
    // whole-fragment highlight decided below (their widget must stay one
    // piece for hit-testing, pill painting or display order).
    if let Some(fr) = find.as_mut() {
        if !text.is_empty() {
            let splittable = f.link.is_none() && !f.code && !super::bidi::contains_rtl(text);
            if splittable && render_plain_split(ui, f, text, selected, fr) {
                return;
            }
        }
    }
    // Whole-fragment fallback ordinal for the branches below. Reached for
    // links, inline code, RTL runs and EMOJI fragments (the wash goes behind
    // their textures), and for a splittable fragment whose precise split
    // bailed. Claiming here is safe: a successful split RETURNS before this
    // point, so nothing is ever counted twice.
    let (find_hl, find_scroll): (Option<bool>, bool) = match find.as_mut() {
        Some(fr) if !text.is_empty() => {
            // Pre-gate like `render_plain_split`: skips the lowercase +
            // `Vec` for the common non-matching fragment.
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
                        // Past the global cap: the fragment renders normally.
                        (None, false)
                    } else {
                        let is_current = (start..start + claimed).contains(&fr.current);
                        (Some(is_current), fr.scroll)
                    }
                }
            }
        }
        _ => (None, false),
    };
    // Find highlight wins over select-all (the match must stay visible on
    // the pale selection wash, like links do via `selected_text`).
    let apply_hl =
        |ui: &egui::Ui, rt: egui::RichText, current: bool| theme::find_text(ui, rt, current);
    // Scrolls the current match into view and RECORDS its rect (the caller
    // retries while it stays off screen: a scroll request dropped by a
    // concurrent user scroll must not be lost).
    let scroll_to =
        |find: &mut Option<crate::viewer::find::FindRender>, ui: &egui::Ui, rect: egui::Rect| {
            if let Some(fr) = find.as_mut() {
                fr.current_rect = Some(rect);
            }
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        };

    // Color emoji for plain text fragments: links and code keep the
    // monochrome text path (a link must stay one widget for hit-testing).
    // The scalar pre-check skips the grapheme split (and its Vec) for
    // emoji-free text, which is almost every fragment on almost every page.
    // An emoji fragment NEVER takes the precise split (a galley cannot draw
    // a texture): the whole fragment takes the wash here, and the color
    // textures keep their color, so opening the find bar no longer turns
    // emoji monochrome.
    if f.link.is_none() && !f.code && crate::media::emoji::may_contain_emoji(text) {
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        let emoji_bg = find_hl
            .map(|current| {
                if current {
                    theme::find_current_bg(ui)
                } else {
                    theme::find_match_bg(ui)
                }
            })
            .or_else(|| selected.then(|| theme::select_bg(ui)));
        let rendered = render_emoji_text(ui, text, size, emoji, emoji_bg, &|ui, piece| {
            let mut rt = egui::RichText::new(piece);
            if f.strong {
                rt = theme::strong(rt);
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            rt = rt.line_height(Some(theme::BODY_LINE_HEIGHT));
            if let Some(current) = find_hl {
                rt = apply_hl(ui, rt, current);
            } else if selected {
                rt = theme::selected_text(ui, rt);
            }
            rt
        });
        if let Some(rect) = rendered {
            if let Some(current) = find_hl {
                if current {
                    if let Some(fr) = find.as_mut() {
                        fr.current_rect = Some(rect);
                    }
                    if find_scroll {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                }
            }
            return;
        }
    }

    if let Some(dest) = f.link.as_ref() {
        if f.code {
            // Code inside a link: the same tight pill as bare code (the old
            // `.code()` background painted the full line box, with a gray
            // band under the words), clickable only when the link is live.
            let color = if selected {
                theme::scheme_ref(ui).on_surface
            } else if dest.is_empty() {
                ui.visuals().text_color()
            } else {
                ui.visuals().hyperlink_color
            };
            let clickable = (interactive && !dest.is_empty()).then_some(dest);
            render_code_span(
                ui,
                f,
                text,
                selected,
                color,
                clickable,
                on_link,
                links,
                find,
                find_hl,
                find_scroll,
            );
            return;
        }
        if dest.is_empty() {
            // Ignored destination (scheme the base may not follow / `#frag`):
            // plain text, never nav.
            let mut rt = egui::RichText::new(text);
            if f.strong {
                rt = theme::strong(rt);
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            rt = rt.line_height(Some(theme::BODY_LINE_HEIGHT));
            if let Some(current) = find_hl {
                let resp = body_text(ui, apply_hl(ui, rt, current));
                if current && find_scroll {
                    scroll_to(find, ui, resp.rect);
                }
            } else {
                if selected {
                    rt = theme::selected_text(ui, rt);
                }
                body_text(ui, rt);
            }
            return;
        }
        if !interactive {
            // Backdrop: blue like a link but inert (no hand, no nav) so the
            // page doesn't flash black->blue when the load resolves.
            // Select-all forces readable on-surface instead of blue-on-blue.
            let mut rt = egui::RichText::new(text).color(ui.visuals().hyperlink_color);
            if f.strong {
                rt = theme::strong(rt);
            }
            if f.em {
                rt = rt.italics();
            }
            if f.strike {
                rt = rt.strikethrough();
            }
            rt = rt.line_height(Some(theme::BODY_LINE_HEIGHT));
            if let Some(current) = find_hl {
                let resp = body_text(ui, apply_hl(ui, rt, current));
                if current && find_scroll {
                    scroll_to(find, ui, resp.rect);
                }
            } else {
                if selected {
                    rt = theme::selected_text(ui, rt);
                }
                body_text(ui, rt);
            }
            return;
        }
        let mut rt = egui::RichText::new(text).color(ui.visuals().hyperlink_color);
        if f.strong {
            rt = theme::strong(rt);
        }
        if f.em {
            rt = rt.italics();
        }
        if f.strike {
            rt = rt.strikethrough();
        }
        rt = rt.line_height(Some(theme::BODY_LINE_HEIGHT));
        let find_current = find_hl;
        if let Some(current) = find_current {
            rt = apply_hl(ui, rt, current);
        } else if selected {
            rt = theme::selected_text(ui, rt);
        }
        let resp = ui.link(rt);
        if let Some(current) = find_current {
            if current && find_scroll {
                scroll_to(find, ui, resp.rect);
            }
        }
        // In-document anchors are jump targets, not pages: never offer
        // "Open link in new tab" for them.
        if !dest.starts_with('#') {
            push_link_hit(links, resp.rect, dest);
        }
        if resp.clicked() {
            on_link(dest);
        }
    } else if f.code {
        // Inline code (styled variants included): tight pill, body baseline.
        render_code_span(
            ui,
            f,
            text,
            selected,
            ui.visuals().text_color(),
            None,
            on_link,
            links,
            find,
            find_hl,
            find_scroll,
        );
    } else if f.strong || f.em || f.strike {
        let mut rt = egui::RichText::new(text);
        if f.strong {
            rt = theme::strong(rt);
        }
        if f.em {
            rt = rt.italics();
        }
        if f.strike {
            rt = rt.strikethrough();
        }
        rt = rt.line_height(Some(theme::BODY_LINE_HEIGHT));
        if let Some(current) = find_hl {
            let resp = body_text(ui, apply_hl(ui, rt, current));
            if current && find_scroll {
                scroll_to(find, ui, resp.rect);
            }
        } else {
            if selected {
                rt = theme::selected_text(ui, rt);
            }
            body_text(ui, rt);
        }
    } else if let Some(current) = find_hl {
        let resp = body_text(
            ui,
            apply_hl(
                ui,
                egui::RichText::new(text).line_height(Some(theme::BODY_LINE_HEIGHT)),
                current,
            ),
        );
        if current && find_scroll {
            scroll_to(find, ui, resp.rect);
        }
    } else if selected {
        body_text(
            ui,
            theme::selected_text(
                ui,
                egui::RichText::new(text).line_height(Some(theme::BODY_LINE_HEIGHT)),
            ),
        );
    } else {
        body_text(
            ui,
            egui::RichText::new(text).line_height(Some(theme::BODY_LINE_HEIGHT)),
        );
    }
}

mod code_span;
#[cfg(test)]
use code_span::code_pill_rows;
pub(crate) use code_span::render_code_span;

/// Horizontal padding of a badge pill beyond its text (px).
const BADGE_PAD_X: f32 = 6.0;
/// Vertical padding of a badge pill beyond its text (px).
const BADGE_PAD_Y: f32 = 2.0;
/// Badge text scale relative to the body size (small, like a tag).
const BADGE_SCALE: f32 = 0.8;
/// Ceiling for a badge's text size: a heading badge scales with its heading
/// but never past the H3 body step, so an H1 pill stays a tag (not a banner).
const BADGE_MAX_SIZE: f32 = 19.0;

/// Fill, ink and border of a badge pill.
///
/// The accent is used as the INK over a faint tint of itself (not as a solid
/// fill with body-colored text, which read dark-on-dark in light mode and
/// light-on-light in dark mode). A neutral badge uses the chrome tokens.
fn badge_colors(
    scheme: &theme::Scheme,
    badge: Badge,
) -> (egui::Color32, egui::Color32, egui::Color32) {
    let accent = match badge {
        Badge::Neutral => {
            return (
                scheme.container,
                scheme.on_surface_variant,
                scheme.outline_variant,
            );
        }
        Badge::Note => scheme.alert_note,
        Badge::Tip => scheme.alert_tip,
        Badge::Warning => scheme.alert_warning,
        Badge::Caution => scheme.alert_caution,
    };
    let tint = |a: u8| theme::tint(accent, a);
    // A faint tint (the accent stays the ink) plus a slightly stronger
    // border: enough to read the chip, light enough to keep >=3.5:1 contrast
    // for the darker accents (warning) in light mode.
    (tint(30), accent, tint(110))
}

/// Draws one inline `<Badge>` as a small rounded pill hugging its text: a
/// normal (proportional) face, NOT the code pill, in the accent of its
/// `type` (neutral chrome by default) with a readable ink in both themes.
/// `base_size` is the size of the text the badge sits in (body or heading),
/// so a heading badge scales with its heading.
pub(super) fn render_badge(
    ui: &mut egui::Ui,
    text: &str,
    badge: Badge,
    selected: bool,
    base_size: f32,
) {
    let scheme = theme::scheme_ref(ui);
    let (fill, ink, stroke) = badge_colors(scheme, badge);
    let size = (base_size * BADGE_SCALE).min(BADGE_MAX_SIZE);
    let font = egui::FontId::proportional(size);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let desired = egui::vec2(
        galley.size().x + BADGE_PAD_X * 2.0,
        galley.size().y + BADGE_PAD_Y * 2.0,
    );
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let radius = egui::CornerRadius::same(theme::radius::FULL);
    if selected {
        ui.painter().rect_filled(rect, radius, theme::select_bg(ui));
    } else {
        ui.painter().rect(
            rect,
            radius,
            fill,
            egui::Stroke::new(theme::stroke::HAIRLINE, stroke),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter()
        .galley(rect.min + egui::vec2(BADGE_PAD_X, BADGE_PAD_Y), galley, ink);
}

/// Scale of a footnote reference marker relative to the text it follows
/// (a superscript marker draws visibly smaller).
const SUP_REF_SCALE: f32 = 0.72;
/// How far above the box top the marker is raised, as a fraction of the box
/// height: a superscript sits ~0.3em above the text baseline, and the
/// baseline sits ~0.75em into the box, so ~0.09 of it clears the line.
const SUP_REF_RAISE_FACTOR: f32 = 0.09;

/// Renders one footnote reference as a superscript marker.
///
/// The source `[^label]` spelling never reaches the screen — the marker shows
/// the reference NUMBER (assigned by `markdown::anchors` in first-reference
/// order) — and it links to the definition like any other anchor, with the
/// same keyboard focus ring and hit target as a link. `size` is the size of
/// the text the marker belongs to (body, heading or table cell), so a large
/// heading gets a proportionally large marker.
///
/// The painted rect is remembered in egui memory under the reserved
/// definition anchor id: the definition's back-arrow scrolls here.
pub(crate) fn render_superscript_ref(
    ui: &mut egui::Ui,
    f: &Frag<'_>,
    size: f32,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
) {
    let s = theme::scheme_ref(ui);
    let dest = f.link.as_deref().unwrap_or("");
    let clickable = interactive && !dest.is_empty();
    let color = if selected {
        s.on_surface
    } else if dest.is_empty() {
        // No definition in this document: the marker is inert, and a muted
        // tone is the one signal that it is not a link.
        s.on_surface_variant
    } else {
        ui.visuals().hyperlink_color
    };
    let font = egui::FontId::proportional((size * SUP_REF_SCALE).round());
    let galley =
        ui.fonts_mut(|fonts| fonts.layout_no_wrap(f.text.clone().into_owned(), font, color));
    // Tall enough for its own line and never taller than the body line: a
    // marker must not grow a 24px flow row, and a heading keeps its natural
    // row height.
    let box_h = (size * 1.2).max(theme::BODY_LINE_HEIGHT.min(size));
    let size = egui::vec2(galley.size().x + theme::spacing::XXS, box_h);
    let sense = if clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::select_bg(ui),
        );
    }
    // Borderless: the marker is raised OUTSIDE the line box, so a
    // rect-shrunk ring would draw a box around the text below it.
    let raise = -SUP_REF_RAISE_FACTOR * box_h;
    if clickable && resp.has_focus() {
        theme::focus_ring(
            ui,
            rect.translate(egui::vec2(0.0, raise)),
            theme::radius::SMALL,
        );
    }
    ui.painter().galley(
        egui::pos2(rect.min.x + 1.0, rect.min.y + raise),
        galley,
        color,
    );
    let resp = resp.on_hover_cursor(if clickable {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::Default
    });
    // Accessible name: the marker's meaning is not in its glyphs.
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Link,
            true,
            format!("Footnote {}", f.text.as_ref()),
        )
    });
    if clickable {
        if dest.starts_with('#') {
            // Remember where this reference was painted so the definition's
            // back-arrow can scroll to it (the definition renders later in
            // the same frame). Stored WITH the document generation: the rect
            // belongs to one painted frame of one document, so a later
            // document that reuses the same `#fn-...` label can never scroll
            // to a stale rect left by the previous one (the entry stays
            // parked in egui memory; the guard makes it inert).
            let remembered = egui::Id::new(("mdowser-fnref", dest));
            ui.data_mut(|d| d.insert_temp(remembered, (fnref_generation(), rect)));
        }
        if resp.clicked() {
            on_link(dest);
        }
    }
}

thread_local! {
    /// `(document salt, generation)` of the document currently being
    /// rendered. The generation is bumped when the SALT changes (a new
    /// document), never per frame. See the footnote-reference memo above.
    static FNREF_GENERATION: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// Starts (or resumes) a document pass and returns its generation. Called by
/// `show_markdown` before the event walk with the per-document salt.
///
/// A per-FRAME bump made the memo useless for a definition that precedes its
/// reference in the source: the back-arrow was painted before the reference
/// of the same frame, and the previous frame's stamp never matched, so the
/// click did nothing. Keyed to the salt, the rect written on any frame of
/// this document stays valid.
pub(crate) fn begin_fnref_generation(doc_salt: u64) -> u64 {
    FNREF_GENERATION.with(|cell| {
        let (salt, generation) = cell.get();
        if salt == doc_salt && generation != 0 {
            return generation;
        }
        let next = generation.wrapping_add(1).max(1);
        cell.set((doc_salt, next));
        next
    })
}

/// The generation of the document being rendered now.
fn fnref_generation() -> u64 {
    FNREF_GENERATION.with(std::cell::Cell::get).1
}

/// Readers of the footnote-reference memo (the back-arrow in
/// `viewer::markdown::walk`): returns the generation stamp and only scrolls
/// when it still belongs to the document being rendered.
pub(crate) fn fnref_rect_for(ui: &egui::Ui, dest: &str) -> Option<egui::Rect> {
    let key = egui::Id::new(("mdowser-fnref", dest));
    let (generation, rect) = ui.data(|d| d.get_temp::<(u64, egui::Rect)>(key))?;
    (generation == fnref_generation()).then_some(rect)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_find;
