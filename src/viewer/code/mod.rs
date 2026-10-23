// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::code`: fenced code blocks with lightweight highlighting.
//!
//! - One selectable label per line (via `LayoutJob`) so drag-select works
//!   and long lines scroll horizontally instead of wrapping.
//! - The concatenated body and its token spans are cached per
//!   (document salt, block EVENT index, language) in a BOUNDED multi-entry
//!   cache: repaints (scroll/selection) neither re-copy nor re-tokenize a
//!   block, and its laid-out line galleys are cached too, so they are not
//!   rebuilt per frame (a theme/selection change rebuilds them). The event
//!   index is stable per document, so switching a `<Tabs>` panel or opening
//!   an accordion (which changes what is rendered, not the document) can
//!   never hand one block another's cached body.
//! - The cache is keyed by a per-tab document GENERATION
//!   (`Tab::doc_generation`, mixed into the salt), bumped when the visible
//!   document is replaced, so reloading the same URL never renders or
//!   copies a stale body without flushing other tabs' cached blocks.
//! - Token spans are filled LAZILY: a block first rendered while the
//!   per-document line budget was spent is cached body-only and highlights
//!   on a later frame that can show it.
//! - The trailing newline pulldown-cmark appends to fenced blocks is
//!   stripped for display: no phantom empty last line.
//! - Syntax colors and the BODY monospace size (code measures exactly like
//!   the prose around it); no line-number gutter (READMEs
//!   do not show one).
//! - Huge blocks fall back to plain selectable lines to keep the UI
//!   responsive; `Copy` always copies the full body.
//! - Rendering is budgeted (`config::MAX_RENDERED_CODE_LINES` per document):
//!   a block past the budget shows a one-line "hidden lines" notice instead
//!   of millions of labels.

use crate::markdown::highlight as hl;
use crate::ui::theme;
use eframe::egui;
use pulldown_cmark::{Event, TagEnd};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Huge-block guard: above either bound the block renders as plain
/// selectable lines instead of per-line highlighted spans.
/// Single sources in `config::MAX_CODE_HIGHLIGHT_*`.
const MAX_LINES: usize = crate::config::MAX_CODE_HIGHLIGHT_LINES;
const MAX_BYTES: usize = crate::config::MAX_CODE_HIGHLIGHT_BYTES;

/// Code line height (px): the body line height, so a code card and the
/// prose around it share one row grid (the monospace family draws at the
/// body size: see `CODE_FONT_SIZE`). epaint rounds the laid-out row to a
/// whole physical pixel, so the PAINTED stride is read back from the layout
/// (see `render_body`) instead of assuming this token.
const CODE_LINE_HEIGHT: f32 = theme::BODY_LINE_HEIGHT;

thread_local! {
    /// Bounded per-document cache: concatenated body + token spans per
    /// (document salt, block event index, language). The salt identifies the
    /// document AND its generation on screen: a replaced document re-keys
    /// instead of flushing the whole cache.
    static BLOCKS: RefCell<HashMap<BlockKey, Rc<CachedBlock>>> =
        RefCell::new(HashMap::new());
    /// Approximate bytes currently held by `BLOCKS` (body + spans).
    static CACHED_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Monotonic use clock for the LRU eviction order.
    static CACHE_TICK: Cell<u64> = const { Cell::new(0) };
}

/// Cache key: document salt + the block's `Start(CodeBlock)` event index +
/// language.
type BlockKey = (u64, usize, hl::Lang);
/// Highlighted spans per line.
type SpanLines = Vec<Vec<hl::Span>>;

/// One cached block: the visible body and (when small/known) its spans.
struct CachedBlock {
    /// Body with the trailing fenced newline already stripped.
    body: String,
    /// Line total of `body` (1 for an empty block), computed once at cache
    /// insertion: `render_body` needs it every frame to apply the
    /// per-document line budget, and re-scanning a multi-MB body per frame
    /// wasted the whole point of caching it.
    line_count: usize,
    /// True when the block is small enough and its language is supported:
    /// [`Self::ensure_spans`] may still tokenize it after a cache hit.
    tokenizable: bool,
    /// Token spans per line, filled lazily (a block first seen while the
    /// per-document line budget was spent is cached body-only); `None` for
    /// huge/unsupported blocks, which render as plain selectable lines.
    lines: RefCell<Option<SpanLines>>,
    /// Laid-out galleys for the visible lines, keyed by (dark theme,
    /// selection, font size): rebuilding one `LayoutJob` per line per frame
    /// (with a string copy per span) was the remaining repaint cost on
    /// large blocks; the galleys are reused until one of those inputs
    /// changes.
    galleys: RefCell<Galleys>,
    /// Widest PLAIN row ever measured for this block (huge/unsupported
    /// blocks, which lay out only the rows on screen). Monotonic: the
    /// horizontal scroll range must not shrink when a vertical scroll
    /// moves a long line out of the viewport, or egui clamps the offset
    /// (and thereby the view) back to the left edge.
    plain_max_w: Cell<f32>,
    /// Approximate bytes retained by `galleys` (cache budget accounting).
    galley_bytes: Cell<usize>,
    /// Approximate retained bytes (accounting for the cache budget).
    bytes: usize,
    /// Last-use stamp ([`CACHE_TICK`]); the smallest is evicted first.
    used: Cell<u64>,
}

