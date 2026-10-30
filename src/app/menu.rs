// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::menu`: the native macOS menu bar.
//!
//! macOS ALWAYS shows a system menu bar, so MDowser completes it with the
//! browser's commands (About/Preferences/Quit + File/Edit/View/History/
//! Window). Windows and Linux get NO menu at all — the toolbar is the whole
//! chrome there — so this module and its `muda`/`arboard`/`objc2`
//! dependencies are compiled only on macOS.
//!
//! Two AppKit defaults are switched off/overridden so the bar shows exactly
//! MDowser's commands: automatic window tabbing is opted out at startup
//! ([`disable_automatic_tabbing`], called by `main` BEFORE any window
//! exists), which keeps AppKit's own "Show Tab Bar"/"Show All Tabs" items
//! out of the View menu, and the About panel carries the app's icon
//! ([`about_icon`]) instead of AppKit's generic placeholder folder.
//!
//! The bar owns no behaviour of its own: activating a normal item replays the
//! very shortcut the keyboard would ([`MenuAction::shortcut`]), queued by the
//! `muda` handler and drained into the raw input the next frame
//! ([`MdowserApp::poll_menu`] + `App::raw_input_hook`), so the menu and the
//! keys share ONE handler (`App::handle_shortcuts`). `Edit` deliberately
//! carries Copy / Paste / Select all only. Check for Updates has no key
//! equivalent: it opens the Updates card directly instead of replaying a key.
//!
//! A menu activation is not a winit event, so the handler pushes the command
//! and wakes the UI (`Context::request_repaint`); the same handler also
//! proves the item's key equivalent was consumed by AppKit, so a command can
//! never be handled twice (the real key never reaches egui then).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use eframe::egui;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use objc2::MainThreadMarker;
use objc2_app_kit::NSWindow;

use super::MdowserApp;

/// A command the macOS menu bar can raise, one per browser action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuAction {
    /// New tab (`Ctrl/Cmd+T`).
    NewTab,
    /// Close the active tab (`Ctrl/Cmd+W`).
    CloseTab,
    /// Reload the active page (`Ctrl/Cmd+R`).
    Reload,
    /// Open Preferences (`Ctrl/Cmd+,`).
    Preferences,
    /// Open the Updates card and check now (no accelerator: AppKit standard).
    CheckUpdates,
    /// Open the History page (`Ctrl/Cmd+Y`).
    History,
    /// Zoom in (`Ctrl/Cmd+=`).
    ZoomIn,
    /// Zoom out (`Ctrl/Cmd+-`).
    ZoomOut,
    /// Reset the zoom to 100% (`Ctrl/Cmd+0`).
    ZoomReset,
    /// Save the page on screen as Markdown in the downloads folder
    /// (`Ctrl/Cmd+S`).
    SavePage,
    /// Copy the selection (`Ctrl/Cmd+C`).
    Copy,
    /// Paste from the clipboard (`Ctrl/Cmd+V`).
    Paste,
    /// Select all (`Ctrl/Cmd+A`).
    SelectAll,
}

