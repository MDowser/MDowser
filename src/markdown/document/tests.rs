// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::plain::plain_text;
use super::{is_html_break, is_html_comment, normalize_body, parse_events, parser_options};
use pulldown_cmark::{Event, Parser, Tag};

#[test]
fn detects_html_breaks() {
    assert!(is_html_break("<br>"));
    assert!(is_html_break("<br/>"));
    assert!(is_html_break("<BR />"));
    assert!(is_html_break("</br>"));
    assert!(is_html_break("  <br clear=\"all\"> "));
}

#[test]
fn rejects_non_break_tags() {
    assert!(!is_html_break("<bracket>"));
    assert!(!is_html_break("<kbd>"));
    assert!(!is_html_break("<b>"));
    assert!(!is_html_break(""));
    assert!(!is_html_break("<br"));
}

#[test]
fn normalizes_crlf_without_touching_pipes() {
    let md = "# T\r\n\r\nHome | Docs\r\n";
    let out = normalize_body(md);
    assert!(!out.contains('\r'), "CR removed: {out:?}");
    assert!(out.contains("Home | Docs"), "menu line kept: {out:?}");
}

/// The `[#id]` heading-anchor convention (a docs-generator convention)
/// becomes a heading attribute: the title no longer shows the marker and the
/// explicit id is what `#id` links resolve to.
#[test]
fn heading_bracket_anchors_become_attributes() {
    let md = "# Project Overview\n\n## Observability [#observability]\n\n## Prompt Management [#prompts]\n\n## Quickstarts [#quickstarts]\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("## Observability {#observability}"), "{out:?}");
    assert!(!out.contains("[#observability]"), "{out:?}");
    assert!(out.contains("## Prompt Management {#prompts}"), "{out:?}");
    assert!(out.contains("## Quickstarts {#quickstarts}"), "{out:?}");
    // The heading text renders without the marker and the explicit id wins.
    let events = parse_events(&out).expect("parse");
    let mut ids = Vec::new();
    let mut text = String::new();
    for ev in &events {
        match ev {
            Event::Start(Tag::Heading { id: Some(id), .. }) => {
                ids.push(id.to_string());
            }
            Event::Text(t) => text.push_str(t),
            _ => {}
        }
    }
    assert_eq!(ids, vec!["observability", "prompts", "quickstarts"]);
    assert!(text.contains("Observability"), "{text:?}");
    assert!(!text.contains("[#"), "markers never render: {text:?}");
}

/// The bracket-anchor rewrite is conservative: code, escaped markers and
/// non-heading lines keep their literal `[#…]`.
#[test]
fn heading_bracket_anchors_leave_other_text_alone() {
    let md = "```\n## Code [#not-an-anchor]\n```\n\n    indented [#no]\n\nA paragraph [#no].\n\n## Escaped \\[#no]\n\n## Joined word[#no]\n\n## Already {#yes}\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("## Code [#not-an-anchor]"), "{out:?}");
    assert!(out.contains("indented [#no]"), "{out:?}");
    assert!(out.contains("A paragraph [#no]."), "{out:?}");
    assert!(out.contains("Escaped \\[#no]"), "{out:?}");
    assert!(out.contains("Joined word[#no]"), "{out:?}");
    assert!(out.contains("## Already {#yes}"), "{out:?}");
}

/// Trailing whitespace and tabs around the marker are fine; the rewritten
/// line keeps the heading's own text and separator.
#[test]
fn heading_bracket_anchors_trim_trailing_space() {
    let out = normalize_body("### Install   [#install]  \n").into_owned();
    assert_eq!(out, "### Install {#install}\n");
}

/// The clipboard agrees with the screen: the `[#id]` marker never reaches
/// `Copy`.
#[test]
fn heading_bracket_anchors_leave_plain_text_clean() {
    let t = plain_text("## Observability [#observability]\n\nBody.\n");
    assert!(t.contains("Observability"), "{t:?}");
    assert!(!t.contains("[#"), "marker never copied: {t:?}");
}

#[test]
fn replaces_nul_with_the_replacement_character() {
    // CommonMark/GFM "Insecure characters": U+0000 -> U+FFFD everywhere,
    // including code (the swap happens on the raw body before parsing).
    let md = "a\u{0}b\n\n```\ncode\u{0}here\n```\n";
    let out = normalize_body(md).into_owned();
    assert!(!out.contains('\0'), "NUL must not survive: {out:?}");
    assert!(out.contains("a\u{FFFD}b"), "{out:?}");
    assert!(out.contains("code\u{FFFD}here"), "{out:?}");
    let events = parse_events(&out).expect("parse");
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) | Event::Code(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect();
    assert!(!text.contains('\0'), "no NUL in events: {text:?}");
    assert!(text.contains('\u{FFFD}'), "{text:?}");
}

