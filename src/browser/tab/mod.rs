// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab`: one browser tab.
//!
//! `TabState` (Home/Loading/Settling/Loaded/History/Licenses/Failed) plus the
//! `Tab` struct: back/forward stacks, address editing, fetch polling
//! and the internal pages. Navigation methods live in [`nav`], the page
//! summary behind the omnibox indicator card in [`info`]; the
//! poll loop and the simple accessors stay in this file.
//!
//! LIFECYCLE:
//! - `Home`: landing page (large URL box).
//! - `Loading`: a `Promise` is downloading on a background thread.
//!   The UI stays responsive; each frame asks `promise.ready()`. The
//!   previous document stays on screen as backdrop when there is one.
//! - `Settling`: the document arrived and its images resolve in a separate
//!   cache while the backdrop (or the stable empty panel) stays on screen
//!   with the loading indicator. The swap to `Loaded` is atomic once every
//!   image settles or `DOC_IMAGE_GATE_SECS` passes.
//! - `Loaded`: Markdown document ready to render.
//! - `History` / `Licenses`: internal `about:` pages (encrypted local
//!   history; embedded third-party notices); never fetched.
//! - `Failed`: no route worked; the line-by-line log is shown.
//!
//! HISTORY:
//! - `back`: stack of previously shown URLs (normalized `/.../`).
//! - `forward`: stack for the forward button.
//! - Navigating to a new URL pushes `current` to `back` and clears `forward`.
//! - Back/forward/refresh/retry re-navigate: a fresh encrypted disk-cache
//!   record may serve without network, a stale one revalidates, and a load
//!   that fails with a stored copy serves that copy MARKED as stale, never
//!   as current (see [`Tab::stale_copy`]). Home lands on the new tab page
//!   (no network).
//!
//! MINIMAL UI:
//! - `address` is shown WITHOUT `https://` (always https internally; a local
//!   file shows its path). `parse_navigable` accepts https with or without
//!   scheme plus `file://`/absolute/home paths, so editing any of them works.
//!
//! IMAGES:
//! - `images` holds the CURRENT document's decoded textures. Navigation
//!   keeps them only while the old page is the visible backdrop; resolving
//!   the new document (or failing) drops them: no cross-page image cache.
//!   A visual timeout with a backdrop keeps the page (and its textures) and
//!   reports the timeout as a toast instead of dropping both.
//!
//! THROTTLE:
//! - `last_navigate`: instant of the last navigation or refresh.
//! - `can_refresh()` requires 5s (see `config::REFRESH_COOLDOWN_SECS`).

use crate::config;
use crate::error::BrowserError;
use crate::media::cache::ImageCache;
use crate::protocol::client::{self, Fetched};
use crate::protocol::resolver;
use crate::util::{sanitize_title, strip_scheme_prefix, truncate_chars};
use eframe::egui;
use poll_promise::Promise;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

/// Result type traveling from the background thread to the UI.
type FetchResult = Result<Fetched, BrowserError>;

// Short tab titles and window titles are capped in [`crate::config`]
// (`TAB_TITLE_CHARS` / `WINDOW_TITLE_CHARS`) so remote titles cannot bloat
// the rail or reach the window manager unbounded.

/// Internal URL of the local-history page. Intercepted before `parse_navigable`
/// (the `about:` scheme is otherwise blocked), never fetched.
const HISTORY_URL: &str = crate::config::HISTORY_URL;

/// Internal URL of the third-party licenses page (see `licenses`). Same
/// interception rule as [`HISTORY_URL`].
const LICENSES_URL: &str = crate::config::LICENSES_URL;

/// Internal URL of the new-tab page. Like the other `about:` pages it is a
/// real stack entry (`current`), so Back/Forward traverse Home; unlike them
/// the address field stays empty on it (revert clears, see `go_home`).
const HOME_URL: &str = crate::config::HOME_URL;

/// True when `input` names the internal history page (accepted with or
/// without surrounding whitespace, case-insensitively).
fn is_history_input(input: &str) -> bool {
    input.trim().eq_ignore_ascii_case(HISTORY_URL)
}

/// True when `url` is the internal history page (back/forward/refresh pass a
/// parsed `Url`, not the typed text).
fn is_history_url(url: &Url) -> bool {
    url.scheme() == "about" && url.path() == "history"
}

