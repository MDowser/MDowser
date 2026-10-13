// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::navbar`: modern top toolbar.
//!
//! LAYOUT (left to right):
//! - Navigation cluster: Back, Forward, Reload (becomes Stop while loading),
//!   Home (the new tab page). Circular hover fills; monochrome vector icons
//!   with a subtle hover/press effect (`ui::icons`).
//! - Omnibox (`ui::omnibox`): rounded capsule with a padlock indicator
//!   (clicking it opens a page-info card with the facts of the shown page),
//!   the schemeless address field, a right slot that is the clear X while
//!   editing or the bookmark ribbon otherwise (filled when the shown page is
//!   bookmarked, outline when not, disabled when there is nothing to
//!   bookmark) and a loading progress sweep. NO `Go` button: Enter (or
//!   clicking a link) navigates.
//! - Zoom pill: `-` / `%` / `+` in a compact segmented capsule; clicking the
//!   percentage resets to 100%. Hidden on very narrow windows (the omnibox
//!   keeps the space), compact without the readout below ~720px.
//! - Preferences (gear icon) at the very END of the bar, after the zoom
//!   pill: always visible on any window width; toggles the Preferences
//!   window owned by `ui::preferences`.
//!
//! Reload keeps its 5s cooldown (tooltip carries the countdown); while a
//! fetch is in flight the button turns into Stop, which cancels it and
//! restores the page that was on screen (`Tab::stop`). Every button owns a
//! fixed slot whatever it renders (Reload/Stop share one `BUTTON` slot, the
//! ribbon shares the clear-X slot, the zoom pill has a fixed width per
//! mode), so swapping an icon never shifts its neighbours.
//!
//! The tab rail lives to the LEFT of the content and below this toolbar
//! (`ui::tabs`), so the toolbar spans the full window width.
//!
//! FIELD ALIGNMENT (fixed bug): `TextEdit` defaults to `LEFT_TOP`; the
//! omnibox sets `.vertical_align(Center)` so text never sticks to the top.

use crate::bookmarks::BookmarkStore;
use crate::browser::tab::Tab;
use crate::config::{ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};
use crate::history::HistoryStore;
use crate::ui::{icons, omnibox, theme};
use eframe::egui;

/// Omnibox capsule height (and the toolbar row's reference height).
const ROW_H: f32 = crate::config::CHROME_ROW_H;
/// Zoom pill geometry.
const ZOOM_H: f32 = crate::config::CHROME_ZOOM_H;
const ZOOM_BTN: f32 = 26.0;
/// Gap between the omnibox and the zoom pill (and before Preferences).
const ZOOM_GAP: f32 = 8.0;
/// Preferences icon button width (`icons::icon_button`'s box).
const PREFS_W: f32 = icons::BUTTON;

/// Responsive zoom presentation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ZoomMode {
    /// `- 100% +` (wide windows).
    Full,
    /// `- +` without the readout (narrow windows).
    Compact,
    /// No zoom controls (very narrow windows): the omnibox wins the space.
    Hidden,
}

impl ZoomMode {
    /// Outer width of the pill in this mode.
    fn width(self) -> f32 {
        match self {
            ZoomMode::Full => 4.0 + ZOOM_BTN + 44.0 + ZOOM_BTN + 4.0,
            ZoomMode::Compact => 4.0 + ZOOM_BTN * 2.0 + 4.0,
            ZoomMode::Hidden => 0.0,
        }
    }
}

