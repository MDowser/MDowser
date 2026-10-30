// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `settings`: persistent user preferences + session, encrypted at rest.
//!
//! Everything lives in `settings.bin` (encrypted) inside the same per-user
//! data directory the encrypted history uses (`history::default_data_dir`,
//! moved here):
//!
//! ```text
//! retention=90
//! theme=system
//! zoom=1
//! restore_tabs=true
//! check_updates=true
//! update_checked=1780000000
//! update_available=0.8.0
//! window=120,80,1536,864,0
//! ```
//!
//! The plaintext above is encrypted with the single shared store key
//! (`store.key`, ChaCha20-Poly1305, see `crate::store::crypto`): preferences include
//! the retention window and restore-tabs flag, so they can reveal browsing
//! habits, and are no longer plain text. Every key is still written
//! INDEPENDENTLY (read-modify-write against a per-directory plaintext cache +
//! encrypt + atomic rename), so
//! the history module and the Preferences window can each update their own
//! keys without clobbering the others, and the file write + `fsync` run on a
//! dedicated background worker (the UI thread never blocks on the disk; see
//! [`write_setting_async`] and [`flush_settings_writes`]). A missing or
//! corrupt value silently
//! falls back to its default: preferences never block or crash the app.
//!
//! `session.bin` is a separate, optional encrypted file that remembers the
//! open tabs for "restore tabs on startup". It holds visited URLs, so it is
//! written only while that preference is enabled and deleted the moment it
//! is turned off. Clean break, no migration: legacy plaintext
//! `settings.conf` / `session.conf` and per-store `history.key` /
//! `bookmarks.key` files are deleted on load, never read.

use crate::config;
use crate::store::crypto::{decrypt, encrypt, load_or_create_key};
use eframe::egui;
use semver::Version;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Encrypted settings file name inside the data directory.
pub(crate) const SETTINGS_FILE: &str = "settings.bin";
/// Encrypted session file name inside the data directory.
const SESSION_FILE: &str = "session.bin";
/// Single shared key file for every encrypted store
/// (`history.bin`, `bookmarks.bin`, `settings.bin`, `session.bin`).
pub(crate) const STORE_KEY_FILE: &str = "store.key";
/// AEAD magic/version for the encrypted settings blob.
const SETTINGS_MAGIC: &[u8; 4] = b"MDST";
const SETTINGS_VERSION: u8 = 1;
/// AEAD magic/version for the encrypted session blob (the inner plaintext
/// keeps the `MDowser-session` header below).
const SESSION_MAGIC_BIN: &[u8; 4] = b"MDSS";
const SESSION_VERSION_BIN: u8 = 1;
/// Legacy files from the pre-unified layout: deleted on load, never read
/// (clean break, no migration).
const LEGACY_SETTINGS_FILE: &str = "settings.conf";
const LEGACY_SESSION_FILE: &str = "session.conf";
const LEGACY_HISTORY_KEY: &str = "history.key";
const LEGACY_BOOKMARKS_KEY: &str = "bookmarks.key";
/// First line of the session plaintext (inside the encrypted blob).
const SESSION_MAGIC: &str = "MDowser-session";
/// Version of the session layout (inside the encrypted blob).
const SESSION_VERSION: u8 = 1;
/// Upper bound on the settings file we are willing to read before rewriting
/// it from scratch ([`config::MAX_SETTINGS_BYTES`]: a corrupt/hostile local
/// file must not be loaded whole).
const MAX_SETTINGS_BYTES: u64 = config::MAX_SETTINGS_BYTES;
/// Upper bound on the session plaintext: one capped URL per tab plus the header.
///
/// The input cap counts CHARS, but `Url::to_string` percent-encodes: one
/// 4-byte UTF-8 char becomes 12 ASCII characters (`%XX` per byte), so a
/// session the app itself wrote could exceed a char-derived byte cap and be
/// silently ignored at startup. The bound below covers the worst encoding
/// of a capped input, and the reader accepts exactly what the writer can
/// produce (URL serialization is ASCII, so bytes == chars here).
pub(crate) const MAX_SESSION_URL_BYTES: usize = config::MAX_URL_BYTES;
const MAX_SESSION_BYTES: u64 = (config::MAX_TABS as u64) * (MAX_SESSION_URL_BYTES as u64 + 1) + 256;

// ---------------------------------------------------------------------------
// Theme preference
// ---------------------------------------------------------------------------

