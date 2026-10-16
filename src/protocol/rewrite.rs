// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::rewrite`: absolutizing document links.
//!
//! RULE (scheme policy depends on where the document came from):
//! - From an `https` document only `https:` destinations are live; every
//!   other scheme (`mailto:`, `data:`, `file:`, ...) is `""` (ignore
//!   signal). A remote page must never name a local file.
//! - From a local (`file:`) document, `https:` and `file:` destinations are
//!   live (so local notes can link the web and each other); every other
//!   scheme stays inert.
//! - Relative or partial (`other`, `./other`, `../x`, `/absolute`, `?q`)
//!   => resolved against the DISPLAY url. Callers must pass `display_url`,
//!   not the raw `loaded_url` (`/love.md` would otherwise resolve `other`
//!   to `/other` instead of `/love/other`; a local file resolves against
//!   its containing directory).
//! - Same-document anchors (`#section`, `#fn-a`) are preserved verbatim so
//!   the viewer can scroll to the target; a bare `#` returns `""`. Whether
//!   the target exists is decided at render time (unknown anchors render as
//!   text), and a `#` fragment on another document is kept so the load can
//!   scroll to it once resolved.
//!
//! - This happens BEFORE rendering (the renderer only sees absolute URLs).
//! - Unresolvable destinations return `""` (inert): the renderer shows
//!   plain text and never navigates.
//! - Schemes are blocked at navigation time
//!   (here we only rewrite; blocking lives in `protocol::client`).

use crate::markdown::anchors::AnchorIndex;
use pulldown_cmark::{Event, Tag};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

/// True when `dest` names the local filesystem of the machine the browser
/// runs on (a `file:` URL with an empty or localhost authority). The
/// authority rule lives in [`crate::protocol::local::is_local_host`] so
/// rewrite and the reader cannot disagree (`file://localhost/x` used to be
/// rewritten as a live link and then rejected on navigation).
///
/// Credentials and ports are rejected too, mirroring
/// [`crate::protocol::local::parse_file_url`]: `file://user:pass@localhost/x`
/// can never be a live link (it would only fail at navigation time, and
/// credentials must never ride along a destination).
fn is_local_file(u: &Url) -> bool {
    u.scheme() == "file"
        && crate::protocol::local::is_local_host(u)
        && u.username().is_empty()
        && u.password().is_none()
        && u.port().is_none()
}

/// True when a `file:` destination is written in an ABSOLUTE spelling
/// (`file:/x`, `file:///x`, `file://localhost/x`).
///
/// The WHATWG parser accepts a relative spelling (`file:notes.md`) by
/// rebasing it onto the filesystem ROOT (`/notes.md`), which would open a
/// file the document never named; the reader rejects the same spelling (see
/// `protocol::local::parse_file_url`).
fn file_dest_is_absolute(d: &str) -> bool {
    match d.split_once(':') {
        Some((scheme, rest)) => scheme.eq_ignore_ascii_case("file") && rest.starts_with('/'),
        None => false,
    }
}

/// True when a destination carries authority credentials. Such a
/// destination is never live: it would render a link whose visible URL
/// embeds a secret, and a scheme-relative spelling (`//user:pass@host/x`)
/// only reveals the real target after the join (the click would land
/// somewhere other than the label suggests).
fn has_credentials(u: &Url) -> bool {
    !u.username().is_empty() || u.password().is_some()
}

