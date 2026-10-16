// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::client::get`: the shared capped streaming GET.
//!
//! `get_bytes` performs one cancellable HTTPS GET (redirect/https check,
//! per-chunk stall timeout, running byte cap, optional `Content-Type` gate
//! before the body is read) and returns a [`RawGet`]. Document fetches and
//! image/favicon subresources share it; validation differs per caller.

use super::*;

/// One successful GET: the capped body plus the URL we landed on.
pub(crate) struct RawGet {
    /// Response body (already decompressed by reqwest when the server sent
    /// `Content-Encoding: gzip|br|deflate`). The cap applies to these bytes.
    pub bytes: Vec<u8>,
    /// URL where the request landed after redirects.
    pub final_url: Url,
}

/// Performs one cancellable GET and reads the body with a hard cap.
///
/// Shared by document fetches (`fetch_one`) and image/favicon subresources
/// (`media::fetch`): both need the same HTTPS-only, redirect-checked,
/// stall-bounded, capped streaming read. Validation differs per caller.
///
/// `gate` runs on the response `Content-Type` values BEFORE the body is
/// streamed: a response whose declared type the caller cannot accept is
/// rejected without downloading (and without decompressing) its body. This
/// is the documented anti-abuse contract for both documents and images.
///
/// Error strings are part of the error-view contract; do not reword.
pub(crate) async fn get_bytes(
    client: &reqwest::Client,
    url: &Url,
    accept: &str,
    max_bytes: usize,
    timeout: std::time::Duration,
    gate: &(dyn Fn(&[String]) -> Result<(), String> + Send + Sync),
    cancel: &AtomicBool,
) -> Result<RawGet, String> {
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
                "access to {} with credentials (blocked)",
                crate::util::strip_credentials_for_log(url.as_str())
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
    let send_fut = client
        .get(url.clone())
        .header(config::ACCEPT_HEADER_NAME, accept)
        .timeout(timeout)
        .send();
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
    gate(&content_types)?;

    // Early cutoff on the declared `Content-Length` (anti-abuse, no download).
    // Not a cache: just avoids fetching giant bodies we would reject anyway.
    // With transparent decompression this is the COMPRESSED length, a lower
    // bound; the running cap below still bounds the real body.
    let content_len_hint = resp.content_length();
    if let Some(len) = content_len_hint {
        if len > max_bytes as u64 {
            return Err(format!("access to {url} exceeds {max_bytes} bytes"));
        }
    }

    // Chunked reads with a running cap: avoids OOM when the server lies
    // about `Content-Length` or streams infinite chunks. Fast and safe.
    use futures_util::StreamExt as _;
    let mut buf: Vec<u8> = Vec::new();
    // Pre-reserve from hint (capped) to avoid reallocs up to `max_bytes`.
    if let Some(len) = content_len_hint {
        let hint = (len as usize).min(max_bytes);
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
                if buf.len().saturating_add(bytes.len()) > max_bytes {
                    return Err(format!("access to {url} exceeds {max_bytes} bytes"));
                }
                buf.extend_from_slice(&bytes);
            }
        }
    }

    Ok(RawGet {
        bytes: buf,
        final_url,
    })
}
