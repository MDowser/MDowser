// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document`: CommonMark parser options.
//!
//! ENABLED (requirement):
//! - Base CommonMark (always).
//! - Table extension (`ENABLE_TABLES`).
//! - `[^N]` footnotes (`ENABLE_FOOTNOTES`, the current footnote syntax).
//! - Strikethrough + tasklists (Markdown extensions; harmless and useful).
//! - `+++` metadata blocks: parsed so the viewer can SKIP them
//!   (never rendered, never copied).
//! - Heading attributes (`{#id}`): parsed so they don't leak as text.
//! - Smart punctuation OFF (never alters the original text).
//!
//! YAML `---` METADATA BLOCKS ARE DELIBERATELY DISABLED:
//! - Frontmatter detection/removal happens once in
//!   `markdown::frontmatter::split` BEFORE parsing.
//! - A leading `---` that `split` decided is NOT frontmatter (no closer, or
//!   a block without a `:` mapping) must render as CommonMark content
//!   (thematic break + setext heading/paragraph). Enabling YAML metadata
//!   blocks here would silently swallow that content.
//!
//! EMOJI:
//! - `:shortcode:` text is expanded here (never inside code spans or
//!   code blocks) by `markdown::emoji`.
//!
//! BARE URLS:
//! - `www.example.com` / `https://example.com` typed as plain text are
//!   linkified after parsing by `markdown::autolink`, so they render like
//!   markdown links. Bare domains (`celticos.com`) linkify too when their
//!   last label is a real IANA root-zone TLD (`markdown::tld`), with a
//!   file-name guard for extension-like TLDs (`config.rs`, `README.md`
//!   stay text), and local `file:` URLs linkify as local files. No other
//!   scheme does (`http:`, `mailto:`, `ftp:`, `data:` stay text), and
//!   emails never link. Code, metadata, existing links and image alt text
//!   are never touched.
//!
//! MDX AND RAW TAGS (`normalize_markup`):
//! - MDX is never EXECUTED (no JavaScript), but its STRUCTURE is recognized
//!   before parsing: known callout components (`<Note>`, `<Warning>`,
//!   `<Callout type=… title=…>`, …) become alert blockquotes, and every
//!   other tag (JSX or HTML: `<div>`, `<a>`, `<h1>`, …) is a TRANSPARENT
//!   wrapper — the tag is dropped and its content parses as Markdown.
//!   `<br>` stays a break and `<img src>` becomes a Markdown image.
//! - Structural components (`<Tabs>`/`<Tab>`, `<Accordion>`, `<Card>`,
//!   `<Steps>`/`<Step>`, `<CodeGroup>`) become canonical `<mdowser-cmp …/>`
//!   markers (one HTML block each) that `viewer::markdown::components` draws
//!   natively; inline `<Badge>` becomes a `<mdowser-badge>` pill and
//!   `<Frame>`/`<Tiles>` are transparent wrappers. The markers are
//!   structure, never text: `plain_text` skips them.
//! - In an MDX document (`import`/`export` or a capitalized component), ESM
//!   statements and `{…}` expressions are removed (they cannot be
//!   evaluated); heading attributes (`{#id}`) survive.
//! - `<script>`/`<style>` regions are dropped; `<pre>`/`<textarea>` stay
//!   inert verbatim (their bytes are not Markdown). Code spans and blocks
//!   are never touched.
//! - HTML comments (`<!-- ... -->`) are INVISIBLE instead: neither the
//!   viewer nor `plain_text` (clipboard) shows a comment, block or
//!   inline, exactly like a browser.
//!
//! Color emoji ARE supported for text nodes (`media::emoji` rasterizes the
//! embedded Noto Color Emoji into inline textures); code spans/blocks and
//! SVG text keep the monochrome fallback.
//!
//! The implementation is split by concern:
//! - [`normalize`]: the pre-parse repair pass (line endings, link joins,
//!   `:::` admonitions + structural custom containers, MDX/HTML markup,
//!   table-delimiter padding,
//!   table-bomb guard) shared by the viewer and the plain-text path.
//! - [`plain`]: the whole-page Copy text, derived from the same normalized
//!   events (never a second parse).
//!
//! This file keeps the parser options, the event walk ([`parse_events`])
//! and the shared HTML-break check.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Returns `true` for HTML line breaks (`<br>`, `<br/>`, `<br />`, `</br>`,
/// any letter case, optional attributes).
///
/// Single home for the break check so the viewer and `plain_text`
/// (clipboard) always agree. Non-break tags (`<kbd>`, `<bracket>`, ...)
/// return `false` and are stripped to a space by callers.
pub fn is_html_break(html: &str) -> bool {
    fn delim_ok(rest: &str) -> bool {
        match rest.chars().next() {
            Some('>') | Some('/') => true,
            Some(c) => c.is_ascii_whitespace(),
            None => false,
        }
    }

    // Allocation-free case-insensitive prefixes (`to_ascii_lowercase` ran on
    // every HTML event of every frame, most of them not breaks).
    let s = html.trim();
    if s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("<br")) {
        return delim_ok(&s[3..]);
    }
    if s.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("</br")) {
        return delim_ok(&s[4..]);
    }
    false
}

