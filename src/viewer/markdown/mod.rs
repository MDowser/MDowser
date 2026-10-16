// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::markdown`: CommonMark + table extensions block renderer.
//!
//! - Renders from OWNED events parsed once per fetch (`Fetched.events`):
//!   no `pulldown-cmark` work on repaint (scrolling/selection).
//! - RECURSIVE descent over the event stream: list items, block quotes and
//!   footnotes nest correctly (loose lists keep their bullets, task lists
//!   keep checkbox + text together, quotes draw one bar per depth level,
//!   rules/headings inside a context stay indented). Unordered items draw
//!   one large document bullet `·` in a fixed gutter, hugging the text
//!   column and aligned with the first text row, with a hanging indent for
//!   every wrapped row and compact item spacing.
//! - Links are absolutized with `protocol::rewrite` BEFORE rendering;
//!   clicks navigate internally (never open an external browser).
//! - Images become image fragments: `viewer::image` draws them from the
//!   per-document cache while the `media` pipeline fetches/decodes in the
//!   background. Non-https destinations stay `[image: alt]` text.
//! - Standalone images render as centered blocks; inline occurrences stay in
//!   the text flow. BOTH use the image's natural pixel size, scaled down to
//!   the reading-column width when wider (never upscaled).
//! - Runs of 2+ consecutive large images (site photo strips) render as ONE
//!   centered, wrapping gallery row instead of one block per image: source
//!   order and horizontal position stay put while the background fetches
//!   fill the textures in.
//! - Runs of 2+ inline SVG icons share ONE height, and that height is shared
//!   across the whole block (a card list's logos match even though each card
//!   is its own row); a lone icon keeps its size.
//! - A light/dark logo pair (`x-light.svg` next to `x-dark.svg`) collapses
//!   to the single variant matching the document theme.
//! - HTML comments (`<!-- ... -->`) are invisible, like in a browser: a
//!   comment-only block paints nothing (an empty list item keeps its
//!   marker row) and an inline comment leaves no gap, so
//!   `foo<!-- x -->bar` reads `foobar`. `plain_text` drops them the same
//!   way, so Copy keeps matching the page.
//! - Structural MDX components render natively ([`components`]): `<Tabs>`
//!   and `<Accordion>` are interactive (their state lives in egui's temp
//!   store, keyed by document + labels), `<Card>` is a bordered box and
//!   `<Steps>` numbers its rows. The normalizer leaves `<mdowser-cmp …/>`
//!   markers around the content, one HTML block each.
//!
//! The implementation is split into submodules by concern, all in one
//! render pass:
//! - [`frags`]: inline fragment collection (styles, links, images,
//!   theme twins) from the owned event stream.
//! - [`items`]: list item markers, task checkboxes and indentation.
//! - [`blocks`]: paragraph/heading/table/HTML/image-block rendering.
//! - [`budget`]: block-budget helpers (skip, truncation notice, flat fallback).
//! - [`links`]: link runs, skip links and callout blocks.
//! - [`menu`]: inline navigation menu detection and rendering.
//! - [`components`]: native MDX component blocks (`<Tabs>`, `<Accordion>`,
//!   `<Card>`, `<Steps>`), rendered from the normalizer's markers.

use super::code::render_code_block;
use super::inline::{
    body_text, inline_flow, line_is_rtl, render_emoji_text, render_frag, visual_lines, Frag,
    ImageFrag,
};
use super::push_link_hit;
use super::table::{flush_table_with_find, TableCtx};
use super::LinkHit;
use crate::markdown::anchors::AnchorIndex;
use crate::markdown::document::{is_html_break, is_html_comment};
use crate::media::cache::{ImageCache, Status};
use crate::ui::theme;
use crate::util::truncate_chars;
use eframe::egui;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use std::borrow::Cow;
use std::sync::Arc;
use url::Url;