impl CachedBlock {
    /// Token spans for the visible lines, tokenizing on first NEED.
    ///
    /// The span lines are not part of the cache key and are not computed at
    /// insertion when the per-document line budget is spent: a block that
    /// was hidden then would otherwise render monochrome forever after a
    /// later frame (another `<Tabs>` panel selected, budget free) shows it.
    /// `tokenize` is false while no row of this block will be shown at all.
    /// `None` for huge/unsupported blocks (plain rendering).
    fn ensure_spans(
        &self,
        lang: hl::Lang,
        tokenize: bool,
    ) -> std::cell::Ref<'_, Option<SpanLines>> {
        if tokenize && self.lines.borrow().is_none() && self.tokenizable && hl::is_supported(lang) {
            let spans = self
                .body
                .split('\n')
                .map(|line| hl::spans(lang, line))
                .collect::<SpanLines>();
            *self.lines.borrow_mut() = Some(spans);
        }
        self.lines.borrow()
    }
}

/// Laid-out code lines for one block.
#[derive(Default)]
struct Galleys {
    /// `None` until the first layout; a mismatch clears `lines`.
    key: Option<(bool, bool, u32)>,
    /// One galley per visible line, in body order.
    lines: Vec<std::sync::Arc<egui::Galley>>,
    /// Widest laid-out line, so the horizontal scroll range covers every
    /// line (recomputing it from the visible rows only let the range shrink
    /// while a long line was scrolled out of view, clamping the offset).
    max_w: f32,
}

/// Approximate heap bytes of one laid-out galley: the owned text plus one
/// glyph struct per glyph and row metadata.
fn galley_heap_bytes(galley: &egui::Galley) -> usize {
    let glyphs: usize = galley.rows.iter().map(|row| row.row.glyphs.len()).sum();
    galley.text().len() + glyphs * 32 + galley.rows.len() * 48
}

/// Max cached blocks. Tied to the per-document render budget: a document
/// can request at most that many blocks per frame, so a sequential scan
/// never evicts a block it will ask for again next frame (the old fixed
/// 64-entry LRU re-copied and re-tokenized EVERY block of a larger
/// document, every frame). The byte budget below is the real memory bound.
const CACHE_MAX: usize = crate::config::MAX_RENDERED_BLOCKS;

/// Bumps and returns the LRU use clock (thread-local like the cache).
fn cache_tick() -> u64 {
    CACHE_TICK.with(|c| {
        let value = c.get().wrapping_add(1);
        c.set(value);
        value
    })
}

/// Max approximate bytes retained by the cache across all blocks.
/// ([`crate::config::MAX_CODE_CACHE_BYTES`]: a body alone can reach
/// `MAX_BODY_BYTES`, so a handful of giant blocks would otherwise sit in
/// RAM for the process lifetime.)
const CACHE_BYTES_MAX: usize = crate::config::MAX_CODE_CACHE_BYTES;

/// Approximate heap bytes of one span line set.
fn spans_bytes(lines: &SpanLines) -> usize {
    lines
        .iter()
        .map(|line| line.len() * std::mem::size_of::<hl::Span>())
        .sum::<usize>()
        + lines.len() * std::mem::size_of::<Vec<hl::Span>>()
}

/// Visible code content: the single trailing newline that fenced blocks
/// carry is removed so the renderer never shows a phantom last line.
/// Blank lines in the middle are preserved.
fn visible_body(buf: &str) -> &str {
    buf.strip_suffix('\n').unwrap_or(buf)
}

/// Line count of the visible body (1 for an empty block).
fn line_count(body: &str) -> usize {
    if body.is_empty() {
        1
    } else {
        body.as_bytes().iter().filter(|b| **b == b'\n').count() + 1
    }
}

/// Consumes one code block from `*i` (which must point just past its
/// `Start(CodeBlock)`) and returns the concatenated text with `*i` left ON
/// the `End(CodeBlock)`.
fn collect_body(events: &[Event<'static>], i: &mut usize) -> String {
    let mut buf = String::new();
    while *i < events.len() {
        match &events[*i] {
            Event::End(TagEnd::CodeBlock) => break,
            Event::Text(t) => {
                buf.push_str(t);
                *i += 1;
            }
            _ => *i += 1,
        }
    }
    buf
}

/// Advances `*i` past a code block WITHOUT copying its text (cache hit).
fn skip_body(events: &[Event<'static>], i: &mut usize) {
    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::CodeBlock)) {
        *i += 1;
    }
}

