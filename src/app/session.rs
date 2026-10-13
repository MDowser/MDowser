// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::session`: open-tab setup helpers.
//!
//! `tabs_from_initial` opens one tab per OS/CLI target; the snapshot
//! helpers remember one session entry per tab (`None` for a Home tab)
//! and compare them without allocating on the per-frame check.

use super::*;
use url::Url;

/// One tab per OS/CLI open target, in order, capped at `MAX_TABS`.
///
/// Each target is navigated with `navigate_to_text`, so a bad path/URL
/// becomes that tab's local error instead of being dropped silently.
pub(super) fn tabs_from_initial(initial: Vec<String>, next_id: &mut usize) -> Vec<Tab> {
    initial
        .into_iter()
        .take(config::MAX_TABS)
        .map(|target| {
            let mut tab = Tab::new(*next_id);
            *next_id += 1;
            tab.navigate_to_text(&target, false);
            tab
        })
        .collect()
}

/// One remembered session entry per tab (`None` for a `Home` tab, whose
/// `current` is the internal `about:home` page), so the tab count and
/// active index survive a restore.
pub(super) fn session_snapshot(tabs: &[Tab]) -> Vec<settings::SessionTab> {
    tabs.iter().map(session_tab_url).collect()
}

/// Session form of one tab's remembered URL: the current page, or the last
/// real navigation target while its load is still in flight (a restored tab
/// that has not resolved, or a failed one, keeps the URL the user asked for
/// instead of collapsing to the `None` Home marker). The internal
/// `about:home` page keeps the `None` marker (no format change; restoring
/// `None` opens a fresh Home tab, which is `about:home` again).
pub(super) fn session_tab_url(tab: &Tab) -> settings::SessionTab {
    tab.session_url().map(|url| url.to_string())
}

/// Compares the live tabs against a saved snapshot without allocating: the
/// per-frame session check must not build a `Vec<String>` just to diff it.
pub(super) fn session_matches(tabs: &[Tab], saved: &[settings::SessionTab]) -> bool {
    tabs.len() == saved.len()
        && tabs
            .iter()
            .zip(saved)
            .all(|(tab, url)| tab.session_url().map(Url::as_str) == url.as_deref())
}

/// Does the stored session need a (re)write? False right after startup when
/// the preference was already on: the startup snapshot was taken from the
/// current tabs, so an untouched window never overwrites a stored session
/// that OS/CLI open targets superseded only for this launch.
pub(super) fn session_needs_save(app: &MdowserApp) -> bool {
    !app.session_restore_enabled
        || app.session_saved_active != app.active
        || !session_matches(&app.tabs, &app.session_saved)
}