/// A `<modifiers>+<key>` press event, built exactly as `egui-winit` builds
/// one, so `App::handle_shortcuts` and every `TextEdit` react to it
/// identically.
fn key_event(modifiers: egui::Modifiers, key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// [`key_event`] for the `Cmd`-only chord, the shape of most shortcuts.
fn command_key(key: egui::Key) -> egui::Event {
    key_event(egui::Modifiers::COMMAND, key)
}

impl MenuAction {
    /// The egui event that replays this command the way the keyboard would
    /// (`egui-winit` maps `Cmd+C` to `Event::Copy`, not to a key press).
    /// `Paste` returns `None`: it needs the clipboard, read in [`paste_event`].
    fn shortcut(self) -> Option<egui::Event> {
        Some(match self {
            Self::Copy => egui::Event::Copy,
            Self::Paste | Self::CheckUpdates => return None,
            Self::SelectAll => command_key(egui::Key::A),
            Self::NewTab => command_key(egui::Key::T),
            Self::CloseTab => command_key(egui::Key::W),
            Self::Reload => command_key(egui::Key::R),
            Self::Preferences => command_key(egui::Key::Comma),
            Self::History => command_key(egui::Key::Y),
            Self::ZoomIn => command_key(egui::Key::Equals),
            Self::ZoomOut => command_key(egui::Key::Minus),
            Self::ZoomReset => command_key(egui::Key::Num0),
            Self::SavePage => command_key(egui::Key::S),
        })
    }
}

/// The `MenuAction` a custom item id stands for, or `None` for the standard
/// items (About, Services, Hide, Quit, Minimize, ...) whose selectors AppKit
/// runs itself (they never reach our event queue).
fn action_for(id: &MenuId) -> Option<MenuAction> {
    Some(match id.0.as_str() {
        "new-tab" => MenuAction::NewTab,
        "close-tab" => MenuAction::CloseTab,
        "reload" => MenuAction::Reload,
        "preferences" => MenuAction::Preferences,
        "check-updates" => MenuAction::CheckUpdates,
        "history" => MenuAction::History,
        "zoom-in" => MenuAction::ZoomIn,
        "zoom-out" => MenuAction::ZoomOut,
        "zoom-reset" => MenuAction::ZoomReset,
        "save-page" => MenuAction::SavePage,
        "copy" => MenuAction::Copy,
        "paste" => MenuAction::Paste,
        "select-all" => MenuAction::SelectAll,
        _ => return None,
    })
}

/// Reads text from the clipboard as a paste event, mirroring `egui-winit`'s
/// own `Cmd+V`: `\r\n` folds to `\n` and an empty clipboard pastes nothing.
fn paste_event() -> Option<egui::Event> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let text = clipboard.get_text().ok()?;
    let text = text.replace("\r\n", "\n");
    (!text.is_empty()).then_some(egui::Event::Paste(text))
}

/// Maps activated commands to the egui events that replay them.
fn menu_events(actions: impl IntoIterator<Item = MenuAction>) -> Vec<egui::Event> {
    let mut events = Vec::new();
    for action in actions {
        match action {
            MenuAction::Paste => events.extend(paste_event()),
            // Handled directly by `poll_menu` (not a keyboard shortcut).
            MenuAction::CheckUpdates => {}
            _ => events.extend(action.shortcut()),
        }
    }
    events
}

/// The native menu item title (mnemonics are Windows-only; `&` would show).
fn item(id: &str, text: &str, code: Code) -> MenuItem {
    MenuItem::with_id(
        id,
        text,
        true,
        Some(Accelerator::new(Modifiers::META, code)),
    )
}

/// Opts the process out of AppKit's automatic window tabbing.
///
/// With tabbing on (the AppKit default), the system injects its own
/// "Show Tab Bar" / "Show All Tabs" items into the View menu of any app
/// whose windows can be tabbed; MDowser's tabs are the in-window rail, so
/// those entries are dead UI. `main` calls this BEFORE any window exists
/// (the property is process-wide) and always runs on the main thread, so
/// the marker is there; off the main thread there is none and the call is
/// skipped rather than panicking.
pub(crate) fn disable_automatic_tabbing() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
    }
}

/// The About panel's icon: the embedded 256px application raster, the same
/// bytes Windows/Linux hand to the OS window (macOS itself uses the `.app`
/// bundle's `.icns`, see [`crate::assets`]). Without it AppKit falls back
/// to its generic application placeholder (the plain folder). `None` when
/// the embedded PNG cannot be decoded: the panel then keeps the placeholder
/// rather than failing the launch.
fn about_icon() -> Option<muda::Icon> {
    let icon = crate::assets::window_icon()?;
    muda::Icon::from_rgba(icon.rgba, icon.width, icon.height).ok()
}

