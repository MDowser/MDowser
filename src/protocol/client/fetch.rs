// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::client::fetch`: the document route cascade with the encrypted
//! disk cache.
//!
//! `fetch_with_fallback` tries every resolver candidate (2xx + markdown
//! Content-Type + UTF-8 + YAML), `fetch_one` runs one attempt, and
//! `finish_document` turns a body into the renderable [`Fetched`] shared by
//! the HTTPS cascade and `protocol::local`. `fetch_blocking_with_mode` is the
//! background-thread entry point (panic-guarded).
//!
//! Cache contract: a fresh cache record (canonical key, see
//! `cache::policy::doc_key`) short-circuits the network entirely; a stale
//! record with validators revalidates conditionally (304 refreshes it, 200
//! replaces it). When no fresh copy can be obtained — the cascade failed,
//! the network never answered, the server rejected every route, or a stale
//! revalidation could not run — the STALE record is served instead of the
//! error view, marked through [`Fetched::stale_copy`] (offline copy when no
//! HTTP response arrived, saved copy when the server answered without a
//! document). A stale body is therefore never served as if it were
//! current. `FetchMode::ForceRevalidate` (Refresh) skips fresh hits but
//! still stores, and falls back to the stale copy on failure too.

use super::*;
use crate::cache::docs::{self, DocRecord};
use crate::cache::format::now_secs;
use crate::cache::global;
use crate::cache::policy::{
    doc_key, freshness, must_not_store, refresh_freshness, CapturedHeaders,
};

/// How a fetch consults the encrypted disk cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchMode {
    /// Normal navigation: fresh records serve without network.
    Normal,
    /// Refresh: skip fresh hits, revalidate (or refetch) and store.
    ForceRevalidate,
}

/// Outcome of one document candidate attempt.
enum AttemptOutcome {
    /// Fresh body (network or revalidated).
    Fresh {
        text: String,
        final_url: Url,
        headers: CapturedHeaders,
    },
    /// The cache record is still valid (304): serve the stored body.
    NotModified { headers: CapturedHeaders },
}

/// One failed candidate attempt: the error-view line plus whether ANY HTTP
/// response arrived. `transport` is the only "offline" evidence
/// [`serve_stale_copy`] may use: a body pipeline rejection (UTF-8, YAML,
/// parse) or an HTTP/gate verdict is an ANSWERED attempt, so a fallback
/// after those is a saved copy, never an "offline" one.
pub(super) struct AttemptError {
    pub(super) line: String,
    transport: bool,
}

impl std::fmt::Display for AttemptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.line)
    }
}

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
    reload: bool,
    cancel: &AtomicBool,
) -> Result<(String, Url, CapturedHeaders), AttemptError> {
    match fetch_one_attempt(client, url, None, reload, cancel).await? {
        AttemptOutcome::Fresh {
            text,
            final_url,
            headers,
        } => Ok((text, final_url, headers)),
        // No validators were sent: a 304 here answers nothing we asked.
        AttemptOutcome::NotModified { .. } => Err(AttemptError {
            line: format!("access to {url} returned 304 (unexpected)"),
            transport: false,
        }),
    }
}

/// Conditional variant of [`fetch_one`]: sends `If-None-Match` /
/// `If-Modified-Since` from a stale cache record. A 304 answer surfaces as
/// [`AttemptOutcome::NotModified`] (the stored body is still good).
async fn fetch_one_attempt(
    client: &reqwest::Client,
    url: &Url,
    conditional: Option<&get::Conditional>,
    reload: bool,
    cancel: &AtomicBool,
) -> Result<AttemptOutcome, AttemptError> {
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
    let outcome = get_bytes(
        client,
        url,
        &get::GetOptions {
            accept: config::ACCEPT_HEADER_VALUE,
            max_bytes: config::MAX_BODY_BYTES,
            timeout: std::time::Duration::from_secs(config::REQUEST_TIMEOUT_SECS),
            gate: &gate,
            conditional,
            reload,
        },
        cancel,
    )
    .await
    .map_err(|e| AttemptError {
        line: e.message,
        transport: e.transport,
    })?;
    match outcome {
        get::GetOutcome::NotModified { headers } => Ok(AttemptOutcome::NotModified { headers }),
        get::GetOutcome::Fresh(raw) => {
            // UTF-8 is mandatory. `String::from_utf8` takes the owned body: no
            // second full-body copy (the bytes are dead either way).
            let text = String::from_utf8(raw.bytes).map_err(|e| AttemptError {
                line: BrowserError::InvalidUtf8 {
                    url: url.to_string(),
                    detail: e.utf8_error().to_string(),
                }
                .to_string(),
                transport: false,
            })?;
            Ok(AttemptOutcome::Fresh {
                text,
                final_url: raw.final_url,
                headers: raw.headers,
            })
        }
    }
}

