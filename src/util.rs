// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `util`: small shared helpers (display text, page validation, time).
//!
//! - Single home for the scheme-hiding and truncation helpers that used
//!   to be copy-pasted across `browser::tab`, `ui::error_view` and
//!   `viewer`.
//! - [`normalize_stored_url`] is the shared validation/normalization for the
//!   encrypted `history` and `bookmarks` stores; [`now_secs`] their shared
//!   clock.
//! - All functions are pure, allocation-light and ASCII-safe where noted.
//! - Internal navigation always keeps the full `https://`/`file://` URL;
//!   these helpers only shape what the user sees.
//!
//! Sanitizer map (which helper for which sink; they overlap on purpose):
//! - [`strip_credentials_for_log`] — LOG/ERROR lines: removes `user:pass@`
//!   userinfo token by token. Every "bad address" message goes through
//!   [`safe_input_echo`] (truncate + strip); the error view applies it via
//!   `display_line` to `Requested:`, details and Copy-log alike.
//! - [`sanitize_title`] — DISPLAY/STORAGE titles: bounds and cleans
//!   frontmatter/first-heading titles before tabs, history and bookmarks
//!   store or show them.
//! - `protocol::client::sanitize_for_display` — DISPLAY addresses: strips
//!   credentials and hides the scheme before a URL is stored or echoed.
//! - `protocol::local::echo_url` — the local-file twin of the log sink
//!   above (bounded + stripped `file:` URL for read-error lines).
//! - `bookmarks::sanitize_glyph`, `media::decode::sanitize_svg_structure` —
//!   STORAGE/DECODE gates for tile glyphs and SVG structure, not addresses.

/// Removes one leading `https://`, `http://` or `file://` prefix for display.
///
/// - Case-insensitive (`HTTPS://` is hidden too).
/// - No-op when no scheme is present.
/// - Trims surrounding whitespace first; the UI never shows the scheme
///   because the browser owns the transport ({https|file}:// internally).
/// - Returns `Cow::Borrowed` when nothing is stripped (the common display
///   path), so per-frame title/URL rows do not allocate.
pub fn strip_scheme_prefix(value: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = value.trim();
    // Case-insensitive prefix check without allocating a lowered copy of
    // the whole URL (the old `to_ascii_lowercase` allocated per row per
    // frame on history/suggestion lists).
    fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
        if s.len() < prefix.len() {
            return None;
        }
        if s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
            Some(&s[prefix.len()..])
        } else {
            None
        }
    }
    if let Some(rest) = strip_prefix_ignore_case(trimmed, "https://") {
        return std::borrow::Cow::Borrowed(rest);
    }
    if let Some(rest) = strip_prefix_ignore_case(trimmed, "http://") {
        return std::borrow::Cow::Borrowed(rest);
    }
    if let Some(rest) = strip_prefix_ignore_case(trimmed, "file://") {
        return std::borrow::Cow::Borrowed(rest);
    }
    if trimmed.len() == value.len() {
        std::borrow::Cow::Borrowed(value)
    } else {
        std::borrow::Cow::Borrowed(trimmed)
    }
}

/// Case-insensitive ASCII `starts_with`, allocation-free.
///
/// The error view used to `to_ascii_lowercase()` the whole asked URL every
/// frame just to decide whether it is a local path.
pub(crate) fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len()
        && value.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Removes every `https://` and `file://` occurrence (any letter case) for
