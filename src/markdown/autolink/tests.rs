// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::scan::scan;
use crate::markdown::document::parse_events;
use pulldown_cmark::{Event, Tag};

/// `(label, destination)` of every link in the event stream.
fn links(events: &[Event<'static>]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for ev in events {
        match ev {
            Event::Start(Tag::Link { dest_url, .. }) => {
                current = Some((String::new(), dest_url.to_string()));
            }
            Event::Text(t) => {
                if let Some((label, _)) = current.as_mut() {
                    label.push_str(t);
                }
            }
            Event::End(pulldown_cmark::TagEnd::Link) => {
                if let Some(link) = current.take() {
                    out.push(link);
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn linkifies_a_bare_www_url_with_https() {
    let events = parse_events("Visita www.celticos.com ahora\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "www.celticos.com".to_string(),
            "https://www.celticos.com".to_string()
        )]
    );
    // The surrounding prose is untouched.
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "Visita "), "{texts:?}");
    assert!(texts.iter().any(|t| t == " ahora"), "{texts:?}");
}

#[test]
fn linkifies_https_urls_and_keeps_the_path() {
    let events = parse_events("(Visit https://encrypted.google.com/search?q=Markup+(business))\n")
        .expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "https://encrypted.google.com/search?q=Markup+(business)".to_string(),
            "https://encrypted.google.com/search?q=Markup+(business)".to_string()
        )]
    );
    // Plain `http` does NOT linkify: the browser only navigates https.
    let events = parse_events("http://commonmark.org\n").expect("parse");
    assert!(links(&events).is_empty(), "http must stay text");
}

#[test]
fn linkifies_only_local_file_urls() {
    for md in [
        "abre file:///Users/me/notes.md ahora\n",
        "abre file:/Users/me/notes.md ahora\n",
        "abre file://localhost/Users/me/notes.md ahora\n",
        "abre file:///tmp/notes.md#L2 ahora\n",
    ] {
        let events = parse_events(md).expect("parse");
        let found = links(&events);
        assert_eq!(found.len(), 1, "{md:?}: {found:?}");
        assert!(found[0].1.starts_with("file:"), "{md:?}: {found:?}");
    }
    // A remote share, a relative spelling, an empty path and a
    // capitalized spelling stay text (recognition is lowercase only,
    // like `www.`/`https://`).
    for md in [
        "abre file://server/share/notes.md ahora\n",
        "abre file:notes.md ahora\n",
        "abre file:/// ahora\n",
        "abre File:/Users/me/notes.md ahora\n",
    ] {
        let events = parse_events(md).expect("parse");
        let found = links(&events);
        assert!(found.is_empty(), "{md:?} must stay text: {found:?}");
    }
}

#[test]
fn linkifies_bare_domains_in_prose_with_https() {
    let events =
        parse_events("Visita celticos.com y scholar-sidekick.com, luego vuelve.\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![
            (
                "celticos.com".to_string(),
                "https://celticos.com".to_string()
            ),
            (
                "scholar-sidekick.com".to_string(),
                "https://scholar-sidekick.com".to_string()
            ),
        ]
    );
}

#[test]
fn bare_domain_accepts_paths_and_known_tlds_like_md() {
    // `agentready.md` is a real host: `.md` alone must not veto it.
    let events =
        parse_events("Mira agentready.md/leaderboard/ y example.com/docs.\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![
            (
                "agentready.md/leaderboard/".to_string(),
                "https://agentready.md/leaderboard/".to_string()
            ),
            (
                "example.com/docs".to_string(),
                "https://example.com/docs".to_string()
            ),
        ]
    );
    // `mkplandeki.pl` is a real host too.
    let events = parse_events("Mira mkplandeki.pl ahora\n").expect("parse");
    assert_eq!(links(&events).len(), 1, "{:?}", links(&events));
}

#[test]
fn bare_domain_needs_a_root_zone_tld() {
    for md in [
        "foo.toml\n",    // not a TLD
        "bar.local\n",   // reserved, not in the root zone
        "baz.invalid\n", // reserved
        "v1.2\n",        // version token
        "1.2.3.4\n",     // IPv4 is not linkified
        "qux.zzz\n",     // unknown suffix
        "example.c\n",   // single-char suffix
        "z.B.\n",        // abbreviation
        "node.js\n",     // `.js` is not a TLD
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} must stay text");
    }
}