/// User-selectable application theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePref {
    /// Follow the operating system (default).
    #[default]
    System,
    /// Force the light palette.
    Light,
    /// Force the dark palette.
    Dark,
}

impl ThemePref {
    /// The options shown in Preferences, in menu order.
    pub fn options() -> [(ThemePref, &'static str); 3] {
        [
            (ThemePref::System, "System"),
            (ThemePref::Light, "Light"),
            (ThemePref::Dark, "Dark"),
        ]
    }

    /// Human label (also shown as the dropdown field's selected text).
    pub fn label(self) -> &'static str {
        match self {
            ThemePref::System => "System",
            ThemePref::Light => "Light",
            ThemePref::Dark => "Dark",
        }
    }

    /// Setting-file spelling.
    fn as_setting(self) -> &'static str {
        match self {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    /// Parses a setting value; unknown values return `None` (the caller
    /// keeps its default instead of guessing a theme).
    fn from_setting(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Some(ThemePref::System),
            "light" => Some(ThemePref::Light),
            "dark" => Some(ThemePref::Dark),
            _ => None,
        }
    }

    /// egui's equivalent.
    fn to_egui(self) -> egui::ThemePreference {
        match self {
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::Dark => egui::ThemePreference::Dark,
        }
    }
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

/// Persistent preferences owned by `App`.
pub struct Prefs {
    /// Active theme preference.
    theme: ThemePref,
    /// Zoom applied on startup (the toolbar changes it for the session).
    default_zoom: f32,
    /// Reopen the last session's tabs on the next launch.
    restore_tabs: bool,
    /// Check for newer releases automatically (once a day).
    check_updates: bool,
    /// Unix seconds of the last successful update check.
    update_checked: Option<u64>,
    /// Stable version recorded by the last successful check (the view
    /// filters it against the running build).
    update_available: Option<Version>,
    /// Data directory, when persistence is available.
    dir: Option<PathBuf>,
    /// Last failure while deleting the session file, shown in
    /// Preferences: turning restore off must not look like the URLs went
    /// away while `session.bin` survives on disk.
    session_delete_error: Option<String>,
    /// Last failure while WRITING the session snapshot (a background write
    /// on the session worker: full disk, revoked directory), shown beside
    /// the restore-tabs toggle like a delete failure.
    session_save_error: Option<String>,
    /// Last failure while persisting a preference, shown in Preferences
    /// beside the Appearance controls: a silently dropped write would
    /// "revert" the setting on the next launch without explanation.
    settings_error: Option<String>,
    /// Last failure while deleting ALL local data ("Delete all data and
    /// close"): the app stays open and says so instead of exiting as if the
    /// history/bookmarks/settings/session/key were gone.
    wipe_error: Option<String>,
}

impl Prefs {
    /// Loads preferences from the per-user data directory (defaults when no
    /// directory can be resolved).
    pub fn load() -> Self {
        match default_data_dir() {
            Some(dir) => Self::load_from(dir),
            None => Self {
                dir: None,
                ..Self::default()
            },
        }
    }