/// Item indentation step per list nesting level (px).
/// Also the minimum content column of an item: nested lists and item text
/// line up at `ITEM_INDENT`, so level N+1 never hangs left of level N's text.
const ITEM_INDENT: f32 = 24.0;
/// Content column of an unordered list item: the `·` bullet hugs the text
/// column inside this fixed gutter (24px = 1.5em at the body size, the
/// classic document list indent).
const BULLET_GUTTER: f32 = 24.0;
/// Distance from the text column (the gutter's inner edge) to the centered
/// ink of the document bullet `·`: the dot sits close to its text instead
/// of floating in the middle of the gutter.
const BULLET_TEXT_GAP: f32 = 9.0;
/// Minimum gap between an ordered marker (`1. `) and its text when the
/// number is wide enough to fill the gutter.
const MARKER_GAP: f32 = 4.0;
/// Font-size factor of the document bullet `·`. The glyph's ink is tiny
/// (about 0.1em, a speck at body size), so it is drawn larger to reach
/// a printed-bullet ink of ~4-5px at the 16px body size; the dot stays
/// inside the gutter (never invades the text column).
const BULLET_SCALE: f32 = 2.5;
/// Max chars of an image `alt` shown as caption/tooltip.
/// Single source in `config::MAX_IMAGE_ALT_CHARS`.
const MAX_IMAGE_ALT_CHARS: usize = crate::config::MAX_IMAGE_ALT_CHARS;
/// Max chars of an image `title` tooltip.
/// Single source in `config::MAX_IMAGE_TITLE_CHARS`.
const MAX_IMAGE_TITLE_CHARS: usize = crate::config::MAX_IMAGE_TITLE_CHARS;
/// Max recursion depth of the renderer. A hostile document (`> > > …`,
/// `- - - …`, `***…`) can nest arbitrarily; the renderer is recursive and a
/// stack overflow on the UI thread aborts the process (it cannot be caught).
/// Deeper content renders flattened instead of recursing.
/// Single source in `config::MAX_MARKDOWN_RENDER_DEPTH`.
const MAX_RENDER_DEPTH: usize = crate::config::MAX_MARKDOWN_RENDER_DEPTH;

/// State of the list item currently being rendered.
struct ItemState {
    /// Nesting depth (0 = top-level list).
    depth: usize,
    /// Marker text (`·` for unordered items, `N. ` for ordered ones).
    /// Borrowed for the shared bullet (no per-item allocation per frame);
    /// owned only for the integer prefix of ordered items.
    marker: std::borrow::Cow<'static, str>,
    /// Ordinal of an ordered item (0 for unordered ones): the marker text for
    /// an ordered item is exactly `"{ordinal}. "`, so the marker-width cache
    /// keys on this number instead of allocating a `String` key per item per
    /// frame.
    ordinal: u64,
    /// `true` for unordered items: the marker is a document bullet drawn in
    /// the fixed [`BULLET_GUTTER`] column instead of its measured width.
    bullet: bool,
    /// `true` for a footnote DEFINITION marker: `"{number}. "` drawn small
    /// and bold on the note's first text line — the number the raised
    /// reference shows (a list ordinal draws at body size instead).
    note: bool,
    /// Task checkbox state, when the item is a task list item.
    check: Option<bool>,
    /// Whether the marker (bullet or checkbox) was already drawn.
    marker_rendered: bool,
    /// Measured content offset after the marker, for continuation lines.
    align: f32,
}