#[test]
fn bare_domain_skips_file_like_tokens() {
    for md in [
        "abre config.rs ahora\n",
        "el README.md del proyecto\n",
        "ejecuta script.pl o setup.py\n",
        "sube logo.ai y banner.ai\n",
        "descomprime package.zip\n",
        "mira el CHANGELOG.md\n",
        "abre docs/index.md\n",
        "abre foo.py y bar.rs ahora\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert!(
            links(&events).is_empty(),
            "{md:?} is a file name, not a host: {:?}",
            links(&events)
        );
    }
    // A real host with an extension-like TLD still links.
    let events = parse_events("Mira agentready.md ahora\n").expect("parse");
    assert_eq!(links(&events).len(), 1, "{:?}", links(&events));
}

/// The file guard reads the domain label IMMEDIATELY before the TLD, not
/// the host's first label: `blog.readme.md` ends in the file name
/// `readme.md` (text), while `readme.blog.md` is a host under `.md`.
#[test]
fn file_like_guard_reads_the_label_before_the_tld() {
    for md in ["ve blog.readme.md ahora\n", "mira docs.readme.md\n"] {
        let events = parse_events(md).expect("parse");
        assert!(
            links(&events).is_empty(),
            "{md:?} ends in a file name, not a host: {:?}",
            links(&events)
        );
    }
    for md in [
        "ve readme.blog.md ahora\n",
        "mira mkplandeki.pl\n",
        "mira blog.md\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert_eq!(
            links(&events).len(),
            1,
            "{md:?} is a host under .md: {:?}",
            links(&events)
        );
    }
}

#[test]
fn bare_domain_never_links_email_domains() {
    for md in [
        "escribe a foo@bar.com\n",
        "escribe a user.name@example.com\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} must stay text");
    }
}

#[test]
fn linkifies_a_bare_domain_that_fills_a_table_cell() {
    let md = "| Domain | Score |\n| --- | --- |\n\
              | vimal-gandhi.vercel.app | 94 |\n\
              | scholar-sidekick.com | 93 |\n\
              | example.com/docs | 90 |\n\
              | example.com. | 89 |\n";
    let events = parse_events(md).expect("parse");
    let found = links(&events);
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(
        found[0],
        (
            "vimal-gandhi.vercel.app".to_string(),
            "https://vimal-gandhi.vercel.app".to_string()
        )
    );
    assert_eq!(
        found[1],
        (
            "scholar-sidekick.com".to_string(),
            "https://scholar-sidekick.com".to_string()
        )
    );
    assert_eq!(
        found[2].1, "https://example.com/docs",
        "a path stays in the destination"
    );
    assert_eq!(found[3].0, "example.com", "trailing punctuation is trimmed");
}

#[test]
fn trims_trailing_punctuation() {
    let events = parse_events("Visit www.commonmark.org.\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "www.commonmark.org".to_string(),
            "https://www.commonmark.org".to_string()
        )]
    );
    let events = parse_events("Visit www.commonmark.org/a.b.\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "www.commonmark.org/a.b".to_string(),
            "https://www.commonmark.org/a.b".to_string()
        )]
    );
    let events = parse_events("See www.celticos.com, then leave.\n").expect("parse");
    assert_eq!(links(&events)[0].0, "www.celticos.com");
    // Bare domains trim the same way.
    let events = parse_events("See celticos.com, then leave.\n").expect("parse");
    assert_eq!(links(&events)[0].0, "celticos.com");
}

#[test]
fn keeps_balanced_parentheses_and_drops_unmatched_ones() {
    let md = "www.google.com/search?q=Markup+(business)\n\n\
              www.google.com/search?q=Markup+(business)))\n\n\
              (www.google.com/search?q=Markup+(business))\n";
    let events = parse_events(md).expect("parse");
    for (label, dest) in links(&events) {
        assert!(
            label.ends_with("(business)"),
            "balanced parens kept: {label:?}"
        );
        assert_eq!(dest, format!("https://{label}"));
    }
    assert_eq!(links(&events).len(), 3);
}

