// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{is_transient, Entry, ImageCache};
use crate::config;

#[test]
fn resolved_epoch_tracks_width_affecting_transitions() {
    let mut cache = ImageCache::new();
    assert_eq!(cache.resolved_epoch(), 0);
    // New registration: Unavailable -> Queued renders a new placeholder.
    assert!(cache.ensure("https://ex.com/a.png"));
    assert_eq!(cache.resolved_epoch(), 1);
    // Already registered: no state change, no bump (per-frame `ensure`
    // calls from the renderer must not invalidate the table cache).
    assert!(cache.ensure("https://ex.com/a.png"));
    assert_eq!(cache.resolved_epoch(), 1);
    // Navigation reset drains everything back to Unavailable.
    cache.cancel_all();
    assert_eq!(cache.resolved_epoch(), 2);
}

#[test]
fn retries_only_transient_failures() {
    // The REAL strings produced by `protocol::client::get_bytes`.
    assert!(is_transient(
        "access to https://x/y network error: timed out"
    ));
    assert!(is_transient(
        "access to https://x/y stalled (no data for 15s)"
    ));
    assert!(is_transient(
        "access to https://x/y error reading body: broken pipe"
    ));
    assert!(is_transient("access to https://x/y returned HTTP 503"));
    assert!(is_transient("access to https://x/y returned HTTP 429"));
    // Decode errors are often truncated transfers: one retry can fix.
    assert!(is_transient("image: unexpected end of file"));
    assert!(is_transient("svg: invalid size"));
    // Deterministic DECODE caps stay failed too: the bytes will not
    // change, so a retry only re-downloads and re-decodes the same bomb.
    assert!(!is_transient(
        "image is too large (20000x10, max 12288 px per side)"
    ));
    assert!(!is_transient("image has too many pixels"));
    assert!(!is_transient("svg: more than 400000 markup tokens"));
    // REAL decoder strings: CSS filters and duplicate ids are rejected
    // structurally, so the same bytes stay rejected.
    assert!(!is_transient(
        "svg: CSS filter declarations are not allowed"
    ));
    assert!(!is_transient("svg: duplicate element ids are not allowed"));
    assert!(!is_transient("svg: <pattern> elements are not allowed"));
    assert!(!is_transient(
        "jxl: decoded frame exceeds the allocation budget"
    ));
    // Deterministic outcomes stay failed: retrying cannot help.
    assert!(!is_transient(
        "access to https://x/y returned text/html (expected an image)"
    ));
    assert!(!is_transient("access to https://x/y does not exist (404)"));
    assert!(!is_transient("access to https://x/y returned HTTP 400"));
    assert!(!is_transient(
        "access to https://x/y exceeds 16777216 bytes"
    ));
    assert!(!is_transient(
        "access to https://x/y redirected off https (blocked)"
    ));
    assert!(!is_transient(
        "access to https://x/y redirected with credentials (blocked)"
    ));
    assert!(!is_transient(
        "access to https://x/y redirected to an over-long URL (blocked)"
    ));
    assert!(!is_transient(
        "access to https://u:p@x/y with credentials (blocked)"
    ));
    assert!(!is_transient("access to https://x/y cancelled"));
    assert!(!is_transient("cancelled"));
    assert!(!is_transient(
        "internal error: background job panicked: boom"
    ));
    // Local read/open IO errors are final (same path, same bytes), while a
    // NETWORK error keeps its retry even though it also carries an
    // `(os error N)` suffix.
    assert!(!is_transient(
        "access to file:///tmp/notes.png: Permission denied (os error 13)"
    ));
    assert!(!is_transient(
        "access to file:///tmp/notes.png error reading file: Input/output error (os error 5)"
    ));
    assert!(is_transient(
        "access to https://x/y network error: Connection reset by peer (os error 54)"
    ));
}

/// An entry stuck in the queue past its deadline must become `Failed`:
/// the old code reinserted it as `Queued`, so when no image slot ever
/// freed up it stayed "pending" forever and the app kept its 120 ms
/// repaint loop alive for the whole session.
#[test]
fn expired_queued_entry_fails_instead_of_staying_pending() {
    let mut cache = ImageCache::default();
    let url = "https://ex.com/a.png".to_string();
    cache.entries.insert(
        url.clone(),
        Entry::Queued {
            since: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(
                    config::IMAGE_JOB_TIMEOUT_SECS + 5,
                ))
                .expect("instant arithmetic"),
            not_before: None,
        },
    );
    cache.queue.push_back(url.clone());
    let ctx = eframe::egui::Context::default();
    assert!(cache.poll(&ctx), "timeout reports a change");
    assert!(matches!(cache.entries.get(&url), Some(Entry::Failed(_))));
    assert!(!cache.has_pending(), "no pending work left behind");
    assert!(cache.queue.is_empty(), "stale queue item removed");
}

/// A budget backoff entry is re-queued, not started: the fetch waits at
/// least until `not_before` (the same bytes decoded fine; only VRAM is
/// tight), and the entry keeps counting as pending meanwhile.
#[test]
fn budget_backoff_defers_the_fetch() {
    let mut cache = ImageCache::default();
    let url = "https://ex.com/a.png".to_string();
    cache.entries.insert(
        url.clone(),
        Entry::Queued {
            since: std::time::Instant::now(),
            not_before: Some(std::time::Instant::now() + std::time::Duration::from_secs(60)),
        },
    );
    cache.queue.push_back(url.clone());
    cache.pump();
    assert!(
        matches!(cache.entries.get(&url), Some(Entry::Queued { .. })),
        "must not start during the backoff"
    );
    assert!(!cache.queue.is_empty(), "re-queued for after the backoff");
    assert!(cache.has_pending(), "a deferred entry is still pending");
    // The whole queue is waiting on budget: the earliest deadline is
    // remembered so `ensure` does not re-walk the queue per visible image
    // per frame (the O(visible x queued) scan), and draining the queue
    // clears the gate.
    let at = match cache.entries.get(&url) {
        Some(Entry::Queued { not_before, .. }) => not_before.expect("backoff deadline"),
        _ => panic!("must stay queued"),
    };
    assert_eq!(cache.next_budget_retry, Some(at), "gate armed");
    cache.queue.clear();
    cache.pump();
    assert_eq!(
        cache.next_budget_retry, None,
        "an empty queue clears the gate"
    );
}
