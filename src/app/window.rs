// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::window`: native-window geometry.
//!
//! - The FIRST frame sizes and positions the window once: it restores the
//!   geometry remembered in the encrypted preferences, or (first run) opens
//!   at [`config::initial_window_size`] of the current monitor, centered.
//! - Every later frame keeps that geometry in sync with the OS (a move or a
//!   resize) and persists it, debounced, so the next launch opens where the
//!   user left the window.
//!
//! The monitor list and the window rects come from the winit window eframe
//! hands out (`eframe::Frame::winit_window`); only inherent methods are
//! used, so no direct `winit` dependency is added.
//!
//! The geometry is kept in PHYSICAL pixels and converted to native-scale
//! points only for the viewport commands: egui applies a `set_zoom_factor`
//! only at the start of the NEXT pass, so the first frame's commands run at
//! the default zoom, and physical pixels are the only unit that survives a
//! DPI or zoom change without drifting (a window remembered on a monitor at
//! a different scale reopens at the same physical size).

use super::*;
use crate::settings::WindowGeometry;
use std::time::{Duration, Instant};

/// Native-window geometry bookkeeping for `App`.
#[derive(Debug)]
pub(super) struct WindowGeometryState {
    /// The one-shot first-frame adjustment already ran.
    applied: bool,
    /// The frame right after the adjustment has passed, so the window rects
    /// now reflect it (they still report the creation geometry on the frame
    /// the commands were sent).
    settled: bool,
    /// Geometry to persist: the last WINDOWED rect plus the maximized flag.
    /// Seeded with the geometry applied on the first frame, then corrected
    /// from the OS every frame.
    current: Option<WindowGeometry>,
    /// Last geometry known to be persisted (the stored value at startup), so
    /// an unchanged window never rewrites `settings.bin`.
    written: Option<WindowGeometry>,
    /// When `current` last changed (debounce anchor); `None` while clean.
    dirty_since: Option<Instant>,
}

impl WindowGeometryState {
    /// Seeds the state from the geometry loaded at startup (`None` = first
    /// run).
    pub(super) fn new(stored: Option<WindowGeometry>) -> Self {
        Self {
            applied: false,
            settled: false,
            current: None,
            written: stored,
            dirty_since: None,
        }
    }
}

impl MdowserApp {
    /// Sizes and positions the native window ONCE, on the first frame.
    ///
    /// eframe already created the window centered on the primary monitor
    /// (`NativeOptions::centered`), so this runs before the first paint and
    /// the window never settles at the OS default cascade position. A stored
    /// geometry is restored (validated against the attached monitors);
    /// without one, the window opens at [`config::initial_window_size`] of
    /// the current monitor, centered on it.
    pub(super) fn place_initial_window(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.window_geometry.applied {
            return;
        }
        // One shot: from here on the window belongs to the user.
        self.window_geometry.applied = true;

        let Some(window) = frame.winit_window() else {
            return; // Headless (tests): nothing to place.
        };
        // Native scale: the only conversion between the physical geometry
        // kept here and the points the viewport commands take. The closure
        // avoids naming winit types (no direct dependency).
        let scale = (window.scale_factor() as f32).max(f32::EPSILON);
        let to_rect = |x: i32, y: i32, w: u32, h: u32| {
            egui::Rect::from_min_size(
                egui::pos2(x as f32, y as f32),
                egui::vec2(w as f32, h as f32),
            )
        };
        let monitors: Vec<egui::Rect> = window
            .available_monitors()
            .map(|monitor| {
                let pos = monitor.position();
                let size = monitor.size();
                to_rect(pos.x, pos.y, size.width, size.height)
            })
            .collect();
        let current_monitor = window.current_monitor().map(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            to_rect(pos.x, pos.y, size.width, size.height)
        });
        // Frame chrome (title bar + borders) in physical pixels: a centered
        // window is centered by its OUTER rect.
        let outer_px = window.outer_size();
        let inner_px = window.inner_size();
        let decorations = egui::vec2(
            outer_px.width.saturating_sub(inner_px.width) as f32,
            outer_px.height.saturating_sub(inner_px.height) as f32,
        );