/// Resolves the omnibox's schemeless `host:port` spellings that `Url::parse`
/// cannot read as a URL (`127.0.0.1:8080/x`, `[::1]:8080/x`): a URL scheme
/// must start with a letter, so the parser rejects a digit or `[` there
/// while `client::parse_https` accepts the same text as `https://…`.
///
/// `None` for every other shape (relative paths, blocked schemes, bare
/// `word:digits` single labels) and whenever the resolved URL would carry
/// credentials. The returned string is the exact form the omnibox would
/// navigate to.
fn schemeless_host_port(d: &str) -> Option<String> {
    let slash = d.find('/').unwrap_or(d.len());
    // The PORT colon: the last one before the path. `[::1]:8080/y` carries
    // colons inside the brackets, so a first-colon split would misread it.
    let colon = d[..slash].rfind(':')?;
    let pre = d[..colon].trim();
    // Same host-like rule as `parse_https` minus the alpha-only forms the
    // parser branch above already handles: dotted names, IP literals and
    // `localhost`. A single-label `word:digits` stays inert (a scheme).
    let host_like = pre.contains('.') || pre.contains('[') || pre.eq_ignore_ascii_case("localhost");
    if !host_like || crate::protocol::client::is_blocked_scheme(pre) {
        return None;
    }
    // The tail must open with a NUMERIC port (`host:8080`), like the omnibox.
    if !d[colon + 1..]
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
    {
        return None;
    }
    let resolved = Url::parse(&format!("https://{d}")).ok()?;
    (resolved.host_str().is_some()
        && resolved.username().is_empty()
        && resolved.password().is_none())
    .then(|| resolved.to_string())
}

