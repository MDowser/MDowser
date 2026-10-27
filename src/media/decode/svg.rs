// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::decode::svg`: static SVG parsing, sanitizing and rasterization.
//!
//! - `build_svg_options`: the shared `usvg` options (bundled fonts only).
//! - `sanitize_svg_structure`: the structural pre-pass that strips
//!   `filter`/`fe*` markup and rejects scripts, foreign objects, SMIL,
//!   markers, patterns, CSS filters and DTDs BEFORE the parser runs.
//! - `check_svg_depth`: an iterative nesting cap (no decode-thread stack
//!   overflow).
//! - `decode_svg`/`decode_svg_pixels`: rasterize the sanitized tree.
//!
//! Security posture and caps: see the parent module docs and
//! `docs/SECURITY.md`.

use std::borrow::Cow;
use std::collections::HashMap;

use super::*;

/// `usvg` options with the app's bundled fonts and WITHOUT filesystem or
/// network access: an `<image href="/etc/...">` in a remote SVG can never
/// read local files, and no `data:` URL resolves. Shared process-wide: the
/// font database embeds every bundled face (the CJK collection alone is
/// ~19 MB), so building it once per decode thread wasted seconds of CPU per
/// SVG-heavy page. `usvg` only reads the database through the `Arc`.
static SVG_OPTIONS: std::sync::OnceLock<resvg::usvg::Options<'static>> = std::sync::OnceLock::new();

/// Loads an embedded font slice into the usvg database WITHOUT copying it:
/// `fontdb` keeps the `Arc` and shares it across every face of a `.ttc`
/// collection, so the ~19 MB CJK asset is never duplicated per decode
/// thread.
fn load_static_font(db: &mut resvg::usvg::fontdb::Database, bytes: &'static [u8]) {
    let data: std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> = std::sync::Arc::new(bytes);
    db.load_font_source(resvg::usvg::fontdb::Source::Binary(data));
}

/// Builds the resvg options once per decoding thread.
fn build_svg_options() -> resvg::usvg::Options<'static> {
    let mut db = resvg::usvg::fontdb::Database::new();
    // The app's text stack: the primary Noto Sans plus the coverage faces
    // (emoji, CJK, extended Latin/Vietnamese, every bundled script). usvg's
    // fallback resolver walks the whole database in insertion order and
    // reshapes the whole text run per fallback font, so Noto Sans is
    // inserted BEFORE the CJK collection: extended-Latin runs (Vietnamese,
    // Polish, Czech, ...) then match entirely in one face instead of
    // stranding on the CJK face, which lacks Latin Extended-A; CJK-only
    // runs still resolve to the CJK faces right after, and every script
    // face follows. Long runs mixing scripts remain best-effort (a usvg
    // limitation, not a coverage gap). No face epaint bundles is loaded:
    // Noto Sans and its script fallbacks are the whole database, exactly
    // like the UI stack.
    load_static_font(&mut db, crate::assets::NOTO_EMOJI);
    load_static_font(&mut db, crate::assets::NOTO_SANS);
    load_static_font(&mut db, crate::assets::noto_sans_cjk());
    for (_, bytes) in crate::assets::SCRIPT_FONTS {
        load_static_font(&mut db, bytes);
    }
    // Every generic family (and the no-family default) resolves to a
    // bundled Noto face: proportional generics to Noto Sans, `monospace`
    // to the bundled Noto Sans Mono CJK cut (the UI's monospace primary).
    // An SVG can name a font it does not ship and still paint a bundled
    // face, never a system font.
    db.set_sans_serif_family("Noto Sans");
    db.set_serif_family("Noto Sans");
    db.set_cursive_family("Noto Sans");
    db.set_fantasy_family("Noto Sans");
    db.set_monospace_family("Noto Sans Mono CJK JP");
    // Static only: never resolve external/local paths or inline `data:` URLs
    // (nested `data:image/svg+xml` would bypass the source pre-scan and
    // recurse into the parser), never run scripts.
    let resolver = resvg::usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    resvg::usvg::Options {
        fontdb: std::sync::Arc::new(db),
        font_family: "Noto Sans".to_string(),
        // Browsers resolve `width="1em"` against the initial 16px font
        // size; usvg's default (12) rendered agent icons undersized.
        font_size: 16.0,
        image_href_resolver: resolver,
        ..Default::default()
    }
}