#[test]
fn excludes_a_trailing_entity_reference() {
    let events = parse_events("www.google.com/search?q=commonmark&hl;\n").expect("parse");
    let found = links(&events);
    assert_eq!(
        found,
        vec![(
            "www.google.com/search?q=commonmark".to_string(),
            "https://www.google.com/search?q=commonmark".to_string()
        )]
    );
    // A real query separator (`&hl=en`) stays inside the link.
    let events = parse_events("www.google.com/search?q=commonmark&hl=en\n").expect("parse");
    assert_eq!(
        links(&events)[0].1,
        "https://www.google.com/search?q=commonmark&hl=en"
    );
}

#[test]
fn requires_a_gfm_boundary_at_the_candidate_start() {
    // Glued to an alphanumeric, the `www.` prefix itself is not a GFM
    // autolink; the whole token is still a valid bare host, so it links
    // as one (`xwww.celticos.com`).
    let events = parse_events("xwww.celticos.com\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "xwww.celticos.com".to_string(),
            "https://xwww.celticos.com".to_string()
        )]
    );
    // A scheme glued to a word is not a scheme (`foohttps://x.com`).
    let events = parse_events("foohttps://x.com\n").expect("parse");
    assert!(links(&events).is_empty(), "no boundary before https");
    // Delimiters from the GFM set start a link.
    let events = parse_events("*www.celticos.com* and (www.celticos.com)\n").expect("parse");
    assert_eq!(links(&events).len(), 2, "{:?}", links(&events));
    // A preceding `@` or `:` is not a delimiter.
    for md in ["foo@www.celticos.com\n", "foo:celticos.com\n"] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} stays text");
    }
    // Cross-run: a bare domain glued to a previous code span is not a URL.
    let events = parse_events("`foo`celticos.com\n").expect("parse");
    assert!(links(&events).is_empty(), "no boundary after a code span");
    // Even when the span's LAST character is itself a GFM delimiter:
    // the source boundary is the backtick, not the content.
    for md in [
        "`(`celticos.com\n",
        "`*`celticos.com\n",
        "` `celticos.com\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} stays text after code");
    }
    // ...but a delimiter opens one.
    let events = parse_events("(celticos.com)\n").expect("parse");
    assert_eq!(links(&events).len(), 1, "{:?}", links(&events));
}

#[test]
fn linkifies_inside_emphasis_and_headings() {
    let events = parse_events("# Docs: https://ex.com/a\n\n**www.celticos.com**\n").expect("parse");
    let found = links(&events);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].1, "https://ex.com/a");
    assert_eq!(found[1].1, "https://www.celticos.com");
}

#[test]
fn never_touches_code_links_or_image_alt() {
    // Inline code and fenced code keep the literal text.
    let events = parse_events("`www.celticos.com`\n\n```\ncelticos.com\n```\n").expect("parse");
    assert!(links(&events).is_empty(), "code stays literal: {events:?}");
    // An explicit link's label/destination is not re-linkified.
    let events = parse_events("[www.celticos.com](https://ex.com/a)\n").expect("parse");
    assert_eq!(
        links(&events),
        vec![(
            "www.celticos.com".to_string(),
            "https://ex.com/a".to_string()
        )]
    );
    let events = parse_events("[celticos.com](https://ex.com/a)\n").expect("parse");
    assert_eq!(links(&events).len(), 1, "{:?}", links(&events));
    // Image alt text stays alt text.
    let events = parse_events("![celticos.com](https://ex.com/i.png)\n").expect("parse");
    assert!(links(&events).is_empty(), "image alt: {events:?}");
}

#[test]
fn rejects_other_schemes_and_incomplete_hosts() {
    for md in [
        "ftp://celticos.com\n",      // only https/file: are recognized
        "mailto:foo@celticos.com\n", // email scheme
        "data:text/plain,hola\n",    // data scheme
        "www.\n",                    // no domain
        "https://localhost\n",       // no period in the domain
        "Visit https\n",             // not even a scheme
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} must stay text");
    }
}

