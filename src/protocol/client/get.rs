// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::client::get`: the shared capped streaming GET.
//!
//! `get_bytes` performs one cancellable HTTPS GET (redirect/https check,
//! per-chunk stall timeout, running byte cap, optional `Content-Type` gate
//! before the body is read) and returns a [`GetOutcome`]. Document fetches
//! and image/favicon subresources share it; validation differs per caller.
//!
//! The response headers relevant to the encrypted disk cache
//! (`Cache-Control`, `Expires`, `ETag`, `Last-Modified`, `Date`, `Age`,
//! `Vary`) are captured on every outcome (first value wins, length-capped).
//! A conditional request (`If-None-Match`/`If-Modified-Since`) may answer
//! `304 Not Modified`: no body then, only refreshed headers.

use super::*;
use crate::cache::policy::CapturedHeaders;
use crate::util::truncate_chars;

/// Conditional request validators (from a stale cache record).
#[derive(Debug, Clone, Default)]
pub(crate) struct Conditional {
    /// `ETag` to send as `If-None-Match` (empty = absent).
    pub etag: String,
    /// `Last-Modified` to send as `If-Modified-Since` (empty = absent).
    pub last_modified: String,
}

/// Bundled per-request options for [`get_bytes`] (keeps the shared entry
/// point's arity auditable instead of growing a bare parameter list).
pub(crate) struct GetOptions<'a> {
    /// `Accept` header value.
    pub accept: &'a str,
    /// Anti-abuse cap applied to the decompressed body.
    pub max_bytes: usize,
    /// Total per-request budget.
    pub timeout: std::time::Duration,
    /// Runs on the `Content-Type` values BEFORE the body is streamed.
    pub gate: &'a (dyn Fn(&[String]) -> Result<(), String> + Send + Sync),
    /// Conditional validators (`If-None-Match`/`If-Modified-Since`).
    pub conditional: Option<&'a Conditional>,
    /// Refresh: also sends `Cache-Control: no-cache` on the request.
    pub reload: bool,
}

/// One successful GET: the capped body plus the URL we landed on.
pub(crate) struct RawGet {
    /// Response body (already decompressed by reqwest when the server sent
    /// `Content-Encoding: gzip|br|deflate`). The cap applies to these bytes.
    pub bytes: Vec<u8>,
    /// URL where the request landed after redirects.
    pub final_url: Url,
    /// Cache headers captured from the response.
    pub headers: CapturedHeaders,
}

/// Outcome of one GET: a fresh body, or a `304 Not Modified` (the cached
/// body is still valid; only the headers were refreshed).
pub(crate) enum GetOutcome {
    Fresh(RawGet),
    NotModified { headers: CapturedHeaders },
}