#[test]
fn pads_short_table_delimiter() {
    let md = "| a | b | c |\n| --- | --- |\n| 1 | 2 | 3 |\n";
    let out = normalize_body(md).into_owned();
    // Delimiter now declares 3 columns like the header.
    let delim = out.lines().nth(1).unwrap();
    assert_eq!(
        delim.split('|').count(),
        5,
        "3 cols + 2 outer empties: {delim:?}"
    );
    // And it actually parses as a 3-column table.
    let mut tables = 0;
    let mut cols = 0;
    for ev in Parser::new_ext(&out, parser_options()) {
        if let Event::Start(Tag::Table(a)) = ev {
            tables += 1;
            cols = a.len();
        }
    }
    assert_eq!(tables, 1);
    assert_eq!(cols, 3);
}

#[test]
fn leaves_correct_tables_borrowed() {
    let md = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let out = normalize_body(md);
    assert!(
        matches!(out, std::borrow::Cow::Borrowed(_)),
        "no alloc needed"
    );
}

/// Escaped pipes are cell CONTENT: a header like `| a \| b | c |` has
/// two columns. Counting raw pipes used to pad the delimiter to three,
/// after which pulldown rejected the whole (valid) table.
#[test]
fn escaped_pipes_count_as_one_cell() {
    let md = "| a \\| b | c |\n| --- | --- |\n| 1 | 2 |\n";
    let out = normalize_body(md);
    assert!(
        matches!(out, std::borrow::Cow::Borrowed(_)),
        "no delimiter fix for an escaped-pipe table: {out:?}"
    );
    let mut tables = 0;
    let mut cols = 0;
    for ev in parse_events(&out).expect("parse") {
        if let Event::Start(Tag::Table(a)) = ev {
            tables += 1;
            cols = a.len();
        }
    }
    assert_eq!(tables, 1, "a valid escaped-pipe table must render");
    assert_eq!(cols, 2);
}

/// A single line of pipes is a parser bomb for the EAGER pulldown first
/// pass (one tree item per cell before the event cap can fire) and for
/// the delimiter padding (a body-sized pad string): rejected up front.
#[test]
fn rejects_oversized_table_line() {
    let md = format!("{}\n", "|".repeat(crate::config::MAX_TABLE_COLS + 1));
    let err = parse_events(&md).expect_err("oversized table line");
    assert!(err.contains("table columns"), "{err}");
    // Hostile matching delimiter: the header alone already rejects it.
    let md = format!(
        "{}\n{}\n",
        "a|".repeat(crate::config::MAX_TABLE_COLS + 1),
        "|-|".repeat(crate::config::MAX_TABLE_COLS + 1)
    );
    assert!(parse_events(&md).is_err());
}

/// Pipe-heavy lines INSIDE a fence/indented code are code: a JSON/CSV or
/// ASCII-art line with thousands of pipes is legitimate document content
/// and must not reject the whole document (route fallthrough).
#[test]
fn oversized_pipe_line_inside_code_is_not_a_table_bomb() {
    let pipes = "|".repeat(crate::config::MAX_TABLE_COLS + 10);
    let md = format!("```\n{pipes}\n```\n");
    assert!(
        parse_events(&md).is_ok(),
        "fenced pipe line must parse as code"
    );
    // A fence with an info string is the same.
    let md = format!("```json\n{pipes}\n```\n");
    assert!(parse_events(&md).is_ok(), "fenced pipe line with info");
    // The guard still fires outside verbatim code. Indented lines stay
    // scanned on purpose: a list continuation can still be a table to
    // pulldown, so exempting them would reopen the bomb through lists.
    let md = format!("text\n{pipes}\n");
    assert!(
        parse_events(&md).is_err(),
        "plain table line stays rejected"
    );
}

/// A backtick "fence" whose info string contains a backtick is NOT a
/// fence (CommonMark/pulldown-cmark keep the line as paragraph text):
/// the pre-parse guard must still see the following lines, or a
/// 4097-column table bomb rides in behind it.
#[test]
fn invalid_backtick_fence_does_not_hide_a_table_bomb() {
    let pipes = "|".repeat(crate::config::MAX_TABLE_COLS + 1);
    let md = format!("``` x`\n{pipes}\n");
    assert!(
        parse_events(&md).is_err(),
        "the guard must see the pipe line behind the invalid fence"
    );
    // Tilde fences have no info-string backtick restriction: the same
    // shape is real code and must stay accepted.
    let md = format!("~~~ x`\n{pipes}\n~~~\n");
    assert!(parse_events(&md).is_ok(), "tilde fence hides the code");
}

/// A fence opened inside a BLOCKQUOTE stops at the quote's end: an unquoted
/// line is not code, so a table bomb behind `> ``` was hidden from the guard
/// by the old "everything after an opener is code" model (pulldown ends the
/// code block and the quote, then parses the line as a paragraph).
#[test]
fn quoted_fence_does_not_hide_an_unquoted_table_bomb() {
    let pipes = "|".repeat(crate::config::MAX_TABLE_COLS + 1);
    let md = format!("> ```\n{pipes}\n");
    assert!(
        parse_events(&md).is_err(),
        "the unquoted pipe line must reach the guard"
    );
    // Quoted content stays code: the same line WITH the quote prefix is
    // inside the fenced block and is accepted (and really parses as code,
    // not as a table).
    let md = format!("> ```\n> {pipes}\n> ```\n");
    let events = parse_events(&md).expect("quoted pipe line is verbatim code");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Table(_)))),
        "the quoted pipe line must parse as code, not a table"
    );
}

