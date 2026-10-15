// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `suggest`: address-field autocomplete sources + ranking.
//!
//! - ONE shared recipe for both address fields (toolbar + Home): they both
//!   draw `ui::omnibox`, so they share these suggestions.
//! - Sources, in display priority: bookmarks (explicit user data, newest
//!   first), history visits (newest first), then the curated builtin list
//!   (`crate::builtin_sites::builtin_suggestions()`, listed order, skipping
//!   URLs already covered).
//! - Matching is a case-insensitive substring on URL + title. An empty query
//!   shows recent bookmarks instead of nothing (confirmed product choice).
//! - Deduplicated by exact normalized URL: history/bookmarks already store
//!   normalized URLs (`util::normalize_stored_url`), and the builtin entries
//!   are written in that same form.
//! - Capped by `config::MAX_OMNIBOX_SUGGESTIONS`.
//! - Pure + allocation-light on the hot path: the caller (`ui::omnibox`)
//!   caches the result on the query text + both store generations, so this
//!   20000-entry scan runs on keystrokes, never per frame.
//!
//! TO GROW THE LIST: propose one `<url> | <title>` line upstream in
//! MDowser-Sites (vendored with `make sync-sites` into
//! `assets/builtin_sites.txt`, embedded with `include_str!` by
//! [`crate::builtin_sites`]). URLs must be pages this browser can open
//! (`https://` or a local `file://` path) in their normalized form
//! (trailing `/` for HTTPS roots/paths, like the stores keep them).

use crate::bookmarks::Bookmark;
use crate::builtin_sites::builtin_suggestions;
use crate::history::Visit;
use std::collections::HashSet;

/// Where one suggestion came from (display priority order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestSource {
    /// Explicit user data, newest first.
    Bookmark,
    /// Past visits, newest first.
    History,
    /// The curated [`crate::builtin_sites::builtin_suggestions`] list.
    Builtin,
}

/// One address-field suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Normalized absolute `https://` URL, or a local `file://` path.
    pub url: String,
    /// Page title when known (bookmark/visit title, builtin label).
    pub title: Option<String>,
    /// Which source produced it.
    pub source: SuggestSource,
}

/// Ranks suggestions for `query` over the three sources.
///
/// - Empty/blank query: recent bookmarks, newest first (the store keeps
///   oldest-first, so iterate in reverse).
/// - Otherwise: bookmarks (newest first) that match, then history (newest
///   first) that match and are not already listed, then builtin entries
///   (listed order) that match and are not already listed.
/// - Match = case-insensitive substring of the URL or the title.
/// - Always capped at `config::MAX_OMNIBOX_SUGGESTIONS`.
pub fn suggest(query: &str, history: &[Visit], bookmarks: &[Bookmark]) -> Vec<Suggestion> {
    let cap = crate::config::MAX_OMNIBOX_SUGGESTIONS;
    let mut out = Vec::new();
    if cap == 0 {
        return out;
    }
    let mut seen = HashSet::new();
    if query.trim().is_empty() {
        for bookmark in bookmarks.iter().rev() {
            if out.len() >= cap {
                break;
            }
            if seen.insert(bookmark.url.clone()) {
                out.push(Suggestion {
                    url: bookmark.url.clone(),
                    title: bookmark.title.clone(),
                    source: SuggestSource::Bookmark,
                });
            }
        }
        return out;
    }
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return out;
    }
    for bookmark in bookmarks.iter().rev() {
        if out.len() >= cap {
            break;
        }
        if seen.contains(&bookmark.url) {
            continue;
        }
        if matches_candidate(&bookmark.url, bookmark.title.as_deref(), &needle) {
            seen.insert(bookmark.url.clone());
            out.push(Suggestion {
                url: bookmark.url.clone(),
                title: bookmark.title.clone(),
                source: SuggestSource::Bookmark,
            });
        }
    }
    for visit in history.iter().rev() {
        if out.len() >= cap {
            break;
        }
        if seen.contains(&visit.url) {
            continue;
        }
        if matches_candidate(&visit.url, visit.title.as_deref(), &needle) {
            seen.insert(visit.url.clone());
            out.push(Suggestion {
                url: visit.url.clone(),
                title: visit.title.clone(),
                source: SuggestSource::History,
            });
        }
    }
    for (url, title) in builtin_suggestions() {
        if out.len() >= cap {
            break;
        }
        if seen.contains(*url) {
            continue;
        }
        if matches_candidate(url, Some(title), &needle) {
            seen.insert((*url).to_string());
            out.push(Suggestion {
                url: (*url).to_string(),
                title: Some((*title).to_string()),
                source: SuggestSource::Builtin,
            });
        }
    }
    out
}