/// display.
///
/// Used for multi-line logs where a candidate URL may appear mid-sentence.
/// Same diagnostic value, less visual noise.
///
/// Returns `Cow::Borrowed` when nothing had to be removed (the common
/// display path: most log lines name no URL), so the per-frame error view
/// does not allocate one `String` per line per repaint.
///
/// NOTE: `to_ascii_lowercase` preserves byte length for ASCII, so indices
/// into the lowered view are valid for the original string. Both schemes
/// are pure ASCII, hence the scan is sound.
pub fn strip_scheme_everywhere(value: &str) -> std::borrow::Cow<'_, str> {
    // Fast path FIRST, without materializing the lowercase copy: most log
    // lines name no URL, and the old unconditional `to_ascii_lowercase`
    // allocated one String the size of the input per line per repaint
    // (defeating this function's own Cow contract).
    if !contains_ignore_ascii_case(value, "https://")
        && !contains_ignore_ascii_case(value, "file://")
    {
        return std::borrow::Cow::Borrowed(value);
    }
    let lower = value.to_ascii_lowercase();
    debug_assert_eq!(lower.len(), value.len());
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    let mut rest_lower = lower.as_str();
    while let Some(pos) = {
        let https = rest_lower.find("https://");
        let file = rest_lower.find("file://");
        match (https, file) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    } {
        let len = if rest_lower[pos..].starts_with("https://") {
            "https://".len()
        } else {
            "file://".len()
        };
        out.push_str(&rest[..pos]);
        rest = &rest[pos + len..];
        rest_lower = &rest_lower[pos + len..];
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// Truncates text to at most `max_chars` characters INCLUDING the `...`
/// appended when cut.
///
/// - Counts Unicode scalar values (the documented cap), so multi-byte
///   characters stay intact (never panics on UTF-8 boundaries).
/// - Returns a borrowed `Cow` when the input already fits (no allocation
///   on the common display path).
/// - `max_chars < 3` yields that many `.` (the result never exceeds the cap).
/// - Never cuts INSIDE a grapheme cluster: a combining mark, a ZWJ emoji
///   family or a flag stays whole (or is dropped whole when it cannot fit),
///   so the cap cannot produce a dangling joiner or a bare combining mark.
pub fn truncate_chars(value: &str, max_chars: usize) -> std::borrow::Cow<'_, str> {
    // Single walk: detect whether the string exceeds the cap.
    let mut exceeds = false;
    for (count, _) in value.char_indices().enumerate() {
        if count == max_chars {
            exceeds = true;
            break;
        }
    }
    if !exceeds {
        return std::borrow::Cow::Borrowed(value);
    }
    // Cut to `max_chars - 3` SCALARS, but only at a cluster boundary, then
    // append the ellipsis, so the result is at most `max_chars` chars (the
    // doc/config contract). Clusters are taken whole: the first one alone can
    // exceed the budget (`👨‍👩‍👧‍👦` is 7 scalars), in which case nothing is
    // kept — a partial cluster would render as a different character.
    let keep = max_chars.saturating_sub(3);
    let mut out = String::with_capacity(value.len().min(max_chars * 4));
    let mut scalars = 0usize;
    for cluster in unicode_segmentation::UnicodeSegmentation::graphemes(value, true) {
        let n = cluster.chars().count();
        if scalars + n > keep {
            break;
        }
        scalars += n;
        out.push_str(cluster);
    }
    out.extend(std::iter::repeat_n('.', max_chars.min(3)));
    std::borrow::Cow::Owned(out)
}

/// Case-insensitive ASCII substring search.
///
/// `needle` must already be lowercase and ASCII (the autocomplete caller
/// lowercases the query once, not the candidates); `haystack` may be any
/// case. Allocation-free: the old `haystack.to_lowercase().contains(...)`
/// allocated one `String` per candidate per keystroke, i.e. tens of
/// thousands of allocations on a 20 000-entry history scan.
pub(crate) fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let (hay, needle) = (haystack.as_bytes(), needle.as_bytes());
    if needle.len() > hay.len() {
        return false;
    }
    // Byte-window comparison: every boundary tested is a fixed-length
    // window, never a `str` slice, so non-ASCII haystacks cannot panic.
    hay.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Strips `user[:pass]@` credentials from a log line.
///
/// - Applied to network error text before it reaches the error view:
///   `reqwest` errors can embed the redirect URL, and a malicious server
///   could redirect to `https://user:pass@host/` so its credentials land
///   in our line-by-line log. Pasted credentials are also stripped before
///   they reach the error view or the omnibox (see
///   `protocol::client::sanitize_for_display`), but this closes the server
///   side too.
/// - Recognizes every authority spelling a URL parser recognizes:
///   `scheme://[extra slashes]userinfo@authority`,
///   the malformed `scheme:/userinfo@authority`, and protocol-relative
///   `//userinfo@authority`. Userinfo is `@` inside the authority (before
///   the next `/`, `\`, `?` or `#`), with or without a password.
/// - Plain `mailto:a@b` text is left alone: after `scheme:` there is no
///   slash run, so no authority position exists.
/// - Works token by token (whitespace-delimited) and copies every other
///   byte untouched; all slice indices are ASCII delimiter boundaries, so
///   slicing never panics on UTF-8.
pub fn strip_credentials_for_log(line: &str) -> String {
    let mut out: Option<String> = None;
    let mut last = 0usize;
    let mut i = 0usize;
    let bytes = line.as_bytes();
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let token = &line[start..i];
        if let Some(clean) = strip_token_credentials(token) {
            let buf = out.get_or_insert_with(|| String::with_capacity(line.len()));
            buf.push_str(&line[last..start]);
            buf.push_str(&clean);
            last = i;
        }
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&line[last..]);
            buf
        }
        None => line.to_string(),
    }
}