/// Mutable rendering context (recursion state).
struct RenderCtx<'a> {
    /// Anchor targets of this document (`#id` links resolve against it, and
    /// heading ids are keyed by their event index).
    anchors: &'a AnchorIndex,
    /// Link/image destinations pre-resolved at load time: the per-frame
    /// fragment tree must not `Url::parse` anything (see `DestCache`).
    dests: &'a crate::protocol::rewrite::DestCache,
    /// Requested scroll target (from an anchor click or a cross-page
    /// fragment). Consumed by the matching heading/footnote; cleared at the
    /// end of the document when nothing claimed it.
    pending_anchor: &'a mut Option<String>,
    selected: bool,
    interactive: bool,
    /// Current document theme (`ui.visuals().dark_mode`): a light/dark image
    /// twin keeps the variant matching it.
    dark: bool,
    viewport_w: f32,
    type_scale: f32,
    on_link: &'a mut dyn FnMut(&str),
    /// Per-document image cache: `ensure()` registers live-document images.
    images: &'a mut ImageCache,
    /// Per-tab color-emoji texture cache (bounded, process-budgeted).
    emoji: &'a mut crate::media::emoji::EmojiCache,
    /// Link hitboxes collected this frame (right-click "Open in new tab").
    links: &'a mut Vec<LinkHit>,
    doc_salt: u64,
    /// Current blockquote nesting depth (0 = outside).
    quote: usize,
    /// Blocks already rendered directly inside the current quote (0 at its
    /// start): lets the quote give paragraphs air BETWEEN them instead of
    /// after the last one, so the box's bottom edge hugs the text.
    quote_blocks: usize,
    /// Current list item, when rendering inside one.
    item: Option<ItemState>,
    /// Current recursion depth (blocks + inline nesting), capped at
    /// [`MAX_RENDER_DEPTH`] so hostile nesting cannot overflow the stack.
    depth: usize,
    /// True until the first TOP-LEVEL block has been consumed. Only that
    /// block may be a leaked accessibility skip link.
    first_block: bool,
    /// Set while rendering the first top-level block: `render_paragraph`
    /// consults it before dropping a skip link.
    allow_skip: bool,
    /// Remaining code-block LINES this document may render this frame.
    /// A fenced block is one event, so the event cap does not bound visual
    /// lines: without this budget a few MB of newlines would render millions
    /// of labels per frame and freeze the UI. Beyond it, blocks show a
    /// one-line "hidden" notice; `Copy` still copies the full body.
    code_lines_left: usize,
    /// Remaining BLOCKS this document may render this frame
    /// (`config::MAX_RENDERED_BLOCKS`). `MAX_EVENTS_PER_DOC` bounds parsing
    /// memory, not per-frame layout: without this a crafted few-hundred-KB
    /// body can queue hundreds of thousands of widgets every frame.
    blocks_left: usize,
    /// Remaining table CELLS this document may collect and render this frame
    /// (`config::MAX_RENDERED_TABLE_CELLS`): one row is charged one block,
    /// but a row may hold thousands of cells, each building its own widget
    /// subtree.
    table_cells_left: usize,
    /// Set when the block budget skipped content (drives the truncation
    /// notice; `Copy` always keeps the full text).
    truncated: bool,
    /// True once the first footnote DEFINITION was rendered: the footnotes
    /// section draws one rule before the first one only.
    footnotes_started: bool,
    /// Definitions the document never references, in render order: their
    /// numbers continue after the referenced ones so every note has a
    /// distinct marker.
    unreferenced_footnotes: usize,
    /// Back-arrow of the footnote definition being rendered, when it has one
    /// (`#fn-…` anchor id). Set before the note's blocks and CONSUMED by the
    /// next inline flow they lay out, which appends the button as its last
    /// inline box: the arrow then sits at the end of the note's text on its
    /// own line, and wraps like text when the line has no room left. A note
    /// whose blocks never run an inline flow (and an empty one) leaves it
    /// set, and the footnote branch places the button on a trailing row.
    pending_back_arrow: Option<&'a str>,
    /// In-page find cursor for this frame (`None` when the bar is closed or
    /// the query is empty). Assigns match ordinals in document order while
    /// painting; the total is handed back to `FindState` after the walk.
    find: Option<crate::viewer::find::FindRender>,
}

impl<'a> RenderCtx<'a> {
    /// Bundles the shared image-render parameters for one `image::render_*`
    /// call (see `image::ImgCtx`): reborrowed per call, so sequential
    /// images in one scope each get a fresh bundle without holding the walk
    /// borrow across the paint.
    pub(crate) fn img_ctx(&mut self) -> crate::viewer::image::ImgCtx<'_> {
        crate::viewer::image::ImgCtx {
            selected: self.selected,
            interactive: self.interactive,
            on_link: &mut *self.on_link,
            images: &mut *self.images,
            links: &mut *self.links,
        }
    }
}

