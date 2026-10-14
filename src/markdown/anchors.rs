// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::anchors`: in-document navigation targets.
//!
//! Markdown carries no link graph and no element ids, so anchors are a
//! convention:
//! - Explicit heading attributes (`# Title {#custom-id}`) are honored.
//! - Otherwise the heading text is slugified (lowercase, drop punctuation,
//!   spaces to hyphens), with `-1`, `-2` suffixes for repeats.
//! - Footnote definitions register `fn-<label>`, so `[^N]` references can
//!   jump to their definition.
//!
//! The viewer resolves a `#id` destination against [`AnchorIndex::set`]:
//! a known target stays a clickable anchor; an unknown one renders as plain
//! text. [`build`] runs ONCE per fetch (off the UI thread) and is bounded by
//! `config::MAX_ANCHORS_PER_DOC`.

use pulldown_cmark::{Event, Tag, TagEnd};
use std::collections::{HashMap, HashSet};

/// Anchor targets of one document, plus the heading ids by event index so
/// the viewer can attach a scroll target to each heading it draws.
#[derive(Debug, Clone, Default)]
pub struct AnchorIndex {
    /// One id per heading, in document order. Empty string when the heading
    /// had no usable slug (still keeps the per-heading order aligned).
    pub heading_ids: Vec<String>,
    /// Event index of each heading, parallel to [`Self::heading_ids`]. The
    /// viewer looks a heading's id up BY the index of its `Start` event, so
    /// skipped content (`<Tabs>` panels, collapsed accordions, the block
    /// budget) can never shift a later heading's target.
    heading_at: Vec<usize>,
    /// Every navigable target (heading ids + `fn-<label>` footnote names).
    pub set: HashSet<String>,
    /// Reserved id per footnote label (`label -> fn-<label>` or its deduped
    /// form). The viewer's reference and definition both use this instead of
    /// rebuilding `fn-<label>`, so a colliding explicit heading id
    /// (`# Head {#fn-a}`) cannot hijack the reference.
    footnotes: HashMap<String, String>,
    /// Footnote labels in FIRST-REFERENCE order (`label -> 1-based number`).
    /// Both the reference marker and the definition marker show this number:
    /// footnotes are numbered by reference, not by definition order, so
    /// `[^note]` followed by `[^2]` renders as 1 and 2.
    footnote_numbers: HashMap<String, usize>,
    /// Highest number handed out to a referenced footnote (a definition the
    /// document never references continues the sequence from here instead of
    /// colliding with a reference's number).
    footnote_count: usize,
    /// [`Self::set`] normalized for fragment matching (percent-decoded,
    /// lowercased): links and omnibox URLs arrive encoded (`#caf%C3%A9`,
    /// `#Section`), while slugs are stored decoded and lowercase.
    normalized: HashSet<String>,
}

impl AnchorIndex {
    /// Is `id` (without the leading `#`) a navigable target?
    /// Accepts the raw form and the percent-encoded / mixed-case form.
    pub fn contains(&self, id: &str) -> bool {
        self.set.contains(id) || self.normalized.contains(&canonical_fragment(id))
    }

    /// The id of the heading whose `Start` event sits at `event_index`, or
    /// `None` when no heading starts there or it had no usable slug.
    ///
    /// Keyed by the EVENT index instead of a render-order counter: the
    /// viewer skips content (`<Tabs>` panels render only the selected one,
    /// a closed `<Accordion>` renders none, the block budget drops blocks),
    /// and a counter would hand every later heading the wrong target.
    pub fn heading_id_at(&self, event_index: usize) -> Option<&str> {
        let slot = self.heading_at.binary_search(&event_index).ok()?;
        self.heading_ids
            .get(slot)
            .map(String::as_str)
            .filter(|id| !id.is_empty())
    }

    /// Reserved anchor id for footnote `label` (`fn-<label>`, deduped when a
    /// heading claimed the plain id), when the definition exists within the
    /// anchor cap. Callers must bound `label` exactly like [`build`] does
    /// (`config::MAX_FOOTNOTE_LABEL_CHARS`).
    pub fn footnote_id(&self, label: &str) -> Option<&str> {
        self.footnotes.get(label).map(String::as_str)
    }

    /// 1-based number of footnote `label`, assigned in FIRST-REFERENCE order
    /// (the number a rendered footnote marker shows). `None` when the
    /// document never references the label: the definition still renders, it
    /// just has no reference to number.
    pub fn footnote_number(&self, label: &str) -> Option<usize> {
        self.footnote_numbers.get(label).copied()
    }

    /// Numbers already assigned to referenced footnotes (see
    /// [`Self::footnote_number`]): an unreferenced definition continues the
    /// sequence from here.
    pub fn footnote_count(&self) -> usize {
        self.footnote_count
    }
}

/// True when the (possibly percent-encoded, mixed-case) fragment names the
/// anchor `id` (`#caf%C3%A9` and `#Section` match `café` and `section`).
pub fn fragment_matches(id: &str, fragment: &str) -> bool {
    id == fragment || canonical_fragment(id) == canonical_fragment(fragment)
}

