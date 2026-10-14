// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `main`: browser entry point.
//!
//! - Creates the `eframe` window (winit + wgpu, cross-platform).
//! - Command-line arguments (and the "Open with" hand-off from Linux/Windows
//!   file managers) are opened as documents: an absolute path, a `file://`
//!   URL or an `https://` URL. See `app::MdowserApp::new`.
//! - All logic lives in `app`, `browser`, `protocol`, `markdown`, `media`,
//!   `viewer`, `store` and `ui`, plus small shared modules (`config`,
//!   `error`, `settings`, `history`, `licenses`, `bookmarks`, `suggest`,
//!   `builtin_sites`, `util`).

// A GUI, not a console program: on Windows the default subsystem pops a
// console window behind the app. This selects the windowed subsystem there
// (the paired bits live in the embedded manifest, `build.rs`); every other
// platform and the test binaries keep their default.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod assets;
mod bookmarks;
mod browser;
mod builtin_sites;
mod config;
mod error;
mod history;
mod licenses;
mod markdown;
mod media;
mod protocol;
mod settings;
mod store;
mod suggest;
mod ui;
mod util;
mod viewer;

use eframe::egui;

/// Collects OS/CLI open targets from raw arguments.
///
/// Skips flags (`-psn_...` on a macOS Finder launch, and any future
/// option) and empty strings, so nothing machine-generated is opened as a
/// document. Capped at [`crate::config::MAX_TABS`]: the hand-off is
/// OS-controlled and must never allocate an unbounded tab list; per-target
/// length is enforced later by `parse_navigable` (oversized input becomes
/// that tab's local error, never a truncated navigation).
fn collect_initial_targets(args: impl Iterator<Item = String>) -> Vec<String> {
    args.filter(|arg| !arg.is_empty() && !arg.starts_with('-'))
        .take(crate::config::MAX_TABS)
        .collect()
}

fn main() -> eframe::Result<()> {
    // macOS: AppKit adds its own "Show Tab Bar"/"Show All Tabs" items to the
    // View menu while automatic window tabbing is on; MDowser's tabs are the
    // in-window rail, so opt out BEFORE any window exists. Windows and Linux
    // have no menu at all (`app::menu` is macOS-only).
    #[cfg(target_os = "macos")]
    app::menu::disable_automatic_tabbing();
    // Open targets from the OS. File managers/launchers pass absolute paths
    // or `file://` URLs (`Exec=... %U`); terminals pass whatever the user
    // typed. Flags are skipped (`-psn_...` on a macOS Finder launch, and any
    // future option), so nothing machine-generated is opened as a document.
    let initial = collect_initial_targets(std::env::args().skip(1));
    // Native options: title, creation + minimum size (the first frame then
    // applies the remembered geometry or the monitor-proportional size; see
    // `app::window`), vsync for smooth scrolling. `app_id` drives the Wayland
    // app id and the persistence directory; the app icon goes to the OS
    // window/taskbar (Windows and Linux; macOS uses the `.app` bundle icon).
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(egui::Vec2::new(
            crate::config::WINDOW_INITIAL_W,
            crate::config::WINDOW_INITIAL_H,
        ))
        .with_min_inner_size(egui::Vec2::new(
            crate::config::WINDOW_MIN_W,
            crate::config::WINDOW_MIN_H,
        ))
        .with_title("MDowser")
        .with_app_id("mdowser");
    if let Some(icon) = assets::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // eframe centers the window on the primary monitor while CREATING
        // it (it reaches winit's monitors from the event loop), so Windows
        // never opens MDowser at its `CW_USEDEFAULT` cascade position (which
        // appeared low and off to one side). The first frame then applies the
        // stored geometry, or a monitor-proportional size, in `app::window`.
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "MDowser",
        options,
        Box::new(move |cc| Ok(Box::new(app::MdowserApp::new(cc, initial)) as Box<dyn eframe::App>)),
    )
}

#[cfg(test)]
mod tests {
    use super::collect_initial_targets;

    #[test]
    fn skips_flags_and_empty_args() {
        let out = collect_initial_targets(
            [
                "-psn_0_123".to_string(),
                String::new(),
                "--help".to_string(),
                "notes.md".to_string(),
            ]
            .into_iter(),
        );
        assert_eq!(out, vec!["notes.md".to_string()]);
    }

    #[test]
    fn caps_os_targets_at_max_tabs() {
        let out = collect_initial_targets(
            (0..crate::config::MAX_TABS + 10).map(|i| format!("/tmp/{i}.md")),
        );
        assert_eq!(out.len(), crate::config::MAX_TABS);
    }
}
