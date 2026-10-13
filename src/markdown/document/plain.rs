// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::plain`: plain-text derivation.
//!
//! Whole-page Copy text from the same normalized events the viewer
//! renders: skip links and callout markers are stripped, code keeps
//! its body exactly as written.

use super::*;

/// [`plain_text_normalized`] for a RAW body: normalizes first. Used by the
/// unit tests (the fetch path normalizes once and calls the normalized
/// variant, so production never scans the body twice).
#[cfg(test)]
pub fn plain_text(body: &str) -> String {
    plain_text_normalized(&normalize_body(body))
}

/// Accessibility skip-link phrase, shared by the viewer (which drops such a
/// leaked FIRST block instead of rendering it) and the clipboard extraction
/// (which must not paste a line that is not on screen).
///
/// Normalizes the candidate label (trim, drop trailing arrow/scroll glyphs,
/// collapse whitespace, lowercase) and matches the known phrases. A longer
/// sentence containing the words never matches.
pub(crate) fn is_skip_link_label(text: &str) -> bool {
    // Cheap O(cap) pre-reject BEFORE the whitespace normalization: a hostile
    // first block can be a single fragment link with a multi-megabyte label,
    // and `split_whitespace().collect().join().to_lowercase()` would
    // allocate several body-sized strings for a label that can never match.
    if text
        .chars()
        .nth(crate::config::SKIP_LINK_MAX_CHARS)
        .is_some()
    {
        return false;
    }
    let label = text
        .trim()
        .trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, '↓' | '→' | '»' | '⤓' | '⏬' | '↧')
        })
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if label.chars().count() > crate::config::SKIP_LINK_MAX_CHARS {
        return false;
    }
    matches!(
        label.as_str(),
        "skip to content"
            | "skip to main content"
            | "skip to main"
            | "skip navigation"
            | "skip to navigation"
            | "saltar al contenido"
            | "saltar al contenido principal"
            | "saltar a la navegación"
            | "ir al contenido"
            | "ir al contenido principal"
    )
}

