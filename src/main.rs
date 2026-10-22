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
//!   `viewer`, `store` and `ui`, plus small shared modules (`cache`,
//!   `config`, `error`, `settings`, `history`, `licenses`, `bookmarks`,
//!   `suggest`, `builtin_sites`, `util`).

// A GUI, not a console program: on Windows the default subsystem pops a
// console window behind the app. This selects the windowed subsystem there
// (the paired bits live in the embedded manifest, `build.rs`); every other
// platform and the test binaries keep their default.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
// The shipped binary must never abort on a user/network-controlled path:
// every failure is a typed error, a graceful skip or a guarded fallback. The
// gate is lifted for the test harness, whose fixtures use concise
// `unwrap`/`expect` freely. `debug_assert!` stays available (it documents
// invariants without shipping a panic in release builds).
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

mod app;
mod assets;
mod bookmarks;
mod browser;
mod builtin_sites;
mod cache;
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
use std::path::Path;

/// What a command-line invocation asks the process to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliAction {
    /// Open the window (possibly with initial document targets).
    Open,
    /// Print usage and exit (no window).
    Help,
    /// Print the version and exit (no window).
    Version,
}

/// Classifies raw arguments (the ones after `argv[0]`).
///
/// `--help`/`-h` and `--version`/`-V` are answered on the spot instead of
/// being skipped like every other flag: the app used to ignore them and open
/// its window, which made `mdowser --version` silently launch the GUI. The
/// scan stops at the first match, so the order matches what a shell user
/// types first; every other argument is left to
/// [`collect_initial_targets`].
fn cli_action(args: &[String]) -> CliAction {
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return CliAction::Help,
            "--version" | "-V" => return CliAction::Version,
            _ => {}
        }
    }
    CliAction::Open
}

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

/// Resolves bare relative command-line targets that exist on disk.
///
/// A terminal user types `mdowser notes.md` far more often than they mean
/// the `.md` TLD, but a schemeless target is otherwise a host. A target is
/// rewritten to its absolute path only when the relative spelling really
/// names a file or directory under `cwd` (or, for a stem, its `.md` sibling,
/// matching the local route's extension completion). Explicit URLs and
/// path spellings (`https://`, `file:`, `~/`, `./`, `../`, `/`, a Windows
/// drive path) are left untouched, as is anything that does not exist, so
/// `mdowser example.com` keeps navigating there.
fn resolve_cli_targets(targets: Vec<String>, cwd: Option<&Path>) -> Vec<String> {
    let Some(cwd) = cwd else {
        return targets;
    };
    targets
        .into_iter()
        .map(|target| resolve_cli_target(target, cwd))
        .collect()
}

/// One target's [`resolve_cli_targets`] rewrite.
fn resolve_cli_target(target: String, cwd: &Path) -> String {
    if target.is_empty()
        || target.starts_with('-')
        || target.contains("://")
        || crate::util::starts_with_ignore_ascii_case(&target, "file:")
        || target == "~"
        || target.starts_with("~/")
        || target.starts_with("./")
        || target.starts_with("../")
        || Path::new(&target).is_absolute()
    {
        return target;
    }
    let path = cwd.join(&target);
    let candidate = if path.exists() {
        path
    } else {
        // Match the local route: a stem without a Markdown extension also
        // finds its `.md` sibling (`mdowser notes` -> `notes.md`).
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if matches!(ext.as_deref(), Some("md") | Some("markdown")) {
            return target;
        }
        let mut name = path.clone().into_os_string();
        name.push(".md");
        let with_md = std::path::PathBuf::from(name);
        if with_md.is_file() {
            with_md
        } else {
            return target;
        }
    };
    candidate.to_string_lossy().into_owned()
}