/// True when `input` names the internal licenses page (same rule as
/// [`is_history_input`]).
fn is_licenses_input(input: &str) -> bool {
    input.trim().eq_ignore_ascii_case(LICENSES_URL)
}

/// True when `url` is the internal licenses page.
fn is_licenses_url(url: &Url) -> bool {
    url.scheme() == "about" && url.path() == "licenses"
}

/// True when `input` names the internal new-tab page (same rule as
/// [`is_history_input`]).
fn is_home_input(input: &str) -> bool {
    input.trim().eq_ignore_ascii_case(HOME_URL)
}

/// True when `url` is the internal new-tab page. Shared with the session
/// snapshot, which keeps the `None` Home marker for it (no format change).
pub(crate) fn is_home_url(url: &Url) -> bool {
    url.scheme() == "about" && url.path() == "home"
}

/// Post-load scroll target carried by a URL fragment (`page#section`), if any.
/// A bare `#` (empty fragment) has no target.
fn anchor_from_url(url: &Url) -> Option<String> {
    url.fragment().filter(|f| !f.is_empty()).map(str::to_string)
}

/// One tab: URL, history, state and throttle.
pub struct Tab {
    /// Unique id (for "Tab N" titles).
    pub id: usize,
    /// Schemeless address field (minimal, e.g. `ex.com/love/`; a local file
    /// shows its decoded path).
    /// - While typing, free text (with or without `https://`/`file://`).
    /// - After loading, pinned to `display_string(display_url)` (normalized, schemeless).
    pub address: String,
    /// Current state (home / loading / loaded / error).
    pub state: TabState,
    /// Back history (full normalized URLs, schemed internally).
    back: Vec<Url>,
    /// Forward history.
    forward: Vec<Url>,
    /// Current normalized URL (when a document exists; kept after a failure
    /// so back navigation is not lost). The new-tab page is a real entry
    /// too (`about:home`), so Back/Forward traverse it; the address field
    /// still shows empty on it.
    pub current: Option<Url>,
    /// Last real (non-internal) navigation target, kept while its load is
    /// still in flight or has failed: the session snapshot remembers the URL
    /// the user asked for instead of serializing a slow/failed restore as
    /// Home (which erased it from `session.bin` on the first save). Cleared
    /// when an internal page (`about:home`/`history`/`licenses`) is opened;
    /// `current` wins whenever it is not the Home marker.
    /// Read through [`Self::session_url`].
    session_target: Option<Url>,
    /// Last navigation/refresh (disables "Refresh" for 5s).
    last_navigate: Option<Instant>,
    /// "Select all" flag (content menu or Ctrl/Cmd+A): the whole document
    /// renders highlighted until the next click/drag or navigation. `Copy`
    /// and Ctrl/Cmd+C then copy the full plain text.
    pub selected_all: bool,
    /// Right-click menu state (global secondary-click detector + `Area`
    /// popup, so right-click works on text too, not just empty padding).
    pub context_open: bool,
    /// Popup anchor (cursor pos at secondary click, screen coords).
    pub context_pos: Option<egui::Pos2>,
    /// Viewport rect when the menu opened. The anchor is in screen coords,
    /// so a window move invalidates it: the menu closes instead of drifting.
    pub context_viewport: Option<egui::Rect>,
    /// Link under the cursor when the menu opened: the menu offers
    /// "Open link in new tab" and "Copy link address" above Copy / Select all.
    pub context_link: Option<String>,
    /// A secondary press over the document was intercepted BEFORE egui (see
    /// `MdowserApp::raw_input_hook`) because a text selection existed: the
    /// label-selection plugin reads any press as the start of a drag and
    /// would collapse the selection before the menu could copy it. True from
    /// that press until its release.
    pub(crate) secondary_press_intercepted: bool,
    /// The intercepted gesture's release, delivered to the viewer as a
    /// context click (the menu opens from it instead of `secondary_clicked`,
    /// which never fires for an intercepted gesture).
    pub(crate) pending_context_click: Option<egui::Pos2>,
    /// Screen-space hitboxes of the links rendered this frame (rebuilt by
    /// `viewer`). Used to find the link a right-click landed on.
    pub link_hits: Vec<crate::viewer::LinkHit>,
    /// Transient copy confirmation ("Text copied").
    pub toast_text: Option<String>,
    /// When the toast was set (auto-dismiss after ~2s).
    pub toast_at: Option<Instant>,
    /// Last rendered document area (screen coords, previous frame), used to
    /// attach the right-click context menu. Updated after each central panel.
    pub doc_rect: Option<egui::Rect>,
    /// In-document anchor requested by an anchor link click or a cross-page
    /// `#fragment`. Consumed by the viewer's first matching heading/footnote;
    /// dropped when the loaded document has no such target.
    pub pending_anchor: Option<String>,
    /// True when the next freshly loaded document must start at the top.
    /// Set by a new navigation that pushes history, consumed by the
    /// viewer's first `Loaded` frame (`render_doc` forces offset 0).
    /// Back/Forward/Refresh leave it false so egui restores the saved
    /// offset; failed or abandoned loads clear it so the restored page
    /// keeps its position.
    pub(crate) scroll_top_requested: bool,
    /// Cancel flag for the current in-flight load (if any).
    /// Cloned into the background thread; set on supersede/close/timeout.
    cancel: Option<Arc<AtomicBool>>,
    /// True when the user edited `address` after the last navigation started.
    /// Set by navbar/home via `mark_address_edited()` on `TextEdit::changed()`;
    /// `poll()` preserves typing when dirty instead of comparing strings
    /// (which failed when the user typed and reverted to the same text).
    pub address_dirty: bool,
    /// Decoded images of the CURRENT document (keyed by absolute URL).
    /// Dropped/cancelled on every navigation: decoded TEXTURES are never
    /// shared across pages (the encrypted disk cache below this layer still
    /// avoids re-downloading them).
    pub images: ImageCache,
    /// Color-emoji textures for this tab (bounded, process-budgeted).
    pub emoji: crate::media::emoji::EmojiCache,
    /// True when an address field (toolbar or home omnibox) had keyboard
    /// focus at the end of the last drawn frame. egui clears focus on Esc
    /// BEFORE the UI runs, so `App` must remember this to let Esc revert the
    /// field (`ui::omnibox`) instead of stopping the load.
    pub address_focused: bool,
    /// True when an inline bookmark-tile field (rename / icon) was open at
    /// the end of the last drawn frame. Same Esc race as `address_focused`:
    /// `App` must leave the key to the field (`ui::bookmarks` cancels on
    /// it) instead of consuming it as a Stop.
    pub tile_editing: bool,
    /// True when a Home bookmark tile (or one of its pencil/X buttons) had
    /// keyboard focus at the end of the last drawn frame. `Alt+Left`/
    /// `Alt+Right` are the grid's keyboard reorder on a focused tile, so the
    /// app-level Back/Forward shortcut must leave those keys alone; the flag
    /// is refreshed by every Home pass and cleared with the other overlay
    /// state on navigation/tab switch.
    pub tile_focused: bool,
    /// Omnibox page-info popup owner: the id salt (`"address-bar"` /
    /// `"home-address"`) of the surface whose security-indicator popup is
    /// open for this tab, if any. View state like `address_focused`:
    /// every navigation and tab switch resets it through `clear_overlay`,
    /// so the popup can never describe a page that is gone.
    pub(crate) site_info: Option<&'static str>,
    /// A successfully loaded page waiting to be recorded in the local
    /// history store: `(normalized URL, raw page title)` where the title is
    /// the frontmatter `title` or the first-H1 fallback
    /// ([`Fetched::page_title`]). Set when a
    /// fetch resolves and taken by `App::logic` right after `poll`; only
    /// pages that actually loaded are recorded. `None` when nothing new.
    pub pending_visit: Option<(Url, Option<String>)>,
    /// Bumped every time the visible document is replaced (resolve /
    /// history page). Mixed into the code-block cache salt so a refresh
    /// can never serve the previous body, without flushing OTHER tabs'
    /// cached blocks (the old global `code::clear_cache` did). It is ALSO
    /// the validity key of [`Self::display_cache`]: every transition that
    /// can change a tab's title or favicon origin bumps it.
    pub(crate) doc_generation: u64,
    /// In-page find (`Ctrl/Cmd+F`) state: query, current match and last
    /// rendered total. Per tab; cleared on navigation/tab switch with the
    /// rest of the overlay state.
    pub find: crate::viewer::find::FindState,
    /// Document outline sidebar state (reading aid): scrollspy cursor,
    /// measured row heights and the frame's heading rects. Per tab (each tab
    /// keeps its own section cursor); cleared on navigation/tab switch with
    /// the rest of the overlay state, since the cursor belongs to the page on
    /// screen.
    pub outline: crate::viewer::outline::OutlineState,
    /// Set by the context menu's "Save page" row (the menu has no access to
    /// the app's save path): `App::ui` reads it right after
    /// `viewer::show_content` and saves the page then on screen, so a
    /// navigation later in the same frame cannot swap the document out from
    /// under the write.
    pub save_asked: bool,
    /// Next fetch bypasses fresh disk-cache hits and revalidates instead
    /// (set by Refresh; consumed when the fetch spawns). Back/Forward and
    /// plain navigations serve fresh records without network.
    force_revalidate: bool,
    /// Memoized `(state kind, generation, title, favicon origin)` for the
    /// tab strip. See [`Self::display`].
    display_cache: Option<(TabStateKind, u64, TabDisplay)>,
}

