// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::outline`: the document's heading outline (reading aid).
//!
//! The outline is the flat list of the document's headings, in document
//! order, exactly the set `anchors::build` gives a scroll target:
//! - Every heading of the parsed body is one item (a heading the normalizer
//!   left inside a quote, a list item or an MDX component included).
//! - The item's `id` is the RESERVED heading id from
//!   [`crate::markdown::anchors::AnchorIndex`], so the outline sidebar scrolls
//!   through the same mechanism as a `#id` link click (`Tab::pending_anchor`)
//!   and a reserved slugless-heading id (`heading-N`) navigates too.
//! - The item's `at` is the heading's own `Start` event index: the viewer
//!   records the rendered heading's rect by that index, so skipped content
//!   (an unselected `<Tabs>` panel, a closed accordion, the block budget)
//!   can never shift a later heading's scrollspy position.
//!
//! [`build`] runs ONCE per fetch (off the UI thread) next to the anchor
//! index, from the same owned events; it is bounded by
//! `config::MAX_OUTLINE_ITEMS` items and `config::MAX_OUTLINE_TITLE_CHARS`
//! chars per title, so a hostile heading list can never grow the per-tab
//! state beyond a fixed budget.

use crate::config;
use crate::markdown::anchors::AnchorIndex;
use crate::util::truncate_chars;
use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};

/// One entry of the document outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// Heading level (1 = `#` ... 6 = `######`), as written in the source.
    pub level: u8,
    /// Heading text with whitespace collapsed to single spaces, bounded by
    /// `config::MAX_OUTLINE_TITLE_CHARS`. Never empty (`Untitled` stands in
    /// for a heading whose text is empty, so the row is never blank).
    pub title: String,
    /// Reserved anchor id of the heading ([`AnchorIndex::heading_id_at`]):
    /// the outline sidebar requests a scroll with this id.
    pub id: String,
    /// Event index of the heading's `Start` event (scrollspy key).
    pub at: usize,
}

/// The document's headings, in document order.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    /// One entry per heading, in document order.
    pub items: Vec<OutlineItem>,
    /// Shallowest heading level in the document (0 when there are none): the
    /// indent origin of the panel, so a document whose headings start at
    /// `##` does not waste the first indent step.
    pub min_level: u8,
}

impl Outline {
    /// True when the document has no headings (the sidebar keeps its slot
    /// with an empty state).
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Indent step of `item` (0 for the shallowest heading level in the
    /// document), capped so a deep `######` list stays readable in a narrow
    /// panel.
    pub fn indent_steps(&self, item: &OutlineItem) -> u8 {
        if self.min_level == 0 {
            return 0;
        }
        item.level
            .saturating_sub(self.min_level)
            .min(config::MAX_OUTLINE_INDENT_STEPS)
    }
}

/// Walks the parsed events once and collects the heading outline.
///
/// Headings without a reserved anchor id (only possible past
/// `config::MAX_ANCHORS_PER_DOC`) are skipped: an entry the panel cannot
/// jump to is worse than a shorter list.
pub fn build(events: &[Event<'static>], anchors: &AnchorIndex) -> Outline {
    let mut items: Vec<OutlineItem> = Vec::new();
    let mut min_level = 0u8;
    let mut i = 0usize;
    while i < events.len() {
        if items.len() >= config::MAX_OUTLINE_ITEMS {
            break;
        }
        if let Event::Start(Tag::Heading { level, .. }) = &events[i] {
            let level = level_number(*level);
            // Consume the heading's inline text, like `anchors::build` does
            // (the two walks must collect the same title), and leave `i` on
            // the heading's `End`.
            let mut text = String::new();
            let mut j = i + 1;
            while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                match &events[j] {
                    Event::Text(t) | Event::Code(t) => text.push_str(t),
                    Event::SoftBreak | Event::HardBreak => text.push(' '),
                    _ => {}
                }
                j += 1;
            }
            if let Some(id) = anchors.heading_id_at(i) {
                let title = collapse_whitespace(&text);
                let title = if title.is_empty() {
                    "Untitled".to_string()
                } else {
                    truncate_chars(&title, config::MAX_OUTLINE_TITLE_CHARS).into_owned()
                };
                items.push(OutlineItem {
                    level,
                    title,
                    id: id.to_string(),
                    at: i,
                });
                min_level = if min_level == 0 {
                    level
                } else {
                    min_level.min(level)
                };
            }
            i = j;
        } else {
            i += 1;
        }
    }
    Outline { items, min_level }
}

/// Heading level as the 1-6 number the outline shows/indents by.
fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Collapses every whitespace run to one space and trims the ends: a setext
/// heading (or one broken over source lines) reads as one row in the panel.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{build, collapse_whitespace, Outline};
    use crate::markdown::anchors;
    use crate::markdown::document::parse_events;