/// Turns one successfully obtained body into a renderable [`Fetched`].
///
/// Shared by the HTTPS cascade and `protocol::local`, so a local document
/// and a remote one produce identical parsed events, copy text, anchors and
/// normalized display URL. `error_url` names the attempt in the YAML error
/// line (the loaded URL may differ after a redirect).
///
/// `text` is TAKEN ownership of: the raw body becomes `Fetched::source`
/// ("Export…"), so callers move the buffer instead of copying it once more.
pub(crate) fn finish_document(
    text: String,
    landed: Url,
    error_url: &Url,
) -> Result<Fetched, String> {
    let (meta, body) = markdown::frontmatter::split(&text).map_err(|e| {
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
    // The line names the attempt like every other route error (the same
    // body used to push five identical, context-free lines through the
    // candidate cascade).
    let events = crate::markdown::document::parse_events(&normalized)
        .map_err(|e| format!("access to {error_url}: {e}"))?;
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
    // Heading outline for the reading-aid panel: also from the same events,
    // using the reserved heading ids `anchors` just assigned (bounded).
    let outline = crate::markdown::outline::build(&events, &anchors);
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
        source: text,
        events,
        anchors,
        outline,
        dests,
        // Set only by the stale-copy fallback (`serve_stale_copy`): a body
        // freshly finished here is never marked.
        stale_copy: None,
    })
}

/// Resolves an already-parsed URL by trying candidates in order.
///
/// - Returns `Fetched` with the first valid candidate.
/// - When none qualifies, returns `BrowserError::NoCandidateWorked` with lines.
/// - `cancel`: checked before each candidate; abandoned loads stop early
///   instead of walking all `MAX_ROUTE_CANDIDATES` routes in background.
/// - `mode`: `ForceRevalidate` (Refresh) skips fresh cache hits but still
///   stores the fresh result; `Normal` serves fresh records without network.
pub async fn fetch_with_fallback(
    start: &Url,
    cancel: &AtomicBool,
    mode: FetchMode,
) -> Result<Fetched, BrowserError> {
    // Reuse the global client (TLS/HTTP2 pool). `clone()` is cheap.
    // `https_only` already comes enabled alongside `parse_https`.
    // `?` propagates the error panic-free (see `global_client`).
    let client = global_client()?.clone();
    let reload = mode == FetchMode::ForceRevalidate;

    let candidates = resolver::candidates_for(start);
    let mut lines: Vec<String> = Vec::new();
    // `true` once ANY attempt got an HTTP verdict (status, gate, body,
    // parse): evidence the network answers, so a stale fallback is a SAVED
    // copy and must not claim there is no connection. The fallback labels
    // itself offline only when every attempt failed at the transport level.
    let mut any_response = false;

    for cand in candidates {
        // Cooperative cancellation: a superseded navigation stops here.
        if cancel.load(Ordering::Relaxed) {
            return Err(BrowserError::Cancelled);
        }
        match fetch_one(&client, &cand, reload, cancel).await {
            Ok((text, final_url, headers)) => {
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
                // malformed, try the next route (requirement). The body is
                // MOVED into `finish_document` (it becomes the document's
                // export source); the cache stores the same string from the
                // finished document.
                match finish_document(text, landed.clone(), &cand) {
                    Ok(fetched) => {
                        // Store under BOTH the asked key and the landed key:
                        // the next navigation may type either spelling, and
                        // both are the same canonical page.
                        store_doc_in_cache(start, &landed, &cand, &fetched.source, &headers);
                        return Ok(fetched);
                    }
                    Err(line) => {
                        // The body arrived and failed to parse: an answered
                        // attempt (never "offline" evidence).
                        any_response = true;
                        // Same defense in depth as the network-error arm:
                        // the line embeds the candidate URL, so strip
                        // credentials before it reaches the error view.
                        let clean = crate::util::strip_credentials_for_log(&line);
                        lines.push(
                            crate::util::truncate_chars(&clean, config::MAX_ERROR_LINE_CHARS)
                                .into_owned(),
                        );
                        continue;
                    }
                }
            }
            Err(err) => {
                // A cancel racing the chunk loop surfaces as a "cancelled"
                // line: map it to the typed variant instead of polluting
                // the route log and walking the remaining candidates.
                if cancel.load(Ordering::Relaxed) {
                    return Err(BrowserError::Cancelled);
                }
                any_response |= !err.transport;
                // Server-controlled redirect URLs can land inside reqwest
                // error text (e.g. `https://user:pass@host/`); strip
                // credentials before the line reaches the error view.
                let clean = crate::util::strip_credentials_for_log(&err.line);
                lines.push(
                    crate::util::truncate_chars(&clean, config::MAX_ERROR_LINE_CHARS).into_owned(),
                );
                continue;
            }
        }
    }

    // Every route failed. With a stored copy of the asked page, serve it
    // MARKED instead of the error view (the product decision: marked stale
    // beats an error page; see docs/NAVIGATION.md). Without one, the error
    // stands.
    if cancel.load(Ordering::Relaxed) {
        return Err(BrowserError::Cancelled);
    }
    if let Some(fetched) = serve_stale_copy(start, cancel, any_response) {
        return Ok(fetched);
    }

    Err(BrowserError::NoCandidateWorked {
        asked: start.to_string(),
        lines,
    })
}

