// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::run`: the eframe application contract (per-frame logic and ui).
//!
//! `logic` polls every tab and schedules repaints only while work is
//! pending; `ui` draws the toolbar, the tab rail, the central content and
//! the Preferences window; `on_exit` flushes the encrypted stores.

use super::*;

impl eframe::App for MdowserApp {
    /// Window clear color: the chrome tone, not eframe's default dark gray.
    ///
    /// The central page frame keeps a rounded rail-side corner, so the
    /// uncovered pixel at the corner notch used to show that dark gray as an
    /// ugly black frame around the document. Painting the chrome tone there
    /// makes the page read as a sheet on the toolbar/rail background.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    /// Intercepts a secondary PRESS over the document while a text selection
    /// exists, BEFORE egui sees it.
    ///
    /// egui's label-selection plugin reads any press over a selectable label
    /// as the start of a drag: the press collapses the selection to the click
    /// point (and, once the context-menu popup covers the label, clears it),
    /// so right-clicking a selection destroyed it before "Copy" could use it.
    /// Dropping the press keeps the selection; its release is recorded as
    /// [`Tab::pending_context_click`] and the viewer's context menu opens
    /// from that instead of `secondary_clicked` (which never fires for a
    /// gesture egui did not see the press of). A press outside the document
    /// (or without a selection) is left untouched: clicking elsewhere must
    /// still clear the selection, and a plain right-click still takes the
    /// normal path.
    ///
    /// On macOS it also injects the menu bar commands queued by the platform
    /// menu, as the raw key events they stand for.
    ///
    /// OS file drags are captured here too, before the label-selection early
    /// return: the hover state drives the full-window drop target and dropped
    /// paths are queued for `logic`, which creates the tabs.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // macOS menu bar: replay each queued command as its keyboard event,
        // into the raw input egui is about to process.
        #[cfg(target_os = "macos")]
        if !self.pending_menu_events.is_empty() {
            raw_input.events.append(&mut self.pending_menu_events);
        }
        // A file drag is not a keyboard/pointer event, so it must be handled
        // before the early return below. Native drops always carry a path.
        self.drop_hover = !raw_input.hovered_files.is_empty();
        if !raw_input.dropped_files.is_empty() {
            self.drop_hover = false;
            // Bound the queue like the OS/CLI target list. Keep one path
            // beyond the tab cap so `open_dropped_files` can still report the
            // overflow with the usual toast instead of dropping it silently.
            // The room is computed against what is already queued, so several
            // drops before the next `logic` pass cannot grow it without bound.
            let room = config::MAX_TABS
                .saturating_add(1)
                .saturating_sub(self.pending_drops.len());
            self.pending_drops.extend(
                raw_input
                    .dropped_files
                    .drain(..)
                    .take(room)
                    .map(|file| file.path().to_path_buf()),
            );
        }
        if raw_input.events.is_empty() || !crate::viewer::has_label_selection(ctx) {
            return;
        }
        self.heal_active();
        let idx = self.active.min(self.tabs.len().saturating_sub(1));
        // Only the states that DRAW the context menu may intercept: during a
        // load or on the error view the pending click would linger until the
        // menu is rendered again (and then pop open unprompted).
        let interceptable = matches!(
            self.tabs.get(idx).map(|tab| &tab.state),
            Some(TabState::Loaded { .. } | TabState::Licenses)
        );
        let Some(doc_rect) = self.tabs.get(idx).and_then(|tab| tab.doc_rect) else {
            return;
        };
        if !interceptable {
            return;
        }
        // Starts from the tab's state so a press whose release lands in a
        // later frame (or a stale one from a cancelled gesture) still pairs.
        let mut intercepted = self.tabs[idx].secondary_press_intercepted;
        let mut click: Option<egui::Pos2> = None;
        raw_input.events.retain(|event| {
            let egui::Event::PointerButton {
                button,
                pressed,
                pos,
                ..
            } = event
            else {
                return true;
            };
            if *button != egui::PointerButton::Secondary {
                return true;
            }
            if *pressed {
                // Inside the document: swallow the press. Outside: heal any
                // stale interception and let the press take the normal path.
                intercepted = doc_rect.contains(*pos);
                !intercepted
            } else if intercepted {
                intercepted = false;
                click = Some(*pos);
                false
            } else {
                true
            }
        });
        let tab = &mut self.tabs[idx];
        tab.secondary_press_intercepted = intercepted;
        if let Some(pos) = click {
            tab.pending_context_click = Some(pos);
        }
    }

    /// Logic without drawing: poll downloads (documents + images + favicons)
    /// and repaint only while something is pending.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Native window: size/position ONCE on the first frame, then keep
        // the remembered geometry in sync (debounced persistence).
        self.place_initial_window(ctx, frame);
        self.track_window_geometry(ctx, frame);
        // macOS menu bar: queue the commands activated since the last pass
        // (replayed through the raw input by `raw_input_hook`). A no-op on
        // the platforms that have no menu.
        #[cfg(target_os = "macos")]
        self.poll_menu(ctx);
        // OS file drops captured by `raw_input_hook`: open them before the
        // poll below so their loads start on this same frame.
        if !self.pending_drops.is_empty() {
            let drops = std::mem::take(&mut self.pending_drops);
            self.open_dropped_files(drops);
        }
        // Poll ALL tabs: background loads must resolve even when not visible.
        let mut any_loading = false;
        let mut any_changed = false;
        let mut has_backdrop = false;
        let mut media_pending = false;
        for tab in &mut self.tabs {
            if tab.poll(ctx) {
                any_changed = true;
            }
            // A resolved page goes to the encrypted local history. Only
            // pages that actually loaded are recorded (not failures/stops).
            if let Some((url, title)) = tab.pending_visit.take() {
                self.history.record(url.as_str(), title.as_deref());
            }
            if let crate::browser::tab::TabState::Loading { backdrop, .. }
            | crate::browser::tab::TabState::Settling { backdrop, .. } = &tab.state
            {
                any_loading = true;
                if backdrop.is_some() {
                    has_backdrop = true;
                }
            }
            if tab.media_pending() {
                media_pending = true;
            }
        }
        if self.favicons.poll(ctx) {
            any_changed = true;
        }
        // Chrome glyph rasters (bookmark-tile emoji): uploaded here so a
        // tile's color glyph lands on a later frame.
        if ui::icons::poll_text_glyphs(ctx) {
            any_changed = true;
        }
        if self.favicons.has_pending() || ui::icons::text_glyphs_pending() {
            media_pending = true;
        }
        // Debounced encrypted-history write (no-op while clean; a no-write
        // no-op in `Session` retention, which persists only the preference).
        // A pending write schedules its own repaint: a resolve is often the
        // last event before the window goes idle.
        if self.history.tick() {
            ctx.request_repaint_after(std::time::Duration::from_secs(
                config::HISTORY_SAVE_DEBOUNCE_SECS,
            ));
        }
        // Debounced encrypted-cache index flush (no-op while clean): only
        // the small index files land here; body files are written
        // synchronously by the background job threads that fetched them.
        if let Some(cache) = crate::cache::global() {
            cache.tick();
        }
        // Same debounced encrypted save for the bookmark store (Ctrl/Cmd+D
        // and the Home rows are the only writers).
        if self.bookmarks.tick() {
            ctx.request_repaint_after(std::time::Duration::from_secs(
                config::BOOKMARKS_SAVE_DEBOUNCE_SECS,
            ));
        }
        // Surface a finished background preference/session write (a failure
        // off the UI thread must still reach Preferences; a confirmed
        // success clears the warning).
        self.prefs.tick();
        // Remember the open tabs while "restore tabs on startup" is on. The
        // per-frame check compares against the saved snapshot WITHOUT
        // allocating; the disk is only touched when the URLs/index actually
        // changed. The file is encrypted, so it is never written (and is
        // deleted by `Prefs::set_restore_tabs`/the retention switch) when
        // the preference is off or "Forget on exit" is active.
        if self.prefs.restore_tabs() && !self.history.forgets_on_exit() {
            // A session write that failed on the WORKER (disk full, revoked
            // directory) must still land: the snapshot bookkeeping is
            // invalidated once (the retry then shares the save backoff), so
            // the state is not treated as saved while the file on disk is
            // stale.
            if self.prefs.session_save_error().is_some() && !self.session_saved.is_empty() {
                self.session_saved.clear();
                self.session_saved_active = usize::MAX;
                self.session_save_retry_at = Some(std::time::Instant::now());
                // The retry must land even on an idle window (no further
                // input events would otherwise tick the backoff).
                ctx.request_repaint_after(std::time::Duration::from_secs(
                    config::SESSION_SAVE_RETRY_SECS,
                ));
            }
            if session_needs_save(self) {
                // A failed save is retried after a short backoff (the state
                // changed and must eventually land), but not every frame:
                // retrying immediately on a machine with no data directory
                // would allocate an error string per frame forever.
                let retry_due = self.session_save_retry_at.is_none_or(|at| {
                    at.elapsed() >= std::time::Duration::from_secs(config::SESSION_SAVE_RETRY_SECS)
                });
                if retry_due {
                    // Snapshot ONLY when the save is due: during a failed-save
                    // backoff the state genuinely changed, and building a
                    // String per tab on every frame (up to MAX_TABS) was pure
                    // churn while the retry window was open.
                    let snapshot = session_snapshot(&self.tabs);
                    match settings::save_session(&snapshot, self.active) {
                        Ok(()) => {
                            self.session_saved = snapshot;
                            self.session_saved_active = self.active;
                            self.session_save_retry_at = None;
                        }
                        Err(e) => {
                            self.session_save_retry_at = Some(std::time::Instant::now());
                            self.prefs
                                .note_session_error(format!("could not save the session: {e}"));
                            // Wake the app when the retry window opens.
                            ctx.request_repaint_after(std::time::Duration::from_secs(
                                config::SESSION_SAVE_RETRY_SECS,
                            ));
                        }
                    }
                }
            }
            self.session_restore_enabled = true;
        } else {
            // The preference was turned off: `Prefs::set_restore_tabs`
            // deletes the file; keep the "was enabled" flag in sync so
            // re-enabling forces an immediate rewrite.
            self.session_restore_enabled = false;
        }
        if any_changed {
            // Resolve happened: repaint immediately so the new doc shows
            // without waiting for the throttle window.
            ctx.request_repaint();
        } else if any_loading {
            // Throttled repaint. A first load has nothing to draw (cheap,
            // fast poll); a backdrop re-renders a full document each frame,
            // so it uses a slower cadence until the resolve repaint above.
            let ms = if has_backdrop { 300 } else { 100 };
            ctx.request_repaint_after(std::time::Duration::from_millis(ms));
        } else if media_pending {
            // Image/favicon jobs: quick ticks so textures upload promptly.
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        } else if let Some(active) = self
            .tabs
            .get(self.active.min(self.tabs.len().saturating_sub(1)))
        {
            // Clamped lookup: a corrupted active index must still schedule
            // the cooldown repaint (self-heal happens in `ui`).
            // Only a REAL countdown needs ticks: Home and fresh errors have
            // no cooldown and must stay idle.
            if active.cooldown_pending() {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    /// Drawing (immediate mode) on the root `Ui`.
    /// Panel order: top toolbar first, then the left tab rail, then central.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ui.ctx());

        // Toolbar zoom: only apply when changed (every-frame `set_zoom_factor`
        // invalidates layout and forces repaint, breaking idle rest).
        if (ui.ctx().zoom_factor() - self.zoom).abs() > f32::EPSILON {
            ui.ctx().set_zoom_factor(self.zoom);
        }
        // Window title = doc title (requirement). Only on change: the
        // comparison and the title itself come from the tab's display memo,
        // so an idle window allocates nothing here.
        // eframe 0.37 API: no `Frame::set_window_title`; send a viewport command.
        if self.window_title_changed() {
            let want_title = self.active_window_title().to_string();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(want_title.clone()));
            self.last_window_title = want_title;
        }

        // 1) Top navigation bar FIRST so it spans the full window width.
        // Copy zoom out/in around the call: the tab and the stores are
        // borrowed as disjoint fields, since the ribbon state needs the
        // store while the bar mutates the tab (and the omnibox reads both
        // stores for its suggestions).
        let focus = std::mem::take(&mut self.focus_address);
        let mut zoom = self.zoom;
        // Copy the preference window flag in/out: the tab borrow cannot
        // stay alive across the other `self` fields.
        let mut prefs_open = self.windows.prefs_open;
        let want_bookmark = {
            self.heal_active();
            let active = self.active;
            let bookmark = self.bookmark_state(active);
            let tab = &mut self.tabs[active];
            let history = &self.history;
            let bookmarks = &self.bookmarks;
            ui::navbar::show_navbar(
                ui,
                tab,
                history,
                bookmarks,
                &mut zoom,
                focus,
                &mut prefs_open,
                bookmark,
            )
        };
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.windows.prefs_open = prefs_open;
        if want_bookmark {
            self.toggle_bookmark();
        }

        // 2) Left rail: collapsed favicons (below the toolbar); expands on
        // hover into an overlay that never reflows the page.
        {
            let resp = ui::tabs::show_tabs(
                ui,
                &mut self.tabs,
                self.active,
                &mut self.favicons,
                &mut self.tabs_overlay_rect,
            );
            // The reorder is applied FIRST (the same order the bookmark grid
            // uses for its own drag): its indices come from the layout this
            // frame drew, and a drag never produces a click on that row.
            if let Some((from, to)) = resp.move_tab {
                self.move_tab(from, to);
            }
            if let Some(i) = resp.select {
                self.select_tab(i);
            }
            if resp.new {
                self.new_tab();
            }
            if resp.history {
                self.open_history();
            }
            if let Some(i) = resp.close {
                self.close_tab(ui.ctx(), i);
            }
        }

        // 3) Central content (always the last panel). No bottom bar.
        // The viewer returns a link URL for "Open link in new tab": only
        // `App` may create tabs. The active `Tab` and the `HistoryStore`
        // are borrowed as disjoint fields (the internal history page needs
        // both), which `active_tab_mut` alone cannot express.
        self.heal_active();
        let active = self.active;
        let new_tab_url = {
            let tab = &mut self.tabs[active];
            crate::viewer::show_content(
                ui,
                tab,
                &mut self.history,
                &mut self.bookmarks,
                &mut self.favicons,
            )
        };
        if let Some(url) = new_tab_url {
            self.open_url_in_new_tab(&url);
        }

        // The context menu's "Save page" row only ASKS (`save_asked`): the
        // document payload lives in the tab the viewer just drew, so read and
        // clear the request here, after that draw. Ctrl/Cmd+S reaches the
        // same `save_page`.
        if self.tabs.get(active).is_some_and(|t| t.save_asked) {
            self.tabs[active].save_asked = false;
            self.save_page();
        }

        // Floating Preferences window LAST so it draws over the page. A
        // changed "Default zoom" applies to the current view immediately;
        // "View third-party licenses" opens the embedded notices in a new
        // tab (the dialog never navigates by itself); "Delete all data and
        // close" wipes the data directory and exits immediately without saving.
        let prefs_actions = self
            .windows
            .show(ui.ctx(), &mut self.history, &mut self.prefs);
        if prefs_actions.wipe_all {
            self.wipe_all_and_exit();
        }
        if let Some(default_zoom) = prefs_actions.zoom {
            self.zoom = default_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        }
        if prefs_actions.open_licenses {
            self.open_licenses();
        }

        // File drag over the window: the target sits above every panel and
        // the Preferences window, and never takes pointer input (it is only
        // a layer painter, not a widget).
        if self.drop_hover {
            ui::drop_overlay::show(ui.ctx());
        }

        // Jobs registered while drawing this frame (image `ensure`, favicon
        // probes) start AFTER `logic` decided the repaint cadence. Without
        // this tick their completion would only be noticed on the next input
        // event, leaving loading placeholders hanging on an idle window.
        // Settling gates count too: their images resolve in a separate cache
        // while the backdrop stays on screen.
        if self.favicons.has_pending()
            || ui::icons::text_glyphs_pending()
            || self.tabs.iter().any(|t| t.media_pending())
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    /// Clean exit: writes the pending history and bookmarks (or wipes the
    /// history under `Retention::Session`). A hard kill skips this by
    /// design; session mode never writes anything in the first place.
    fn on_exit(&mut self) {
        self.history.on_exit();
        self.bookmarks.on_exit();
        // Encrypted cache indexes (bounded wait): body files are already on
        // disk; this only persists the debounced metadata.
        if let Some(cache) = crate::cache::global() {
            cache.flush_blocking(std::time::Duration::from_secs(
                config::CACHE_FLUSH_TIMEOUT_SECS,
            ));
        }
        // Last window geometry (a move or resize right before closing must
        // not be lost to the debounce).
        self.save_window_geometry_on_exit();
        // Final session snapshot (also written during `logic`): cheap and
        // guarantees the last state is on disk even if no frame ran after
        // the last change. Only when something actually changed, so a plain
        // "open file and quit" never replaces the stored session; never
        // under "Forget on exit" (nothing on disk).
        if self.prefs.restore_tabs() && !self.history.forgets_on_exit() && session_needs_save(self)
        {
            let _ = settings::save_session(&session_snapshot(&self.tabs), self.active);
        }
        // Preference/session writes run on the background settings worker
        // (the UI thread never `fsync`s): drain them before the process
        // exits. Bounded, so a hung disk cannot freeze the exit.
        settings::flush_settings_writes();
    }
}
