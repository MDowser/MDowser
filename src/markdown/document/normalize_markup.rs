// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::normalize_markup`: MDX components and raw tags.
//!
//! MDX is Markdown plus JSX, and MDowser has no JavaScript, so "support"
//! means recognizing STRUCTURE and never evaluating it:
//!
//! - Known callout components (`<Note>`, `<Tip>`, `<Warning>`,
//!   `<Caution>`, `<Info>`, `<Danger>`, `<Important>`, `<Check>`) and the
//!   `type`/`title` containers (`<Callout>`, `<Alert>`, `<Aside>`,
//!   `<Admonition>`) are rewritten to alert blockquotes, so the existing
//!   callout rendering (and `alert_marker` mapping) handles them.
//! - Every other tag — JSX component or plain HTML (`<div>`, `<a>`,
//!   `<h1>`, `<details>`, …) — is a TRANSPARENT wrapper: the tag is dropped
//!   and its content stays, so the Markdown inside parses normally instead
//!   of leaking as inert monospace text. `<br>` inline is kept (it is a
//!   break), and `<img>` becomes a Markdown image so badges survive.
//! - In an MDX document (`import`/`export`, or any capitalized component),
//!   ESM statements and `{…}` expressions are removed (they cannot be
//!   evaluated). HTML comments are always removed.
//!
//! Raw-text elements are the one exception: `<script>`/`<style>` regions are
//! dropped (never content) and `<pre>`/`<textarea>` regions stay inert
//! verbatim, because their bytes are not Markdown and stripping the tags
//! would let the parser reinterpret them.
//!
//! Nothing here executes, imports, fetches or evaluates anything: it is a
//! textual, fence- and inline-code-aware pass over the normalized body,
//! shared by the viewer and the clipboard (both read the normalized text).
//! Unbalanced or unrecognized markup is left literal (same lossless
//! fallback as the `:::` admonitions).

use super::normalize::{alert_marker, fence_update, is_indented_code, FenceState};

/// Longest multi-line statement (ESM or `{}` expression) the balance scan
/// will follow before giving up and leaving the source literal.
const MAX_STATEMENT_LINES: usize = 200;
/// Deepest callout nesting expanded recursively; past this, inner callout
/// tags become transparent wrappers instead of nested badges.
const MAX_CALLOUT_DEPTH: usize = 8;

/// Cheap pre-check: does the body contain anything this pass could rewrite?
/// A `<` that could start a tag, or an `import`/`export` statement.
pub(super) fn needs_markup(text: &str) -> bool {
    if text.contains("import ")
        || text.contains("import{")
        || text.contains("export ")
        || text.contains("export{")
        || text.contains("export*")
    {
        return true;
    }
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'<' {
            let n = b[i + 1];
            if n.is_ascii_alphabetic() || n == b'/' || n == b'!' || n == b'?' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Rewrites MDX components and transparent tags. Returns `None` when the body
/// is unchanged, so the caller keeps its existing allocation.
pub(super) fn expand_markup(text: &str) -> Option<String> {
    let mut scan_budget = MAX_MARKUP_SCAN_LINES;
    markup_pass(text, 0, &mut scan_budget)
}

/// Max lines the component/callout scans may VISIT per document (all
/// openers, all recursion levels). A body of `<Tabs>`/`<Note>` openers with
/// no close tag used to scan to EOF for every opener — O(n²) on a 10 MiB
/// body. Past the budget an unmatched multi-line scan fails closed (the
/// opener is treated as a transparent tag, exactly like a missing close
/// tag); legitimate documents visit each line a small constant number of
/// times and never reach it.
const MAX_MARKUP_SCAN_LINES: usize = 2_000_000;

/// One tag parsed out of a line.
struct Tag<'a> {
    name: &'a str,
    closing: bool,
    self_closing: bool,
    attrs: &'a str,
    raw: &'a str,
    /// Byte index just past the closing `>`.
    end: usize,
}

/// Parses the tag at the start of `s` (which must begin with `<`). Returns
/// `None` for anything that is not a complete tag on this line — an autolink
/// (`<https://…>`), a `a < b` comparison, a CDATA section, an open tag that
/// wraps to the next line — so those stay literal.
fn scan_tag(s: &str) -> Option<Tag<'_>> {
    let b = s.as_bytes();
    if b.len() < 2 {
        return None;
    }
    match b[1] {
        b'!' => {
            if s.starts_with("<!--") {
                let p = s.find("-->")?;
                return Some(Tag {
                    name: "!--",
                    closing: false,
                    self_closing: false,
                    attrs: "",
                    raw: &s[..p + 3],
                    end: p + 3,
                });
            }
            // Declaration (`<!DOCTYPE …>`): the byte after `<!` must be a
            // letter; `<![CDATA[` is deliberately not matched.
            if b.len() >= 3 && b[2].is_ascii_alphabetic() {
                let p = s.find('>')?;
                return Some(Tag {
                    name: "!",
                    closing: false,
                    self_closing: false,
                    attrs: "",
                    raw: &s[..p + 1],
                    end: p + 1,
                });
            }
            return None;
        }
        b'?' => {
            let p = s.find("?>").or_else(|| s.find('>'))?;
            let end = if s[p..].starts_with("?>") {
                p + 2
            } else {
                p + 1
            };
            return Some(Tag {
                name: "?",
                closing: false,
                self_closing: false,
                attrs: "",
                raw: &s[..end],
                end,
            });
        }
        _ => {}
    }
    let (closing, name_start) = if b[1] == b'/' { (true, 2) } else { (false, 1) };
    let mut j = name_start;
    if j >= b.len() || !b[j].is_ascii_alphabetic() {
        return None;
    }
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b'.') {
        j += 1;
    }
    let name = &s[name_start..j];
    match b.get(j) {
        Some(b'>' | b'/') => {}
        Some(c) if c.is_ascii_whitespace() => {}
        _ => return None,
    }
    // Find the closing `>`, ignoring `>` inside quoted attribute values.
    let mut k = j;
    let mut quote: Option<u8> = None;
    while k < b.len() {
        let c = b[k];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    quote = Some(c);
                } else if c == b'>' {
                    break;
                }
            }
        }
        k += 1;
    }
    if k >= b.len() {
        return None;
    }
    let attrs = &s[j..k];
    let self_closing = attrs.trim_end().ends_with('/');
    Some(Tag {
        name,
        closing,
        self_closing,
        attrs,
        raw: &s[..k + 1],
        end: k + 1,
    })
}

/// Reads `key="value"` / `key='value'` / `key=value` from a tag's attribute
/// text. Returns the raw value (an MDX expression like `{x}` comes back as
/// `{x}` and is treated as unknown by callers).
pub(super) fn attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Progress guard: garbage between pairs (a quote, `=`, non-ASCII…)
        // must be skipped, never rescanned forever.
        let iter_start = i;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < b.len()
            && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'_' | b':' | b'.'))
        {
            i += 1;
        }
        let k = &attrs[ks..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let value = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                let v = &attrs[vs..i];
                if i < b.len() {
                    i += 1;
                }
                v
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                &attrs[vs..i]
            };
            if k.eq_ignore_ascii_case(key) {
                return Some(value);
            }
        }
        if i == iter_start {
            // No `key=` pair and no progress this pass: skip one whole char
            // so garbage can never pin the scan (byte steps would split
            // multi-byte sequences and panic the slicing above).
            i += attrs[iter_start..]
                .chars()
                .next()
                .map_or(1, |c| c.len_utf8());
        }
    }
    None
}