/// Case-insensitive substring match of `needle` (already lowercased +
/// trimmed by the caller) against the URL and the optional title.
///
/// ASCII candidates (virtually every URL and title) compare in place: the
/// old `to_lowercase()` allocated a String per candidate per keystroke,
/// i.e. tens of thousands of allocations on a 20 000-entry history scan.
/// Non-ASCII text keeps the exact Unicode-lowercase path.
///
/// Shared with `ui::history_page`, which runs the same scan over the same
/// store; single-sourcing the rule keeps the omnibox and the history page
/// matching identically.
pub(crate) fn matches_candidate(url: &str, title: Option<&str>, needle: &str) -> bool {
    fn contains(haystack: &str, needle: &str) -> bool {
        if haystack.is_ascii() && needle.is_ascii() {
            return crate::util::contains_ignore_ascii_case(haystack, needle);
        }
        haystack.to_lowercase().contains(needle)
    }
    if contains(url, needle) {
        return true;
    }
    title.is_some_and(|title| contains(title, needle))
}

#[cfg(test)]
mod tests {
    use super::{suggest, SuggestSource};
    use crate::bookmarks::{Bookmark, BookmarkStore};
    use crate::builtin_sites::builtin_suggestions;
    use crate::history::{HistoryStore, Visit};

    fn bookmark(url: &str, title: Option<&str>) -> Bookmark {
        Bookmark {
            url: url.to_string(),
            title: title.map(str::to_string),
            added_at: 1,
            icon: None,
            color: None,
        }
    }

    fn visit(url: &str, title: Option<&str>) -> Visit {
        Visit {
            url: url.to_string(),
            title: title.map(str::to_string),
            visited_at: 1,
        }
    }

    #[test]
    fn builtin_list_holds_the_seeded_defaults_and_stays_valid() {
        use std::collections::HashSet;
        // Single source of truth: the expected core is owned by
        // `bookmarks::DEFAULT_BOOKMARKS`, and the rest of the list is
        // variable-length, so this test never hard-codes a count nor
        // repeats the full list — growing `assets/builtin_sites.txt`
        // touches only that file.
        let list = builtin_suggestions();
        for (url, _) in crate::bookmarks::DEFAULT_BOOKMARKS {
            assert!(
                list.iter().any(|(u, _)| *u == url),
                "builtin list must hold seeded default {url}"
            );
        }
        assert!(!list.is_empty(), "builtin list must not be empty");
        assert!(
            std::ptr::eq(list, builtin_suggestions()),
            "the embedded list is parsed once and shared"
        );
        let mut seen = HashSet::new();
        for (url, title) in list {
            assert!(
                !title.trim().is_empty(),
                "builtin entry needs a title: {url}"
            );
            assert!(seen.insert(*url), "builtin list must not repeat {url}");
            // Written in normalized store form (see the module docs): the
            // stores keep the URL byte-for-byte, so dedup by exact URL
            // only works when builtins match that form.
            assert_eq!(
                crate::util::normalize_stored_url(url).as_deref(),
                Some(*url),
                "builtin URL must be normalized: {url}"
            );
            // Every entry surfaces through the ranking on its own URL.
            assert!(
                suggest(url, &[], &[])
                    .iter()
                    .any(|s| s.url == *url && s.source == SuggestSource::Builtin),
                "builtin entry must be searchable: {url}"
            );
        }
    }

    #[test]
    fn empty_query_shows_recent_bookmarks_newest_first() {
        let bookmarks = vec![
            bookmark("https://ex.com/old/", Some("Old")),
            bookmark("https://ex.com/new/", Some("New")),
        ];
        let out = suggest("", &[], &bookmarks);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://ex.com/new/");
        assert_eq!(out[0].source, SuggestSource::Bookmark);
        assert_eq!(out[1].url, "https://ex.com/old/");
        // Blank input behaves like empty (no history/builtin mixed in).
        let blank = suggest("   ", &[visit("https://ex.com/h/", None)], &bookmarks);
        assert_eq!(blank, out);
    }

