// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui`: all egui views (immediate mode).
//!
//! - `theme`: document styling (palette, spacing) + neutral chrome, spacing
//!   and component helpers.
//! - `icons`: monochrome interface icons drawn as themed vector strokes
//!   (one 24x24 design grid, hover/press effect) plus the toolbar button
//!   helpers; `paint_text` still renders user glyphs (bookmark tiles)
//!   through `media::emoji`.
//! - `tabs`: vertical tab rail (collapsed favicons, expands on hover into an
//!   overlay).
//! - `omnibox`: modern address capsule (padlock indicator with a
//!   click-to-open page-info card, clear action, Enter submit, Esc revert,
//!   loading progress) shared by toolbar and home.
//! - `navbar`: modern top toolbar (back/forward/reload-stop/home + omnibox +
//!   zoom).
//! - `home`: landing page with the large centered URL box and the
//!   encrypted bookmark "quick access" grid (the ONLY place bookmarks are
//!   listed).
//! - `bookmarks`: the Home tile grid (favicon/letter/emoji tiles, open /
//!   new tab / drag reorder / remove) and the inline rename editor (no
//!   window).
//! - `history_page`: the internal `about:history` page, rendered in a tab
//!   like a document (capsule search over date-grouped rows with favicons,
//!   per-row remove, delete-all footer). The
//!   internal `about:licenses` page renders through `viewer` from the
//!   embedded `THIRD_PARTY_NOTICES.md` (`licenses::document`).
//! - `error_view`: line-by-line access error.
//! - `preferences`: the floating, tabbed Preferences window (General:
//!   theme, zoom, restore-tabs + version/licenses link; Data: retention and
//!   data deletion; Shortcuts: keyboard reference); the history LIST
//!   is the internal `about:history` page above.
//! - `export`: the floating "Export…" card (Ctrl/Cmd+S or the document
//!   context menu): the page on screen to a local file, as markdown source
//!   or plain text, destination editable and defaulted in `~/Downloads`.
//!
//! Document rendering lives in the top-level `viewer` module; `ui` keeps
//! only browser chrome.
//!
//! CONVENTION:
//! - Each `show_*` takes `&mut Tab` and mutates navigation, never does net I/O.
//! - Document links are absolutized before rendering (see
//!   `protocol::rewrite`); clicks navigate via the resolver.
//! - Images never block rendering: the `media` pipeline fetches/decodes on
//!   background threads and the viewer draws placeholders until ready.

use eframe::egui;

/// Platform-correct History shortcut label for tooltips and hints: `Cmd+Y`
/// on macOS (`Cmd+H` would hide the app) and `Ctrl+H` elsewhere.
/// `App::handle_shortcuts` consumes the matching key.
pub const HISTORY_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "Cmd+Y"
} else {
    "Ctrl+H"
};

/// Platform-correct bookmark toggle label for hints: `Cmd+D` on macOS and
/// `Ctrl+D` elsewhere. `App::handle_shortcuts` consumes the matching key.
pub const BOOKMARK_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "Cmd+D"
} else {
    "Ctrl+D"
};

/// Paints a texture centered in `rect`, scaled to FIT while keeping its
/// aspect ratio (never stretched) and with optional rounded corners.
///
/// Favicons are commonly non-square (wordmarks, 32x16 `.ico` frames);
/// `Image::paint_at` would squash them into the allocated square, so every
/// favicon painter goes through this instead.
pub(crate) fn paint_texture_fit(
    ui: &egui::Ui,
    rect: egui::Rect,
    texture: &egui::TextureHandle,
    corner_radius: u8,
) {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let art = egui::Rect::from_center_size(rect.center(), size * scale);
    egui::Image::new(texture)
        .corner_radius(egui::CornerRadius::same(corner_radius))
        .paint_at(ui, art);
}