/// SVG namespace URI: usvg recognizes elements by local name inside it.
const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// SVG/XLink `href` attribute namespace (for `<use xlink:href="#id">`).
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// `xml:` attribute namespace. usvg's attribute pass accepts attributes in
/// the None/SVG/XLink/XML namespaces, so the filter strip must cover them all.
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// True for `feGaussianBlur`, `feTurbulence`, ... (filter primitives).
fn is_filter_primitive(local: &str) -> bool {
    local
        .strip_prefix("fe")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// True when usvg recognizes an element with this tag name: it accepts the
/// SVG namespace and namespace-less documents alike (see usvg's
/// `parse_tag_name`). A foreign-namespace spelling is ignored by the parser,
/// so the structural checks must mirror that acceptance exactly or a
/// namespace-less document bypasses them.
fn is_svg_element(tag: &roxmltree::ExpandedName<'_, '_>) -> bool {
    matches!(tag.namespace(), None | Some(SVG_NS))
}

/// Structural SVG pre-check + filter stripping for markers, patterns, CSS
/// filters and `<use>` expansion.
///
/// - `<filter>`/`fe*` elements and every `filter` REFERENCE are removed from
///   the source (`Cow::Owned`), i.e. filter markup is IGNORED instead of
///   rendered: an image that ships a blur shadow (status badges) shows
///   its unfiltered content instead of failing, and no filter can ever run
///   (the filter cost model — `numOctaves`, morphology, convolution, filter
///   pixmaps — stays unreachable). Removing the references matters too: usvg
///   DROPS an element whose `filter` link dangles (`converter.rs`), so
///   stripping only the definitions would delete the content.
/// - CSS-spelled filters cannot be stripped textually without a full CSS
///   parser, so a `filter` declaration at a property position in a `style`
///   attribute or a `<style>` element still rejects the image. The scan
///   skips comments and strings; a spelling it misses can only make usvg
///   drop the referencing element (the definitions are gone), never render a
///   filter.
/// - `marker`/`pattern` are still rejected: a marker subtree is re-converted
///   per path vertex, and a pattern's pixmap is sized by the pattern's OWN
///   rect (an allocation whose failure aborts the process).
/// - usvg resolves elements by LOCAL NAME in the SVG namespace (or no
///   namespace at all), so the checks cannot be literal `<filter` byte scans:
///   a document using a namespace prefix (`<s:filter>`, `<s:feTurbulence>`)
///   would slip past them. Foreign-namespace spellings are ignored by usvg
///   and left alone.
///
/// The size bound matters because usvg recursively expands `<use>`: a chain
/// of nested `<use>` references grows exponentially while the source stays a
/// few hundred bytes, defeating the `<`-count cap. The walk is ITERATIVE (an
/// explicit stack): a source can nest deep, and the old recursive counter
/// overflowed the decode thread's 2 MiB stack and aborted the process (a
/// stack overflow is not catchable). Every reference is counted as a fresh
/// expansion, exactly like usvg builds the tree, so a cyclic `<use>` graph
/// trips the cap too (usvg only skips an immediate self reference, not
/// mutual recursion; a "visiting" set here used to exempt exactly the
/// cycles usvg still expands). Element depth is bounded too
/// (`MAX_SVG_DEPTH`): usvg converts groups recursively, so a deep chain
/// would overflow the stack before the token count mattered.
///
/// Returns the source with the filter markup removed (`Cow::Borrowed` when
/// there is nothing to strip).
pub(super) fn sanitize_svg_structure(bytes: &[u8]) -> Result<Cow<'_, [u8]>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "svg: source is not UTF-8".to_string())?;
    // Pin `allow_dtd: false` explicitly instead of relying on roxmltree's
    // default: a DTD (`<!DOCTYPE>`/`<!ENTITY>`) enables unbounded entity
    // expansion inside usvg's entity-expanding parser, so it is rejected here
    // before usvg ever sees the source.
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let doc =
        roxmltree::Document::parse_with_options(text, opts).map_err(|e| format!("svg: {e}"))?;
    let root = doc.root_element();
    // Byte ranges to delete: filter definitions, their `fe*` primitives and
    // every `filter` reference. XML token boundaries, so always char-safe.
    let mut cut: Vec<std::ops::Range<usize>> = Vec::new();
    // Index `id` targets once: `<use href="#id">` clones that element.
    let mut ids: HashMap<&str, roxmltree::Node> = HashMap::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name();
        // Filter markup is IGNORED, not rendered: strip the element (the
        // whole subtree goes with it; a nested `fe*` range is contained).
        if is_svg_element(&tag) && (tag.name() == "filter" || is_filter_primitive(tag.name())) {
            cut.push(node.range());
            continue;
        }
        // A `<pattern>` tile is rasterized into a pixmap sized by the
        // pattern's OWN rect (`resvg::render_pattern_pixmap`), not by the
        // rendered target: a tiny source declaring a huge pattern demands a
        // huge zeroed allocation, and `tiny_skia::Pixmap::new` ABORTS the
        // process when it fails (uncatchable, not a panic). Rejected like
        // markers.
        if is_svg_element(&tag) && tag.name() == "pattern" {
            return Err("svg: pattern elements are not allowed".to_string());
        }
        // usvg re-converts a marker's whole subtree for EVERY path vertex
        // (`marker.rs` -> `convert_children`, no cache), so a small marker
        // plus a long path builds an unbounded render tree that no source
        // token cap can see. Rejected.
        if is_svg_element(&tag) && tag.name() == "marker" {
            return Err("svg: marker elements are not allowed".to_string());
        }
        // Defense in depth: the static resvg/usvg pipeline ignores scripts,
        // foreign content and SMIL today, but reject them structurally so a
        // future parser feature cannot start executing them silently.
        // `animateMotion` and `discard` are SMIL too (motion paths, timed
        // removal): the same reasoning applies.
        if is_svg_element(&tag)
            && matches!(
                tag.name(),
                "script"
                    | "foreignObject"
                    | "animate"
                    | "animateTransform"
                    | "animateMotion"
                    | "set"
                    | "discard"
            )
        {
            return Err(format!("svg: <{}> elements are not allowed", tag.name()));
        }
        // CSS-spelled filters (see above): checked in EVERY `<style>`
        // element regardless of `type` (usvg may honor the text even when
        // `type` is not `text/css`; gating on the attribute would let
        // `type="text/scss"` smuggle a `filter:` declaration past us).
        if tag.name() == "style" && node.text().is_some_and(css_has_filter_declaration) {
            return Err("svg: CSS filter declarations are not allowed".to_string());
        }
        if node
            .attribute("style")
            .is_some_and(css_has_filter_declaration)
        {
            return Err("svg: CSS filter declarations are not allowed".to_string());
        }
        // `filter` references, in every namespace spelling usvg reads
        // (`parse_svg_element` accepts None/SVG/XLink/XML namespaces and
        // matches the attribute by local name).
        for attr in node.attributes() {
            if attr.name() == "filter"
                && matches!(attr.namespace(), None | Some(SVG_NS | XLINK_NS | XML_NS))
            {
                cut.push(attr.range());
            }
        }
        if let Some(id) = node.attribute("id") {
            // Fail-closed on duplicates: `getElementById` semantics differ
            // between parsers (first- vs last-wins). Counting a different
            // target than the renderer would undercount a `<use>` bomb, so
            // a repeated `id` rejects the image instead of guessing.
            if ids.insert(id, node).is_some() {
                return Err("svg: duplicate element ids are not allowed".to_string());
            }
        }
    }
    let mut stack: Vec<(roxmltree::Node, usize)> = vec![(root, 0)];
    let mut count = 0usize;
    while let Some((node, depth)) = stack.pop() {
        count += 1;
        if count > config::MAX_SVG_TOKENS {
            return Err(format!(
                "svg: more than {} markup tokens",
                config::MAX_SVG_TOKENS
            ));
        }
        // usvg converts the tree recursively; a deeply nested source (each
        // level costs only a few bytes) overflows the decode thread's stack
        // long before the token cap can fire.
        if depth > config::MAX_SVG_DEPTH {
            return Err(format!(
                "svg: nesting exceeds {} levels",
                config::MAX_SVG_DEPTH
            ));
        }
        let tag = node.tag_name();
        if is_svg_element(&tag) && tag.name() == "use" {
            if let Some(id) = use_target(node) {
                if let Some(target) = ids.get(id).copied() {
                    // usvg skips an immediate self reference (it could never
                    // terminate anyway); every other expansion is counted.
                    if target != node {
                        // An expansion adds a level in usvg's rendered tree.
                        stack.push((target, depth + 1));
                    }
                }
            }
        }
        for child in node.children().filter(|n| n.is_element()) {
            stack.push((child, depth + 1));
        }
    }
    if cut.is_empty() {
        return Ok(Cow::Borrowed(bytes));
    }
    Ok(Cow::Owned(strip_svg_ranges(text, cut).into_bytes()))
}

