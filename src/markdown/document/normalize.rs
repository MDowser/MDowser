// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::normalize`: pre-parse body repair.
//!
//! Every fix is cheap and allocation-free when nothing is wrong; the
//! table-bomb guard runs BEFORE the eager parser can allocate one tree
//! item per cell. The link-join pass lives in `normalize_links` and the
//! delimiter-row repair in `normalize_tables`; both share the fenced/
//! indented-code guards defined here.

use super::normalize_links::*;
use super::normalize_markup::*;
use super::normalize_tables::*;

/// Lax-tolerant normalization applied BEFORE parsing (and before `plain_text`).
///
/// What it fixes (all cheap, allocation-free when nothing is wrong):
/// - `\r\n` / `\r` line endings -> `\n` (Windows pastes, some servers).
/// - NUL (`U+0000`) -> `U+FFFD`: CommonMark/GFM "Insecure characters"
///   require the replacement before parsing, and `pulldown-cmark` keeps the
///   raw byte in its events (it would reach the rendered text otherwise).
/// - Table delimiter rows with FEWER columns than their header
///   (`| a | b | c |` + `|---|---|`): pads the delimiter so pulldown-cmark
///   (which keys column count off the delimiter) doesn't drop header cells.
///   Body rows with missing cells are already padded by the parser; excess
///   cells are dropped.
/// - Inline links/images whose text a markdown exporter split with a blank
///   line (`[label` `\n\n` `body](url)`): the blank lines inside the balanced
///   `[...](...)` span are removed so pulldown-cmark (which ends the link at
///   the blank line) does not leak a literal `[` / `](url)`. Only blank lines
///   that fall INSIDE such a span are touched.
/// - The `[#id]` heading-anchor convention (`## Title [#custom-id]`) is
///   rewritten to the heading attribute `{#custom-id}`: the id becomes the
///   heading's anchor and the marker stops showing as text.
/// - Single `---` lines are NOT touched (HR / frontmatter handled elsewhere).
///
/// What it deliberately does NOT do:
/// - Never turns a `a | b` line without a delimiter row into a table.
///   Menus (`Home | Docs`) stay paragraphs and are rendered as menus
///   by `viewer`.
pub fn normalize_body(body: &str) -> std::borrow::Cow<'_, str> {
    // Fast path: no `\r`, no NUL, no table-delimiter fix, no admonition fence.
    let needs_cr = body.contains('\r');
    // CommonMark/GFM section "Insecure characters": U+0000 must be replaced
    // with U+FFFD before parsing. `pulldown-cmark` passes the raw NUL
    // through its Text events, so the swap has to happen here.
    let needs_nul = body.contains('\0');
    // ONE delimiter scan even when a rewrite is needed anyway: the old
    // `needs_delimiter_fix` + `fix_table_delimiters` pair scanned (and
    // allocated the fix list) twice on every document with a mismatch.
    // NUL replacement does not touch line structure, so the scan still
    // runs on the original body when only NUL forces the owned path.
    let fixes = if needs_cr {
        Vec::new()
    } else {
        table_delimiter_fixes(body)
    };
    if !needs_cr && !needs_nul && fixes.is_empty() && !needs_container_expand(body) {
        // Only the link join and the MDX/HTML markup pass may still apply;
        // both return `None` when nothing changed, keeping this borrowed.
        let mut result: std::borrow::Cow<'_, str> = std::borrow::Cow::Borrowed(body);
        if needs_link_join(body) {
            if let Some(joined) = join_broken_links(body) {
                result = std::borrow::Cow::Owned(joined);
            }
        }
        if needs_markup(result.as_ref()) {
            if let Some(markup) = expand_markup(result.as_ref()) {
                result = std::borrow::Cow::Owned(markup);
            }
        }
        if let Some(fixed) = heading_anchor_fixes(result.as_ref()) {
            result = std::borrow::Cow::Owned(fixed);
        }
        return result;
    }
    let mut owned: String = if needs_cr {
        body.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        body.to_string()
    };
    if needs_nul {
        owned = owned.replace('\0', "\u{FFFD}");
    }
    // `\r` removal can create the header/delimiter adjacency being fixed.
    let fixes = if needs_cr {
        table_delimiter_fixes(&owned)
    } else {
        fixes
    };
    if !fixes.is_empty() {
        owned = apply_delimiter_fixes(&owned, fixes);
    }
    if needs_container_expand(&owned) {
        owned = expand_containers(&owned);
    }
    if needs_link_join(&owned) {
        if let Some(joined) = join_broken_links(&owned) {
            owned = joined;
        }
    }
    if needs_markup(&owned) {
        if let Some(markup) = expand_markup(&owned) {
            owned = markup;
        }
    }
    if let Some(fixed) = heading_anchor_fixes(&owned) {
        owned = fixed;
    }
    std::borrow::Cow::Owned(owned)
}

