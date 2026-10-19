// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::client`: capped downloads with a route cascade.
//!
//! CONTRACT:
//! - `https://` over the network, or a local `file://` document
//!   (`protocol::local`); any other scheme is rejected BEFORE touching the
//!   network or the disk.
//! - Only `GET`. No POST/HEAD. Works with any static server.
//! - `Accept: text/markdown;q=0.8` header (see `config`).
//! - ALL response headers are ignored except `Content-Type`
//!   (validated), `Content-Length` (only as an early anti-abuse cutoff,
//!   not as cache) and the cache-relevant set captured for the encrypted
//!   disk cache (`Cache-Control`, `Expires`, `ETag`, `Last-Modified`,
//!   `Date`, `Age`, `Vary`; see `cache::policy`). The only client-side
//!   state is that encrypted cache plus the visible tab document.
//! - Each candidate is validated: 2xx status + markdown Content-Type + UTF-8 + YAML ok.
//!   On any failure the next one is tried. When none works, returns
//!   `BrowserError::NoCandidateWorked` with the line-by-line log.
//! - When NO candidate works, the cache's stale record for the asked page is
//!   served instead of the error, marked through [`Fetched::stale_copy`]
//!   ("offline copy" when nothing answered, "saved copy" when the server
//!   answered without a document): stale is served VISIBLY, never as
//!   current. Without a stored record the error view stands.
//! - Background networking: each `fetch` runs on a `poll-promise` thread,
//!   but the async work itself runs on ONE process-wide tokio runtime
//!   (`run_network`) so the HTTP client's pooled connections keep their
//!   background tasks alive between jobs. The UI never blocks.
//! - Reused global HTTP client (TLS/HTTP2 connection pool) for speed.

use crate::config;
use crate::error::BrowserError;
use crate::markdown;
use crate::protocol::content_type::is_markdown_content_type;
use crate::protocol::resolver;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use url::Url;

/// Why a loaded document came from the cache's STALE record instead of a
/// fresh network response (or a fresh cache hit).
///
/// The distinction drives the address-bar indicator's wording: a document
/// served after the network never answered is an "offline copy" (the device
/// looks offline), while one served after the server answered without
/// providing a document (route errors, a wrong `Content-Type`, a page that
/// is gone) is a "saved copy" and must not claim there is no connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleCopy {
    /// No HTTP response arrived for any attempt: offline copy.
    Offline,
    /// The server answered, but no fresh copy was available: saved copy.
    Saved,
}

/// Successfully loaded document.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Normalized URL to show (`/love/`).
    pub display_url: Url,
    /// YAML metadata (when frontmatter existed). Only `title` is surfaced,
    /// through [`Fetched::page_title`] (which falls back to the first `#`
    /// heading); the UI shows no metadata box.
    pub meta: markdown::frontmatter::Meta,
    /// Text of the document's first `#` heading, derived once at fetch time
    /// (`markdown::document::first_h1_title`, bounded): the page-title
    /// fallback when the frontmatter declares no `title`. Read it through
    /// [`Fetched::page_title`] so no caller re-implements the precedence.
    pub h1_title: Option<String>,
    /// Plain text for "Select all"/Copy, derived ONCE here (off the UI
    /// thread): Ctrl/Cmd+C must not re-parse up to 10 MiB on the UI thread.
    pub plain_text: String,
    /// Body parsed ONCE at fetch time into owned events (`'static`).
    /// The viewer renders from this list, so per-frame repaints (scrolling,
    /// selection) never re-run `pulldown-cmark` on up to 10 MiB.
    pub events: Vec<pulldown_cmark::Event<'static>>,
    /// Anchor targets (heading slugs, explicit `{#id}`s and footnote names)
    /// collected from the same events. `#id` links resolve against this.
    pub anchors: markdown::anchors::AnchorIndex,
    /// Every link/image destination resolved ONCE here, so the per-frame
    /// fragment tree does no URL parsing (`protocol::rewrite::DestCache`).
    pub dests: crate::protocol::rewrite::DestCache,
    /// `Some` exactly when this document was served from the cache's stale
    /// record because no fresh copy could be obtained: the address-bar
    /// indicator marks it ([`StaleCopy`]) and the page-info card says so.
    /// `None` for live fetches AND fresh cache hits (a record within its
    /// HTTP freshness is current, deliberately indistinguishable).
    pub stale_copy: Option<StaleCopy>,
}