/// Draws the navigation bar for the active tab. `zoom` is the App zoom
/// factor, adjusted by the zoom pill (clamped here). `focus` moves keyboard
/// focus into the omnibox once (new tab / Ctrl+Cmd+L). `prefs_open` is
/// toggled by the Preferences button (the window itself lives in `App`).
/// `bookmark` is the ribbon state for the shown page (see
/// [`omnibox::BookmarkState`]). `history`/`bookmarks` feed the shared
/// omnibox autocomplete (read-only).
///
/// Returns `true` when the bookmark ribbon was clicked: the `App` owns the
/// store, so it performs the toggle.
#[allow(clippy::too_many_arguments)]
pub fn show_navbar(
    ui: &mut egui::Ui,
    tab: &mut Tab,
    history: &HistoryStore,
    bookmarks: &BookmarkStore,
    zoom: &mut f32,
    focus: bool,
    prefs_open: &mut bool,
    bookmark: omnibox::BookmarkState,
) -> bool {
    let mut bookmark_toggled = false;
    // Explicit frame: known 8px gutters on the chrome tone (like the tab
    // rail, distinct from the white page). Full-width: App draws this
    // Panel::top BEFORE the left rail, so it spans the window.
    let s = theme::scheme_ref(ui);
    egui::Panel::top("navbar")
        .frame(
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 7,
                    bottom: 7,
                })
                .fill(s.container_low)
                .stroke(egui::Stroke::NONE),
        )
        .show_separator_line(false)
        .show(ui, |ui| {
            // Responsive breakpoints use the UNZOOMED width: egui points
            // shrink as the zoom factor grows, so measuring raw points hid
            // the zoom pill exactly when zooming in (the only visible way
            // back disappeared). Normalizing by the active zoom restores
            // the width the layout would measure at 100%.
            let total_w = ui.available_width() * ui.ctx().zoom_factor();
            let zoom_mode = if total_w >= 720.0 {
                ZoomMode::Full
            } else if total_w >= 520.0 {
                ZoomMode::Compact
            } else {
                ZoomMode::Hidden
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let loading = tab.is_loading();

                // Back / forward: enabled while loading too (navigating away
                // cancels the pending fetch).
                if icons::icon_button(ui, icons::Icon::Back, "Back", tab.can_back()).clicked() {
                    tab.go_back();
                }
                if icons::icon_button(ui, icons::Icon::Forward, "Forward", tab.can_forward())
                    .clicked()
                {
                    tab.go_forward();
                }
                // Reload / Stop: one slot, two faces. Reload honors the 5s
                // cooldown (tooltip counts it down); while loading this is a
                // Stop button that cancels the fetch and restores the page.
                if loading {
                    if icons::icon_button(ui, icons::Icon::Close, "Stop loading (Esc)", true)
                        .clicked()
                    {
                        tab.stop();
                    }
                } else if tab.can_refresh() {
                    // Hot path: static tooltip, no per-frame allocation.
                    if icons::icon_button(ui, icons::Icon::Reload, "Reload", true).clicked() {
                        tab.refresh();
                    }
                } else {
                    // Cooldown path only: the countdown changes every second,
                    // so it is built lazily inside the tooltip closure (a
                    // settled Failed/Home page must not `format!` per frame).
                    // A tab that cannot refresh for a NON-cooldown reason
                    // (Home, a fresh local rejection) has no countdown at all:
                    // "available in 0s" was nonsense copy.
                    let remaining = tab.refresh_remaining();
                    let tip = move || {
                        if remaining == 0 {
                            "Nothing to reload".to_string()
                        } else {
                            format!("Reload (available in {remaining}s)")
                        }
                    };
                    // A disabled button never reports a click, so there is no
                    // action to run here (the old branch kept an unreachable
                    // `refresh()` "for symmetry").
                    let _ = icons::icon_button_lazy(ui, icons::Icon::Reload, "Reload", tip, false);
                }
                // Home: the new tab page (landing page with the large
                // omnibox and the bookmark grid). Enabled even while
                // loading (abandons the pending fetch).
                if icons::icon_button(ui, icons::Icon::Home, "Home: new tab page", true).clicked() {
                    tab.go_home();
                }
                ui.add_space(6.0);

                // Omnibox + zoom + preferences: the field takes every pixel
                // left over. Reserve the pill (when shown), the preferences
                // button and the gaps around them, so Preferences ends
                // exactly on the panel's right edge. egui adds one item
                // spacing per allocation (the field, the pill when shown and
                // Preferences) plus the explicit ZOOM_GAP spaces, so the
                // reservation must charge exactly those.
                let spacing = ui.spacing().item_spacing.x;
                let spacings_after_field = if zoom_mode == ZoomMode::Hidden {
                    1.0
                } else {
                    2.0
                };
                let reserved =
                    zoom_mode.width() + PREFS_W + spacings_after_field * (ZOOM_GAP + spacing);
                let field_w = (ui.available_width() - reserved).max(90.0);
                let response = omnibox::show_omnibox(
                    ui,
                    tab,
                    field_w,
                    ROW_H,
                    focus,
                    "address-bar",
                    Some(bookmark),
                    history,
                    bookmarks,
                );
                if response.submitted {
                    tab.navigate_from_address();
                }
                bookmark_toggled |= response.bookmark_toggled;
                if zoom_mode != ZoomMode::Hidden {
                    ui.add_space(ZOOM_GAP);
                    zoom_pill(ui, zoom, zoom_mode);
                }
                ui.add_space(ZOOM_GAP);
                // Last widget in the bar: Preferences stays reachable even on
                // very narrow windows where the zoom pill hides.
                if icons::icon_button(ui, icons::Icon::Settings, "Preferences (Ctrl/Cmd+,)", true)
                    .clicked()
                {
                    *prefs_open = !*prefs_open;
                }
            });
        });
    bookmark_toggled
}