/// Returns `true` when some line looks like a `:::`/`!!!` admonition fence.
/// Cheap pre-check so documents without them keep the borrowed fast path.
fn needs_container_expand(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with(":::") || t.starts_with("!!!")
    })
}

/// One admonition opener (`::: type [title]` or `!!! type "title"`).
struct ContainerOpener {
    /// `true` for the `!!!` form (indented body); `false` for the `:::`
    /// form (closed by a `:::` line).
    bang: bool,
    /// Lowercased type token.
    kind: String,
    /// Optional title (quotes already stripped).
    title: String,
}

/// Parses an admonition opener, or `None` when the line is not one (a bare
/// `:::` closer, a `::::` nested fence, an empty `!!!`, ...).
fn container_opener(line: &str) -> Option<ContainerOpener> {
    let t = line.trim();
    let (bang, rest) = if let Some(rest) = t.strip_prefix("!!!") {
        (true, rest)
    } else {
        let rest = t.strip_prefix(":::")?;
        (false, rest)
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let kind = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    if kind.is_empty()
        || !kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let title = parts
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['"', '\''])
        .trim()
        .to_string();
    Some(ContainerOpener { bang, kind, title })
}

/// Maps an admonition type to the alert badge it renders as, or
/// `None` for a type this renderer declines to reinterpret (left literal).
pub(super) fn alert_marker(kind: &str) -> Option<&'static str> {
    match kind {
        "note" | "info" | "abstract" | "summary" | "tldr" | "example" | "quote" | "question"
        | "help" | "faq" => Some("NOTE"),
        "tip" | "hint" | "success" | "check" | "done" => Some("TIP"),
        "important" => Some("IMPORTANT"),
        "warning" | "attention" => Some("WARNING"),
        "caution" | "danger" | "error" | "failure" | "fail" | "bug" => Some("CAUTION"),
        _ => None,
    }
}

/// Strips the 4-space/tab indent of an admonition body line.
fn strip_container_indent(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix('\t') {
        Some(rest)
    } else {
        line.strip_prefix("    ")
    }
}

/// Max lines the structural `:::`/`!!!` scans may VISIT per document (all
/// openers, all recursion levels). A malformed body of `:::tabs` lines with
/// no closer used to scan to EOF for every opener — O(n²) on a 10 MiB body.
/// Past the budget every unmatched multi-line scan fails closed (the opener
/// stays literal, exactly like a malformed block); legitimate documents
/// visit each line a small constant number of times and never reach it.
const MAX_STRUCTURAL_SCAN_LINES: usize = 2_000_000;