/// Builds the whole menu bar. The first submenu is the application menu
/// (macOS titles it with the app name itself).
fn build() -> muda::Result<Menu> {
    let about = AboutMetadata {
        name: Some("MDowser".to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        copyright: Some("MPL-2.0".to_string()),
        icon: about_icon(),
        ..AboutMetadata::default()
    };
    let app = Submenu::with_items(
        "MDowser",
        true,
        &[
            &PredefinedMenuItem::about(Some("About MDowser"), Some(about)),
            &item("preferences", "Preferences…", Code::Comma),
            // AppKit's conventional no-accelerator item.
            &MenuItem::with_id("check-updates", "Check for Updates…", true, None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(Some("Services")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some("Hide MDowser")),
            &PredefinedMenuItem::hide_others(Some("Hide Others")),
            &PredefinedMenuItem::show_all(Some("Show All")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(Some("Quit MDowser")),
        ],
    )?;
    let file = Submenu::with_items(
        "File",
        true,
        &[
            &item("new-tab", "New Tab", Code::KeyT),
            &item("close-tab", "Close Tab", Code::KeyW),
            &PredefinedMenuItem::separator(),
            // The save path asks for no destination, so this is not
            // AppKit's "Save Page As…" sheet, but the same direct save as
            // Ctrl/Cmd+S.
            &item("save-page", "Save Page", Code::KeyS),
        ],
    )?;
    // Edit is deliberately minimal: Copy / Paste / Select all and nothing else.
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &item("copy", "Copy", Code::KeyC),
            &item("paste", "Paste", Code::KeyV),
            &PredefinedMenuItem::separator(),
            &item("select-all", "Select All", Code::KeyA),
        ],
    )?;
    let view = Submenu::with_items(
        "View",
        true,
        &[
            &item("reload", "Reload", Code::KeyR),
            &PredefinedMenuItem::separator(),
            &item("zoom-in", "Zoom In", Code::Equal),
            &item("zoom-out", "Zoom Out", Code::Minus),
            &item("zoom-reset", "Actual Size", Code::Digit0),
        ],
    )?;
    let history = Submenu::with_items(
        "History",
        true,
        &[&item("history", "Show History", Code::KeyY)],
    )?;
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(Some("Minimize")),
            &PredefinedMenuItem::zoom(Some("Zoom")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(Some("Bring All to Front")),
        ],
    )?;
    Menu::with_items(&[&app, &file, &edit, &view, &history, &window])
}

/// The installed menu bar and the queue of commands it raised.
pub(super) struct AppMenu {
    /// Keeps the `NSMenu` alive for the process (NSApp also retains it).
    /// Dropping it early would leave a dangling main menu.
    _menu: Menu,
    /// Commands activated since the last frame, drained by [`Self::poll`].
    queue: Arc<Mutex<VecDeque<MenuAction>>>,
}

impl AppMenu {
    /// Builds the bar, installs it over winit's default app menu and wires
    /// the activation handler that queues commands and wakes the UI.
    /// `None` if AppKit refused a menu allocation (the default bar stays).
    pub(super) fn install(ctx: &egui::Context) -> Option<Self> {
        let menu = build().ok()?;
        menu.init_for_nsapp();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let producer = Arc::clone(&queue);
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let Some(action) = action_for(&event.id) else {
                return;
            };
            match producer.lock() {
                Ok(mut queue) => queue.push_back(action),
                Err(poisoned) => poisoned.into_inner().push_back(action),
            }
            // Not a winit event: wake the UI so an idle window still drains
            // the queue on its next `logic` pass.
            ctx.request_repaint();
        }));
        Some(Self { _menu: menu, queue })
    }

    /// Drains the commands activated since the last call.
    fn poll(&self) -> Vec<MenuAction> {
        match self.queue.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(poisoned) => poisoned.into_inner().drain(..).collect(),
        }
    }
}

