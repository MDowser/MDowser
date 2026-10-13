// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::decode;
use eframe::egui;

/// An SVG with more markup tokens than `MAX_SVG_TOKENS` is rejected
/// before parsing (usvg has no node limit).
#[test]
fn rejects_svg_markup_bombs() {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{}</svg>"#,
        "<rect width=\"1\" height=\"1\"/>".repeat(crate::config::MAX_SVG_TOKENS + 1)
    );
    let err = match decode(svg.as_bytes()) {
        Ok(_) => panic!("markup bomb must be rejected"),
        Err(e) => e,
    };
    assert!(err.contains("markup tokens"), "{err}");
}

/// Internal DTD entities expand without a total-size bound in roxmltree
/// (inside usvg): a tiny source used to force multi-GB allocations.
#[test]
fn rejects_svg_doctype_and_entities() {
    let svg = br#"<?xml version="1.0"?>
        <!DOCTYPE svg [<!ENTITY a "AAAA">]>
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <text>&a;&a;&a;</text>
        </svg>"#;
    let err = decode(svg).err().expect("DTD must be rejected");
    assert!(err.contains("DTD"), "{err}");
}

/// Comments and CDATA that merely CONTAIN the text `<!DOCTYPE` /
/// `<!ENTITY` are not declarations: the structural parse decides, so a
/// valid exported SVG is not rejected by a raw byte scan anymore.
#[test]
fn comments_mentioning_doctypes_still_decode() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <!-- exported from <!DOCTYPE svg> style markup -->
          <text><![CDATA[ <!ENTITY a "x"> ]]></text>
          <rect width="10" height="10" fill="#00ff00"/>
        </svg>"##;
    let decoded = decode(svg).expect("comment/CDATA text is not a DTD");
    assert_eq!(decoded.pixels.size, [10, 10]);
}

/// A pattern's raster pixmap is sized by the pattern's OWN rect
/// (`resvg::render_pattern_pixmap`), not by the rendered target: a tiny
/// source can demand an allocation that aborts the process. Patterns
/// are rejected like markers.
#[test]
fn rejects_svg_patterns() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <pattern id="p" patternUnits="userSpaceOnUse" width="100000" height="100000">
            <rect width="10" height="10" fill="red"/>
          </pattern>
          <rect width="10" height="10" fill="url(#p)"/>
        </svg>"##;
    let err = decode(svg).err().expect("patterns must be rejected");
    assert!(err.contains("pattern"), "{err}");
}

/// Filter markup is IGNORED, not fatal: the `<filter>` definition and the
/// `filter` reference are stripped, so the element renders UNFILTERED
/// (the red pixels prove it was not dropped — usvg deletes an element
/// whose filter link dangles, which is why the references go too).
#[test]
fn svg_filters_are_stripped_and_content_renders() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <filter id="f"><feTurbulence numOctaves="4000000000"/></filter>
          <rect width="10" height="10" fill="#ff0000" filter="url(#f)"/>
        </svg>"##;
    let decoded = decode(svg).expect("filters are ignored, not fatal");
    assert_eq!(decoded.pixels.size, [10, 10]);
    assert_eq!(
        decoded.pixels.pixels[0],
        egui::Color32::from_rgb(255, 0, 0),
        "the filtered element still renders"
    );
}

/// The namespace-prefixed spelling (`<s:filter>`, `s:fe*`) is stripped
/// too: usvg resolves elements by local name in the SVG namespace.
#[test]
fn namespace_prefixed_filters_are_stripped() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg"
          xmlns:s="http://www.w3.org/2000/svg" width="10" height="10">
          <s:filter id="f"><s:feTurbulence numOctaves="4000000000"/></s:filter>
          <rect width="10" height="10" fill="#00ff00" filter="url(#f)"/>
        </svg>"##;
    let decoded = decode(svg).expect("prefixed filters are ignored too");
    assert_eq!(decoded.pixels.pixels[0], egui::Color32::from_rgb(0, 255, 0));
}

/// A dangling `filter` reference used to make usvg DROP the element
/// (`converter.rs` ignores an element whose filter link is invalid): the
/// strip removes the attribute, so the content survives.
#[test]
fn dangling_filter_reference_keeps_the_element() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
          <rect width="4" height="4" fill="#0000ff" filter="url(#missing)"/>
        </svg>"##;
    let decoded = decode(svg).expect("decode");
    assert_eq!(decoded.pixels.pixels[0], egui::Color32::from_rgb(0, 0, 255));
}

