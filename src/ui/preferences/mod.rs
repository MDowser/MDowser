// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::preferences`: the floating Preferences dialog.
//!
//! - `Preferences`: a FIXED-SIZE, card-style dialog (custom header, tab bar,
//!   section cards, list rows) sized to its disarmed content. Three tabs:
//!   General (appearance + Updates card + version/licenses footer), Data
//!   (history retention with the destructive "Delete history" action,
//!   per-class encrypted-cache clears, plus the "Delete all data and close"
//!   card) and Shortcuts (read-only keyboard shortcut reference in English).
//!   The HISTORY LIST is not here: it is the internal
//!   `about:history` page (`ui::history_page`), rendered in a tab like a
//!   document. This module keeps only the Preferences overlay: `App` owns
//!   its visibility; the third-party licenses page (`about:licenses`) is
//!   opened by `App` (this module never navigates).
//!
//! STABLE HOVER (fixed bug): every interactive row keeps the SAME geometry
//! in all visual states. The egui default `selectable_label` only grew its
//! frame (and inner padding) on hover, which shifted the text and pushed the
//! whole list. The dropdown rows here are painted into a fixed rect instead,
//! and the window itself has no title bar, a fixed width and a custom header
//! (the height follows the content; see `WIN_W`).
//!
//! FIT CONTENT: the dialog is sized to the disarmed card stack, and each
//! delete-confirmation note only takes space while armed. Arming a delete
//! may push the cards below it (the inner scroll appears when the viewport
//! cannot fit the armed stack).
//!
//! Nothing here navigates (the viewer/tab rules live in `App`), and every
//! remote string is already sanitized/bounded by `history::HistoryStore`.

use crate::config;
use crate::history::{HistoryStore, Retention};
use crate::settings::{Prefs, ThemePref};
use crate::ui::{icons, theme};
use crate::update::{CheckFailure, UpdateView};
use eframe::egui;
use std::time::{Duration, Instant};

/// Outer dialog width. Fixed on purpose so hover never resizes it; the
/// HEIGHT is content-sized (see `show_prefs`), so the disarmed stack hugs
/// the footer instead of leaving an empty band, and the inner scroll covers
/// a viewport too short for the armed stack.
const WIN_W: f32 = 480.0;
/// Smallest dialog height: a sane first frame on a platform that reports no
/// viewport yet; real content takes over from the first laid-out frame.
const WIN_MIN_H: f32 = 240.0;
/// Fixed distance from the window top to the dialog's top edge.
const TOP_MARGIN: f32 = 48.0;
/// Dialog corner radius (extra-large shape).
const RADIUS: u8 = 14;
/// Body horizontal padding.
const PAD: f32 = 18.0;
/// Card inner padding.
const CARD_PAD: f32 = 12.0;
/// One setting row (label + control).
const ROW_H: f32 = 44.0;
/// Dropdown field width.
const COMBO_W: f32 = 168.0;
/// Height of one dropdown popup row (fixed, so hover never moves it).
const DROPDOWN_ROW_H: f32 = crate::config::CHROME_DROPDOWN_ROW_H;
/// Toggle switch geometry.
const SWITCH_W: f32 = 42.0;
const SWITCH_H: f32 = 24.0;
/// Seconds the "Delete history" action stays armed.
const CLEAR_CONFIRM_SECS: u64 = config::DELETE_CONFIRM_SECS;
/// Seconds the "Delete all data and close" action stays armed.
const WIPE_CONFIRM_SECS: u64 = config::DELETE_CONFIRM_SECS;

/// Visibility + transient state of the floating Preferences dialog.
pub struct PreferencesState {
    /// Preferences dialog visible.
    pub prefs_open: bool,
    /// Active tab (kept across opens; never persisted to disk).
    selected: PreferencesTab,
    /// Deadline of the armed "Delete history" confirmation.
    clear_confirm: Option<Instant>,
    /// Deadlines of the armed per-class cache clears ("Clear documents" /
    /// "Clear images" / "Clear favicons").
    clear_docs_confirm: Option<Instant>,
    clear_images_confirm: Option<Instant>,
    clear_favicons_confirm: Option<Instant>,
    /// Deadline of the armed "Delete all data and close" confirmation.
    wipe_confirm: Option<Instant>,
}