/// Display data the tab strip reads every frame (see [`Tab::display`]).
///
/// One struct instead of four `Tab` accessors: the rail holds the memo
/// (`tab.display()`) while it paints, so every per-frame value must come
/// from the same borrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabDisplay {
    /// Short title (`Tab::title`).
    pub title: String,
    /// OS window title (`Tab::window_title`, test-only as a direct call),
    /// memoized with the rest so
    /// the per-frame title comparison allocates nothing.
    pub window_title: String,
    /// Tab-strip favicon origin (`Tab::favicon_origin`); `None` for Home,
    /// local files and internal pages.
    pub origin: Option<Url>,
    /// State discriminant (favicon placeholder family).
    pub kind: TabStateKind,
    /// `Tab::is_loading()` at memo time: favicons still probing show the
    /// hourglass (and a loading dot over a stale icon).
    pub loading: bool,
}

/// Fallback display for the unreachable "memo not filled" case in
/// [`Tab::display`]: static, allocation-free at first use (`String::new()`
/// is const), so the UI thread never panics on a defensive `expect`.
static EMPTY_DISPLAY: TabDisplay = TabDisplay {
    title: String::new(),
    window_title: String::new(),
    origin: None,
    kind: TabStateKind::Home,
    loading: false,
};

impl Tab {
    /// Creates a new tab on `Home` (`about:home`, address empty).
    pub fn new(id: usize) -> Self {
        Self {
            id,
            address: String::new(),
            state: TabState::Home,
            back: Vec::new(),
            forward: Vec::new(),
            // `Url::parse` of a static `about:` URL cannot fail; degrade to
            // no current (the old shape) instead of panicking the UI thread.
            current: Url::parse(HOME_URL).ok(),
            session_target: None,
            last_navigate: None,
            selected_all: false,
            context_open: false,
            context_pos: None,
            context_viewport: None,
            context_link: None,
            secondary_press_intercepted: false,
            pending_context_click: None,
            link_hits: Vec::new(),
            toast_text: None,
            toast_at: None,
            doc_rect: None,
            pending_anchor: None,
            scroll_top_requested: false,
            cancel: None,
            address_dirty: false,
            images: ImageCache::new(),
            emoji: crate::media::emoji::EmojiCache::new(),
            address_focused: false,
            tile_editing: false,
            tile_focused: false,
            site_info: None,
            pending_visit: None,
            doc_generation: 0,
            find: crate::viewer::find::FindState::default(),
            outline: crate::viewer::outline::OutlineState::default(),
            save_asked: false,
            force_revalidate: false,
            display_cache: None,
        }
    }