/// Deletes byte ranges from `text`. Ranges may nest or overlap (a `filter`
/// element contains its `fe*` children and their attributes): anything inside
/// an earlier removal is dropped with it. Ranges are XML token boundaries, so
/// every cut lands on a char boundary.
fn strip_svg_ranges(text: &str, mut cut: Vec<std::ops::Range<usize>>) -> String {
    cut.sort_unstable_by_key(|r| r.start);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for range in cut {
        if range.start <= pos {
            pos = pos.max(range.end);
            continue;
        }
        out.push_str(&text[pos..range.start]);
        pos = range.end;
    }
    out.push_str(&text[pos..]);
    out
}

/// True when a CSS declaration list contains a `filter` property at a
/// property position (case-insensitive).
///
/// The scan skips comments and quoted strings, so a comment, a selector or an
/// attribute value that merely mentions the word does not reject the image
/// (the old DTD episode: a raw byte scan over-rejected valid exports). It is
/// deliberately conservative in the safe direction: a false positive only
/// rejects (the pre-existing behavior for filters), while a spelling the scan
/// misses cannot render a filter because every `<filter>` definition and
/// direct reference was already stripped — it can only make usvg drop the
/// referencing element.
fn css_has_filter_declaration(css: &str) -> bool {
    let bytes = css.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    // Last significant byte outside comments/whitespace: what a property
    // name may follow (`{`, `;` or the start of the list).
    let mut prev: Option<u8> = None;
    while i < n {
        let b = bytes[i];
        // Comments vanish without updating `prev`: `{/*x*/filter:` is a
        // property.
        if b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            match find_pattern(&bytes[i + 2..], b"*/") {
                Some(end) => i += 2 + end + 2,
                None => break, // unterminated comment: nothing else is CSS
            }
            continue;
        }
        // Strings are skipped whole (escapes included).
        if b == b'"' || b == b'\'' {
            let quote = b;
            let mut j = i + 1;
            while j < n {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == quote {
                    j += 1;
                    break;
                }
                j += 1;
            }
            i = j.min(n);
            prev = Some(quote);
            continue;
        }
        if css_word_is_filter(bytes, i) {
            // `*` accepts the IE star hack simplecss itself strips before
            // reading the property name (`*filter:blur(1px)`).
            let after_separator = matches!(prev, None | Some(b'{' | b';' | b'*'));
            if after_separator {
                // simplecss skips spaces AND comments between the property
                // name and the `:` (`filter/**/:blur(1px)` is a filter), so
                // this scan must skip the same bytes or the rejection is
                // bypassable.
                let mut k = i + "filter".len();
                loop {
                    while k < n && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                        match find_pattern(&bytes[k + 2..], b"*/") {
                            Some(end) => {
                                k += 2 + end + 2;
                                continue;
                            }
                            None => break,
                        }
                    }
                    break;
                }
                if k < n && bytes[k] == b':' {
                    return true;
                }
            }
        }
        if !b.is_ascii_whitespace() {
            prev = Some(b);
        }
        i += 1;
    }
    false
}