impl Fetched {
    /// The page's own title: the frontmatter `title` when it has one, else
    /// the first `#` heading's text, else `None` (the URL stays the last
    /// resort of each surface).
    ///
    /// Raw and already bounded (`MAX_TITLE_CHARS`): every display site
    /// sanitizes it (`util::sanitize_title`) and re-caps for its own
    /// surface (tab rail, OS window, page-info card).
    pub fn page_title(&self) -> Option<&str> {
        match self.meta.title.as_deref() {
            // A whitespace-only `title: ""` is "no title", not a page title.
            Some(title) if !title.trim().is_empty() => Some(title),
            _ => self.h1_title.as_deref(),
        }
    }
}

/// Global concurrent-fetch limiter (process-wide, all tabs).
/// `fetch_blocking` acquires a slot before touching the network so rapid
/// multi-tab navigation cannot spawn unbounded threads/buffers.
static FETCH_SLOTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Global concurrent LOCAL-file read limiter, deliberately SEPARATE from
/// [`FETCH_SLOTS`]: a hung NFS/FUSE mount holds its own limiter without
/// starving HTTPS navigations, while a session restore of many local files
/// still cannot spawn unbounded threads/transient buffers.
static LOCAL_SLOTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct SlotGuard;

impl Drop for SlotGuard {
    fn drop(&mut self) {
        FETCH_SLOTS.fetch_sub(1, Ordering::SeqCst);
    }
}

struct LocalSlotGuard;

impl Drop for LocalSlotGuard {
    fn drop(&mut self) {
        LOCAL_SLOTS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Bounded wait for a free slot in `counter` (limit `max`), honoring the
/// cancel flag. On success the counter is already incremented.
fn try_acquire(counter: &std::sync::atomic::AtomicUsize, max: usize, cancel: &AtomicBool) -> bool {
    let deadline = config::deadline_after(config::FETCH_SLOT_WAIT_SECS);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let cur = counter.load(Ordering::SeqCst);
        if cur < max {
            if counter
                .compare_exchange_weak(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
            continue;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(config::FETCH_SLOT_POLL_MS));
    }
}

fn acquire_slot(cancel: &AtomicBool) -> Option<SlotGuard> {
    // `then_some` would CONSTRUCT the guard eagerly and drop it on failure,
    // running `Drop` (a decrement) without a matching increment; the counter
    // then underflows to `usize::MAX` and every later fetch waits out the
    // full slot timeout and reports "busy" forever. `then` is lazy.
    try_acquire(&FETCH_SLOTS, config::MAX_CONCURRENT_FETCHES, cancel).then(|| SlotGuard)
}

fn acquire_local_slot(cancel: &AtomicBool) -> Option<LocalSlotGuard> {
    try_acquire(&LOCAL_SLOTS, config::MAX_CONCURRENT_LOCAL_READS, cancel).then(|| LocalSlotGuard)
}

/// Reused global client: TLS/HTTP2 connection pool across navigations.
/// - Built once (`OnceLock`); `reqwest::Client::clone()` is cheap (Arc).
/// - No cookies, no system proxy, no plaintext cache (deterministic and
///   fast). The only persistent response state is the encrypted disk cache
///   in `cache/`, which this layer feeds but does not own.
/// - Returns `Result`: an `expect()` here would panic the background thread
///   and `poll-promise` panics when polled for a promise whose thread
///   died without sending a value (whole app crash). Must never panic.
pub(crate) fn global_client() -> Result<&'static reqwest::Client, BrowserError> {
    // `get_or_try_init` is unstable on stable: store only `Client` (never `Err`)
    // so a first `build()` failure is NOT memoized and can be retried.
    static CELL: OnceLock<reqwest::Client> = OnceLock::new();
    static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if let Some(c) = CELL.get() {
        return Ok(c);
    }
    // Serialize first-use construction: without the lock two threads racing
    // the first request both build a client and one is dropped for nothing.
    let _guard = INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = CELL.get() {
        return Ok(c);
    }
    match reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(config::MAX_REDIRECTS))
        .timeout(std::time::Duration::from_secs(config::REQUEST_TIMEOUT_SECS))
        .user_agent(config::USER_AGENT)
        .no_proxy()
        .build()
    {
        Ok(c) => Ok(CELL.get_or_init(|| c)),
        Err(e) => Err(BrowserError::Fetch(e.to_string())),
    }
}