/// One block's body + spans, from the cache or freshly built.
///
/// On a cache hit the events are still consumed (the renderer walks the
/// stream once per frame) but the text is not copied again. `tokenize` is
/// false once the per-document line budget is spent: the body is still
/// collected (needed to consume the events) but tokenizing content that
/// will never be shown is wasted work; [`CachedBlock::ensure_spans`] fills
/// the spans later if a frame can show it after all.
///
/// `block_at` is the event index of this block's `Start(CodeBlock)`: stable
/// per document, so a render-order counter (which shifts with `<Tabs>`
/// selection and accordion state) cannot alias two blocks.
fn block_for(
    events: &[Event<'static>],
    i: &mut usize,
    lang: hl::Lang,
    doc_salt: u64,
    block_at: usize,
    tokenize: bool,
) -> Rc<CachedBlock> {
    let key = (doc_salt, block_at, lang);
    BLOCKS.with(|cell| {
        let mut map = cell.borrow_mut();
        if let Some(block) = map.get(&key) {
            // LRU refresh: keeps blocks actually on screen while scrolling.
            block.used.set(cache_tick());
            skip_body(events, i);
            return block.clone();
        }
        let raw = collect_body(events, i);
        let body = visible_body(&raw).to_string();
        let line_count = line_count(&body);
        let too_big = raw.len() > MAX_BYTES || line_count > MAX_LINES;
        let tokenizable = !too_big && hl::is_supported(lang);
        let lines = if tokenize && tokenizable {
            Some(
                body.split('\n')
                    .map(|line| hl::spans(lang, line))
                    .collect::<SpanLines>(),
            )
        } else {
            None
        };
        let bytes = body.len() + lines.as_ref().map_or(0, spans_bytes);
        // Evict least-recently-used entries in ONE pass (sorted by age):
        // the old `while min_by_key` loop re-scanned the whole map per
        // eviction — O(cap²) in one frame when a new generation refilled a
        // full cache. LAZY eviction, never arbitrary: a >CACHE_MAX-block
        // document would otherwise re-copy and re-tokenize every block on
        // every frame.
        let slots_needed = map.len().saturating_add(1).saturating_sub(CACHE_MAX);
        let bytes_deficit = CACHED_BYTES
            .with(Cell::get)
            .saturating_add(bytes)
            .saturating_sub(CACHE_BYTES_MAX);
        evict_lru(&mut map, slots_needed, bytes_deficit);
        CACHED_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
        let block = Rc::new(CachedBlock {
            body,
            line_count,
            tokenizable,
            lines: RefCell::new(lines),
            galleys: RefCell::new(Galleys::default()),
            plain_max_w: Cell::new(0.0),
            galley_bytes: Cell::new(0),
            bytes,
            used: Cell::new(cache_tick()),
        });
        map.insert(key, block.clone());
        block
    })
}

/// Evicts least-recently-used blocks until `slots` entries and
/// `bytes_deficit` bytes have been freed (one sorted pass; the old
/// `while min_by_key` rescan was O(cap²) inside a single frame).
/// A block's held bytes include its laid-out galleys, so this reclaims
/// them too. The pass itself is shared (`viewer::cache::evict_oldest`).
fn evict_lru(map: &mut HashMap<BlockKey, Rc<CachedBlock>>, slots: usize, bytes_deficit: usize) {
    let aged: Vec<(u64, BlockKey)> = map
        .iter()
        .map(|(key, block)| (block.used.get(), *key))
        .collect();
    super::cache::evict_oldest(aged, slots, bytes_deficit, |key| {
        map.remove(&key).map(|old| {
            let held = old.bytes.saturating_add(old.galley_bytes.get());
            CACHED_BYTES.with(|total| total.set(total.get().saturating_sub(held)));
            held
        })
    });
}

/// Height of one plain code row: the first visible line's styled
/// `LayoutJob` measured once (the block's rows share one style, so one
/// measurement is exact even for an empty line), falling back to the
/// design token when the line does not lay out at all.
fn plain_row_height(ui: &mut egui::Ui, first_line: &str, selected: bool, s: &theme::Scheme) -> f32 {
    let text = if first_line.is_empty() {
        " "
    } else {
        first_line
    };
    let mut rt = egui::RichText::new(text)
        .monospace()
        .line_height(Some(CODE_LINE_HEIGHT));
    if selected {
        rt = theme::selected_text(ui, rt);
    } else {
        rt = rt.color(s.on_surface);
    }
    let mut job = std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(rt).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::LEFT,
    ));
    job.wrap.max_width = f32::INFINITY;
    let h = ui.fonts_mut(|fonts| fonts.layout_job(job)).size().y;
    if h > 0.0 {
        h
    } else {
        CODE_LINE_HEIGHT
    }
}