/// A fence opened on a list item's first line (`- ```json`) only covers
/// lines indented into the item: an unindented table bomb after it must
/// still be rejected (pulldown ends the item there).
#[test]
fn list_item_fence_does_not_hide_an_unindented_table_bomb() {
    let pipes = "|".repeat(crate::config::MAX_TABLE_COLS + 1);
    let md = format!("- ```json\n{pipes}\n");
    assert!(
        parse_events(&md).is_err(),
        "the unindented pipe line must reach the guard"
    );
    // Indented content is inside the item's code block and is accepted (and
    // really parses as code, not as a table).
    let md = format!("- ```json\n  {pipes}\n  ```\n");
    let events = parse_events(&md).expect("indented pipe line is verbatim code");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Table(_)))),
        "the indented pipe line must parse as code, not a table"
    );
}

/// A card link split by an exporter (`[label` blank `body](url)`) is
/// joined into ONE link: the literal `[` / `](url)` leak is gone.
#[test]
fn joins_links_split_by_a_blank_line() {
    let md = "[![icon](https://ex.com/i.svg) Vendor<h3>Name</h3>\n\nDescription body](https://ex.com/a)\n";
    let out = normalize_body(md).into_owned();
    assert!(!out.contains("\n\n"), "blank line removed: {out:?}");
    assert!(
        out.contains("Vendor Name Description body](https://ex.com/a)"),
        "inline tags are transparent: {out:?}"
    );
    assert!(!out.contains("<h3>"), "tags are stripped: {out:?}");
    let events = parse_events(&out).expect("parse");
    let links = events
        .iter()
        .filter(|e| matches!(e, Event::Start(Tag::Link { .. })))
        .count();
    assert_eq!(links, 1, "one joined link: {events:?}");
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect();
    assert!(text.contains("Name"), "{text:?}");
    assert!(text.contains("Description body"), "{text:?}");
    assert!(!text.contains("]("), "no leaked close: {text:?}");
}

/// Ordinary paragraph breaks are never inside a link span: the body stays
/// byte-for-byte (and borrowed).
#[test]
fn leaves_blank_lines_outside_links_borrowed() {
    let md = "[a](https://ex.com/x)\n\nparagraph\n";
    assert!(matches!(normalize_body(md), std::borrow::Cow::Borrowed(_)));
}

/// `[the docs]` is a bare reference (no `](`): a later `](url)` must not
/// match across it and swallow an unrelated blank line.
#[test]
fn does_not_join_across_a_bare_reference() {
    let md = "See [the docs]\n\nmore text](https://ex.com/x)\n";
    assert!(matches!(normalize_body(md), std::borrow::Cow::Borrowed(_)));
}

/// A split-link lookalike inside fenced, indented OR inline code is left
/// alone. Inline code matters: an unmatched `` `[` `` must not be matched
/// by a later `](` that is ordinary text.
#[test]
fn never_joins_links_inside_code() {
    let fenced = "```\n[foo\n\nbar](https://ex.com/x)\n```\n";
    assert!(matches!(
        normalize_body(fenced),
        std::borrow::Cow::Borrowed(_)
    ));
    let indented = "text\n\n    [foo\n\n    bar](https://ex.com/x)\n";
    assert!(matches!(
        normalize_body(indented),
        std::borrow::Cow::Borrowed(_)
    ));
    // `[` inside an inline code span, `](` in ordinary text: never joined.
    let inline = "Use the key `[`\n\nand the text ](https://ex.com/x)\n";
    assert!(matches!(
        normalize_body(inline),
        std::borrow::Cow::Borrowed(_)
    ));
}

/// A crafted body of unmatched backtick runs (distinct lengths, no
/// closers) must not turn the inline-code masking into O(n^2): the scan
/// budget bounds it. The `](` + blank line force the join pass to run.
#[test]
fn inline_code_masking_is_bounded() {
    let mut md = String::new();
    for len in 1..=800 {
        md.push_str(&"`".repeat(len));
        md.push('x');
    }
    md.push_str("\n\n[a](https://ex.com/x)\n");
    // ~320 KiB of backticks: without the budget every opener rescanned to
    // the end. Must return promptly and never panic.
    let _ = normalize_body(&md);
}