/// Absolutizes a Markdown link destination against `base`.
///
/// - Returns the absolute URL as a `String`, ready to render/navigate.
/// - `dest` may carry whitespace (we trim it).
/// - Destinations the BASE document may not follow (`http:`, `mailto:`,
///   `data:`, `javascript:`, and `file:` from a remote page) return `""`
///   (ignore signal): the caller must NOT navigate, so clicking never
///   destroys the current document with a `Failed` state.
/// - A same-document anchor (`#frag`) is preserved (the viewer decides
///   whether the target exists); a bare `#` returns `""`.
pub fn absolutize(base: &Url, dest: &str) -> String {
    let base_is_file = base.scheme() == "file";
    // The destination schemes this base may follow.
    let allowed = |scheme: &str| scheme == "https" || (base_is_file && scheme == "file");
    let d = dest.trim();
    // Empty destination: inert (no reload of the same page).
    if d.is_empty() {
        return String::new();
    }
    // In-page anchor: preserved for the viewer's anchor navigation. A bare
    // `#` has no target, so it stays inert.
    if let Some(fragment) = d.strip_prefix('#') {
        return if fragment.trim().is_empty() {
            String::new()
        } else {
            d.to_string()
        };
    }
    // 1) Already absolute? If it has a scheme (`https:`, `mailto:`, ...),
    //    keep it only when this base may follow it; otherwise signal ignore.
    if let Ok(u) = Url::parse(d) {
        // A `file:` destination is live only when it names this device,
        // with an absolute spelling AND a real path (`file://` and
        // `file:///` parse but would die at navigation time).
        let local_file_ok = u.scheme() != "file"
            || (is_local_file(&u)
                && file_dest_is_absolute(d)
                && !u.path().is_empty()
                && u.path() != "/");
        if allowed(u.scheme()) && !has_credentials(&u) && local_file_ok {
            return d.to_string();
        }
        // A `host:port` spelling (`localhost:3000`, `example.com:8080/x`) is
        // read by `Url::parse` as a URL whose scheme is the HOST and whose
        // port lives in the path. The omnibox accepts these spellings
        // (`client::parse_https`), so links must resolve the same way
        // instead of silently rendering as plain text. Only a host-LIKE
        // left side with a NUMERIC port qualifies, and never a blocked
        // scheme: `javascript:alert(1)` and `mailto:x` stay inert.
        let scheme = u.scheme();
        let host_like = scheme.eq_ignore_ascii_case("localhost") || scheme.contains('.');
        let rest = d.get(scheme.len() + 1..).unwrap_or_default();
        let numeric_port = rest
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit());
        if host_like && numeric_port && !crate::protocol::client::is_blocked_scheme(scheme) {
            if let Ok(resolved) = Url::parse(&format!("https://{d}")) {
                if resolved.host_str().is_some()
                    && resolved.username().is_empty()
                    && resolved.password().is_none()
                {
                    return resolved.to_string();
                }
            }
        }
        return String::new();
    }
    // 1b) A `host:port` spelling the URL parser cannot even read as a URL:
    //     IPv4/IPv6 literals (`127.0.0.1:8080/x`, `[::1]:8080/x`). A URL
    //     scheme must start with a letter, so `Url::parse` rejects these
    //     while the omnibox accepts them (`client::parse_https` treats a
    //     pre-colon token containing `.` or `[` as a host). Without this,
    //     the join below turned one into a relative path under the base.
    if let Some(resolved) = schemeless_host_port(d) {
        return resolved;
    }
    // Extra guard for `xxx:...` before any `/` (scheme-like, no `//`).
    if let Some(colon) = d.find(':') {
        let slash = d.find('/').unwrap_or(d.len());
        if colon < slash {
            let pre = d[..colon].trim();
            let scheme_syntax = !pre.is_empty()
                && pre.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && pre
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
            // An ALLOWED scheme spelling that failed to parse (`https://`,
            // `https:`, `file:` from a local base) is unresolvable, never a
            // relative path: the join below would turn it into
            // `.../https://` and render a bogus link.
            if scheme_syntax
                && (pre.eq_ignore_ascii_case("https")
                    || (base_is_file && pre.eq_ignore_ascii_case("file")))
            {
                return String::new();
            }
            // Every other scheme-like token stays inert; a dot distinguishes
            // a host:port spelling (`example.com:8080`), already resolved by
            // the parser branch above when it parses.
            if scheme_syntax && !pre.contains('.') {
                return String::new();
            }
        }
    }
    // 2) Relative/partial: `join` per RFC 3986.
    //    Covers `other`, `./other`, `../x`, `/abs`, `?q=1`, `#frag`.
    match base.join(d) {
        Ok(u) => {
            // A same-document anchor written in a relative spelling
            // (`./#sec`, `.//#sec`) comes back as base + `#frag`. Hand the
            // viewer the canonical `#frag` form (like a literal `[x](#sec)`)
            // instead of reloading the page (fragment resolution strips the
            // old fragment) or silently dropping it.
            //
            // The ORIGIN must match too: `//other.example/same/path#install`
            // joins to a different host with the same path, and must
            // navigate (or be rejected), never scroll the current page.
            // A remote file share (`//server/share/x.md#s`) must not slip
            // through as an in-page anchor either.
            if u.query().is_none()
                && u.fragment().is_some()
                && u.path() == base.path()
                && u.scheme() == base.scheme()
                && u.host_str() == base.host_str()
                && u.port_or_known_default() == base.port_or_known_default()
            {
                return format!("#{}", u.fragment().unwrap_or_default());
            }
            // Joined to a scheme this base may not follow (shouldn't happen
            // with the checks above, but be safe). A file join that landed on
            // a remote host (`//host/x`) is refused too, and a join that
            // introduced credentials (`//user:pass@host/x`) is never live.
            if !allowed(u.scheme()) || has_credentials(&u) {
                return String::new();
            }
            if u.scheme() == "file" && !is_local_file(&u) {
                return String::new();
            }
            u.to_string()
        }
        // Unresolvable: inert (never attempt navigation with garbage).
        Err(_) => String::new(),
    }
}

/// True when a link/image destination is within the navigation input cap.
///
/// A longer destination can never load (`parse_navigable` rejects it) and used
/// to be `Url::parse`d every frame; treat it as inert at capture time. The
/// byte check makes the common case O(1): a UTF-8 char is 1..=4 bytes, so
/// a string under the byte cap is under the char cap too.
pub(crate) fn dest_within_cap(dest: &str) -> bool {
    dest.len() <= crate::config::MAX_URL_INPUT_CHARS
        || dest.chars().count() <= crate::config::MAX_URL_INPUT_CHARS
}