/// Default-zoom control for Preferences: the same `- % +` capsule as the
/// toolbar (always the full `- 100% +` face; clicking the readout resets
/// to 100%). Steps and clamps exactly like the toolbar pill.
pub(crate) fn default_zoom_control(ui: &mut egui::Ui, zoom: &mut f32) {
    zoom_pill(ui, zoom, ZoomMode::Full);
}

/// Compact zoom capsule: `-` steps down, the readout resets to 100%, `+`
/// steps up. Always enabled: zooming never touches the network.
fn zoom_pill(ui: &mut egui::Ui, zoom: &mut f32, mode: ZoomMode) {
    let s = theme::scheme_ref(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(mode.width(), ZOOM_H), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::pill_radius(ZOOM_H), s.omnibox_bg);
    let out_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + 4.0 + ZOOM_BTN * 0.5, rect.center().y),
        egui::vec2(ZOOM_BTN, ZOOM_BTN),
    );
    if icons::icon_button_at(
        ui,
        out_rect,
        "zoom-out",
        icons::Icon::ZoomOut,
        "Zoom out",
        true,
    )
    .clicked()
    {
        *zoom = (*zoom - ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }
    // Computed ONCE for the readout, the tooltip and the accessible name:
    // the two `format!` sites below each built their own value per frame.
    let zoom_percent = (*zoom * 100.0).round() as i32;
    if mode == ZoomMode::Full {
        let pct_rect = egui::Rect::from_min_max(
            egui::pos2(out_rect.max.x, rect.min.y + 2.0),
            egui::pos2(rect.max.x - ZOOM_BTN - 4.0, rect.max.y - 2.0),
        );
        let pct_resp = ui.interact(pct_rect, ui.id().with("zoom-reset"), egui::Sense::click());
        if pct_resp.hovered() {
            ui.painter().rect_filled(
                pct_rect,
                egui::CornerRadius::same(theme::radius::CARD),
                s.omnibox_hover,
            );
        } else if pct_resp.has_focus() {
            // Keyboard focus must be visible here like on every other
            // hand-painted control.
            theme::focus_ring(ui, pct_rect, theme::radius::CARD);
        }
        ui.painter().text(
            pct_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{zoom_percent}%"),
            egui::TextStyle::Small.resolve(ui.style()),
            s.on_surface,
        );
        let pct_resp = pct_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let pct_resp = crate::ui::hover_tooltip(pct_resp, "Reset zoom to 100%");
        pct_resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Zoom {zoom_percent}%, click to reset"),
            )
        });
        if pct_resp.clicked() {
            *zoom = 1.0;
        }
    }
    let in_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 4.0 - ZOOM_BTN * 0.5, rect.center().y),
        egui::vec2(ZOOM_BTN, ZOOM_BTN),
    );
    // Dynamic tooltip keeps the current zoom visible in Compact mode (no
    // readout); it is built on hover only, so a settled frame formats
    // nothing.
    if icons::icon_button_at_lazy(
        ui,
        in_rect,
        "zoom-in",
        icons::Icon::ZoomIn,
        "Zoom in",
        move || format!("Zoom in ({zoom_percent}%)"),
        true,
    )
    .clicked()
    {
        *zoom = (*zoom + ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

#[cfg(test)]
mod tests;
