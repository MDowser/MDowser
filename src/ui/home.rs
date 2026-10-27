// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::home`: landing page with a large centered omnibox.
//!
//! - Shown when the tab is in `TabState::Home`.
//! - Exactly three elements, nothing else: the wordmark lockup (vector app
//!   mark + "MDowser"), one large address
//!   capsule (the shared `ui::omnibox`: padlock indicator, clear action, no
//!   `https://`; Enter navigates with cascading resolution, no `Go`) and the
//!   encrypted bookmark grid (`ui::bookmarks`), the ONLY place bookmarks are
//!   shown per the product contract. No hint rows, feature lists or shortcut
//!   catalogs: the page stays calm and professional.
//! - ASCII-only UI strings (every glyph paints in the bundled Noto Sans).

use crate::bookmarks::BookmarkStore;
use crate::browser::tab::Tab;
use crate::history::HistoryStore;
use crate::media::favicon::FaviconCache;
use crate::ui::{omnibox, theme};
use eframe::egui;

/// Top air above the wordmark and the gap below it: named (not inline
/// literals) so the landing rhythm stays reviewable in one place.
/// Same pixels as before (`XL` = 16 would undershoot the calm band).
const HOME_TOP_AIR: f32 = 84.0;
const HOME_WORDMARK_GAP: f32 = 36.0;

/// Side of the brand mark beside the wordmark: a touch taller than the
/// display face's cap height so the tile and the letters read as one unit.
const HOME_MARK: f32 = 46.0;
/// Gap between the brand mark and the wordmark.
const HOME_MARK_GAP: f32 = 14.0;

/// The wordmark lockup: the vector app mark ([`crate::ui::icons::paint_brand_mark`])
/// followed by "MDowser" in the display face, centered as ONE unit. A window
/// too narrow for both keeps the wordmark alone, so the lockup never clips.
fn brand_lockup(ui: &mut egui::Ui) {
    let text = theme::display(ui, "MDowser");
    let galley = egui::WidgetText::from(text.clone()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let total = HOME_MARK + HOME_MARK_GAP + galley.size().x;
    let avail = ui.available_width();
    if avail < total {
        ui.label(text);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((avail - total) * 0.5).max(0.0));
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(HOME_MARK, HOME_MARK), egui::Sense::hover());
        crate::ui::icons::paint_brand_mark(ui.painter(), rect);
        ui.add_space(HOME_MARK_GAP);
        ui.label(text);
    });
}

/// Drops the Home page's egui-memory state for one closed tab (the two
/// scroll offsets). Called from the single registry
/// [`crate::ui::forget_tab`].
pub(crate) fn forget_tab_data(ctx: &egui::Context, tab_id: usize) {
    crate::ui::forget_scroll_state(ctx, crate::ui::HOME_SCROLL_ID, tab_id);
    crate::ui::forget_scroll_state(ctx, crate::ui::BOOKMARKS_SCROLL_ID, tab_id);
}

/// Centered welcome content.
///
/// `gutter` is the sheet's responsive content gutter
/// (`theme::content_gutter`), applied through the scroll area's content
/// margin: the sheet (and its always-visible bar) stays flush with the
/// window edges while the at-rest spacing is unchanged.
///
/// Returns the URL of a bookmark the user Cmd/Ctrl+clicked ("open in a new
/// tab"): only `App` may create tabs.
pub fn show_home(
    ui: &mut egui::Ui,
    tab: &mut Tab,
    history: &HistoryStore,
    bookmarks: &mut BookmarkStore,
    favicons: &mut FaviconCache,
    gutter: f32,
) -> Option<String> {
    let mut new_tab_url = None;
    // Scrollable + max-width (not min-width): narrow/low windows never clip.
    let out = egui::ScrollArea::vertical()
        .id_salt((crate::ui::HOME_SCROLL_ID, tab.id))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .content_margin(egui::Margin::same(gutter as i8))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                // Generous top air: the wordmark sits in a calm band under
                // the toolbar (optical, not mechanical, centering).
                ui.add_space(HOME_TOP_AIR);
                brand_lockup(ui);
                ui.add_space(HOME_WORDMARK_GAP);

                // One large address capsule (shared widget): same behavior
                // as the toolbar, so there is nothing new to learn.
                // Floor at 90px: the bookmark grid needs two 40px tiles + a
                // 10px gap below that, so a narrower capsule would overflow.
                let avail = ui.available_width().max(0.0);
                let w = 560.0_f32.min((avail - 24.0).max(90.0));
                let response = omnibox::show_omnibox(
                    ui,
                    tab,
                    w,
                    46.0,
                    false,
                    "home-address",
                    None,
                    history,
                    bookmarks,
                );
                if response.submitted {
                    tab.navigate_from_address();
                }
                // Bookmarks live ONLY here (Home / new tab page).
                new_tab_url = crate::ui::bookmarks::show_bookmarks(ui, tab, bookmarks, favicons, w);
                // Same blank end margin as the reading column (the Home page
                // scrolls too): text never hugs the sheet's bottom edge.
                ui.add_space(theme::spacing::PAGE_END);
            });
        });
    crate::ui::remember_scroll(ui.ctx(), crate::ui::HOME_SCROLL_ID, tab.id, out.id);
    new_tab_url
}

#[cfg(test)]
mod tests {
    use crate::bookmarks::BookmarkStore;
    use crate::browser::tab::Tab;
    use crate::history::HistoryStore;
    use crate::media::favicon::FaviconCache;
    use eframe::egui;

    /// One rendered Home frame at a desktop-ish viewport.
    fn render(
        ctx: &egui::Context,
        tab: &mut Tab,
        history: &HistoryStore,
        store: &mut BookmarkStore,
        favicons: &mut FaviconCache,
    ) -> Vec<egui::epaint::ClippedShape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let _ = super::show_home(ui, tab, history, store, favicons, 12.0);
            });
        });
        out.textures_delta.clear();
        out.shapes
    }

    /// Every painted text run.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// The new tab page keeps exactly its three elements (wordmark, address
    /// capsule, bookmark grid) and none of the old hint/catalog rows.
    #[test]
    fn home_keeps_only_the_wordmark_address_and_bookmarks() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let mut tab = Tab::new(1);
        let history = HistoryStore::in_memory();
        let mut store = BookmarkStore::in_memory();
        let mut favicons = FaviconCache::new();
        store.toggle("file:///tmp/notes.md", Some("Notes"));

        // The bookmark grid settles over a frame; assert on the second.
        let _ = render(&ctx, &mut tab, &history, &mut store, &mut favicons);
        let painted = texts(&render(&ctx, &mut tab, &history, &mut store, &mut favicons));

        assert!(painted.iter().any(|t| t == "MDowser"), "wordmark painted");
        assert!(
            painted.iter().any(|t| t == "example.com/love"),
            "the empty address capsule shows its hint"
        );
        assert!(
            painted.iter().any(|t| t == "Bookmarks"),
            "the bookmark section header painted"
        );
        assert!(
            painted.iter().any(|t| t == "Notes"),
            "the bookmark tile painted"
        );

        // The old copy is gone for good: no hints, examples or shortcut
        // catalogs clutter the page.
        for clutter in [
            "Minimal browser for text/markdown",
            "Press Enter to open",
            "Examples:",
            "Local files:",
            "Tried in order",
            "Images: PNG",
            "Ctrl/Cmd+T new tab",
            "HTTPS and local files. GET only.",
        ] {
            assert!(
                !painted.iter().any(|t| t.contains(clutter)),
                "clutter still painted: {clutter}"
            );
        }
    }
}
