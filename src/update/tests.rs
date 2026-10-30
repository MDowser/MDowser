// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{
    accept_marker_content_type, current_version, is_due, parse_marker, release_url, UpdateState,
};
use crate::config;
use crate::settings::Prefs;
use semver::Version;

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
    let day = config::UPDATE_CHECK_INTERVAL_SECS;
    assert!(is_due(None, 1_000));
    assert!(!is_due(Some(1_000), 1_000 + day - 1));
    assert!(is_due(Some(1_000), 1_000 + day));
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
    view.failed = true;
    // A known release stays visible even if a later check failed.
    assert_eq!(view.status_text(), format!("Version {after} is available."));
    view.available = None;
    assert_eq!(view.status_text(), "Couldn't check for updates.");
    view.failed = false;
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
    update.retry_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    let wait = update.next_check_in(&prefs).expect("automatic checks on");
    assert!(
        wait > std::time::Duration::ZERO,
        "expired gate must wait for the due time, not spin"
    );
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
