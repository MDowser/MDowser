// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `update`: notification-only release check.
//!
//! The app never downloads or replaces itself. Once a day it asks the fixed
//! GitHub `latest.txt` asset for the newest stable version marker
//! (`X.Y.Z\n`, no `v`), compares it with [`current_version`] and, when the
//! release is newer, exposes the fact through the Preferences button's dot
//! and the Updates card. The user opens the official release page and
//! installs the build the same way they installed this one.
//!
//! - Network: one capped, HTTPS-only GET through the shared client
//!   ([`crate::protocol::client::get_bytes`]); [`config::MAX_UPDATE_MARKER_BYTES`]
//!   and [`config::UPDATE_REQUEST_TIMEOUT_SECS`] bound the response.
//! - Cache: none. The marker is tiny and must never be served stale, so the
//!   request sends `Cache-Control: no-cache` and the answer is not stored.
//! - Failure: classified into a missing marker (HTTP 404, informational), no
//!   network answer, or an unusable answer. Preferences shows a friendly line
//!   for each; raw protocol text never reaches the UI. There is no retry loop:
//!   the session waits out a 24h backoff, a restart follows the normal due
//!   time (a failure writes no timestamp), and `Check now` stays available.
//! - The marker only names a version; it never carries a URL, a signature or
//!   an instruction. The release page URL is built locally from the parsed
//!   semver and the fixed GitHub repository.

use crate::config;
use crate::protocol::client::{self, GetError, GetOptions, GetOutcome};
use crate::settings::Prefs;
use poll_promise::Promise;
use semver::Version;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use url::Url;

/// The current build's version, parsed once.
fn current_version() -> &'static Version {
    static CURRENT: OnceLock<Version> = OnceLock::new();
    CURRENT.get_or_init(|| {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
    })
}

/// Why the last check failed, in the small set the Updates card distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckFailure {
    /// The latest release has no `latest.txt` marker yet (HTTP 404). This is
    /// informational, not an error: a release made before update checks
    /// existed does not publish the asset.
    NoMarker,
    /// No HTTP answer arrived (offline, DNS, TLS, timeout, stall, body IO).
    Offline,
    /// GitHub answered, but the result was not a usable marker (another
    /// HTTP error, a rejected content type or a malformed body).
    Unusable,
}

impl CheckFailure {
    /// Classifies one shared-client failure. A 404 is checked before
    /// `transport`: `transport` is false for every answered response anyway,
    /// but this keeps the most specific state first.
    fn from_get_error(error: &GetError) -> Self {
        if error.status == Some(404) {
            Self::NoMarker
        } else if error.transport {
            Self::Offline
        } else {
            Self::Unusable
        }
    }

    /// Friendly one-line status; raw protocol text stays out of the UI.
    fn status_text(self) -> &'static str {
        match self {
            Self::NoMarker => "The latest release doesn't publish update information yet.",
            Self::Offline => "Couldn't reach GitHub. Check your connection.",
            Self::Unusable => "Couldn't read the update information. Try again later.",
        }
    }

    /// Whether the status should use the error color. A missing marker is a
    /// normal transitional state, not a failure to alarm the user about.
    pub(crate) fn is_error(self) -> bool {
        !matches!(self, Self::NoMarker)
    }
}

/// UI-facing snapshot of the updater: persisted prefs plus live check state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateView {
    /// Automatic checks are enabled (`check_updates`).
    pub enabled: bool,
    /// A check is running right now.
    pub checking: bool,
    /// Why the last automatic/manual attempt in this session failed.
    pub failure: Option<CheckFailure>,
    /// A strictly newer stable version, when one is known.
    pub available: Option<Version>,
    /// Unix seconds of the last successful check, when there is one.
    pub checked_at: Option<u64>,
}

impl UpdateView {
    /// A view with no live check state (the base used by
    /// [`UpdateState::view`] and by the Preferences tests).
    pub(crate) fn from_prefs(prefs: &Prefs) -> Self {
        let available = prefs
            .update_available()
            .filter(|version| *version > current_version())
            .cloned();
        Self {
            enabled: prefs.check_updates(),
            checking: false,
            failure: None,
            available,
            checked_at: prefs.update_checked(),
        }
    }