/// True when `bytes[i..]` starts with `filter` (ASCII case-insensitive) and
/// neither neighbour is an identifier byte, so `xfilter`, `my-filter` and
/// `filtered` never match.
fn css_word_is_filter(bytes: &[u8], i: usize) -> bool {
    const WORD: &[u8] = b"filter";
    let n = bytes.len();
    if i + WORD.len() > n || !bytes[i..i + WORD.len()].eq_ignore_ascii_case(WORD) {
        return false;
    }
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    if i > 0 && ident(bytes[i - 1]) {
        return false;
    }
    if i + WORD.len() < n && ident(bytes[i + WORD.len()]) {
        return false;
    }
    true
}

/// The `#id` an SVG `<use>` points at.
///
/// Mirrors usvg's `resolve_href` precedence exactly: the UNPREFIXED `href`
/// first, then `xlink:href`. A bare `attribute("href")` matches by local
/// name in any namespace, so a forged `<use xmlns:e="urn:x" e:href="#small"
/// href="#bomb"/>` used to make the structural counter follow a different
/// target than usvg expands.
fn use_target<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> Option<&'a str> {
    let link = node
        .attributes()
        .find(|a| a.name() == "href" && a.namespace().is_none())
        .or_else(|| {
            node.attributes()
                .find(|a| a.name() == "href" && a.namespace() == Some(XLINK_NS))
        })?;
    iri_target(link.value())
}