/// Plain monospace lines (huge or unsupported blocks).
///
/// Virtualized: only the rows inside `visible` are built and painted at the
/// uniform `row_h` grid; the rest of the block's height comes from the
/// caller's reserved content rect. Returns the widest row EVER seen for the
/// block (stored monotonically in `block.plain_max_w`), which the caller
/// adds to the scroll content so long lines stay reachable: recomputing it
/// from the visible rows alone let the range shrink as soon as a long line
/// scrolled out of view, and egui then clamped the horizontal offset back.
/// A 10 000-line log used to build one `LayoutJob` and one `Label` per row
/// per frame while ~20 rows were on screen.
fn render_plain_lines(
    ui: &mut egui::Ui,
    block: &CachedBlock,
    selected: bool,
    visible: std::ops::Range<usize>,
    row_h: f32,
    find_query: Option<&str>,
    find_current_row: Option<usize>,
) -> f32 {
    let mut widest = 0.0_f32;
    let find_bg = find_query.is_some().then(|| theme::find_match_bg(ui));
    // The current match's row gets the stronger color (see `render_body`).
    let find_current_bg = find_current_row
        .is_some()
        .then(|| theme::find_current_bg(ui));
    for (row, line) in block
        .body
        .split('\n')
        .enumerate()
        .take(visible.end)
        .skip(visible.start)
    {
        let text = if line.is_empty() { " " } else { line };
        let mut rt = egui::RichText::new(text)
            .monospace()
            .line_height(Some(CODE_LINE_HEIGHT));
        if selected {
            rt = theme::selected_text(ui, rt);
        } else {
            rt = rt.color(theme::scheme_ref(ui).on_surface);
        }
        let mut job = std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(rt).into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::LEFT,
        ));
        // Unwrapped: one code line must stay one row, however long it is.
        job.wrap.max_width = f32::INFINITY;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        widest = widest.max(galley.size().x);
        // ABSOLUTE row offset: the content origin already carries the scroll
        // translation, so a scrolled block must paint each visible row at
        // its own row position (`origin + row * row_h`). The relative
        // `row - visible.start` painted the visible rows at the block TOP,
        // leaving the scrolled viewport blank.
        let y = ui.max_rect().top() + row as f32 * row_h;
        if let (Some(query), Some(bg)) = (find_query, find_bg) {
            if crate::viewer::find::count_occurrences(line, query) > 0 {
                let bg = if find_current_row == Some(row) {
                    find_current_bg.unwrap_or(bg)
                } else {
                    bg
                };
                let rect = egui::Rect::from_min_size(
                    egui::pos2(ui.max_rect().left(), y),
                    egui::vec2(ui.max_rect().width().max(1.0), row_h),
                );
                ui.painter().rect_filled(rect, 0.0, bg);
            }
        }
        ui.painter().galley(
            egui::pos2(ui.max_rect().left(), y),
            galley,
            ui.visuals().text_color(),
        );
    }
    // Monotonic across frames and scroll positions (see the doc comment).
    let widest = widest.max(block.plain_max_w.get());
    block.plain_max_w.set(widest);
    widest
}

/// How long a code card's Copy button confirms the copy (`Copied`) before
/// returning to its idle label. Matches the shared toast's ~2s lifetime.
const COPY_CONFIRMATION: Duration = Duration::from_secs(2);

/// Reserved width of the Copy button: the confirmation label ("Copied") is
/// wider than the idle one ("Copy"), and a changing width would shift the
/// language chip on every copy. 64px fits both at the small button text size
/// and the 12px horizontal padding.
const COPY_BUTTON_WIDTH: f32 = 64.0;

/// Accessible/tooltip label for a code block's Copy button (`"Copy code"`,
/// or `"Copy <lang> code"` so N blocks do not list N identical nodes).
fn copy_a11y(chip: &str) -> String {
    if chip.is_empty() {
        "Copy code".to_string()
    } else {
        format!("Copy {chip} code")
    }
}

/// Same label after a successful copy, so screen readers and the hover card
/// report the action's outcome instead of repeating the idle command.
fn copied_a11y(chip: &str) -> String {
    if chip.is_empty() {
        "Copied code".to_string()
    } else {
        format!("Copied {chip} code")
    }
}

/// Temporary-data key for one block's Copy confirmation. Salted by document
/// and block event index like the block cache, so two tabs (or two blocks)
/// never share a confirmation timer.
fn copy_feedback_id(doc_salt: u64, block_at: usize) -> egui::Id {
    egui::Id::unique(("code-copy-feedback", doc_salt, block_at))
}

/// Consumes one fenced code block (from just past its `Start` to its `End`)
/// and draws the code card. Body and spans come from the bounded
/// per-document cache, so repaints never re-copy or re-tokenize the block.
/// `budget` is the remaining per-document line allowance. `block_at` is the
/// event index of the block's `Start`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_code_block(
    ui: &mut egui::Ui,
    events: &[Event<'static>],
    i: &mut usize,
    lang_info: &str,
    selected: bool,
    doc_salt: u64,
    block_at: usize,
    budget: &mut usize,
    find: &mut Option<crate::viewer::find::FindRender>,
) {
    let lang = hl::norm_lang(lang_info);
    // Past the per-document line budget nothing will be shown: collect the
    // body (to advance the event stream) but skip tokenization.
    let tokenize = *budget > 0;
    let block = block_for(events, i, lang, doc_salt, block_at, tokenize);
    code_card(
        ui, &block, lang, lang_info, selected, block_at, doc_salt, budget, find,
    );
}