/// Returns `true` for an HTML comment (`<!-- ... -->`).
///
/// Comments are invisible in a browser, so the viewer paints nothing and
/// `plain_text` (clipboard) drops them: the block form (a line starting
/// with `<!--`, up to 3 spaces of indent) and the inline form
/// (`foo<!-- x -->bar`) both. An unterminated comment block counts too:
/// CommonMark feeds the rest of the document to the block and a browser
/// hides it, so MDowser does the same.
///
/// Literal `<!--` inside code spans or fenced/indented code blocks arrives
/// as `Event::Text`/`Event::Code`, never here: code keeps its source.
pub fn is_html_comment(html: &str) -> bool {
    html.trim_start().starts_with("<!--")
}

/// Parser options shared by the whole browser.
/// - Centralizing here keeps UI and tests from diverging.
/// - YAML `---` metadata blocks stay OFF: `frontmatter::split` already
///   removed real frontmatter, and anything left must render as content
///   (see the module docs).
/// - `+++` metadata blocks are ENABLED so they parse as
///   `Tag::MetadataBlock`; both the viewer and `plain_text` explicitly SKIP
///   them (never rendered).
/// - Heading attributes are ENABLED so `{#id .cls}` doesn't leak as text.
/// - `ENABLE_GFM` stays OFF: with it, `> [!NOTE]` markers are consumed by
///   the parser and the manual callout badges in `viewer` would break.
///   The GFM autolink extension is not part of the parser at all: bare
///   `www.` / `https://` / local `file:` URLs are added by
///   `markdown::autolink` as a post-parse pass over TEXT nodes.
pub fn parser_options() -> Options {
    let mut o = Options::empty();
    o.insert(Options::ENABLE_TABLES);
    o.insert(Options::ENABLE_FOOTNOTES);
    o.insert(Options::ENABLE_STRIKETHROUGH);
    o.insert(Options::ENABLE_TASKLISTS);
    o.insert(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS);
    o.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    o
}