/// Which marker a stale-copy fallback carries: only a navigation where
/// NOTHING answered (no HTTP response at all) may claim "offline"; any
/// answered attempt means the saved-copy wording.
fn stale_cause(any_response: bool) -> StaleCopy {
    if any_response {
        StaleCopy::Saved
    } else {
        StaleCopy::Offline
    }
}

/// Rebuilds a `Fetched` from a stale cache record and stamps the marker
/// that drives the address-bar indicator. `Err(key)` when the stored body no
/// longer parses (a hand-edited cache file, decoder drift), so it is never
/// served and its key goes back for cleanup.
fn mark_stale_copy(record: DocRecord, start: &Url, cause: StaleCopy) -> Result<Fetched, String> {
    rebuild_cached(record, start).map(|mut fetched| {
        fetched.stale_copy = Some(cause);
        fetched
    })
}

/// Serves the asked page from the cache's STALE record when no fresh copy
/// could be obtained (the attempted cascade in `fetch_with_fallback`, or a
/// Refresh that never reached the network). `any_response` selects the
/// marker (see [`stale_cause`]).
///
/// A FRESH record can only reach here through Refresh (a Normal navigation
/// serves those before the cascade), and is still a stored copy: the load
/// did not come from the network. `None` when the cache is disabled, has no
/// record for the asked page, or the record no longer parses (the poisoned
/// record is dropped, like the fresh-hit path).
fn serve_stale_copy(start: &Url, cancel: &AtomicBool, any_response: bool) -> Option<Fetched> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let cache = global()?;
    if !cache.enabled() {
        return None;
    }
    let key = doc_key(start)?;
    let record = match cache.doc_lookup(&key)? {
        docs::DocHit::Fresh(record) | docs::DocHit::Stale(record) => record,
    };
    match mark_stale_copy(record, start, stale_cause(any_response)) {
        Ok(fetched) => Some(fetched),
        Err(poisoned_key) => {
            cache.clear_docs_for_key(&poisoned_key);
            None
        }
    }
}