    /// One-line status shown at the top of the Updates card. Priority:
    /// checking > available > failure > automatic-off > up-to-date > never.
    pub fn status_text(&self) -> String {
        if self.checking {
            "Checking for updates…".to_string()
        } else if let Some(version) = &self.available {
            format!("Version {version} is available.")
        } else if let Some(failure) = self.failure {
            failure.status_text().to_string()
        } else if !self.enabled {
            "Automatic checks are off.".to_string()
        } else if self.checked_at.is_some() {
            "MDowser is up to date.".to_string()
        } else {
            "Not checked yet.".to_string()
        }
    }
}

/// Live updater state owned by `App`: the in-flight check and its retry gate.
#[derive(Default)]
pub(crate) struct UpdateState {
    /// Background fetch, polled once per frame.
    check: Option<Promise<Result<Version, CheckFailure>>>,
    /// Why the last attempt failed (shown until the next successful check).
    failure: Option<CheckFailure>,
    /// Do not retry an automatic check before this monotonic instant.
    retry_at: Option<Instant>,
}

impl std::fmt::Debug for UpdateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateState")
            .field("checking", &self.check.is_some())
            .field("failure", &self.failure)
            .field("retry_at", &self.retry_at)
            .finish()
    }
}

impl UpdateState {
    /// No check started yet.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Current view over the persisted prefs and live state.
    pub(crate) fn view(&self, prefs: &Prefs) -> UpdateView {
        let mut view = UpdateView::from_prefs(prefs);
        view.checking = self.check.is_some();
        view.failure = self.failure;
        view
    }

    /// Polls a finished check and starts the daily one when it is due.
    ///
    /// A failed check does NOT write `update_checked`: after a restart the
    /// normal due time still applies (a recent success is not rechecked just
    /// because a later attempt failed), while this running session waits one
    /// normal interval (see [`Self::retry_at`]) instead of looping.
    pub(crate) fn tick(&mut self, prefs: &mut Prefs) {
        self.poll(prefs);
        if self.check.is_some() || !prefs.check_updates() {
            return;
        }
        if let Some(at) = self.retry_at {
            if at > Instant::now() {
                return;
            }
            // An expired backoff must not keep `next_check_in` pinned to
            // zero when the last successful check is still fresh.
            self.retry_at = None;
        }
        if !is_due(prefs.update_checked(), now_epoch()) {
            return;
        }
        self.start();
    }

    /// Manual `Check now` (also reachable from the macOS menu). Ignored while
    /// a check is already running; works even when automatic checks are off.
    pub(crate) fn check_now(&mut self) {
        if self.check.is_none() {
            self.start();
        }
    }

    /// How long until the UI should wake this module again: while checking
    /// (poll the promise), at the next due time, or at the retry gate. Capped
    /// at an hour so a suspended/woken machine notices an overdue check soon.
    pub(crate) fn next_check_in(&self, prefs: &Prefs) -> Option<Duration> {
        const MAX_WAIT: Duration = Duration::from_secs(60 * 60);
        if self.check.is_some() {
            return Some(Duration::from_millis(250));
        }
        if !prefs.check_updates() {
            return None;
        }
        if let Some(at) = self.retry_at {
            let wait = at.saturating_duration_since(Instant::now()).min(MAX_WAIT);
            if !wait.is_zero() {
                return Some(wait);
            }
        }
        let now = now_epoch();
        if is_due(prefs.update_checked(), now) {
            return Some(Duration::ZERO);
        }
        let last = prefs.update_checked().unwrap_or(now);
        let due = last.saturating_add(config::UPDATE_CHECK_INTERVAL_SECS);
        let remaining = due.saturating_sub(now).min(MAX_WAIT.as_secs());
        Some(Duration::from_secs(remaining))
    }

    /// Applies a finished background check to the persisted prefs.
    fn poll(&mut self, prefs: &mut Prefs) {
        let finished = match &self.check {
            Some(promise) => promise.ready().cloned(),
            None => return,
        };
        let Some(result) = finished else {
            return;
        };
        self.check = None;
        match result {
            Ok(latest) => {
                let checked_at = now_epoch();
                let newer = (latest > *current_version()).then_some(&latest);
                prefs.record_update_check(checked_at, newer);
                self.failure = None;
                self.retry_at = None;
            }
            Err(failure) => {
                // Keep any previously known newer release visible; only the
                // check status changes. No persistence on failure.
                self.failure = Some(failure);
                self.retry_at = Some(config::deadline_after(config::UPDATE_CHECK_INTERVAL_SECS));
            }
        }
    }