/// One tab of the Preferences dialog.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreferencesTab {
    /// Appearance + Updates card + version/licenses footer.
    #[default]
    General,
    /// History retention + both destructive actions.
    Data,
    /// Read-only keyboard shortcut reference (English).
    Shortcuts,
}

impl PreferencesTab {
    /// All tabs in bar order.
    const ALL: [Self; 3] = [Self::General, Self::Data, Self::Shortcuts];

    /// Tab bar label.
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Data => "Data",
            Self::Shortcuts => "Shortcuts",
        }
    }
}

/// Grouped keyboard shortcut reference (English): `(group, &[(action, keys)])`.
///
/// Single source for the Shortcuts tab: a CURATED quick reference, not the
/// full key map (the complete list lives in `docs/INTERFACE.md`). The bare
/// `Esc` rows (stop/close) and the keyboard reorders (`Alt`+arrows on a
/// focused tab or tile) are deliberately left out: they are obvious or niche,
/// and every row here should be a key a user would not guess. Mirrors
/// `app::shortcuts` (`handle_shortcuts`), the find bar stepping
/// (`viewer::overlays::find`) and the macOS menu accelerators (`app::menu`).
const SHORTCUT_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "TABS",
        &[
            ("New tab", "Ctrl/Cmd + T"),
            ("Close tab", "Ctrl/Cmd + W"),
            ("Reopen closed tab", "Ctrl/Cmd + Shift + T"),
            ("Next / previous tab", "Ctrl + Tab / Ctrl + Shift + Tab"),
            ("Select tab 1-9", "Ctrl/Cmd + 1-9"),
        ],
    ),
    (
        "NAVIGATION",
        &[
            ("Back / forward", "Alt + Left / Right"),
            ("Focus address field", "Ctrl/Cmd + L"),
            ("Reload page", "Ctrl/Cmd + R / F5"),
            ("Open history", "Ctrl + H / Cmd + Y"),
            ("Bookmark this page", "Ctrl/Cmd + D"),
            ("Open preferences", "Ctrl/Cmd + ,"),
        ],
    ),
    (
        "PAGE",
        &[
            ("Find in page", "Ctrl/Cmd + F"),
            (
                "Next / previous match",
                "Enter / Shift+Enter (F3 / Shift+F3)",
            ),
            ("Save page as Markdown", "Ctrl/Cmd + S"),
            ("Select all page text", "Ctrl/Cmd + A"),
            ("Copy selection or page", "Ctrl/Cmd + C"),
        ],
    ),
    (
        "ZOOM",
        &[
            ("Zoom in", "Ctrl/Cmd + + / ="),
            ("Zoom out", "Ctrl/Cmd + -"),
            ("Reset zoom to 100%", "Ctrl/Cmd + 0"),
        ],
    ),
];

/// Side effects of one Preferences frame, performed by `App` (this module
/// never touches tabs, navigation or the network itself).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PrefsActions {
    /// The user changed the startup zoom; `App` applies it to the live view.
    pub zoom: Option<f32>,
    /// The user asked for the internal third-party licenses page
    /// (`about:licenses`, opened by `App` in a new tab).
    pub open_licenses: bool,
    /// The user confirmed "Delete all data and close": `App` wipes the data
    /// directory and exits immediately without further saves.
    pub wipe_all: bool,
    /// The user pressed "Check now" on the Updates card.
    pub check_now: bool,
    /// The user pressed "Open release page": `App` opens the official GitHub
    /// release in the system browser (this module never navigates).
    pub open_release: bool,
}

impl PreferencesState {
    /// Dialog closed.
    pub fn new() -> Self {
        Self {
            prefs_open: false,
            selected: PreferencesTab::General,
            clear_confirm: None,
            clear_docs_confirm: None,
            clear_images_confirm: None,
            clear_favicons_confirm: None,
            wipe_confirm: None,
        }
    }