    /// Memoized display data the tab strip reads for every tab every frame.
    /// Recomputed exactly when the state kind or `doc_generation` changes —
    /// every transition that can alter the title or the favicon origin
    /// bumps the generation or changes the kind, so both consumers stay
    /// correct while costing one comparison per frame.
    ///
    /// `&mut self` on purpose: the rail owns the list and mutates nothing
    /// else; no interior mutability is needed. Returns the memo plus
    /// whether it was just rebuilt, so a caller that needs owned data can
    /// clone only on a real change.
    pub fn display(&mut self) -> (&TabDisplay, bool) {
        let rebuilt = self.refresh_display();
        match self.display_cache.as_ref() {
            Some((_, _, display)) => (display, rebuilt),
            // Unreachable (`refresh_display` always fills the cache), but the
            // UI thread never panics on a defensive `expect`: fall back to a
            // static empty display.
            None => (&EMPTY_DISPLAY, rebuilt),
        }
    }

    /// Rebuilds the memo when stale; `true` when it was rebuilt.
    fn refresh_display(&mut self) -> bool {
        let key = (self.state_kind(), self.doc_generation);
        let stale = !matches!(
            &self.display_cache,
            Some((kind, generation, _)) if (*kind, *generation) == key
        );
        if stale {
            let title = self.compute_title();
            let window_title = self.compute_window_title();
            let origin = self.compute_favicon_origin();
            self.display_cache = Some((
                key.0,
                key.1,
                TabDisplay {
                    title,
                    window_title,
                    origin,
                    kind: key.0,
                    loading: self.is_loading(),
                },
            ));
        }
        stale
    }