/// True for the bytes `svgtypes` treats as whitespace (`ByteExt::is_space`):
/// ASCII space, tab, LF and CR. Deliberately NOT Rust's Unicode `trim`: NBSP
/// and other Unicode whitespace are name characters to `svgtypes`.
fn is_svg_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// The id `svgtypes::IRI::from_str` would extract from `href`, byte for byte.
///
/// usvg resolves `<use>` through that parser and silently drops a value the
/// parser rejects (`resolve_href` uses `.ok()?`), so `None` here means
/// "usvg will not expand this reference either". Replicating it exactly
/// matters for the structural counter: the old `trim().strip_prefix('#')`
/// stripped Unicode whitespace (NBSP) and trailing ASCII tabs, while
/// `IRI` only stops the ID at an ASCII SPACE -- `href="#bomb&#9;"` used to
/// look up a trimmed id that matched nothing, so the `<use>` bomb expanded
/// un-counted (`&#9;` survives attribute normalization as a real tab; a
/// literal tab would have been normalized to a space by the XML parser).
pub(super) fn iri_target(value: &str) -> Option<&str> {
    let rest = value.trim_start_matches(is_svg_space);
    let rest = rest.strip_prefix('#')?;
    // `IRI` consumes up to the first ASCII SPACE only: tabs, CR, NBSP and
    // every other character stay part of the id.
    let end = rest.find(' ').unwrap_or(rest.len());
    let (link, tail) = rest.split_at(end);
    if link.is_empty() || !tail.chars().all(is_svg_space) {
        return None;
    }
    Some(link)
}

/// Finds `pat` in `hay` (plain byte search; patterns are tiny).
fn find_pattern(hay: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || pat.len() > hay.len() {
        return None;
    }
    hay.windows(pat.len()).position(|w| w == pat)
}

/// Cheap DEPTH guard run BEFORE `roxmltree::Document::parse`.
///
/// The XML parser (and usvg's tree conversion) recurses per nesting level,
/// so a deeply nested source must be rejected before it is parsed. Counting
/// `<` bytes is NOT a depth bound (`</g>` adds tokens without nesting) and
/// the token cap alone still admitted ~100k levels from a small source,
/// overflowing the thread stack (an uncatchable abort).
///
/// The scan is quote-aware: `>` inside attribute values does not end a tag,
/// `<tag/>` self-closes without opening a level, and comments, CDATA and
/// processing instructions/declarations are skipped. Malformed markup simply
/// stops the scan: `roxmltree` then rejects it with a normal error.
pub(super) fn check_svg_depth(bytes: &[u8], max: usize) -> Result<(), String> {
    let mut depth = 0usize;
    let mut i = 0usize;
    while let Some(pos) = bytes[i..].iter().position(|b| *b == b'<') {
        i += pos;
        let rest = &bytes[i..];
        if rest.starts_with(b"<!--") {
            i += 4 + find_pattern(&rest[4..], b"-->").map_or(rest.len() - 4, |p| p + 3);
            continue;
        }
        if rest.starts_with(b"<![CDATA[") {
            i += 9 + find_pattern(&rest[9..], b"]]>").map_or(rest.len() - 9, |p| p + 3);
            continue;
        }
        if rest.len() > 1 && (rest[1] == b'!' || rest[1] == b'?') {
            i += 2 + find_pattern(&rest[2..], b">").map_or(rest.len() - 2, |p| p + 1);
            continue;
        }
        if rest.len() > 1 && rest[1] == b'/' {
            depth = depth.saturating_sub(1);
            i += 2;
            continue;
        }
        depth += 1;
        if depth > max {
            return Err(format!("svg: nesting exceeds {max} levels"));
        }
        // Find this tag's `>` (quoted attribute values may contain it).
        let mut quote: Option<u8> = None;
        let mut j = 1usize;
        while j < rest.len() {
            let b = rest[j];
            match quote {
                Some(q) => {
                    if b == q {
                        quote = None;
                    }
                }
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => break,
                None => {}
            }
            j += 1;
        }
        if j == rest.len() {
            break; // unterminated tag: roxmltree rejects the document
        }
        // `<tag/>` closes itself: no nesting level was opened.
        let mut k = j;
        while k > 0 && rest[k - 1].is_ascii_whitespace() {
            k -= 1;
        }
        if k > 0 && rest[k - 1] == b'/' {
            depth -= 1;
        }
        i += j + 1;
    }
    Ok(())
}

