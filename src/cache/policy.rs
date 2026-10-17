// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cache::policy`: HTTP cache semantics (RFC 9111, private-cache subset).
//!
//! - [`CapturedHeaders`]: the response headers the network layer keeps per
//!   response (`Cache-Control`, `Expires`, `ETag`, `Last-Modified`, `Date`,
//!   `Age`, `Vary`). Everything else stays ignored, as before. A capture
//!   that exceeds its bounds (`too_many_headers`) is treated as
//!   non-storable: a partial policy view could hide `no-store`/`Vary: *`.
//! - [`Directives`]: parsed `Cache-Control` (`no-store`, `no-cache`,
//!   `max-age`, `must-revalidate`, `immutable`; `s-maxage` is a shared-cache
//!   directive and is ignored, like `stale-while-revalidate`/`stale-if-error`
//!   in v1: expired + unreachable means fail, never serve stale).
//! - [`freshness`]: computes `fresh_until` from the headers with the app's
//!   default TTL and the absolute 1-year ceiling (`MAX_CACHE_FRESHNESS_SECS`).
//! - [`doc_key`]: the canonical document identity — scheme + lowercase host
//!   + port + `logical_base` + query — so every spelling of one logical page
//!     maps to the SAME cache record (`/love`, `/love/`, `/love.md`,
//!     `/love/index.md`, `/love/README.md`, plus query-preserving variants).
//!     The fragment never participates (stripped before the network).

use crate::config;
use crate::protocol::resolver;
use url::Url;

/// Response headers kept for cache decisions (first value wins; values are
/// length-capped at capture time by the network layer).
#[derive(Debug, Clone, Default)]
pub(crate) struct CapturedHeaders {
    /// All `Cache-Control` values, comma-joined at capture.
    pub cache_control: Option<String>,
    /// `Expires` value.
    pub expires: Option<String>,
    /// `ETag` value (opaque, passed back verbatim on revalidation).
    pub etag: Option<String>,
    /// `Last-Modified` value (passed back verbatim on revalidation).
    pub last_modified: Option<String>,
    /// `Date` value (origin timestamp for age computation).
    pub date: Option<String>,
    /// `Age` value (seconds the response already spent in upstream caches).
    pub age: Option<String>,
    /// `Vary` value.
    pub vary: Option<String>,
    /// The `Content-Type` values the gate already validated (kept for the
    /// record, so a revalidation mismatch can be detected).
    pub content_type: Option<String>,
    /// Response carried `Pragma: no-cache` (HTTP/1.0 backwards compat).
    pub pragma_no_cache: bool,
    /// A cache-relevant repeated header exceeded the capture cap. The
    /// missing values could change the policy, so the response is treated
    /// as non-storable (fail closed, never cache on a partial view).
    pub too_many_headers: bool,
}

/// Parsed `Cache-Control` request/response directives (the subset MDowser
/// honors as a private cache).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Directives {
    /// `no-store`: never persist (not even the index entry).
    pub no_store: bool,
    /// `no-cache` (with or without field names): always revalidate.
    pub no_cache: bool,
    /// `max-age=N` (seconds). Invalid values are ignored (directive absent).
    pub max_age: Option<u64>,
    /// `must-revalidate` (or `proxy-revalidate`): expired means revalidate
    /// or fail, never serve stale.
    pub must_revalidate: bool,
    /// `immutable`: honored inside the absolute ceiling, never "forever".
    pub immutable: bool,
}