    /// Which variant the state is (for the favicon placeholder family).
    pub fn state_kind(&self) -> TabStateKind {
        self.state.kind()
    }
    /// Cancels any in-flight load (cooperative: the thread checks the flag).
    /// A settling gate is cancelled too: its image jobs are dropped with
    /// the state, so their slots free for the next navigation.
    pub(crate) fn cancel_inflight(&mut self) {
        if let Some(flag) = self.cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        // Also flag the promise-held copy if state still holds it.
        if let TabState::Loading { cancel, .. } = &self.state {
            cancel.store(true, Ordering::Relaxed);
        }
        if let TabState::Settling { settling, .. } = &mut self.state {
            settling.cancel_all();
        }
    }

    /// True while any media (current or settling images, color-emoji rasters)
    /// is queued or downloading (repaint scheduler).
    pub fn media_pending(&self) -> bool {
        if self.images.has_pending() || self.emoji.has_pending() {
            return true;
        }
        if let TabState::Settling { settling, .. } = &self.state {
            return settling.has_pending();
        }
        false
    }

    /// Clears per-page overlay state (selection highlight + open menu).
    /// Called on every navigation so the menu never leaks across documents,
    /// and by `App` when a tab stops being active (the menu belongs to the
    /// visible page, not to the tab forever).
    pub(crate) fn clear_overlay(&mut self) {
        self.selected_all = false;
        self.context_open = false;
        self.context_pos = None;
        self.context_viewport = None;
        self.context_link = None;
        self.secondary_press_intercepted = false;
        self.pending_context_click = None;
        self.link_hits.clear();
        self.doc_rect = None;
        // Find never leaks across documents: a stale query would highlight
        // the wrong page (and its count belongs to that page).
        self.find.reset();
        // The outline sidebar describes the page that was on screen, and its
        // scrollspy cursor belongs to that page's headings.
        self.outline.reset();
        self.save_asked = false;
        // The keyboard focus that owned a bookmark tile belongs to the Home
        // page that was on screen; a stale flag would keep the next page's
        // Alt+Left/Right Back/Forward shortcuts inert.
        self.tile_focused = false;
        // The indicator popup describes the page that was on screen; every
        // navigation and tab switch goes through here, so it can never
        // linger over a different page.
        self.site_info = None;
    }

    /// Marks the address field as user-edited (called on `TextEdit::changed()`).
    /// `poll()` then preserves typing instead of overwriting with the
    /// normalized URL when the load resolves.
    pub fn mark_address_edited(&mut self) {
        self.address_dirty = true;
    }

    /// Shows a transient confirmation (copy), auto-dismissed by the viewer.
    pub fn set_toast(&mut self, text: &str) {
        self.toast_text = Some(text.to_string());
        self.toast_at = Some(Instant::now());
    }