/// Stores a freshly fetched document in the encrypted disk cache under the
/// asked key and (when different) the landed key. `no-store` / uncachable
/// `Vary` responses are skipped silently.
fn store_doc_in_cache(
    start: &Url,
    landed: &Url,
    fetched: &Url,
    text: &str,
    headers: &CapturedHeaders,
) {
    let Some(cache) = global() else { return };
    if !cache.enabled() {
        return;
    }
    let start_key = doc_key(start);
    let landed_key = doc_key(landed);
    // A response that forbids storage must also evict an older record for
    // the same page: otherwise a previous cacheable response would keep
    // serving across a `no-store`/`Vary: *` switch.
    if must_not_store(headers, headers.pragma_no_cache) {
        for key in [start_key.as_deref(), landed_key.as_deref()]
            .into_iter()
            .flatten()
        {
            cache.clear_docs_for_key(key);
        }
        return;
    }
    let now = now_secs();
    let fresh = freshness(headers, now, config::CACHE_DEFAULT_DOC_TTL_SECS);
    let make = |key: &str, fetched_url: &Url| DocRecord {
        key: key.to_string(),
        fetched_url: fetched_url.as_str().to_string(),
        final_url: landed.as_str().to_string(),
        content_type: headers.content_type.clone().unwrap_or_default(),
        stored_at: now,
        fresh_until: fresh.fresh_until,
        etag: headers.etag.clone().unwrap_or_default(),
        last_modified: headers.last_modified.clone().unwrap_or_default(),
        body: text.to_string(),
    };
    if let Some(key) = start_key.as_deref() {
        // Revalidate the exact candidate that produced this body.
        cache.doc_store(make(key, fetched));
    }
    // A redirect may land on a different logical page: that spelling gets
    // its own record so a direct navigation there hits too. Its validator
    // belongs to the LANDED URL, never to the original candidate (which may
    // later serve a different page).
    if let Some(key) = landed_key.as_deref() {
        if start_key.as_deref() != Some(key) {
            cache.doc_store(make(key, landed));
        }
    }
}

/// Test-only convenience entry point (the app always selects a
/// [`FetchMode`] explicitly).
#[cfg(test)]
pub fn fetch_blocking(start: &Url, cancel: Arc<AtomicBool>) -> Result<Fetched, BrowserError> {
    fetch_blocking_with_mode(start, cancel, FetchMode::Normal)
}

/// Background-thread entry point: runs the fetch on the shared network
/// runtime (or serves the encrypted disk cache without touching it). The UI
/// never blocks. `cancel` lets a superseded navigation abort between
/// candidates/chunks; `mode` forces revalidation for Refresh.
///
/// The whole job is wrapped by `util::guard_panics`: a panic inside the
/// background thread would drop the promise sender WITHOUT sending, and
/// `poll-promise` PANICS the UI thread when polling such a promise. A
/// parser/YAML invariant panic thus becomes a normal route error.
pub fn fetch_blocking_with_mode(
    start: &Url,
    cancel: Arc<AtomicBool>,
    mode: FetchMode,
) -> Result<Fetched, BrowserError> {
    crate::util::guard_panics(move || fetch_blocking_inner(start, cancel, mode))
        .map_err(BrowserError::Fetch)
        .and_then(|r| r)
}