/// One process-wide runtime drives EVERY network future.
///
/// The reused HTTP client keeps idle connections in a pool whose background
/// tasks are spawned on the runtime that drove the request. Building a fresh
/// `current_thread` runtime per job (the previous pattern) dropped those
/// tasks together with the job, so a later request could check out a dead
/// pooled connection and fail with `error sending request` (several
/// subresources of one page lost that race). This runtime lives for the
/// whole process, so the pool stays valid across jobs.
static NET_RUNTIME: OnceLock<std::sync::Arc<tokio::runtime::Runtime>> = OnceLock::new();

/// Serializes first-use runtime construction (see [`network_runtime`]).
static NET_RUNTIME_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Returns the shared runtime, creating it on first use.
///
/// Never memoizes a failure (a builder error is returned and can be retried,
/// matching [`global_client`]). A first-use lock keeps two concurrent jobs
/// from building two runtimes, one of which would be dropped unused.
fn network_runtime() -> Result<std::sync::Arc<tokio::runtime::Runtime>, BrowserError> {
    if let Some(rt) = NET_RUNTIME.get() {
        return Ok(rt.clone());
    }
    let _guard = NET_RUNTIME_INIT_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(rt) = NET_RUNTIME.get() {
        return Ok(rt.clone());
    }
    let rt = std::sync::Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config::NETWORK_WORKER_THREADS)
            .enable_all()
            .build()
            .map_err(|e| BrowserError::Fetch(format!("no runtime: {e}")))?,
    );
    Ok(NET_RUNTIME.get_or_init(|| rt).clone())
}

/// Runs `fut` to completion on the shared network runtime and blocks the
/// calling JOB thread until its value arrives.
///
/// `fut` must be `'static`: its connection-pool tasks outlive this call and
/// stay driven by the shared runtime. Panics inside the future are caught
/// and reported as an `internal error: ...` line, so a `poll-promise` thread
/// always sends a value and the UI can never be panicked by polling.
pub(crate) fn run_network<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    use futures_util::FutureExt as _;
    let rt = network_runtime().map_err(|e| e.to_string())?;
    // The job thread drives its own future; the runtime's workers keep the
    // client's pooled background tasks moving. No extra channel/task hop.
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt.block_on(async move { std::panic::AssertUnwindSafe(fut).catch_unwind().await })
    }));
    match out {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(payload)) => Err(crate::util::panic_reason(payload.as_ref())),
        Err(payload) => Err(crate::util::panic_reason(payload.as_ref())),
    }
}