/// Fragment normalization for matching: percent-decode, then lowercase
/// (default slugs are lowercase; explicit ids are matched raw first).
fn canonical_fragment(fragment: &str) -> String {
    percent_decode(fragment).to_lowercase()
}

/// Percent-decodes `fragment` (`%C3%A9` -> `é`). Invalid escapes and
/// invalid UTF-8 are kept literally instead of failing.
fn percent_decode(value: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

/// Slug of a heading's text.
///
/// Lowercase, keep alphanumerics/`-`/`_`, turn whitespace into `-` and drop
/// everything else (punctuation, emoji). A run of whitespace becomes one
/// separator; leading/trailing hyphens are trimmed. Unicode letters are kept
/// (`Instalación rápida` -> `instalación-rápida`).
pub fn slugify(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '_' {
            kept.push(ch);
        } else if ch.is_whitespace() {
            kept.push(' ');
        } else if ch == '-' {
            kept.push('-');
        }
        // Everything else (punctuation, symbols, emoji) is dropped.
    }
    let joined = kept.split_whitespace().collect::<Vec<_>>().join("-");
    joined.trim_matches('-').to_string()
}

/// Walks the parsed events once and collects every anchor target.
pub fn build(events: &[Event<'static>]) -> AnchorIndex {
    let cap = crate::config::MAX_ANCHORS_PER_DOC;
    let mut index = AnchorIndex::default();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut i = 0usize;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Heading { id, .. }) => {
                // Consume the heading's inline text (not rendered here).
                let explicit = id
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
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
                let base = explicit.unwrap_or_else(|| slugify(&text));
                index.heading_at.push(i);
                if base.is_empty() || index.set.len() >= cap {
                    // Keep the per-heading slot so later ids stay aligned.
                    index.heading_ids.push(String::new());
                } else {
                    let unique = reserve(&base, &mut used, &mut index.set);
                    index.heading_ids.push(unique);
                }
                i = j;
            }
            Event::Start(Tag::FootnoteDefinition(label)) if index.set.len() < cap => {
                // Bounded like every server-controlled label. Reserved like a
                // heading id so a colliding heading (`# Head {#fn-a}`) cannot
                // claim the footnote's target first (the viewer would scroll
                // to the heading and clear the pending fragment). The
                // RESERVED id is remembered: on collision the reference and
                // the definition both use `fn-a-1`, never the heading's id.
                let label =
                    crate::util::truncate_chars(label, crate::config::MAX_FOOTNOTE_LABEL_CHARS);
                let id = reserve(&format!("fn-{label}"), &mut used, &mut index.set);
                // A duplicate label (two definitions) keeps the first id.
                index.footnotes.entry(label.into_owned()).or_insert(id);
            }
            // Footnote REFERENCES carry the numbering: the marker in the text
            // and the definition both show the number of the reference, in
            // the order the references appear.
            Event::FootnoteReference(label) if index.footnote_numbers.len() < cap => {
                let label =
                    crate::util::truncate_chars(label, crate::config::MAX_FOOTNOTE_LABEL_CHARS);
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    index.footnote_numbers.entry(label.into_owned())
                {
                    index.footnote_count += 1;
                    slot.insert(index.footnote_count);
                }
            }
            _ => {}
        }
        i += 1;
    }
    index.normalized = index.set.iter().map(|s| canonical_fragment(s)).collect();
    index
}