/// The code card itself: language chip + Copy + horizontal scroll.
#[allow(clippy::too_many_arguments)]
fn code_card(
    ui: &mut egui::Ui,
    block: &CachedBlock,
    lang: hl::Lang,
    lang_info: &str,
    selected: bool,
    block_at: usize,
    doc_salt: u64,
    budget: &mut usize,
    find: &mut Option<crate::viewer::find::FindRender>,
) {
    ui.add_space(theme::spacing::S);
    // Code block: `#f6f8fa` surface, no border, 6px corners, 16px padding
    // (see `theme::code_card`); a subtle header with
    // the language and a Copy button (a plain `pre` shows no header, but the
    // copy affordance stays). Full reading-column width.
    let avail = ui.available_width().max(0.0);
    ui.set_min_width(avail);
    theme::code_card(ui).show(ui, |ui| {
        // Card inner width: `code_card` pads `CODE_PAD_X` per side; leave
        // 2px of slack for the scroll area's rounding.
        ui.set_min_width((avail - 2.0 * theme::CODE_PAD_X - 2.0).max(0.0));
        // The idle "Copy" and confirmation "Copied" labels share one reserved
        // width: changing the button's size for two seconds would otherwise
        // shift the language chip left on every copy.
        let copy_id = copy_feedback_id(doc_salt, block_at);
        let copied_at = ui.ctx().data(|d| d.get_temp::<Instant>(copy_id));
        let copied = match copied_at {
            Some(at) => {
                let elapsed = at.elapsed();
                if elapsed < COPY_CONFIRMATION {
                    // Wake up exactly when the confirmation expires, so the
                    // label returns even while the app is otherwise idle.
                    ui.ctx().request_repaint_after(COPY_CONFIRMATION - elapsed);
                    true
                } else {
                    false
                }
            }
            None => false,
        };
        ui.horizontal(|ui| {
            let s = theme::scheme_ref(ui);
            // `lang_info` is ALREADY bounded by the caller
            // (`markdown::render_block` truncates to `config::MAX_LANG_CHARS`
            // once); re-truncating here allocated a String per block per
            // frame for no benefit. Lowercase without allocating when the
            // tag is already lowercase (the common case).
            let lowered;
            let chip: &str = if lang_info.bytes().any(|b| b.is_ascii_uppercase()) {
                lowered = lang_info.to_ascii_lowercase();
                &lowered
            } else {
                lang_info
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // `small()` keeps the compact typography but zeroes the
                // vertical padding AND skips the `interact_size` floor: an
                // explicit 24px floor keeps the primary click target from
                // being a ~17px sliver next to 30-36px controls.
                // Accessible name carries the language: a page with N code
                // blocks listed N identical "Copy" buttons otherwise. Both
                // built lazily (`widget_info` materializes only on
                // interaction, the tooltip only on hover), so a settled
                // frame allocates nothing here.
                let copy = ui.add(
                    egui::Button::new(if copied { "Copied" } else { "Copy" })
                        .small()
                        .min_size(egui::vec2(COPY_BUTTON_WIDTH, theme::BODY_LINE_HEIGHT)),
                );
                let copy = crate::ui::hover_tooltip_lazy(copy, || {
                    if copied {
                        copied_a11y(chip)
                    } else {
                        copy_a11y(chip)
                    }
                });
                copy.widget_info(|| {
                    let label = if copied {
                        copied_a11y(chip)
                    } else {
                        copy_a11y(chip)
                    };
                    egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                });
                if copy
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    // The cached body (no trailing newline): one copy, no
                    // per-frame clone.
                    ui.ctx().copy_text(block.body.clone());
                    // Confirm on THIS card only (the id carries the document
                    // salt and block index); the next frame paints "Copied"
                    // and schedules its own return to "Copy".
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(copy_id, Instant::now()));
                    ui.ctx().request_repaint_after(COPY_CONFIRMATION);
                }
                // The language chip fills the space LEFT of Copy and
                // truncates there: laid out before Copy it could push the
                // button past the card edge on a narrow sheet. The full
                // (bounded) info string stays in the hover tooltip.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let chip_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(chip)
                                .small()
                                .color(s.on_surface_variant),
                        )
                        .truncate(),
                    );
                    crate::ui::hover_tooltip_lazy(chip_resp, || lang_info.to_owned());
                });
            });
        });
        ui.add_space(theme::spacing::XXS);
        // Horizontal scroll for long lines, virtualized on the vertical
        // axis. Id salted by document (tab included) + the block's event
        // index, so two blocks of one document never share scroll state and
        // a block keeps its offset across `<Tabs>` switches.
        egui::ScrollArea::horizontal()
            .id_salt(("code-scroll", block_at, doc_salt))
            .auto_shrink([false, false])
            // The bar gets its own lane under the lines (the card's bottom
            // padding is 4px, so the outer height stays 16px below the code):
            // an overlay bar used to cover the last line while scrolling.
            .content_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 0,
                bottom: theme::scrollbar_lane(ui),
            })
            .show(ui, |ui| {
                render_body(ui, block, lang, selected, budget, find);
            });
    });
    ui.add_space(theme::spacing::M);
}