/// A "blank" line containing spaces or tabs still separates the link text:
/// exporters and editors leave trailing whitespace behind.
#[test]
fn joins_links_across_a_whitespace_only_line() {
    let md = "[label\n   \nbody](https://ex.com/x)\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("[label body](https://ex.com/x)"), "{out:?}");
    let md = "[label\n\t\nbody](https://ex.com/x)\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("[label body](https://ex.com/x)"), "{out:?}");
}

/// A table-lookalike inside fenced or indented code must stay
/// byte-for-byte: the delimiter fix used to pad it, corrupting both the
/// rendered code and the clipboard text.
#[test]
fn never_pads_inside_code_blocks() {
    let fenced = "```\n| a | b | c |\n| --- | --- |\n```\n";
    assert!(
        matches!(normalize_body(fenced), std::borrow::Cow::Borrowed(_)),
        "no fix inside a backtick fence"
    );
    let tilded = "~~~\n| a | b | c |\n| --- | --- |\n~~~\n";
    assert!(
        matches!(normalize_body(tilded), std::borrow::Cow::Borrowed(_)),
        "no fix inside a tilde fence"
    );
    let indented = "text\n\n    | a | b | c |\n    | --- | --- |\n";
    assert!(
        matches!(normalize_body(indented), std::borrow::Cow::Borrowed(_)),
        "no fix inside indented code"
    );
    // And the clipboard keeps the original delimiter row.
    let t = plain_text(fenced);
    assert!(t.contains("| --- | --- |"), "copy keeps the code: {t:?}");
}

/// A real table right AFTER a closed fence is still padded.
#[test]
fn still_pads_tables_after_a_closed_fence() {
    let md = "```\ncode\n```\n| a | b | c |\n| --- | --- |\n";
    let out = normalize_body(md).into_owned();
    let delim = out.lines().nth(4).unwrap();
    assert_eq!(
        delim.split('|').count(),
        5,
        "table after the fence padded: {delim:?}"
    );
}

#[test]
fn menu_line_is_not_a_table() {
    let md = "Home | Docs | About\n";
    let mut tables = 0;
    for ev in Parser::new_ext(md, parser_options()) {
        if let Event::Start(Tag::Table(_)) = ev {
            tables += 1;
        }
    }
    assert_eq!(tables, 0, "menu must stay a paragraph");
}

#[test]
fn expands_colon_and_bang_admonitions() {
    // `:::` containers become alert blockquotes.
    let md = "::: tip\nA helpful tip.\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("> [!TIP]"), "{out:?}");
    assert!(out.contains("> A helpful tip."), "{out:?}");
    let events = parse_events(&out).expect("parse");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::BlockQuote(_)))),
        "renders as a blockquote callout: {events:?}"
    );
    // `!!!` with a quoted title and indented body.
    let mk = "!!! warning \"Careful\"\n    Danger ahead.\n";
    let out = normalize_body(mk).into_owned();
    assert!(out.contains("> [!WARNING] Careful"), "{out:?}");
    assert!(out.contains("> Danger ahead."), "{out:?}");
}

#[test]
fn leaves_unknown_or_malformed_containers_literal() {
    // Unknown type: never reinterpreted.
    let md = "::: mystery\nbody\n:::\n";
    assert_eq!(normalize_body(md).as_ref(), md);
    // No closer: the source is preserved.
    let md = "::: note\nbody\n";
    assert_eq!(normalize_body(md).as_ref(), md);
    // Inside a fenced code block: never touched.
    let md = "```\n::: tip\n:::\n```\n";
    assert_eq!(normalize_body(md).as_ref(), md);
}

/// A fence INSIDE a `:::` admonition body may contain a `:::` line: it
/// is code, not the closer. The old collector ended the admonition
/// there, leaving the fence unclosed and the trailing lines literal.
#[test]
fn colon_admonition_ignores_colons_inside_a_body_fence() {
    let md = "::: note\nx\n```\n:::\n```\ny\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("> [!NOTE]"), "{out:?}");
    assert!(out.contains("> x"), "{out:?}");
    assert!(
        out.contains("> y"),
        "after the fence still in the body: {out:?}"
    );
    // The whole block renders as one callout; `y` is not a stray line.
    let events = parse_events(&out).expect("parse");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::BlockQuote(_)))),
        "{events:?}"
    );
}

#[test]
fn callout_markers_are_stripped_from_plain_text() {
    // A `:::` admonition's marker must not leak into the clipboard.
    let md = "::: tip\nHelpful.\n:::\n";
    let t = plain_text(md);
    assert!(t.contains("Helpful."), "{t:?}");
    assert!(!t.contains("[!TIP]"), "marker stripped: {t:?}");
    // An alert written directly is stripped too.
    let t = plain_text("> [!NOTE]\n> Body\n");
    assert!(t.contains("Body"), "{t:?}");
    assert!(!t.contains("[!NOTE]"), "{t:?}");
    // The marker is only stripped inside a quote: plain text keeps it.
    let t = plain_text("[!NOTE] is the marker syntax\n");
    assert!(
        t.contains("[!NOTE]"),
        "outside a quote it is content: {t:?}"
    );
    // A NESTED quote renders the same badge, so Copy strips the marker at
    // any depth (it used to strip only the innermost `>` prefix and paste
    // the marker).
    let t = plain_text("> > [!WARNING]\n> > Nested body\n");
    assert!(t.contains("Nested body"), "{t:?}");
    assert!(!t.contains("[!WARNING]"), "marker stripped at depth: {t:?}");
}

