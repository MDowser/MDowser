// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::resolver`: candidate generation and route normalization.
//!
//! HTTPS ROUTES (required order, for a logical `/love` page):
//! 1. `/love`
//! 2. `/love/`
//! 3. `/love.md`
//! 4. `/love/index.md`
//! 5. `/love/README.md`
//!
//! Only the standard spellings are ever fetched: no `.markdown` extension
//! and no `readme`/`index.markdown` case variants. Legacy `.markdown` URLs
//! still normalize to the same logical page (see `logical_base`) so they
//! resolve through this cascade instead of failing.
//!
//! HOME CASE (`/`):
//! - `/`, `/index.md`, `/README.md` (in order).
//!
//! VIEW NORMALIZATION:
//! - Always show the trailing-`/` form: `/love/` (or `/` for the root),
//!   whichever candidate actually loaded.
//! - LOCAL FILES are the exception: `protocol::local` resolves them
//!   directly and the display keeps the exact file path (`/a/notes.md`),
//!   never collapsing it to a directory.
//!
//! DESIGN:
//! - `logical_base()` reduces any input (`/love`, `/love/`, `/love.md`,
//!   `/love/index.md`, ...) to its logical base (`/love` or `""` for root).
//! - `candidates_for()` expands the base into the ordered `Url` list to try.
//! - `normalized_display()` returns the view URL (`.../love/`).
//! - `display_string()` returns the URL for the minimal UI WITHOUT scheme
//!   (`ex.com/love/`); internal navigation always uses `Url` with `https://`.

use url::Url;

/// Computes the logical base of a path.
///
/// - `"/"` -> `""` (root).
/// - `"/love/"` -> `"/love"`.
/// - `"/love.md"` -> `"/love"`.
/// - `"/love/index.md"` -> `"/love"`.
/// - `"/love/README.md"` (any capitalization) -> `"/love"`.
/// - Rest -> path without trailing `/`.
pub(crate) fn logical_base(path: &str) -> String {
    // Exact root.
    if path.is_empty() || path == "/" {
        return String::new();
    }
    // Strip trailing `/` (one or many) except root.
    let mut p = path.trim_end_matches('/').to_string();
    if p.is_empty() {
        return String::new();
    }
    // File suffixes collapsing to the base.
    // Order: longest first so `/index.markdown` is not confused with `.md`.
    let lower = p.to_ascii_lowercase();
    for suffix in [
        "/index.markdown",
        "/readme.markdown",
        "/index.md",
        "/readme.md",
    ] {
        if lower.ends_with(suffix) {
            // Cut the suffix by byte length (ASCII, safe).
            p.truncate(p.len() - suffix.len());
            // `/love//index.md` leaves a trailing slash behind: normalize it
            // away so the base keeps its documented "no trailing `/`"
            // contract (otherwise `/love//` candidates and display strings
            // leak out of a double slash).
            let p = p.trim_end_matches('/');
            if p.is_empty() {
                return String::new();
            }
            return p.to_string();
        }
    }
    // A bare `.md` / `.markdown` file collapses to its stem (any case).
    let ext = if lower.ends_with(".markdown") {
        Some(".markdown")
    } else if lower.ends_with(".md") {
        Some(".md")
    } else {
        None
    };
    if let Some(ext) = ext {
        // Guard `/.md` and nested dotfiles like `/a/.md`: stripping the
        // extension would collapse the path to a directory (or the home).
        // Only strip when a real name precedes the extension.
        let stem_len = p.len() - ext.len();
        let stem = &p[..stem_len];
        // A stem ending in `/` means the "file name" was only the extension
        // (a dotfile such as `.md`): keep the original path.
        if stem.is_empty() || stem.ends_with('/') {
            return p;
        }
        let stem_name = stem.trim_matches('/');
        if stem_name.is_empty() || stem_name == "." {
            return p;
        }
        p.truncate(stem_len);
        if p.is_empty() {
            return String::new();
        }
        // A trailing slash left after stripping the extension is cleaned.
        let p = p.trim_end_matches('/').to_string();
        if p.is_empty() {
            return String::new();
        }
        return p;
    }
    p
}