/// Draws the cached block: highlighted spans when available, plain
/// monospace lines for huge/unsupported blocks. Rendering is
/// capped by the per-document line budget; the remainder is one notice.
///
/// Only the rows the scroll viewport shows are built (see
/// [`code_viewport`]): the widget count per frame no longer scales with the
/// block's line count. Highlighted rows are painted galleys, so per-line
/// drag selection is gone inside a highlighted block; the Copy button (or
/// select-all + Ctrl/Cmd+C) copies the whole body.
fn render_body(
    ui: &mut egui::Ui,
    block: &CachedBlock,
    lang: hl::Lang,
    selected: bool,
    budget: &mut usize,
    find: &mut Option<crate::viewer::find::FindRender>,
) {
    let total = block.line_count;
    let shown = total.min(*budget);
    *budget = budget.saturating_sub(shown);
    // In-page find claims the block's occurrences up front (document order),
    // highlights visible matching rows with the amber wash and scrolls the
    // current match's row into view on navigation frames only.
    let (find_start, find_n, find_scroll) = match find.as_mut() {
        Some(fr) if !fr.query_lower.is_empty() => {
            // Pre-gate: a non-matching block skips the whole-body lowercase
            // + `Vec` (the body can be kilobytes of code).
            if !crate::viewer::find::contains_match(&block.body, &fr.query_lower) {
                (0, 0, false)
            } else {
                let (ranges, block_capped) =
                    crate::viewer::find::find_ranges_capped(&block.body, &fr.query_lower);
                let n = ranges.len();
                if n == 0 {
                    (0, 0, false)
                } else {
                    let start = fr.claim(n);
                    let claimed = fr.next_ordinal - start;
                    if block_capped && claimed == n {
                        // More occurrences than the per-block cap allows: the
                        // bar shows `N+` even when the GLOBAL cap was not hit.
                        fr.capped = true;
                    }
                    (start, claimed, fr.scroll)
                }
            }
        }
        _ => (0, 0, false),
    };
    let find_active = find_n > 0;
    // Borrow the lowercased query (no per-block `String` clone per frame):
    // `fr` outlives this frame's paint, so the `&str` stays valid.
    let find_query: Option<&str> = find
        .as_ref()
        .and_then(|fr| find_active.then_some(fr.query_lower.as_str()));
    let s = theme::scheme_ref(ui);
    // Spans first (filled lazily after a body-only cache hit), then the
    // ROW STRIDE: epaint rounds every row height to a whole device pixel,
    // so the painted rows sit on `row_h` while the design token only states
    // the layout height. The viewport math, the reserved height and the
    // painted offsets must all use `row_h`, or a long block scrolls into an
    // unpainted band.
    let spans = block.ensure_spans(lang, shown > 0);
    let lines = spans.as_deref();
    let sel_bg = selected.then_some(theme::select_bg(ui));
    let sel_fg = selected.then_some(s.on_surface);
    let row_h = match lines {
        Some(lines) => {
            // Laid-out galleys live in the block cache: a repaint only clones
            // an `Arc` per line instead of rebuilding a `LayoutJob` (and
            // copying every span's text) per line per frame.
            ensure_galleys(ui, block, lines, shown, selected, sel_bg, sel_fg, *s);
            let cache = block.galleys.borrow();
            cache
                .lines
                .first()
                .map_or(CODE_LINE_HEIGHT, |galley| galley.size().y)
        }
        None => plain_row_height(ui, block.body.split('\n').next().unwrap_or(""), selected, s),
    };
    let row_h = row_h.max(1.0);
    let visible = code_viewport(ui, shown, row_h);
    // The row that owns the CURRENT match when this block claims it: its
    // wash uses the stronger current color (every other match keeps the
    // plain amber) and, on navigation frames, the row is scrolled into view.
    // Found once per frame by counting occurrences line by line; the scan
    // stops as soon as the target ordinal is passed. `find_start`/`find_n`
    // are this block's claimed ordinals. The row rect is kept for the
    // caller's visibility retry (recorded on the find cursor after the
    // painting borrows end below).
    let mut current_row: Option<usize> = None;
    let mut current_row_rect: Option<egui::Rect> = None;
    if let (Some(query), Some(fr)) = (find_query, find.as_ref()) {
        let current = fr.current;
        if current >= find_start && current < find_start + find_n {
            let mut ordinal = find_start;
            for (idx, line) in block.body.split('\n').enumerate() {
                let n = crate::viewer::find::count_occurrences(line, query);
                if current < ordinal + n {
                    current_row = Some(idx);
                    let y = ui.max_rect().top() + idx as f32 * row_h;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), y),
                        egui::vec2(ui.max_rect().width().max(1.0), row_h),
                    );
                    current_row_rect = Some(rect);
                    if find_scroll {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                    break;
                }
                ordinal += n;
                if ordinal > current {
                    break;
                }
            }
        }
    }
    // Painted galleys register no layout, so the content rect must be
    // widened/heightened explicitly: without this the horizontal scroll area
    // sees a content width of zero and a long line could never be reached.
    let find_bg = find_active.then(|| theme::find_match_bg(ui));
    let find_current_bg = find_active.then(|| theme::find_current_bg(ui));
    let content_w = if lines.is_none() {
        render_plain_lines(
            ui,
            block,
            selected,
            visible.clone(),
            row_h,
            find_query,
            current_row,
        )
    } else {
        let cache = block.galleys.borrow();
        // The widest laid-out line covers every row, not just the visible
        // ones: recomputing from the viewport let the scroll range (and the
        // clamped offset) shrink while a long line was above/below it.
        // Paint the visible slice directly (the old `take(end).skip(start)`
        // walked every row scrolled past on each frame).
        //
        // Find wash for visible matching rows (painted BEFORE the galley so
        // the text stays on top): line text comes from the cached body, so
        // the galley cache stays find-free. `contains_match` stops at the
        // first hit, and the line iterator is consumed inline: the old
        // `match_lines: Vec<bool>` allocated per block per frame.
        let mut match_iter = find_query.map(|query| {
            block
                .body
                .split('\n')
                .skip(visible.start)
                .map(move |line| crate::viewer::find::contains_match(line, query))
        });
        for (row, galley) in cache
            .lines
            .get(visible.start..visible.end)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let offset = visible.start + row;
            let y = ui.max_rect().top() + offset as f32 * row_h;
            let matched = match_iter
                .as_mut()
                .and_then(|it| it.next())
                .unwrap_or(false);
            if matched {
                let bg = if current_row == Some(offset) {
                    find_current_bg
                } else {
                    find_bg
                };
                if let Some(bg) = bg {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), y),
                        egui::vec2(cache.max_w.max(1.0), row_h),
                    );
                    ui.painter().rect_filled(rect, 0.0, bg);
                }
            }
            ui.painter().galley(
                egui::pos2(ui.max_rect().left(), y),
                std::sync::Arc::clone(galley),
                s.on_surface,
            );
        }
        cache.max_w
    };
    let content_h = shown as f32 * row_h;
    let content_rect =
        egui::Rect::from_min_size(ui.max_rect().left_top(), egui::vec2(content_w, content_h));
    ui.expand_to_include_rect(content_rect);
    // Hand this block's current-match row to the caller's visibility retry
    // (`ShowMarkdown`'s tail): done HERE, after every immutable borrow of
    // the find cursor above.
    if let Some(rect) = current_row_rect {
        if let Some(fr) = find.as_mut() {
            fr.current_rect = Some(rect);
        }
    }
    if shown < total {
        // The rows above are PAINTED, not laid out: the layout cursor is
        // still at the block's top, so the notice must be advanced past the
        // reserved height or it lands over the first visible line (it used
        // to cover row 0 of any truncated block).
        ui.advance_cursor_after_rect(content_rect);
        hidden_lines_notice(ui, total - shown);
    }
}