/// Like [`attr_value`], but returns the WHOLE balanced `{…}` JSX expression
/// for the attribute (`items={["A", "B"]}`), where `attr_value` would stop
/// at the first space. Quoted/bare values are returned as-is.
fn attr_expr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Same progress guard as [`attr_value`]: malformed attribute text
        // must terminate the scan, never pin it.
        let iter_start = i;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < b.len()
            && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'_' | b':' | b'.'))
        {
            i += 1;
        }
        let k = &attrs[ks..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let vs = i;
            if i < b.len() && b[i] == b'{' {
                let len = brace_expr_len(&attrs[i..])?;
                if k.eq_ignore_ascii_case(key) {
                    return Some(&attrs[vs..vs + len]);
                }
                i = vs + len;
                continue;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                if i < b.len() {
                    i += 1;
                }
            } else {
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            if k.eq_ignore_ascii_case(key) {
                return Some(&attrs[vs..i]);
            }
        }
        if i == iter_start {
            i += attrs[iter_start..]
                .chars()
                .next()
                .map_or(1, |c| c.len_utf8());
        }
    }
    None
}

/// The string entries of a JSX array literal (`{["A", 'B']}`), in order.
/// `None` when the expression is not an array literal or any entry is not a
/// string literal (an identifier, spread or nested array would make the
/// positional mapping unknowable). Used for the `<Tabs items={[…]}>` labels
/// of docs-generator markup.
fn jsx_string_array(expr: &str) -> Option<Vec<String>> {
    let body = expr.trim();
    let body = body.strip_prefix('{')?.strip_suffix('}')?.trim();
    let body = body.strip_prefix('[')?.strip_suffix(']')?;
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let quote = b[i];
        if quote != b'"' && quote != b'\'' {
            // A non-string entry (identifier, spread, nested array) makes
            // the positional mapping unknowable: decline the whole array.
            return None;
        }
        i += 1;
        let mut label = String::new();
        while i < b.len() {
            if b[i] == b'\\' && i + 1 < b.len() {
                let next = body[i + 1..].chars().next()?;
                label.push(next);
                i += 1 + next.len_utf8();
                continue;
            }
            if b[i] == quote {
                i += 1;
                break;
            }
            let ch = body[i..].chars().next()?;
            label.push(ch);
            i += ch.len_utf8();
        }
        out.push(label);
        while i < b.len() && b[i] != b',' {
            i += 1;
        }
    }
    Some(out)
}

/// `items={["A", "B"]}` labels of a `<Tabs>` tag, when it carries a literal
/// string array. `None` for an absent/expression-valued attribute.
fn items_labels(attrs: &str) -> Option<Vec<String>> {
    jsx_string_array(attr_expr_value(attrs, "items")?)
}

/// `<img …>` -> Markdown image, so a badge/logo wrapped by HTML survives the
/// tag stripping with a real fetcher path.
fn markdown_image(alt: &str, src: &str) -> String {
    let alt = alt
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let src = src.trim();
    if src.is_empty() {
        return format!("![{alt}]()");
    }
    if src.contains(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<') {
        format!("![{alt}](<{}>)", src.replace('>', "%3E"))
    } else {
        format!("![{alt}]({src})")
    }
}

/// Pushes a single separating space only when the removed span sat between
/// two word characters, so transparent tags neither glue words nor leave
/// stray spaces.
fn sep(out: &mut String, after: Option<char>) {
    if let (Some(a), Some(b)) = (out.chars().last(), after) {
        if a.is_alphanumeric() && b.is_alphanumeric() {
            out.push(' ');
        }
    }
}

/// Length of a balanced same-line `{…}` expression starting at `s[0] == '{'`,
/// or `None` when there is no close (the `{` then stays literal).
fn brace_expr_len(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (idx, ch) in s.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' | '`' => quote = Some(ch),
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(idx + 1);
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// True for a line that starts an ESM statement. Deliberately shaped: prose
/// that merely begins with the words (`import the module.`, `export sales`)
/// must not flip a document into MDX mode or disappear.
fn is_esm_statement(line: &str) -> bool {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("import") {
        let r = rest.trim_start();
        if r.starts_with('{') || r.starts_with('*') || r.starts_with('(') {
            return true;
        }
        return r.starts_with('\'')
            || r.starts_with('"')
            || r.contains(" from ")
            || r.contains("from'")
            || r.contains("from\"");
    }
    if let Some(rest) = t.strip_prefix("export") {
        let r = rest.trim_start();
        if r.starts_with('{') || r.starts_with('*') {
            return true;
        }
        let first = r
            .split(|c: char| c.is_whitespace() || c == '{' || c == ';')
            .next()
            .unwrap_or("");
        return [
            "default",
            "const",
            "let",
            "var",
            "function",
            "class",
            "type",
            "interface",
            "enum",
            "async",
            "abstract",
            "declare",
        ]
        .contains(&first);
    }
    false
}

/// Number of lines an ESM statement spans, by bracket/brace balance with
/// simple quote awareness. `None` when it never balances within the cap
/// (then the caller keeps the source untouched).
fn esm_extent(lines: &[&str], i: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for n in 0..MAX_STATEMENT_LINES {
        let l = lines.get(i + n)?;
        for ch in l.chars() {
            match quote {
                Some(q) => {
                    if ch == q {
                        quote = None;
                    }
                }
                None => match ch {
                    '"' | '\'' | '`' => quote = Some(ch),
                    '{' | '(' | '[' => depth += 1,
                    '}' | ')' | ']' => depth -= 1,
                    _ => {}
                },
            }
        }
        if depth <= 0 {
            return Some(n + 1);
        }
    }
    None
}

/// A recognized callout component.
struct Callout {
    name: String,
    marker: &'static str,
    title: String,
    self_closing: bool,
}

/// Recognizes a line that is ONLY a JSX callout open tag.
fn callout_opener(line: &str) -> Option<Callout> {
    let t = line.trim();
    if !t.starts_with('<') {
        return None;
    }
    let tag = scan_tag(t)?;
    if tag.closing || tag.end != t.len() || tag.name.starts_with('!') {
        return None;
    }
    let lower = tag.name.to_ascii_lowercase();
    const DIRECT: [&str; 12] = [
        "note",
        "tip",
        "warning",
        "caution",
        "important",
        "info",
        "danger",
        "check",
        "success",
        "hint",
        "attention",
        "error",
    ];
    const CONTAINERS: [&str; 4] = ["callout", "alert", "aside", "admonition"];
    let marker = if DIRECT.contains(&lower.as_str()) {
        alert_marker(&lower)?
    } else if CONTAINERS.contains(&lower.as_str()) {
        let ty = attr_value(tag.attrs, "type")
            .unwrap_or("")
            .to_ascii_lowercase();
        if ty.is_empty() {
            "NOTE"
        } else {
            alert_marker(&ty).unwrap_or("NOTE")
        }
    } else {
        return None;
    };
    let title = attr_value(tag.attrs, "title").unwrap_or("").to_string();
    Some(Callout {
        name: tag.name.to_string(),
        marker,
        title,
        self_closing: tag.self_closing,
    })
}

// ---------------------------------------------------------------------------
// Structural MDX components (Tabs/Tab, Accordion, Card, Steps/Step, …)
// ---------------------------------------------------------------------------
//
// Interactive/structured components cannot be flattened to Markdown: the
// viewer draws them (a bordered card, a tab bar with per-document state). The
// normalizer therefore emits CANONICAL MARKERS — a self-closing
// `<mdowser-cmp data="…" />` on its own line, surrounded by blank lines — so
// pulldown parses each one as an HTML block and the content between them
// parses as normal Markdown. `viewer::markdown::components` reads them back;
// `plain_text` skips them.

