// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::client::fetch`: the document route cascade.
//!
//! `fetch_with_fallback` tries every resolver candidate (2xx + markdown
//! Content-Type + UTF-8 + YAML), `fetch_one` runs one attempt, and
//! `finish_document` turns a body into the renderable [`Fetched`] shared by
//! the HTTPS cascade and `protocol::local`. `fetch_blocking` is the
//! background-thread entry point (panic-guarded).

use super::*;

/// Downloads ONE document candidate and validates it.
/// Returns the body text + final URL, or a failure line.
/// `cancel` is checked before each chunk and after every stall timeout so an
/// abandoned thread stops promptly instead of downloading up to 10 MiB for
/// nothing. Each `bytes_stream().next()` is additionally bounded by
/// `CHUNK_TIMEOUT_SECS`: a trickling server cannot pin the background thread
/// past the visual timeout (single-flight per tab still holds: the old thread
/// exits on timeout/cancel instead of lingering).
pub(super) async fn fetch_one(
    client: &reqwest::Client,
    url: &Url,
    cancel: &AtomicBool,
) -> Result<(String, Url), String> {
    // Header gate: only a declared markdown type is downloaded at
    // all. EVERY declared value must be a markdown type: a server naming
    // `text/html` alongside `text/markdown` gets no precedence game (the
    // same strict rule the image gate uses). A missing header stays
    // rejected. The error wording matches `BrowserError::WrongContentType`
    // (the error-view contract).
    let gate = |cts: &[String]| -> Result<(), String> {
        if !cts.is_empty() && cts.iter().all(|s| is_markdown_content_type(Some(s))) {
            return Ok(());
        }
        let found = if cts.is_empty() {
            "(missing)".to_string()
        } else {
            // Servers control header values: bound the joined log line.
            crate::util::truncate_chars(&cts.join(", "), config::MAX_LOG_LINE_CHARS).into_owned()
        };
        Err(BrowserError::WrongContentType {
            url: url.to_string(),
            found,
        }
        .to_string())
    };
    let raw = get_bytes(
        client,
        url,
        config::ACCEPT_HEADER_VALUE,
        config::MAX_BODY_BYTES,
        std::time::Duration::from_secs(config::REQUEST_TIMEOUT_SECS),
        &gate,
        cancel,
    )
    .await?;

    // UTF-8 is mandatory. `String::from_utf8` takes the owned body: no
    // second full-body copy (the bytes are dead either way).
    let text = String::from_utf8(raw.bytes).map_err(|e| {
        BrowserError::InvalidUtf8 {
            url: url.to_string(),
            detail: e.utf8_error().to_string(),
        }
        .to_string()
    })?;

    Ok((text, raw.final_url))
}

/// Turns one successfully obtained body into a renderable [`Fetched`].
///
/// Shared by the HTTPS cascade and `protocol::local`, so a local document
/// and a remote one produce identical parsed events, copy text, anchors and
/// normalized display URL. `error_url` names the attempt in the YAML error
/// line (the loaded URL may differ after a redirect).
pub(crate) fn finish_document(text: &str, landed: Url, error_url: &Url) -> Result<Fetched, String> {
    let (meta, body) = markdown::frontmatter::split(text).map_err(|e| {
        // `serde_norway` details can echo input snippets: callers bound the
        // line like every other document-influenced log entry.
        BrowserError::BadFrontmatter {
            url: error_url.to_string(),
            detail: e,
        }
        .to_string()
    })?;
    let normalized = crate::markdown::document::normalize_body(&body);
    // Event-stream bomb guard: an over-complex document fails this route
    // with a bounded line instead of building a several-hundred-MB vector.
    let events = crate::markdown::document::parse_events(&normalized)?;
    // Copy text is derived here too: off the UI thread, once per document,
    // from the SAME owned events the viewer renders (no second parse).
    let plain_text = crate::markdown::document::plain_text_from_events(&events);
    // First-`#`-heading title fallback: also from the same events, so a
    // document without a frontmatter `title` still names its tab/window
    // (computed unconditionally: an EMPTY frontmatter title falls through
    // to it in `Fetched::page_title`).
    let h1_title = crate::markdown::document::first_h1_title(&events);
    // Anchor targets for in-document navigation, collected from the same
    // owned events (bounded).
    let anchors = crate::markdown::anchors::build(&events);
    // Display URL (what the omnibox shows and what relative links resolve
    // against) is derived once here.
    let display_url = resolver::normalized_display(&landed);
    // Link/image destinations resolved once here: the renderer runs on the
    // UI thread every frame and must not re-parse them (see `DestCache`).
    let dests = crate::protocol::rewrite::DestCache::build(&display_url, &anchors, &events);
    Ok(Fetched {
        display_url,
        meta,
        h1_title,
        plain_text,
        events,
        anchors,
        dests,
    })
}