/// Builds the ordered candidates from an already validated (https) URL.
///
/// - Keeps the original URL scheme + host + port.
/// - Preserves `?query` on every candidate (content may be query-addressed);
///   fragments are always stripped (no scroll model, avoids reload loops).
pub fn candidates_for(input: &Url) -> Vec<Url> {
    // Local files have their own candidate walker (`protocol::local`):
    // never route one through the HTTPS cascade.
    if input.scheme() != "https" {
        return vec![input.clone()];
    }
    let base = logical_base(input.path());
    let mut out = Vec::with_capacity(crate::config::MAX_ROUTE_CANDIDATES);

    // Helper: builds `scheme://host[:port] + path` through the same parser
    // the address path uses, so candidates and display URLs agree and the
    // already-percent-encoded path keeps its exact escapes.
    // Since `base` comes from the parsed path, building cannot fail;
    // if it ever did, that candidate is simply skipped.
    // Query is preserved from the input; fragment is always stripped.
    let input_query = input.query().map(|q| q.to_string());
    let mut push = |path: String| {
        // Preserve scheme + host + port exactly, avoid double-encoding.
        let host = match input.host_str() {
            Some(h) => h,
            None => return,
        };
        // IPv6 hosts need brackets.
        let host_fmt = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port_part = input.port().map(|p| format!(":{p}")).unwrap_or_default();
        let cand_str = format!("{}://{host_fmt}{port_part}{path}", input.scheme());
        if let Ok(mut u) = Url::parse(&cand_str) {
            if let Some(q) = input_query.as_deref() {
                u.set_query(Some(q));
            } else {
                u.set_query(None);
            }
            u.set_fragment(None);
            // The `.md` / index expansion appends bytes to a capped input:
            // re-enforce the BYTE cap (the serialized URL is percent-encoded,
            // so a char count here would silently drop valid candidates).
            if u.as_str().len() <= crate::config::MAX_URL_BYTES {
                out.push(u);
            }
        }
    };

    if base.is_empty() {
        // HOME: `/`, then the conventional directory indexes
        // (`/index.md`, `/README.md`, ...).
        push("/".to_string());
        for name in crate::config::DIR_INDEXES {
            push(format!("/{name}"));
        }
    } else {
        push(base.clone());
        push(format!("{base}/"));
        push(format!("{base}.md"));
        for name in crate::config::DIR_INDEXES {
            push(format!("{base}/{name}"));
        }
    }
    out
}

/// Normalized URL to SHOW in the bar (always with trailing `/`).
///
/// - Derived from the URL that actually loaded, not the one the user asked.
/// - E.g.: loaded `https://h/love.md` -> shows `https://h/love/`.
/// - Root -> `https://h/`.
/// - Preserves `?query`, strips `#fragment`.
/// - Local `file:` URLs keep their exact path (fragment and the
///   filesystem-meaningless query stripped), so the bar shows which file is
///   open instead of a directory.
pub fn normalized_display(loaded: &Url) -> Url {
    if loaded.scheme() == "file" {
        let mut u = loaded.clone();
        u.set_fragment(None);
        u.set_query(None);
        return u;
    }
    let base = logical_base(loaded.path());
    let path = if base.is_empty() {
        "/".to_string()
    } else {
        format!("{base}/")
    };
    // Same parser as the address path: keeps percent-encoding exact.
    if let Some(host) = loaded.host_str() {
        let host_fmt = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port_part = loaded.port().map(|p| format!(":{p}")).unwrap_or_default();
        let cand_str = format!("{}://{host_fmt}{port_part}{path}", loaded.scheme());
        if let Ok(mut u) = Url::parse(&cand_str) {
            if let Some(q) = loaded.query() {
                u.set_query(Some(q));
            } else {
                u.set_query(None);
            }
            u.set_fragment(None);
            // Same BYTE cap as the candidates above: the display URL feeds
            // the address bar, history and session, which all assume it.
            if u.as_str().len() <= crate::config::MAX_URL_BYTES {
                return u;
            }
            // Over-long after expansion: fall through to the fragment-only
            // strip below (still capped by the fetch-time redirect check).
        }
    }
    // Fallback: strip fragment, keep original path + query.
    let mut u = loaded.clone();
    u.set_fragment(None);
    u
}

/// Returns `true` when two URLs address the same logical document
/// (same scheme/host/port + same `logical_base` + same query).
/// Used to dedup navigations like `/love` vs `/love/` that would
/// otherwise push duplicate history and spawn duplicate fetches.
///
/// Local files are compared by their EXACT path: two different files in
/// the same directory are different documents (the `/.../` collapsing
/// rules are an HTTP route convention only), and a query has no
/// filesystem meaning (`file:///a.md?x` and `file:///a.md` are the same
/// file, which is also how the display normalizes them).
pub(crate) fn is_same_logical(a: &Url, b: &Url) -> bool {
    if a.scheme() != b.scheme() {
        return false;
    }
    if a.host_str() != b.host_str() {
        return false;
    }
    if a.port() != b.port() {
        return false;
    }
    if a.scheme() == "file" {
        return a.path() == b.path();
    }
    if logical_base(a.path()) != logical_base(b.path()) {
        return false;
    }
    a.query() == b.query()
}