/// Reserves the full height of `shown` code rows and returns the row range
/// the scroll viewport actually shows. `stride` is the PAINTED row height
/// (see [`render_body`]).
///
/// One row is one measured code line tall: each row is painted at a
/// computed offset instead of being stacked by the vertical layout, so the
/// virtual height above and below the viewport cannot drift from the
/// painted rows (and the whole block stays navigable). The reserved height
/// keeps the outer card's size exact for the line budget and the sibling
/// widgets below.
fn code_viewport(ui: &mut egui::Ui, shown: usize, stride: f32) -> std::ops::Range<usize> {
    ui.set_min_height(shown as f32 * stride);
    // The scroll viewport in content coordinates (its `min` is the scroll
    // offset), as `ScrollArea::show_rows` uses it: the content rect is
    // built from the scroll area's viewport minus the offset, so the
    // visible rows are the ones between `viewport.min.y` and
    // `viewport.max.y` relative to the content origin
    // (`ui.max_rect().top()`, which already carries the `-offset`
    // translation). The clip rect bounds it vertically: a test harness has
    // no real window, and without the bound the whole block would count as
    // visible (the outer scroll areas shrink the clip rect, so production
    // clips there too).
    //
    // The bound is the WINDOW-LOCAL viewport rect. `ViewportInfo::inner_rect`
    // is in MONITOR space (winit reports the window position there), so
    // intersecting it with the local clip rect shifted the bound by the
    // window's position on screen and culled rows early.
    let window = ui.ctx().input(|i| i.viewport_rect());
    let viewport = ui.clip_rect().intersect(window);
    let top = (viewport.min.y - ui.max_rect().top()).max(0.0);
    let first = (top / stride).floor() as usize;
    let visible_rows = ((viewport.height() / stride).ceil() as usize) + 1;
    let start = first.min(shown);
    let end = start.saturating_add(visible_rows).min(shown);
    start..end
}

/// Code font size: the body size, so code is exactly as wide as the prose
/// around it (the user-facing contract) and shares its 24px row grid.
/// MUST stay equal to `TextStyle::Monospace` and `TextStyle::Body` in
/// `ui::theme::apply_spacing_type`: pills and code cards share one
/// monospace scale. `theme::BODY_FONT_SIZE` is the single source.
const CODE_FONT_SIZE: f32 = theme::BODY_FONT_SIZE;