/// The viewer drops a leaked first-block accessibility skip link; the
/// clipboard must agree instead of pasting a line that is not on screen.
#[test]
fn plain_text_drops_a_leaked_first_block_skip_link() {
    let t = plain_text("[Skip to content](#main-content)\n\n# Body\n");
    assert!(!t.contains("Skip to content"), "{t:?}");
    assert!(t.contains("Body"), "{t:?}");
    // Same words as a plain paragraph: content, kept.
    let t = plain_text("Skip to content\n\n# Body\n");
    assert!(t.contains("Skip to content"), "{t:?}");
    // Not the first block: kept.
    let t = plain_text("# Body\n\n[Skip to content](#main-content)\n");
    assert!(t.contains("Skip to content"), "{t:?}");
    // Nested inside a quote: not the first TOP-LEVEL paragraph, kept.
    let t = plain_text("> [Skip to content](#main-content)\n");
    assert!(t.contains("Skip to content"), "{t:?}");
    // Spanish variant with the trailing arrow the exporter appends.
    let t = plain_text("[Saltar al contenido ↓](#main)\n\nBody\n");
    assert!(!t.contains("Saltar"), "{t:?}");
    // A non-fragment link is a real link: kept.
    let t = plain_text("[Skip to content](https://ex.com/other)\n\nBody\n");
    assert!(t.contains("Skip to content"), "{t:?}");
    // Text outside the link disqualifies even when the label matches:
    // the viewer only drops paragraphs whose every fragment is a link.
    let t = plain_text("Skip to content [](#main)\n\nBody\n");
    assert!(t.contains("Skip to content"), "{t:?}");
    let t = plain_text("[Skip to content](#main) more\n\nBody\n");
    assert!(t.contains("Skip to content"), "{t:?}");
}

/// The marker prefix check must be boundary-safe: a multi-byte character
/// straddling the marker length used to panic the fetch thread (caught
/// by `guard_panics`, so the whole document failed).
#[test]
fn alert_stripping_never_panics_on_multibyte_lines() {
    // "Hello é" is 8 bytes; byte 7 lands inside the two-byte `é`.
    let t = plain_text("> [!NOTE]\n> Hello é\n");
    assert!(t.contains("Hello é"), "{t:?}");
    let t = plain_text("> [!TIP] café\n");
    assert!(t.contains("café"), "{t:?}");
}

/// A `> [!NOTE]` line inside fenced (or indented) code is literal code:
/// the viewer renders it literally and the code card copies it, so the
/// whole-page clipboard must keep it too.
#[test]
fn alert_marker_stripping_skips_code() {
    let t = plain_text("```\n> [!NOTE]\n```\n");
    assert!(t.contains("> [!NOTE]"), "{t:?}");
    let t = plain_text("    > [!TIP]\n");
    assert!(t.contains("> [!TIP]"), "{t:?}");
    // Outside code it is still stripped.
    let t = plain_text("> [!NOTE]\n> Body\n");
    assert!(!t.contains("[!NOTE]"), "{t:?}");
}

/// The fetch path normalizes once and calls `plain_text_normalized`; the
/// clipboard text must be identical to the raw wrapper.
#[test]
fn normalized_plain_text_matches_the_raw_entry_point() {
    let md = "> [!NOTE]\n> Hello :rocket: `code`\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    assert_eq!(
        plain_text(md),
        super::plain_text_normalized(&super::normalize_body(md))
    );
}

/// The fetch path derives Copy text from the OWNED events (one parse); it
/// must be byte-identical to the raw-body oracle (`plain_text_normalized`,
/// kept for the raw entry point) on every construct: callouts with split
/// markers, code, skip links, tables, tasks, footnotes, comments, metadata,
/// autolinks and emoji shortcodes.
#[test]
fn event_plain_text_matches_the_body_oracle() {
    const DOCS: &[&str] = &[
        "",
        "Plain paragraph with **bold** and `code`.\n",
        "> [!NOTE]\n> Hello :rocket: `code`\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n",
        "> [!TIP] Inline marker on the same line\n",
        "> > [!WARNING] nested quote\n",
        "> [!NOTE]\n",
        "> [!note] lowercase marker\n> Body\n",
        "> [!NOTE]no space after marker\n",
        "```\n> [!NOTE]\n:rocket:\n```\n",
        "    > [!TIP]\n",
        "[Skip to content](#content)\n\nBody text\n",
        "[Skip to main content ↓](#main)\n",
        "# Heading\n\n- [x] done\n- [ ] pending\n\n1. first\n2. second\n",
        "Text with a footnote[^1].\n\n[^1]: The note.\n",
        "<!-- hidden -->\n\nvisible <!-- inline --> text\n",
        "+++\nkey: v\n+++\n\nMore\n",
        "Visit www.example.com or https://example.com/x now.\n",
        "<div>\nraw html\n</div>\n\nInline <br> break.\n",
        "---\n\nAfter rule\n",
        "Term\n\n[![badge](https://ex.com/b.svg)](https://ex.com)\n",
        "Nested <Tabs>\n<Tab label=\"A\">\n\nContent A\n\n</Tab>\n</Tabs>\n",
        "[link](https://ex.com) and ![alt text](https://ex.com/i.png)\n",
    ];
    for md in DOCS {
        let normalized = super::normalize_body(md);
        let expected = super::plain_text_normalized(&normalized);
        let events = super::parse_events(&normalized).expect("parse");
        let actual = super::plain_text_from_events(&events);
        assert_eq!(actual, expected, "plain text diverged for {md:?}");
    }
}

