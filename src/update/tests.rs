// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    accept_marker_content_type, current_version, is_due, parse_marker, release_url, CheckFailure,
    Retry, UpdateState,
};
use crate::config;
use crate::protocol::client::GetError;
use crate::settings::Prefs;
use semver::Version;
use std::time::{Duration, Instant};

#[test]
fn marker_accepts_one_stable_version_with_optional_newline() {
    for text in ["0.8.0", "0.8.0\n", "0.8.0\r\n"] {
        assert_eq!(
            parse_marker(text.as_bytes()),
            Ok(Version::new(0, 8, 0)),
            "{text:?}"
        );
    }
}

#[test]
fn marker_rejects_prefixes_multi_lines_and_prereleases() {
    for text in [
        "",
        "\n",
        "v0.8.0",
        "0.8",
        "0.8.0\n0.8.1",
        "0.8.0\n\n",
        "0.8.0 ",
        " 0.8.0\n",
        "\n0.8.0",
        "0.8.0-rc.1",
        "0.8.0+build",
        "release 0.8.0",
    ] {
        assert!(
            parse_marker(text.as_bytes()).is_err(),
            "{text:?} must be rejected"
        );
    }
    assert!(parse_marker(&[0xff, 0xfe]).is_err(), "non-UTF-8 rejected");
}

#[test]
fn due_logic_handles_first_check_interval_and_clock_rollback() {
    let interval = config::UPDATE_CHECK_INTERVAL_SECS;
    assert_eq!(interval, 7 * 24 * 60 * 60, "automatic checks are weekly");
    assert!(is_due(None, 1_000));
    assert!(!is_due(Some(1_000), 1_000 + interval - 1));
    assert!(is_due(Some(1_000), 1_000 + interval));
    // A stored timestamp in the future (clock moved backwards) must not
    // disable checks until real time catches up.
    assert!(is_due(Some(2_000), 1_000));
}

#[test]
fn release_url_is_built_from_the_fixed_repository() {
    assert_eq!(
        release_url(&Version::new(0, 8, 1)),
        "https://github.com/MDowser/MDowser/releases/tag/v0.8.1"
    );
}

#[test]
fn marker_content_type_gate_is_strict() {
    for value in [
        "text/plain",
        "text/plain; charset=utf-8",
        "application/octet-stream",
        "Application/Octet-Stream",
    ] {
        assert!(
            accept_marker_content_type(&[value.to_string()]).is_ok(),
            "{value:?}"
        );
    }
    for values in [
        vec![],
        vec!["text/html".to_string()],
        vec!["text/markdown".to_string()],
    ] {
        assert!(
            accept_marker_content_type(&values).is_err(),
            "{values:?} must be rejected"
        );
    }
    // One acceptable value among invalid duplicates is enough: the gate is
    // the same "any declared type is acceptable" rule as documents.
    assert!(
        accept_marker_content_type(&["text/html".to_string(), "text/plain".to_string()]).is_ok()
    );
}

#[test]
fn view_priority_and_newer_than_current_filter() {
    let mut prefs = Prefs::default();
    let after = Version::new(999, 0, 0);
    prefs.record_update_check(42, Some(&after));
    let mut view = UpdateState::new().view(&prefs);
    assert_eq!(view.available.as_ref(), Some(&after));
    assert_eq!(view.status_text(), format!("Version {after} is available."));

    view.checking = true;
    assert_eq!(view.status_text(), "Checking for updates…");
    view.checking = false;
    view.failure = Some(CheckFailure::Unusable);
    // A known release stays visible even if a later check failed.
    assert_eq!(view.status_text(), format!("Version {after} is available."));
    view.available = None;
    assert_eq!(view.status_text(), "Couldn't read the update information.");
    view.retrying = true;
    view.failure = Some(CheckFailure::Offline);
    assert_eq!(view.status_text(), "Couldn't reach GitHub. Trying again…");
    view.retrying = false;
    view.failure = None;
    view.enabled = false;
    assert_eq!(view.status_text(), "Automatic checks are off.");
    view.enabled = true;
    assert_eq!(view.status_text(), "MDowser is up to date.");
    view.checked_at = None;
    assert_eq!(view.status_text(), "Not checked yet.");

    // A stored version that is not newer than the running build never
    // raises the dot (e.g. after the user installed the release by hand).
    prefs.record_update_check(43, Some(current_version()));
    assert!(UpdateState::new().view(&prefs).available.is_none());
}

#[test]
fn check_failures_classify_and_stay_friendly() {
    let answered = |status| GetError {
        message: "raw protocol text".to_string(),
        transport: false,
        status: Some(status),
    };
    assert_eq!(
        CheckFailure::from_get_error(&answered(404)),
        CheckFailure::NoMarker,
        "a missing release marker is its own state"
    );
    assert_eq!(
        CheckFailure::from_get_error(&answered(500)),
        CheckFailure::Unusable,
        "another answered error is an unusable check"
    );
    assert_eq!(
        CheckFailure::from_get_error(&GetError {
            message: "raw protocol text".to_string(),
            transport: true,
            status: None,
        }),
        CheckFailure::Offline,
        "no HTTP answer is offline"
    );

    let mut view = UpdateState::new().view(&Prefs::default());
    view.failure = Some(CheckFailure::NoMarker);
    assert_eq!(
        view.status_text(),
        "The latest release has no update information yet."
    );
    assert!(
        !CheckFailure::NoMarker.is_error(),
        "a missing marker is informational, not an error"
    );
    view.failure = Some(CheckFailure::Offline);
    assert_eq!(view.status_text(), "Couldn't reach GitHub.");
    assert!(CheckFailure::Offline.is_error());
    assert!(
        CheckFailure::Offline.is_retryable(),
        "a transport failure may only be the network coming up"
    );
    assert!(!CheckFailure::NoMarker.is_retryable());
    assert!(!CheckFailure::Unusable.is_retryable());
    view.failure = Some(CheckFailure::Unusable);
    assert_eq!(view.status_text(), "Couldn't read the update information.");
    assert!(
        !view.status_text().contains("raw"),
        "raw protocol text never reaches the card"
    );
}