/// The strip removes definitions and references and leaves the rest of
/// the source intact (no `filter` token survives).
#[test]
fn filter_markup_is_removed_from_the_source() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <defs><filter id="f"><feGaussianBlur stdDeviation="2"/></filter></defs>
          <rect width="10" height="10" filter="url(#f)"/>
        </svg>"#;
    let sanitized = super::svg::sanitize_svg_structure(svg).expect("sanitize");
    let text = std::str::from_utf8(&sanitized).expect("utf-8");
    assert!(!text.contains("filter"), "{text}");
    assert!(!text.contains("feGaussianBlur"), "{text}");
    assert!(text.contains("<rect"), "the content survives: {text}");
}

/// CSS-spelled filters cannot be stripped textually without a full CSS
/// parser: they still reject the image, in both the `style` attribute and
/// a `<style>` rule (any case; a comment before the declaration is
/// skipped like CSS does).
///
/// The last two cases mirror `simplecss`'s own grammar exactly: it skips
/// spaces AND comments between the property name and the `:`
/// (`filter/**/:blur(1px)` is a filter), and strips a leading `*` (the IE
/// star hack) before reading the property name.
#[test]
fn rejects_svg_css_filters() {
    let cases: [&[u8]; 6] = [
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <rect width="10" height="10" style="filter:url(#f)"/>
            </svg>"#,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <style>rect { filter: url(#f) }</style>
              <rect width="10" height="10"/>
            </svg>"#,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <style>rect{FILTER:url(#f)}</style>
              <rect width="10" height="10"/>
            </svg>"#,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <style>rect{/* x */filter:url(#f)}</style>
              <rect width="10" height="10"/>
            </svg>"#,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <style>rect{filter/**/:blur(5px)}</style>
              <rect width="10" height="10"/>
            </svg>"#,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
              <rect width="10" height="10" style="*filter:blur(5px)"/>
            </svg>"#,
    ];
    for svg in cases {
        let err = decode(svg).err().expect("CSS filters must be rejected");
        assert!(err.contains("CSS filter"), "{err}");
    }
}

/// Comments, strings and selector text that merely mention `filter` are
/// not declarations: the scan must not reject a valid image (the DTD
/// episode: a raw byte scan over-rejected valid exports).
#[test]
fn css_mentions_of_filter_that_are_not_declarations_still_decode() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <style>/* filter: url(#x) */ [data-x="filter:"] rect { fill: #00ff00 }</style>
          <rect data-x="filter:" width="10" height="10" fill="#00ff00"/>
        </svg>"##;
    let decoded = decode(svg).expect("non-declarations must not reject");
    assert_eq!(decoded.pixels.pixels[0], egui::Color32::from_rgb(0, 255, 0));
}

/// A tiny chain of nested `<use>` references expands exponentially
/// inside usvg (2^40 nodes) while carrying few source tokens: the
/// structural count must reject it before usvg burns seconds of CPU.
#[test]
fn rejects_svg_use_expansion_bombs() {
    let mut defs = String::from("<rect id=\"n0\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"n{i}\"><use href=\"#n{}\"/><use href=\"#n{}\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs>{defs}</defs><use href="#n39"/>
        </svg>"##
    );
    let err = decode(svg.as_bytes()).err().expect("expansion bomb");
    assert!(err.contains("markup tokens"), "{err}");
}