/// Parses one `Cache-Control` header value (or a comma-joined set) into
/// [`Directives`]. Case-insensitive, whitespace-tolerant, quote-tolerant;
/// unknown directives are ignored. Never panics on hostile input.
pub(crate) fn parse_cache_control(value: Option<&str>) -> Directives {
    let mut out = Directives::default();
    let Some(raw) = value else {
        return out;
    };
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, arg) = match part.split_once('=') {
            Some((n, a)) => (n.trim(), Some(a.trim())),
            None => (part, None),
        };
        // Strip one layer of DQUOTEs (`max-age="60"` is legal).
        let arg = arg.map(|a| a.trim_matches('"'));
        if name.eq_ignore_ascii_case("no-store") {
            out.no_store = true;
        } else if name.eq_ignore_ascii_case("no-cache") {
            out.no_cache = true;
        } else if name.eq_ignore_ascii_case("max-age") {
            if let Some(a) = arg {
                // `saturating` on huge values: a `max-age=99999999999999999999`
                // must not wrap into "fresh for 3 seconds".
                if let Ok(secs) = a.parse::<u64>() {
                    out.max_age = Some(secs);
                } else if let Ok(secs) = a.parse::<u128>() {
                    out.max_age = Some(secs.min(u64::MAX as u128) as u64);
                }
            }
        } else if name.eq_ignore_ascii_case("must-revalidate")
            || name.eq_ignore_ascii_case("proxy-revalidate")
        {
            out.must_revalidate = true;
        } else if name.eq_ignore_ascii_case("immutable") {
            out.immutable = true;
        }
        // `s-maxage`, `public`, `private`, `stale-*`, `min-fresh`,
        // `max-stale`, `only-if-cached`, `no-transform`: deliberately
        // unhandled in v1 (see the module docs).
    }
    out
}

/// `true` when the `Vary` value forbids caching: `*`, or any field other
/// than `Accept`/`Accept-Encoding` (our requests only vary on those: a fixed
/// `Accept` plus reqwest's transparent `Accept-Encoding`).
pub(crate) fn vary_forbids_cache(vary: Option<&str>) -> bool {
    let Some(raw) = vary else {
        return false;
    };
    for part in raw.split(',') {
        let field = part.trim();
        if field == "*" {
            return true;
        }
        if !(field.eq_ignore_ascii_case("accept") || field.eq_ignore_ascii_case("accept-encoding"))
        {
            return true;
        }
    }
    false
}

/// Parses an HTTP date (IMF-fixdate, RFC 850, asctime) into Unix seconds.
/// Returns `None` on any malformed input (never panics).
///
/// Only the fixed-format shapes are accepted. Each shape is tried in turn:
/// a weekday-shaped input that is not a valid IMF date falls through to the
/// RFC 850 and asctime shapes instead of failing early. The weekday itself
/// is parsed but not cross-checked against the date (a wrong weekday with a
/// right date still yields the right instant).
pub(crate) fn parse_http_date(value: &str) -> Option<u64> {
    parse_imf_fixdate(value.trim())
        .or_else(|| parse_rfc850(value.trim()))
        .or_else(|| parse_asctime(value.trim()))
}

/// IMF-fixdate: `Sun, 06 Nov 1994 08:49:37 GMT`.
fn parse_imf_fixdate(value: &str) -> Option<u64> {
    let rest = value.strip_suffix(" GMT")?;
    let mut parts = rest.split(' ');
    let _weekday = parts.next()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year: i32 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let (hh, mm, ss) = parse_time(time)?;
    ymd_hms_to_unix(sane_year(year)?, month, day, hh, mm, ss)
}

/// Sane-year gate for parsed HTTP dates: server-controlled input must never
/// reach the calendar math outside a representable range (an extreme `i32`
/// from `Expires: ..., 06-Nov-2147483647 ...` would overflow the `i32` year
/// adjustments under `overflow-checks`). Dates only ever feed freshness
/// comparisons against `now`, so years far outside the HTTP era are dropped.
fn sane_year(year: i32) -> Option<i32> {
    (1900..=9999).contains(&year).then_some(year)
}