/// Renders CommonMark Markdown (tables, footnotes, tasklists) in egui.
///
/// - `events`: body parsed once at fetch time (`Fetched.events`).
/// - `anchors`: in-document anchor targets (`#id` links resolve against it;
///   unknown anchors render as plain text; heading ids are looked up by
///   event index).
/// - `dests`: link/image destinations resolved once at fetch time
///   (`Fetched.dests`); the per-frame tree does no URL parsing.
/// - `pending_anchor`: requested scroll target; consumed by the first
///   matching element and cleared here when nothing matches.
/// - `doc_salt`: per-document widget id salt. The caller mixes the URL hash
///   with the TAB id, so two tabs on the same document keep independent
///   scroll/table/code state.
/// - `interactive`: false for loading backdrop (links render as plain text,
///   no hover hand, no navigation, no image fetches).
/// - `viewport_w`: CentralPanel width BEFORE the reading column (responsive).
/// - `type_scale`: precomputed from `viewport_w` (headings).
/// - `images`: per-document image cache (`ensure()` starts background loads).
/// - `on_link`: callback with the ABSOLUTE destination when a link is clicked
///   (or the `#anchor` form for an in-document jump).
/// - `links`: screen-space hitboxes of every rendered link (cleared by the
///   caller; the right-click menu hit-tests them; anchors are excluded).
/// - `find`: in-page find state (query/current/scroll in, total/capped out).
/// - `doc_generation`: per-tab document generation (stale find totals are
///   dropped when the visible document is replaced).
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_markdown_with_find(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    anchors: &AnchorIndex,
    dests: &crate::protocol::rewrite::DestCache,
    pending_anchor: &mut Option<String>,
    doc_salt: u64,
    selected: bool,
    interactive: bool,
    viewport_w: f32,
    type_scale: f32,
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    on_link: &mut dyn FnMut(&str),
    links: &mut Vec<LinkHit>,
    find: &mut crate::viewer::find::FindState,
    doc_generation: u64,
) {
    // A replaced document invalidates the last count (it belongs to the old
    // page); the fresh walk below recounts it before the bar reads it.
    if find.generation != doc_generation {
        find.generation = doc_generation;
        find.current = 0;
        find.total = 0;
        find.capped = false;
    }
    let find_render = if find.active() {
        Some(crate::viewer::find::FindRender::new(
            &find.query,
            find.current,
            find.scroll_to_current,
        ))
    } else {
        None
    };
    let mut ctx = RenderCtx {
        anchors,
        dests,
        pending_anchor,
        selected,
        interactive,
        dark: ui.visuals().dark_mode,
        viewport_w,
        type_scale,
        on_link,
        images,
        emoji,
        links,
        doc_salt,
        quote: 0,
        quote_blocks: 0,
        item: None,
        depth: 0,
        first_block: true,
        allow_skip: false,
        code_lines_left: crate::config::MAX_RENDERED_CODE_LINES,
        blocks_left: crate::config::MAX_RENDERED_BLOCKS,
        table_cells_left: crate::config::MAX_RENDERED_TABLE_CELLS,
        truncated: false,
        footnotes_started: false,
        unreferenced_footnotes: 0,
        pending_back_arrow: None,
        find: find_render,
    };
    let mut i = 0usize;
    // Footnote-reference memo generation: bumped only when the DOCUMENT
    // changes (its salt), not on every frame, so the "where was the
    // reference painted" rect written while walking the reference is still
    // valid when the back-arrow (which may sit EARLIER in the document) is
    // clicked on a later frame (see `viewer::inline`).
    crate::viewer::inline::begin_fnref_generation(doc_salt);
    while i < events.len() {
        render_block(ui, events, &mut i, &mut ctx);
    }
    if ctx.truncated {
        render_truncation_notice(ui);
    }
    // A target that no element claimed is dropped so it is not retried every
    // frame — but only when the document does not KNOW it. Content skipped
    // this pass (an unselected `<Tab>` panel, a closed `<Accordion>`, the
    // block budget) can hold the target legitimately: dropping it there made
    // a cross-page `…/page#heading-inside-a-tab` scroll nowhere, and the
    // request could never be applied when the reader later opened that tab.
    if let Some(target) = ctx.pending_anchor.as_deref() {
        if !anchors.contains(target) {
            *ctx.pending_anchor = None;
        }
    }
    // Hand the find count back to the tab (the bar reads it next frame, and
    // this frame already highlighted from the same ordinals).
    //
    // The scroll request is RETRIED until the current match shows up: a
    // target set on a frame the user is still scrolling (egui's wheel
    // smoothing keeps a scroll delta alive for a few frames) is dropped by
    // the `ScrollArea` — consuming the request on that first frame lost it
    // forever. A real user scroll (raw wheel/drag input) cancels the retry
    // for good: the user's scroll wins.
    if let Some(render) = ctx.find {
        find.total = render.next_ordinal;
        find.capped = render.capped;
        if find.total == 0 {
            find.current = 0;
            find.scroll_to_current = false;
        } else {
            if find.current >= find.total {
                // Wrapped (a shorter query after edits can strand the cursor
                // past the end): the first match is the sensible target.
                find.current = 0;
                find.scroll_to_current = true;
            }
            if find.scroll_to_current {
                let viewport = ui.clip_rect();
                let visible = render.current_rect.is_some_and(|r| viewport.intersects(r));
                // NEW user input this frame (a wheel gesture or a pointer
                // drag) means the user is driving the sheet: their scroll
                // wins and the retry stops. The wheel's internal SMOOTHING
                // tail (`is_scrolling`, which keeps eating the scroll target
                // for a few frames) does NOT cancel: the retry simply lands
                // once the tail decays.
                let user_scrolled = ui.ctx().input(|i| {
                    i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::MouseWheel { .. }))
                        || i.pointer.primary_down()
                });
                if visible || user_scrolled {
                    find.scroll_to_current = false;
                }
            }
        }
    } else {
        find.total = 0;
        find.capped = false;
        find.scroll_to_current = false;
    }
    // Blank margin at the very END of the document (`theme::spacing::PAGE_END`):
    // the reading sheet's gutter is symmetric, so a document scrolled to its
    // last line used to end with only that gutter (plus whatever trailing
    // block spacing) under the text. The pad is bottom-only, so the top
    // gutter and the scrolled-edge clipping stay unchanged.
    ui.add_space(theme::spacing::PAGE_END);
}

