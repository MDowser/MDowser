// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::autolink::scan`: the bare-URL scan and candidate validation.
//!
//! `linkify` rewrites text nodes into `Link` events; `scan` finds candidate
//! spans, and the `finish_*`/`valid_*`/`trim_*` helpers validate the domain
//! against the root-zone TLD allowlist and strip trailing punctuation
//! per the GFM spec's extended autolink path validation.

use super::*;

pub fn linkify(events: Vec<Event<'static>>) -> Vec<Event<'static>> {
    let cap = crate::config::MAX_AUTOLINKS_PER_DOC;
    let mut out: Vec<Event<'static>> = Vec::with_capacity(events.len());
    let mut code_depth = 0usize;
    let mut meta_depth = 0usize;
    let mut link_depth = 0usize;
    let mut added = 0usize;
    let mut last: Option<char> = None;
    for ev in events {
        match &ev {
            Event::Start(Tag::CodeBlock(_)) => code_depth = code_depth.saturating_add(1),
            Event::End(TagEnd::CodeBlock) => code_depth = code_depth.saturating_sub(1),
            Event::Start(Tag::MetadataBlock(_)) => meta_depth = meta_depth.saturating_add(1),
            Event::End(TagEnd::MetadataBlock(_)) => meta_depth = meta_depth.saturating_sub(1),
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                link_depth = link_depth.saturating_add(1)
            }
            Event::End(TagEnd::Link | TagEnd::Image) => link_depth = link_depth.saturating_sub(1),
            _ => {}
        }
        match ev {
            Event::Text(t)
                if code_depth == 0 && meta_depth == 0 && link_depth == 0 && added < cap =>
            {
                if may_contain_url(&t) {
                    // The candidate at the very start of this run is only
                    // recognized after a GFM boundary, so the PREVIOUS run's
                    // last character goes in; this run's last character then
                    // becomes that boundary for the next one.
                    let prev = last;
                    last = t.chars().next_back();
                    push_linkified(&t, prev, &mut out, &mut added);
                } else {
                    // The common case: no candidate, keep the owned text.
                    last = t.chars().next_back();
                    out.push(Event::Text(t));
                }
            }
            Event::Text(t) => {
                last = t.chars().next_back();
                out.push(Event::Text(t));
            }
            Event::Code(c) => {
                // The boundary is the SOURCE backtick, not the span's content:
                // a code span ending in `(`, `*`, `_`, `~` or a space used to
                // open a URL right after it (`` `(`celticos.com ``), which
                // GFM does not linkify.
                last = Some('`');
                out.push(Event::Code(c));
            }
            Event::FootnoteReference(label) => {
                last = Some(']');
                out.push(Event::FootnoteReference(label));
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                // The closing `)` is SOURCE text: a run glued to a link's
                // end (`[a](b)www.evil.com`) is not at a valid GFM start
                // boundary and must not linkify. The code-span case above
                // does the same with its backtick.
                last = Some(')');
                out.push(ev);
            }
            Event::Start(_)
            | Event::End(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::TaskListMarker(_) => {
                last = None;
                out.push(ev);
            }
            other => {
                last = None;
                out.push(other);
            }
        }
    }
    out
}

/// Appends `text` with every recognized bare URL turned into a link.
///
/// The caller guarantees `may_contain_url(text)` (its only call site checks
/// it first), so the scan below is the single URL pass for this run.
fn push_linkified(
    text: &str,
    last: Option<char>,
    out: &mut Vec<Event<'static>>,
    added: &mut usize,
) {
    let cap = crate::config::MAX_AUTOLINKS_PER_DOC;
    let mut plain_start = 0usize;
    let mut from = 0usize;
    while *added < cap {
        let Some(link) = scan(text, from, last) else {
            break;
        };
        // The scanner only returns matches at/after `from` (defensive: a
        // future regression must not re-emit or loop).
        if link.start < plain_start || link.end <= link.start {
            break;
        }
        if plain_start < link.start {
            out.push(Event::Text(
                text[plain_start..link.start].to_string().into(),
            ));
        }
        out.push(Event::Start(Tag::Link {
            link_type: LinkType::Autolink,
            dest_url: CowStr::from(link.dest),
            title: CowStr::Borrowed(""),
            id: CowStr::Borrowed(""),
        }));
        out.push(Event::Text(text[link.start..link.end].to_string().into()));
        out.push(Event::End(TagEnd::Link));
        *added += 1;
        plain_start = link.end;
        from = link.end;
    }
    if plain_start < text.len() {
        out.push(Event::Text(text[plain_start..].to_string().into()));
    }
}

/// Cheap pre-check so the common text node (no URL) never pays for a scan.
///
/// Any dot may start a bare-domain candidate; `https://` covers hosts
/// without a dot (`https://localhost`) and `file:` a path without one.
fn may_contain_url(text: &str) -> bool {
    text.contains('.') || text.contains("https://") || text.contains("file:")
}

/// GFM start condition: beginning of a run, whitespace, or `*`, `_`, `~`, `(`.
fn boundary_ok(prev: char) -> bool {
    prev.is_whitespace() || matches!(prev, '*' | '_' | '~' | '(')
}

/// Finds the next bare URL in `text` at or after `from`.
///
/// `last` is the last character of the previous inline run (`None` at a
/// block/inline boundary, where a candidate is always allowed).
///
/// Linear: every byte is visited once — a candidate the final validation
/// rejects (`Url::parse` failure, over-long destination, unknown TLD) is
/// consumed whole before the scan continues, matching GFM's greedy
/// left-to-right scan.
pub(super) fn scan(text: &str, from: usize, last: Option<char>) -> Option<AutoLink> {
    let mut i = from;
    while i < text.len() {
        let rest = &text[i..];
        let www = rest.starts_with("www.");
        let file = rest.starts_with("file:");
        let prefix_len = if www {
            4
        } else if rest.starts_with("https://") {
            8
        } else if file {
            5
        } else {
            0
        };
        let prev = if i == 0 {
            last
        } else {
            text[..i].chars().next_back()
        };
        // ---------- Recognized forms ---------- //
        // `https://` as typed, `www.` with `https://` inserted, and local
        // `file:` URLs. Every other scheme (`http:`, `mailto:`, `ftp:`,
        // `data:`, ...) stays plain text: the browser only navigates https
        // and local files.
        if prefix_len > 0 {
            if prev.is_none_or(boundary_ok) {
                if file {
                    let (link, consume) = finish_file_candidate(text, i);
                    if let Some(link) = link {
                        return Some(link);
                    }
                    i = consume;
                    continue;
                }
                if let Some(dom_end) = valid_domain_end(text, i + prefix_len) {
                    // A domain glued to `@` is an email spelling
                    // (`www.example.com@evil.com`), never a URL: `Url::parse`
                    // would read the domain as userinfo and send the click to
                    // the part after the `@`. Mirrors the bare-domain rule.
                    if text[dom_end..].starts_with('@') {
                        i = dom_end;
                        continue;
                    }
                    let (link, consume) = finish_candidate(text, i, dom_end, www);
                    if let Some(link) = link {
                        return Some(link);
                    }
                    i = consume;
                    continue;
                }
            }
            i += prefix_len;
            continue;
        }
        // ---------- Bare domain (sugar beyond GFM) ---------- //
        let c = rest.chars().next().unwrap_or(' ');
        if is_domain_char(c) {
            if !prev.is_none_or(boundary_ok) {
                i += c.len_utf8();
                continue;
            }
            let (dom, run_end) = valid_domain_span(text, i);
            if let Some(dom_end) = dom {
                let tld = text[i..dom_end].rsplit('.').next().unwrap_or("");
                if crate::markdown::tld::is_tld(tld)
                    // `user.name@example.com`: the domain precedes an email
                    // local part, not a URL.
                    && !text[dom_end..].starts_with('@')
                    && !looks_like_filename(text, i, dom_end)
                {
                    let (link, consume) = finish_candidate(text, i, dom_end, true);
                    if let Some(link) = link {
                        return Some(link);
                    }
                    i = consume;
                    continue;
                }
            }
            // A rejected candidate is consumed whole (never rescanned from
            // the inside): keeps the pass linear on hostile dot runs.
            i = run_end;
            continue;
        }
        i += c.len_utf8();
    }
    None
}

/// Consumes the URL candidate whose domain is `text[start..dom_end]`.
///
/// Path: every non-space non-`<` character (GFM). Returns `(link, end)`:
/// `link` is `None` when the candidate is unusable (over the navigation cap
/// or not a parseable host URL); `end` is where the caller continues either
/// way, so a rejected candidate is never rescanned.
fn finish_candidate(
    text: &str,
    start: usize,
    dom_end: usize,
    insert_https: bool,
) -> (Option<AutoLink>, usize) {
    let bytes = text.as_bytes();
    let cap = crate::config::MAX_URL_INPUT_CHARS;
    let mut path_end = dom_end;
    // Count CHARACTERS, like `dest_within_cap`/the navigation parser: a
    // byte-count cutoff rejected non-ASCII URLs whose char count fits.
    let mut chars = text[start..dom_end].chars().count();
    let mut over_long = chars > cap;
    while !over_long && path_end < bytes.len() {
        let c = text[path_end..].chars().next().unwrap_or(' ');
        if c.is_whitespace() || c == '<' {
            break;
        }
        path_end += c.len_utf8();
        chars += 1;
        over_long = chars > cap;
    }
    if !over_long {
        let end = trim_trailing(text, start, path_end).max(dom_end);
        let label = &text[start..end];
        let dest = if insert_https {
            format!("https://{label}")
        } else {
            label.to_string()
        };
        if dest_within_cap(&dest) && valid_destination(&dest) {
            return (Some(AutoLink { start, end, dest }), path_end);
        }
    }
    (None, path_end)
}

/// Consumes a `file:` candidate and validates it as a LOCAL file URL.
///
/// Only spellings the reader accepts linkify: an absolute path on this
/// device (`file:///x`, `file:/x`, `file://localhost/x`). A remote share
/// (`file://host/x`), a relative `file:name`, a port or credentials stay
/// text. `protocol::local::parse_file_url` is the one check shared with the
/// reader, so the linker can never offer a destination the reader refuses.
fn finish_file_candidate(text: &str, start: usize) -> (Option<AutoLink>, usize) {
    let bytes = text.as_bytes();
    let cap = crate::config::MAX_URL_INPUT_CHARS;
    let mut path_end = start + 5;
    // Character count, matching `dest_within_cap` (see `finish_candidate`).
    let mut chars = 5usize;
    let mut over_long = false;
    while path_end < bytes.len() {
        let c = text[path_end..].chars().next().unwrap_or(' ');
        if c.is_whitespace() || c == '<' {
            break;
        }
        path_end += c.len_utf8();
        chars += 1;
        if chars > cap {
            over_long = true;
            break;
        }
    }
    if !over_long {
        // Only a path spelling (`file:/...`, `file:///...`) is a URL here:
        // the URL parser would otherwise absolutize a relative spelling
        // (`file:notes.md`) to `/notes.md` and link a file nobody wrote.
        if !text[start + 5..].starts_with('/') {
            return (None, path_end);
        }
        let end = trim_trailing(text, start, path_end).max(start + 5);
        let label = &text[start..end];
        if dest_within_cap(label) && crate::protocol::local::parse_file_url(label).is_ok() {
            return (
                Some(AutoLink {
                    start,
                    end,
                    dest: label.to_string(),
                }),
                path_end,
            );
        }
    }
    (None, path_end)
}

/// The stem before an extension-like TLD reads as a file name, not a host.
///
/// Only consulted when the TLD also names a common file extension (see
/// [`FILE_EXT_TLDS`]): `config.rs`, `README.md`, `script.pl`, `logo.ai` and
/// `package.zip` stay text, while `agentready.md` / `mkplandeki.pl` (real
/// hosts) link. The stem is the domain label IMMEDIATELY BEFORE the TLD
/// (so `blog.readme.md` keeps its `.md` stem, and `readme.blog.md` links),
/// compared case-insensitively against [`FILE_STEMS`]; the ambiguity is
/// documented, not hidden.
fn looks_like_filename(text: &str, start: usize, dom_end: usize) -> bool {
    let domain = &text[start..dom_end];
    let Some((before_tld, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    if !FILE_EXT_TLDS.iter().any(|e| e.eq_ignore_ascii_case(tld)) {
        return false;
    }
    let stem = before_tld.rsplit('.').next().unwrap_or(before_tld);
    FILE_STEMS.iter().any(|s| s.eq_ignore_ascii_case(stem))
}

/// Destination must fit the navigation input cap, byte- or char-wise (the
/// same rule the renderer applies before absolutizing).
fn dest_within_cap(dest: &str) -> bool {
    dest.len() <= crate::config::MAX_URL_INPUT_CHARS
        || dest.chars().count() <= crate::config::MAX_URL_INPUT_CHARS
}

/// The generated destination must be a real `https` URL with a host: the
/// autolink pass only ever builds `https://` destinations (`www.`/bare forms
/// insert it) or local `file:` ones (see [`finish_file_candidate`]).
///
/// Credentials are rejected too: in `https://example.com.@evil.com` the URL
/// parser reads `example.com.` as userinfo and `evil.com` as the host, so an
/// accepted destination would send the click somewhere other than the
/// visible label (the same `@` trap the email guard closes one byte earlier;
/// a trailing dot makes the guard miss it).
fn valid_destination(dest: &str) -> bool {
    matches!(
        Url::parse(dest),
        Ok(u) if u.host_str().is_some()
            && u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
    )
}

/// `true` for characters a GFM domain may contain.
fn is_domain_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// `(domain, consume)` for a domain starting at `start`.
///
/// - `domain` is the byte end of a valid GFM domain, or `None` when the
///   candidate at `start` is not one.
/// - `consume` is the end of the raw run of domain characters (which may
///   extend past trailing dots): callers skip to it on failure, so a run
///   that is not a host is never rescanned from the inside. Always past
///   `start`, so the scan makes progress.
///
/// `domain` is `None` for: a run over `config::AUTOLINK_MAX_DOMAIN_CHARS`
/// (the DNS limit), fewer than two dot-separated segments, empty segments,
/// a segment without an alphanumeric, or `_` in either of the last two
/// segments. Trailing `.` characters are not part of the domain (GFM trims
/// them as trailing punctuation).
fn valid_domain_span(text: &str, start: usize) -> (Option<usize>, usize) {
    let mut end = start;
    let mut truncated = false;
    for (pos, c) in text[start..].chars().enumerate() {
        if !is_domain_char(c) {
            break;
        }
        if pos >= crate::config::AUTOLINK_MAX_DOMAIN_CHARS {
            truncated = true;
            break;
        }
        end += c.len_utf8();
    }
    let consume = end.max(start + 1);
    if truncated {
        return (None, consume);
    }
    let mut domain_end = end;
    while domain_end > start && text.as_bytes()[domain_end - 1] == b'.' {
        domain_end -= 1;
    }
    if domain_end == start {
        return (None, consume);
    }
    // Validation in place: the old `split('.').collect::<Vec<_>>()` allocated
    // one Vec PER candidate word (a dotted text node paid one allocation per
    // word per document pass), and the first check only needs the count.
    let domain = &text[start..domain_end];
    let count = domain.split('.').count();
    if count < 2 {
        return (None, consume);
    }
    for (idx, segment) in domain.split('.').enumerate() {
        if segment.is_empty()
            || (idx + 2 >= count && segment.contains('_'))
            || !segment.chars().any(|c| c.is_alphanumeric())
        {
            return (None, consume);
        }
    }
    (Some(domain_end), consume)
}

/// End byte offset of a valid GFM domain starting at `start`, or `None`.
fn valid_domain_end(text: &str, start: usize) -> Option<usize> {
    valid_domain_span(text, start).0
}

/// GFM "extended autolink path validation" on `text[start..end]`:
/// trims trailing punctuation (`?`, `!`, `.`, `,`, `:`, `*`, `_`, `~`),
/// unmatched `)` (keeping balanced interior pairs) and a trailing
/// entity-like `&word;` run. Returns the trimmed end.
fn trim_trailing(text: &str, start: usize, end: usize) -> usize {
    let mut end = end;
    let mut opens = 0usize;
    let mut closes = 0usize;
    for c in text[start..end].chars() {
        match c {
            '(' => opens = opens.saturating_add(1),
            ')' => closes = closes.saturating_add(1),
            _ => {}
        }
    }
    while let Some(c) = text[..end].chars().next_back() {
        if matches!(c, '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~') {
            end -= c.len_utf8();
            continue;
        }
        if c == ')' {
            if closes > opens {
                end -= 1;
                closes -= 1;
                continue;
            }
            break;
        }
        if c == ';' {
            // `&hl;` reads as an entity reference: excluded from the link.
            let bytes = text.as_bytes();
            let mut k = end - 1; // position of the `;`
            let mut alnum = 0usize;
            while k > start {
                let b = bytes[k - 1];
                if b.is_ascii_alphanumeric() {
                    k -= 1;
                    alnum += 1;
                } else {
                    break;
                }
            }
            if alnum > 0 && k > start && bytes[k - 1] == b'&' {
                end = k - 1;
                continue;
            }
        }
        break;
    }
    end
}