/// "Server home" URL: `https://host/` (root).
/// Later resolution (`candidates_for`) tries `/`, `/index.md`, etc.
///
/// For a local file, "home" is its containing directory, resolved through
/// the same conventional indexes (`index.md`, `README.md`, ...).
pub fn server_home_of(current: &Url) -> Url {
    if current.scheme() == "file" {
        if let Ok(path) = current.to_file_path() {
            let parent = path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| std::path::PathBuf::from("/"));
            if let Ok(mut u) = Url::from_directory_path(&parent) {
                u.set_fragment(None);
                u.set_query(None);
                return u;
            }
        }
    }
    let mut u = current.clone();
    u.set_path("/");
    u.set_query(None);
    u.set_fragment(None);
    u
}

/// Minimal text to SHOW a URL in the UI, without scheme.
///
/// - Always `https://` internally (see `parse_https`), so it is hidden.
/// - E.g.: `https://ex.com/love/` -> `ex.com/love/`.
/// - Includes the port unless default 443, plus query (no fragment).
/// - IPv6 hosts are bracketed (`[::1]:8080/x`).
/// - A local `file:` URL shows its decoded path (`/Users/you/notes.md`) —
///   which is also valid input for `client::parse_navigable`.
/// - Used in navbar, titles and headers. Internal navigation still
///   uses the full schemed `Url`.
pub fn display_string(url: &Url) -> String {
    if url.scheme() == "file" {
        if let Ok(path) = url.to_file_path() {
            return path.display().to_string();
        }
        return url.path().to_string();
    }
    let mut s = String::new();
    if let Some(host) = url.host_str() {
        // IPv6: bracket exactly once regardless of `url` version
        // (`host_str` may or may not include them).
        if host.starts_with('[') {
            s.push_str(host);
        } else if host.contains(':') {
            s.push('[');
            s.push_str(host);
            s.push(']');
        } else {
            s.push_str(host);
        }
        // Explicit port only when not the https default.
        if let Some(port) = url.port() {
            s.push_str(&format!(":{port}"));
        }
    }
    let path = url.path();
    if path.is_empty() {
        s.push('/');
    } else {
        s.push_str(path);
    }
    if let Some(q) = url.query() {
        s.push('?');
        s.push_str(q);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn love_order() {
        let c = candidates_for(&u("https://ex.com/love"));
        let paths: Vec<_> = c.iter().map(|x| x.path().to_string()).collect();
        assert_eq!(
            paths,
            vec![
                "/love",
                "/love/",
                "/love.md",
                "/love/index.md",
                "/love/README.md",
            ]
        );
        // The cascade never exceeds the budget the visual timeout derives from.
        assert!(c.len() <= crate::config::MAX_ROUTE_CANDIDATES);
    }

    #[test]
    fn home_in_order() {
        let c = candidates_for(&u("https://ex.com/"));
        let paths: Vec<_> = c.iter().map(|x| x.path().to_string()).collect();
        assert_eq!(paths, vec!["/", "/index.md", "/README.md"]);
        assert!(c.len() <= crate::config::MAX_ROUTE_CANDIDATES);
    }

    #[test]
    fn normalizes_to_slash() {
        assert_eq!(
            normalized_display(&u("https://ex.com/love.md")).as_str(),
            "https://ex.com/love/"
        );
        assert_eq!(
            normalized_display(&u("https://ex.com/love")).as_str(),
            "https://ex.com/love/"
        );
        assert_eq!(
            normalized_display(&u("https://ex.com/")).as_str(),
            "https://ex.com/"
        );
    }

    #[test]
    fn base_from_variants() {
        assert_eq!(logical_base("/love/index.md"), "/love");
        assert_eq!(logical_base("/love/index.markdown"), "/love");
        assert_eq!(logical_base("/love/README.md"), "/love");
        assert_eq!(logical_base("/love/readme.md"), "/love");
        assert_eq!(logical_base("/love.md"), "/love");
        assert_eq!(logical_base("/love.markdown"), "/love");
        assert_eq!(logical_base("/love/"), "/love");
        assert_eq!(logical_base("/"), "");
        // A double slash before `index.md` must not leave a trailing slash.
        assert_eq!(logical_base("/love//index.md"), "/love");
        assert_eq!(logical_base("//index.md"), "");
        // A dotfile named `.md` stays a path, never the home base.
        assert_eq!(logical_base("/.md"), "/.md");
        // Nested dotfiles keep their file name too: `/a/.md` is not `/a`.
        assert_eq!(logical_base("/a/.md"), "/a/.md");
        assert_eq!(logical_base("/a/.markdown"), "/a/.markdown");
        assert_eq!(logical_base("/.markdown"), "/.markdown");
    }

    #[test]
    fn display_hides_scheme() {
        assert_eq!(display_string(&u("https://ex.com/love/")), "ex.com/love/");
        assert_eq!(display_string(&u("https://ex.com/")), "ex.com/");
        assert_eq!(
            display_string(&u("https://ex.com:8443/a?q=1")),
            "ex.com:8443/a?q=1"
        );
    }

    #[test]
    fn preserves_query_strips_fragment() {
        let c = candidates_for(&u("https://ex.com/love?q=1#frag"));
        assert!(c.iter().all(|x| x.query() == Some("q=1")));
        assert!(c.iter().all(|x| x.fragment().is_none()));
        assert_eq!(
            normalized_display(&u("https://ex.com/love.md?q=1#frag")).as_str(),
            "https://ex.com/love/?q=1"
        );
    }

    #[test]
    fn handles_ports_ipv6_and_case() {
        let c = candidates_for(&u("https://ex.com:8443/love"));
        assert!(c.iter().all(|x| x.port() == Some(8443)));
        let v6 = u("https://[::1]:8080/love");
        let c6 = candidates_for(&v6);
        assert!(!c6.is_empty());
        assert_eq!(display_string(&v6), "[::1]:8080/love");
        // Case-insensitive suffix collapse.
        assert_eq!(logical_base("/Love/INDEX.MD"), "/Love");
        assert_eq!(logical_base("/Love/ReadMe.Md"), "/Love");
    }

    #[test]
    fn same_logical_dedups_variants() {
        assert!(is_same_logical(
            &u("https://ex.com/love"),
            &u("https://ex.com/love/")
        ));
        assert!(is_same_logical(
            &u("https://ex.com/love.md"),
            &u("https://ex.com/love/index.md")
        ));
        assert!(!is_same_logical(
            &u("https://ex.com/love"),
            &u("https://ex.com/other")
        ));
        assert!(!is_same_logical(
            &u("https://ex.com/love?q=1"),
            &u("https://ex.com/love?q=2")
        ));
    }

    /// Local files keep their exact path for display and dedup; their
    /// "server home" is the containing directory.
    #[test]
    fn local_files_keep_their_exact_path() {
        let a = u("file:///Users/me/notes.md");
        let b = u("file:///Users/me/other.md");
        assert_eq!(display_string(&a), "/Users/me/notes.md");
        assert_eq!(normalized_display(&a).as_str(), "file:///Users/me/notes.md");
        // Same directory, different documents.
        assert!(!is_same_logical(&a, &b));
        // A fragment does not change the document.
        assert!(is_same_logical(&a, &u("file:///Users/me/notes.md#sec")));
        // A query has no filesystem meaning (the display drops it too).
        assert!(is_same_logical(&a, &u("file:///Users/me/notes.md?tab=1")));
        assert_eq!(server_home_of(&a).as_str(), "file:///Users/me/");
        // The HTTPS cascade never expands a file URL.
        assert_eq!(candidates_for(&a).len(), 1);
    }

    /// The candidate/display caps are BYTE-based: a path the input cap
    /// allows can percent-encode to more characters than it had bytes, and
    /// the old char check on the serialized URL dropped every candidate —
    /// the request was never even attempted.
    #[test]
    fn percent_encoded_paths_stay_within_the_byte_cap() {
        // 4096 4-byte chars: within the input cap (chars) and its worst-case
        // encoding (bytes).
        let encoded = "%F0%9F%98%80".repeat(4_096);
        let input = u(&format!("https://ex.com/{encoded}"));
        assert!(
            input.as_str().len() <= crate::config::MAX_URL_BYTES,
            "the encoded URL is under the byte cap"
        );
        let c = candidates_for(&input);
        assert_eq!(
            c.len(),
            crate::config::MAX_ROUTE_CANDIDATES,
            "every candidate survives the byte cap"
        );
        assert!(c
            .iter()
            .all(|u| u.as_str().len() <= crate::config::MAX_URL_BYTES));
        // ...and the normalized display URL keeps them too (the logical form
        // appends a trailing `/`, so compare the escaped path).
        let display = normalized_display(&input);
        assert!(display.as_str().starts_with("https://ex.com/"));
        assert!(display.as_str().contains("%F0%9F%98%80"));
        assert!(display.as_str().len() <= crate::config::MAX_URL_BYTES);
    }
}