/// Unguarded implementation of [`fetch_blocking_with_mode`] (see it for the
/// panic contract).
fn fetch_blocking_inner(
    start: &Url,
    cancel: Arc<AtomicBool>,
    mode: FetchMode,
) -> Result<Fetched, BrowserError> {
    // Local files never touch the network or the shared runtime: read on
    // this job thread (the panic guard above still applies) under their OWN
    // limiter, so a slow/hung filesystem cannot hold a NETWORK fetch slot
    // and heavy network activity never delays opening a local document.
    // Local reads are never cached (the disk is the cache).
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
    // Encrypted disk cache BEFORE the network slot: a fresh record serves
    // without consuming a fetch slot, and a stale record with validators
    // revalidates with one cheap conditional request.
    if mode == FetchMode::Normal {
        if let Some(fetched) = try_serve_cached(start, &cancel) {
            return fetched;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(BrowserError::Cancelled);
        }
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
    run_network(async move { fetch_with_fallback(&start, &cancel, mode).await })
        .map_err(BrowserError::Fetch)?
}

/// Serves a navigation from the encrypted disk cache when possible.
///
/// - Fresh record: parses the stored body and returns it (no network).
/// - Stale record WITH validators: one conditional request against the exact
///   candidate that produced it. 304 refreshes the record and serves the
///   stored body; 200 replaces it through the normal store path; a network
///   failure returns `None` so the caller runs the full cascade (and when
///   that fails too, `fetch_with_fallback` serves the stored copy MARKED as
///   an offline/saved one, never as current).
/// - Stale without validators, disabled cache, or any poisoned record:
///   `None` (full cascade).
/// - `Refresh` never reaches here (it forces revalidation in the cascade).
fn try_serve_cached(
    start: &Url,
    cancel: &Arc<AtomicBool>,
) -> Option<Result<Fetched, BrowserError>> {
    let cache = global()?;
    if !cache.enabled() {
        return None;
    }
    let key = doc_key(start)?;
    match cache.doc_lookup(&key)? {
        docs::DocHit::Fresh(record) => {
            if cancel.load(Ordering::Relaxed) {
                return Some(Err(BrowserError::Cancelled));
            }
            // A stored body that no longer parses (hand-edited cache file,
            // decoder drift) is dropped, never served: fall to the network.
            match rebuild_cached(record, start) {
                Ok(fetched) => Some(Ok(fetched)),
                Err(poisoned_key) => {
                    cache.clear_docs_for_key(&poisoned_key);
                    None
                }
            }
        }
        docs::DocHit::Stale(record) => {
            if record.etag.is_empty() && record.last_modified.is_empty() {
                // Nothing to validate with: full cascade (which stores).
                return None;
            }
            revalidate_cached(start, record, cancel)
        }
    }
}

/// Rebuilds a `Fetched` from a fresh cache record.
///
/// Takes the record BY VALUE: its body is moved into the document
/// (`Fetched::source`), so a cache hit never copies a 10 MiB body once more.
/// On failure the record's cache key comes back, so the caller clears the
/// poisoned entry without holding the body.
fn rebuild_cached(record: DocRecord, error_url: &Url) -> Result<Fetched, String> {
    let DocRecord {
        key,
        final_url,
        body,
        ..
    } = record;
    let Ok(landed) = final_url.parse::<Url>() else {
        return Err(key);
    };
    if landed.scheme() != "https" {
        return Err(key);
    }
    finish_document(body, landed, error_url).map_err(|_| key)
}

/// One conditional request for a stale record. 304 refreshes the record
/// and serves the stored body; 200 replaces it (same store path as the
/// cascade); anything else falls back to the full cascade (`None`), whose
/// failure falls back again to the stale copy, served MARKED
/// (`serve_stale_copy`) instead of the error view.
///
/// The conditional GET is real network work with the same whole-body buffer
/// as any fetch (a hostile origin can answer a validator with 200 + the full
/// cap), so it takes the SAME global slot as a full fetch: without one, many
/// stale hits could bypass the documented concurrency bound. A busy or
/// cancelled slot drops to the full cascade, which re-acquires and reports
/// the same error shape.
fn revalidate_cached(
    start: &Url,
    record: DocRecord,
    cancel: &Arc<AtomicBool>,
) -> Option<Result<Fetched, BrowserError>> {
    let _slot = acquire_slot(cancel)?;
    let fetched_url: Url = record.fetched_url.parse().ok()?;
    if fetched_url.scheme() != "https" {
        return None;
    }
    let conditional = get::Conditional {
        etag: record.etag.clone(),
        last_modified: record.last_modified.clone(),
    };
    let start_clone = start.clone();
    // Owned clones for the `'static` future: `cancel` is only borrowed here.
    let job_cancel = Arc::clone(cancel);
    let attempt_url = fetched_url.clone();
    let outcome = run_network(async move {
        let client = match global_client() {
            Ok(client) => client.clone(),
            // No client, no network: transport evidence (the caller falls
            // back to the cascade, then to the marked stale copy).
            Err(e) => {
                return Err(AttemptError {
                    line: e.to_string(),
                    transport: true,
                })
            }
        };
        fetch_one_attempt(
            &client,
            &attempt_url,
            Some(&conditional),
            false,
            &job_cancel,
        )
        .await
    });
    match outcome {
        Err(_) => {
            // Transport/runtime failure on the revalidation: let the caller
            // run the full cascade (a fresh fetch may still succeed). When
            // it fails too, the stale copy is what the user sees — MARKED
            // as an offline/saved one by `serve_stale_copy`.
            None
        }
        Ok(Err(_)) => {
            // `run_network` transports panics/transport as `Err(String)`;
            // inner fetch errors (gate, status) also fall back to cascade.
            None
        }
        Ok(Ok(AttemptOutcome::NotModified { headers })) => {
            if cancel.load(Ordering::Relaxed) {
                return Some(Err(BrowserError::Cancelled));
            }
            if must_not_store(&headers, headers.pragma_no_cache) {
                // The origin changed its mind (`no-store` on revalidation):
                // drop the record (and the redirect alias) and refetch
                // unconditionally.
                if let Some(cache) = global() {
                    cache.clear_docs_for_key(&record.key);
                    if let Ok(landed) = record.final_url.parse::<Url>() {
                        if let Some(key) = doc_key(&landed) {
                            cache.clear_docs_for_key(&key);
                        }
                    }
                }
                return None;
            }
            let now = now_secs();
            let fresh_until = refresh_freshness(
                &headers,
                record.stored_at,
                record.fresh_until,
                now,
                config::CACHE_DEFAULT_DOC_TTL_SECS,
            );
            if let Some(cache) = global() {
                cache.doc_refresh(&record.key, fresh_until, &headers);
            }
            match rebuild_cached(record, &start_clone) {
                Ok(fetched) => Some(Ok(fetched)),
                Err(poisoned_key) => {
                    if let Some(cache) = global() {
                        cache.clear_docs_for_key(&poisoned_key);
                    }
                    None
                }
            }
        }
        Ok(Ok(AttemptOutcome::Fresh {
            text,
            final_url,
            headers,
        })) => {
            if cancel.load(Ordering::Relaxed) {
                return Some(Err(BrowserError::Cancelled));
            }
            let landed = if final_url.scheme() == "https" {
                final_url
            } else {
                fetched_url.clone()
            };
            match finish_document(text, landed.clone(), &start_clone) {
                Ok(fetched) => {
                    store_doc_in_cache(
                        &start_clone,
                        &landed,
                        &fetched_url,
                        &fetched.source,
                        &headers,
                    );
                    Some(Ok(fetched))
                }
                // A 200 that no longer parses is a server change, not a
                // cache problem: surface it as a normal route failure would
                // (the cascade already ran — this WAS the cascade shortcut).
                Err(line) => {
                    let clean = crate::util::strip_credentials_for_log(&line);
                    let line = crate::util::truncate_chars(&clean, config::MAX_ERROR_LINE_CHARS)
                        .into_owned();
                    Some(Err(BrowserError::NoCandidateWorked {
                        asked: start_clone.to_string(),
                        lines: vec![line],
                    }))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One stored record whose body is the given markdown.
    fn record(body: &str) -> DocRecord {
        DocRecord {
            key: "https://ex.com/page/".to_string(),
            fetched_url: "https://ex.com/page.md".to_string(),
            final_url: "https://ex.com/page/".to_string(),
            content_type: "text/markdown".to_string(),
            stored_at: 1_700_000_000,
            fresh_until: 1_700_003_600,
            etag: String::new(),
            last_modified: String::new(),
            body: body.to_string(),
        }
    }

    /// The stale fallback rebuilds the stored body and stamps the marker the
    /// address bar reads; the rebuilt document is otherwise identical to a
    /// fetched one (same display URL, title and export source).
    #[test]
    fn stale_fallback_marks_the_rebuilt_copy() {
        let start = Url::parse("https://ex.com/page/").unwrap();
        let body = "# Hello\n\nbody\n";
        let offline = mark_stale_copy(record(body), &start, StaleCopy::Offline).expect("rebuild");
        assert_eq!(offline.stale_copy, Some(StaleCopy::Offline));
        assert_eq!(offline.display_url.as_str(), "https://ex.com/page/");
        assert_eq!(offline.page_title(), Some("Hello"));
        assert_eq!(offline.source, body);

        let saved = mark_stale_copy(record(body), &start, StaleCopy::Saved).expect("rebuild");
        assert_eq!(saved.stale_copy, Some(StaleCopy::Saved));
    }

    /// A stored body that no longer parses is never served: the caller gets
    /// the cache key back so it can drop the poisoned record.
    #[test]
    fn stale_fallback_refuses_a_body_that_no_longer_parses() {
        let start = Url::parse("https://ex.com/page/").unwrap();
        let poisoned = mark_stale_copy(
            record("---\ntitle: [unclosed\n---\n\nbody\n"),
            &start,
            StaleCopy::Saved,
        );
        assert_eq!(
            poisoned.expect_err("must not serve"),
            "https://ex.com/page/"
        );
    }

    /// Only a navigation where NOTHING answered may claim "offline": any
    /// answered attempt (status, gate, body or parse verdict) makes it a
    /// saved copy.
    #[test]
    fn stale_marker_never_claims_offline_when_the_server_answered() {
        assert_eq!(stale_cause(false), StaleCopy::Offline);
        assert_eq!(stale_cause(true), StaleCopy::Saved);
    }
}