    /// Loads from an explicit directory (tests + [`Self::load`]).
    pub(crate) fn load_from(dir: PathBuf) -> Self {
        // Check FIRST: a read below preserves an undecryptable blob as `.bak`
        // (rename), so a later error check would see "missing" and stay silent
        // instead of warning in Preferences.
        let settings_error = settings_file_error(&dir);
        let theme = read_setting(&dir, "theme")
            .and_then(|value| ThemePref::from_setting(&value))
            .unwrap_or_default();
        let default_zoom = read_setting(&dir, "zoom")
            .and_then(|value| value.parse::<f32>().ok())
            .map_or(1.0, Self::clamp_zoom);
        let restore_tabs = match read_setting(&dir, "restore_tabs") {
            // No stored value: restore is ON by default.
            None => true,
            // Only an explicit false/0 disables restore: an unknown/corrupt
            // spelling must keep the DEFAULT (true), exactly like the theme
            // and zoom keys. Treating garbage as "off" deleted the encrypted
            // session on the next start (the default for `restore_tabs` is
            // the opposite polarity to every other key).
            Some(value) => {
                let value = value.trim();
                !(value.eq_ignore_ascii_case("false") || value == "0")
            }
        };
        // Same polarity rule: automatic update checks are ON unless the
        // stored value explicitly says off. A corrupt value must not silently
        // disable the notification feature.
        let check_updates = match read_setting(&dir, "check_updates") {
            None => true,
            Some(value) => {
                let value = value.trim();
                !(value.eq_ignore_ascii_case("false") || value == "0")
            }
        };
        let update_checked =
            read_setting(&dir, "update_checked").and_then(|value| value.trim().parse::<u64>().ok());
        let update_available = read_setting(&dir, "update_available").and_then(|value| {
            let version = Version::parse(value.trim()).ok()?;
            (version.pre.is_empty() && version.build.is_empty()).then_some(version)
        });
        // A MISSING file is the documented default state. An UNREADABLE one
        // (permissions, undecryptable, over the cap) must not masquerade as
        // "restore tabs off": that used to delete the session on a
        // transient read failure.
        let mut prefs = Self {
            theme,
            default_zoom,
            restore_tabs,
            check_updates,
            update_checked,
            update_available,
            dir: Some(dir),
            session_delete_error: None,
            session_save_error: None,
            settings_error,
            wipe_error: None,
        };
        if !prefs.restore_tabs && prefs.settings_error.is_none() {
            // A crash between writing `restore_tabs=false` and deleting the
            // session file (or a failed delete the user never retried)
            // would leave visited URLs on disk with the preference off:
            // retry the delete at every start until it succeeds.
            prefs.session_delete_error = delete_session_from(prefs.dir.as_deref())
                .err()
                .map(|e| format!("could not delete {SESSION_FILE}: {e}"));
        }
        prefs
    }

    /// Active theme preference.
    pub fn theme(&self) -> ThemePref {
        self.theme
    }

    /// Zoom applied on startup.
    pub fn default_zoom(&self) -> f32 {
        self.default_zoom
    }

    /// Reopen the last session's tabs on the next launch.
    pub fn restore_tabs(&self) -> bool {
        self.restore_tabs
    }

    /// Automatic update checks (ON by default). Manual `Check now` still
    /// works when this is off.
    pub fn check_updates(&self) -> bool {
        self.check_updates
    }

    /// Enables/disables automatic update checks and persists the choice.
    pub fn set_check_updates(&mut self, enabled: bool) {
        if self.check_updates == enabled {
            return;
        }
        self.check_updates = enabled;
        self.write("check_updates", if enabled { "true" } else { "false" });
    }

    /// Unix seconds of the last successful update check.
    pub fn update_checked(&self) -> Option<u64> {
        self.update_checked
    }

    /// Stable version recorded by the last successful check. `UpdateView`
    /// filters it against the running build, so a stored version from before
    /// a manual upgrade never raises the dot.
    pub fn update_available(&self) -> Option<&Version> {
        self.update_available.as_ref()
    }

    /// Records a successful check: its timestamp plus the newer version (or
    /// `None` when this build is current). Persisted encrypted like every
    /// other preference. A failed check never reaches this method, so the
    /// previous "available" result survives a transient network error.
    pub(crate) fn record_update_check(&mut self, checked_at: u64, available: Option<&Version>) {
        self.update_checked = Some(checked_at);
        self.update_available = available.cloned();
        // Version first, timestamp last: a crash between the two queued
        // writes leaves the OLD timestamp (so the next launch checks again)
        // instead of a new timestamp with a stale "available" version.
        let text = available.map_or_else(String::new, |version| version.to_string());
        self.write("update_available", &text);
        self.write("update_checked", &checked_at.to_string());
    }

    /// Remembered native-window geometry (position, size, maximized), when a
    /// valid `window` key is stored. `None` means "first run" (or a corrupt
    /// value): the app then opens with a centered, monitor-proportional size.
    pub fn window_geometry(&self) -> Option<WindowGeometry> {
        let dir = self.dir.as_deref()?;
        read_setting(dir, WINDOW_KEY).and_then(|value| WindowGeometry::from_setting(&value))
    }

    /// Persists the window geometry through the background writer (encrypted
    /// like every other preference; a missing data directory is a no-op).
    pub fn set_window_geometry(&mut self, geometry: WindowGeometry) {
        self.write(WINDOW_KEY, &geometry.as_setting());
    }

    /// Sets the theme and persists it.
    pub fn set_theme(&mut self, theme: ThemePref) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.write("theme", theme.as_setting());
    }

