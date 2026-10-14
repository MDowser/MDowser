// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::preferences`: the floating Preferences dialog.
//!
//! - `Preferences`: a FIXED-SIZE, Material-style dialog (custom header,
//!   section cards, list rows) sized to its disarmed content: the History
//!   and Local data cards fit their rows with no reserved bottom space.
//!   History retention and the destructive "Delete history" action live in
//!   the History card; theme, startup zoom and "restore tabs on startup" in
//!   the Appearance card; "Delete all data and close" (wipes every local
//!   file and exits) in the Local data card; a compact footer line carries
//!   the version/own license and the link to the internal third-party
//!   licenses page (`about:licenses`, opened by `App`; this module never
//!   navigates).
//! - The HISTORY LIST is not here: it is the internal `about:history` page
//!   (`ui::history_page`), rendered in a tab like a document. This module
//!   keeps only the Preferences overlay: `App` owns its visibility.
//!
//! STABLE HOVER (fixed bug): every interactive row keeps the SAME geometry
//! in all visual states. The egui default `selectable_label` only grew its
//! frame (and inner padding) on hover, which shifted the text and pushed the
//! whole list. The dropdown rows here are painted into a fixed rect instead,
//! and the window itself has no title bar, a fixed size and a custom header.
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
/// Dialog corner radius (Material extra-large shape).
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
    /// Deadline of the armed "Delete history" confirmation.
    clear_confirm: Option<Instant>,
    /// Deadline of the armed "Delete all data and close" confirmation.
    wipe_confirm: Option<Instant>,
}

/// Side effects of one Preferences frame, performed by `App` (this module
/// never touches tabs or navigation).
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
}

impl PreferencesState {
    /// Dialog closed.
    pub fn new() -> Self {
        Self {
            prefs_open: false,
            clear_confirm: None,
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

    /// Closes the dialog (Esc while it owns the keyboard).
    pub fn close_windows(&mut self) {
        self.prefs_open = false;
        self.clear_confirm = None;
        self.wipe_confirm = None;
    }

    /// Draws the Preferences dialog when open. Returns the side effects `App`
    /// must perform (apply a new zoom, open the licenses page).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
    ) -> PrefsActions {
        if self.prefs_open {
            self.show_prefs(ctx, store, prefs)
        } else {
            PrefsActions::default()
        }
    }

    /// Preferences: history, appearance and privacy.
    fn show_prefs(
        &mut self,
        ctx: &egui::Context,
        store: &mut HistoryStore,
        prefs: &mut Prefs,
    ) -> PrefsActions {
        let mut open = self.prefs_open;
        let mut confirm_until = self.clear_confirm;
        let mut wipe_until = self.wipe_confirm;
        let mut applied_zoom = None;
        let mut open_licenses = false;
        let mut wipe_all = false;
        let dark = ctx.theme() == egui::Theme::Dark;
        // Interned palette reference, not a per-frame ~50-field rebuild.
        let s = theme::scheme_ref_ctx(ctx);
        // Borderless dialog: the shadow and rounded surface come from a
        // custom frame, and the header below carries the title + close.
        let frame = egui::Frame::new()
            .fill(s.surface)
            .corner_radius(egui::CornerRadius::same(RADIUS))
            .shadow(if dark {
                egui::Shadow::NONE
            } else {
                egui::Shadow {
                    offset: [0, 8],
                    blur: 28,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(40),
                }
            })
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
                // No inter-widget gaps: the header/divider/scroll stack
                // flush, so the title keeps equal air above and below
                // (the theme's 8px item spacing would otherwise pile two
                // gaps plus the body padding under the title).
                ui.spacing_mut().item_spacing.y = 0.0;
                dialog_header(ui, s, &mut open);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: PAD as i8,
                                right: PAD as i8,
                                top: 10,
                                bottom: 16,
                            })
                            .show(ui, |ui| {
                                // ---- History -------------------------------------------
                                card(ui, s, |ui| {
                                    // Tight fit: no trailing reserved space,
                                    // so the card hugs its rows when disarmed.
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    section_title(ui, s, "HISTORY");
                                    setting_row(ui, s, "Keep visits", |ui| {
                                        let current = store.retention();
                                        let selected = Retention::options()
                                            .iter()
                                            .find(|(retention, _)| *retention == current)
                                            .map(|(_, label)| *label)
                                            .unwrap_or("Keep 3 months (default)");
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
                                            // "Forget on exit" extends to the
                                            // encrypted session snapshot:
                                            // restore-tabs is turned off (and
                                            // any leftover file deleted) so
                                            // the privacy promise holds.
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
                                            confirm_until = None;
                                        } else {
                                            // Shared fail-closed deadline helper
                                            // (panic-free `Instant` arithmetic).
                                            confirm_until = Some(crate::config::deadline_after(
                                                CLEAR_CONFIRM_SECS,
                                            ));
                                        }
                                    }
                                    // Fit content: the note only takes space
                                    // while armed, pushing the cards below.
                                    let armed =
                                        confirm_until.is_some_and(|until| until > Instant::now());
                                    confirm_note(
                                        ui,
                                        armed,
                                        "Click again to delete every visit.",
                                    );
                                    if armed {
                                        // Keep the armed label from going stale.
                                        ctx.request_repaint_after(Duration::from_millis(250));
                                    }
                                });

                                ui.add_space(theme::spacing::L);

                                // ---- Appearance ----------------------------------------
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
                                        // Same `- % +` capsule as the toolbar
                                        // (clicking the readout resets to
                                        // 100%), stepping like it instead of
                                        // a slider.
                                        let mut zoom = prefs.default_zoom();
                                        crate::ui::navbar::default_zoom_control(ui, &mut zoom);
                                        if (zoom - prefs.default_zoom()).abs() > f32::EPSILON {
                                            prefs.set_default_zoom(zoom);
                                            applied_zoom = Some(prefs.default_zoom());
                                        }
                                    });
                                    divider(ui, s);
                                    setting_row(ui, s, "Restore tabs on startup", |ui| {
                                        let mut restore = prefs.restore_tabs();
                                        if switch(ui, s, "Restore tabs on startup", &mut restore)
                                            .changed()
                                        {
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
                                    // A failed delete of the encrypted session
                                    // file must not pass silently.
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
                                        // Truncated at dialog width: the full
                                        // OS message must stay readable.
                                        crate::ui::hover_tooltip_lazy(resp, || error.to_owned());
                                    }
                                    // Same for a session snapshot that could
                                    // not be written on the worker: restore
                                    // would reopen a stale tab list.
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
                                    // Same for a preference that could not be
                                    // persisted (it would silently "revert"
                                    // on the next launch).
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

                                ui.add_space(theme::spacing::L);

                                // ---- Local data ------------------------------------
                                // "Delete all data and close": removes the whole
                                // data directory (history, bookmarks, settings,
                                // session and key) and exits without saving
                                // anything else. Two clicks like every other
                                // destructive action.
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
                                            wipe_until = None;
                                            wipe_all = true;
                                        } else {
                                            wipe_until = Some(crate::config::deadline_after(
                                                WIPE_CONFIRM_SECS,
                                            ));
                                        }
                                    }
                                    let wipe_armed =
                                        wipe_until.is_some_and(|until| until > Instant::now());
                                    confirm_note(
                                        ui,
                                        wipe_armed,
                                        "Click again to delete all data and close.",
                                    );
                                    // A wipe that did not go through keeps the
                                    // app open: say so instead of exiting as if
                                    // the data were gone.
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