/// Reserved wire tag for a structural component marker.
const MARKER_TAG: &str = "mdowser-cmp";
/// Inline `<Badge>` marker (a custom element the viewer turns into a pill).
const BADGE_TAG: &str = "mdowser-badge";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Kinds the viewer understands (`/`-prefixed = close marker).
const MARKER_KINDS: [&str; 12] = [
    "tabs", "/tabs", "tab", "/tab", "sec", "/sec", "card", "/card", "steps", "/steps", "step",
    "/step",
];

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encodes the bytes that would break the marker wire format or the
/// `data="…"` attribute (`%`, `|`, `"`, `<`, `>`, whitespace, controls).
/// Non-ASCII is kept as-is (the value is valid UTF-8).
fn encode_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii() {
            let b = ch as u8;
            if b == b'%'
                || b == b'|'
                || b == b'"'
                || b == b'<'
                || b == b'>'
                || b <= 0x20
                || b == 0x7f
            {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn decode_field(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                // Only ASCII specials are encoded, so the byte is a char.
                out.push((h * 16 + l) as char);
                i += 3;
                continue;
            }
        }
        let Some(rest) = s.get(i..) else { break };
        let Some(ch) = rest.chars().next() else { break };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn marker(fields: &[&str]) -> String {
    let mut data = String::new();
    for (idx, f) in fields.iter().enumerate() {
        if idx > 0 {
            data.push('|');
        }
        data.push_str(&encode_field(f));
    }
    format!("<{MARKER_TAG} data=\"{data}\" />")
}

fn marker_open(kind: &str) -> String {
    marker(&[kind])
}

fn marker_open1(kind: &str, first: &str) -> String {
    marker(&[kind, first])
}

fn marker_close(kind: &str) -> String {
    let close = format!("/{kind}");
    marker(&[&close])
}

/// Parses a whole line that is a single tag (no trailing content).
fn whole_tag(line: &str) -> Option<Tag<'_>> {
    let t = line.trim();
    if !t.starts_with('<') {
        return None;
    }
    let tag = scan_tag(t)?;
    (tag.end == t.len()).then_some(tag)
}

/// Maps a (lowercased) tag name to the component kind; `transparent` means
/// the wrapper disappears and only its content is kept.
pub(super) fn structural_kind(lower: &str) -> Option<&'static str> {
    match lower {
        "tabs" => Some("tabs"),
        "tab" => Some("tab"),
        "accordion" => Some("sec"),
        "card" => Some("card"),
        "steps" => Some("steps"),
        "step" => Some("step"),
        "codegroup" => Some("codegroup"),
        "accordiongroup" | "cardgroup" | "frame" | "tiles" => Some("transparent"),
        _ => None,
    }
}

/// Maps a lowercased `:::` container name to the structural component kind.
/// Dashes/underscores are ignored (`code-group` = `codegroup`), `details` /
/// `detail` / `sec` are the collapsible section, and the transparent JSX
/// wrappers stay literal under `:::` on purpose (dropping fences there
/// would surprise). Admonition names never match here.
pub(super) fn colon_structural_kind(lower: &str) -> Option<&'static str> {
    let flat: String = lower.chars().filter(|c| *c != '-' && *c != '_').collect();
    match flat.as_str() {
        "tabs" => Some("tabs"),
        "tab" => Some("tab"),
        "accordion" | "details" | "detail" | "sec" => Some("sec"),
        "card" => Some("card"),
        "steps" => Some("steps"),
        "step" => Some("step"),
        "codegroup" => Some("codegroup"),
        _ => None,
    }
}

fn escape_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '[' | ']' | '*' | '_' | '`' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A Markdown link destination, `<…>`-wrapped when it carries spaces or
/// parentheses.
fn markdown_dest(src: &str) -> String {
    let src = src.trim();
    if src.contains(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<') {
        format!("<{}>", src.replace('>', "%3E"))
    } else {
        src.to_string()
    }
}

/// The card's title line: bold, or a link when `href` is present.
fn card_head(attrs: &str) -> String {
    let title = attr_value(attrs, "title").unwrap_or("");
    if title.is_empty() {
        return String::new();
    }
    let href = attr_value(attrs, "href").unwrap_or("");
    if href.is_empty() {
        format!("**{}**", escape_inline(title))
    } else {
        format!("[{}]({})", escape_inline(title), markdown_dest(href))
    }
}

/// `open` / blank / body / blank / `close` / blank, so each marker is its own
/// HTML block (a type-7 HTML block only ends at a blank line) and the body
/// parses as Markdown.
fn wrap_marker(open: &str, close: &str, body: &str) -> String {
    let mut s = String::with_capacity(open.len() + close.len() + body.len() + 10);
    s.push_str(open);
    s.push_str("\n\n");
    s.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        s.push('\n');
    }
    s.push('\n');
    s.push_str(close);
    s.push_str("\n\n");
    s
}

pub(super) fn render_component_markup(kind: &str, attrs: &str, body: &str) -> String {
    match kind {
        // `<Tabs items={[…]}>`: the item strings label the `<Tab>` children
        // that carry no `label`/`title` of their own (a common
        // docs-generator convention).
        "tabs" => {
            let body = match items_labels(attrs) {
                Some(labels) if !labels.is_empty() => label_items_tabs(body, &labels),
                _ => body.to_string(),
            };
            wrap_marker(&marker_open(kind), &marker_close(kind), &body)
        }
        "steps" => wrap_marker(&marker_open(kind), &marker_close(kind), body),
        "tab" => {
            let label = attr_value(attrs, "label")
                .or_else(|| attr_value(attrs, "title"))
                .unwrap_or("");
            wrap_marker(&marker_open1("tab", label), &marker_close("tab"), body)
        }
        "sec" => {
            let title = attr_value(attrs, "title")
                .or_else(|| attr_value(attrs, "label"))
                .unwrap_or("");
            wrap_marker(&marker_open1("sec", title), &marker_close("sec"), body)
        }
        "step" => {
            let title = attr_value(attrs, "title").unwrap_or("");
            wrap_marker(&marker_open1("step", title), &marker_close("step"), body)
        }
        "card" => {
            let head = card_head(attrs);
            let inner = if head.is_empty() {
                body.to_string()
            } else {
                format!("{head}\n\n{body}")
            };
            wrap_marker(&marker_open("card"), &marker_close("card"), &inner)
        }
        // `<CodeGroup>`: each fenced child becomes a tab labelled by its
        // `title=`/language, so the code blocks are switchable.
        "codegroup" => codegroup_markup(body),
        // `transparent` (Frame/CardGroup/AccordionGroup/CodeGroup/Tiles): the
        // wrapper disappears, content is kept.
        _ => {
            let mut s = body.to_string();
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s
        }
    }
}

/// Gives the label-less `<Tab>` children of a `<Tabs items={[…]}>` their
/// item labels, in order, at the parent's own depth: a nested tab set is
/// labeled by its own parent, never by an outer `items` list. Explicit
/// `label`/`title` attributes always win; items are consumed positionally
/// (one per top-level `<Tab>`).
fn label_items_tabs(body: &str, labels: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut depth = 0usize;
    let mut next = 0usize;
    for line in body.lines() {
        match component_fields(line) {
            Some(fields) => match fields[0].as_str() {
                "tabs" => {
                    depth += 1;
                    out.push_str(line);
                }
                "/tabs" => {
                    depth = depth.saturating_sub(1);
                    out.push_str(line);
                }
                "tab" if depth == 0 => {
                    let label = fields.get(1).map_or("", String::as_str);
                    let item = labels.get(next);
                    next += 1;
                    if label.is_empty() {
                        match item {
                            Some(item) => out.push_str(&marker_open1("tab", item)),
                            None => out.push_str(line),
                        }
                    } else {
                        out.push_str(line);
                    }
                }
                _ => out.push_str(line),
            },
            None => out.push_str(line),
        }
        out.push('\n');
    }
    if !body.ends_with('\n') {
        out.pop();
    }
    out
}