/// Resolves a link destination for rendering: [`absolutize`], then an
/// unknown same-document `#anchor` becomes the inert `""` (plain text).
fn resolve_link(base: &Url, anchors: &AnchorIndex, raw: &str) -> String {
    if !dest_within_cap(raw) {
        return String::new();
    }
    let dest = absolutize(base, raw);
    // A resolved destination can outgrow the raw one (percent-encoding, a
    // long base). Anything past the navigation input cap can never load, so
    // it is inert here too — and the cache below can bound each entry.
    if !dest_within_cap(&dest) {
        return String::new();
    }
    if let Some(id) = dest.strip_prefix('#') {
        if !anchors.contains(id) {
            return String::new();
        }
    }
    dest
}

/// Every link/image destination of one document, resolved ONCE at load time.
///
/// The viewer rebuilds its fragment tree on every frame; parsing each
/// destination (`Url::parse` + `base.join`) per frame was measurable on
/// link-dense READMEs. The cache is keyed by the raw `dest_url` exactly as
/// the event stream spells it, so a lookup is one hash of a borrowed `&str`
/// and only distinct destinations allocate.
///
/// One map serves both links and images: the image form differs ONLY for a
/// same-document anchor (an image never navigates), which [`Self::image_rc`]
/// derives from the stored link instead of duplicating every entry. Storage
/// is bounded by [`crate::config::MAX_DEST_CACHE_BYTES`]: a hostile
/// link-farm that resolves hundreds of thousands of distinct URLs cannot
/// retain gigabytes from one small body. Past the budget the missing
/// destinations fail SAFE (plain text / placeholder), exactly like an
/// unresolvable one.
#[derive(Debug, Default, Clone)]
pub struct DestCache {
    links: HashMap<String, Arc<str>>,
}