/// Removes the userinfo from one whitespace-delimited token when the token
/// contains an authority position. `None` when there is nothing to strip.
fn strip_token_credentials(token: &str) -> Option<String> {
    let mut current = token.to_string();
    let mut changed = false;
    // 1) `scheme://[more slashes]userinfo@authority` (`http:///u:p@h`),
    //    left to right: an error line can embed a redirect URL whose PATH
    //    contains another URL (`?next=https://u:p@evil`), so every
    //    occurrence is examined.
    let mut search = 0usize;
    while let Some(rel) = current[search..].find("://") {
        let sep = search + rel;
        let mut auth_start = sep + "://".len();
        while matches!(current.as_bytes().get(auth_start), Some(b'/' | b'\\')) {
            auth_start += 1;
        }
        if let Some(clean) = strip_userinfo_at(&current, auth_start) {
            current = clean;
            changed = true;
        }
        // Always advance past the authority separator (whether rewritten or
        // not), so the scan makes progress and a later `://` is still seen.
        search = auth_start.max(sep + "://".len());
    }
    let bytes = current.as_bytes();
    // 2) Protocol-relative `//userinfo@authority` (no scheme to anchor on).
    if current.starts_with("//") || current.starts_with(r"\\") {
        let run = bytes
            .iter()
            .take_while(|b| **b == b'/' || **b == b'\\')
            .count();
        if let Some(clean) = strip_userinfo_at(&current, run) {
            current = clean;
            changed = true;
        }
    } else if let Some(colon) = current.find(':') {
        // 3) Malformed `scheme:/userinfo@authority` (a single separator, so
        //    no `://` above) and the no-separator spelling
        //    `scheme:user:pass@host` that `parse_https` blocks as scheme
        //    syntax: both must lose the pasted password before display. A
        //    second colon before the `@` marks userinfo (`user:pass`); a
        //    single colon keeps `host:port` untouched.
        if is_scheme_token(&current[..colon]) {
            let auth_start = colon
                + 1
                + bytes[colon + 1..]
                    .iter()
                    .take_while(|b| **b == b'/' || **b == b'\\')
                    .count();
            let run = auth_start - (colon + 1);
            let authority_end = current[auth_start..]
                .find(['/', '?', '#', '\\'])
                .map_or(current.len(), |rel| auth_start + rel);
            let has_userinfo_password = current[auth_start..authority_end]
                .rfind('@')
                .is_some_and(|at| current[auth_start..auth_start + at].contains(':'));
            if run > 0 || has_userinfo_password {
                if let Some(clean) = strip_userinfo_at(&current, auth_start) {
                    current = clean;
                    changed = true;
                }
            }
        }
    }
    changed.then_some(current)
}

/// Rebuilds `token` without the userinfo found at `auth_start`.
fn strip_userinfo_at(token: &str, auth_start: usize) -> Option<String> {
    let after = token.get(auth_start..)?;
    let auth_end = after.find(['/', '?', '#', '\\']).unwrap_or(after.len());
    let authority = &after[..auth_end];
    let at = authority.rfind('@')?;
    let mut clean = String::with_capacity(token.len());
    clean.push_str(&token[..auth_start]);
    clean.push_str(&authority[at + 1..]);
    clean.push_str(&after[auth_end..]);
    Some(clean)
}