/// Returns `true` when the text looks like a bare email address
/// (`user@domain.tld`): no scheme, no path, one `@`, a dotted domain.
/// Used to reject it with a clear message instead of silently navigating
/// to the domain (`https://user@domain/` strips the local part).
fn looks_like_email(s: &str) -> bool {
    if s.contains('/') || s.contains(':') || s.contains(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    let domain = domain.trim_matches('.'); // trailing dot is a valid FQDN root
    domain.contains('.')
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Non-hierarchical / non-navigable schemes that look like `user:pass@host`
/// but are not URLs: blocked before the userinfo heuristic can misread them
/// as `user:pass@host` or `host:port`.
///
/// `http`/`ftp`/`ftps` are intentionally NOT listed: they are blocked by the
/// generic `<scheme>://` / `<scheme>:` branches in [`parse_https`] (and must
/// keep their scheme in [`sanitize_for_display`], while the schemes below
/// collapse to a domain-only display when they carry a pasted password).
const BLOCKED_SCHEMES: &[&str] = &[
    "mailto",
    "tel",
    "sms",
    "sip",
    "sips",
    "callto",
    "skype",
    "data",
    "javascript",
    "blob",
    "file",
    "about",
    "view-source",
];

/// True for the non-navigable schemes above. Shared with
/// [`crate::protocol::rewrite`] so a `host:port` link spelling
/// (`localhost:3000`) is resolved while `javascript:...` and friends are
/// never mistaken for one.
pub(crate) fn is_blocked_scheme(scheme: &str) -> bool {
    BLOCKED_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
}

/// Validates a URL as `https://`. Returns the parsed `Url` or an error.
///
/// - Accepts with or without scheme: `example.com/love` => `https://example.com/love`.
/// - Accepts schemeless host with port: `example.com:8080/x`, `localhost:3000`.
/// - Rejects `http://`, `mailto:`, `data:`, etc. BEFORE networking (never HTTP).
/// - Rejects bare emails (`user@domain.tld`) instead of navigating to the
///   domain (the local part would be treated as userinfo and stripped).
/// - Detects schemes with `://` and with bare `:` (`mailto:x`).
pub fn parse_https(input: &str) -> Result<Url, BrowserError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::BadUrl("Empty URL".into()));
    }
    // Bound input before `Url::parse`: pasting megabytes must not bloat
    // tab state, logs or error views.
    if trimmed.chars().count() > config::MAX_URL_INPUT_CHARS {
        return Err(BrowserError::BadUrl(format!(
            "URL exceeds {} chars",
            config::MAX_URL_INPUT_CHARS
        )));
    }
    // 1) Explicit scheme? Look for `:` before any `/`.
    if let Some(colon) = trimmed.find(':') {
        let slash = trimmed.find('/').unwrap_or(trimmed.len());
        if colon < slash {
            let pre = trimmed[..colon].trim();
            let post = trimmed[colon + 1..].trim_start();
            // `mailto:addr@host` looks like userinfo but is an email scheme:
            // block it and its `@`-capable or non-hierarchical cousins
            // (`data:`, `javascript:`...) before the userinfo heuristic below
            // can misread them as `user:pass@host` or `host:port`.
            if is_blocked_scheme(pre) {
                // Never echo userinfo credentials into the error view.
                return Err(BrowserError::NotHttps(
                    crate::util::strip_credentials_for_log(trimmed).into_owned(),
                ));
            }
            // `<scheme>://…` (or `scheme:\\…`) is DEFINITIVELY a scheme, never
            // `user:pass@host` (userinfo forms have no authority separator).
            // Without this, `http://user:pass@host` was read as a host with
            // userinfo and sent to `https://http//user:pass@host`, leaking
            // the credentials into the request path, the address bar and the
            // route log; every non-https scheme is blocked here.
            let authority_sep = post.starts_with("//") || post.starts_with('\\');
            // `user:pass@host` carries userinfo: never a scheme. The `@`
            // must sit INSIDE the authority (before the first `/`, `?` or
            // `#`): `http:/user:pass@host` is scheme syntax with an
            // authority separator (one slash), not a userinfo host, and
            // must be blocked instead of folding the credentials into a
            // request path to host `http`.
            let authority_end = post.find(['/', '\\', '?', '#']).unwrap_or(post.len());
            let has_userinfo = post[..authority_end].contains('@') || pre.contains('@');
            let is_localhost = pre.eq_ignore_ascii_case("localhost");
            let looks_like_host =
                pre.contains('.') || pre.contains('[') || is_localhost || has_userinfo;
            // `https:` -> parse as-is (with or without `//`).
            if pre.eq_ignore_ascii_case("https") {
                // Tolerate `https:example.com/x` (missing `//`).
                let normalized = if post.starts_with("//") {
                    trimmed.to_string()
                } else {
                    // Strip the `https:` prefix and any stray `/` or `\`
                    // (the URL parser treats `\` like `/` in an authority),
                    // then re-add `://`.
                    let rest = post.trim_start_matches(['/', '\\']);
                    format!("https://{rest}")
                };
                let mut url =
                    Url::parse(&normalized).map_err(|e| BrowserError::BadUrl(e.to_string()))?;
                if url.host_str().is_none() {
                    return Err(BrowserError::BadUrl(format!(
                        "no host: {}",
                        crate::util::safe_input_echo(trimmed)
                    )));
                }
                // Strip credentials: never send `user:pass@` on the network.
                if !url.username().is_empty() || url.password().is_some() {
                    let _ = url.set_username("");
                    let _ = url.set_password(None);
                }
                return Ok(url);
            }
            if authority_sep {
                // Any other `<scheme>://` form is blocked before networking.
                return Err(BrowserError::NotHttps(
                    crate::util::strip_credentials_for_log(trimmed).into_owned(),
                ));
            }
            // `scheme:user:pass@host` (a SECOND colon before the `@`): the
            // pre-colon token is scheme syntax, not a username (a userinfo
            // username never carries the scheme name only to repeat it).
            // Without this, `ftp:user:pass@host` was read as a userinfo host
            // (`ftp` = username) and navigated to `https://host`, silently
            // reinterpreting a spelling the contract says is blocked. Colons
            // AFTER the `@` (a port) do not count: `user:pass@host:8080`
            // keeps working below.
            let userinfo_end = post[..authority_end].find('@').unwrap_or(authority_end);
            let colons_before_at = 1 + post[..userinfo_end].matches(':').count();
            if has_userinfo
                && colons_before_at >= 2
                && !pre.contains('.')
                && !is_localhost
                && is_scheme_syntax(pre)
            {
                return Err(BrowserError::NotHttps(
                    crate::util::strip_credentials_for_log(trimmed).into_owned(),
                ));
            }
            // Host with port (`example.com:8080`, `localhost:3000`, `user:pass@host`)
            // falls through to prepend `https://`.
            if looks_like_host {
                // Fall through to step 2.
            } else if is_scheme_syntax(pre) {
                // `nas:5000` / `mypi:8080`: a single-label host with a port is
                // still navigable; only non-numeric tails are scheme syntax
                // (`javascript:alert`, `foo:bar`).
                let port_like = post.chars().next().is_some_and(|c| c.is_ascii_digit());
                if !port_like {
                    // Any other `scheme:` (`http`, `mailto`, `data`, ...)
                    // is blocked before touching the network.
                    return Err(BrowserError::NotHttps(
                        crate::util::strip_credentials_for_log(trimmed).into_owned(),
                    ));
                }
            }
            // Otherwise (e.g. `foo bar:baz`): fall through, `Url::parse`
            // with prepended scheme will decide BadUrl.
        }
    }
    // 2) No scheme (or host:port): reject bare emails first, then prepend
    //    `https://` (protocol-relative input keeps its `//`).
    if looks_like_email(trimmed) {
        return Err(BrowserError::BadUrl(format!(
            "{trimmed} looks like an email address, not a URL"
        )));
    }
    let with_scheme = if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        format!("https://{trimmed}")
    };
    let mut url = Url::parse(&with_scheme).map_err(|e| BrowserError::BadUrl(e.to_string()))?;
    if url.host_str().is_none() {
        return Err(BrowserError::BadUrl(format!(
            "no host: {}",
            crate::util::safe_input_echo(&with_scheme)
        )));
    }
    // Strip credentials coming from `user:pass@host` forms.
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    // A non-numeric port (`localhost:abc`) fails at `Url::parse` => BadUrl. Good.
    Ok(url)
}