impl MdowserApp {
    /// Queues the commands the menu bar raised, as the egui events that
    /// replay their shortcuts. Check for Updates has no shortcut and is
    /// handled here directly. Called from `App::logic`; the events are
    /// injected into the raw input by `App::raw_input_hook`, so they reach
    /// `handle_shortcuts` and the focused field on the next pass.
    pub(super) fn poll_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = &self.mac_menu else {
            return;
        };
        let actions = menu.poll();
        let check_updates = actions.contains(&MenuAction::CheckUpdates);
        let events = menu_events(
            actions
                .into_iter()
                .filter(|action| *action != MenuAction::CheckUpdates),
        );
        if check_updates {
            // No key equivalent: opening the card and starting the check is
            // the whole command. `check_for_updates` requests the repaint.
            self.check_for_updates(ctx);
        }
        if events.is_empty() {
            return;
        }
        self.pending_menu_events.extend(events);
        // The events are injected on the NEXT pass (`raw_input_hook` runs
        // before `logic`): schedule it, so a command is never stranded when
        // this pass was logic-only (the window minimized/occluded).
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_custom_id_maps_to_its_command() {
        for (id, action) in [
            ("new-tab", MenuAction::NewTab),
            ("close-tab", MenuAction::CloseTab),
            ("reload", MenuAction::Reload),
            ("preferences", MenuAction::Preferences),
            ("check-updates", MenuAction::CheckUpdates),
            ("history", MenuAction::History),
            ("zoom-in", MenuAction::ZoomIn),
            ("zoom-out", MenuAction::ZoomOut),
            ("zoom-reset", MenuAction::ZoomReset),
            ("save-page", MenuAction::SavePage),
            ("copy", MenuAction::Copy),
            ("paste", MenuAction::Paste),
            ("select-all", MenuAction::SelectAll),
        ] {
            assert_eq!(action_for(&MenuId::new(id)), Some(action), "id {id:?}");
        }
        // Standard items (About, Hide, Quit, ...) never reach the queue.
        assert_eq!(action_for(&MenuId::new("quit")), None);
    }

    #[test]
    fn commands_replay_their_keyboard_shortcuts() {
        assert_eq!(MenuAction::Copy.shortcut(), Some(egui::Event::Copy));
        // Paste reads the clipboard, so its event is built separately.
        assert_eq!(MenuAction::Paste.shortcut(), None);
        // Check for Updates is a direct action, not a replayed key chord.
        assert_eq!(MenuAction::CheckUpdates.shortcut(), None);
        for (action, modifiers, key) in [
            (
                MenuAction::SelectAll,
                egui::Modifiers::COMMAND,
                egui::Key::A,
            ),
            (MenuAction::NewTab, egui::Modifiers::COMMAND, egui::Key::T),
            (MenuAction::CloseTab, egui::Modifiers::COMMAND, egui::Key::W),
            (MenuAction::Reload, egui::Modifiers::COMMAND, egui::Key::R),
            (
                MenuAction::Preferences,
                egui::Modifiers::COMMAND,
                egui::Key::Comma,
            ),
            (MenuAction::History, egui::Modifiers::COMMAND, egui::Key::Y),
            (
                MenuAction::ZoomIn,
                egui::Modifiers::COMMAND,
                egui::Key::Equals,
            ),
            (
                MenuAction::ZoomOut,
                egui::Modifiers::COMMAND,
                egui::Key::Minus,
            ),
            (
                MenuAction::ZoomReset,
                egui::Modifiers::COMMAND,
                egui::Key::Num0,
            ),
            (MenuAction::SavePage, egui::Modifiers::COMMAND, egui::Key::S),
        ] {
            assert_eq!(
                action.shortcut(),
                Some(key_event(modifiers, key)),
                "{action:?}"
            );
        }
    }

    #[test]
    fn commands_map_to_their_events_in_order() {
        // `Paste` is not exercised here: it reads the real clipboard.
        let events = menu_events([MenuAction::SelectAll, MenuAction::Copy]);
        assert_eq!(events, vec![command_key(egui::Key::A), egui::Event::Copy]);
    }

    #[test]
    fn about_icon_builds_from_the_embedded_raster() {
        // The About panel must show MDowser's own art, never AppKit's
        // generic placeholder folder: the very PNG the OS window icon
        // decodes on every platform must produce a valid `muda::Icon`.
        assert!(about_icon().is_some());
    }
}