/// Prints the `--help` text to stdout.
///
/// `print_help` and [`print_version`] are the ONLY stdout writers in the app
/// (the GUI never prints), so the global `clippy::print_stdout` gate is waived
/// here and nowhere else. Windows GUI subsystem builds have no console: there
/// the flags are no-ops (documented in `docs/BUILDING.md`).
#[allow(clippy::print_stdout, reason = "the deliberate CLI text entry point")]
fn print_help() {
    println!(
        "\
MDowser {} - browser for text/markdown over HTTPS and local files

Usage:
  mdowser [TARGET]...

Open each TARGET as a document: an https:// URL (the scheme is optional),
a file:// URL, an absolute, ~ or relative path, or a Markdown file. A
relative TARGET that exists on disk is opened as a file, not as a host.
Without a TARGET the app opens on its new-tab page.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print the version and exit

Full documentation: https://github.com/MDowser/MDowser/tree/main/docs",
        env!("CARGO_PKG_VERSION")
    );
}

/// Prints the `--version` line to stdout (see [`print_help`] on the waived
/// stdout gate).
#[allow(clippy::print_stdout, reason = "the deliberate CLI text entry point")]
fn print_version() {
    println!("MDowser {}", env!("CARGO_PKG_VERSION"));
}

fn main() -> eframe::Result<()> {
    // `--help` / `--version` answer on the spot (no window): the app used to
    // skip every flag and silently launch the GUI for them.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli_action(&args) {
        CliAction::Version => {
            print_version();
            return Ok(());
        }
        CliAction::Help => {
            print_help();
            return Ok(());
        }
        CliAction::Open => {}
    }
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
    let initial = resolve_cli_targets(
        collect_initial_targets(args.into_iter()),
        std::env::current_dir().ok().as_deref(),
    );
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
        // eframe 0.37 adds a system-font provider for glyphs the installed
        // faces lack. MDowser promises bundled Noto rendering only (see
        // `docs/RENDERING.md`), so keep every machine's text identical.
        system_font_fallback: false,
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
    use super::{cli_action, collect_initial_targets, resolve_cli_target, CliAction};

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

    /// `mdowser notes.md` from a terminal opens the file in the current
    /// directory, not the `.md` host. Only an EXISTING relative target is
    /// rewritten: an explicit URL or path spelling keeps its meaning, and a
    /// missing name still navigates as a host.
    #[test]
    fn existing_relative_cli_targets_become_absolute_paths() {
        let dir = std::env::temp_dir().join(format!("mdowser-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(dir.join("notes.md"), "# Notes\n").expect("note");
        std::fs::create_dir(dir.join("docs")).expect("dir target");

        let notes = dir.join("notes.md").to_string_lossy().into_owned();
        let docs = dir.join("docs").to_string_lossy().into_owned();
        assert_eq!(resolve_cli_target("notes.md".into(), &dir), notes);
        assert_eq!(
            resolve_cli_target("notes".into(), &dir),
            notes,
            "a bare stem finds its .md sibling"
        );
        assert_eq!(resolve_cli_target("docs".into(), &dir), docs);
        for unchanged in [
            "https://ex.com/notes.md",
            "example.com",
            "file:///tmp/notes.md",
            "./notes.md",
            "../notes.md",
            "~/notes.md",
            "/tmp/notes.md",
            "missing.md",
        ] {
            assert_eq!(
                resolve_cli_target(unchanged.into(), &dir),
                unchanged,
                "{unchanged} must be left for the navigability parser"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--help`/`-h` and `--version`/`-V` are answered before any window is
    /// created; anything else opens the app. An unknown flag (or a Finder's
    /// `-psn_...`) must NOT be read as a request for help.
    #[test]
    fn cli_flags_are_classified_before_opening() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli_action(&args(&["--help"])), CliAction::Help);
        assert_eq!(cli_action(&args(&["-h"])), CliAction::Help);
        assert_eq!(cli_action(&args(&["--version"])), CliAction::Version);
        assert_eq!(cli_action(&args(&["-V"])), CliAction::Version);
        // A target, an unknown flag or a Finder launch all open the window.
        assert_eq!(cli_action(&args(&["notes.md"])), CliAction::Open);
        assert_eq!(cli_action(&args(&["--nope"])), CliAction::Open);
        assert_eq!(cli_action(&args(&["-psn_0_1234"])), CliAction::Open);
        assert_eq!(cli_action(&[]), CliAction::Open);
    }
}