    /// Short title for the tab strip (expanded rail rows / tooltips).
    /// Prefers the page's own title (frontmatter `title`, else the first
    /// `#` heading), then the schemeless host + path.
    /// Never empty: falls back to `New Tab` / `Loading...` / `Error`.
    /// Capped at [`crate::config::TAB_TITLE_CHARS`] so a remote title cannot
    /// bloat the UI. Allocates: the rail's per-frame path goes through
    /// [`Self::display`]'s memo; this stays for tests.
    #[cfg(test)]
    pub fn title(&self) -> String {
        self.compute_title()
    }

    /// Title computation behind [`Self::display`].
    fn compute_title(&self) -> String {
        use crate::config::TAB_TITLE_CHARS;
        match &self.state {
            TabState::Home => "New Tab".to_string(),
            TabState::History => "History".to_string(),
            TabState::Licenses => "Licenses".to_string(),
            TabState::Loading { asked, .. } | TabState::Settling { asked, .. } => {
                let safe = client::sanitize_for_display(asked);
                let stripped = strip_scheme_prefix(&safe);
                let short = truncate_chars(&stripped, TAB_TITLE_CHARS).into_owned();
                if short.is_empty() {
                    "Loading...".to_string()
                } else {
                    short
                }
            }
            TabState::Loaded { fetched } => {
                // Prefer the page's own title (frontmatter `title`, else the
                // first H1); else host + path.
                // Sanitized: no control chars from remote YAML or heading
                // text reach the UI.
                if let Some(t) = fetched.page_title() {
                    let clean = sanitize_title(t);
                    if !clean.is_empty() {
                        return truncate_chars(&clean, TAB_TITLE_CHARS).into_owned();
                    }
                }
                let disp_string = resolver::display_string(&fetched.display_url);
                let disp = truncate_chars(&disp_string, TAB_TITLE_CHARS).into_owned();
                if disp.is_empty() {
                    "Untitled".to_string()
                } else {
                    disp
                }
            }
            TabState::Failed { .. } => "Error".to_string(),
        }
    }

    /// OS window title: the page's own title with no decoration while a
    /// page is open, and just `MDowser` on Home (nothing open).
    /// - Frontmatter `title` when present (sanitized), else the first `#`
    ///   heading, else the schemeless URL, else "Untitled". Loading shows
    ///   the asked URL and failures `Error: <asked>`, so the window still
    ///   identifies the load.
    ///
    /// Allocates: the per-frame window-title comparison goes through
    /// [`Self::window_title_cached`] (the [`TabDisplay`] memo), like the
    /// tab strip's title. Kept for tests.
    #[cfg(test)]
    pub fn window_title(&self) -> String {
        self.compute_window_title()
    }

    /// The OS window title from the display memo: refreshes the memo when
    /// stale and borrows the stored string, so the per-frame title check
    /// allocates nothing.
    pub fn window_title_cached(&mut self) -> &str {
        self.refresh_display();
        self.display_cache
            .as_ref()
            .map_or("MDowser", |(_, _, display)| display.window_title.as_str())
    }

    /// Title computation behind the window-title accessors / the memo.
    fn compute_window_title(&self) -> String {
        use crate::config::{WINDOW_TITLE_CHARS, WINDOW_TITLE_ERROR_URL_CHARS};
        match &self.state {
            TabState::Loaded { fetched } => {
                if let Some(t) = fetched.page_title() {
                    let clean = sanitize_title(t);
                    if !clean.is_empty() {
                        return truncate_chars(&clean, WINDOW_TITLE_CHARS).into_owned();
                    }
                }
                let disp = resolver::display_string(&fetched.display_url);
                if disp.is_empty() {
                    "Untitled".to_string()
                } else {
                    // Bound the OS title like every other branch: an 8 KiB
                    // URL fallback used to reach the window manager whole.
                    truncate_chars(&disp, WINDOW_TITLE_CHARS).into_owned()
                }
            }
            TabState::Loading { asked, .. } | TabState::Settling { asked, .. } => {
                let safe = client::sanitize_for_display(asked);
                let stripped = strip_scheme_prefix(&safe);
                let short = truncate_chars(&stripped, WINDOW_TITLE_CHARS).into_owned();
                if short.is_empty() {
                    "Loading...".to_string()
                } else {
                    short
                }
            }
            TabState::Failed { asked, .. } => {
                let safe = client::sanitize_for_display(asked);
                let stripped = strip_scheme_prefix(&safe);
                let short = truncate_chars(&stripped, WINDOW_TITLE_ERROR_URL_CHARS).into_owned();
                if short.is_empty() {
                    "Error".to_string()
                } else {
                    format!("Error: {short}")
                }
            }
            TabState::Home => "MDowser".to_string(),
            TabState::History => "History".to_string(),
            TabState::Licenses => "Licenses".to_string(),
        }
    }