/// URI-scheme syntax (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`).
fn is_scheme_token(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => (),
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

/// Bounded, credential-free echo of rejected user input for error messages:
/// truncated to `MAX_LOG_LINE_CHARS` and stripped of `user:pass@` userinfo.
///
/// Every "bad address" message uses this, so a pasted password can never
/// reach the error view (or "Copy log") even before the display-level
/// sanitizer runs.
pub(crate) fn safe_input_echo(input: &str) -> String {
    strip_credentials_for_log(&truncate_chars(input, crate::config::MAX_LOG_LINE_CHARS))
}

/// `$HOME` (or `$USERPROFILE`), when set and ABSOLUTE.
///
/// A relative value would make every path derived from it — the per-user
/// data directory, `wipe_all_data`'s recursive delete, `~/` expansion —
/// resolve under the current working directory, so it is rejected
/// (fail-closed `None`).
pub(crate) fn home_dir() -> Option<std::path::PathBuf> {
    ["HOME", "USERPROFILE"].iter().find_map(|name| {
        let value = std::env::var_os(name)?;
        let path = std::path::PathBuf::from(value);
        path.is_absolute().then_some(path)
    })
}

/// True when `url` is a page this browser can open and record: an absolute
/// `https` URL or a local `file:` URL without a remote host, within the
/// input cap.
///
/// Shared by the encrypted `history` and `bookmarks` stores (record/add and
/// deserialization alike), so a hand-edited (or corrupted) store cannot
/// smuggle other schemes back into the UI.
pub(crate) fn is_recordable_url(url: &str) -> bool {
    if url.len() > crate::config::MAX_URL_BYTES {
        return false;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => true,
        // Mirror `protocol::local::parse_file_url`: only THIS device, and
        // never with credentials or a port (the reader rejects those at
        // navigation time, so they must never be stored either).
        "file" => {
            crate::protocol::local::is_local_host(&parsed)
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.port().is_none()
        }
        _ => false,
    }
}

/// Normalizes a page URL for the encrypted stores: validates it with
/// [`is_recordable_url`] and removes authority credentials (`user:pass@`)
/// from the URL ITSELF.
///
/// Unlike [`strip_credentials_for_log`] — which rewrites every nested URL
/// inside a log token — this only touches the userinfo of the page being
/// stored: a legitimate page whose query embeds another URL
/// (`?next=https://u:p@evil/`) keeps its exact path/query bytes, so the
/// stored page is the page that was loaded. `None` when the input is not a
/// recordable page (or credentials cannot be removed from it).
pub(crate) fn normalize_stored_url(url: &str) -> Option<String> {
    let url = url.trim();
    if !is_recordable_url(url) {
        return None;
    }
    let mut parsed = url::Url::parse(url).ok()?;
    let has_credentials = !parsed.username().is_empty() || parsed.password().is_some();
    if has_credentials {
        // Fail CLOSED: if the credentials cannot be removed, refuse to store
        // the URL. (`set_username`/`set_password` fail only on
        // cannot-be-a-base URLs, which `is_recordable_url` already rejects,
        // but a future scheme/parser change must never store a secret.)
        parsed.set_username("").ok()?;
        parsed.set_password(None).ok()?;
        let cleaned = parsed.to_string();
        if !is_recordable_url(&cleaned) {
            return None;
        }
        return Some(cleaned);
    }
    Some(url.to_string())
}

/// Current Unix time in seconds (0 on a pre-epoch clock).
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

/// Runs `f`, converting a panic into a bounded `Err` string.
///
/// Background fetch jobs run inside `poll_promise::Promise` threads: if the
/// thread dies from a panic WITHOUT sending a value, polling that promise
/// PANICS the UI thread (documented `poll-promise` behavior). Image
/// decoders and YAML parsing are the only panic-capable code on that path,
/// so the sync entry points (`protocol::client::fetch_blocking`,
/// `media::fetch::*`) wrap the whole job: a decoder/parser panic becomes a
/// normal per-item error instead of crashing the app.
///
/// The panic hook still prints the original message to stderr; the UI only
/// sees the bounded reason.
pub(crate) fn guard_panics<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|payload| panic_reason(payload.as_ref()))
}

/// Bounded `internal error: ...` line for a caught panic payload.
/// Shared by [`guard_panics`] and `protocol::client::run_network` (a panic
/// inside a spawned network task must reach the UI as a normal error line,
/// never as a dropped promise).
pub(crate) fn panic_reason(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    truncate_chars(
        &format!("internal error: background job panicked: {detail}"),
        crate::config::MAX_ERROR_LINE_CHARS,
    )
    .into_owned()
}