/// RFC 850: `Sunday, 06-Nov-94 08:49:37 GMT`.
fn parse_rfc850(value: &str) -> Option<u64> {
    let rest = value.strip_suffix(" GMT")?;
    let (_weekday, rest) = rest.split_once(',')?;
    let rest = rest.trim();
    let (date_part, time) = rest.rsplit_once(' ')?;
    let mut dparts = date_part.split('-');
    let day: u32 = dparts.next()?.parse().ok()?;
    let month = month_number(dparts.next()?)?;
    let year2: i32 = dparts.next()?.parse().ok()?;
    if dparts.next().is_some() || !(0..=99).contains(&year2) {
        return None;
    }
    // Two-digit years: 00-49 map to 2000s, 50-99 to 1900s. Either way the
    // result only feeds freshness math (clamped by the absolute ceiling), so
    // a wrong century just expires or freshens conservatively.
    let year = if year2 < 50 {
        2000 + year2
    } else {
        1900 + year2
    };
    sane_year(year)?;
    let (hh, mm, ss) = parse_time(time)?;
    ymd_hms_to_unix(year, month, day, hh, mm, ss)
}

/// asctime: `Sun Nov  6 08:49:37 1994` (double space before day < 10).
fn parse_asctime(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?;
    let month = month_number(parts.next()?)?;
    let day: u32 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let (hh, mm, ss) = parse_time(time)?;
    ymd_hms_to_unix(sane_year(year)?, month, day, hh, mm, ss)
}

fn month_number(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn parse_time(time: &str) -> Option<(u32, u32, u32)> {
    let mut parts = time.split(':');
    let hh: u32 = parts.next()?.parse().ok()?;
    let mm: u32 = parts.next()?.parse().ok()?;
    let ss: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    Some((hh, mm, ss))
}

/// Days-from-civil (Howard Hinnant's algorithm): proleptic Gregorian days
/// since 1970-01-01. Pure integer math, no timezone tables.
fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    // `i64` before the January/February adjustment: a caller-supplied year
    // could otherwise make `year - 1` itself overflow (see `sane_year`,
    // which every parser applies first; this stays defensive).
    let y = if month <= 2 {
        i64::from(year) - 1
    } else {
        i64::from(year)
    };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month as i64 + 9).rem_euclid(12);
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

fn ymd_hms_to_unix(year: i32, month: u32, day: u32, hh: u32, mm: u32, ss: u32) -> Option<u64> {
    let days = days_from_civil(year, month, day)?;
    let secs = days
        .checked_mul(86_400)?
        .checked_add(hh as i64 * 3600 + mm as i64 * 60 + ss as i64)?;
    u64::try_from(secs).ok()
}

/// Freshness outcome for a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Freshness {
    /// Unix seconds until which the entry may be served without revalidation.
    pub fresh_until: u64,
    /// Whether the response carried an explicit validator for later
    /// conditional requests (`ETag` or `Last-Modified` present).
    pub has_validator: bool,
}

/// Computes `fresh_until` for a response received at `now`.
///
/// Precedence: `max-age` > `Expires` (+`Date`) > caller default TTL; the
/// result is clamped to `now + MAX_CACHE_FRESHNESS_SECS` (absolute ceiling).
/// The response's age (RFC 9111 §4.2.3) is the larger of the `Age` header
/// and the apparent age `now - Date`: a response that spent 50s in a CDN
/// with `max-age=60` is fresh for 10s here, not 60, and one generated hours
/// ago with a short `max-age` is already stale. A future `Date` (clock
/// skew) contributes no age.
pub(crate) fn freshness(headers: &CapturedHeaders, now: u64, default_ttl: u64) -> Freshness {
    let directives = parse_cache_control(headers.cache_control.as_deref());
    let age_secs = headers
        .age
        .as_deref()
        .and_then(|a| a.trim().parse::<u64>().ok())
        .unwrap_or(0)
        .min(3600 * 24 * 365 * 10); // hostile `Age: 999...` must not underflow below
    let apparent_age = headers
        .date
        .as_deref()
        .and_then(parse_http_date)
        .map_or(0, |date| now.saturating_sub(date));
    let base_age = now.saturating_sub(age_secs.max(apparent_age));
    let lifetime: u64 = if let Some(max_age) = directives.max_age {
        max_age
    } else if let Some(expires) = headers.expires.as_deref().and_then(parse_http_date) {
        // `Expires - Date` when both exist (clock-skew safe); else `Expires - now`.
        match headers.date.as_deref().and_then(parse_http_date) {
            Some(date) => expires.saturating_sub(date),
            None => expires.saturating_sub(now),
        }
    } else {
        default_ttl
    };
    // `no-cache` forces immediate revalidation: fresh for 0 seconds.
    let lifetime = if directives.no_cache { 0 } else { lifetime };
    let ceiling = config::MAX_CACHE_FRESHNESS_SECS;
    let fresh_for = lifetime.min(ceiling);
    Freshness {
        fresh_until: base_age.saturating_add(fresh_for),
        has_validator: headers.etag.is_some() || headers.last_modified.is_some(),
    }
}