/// Performs one cancellable GET and reads the body with a hard cap.
///
/// Shared by document fetches (`fetch_one`) and image/favicon subresources
/// (`media::fetch`): both need the same HTTPS-only, redirect-checked,
/// stall-bounded, capped streaming read. Validation differs per caller.
///
/// `GetOptions::gate` runs on the response `Content-Type` values BEFORE the
/// body is streamed: a response whose declared type the caller cannot accept
/// is rejected without downloading (and without decompressing) its body. This
/// is the documented anti-abuse contract for both documents and images.
/// The gate is skipped for `304 Not Modified` (no body, no content to gate).
///
/// `GetOptions::conditional` sends `If-None-Match`/`If-Modified-Since` when
/// set: a 304 answer becomes [`GetOutcome::NotModified`] instead of an error.
///
/// `GetOptions::reload` (Refresh) sends `Cache-Control: no-cache` on the
/// request: the honest HTTP reload semantic, so upstream shared caches
/// revalidate too.
///
/// Error strings are part of the error-view contract; do not reword.
pub(crate) async fn get_bytes(
    client: &reqwest::Client,
    url: &Url,
    options: &GetOptions<'_>,
    cancel: &AtomicBool,
) -> Result<GetOutcome, String> {
    // Defense in depth: callers are expected to hand in a credential-free
    // URL (`parse_navigable` strips userinfo), but this shared entry point
    // must never let a secret ride an `Authorization` header to the wire or
    // echo it in an error/log line. Strip once, up front; fail CLOSED when
    // it cannot be removed. The clone only happens when userinfo is present:
    // the common path borrows the caller's URL (no per-request allocation).
    let stripped;
    let url = if !url.username().is_empty() || url.password().is_some() {
        let mut clean = url.clone();
        if clean.set_username("").is_err() || clean.set_password(None).is_err() {
            return Err(format!(
                "access to {url} with credentials (blocked)",
                url = crate::util::strip_credentials_for_log(url.as_str())
            ));
        }
        stripped = clean;
        &stripped
    } else {
        url
    };
    // Cancellable connect+headers: `send()` alone can block up to the
    // timeout even after the tab was abandoned. Race it against the
    // cooperative flag so superseded navigations drop fast.
    // (`futures_util::select` instead of `tokio::select!`: the crate avoids
    // the `macros` feature on purpose, see Cargo.toml.)
    //
    // `timeout` is per-request ON PURPOSE: the global client keeps the
    // short document budget (`REQUEST_TIMEOUT_SECS`), while images get a
    // longer one (`IMAGE_REQUEST_TIMEOUT_SECS`): binary payloads are
    // legitimately larger and used to abort mid-transfer on slow links.
    use futures_util::future::{select, Either};
    let mut request = client
        .get(url.clone())
        .header(config::ACCEPT_HEADER_NAME, options.accept)
        .timeout(options.timeout);
    // Conditional revalidation (encrypted disk cache): the validators are
    // opaque server tokens, echoed back verbatim and length-capped like
    // every other server-controlled value. A reload also asks upstream
    // shared caches to revalidate (`Cache-Control: no-cache` request).
    if options.reload {
        request = request.header(reqwest::header::CACHE_CONTROL, "no-cache");
    }
    if let Some(cond) = options.conditional {
        if !cond.etag.is_empty() {
            request = request.header(
                reqwest::header::IF_NONE_MATCH,
                truncate_chars(&cond.etag, config::MAX_CACHE_HEADER_CHARS).into_owned(),
            );
        }
        if !cond.last_modified.is_empty() {
            request = request.header(
                reqwest::header::IF_MODIFIED_SINCE,
                truncate_chars(&cond.last_modified, config::MAX_CACHE_HEADER_CHARS).into_owned(),
            );
        }
    }
    let send_fut = request.send();
    let cancel_fut = async {
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(config::FETCH_SLOT_POLL_MS)).await;
        }
    };
    futures_util::pin_mut!(send_fut, cancel_fut);
    let resp = match select(send_fut, cancel_fut).await {
        Either::Left((r, _)) => r.map_err(|e| format!("access to {url} network error: {e}"))?,
        Either::Right((_, _)) => return Err(format!("access to {url} cancelled")),
    };

    // Final URL after redirects (reqwest follows up to MAX_REDIRECTS).
    // History/display must use where we landed, not what we asked.
    let mut final_url = resp.url().clone();
    // Strip any userinfo the redirect chain carried: credentials must never
    // reach history, the address bar, or the log, and reqwest must not get a
    // chance to turn them into an Authorization header on a later hop.
    // Fail closed: when the credentials cannot be removed, reject the load.
    if (!final_url.username().is_empty() || final_url.password().is_some())
        && (final_url.set_username("").is_err() || final_url.set_password(None).is_err())
    {
        return Err(format!(
            "access to {url} redirected with credentials (blocked)"
        ));
    }

    // Defense in depth: `https_only(true)` should already reject downgrades,
    // but never read a body that did not land on https.
    if final_url.scheme() != "https" {
        return Err(format!("access to {url} redirected off https (blocked)"));
    }
    // A redirect can land on a URL far longer than the input cap (the
    // `Location` is server-controlled). Everything downstream (display,
    // history, relative resolution) assumes the cap, so reject it here
    // before any body is read. Byte-based: the landed URL is serialized.
    if final_url.as_str().len() > config::MAX_URL_BYTES {
        return Err(format!(
            "access to {url} redirected to an over-long URL (blocked)"
        ));
    }

    let status = resp.status();
    // A conditional answer: the cached body is still valid. No `Content-Type`
    // gate (there is no body to gate) and no body read: only the refreshed
    // headers travel back, so the cache entry can extend its freshness.
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(GetOutcome::NotModified {
            headers: capture_headers(&resp),
        });
    }
    // Only 2xx counts as "exists". 404/500/... move on to the next route.
    if !status.is_success() {
        let code = status.as_u16();
        // 404 gets its own message ("does not exist") for the line-by-line log.
        if code == 404 {
            return Err(format!("access to {url} does not exist (404)"));
        }
        return Err(format!("access to {url} returned HTTP {code}"));
    }

    // All Content-Type values (case-insensitive, whitespace-tolerant).
    // Non-latin1 is `(invalid)`, missing is an empty vec. Values reach the
    // gate UNTRUNCATED: the markdown gate inspects every parameter and a
    // bounded copy could hide a `charset=` beyond the cut (a server padded
    // the header past the old `MAX_LOG_LINE_CHARS` truncation and the
    // non-UTF-8 charset was never seen). The COUNT is capped (hostile
    // servers control how many values are declared) and every error/log
    // line built from these values is bounded by its own formatter.
    // More values than the cap: fail closed instead of inspecting a prefix
    // an attacker chose (a `text/html` past the cut would otherwise pass).
    if resp
        .headers()
        .get_all(reqwest::header::CONTENT_TYPE)
        .iter()
        .count()
        > config::MAX_CONTENT_TYPE_VALUES
    {
        return Err(format!(
            "access to {url} returned too many content-types (blocked)"
        ));
    }
    let content_types: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::CONTENT_TYPE)
        .iter()
        .take(config::MAX_CONTENT_TYPE_VALUES)
        .map(|v| {
            v.to_str()
                .map(str::to_string)
                .unwrap_or_else(|_| "(invalid)".to_string())
        })
        .collect();

    // Caller gate BEFORE the body is read: a concrete non-accepted type is
    // rejected without downloading/decompressing a single byte.
    (options.gate)(&content_types)?;

    // Early cutoff on the declared `Content-Length` (anti-abuse, no download).
    // Not a cache: just avoids fetching giant bodies we would reject anyway.
    // With transparent decompression this is the COMPRESSED length, a lower
    // bound; the running cap below still bounds the real body.
    let content_len_hint = resp.content_length();
    if let Some(len) = content_len_hint {
        if len > options.max_bytes as u64 {
            return Err(format!(
                "access to {url} exceeds {} bytes",
                options.max_bytes
            ));
        }
    }

    // Capture the cache headers BEFORE the body is consumed (the response
    // headers stay available after, but capturing once keeps the two paths
    // identical).
    let headers = capture_headers(&resp);

    // Chunked reads with a running cap: avoids OOM when the server lies
    // about `Content-Length` or streams infinite chunks. Fast and safe.
    use futures_util::StreamExt as _;
    let mut buf: Vec<u8> = Vec::new();
    // Pre-reserve from hint (capped) to avoid reallocs up to the cap.
    if let Some(len) = content_len_hint {
        let hint = (len as usize).min(options.max_bytes);
        // Cap initial reserve to 1MiB to avoid big alloc on lying header
        // (growth continues via extend).
        buf.reserve(hint.min(1024 * 1024));
    }
    let mut stream = resp.bytes_stream();
    loop {
        // Abandoned navigation: stop downloading immediately. The `select`
        // below also races the flag against a stalled chunk wait, so a
        // superseded load never sits behind the full stall timeout.
        if cancel.load(Ordering::Relaxed) {
            return Err(format!("access to {url} cancelled"));
        }
        // Bound each chunk wait: without this a server trickling one byte at
        // a time would defeat the request timeout (which covers the whole
        // response) and hold the thread for the full fallback budget.
        let chunk_fut = tokio::time::timeout(
            std::time::Duration::from_secs(config::CHUNK_TIMEOUT_SECS),
            stream.next(),
        );
        let cancel_fut = async {
            while !cancel.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(config::FETCH_SLOT_POLL_MS))
                    .await;
            }
        };
        futures_util::pin_mut!(chunk_fut, cancel_fut);
        match select(chunk_fut, cancel_fut).await {
            // Superseded while waiting for data: drop promptly.
            Either::Right((_, _)) => return Err(format!("access to {url} cancelled")),
            Either::Left((Err(_), _)) => {
                return Err(format!(
                    "access to {url} stalled (no data for {}s)",
                    config::CHUNK_TIMEOUT_SECS
                ));
            }
            Either::Left((Ok(None), _)) => break,
            Either::Left((Ok(Some(chunk)), _)) => {
                let bytes =
                    chunk.map_err(|e| format!("access to {url} error reading body: {e}"))?;
                // `saturating` in case a giant chunk overflows: still fails by limit.
                if buf.len().saturating_add(bytes.len()) > options.max_bytes {
                    return Err(format!(
                        "access to {url} exceeds {} bytes",
                        options.max_bytes
                    ));
                }
                buf.extend_from_slice(&bytes);
            }
        }
    }

    Ok(GetOutcome::Fresh(RawGet {
        bytes: buf,
        final_url,
        headers,
    }))
}