/// Converts the admonition starting at `lines[i]` into an alert blockquote. Returns the number of source lines consumed and the
/// replacement text, or `None` when the block is not a well-formed,
/// top-level admonition (the caller then keeps it literal).
fn build_container(
    lines: &[&str],
    i: usize,
    opener: &ContainerOpener,
    scan_budget: &mut usize,
) -> Option<(usize, String)> {
    let marker = alert_marker(&opener.kind)?;
    let (consumed, body): (usize, Vec<String>) = if opener.bang {
        // The `!!!` form: the body is the following indented block (blank
        // lines inside it are kept; the first unindented line ends it).
        let mut j = i + 1;
        let mut body: Vec<String> = Vec::new();
        while j < lines.len() {
            if *scan_budget == 0 {
                return None;
            }
            *scan_budget -= 1;
            let l = lines[j];
            if l.trim().is_empty() {
                body.push(String::new());
                j += 1;
                continue;
            }
            match strip_container_indent(l) {
                Some(stripped) => {
                    body.push(stripped.to_string());
                    j += 1;
                }
                None => break,
            }
        }
        while body.last().is_some_and(|l| l.is_empty()) {
            body.pop();
            j -= 1;
        }
        if body.is_empty() {
            return None;
        }
        (j - i, body)
    } else {
        // The `:::` form: content until a line that is exactly `:::`.
        // Fenced/indented code inside the body is copied verbatim: a `:::`
        // line inside a fence is code, not the closer (the admonition used
        // to end there, leaving the fence unclosed and the trailing lines
        // literal).
        let mut j = i + 1;
        let mut fence: FenceState = None;
        let mut body: Vec<String> = Vec::new();
        while j < lines.len() {
            if *scan_budget == 0 {
                return None;
            }
            *scan_budget -= 1;
            if fence_update(&mut fence, lines[j]) || is_indented_code(lines[j]) {
                body.push(lines[j].to_string());
                j += 1;
                continue;
            }
            if lines[j].trim() == ":::" {
                break;
            }
            // Nested admonition: do not reinterpret the whole block.
            if container_opener(lines[j]).is_some() {
                return None;
            }
            body.push(lines[j].to_string());
            j += 1;
        }
        if j >= lines.len() {
            return None; // No closer: leave the source untouched.
        }
        (j - i + 1, body)
    };
    let mut rep = String::new();
    if opener.title.is_empty() {
        rep.push_str(&format!("> [!{marker}]\n"));
    } else {
        rep.push_str(&format!("> [!{marker}] {}\n", opener.title));
    }
    for l in &body {
        if l.trim().is_empty() {
            rep.push_str(">\n");
        } else {
            rep.push_str("> ");
            rep.push_str(l);
            rep.push('\n');
        }
    }
    Some((consumed, rep))
}

/// Rewrites `:::`/`!!!` admonitions into alert blockquotes so the
/// existing callout rendering handles them. Fenced and indented code lines
/// are never touched; unknown types and malformed blocks stay literal.
/// Structural `:::` containers (`tabs`, `code-group`, `steps`, `card`,
/// `accordion`/`details`, ...) become the same canonical markers as their
/// JSX equivalents before that (see below).
fn expand_containers(text: &str) -> String {
    let mut scan_budget = MAX_STRUCTURAL_SCAN_LINES;
    expand_containers_depth(text, 0, &mut scan_budget)
}

/// Deepest `:::` structural nesting expanded recursively; past this the
/// opener stays literal (same lossless fallback as malformed blocks).
const MAX_COLON_STRUCTURAL_DEPTH: usize = 8;

/// One structural `:::` opener: the marker kind plus the synthesized
/// attribute string its JSX equivalent understands (`label="…"` /
/// `title="…"` / `href="…"`).
struct ColonStructural {
    kind: &'static str,
    attrs: String,
}

/// Quotes one attribute value with the quote that avoids conflicts.
fn quote_colon_attr(key: &str, value: &str) -> String {
    if !value.contains('"') {
        format!("{key}=\"{value}\"")
    } else if !value.contains('\'') {
        format!("{key}='{value}'")
    } else {
        format!("{key}=\"{}\"", value.replace('"', ""))
    }
}

/// Bare/quoted label remainder (`:::tab Uno`, `:::tab "Uno"`) to text.
fn bare_colon_label(remainder: &str) -> String {
    remainder
        .trim()
        .trim_matches(['"', '\''])
        .trim()
        .to_string()
}