/// Freshness for a `304 Not Modified` answer.
///
/// RFC 9111 says the stored headers are updated from the 304, but a server
/// may omit `Cache-Control`/`Expires` on the 304 (the stored policy still
/// applies) and commonly does. Reusing the entry's previous lifetime keeps
/// `no-cache`/short-TTL responses honest instead of silently extending them
/// to the class default; an explicit directive on the 304 (including
/// `no-cache`) wins.
pub(crate) fn refresh_freshness(
    headers: &CapturedHeaders,
    stored_at: u64,
    fresh_until: u64,
    now: u64,
    default_ttl: u64,
) -> u64 {
    let directives = parse_cache_control(headers.cache_control.as_deref());
    // Defensive: a caller that forgot `must_not_store` must never extend a
    // `no-store` answer.
    if directives.no_store {
        return now;
    }
    if headers.expires.is_none() && directives.max_age.is_none() && !directives.no_cache {
        let lifetime = fresh_until
            .saturating_sub(stored_at)
            .min(config::MAX_CACHE_FRESHNESS_SECS);
        now.saturating_add(lifetime)
    } else {
        freshness(headers, now, default_ttl).fresh_until
    }
}

/// `true` when the response must not be stored at all: `no-store`, a
/// `Vary` we cannot satisfy, a `Pragma: no-cache` on the response, or a
/// header set whose capture was truncated (a partial view cannot prove the
/// response is storable).
pub(crate) fn must_not_store(headers: &CapturedHeaders, pragma_no_cache: bool) -> bool {
    let directives = parse_cache_control(headers.cache_control.as_deref());
    directives.no_store
        || headers.too_many_headers
        || vary_forbids_cache(headers.vary.as_deref())
        || pragma_no_cache
}

/// Canonical document cache key: `scheme://lower-host[:port]<logical_base>[?query]`.
///
/// - Lowercased host (DNS is case-insensitive; `Url` keeps the input case).
/// - `logical_base` collapses ALL route spellings (`/love`, `/love/`,
///   `/love.md`, `/love/index.md`, `/love/README.md`, legacy `.markdown`)
///   to one identity, so one record matches every canonical URL of the page.
/// - Query preserved (it addresses content); fragment never (stripped before
///   the network). Default ports vanish (`Url::port` returns `None` for
///   `:443`), so `:443` spellings alias correctly.
/// - `None` for non-`https` URLs (local files and internal pages are never
///   cached).
pub(crate) fn doc_key(url: &Url) -> Option<String> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let mut key = String::with_capacity(url.as_str().len());
    key.push_str("https://");
    key.push_str(&host);
    if let Some(port) = url.port() {
        key.push(':');
        key.push_str(&port.to_string());
    }
    key.push_str(&resolver::logical_base(url.path()));
    if let Some(query) = url.query() {
        key.push('?');
        key.push_str(query);
    }
    Some(key)
}

/// Host-directory key for the per-domain document file: lowercase host
/// (+ non-default port). One encrypted file per domain.
pub(crate) fn doc_host_key(url: &Url) -> Option<String> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    match url.port() {
        Some(port) => Some(format!("{host}:{port}")),
        None => Some(host),
    }
}