/// The fenced info string of an opener line (`"js title=\"app.js\""`).
fn fence_info(line: &str) -> String {
    let t = line.trim_start();
    let marker = t.as_bytes().first().copied().unwrap_or(0);
    if marker != b'`' && marker != b'~' {
        return String::new();
    }
    let run = t.bytes().take_while(|b| *b == marker).count();
    t[run..].trim().to_string()
}

/// `<CodeGroup>`: each fenced child becomes one `<Tab>` (label = its
/// `title=` or language), so the blocks switch like tabs. Falls back to a
/// transparent wrapper when there are no fences or there is other content.
fn codegroup_markup(body: &str) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let mut info = String::new();
    let mut state: FenceState = None;
    let mut other = String::new();
    for line in body.lines() {
        let was = state;
        let verbatim = fence_update(&mut state, line) || is_indented_code(line);
        if verbatim {
            if was.is_none() && state.is_some() {
                block.clear();
                info = fence_info(line);
            }
            block.push(line);
            if was.is_some() && state.is_none() {
                let label = crate::markdown::highlight::info_title(&info)
                    .or_else(|| {
                        let tok = crate::markdown::highlight::info_lang(&info);
                        (!tok.is_empty()).then(|| tok.to_string())
                    })
                    .unwrap_or_else(|| "Code".to_string());
                parts.push((label, block.join("\n")));
            }
            continue;
        }
        other.push_str(line);
        other.push('\n');
    }
    if parts.is_empty() || !other.trim().is_empty() {
        let mut s = body.to_string();
        if !s.ends_with('\n') {
            s.push('\n');
        }
        return s;
    }
    let mut out = String::new();
    out.push_str(&marker_open("tabs"));
    out.push_str("\n\n");
    for (label, blk) in &parts {
        out.push_str(&marker_open1("tab", label));
        out.push_str("\n\n");
        out.push_str(blk);
        out.push_str("\n\n");
        out.push_str(&marker_close("tab"));
        out.push_str("\n\n");
    }
    out.push_str(&marker_close("tabs"));
    out.push_str("\n\n");
    out
}

/// Consumes a whole structural component (`<Tabs>…</Tabs>`) and returns the
/// number of source lines plus its marker replacement. `None` when the close
/// tag is missing (the caller then treats the opener as a transparent tag).
fn expand_component(
    lines: &[&str],
    i: usize,
    name_lower: &str,
    attrs: &str,
    self_closing: bool,
    depth: usize,
    scan_budget: &mut usize,
) -> Option<(usize, String)> {
    let kind = structural_kind(name_lower)?;
    if self_closing {
        return Some((1, wrap_marker(&marker_open(kind), &marker_close(kind), "")));
    }
    // Recursion cap: past `MAX_CALLOUT_DEPTH` levels the opener is treated
    // as a transparent tag instead of scanning/expanding its body. Without
    // this, deeply nested components recurse until the shared scan budget
    // runs out (~1400 frames), risking a background-thread stack overflow.
    if depth >= MAX_CALLOUT_DEPTH {
        return None;
    }
    let close = format!("</{name_lower}>");
    let mut j = i + 1;
    let mut nest = 0usize;
    let mut fence: FenceState = None;
    while j < lines.len() {
        if *scan_budget == 0 {
            return None;
        }
        *scan_budget -= 1;
        // A close tag inside a fence or an indented code block is sample
        // text, not structure. Without this the component ended at the
        // fenced line, leaving the fence unclosed and swallowing the rest
        // of the document.
        if fence_update(&mut fence, lines[j]) || is_indented_code(lines[j]) {
            j += 1;
            continue;
        }
        let t = lines[j].trim();
        if t.eq_ignore_ascii_case(&close) {
            if nest == 0 {
                break;
            }
            nest -= 1;
        } else if lines[j].starts_with('<') {
            if let Some(tag) = whole_tag(lines[j]) {
                if !tag.closing && !tag.self_closing && tag.name.eq_ignore_ascii_case(name_lower) {
                    nest += 1;
                }
            }
        }
        j += 1;
    }
    if j >= lines.len() {
        return None;
    }
    let body = lines[i + 1..j].join("\n");
    let body = markup_pass(&body, depth + 1, scan_budget).unwrap_or(body);
    Some((j - i + 1, render_component_markup(kind, attrs, &body)))
}

/// A structural component written on ONE line (`<Step title="A">Body</Step>`)
/// expands exactly like its multi-line form, with the inline text as the
/// body. Without this the line fell through to transparent tag stripping:
/// the step lost its title and badge, and a `<Steps>` whose children were all
/// one-liners rendered nothing at all. `None` unless the whole line is
/// `<Name …>body</Name>` for one structural name, with no second close of
/// that name inside (`<Tab>a</Tab><Tab>b</Tab>` keeps the transparent path).
fn expand_one_line_component(line: &str, depth: usize, scan_budget: &mut usize) -> Option<String> {
    if depth >= MAX_CALLOUT_DEPTH {
        return None;
    }
    let t = line.trim_end();
    let open = scan_tag(t)?;
    if open.closing || open.self_closing {
        return None;
    }
    let lower = open.name.to_ascii_lowercase();
    // A `<CodeGroup>` needs fenced children, which cannot share one line.
    let kind = structural_kind(&lower).filter(|k| *k != "codegroup")?;
    let close = format!("</{lower}>");
    let close_at = t.len().checked_sub(close.len())?;
    if close_at < open.end
        || !t.is_char_boundary(close_at)
        || !t[close_at..].eq_ignore_ascii_case(&close)
    {
        return None;
    }
    let inner = t[open.end..close_at].trim();
    if crate::util::contains_ignore_ascii_case(inner, &close[..close.len() - 1]) {
        return None;
    }
    let body = markup_pass(inner, depth + 1, scan_budget).unwrap_or_else(|| inner.to_string());
    Some(render_component_markup(kind, open.attrs, &body))
}

/// Decoded marker fields of an HTML block, when it is a component marker.
/// First field is the kind (`tabs`, `/tab`, …).
pub(crate) fn component_fields(html: &str) -> Option<Vec<String>> {
    let t = html.trim();
    // Cheap shape gate BEFORE `whole_tag`: only `<mdowser-cmp` can carry
    // fields, and `whole_tag` otherwise scans the whole block for a `>` that
    // never comes (`<div ` + a 512 KiB body), on every frame in the viewer.
    let prefix = t.get(..MARKER_TAG.len() + 1)?;
    if !prefix.starts_with('<')
        || !prefix[1..].eq_ignore_ascii_case(MARKER_TAG)
        || t.as_bytes()
            .get(MARKER_TAG.len() + 1)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'/' && *b != b'>')
    {
        return None;
    }
    let tag = whole_tag(t)?;
    if tag.closing || !tag.name.eq_ignore_ascii_case(MARKER_TAG) {
        return None;
    }
    let data = attr_value(tag.attrs, "data")?;
    let fields: Vec<String> = data.split('|').map(decode_field).collect();
    let kind = fields.first()?;
    MARKER_KINDS.contains(&kind.as_str()).then_some(fields)
}