/// usvg accepts namespace-less documents (`parse_tag_name` allows
/// `None | SVG_NS`), so the structural pass must too. Without the
/// namespace the bomb used to pass the check and usvg expanded it until
/// its own 1M-node cutoff.
#[test]
fn rejects_namespaceless_use_expansion_bombs() {
    let mut defs = String::from("<rect id=\"n0\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"n{i}\"><use href=\"#n{}\"/><use href=\"#n{}\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg =
        format!(r##"<svg width="10" height="10"><defs>{defs}</defs><use href="#n39"/></svg>"##);
    let err = decode(svg.as_bytes()).err().expect("expansion bomb");
    assert!(err.contains("markup tokens"), "{err}");
}

/// A `<use>` whose TARGET is another `<use>` must be followed through:
/// the old counter returned the target element alone (1 node) and never
/// resolved its href, so this 2^40 shape passed the check.
#[test]
fn rejects_use_targeting_a_use() {
    let mut defs = String::from("<rect id=\"r0\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!("<use id=\"u{i}\" href=\"#r{}\"/>", i - 1));
        defs.push_str(&format!(
            "<g id=\"r{i}\"><use href=\"#u{i}\"/><use href=\"#u{i}\"/></g>"
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs>{defs}</defs><use href="#r39"/>
        </svg>"##
    );
    let err = decode(svg.as_bytes()).err().expect("expansion bomb");
    assert!(err.contains("markup tokens"), "{err}");
}

/// Two groups referencing each other through `<use>` are NOT detected by
/// usvg's cycle guard (it only skips an immediate self reference), so a
/// tiny source would recurse until usvg's own 1M-node limit. The counter
/// must follow every expansion and trip the DEPTH cap (the cycle adds
/// one level per hop; the token cap would only fire after hundreds of
/// thousands of expansions).
#[test]
fn rejects_mutual_use_recursion() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <defs>
            <g id="a"><use href="#b"/></g>
            <g id="b"><use href="#a"/></g>
          </defs>
          <use href="#a"/>
        </svg>"##;
    let err = decode(svg)
        .err()
        .expect("mutual recursion must be rejected");
    assert!(err.contains("nesting exceeds"), "{err}");
}

/// usvg parses `href` with `svgtypes::IRI`, which skips surrounding
/// whitespace: `href=" #bomb "` expands, so the structural counter must
/// trim the value before the `#` or the same bomb slips through.
#[test]
fn rejects_whitespace_padded_use_href() {
    let mut defs = String::from("<rect id=\"n0\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"n{i}\"><use href=\"#n{}\"/><use href=\"#n{}\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs>{defs}</defs><use href=" #n39 "/>
        </svg>"##
    );
    let err = decode(svg.as_bytes())
        .err()
        .expect("padded IRI must not bypass the counter");
    assert!(err.contains("markup tokens"), "{err}");
}

/// `iri_target` must reproduce `svgtypes::IRI::from_str` byte for byte:
/// only an ASCII SPACE ends the id, only ASCII whitespace is skipped, and
/// trailing data is a parse error usvg drops (no expansion).
#[test]
fn iri_target_mirrors_svgtypes() {
    use super::svg::iri_target;
    assert_eq!(iri_target("#id"), Some("id"));
    assert_eq!(iri_target("   #id   "), Some("id"));
    assert_eq!(iri_target("#n0\t"), Some("n0\t"), "tab is an id char");
    assert_eq!(
        iri_target("#n0\u{a0}"),
        Some("n0\u{a0}"),
        "NBSP is an id char"
    );
    assert_eq!(iri_target("#n0\tx"), Some("n0\tx"), "tab inside the id");
    assert_eq!(iri_target(" #id\t "), Some("id\t"), "trailing tab + space");
    assert_eq!(iri_target("#n0 x"), None, "trailing data is a parse error");
    assert_eq!(iri_target("#"), None, "empty id");
    assert_eq!(iri_target("id"), None, "missing #");
}

/// Character references survive XML attribute normalization (`&#9;` is a
/// real tab in the parsed value), so usvg expands them while the old
/// `trim()` counter stripped the trailing tab and never matched the
/// `id` map: a 40-level doubling chain expanded un-counted. The counter
/// must follow the same value usvg resolves.
#[test]
fn rejects_character_reference_padded_use_href() {
    let mut defs = String::from("<rect id=\"n0&#9;\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"n{i}&#9;\"><use href=\"#n{}&#9;\"/><use href=\"#n{}&#9;\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs>{defs}</defs><use href="#n39&#9;"/>
        </svg>"##
    );
    let err = decode(svg.as_bytes())
        .err()
        .expect("a character-reference tab must not bypass the counter");
    assert!(err.contains("markup tokens"), "{err}");
}

/// Same bypass with NBSP (`&#xA0;`): Rust's `trim()` treats U+00A0 as
/// whitespace, `svgtypes` does not.
#[test]
fn rejects_nbsp_padded_use_href() {
    let mut defs = String::from("<rect id=\"n0&#xA0;\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"n{i}&#xA0;\"><use href=\"#n{}&#xA0;\"/><use href=\"#n{}&#xA0;\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs>{defs}</defs><use href="#n39&#xA0;"/>
        </svg>"##
    );
    let err = decode(svg.as_bytes())
        .err()
        .expect("an NBSP-padded href must not bypass the counter");
    assert!(err.contains("markup tokens"), "{err}");
}

/// A forged foreign-namespace `href` must not hide the real target: the
/// resolver follows usvg's precedence (unprefixed `href` first), not the
/// first attribute that happens to have the local name.
#[test]
fn rejects_foreign_namespace_href_decoy() {
    let mut defs = String::from("<rect id=\"small\" width=\"1\" height=\"1\"/>");
    defs.push_str("<rect id=\"b0\" width=\"1\" height=\"1\"/>");
    for i in 1..40 {
        defs.push_str(&format!(
            "<g id=\"b{i}\"><use href=\"#b{}\"/><use href=\"#b{}\"/></g>",
            i - 1,
            i - 1
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:e="urn:x" width="10" height="10">
            <defs>{defs}</defs><use e:href="#small" href="#b39"/>
        </svg>"##
    );
    let err = decode(svg.as_bytes()).err().expect("real target is a bomb");
    assert!(err.contains("markup tokens"), "{err}");
}

/// `<marker>` subtrees are re-converted per path vertex by usvg with no
/// cache: a small marker plus a long path is an unbounded render tree.
/// Markers are rejected (byte scan + structural pass).
#[test]
fn rejects_svg_markers() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <marker id="m"><rect width="1" height="1"/></marker>
          <path d="M0 0 L1 1" marker-mid="url(#m)"/>
        </svg>"#;
    let err = decode(svg).err().expect("markers must be rejected");
    assert!(err.contains("marker"), "{err}");
    // Namespace-prefixed spelling: the structural pass catches it.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg"
          xmlns:s="http://www.w3.org/2000/svg" width="10" height="10">
          <s:marker id="m"><rect width="1" height="1"/></s:marker>
        </svg>"##;
    let err = decode(svg)
        .err()
        .expect("prefixed markers must be rejected");
    assert!(err.contains("marker"), "{err}");
}

/// Every executable/SMIL element family is rejected structurally, so a
/// future parser feature can never start executing it silently. The
/// motion/animation family covers `animateMotion` and `discard` on top
/// of the original three.
#[test]
fn rejects_every_smil_animation_element() {
    for tag in [
        "animate",
        "animateTransform",
        "animateMotion",
        "set",
        "discard",
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><{tag}/></svg>"#
        );
        let err = decode(svg.as_bytes())
            .err()
            .unwrap_or_else(|| panic!("<{tag}> must be rejected"));
        assert!(err.contains(tag), "{err}");
    }
}

/// Hostile deep nesting must fail as a normal error without recursing
/// through the decode thread's 2 MiB stack (which used to abort the
/// whole process, since a stack overflow cannot be caught).
#[test]
fn deeply_nested_svg_never_overflows_the_stack() {
    let depth = 100_000;
    let mut svg = String::with_capacity(depth * 12 + 64);
    svg.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">"#);
    svg.push_str(&"<g>".repeat(depth));
    svg.push_str("</g>".repeat(depth).as_str());
    svg.push_str("</svg>");
    // Must be a normal `Err` (the nesting-depth cap), never a process
    // abort.
    assert!(decode(svg.as_bytes()).is_err());
}

/// The pre-parse depth scan must count real nesting only: sibling
/// self-closing tags, `>` inside attribute values, comments, CDATA and
/// declarations never accumulate depth, while a chain over
/// `MAX_SVG_DEPTH` is rejected BEFORE `roxmltree` (and usvg) recurse
/// into it.
#[test]
fn svg_depth_scan_counts_only_real_nesting() {
    use super::svg::check_svg_depth;
    let max = crate::config::MAX_SVG_DEPTH;
    // Siblings, self-closing tags and a `>` inside an attribute value:
    // none of them opens a nesting level.
    let ok = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" data-x="a > b">{}</svg>"#,
        r#"<rect width="1" height="1"/>"#.repeat(max * 2)
    );
    assert!(check_svg_depth(ok.as_bytes(), max).is_ok());
    // Comments and CDATA may contain `<`/`>` without counting.
    let ok = format!(
        "<svg><!--{0}--><![CDATA[{1}]]></svg>",
        "<g><g><g>".repeat(max + 1),
        "</g>".repeat(max + 1)
    );
    assert!(check_svg_depth(ok.as_bytes(), max).is_ok());
    // Real nesting over the cap is rejected.
    let deep = format!("<svg>{}", "<g>".repeat(max + 1));
    let err = check_svg_depth(deep.as_bytes(), max).unwrap_err();
    assert!(err.contains("nesting exceeds"), "{err}");
}

/// A plain `<use>` (a very common SVG idiom) still decodes.
#[test]
fn keeps_plain_svg_use() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <defs><rect id="r" width="10" height="10" fill="#00ff00"/></defs>
          <use href="#r"/>
        </svg>"##;
    let decoded = decode(svg).expect("plain use must decode");
    assert_eq!(decoded.pixels.size, [10, 10]);
    let px = decoded.pixels.pixels[0];
    assert_eq!((px.r(), px.g(), px.b()), (0, 255, 0));
}

/// Inline `data:` hrefs never resolve, so a nested SVG cannot bypass the
/// pre-scan by encoding its markup.
#[test]
fn rejects_nested_svg_data_urls() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
          <image href="data:image/svg%2Bxml;base64,PHN2Zy8%2B" width="10" height="10"/>
        </svg>"#;
    // The outer tree still parses/rasterizes, but the nested SVG is not
    // loaded: the render is just empty (no error, no recursion).
    let decoded = decode(svg).expect("outer svg decodes");
    assert_eq!(decoded.pixels.size, [10, 10]);
}