/// Plain-text extraction of an ALREADY-normalized Markdown body (no
/// frontmatter).
///
/// Kept as the raw-body entry point and the ORACLE for
/// [`plain_text_from_events`] (the fetch path derives the clipboard from the
/// owned events, so production parses once; the tests compare the two).
/// - Used by "Select all"/"Copy": what the user would get selecting everything.
/// - Keeps text, inline code, breaks and list/task markers; drops markup.
/// - HTML tags (`<br/>`, `<kbd>`) are stripped to spaces/newlines instead of
///   leaking raw `<...>` into the clipboard. HTML comments
///   (`<!-- ... -->`) are dropped ENTIRELY (block and inline): they never
///   reach the screen, so they must not reach the clipboard.
/// - `+++` metadata blocks are skipped entirely (YAML frontmatter is already
///   removed by `frontmatter::split` before this point).
/// - `:shortcode:` emoji are expanded in TEXT nodes, exactly like the viewer
///   renders them; code spans/blocks keep the literal source.
/// - A leaked first-block accessibility skip link is dropped exactly like
///   the viewer drops it, so Copy matches the page.
/// - Deterministic and allocation-light (single pass, one String).
#[cfg(test)]
pub fn plain_text_normalized(normalized: &str) -> String {
    // Alert markers (`> [!NOTE]`) render as a badge, not as text:
    // drop them from the source lines before extracting the clipboard text,
    // so copy matches what is on screen. Markers outside a quote are content.
    let normalized = if normalized.contains("[!") {
        std::borrow::Cow::Owned(strip_quote_markers(normalized))
    } else {
        std::borrow::Cow::Borrowed(normalized)
    };
    let mut out = String::with_capacity(normalized.len());
    let mut in_meta = 0u32;
    let mut code_depth = 0u32;
    // HTML comment blocks emit one `Html` event PER LINE: the block's
    // FIRST line decides (`<!--` starts a comment block; a `<!--` deeper
    // inside `<div>` stays inert source), and every line is dropped.
    let mut html_block_comment = false;
    let mut html_block_first = false;
    // First top-level block skip-link filter (see `is_skip_link_label`):
    // only a paragraph whose non-empty text is ALL fragment links qualifies.
    let mut container_depth = 0u32;
    let mut first_block_done = false;
    let mut capturing_para = false;
    let mut captured = String::new();
    let mut para_links_only_frag = true;
    let mut para_saw_link = false;
    let mut link_depth = 0u32;
    for ev in Parser::new_ext(&normalized, parser_options()) {
        // Container/first-block bookkeeping (before the content match, so
        // nested paragraphs are never captured as the first block).
        match &ev {
            Event::Start(
                Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_),
            ) => {
                if container_depth == 0 {
                    first_block_done = true;
                }
                container_depth = container_depth.saturating_add(1);
            }
            Event::End(
                TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition,
            ) => container_depth = container_depth.saturating_sub(1),
            Event::Start(Tag::Link { .. }) if capturing_para => {
                // The viewer requires every non-empty fragment of the
                // skip-link paragraph to BE a link; track link text the
                // same way (text outside a link disqualifies below).
                link_depth = link_depth.saturating_add(1);
                para_saw_link = true;
            }
            Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
            Event::Start(Tag::Paragraph) => {
                if container_depth == 0 {
                    if !first_block_done {
                        capturing_para = true;
                        captured.clear();
                        para_links_only_frag = true;
                        para_saw_link = false;
                        link_depth = 0;
                    }
                    first_block_done = true;
                }
            }
            Event::Start(
                Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Table(_),
            )
            | Event::Rule
                if container_depth == 0 =>
            {
                first_block_done = true;
            }
            _ => {}
        }
        match ev {
            Event::Start(Tag::MetadataBlock(_)) => in_meta += 1,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = in_meta.saturating_sub(1),
            Event::Start(Tag::HtmlBlock) => {
                html_block_comment = false;
                html_block_first = true;
            }
            Event::End(TagEnd::HtmlBlock) => {
                html_block_comment = false;
                html_block_first = false;
            }
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                if in_meta == 0 {
                    out.push('\n');
                }
            }
            Event::Text(t) => {
                if in_meta != 0 {
                    continue;
                }
                if capturing_para {
                    // Text outside a link disqualifies the paragraph
                    // (the viewer requires all fragments to be links).
                    if link_depth == 0 && !t.trim().is_empty() {
                        para_links_only_frag = false;
                    }
                    // Text shortcodes expand like every other text node;
                    // code spans stay literal (handled below).
                    match crate::markdown::emoji::expand_shortcodes(&t) {
                        std::borrow::Cow::Borrowed(_) => captured.push_str(&t),
                        std::borrow::Cow::Owned(s) => captured.push_str(&s),
                    }
                    continue;
                }
                // Code blocks keep the literal source, exactly like the
                // viewer and the code card's Copy button (`:rocket:` inside
                // a fence must not become 🚀 on the clipboard).
                if code_depth > 0 {
                    out.push_str(&t);
                } else {
                    match crate::markdown::emoji::expand_shortcodes(&t) {
                        std::borrow::Cow::Borrowed(_) => out.push_str(&t),
                        std::borrow::Cow::Owned(s) => out.push_str(&s),
                    }
                }
            }
            Event::Code(t) => {
                if in_meta == 0 {
                    if capturing_para {
                        // Code spans follow the same link rule as text.
                        if link_depth == 0 && !t.trim().is_empty() {
                            para_links_only_frag = false;
                        }
                        captured.push_str(&t);
                    } else {
                        out.push_str(&t);
                    }
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) if capturing_para => {
                para_saw_link = true;
                if !dest_url.starts_with('#') {
                    para_links_only_frag = false;
                }
            }
            Event::Html(h) => {
                if in_meta != 0 {
                    continue;
                }
                // Component markers are structure, not text: invisible on the
                // clipboard (the component's content still copies below).
                if is_component_marker(&h) {
                    continue;
                }
                if html_block_first {
                    html_block_first = false;
                    html_block_comment = is_html_comment(&h);
                }
                if html_block_comment {
                    continue; // invisible: no text, no break
                }
                if capturing_para {
                    captured.push_str(&h);
                    continue;
                }
                // Block-level HTML is shown as inert SOURCE by the viewer:
                // copy it verbatim (tags included) so the clipboard matches
                // what is on screen instead of dropping the content lines.
                if is_html_break(&h) {
                    out.push('\n');
                } else {
                    out.push_str(&h);
                    if !h.ends_with('\n') && !h.is_empty() {
                        out.push('\n');
                    }
                }
            }
            Event::InlineHtml(h) => {
                if in_meta != 0 {
                    continue;
                }
                if is_html_comment(&h) {
                    continue; // invisible: no text, no space
                }
                if capturing_para {
                    // A tag inside the label separates words; `<br>` too.
                    if link_depth == 0 && !h.trim().is_empty() {
                        para_links_only_frag = false;
                    }
                    if !captured.is_empty() && !captured.ends_with(' ') {
                        captured.push(' ');
                    }
                    continue;
                }
                // `<br>` variants -> newline; other inline tags -> space
                // (avoids words gluing when a tag sat between them).
                if is_html_break(&h) {
                    out.push('\n');
                } else if !h.trim().is_empty() {
                    // Only separate when needed (avoid leading spaces).
                    if !out.ends_with([' ', '\n']) && !out.is_empty() {
                        out.push(' ');
                    }
                }
            }
            Event::SoftBreak => {
                if in_meta == 0 {
                    if capturing_para {
                        captured.push(' ');
                    } else {
                        out.push(' ');
                    }
                }
            }
            Event::HardBreak => {
                if in_meta == 0 {
                    if capturing_para {
                        captured.push('\n');
                    } else {
                        out.push('\n');
                    }
                }
            }
            Event::Rule => {
                if in_meta == 0 {
                    out.push_str("\n---\n");
                }
            }
            Event::TaskListMarker(checked) => {
                if in_meta == 0 && !capturing_para {
                    out.push_str(if checked { "[x] " } else { "[ ] " });
                }
            }
            Event::FootnoteReference(label) => {
                if in_meta == 0 && !capturing_para {
                    // Same cap the viewer applies to the on-screen label, so
                    // the clipboard never carries content that is not shown.
                    out.push_str("[^");
                    out.push_str(&crate::util::truncate_chars(
                        &label,
                        crate::config::MAX_FOOTNOTE_LABEL_CHARS,
                    ));
                    out.push(']');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if in_meta != 0 {
                    continue;
                }
                if capturing_para {
                    capturing_para = false;
                    if !(para_saw_link && para_links_only_frag && is_skip_link_label(&captured)) {
                        out.push_str(&captured);
                        out.push('\n');
                    }
                } else {
                    out.push('\n');
                }
            }
            Event::End(
                TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableRow | TagEnd::FootnoteDefinition,
            ) => {
                if in_meta == 0 {
                    out.push('\n');
                }
            }
            Event::End(TagEnd::TableCell) if in_meta == 0 => out.push_str(" | "),
            _ => {}
        }
    }
    out
}