    #[test]
    fn matching_is_case_insensitive_on_url_and_title() {
        let bookmarks = vec![bookmark("https://ex.com/Cloud/", Some("Mixed Case"))];
        let history = vec![visit("https://docs.EXAMPLE.com/", Some("UPPER"))];
        assert_eq!(
            suggest("cloud", &[], &bookmarks)[0].url,
            "https://ex.com/Cloud/"
        );
        assert_eq!(
            suggest("mixed", &[], &bookmarks)[0].url,
            "https://ex.com/Cloud/"
        );
        assert_eq!(
            suggest("upper", &history, &[])[0].url,
            "https://docs.EXAMPLE.com/"
        );
        // Schemeless typing still matches the stored https URL.
        assert!(suggest("ex.com/cloud", &[], &bookmarks)
            .iter()
            .any(|s| s.url == "https://ex.com/Cloud/"));
    }

    #[test]
    fn bookmarks_win_over_history_win_over_builtin() {
        let url = "https://www.cloudflare.com/";
        let bookmarks = vec![bookmark(url, Some("Mine"))];
        let history = vec![visit(url, Some("Visited"))];
        let out = suggest("cloudflare", &history, &bookmarks);
        // The bookmark copy wins; the same URL never repeats from history
        // nor from the builtin list.
        assert_eq!(out.iter().filter(|s| s.url == url).count(), 1);
        assert_eq!(out[0].source, SuggestSource::Bookmark);
        assert_eq!(out[0].title.as_deref(), Some("Mine"));
        // Without the bookmark, the history copy wins over builtin.
        let out = suggest("cloudflare", &history, &[]);
        assert_eq!(out[0].source, SuggestSource::History);
        // Without either, the builtin entry still surfaces.
        let out = suggest("cloudflare", &[], &[]);
        assert!(out
            .iter()
            .any(|s| s.url == url && s.source == SuggestSource::Builtin));
        assert!(out
            .iter()
            .any(|s| s.url == "https://developers.cloudflare.com/"));
    }

    #[test]
    fn results_are_capped() {
        let cap = crate::config::MAX_OMNIBOX_SUGGESTIONS;
        let bookmarks: Vec<Bookmark> = (0..cap + 10)
            .map(|i| bookmark(&format!("https://ex.com/{i}/"), None))
            .collect();
        let out = suggest("ex.com", &[], &bookmarks);
        assert_eq!(out.len(), cap);
        // Newest bookmarks first, even at the cap.
        assert_eq!(out[0].url, format!("https://ex.com/{}/", cap + 9));
    }

    #[test]
    fn local_files_match_on_path() {
        let history = vec![visit("file:///tmp/notes.md", Some("Notes"))];
        let out = suggest("notes", &history, &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "file:///tmp/notes.md");
        assert_eq!(out[0].source, SuggestSource::History);
    }

    #[test]
    fn no_match_is_empty_not_a_fallback_dump() {
        let history = vec![visit("https://ex.com/a/", None)];
        let bookmarks = vec![bookmark("https://ex.com/b/", None)];
        assert!(suggest("zzz-no-such-page", &history, &bookmarks).is_empty());
    }

    /// End-to-end through the real stores (normalization + ordering): what
    /// the omnibox actually passes in.
    #[test]
    fn real_stores_feed_the_ranking() {
        let mut bookmarks = BookmarkStore::in_memory();
        bookmarks.toggle("https://ex.com/kept/", Some("Kept"));
        let mut history = HistoryStore::in_memory();
        history.record("https://ex.com/kept/", Some("Kept visit"));
        history.record("https://ex.com/other/", Some("Other"));
        let out = suggest("ex.com", history.entries(), bookmarks.entries());
        assert_eq!(out[0].url, "https://ex.com/kept/");
        assert_eq!(out[0].source, SuggestSource::Bookmark);
        assert!(out.iter().any(|s| s.url == "https://ex.com/other/"));
    }
}