/// Rasterizes an SVG statically via `resvg`.
///
/// - The tree is parsed once; `<script>`/`<foreignObject>` are ignored by
///   usvg and external references are disabled (see [`SVG_OPTIONS`]).
/// - Pre-scan: the source is token-capped (`MAX_SVG_TOKENS`), nesting is
///   depth-capped (`MAX_SVG_DEPTH`, BEFORE `roxmltree` parses: its parser
///   recurses per level, so a deep source overflows the thread stack before
///   any count could help), DTDs are rejected by the structural parse
///   (roxmltree runs with `allow_dtd: false` before usvg, which expands
///   internal entities with no total-size bound), and a structural pass
///   strips `filter`/`fe*` markup (filter definitions, primitives and
///   references are removed, so filters are IGNORED and never rendered),
///   still rejects `marker`/`pattern` elements and CSS `filter`
///   declarations by local name (any namespace prefix), and bounds `<use>`
///   expansion iteratively.
/// - The raster pixmap is capped at `max_dim` per side (vector documents can
///   declare arbitrarily large viewports).
/// - tiny-skia pixels are premultiplied RGBA, stored as egui `Color32`.
pub(super) fn decode_svg(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    check_svg_depth(bytes, config::MAX_SVG_DEPTH)?;
    if bytes.iter().filter(|b| **b == b'<').count() > config::MAX_SVG_TOKENS {
        return Err(format!(
            "svg: more than {} markup tokens",
            config::MAX_SVG_TOKENS
        ));
    }
    // DTDs (`<!DOCTYPE>` with internal `<!ENTITY>`) never reach usvg's
    // parser (which allows DTDs and expands entities without a total-size
    // bound): `sanitize_svg_structure` below parses with roxmltree's default
    // `allow_dtd: false` and rejects the document first. Deliberately NOT a
    // raw byte scan: that also matched the literal text inside comments and
    // CDATA and rejected valid exported SVGs.
    let sanitized = sanitize_svg_structure(bytes)?;
    let options = SVG_OPTIONS.get_or_init(build_svg_options);
    let tree =
        resvg::usvg::Tree::from_data(&sanitized, options).map_err(|e| format!("svg: {e}"))?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err("svg: degenerate size".to_string());
    }
    let scale = (max_dim as f32 / w.max(h)).min(1.0);
    let pw = ((w * scale).ceil().max(1.0)) as u32;
    let ph = ((h * scale).ceil().max(1.0)) as u32;
    check_dims(pw, ph)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pw, ph)
        .ok_or_else(|| "svg: raster buffer refused".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels: Vec<egui::Color32> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    Ok(DecodedImage {
        pixels: egui::ColorImage::new([pw as usize, ph as usize], pixels),
        vector: true,
    })
}

/// Color-emoji entry point: rasterizes one SVG document embedded in the
/// bundled Noto Color Emoji font through the same hardened static-SVG
/// pipeline. Font documents are trusted app assets, but the caps and parser
/// guards still apply (a malformed document falls back to monochrome text).
pub(crate) fn decode_svg_pixels(bytes: &[u8], max_dim: u32) -> Result<egui::ColorImage, String> {
    decode_svg(bytes, max_dim).map(|decoded| decoded.pixels)
}