/// Unicode FORMAT (Cf) code points used for visual spoofing: zero-width
/// characters, bidi embeddings/overrides and isolates. The standard library
/// has no `is_format` predicate, so this is a conservative explicit list.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'                // soft hyphen
        | '\u{034F}'              // combining grapheme joiner
        | '\u{0600}'..='\u{0605}' // Arabic number signs
        | '\u{061C}'              // Arabic letter mark
        | '\u{06DD}'
        | '\u{070F}'
        | '\u{08E2}'
        | '\u{180B}'..='\u{180E}' // Mongolian FVS + vowel separator
        | '\u{200B}'..='\u{200F}' // zero-width space/joiners + direction marks
        | '\u{202A}'..='\u{202E}' // bidi embeddings/overrides (RLO spoofing)
        | '\u{2060}'..='\u{2065}' // word joiner / invisible operators
        | '\u{2066}'..='\u{2069}' // bidi isolates
        | '\u{FE00}'..='\u{FE0F}' // variation selectors
        | '\u{FEFF}'              // BOM / zero-width no-break space
        | '\u{FFF9}'..='\u{FFFB}' // interlinear annotation
        | '\u{E0020}'..='\u{E007F}' // tags
    )
}

/// Sanitizes a frontmatter title for window/tab display.
///
/// - Trims surrounding whitespace.
/// - Replaces control characters (`\r`, `\n`, `\t`, other `Cc`) with a space
///   so a malicious `title: "a\nb"` cannot inject fake window-manager lines.
/// - Drops Unicode format characters (zero-width/bidi), so a remote title
///   cannot visually spoof the OS window/tab title (`RLO` reversed it).
/// - Collapses consecutive whitespace runs into a single space.
/// - Returns an empty string when nothing printable remains.
/// - Caps the result at `MAX_TITLE_CHARS` (including the ellipsis).
pub fn sanitize_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(crate::config::MAX_TITLE_CHARS));
    let mut last_was_space = true; // trim leading
    for c in raw.chars() {
        if is_format_char(c) {
            continue;
        }
        let mapped = if c.is_control() { ' ' } else { c };
        if mapped.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(mapped);
            last_was_space = false;
        }
    }
    // Trim trailing single space possibly added above.
    if out.ends_with(' ') {
        out.pop();
    }
    // Common case: the sanitized title fits. Return it directly instead of
    // letting `truncate_chars` borrow it and `into_owned()` clone the whole
    // String again.
    if out.chars().count() <= crate::config::MAX_TITLE_CHARS {
        return out;
    }
    truncate_chars(&out, crate::config::MAX_TITLE_CHARS).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        guard_panics, is_recordable_url, normalize_stored_url, now_secs, sanitize_title,
        strip_credentials_for_log, strip_scheme_everywhere, strip_scheme_prefix, truncate_chars,
    };

    #[test]
    fn strips_scheme_case_insensitively() {
        assert_eq!(strip_scheme_prefix("https://ex.com/a"), "ex.com/a");
        assert_eq!(strip_scheme_prefix("HTTPS://ex.com/a"), "ex.com/a");
        assert_eq!(strip_scheme_prefix("http://ex.com/a"), "ex.com/a");
        assert_eq!(
            strip_scheme_prefix("file:///Users/me/a.md"),
            "/Users/me/a.md"
        );
        assert_eq!(strip_scheme_prefix("FILE:///x/a.md"), "/x/a.md");
        assert_eq!(strip_scheme_prefix("ex.com/a"), "ex.com/a");
    }

    #[test]
    fn strips_every_occurrence() {
        assert_eq!(
            strip_scheme_everywhere("access to https://a and HTTPS://b"),
            "access to a and b"
        );
        assert_eq!(
            strip_scheme_everywhere("file:///a/b.md: FILE:///c/d.md"),
            "/a/b.md: /c/d.md"
        );
    }

    /// Only pages this browser can open are recordable: HTTPS, and local
    /// files without a remote host. Shared by the encrypted history and
    /// bookmark stores, so one policy covers both.
    #[test]
    fn recordable_urls_are_https_or_local_files() {
        assert!(is_recordable_url("https://ex.com/a/"));
        assert!(is_recordable_url("file:///tmp/notes.md"));
        assert!(is_recordable_url("file:///C:/notes.md"));
        assert!(!is_recordable_url("http://ex.com/a/"));
        assert!(!is_recordable_url("file://server/share/x.md"));
        // `url` normalizes `file://localhost/...` to the local machine
        // (empty host), exactly like the local-file reader accepts it.
        assert!(is_recordable_url("file://localhost/tmp/x.md"));
        assert!(!is_recordable_url("javascript:alert(1)"));
        assert!(!is_recordable_url(""));
        // The cap is BYTE-based on the serialized URL: a percent-encoded
        // path that outgrew the input's char count (but not its byte bound)
        // is a page this browser can open and must be recordable.
        let encoded = format!("https://ex.com/{}", "%C3%A9".repeat(2_000));
        assert!(
            encoded.chars().count() > crate::config::MAX_URL_INPUT_CHARS,
            "over the char cap"
        );
        assert!(
            encoded.len() <= crate::config::MAX_URL_BYTES,
            "under the byte cap"
        );
        assert!(is_recordable_url(&encoded), "recordable by bytes");
        let over = format!(
            "https://ex.com/{}",
            "a".repeat(crate::config::MAX_URL_BYTES)
        );
        assert!(!is_recordable_url(&over), "over the byte cap");
        assert!(now_secs() > 1_600_000_000, "a real wall clock");
    }

    /// Stored URLs keep their path/query bytes exactly: only authority
    /// credentials are dropped, never a nested URL inside the query (which
    /// `strip_credentials_for_log` used to rewrite, recording a DIFFERENT
    /// page than the one loaded).
    #[test]
    fn stored_urls_lose_only_authority_credentials() {
        let nested = "https://ex.com/?next=https://user:pass@evil/x";
        assert_eq!(
            normalize_stored_url(nested).as_deref(),
            Some(nested),
            "nested URLs in the query are payload, not credentials"
        );
        assert_eq!(
            normalize_stored_url("https://user:pass@ex.com/a/?q=1").as_deref(),
            Some("https://ex.com/a/?q=1")
        );
        assert_eq!(
            normalize_stored_url("https://user@ex.com/a/").as_deref(),
            Some("https://ex.com/a/")
        );
        // URLs without credentials come back unchanged, byte for byte.
        assert_eq!(
            normalize_stored_url("file:///tmp/a%20b.md").as_deref(),
            Some("file:///tmp/a%20b.md")
        );
        // Non-recordable input stays refused.
        assert_eq!(normalize_stored_url("http://ex.com/"), None);
        assert_eq!(normalize_stored_url(""), None);
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        // The cap INCLUDES the ellipsis: "hello world" with 5 -> "he...".
        assert_eq!(truncate_chars("hello world", 5), "he...");
        assert_eq!(truncate_chars("", 5), "");
        assert_eq!(truncate_chars("héllo wörld", 5), "hé...");
        // Pathological caps never exceed the budget.
        assert_eq!(truncate_chars("hello", 2), "..");
        assert_eq!(truncate_chars("hello", 0), "");
        assert_eq!(truncate_chars("hello", 3), "...");
        // Exact boundary: fits untouched.
        assert_eq!(truncate_chars("hello", 5), "hello");
        // A cut never lands inside a grapheme cluster: the ZWJ family (7
        // scalars) does not fit a 2-scalar budget, so it is dropped whole
        // instead of leaving a bare base + dangling joiner.
        assert_eq!(truncate_chars("👨‍👩‍👧‍👦x", 5), "...");
        // A cluster that fits is kept whole and the cut moves back to its end.
        assert_eq!(truncate_chars("👨‍👩‍👧‍👦 x y", 10), "👨‍👩‍👧‍👦...");
        // Combining marks stay attached to their base character.
        assert_eq!(truncate_chars("e\u{301}wxyz", 5), "e\u{301}...");
    }

    #[test]
    fn strips_credentials_from_log_lines() {
        assert_eq!(
            strip_credentials_for_log("access to https://user:pass@ex.com/x network error"),
            "access to https://ex.com/x network error"
        );
        assert_eq!(
            strip_credentials_for_log("access to https://ex.com/x ok"),
            "access to https://ex.com/x ok"
        );
        assert_eq!(
            strip_credentials_for_log("https://u@h/a and https://x:y@z/b"),
            "https://h/a and https://z/b"
        );
        // Non-URL `user@host` text is left alone.
        assert_eq!(
            strip_credentials_for_log("mailto:a@b.com"),
            "mailto:a@b.com"
        );
        // Malformed authority spellings must not leak the password either:
        // one slash, extra slashes, a backslash, protocol-relative.
        assert_eq!(
            strip_credentials_for_log("blocked: http:/user:pass@ex.com/x"),
            "blocked: http:/ex.com/x"
        );
        assert_eq!(
            strip_credentials_for_log("http:///user:pass@ex.com/x"),
            "http:///ex.com/x"
        );
        assert_eq!(
            strip_credentials_for_log(r"http:\user:pass@ex.com\x"),
            r"http:\ex.com\x"
        );
        assert_eq!(
            strip_credentials_for_log("//user:pass@ex.com/x"),
            "//ex.com/x"
        );
        // Punctuation around the URL survives, and IPv6 authority (`:` but
        // no `@`) is untouched.
        assert_eq!(
            strip_credentials_for_log("(https://u:p@h/a), then"),
            "(https://h/a), then"
        );
        assert_eq!(
            strip_credentials_for_log("https://[::1]:8080/x"),
            "https://[::1]:8080/x"
        );
        // Nested URL inside a path/query: every occurrence is stripped.
        assert_eq!(
            strip_credentials_for_log("https://ex.com/?next=http://user:pass@evil/x"),
            "https://ex.com/?next=http://evil/x"
        );
        // The no-separator spelling `scheme:user:pass@host` (blocked as
        // scheme syntax by `parse_https`) must lose the password too.
        assert_eq!(
            strip_credentials_for_log("blocked: http:user:pass@ex.com/x"),
            "blocked: http:ex.com/x"
        );
        assert_eq!(
            strip_credentials_for_log("blocked: mailto:user:pass@b.com"),
            "blocked: mailto:b.com"
        );
        // A single colon stays `host:port` / email local part: nothing to
        // strip without a userinfo password.
        assert_eq!(
            strip_credentials_for_log("nas:5000/x and mailto:a@b.com"),
            "nas:5000/x and mailto:a@b.com"
        );
        assert_eq!(
            strip_credentials_for_log("user:pass@example.com"),
            "user:pass@example.com"
        );
    }

    #[test]
    fn sanitizes_control_chars() {
        assert_eq!(sanitize_title("  Hello\nWorld\r\n  "), "Hello World");
        assert_eq!(sanitize_title("a\t\tb"), "a b");
        assert_eq!(sanitize_title("   "), "");
    }

    #[test]
    fn sanitize_title_drops_format_and_bidi_controls() {
        // RLO reverses the visual order of the rest of the title.
        assert_eq!(sanitize_title("Invoice\u{202E}gnp.exe"), "Invoicegnp.exe");
        // Zero-width characters cannot hide text in the title either.
        assert_eq!(sanitize_title("a\u{200B}b\u{200D}c"), "abc");
        assert_eq!(sanitize_title("\u{FEFF}Hi"), "Hi");
    }

    #[test]
    fn sanitized_title_is_bounded() {
        let big = "a".repeat(crate::config::MAX_TITLE_CHARS + 50);
        let clean = sanitize_title(&big);
        assert!(clean.chars().count() <= crate::config::MAX_TITLE_CHARS);
    }

    #[test]
    fn panic_guard_converts_panics_to_bounded_errors() {
        // Normal results pass through untouched.
        assert_eq!(guard_panics(|| 40 + 2).unwrap(), 42);
        // A panic becomes an `Err` instead of unwinding the caller (which,
        // inside a `poll-promise` thread, would panic the UI on poll).
        let msg = guard_panics(|| -> u8 { panic!("decoder exploded") }).unwrap_err();
        assert!(msg.contains("decoder exploded"), "panic detail kept: {msg}");
        assert!(msg.chars().count() <= crate::config::MAX_ERROR_LINE_CHARS);
        // Non-string panic payloads still yield a message.
        let msg = guard_panics(|| std::panic::panic_any(7u32)).unwrap_err();
        assert!(msg.contains("background job panicked"), "{msg}");
    }
}