/// Shows `tooltip` only while `resp` is actually hovered.
///
/// `Response::on_hover_text` converts its argument EAGERLY (egui builds the
/// `RichText`/`String` before checking the hover state), so every call with
/// a `&str` allocated one `String` per widget per frame whether or not a
/// tooltip was displayed. Gating on `hovered()` keeps the allocation to the
/// frames that show it; egui's own hover delay still applies on the frame
/// the pointer arrives, exactly like `on_hover_text`.
pub(crate) fn hover_tooltip(resp: egui::Response, tooltip: &str) -> egui::Response {
    if resp.hovered() {
        resp.on_hover_text(tooltip)
    } else {
        resp
    }
}

/// [`hover_tooltip`] for a tooltip whose text is built (allocates) only
/// while the widget is actually hovered: the closure runs on hover frames
/// alone, so a `format!`-built label costs nothing on settled frames.
pub(crate) fn hover_tooltip_lazy(
    resp: egui::Response,
    tooltip: impl FnOnce() -> String,
) -> egui::Response {
    if resp.hovered() {
        resp.on_hover_text(tooltip())
    } else {
        resp
    }
}

/// Stores a full-range selection for the `TextEdit` field at `id`, so the
/// next keystroke replaces the whole text (standard omnibox / inline-rename
/// behavior). Shared by the address field and the bookmark tile editor: the
/// cursor-state plumbing is subtle enough to live in one place (the two
/// copies had already drifted in shape).
pub(crate) fn select_all_in(ctx: &egui::Context, id: egui::Id, text: &str) {
    if text.is_empty() {
        return;
    }
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = egui::text::CCursor::new(text.chars().count());
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            end,
        )));
    state.store(ctx, id);
}

/// Drops ALL egui-memory state a closed tab parked (both omnibox
/// suggestion caches + field edit states, history search/filter/clear +
/// field edit state, Home rename/drag drafts, the find-bar query field,
/// every per-tab scroll state).
///
/// Single registry: a new per-tab key must be added HERE, so a future
/// surface cannot leak its closed-tab state by forgetting to extend a
/// fan-out in `app`.
pub(crate) fn forget_tab(ctx: &egui::Context, tab_id: usize) {
    history_page::forget_tab_data(ctx, tab_id);
    bookmarks::forget_tab_data(ctx, tab_id);
    home::forget_tab_data(ctx, tab_id);
    omnibox::forget_tab_data(ctx, tab_id);
    // Find-bar query field (`viewer::overlays::find`): its `TextEditState`
    // would otherwise linger one entry per closed tab that ever searched.
    ctx.data_mut(|d| {
        d.remove::<egui::text_edit::TextEditState>(egui::Id::new(("find-query", tab_id)));
    });
    // Error-page scroll offset (`viewer::show_content`'s Failed branch).
    forget_scroll_state(ctx, ERROR_SCROLL_ID, tab_id);
    // Document scroll offsets: one state per visited URL, all recorded by
    // `viewer::show_content` through `remember_scroll` and dropped here.
    forget_scroll_state(ctx, DOC_SCROLL_ID, tab_id);
    // Outline panel row list (`viewer::overlays::outline`).
    forget_scroll_state(ctx, OUTLINE_SCROLL_ID, tab_id);
}

/// Salts of the per-tab `ScrollArea`s whose state `forget_tab` drops.
pub(crate) const HOME_SCROLL_ID: &str = "home-scroll";
pub(crate) const BOOKMARKS_SCROLL_ID: &str = "home-bookmarks-scroll";
pub(crate) const HISTORY_SCROLL_ID: &str = "history-page-scroll";
pub(crate) const ERROR_SCROLL_ID: &str = "error-scroll";
/// Document reading sheet (`viewer::show_content`): one state per visited
/// URL, registered through [`remember_scroll`] so a closed tab drops them
/// all (it used to accumulate for the whole process).
pub(crate) const DOC_SCROLL_ID: &str = "doc-scroll";
/// Row list of the outline panel (`viewer::overlays::outline`), registered
/// through [`remember_scroll`] like the document sheet.
pub(crate) const OUTLINE_SCROLL_ID: &str = "outline-scroll";

