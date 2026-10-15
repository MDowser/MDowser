// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `builtin_sites`: the single curated site list behind the suggestions.
//!
//! - ONE source of truth for the builtin omnibox entries:
//!   `assets/builtin_sites.txt`, embedded here with `include_str!` and
//!   parsed once per process by [`builtin_suggestions`], which `suggest`
//!   imports (ranking) and nobody else. The first-run bookmark grid
//!   (`bookmarks::DEFAULT_BOOKMARKS`) keeps its own five-tile selection,
//!   but every seeded default must also appear in the list (enforced by
//!   `suggest`'s test), so the omnibox always offers the full set.
//! - Entries are `(url, title)` in display order: `suggest` walks them in
//!   listed order, skipping URLs already covered by bookmarks/history.
//! - URLs must be pages this browser can open (`https://`, or a local
//!   `file://` path) in their normalized store form (trailing `/` for
//!   HTTPS roots, like the stores keep them): ranking dedups by exact URL,
//!   and the test asserts `util::normalize_stored_url` keeps every entry
//!   byte-for-byte.
//!
//! TO GROW THE LIST: propose the line upstream in MDowser-Sites, then run
//! `make sync-sites`: the file is vendored from that repo (via curl) and
//! every URL is validated the way the browser resolves it (see
//! `scripts/sync-builtin-sites.sh`). Nothing else needs to change: no count
//! to bump, no test list to mirror.

use std::sync::OnceLock;

/// The curated list, compiled into the binary from
/// `assets/builtin_sites.txt` (one `<url> | <title>` line per entry;
/// blank lines and `#` comments are allowed).
const BUILTIN_SITES: &str = include_str!("../assets/builtin_sites.txt");

/// The curated builtin suggestions as `(url, title)`, in display order:
/// the seeded bookmarks plus the docs kept out of the first-run grid and
/// the curated site/docs list.
///
/// Parsed once per process (`OnceLock`); every slice borrows the embedded
/// file, so no per-entry allocation. Panics only on a malformed embedded
/// line, which is a source bug [`parse`] reports with its line number.
pub fn builtin_suggestions() -> &'static [(&'static str, &'static str)] {
    static PARSED: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    PARSED.get_or_init(|| parse(BUILTIN_SITES)).as_slice()
}

/// Parses the embedded list: blank lines and `#` comments are skipped;
/// every other line must be `<url> | <title>` (split on the first `|`,
/// both sides trimmed, so titles may hold further `|`).
///
/// The file is compiled into the binary, so a malformed line cannot be
/// fixed at runtime: panic with the file and line instead of silently
/// dropping an entry or mis-splitting the title.
fn parse(raw: &'static str) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((url, title)) = line.split_once('|') else {
            panic!(
                "assets/builtin_sites.txt:{}: expected `<url> | <title>`: {line}",
                index + 1
            );
        };
        out.push((url.trim(), title.trim()));
    }
    out
}