        let target = match self.window_geometry.written {
            Some(stored) => stored_target(stored, &monitors, current_monitor, decorations, scale),
            None => first_run_target(
                current_monitor.or_else(|| monitors.first().copied()),
                decorations,
                scale,
            ),
        };
        let Some(target) = target else {
            return;
        };
        // Physical -> native-scale points, the unit of the commands.
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target.size / scale));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            target.pos.x / scale,
            target.pos.y / scale,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(target.maximized));
        // The applied geometry is the baseline; the next frames correct it
        // from what the OS actually granted.
        self.window_geometry.current = Some(target);
        if self.window_geometry.written != Some(target) {
            self.window_geometry.dirty_since = Some(Instant::now());
        }
        // Make sure the settle frame runs even when the requested geometry
        // happens to match the creation one (no OS event would wake us).
        ctx.request_repaint();
    }

    /// Captures the settled window geometry and persists it (debounced).
    ///
    /// While the window is maximized only the flag is updated (the stored
    /// rect stays the last WINDOWED one, so un-maximizing restores it); a
    /// minimized window has no meaningful rect and is skipped.
    pub(super) fn track_window_geometry(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if !self.window_geometry.applied {
            return;
        }
        if !self.window_geometry.settled {
            // First frame after the adjustment: the window rects still
            // report the creation geometry. Start observing next frame.
            self.window_geometry.settled = true;
            return;
        }
        let observed = frame.winit_window().and_then(|window| {
            if window.is_minimized() == Some(true) {
                return None;
            }
            if window.is_maximized() {
                // Keep the last windowed rect; only the flag changes.
                let mut geometry = self.window_geometry.current?;
                geometry.maximized = true;
                return Some(geometry);
            }
            let pos = window.outer_position().ok()?;
            let size = window.inner_size();
            if size.width == 0 || size.height == 0 {
                return None; // Hidden/minimized without the flag: skip.
            }
            Some(WindowGeometry {
                pos: egui::pos2(pos.x as f32, pos.y as f32),
                size: egui::vec2(size.width as f32, size.height as f32),
                maximized: false,
            })
        });
        if let Some(observed) = observed {
            if self.window_geometry.current != Some(observed) {
                self.window_geometry.current = Some(observed);
                self.window_geometry.dirty_since = Some(Instant::now());
            }
        }
        self.flush_window_geometry(ctx);
    }

    /// Writes the pending geometry once it has been still for
    /// [`config::WINDOW_GEOMETRY_SAVE_DEBOUNCE_SECS`], waking an idle window
    /// while the quiet period is still open.
    fn flush_window_geometry(&mut self, ctx: &egui::Context) {
        let Some(since) = self.window_geometry.dirty_since else {
            return;
        };
        let quiet = Duration::from_secs(config::WINDOW_GEOMETRY_SAVE_DEBOUNCE_SECS);
        let elapsed = since.elapsed();
        if elapsed >= quiet {
            if let Some(current) = self.window_geometry.current {
                self.prefs.set_window_geometry(current);
                self.window_geometry.written = Some(current);
            }
            self.window_geometry.dirty_since = None;
        } else {
            ctx.request_repaint_after(quiet.saturating_sub(elapsed));
        }
    }

    /// Persists the geometry one last time if it changed after the last
    /// debounced write (a move or resize right before closing). Called from
    /// `on_exit` before the settings writers are drained.
    pub(super) fn save_window_geometry_on_exit(&mut self) {
        if self.window_geometry.current != self.window_geometry.written {
            if let Some(current) = self.window_geometry.current {
                self.prefs.set_window_geometry(current);
                self.window_geometry.written = Some(current);
            }
        }
    }
}