    /// Is the floating dialog visible? (Esc closes it first.)
    pub fn any_open(&self) -> bool {
        self.prefs_open
    }

    /// Preferences entry point: show the dialog or hide it.
    pub fn toggle_prefs(&mut self) {
        self.prefs_open = !self.prefs_open;
    }

    /// Opens the dialog on its General tab, where the Updates card lives
    /// (gear dot / macOS "Check for Updates…").
    pub fn open_updates(&mut self) {
        self.prefs_open = true;
        self.selected = PreferencesTab::General;
    }

    /// Closes the dialog (Esc while it owns the keyboard).
    pub fn close_windows(&mut self) {
        self.prefs_open = false;
        self.clear_confirm = None;
        self.clear_docs_confirm = None;
        self.clear_images_confirm = None;
        self.clear_favicons_confirm = None;
        self.wipe_confirm = None;
    }

    /// Draws the Preferences dialog when open. Returns the side effects `App`
    /// must perform (apply a new zoom, open the licenses page).
    ///
    /// Test/headless entry point: the live app uses
    /// [`Self::show_with_update`] so the in-flight check and its failure are
    /// visible.
    #[cfg(test)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
    ) -> PrefsActions {
        let update = UpdateView::from_prefs(prefs);
        self.show_with_update(ctx, store, prefs, &update)
    }

    /// The full entry point, with the live updater snapshot (in-flight check
    /// and a session-only failure). The test-only `show` wrapper derives that
    /// snapshot from persisted prefs.
    pub fn show_with_update(
        &mut self,
        ctx: &egui::Context,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
        update: &UpdateView,
    ) -> PrefsActions {
        if self.prefs_open {
            self.show_prefs(ctx, store, prefs, update)
        } else {
            PrefsActions::default()
        }
    }

    /// Preferences dialog with tabs: General, Data and Shortcuts.
    fn show_prefs(
        &mut self,
        ctx: &egui::Context,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
        update: &UpdateView,
    ) -> PrefsActions {
        let mut open = self.prefs_open;
        let mut selected = self.selected;
        let mut confirm_until = self.clear_confirm;
        let mut docs_until = self.clear_docs_confirm;
        let mut images_until = self.clear_images_confirm;
        let mut favicons_until = self.clear_favicons_confirm;
        let mut wipe_until = self.wipe_confirm;
        let mut applied_zoom = None;
        let mut open_licenses = false;
        let mut wipe_all = false;
        let mut check_now = false;
        let mut open_release = false;
        let dark = ctx.theme() == egui::Theme::Dark;
        // Interned palette reference, not a per-frame ~50-field rebuild.
        let s = theme::scheme_ref_ctx(ctx);
        // Borderless dialog: the shadow and rounded surface come from a
        // custom frame, and the header below carries the title + close.
        let frame = egui::Frame::new()
            .fill(s.surface)
            .corner_radius(egui::CornerRadius::same(RADIUS))
            .shadow(theme::dialog_shadow(dark))
            .inner_margin(egui::Margin::same(0));
        // A platform that reports no viewport (headless tests) must not
        // clamp the dialog: `auto_sized` then hugs the content exactly, and
        // egui itself still constrains the window to the screen area.
        let viewport = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        let win_w = viewport.map_or(WIN_W, |v| WIN_W.min(v.x * 0.92));
        let win_max_h = viewport.map_or(f32::INFINITY, |v| (v.y * 0.9).max(WIN_MIN_H));
        egui::Window::new("Preferences")
            .title_bar(false)
            .collapsible(false)
            .auto_sized()
            .min_width(win_w)
            .max_width(win_w)
            .max_height(win_max_h)
            // TOP-anchored on purpose: the dialog is content-sized, and a
            // centered anchor moved every row UP when an armed confirmation
            // (or a persistence warning) grew the stack — the second click
            // of a two-click confirm must land on a control that did not
            // move. Growth is downward-only from a fixed top margin.
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, TOP_MARGIN))
            .frame(frame)
            .show(ctx, |ui| {
                // No inter-widget gaps: the header/tab/scroll stack stays
                // flush, so the title keeps equal air above and below
                // (the theme's 8px item spacing would otherwise pile two
                // gaps plus the body padding under the title).
                ui.spacing_mut().item_spacing.y = 0.0;
                dialog_header(ui, s, &mut open);
                if let Some(tab) = tab_bar(ui, s, selected) {
                    selected = tab;
                }
                // Per-tab scroll id: each tab keeps its own offset (a shared
                // one handed the scrolled offset to whichever tab came next,
                // which egui then clamped to that tab's content).
                egui::ScrollArea::vertical()
                    .id_salt(("prefs-scroll", selected.label()))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: PAD as i8,
                                right: PAD as i8,
                                top: 10,
                                bottom: 16,
                            })
                            .show(ui, |ui| match selected {
                                PreferencesTab::General => {
                                    Self::render_general(
                                        ui,
                                        store,
                                        prefs,
                                        update,
                                        &mut applied_zoom,
                                        &mut open,
                                        &mut open_licenses,
                                        &mut check_now,
                                        &mut open_release,
                                    );
                                }
                                PreferencesTab::Data => {
                                    Self::render_data(
                                        ui,
                                        ctx,
                                        s,
                                        store,
                                        prefs,
                                        &mut confirm_until,
                                        &mut docs_until,
                                        &mut images_until,
                                        &mut favicons_until,
                                        &mut wipe_until,
                                        &mut wipe_all,
                                    );
                                }
                                PreferencesTab::Shortcuts => {
                                    Self::render_shortcuts(ui, s);
                                }
                            });
                    });
            });
        self.prefs_open = open;
        self.selected = selected;
        // Closing the dialog (its X or Esc) disarms pending deletes: the
        // two-click confirmations must not survive into a later visit. An
        // ELAPSED deadline is dropped too (keeping a stale `Some(Instant)`
        // would re-arm the label on a later visit without a new click).
        let live = |until: Option<Instant>| until.filter(|at| *at > Instant::now());
        self.clear_confirm = if open { live(confirm_until) } else { None };
        self.clear_docs_confirm = if open { live(docs_until) } else { None };
        self.clear_images_confirm = if open { live(images_until) } else { None };
        self.clear_favicons_confirm = if open { live(favicons_until) } else { None };
        self.wipe_confirm = if open { live(wipe_until) } else { None };
        PrefsActions {
            zoom: applied_zoom,
            open_licenses,
            wipe_all,
            check_now,
            open_release,
        }
    }

    /// General tab: appearance + updates cards and the compact
    /// version/licenses footer.
    #[allow(clippy::too_many_arguments)]
    fn render_general(
        ui: &mut egui::Ui,
        store: &HistoryStore,
        prefs: &mut Prefs,
        update: &UpdateView,
        applied_zoom: &mut Option<f32>,
        open: &mut bool,
        open_licenses: &mut bool,
        check_now: &mut bool,
        open_release: &mut bool,
    ) {
        let s = theme::scheme_ref(ui);
        card(ui, s, |ui| {
            section_title(ui, s, "APPEARANCE");
            setting_row(ui, s, "Theme", |ui| {
                if let Some(theme_pref) = dropdown(
                    ui,
                    s,
                    "prefs-theme",
                    "Theme",
                    prefs.theme().label(),
                    &ThemePref::options(),
                    prefs.theme(),
                ) {
                    prefs.set_theme(theme_pref);
                    prefs.apply_theme(ui.ctx());
                }
            });
            divider(ui, s);
            setting_row(ui, s, "Default zoom", |ui| {
                // Same `- % +` capsule as the toolbar (clicking the readout
                // resets to 100%), stepping like it instead of a slider.
                let mut zoom = prefs.default_zoom();
                crate::ui::navbar::default_zoom_control(ui, &mut zoom);
                if (zoom - prefs.default_zoom()).abs() > f32::EPSILON {
                    prefs.set_default_zoom(zoom);
                    *applied_zoom = Some(prefs.default_zoom());
                }
            });
            divider(ui, s);
            setting_row(ui, s, "Restore tabs on startup", |ui| {
                let mut restore = prefs.restore_tabs();
                if switch(ui, s, "Restore tabs on startup", &mut restore).changed() {
                    prefs.set_restore_tabs(restore);
                }
            });
            if store.forgets_on_exit() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "Off while history is set to Forget on exit: tab URLs are never written to disk.",
                        )
                        .size(theme::typography::UI_SMALL)
                        .color(s.on_surface_variant),
                    )
                    .selectable(false),
                );
            }
            // A failed delete of the encrypted session file must not pass
            // silently.
            if let Some(error) = prefs.session_delete_error() {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(error)
                            .size(theme::typography::UI_SMALL)
                            .color(s.error),
                    )
                    .truncate()
                    .selectable(false),
                );
                // Truncated at dialog width: the full OS message must stay
                // readable.
                crate::ui::hover_tooltip_lazy(resp, || error.to_owned());
            }
            // Same for a session snapshot that could not be written on the
            // worker: restore would reopen a stale tab list.
            if let Some(error) = prefs.session_save_error() {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(error)
                            .size(theme::typography::UI_SMALL)
                            .color(s.error),
                    )
                    .truncate()
                    .selectable(false),
                );
                crate::ui::hover_tooltip_lazy(resp, || error.to_owned());
            }
            // Same for a preference that could not be persisted (it would
            // silently "revert" on the next launch).
            if let Some(error) = prefs.settings_error() {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(error)
                            .size(theme::typography::UI_SMALL)
                            .color(s.error),
                    )
                    .truncate()
                    .selectable(false),
                );
                crate::ui::hover_tooltip_lazy(resp, || error.to_owned());
            }
        });

        ui.add_space(theme::spacing::M);

        // ---- Updates (notification only) ------------------------------
        // The card reports the background check; "Open release page" hands
        // the official GitHub release to the system browser. Nothing is
        // downloaded or installed by the app.
        card(ui, s, |ui| {
            // The dialog zeroes the inter-widget gap so the header stays
            // flush; this card lays out its text block with explicit air
            // instead of letting the status, caption and actions collide.
            ui.spacing_mut().item_spacing.y = 0.0;
            section_title(ui, s, "UPDATES");
            ui.add_space(theme::spacing::S);
            let status_color = if update.available.is_some() {
                s.primary
            } else if update.failure.is_some_and(CheckFailure::is_error) && !update.retrying {
                // A failed check is informational for this notification-only
                // feature: amber, not the red used for real errors.
                s.alert_warning
            } else {
                s.on_surface
            };
            ui.label(
                egui::RichText::new(update.status_text())
                    .size(theme::typography::STATUS)
                    .color(status_color),
            );
            let caption = if update.enabled {
                "Checks GitHub once a week. Updates are never downloaded or installed automatically."
            } else {
                "Automatic checks are off. Check now still works; updates are never installed automatically."
            };
            ui.add_space(theme::spacing::XS);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(caption)
                        .size(theme::typography::UI_SMALL)
                        .line_height(Some(theme::typography::UI_LINE_HEIGHT))
                        .color(s.on_surface_variant),
                )
                .selectable(false),
            );
            ui.add_space(theme::spacing::L);
            ui.horizontal(|ui| {
                if update.checking {
                    ui.label(theme::weak(ui, "Checking…"));
                } else {
                    let (label, tooltip) = if update.failure.is_some() {
                        ("Try again", "Ask GitHub again now")
                    } else {
                        ("Check now", "Ask GitHub for a newer release now")
                    };
                    if text_link(ui, s, label, tooltip).clicked() {
                        *check_now = true;
                    }
                }
                if update.available.is_some() {
                    ui.label(theme::weak(ui, "·"));
                    if text_link(
                        ui,
                        s,
                        "Open release page",
                        "Open the official GitHub release in your browser",
                    )
                    .clicked()
                    {
                        *open_release = true;
                    }
                }
            });
            ui.add_space(theme::spacing::M);
            divider(ui, s);
            setting_row(ui, s, "Check automatically", |ui| {
                let mut enabled = update.enabled;
                if switch(ui, s, "Check automatically", &mut enabled).changed() {
                    prefs.set_check_updates(enabled);
                }
            });
        });

        ui.add_space(theme::spacing::M);

        // ---- Version (compact) ----------------------------------------
        // ONE left-to-right line on purpose: version and license identity
        // stay visible without a whole card; the link opens the embedded
        // `THIRD_PARTY_NOTICES.md`.
        ui.horizontal(|ui| {
            ui.label(theme::weak(
                ui,
                concat!("MDowser ", env!("CARGO_PKG_VERSION"), " · MPL-2.0"),
            ));
            ui.label(theme::weak(ui, "·"));
            let link = text_link(
                ui,
                s,
                "View third-party licenses",
                "Licenses of the bundled crates and fonts",
            );
            if link.clicked() {
                // Also dismiss the dialog: `App` opens the page in a new
                // tab, and a floating Preferences window would otherwise sit
                // on top of it.
                *open_licenses = true;
                *open = false;
            }
        });
    }

    /// Data tab: history retention + delete, the per-class cache clears,
    /// and the wipe-everything card.
    #[allow(clippy::too_many_arguments)]
    fn render_data(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        s: &theme::Scheme,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
        confirm_until: &mut Option<Instant>,
        docs_until: &mut Option<Instant>,
        images_until: &mut Option<Instant>,
        favicons_until: &mut Option<Instant>,
        wipe_until: &mut Option<Instant>,
        wipe_all: &mut bool,
    ) {
        // ---- History --------------------------------------------------
        card(ui, s, |ui| {
            // Tight fit: no trailing reserved space, so the card hugs its
            // rows when disarmed.
            ui.spacing_mut().item_spacing.y = 4.0;
            section_title(ui, s, "HISTORY");
            setting_row(ui, s, "Keep visits", |ui| {
                let current = store.retention();
                let selected = Retention::options()
                    .iter()
                    .find(|(retention, _)| *retention == current)
                    .map_or("Keep 3 months (default)", |(_, label)| *label);
                if let Some(retention) = dropdown(
                    ui,
                    s,
                    "prefs-retention",
                    "Keep visits",
                    selected,
                    &Retention::options(),
                    current,
                ) {
                    store.set_retention(retention);
                    // "Forget on exit" extends to the encrypted session
                    // snapshot: restore-tabs is turned off (and any leftover
                    // file deleted) so the privacy promise holds.
                    if store.forgets_on_exit() {
                        prefs.set_restore_tabs(false);
                    }
                }
            });
            history_caption(ui, s, store);
            divider(ui, s);
            let now = Instant::now();
            let confirming = confirm_until.is_some_and(|until| until > now);
            let label = if confirming {
                "Confirm delete"
            } else {
                "Delete history"
            };
            if danger_row(ui, s, label, icons::Icon::Delete).clicked() {
                if confirming {
                    store.clear();
                    *confirm_until = None;
                } else {
                    // Shared fail-closed deadline helper (panic-free
                    // `Instant` arithmetic).
                    *confirm_until = Some(crate::config::deadline_after(CLEAR_CONFIRM_SECS));
                }
            }
            // Fit content: the note only takes space while armed, pushing
            // the cards below.
            let armed = confirm_until.is_some_and(|until| until > Instant::now());
            confirm_note(ui, armed, "Click again to delete every visit.");
            if armed {
                // Keep the armed label from going stale.
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        });

        ui.add_space(theme::spacing::L);

        // ---- Cache ---------------------------------------------------
        // Encrypted HTTP disk cache (documents, images, favicons): each
        // class clears separately, two clicks like every other destructive
        // action. Sizes come from the live in-memory accounting (no disk
        // walk per frame).
        card(ui, s, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            section_title(ui, s, "CACHE");
            cache_caption(ui, s);
            divider(ui, s);
            let stats = crate::cache::global().map(|c| c.stats());
            let docs_armed = cache_clear_row(
                ui,
                s,
                stats.as_ref(),
                docs_until,
                &CacheClearSpec {
                    noun: "documents",
                    unit: "pages",
                    size: |st| st.doc_bytes,
                    count: |st| st.doc_entries,
                    clear: crate::cache::CacheManager::clear_docs,
                },
            );
            divider(ui, s);
            let images_armed = cache_clear_row(
                ui,
                s,
                stats.as_ref(),
                images_until,
                &CacheClearSpec {
                    noun: "images",
                    unit: "files",
                    size: |st| st.image_bytes,
                    count: |st| st.image_entries,
                    clear: crate::cache::CacheManager::clear_images,
                },
            );
            divider(ui, s);
            let favicons_armed = cache_clear_row(
                ui,
                s,
                stats.as_ref(),
                favicons_until,
                &CacheClearSpec {
                    noun: "favicons",
                    unit: "icons",
                    size: |st| st.favicon_bytes,
                    count: |st| st.favicon_entries,
                    clear: crate::cache::CacheManager::clear_favicons,
                },
            );
            if docs_armed || images_armed || favicons_armed {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        });

        ui.add_space(theme::spacing::L);

        // ---- Local data ------------------------------------------------
        // "Delete all data and close": removes the whole data directory
        // (history, bookmarks, settings, session and key) and exits without
        // saving anything else. Two clicks like every other destructive
        // action.
        card(ui, s, |ui| {
            // Tight fit like the History card above.
            ui.spacing_mut().item_spacing.y = 4.0;
            section_title(ui, s, "LOCAL DATA");
            let now = Instant::now();
            let wiping = wipe_until.is_some_and(|until| until > now);
            let wipe_label = if wiping {
                "Confirm delete and close"
            } else {
                "Delete all data and close"
            };
            if danger_row(ui, s, wipe_label, icons::Icon::Delete).clicked() {
                if wiping {
                    *wipe_until = None;
                    *wipe_all = true;
                } else {
                    *wipe_until = Some(crate::config::deadline_after(WIPE_CONFIRM_SECS));
                }
            }
            let wipe_armed = wipe_until.is_some_and(|until| until > Instant::now());
            confirm_note(ui, wipe_armed, "Click again to delete all data and close.");
            // A wipe that did not go through keeps the app open: say so
            // instead of exiting as if the data were gone.
            if let Some(error) = prefs.wipe_error() {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(error)
                            .size(theme::typography::UI_SMALL)
                            .color(s.error),
                    )
                    .truncate()
                    .selectable(false),
                );
                crate::ui::hover_tooltip_lazy(resp, || error.to_owned());
            }
            if wipe_armed {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        });
    }

    /// Shortcuts tab: read-only keyboard shortcut reference (English).
    fn render_shortcuts(ui: &mut egui::Ui, s: &theme::Scheme) {
        card(ui, s, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            section_title(ui, s, "KEYBOARD SHORTCUTS");
            // A beat of air above the first group: the title and the group
            // label would otherwise sit as close as two rows inside a group.
            ui.add_space(theme::spacing::L);
            for (gi, (group, rows)) in SHORTCUT_GROUPS.iter().enumerate() {
                if gi > 0 {
                    divider(ui, s);
                }
                ui.label(
                    theme::strong(egui::RichText::new(*group).size(theme::typography::UI_SMALL))
                        .color(s.on_surface_variant),
                );
                for (action, keys) in *rows {
                    shortcut_row(ui, s, action, keys);
                }
            }
            ui.add_space(theme::spacing::S);
            ui.label(theme::weak(
                ui,
                "Ctrl/Cmd means Ctrl on Windows/Linux, Cmd on macOS.",
            ));
        });
    }
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self::new()
    }
}

mod widgets;

use widgets::*;

#[cfg(test)]
mod tests;