/// Validates a user-entered address as either an `https://` URL or a local
/// file target. Returns the parsed `Url` or an error.
///
/// Local targets (`file:///x`, `/x`, `~/x`, `./x`, `C:\x`) are delegated to
/// `protocol::local`, which rejects remote `file://host/` shares. Everything
/// else goes through [`parse_https`] (the HTTPS-only network contract).
///
/// This is the ONLY parser navigation should use: `parse_https` stays for
/// callers that specifically need a network URL.
pub fn parse_navigable(input: &str) -> Result<Url, BrowserError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::BadUrl("Empty URL".into()));
    }
    // Bound input before either parser: pasting megabytes must not bloat
    // tab state, logs or error views.
    if trimmed.chars().count() > config::MAX_URL_INPUT_CHARS {
        return Err(BrowserError::BadUrl(format!(
            "URL exceeds {} chars",
            config::MAX_URL_INPUT_CHARS
        )));
    }
    // Local files first: `file:` and unambiguous path spellings never look
    // like a host, and `parse_https` would otherwise reject/misread them.
    if let Some(result) = crate::protocol::local::parse_input(trimmed) {
        return result;
    }
    parse_https(trimmed)
}

/// Returns `true` when `s` has URI scheme syntax
/// (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`).
/// - Keeps `example.com:8080` (host with port) from looking like a scheme.
/// - `.` is allowed by the syntax, but hosts are already filtered by `.` here.
/// - The predicate itself lives in `util` (`is_scheme_syntax`): the log
///   credential stripper uses the SAME rule, so the two can never drift.
fn is_scheme_syntax(s: &str) -> bool {
    crate::util::is_scheme_syntax(s)
}