/// Normalizes a URL for use as an image/favicon cache key: the serialized
/// form with the fragment stripped (fragments never reach the network).
/// Credentials are stripped defensively (callers already do, but the key
/// must never retain them).
pub(crate) fn resource_key(url: &Url) -> Option<String> {
    if url.scheme() != "https" {
        return None;
    }
    let mut clean = url.clone();
    let _ = clean.set_username("");
    let _ = clean.set_password(None);
    clean.set_fragment(None);
    Some(clean.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn directives_matrix() {
        let d = parse_cache_control(Some("no-store"));
        assert!(d.no_store);
        let d = parse_cache_control(Some("no-cache"));
        assert!(d.no_cache);
        let d = parse_cache_control(Some("max-age=60, must-revalidate"));
        assert_eq!(d.max_age, Some(60));
        assert!(d.must_revalidate);
        let d = parse_cache_control(Some("public, max-age=\"120\", immutable"));
        assert_eq!(d.max_age, Some(120));
        assert!(d.immutable);
        // Invalid max-age is ignored, not fatal.
        let d = parse_cache_control(Some("max-age=bogus"));
        assert_eq!(d.max_age, None);
        assert!(!d.no_store);
        // Huge max-age saturates instead of wrapping.
        let d = parse_cache_control(Some("max-age=99999999999999999999999"));
        assert_eq!(d.max_age, Some(u64::MAX));
        // s-maxage is a shared-cache directive: ignored here.
        let d = parse_cache_control(Some("s-maxage=10"));
        assert_eq!(d.max_age, None);
        // Case + whitespace tolerance.
        let d = parse_cache_control(Some("  No-Store , Max-Age = 7 "));
        assert!(d.no_store);
        assert_eq!(d.max_age, Some(7));
        assert_eq!(parse_cache_control(None), Directives::default());
    }

    #[test]
    fn vary_rules() {
        assert!(!vary_forbids_cache(None));
        assert!(!vary_forbids_cache(Some("Accept")));
        assert!(!vary_forbids_cache(Some("accept-encoding, Accept")));
        assert!(vary_forbids_cache(Some("*")));
        assert!(vary_forbids_cache(Some("Cookie")));
        assert!(vary_forbids_cache(Some("Accept, Cookie")));
    }

    #[test]
    fn http_dates_parse() {
        // IMF-fixdate.
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        // RFC 850.
        assert_eq!(
            parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"),
            Some(784_111_777)
        );
        // asctime (double space before single-digit day).
        assert_eq!(
            parse_http_date("Sun Nov  6 08:49:37 1994"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("garbage"), None);
        assert_eq!(parse_http_date("Sun, 99 Foo 1994 08:49:37 GMT"), None);
        // Extreme years (hostile `Expires`) are rejected, never fed to the
        // calendar math that would overflow `i32` under `overflow-checks`.
        assert_eq!(parse_http_date("Sun, 06 Nov 2147483647 08:49:37 GMT"), None);
        assert_eq!(
            parse_http_date("Sun, 06 Nov -2147483648 08:49:37 GMT"),
            None
        );
        assert_eq!(
            parse_http_date("Sunday, 06-Nov-2147483647 08:49:37 GMT"),
            None
        );
        assert_eq!(parse_http_date("Sunday, 06-Nov--1 08:49:37 GMT"), None);
        assert_eq!(parse_http_date("Sun Nov  6 08:49:37 2147483647"), None);
        // Out-of-era but representable years are still rejected.
        assert_eq!(parse_http_date("Sun, 06 Nov 10000 08:49:37 GMT"), None);
    }

    #[test]
    fn freshness_precedence_and_ceiling() {
        let now = 1_700_000_000;
        // max-age wins over Expires.
        let h = CapturedHeaders {
            cache_control: Some("max-age=60".into()),
            expires: Some("Sun, 06 Nov 2034 08:49:37 GMT".into()),
            ..Default::default()
        };
        assert_eq!(
            freshness(&h, now, 300),
            Freshness {
                fresh_until: now + 60,
                has_validator: false
            }
        );
        // Expires without max-age: lifetime is `Expires - Date` and the
        // apparent age (`now - Date`) is subtracted, so the entry is fresh
        // until `Expires` exactly — a long-past `Date` is already stale and
        // can never be resurrected for a lifetime from now.
        let h = CapturedHeaders {
            expires: Some("Sun, 06 Nov 1994 09:49:37 GMT".into()),
            date: Some("Sun, 06 Nov 1994 08:49:37 GMT".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, 784_111_777 + 3600);
        // A recent Date counts as age for `max-age` too (RFC 9111 §4.2.3).
        let h = CapturedHeaders {
            cache_control: Some("max-age=3600".into()),
            date: Some("Tue, 14 Nov 2023 22:12:30 GMT".into()), // now - 50
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now + 3600 - 50);
        // The larger of `Age` and the apparent age wins.
        let h = CapturedHeaders {
            cache_control: Some("max-age=3600".into()),
            age: Some("900".into()),
            date: Some("Tue, 14 Nov 2023 22:12:30 GMT".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now + 3600 - 900);
        // Default TTL when silent.
        let h = CapturedHeaders::default();
        assert_eq!(freshness(&h, now, 300).fresh_until, now + 300);
        // no-cache forces revalidation.
        let h = CapturedHeaders {
            cache_control: Some("no-cache".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now);
        // Absolute ceiling clamps even huge max-age.
        let h = CapturedHeaders {
            cache_control: Some(format!("max-age={}", u64::MAX)),
            ..Default::default()
        };
        assert_eq!(
            freshness(&h, now, 300).fresh_until,
            now + config::MAX_CACHE_FRESHNESS_SECS
        );
        // Validator detection.
        let h = CapturedHeaders {
            etag: Some("\"abc\"".into()),
            ..Default::default()
        };
        assert!(freshness(&h, now, 300).has_validator);
    }

    #[test]
    fn doc_key_matches_every_canonical_spelling() {
        let variants = [
            "https://ex.com/love",
            "https://ex.com/love/",
            "https://ex.com/love.md",
            "https://ex.com/love/index.md",
            "https://ex.com/love/README.md",
            "https://ex.com/love/readme.markdown",
            "https://EX.com:443/love.md",
        ];
        let first = doc_key(&url(variants[0])).unwrap();
        for v in variants {
            assert_eq!(doc_key(&url(v)).unwrap(), first, "{v}");
        }
        // Query distinguishes; fragment does not.
        assert_ne!(
            doc_key(&url("https://ex.com/love?q=1")).unwrap(),
            doc_key(&url("https://ex.com/love?q=2")).unwrap()
        );
        assert_eq!(
            doc_key(&url("https://ex.com/love.md#sec")).unwrap(),
            doc_key(&url("https://ex.com/love")).unwrap()
        );
        // Different pages, different keys.
        assert_ne!(
            doc_key(&url("https://ex.com/love")).unwrap(),
            doc_key(&url("https://ex.com/other")).unwrap()
        );
        // Non-https never cached.
        assert!(doc_key(&url("file:///a/b.md")).is_none());
        // A path ENDING in `/` names a directory (resolver contract): the
        // file spelling and the directory spelling are DIFFERENT logical
        // pages, each stable across its own spellings.
        assert_ne!(
            doc_key(&url("https://ex.com/docs.md")).unwrap(),
            doc_key(&url("https://ex.com/docs.md/")).unwrap()
        );
        assert_eq!(
            doc_key(&url("https://ex.com/docs.md/")).unwrap(),
            doc_key(&url("https://ex.com/docs.md/#frag")).unwrap()
        );
    }

    #[test]
    fn freshness_edge_cases() {
        let now = 1_700_000_000;
        // `max-age=0` is immediately stale (revalidate every time).
        let h = CapturedHeaders {
            cache_control: Some("max-age=0".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now);
        // Garbage `Expires` falls back to the default TTL (never "fresh
        // forever" on a misunderstood header).
        let h = CapturedHeaders {
            expires: Some("not a date".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now + 300);
        // A past `Expires` without `max-age` is already stale: its
        // `fresh_until` lands on `Expires` itself (date + zero lifetime),
        // decades before `now`.
        let h = CapturedHeaders {
            expires: Some("Sun, 06 Nov 1994 08:49:37 GMT".into()),
            date: Some("Sun, 06 Nov 1994 08:49:37 GMT".into()),
            ..Default::default()
        };
        let past = freshness(&h, now, 300).fresh_until;
        assert_eq!(past, 784_111_777);
        assert!(past < now, "a past Expires can never be fresh");
        // A hostile `Age` saturates at the clamp instead of underflowing:
        // unparseable values are ignored entirely; huge but parseable ones
        // land at `now - clamp + lifetime`, i.e. simply stale.
        let h = CapturedHeaders {
            cache_control: Some("max-age=60".into()),
            age: Some("99999999999".into()),
            ..Default::default()
        };
        assert_eq!(
            freshness(&h, now, 300).fresh_until,
            now - 3600 * 24 * 365 * 10 + 60
        );
        // `Age` within range is subtracted: 50s in a CDN + max-age=60.
        let h = CapturedHeaders {
            cache_control: Some("max-age=60".into()),
            age: Some("50".into()),
            ..Default::default()
        };
        assert_eq!(freshness(&h, now, 300).fresh_until, now + 10);
    }

    #[test]
    fn must_not_store_rules() {
        let no_store = CapturedHeaders {
            cache_control: Some("no-store".into()),
            ..Default::default()
        };
        assert!(must_not_store(&no_store, false));
        let vary_cookie = CapturedHeaders {
            vary: Some("Cookie".into()),
            ..Default::default()
        };
        assert!(must_not_store(&vary_cookie, false));
        let pragma = CapturedHeaders::default();
        assert!(must_not_store(&pragma, true));
        let plain = CapturedHeaders {
            cache_control: Some("max-age=60".into()),
            ..Default::default()
        };
        assert!(!must_not_store(&plain, false));
        // A truncated cache-header capture is fail-closed: the missing
        // values could have forbidden storage.
        let truncated = CapturedHeaders {
            too_many_headers: true,
            ..Default::default()
        };
        assert!(must_not_store(&truncated, false));
    }

    #[test]
    fn not_modified_reuses_the_stored_lifetime_unless_overridden() {
        let now = 1_700_000_000;
        // A 304 without policy headers keeps the ORIGINAL lifetime: a
        // `no-cache` entry must not become 5-minute-fresh after validating.
        let empty = CapturedHeaders::default();
        assert_eq!(refresh_freshness(&empty, now, now, now, 300), now);
        assert_eq!(
            refresh_freshness(&empty, now - 3600, now - 60, now, 300),
            now + 3540
        );
        // An explicit 304 directive wins.
        let max_age = CapturedHeaders {
            cache_control: Some("max-age=120".into()),
            ..Default::default()
        };
        assert_eq!(
            refresh_freshness(&max_age, now - 3600, now - 60, now, 300),
            now + 120
        );
        let no_cache = CapturedHeaders {
            cache_control: Some("no-cache".into()),
            ..Default::default()
        };
        assert_eq!(
            refresh_freshness(&no_cache, now - 3600, now - 60, now, 300),
            now
        );
        // A `no-store` answer never extends anything (defensive).
        let no_store = CapturedHeaders {
            cache_control: Some("no-store".into()),
            ..Default::default()
        };
        assert_eq!(
            refresh_freshness(&no_store, now - 3600, now + 3600, now, 300),
            now
        );
    }

    #[test]
    fn resource_key_strips_fragment_and_credentials() {
        let key = resource_key(&url("https://user:pass@ex.com/i.png#frag")).unwrap();
        assert!(!key.contains("user"));
        assert!(!key.contains("frag"));
        assert!(resource_key(&url("file:///a.png")).is_none());
    }
}