    /// Can go back? (history exists).
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// Can go forward?
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Origin (`https://host[:port]/`) whose favicon represents this tab.
    /// `None` for Home and for local files (nothing to probe over the
    /// network). Used only by the tab strip; never navigated to.
    ///
    /// Allocates: the rail's per-frame path goes through [`Self::display`]'s
    /// memo; this stays for tests.
    #[cfg(test)]
    pub fn favicon_origin(&self) -> Option<Url> {
        self.compute_favicon_origin()
    }

    /// Origin computation behind [`Self::display`].
    fn compute_favicon_origin(&self) -> Option<Url> {
        match &self.state {
            // Borrow the display URL directly (no clone): only the small
            // origin URL is allocated.
            TabState::Loaded { fetched } => {
                let url = &fetched.display_url;
                if url.scheme() == "file" {
                    return None;
                }
                Some(resolver::server_home_of(url))
            }
            TabState::Loading { asked, .. }
            | TabState::Settling { asked, .. }
            | TabState::Failed { asked, .. } => {
                let url = match client::parse_navigable(asked) {
                    Ok(u) => u,
                    Err(_) => self.current.clone()?,
                };
                if url.scheme() == "file" {
                    return None;
                }
                Some(resolver::server_home_of(&url))
            }
            TabState::Home => None,
            TabState::History | TabState::Licenses => None,
        }
    }

    /// Current page as a bookmark target: `(normalized URL, raw page title)`
    /// when a LOADED document is on screen, where the title is the
    /// frontmatter `title` or the first-H1 fallback
    /// ([`Fetched::page_title`]). `None` on Home /
    /// Loading / History / Licenses / Failed: Ctrl/Cmd+D has no page to
    /// bookmark there. The store sanitizes the title and validates the URL.
    pub fn bookmark_target(&self) -> Option<(Url, Option<String>)> {
        match &self.state {
            TabState::Loaded { fetched } => Some((
                fetched.display_url.clone(),
                fetched.page_title().map(str::to_string),
            )),
            _ => None,
        }
    }

    /// URL of the LOADED document, borrowed: the per-frame toolbar state
    /// (bookmark ribbon) must not clone the URL and title every frame.
    pub fn loaded_url(&self) -> Option<&Url> {
        match &self.state {
            TabState::Loaded { fetched } => Some(&fetched.display_url),
            _ => None,
        }
    }

    /// The stale-copy marker of the LOADED document: `Some` exactly when the
    /// page on screen came from the cache's stale record because no fresh
    /// copy could be obtained ([`Fetched::stale_copy`]). The omnibox
    /// indicator reads it to mark an offline/saved copy instead of a page
    /// loaded just now; `None` while loading, on failures and on a document
    /// that came from the network or a fresh cache hit.
    pub fn stale_copy(&self) -> Option<client::StaleCopy> {
        match &self.state {
            TabState::Loaded { fetched } => fetched.stale_copy,
            _ => None,
        }
    }

    /// True when the tab shows (or is loading) a local file. The omnibox
    /// uses it to show the file indicator instead of the HTTPS padlock.
    pub fn is_local_file(&self) -> bool {
        match &self.state {
            TabState::Loaded { fetched } => fetched.display_url.scheme() == "file",
            TabState::Loading { asked, .. }
            | TabState::Settling { asked, .. }
            | TabState::Failed { asked, .. } => asked
                .get(..5)
                .is_some_and(|p| p.eq_ignore_ascii_case("file:")),
            TabState::Home | TabState::History | TabState::Licenses => false,
        }
    }

    /// True when the omnibox security-indicator popup is open for this
    /// tab. `App::handle_shortcuts` reads it before the UI runs, so Esc
    /// closes the popup instead of stopping the load (the same
    /// last-frame race as `address_focused`).
    pub fn site_info_open(&self) -> bool {
        self.site_info.is_some()
    }