#[test]
fn skips_pluses_metadata_in_plain_text() {
    // `+++` blocks are metadata by design: skipped.
    let md2 = "Text\n\n+++\nkey: v\n+++\n\nMore\n";
    let t2 = plain_text(md2);
    assert!(!t2.contains("key"), "plus metadata hidden: {t2:?}");
    assert!(t2.contains("More"), "body kept: {t2:?}");
}

#[test]
fn keeps_leading_yaml_like_content() {
    // A leading `---` block without a `:` mapping is content, not
    // metadata (frontmatter::split returns it intact and the parser no
    // longer enables YAML metadata blocks).
    let md = "---\nhello\n---\n# Body\n";
    let t = plain_text(md);
    assert!(t.contains("hello"), "content kept: {t:?}");
    assert!(t.contains("Body"), "body kept: {t:?}");
    // A closerless leading `---` is a thematic break + text.
    let t2 = plain_text("---\n\nRemember: https\n");
    assert!(t2.contains("Remember: https"), "content kept: {t2:?}");
}

#[test]
fn expands_emoji_shortcodes_outside_code() {
    let events = super::parse_events("Ship :rocket: but `:rocket:` stays\n").expect("parse");
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.to_string()),
            Event::Code(c) => Some(c.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains('🚀')),
        "text shortcode expanded: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == ":rocket:"),
        "code span untouched: {texts:?}"
    );
    // Fenced code blocks are never expanded either.
    let events = super::parse_events("```\n:rocket:\n```\n").expect("parse");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Text(t) if t.as_ref() == ":rocket:\n")),
        "fenced code untouched: {events:?}"
    );
}

#[test]
fn parse_events_is_static_and_lossless() {
    let events = super::parse_events("# Hi\n\n- a\n").expect("parse");
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Start(Tag::Heading { .. }))));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Text(t) if t.as_ref() == "Hi")));
    // Owned events must render a leading `---` block as content.
    let events = super::parse_events("---\nhello\n---\n# B\n").expect("parse");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Text(t) if t.contains("hello"))),
        "leading block kept: {events:?}"
    );
}

/// The event stream amplifies the body (one `Event` per paragraph piece);
/// a hostile dense document must be rejected instead of allocating
/// hundreds of MB.
#[test]
fn parse_events_rejects_event_bombs() {
    // ~1.5M events (Start/Text/End per paragraph) > MAX_EVENTS_PER_DOC.
    let dense = "a\n\n".repeat(crate::config::MAX_EVENTS_PER_DOC / 2);
    let err = super::parse_events(&dense).expect_err("event bomb");
    assert!(err.contains("parsed events"), "{err}");
    // A normal document stays far below the cap.
    assert!(super::parse_events("# Hi\n\nText\n").is_ok());
}

/// Copy text keeps the literal source inside fenced code blocks, exactly
/// like the viewer and the code card's Copy button.
#[test]
fn plain_text_keeps_code_blocks_literal() {
    let t = plain_text("```\n:rocket:\n```\n");
    assert!(t.contains(":rocket:"), "code block untouched: {t:?}");
    assert!(!t.contains('🚀'), "no expansion inside a fence: {t:?}");
    // Indented code too.
    let t = plain_text("    :rocket:\n");
    assert!(t.contains(":rocket:"), "indented code untouched: {t:?}");
}

/// Block HTML is shown as inert source by the viewer: the clipboard keeps
/// its text instead of dropping whole content lines.
#[test]
fn plain_text_keeps_html_block_content() {
    let t = plain_text("<div>\nImportant\n</div>\n");
    assert!(t.contains("Important"), "html block content kept: {t:?}");
}

#[test]
fn extracts_readable_text() {
    let md = "# Love\n\nHello **brave** `world`.\n\n- [ ] milk\n- [x] bread\n";
    let t = plain_text(md);
    assert!(t.contains("Love\n"), "heading text + break: {t:?}");
    assert!(t.contains("Hello brave world."), "markup stripped: {t:?}");
    assert!(t.contains("[ ] milk"), "task open: {t:?}");
    assert!(t.contains("[x] bread"), "task done: {t:?}");
}

/// Copy text matches what the viewer renders: shortcodes expand in text
/// nodes but code spans keep the literal source.
#[test]
fn plain_text_expands_emoji_like_the_viewer() {
    let t = plain_text("Ship :rocket: and `:rocket:`\n");
    assert!(t.contains('🚀'), "text shortcode expanded: {t:?}");
    assert!(t.contains(":rocket:"), "code span untouched: {t:?}");
}

/// Dash bullet lists (`- item`, the marker most documents use) parse as
/// lists in every common context: top level, nested, after a paragraph
/// without a blank line, inside quotes, and with task markers. The
/// renderer then draws every unordered item with the document bullet
/// `·` (see `viewer::markdown`).
#[test]
fn dash_lists_parse_in_every_common_context() {
    for md in [
        "- a\n- b\n",
        "text\n- a\n- b\n",
        "> - a\n> - b\n",
        "  - a\n  - b\n",
        "- [ ] task\n- [x] done\n",
        "- a\n  - a1\n- b\n",
    ] {
        let events = parse_events(&normalize_body(md)).expect("parse");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Start(Tag::List(_)))),
            "dash list must parse as a list: {md:?} -> {events:?}"
        );
        // The source marker itself never leaks as text: only the item
        // content is in the event stream.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Text(t) if t.trim_start().starts_with("- "))),
            "literal dash marker must not leak: {md:?} -> {events:?}"
        );
    }
}