/// Reserves `base`, or `base-N` for the Nth duplicate, in `set`.
fn reserve(base: &str, used: &mut HashMap<String, usize>, set: &mut HashSet<String>) -> String {
    if set.insert(base.to_string()) {
        used.entry(base.to_string()).or_insert(0);
        return base.to_string();
    }
    let mut n = used.get(base).copied().unwrap_or(0) + 1;
    loop {
        let candidate = format!("{base}-{n}");
        if set.insert(candidate.clone()) {
            used.insert(base.to_string(), n);
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{build, slugify};
    use crate::markdown::document::parse_events;
    use pulldown_cmark::{Event, Tag};

    #[test]
    fn slugify_matches_heading_conventions() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("Instalación rápida"), "instalación-rápida");
        assert_eq!(slugify("API: v2.0"), "api-v20");
        assert_eq!(slugify("  spaced  out  "), "spaced-out");
        assert_eq!(slugify("C++ & Rust"), "c-rust");
        assert_eq!(slugify("###"), "");
    }

    #[test]
    fn collects_heading_ids_in_order() {
        let events = parse_events("# One\n\n## Two\n").expect("parse");
        let idx = build(&events);
        assert_eq!(idx.heading_ids, vec!["one".to_string(), "two".to_string()]);
        assert!(idx.contains("one") && idx.contains("two"));
        assert!(!idx.contains("three"));
        // Lookup by the heading's own event index: skipping content (a
        // `<Tabs>` panel, a collapsed accordion) can never shift a target.
        let one_at = events
            .iter()
            .position(|e| matches!(e, Event::Start(Tag::Heading { .. })))
            .expect("first heading");
        assert_eq!(idx.heading_id_at(one_at), Some("one"));
        assert_eq!(idx.heading_id_at(one_at + 1), None, "not a heading start");
        assert_eq!(idx.heading_id_at(99), None, "past the end");
    }

    /// A heading with no usable slug keeps its slot (empty string) and
    /// reports no id; the headings after it keep their own.
    #[test]
    fn slugless_headings_keep_their_slot() {
        let events = parse_events("# ###\n\n## Two\n").expect("parse");
        let idx = build(&events);
        assert_eq!(idx.heading_ids, vec![String::new(), "two".to_string()]);
        let two_at = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Event::Start(Tag::Heading { .. })))
            .nth(1)
            .map(|(i, _)| i)
            .expect("second heading");
        assert_eq!(idx.heading_id_at(two_at), Some("two"));
        assert_eq!(idx.heading_id_at(0), None, "slugless heading has no id");
    }

    #[test]
    fn honors_explicit_ids_and_dedupes_repeats() {
        let events = parse_events("# Same\n\n# Same\n\n# Custom {#chosen}\n").expect("parse");
        let idx = build(&events);
        assert_eq!(
            idx.heading_ids,
            vec![
                "same".to_string(),
                "same-1".to_string(),
                "chosen".to_string()
            ]
        );
        assert!(idx.contains("same-1") && idx.contains("chosen"));
    }

    #[test]
    fn registers_footnote_targets() {
        let events = parse_events("Ref[^a]\n\n[^a]: note\n").expect("parse");
        let idx = build(&events);
        assert!(idx.contains("fn-a"));
        // Footnote names never enter the heading order.
        assert!(idx.heading_ids.is_empty());
    }

    /// A heading claiming the footnote id must not shadow it: the footnote
    /// target is reserved with the same collision suffix as a heading, and
    /// the reserved id is the one references and definitions both use.
    #[test]
    fn footnote_ids_do_not_collide_with_heading_ids() {
        let events = parse_events("# Head {#fn-a}\n\nRef[^a]\n\n[^a]: note\n").expect("parse");
        let idx = build(&events);
        assert!(idx.contains("fn-a"), "{:?}", idx.set);
        assert!(idx.contains("fn-a-1"), "footnote must get a free id");
        assert_eq!(idx.heading_ids, vec!["fn-a".to_string()]);
        assert_eq!(
            idx.footnote_id("a"),
            Some("fn-a-1"),
            "the reference must navigate to the reserved id, not the heading"
        );
    }

    /// Footnote labels are truncated like the viewer truncates them, and the
    /// accessor maps the bounded label to its reserved id.
    #[test]
    fn footnote_id_maps_bounded_labels() {
        let events = parse_events("Ref[^b]\n\n[^b]: note\n").expect("parse");
        let idx = build(&events);
        assert_eq!(idx.footnote_id("b"), Some("fn-b"));
        assert_eq!(idx.footnote_id("missing"), None);
    }

    /// Footnote references carry the numbering: first-reference order,
    /// repeats keep their number, and the count seeds the numbering of a
    /// definition the document never references.
    #[test]
    fn footnote_numbers_follow_first_reference_order() {
        // `[^a]` needs its own definition too: pulldown-cmark only emits a
        // footnote REFERENCE for a label that HAS one (an undefined label
        // stays literal text).
        let events = parse_events(
            "A[^b] B[^a] C[^b] D[^2]\n\n[^b]: b\n[^a]: a\n[^2]: two\n[^c]: never referenced\n",
        )
        .expect("parse");
        let idx = build(&events);
        assert_eq!(idx.footnote_number("b"), Some(1), "first reference");
        assert_eq!(idx.footnote_number("a"), Some(2), "second reference");
        assert_eq!(idx.footnote_number("2"), Some(3), "third reference");
        assert_eq!(
            idx.footnote_number("c"),
            None,
            "an unreferenced definition has no reference number"
        );
        assert_eq!(idx.footnote_count(), 3, "three referenced labels");
        assert!(
            idx.footnote_id("c").is_some(),
            "the unreferenced definition still resolves as a target"
        );
    }

    /// Fragments arrive percent-encoded (`Url::fragment()` returns
    /// `caf%C3%A9` for `#café`) and usually mixed-case, while slugs are
    /// stored decoded and lowercase: both forms must resolve.
    #[test]
    fn resolves_percent_encoded_and_mixed_case_fragments() {
        let events = parse_events("# Instalación rápida\n\n# Section\n").expect("parse");
        let idx = build(&events);
        assert!(idx.contains("instalación-rápida"));
        assert!(idx.contains("instalaci%C3%B3n-r%C3%A1pida"), "encoded form");
        assert!(idx.contains("Section"), "mixed case");
        assert!(!idx.contains("missing"));
        assert!(!idx.contains("instalaci%C3%B3n"));
        // The shared matcher powers the heading/footnote scroll compare.
        assert!(super::fragment_matches("section", "Section"));
        assert!(super::fragment_matches("fn-nota", "fn-nota"));
        assert!(!super::fragment_matches("section", "other"));
    }
}