    /// Sets the startup zoom (clamped) and persists it.
    pub fn set_default_zoom(&mut self, zoom: f32) {
        let zoom = Self::clamp_zoom(zoom);
        if (self.default_zoom - zoom).abs() < f32::EPSILON {
            return;
        }
        self.default_zoom = zoom;
        self.write("zoom", &format_zoom(zoom));
    }

    /// Enables/disables session restore and persists it. Turning it off
    /// deletes the session file immediately; a failed delete is
    /// kept for the Preferences warning (never a silent leak).
    /// Calling it with `false` while already off retries that delete, so the
    /// user never has to toggle on/off to clear leftover URLs.
    pub fn set_restore_tabs(&mut self, restore: bool) {
        if self.restore_tabs == restore {
            if !restore {
                self.delete_session_now();
            }
            return;
        }
        self.restore_tabs = restore;
        self.write("restore_tabs", if restore { "true" } else { "false" });
        if restore {
            self.session_delete_error = None;
        } else {
            self.delete_session_now();
        }
    }

    /// Deletes `session.bin` (and its `.bak`/temp siblings) the safe way:
    /// first as a DISCARD JOB on the session's own worker, so a
    /// queued-or-in-flight snapshot can never be renamed into place AFTER
    /// the delete (the history wipe uses the same serialized job), then a
    /// direct sweep that also reports a failure the user must see. The
    /// bounded flush is acceptable here: turning the preference off is a
    /// rare, user-initiated action.
    fn delete_session_now(&mut self) {
        if let Some(dir) = self.dir.clone() {
            settings_writer().discard_session(dir);
        }
        flush_settings_writes();
        self.session_delete_error = delete_session_from(self.dir.as_deref())
            .err()
            .map(|e| format!("could not delete {SESSION_FILE}: {e}"));
    }

    /// Last failure while deleting `session.bin`, if any (Preferences shows
    /// it under the restore-tabs toggle).
    pub fn session_delete_error(&self) -> Option<&str> {
        self.session_delete_error.as_deref()
    }

    /// Last failure while WRITING the session snapshot, if any (Preferences
    /// shows it under the restore-tabs toggle; a failed background write
    /// also invalidates the "already saved" bookkeeping in `App`).
    pub fn session_save_error(&self) -> Option<&str> {
        self.session_save_error.as_deref()
    }

    /// Last failure while persisting a preference, if any (Preferences
    /// shows it in the Appearance card).
    pub fn settings_error(&self) -> Option<&str> {
        self.settings_error.as_deref()
    }

    /// Per-frame poll of the background settings/session writers: consumes a
    /// finished write's outcome so a failure that happened OFF the UI thread
    /// (full disk, revoked permissions) still reaches the Preferences
    /// warning, and a confirmed success clears a previous one. Cheap: two
    /// mutexes per writer and no allocation while healthy.
    ///
    /// The two files report separately: a successful per-tab session save
    /// (far more frequent) must not clear a settings-write failure, and a
    /// session write that failed on the worker must not pass silently.
    pub fn tick(&mut self) {
        let outcomes = settings_writer().take_outcome();
        if let Some(error) = outcomes.settings_error {
            self.settings_error = Some(error);
        } else if outcomes.settings_succeeded {
            self.settings_error = None;
        }
        if let Some(error) = outcomes.session_error {
            self.session_save_error = Some(error);
        } else if outcomes.session_succeeded {
            self.session_save_error = None;
        }
    }

    /// Records a session-snapshot write failure (synchronous: the blob could
    /// not even be built/enqueued).
    pub(crate) fn note_session_error(&mut self, message: String) {
        self.session_save_error = Some(message);
    }

    /// Records a failed "Delete all data and close" (Preferences shows it in
    /// the Local data card; the app keeps running so the user can retry).
    pub(crate) fn note_wipe_error(&mut self, message: String) {
        self.wipe_error = Some(message);
    }

    /// Last failure while deleting all local data, if any.
    pub fn wipe_error(&self) -> Option<&str> {
        self.wipe_error.as_deref()
    }

    /// Applies the theme preference to egui. Idempotent: it only calls
    /// `set_theme` when the preference actually differs, so an idle window
    /// never invalidates its style every frame.
    pub fn apply_theme(&self, ctx: &egui::Context) {
        let want = self.theme.to_egui();
        if ctx.options(|o| o.theme_preference) != want {
            ctx.set_theme(want);
        }
    }