                                ui.add_space(theme::spacing::M);

                                // ---- About (compact) ----------------------------------
                                // ONE left-to-right line on purpose: the dialog
                                // is content-sized with a bounded height, so
                                // the card stack above must keep the whole
                                // option set inside the scroll viewport; a full
                                // About card is therefore not an option (the
                                // geometry test pins this). Version and license
                                // identity stay visible; the link opens the
                                // embedded `THIRD_PARTY_NOTICES.md`.
                                ui.horizontal(|ui| {
                                    ui.label(theme::weak(
                                        ui,
                                        concat!(
                                            "MDowser ",
                                            env!("CARGO_PKG_VERSION"),
                                            " · MPL-2.0"
                                        ),
                                    ));
                                    ui.label(theme::weak(ui, "·"));
                                    let link = text_link(
                                        ui,
                                        s,
                                        "View third-party licenses",
                                        "Licenses of the bundled crates and fonts",
                                    );
                                    if link.clicked() {
                                        // Also dismiss the dialog: `App`
                                        // opens the page in a new tab, and a
                                        // floating Preferences window would
                                        // otherwise sit on top of it.
                                        open_licenses = true;
                                        open = false;
                                    }
                                });
                            });
                    });
            });
        self.prefs_open = open;
        // Closing the dialog (its X or Esc) disarms pending deletes: the
        // two-click confirmations must not survive into a later visit. An
        // ELAPSED deadline is dropped too (keeping a stale `Some(Instant)`
        // would re-arm the label on a later visit without a new click).
        let live = |until: Option<Instant>| until.filter(|at| *at > Instant::now());
        self.clear_confirm = if open { live(confirm_until) } else { None };
        self.wipe_confirm = if open { live(wipe_until) } else { None };
        PrefsActions {
            zoom: applied_zoom,
            open_licenses,
            wipe_all,
        }
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