/// True when an HTML block is one of our component markers.
pub(crate) fn is_component_marker(html: &str) -> bool {
    component_fields(html).is_some()
}

/// True when an inline HTML event is the closing `<Badge>` marker.
pub(crate) fn is_badge_close(html: &str) -> bool {
    // Allocation-free: the `format!("</{BADGE_TAG}>")` comparison built a
    // String per inline HTML event per frame.
    let t = html.trim();
    t.len() == BADGE_TAG.len() + 3
        && t.as_bytes().starts_with(b"</")
        && t.ends_with('>')
        && t.get(2..t.len() - 1)
            .is_some_and(|name| name.eq_ignore_ascii_case(BADGE_TAG))
}

/// The `type` of an inline `<Badge>` open marker, or `None` when `html` is
/// not one. Empty string = neutral badge.
pub(crate) fn badge_open(html: &str) -> Option<String> {
    let t = html.trim();
    if t.starts_with("</") {
        return None;
    }
    let tag = whole_tag(t)?;
    if !tag.name.eq_ignore_ascii_case(BADGE_TAG) {
        return None;
    }
    let ty = attr_value(tag.attrs, "data")
        .map(decode_field)
        .unwrap_or_default();
    Some(ty)
}

/// Result of stripping one line.
struct Stripped {
    out: String,
    changed: bool,
}