/// A transport failure retries soon instead of leaving the card failed until
/// the next week; the ladder is bounded and non-transient failures skip it.
#[test]
fn transient_failures_use_a_short_bounded_retry_ladder() {
    let mut update = UpdateState::new();
    for (index, expected) in config::UPDATE_TRANSIENT_RETRY_SECS.iter().enumerate() {
        update.schedule_failure(CheckFailure::Offline);
        let retry = update.retry.expect("a retry is scheduled");
        assert!(retry.force, "a transport failure uses the short ladder");
        assert_eq!(update.retries, index as u8 + 1);
        let wait = retry.at.saturating_duration_since(Instant::now());
        assert!(
            wait > Duration::from_secs(expected.saturating_sub(1))
                && wait <= Duration::from_secs(*expected),
            "wait {wait:?} must be the {expected}s backoff"
        );
    }
    // The ladder is exhausted: fall back to the normal weekly gate.
    update.schedule_failure(CheckFailure::Offline);
    let retry = update.retry.expect("a retry is scheduled");
    assert!(!retry.force, "the weekly gate is not a forced retry");
    assert_eq!(update.retries, 0);
    assert!(
        retry.at.saturating_duration_since(Instant::now())
            > Duration::from_secs(config::UPDATE_CHECK_INTERVAL_SECS - 1),
        "the exhausted ladder falls back to the weekly gate"
    );

    for failure in [CheckFailure::NoMarker, CheckFailure::Unusable] {
        let mut update = UpdateState::new();
        update.schedule_failure(failure);
        let retry = update.retry.expect("a retry is scheduled");
        assert!(!retry.force, "{failure:?} must skip the short ladder");
        assert_eq!(update.retries, 0);
    }
}

/// The short retry runs even when a recent success makes the weekly check not
/// due, and the view says it is trying again instead of showing a dead error.
#[test]
fn transient_retry_is_forced_and_visible() {
    let mut prefs = Prefs::default();
    prefs.record_update_check(super::now_epoch(), None);
    let mut update = UpdateState::new();
    update.failure = Some(CheckFailure::Offline);
    update.retry = Some(Retry {
        at: Instant::now() - Duration::from_secs(1),
        force: true,
    });
    assert_eq!(update.next_check_in(&prefs), Some(Duration::ZERO));

    update.retry = Some(Retry {
        at: Instant::now() + Duration::from_secs(30),
        force: true,
    });
    let view = update.view(&prefs);
    assert!(view.retrying);
    assert_eq!(view.status_text(), "Couldn't reach GitHub. Trying again…");

    // With automatic checks off the card must not promise a retry that will
    // not run; the failure itself stays visible.
    prefs.set_check_updates(false);
    let view = update.view(&prefs);
    assert!(!view.retrying);
    assert_eq!(view.status_text(), "Couldn't reach GitHub.");
}

#[test]
fn next_wake_is_bounded_and_respects_the_opt_out() {
    let mut prefs = Prefs::default();
    prefs.set_check_updates(false);
    assert!(UpdateState::new().next_check_in(&prefs).is_none());
    prefs.set_check_updates(true);
    let wait = UpdateState::new()
        .next_check_in(&prefs)
        .expect("first check is due");
    assert_eq!(wait, std::time::Duration::ZERO);
}

/// An expired failure backoff must fall through to the normal due time
/// instead of requesting an immediate repaint forever when a recent success
/// keeps the check from being due.
#[test]
fn expired_retry_gate_does_not_spin() {
    let mut prefs = Prefs::default();
    prefs.record_update_check(super::now_epoch(), None);
    let mut update = UpdateState::new();
    update.retry = Some(Retry {
        at: Instant::now() - Duration::from_secs(1),
        force: false,
    });
    let wait = update.next_check_in(&prefs).expect("automatic checks on");
    assert!(
        wait > Duration::ZERO,
        "expired gate must wait for the due time, not spin"
    );
}

/// Re-enabling automatic checks must not inherit a failure gate from the
/// previous setting: the normal due time decides again on the next tick.
#[test]
fn toggling_automatic_checks_clears_the_retry_gate() {
    let mut update = UpdateState::new();
    update.schedule_failure(CheckFailure::Offline);
    assert!(update.retry.is_some());
    assert_eq!(update.retries, 1);

    update.automatic_checks_toggled();
    assert!(
        update.retry.is_none(),
        "the old gate must not survive a toggle"
    );
    assert_eq!(update.retries, 0, "the short ladder starts over");

    // After the reset a due check is picked up immediately by the normal
    // path (no success has ever been recorded in this session).
    let mut prefs = Prefs::default();
    prefs.set_check_updates(true);
    assert_eq!(update.next_check_in(&prefs), Some(Duration::ZERO));
}

/// Live smoke test (ignored by default so the suite never touches the
/// network): the real `latest.txt` must be fetchable, within the caps and
/// parse as a stable version.
#[test]
#[ignore = "network: fetches the live latest.txt from GitHub"]
fn live_marker_fetches_and_parses() {
    let version = super::fetch_latest().expect("live marker");
    assert!(
        version.pre.is_empty() && version.build.is_empty(),
        "the live marker must name a stable version"
    );
}