#[test]
fn detects_html_comments() {
    assert!(is_html_comment("<!-- agent-instructions -->"));
    assert!(is_html_comment("  <!-- indented up to 3 spaces -->\n"));
    assert!(is_html_comment("<!-- multi\nline -->"));
    // An unterminated comment block takes the rest of the document:
    // CommonMark feeds it to the block and a browser hides it.
    assert!(is_html_comment("<!-- forever\n"));
    assert!(!is_html_comment("<br>"));
    assert!(!is_html_comment("<div>"));
    assert!(!is_html_comment("<!DOCTYPE html>"));
    assert!(!is_html_comment("<![CDATA[x]]>"));
    assert!(!is_html_comment(""));
}

/// HTML comments are invisible: the block form and the inline form leave
/// nothing in the clipboard (not even a space), while code keeps a
/// literal `<!--` and transparent wrappers keep their content.
#[test]
fn plain_text_drops_html_comments() {
    let t = plain_text("Antes\n\n<!-- agent-instructions\nno leer\n-->\n\nDespués\n");
    assert_eq!(t, "Antes\nDespués\n", "block comment dropped: {t:?}");
    let t = plain_text("foo<!-- oculto -->bar\n");
    assert_eq!(t, "foobar\n", "inline comment leaves no gap: {t:?}");
    let t = plain_text("foo <!-- oculto\nmas --> bar\n");
    assert_eq!(t, "foo  bar\n", "multi-line inline comment dropped: {t:?}");
    let t = plain_text("```\n<!-- keep -->\n```\n");
    assert!(
        t.contains("<!-- keep -->"),
        "code fence keeps literal: {t:?}"
    );
    let t = plain_text("`<!-- keep -->`\n");
    assert!(
        t.contains("<!-- keep -->"),
        "code span keeps literal: {t:?}"
    );
    let t = plain_text("<div>\n<!-- inner -->\nImportant\n</div>\n");
    assert!(t.contains("Important"), "wrapper content kept: {t:?}");
    assert!(!t.contains("inner"), "comment is dropped: {t:?}");
    assert!(!t.contains("div"), "tags are transparent: {t:?}");
}

/// Component markers are structure, not text: they never reach the
/// clipboard, while every panel's content still does.
#[test]
fn plain_text_drops_component_markers() {
    let t = plain_text(
        "<Tabs>\n<Tab label=\"A\">\nBody A\n</Tab>\n<Tab label=\"B\">\nBody B\n</Tab>\n</Tabs>\n",
    );
    assert!(!t.contains("mdowser-cmp"), "no marker leaks: {t:?}");
    assert!(
        t.contains("Body A") && t.contains("Body B"),
        "both panels copy: {t:?}"
    );
}

/// MDX components reach the clipboard like the viewer: a callout's body and
/// a wrapped component's content survive, the tags and the marker do not.
#[test]
fn plain_text_handles_mdx_components_and_tags() {
    let t = plain_text(
        "<Note title=\"Heads up\">\nBody **bold**.\n</Note>\n\n<Card href=\"x\">\nWrapper body\n</Card>\n",
    );
    assert!(t.contains("Body bold."), "callout body kept: {t:?}");
    assert!(t.contains("Wrapper body"), "wrapper content kept: {t:?}");
    assert!(!t.contains('<'), "no tag leaks: {t:?}");
    assert!(!t.contains("Note"), "component name is gone: {t:?}");
    assert!(!t.contains("Card"), "component name is gone: {t:?}");
}

/// `:::tabs` / `:::tab` emit the same markers as `<Tabs>` / `<Tab>`.
#[test]
fn colon_tabs_match_jsx_markers() {
    let md = ":::tabs\n:::tab Uno\nContent A\n:::\n:::tab Dos\nContent B\n:::\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tabs\""), "{out:?}");
    assert!(out.contains("data=\"tab|Uno\""), "{out:?}");
    assert!(out.contains("data=\"tab|Dos\""), "{out:?}");
    assert!(
        out.contains("Content A") && out.contains("Content B"),
        "{out:?}"
    );
    assert!(!out.contains(":::tab"), "no fence leaks: {out:?}");
}

/// `:::tab` labels accept bare, quoted and `label=`/`title=` spellings.
#[test]
fn colon_tab_label_forms() {
    for (md, label) in [
        (":::tabs\n:::tab Uno\nx\n:::\n:::\n", "tab|Uno"),
        // Spaces ride percent-encoded in the marker wire format (the viewer
        // decodes them); the label still reads "Dos tres" on screen.
        (
            ":::tabs\n:::tab \"Dos tres\"\nx\n:::\n:::\n",
            "tab|Dos%20tres",
        ),
        (
            ":::tabs\n:::tab label=\"Cuatro\"\nx\n:::\n:::\n",
            "tab|Cuatro",
        ),
        (":::tabs\n:::tab title='Cinco'\nx\n:::\n:::\n", "tab|Cinco"),
    ] {
        let out = normalize_body(md).into_owned();
        assert!(
            out.contains(&format!("data=\"{label}\"")),
            "{md:?} -> {out:?}"
        );
    }
}