    /// Spawns the capped network fetch on a worker thread.
    fn start(&mut self) {
        self.failure = None;
        self.check = Some(Promise::spawn_thread("update-check", fetch_latest));
    }
}

/// Fetches and parses the live release marker. Blocking (runs on a worker).
///
/// Network and body failures collapse to [`CheckFailure`] so the UI never
/// shows a raw protocol string; only a successfully parsed stable version is
/// returned.
pub(crate) fn fetch_latest() -> Result<Version, CheckFailure> {
    let bytes = client::run_network(async {
        let url = Url::parse(config::UPDATE_ENDPOINT).map_err(|_| CheckFailure::Unusable)?;
        let http = client::global_client().map_err(|_| CheckFailure::Unusable)?;
        // The app-level slot/cancel machinery is for navigations and
        // subresources; this one-shot request has its own hard timeout and
        // never needs cancelling.
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let options = GetOptions {
            accept: config::UPDATE_ACCEPT_HEADER_VALUE,
            max_bytes: config::MAX_UPDATE_MARKER_BYTES,
            timeout: Duration::from_secs(config::UPDATE_REQUEST_TIMEOUT_SECS),
            gate: &accept_marker_content_type,
            conditional: None,
            reload: true,
        };
        match client::get_bytes(http, &url, &options, &cancel).await {
            Ok(GetOutcome::Fresh(raw)) => Ok::<Vec<u8>, CheckFailure>(raw.bytes),
            Ok(GetOutcome::NotModified { .. }) => Err(CheckFailure::Unusable),
            Err(error) => Err(CheckFailure::from_get_error(&error)),
        }
    })
    .map_err(|_| CheckFailure::Unusable)??;
    parse_marker(&bytes).map_err(|_| CheckFailure::Unusable)
}

/// Parses the `latest.txt` marker: exactly one stable semver, no `v` prefix,
/// no pre-release/build suffix. Tolerates one trailing newline (LF or CRLF);
/// surrounding whitespace, extra lines and every other spelling are rejected.
pub(crate) fn parse_marker(bytes: &[u8]) -> Result<Version, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "update marker is not valid UTF-8".to_string())?;
    // Exactly one optional trailing line ending; no leading, trailing or
    // extra whitespace. The generated asset is `printf '%s\n' "$VER"`, so
    // anything else is not the marker this project publishes.
    let text = if let Some(rest) = text.strip_suffix("\r\n") {
        rest
    } else {
        text.strip_suffix('\n').unwrap_or(text)
    };
    if text.is_empty() {
        return Err("update marker is empty".to_string());
    }
    if text.contains('\n') || text.contains('\r') {
        return Err("update marker must be one line".to_string());
    }
    let version =
        Version::parse(text).map_err(|e| format!("invalid update marker version: {e}"))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("update marker must name a stable version".to_string());
    }
    Ok(version)
}

/// Runs before the body is streamed: the marker must be plain text or the
/// generic octet-stream GitHub serves for release assets. Anything else
/// (HTML, an error page, a redirect target with a declared document type)
/// is rejected without reading it.
fn accept_marker_content_type(types: &[String]) -> Result<(), String> {
    if types.is_empty() {
        return Err("update marker has no Content-Type".to_string());
    }
    let accepted = types.iter().any(|value| {
        let mime = value.split(';').next().map_or("", str::trim);
        mime.eq_ignore_ascii_case("text/plain")
            || mime.eq_ignore_ascii_case("application/octet-stream")
    });
    if accepted {
        Ok(())
    } else {
        Err("update marker has an unsupported Content-Type".to_string())
    }
}

/// Daily automatic-check interval, with clock-rollback treated as due (a
/// timestamp in the future must not disable checks until the clock catches up).
pub(crate) fn is_due(last_checked: Option<u64>, now: u64) -> bool {
    last_checked.is_none_or(|last| {
        now < last || now.saturating_sub(last) >= config::UPDATE_CHECK_INTERVAL_SECS
    })
}

/// Official release page for a version (opened externally, never in-app).
pub(crate) fn release_url(version: &Version) -> String {
    format!("{}{version}", config::UPDATE_RELEASE_PREFIX)
}

/// Unix seconds, or 0 if the system clock is before the epoch.
fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