/// First value of a response header, length-capped (server-controlled).
fn first_capped(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    let value = resp.headers().get(name)?;
    let text = value.to_str().ok()?;
    let capped = truncate_chars(text, config::MAX_CACHE_HEADER_CHARS).into_owned();
    if capped.is_empty() {
        None
    } else {
        Some(capped)
    }
}

/// First few values of a repeated response header, each length-capped and
/// comma-joined (server-controlled; bounded count AND bounded length).
/// `skip_empty` drops empty values (single-value headers keep them so the
/// caller's gate still sees every declared value).
fn capped_values(
    resp: &reqwest::Response,
    name: reqwest::header::HeaderName,
    max_values: usize,
    skip_empty: bool,
) -> Option<String> {
    let mut values: Vec<String> = Vec::new();
    for value in resp.headers().get_all(name).iter().take(max_values) {
        if let Ok(text) = value.to_str() {
            let capped = truncate_chars(text, config::MAX_CACHE_HEADER_CHARS).into_owned();
            if !capped.is_empty() || !skip_empty {
                values.push(capped);
            }
        }
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join(", "))
    }
}

/// Captures the cache-relevant response headers (bounded, first-wins).
fn capture_headers(resp: &reqwest::Response) -> CapturedHeaders {
    let count = |name: reqwest::header::HeaderName| resp.headers().get_all(name).iter().count();
    // A repeated policy header past the capture cap, or a single policy
    // value too long to inspect in full, is a partial view: the hidden
    // bytes could contain `no-store`/`Vary: *`, so the response must not be
    // stored at all.
    let too_many_headers = count(reqwest::header::CACHE_CONTROL) > config::MAX_CACHE_HEADER_VALUES
        || count(reqwest::header::VARY) > config::MAX_CACHE_HEADER_VALUES
        || [
            reqwest::header::CACHE_CONTROL,
            reqwest::header::VARY,
            reqwest::header::EXPIRES,
            reqwest::header::DATE,
            reqwest::header::AGE,
        ]
        .into_iter()
        .any(|name| value_too_long(resp, name));
    let pragma_no_cache = resp
        .headers()
        .get_all(reqwest::header::PRAGMA)
        .iter()
        .take(config::MAX_CACHE_HEADER_VALUES)
        .filter_map(|v| v.to_str().ok())
        .any(|text| {
            text.split(',')
                .any(|d| d.trim().eq_ignore_ascii_case("no-cache"))
        });
    CapturedHeaders {
        // `Cache-Control` may legitimately repeat: join the first few
        // values instead of keeping one.
        cache_control: capped_values(
            resp,
            reqwest::header::CACHE_CONTROL,
            config::MAX_CACHE_HEADER_VALUES,
            true,
        ),
        expires: first_capped(resp, reqwest::header::EXPIRES),
        etag: first_capped(resp, reqwest::header::ETAG),
        last_modified: first_capped(resp, reqwest::header::LAST_MODIFIED),
        date: first_capped(resp, reqwest::header::DATE),
        age: first_capped(resp, reqwest::header::AGE),
        vary: capped_values(
            resp,
            reqwest::header::VARY,
            config::MAX_CACHE_HEADER_VALUES,
            true,
        ),
        // The caller's gate already inspected every declared value: the
        // record keeps them all (even empty ones).
        content_type: capped_values(
            resp,
            reqwest::header::CONTENT_TYPE,
            config::MAX_CONTENT_TYPE_VALUES,
            false,
        ),
        pragma_no_cache,
        too_many_headers,
    }
}

/// `true` when any occurrence of `name` is longer than the capture cap
/// (server-controlled; used to fail closed on a partial policy view).
fn value_too_long(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> bool {
    resp.headers()
        .get_all(name)
        .iter()
        .take(config::MAX_CACHE_HEADER_VALUES)
        .filter_map(|v| v.to_str().ok())
        .any(|text| text.chars().count() > config::MAX_CACHE_HEADER_CHARS)
}