/// Target geometry for a stored one: the size is clamped to the monitor the
/// window will open on, and a position that no longer overlaps ANY attached
/// monitor (the screen it was on is gone) falls back to centering on the
/// current one.
///
/// The monitor that owns the stored rectangle is the one with the largest
/// overlap, not the monitor the window happened to be CREATED on: a window
/// remembered on a larger secondary display must keep that display's bounds
/// instead of being shrunk to the primary monitor.
///
/// `monitors`/`current` are physical rects; `scale` is the native scale of
/// the monitor the window was CREATED on (the one the commands will use).
pub(super) fn stored_target(
    stored: WindowGeometry,
    monitors: &[egui::Rect],
    current: Option<egui::Rect>,
    decorations: egui::Vec2,
    scale: f32,
) -> Option<WindowGeometry> {
    if monitors.is_empty() {
        // No monitor list to validate or clamp against: trust the stored
        // geometry, exactly like the OS would.
        return Some(stored);
    }
    let inner = egui::Rect::from_min_size(stored.pos, stored.size);
    let owner = monitor_with_largest_overlap(inner, monitors);
    // A stored position that still touches a monitor keeps that monitor;
    // an off-screen one centers on the creation monitor (or the first one
    // when the backend cannot name the current monitor).
    let target_monitor = owner.or(current).or_else(|| monitors.first().copied());
    let mut size = stored.size;
    if let Some(monitor) = target_monitor {
        // Never smaller than the OS minimum (in physical pixels) nor larger
        // than the monitor it opens on: an oversized window crashes some
        // Linux setups and is useless anywhere.
        let min = egui::vec2(config::WINDOW_MIN_W * scale, config::WINDOW_MIN_H * scale);
        size.x = size.x.clamp(min.x, monitor.width().max(min.x));
        size.y = size.y.clamp(min.y, monitor.height().max(min.y));
    }
    let mut pos = stored.pos;
    if owner.is_none() {
        // Off every screen (the monitor was unplugged): center on the one
        // the window was created on.
        let monitor = target_monitor?;
        pos = centered_pos(monitor, size, decorations);
    }
    Some(WindowGeometry {
        pos,
        size,
        maximized: stored.maximized,
    })
}

/// The attached monitor a stored window rectangle overlaps most, if any.
///
/// Comparing overlap (rather than "contains the top-left corner") keeps a
/// window that straddles two displays on the one it mostly occupies; a
/// rectangle that merely touches an edge still counts, so a window whose
/// title bar is just off-screen is not recentered.
fn monitor_with_largest_overlap(window: egui::Rect, monitors: &[egui::Rect]) -> Option<egui::Rect> {
    let mut best: Option<(f32, egui::Rect)> = None;
    for monitor in monitors {
        let width = (window.max.x.min(monitor.max.x) - window.min.x.max(monitor.min.x)).max(0.0);
        let height = (window.max.y.min(monitor.max.y) - window.min.y.max(monitor.min.y)).max(0.0);
        let area = width * height;
        if area > 0.0 && best.is_none_or(|(best_area, _)| area > best_area) {
            best = Some((area, *monitor));
        }
    }
    best.map(|(_, monitor)| monitor)
}

/// Target geometry on a first run (no stored value): the
/// monitor-proportional size, centered on the current monitor.
///
/// `monitor` is a physical rect and `scale` its native scale, so the
/// `WINDOW_MIN_*`/`WINDOW_INITIAL_MAX_*` bounds (ui points) become the
/// physical pixels the OS enforces.
pub(super) fn first_run_target(
    monitor: Option<egui::Rect>,
    decorations: egui::Vec2,
    scale: f32,
) -> Option<WindowGeometry> {
    let monitor = monitor?;
    let (w, h) = config::initial_window_size(monitor.width() / scale, monitor.height() / scale);
    let size = egui::vec2(w * scale, h * scale);
    Some(WindowGeometry {
        pos: centered_pos(monitor, size, decorations),
        size,
        maximized: false,
    })
}

/// Position that centers a window of inner size `inner` (plus its frame
/// `decorations`) on `monitor`. All three are in the same unit.
fn centered_pos(monitor: egui::Rect, inner: egui::Vec2, decorations: egui::Vec2) -> egui::Pos2 {
    let outer = inner + decorations;
    let (x, y) = config::centered_window_pos(
        monitor.min.x,
        monitor.min.y,
        monitor.width(),
        monitor.height(),
        outer.x,
        outer.y,
    );
    egui::pos2(x, y)
}