/// Resolves an already-parsed URL by trying candidates in order.
///
/// - Returns `Fetched` with the first valid candidate.
/// - When none qualifies, returns `BrowserError::NoCandidateWorked` with lines.
/// - `cancel`: checked before each candidate; abandoned loads stop early
///   instead of walking all `MAX_ROUTE_CANDIDATES` routes in background.
pub async fn fetch_with_fallback(
    start: &Url,
    cancel: &AtomicBool,
) -> Result<Fetched, BrowserError> {
    // Reuse the global client (TLS/HTTP2 pool). `clone()` is cheap.
    // `https_only` already comes enabled alongside `parse_https`.
    // `?` propagates the error panic-free (see `global_client`).
    let client = global_client()?.clone();

    let candidates = resolver::candidates_for(start);
    let mut lines: Vec<String> = Vec::new();

    for cand in candidates {
        // Cooperative cancellation: a superseded navigation stops here.
        if cancel.load(Ordering::Relaxed) {
            return Err(BrowserError::Cancelled);
        }
        match fetch_one(&client, &cand, cancel).await {
            Ok((text, final_url)) => {
                // Superseded while the body was downloading: abandon before
                // the parse/plain-text/anchor work on a document nobody will
                // render.
                if cancel.load(Ordering::Relaxed) {
                    return Err(BrowserError::Cancelled);
                }
                // Use where we landed after redirects, not what we asked.
                // If redirect left https (shouldn't: `https_only(true)`
                // rejects downgrades), keep cand. The downgrade never
                // reaches the network as markdown: `fetch_one`
                // validates scheme via the final URL check.
                let landed = if final_url.scheme() == "https" {
                    final_url
                } else {
                    cand.clone()
                };
                // Validate YAML/parse BEFORE declaring success: when
                // malformed, try the next route (requirement).
                match finish_document(&text, landed, &cand) {
                    Ok(fetched) => return Ok(fetched),
                    Err(line) => {
                        lines.push(
                            crate::util::truncate_chars(&line, config::MAX_ERROR_LINE_CHARS)
                                .into_owned(),
                        );
                        continue;
                    }
                }
            }
            Err(line) => {
                // A cancel racing the chunk loop surfaces as a "cancelled"
                // line: map it to the typed variant instead of polluting
                // the route log and walking the remaining candidates.
                if cancel.load(Ordering::Relaxed) {
                    return Err(BrowserError::Cancelled);
                }
                // Server-controlled redirect URLs can land inside reqwest
                // error text (e.g. `https://user:pass@host/`); strip
                // credentials before the line reaches the error view.
                let clean = crate::util::strip_credentials_for_log(&line);
                lines.push(
                    crate::util::truncate_chars(&clean, config::MAX_ERROR_LINE_CHARS).into_owned(),
                );
                continue;
            }
        }
    }

    Err(BrowserError::NoCandidateWorked {
        asked: start.to_string(),
        lines,
    })
}

/// Sync entry for background threads: runs `fetch_with_fallback` on the
/// shared network runtime. The UI never blocks.
/// `cancel` lets a superseded navigation abort between candidates/chunks.
///
/// The whole job is wrapped by `util::guard_panics`: a panic inside the
/// background thread would drop the promise sender WITHOUT sending, and
/// `poll-promise` PANICS the UI thread when polling such a promise. A
/// parser/YAML invariant panic thus becomes a normal route error.
pub fn fetch_blocking(start: &Url, cancel: Arc<AtomicBool>) -> Result<Fetched, BrowserError> {
    crate::util::guard_panics(move || fetch_blocking_inner(start, cancel))
        .map_err(BrowserError::Fetch)
        .and_then(|r| r)
}

/// Unguarded implementation of [`fetch_blocking`] (see it for the panic
/// contract).
fn fetch_blocking_inner(start: &Url, cancel: Arc<AtomicBool>) -> Result<Fetched, BrowserError> {
    // Local files never touch the network or the shared runtime: read on
    // this job thread (the panic guard above still applies) under their OWN
    // limiter, so a slow/hung filesystem cannot hold a NETWORK fetch slot
    // and heavy network activity never delays opening a local document.
    if crate::protocol::local::is_file_url(start) {
        let _local = match acquire_local_slot(&cancel) {
            Some(g) => g,
            None => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(BrowserError::Cancelled);
                }
                return Err(BrowserError::Fetch(
                    "busy: too many concurrent local reads, retry in a moment".to_string(),
                ));
            }
        };
        return crate::protocol::local::fetch_blocking(start, &cancel);
    }
    // Global network slot: bounds threads/buffers across tabs. Abandoned
    // loads fail fast instead of queueing for the visual budget.
    let _slot = match acquire_slot(&cancel) {
        Some(g) => g,
        None => {
            if cancel.load(Ordering::Relaxed) {
                return Err(BrowserError::Cancelled);
            }
            return Err(BrowserError::Fetch(
                "busy: too many concurrent fetches, retry in a moment".to_string(),
            ));
        }
    };
    // The async work runs on the shared runtime (never a per-job one): the
    // HTTP client's pooled connections are only valid while that runtime
    // drives their background tasks.
    let start = start.clone();
    run_network(async move { fetch_with_fallback(&start, &cancel).await })
        .map_err(BrowserError::Fetch)?
}