/// Parses a normalized body into OWNED events (`'static`).
///
/// - Runs once per fetch (background thread), never per repaint.
/// - The viewer renders from the event list directly, so scrolling a large
///   document no longer re-runs `pulldown-cmark` every frame.
/// - Owned events keep no borrow of the body (safe to store in `Fetched`).
/// - `:shortcode:` emoji are expanded here for TEXT nodes only;
///   code spans (`Event::Code`) and fenced/indented code blocks are left
///   byte-for-byte untouched.
/// - Bounded by `config::MAX_EVENTS_PER_DOC`: the event stream amplifies the
///   body (a 1-char paragraph is 3 events), so a hostile 10 MiB document could
///   otherwise build a several-hundred-MB event vector. `Err` makes the
///   caller try the next route.
/// - Bare URLs are linkified at the end (`markdown::autolink`): the viewer
///   renders them exactly like markdown links, while
///   [`plain_text_from_events`] reads the same text nodes, so the clipboard
///   keeps the URL text the document wrote.
pub fn parse_events(normalized: &str) -> Result<Vec<Event<'static>>, String> {
    // A single line with more pipes than any real table is a parser bomb:
    // pulldown's FIRST pass is eager, so the event cap below runs too late
    // (the tree already exists). Bounded line scan, then reject.
    if has_oversized_table_line(normalized) {
        return Err(format!(
            "document exceeds {} table columns on one line",
            crate::config::MAX_TABLE_COLS
        ));
    }
    let mut code_depth = 0u32;
    let mut events: Vec<Event<'static>> = Vec::new();
    for ev in Parser::new_ext(normalized, parser_options()) {
        if events.len() >= crate::config::MAX_EVENTS_PER_DOC {
            return Err(format!(
                "document exceeds {} parsed events",
                crate::config::MAX_EVENTS_PER_DOC
            ));
        }
        let mapped = match ev {
            Event::Start(Tag::CodeBlock(_)) => {
                code_depth += 1;
                ev.into_static()
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                ev.into_static()
            }
            Event::Text(t) if code_depth == 0 => {
                match crate::markdown::emoji::expand_shortcodes(&t) {
                    std::borrow::Cow::Borrowed(_) => Event::Text(t.into_static()),
                    std::borrow::Cow::Owned(s) => Event::Text(s.into()),
                }
            }
            other => other.into_static(),
        };
        events.push(mapped);
    }
    Ok(crate::markdown::autolink::linkify(events))
}

/// True when one line would yield MORE than `MAX_TABLE_COLS` table cells.
///
/// Pre-parse guard for the eager pulldown first pass: by the time the event
/// loop could enforce `MAX_EVENTS_PER_DOC`, the tree (one item per pipe
/// segment) already exists, and `table_delimiter_fixes` would have built a
/// delimiter string of the same width. A line with N unescaped pipes makes
/// N+1 cells, so the boundary is `pipes + 1 > cap` (a 4 096-pipe header used
/// to pass and yield 4 097 columns, one past the documented cap). Escaped
/// pipes (`\|`) do not separate cells and are not counted.
fn has_oversized_table_line(text: &str) -> bool {
    // Cheap pre-reject before the line walk: a body with no pipe at all can
    // never yield a table cell, and this guard runs on EVERY document.
    if !text.contains('|') {
        return false;
    }
    let cap = crate::config::MAX_TABLE_COLS;
    // Fenced code is NEVER a table: a JSON/CSV/ASCII-art line with thousands
    // of pipes inside a fence is legitimate code, and rejecting the whole
    // document for it (route fallthrough / error page) was wrong.
    // Indented code is deliberately NOT exempt: a 4-space line can also be a
    // LIST CONTINUATION that pulldown-cmark still parses as a table, and
    // exempting it would reopen the eager-first-pass bomb through lists.
    let mut fence: FenceState = None;
    for line in text.lines() {
        if fence_update(&mut fence, line) {
            continue;
        }
        let mut pipes = 0usize;
        let mut escaped = false;
        for b in line.bytes() {
            match b {
                b'\\' => escaped = !escaped,
                b'|' => {
                    if !escaped {
                        pipes += 1;
                        if pipes >= cap {
                            return true;
                        }
                    }
                    escaped = false;
                }
                _ => escaped = false,
            }
        }
    }
    false
}

/// Lax-tolerant normalization applied BEFORE parsing.
mod normalize;
mod normalize_links;
mod normalize_markup;
mod normalize_tables;
/// Plain-text derivation for whole-page Copy.
mod plain;

use normalize::*;

// The pre-split public surface stays addressable at `document::…`
// (protocol and the viewer call these through the module path).

pub use normalize::normalize_body;
pub(crate) use normalize_markup::{
    badge_open, component_fields, is_badge_close, is_component_marker,
};
pub(crate) use plain::is_skip_link_label;
pub use plain::plain_text_from_events;
#[cfg(test)]
pub use plain::plain_text_normalized;

#[cfg(test)]
mod tests;