/// Drops transparent tags, `<img>` -> Markdown image and (in MDX) `{…}`
/// expressions from one line, leaving code spans untouched.
fn strip_line(line: &str, base: usize, masked: &[u8], mdx: bool) -> Stripped {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut changed = false;
    let mut i = 0usize;
    // Hard per-line bound on failed scans: `scan_tag`/`brace_expr_len` walk
    // forward and report `None` when no closer exists, and a line crafted as
    // `"<a <a <a …"` (or `"{x {x {x …"` in MDX) made every opener rescan the
    // rest of the line — O(n²) on a 10 MiB line. A successful scan consumes
    // its bytes as the cursor advances, so only failures are charged here.
    let mut scan_budget = line.len().saturating_add(64);
    // A '<' past the line's last '>' cannot open a tag at all.
    let last_gt = line.rfind('>');
    // A '{' with no '}' after it cannot be an MDX expression.
    let last_brace = line.rfind('}');
    while i < bytes.len() {
        if masked[base + i] == b' ' {
            let Some(rest) = line.get(i..) else { break };
            let Some(ch) = rest.chars().next() else { break };
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        match bytes[i] {
            b'<' => {
                if scan_budget == 0 || last_gt.is_none_or(|gt| i > gt) {
                    out.push('<');
                    i += 1;
                    continue;
                }
                let rest = match line.get(i..) {
                    Some(rest) => rest,
                    None => break,
                };
                if let Some(tag) = scan_tag(rest) {
                    let lower = tag.name.to_ascii_lowercase();
                    if tag.name.starts_with('!') || tag.name.starts_with('?') {
                        changed = true;
                        i += tag.end;
                        continue;
                    }
                    // Internal markers emitted by the `:::` structural pass
                    // (or an earlier component) are wire format, not source
                    // tags: keep them verbatim so a later pass never strips
                    // what the containers pass just built.
                    if lower == MARKER_TAG || lower == BADGE_TAG {
                        out.push_str(tag.raw);
                        i += tag.end;
                        continue;
                    }
                    if lower == "badge" {
                        // Inline `<Badge>text</Badge>` -> a marker pair the
                        // viewer draws as a pill (see `viewer::inline`).
                        changed = true;
                        if tag.closing {
                            out.push_str("</");
                            out.push_str(BADGE_TAG);
                            out.push('>');
                        } else if !tag.self_closing {
                            let ty = attr_value(tag.attrs, "type").unwrap_or("");
                            out.push('<');
                            out.push_str(BADGE_TAG);
                            out.push_str(" data=\"");
                            out.push_str(&encode_field(ty));
                            out.push_str("\">");
                        }
                        i += tag.end;
                        continue;
                    }
                    if !tag.closing && lower == "br" {
                        out.push_str(tag.raw);
                        i += tag.end;
                        continue;
                    }
                    if !tag.closing && lower == "img" {
                        changed = true;
                        match attr_value(tag.attrs, "src") {
                            Some(src) if !src.is_empty() => {
                                let alt = attr_value(tag.attrs, "alt").unwrap_or("");
                                out.push_str(&markdown_image(alt, src));
                            }
                            _ => sep(&mut out, line[i + tag.end..].chars().next()),
                        }
                        i += tag.end;
                        continue;
                    }
                    changed = true;
                    sep(&mut out, line[i + tag.end..].chars().next());
                    i += tag.end;
                    continue;
                }
                // No tag: charge the failed scan (it walked to the line end)
                // to the per-line budget and keep the '<' literal.
                scan_budget = scan_budget.saturating_sub(bytes.len() - i);
                out.push('<');
                i += 1;
            }
            b'{' if mdx => {
                // Heading attributes (`{#id}`, `{.cls}`) are NOT expressions.
                let inner = line[i + 1..].chars().next();
                if inner == Some('#') || inner == Some('.') {
                    out.push('{');
                    i += 1;
                    continue;
                }
                if scan_budget == 0 || last_brace.is_none_or(|br| i >= br) {
                    out.push('{');
                    i += 1;
                    continue;
                }
                match brace_expr_len(&line[i..]) {
                    Some(len) => {
                        changed = true;
                        i += len;
                    }
                    None => {
                        scan_budget = scan_budget.saturating_sub(bytes.len() - i);
                        out.push('{');
                        i += 1;
                    }
                }
            }
            _ => {
                let Some(rest) = line.get(i..) else { break };
                let Some(ch) = rest.chars().next() else { break };
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Stripped { out, changed }
}

/// Removes every HTML comment span (`<!-- … -->`, crossing newlines) and, in
/// MDX, `{/* … */}` span, so the text on BOTH sides joins with no gap. Starts
/// inside code are left alone (the mask shields them). `None` when there was
/// nothing to remove.
fn strip_comment_spans(text: &str, masked: &[u8], mdx: bool) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut i = 0usize;
    let mut changed = false;
    while i < text.len() {
        let Some(rest) = text.get(i..) else { break };
        let Some(ch) = rest.chars().next() else { break };
        if masked[i] != b' ' && (ch == '<' || (mdx && ch == '{')) {
            let open: Option<&str> = if ch == '<' && text[i..].starts_with("<!--") {
                Some("-->")
            } else if mdx && text[i..].starts_with("{/*") {
                Some("*/}")
            } else {
                None
            };
            if let Some(close) = open {
                out.push_str(&text[last..i]);
                changed = true;
                match text[i + 3..].find(close) {
                    Some(p) => {
                        last = i + 3 + p + close.len();
                        i = last;
                    }
                    None => {
                        // Unterminated: the comment hides the rest of the
                        // document, exactly like a browser.
                        last = text.len();
                        i = text.len();
                    }
                }
                continue;
            }
        }
        i += ch.len_utf8();
    }
    if !changed {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// True when the line is only whitespace and `<br>` tags.
fn only_br(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let mut rest = t;
    let mut seen = false;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        match scan_tag(rest) {
            Some(tag) if tag.name.eq_ignore_ascii_case("br") => {
                seen = true;
                rest = &rest[tag.end..];
            }
            _ => return false,
        }
    }
    seen
}

/// One document pass: comment spans are removed first (they can cross line
/// boundaries and shift every later offset), then the tag/expression pass
/// runs. `None` when nothing changed.
fn markup_pass(text: &str, depth: usize, scan_budget: &mut usize) -> Option<String> {
    let mut masked = super::normalize_links::mask_code(text);
    let mdx = looks_like_mdx(text, &masked);
    match strip_comment_spans(text, &masked, mdx) {
        Some(cleaned) => {
            // Re-mask into the SAME buffer: the span removal changed the
            // text, but the old body-sized mask is reused instead of
            // allocating a second one.
            super::normalize_links::mask_code_into(&mut masked, &cleaned);
            Some(markup_core(&cleaned, &masked, mdx, depth, scan_budget).unwrap_or(cleaned))
        }
        None => markup_core(text, &masked, mdx, depth, scan_budget),
    }
}

/// The tag/expression pass over comment-free text. `None` when unchanged.
fn markup_core(
    text: &str,
    masked: &[u8],
    mdx: bool,
    depth: usize,
    scan_budget: &mut usize,
) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let trailing_newline = text.ends_with('\n');
    let mut offsets = Vec::with_capacity(lines.len());
    let mut o = 0usize;
    for l in &lines {
        offsets.push(o);
        o += l.len() + 1;
    }
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    // `<script>`/`<style>` content is dropped; `<pre>`/`<textarea>` stays
    // inert verbatim. Both end at the matching close tag.
    let mut drop_until: Option<String> = None;
    let mut verbatim_until: Option<String> = None;
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let start = offsets[i];
        if let Some(end) = &drop_until {
            if crate::util::contains_ignore_ascii_case(line, end) {
                drop_until = None;
            }
            i += 1;
            continue;
        }
        if let Some(end) = &verbatim_until {
            out.push_str(line);
            out.push('\n');
            if crate::util::contains_ignore_ascii_case(line, end) {
                verbatim_until = None;
            }
            i += 1;
            continue;
        }
        let line_code =
            !line.trim().is_empty() && masked[start..start + line.len()].iter().all(|b| *b == b' ');
        if line_code {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }
        if mdx && is_esm_statement(line) {
            if let Some(n) = esm_extent(&lines, i) {
                changed = true;
                i += n;
                continue;
            }
        }
        // Plain prose line (no tag opener, no MDX expression opener): copy in
        // place. `strip_line` builds a fresh String per line and copies it
        // into `out`; a markup-triggering document paid that for every line,
        // changed or not.
        if !line.contains('<') && (!mdx || !line.contains('{')) {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }
        if let Some(op) = callout_opener(line) {
            if op.self_closing {
                changed = true;
                out.push('\n');
                i += 1;
                continue;
            }
            if let Some(consumed) = build_callout(&lines, i, &op, depth, &mut out, scan_budget) {
                changed = true;
                i += consumed;
                continue;
            }
        }
        // Structural components are expanded only at column 0: an indented
        // one (inside a list item or quote) falls through to the transparent
        // tag stripping, because its markers would otherwise break out of
        // that container.
        if line.starts_with('<') {
            if let Some(tag) = whole_tag(line) {
                let lower = tag.name.to_ascii_lowercase();
                if structural_kind(&lower).is_some() {
                    if tag.closing {
                        // Orphan close tag (its opener was consumed by a
                        // parent): drop it.
                        changed = true;
                        i += 1;
                        out.push('\n');
                        continue;
                    }
                    if let Some((consumed, rep)) = expand_component(
                        &lines,
                        i,
                        &lower,
                        tag.attrs,
                        tag.self_closing,
                        depth,
                        scan_budget,
                    ) {
                        changed = true;
                        out.push_str(&rep);
                        i += consumed;
                        continue;
                    }
                }
            } else if let Some(rep) = expand_one_line_component(line, depth, scan_budget) {
                changed = true;
                out.push_str(&rep);
                i += 1;
                continue;
            }
        }
        if let Some(tag) = scan_tag(line.trim_start()) {
            let lower = tag.name.to_ascii_lowercase();
            if !tag.closing && !tag.self_closing && matches!(lower.as_str(), "script" | "style") {
                changed = true;
                let end = format!("</{lower}>");
                if !crate::util::contains_ignore_ascii_case(line, &end) {
                    drop_until = Some(end);
                }
                i += 1;
                continue;
            }
            if !tag.closing && matches!(lower.as_str(), "pre" | "textarea") {
                out.push_str(line);
                out.push('\n');
                let end = format!("</{lower}>");
                if !crate::util::contains_ignore_ascii_case(line, &end) {
                    verbatim_until = Some(end);
                }
                i += 1;
                continue;
            }
        }
        if only_br(line) {
            changed = true;
            out.push('\n');
            i += 1;
            continue;
        }
        let stripped = strip_line(line, start, masked, mdx);
        if stripped.changed {
            changed = true;
        }
        out.push_str(&stripped.out);
        out.push('\n');
        i += 1;
    }
    if !trailing_newline {
        out.pop();
    }
    (depth <= MAX_CALLOUT_DEPTH && changed).then_some(out)
}

/// Builds the alert blockquote for a callout whose opener is at `lines[i]`.
/// Returns the number of source lines consumed, or `None` when there is no
/// matching close tag (the caller then treats the opener as a wrapper).
fn build_callout(
    lines: &[&str],
    i: usize,
    op: &Callout,
    depth: usize,
    out: &mut String,
    scan_budget: &mut usize,
) -> Option<usize> {
    // Recursion cap, mirroring `expand_component`: past `MAX_CALLOUT_DEPTH`
    // nested callouts become transparent wrappers instead of expanding, so
    // hostile nesting cannot recurse until the scan budget runs out.
    if depth >= MAX_CALLOUT_DEPTH {
        return None;
    }
    let close = format!("</{}>", op.name);
    let mut j = i + 1;
    let mut nest = 0usize;
    let mut fence: FenceState = None;
    while j < lines.len() {
        if *scan_budget == 0 {
            return None;
        }
        *scan_budget -= 1;
        // Fenced/indented code is verbatim: a close tag shown as an example
        // must not end the callout.
        if fence_update(&mut fence, lines[j]) || is_indented_code(lines[j]) {
            j += 1;
            continue;
        }
        let t = lines[j].trim();
        // The opener is matched case-insensitively; the close must be too
        // (`<Note>…</note>` is a valid pair).
        if t.eq_ignore_ascii_case(&close) {
            if nest == 0 {
                break;
            }
            nest -= 1;
        } else if lines[j].starts_with('<') {
            // Same-name callouts nest: the inner close must not end the
            // outer one. A self-closing opener has no body to balance.
            if let Some(tag) = whole_tag(lines[j]) {
                if !tag.closing && !tag.self_closing && tag.name.eq_ignore_ascii_case(&op.name) {
                    nest += 1;
                }
            }
        }
        j += 1;
    }
    if j >= lines.len() {
        return None;
    }
    let body = lines[i + 1..j].join("\n");
    let body = markup_pass(&body, depth + 1, scan_budget).unwrap_or(body);
    if op.title.is_empty() {
        out.push_str(&format!("> [!{}]\n", op.marker));
    } else {
        out.push_str(&format!("> [!{}] {}\n", op.marker, op.title));
    }
    for l in body.lines() {
        if l.trim().is_empty() {
            out.push_str(">\n");
        } else {
            out.push_str("> ");
            out.push_str(l);
            out.push('\n');
        }
    }
    Some(j - i + 1)
}

/// True when the document uses MDX syntax (an ESM statement or a capitalized
/// JSX component), which is what turns `{…}` into removable expressions.
fn looks_like_mdx(text: &str, masked: &[u8]) -> bool {
    let mut o = 0usize;
    for line in text.lines() {
        let lead = line.len() - line.trim_start().len();
        let first = line.trim_start();
        if !first.is_empty() && masked.get(o + lead).is_some_and(|b| *b != b' ') {
            if is_esm_statement(line) {
                return true;
            }
            if let Some(rest) = first.strip_prefix('<') {
                if rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return true;
                }
            }
        }
        o += line.len() + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(md: &str) -> String {
        expand_markup(md).unwrap_or_else(|| md.to_string())
    }

    #[test]
    fn callout_components_become_alerts() {
        let out = run("<Note>\nBody **text**.\n</Note>\n");
        assert!(out.contains("> [!NOTE]"), "{out:?}");
        assert!(out.contains("Body **text**."), "{out:?}");
        assert!(!out.contains('<'), "{out:?}");
    }

    #[test]
    fn callout_type_and_title_props_map() {
        let out = run("<Callout type=\"danger\" title=\"Stop\">\nDanger.\n</Callout>\n");
        assert!(out.contains("> [!CAUTION] Stop"), "{out:?}");
        let out = run("<Warning>\nCareful.\n</Warning>\n");
        assert!(out.contains("> [!WARNING]"), "{out:?}");
        let out = run("<Aside type=\"tip\" title=\"Hint\">\nx\n</Aside>\n");
        assert!(out.contains("> [!TIP] Hint"), "{out:?}");
    }

    #[test]
    fn generic_components_are_transparent() {
        let out = run("<Card title=\"x\">\n# Heading\n\nBody\n</Card>\n");
        assert!(out.contains("# Heading"), "{out:?}");
        assert!(out.contains("Body"), "{out:?}");
        assert!(!out.contains("Card"), "{out:?}");
    }

    #[test]
    fn html_tags_are_transparent() {
        let out = run("<div align=\"center\">\nImportant\n</div>\n");
        assert!(out.contains("Important"), "{out:?}");
        assert!(!out.contains("div"), "{out:?}");
        let out = run("See <a href=\"https://x\">the docs</a> now.\n");
        assert_eq!(out.trim(), "See the docs now.");
    }

    #[test]
    fn image_tags_become_markdown_images() {
        let out = run("<div>\n<img src=\"logo.png\" alt=\"Logo\">\n</div>\n");
        assert!(out.contains("![Logo](logo.png)"), "{out:?}");
    }

    #[test]
    fn inline_br_is_kept_and_block_br_is_dropped() {
        let out = run("a<br>b\n");
        assert!(out.contains("<br>"), "{out:?}");
        let out = run("a\n<br>\nb\n");
        assert!(!out.contains("<br>"), "{out:?}");
    }

    #[test]
    fn code_spans_and_fences_are_untouched() {
        let out = run("Use `<div>` here.\n");
        assert!(out.contains("`<div>`"), "{out:?}");
        let out = run("```\n<div>\n{expr}\n</div>\n```\n");
        assert!(out.contains("<div>"), "{out:?}");
        assert!(out.contains("{expr}"), "{out:?}");
        let out = run("    <div>\n");
        assert!(out.contains("<div>"), "{out:?}");
    }

    #[test]
    fn autolinks_and_unclosed_tags_stay_literal() {
        let out = run("Visit <https://example.com/a> now.\n");
        assert!(out.contains("<https://example.com/a>"), "{out:?}");
        let out = run("a < b and c > d\n");
        assert!(out.contains("a < b and c > d"), "{out:?}");
    }

    #[test]
    fn esm_and_expressions_hidden_only_in_mdx() {
        let mdx = "import X from 'y'\n\nexport const a = { b: 1 }\n\n# Hi {name}\n";
        let out = run(mdx);
        assert!(!out.contains("import"), "{out:?}");
        assert!(!out.contains("export"), "{out:?}");
        assert!(!out.contains("{name}"), "{out:?}");
        assert!(out.contains("# Hi"), "{out:?}");
        // A plain document keeps braces and the word import.
        let plain = "# Hi {name}\n\nimport the module.\n";
        assert_eq!(run(plain), plain);
    }

    #[test]
    fn comments_are_removed() {
        let out = run("<!-- hidden -->\ntext\n");
        assert!(!out.contains("hidden"), "{out:?}");
        assert!(out.contains("text"), "{out:?}");
    }

    #[test]
    fn heading_attributes_survive() {
        let mdx = "import X from 'y'\n\n# Title {#custom-id}\n";
        let out = run(mdx);
        assert!(out.contains("{#custom-id}"), "{out:?}");
    }

    #[test]
    fn script_is_dropped_pre_is_inert() {
        let out = run("<script>\nalert(1)\n</script>\n\ntext\n");
        assert!(!out.contains("alert"), "{out:?}");
        assert!(out.contains("text"), "{out:?}");
        let out = run("<pre>\n  a   b\n</pre>\n");
        assert!(out.contains("<pre>"), "{out:?}");
        assert!(out.contains("a   b"), "{out:?}");
    }

    #[test]
    fn unchanged_bodies_return_none() {
        assert!(expand_markup("plain **markdown**\n").is_none());
        assert!(expand_markup("a < b\n").is_none());
    }

    #[test]
    fn marker_fields_round_trip() {
        let m = marker_open1("tab", "Hello world / a|b\"c<d>");
        let f = component_fields(&m).expect("marker parses");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0], "tab");
        assert_eq!(f[1], "Hello world / a|b\"c<d>");
    }

    #[test]
    fn tabs_emit_markers_around_content() {
        let out = run(
            "<Tabs>\n<Tab label=\"Uno\">\nContent A\n</Tab>\n<Tab label=\"Dos\">\nContent B\n</Tab>\n</Tabs>\n",
        );
        assert!(out.contains("data=\"tabs\""), "{out:?}");
        assert!(out.contains("data=\"tab|Uno\""), "{out:?}");
        assert!(out.contains("data=\"tab|Dos\""), "{out:?}");
        assert!(
            out.contains("Content A") && out.contains("Content B"),
            "{out:?}"
        );
        assert!(
            out.contains("data=\"/tab\"") && out.contains("data=\"/tabs\""),
            "{out:?}"
        );
        assert!(!out.contains("<Tabs>") && !out.contains("<Tab "), "{out:?}");
    }

    #[test]
    fn tabs_items_label_the_unlabeled_children() {
        let out = run(
            "<Tabs items={[\"Trace Details\", 'Sessions', \"Timeline\"]}>\n<Tab>\nContent A\n</Tab>\n<Tab label=\"Explicit\">\nContent B\n</Tab>\n<Tab>\nContent C\n</Tab>\n</Tabs>\n",
        );
        assert!(out.contains("data=\"tab|Trace%20Details\""), "{out:?}");
        assert!(out.contains("data=\"tab|Explicit\""), "{out:?}");
        assert!(out.contains("data=\"tab|Timeline\""), "{out:?}");
        // Non-literal arrays and expression values leave the fallback labels.
        let out = run("<Tabs items={items}>\n<Tab>\nContent\n</Tab>\n</Tabs>\n");
        assert!(out.contains("data=\"tab|\""), "{out:?}");
        let out =
            run("<Tabs items={[first, \"B\"]}>\n<Tab>\nA\n</Tab>\n<Tab>\nB\n</Tab>\n</Tabs>\n");
        assert!(
            out.matches("data=\"tab|\"").count() == 2,
            "an array with a non-string entry labels nothing: {out:?}"
        );
    }

    #[test]
    fn card_title_becomes_content_and_group_is_transparent() {
        let out = run(
            "<CardGroup>\n<Card title=\"Docs\" href=\"https://ex.com/d\">\nBody\n</Card>\n</CardGroup>\n",
        );
        assert!(out.contains("data=\"card\""), "{out:?}");
        assert!(out.contains("[Docs](https://ex.com/d)"), "{out:?}");
        assert!(out.contains("Body"), "{out:?}");
        assert!(!out.contains("CardGroup"), "{out:?}");
    }

    #[test]
    fn steps_and_accordion_emit_markers() {
        let out = run(
            "<Steps>\n<Step title=\"One\">\nFirst\n</Step>\n<Step title=\"Two\">\nSecond\n</Step>\n</Steps>\n",
        );
        assert!(out.contains("data=\"steps\""), "{out:?}");
        assert!(out.contains("data=\"step|One\""), "{out:?}");
        assert!(out.contains("data=\"step|Two\""), "{out:?}");

        let out = run("<AccordionGroup>\n<Accordion title=\"More\">\nHidden\n</Accordion>\n</AccordionGroup>\n");
        assert!(out.contains("data=\"sec|More\""), "{out:?}");
        assert!(!out.contains("AccordionGroup"), "{out:?}");
    }

    #[test]
    fn frame_wraps_content_transparently() {
        let out = run("<Frame>\n![a](b.png)\n</Frame>\n");
        assert!(out.contains("![a](b.png)"), "{out:?}");
        assert!(!out.contains("Frame"), "{out:?}");
    }

    #[test]
    fn indented_components_stay_transparent() {
        // Inside a list item the markers would break out of the item, so the
        // tagged component is only stripped, not expanded.
        let out = run("- item\n  <Card title=\"X\">\n  body\n  </Card>\n");
        assert!(!out.contains("mdowser-cmp"), "no markers: {out:?}");
        assert!(!out.contains('<'), "tags stripped: {out:?}");
        assert!(out.contains("body"), "content kept: {out:?}");
    }

    #[test]
    fn badge_emits_a_marker_pair() {
        let out = run("Status <Badge type=\"tip\">New</Badge> now\n");
        assert!(out.contains("<mdowser-badge data=\"tip\">"), "{out:?}");
        assert!(out.contains("</mdowser-badge>"), "{out:?}");
        assert!(out.contains("New"), "{out:?}");
        assert_eq!(
            badge_open("<mdowser-badge data=\"tip\">").as_deref(),
            Some("tip")
        );
        assert!(is_badge_close("</mdowser-badge>"));
        assert!(badge_open("</mdowser-badge>").is_none());
    }

    #[test]
    fn codegroup_becomes_code_tabs() {
        let out = run(
            "<CodeGroup>\n```js title=\"app.js\"\nconsole.log(1)\n```\n\n```py title=\"app.py\"\nprint(1)\n```\n</CodeGroup>\n",
        );
        assert!(out.contains("data=\"tabs\""), "{out:?}");
        assert!(out.contains("data=\"tab|app.js\""), "{out:?}");
        assert!(out.contains("data=\"tab|app.py\""), "{out:?}");
        assert!(out.contains("console.log(1)"), "{out:?}");
        assert!(!out.contains("CodeGroup"), "{out:?}");
    }

    #[test]
    fn nested_same_name_components_are_balanced() {
        // `</Tabs>` of the inner set must not close the outer one.
        let out = run(
            "<Tabs>\n<Tab label=\"Outer\">\n<Tabs>\n<Tab label=\"Inner\">\nDeep\n</Tab>\n</Tabs>\n</Tab>\n</Tabs>\n",
        );
        assert_eq!(out.matches("data=\"tabs\"").count(), 2, "{out:?}");
        assert_eq!(out.matches("data=\"/tabs\"").count(), 2, "{out:?}");
        assert_eq!(out.matches("data=\"tab|").count(), 2, "{out:?}");
    }

    #[test]
    fn one_line_components_expand_like_multi_line_ones() {
        let out = run(
            "<Steps>\n<Step title=\"Install\">Run the **installer**.</Step>\n<Step title=\"Run\">Open it.</Step>\n</Steps>\n",
        );
        assert_eq!(out.matches("data=\"steps\"").count(), 1, "{out:?}");
        assert!(out.contains("data=\"step|Install\""), "{out:?}");
        assert!(out.contains("data=\"step|Run\""), "{out:?}");
        assert_eq!(out.matches("data=\"/step\"").count(), 2, "{out:?}");
        assert!(out.contains("Run the **installer**."), "{out:?}");
        // A one-line tab and accordion take their label/title attribute.
        let out = run("<Tabs>\n<Tab title=\"A\">Alpha</Tab>\n</Tabs>\n");
        assert!(out.contains("data=\"tab|A\""), "{out:?}");
        let out = run("<Accordion title=\"More\">Hidden text</Accordion>\n");
        assert!(out.contains("data=\"sec|More\""), "{out:?}");
        assert!(out.contains("Hidden text"), "{out:?}");
    }

    #[test]
    fn several_components_on_one_line_stay_transparent() {
        // Two siblings on one line are not one component: the text survives
        // through the transparent path instead of becoming a broken pair.
        let out = run("<Tab title=\"A\">a</Tab><Tab title=\"B\">b</Tab>\n");
        assert!(!out.contains("mdowser-cmp"), "{out:?}");
        assert!(out.contains('a') && out.contains('b'), "{out:?}");
        // Multibyte text right before the close tag never splits a char.
        let out = run("<Step title=\"é\">日本語</Step>\n");
        assert!(out.contains("日本語"), "{out:?}");
    }

    #[test]
    fn self_closing_components_do_not_balance_as_nested() {
        // A `<Tab … />` has no body: counting it as a nested opener made the
        // outer `<Tabs>` miss its real close tag.
        let out = run("<Tabs>\n<Tab label=\"A\" />\n<Tab label=\"B\">\nBody\n</Tab>\n</Tabs>\n");
        assert_eq!(out.matches("data=\"tabs\"").count(), 1, "{out:?}");
        assert_eq!(out.matches("data=\"/tabs\"").count(), 1, "{out:?}");
        assert!(out.contains("data=\"tab\" />"), "{out:?}");
        assert!(out.contains("data=\"tab|B\""), "{out:?}");
    }

    #[test]
    fn close_tags_inside_fences_do_not_end_components() {
        let out = run("<Tabs>\n<Tab label=\"A\">\n```mdx\n</Tabs>\n```\n</Tab>\n</Tabs>\n");
        assert_eq!(out.matches("data=\"tabs\"").count(), 1, "{out:?}");
        assert!(out.contains("</Tabs>"), "fenced closer kept: {out:?}");
    }

    #[test]
    fn callout_close_inside_a_fence_does_not_end_the_callout() {
        let out = run("<Note>\n```mdx\n</Note>\n```\nAfter\n</Note>\n");
        assert_eq!(out.matches("[!NOTE]").count(), 1, "{out:?}");
        assert!(out.contains("> </Note>"), "fenced closer kept: {out:?}");
        assert!(out.contains("> After"), "text after fence kept: {out:?}");
    }

    #[test]
    fn callout_close_is_case_insensitive() {
        let out = run("<Note>\nBody\n</note>\n");
        assert!(out.contains("[!NOTE]"), "{out:?}");
        assert!(out.contains("> Body"), "{out:?}");
    }

    #[test]
    fn nested_same_name_callouts_are_balanced() {
        let out = run("<Note>\nOuter\n<Note>\nInner\n</Note>\nAfter\n</Note>\n");
        assert_eq!(out.matches("[!NOTE]").count(), 2, "{out:?}");
        assert!(out.contains("Outer"), "{out:?}");
        assert!(out.contains("Inner"), "{out:?}");
        assert!(out.contains("After"), "{out:?}");
    }
}