    /// Downloading or settling now? (disables duplicate navigation).
    pub fn is_loading(&self) -> bool {
        matches!(
            self.state,
            TabState::Loading { .. } | TabState::Settling { .. }
        )
    }

    /// Can press "Refresh"? Needs a current document + 5s since last navigation.
    /// - Always false during `Loading`/`Settling` (prevents duplicate fetches).
    /// - Always false on `Home`: there is nothing to reload (re-rendering
    ///   the landing page needs no network); `about:history`/`about:licenses`
    ///   keep the shared internal-page behavior and stay refreshable.
    pub fn can_refresh(&self) -> bool {
        if self.is_loading() {
            return false;
        }
        if matches!(self.state, TabState::Home) {
            return false;
        }
        if self.current.is_none() {
            return false;
        }
        match self.last_navigate {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(config::REFRESH_COOLDOWN_SECS),
        }
    }

    /// Can press "Retry" on the error view?
    /// - Same 5s cooldown as Refresh (anti-hammer): a failing server must
    ///   not be retried in a tight loop by holding Enter.
    /// - Unlike Refresh, no current document is required (fresh failures
    ///   have nothing loaded yet).
    pub fn can_retry(&self) -> bool {
        if self.is_loading() {
            return false;
        }
        match self.last_navigate {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(config::REFRESH_COOLDOWN_SECS),
        }
    }

    /// Cooldown seconds left (0 when refresh is allowed).
    pub fn refresh_remaining(&self) -> u64 {
        match self.last_navigate {
            None => 0,
            Some(t) => {
                let need = Duration::from_secs(config::REFRESH_COOLDOWN_SECS);
                match need.checked_sub(t.elapsed()) {
                    // Ceil to next second (1.1s left -> 2, 0.1s -> 1).
                    Some(r) => r.as_millis().div_ceil(1000) as u64,
                    None => 0,
                }
            }
        }
    }

    /// Shared restore for a settled-but-failed navigation (cancelled or
    /// error): history snapshots back, address schemeless unless the user
    /// typed meanwhile, timers/images/anchors reset. The caller then sets
    /// `Failed` with its own lines. Extracted so the cancelled vs error arms
    /// cannot drift (they previously duplicated six statements).
    fn restore_failed_navigation(
        &mut self,
        saved_back: Vec<url::Url>,
        saved_forward: Vec<url::Url>,
        pushed_history: bool,
        asked: &str,
        address_at_start: &str,
    ) {
        self.cancel = None;
        if !pushed_history {
            self.back = saved_back;
        }
        self.forward = saved_forward;
        if !self.address_dirty && self.address == address_at_start {
            self.address = strip_scheme_prefix(asked).into_owned();
        }
        self.address_dirty = false;
        self.last_navigate = Some(Instant::now());
        self.images.cancel_all();
        self.pending_anchor = None;
        // The failed navigation never shows a document: its top-reset must
        // not leak into whatever page renders next (Back included).
        self.scroll_top_requested = false;
    }

    /// URL the session snapshot should remember for this tab: the current
    /// page when it is not the Home marker, else the last real navigation
    /// target (a still-loading or failed restore), else `None` for Home.
    pub fn session_url(&self) -> Option<&Url> {
        match self.current.as_ref() {
            Some(current) if !crate::browser::tab::is_home_url(current) => Some(current),
            _ => self.session_target.as_ref(),
        }
    }

    /// Is a refresh/retry cooldown countdown still running?
    ///
    /// Used by `App::logic` to schedule the 500 ms repaint ticks that keep
    /// the countdown tooltip/label fresh. `!can_refresh()` was NOT a valid
    /// test: it is also false on Home and on fresh errors (no `current`),
    /// which made those pages repaint every 500 ms forever.
    pub fn cooldown_pending(&self) -> bool {
        self.last_navigate
            .is_some_and(|t| t.elapsed() < Duration::from_secs(config::REFRESH_COOLDOWN_SECS))
    }
}

/// Navigation, history-stack and lifecycle methods, split by concern
/// purely for readability: they are all `impl Tab` (same type, same
/// private state, one module tree).
mod info;
mod nav;

mod nav_moves;
mod poll;
mod state;
pub use info::SiteInfo;
pub use state::{TabState, TabStateKind};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_pages;