/// The real egui id of a per-tab `ScrollArea`, recorded by [`remember_scroll`]
/// under a key of our own.
fn scroll_id_key(id_salt: &'static str, tab_id: usize) -> egui::Id {
    egui::Id::new((id_salt, "id", tab_id))
}

/// Records the REAL id of a per-tab `ScrollArea` (`ScrollAreaOutput::id`).
///
/// A `ScrollArea` derives its persistent id from the PARENT `Ui`
/// (`ui.make_persistent_id(id_salt)`), so `Id::new((id_salt, tab_id))` never
/// matches it (the old `forget_tab` removed five keys that did not exist and
/// leaked one scroll state per closed tab). The recorded id is what
/// [`forget_scroll_state`] removes.
///
/// One surface can park SEVERAL states per tab (the document scroll keeps
/// one per visited URL, so Back/Forward and refresh return to the reading
/// position): the registry holds the list, deduplicated by real id, and the
/// close path removes them all.
pub(crate) fn remember_scroll(
    ctx: &egui::Context,
    id_salt: &'static str,
    tab_id: usize,
    real: egui::Id,
) {
    ctx.data_mut(|d| {
        let key = scroll_id_key(id_salt, tab_id);
        let mut ids = d.get_temp::<Vec<egui::Id>>(key).unwrap_or_default();
        if !ids.contains(&real) {
            ids.push(real);
        }
        d.insert_temp(key, ids);
    });
}

/// Drops the persisted state of every per-tab `ScrollArea` recorded by
/// [`remember_scroll`]. A no-op when the surface was never shown.
pub(crate) fn forget_scroll_state(ctx: &egui::Context, id_salt: &'static str, tab_id: usize) {
    let key = scroll_id_key(id_salt, tab_id);
    let recorded = ctx.data_mut(|d| d.get_temp::<Vec<egui::Id>>(key));
    if let Some(ids) = recorded {
        ctx.data_mut(|d| {
            d.remove::<Vec<egui::Id>>(key);
            for real in ids {
                d.remove::<egui::scroll_area::State>(real);
            }
        });
    }
}

pub mod bookmarks;
pub mod error_view;
pub mod export;
pub mod history_page;
pub mod home;
pub mod icons;
pub mod navbar;
pub mod omnibox;
pub mod preferences;
pub mod row;
pub mod tabs;
pub mod theme;

#[cfg(test)]
mod tests {
    use super::{forget_scroll_state, remember_scroll, HOME_SCROLL_ID};
    use eframe::egui;
    use std::cell::Cell;

    /// The registry must remove the REAL id a `ScrollArea` used. A
    /// `ScrollArea` derives its persistent id from the parent `Ui`
    /// (`ui.make_persistent_id(id_salt)`), so the old
    /// `Id::new((id_salt, tab_id))` removals never matched and every closed tab
    /// leaked one scroll state per surface.
    #[test]
    fn the_scroll_registry_removes_the_real_scroll_area_id() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let real = Cell::new(None);
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let area = egui::ScrollArea::vertical()
                    .id_salt((HOME_SCROLL_ID, 7usize))
                    .show(ui, |ui| {
                        ui.label("content");
                    });
                remember_scroll(ui.ctx(), HOME_SCROLL_ID, 7, area.id);
                real.set(Some(area.id));
            });
        });
        out.textures_delta.clear();
        let real = real.get().expect("scroll area ran");
        assert_ne!(
            real,
            egui::Id::new((HOME_SCROLL_ID, 7usize)),
            "the old key must not be the real scroll id"
        );
        assert!(
            ctx.data_mut(|d| d.get_persisted::<egui::scroll_area::State>(real))
                .is_some(),
            "the scroll area stored its state"
        );
        forget_scroll_state(&ctx, HOME_SCROLL_ID, 7);
        assert!(
            ctx.data_mut(|d| d.get_persisted::<egui::scroll_area::State>(real))
                .is_none(),
            "a closed tab drops the scroll state it parked"
        );
        // Idempotent: a surface that never ran is a no-op.
        forget_scroll_state(&ctx, HOME_SCROLL_ID, 8);
    }
}