/// Alert marker tokens (shared by the viewer's badge detection).
const ALERT_MARKERS: [&str; 5] = [
    "[!NOTE]",
    "[!TIP]",
    "[!IMPORTANT]",
    "[!WARNING]",
    "[!CAUTION]",
];

/// Removes a leading alert marker from one blockquote line (`> [!NOTE] ...`
/// -> `> ...`). Lines outside a quote, or without a marker, are unchanged.
/// Top-level fenced and indented code lines are never touched: a
/// `> [!NOTE]` inside a fence is literal code (the viewer renders it
/// literally and the code card's Copy keeps it), not an alert.
#[cfg(test)]
fn strip_quote_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: FenceState = None;
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if fence_update(&mut fence, line) || is_indented_code(line) {
            out.push_str(line);
            continue;
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !trimmed.starts_with('>') {
            out.push_str(line);
            continue;
        }
        // A nested quote (`> > [!NOTE]`) renders the same badge (the viewer's
        // callout detection runs at any quote depth), so Copy must strip the
        // marker at any depth too: peel every leading `>` and keep the quote
        // nesting itself.
        let mut after = trimmed;
        let mut depth = 0usize;
        while let Some(rest) = after.trim_start().strip_prefix('>') {
            after = rest;
            depth += 1;
        }
        let after = after.trim_start();
        match strip_marker_prefix(after) {
            Some(rest) => {
                out.push_str(&line[..indent]);
                for level in 0..depth {
                    if level > 0 {
                        out.push(' ');
                    }
                    out.push('>');
                }
                if !rest.is_empty() {
                    out.push(' ');
                }
                out.push_str(rest);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Returns the text after a leading `[!TYPE]` marker, if present.
fn strip_marker_prefix(text: &str) -> Option<&str> {
    for marker in ALERT_MARKERS {
        // Boundary-safe prefix test: `text` is document content, and a
        // fixed byte slice panics when a multi-byte character straddles
        // `marker.len()` (e.g. the second byte of `é`).
        if text
            .get(..marker.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(marker))
        {
            return Some(text[marker.len()..].trim_start_matches([' ', '\t']));
        }
    }
    None
}

/// Length in bytes of the longest alert marker (`[!IMPORTANT]`).
const ALERT_MAX_LEN: usize = "[!IMPORTANT]".len();

/// Resolves the buffered start of a quoted paragraph: pushes the text after
/// a leading `[!TYPE]` marker when present, else the buffered text as-is.
/// Returns `true` when a marker was stripped and no text followed it on the
/// same line: the caller then swallows the soft break the marker line left
/// behind (the source-stripping oracle sees one paragraph, not a leading
/// space).
fn flush_alert(out: &mut String, buf: &mut String) -> bool {
    let stripped = strip_marker_prefix(buf.trim_start());
    let empty_after_marker = stripped.as_ref().is_some_and(|rest| rest.is_empty());
    match stripped {
        Some(rest) => out.push_str(rest),
        None => out.push_str(buf),
    }
    buf.clear();
    empty_after_marker
}

/// Plain-text extraction of the OWNED event stream (the same events the
/// viewer renders): the fetch path parses once and derives the clipboard
/// text here instead of running `Parser` a second time over the body.
///
/// Semantics match `plain_text_normalized` (kept as the raw-body entry
/// point and the test oracle):
/// - text, inline code, breaks, list/task markers and table separators are
///   kept; HTML comments are invisible; block HTML copies verbatim;
/// - fenced/indented code keeps its body byte-for-byte (shortcodes are
///   already expanded outside code by `parse_events`);
/// - a `> [!NOTE]`-style marker is stripped from the first text of every
///   quoted paragraph, even when `pulldown-cmark` split it across several
///   `Text` events (the viewer's callout detection sees the same text);
/// - a leaked first-block accessibility skip link is dropped exactly like
///   the viewer drops it;
/// - `+++` metadata blocks are skipped.
pub fn plain_text_from_events(events: &[Event<'static>]) -> String {
    let mut out = String::new();
    let mut in_meta = 0u32;
    let mut code_depth = 0u32;
    // HTML comment blocks emit one `Html` event PER LINE: the block's FIRST
    // line decides (`<!--` starts a comment block; a `<!--` deeper inside
    // `<div>` stays inert source), and every line is dropped.
    let mut html_block_comment = false;
    let mut html_block_first = false;
    // First top-level block skip-link filter (see `is_skip_link_label`):
    // only a paragraph whose non-empty text is ALL fragment links qualifies.
    let mut container_depth = 0u32;
    let mut quote_depth = 0u32;
    let mut first_block_done = false;
    let mut capturing_para = false;
    let mut captured = String::new();
    let mut para_links_only_frag = true;
    let mut para_saw_link = false;
    let mut link_depth = 0u32;
    // Buffered start of a quoted paragraph until the callout marker question
    // is settled (the longest marker is 12 bytes; 64 bounds pathological
    // paragraphs without delaying the decision).
    let mut alert_pending = false;
    let mut alert_buf = String::new();
    // A resolved marker with nothing after it leaves the paragraph starting
    // at the NEXT text; the soft break the marker line produced must not
    // become a leading space in the clipboard.
    let mut swallow_space = false;
    // Output length at the start of the current quoted paragraph, and whether
    // that paragraph's prefix WAS a callout marker: a marker-only quote
    // paragraph (`> [!NOTE]`) is empty in the oracle (the source line became
    // an empty quote line) and must not emit a paragraph newline.
    let mut quote_para_start = 0usize;
    let mut quote_para_marker = false;
    for ev in events {
        // Settle a buffered callout marker before any event that is not the
        // continuation text of the same paragraph.
        if alert_pending && !matches!(ev, Event::Text(_) if in_meta == 0 && !capturing_para) {
            let stripped_empty = flush_alert(&mut out, &mut alert_buf);
            swallow_space |= stripped_empty;
            quote_para_marker = stripped_empty;
            alert_pending = false;
        }
        // The swallow only ever applies to the text (or break) right after
        // the marker. A break is dropped whole; the following text loses the
        // spaces the marker line left; any other event ends the swallow.
        if swallow_space {
            match ev {
                Event::SoftBreak | Event::HardBreak => {
                    swallow_space = false;
                    continue;
                }
                Event::Text(t) => {
                    swallow_space = false;
                    // Marker resolution only happens on the non-metadata,
                    // non-capturing path, so this is a plain paragraph line.
                    out.push_str(t.trim_start_matches([' ', '\t']));
                    continue;
                }
                _ => swallow_space = false,
            }
        }
        // Container/first-block bookkeeping (before the content match, so
        // nested paragraphs are never captured as the first block).
        match ev {
            Event::Start(
                Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_),
            ) => {
                if container_depth == 0 {
                    first_block_done = true;
                }
                if matches!(ev, Event::Start(Tag::BlockQuote(_))) {
                    quote_depth = quote_depth.saturating_add(1);
                }
                container_depth = container_depth.saturating_add(1);
            }
            Event::End(
                TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition,
            ) => {
                if matches!(ev, Event::End(TagEnd::BlockQuote(_))) {
                    quote_depth = quote_depth.saturating_sub(1);
                }
                container_depth = container_depth.saturating_sub(1);
            }
            Event::Start(Tag::Link { dest_url, .. }) if capturing_para => {
                // The viewer requires every non-empty fragment of the
                // skip-link paragraph to BE a fragment link; track link text
                // the same way (text outside a link disqualifies below).
                link_depth = link_depth.saturating_add(1);
                para_saw_link = true;
                if !dest_url.starts_with('#') {
                    para_links_only_frag = false;
                }
            }
            Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
            Event::Start(Tag::Paragraph) => {
                if in_meta == 0 && quote_depth > 0 {
                    alert_pending = true;
                    alert_buf.clear();
                    quote_para_start = out.len();
                    quote_para_marker = false;
                }
                if container_depth == 0 {
                    if !first_block_done {
                        capturing_para = true;
                        captured.clear();
                        para_links_only_frag = true;
                        para_saw_link = false;
                        link_depth = 0;
                    }
                    first_block_done = true;
                }
            }
            Event::Start(
                Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Table(_),
            )
            | Event::Rule
                if container_depth == 0 =>
            {
                first_block_done = true;
            }
            _ => {}
        }
        match ev {
            Event::Start(Tag::MetadataBlock(_)) => in_meta += 1,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = in_meta.saturating_sub(1),
            Event::Start(Tag::HtmlBlock) => {
                html_block_comment = false;
                html_block_first = true;
            }
            Event::End(TagEnd::HtmlBlock) => {
                html_block_comment = false;
                html_block_first = false;
            }
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                if in_meta == 0 {
                    out.push('\n');
                }
            }
            Event::Text(t) => {
                if in_meta != 0 {
                    continue;
                }
                if capturing_para {
                    // Text outside a link disqualifies the paragraph
                    // (the viewer requires all fragments to be links).
                    if link_depth == 0 && !t.trim().is_empty() {
                        para_links_only_frag = false;
                    }
                    captured.push_str(t);
                    continue;
                }
                if alert_pending {
                    let room = 64usize.saturating_sub(alert_buf.len());
                    let mut cut = room.min(t.len());
                    while cut > 0 && !t.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    alert_buf.push_str(&t[..cut]);
                    if alert_buf.contains(']')
                        || alert_buf.trim_start().len() >= ALERT_MAX_LEN
                        || alert_buf.len() >= 64
                    {
                        let stripped_empty = flush_alert(&mut out, &mut alert_buf);
                        swallow_space |= stripped_empty;
                        quote_para_marker = stripped_empty;
                        alert_pending = false;
                    }
                    out.push_str(&t[cut..]);
                    continue;
                }
                out.push_str(t);
            }
            Event::Code(t) => {
                if in_meta == 0 {
                    if capturing_para {
                        // Code spans follow the same link rule as text.
                        if link_depth == 0 && !t.trim().is_empty() {
                            para_links_only_frag = false;
                        }
                        captured.push_str(t);
                    } else {
                        out.push_str(t);
                    }
                }
            }
            Event::Html(h) => {
                if in_meta != 0 {
                    continue;
                }
                // Component markers are structure, not text: invisible on the
                // clipboard (the component's content still copies below).
                if is_component_marker(h) {
                    continue;
                }
                if html_block_first {
                    html_block_first = false;
                    html_block_comment = is_html_comment(h);
                }
                if html_block_comment {
                    continue; // invisible: no text, no break
                }
                if capturing_para {
                    captured.push_str(h);
                    continue;
                }
                // Block-level HTML is shown as inert SOURCE by the viewer:
                // copy it verbatim (tags included) so the clipboard matches
                // what is on screen instead of dropping the content lines.
                if is_html_break(h) {
                    out.push('\n');
                } else {
                    out.push_str(h);
                    if !h.ends_with('\n') && !h.is_empty() {
                        out.push('\n');
                    }
                }
            }
            Event::InlineHtml(h) => {
                if in_meta != 0 {
                    continue;
                }
                if is_html_comment(h) {
                    continue; // invisible: no text, no space
                }
                if capturing_para {
                    // A tag inside the label separates words; `<br>` too.
                    if link_depth == 0 && !h.trim().is_empty() {
                        para_links_only_frag = false;
                    }
                    if !captured.is_empty() && !captured.ends_with(' ') {
                        captured.push(' ');
                    }
                    continue;
                }
                // `<br>` variants -> newline; other inline tags -> space
                // (avoids words gluing when a tag sat between them).
                if is_html_break(h) {
                    out.push('\n');
                } else if !h.trim().is_empty() {
                    // Only separate when needed (avoid leading spaces).
                    if !out.ends_with([' ', '\n']) && !out.is_empty() {
                        out.push(' ');
                    }
                }
            }
            Event::SoftBreak => {
                if in_meta == 0 {
                    if capturing_para {
                        captured.push(' ');
                    } else {
                        out.push(' ');
                    }
                }
            }
            Event::HardBreak => {
                if in_meta == 0 {
                    if capturing_para {
                        captured.push('\n');
                    } else {
                        out.push('\n');
                    }
                }
            }
            Event::Rule => {
                if in_meta == 0 {
                    out.push_str("\n---\n");
                }
            }
            Event::TaskListMarker(checked) => {
                if in_meta == 0 && !capturing_para {
                    out.push_str(if *checked { "[x] " } else { "[ ] " });
                }
            }
            Event::FootnoteReference(label) => {
                if in_meta == 0 && !capturing_para {
                    // Same cap the viewer applies to the on-screen label, so
                    // the clipboard never carries content that is not shown.
                    out.push_str("[^");
                    out.push_str(&crate::util::truncate_chars(
                        label,
                        crate::config::MAX_FOOTNOTE_LABEL_CHARS,
                    ));
                    out.push(']');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if in_meta != 0 {
                    continue;
                }
                // A marker-only quote paragraph emitted nothing: the oracle
                // sees no paragraph at all, so no newline either.
                let marker_only = quote_para_marker && out.len() == quote_para_start;
                quote_para_marker = false;
                if capturing_para {
                    capturing_para = false;
                    if !(para_saw_link && para_links_only_frag && is_skip_link_label(&captured)) {
                        out.push_str(&captured);
                        out.push('\n');
                    }
                } else if !marker_only {
                    out.push('\n');
                }
            }
            Event::End(
                TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableRow | TagEnd::FootnoteDefinition,
            ) => {
                if in_meta == 0 {
                    out.push('\n');
                }
            }
            Event::End(TagEnd::TableCell) if in_meta == 0 => out.push_str(" | "),
            _ => {}
        }
    }
    // A quote paragraph at the very end of the stream still flushes its
    // buffered prefix (never leak a half-resolved marker).
    if alert_pending {
        flush_alert(&mut out, &mut alert_buf);
    }
    out
}
