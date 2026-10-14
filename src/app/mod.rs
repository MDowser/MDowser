// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app`: global browser state (tabs + active tab).
//!
//! - Owns `Vec<Tab>`, the active index, the encrypted local history and
//!   the encrypted bookmark store.
//! - `logic()` (no UI): polls every tab (documents + images) and the shared
//!   favicon cache, records resolved pages in the encrypted local history,
//!   ticks the debounced history/bookmark saves, and schedules repaints
//!   only while work is pending.
//! - `ui()`: draws the top toolbar, the vertical tab rail (collapsed
//!   to favicons, expands on hover), the central content panel (including
//!   the internal `about:history` page) and the floating Preferences window.
//! - OS window title = the page's own title while something is open (no
//!   app name or tab label), and just `MDowser` on Home (nothing open).
//! - Neutral document style (`ui::theme`), light/dark from the OS by
//!   default or forced in Preferences (`settings::Prefs`). Preferences also
//!   hold the startup zoom and "restore tabs on startup" (open tabs are
//!   remembered in `settings` and reopened on the next launch).
//! - Keyboard shortcuts: Ctrl/Cmd+T (new tab), Ctrl/Cmd+W (close tab),
//!   Ctrl/Cmd+R / F5 (refresh), Ctrl/Cmd+L (focus address),
//!   Ctrl/Cmd+D (bookmark/unbookmark the loaded page),
//!   Ctrl/Cmd+1..9 (select tab N), Ctrl/Cmd `+`/`=`/`-`/`0` (zoom in/out,
//!   reset), Ctrl+H / Cmd+Y (history, in a new tab),
//!   Ctrl/Cmd+, (preferences), Esc (closes an open window, else stops the
//!   load; reverts the address field when it owns the keyboard).
//! - On macOS the system menu bar (always present there) is completed with
//!   the same commands (`app::menu`); Windows and Linux have no menu at all.

use crate::bookmarks::{BookmarkStore, Toggle};
use crate::browser::tab::{Tab, TabState};
use crate::config;
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use crate::settings::{self, Prefs};
use crate::ui;
use crate::ui::omnibox::BookmarkState;
use crate::ui::preferences::PreferencesState;
use eframe::egui;

/// Main app (implements `eframe::App`).
pub struct MdowserApp {
    /// All open tabs.
    tabs: Vec<Tab>,
    /// Index of the visible tab.
    active: usize,
    /// Counter for unique tab ids.
    next_id: usize,
    /// Last applied window title (avoids calling the OS every frame).
    last_window_title: String,
    /// UI zoom factor (toolbar magnifiers). 1.0 = 100%.
    zoom: f32,
    /// Session favicon cache shared by the tab rail.
    favicons: FaviconCache,
    /// Last expanded rail rect: keeps the rail open while the pointer is
    /// inside the overlay (the overlay covers the collapsed rail's widgets).
    tabs_overlay_rect: Option<egui::Rect>,
    /// Focus the address field on the next frame (new tab / Ctrl+Cmd+L).
    focus_address: bool,
    /// Encrypted local browsing history (configurable retention).
    history: HistoryStore,
    /// Encrypted local bookmarks (listed only on the Home page).
    bookmarks: BookmarkStore,
    /// Persistent preferences (theme, startup zoom, session restore).
    prefs: Prefs,
    /// Last session persisted to disk (`settings::save_session`), so the
    /// file is only rewritten when the open tabs actually change.
    session_saved: Vec<settings::SessionTab>,
    /// Active tab index in `session_saved`.
    session_saved_active: usize,
    /// When the last session save was ATTEMPTED and failed (error streak).
    /// A failed save is retried on a `SESSION_SAVE_RETRY_SECS` backoff
    /// instead of every frame, so a machine with no data directory does not
    /// allocate an error string per frame while the state still gets retried.
    session_save_retry_at: Option<std::time::Instant>,
    /// Whether the session-writer path is armed: true from startup when the
    /// preference is on (the startup snapshot was taken from the live tabs,
    /// so an untouched window never rewrites the file) and false while the
    /// preference is off, so turning it on forces the first write even when
    /// the snapshot did not change.
    session_restore_enabled: bool,
    /// Native-window geometry: the one-shot first-frame placement plus the
    /// debounced persistence of later moves/resizes (`app::window`).
    window_geometry: WindowGeometryState,
    /// Floating Preferences window (visibility lives here so the toolbar and
    /// the tab rail can toggle it). The history list is an internal page.
    windows: PreferencesState,
    /// The native macOS menu bar, installed at startup and drained every
    /// frame. `None` on the other platforms, where there is no menu at all.
    #[cfg(target_os = "macos")]
    mac_menu: Option<menu::AppMenu>,
    /// macOS menu commands awaiting injection into the next frame's raw input
    /// (see `menu`): a menu activation is not a winit event, so it cannot be
    /// replayed from the same pass.
    #[cfg(target_os = "macos")]
    pending_menu_events: Vec<egui::Event>,
}