impl DestCache {
    /// Builds the cache by walking the parsed event stream once.
    pub fn build(base: &Url, anchors: &AnchorIndex, events: &[Event<'static>]) -> Self {
        Self::build_with_budget(base, anchors, events, crate::config::MAX_DEST_CACHE_BYTES)
    }

    /// [`Self::build`] with an explicit storage budget (tests use a small
    /// one to prove the cap without allocating megabytes).
    fn build_with_budget(
        base: &Url,
        anchors: &AnchorIndex,
        events: &[Event<'static>],
        budget: usize,
    ) -> Self {
        let mut cache = Self::default();
        let mut remaining = budget;
        for event in events {
            let raw = match event {
                Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                    dest_url.as_ref()
                }
                _ => continue,
            };
            if cache.links.contains_key(raw) {
                continue;
            }
            let link = resolve_link(base, anchors, raw);
            // Inert destinations are NOT stored: a missing key already
            // resolves to `""`, so recording thousands of rejected links
            // would only waste the budget. (This also keeps the fail-safe
            // behavior identical.)
            if link.is_empty() {
                continue;
            }
            // The cost covers the REAL retention, not just the string bytes:
            // a `HashMap` entry also carries buckets/control bytes and the
            // `String`/`Arc<str>`/allocator headers (measured ~90 bytes per
            // small entry), so counting payload alone let a link farm retain
            // several times the documented budget.
            const ENTRY_OVERHEAD: usize = 96;
            let cost = raw
                .len()
                .saturating_add(link.len())
                .saturating_add(ENTRY_OVERHEAD);
            if cost > remaining {
                continue;
            }
            remaining -= cost;
            // Interned as `Arc<str>`: the per-frame fragment tree clones the
            // HANDLE (a refcount bump) instead of the URL string, so one
            // destination referenced by thousands of fragments stays one
            // allocation (see `MAX_LINK_HITS` for the rendered hitboxes).
            cache.links.insert(raw.to_string(), Arc::from(link));
        }
        cache
    }

    /// Resolved destination for a link (`None` = inert, render as plain
    /// text). A clone is a refcount bump, never a string copy.
    pub fn link_rc(&self, raw: &str) -> Option<Arc<str>> {
        self.links.get(raw).cloned()
    }

    /// Resolved destination for an image (`None` = inert placeholder).
    /// Images never navigate, so a same-document anchor link is inert for
    /// them.
    pub fn image_rc(&self, raw: &str) -> Option<Arc<str>> {
        match self.links.get(raw) {
            Some(link) if !link.starts_with('#') => Some(link.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolutizes_relative() {
        let base: Url = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(
            absolutize(&base, "other"),
            "https://ex.com/love/other".to_string()
        );
        assert_eq!(
            absolutize(&base, "/absolute"),
            "https://ex.com/absolute".to_string()
        );
        // Absolute: kept intact.
        assert_eq!(
            absolutize(&base, "https://other.com/x"),
            "https://other.com/x".to_string()
        );
    }

    #[test]
    fn blocks_non_https_and_preserves_anchors() {
        let base: Url = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(absolutize(&base, ""), "");
        // Same-document anchors are kept for the viewer to resolve.
        assert_eq!(absolutize(&base, "#frag"), "#frag");
        assert_eq!(absolutize(&base, "  #fn-a  "), "#fn-a");
        // A bare `#` has no target: inert.
        assert_eq!(absolutize(&base, "#"), "");
        assert_eq!(absolutize(&base, "#   "), "");
        assert_eq!(absolutize(&base, "mailto:a@b.com"), "");
        assert_eq!(absolutize(&base, "javascript:alert(1)"), "");
        assert_eq!(absolutize(&base, "data:text/plain,hi"), "");
        assert_eq!(absolutize(&base, "http://ex.com/x"), "");
        assert_eq!(absolutize(&base, "tel:+123"), "");
    }

    #[test]
    fn resolves_dot_segments_and_query() {
        let base: Url = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(absolutize(&base, "../x"), "https://ex.com/x".to_string());
        assert_eq!(
            absolutize(&base, "./other"),
            "https://ex.com/love/other".to_string()
        );
        assert_eq!(
            absolutize(&base, "?q=1"),
            "https://ex.com/love/?q=1".to_string()
        );
        // A relative spelling of a same-document anchor is normalized to the
        // canonical fragment form instead of being dropped.
        assert_eq!(absolutize(&base, "./#sec"), "#sec");
        // ...but only on the SAME origin: a same-path spelling on another
        // host is a real navigation, not an in-page scroll.
        assert_eq!(
            absolutize(&base, "//other.example/love/#sec"),
            "https://other.example/love/#sec"
        );
        assert_eq!(
            absolutize(&base, "//other.example/love/"),
            "https://other.example/love/"
        );
        // A fragment on another document is kept for post-load scrolling.
        assert_eq!(
            absolutize(&base, "other.md#sec"),
            "https://ex.com/love/other.md#sec".to_string()
        );
    }

    /// A local document may follow relative files and the web, but never a
    /// remote file share or a non-navigable scheme.
    #[test]
    fn local_base_follows_relative_files_and_https() {
        let base = Url::parse("file:///Users/me/docs/notes.md").unwrap();
        assert_eq!(
            absolutize(&base, "other.md"),
            "file:///Users/me/docs/other.md"
        );
        assert_eq!(absolutize(&base, "../x.md"), "file:///Users/me/x.md");
        assert_eq!(absolutize(&base, "https://ex.com/x"), "https://ex.com/x");
        assert_eq!(absolutize(&base, "file:///tmp/a.md"), "file:///tmp/a.md");
        // Remote file shares, mail and scripts stay inert.
        assert_eq!(absolutize(&base, "file://server/share/a.md"), "");
        assert_eq!(absolutize(&base, "mailto:a@b.com"), "");
        assert_eq!(absolutize(&base, "javascript:alert(1)"), "");
        // Credentials/ports on a file URL can never be a live link: the
        // reader would reject them at navigation time, so the rewrite must
        // not adopt them either (they would only destroy the page later,
        // and credentials must never ride along a destination).
        assert_eq!(absolutize(&base, "file://user:pass@localhost/tmp/a.md"), "");
        assert_eq!(absolutize(&base, "file://localhost:8080/tmp/a.md"), "");
        // A RELATIVE `file:` spelling is not a navigable target either: the
        // URL parser rebases `file:notes.md` onto the filesystem root, so the
        // link stays inert instead of opening a file the user never named.
        assert_eq!(absolutize(&base, "file:notes.md"), "");
        assert_eq!(absolutize(&base, "file:../notes.md"), "");
        // Same-document anchors still work from a local file.
        assert_eq!(absolutize(&base, "#sec"), "#sec");
    }

    /// `host:port` spellings the omnibox accepts (`client::parse_https`)
    /// must resolve as links too instead of rendering inert; blocked
    /// schemes and bare single-label `word:digits` stay inert.
    #[test]
    fn host_port_spellings_resolve_as_https() {
        let base = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(
            absolutize(&base, "localhost:3000"),
            "https://localhost:3000/"
        );
        assert_eq!(
            absolutize(&base, "example.com:8080/docs"),
            "https://example.com:8080/docs"
        );
        // Single-label host:port (no dot, not localhost) stays inert in
        // document links: `nas:5000` is indistinguishable from a scheme.
        assert_eq!(absolutize(&base, "nas:5000"), "");
        // Blocked schemes never become host:port, even with a numeric tail.
        assert_eq!(absolutize(&base, "javascript:123"), "");
        assert_eq!(absolutize(&base, "data:123"), "");
        assert_eq!(absolutize(&base, "mailto:123"), "");
        // Userinfo spellings are not silently adopted.
        assert_eq!(absolutize(&base, "user:pass@example.com:8080"), "");
        // IP-literal host:port spellings: `Url::parse` rejects a digit-led
        // scheme, but the omnibox accepts these, so links must resolve the
        // same way instead of silently becoming relative paths under the
        // base.
        assert_eq!(
            absolutize(&base, "127.0.0.1:8080/x"),
            "https://127.0.0.1:8080/x"
        );
        assert_eq!(absolutize(&base, "[::1]:8080/y"), "https://[::1]:8080/y");
        // An allowed scheme spelled but unparseable (`https://`) is inert,
        // never a bogus relative link.
        assert_eq!(absolutize(&base, "https://"), "");
        assert_eq!(absolutize(&base, "https:"), "");
        assert_eq!(absolutize(&base, "http://"), "");
    }

    /// The same inert rule from a local base (`file:` joins onto the
    /// filesystem root otherwise) and the relative-path control.
    #[test]
    fn malformed_allowed_schemes_are_inert() {
        let remote = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(absolutize(&remote, "https://"), "");
        let local = Url::parse("file:///Users/me/docs/notes.md").unwrap();
        assert_eq!(absolutize(&local, "file:"), "");
        assert_eq!(absolutize(&local, "file://"), "");
        assert_eq!(absolutize(&local, "https://"), "");
        // A normal relative path still joins.
        assert_eq!(
            absolutize(&remote, "other.md"),
            "https://ex.com/love/other.md"
        );
    }

    /// A remote page can never name a local file (security contract).
    #[test]
    fn remote_base_never_lives_local_files() {
        let base = Url::parse("https://ex.com/love/").unwrap();
        assert_eq!(absolutize(&base, "file:///etc/passwd"), "");
        assert_eq!(absolutize(&base, "file:/etc/passwd"), "");
        assert_eq!(
            absolutize(&base, "notes.md"),
            "https://ex.com/love/notes.md"
        );
    }

    /// The destination cache must resolve exactly like the renderer did when
    /// it resolved per frame: links to unknown anchors go inert, images
    /// never keep `#`, over-cap destinations stay inert, and the image form
    /// differs from the link form ONLY for a same-document anchor.
    #[test]
    fn dest_cache_matches_per_render_resolution() {
        let base = Url::parse("https://ex.com/docs/page/").unwrap();
        let huge = "a".repeat(crate::config::MAX_URL_INPUT_CHARS + 1);
        let md = format!(
            "# Sec\n\n\
             [rel](other.md) [abs](https://x.dev/a) [bad](mailto:a@b.com) \
             [ok](#sec) [missing](#nope) [dot](./#sec) [huge](https://x.dev/{huge})\n\n\
             ![img](pic.png) ![frag](#sec) ![dot](./#sec)\n"
        );
        let normalized = crate::markdown::document::normalize_body(&md);
        let events = crate::markdown::document::parse_events(&normalized).expect("parse");
        let anchors = crate::markdown::anchors::build(&events);
        let cache = DestCache::build(&base, &anchors, &events);

        assert_eq!(
            cache.link_rc("other.md").as_deref(),
            Some("https://ex.com/docs/page/other.md")
        );
        assert_eq!(
            cache.link_rc("https://x.dev/a").as_deref(),
            Some("https://x.dev/a")
        );
        assert_eq!(cache.link_rc("mailto:a@b.com"), None, "scheme stays inert");
        assert_eq!(
            cache.link_rc("#sec").as_deref(),
            Some("#sec"),
            "known anchor survives"
        );
        assert_eq!(cache.link_rc("#nope"), None, "unknown anchor goes inert");
        assert_eq!(
            cache.link_rc("./#sec").as_deref(),
            Some("#sec"),
            "relative anchor canonicalizes"
        );
        assert_eq!(
            cache.link_rc(&format!("https://x.dev/{huge}")),
            None,
            "over-cap destination is inert"
        );

        assert_eq!(
            cache.image_rc("pic.png").as_deref(),
            Some("https://ex.com/docs/page/pic.png")
        );
        assert_eq!(
            cache.image_rc("#sec"),
            None,
            "an image never keeps an anchor"
        );
        assert_eq!(cache.image_rc("./#sec"), None);
        assert_eq!(
            cache.image_rc("other.md").as_deref(),
            cache.link_rc("other.md").as_deref(),
            "non-anchor destinations share the resolved form"
        );
        // A raw destination that was never in the document is inert, never
        // a panic or a silent re-parse.
        assert_eq!(cache.link_rc("never-seen"), None);
        assert_eq!(cache.image_rc("never-seen"), None);
    }

    /// The cache storage is bounded: distinct destinations past the budget
    /// fail SAFE (plain text / placeholder) instead of retaining megabytes,
    /// and the entries that fit still resolve exactly as before.
    #[test]
    fn dest_cache_stops_storing_past_its_budget() {
        let base = Url::parse("https://ex.com/docs/page/").unwrap();
        // Ten distinct links, each resolved to ~30+ bytes.
        let md: String = (0..10).map(|i| format!("[l{i}](file{i}.md)\n\n")).collect();
        let normalized = crate::markdown::document::normalize_body(&md);
        let events = crate::markdown::document::parse_events(&normalized).expect("parse");
        let anchors = crate::markdown::anchors::build(&events);
        // Room for the first couple of destinations only (each entry's cost
        // includes the container/allocator overhead).
        let cache = DestCache::build_with_budget(&base, &anchors, &events, 300);
        assert_eq!(
            cache.link_rc("file0.md").as_deref(),
            Some("https://ex.com/docs/page/file0.md")
        );
        // Later links fall out of the budget: inert (never a panic).
        assert_eq!(cache.link_rc("file9.md"), None);
        // The budget is a running total, not a per-entry cap: count how many
        // fit and prove the rest are empty.
        let stored = (0..10)
            .filter(|i| cache.link_rc(&format!("file{i}.md")).is_some())
            .count();
        assert!(
            (1..10).contains(&stored),
            "some (not all) destinations fit: {stored}"
        );
    }
}