/// True when `s` is just an `href=…` pair, so `:::card href="…"` carries no
/// title (like `<Card href>`): the bare fallback must not become the title.
fn is_bare_href_pair(s: &str) -> bool {
    let t = s.trim();
    if !t.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("href")) {
        return false;
    }
    let Some(value) = t[4..].trim_start().strip_prefix('=') else {
        return false;
    };
    !value.trim().is_empty()
}

/// Attribute string for a labeled `:::` opener: explicit `label=`/`title=`
/// (and `href=` for cards) win, otherwise the bare remainder is the label.
fn colon_labeled_attrs(kind: &str, remainder: &str) -> String {
    let remainder = remainder.trim();
    if remainder.is_empty() {
        return String::new();
    }
    if kind == "card" {
        let href = attr_value(remainder, "href").unwrap_or("");
        let title = attr_value(remainder, "title")
            .or_else(|| attr_value(remainder, "label"))
            .map(str::to_string)
            .unwrap_or_else(|| {
                let bare = bare_colon_label(remainder);
                if !href.is_empty() && is_bare_href_pair(&bare) {
                    String::new()
                } else {
                    bare
                }
            });
        if title.is_empty() {
            return String::new();
        }
        if href.is_empty() {
            return quote_colon_attr("title", &title);
        }
        return format!(
            "{} {}",
            quote_colon_attr("title", &title),
            quote_colon_attr("href", href)
        );
    }
    let label = attr_value(remainder, "label")
        .or_else(|| attr_value(remainder, "title"))
        .map(str::to_string)
        .unwrap_or_else(|| bare_colon_label(remainder));
    if label.is_empty() {
        return String::new();
    }
    if kind == "step" || kind == "sec" {
        quote_colon_attr("title", &label)
    } else {
        quote_colon_attr("label", &label)
    }
}

/// Parses a structural `:::` opener, or `None` when the line is not one
/// (a bare `:::` closer, an admonition type, an unknown name, ...). Only
/// the `:::` form exists structurally; `!!!` stays admonition-only.
/// Like the JSX components, structural openers only count at column 0: an
/// indented one (inside a list item or quote) stays literal instead of
/// emitting markers that would break out of that container.
fn colon_structural_opener(line: &str) -> Option<ColonStructural> {
    let rest = line.strip_prefix(":::")?.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let kind_raw = parts.next().unwrap_or("");
    if kind_raw.is_empty()
        || !kind_raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let kind = colon_structural_kind(&kind_raw.to_ascii_lowercase())?;
    let remainder = parts.next().unwrap_or("").trim();
    let attrs = match kind {
        "tabs" | "steps" | "codegroup" => String::new(),
        _ => colon_labeled_attrs(kind, remainder),
    };
    Some(ColonStructural { kind, attrs })
}

/// True when the line opens a `:::` block of any kind (admonition or
/// structural). `!!!` blocks take no `:::` closer, so they never add depth
/// to a structural scan (their indented body cannot contain the closer).
fn is_triple_colon_opener(line: &str) -> bool {
    matches!(container_opener(line), Some(opener) if !opener.bang)
}

/// Converts the structural `:::` block starting at `lines[i]` into the same
/// canonical markers its JSX equivalent emits. Returns the consumed line
/// count plus the replacement, or `None` when malformed (no closer, past
/// the nesting cap): the caller then keeps the source literal.
fn build_structural(
    lines: &[&str],
    i: usize,
    structural: &ColonStructural,
    depth: usize,
    scan_budget: &mut usize,
) -> Option<(usize, String)> {
    if depth >= MAX_COLON_STRUCTURAL_DEPTH {
        return None;
    }
    // The closer is a line that is exactly `:::`; nested `:::` openers add
    // depth and fenced/indented code is verbatim (a `:::` line inside a
    // fence is code, not structure).
    let mut j = i + 1;
    let mut nest = 1usize;
    let mut fence: FenceState = None;
    while j < lines.len() {
        if *scan_budget == 0 {
            return None;
        }
        *scan_budget -= 1;
        if fence_update(&mut fence, lines[j]) || is_indented_code(lines[j]) {
            j += 1;
            continue;
        }
        if lines[j].trim() == ":::" {
            nest -= 1;
            if nest == 0 {
                break;
            }
            j += 1;
            continue;
        }
        if is_triple_colon_opener(lines[j]) {
            nest += 1;
        }
        j += 1;
    }
    if j >= lines.len() {
        return None;
    }
    let body_raw = lines[i + 1..j].join("\n");
    let body = expand_containers_depth(&body_raw, depth + 1, scan_budget);
    Some((
        j - i + 1,
        render_component_markup(structural.kind, &structural.attrs, &body),
    ))
}