/// Zoom limits and step for the toolbar magnifiers.
pub(crate) use crate::config::{ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};

impl MdowserApp {
    /// Creates the app. `initial` holds the open targets handed over by the
    /// OS/command line (paths, `file://` or `https://` URLs): they WIN over
    /// session restore, one tab each, so double-clicking a document shows
    /// it. Without targets, the last session is restored when that
    /// preference is on; the persisted theme and startup zoom are applied.
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Vec<String>) -> Self {
        crate::ui::theme::apply(&cc.egui_ctx);
        let mut prefs = Prefs::load();
        prefs.apply_theme(&cc.egui_ctx);
        // Warm the color-emoji font, its shared ~14 MB SVG document index
        // and the rasterizer off the UI thread, so the first document emoji
        // does not pay that cost mid-frame.
        std::thread::spawn(crate::media::emoji::prewarm);

        // "Forget on exit" means NOTHING is read from or written to disk.
        // A stored "restore tabs" preference would still read/write the
        // encrypted `session.bin` (visited URLs), so it is
        // turned off here — before any session is read — and leftovers are
        // deleted. Users can re-enable it after leaving that retention mode.
        let history = HistoryStore::load();
        if history.forgets_on_exit() && prefs.restore_tabs() {
            prefs.set_restore_tabs(false);
        }

        let mut next_id = 1usize;
        let mut tabs: Vec<Tab> = tabs_from_initial(initial, &mut next_id);
        let mut active = 0usize;
        if tabs.is_empty() && prefs.restore_tabs() {
            // Restore the last session: one tab per remembered entry
            // (`None` = a `Home` tab), navigating background loads. Any
            // missing/corrupt session simply starts with a fresh `Home` tab.
            if let Some((session, session_active)) = settings::load_session() {
                for entry in session.into_iter().take(config::MAX_TABS) {
                    let mut tab = Tab::new(next_id);
                    next_id += 1;
                    if let Some(url) = entry {
                        tab.navigate_to_text(&url, false);
                    }
                    tabs.push(tab);
                }
                active = session_active.min(tabs.len().saturating_sub(1));
            }
        }
        // Launching never focuses an address field: the popup would open
        // over a fresh window before the user asked for anything. Explicit
        // `new_tab` (Ctrl/Cmd+T) and `Ctrl/Cmd+L` still focus it.
        let focus_address = false;
        if tabs.is_empty() {
            tabs.push(Tab::new(next_id));
            next_id += 1;
        }
        // Snapshot the starting state so the first `logic` pass does not
        // rewrite an unchanged session.
        let session_saved = session_snapshot(&tabs);
        // Start "already enabled" when the preference is on: the snapshot
        // above was taken from the tabs open NOW, so the first `logic` pass
        // sees no change and does not rewrite (and thereby destroy) a stored
        // session that OS/CLI open targets superseded only for this launch.
        // Enabling the preference mid-session still forces the first write
        // (the flag was false while the preference was off).
        let session_restore_enabled = prefs.restore_tabs();
        // Geometry remembered by a previous session (encrypted settings).
        let window_geometry = WindowGeometryState::new(prefs.window_geometry());
        Self {
            tabs,
            active,
            next_id,
            last_window_title: "MDowser".to_string(),
            zoom: prefs.default_zoom(),
            favicons: FaviconCache::new(),
            tabs_overlay_rect: None,
            focus_address,
            history,
            bookmarks: BookmarkStore::load(),
            prefs,
            session_saved,
            session_saved_active: active,
            session_save_retry_at: None,
            session_restore_enabled,
            window_geometry,
            windows: PreferencesState::new(),
            #[cfg(target_os = "macos")]
            mac_menu: menu::AppMenu::install(&cc.egui_ctx),
            #[cfg(target_os = "macos")]
            pending_menu_events: Vec::new(),
        }
    }

    /// Active tab (mutable). The app always keeps at least one tab
    /// (`close_tab` resets instead of removing the last one).
    /// Never panics: an empty vec recreates the initial tab and an
    /// out-of-bounds index clamps to the last tab.
    fn active_tab_mut(&mut self) -> &mut Tab {
        self.heal_active();
        let idx = self.active.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[idx]
    }

    /// Self-heals the tab list / active index in place (never panics).
    /// Split out of [`Self::active_tab_mut`] so callers that need disjoint
    /// field borrows (the viewer takes both the active `Tab` and the
    /// `HistoryStore`) can validate the index first and then borrow the
    /// fields directly instead of through `self`.
    fn heal_active(&mut self) {
        if self.tabs.is_empty() {
            // Unreachable in normal flow (`close_tab` never empties the vec),
            // but never panic the UI thread: recreate the initial tab.
            self.tabs.push(Tab::new(self.next_id));
            self.next_id += 1;
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    /// Opens the internal local-history page (`about:history`) in a NEW tab
    /// (tab rail button / Ctrl+H / Cmd+Y) so the page being read is never
    /// replaced. The new tab is inserted right after the active one and
    /// activated; clicking a row there then navigates that same tab to the
    /// visit, with Back returning to the list. A repeat request while the
    /// active tab already shows the page is a no-op (the same rule the
    /// in-tab version had), and `MAX_TABS` is honored with a toast.
    fn open_history(&mut self) {
        if self
            .tabs
            .get(self.active)
            .is_some_and(|t| matches!(t.state, TabState::History))
        {
            return;
        }
        self.open_internal_tab(Tab::open_history);
    }

    /// Opens the internal third-party licenses page (`about:licenses`) in a
    /// NEW tab (Preferences "View third-party licenses"), so the page being
    /// read is never replaced. Same rules as [`Self::open_history`]: repeat
    /// request on the active tab is a no-op, `MAX_TABS` shows a toast.
    fn open_licenses(&mut self) {
        if self
            .tabs
            .get(self.active)
            .is_some_and(|t| matches!(t.state, TabState::Licenses))
        {
            return;
        }
        self.open_internal_tab(Tab::open_licenses);
    }

    /// Creates the internal page's tab through `open`: never fetched, never
    /// recorded, inserted right AFTER the active tab and activated. The
    /// current page keeps loading in the background untouched.
    fn open_internal_tab(&mut self, open: fn(&mut Tab, bool)) {
        if self.tabs.len() >= config::MAX_TABS {
            self.active_tab_mut().set_toast("Tab limit reached");
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut tab = Tab::new(id);
        open(&mut tab, true);
        // Standard browser behavior: the new tab sits right after the active
        // one and becomes the visible tab.
        let at = self.active.saturating_add(1).min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.activate_tab(at);
    }

    /// Ctrl/Cmd+D: adds/removes the active loaded page in the encrypted
    /// bookmark store. Bookmarks are listed ONLY on the Home page, so the
    /// confirmation is a toast (never a toolbar indicator, by design). On
    /// Home / Loading / History / Licenses / Failed there is no page to
    /// bookmark.
    fn toggle_bookmark(&mut self) {
        let target = self.tabs.get(self.active).and_then(Tab::bookmark_target);
        let Some((url, title)) = target else {
            self.active_tab_mut().set_toast("Nothing to bookmark");
            return;
        };
        let message = match self.bookmarks.toggle(url.as_str(), title.as_deref()) {
            Toggle::Added => "Bookmark added",
            Toggle::Removed => "Bookmark removed",
            Toggle::LimitReached => "Bookmark limit reached",
            Toggle::Unchanged => return,
        };
        self.active_tab_mut().set_toast(message);
    }

    /// Bookmark ribbon state for the toolbar: unavailable (nothing loaded), not
    /// bookmarked, or bookmarked. The store is only queried, never touched.
    /// Borrows the loaded URL (no per-frame clone) and asks the store's URL
    /// index (no per-frame scan).
    fn bookmark_state(&self, index: usize) -> BookmarkState {
        match self.tabs.get(index).and_then(Tab::loaded_url) {
            None => BookmarkState::Unavailable,
            Some(url) => {
                if self.bookmarks.is_bookmarked_url(url) {
                    BookmarkState::Bookmarked
                } else {
                    BookmarkState::NotBookmarked
                }
            }
        }
    }

    /// "Delete all data and close" (Preferences, two-click confirmation):
    /// removes the whole data directory (history, bookmarks, settings,
    /// session and the shared key) and exits with `process::exit` without
    /// going through `on_exit`, so no pending save can recreate it.
    ///
    /// Order matters: writes are suppressed FIRST (a background worker still
    /// draining its queue must not recreate the directory after the wipe),
    /// then every writer is drained (bounded: a hung disk must not freeze
    /// the exit), and only then is the directory removed. A removal that
    /// FAILS (open handles on Windows, permissions, read-only media) does
    /// not exit pretending the data is gone: writes are re-enabled, the
    /// failure is surfaced in Preferences and the app keeps running.
    fn wipe_all_and_exit(&mut self) {
        crate::settings::suppress_writes();
        self.history.on_exit();
        self.bookmarks.on_exit();
        crate::settings::flush_settings_writes();
        match crate::settings::wipe_all_data() {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                crate::settings::allow_writes();
                self.prefs
                    .note_wipe_error(format!("could not delete all local data: {e}"));
                let message = "Could not delete local data";
                self.active_tab_mut().set_toast(message);
            }
        }
    }

    /// Active tab title for the OS window, safe on corrupted index.
    /// Borrows the tab's display memo (no allocation per frame).
    fn active_window_title(&mut self) -> &str {
        self.heal_active();
        let idx = self.active.min(self.tabs.len().saturating_sub(1));
        self.tabs[idx].window_title_cached()
    }

    /// True when the active tab's window title differs from the one last
    /// sent to the OS. Compares borrowed strings: an idle window never
    /// builds a title String.
    fn window_title_changed(&mut self) -> bool {
        self.heal_active();
        let idx = self.active.min(self.tabs.len().saturating_sub(1));
        let want = self.tabs[idx].window_title_cached();
        want != self.last_window_title
    }

    /// Opens a new `Home` tab and activates it. The address field is left
    /// unfocused (like on launch): only an explicit `Ctrl/Cmd+L` focuses it.
    /// At `MAX_TABS`, refuses and shows a toast instead of allocating.
    fn new_tab(&mut self) {
        if self.tabs.len() >= config::MAX_TABS {
            self.active_tab_mut().set_toast("Tab limit reached");
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id));
        let idx = self.tabs.len() - 1;
        // Full switch: clears the outgoing page's menu/highlight (a Ctrl+T
        // used to leave it latched and reopen at a stale anchor).
        self.activate_tab(idx);
    }

    /// Opens `input` in a NEW background tab, inserted right after the
    /// active one (context menu "Open link in new tab", standard browser
    /// behavior).
    /// The current tab stays active and keeps keyboard focus.
    /// At `MAX_TABS`, refuses and shows a toast instead of allocating.
    fn open_url_in_new_tab(&mut self, input: &str) {
        if self.tabs.len() >= config::MAX_TABS {
            self.active_tab_mut().set_toast("Tab limit reached");
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut tab = Tab::new(id);
        // Validates https and starts the load; invalid input becomes the
        // new tab's local error (never touches the current page).
        tab.navigate_to_text(input, true);
        // Defensive: a corrupted active index must never overflow/panic.
        let at = self.active.saturating_add(1).min(self.tabs.len());
        self.tabs.insert(at, tab);
    }

    /// Activates `idx`, closing the previous page's overlays and stale field
    /// state on the way. Every active-tab change goes through here so no
    /// path (rail, shortcuts, new tab) can leave the old page's menu or
    /// `address_focused` latched.
    fn activate_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        // The context menu / select-all highlight belong to the page that was
        // visible: close them when the active tab changes (otherwise the menu
        // reappears later at a stale anchor). `address_focused` belongs to
        // the field that had the keyboard: a stale true would swallow one Esc.
        if let Some(old) = self.tabs.get_mut(self.active) {
            old.clear_overlay();
            old.address_focused = false;
        }
        self.active = idx;
    }

    /// Activates `idx` (tab rail click / Ctrl+1..9).
    fn select_tab(&mut self, idx: usize) {
        self.activate_tab(idx);
    }

    /// Closes tab `idx`. At least one always remains. `ctx` drops the
    /// closed tab's egui-memory state (history search, bookmark drafts):
    /// ids are never reused, so without this every closed tab would park
    /// its caches for the whole process.
    fn close_tab(&mut self, ctx: &egui::Context, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        // Cancel its in-flight document + image loads before dropping.
        self.tabs[idx].cancel_inflight();
        self.tabs[idx].images.cancel_all();
        let closed_id = self.tabs[idx].id;
        if self.tabs.len() <= 1 {
            // With a single tab, "close" resets it to Home. A FRESH id keeps
            // the "ids are never reused" invariant: the egui-memory state
            // parked under the old id can never resurface in the new tab.
            let id = self.next_id;
            self.next_id += 1;
            self.tabs[0] = Tab::new(id);
            self.active = 0;
            Self::forget_tab_data(ctx, closed_id);
            return;
        }
        self.tabs.remove(idx);
        // Keep the same document visible when closing a tab before active.
        // `[A,B,C] active=1(B), close(0)` -> `[B,C] active=0` (still B).
        if idx < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        Self::forget_tab_data(ctx, closed_id);
    }

    /// Drops the egui-memory state a closed tab parked (history search and
    /// filter cache, in-flight bookmark rename/drag drafts, both omnibox
    /// suggestion caches). Single registry in [`crate::ui::forget_tab`].
    fn forget_tab_data(ctx: &egui::Context, tab_id: usize) {
        crate::ui::forget_tab(ctx, tab_id);
    }
}

// The native macOS menu bar (macOS only; Windows/Linux have no menu).
// `pub(crate)` so `main` can opt out of AppKit's automatic window tabbing
// before eframe creates the first window.
#[cfg(target_os = "macos")]
pub(crate) mod menu;
mod run;
mod session;
mod shortcuts;
mod window;

use session::*;
use window::*;

#[cfg(test)]
mod tests;