    /// Persists one key through the background writer (best-effort: a
    /// missing data directory is a no-op). The body update is in-memory; the
    /// encrypted file write + `fsync` run on the shared worker, so a
    /// preference change never stalls a frame. Failures are kept in
    /// [`Self::settings_error`] for the Preferences card; a later success
    /// clears the warning.
    fn write(&mut self, key: &str, value: &str) {
        let Some(dir) = &self.dir else {
            return;
        };
        match write_setting_async(dir, key, value) {
            Ok(()) => self.settings_error = None,
            Err(e) => self.settings_error = Some(format!("could not save {key}: {e}")),
        }
    }

    /// Clamps a zoom factor to the supported range; non-finite input falls
    /// back to 100%.
    fn clamp_zoom(zoom: f32) -> f32 {
        if zoom.is_finite() {
            zoom.clamp(config::ZOOM_MIN, config::ZOOM_MAX)
        } else {
            1.0
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::default(),
            default_zoom: 1.0,
            restore_tabs: true,
            check_updates: true,
            update_checked: None,
            update_available: None,
            dir: None,
            session_delete_error: None,
            session_save_error: None,
            settings_error: None,
            wipe_error: None,
        }
    }
}

/// Setting-file spelling of a zoom factor.
fn format_zoom(zoom: f32) -> String {
    format!("{zoom}")
}

// ---------------------------------------------------------------------------
// Window geometry (remembered across launches)
// ---------------------------------------------------------------------------

/// Setting key holding the last window geometry (`x,y,w,h,maximized`).
const WINDOW_KEY: &str = "window";

/// Remembered native-window geometry, in PHYSICAL pixels.
///
/// Stored in the encrypted `settings.bin` like the theme and zoom: the
/// position is a weak "where you were" datum, so it stays with the rest of
/// the preferences instead of a plaintext window-state file. Physical pixels
/// (not ui points) keep the window stable across a DPI or zoom change, so a
/// window remembered on a monitor at a different scale reopens at the same
/// physical size. While the window is maximized, `pos`/`size` keep the last
/// WINDOWED rectangle, so un-maximizing (now or on the next launch) restores
/// it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// Outer position (top-left of the window frame) in monitor space,
    /// physical pixels.
    pub pos: egui::Pos2,
    /// Inner (content) size, physical pixels.
    pub size: egui::Vec2,
    /// Whether the window was maximized.
    pub maximized: bool,
}

impl WindowGeometry {
    /// Setting-file spelling: `x,y,w,h,maximized` (one line, one key).
    fn as_setting(self) -> String {
        format!(
            "{},{},{},{},{}",
            self.pos.x,
            self.pos.y,
            self.size.x,
            self.size.y,
            u8::from(self.maximized)
        )
    }

    /// Parses the `window` setting. Any malformed/absent field (or a
    /// non-positive/non-finite size) returns `None`, so a corrupt value falls
    /// back to the first-run geometry instead of opening a bogus window.
    fn from_setting(value: &str) -> Option<Self> {
        let mut parts = value.split(',');
        let x = parts.next()?.trim().parse::<f32>().ok()?;
        let y = parts.next()?.trim().parse::<f32>().ok()?;
        let w = parts.next()?.trim().parse::<f32>().ok()?;
        let h = parts.next()?.trim().parse::<f32>().ok()?;
        let maximized = match parts.next().map(str::trim) {
            Some("1") => true,
            Some("0") => false,
            _ => return None,
        };
        if parts.next().is_some() {
            // Trailing fields: refuse rather than silently ignore.
            return None;
        }
        if !x.is_finite()
            || !y.is_finite()
            || !w.is_finite()
            || !h.is_finite()
            || w <= 0.0
            || h <= 0.0
        {
            return None;
        }
        Some(Self {
            pos: egui::pos2(x, y),
            size: egui::vec2(w, h),
            maximized,
        })
    }
}

// ---------------------------------------------------------------------------
// Session (open tabs for restore-on-startup, encrypted)
// ---------------------------------------------------------------------------

/// One remembered tab: `Some(url)` for a page, `None` for a `Home` tab.
pub(crate) type SessionTab = Option<String>;

/// Remembers the open tabs for "restore tabs on startup" (asynchronous:
/// the snapshot is built and encrypted here, the file write + `fsync` run
/// on the shared settings worker, so the per-change session save in
/// `App::logic` never stalls a frame).
mod file;
mod platform;
mod session;

pub(crate) use file::*;
pub(crate) use platform::*;
pub(crate) use session::*;

#[cfg(test)]
mod tests;