fn expand_containers_depth(text: &str, depth: usize, scan_budget: &mut usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let trailing_newline = text.ends_with('\n');
    let mut out = String::with_capacity(text.len() + 64);
    let mut fence: FenceState = None;
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if fence_update(&mut fence, line) || is_indented_code(line) {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }
        if let Some(structural) = colon_structural_opener(line) {
            if let Some((consumed, replacement)) =
                build_structural(&lines, i, &structural, depth, scan_budget)
            {
                out.push_str(&replacement);
                i += consumed;
                continue;
            }
        }
        if let Some(opener) = container_opener(line) {
            if let Some((consumed, replacement)) = build_container(&lines, i, &opener, scan_budget)
            {
                out.push_str(&replacement);
                i += consumed;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    if !trailing_newline {
        out.pop();
    }
    out
}

/// Rewrites the `[#id]` heading-anchor convention (`## Title [#custom-id]`,
/// used by Fumadocs/Mintlify docs such as langfuse.com) into the heading
/// attribute form `{#custom-id}`. pulldown-cmark parses that attribute: it
/// sets the heading's explicit anchor id and never renders as text, so the
/// title stops showing `[#id]` and `#custom-id` links keep resolving.
///
/// Only ATX heading lines are rewritten; fenced/indented code is never
/// touched, `\[#id]` stays literal (the marker needs a space before `[`),
/// and a heading whose body does not end with `[#id]` is left alone.
/// `None` when nothing changed (the caller keeps its allocation).
fn heading_anchor_fixes(text: &str) -> Option<String> {
    if !text.contains("[#") {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    let trailing_newline = text.ends_with('\n');
    let mut out = String::with_capacity(text.len());
    let mut fence: FenceState = None;
    let mut changed = false;
    for line in &lines {
        if fence_update(&mut fence, line) || is_indented_code(line) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        match heading_with_anchor_attribute(line) {
            Some(fixed) => {
                changed = true;
                out.push_str(&fixed);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if !trailing_newline {
        out.pop();
    }
    changed.then_some(out)
}

/// `## Title [#custom-id]` -> `## Title {#custom-id}`, or `None` when the
/// line is not an ATX heading ending with the `[#id]` marker. The id is
/// accepted only as a non-empty run of non-whitespace without brackets.
fn heading_with_anchor_attribute(line: &str) -> Option<String> {
    let trimmed = line.trim_start_matches(' ');
    // CommonMark: an ATX heading may be indented up to 3 spaces; more is code.
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &trimmed[hashes..];
    // The `#` run must be followed by a space (or tab): `#hashtag` is not a
    // heading, exactly like CommonMark.
    if !(after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let body = after.trim_end();
    let inner = body.strip_suffix(']')?;
    let open = inner.rfind('[')?;
    if !inner[open..].starts_with("[#") {
        return None;
    }
    let id = &inner[open + 2..];
    if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == '[' || c == ']') {
        return None;
    }
    let before = &inner[..open];
    // The marker is a separate token: `word[#id]` stays text. This also
    // keeps an escaped `\[#id]` literal (the preceding char is `\`).
    if !(before.ends_with(' ') || before.ends_with('\t')) {
        return None;
    }
    let mut out = String::with_capacity(line.len() + 8);
    out.push_str(&line[..line.len() - after.len()]);
    out.push_str(before.trim_end());
    out.push_str(" {#");
    out.push_str(id);
    out.push('}');
    Some(out)
}

/// One open fenced code block, as the pre-parse passes must model it: the
/// marker byte, the opener's run length, the column its content starts at
/// (relative to the line after its quote prefixes are stripped) and the
/// blockquote depth it was opened at.
///
/// The container columns MATTER for the table-bomb guard: a fence opened
/// inside a blockquote (`> ``` `) or on a list item's first line
/// (`- ```json`) is continued by CommonMark only while the lines stay in
/// that container (enough `>` markers, enough indentation). Treating every
/// following line as code let a hostile unquoted/unindented table header hide
/// behind such an opener and reach the eager parser.
/// One open fenced code block (see [`FenceState`]); `Copy`: the state is
/// passed around by value.
#[derive(Clone, Copy)]
pub(crate) struct Fence {
    marker: u8,
    run: usize,
    content_col: usize,
    quote_depth: usize,
}

/// Fenced-code state carried between lines by the pre-parse passes.
pub(crate) type FenceState = Option<Fence>;

/// Outcome of one line inside an open fence (see [`advance_fence`]).
enum FenceStep {
    /// The line is code (content, or a blank line).
    Inside,
    /// The line is the closing fence: this block is over, the line is code.
    Closed,
    /// The line left the fence's container: it may itself open a new fence.
    Left,
}

/// Updates the fenced-code state for `line`; returns `true` when the line
/// itself is verbatim (an opener, a content line, or a closing fence).
///
/// CommonMark shape: up to 3 leading spaces, 3+ backticks or tildes; a
/// closer uses the same marker with at least the opener's length; a BACKTICK
/// opener whose info string contains another backtick never opens (pulldown
/// keeps the line as paragraph text, and the pre-passes must agree).
///
/// A blockquote prefix (`>`, `> >`, up to 3 spaces around each `>`) is
/// stripped first: a fence opened inside a quote (`> ``` `) masks the quoted
/// code lines exactly like a top-level one. Without this, the pre-passes
/// counted pipes and links inside quoted CODE as table cells / links. The
/// quote depth (and a list item's content column) is REMEMBERED: a line that
/// leaves the container ends the fence, so an unquoted line after a quoted
/// opener is never treated as code.
///
/// A fence may open on a list item's own first line (`- ```json`): the item's
/// content column is remembered, so only lines indented into the item are
/// code.
pub(super) fn fence_update(state: &mut FenceState, line: &str) -> bool {
    if let Some(fence) = *state {
        match advance_fence(&fence, line) {
            FenceStep::Inside => return true,
            FenceStep::Closed => {
                *state = None;
                return true;
            }
            FenceStep::Left => *state = None,
        }
    }
    match open_fence(line) {
        Some(fence) => {
            *state = Some(fence);
            true
        }
        None => false,
    }
}

/// One line's step through an open fence: the container checks first (a line
/// that left the quote or the list item ENDS the fence), then the closer.
fn advance_fence(fence: &Fence, line: &str) -> FenceStep {
    let Some(inner) = strip_quotes_exactly(line, fence.quote_depth) else {
        return FenceStep::Left;
    };
    if inner.trim().is_empty() {
        // A blank line is content: it never ends the container by itself.
        return FenceStep::Inside;
    }
    let indent = inner.len() - inner.trim_start_matches(' ').len();
    if indent < fence.content_col {
        // Less indentation than the item's content column: the item ended.
        return FenceStep::Left;
    }
    // A closer sits within 3 spaces of the container's content column and
    // carries only the marker.
    if indent <= fence.content_col.saturating_add(3) {
        let trimmed = inner.trim_start_matches(' ');
        let run = trimmed.bytes().take_while(|b| *b == fence.marker).count();
        if run >= fence.run && trimmed[run..].trim().is_empty() {
            return FenceStep::Closed;
        }
    }
    FenceStep::Inside
}

/// Detects a fence OPENER on `line` (none of the container checks: a fresh
/// line starts outside every fence).
fn open_fence(line: &str) -> Option<Fence> {
    let (stripped, quote_depth) = strip_quote_prefix(line);
    let after = stripped.trim_start_matches(' ');
    let lead = stripped.len() - after.len();
    // A fence may open on a list item's first line (`- ```json`): the item's
    // content column is where the fence starts, and the fence continues only
    // while the lines stay indented into the item.
    let (content, content_col) = if lead <= 3 {
        match split_list_marker(after) {
            Some((rest, used)) => (rest, lead + used),
            None => (after, 0),
        }
    } else {
        (after, 0)
    };
    let trimmed = content.trim_start_matches(' ');
    if content.len() - trimmed.len() > 3 {
        return None;
    }
    for marker in *b"`~" {
        let run = trimmed.bytes().take_while(|b| *b == marker).count();
        if run >= 3 {
            // CommonMark: a BACKTICK fence whose info string contains
            // another backtick is not a fence (pulldown-cmark keeps the
            // line as paragraph text). Opening state for it here would
            // exempt the following lines from every fence-aware
            // pre-pass: the table-bomb guard could be bypassed with
            // ``` x` in front of a 4097-column table.
            if marker == b'`' && trimmed[run..].contains('`') {
                continue;
            }
            return Some(Fence {
                marker,
                run,
                content_col,
                quote_depth,
            });
        }
    }
    None
}

/// Strips one list marker (`-`/`*`/`+`/`1.`/`1)`) and its following 1..=4
/// spaces, returning the item's content and how many bytes were consumed.
/// `None` when the line does not start with a marker (a `-` run is a rule,
/// not an item, and `-foo` is text).
fn split_list_marker(line: &str) -> Option<(&str, usize)> {
    let marker_end = match line.chars().next()? {
        '-' | '*' | '+' => 1,
        c if c.is_ascii_digit() => {
            let digits_end = line
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit())
                .map(|(idx, c)| idx + c.len_utf8())
                .last()?;
            let punct = line.get(digits_end..)?.chars().next()?;
            if punct != '.' && punct != ')' {
                return None;
            }
            digits_end + punct.len_utf8()
        }
        _ => return None,
    };
    let rest = line.get(marker_end..)?;
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    if !(1..=4).contains(&spaces) {
        return None;
    }
    let consumed = marker_end + spaces;
    Some((line.get(consumed..)?, consumed))
}

/// Strips exactly `n` blockquote markers from `line`, `None` when the line
/// does not carry them (it left the quote the fence lives in).
fn strip_quotes_exactly(line: &str, n: usize) -> Option<&str> {
    let mut rest = line;
    for _ in 0..n {
        let trimmed = rest.trim_start_matches(' ');
        if rest.len() - trimmed.len() > 3 || !trimmed.starts_with('>') {
            return None;
        }
        rest = &trimmed[1..];
        if let Some(after) = rest.strip_prefix(' ') {
            rest = after;
        }
    }
    Some(rest)
}

/// Strips every blockquote prefix from `line`: any run of `>` markers (each
/// optionally preceded by up to 3 spaces and followed by one optional
/// space), as CommonMark reads a quote's content. Returns the content slice
/// and the number of markers consumed, so fence detection (and only fence
/// detection) sees inside quotes and can remember the depth.
fn strip_quote_prefix(line: &str) -> (&str, usize) {
    let mut rest = line;
    let mut depth = 0usize;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        if rest.len() - trimmed.len() > 3 || !trimmed.starts_with('>') {
            return (rest, depth);
        }
        depth += 1;
        rest = &trimmed[1..];
        if let Some(after) = rest.strip_prefix(' ') {
            rest = after;
        }
    }
}

/// `true` for a line indented like CommonMark indented code (tab or 4+
/// spaces). Conservative: such a line is never considered a table header or
/// delimiter, so an indented pipe block is preserved instead of padded (a
/// table nested that deep in a list stays as the author wrote it).
pub(super) fn is_indented_code(line: &str) -> bool {
    line.starts_with('\t') || line.starts_with("    ")
}