/// Builds (or reuses) the laid-out galleys for the visible lines.
///
/// A galley embeds colors, font and selection background, so the cache is
/// rebuilt whenever one of those inputs changes: dark/light theme,
/// select-all, or the code font size (zoom changes pixels per point, not
/// layout points, so it needs no re-layout). `shown` only grows within a
/// document, so a previously built prefix is kept.
#[allow(clippy::too_many_arguments)]
fn ensure_galleys(
    ui: &mut egui::Ui,
    block: &CachedBlock,
    lines: &[Vec<hl::Span>],
    shown: usize,
    selected: bool,
    sel_bg: Option<egui::Color32>,
    sel_fg: Option<egui::Color32>,
    s: theme::Scheme,
) {
    let font = egui::FontId::monospace(CODE_FONT_SIZE);
    let key = (ui.visuals().dark_mode, selected, font.size.to_bits());
    let mut cache = block.galleys.borrow_mut();
    if cache.key == Some(key) && cache.lines.len() >= shown {
        return;
    }
    // Same key: `shown` only grows within a document (a `<Tabs>` switch, an
    // accordion opening or a closed block budget can hand this block more
    // lines than the frame that built the cache), so keep the laid-out
    // prefix and append only the missing tail. Rebuilding every galley made
    // a large block re-lay-out entirely for one extra line.
    let keep = cache.key == Some(key);
    if !keep {
        // Release the old accounting before the old galleys drop.
        let owned = block.galley_bytes.get();
        if owned > 0 {
            CACHED_BYTES.with(|total| total.set(total.get().saturating_sub(owned)));
            block.galley_bytes.set(0);
        }
        cache.lines.clear();
        cache.max_w = 0.0;
    }
    cache.key = Some(key);
    let start = cache.lines.len();
    let old_bytes = block.galley_bytes.get();
    let empty: Vec<hl::Span> = Vec::new();
    let mut bytes = old_bytes;
    let mut max_w = cache.max_w;
    for (idx, line) in block.body.split('\n').take(shown).enumerate().skip(start) {
        let spans = lines.get(idx).unwrap_or(&empty);
        let job = line_job(line, spans, &font, sel_bg, sel_fg, s);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        bytes = bytes.saturating_add(galley_heap_bytes(&galley));
        max_w = max_w.max(galley.size().x);
        cache.lines.push(galley);
    }
    cache.max_w = max_w;
    let delta = bytes.saturating_sub(old_bytes);
    block.galley_bytes.set(bytes);
    CACHED_BYTES.with(|total| total.set(total.get().saturating_add(delta)));
    // Galleys are added on cache HITS (theme/select-all change rebuilds
    // every visible block), so the miss-time eviction above never sees
    // them: enforce the byte budget here too, evicting LRU blocks until the
    // new galleys fit.
    let over = CACHED_BYTES.with(Cell::get).saturating_sub(CACHE_BYTES_MAX);
    if over > 0 {
        BLOCKS.with(|cell| evict_lru(&mut cell.borrow_mut(), 0, over));
    }
}

/// One code line's `LayoutJob` (only built on a galley-cache miss).
fn line_job(
    line: &str,
    spans: &[hl::Span],
    font: &egui::FontId,
    sel_bg: Option<egui::Color32>,
    sel_fg: Option<egui::Color32>,
    s: theme::Scheme,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    if line.is_empty() {
        // Keep the blank line's height.
        let fg = sel_fg.unwrap_or(s.on_surface);
        let mut fmt = egui::text::TextFormat::simple(font.clone(), fg);
        fmt.line_height = Some(CODE_LINE_HEIGHT);
        if let Some(bg) = sel_bg {
            fmt.background = bg;
        }
        job.append(" ", 0.0, fmt);
    }
    for span in spans {
        // Defensive slice: a buggy highlighter range must never panic
        // the UI thread. `get` returns `None` on bad bounds.
        let text = line.get(span.start..span.end).unwrap_or("");
        let color = if let Some(fg) = sel_fg {
            fg
        } else {
            match span.kind {
                hl::TokenKind::Keyword => s.code_keyword,
                hl::TokenKind::Str => s.code_string,
                hl::TokenKind::Comment => s.code_comment,
                hl::TokenKind::Number => s.code_number,
                hl::TokenKind::Func => s.code_func,
                hl::TokenKind::Plain => s.on_surface,
            }
        };
        let mut fmt = egui::text::TextFormat::simple(font.clone(), color);
        fmt.line_height = Some(CODE_LINE_HEIGHT);
        if span.kind == hl::TokenKind::Comment {
            fmt.italics = true;
        }
        if let Some(bg) = sel_bg {
            fmt.background = bg;
        }
        job.append(text, 0.0, fmt);
    }
    job
}

/// One-line truncation notice for code lines past the render budget.
fn hidden_lines_notice(ui: &mut egui::Ui, hidden: usize) {
    let s = theme::scheme_ref(ui);
    let plural = if hidden == 1 { "" } else { "s" };
    ui.label(
        egui::RichText::new(format!("... {hidden} more line{plural} hidden"))
            .small()
            .italics()
            .color(s.on_surface_variant),
    );
}

#[cfg(test)]
mod tests;
