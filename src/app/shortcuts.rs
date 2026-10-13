// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::shortcuts`: browser keyboard shortcuts.
//!
//! Consumed with `consume_key`, so typing in a focused field never triggers
//! navigation; zoom keys are handled here too. Every method is
//! `impl MdowserApp`; the split from `mod.rs` is only for readability.

use super::*;

impl MdowserApp {
    /// Browser keyboard shortcuts. `consume_key` prevents the key from being
    /// typed into a focused text field.
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = egui::Modifiers::COMMAND;
        let digits = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ];
        let mut want_new = false;
        let mut want_close = false;
        let mut want_reload = false;
        let mut want_focus = false;
        let mut want_digit: Option<usize> = None;
        let mut want_stop = false;
        let mut want_history = false;
        let mut want_bookmark = false;
        let mut want_prefs = false;
        let mut want_close_windows = false;
        let mut want_zoom_in = false;
        let mut want_zoom_out = false;
        let mut want_zoom_reset = false;
        // Select-all / whole-page copy only when no TEXT FIELD owns the
        // keyboard: focused fields keep their own Cmd+A/Cmd+C. `focused()`
        // alone is too broad (a focused button used to swallow the keys);
        // `text_edit_focused` is true only for a focused TextEdit.
        let no_text_focus = !ctx.text_edit_focused();
        // With a floating window open, Esc closes it instead of stopping the
        // load (the window owns the context while it is visible).
        let windows_open = self.windows.any_open();
        // A dropdown open inside the window keeps Esc: egui closes the popup
        // on this frame's key while the popup state (persisted in memory by
        // `Popup`; tooltips excluded) still reports it, so the Preferences
        // dialog must not close along with the menu. Read BEFORE `input_mut`:
        // both borrow the context.
        let popup_open = egui::Popup::is_any_open(ctx);
        // With the document context menu open, Esc belongs to the menu
        // (overlays closes it); never steal it for Stop.
        let menu_open = self.tabs.get(self.active).is_some_and(|t| t.context_open);
        // egui clears keyboard focus on Esc BEFORE `App::ui` runs, so
        // `no_text_focus` is already true on that frame: ask the tab whether
        // an address field owned the keyboard last frame (ui::omnibox
        // reverts it then) and leave the key alone.
        let address_focused = self
            .tabs
            .get(self.active)
            .is_some_and(|t| t.address_focused);
        // Same race for the inline tile editor: an open bookmark field must
        // receive Esc (it drops the draft) instead of App consuming it.
        let tile_editing = self.tabs.get(self.active).is_some_and(|t| t.tile_editing);
        // The omnibox page-info popup owns Esc while it is open
        // (`ui::omnibox` closes it); Stop must not steal the key.
        let site_info_open = self.tabs.get(self.active).is_some_and(Tab::site_info_open);
        // Reset now: this frame's omniboxes (toolbar + home) OR it back in.
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.address_focused = false;
            tab.tile_editing = false;
        }
        let mut want_select_all = false;
        ctx.input_mut(|i| {
            want_new = i.consume_key(cmd, egui::Key::T);
            want_close = i.consume_key(cmd, egui::Key::W);
            want_reload = i.consume_key(cmd, egui::Key::R)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::F5);
            want_focus = i.consume_key(cmd, egui::Key::L);
            // macOS uses Cmd+Y (`Cmd+H` hides the app); Ctrl+H elsewhere,
            // matching desktop browsers.
            let history_key = if cfg!(target_os = "macos") {
                egui::Key::Y
            } else {
                egui::Key::H
            };
            want_history = i.consume_key(cmd, history_key);
            want_bookmark = i.consume_key(cmd, egui::Key::D);
            want_prefs = i.consume_key(cmd, egui::Key::Comma);
            // Browser zoom: Ctrl/Cmd `+` or `=` (the Plus logical key arrives
            // with Shift on most layouts), `-` and `0`. They work even while
            // a field is focused, like in a browser.
            want_zoom_in =
                i.consume_key(cmd, egui::Key::Plus) || i.consume_key(cmd, egui::Key::Equals);
            want_zoom_out = i.consume_key(cmd, egui::Key::Minus);
            want_zoom_reset = i.consume_key(cmd, egui::Key::Num0);
            if no_text_focus {
                want_select_all = i.consume_key(cmd, egui::Key::A);
            }
            if no_text_focus && !menu_open && !address_focused && !tile_editing {
                if windows_open {
                    if !popup_open {
                        want_close_windows =
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                    }
                } else if !site_info_open {
                    want_stop = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                }
            }
            for (n, key) in digits.iter().enumerate() {
                if want_digit.is_none() && i.consume_key(cmd, *key) {
                    want_digit = Some(n);
                }
            }
        });
        if want_history {
            self.open_history();
        }
        if want_bookmark {
            self.toggle_bookmark();
        }
        if want_prefs {
            self.windows.toggle_prefs();
        }
        if want_close_windows {
            self.windows.close_windows();
        }
        // Captured before ANY tab mutation: a same-frame Ctrl+W must close the
        // tab that was active when the keys were read, not one just opened by
        // Ctrl+H/Ctrl+Y (which inserts and activates a History tab) or
        // selected by Ctrl+1..9 / created by Ctrl+T below.
        let active_before = self.active;
        // Zoom keys change the factor only: `App::ui` applies it to the
        // context before drawing, so the frame responds immediately.
        if want_zoom_in {
            self.zoom = (self.zoom + ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        if want_zoom_out {
            self.zoom = (self.zoom - ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        if want_zoom_reset {
            self.zoom = 1.0;
        }
        if want_new {
            self.new_tab();
        }
        if want_focus {
            self.focus_address = true;
        }
        if want_reload {
            self.active_tab_mut().refresh();
        }
        if want_stop {
            // Only meaningful while loading (`stop` is a no-op otherwise).
            self.active_tab_mut().stop();
        }
        if want_select_all {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                crate::viewer::select_all(ctx, tab);
            }
        }
        // Whole-page copy while the "Select all" highlight is active: the
        // labels hold no native selection then, so egui alone would copy
        // nothing. A real drag selection is left to egui.
        if no_text_focus
            && self.tabs.get(self.active).is_some_and(|t| t.selected_all)
            && !crate::viewer::has_label_selection(ctx)
        {
            // Cmd/Ctrl+C arrives as `Event::Copy` (egui-winit maps it there).
            let copy_event = ctx.input_mut(|i| {
                if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
                    i.events.retain(|e| !matches!(e, egui::Event::Copy));
                    true
                } else {
                    false
                }
            });
            if copy_event {
                let full = self.tabs.get(self.active).and_then(|t| match &t.state {
                    TabState::Loaded { fetched } => Some(fetched.plain_text.clone()),
                    TabState::Licenses => {
                        crate::licenses::document().map(|doc| doc.plain_text.clone())
                    }
                    _ => None,
                });
                if let Some(full) = full {
                    ctx.copy_text(full);
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.set_toast("Text copied");
                    }
                }
            }
        }
        if want_close {
            self.close_tab(ctx, active_before);
        }
        if let Some(n) = want_digit {
            self.select_tab(n);
        }
    }
}