/// Test-only entry without find state (the bar is closed): existing
/// document tests render without searching.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_markdown(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    anchors: &AnchorIndex,
    dests: &crate::protocol::rewrite::DestCache,
    pending_anchor: &mut Option<String>,
    doc_salt: u64,
    selected: bool,
    interactive: bool,
    viewport_w: f32,
    type_scale: f32,
    images: &mut ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    on_link: &mut dyn FnMut(&str),
    links: &mut Vec<LinkHit>,
) {
    let mut find = crate::viewer::find::FindState::default();
    show_markdown_with_find(
        ui,
        events,
        anchors,
        dests,
        pending_anchor,
        doc_salt,
        selected,
        interactive,
        viewport_w,
        type_scale,
        images,
        emoji,
        on_link,
        links,
        &mut find,
        doc_salt,
    );
}

/// Block-kind renderers (paragraphs, headings, tables, HTML).
mod blocks;
mod budget;
/// Native MDX component blocks (`<Tabs>`, `<Accordion>`, `<Card>`, `<Steps>`).
mod components;
mod frags;
mod headings;
mod html;
mod items;
mod links;
mod menu;
mod tables;
/// Marks every fragment of a table header row `strong` once (at collection
/// time), so rendering never has to clone the row to style it.
mod walk;

// Every submodule sees the parent (imports included) and the parent sees
// every submodule: the renderer's pieces call each other in both
// directions (a block walk draws items, an item collects inline frags).
use blocks::*;
use budget::*;
use components::*;
use frags::*;
use headings::*;
use html::*;
use items::*;
use links::*;
use menu::*;
use tables::*;
use walk::*;

/// Short stable salt for egui widget ids, derived from the document URL.
/// Keeps `Grid` / scroll ids bounded (`md-table-{idx}-{salt}`) while avoiding
/// cross-document state leaks (same-sized tables on different pages no longer
/// share layout state). Uses FNV-1a (no new deps, deterministic).
pub(crate) fn doc_id_salt(base: &Url) -> u64 {
    crate::media::fnv1a(base.as_str())
}

/// egui id salt of a blockquote's inner indent (`ui.indent`).
///
/// Keyed by the block's own `Start(BlockQuote)` event index, never by a
/// per-pass counter: the index is stable whatever a frame happens to render,
/// so two different quotes can never alias onto one set of nested widget ids
/// when the visible block set changes (an unselected `<Tabs>` panel, a
/// closed accordion, the block budget). The nesting `level` keeps a nested
/// quote's ids distinct from its parent's; the document salt keeps two tabs
/// on the same document independent, like every other per-block id.
fn quote_indent_id(doc_salt: u64, level: usize, at: usize) -> egui::Id {
    egui::Id::new((doc_salt, "quote", level, at))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_docs;
#[cfg(test)]
mod tests_images;
#[cfg(test)]
mod tests_layout;