/// `:::code-group` splits fenced children like `<CodeGroup>`.
#[test]
fn colon_code_group_splits_fences() {
    let md = ":::code-group\n```js title=\"app.js\"\ncode\n```\n```py\nmore\n```\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tabs\""), "{out:?}");
    assert!(out.contains("data=\"tab|app.js\""), "{out:?}");
    assert!(out.contains("data=\"tab|py\""), "{out:?}");
    // Dash/case variants map too.
    let md = ":::CodeGroup\n```js\nx\n```\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tab|js\""), "{out:?}");
}

/// `:::steps`, `:::card` and `:::accordion`/`:::details` match their JSX.
#[test]
fn colon_steps_card_accordion_match_jsx() {
    let md = ":::steps\n:::step First\nDo it.\n:::\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"steps\""), "{out:?}");
    assert!(out.contains("data=\"step|First\""), "{out:?}");
    let md = ":::card Mi tarjeta\nBody.\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"card\""), "{out:?}");
    assert!(out.contains("**Mi tarjeta**"), "{out:?}");
    let md = ":::card title=\"T\" href=\"https://ex.com/x\"\nBody.\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("[T](https://ex.com/x)"), "{out:?}");
    // `href` alone carries no title, like `<Card href>`: no head line.
    let md = ":::card href=\"https://ex.com/x\"\nBody.\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"card\""), "{out:?}");
    assert!(
        !out.contains("href="),
        "no pair leaks into the card: {out:?}"
    );
    assert!(out.contains("Body."), "{out:?}");
    for opener in [":::accordion Título", ":::details Título", ":::sec Título"] {
        let md = format!("{opener}\nBody.\n:::\n");
        let out = normalize_body(&md).into_owned();
        assert!(out.contains("data=\"sec"), "{md:?} -> {out:?}");
        assert!(out.contains("Título"), "{md:?} -> {out:?}");
    }
}

/// Structural `:::` blocks are careful: unknown names, missing closers and
/// fenced code stay literal, and admonitions still win their own names.
#[test]
fn colon_structural_stays_literal_when_malformed() {
    let md = ":::mystery\nbody\n:::\n";
    assert_eq!(normalize_body(md).as_ref(), md);
    // No outer closer: the outer opener stays literal while the closed inner
    // block still expands (same partial-expansion rule as nested
    // admonitions, which never reinterpret the whole block).
    let md = ":::tabs\n:::tab A\nBody\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.starts_with(":::tabs\n"), "outer stays literal: {out:?}");
    assert!(out.contains("data=\"tab|A\""), "inner expands: {out:?}");
    assert!(!out.contains(":::tab "), "no inner fence leaks: {out:?}");
    let md = "```\n:::tabs\n:::\n```\n";
    assert_eq!(normalize_body(md).as_ref(), md, "code untouched");
    let md = "  :::tabs\n  :::tab A\n  x\n  :::\n  :::\n";
    assert_eq!(normalize_body(md).as_ref(), md, "indented stays literal");
    // A `:::` line inside a body fence is code, not the closer.
    let md = ":::tabs\n:::tab A\n```\n:::\n```\nAfter\n:::\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tab|A\""), "{out:?}");
    assert!(out.contains("After"), "{out:?}");
    // Admonitions keep working, including inside a structural body.
    let md = ":::tabs\n:::tab A\n::: tip\nHelpful.\n:::\n:::\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tab|A\""), "{out:?}");
    assert!(out.contains("> [!TIP]"), "{out:?}");
}

/// A `!!!` admonition inside a structural body takes no `:::` closer, so it
/// must not steal the outer block's depth (the outer block used to stay
/// literal past one).
#[test]
fn colon_structural_keeps_bang_admonitions_inside() {
    let md = ":::tabs\n:::tab A\n!!! warning \"Careful\"\n    Danger ahead.\n:::\n:::\n";
    let out = normalize_body(md).into_owned();
    assert!(out.contains("data=\"tab|A\""), "{out:?}");
    assert!(out.contains("> [!WARNING] Careful"), "{out:?}");
    assert!(out.contains("Danger ahead."), "{out:?}");
}

/// Structural `:::` markers are structure like JSX: the clipboard keeps
/// every panel's text and never the fences.
#[test]
fn plain_text_keeps_colon_structural_content() {
    let t = plain_text(":::tabs\n:::tab A\nBody A\n:::\n:::tab B\nBody B\n:::\n:::\n");
    assert!(!t.contains("mdowser-cmp"), "no marker leaks: {t:?}");
    assert!(!t.contains(":::"), "no fence leaks: {t:?}");
    assert!(t.contains("Body A") && t.contains("Body B"), "{t:?}");
}
