// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::document::title`: the page-title fallback from the first H1.
//!
//! A document's own title is the YAML frontmatter `title` when it has
//! one; otherwise the text of its FIRST level-1 heading names the page
//! (tab rail, OS window, page-info card, bookmarks/history). Like the
//! clipboard text and the anchor index, this is derived ONCE per fetch
//! from the same owned events the viewer renders, off the UI thread.

use super::*;
use pulldown_cmark::HeadingLevel;

/// Text of the document's FIRST `#` heading, when it has one.
///
/// - One linear walk, returning at that heading's end: even a 1M-event
///   document only costs the walk to its first H1.
/// - `Text` and inline `Code` contribute their content (an inline image's
///   alt text included, exactly like the anchor slugs), soft/hard breaks
///   and `<br>` become one space; every other event is markup.
/// - Bounded by `config::MAX_TITLE_CHARS` (the frontmatter-title cap,
///   ellipsis included): collection STOPS at the cap instead of building
///   a body-sized string first.
/// - `None` when there is no level-1 heading or nothing printable remains
///   (`#` alone, a heading of only whitespace): both mean "no title".
pub fn first_h1_title(events: &[Event<'static>]) -> Option<String> {
    let cap = crate::config::MAX_TITLE_CHARS;
    let mut in_h1 = false;
    let mut text = String::new();
    let mut chars = 0usize;
    let mut truncated = false;
    for ev in events {
        match ev {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => {
                in_h1 = true;
            }
            Event::End(TagEnd::Heading(_)) if in_h1 => {
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                return Some(if truncated {
                    // `truncate_chars` caps INCLUDING its ellipsis, so a
                    // string at exactly the cap would pass through whole:
                    // force the overflow so the cut is visible.
                    if text.chars().count() >= cap {
                        let mut forced = text.to_string();
                        forced.push('.');
                        crate::util::truncate_chars(&forced, cap).into_owned()
                    } else {
                        crate::util::truncate_chars(text, cap).into_owned()
                    }
                } else {
                    text.to_string()
                });
            }
            _ if !in_h1 => {}
            Event::Text(t) | Event::Code(t) => {
                if chars >= cap {
                    truncated = true;
                    continue;
                }
                let mut it = t.chars();
                for _ in 0..(cap - chars) {
                    match it.next() {
                        Some(c) => {
                            text.push(c);
                            chars += 1;
                        }
                        None => break,
                    }
                }
                // Content past the cap: the title is truncated.
                if chars >= cap && it.next().is_some() {
                    truncated = true;
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if chars < cap {
                    text.push(' ');
                    chars += 1;
                } else {
                    truncated = true;
                }
            }
            Event::InlineHtml(h) if is_html_break(h) => {
                if chars < cap {
                    text.push(' ');
                    chars += 1;
                } else {
                    truncated = true;
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::{normalize_body, parse_events};
    use super::first_h1_title;
    use crate::config::MAX_TITLE_CHARS;

    fn title_of(md: &str) -> Option<String> {
        let events = parse_events(&normalize_body(md)).expect("parse");
        first_h1_title(&events)
    }

    #[test]
    fn first_h1_names_the_page() {
        assert_eq!(
            title_of("# Carla Stefania Salvador Robelly\n\nBio text.").as_deref(),
            Some("Carla Stefania Salvador Robelly")
        );
        // Setext H1 is still a level-1 heading.
        assert_eq!(
            title_of("My Notes\n========\n\nBody.").as_deref(),
            Some("My Notes")
        );
    }

    #[test]
    fn only_the_first_h1_counts() {
        assert_eq!(title_of("Intro.\n\n# One\n\n# Two").as_deref(), Some("One"));
        // An empty first H1 is "no title", never the next one.
        assert_eq!(title_of("#\n\n# Real Title"), None);
        assert_eq!(title_of("#   \n\n# Real Title"), None);
    }

    #[test]
    fn lower_headings_never_name_the_page() {
        assert_eq!(title_of("## Section\n\n### Deep"), None);
    }

    #[test]
    fn inline_markup_contributes_its_text() {
        assert_eq!(
            title_of("# Hello **bold** and `code` *here*").as_deref(),
            Some("Hello bold and code here")
        );
        // Shortcodes are expanded by `parse_events`, like on screen.
        assert_eq!(title_of("# :rocket: Launch").as_deref(), Some("🚀 Launch"));
    }

    #[test]
    fn breaks_become_one_space() {
        assert_eq!(title_of("# A<br>B").as_deref(), Some("A B"));
        // A setext H1 can span lines: the break is one space.
        assert_eq!(
            title_of("Hola\nmundo\n=====\n").as_deref(),
            Some("Hola mundo")
        );
    }

    #[test]
    fn oversized_headings_are_capped_with_an_ellipsis() {
        let long = "a".repeat(MAX_TITLE_CHARS + 200);
        let title = title_of(&format!("# {long}")).expect("title");
        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(title.ends_with("..."), "truncation is visible: {title:?}");
        // Exactly at the cap: no ellipsis is added.
        let exact = "b".repeat(MAX_TITLE_CHARS);
        let title = title_of(&format!("# {exact}")).expect("title");
        assert_eq!(title, exact);
    }
}