/// Sanitizes arbitrary pasted input for DISPLAY after `parse_https`
/// rejected it: strips `user:pass@` userinfo so a rejected address (shown in
/// the omnibox and the error view, and copied by "Copy log") can never echo
/// a pasted password. docs/SECURITY.md promises credentials are never logged.
///
/// - `scheme://user:pass@host...` is delegated to
///   [`crate::util::strip_credentials_for_log`].
/// - Schemeless `user:pass@host...` (no `://` to anchor on) is stripped too.
/// - A blocked non-hierarchical scheme (`mailto:a@b`) is kept intact: the
///   `mailto:` side is not a password and stripping it would display a
///   misleading bare domain. `scheme:user:pass@host` (a password after the
///   scheme colon) IS stripped to the domain.
/// - A bare `user@host` (no `:` in the userinfo) carries no secret.
pub(crate) fn sanitize_for_display(input: &str) -> String {
    let stripped = crate::util::strip_credentials_for_log(input);
    // One address token inside an error line is ASCII-whitespace-bounded,
    // and the LAST `@` inside it marks the userinfo. The bound is whitespace
    // only (never `/`, `?` or `#`): a pasted password may itself contain any
    // of those (`user:pa/ss@host`), and the old authority cut hid the `@`
    // beyond them, echoing the password into the address bar, the error
    // view and "Copy log". The bound MUST cover every character the shared
    // stripper treats as a separator: a bare space/tab set let text from a
    // second pasted line ride inside the "token".
    let token_end = stripped
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(stripped.len());
    let Some(at) = stripped[..token_end].rfind('@') else {
        // The shared sanitizer already removed the userinfo. One display
        // nuance survives only when IT did the stripping for a blocked
        // non-hierarchical scheme (`mailto:user:pass@b.com` became
        // `mailto:b.com`): show just the domain. Recovered from the raw
        // input, where the scheme is still visible.
        return blocked_scheme_password_domain(input).unwrap_or_else(|| stripped.into_owned());
    };
    let pre = &stripped[..at];
    let Some(colon) = pre.find(':') else {
        return stripped.into_owned();
    };
    let after = &pre[colon + 1..];
    // `scheme://…` / `scheme:/…` with no other colon before the `@`: the
    // colon is the SCHEME separator, not a `user:pass` pair, and the `@`
    // is path/query data (`https://host/a@b`) — keep the token. A colon
    // after the separators is credential-shaped and falls through.
    if (after.starts_with('/') || after.starts_with('\\')) && !after.contains(':') {
        return stripped.into_owned();
    }
    // `host:port/…@…` (`nas:5000/x@y`): digits followed by a path separator
    // right after the colon read as a port, not as a password — keep the
    // token readable. The SHAPE alone is not proof (a password like
    // `2024/secret` produces the same shape), so the token must also parse
    // as a URL before it is echoed verbatim; the ambiguity with a genuine
    // `host:port/path@…` URL is inherent and accepted (that spelling is
    // navigable when well-formed). Every other shape falls through to the
    // strip below.
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=5).contains(&digits) && matches!(after.as_bytes().get(digits), Some(b'/' | b'\\')) {
        let credential_free =
            Url::parse(&format!("https://{}", &stripped[..token_end])).is_ok_and(|probe| {
                probe.host_str().is_some()
                    && probe.username().is_empty()
                    && probe.password().is_none()
            });
        if credential_free {
            return stripped.into_owned();
        }
    }
    let scheme = pre[..colon].trim();
    if BLOCKED_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
    {
        // `mailto:a@b` has no second colon and stays intact, but
        // `mailto:user:pass@b` carries a pasted password: keep only the
        // domain, exactly like the hierarchical spellings above.
        let has_userinfo_password = after.contains(':');
        return if has_userinfo_password {
            stripped[at + 1..token_end].to_string()
        } else {
            stripped.into_owned()
        };
    }
    // Everything after the token (a second log line, a suffix note) is
    // dropped with the credentials: the display echo is the address only.
    stripped[at + 1..token_end].to_string()
}

/// Domain-only display for a blocked non-hierarchical scheme carrying a
/// pasted password (`mailto:user:pass@b.com` → `b.com`); `None` for every
/// other spelling, where the shared sanitizer's output is already correct.
fn blocked_scheme_password_domain(input: &str) -> Option<String> {
    // Same whitespace bound as [`sanitize_for_display`]: the authority ends
    // at the first separator the URL shape could not contain unencoded.
    let authority_end = input
        .find(|c: char| c.is_ascii_whitespace() || matches!(c, '/' | '?' | '#'))
        .unwrap_or(input.len());
    let at = input[..authority_end].rfind('@')?;
    let pre = &input[..at];
    let colon = pre.find(':')?;
    let scheme = pre[..colon].trim();
    if !BLOCKED_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
    {
        return None;
    }
    pre[colon + 1..]
        .contains(':')
        .then(|| input[at + 1..authority_end].to_string())
}

mod fetch;
mod get;

#[cfg(test)]
pub use fetch::fetch_blocking;
pub use fetch::fetch_blocking_with_mode;
#[cfg(test)]
use fetch::fetch_one;
pub(crate) use fetch::{finish_document, FetchMode};
pub(crate) use get::{get_bytes, Conditional, GetOptions, GetOutcome};

#[cfg(test)]
mod tests;