#[test]
fn caps_the_links_added_per_document() {
    let count = crate::config::MAX_AUTOLINKS_PER_DOC + 50;
    let mut md = String::with_capacity(count * 10);
    for _ in 0..count {
        md.push_str("www.a.com ");
    }
    let events = parse_events(&md).expect("parse");
    assert_eq!(
        links(&events).len(),
        crate::config::MAX_AUTOLINKS_PER_DOC,
        "extra candidates stay plain text"
    );
    // The text is never lost, linkified or not.
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect();
    assert!(text.contains("www.a.com"), "{text:?}");
}

#[test]
fn scan_is_bounded_and_never_panics_on_multibyte_text() {
    // Unicode host and path: slicing must stay on char boundaries.
    let events = parse_events("Visita www.página.es/ruta/ñandú. Adiós\n").expect("parse");
    let found = links(&events);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "www.página.es/ruta/ñandú");
    // A huge run of domain lookalikes is rejected without rescanning
    // (would be O(n²) with a per-candidate full scan). The suffix is a
    // dot chain, so the bare-domain branch sees one giant invalid host.
    let mut md = String::with_capacity(200_000);
    md.push_str("www.");
    for _ in 0..50_000 {
        md.push_str("a_.");
    }
    md.push('\n');
    let events = parse_events(&md).expect("parse");
    assert!(links(&events).is_empty(), "no valid domain in the run");
    let mut md = String::with_capacity(200_000);
    for _ in 0..50_000 {
        md.push_str("a.");
    }
    md.push('\n');
    let events = parse_events(&md).expect("parse");
    assert!(links(&events).is_empty(), "no TLD in the dot chain");
    // Direct scanner calls on hostile shapes must terminate promptly.
    let _ = scan(&md, 0, None);
    let _ = scan("www.a.com", 0, Some('x'));
    let _ = scan("é", 0, None);
}

/// A run glued to a link's closing `)` is not at a GFM start boundary:
/// `[a](b)www.evil.com` must not turn the trailing text into a link.
#[test]
fn text_glued_to_a_link_close_does_not_linkify() {
    let events = parse_events("[a](https://x.dev/b)www.evil.com\n").expect("parse");
    let found = links(&events);
    assert_eq!(
        found,
        vec![("a".to_string(), "https://x.dev/b".to_string())],
        "{found:?}"
    );
}

/// A domain glued to `@` is an email spelling, never a URL, for every
/// candidate form (bare, `www.`, `https://`).
#[test]
fn at_suffixed_domains_stay_text() {
    for md in [
        "www.example.com@evil.com\n",
        "https://example.com@evil.com\n",
        "example.com@evil.com\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} must stay text");
    }
}

/// A trailing dot slips past the one-byte `@` guard (`valid_domain_span`
/// trims trailing dots), but the URL still hides a real target in its
/// userinfo: `https://example.com.@evil.com` resolves to host `evil.com`.
/// The destination validator must reject credentials, so the label never
/// links somewhere else than it reads.
#[test]
fn credential_bearing_destinations_stay_text() {
    for md in [
        "https://example.com.@evil.com\n",
        "www.example.com.@evil.com\n",
        "example.com.@evil.com\n",
        "https://example.com:@evil.com\n",
    ] {
        let events = parse_events(md).expect("parse");
        assert!(links(&events).is_empty(), "{md:?} must stay text");
    }
}

/// The URL cap counts CHARACTERS (like the navigation parser): a
/// non-ASCII URL over 8192 bytes but under 8192 chars still linkifies.
#[test]
fn multibyte_urls_are_capped_by_characters() {
    let path = "é".repeat(crate::config::MAX_URL_INPUT_CHARS / 2);
    let md = format!("https://ex.com/{path}\n");
    assert!(md.len() > crate::config::MAX_URL_INPUT_CHARS, "byte-heavy");
    assert!(
        md.chars().count() < crate::config::MAX_URL_INPUT_CHARS,
        "char-light"
    );
    let events = parse_events(&md).expect("parse");
    let found = links(&events);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, md.trim_end());
}