    /// Parses `md` and builds its outline like `finish_document` does.
    fn outline_of(md: &str) -> Outline {
        let events = parse_events(md).expect("parse");
        let anchors = anchors::build(&events);
        build(&events, &anchors)
    }

    #[test]
    fn collects_headings_in_order_with_their_ids() {
        let outline = outline_of("# One\n\nBody\n\n## Two\n\n### Three\n\n## Two\n");
        let items: Vec<(&str, u8, &str)> = outline
            .items
            .iter()
            .map(|item| (item.title.as_str(), item.level, item.id.as_str()))
            .collect();
        assert_eq!(
            items,
            vec![
                ("One", 1, "one"),
                ("Two", 2, "two"),
                ("Three", 3, "three"),
                // A repeated slug keeps its deduped anchor id.
                ("Two", 2, "two-1"),
            ]
        );
        assert_eq!(outline.min_level, 1);
    }

    /// The item's `at` is the heading's own event index (never a render-order
    /// counter): the scrollspy records each rendered heading's rect by it, so
    /// skipped content cannot shift a later heading's position.
    #[test]
    fn items_carry_the_heading_event_index() {
        let md = "# One\n\nParagraph\n\n## Two\n";
        let events = parse_events(md).expect("parse");
        let anchors = anchors::build(&events);
        let outline = build(&events, &anchors);
        let heading_indexes: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                matches!(
                    e,
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading { .. })
                )
            })
            .map(|(i, _)| i)
            .collect();
        let at: Vec<usize> = outline.items.iter().map(|item| item.at).collect();
        assert_eq!(at, heading_indexes);
    }

    /// A heading that slugs to nothing (an em dash, emoji, punctuation only)
    /// still enters the outline: `anchors::build` reserves its 1-based
    /// `heading-N` id for it, so the panel can jump to it and the row keeps
    /// the heading's own text.
    #[test]
    fn slugless_headings_stay_navigable() {
        let outline = outline_of("# —\n\n## Two\n");
        assert_eq!(outline.items[0].title, "—");
        assert_eq!(outline.items[0].id, "heading-1");
        assert_eq!(outline.items[1].id, "two");
    }

    /// Titles are whitespace-collapsed and bounded: a hostile heading cannot
    /// grow the per-tab outline (or the panel's per-frame layout) without
    /// limit. The `Untitled` fallback keeps the row non-empty.
    #[test]
    fn titles_are_collapsed_bounded_and_never_empty() {
        assert_eq!(collapse_whitespace("  A \n\t B  "), "A B");
        assert_eq!(collapse_whitespace("   "), "");
        let long = format!(
            "# {}",
            "x".repeat(crate::config::MAX_OUTLINE_TITLE_CHARS + 50)
        );
        let outline = outline_of(&long);
        assert_eq!(
            outline.items[0].title.chars().count(),
            crate::config::MAX_OUTLINE_TITLE_CHARS
        );
        // An image-only heading keeps its alt text; a truly empty one still
        // renders a labeled row.
        let outline = outline_of("# \n\n## ![Logo](logo.png)\n");
        assert_eq!(outline.items[0].title, "Untitled");
        assert_eq!(outline.items[1].title, "Logo");
    }

    /// Items past `MAX_OUTLINE_ITEMS` are dropped (the doc's headings are
    /// still anchored; the panel simply shows the first N) and the indent
    /// origin is the shallowest level actually present.
    #[test]
    fn items_are_capped_and_indent_from_the_shallowest_level() {
        let md = format!(
            "{}\n",
            (0..crate::config::MAX_OUTLINE_ITEMS + 5)
                .map(|n| format!("## H{n}"))
                .collect::<Vec<_>>()
                .join("\n\n")
        );
        let outline = outline_of(&md);
        assert_eq!(outline.items.len(), crate::config::MAX_OUTLINE_ITEMS);
        assert_eq!(outline.min_level, 2, "no H1 in this document");
        // The shallowest level indents at 0; a deeper one one step per level.
        let deep = outline_of("# A\n\n### B\n\n###### C\n");
        let steps: Vec<u8> = deep
            .items
            .iter()
            .map(|item| deep.indent_steps(item))
            .collect();
        assert_eq!(steps, vec![0, 2, 5]);
    }
}
