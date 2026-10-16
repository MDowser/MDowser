// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer`: central content (home / loading / internal pages / document / error).
//!
//! - NO metadata bar or "Showing/Source" headers: the normalized URL
//!   already lives schemeless in the navbar. Only the document here.
//! - Internal pages: `about:history` (live, from the encrypted store,
//!   `history_page`) and `about:licenses` (the embedded
//!   `THIRD_PARTY_NOTICES.md` through `licenses::document`) render here too;
//!   neither touches the network.
//! - `markdown` renders CommonMark + tables + footnotes; `image` draws image
//!   widgets (background-fetched by the `media` pipeline).
//! - Loading keeps the previous document as backdrop
//!   (stale-while-revalidate): no white flash when following a link.
//! - Links navigate internally (never open an external browser).
//! - Right-click menu + toasts + the find bar live in `overlays`.
//! - Document text may carry emoji or non-Latin scripts (complete
//!   monochrome Noto Emoji + CJK + Latin + one face per bundled script;
//!   see `ui::theme`); interface icons are monochrome vector strokes
//!   (`ui::icons`). RTL
//!   scripts get display-order reordering from `bidi` (epaint has no bidi
//!   of its own).

mod bidi;
mod cache;
pub(crate) mod find;
mod image;
mod inline;
mod markdown;
// `pub(crate)` like `code`: the app-level tests drive the find bar's own
// entry point (`overlays::find::show_find_bar`), the same one `show_content`
// calls.
pub(crate) mod overlays;
mod table;

pub(crate) mod code;

use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{Tab, TabState};
use crate::history::HistoryStore;
use crate::markdown::anchors::AnchorIndex;
use crate::media::favicon::FaviconCache;
use crate::ui::theme;
use eframe::egui;
use url::Url;

use self::markdown::{doc_id_salt, show_markdown_with_find};

/// Screen-space hitbox of one rendered document link, collected while the
/// document renders so the right-click menu can offer "Open link in new tab".
/// A link split into several inline fragments pushes one hitbox per fragment;
/// they sit next to each other and never overlap.
#[derive(Debug, Clone)]
pub(crate) struct LinkHit {
    /// Link text (or image) rect, screen coordinates.
    pub(crate) rect: egui::Rect,
    /// Absolute destination (`https:` or, inside a local document, `file:`).
    /// Interned (`Arc`): a link split into many fragments shares one
    /// allocation instead of copying the URL per fragment per frame.
    pub(crate) url: std::sync::Arc<str>,
}

/// Pushes one link hitbox, bounded by [`crate::config::MAX_LINK_HITS`].
/// Clicking and navigation never depend on the hitboxes: past the cap the
/// link still works, only its right-click "Open link in new tab" entry is
/// missing. This bounds the per-frame URL copies the hit list retains.
pub(crate) fn push_link_hit(links: &mut Vec<LinkHit>, rect: egui::Rect, url: &std::sync::Arc<str>) {
    if links.len() < crate::config::MAX_LINK_HITS {
        links.push(LinkHit {
            rect,
            url: std::sync::Arc::clone(url),
        });
    }
}

/// Is there a native (drag) label text selection in this viewport?
pub(crate) fn has_label_selection(ctx: &egui::Context) -> bool {
    ctx.with_plugin::<egui::text_selection::LabelSelectionState, _>(|p| p.has_selection())
        .unwrap_or(false)
}

/// Clears the native label text selection, if any.
/// Used when "Select all" replaces a drag selection (the two highlights
/// would disagree) and by `Ctrl/Cmd+A`.
pub(crate) fn clear_label_selection(ctx: &egui::Context) {
    let _ =
        ctx.with_plugin::<egui::text_selection::LabelSelectionState, _>(|p| p.clear_selection());
}

/// "Select all": drops any native drag selection (the page-wide highlight
/// and a partial selection would disagree) and highlights the whole page.
/// The document carries the flag (`Tab::selected_all`); `Copy` and
/// `Ctrl/Cmd+C` then copy the full plain text.
///
/// Only a loaded document (or the embedded licenses page, which is also a
/// real document) can be selected: during `Loading`/`Failed` the backdrop is
/// not the document the flag would copy, so the key is ignored (previously
/// Ctrl+A then Ctrl+C copied nothing while swallowing the event, and the
/// highlight appeared later on the resolved page).
pub(crate) fn select_all(ctx: &egui::Context, tab: &mut Tab) {
    if !matches!(tab.state, TabState::Loaded { .. } | TabState::Licenses) {
        return;
    }
    clear_label_selection(ctx);
    tab.selected_all = true;
}

/// Main entry of the central panel. Called every frame from `App::ui`.
///
/// Returns the URL of a link the user chose "Open link in new tab" for
/// (the `App` owns the tab list; the viewer never creates tabs).
pub fn show_content(
    root: &mut egui::Ui,
    tab: &mut Tab,
    history: &mut HistoryStore,
    bookmarks: &mut BookmarkStore,
    favicons: &mut FaviconCache,
) -> Option<String> {
    // Responsive gutter every sheet applies INSIDE its scroll content
    // (`ScrollArea::content_margin`): the frame stays flush with the rail and
    // the window edges so the always-visible bar hugs the sheet's right edge,
    // while text keeps its gutter and a scrolled document is clipped at the
    // flush top/bottom edges.
    let gutter = theme::content_gutter(root);
    // HOME: needs `&mut Tab` (edits `address`), the bookmark store (the
    // grid lives ONLY there) and the favicon cache (tile icons). Own panel,
    // then return; a Cmd/Ctrl+clicked tile travels up as "open in a new
    // tab".
    if matches!(tab.state, TabState::Home) {
        let opened = egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                crate::ui::home::show_home(ui, tab, history, bookmarks, favicons, gutter)
            })
            .inner;
        overlays::show_toast(root, tab);
        return opened;
    }

    // LOCAL HISTORY: the internal `about:history` page, drawn in the same
    // reading sheet as a document. The page itself navigates the tab when a
    // visit is clicked and mutates the store on remove/delete; row favicons
    // come from the shared session cache (never a new fetch pipeline).
    if matches!(tab.state, TabState::History) {
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                crate::ui::history_page::show_history_page(ui, tab, history, favicons, gutter);
            });
        overlays::show_toast(root, tab);
        return None;
    }

    // THIRD-PARTY LICENSES: the embedded `THIRD_PARTY_NOTICES.md`
    // (`licenses::document`) rendered through the same document pipeline as
    // a fetched page: same reading sheet, text selection, right-click menu
    // and live links. Read from the binary: no fetch, no visit, no image
    // subresources.
    if matches!(tab.state, TabState::Licenses) {
        // MENU FIRST, exactly like `Loaded` (it pushes `Event::Copy` for a
        // native selection before the labels render below).
        let new_tab_url = overlays::content_menu(root, tab);
        // Find bar before the document so a typed query highlights the same
        // frame (no one-frame lag between the bar and the page).
        overlays::find::show_find_bar(root, tab);
        let mut pending_link: Option<String> = None;
        // Cmd/Ctrl+click target this frame: opened in a background tab by
        // the caller, like the context menu's "Open link in new tab" and
        // the bookmark tiles.
        let mut pending_new_tab: Option<String> = None;
        // The notices carry no in-document anchors: a throwaway target is
        // passed so `render_doc`'s signature stays shared with `Loaded`.
        let mut no_anchor: Option<String> = None;
        let mut new_rect: Option<egui::Rect> = None;
        let selected = tab.selected_all;
        let doc = crate::licenses::document();
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                let Some(doc) = doc else {
                    return;
                };
                let ctx = ui.ctx().clone();
                let mut on_link = |dest: &str| {
                    // The notices carry no `#` targets: every live
                    // destination is a navigation, never an anchor jump.
                    if !dest.is_empty() && !dest.starts_with('#') {
                        // Cmd/Ctrl+click opens in a background tab.
                        if ctx.input(|i| i.modifiers.command) {
                            pending_new_tab = Some(dest.to_string());
                        } else {
                            pending_link = Some(dest.to_string());
                        }
                    }
                };
                let viewport = render_doc(
                    ui,
                    &doc.events,
                    &doc.display_url,
                    &doc.anchors,
                    &doc.dests,
                    &mut no_anchor,
                    tab.id,
                    tab.doc_generation,
                    // Internal open clears the document flag: the notices
                    // keep their per-tab offset like Back/Forward do.
                    false,
                    gutter,
                    selected,
                    true,
                    &mut tab.images,
                    &mut tab.emoji,
                    &mut tab.link_hits,
                    &mut tab.find,
                    &mut on_link,
                );
                // Same hit rect as `Loaded`: backs the context menu and
                // excludes the vertical scrollbar.
                let bar = theme::scrollbar_width(ui);
                new_rect = Some(egui::Rect {
                    min: viewport.min,
                    max: egui::pos2((viewport.max.x - bar).max(viewport.min.x), viewport.max.y),
                });
                let scrolled = ui.ctx().input(|i| i.smooth_scroll_delta.y.abs() > 0.0);
                if !scrolled {
                    theme::paint_content_fade(ui, viewport);
                }
            });
        tab.doc_rect = new_rect;
        overlays::show_toast(root, tab);
        if let Some(dest) = pending_link {
            tab.navigate_to_text(&dest, true);
        }
        return pending_new_tab.or(new_tab_url);
    }

    // LOADING/SETTLING: keep the previous document on screen
    // (stale-while-revalidate).
    // NO loading strip/placeholder/separator here: any widget above the document
    // shifts it down on resolve (layout shift). The backdrop renders exactly
    // like `Loaded` (same `render_doc`, inert links); a first load with no
    // backdrop renders nothing (stable empty panel). Status lives in the
    // navbar tooltip ("Loading...") + window title, never in the content.
    // Images already decoded stay visible; no new image fetches start.
    // `Settling` is the same picture one step later: the document already
    // arrived and its images resolve in a separate cache, so the swap to
    // `Loaded` is atomic and never reflows mid-read.
    if matches!(
        tab.state,
        TabState::Loading { .. } | TabState::Settling { .. }
    ) {
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                if let TabState::Loading {
                    backdrop: Some(fetched),
                    ..
                }
                | TabState::Settling {
                    backdrop: Some(fetched),
                    ..
                } = &tab.state
                {
                    let mut noop = |_: &str| {};
                    // Backdrop is static until resolve: skip the fade
                    // overlay (16 extra rects per throttled repaint). Its
                    // anchors are inert (throwaway `pending_anchor`).
                    let mut no_anchor = None;
                    render_doc(
                        ui,
                        &fetched.events,
                        &fetched.display_url,
                        &fetched.anchors,
                        &fetched.dests,
                        &mut no_anchor,
                        tab.id,
                        tab.doc_generation,
                        // Backdrop of a pending load: the old page keeps its
                        // own offset; the reset applies to the NEW document.
                        false,
                        gutter,
                        false,
                        false,
                        &mut tab.images,
                        &mut tab.emoji,
                        &mut tab.link_hits,
                        &mut tab.find,
                        &mut noop,
                    );
                }
            });
        overlays::show_toast(root, tab);
        return None;
    }

    // LOADED: the document alone, no metadata or headers (minimal).
    if let TabState::Loaded { .. } = &tab.state {
        // MENU FIRST: it uses a global secondary-click detector + foreground
        // `Area` (works on text, never blocks links) and, on `Copy`, pushes
        // `Event::Copy` into this frame's input so the text labels below --
        // which still hold the native selection -- perform the copy.
        let new_tab_url = overlays::content_menu(root, tab);
        // Find bar before the document (same reason as `Licenses`).
        overlays::find::show_find_bar(root, tab);
        // Render the document (no background `interact` behind: it either
        // ate link clicks or never saw secondary clicks on text).
        let mut pending_link: Option<String> = None;
        // Cmd/Ctrl+click target this frame: opened in a background tab by
        // the caller (`app::run`), like the context menu's entry and the
        // bookmark tiles.
        let mut pending_new_tab: Option<String> = None;
        // In-document anchor clicked this frame: scroll next frame.
        let mut pending_anchor_click: Option<String> = None;
        let mut new_rect: Option<egui::Rect> = None;
        let selected = tab.selected_all;
        // A fresh navigation starts at the top even when the document was
        // seen before (its saved offset belongs to Back/Forward/Refresh).
        // Taken here so the backdrop never consumes it: only the new
        // document's first frame below applies it.
        let reset_scroll = std::mem::take(&mut tab.scroll_top_requested);
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                if let TabState::Loaded { fetched } = &tab.state {
                    let ctx = ui.ctx().clone();
                    let mut on_link = |dest: &str| {
                        if dest.is_empty() {
                            return;
                        }
                        // `#anchor` is an in-document jump, not a navigation.
                        match dest.strip_prefix('#') {
                            Some(id) if !id.is_empty() => {
                                if ctx.input(|i| i.modifiers.command) {
                                    // Cmd/Ctrl+click: the anchor as its own
                                    // absolute URL in a background tab.
                                    let mut url = fetched.display_url.clone();
                                    url.set_fragment(Some(id));
                                    pending_new_tab = Some(url.to_string());
                                } else {
                                    pending_anchor_click = Some(id.to_string());
                                }
                            }
                            _ => {
                                if ctx.input(|i| i.modifiers.command) {
                                    pending_new_tab = Some(dest.to_string());
                                } else {
                                    pending_link = Some(dest.to_string());
                                }
                            }
                        }
                    };
                    let viewport = render_doc(
                        ui,
                        &fetched.events,
                        &fetched.display_url,
                        &fetched.anchors,
                        &fetched.dests,
                        &mut tab.pending_anchor,
                        tab.id,
                        tab.doc_generation,
                        reset_scroll,
                        gutter,
                        selected,
                        true,
                        &mut tab.images,
                        &mut tab.emoji,
                        &mut tab.link_hits,
                        &mut tab.find,
                        &mut on_link,
                    );
                    // Shrink right edge for hit-testing so clicks on the
                    // vertical scrollbar never open the text menu (same
                    // width the fade overlay reserves).
                    let bar = theme::scrollbar_width(ui);
                    let hit_rect = egui::Rect {
                        min: viewport.min,
                        max: egui::pos2((viewport.max.x - bar).max(viewport.min.x), viewport.max.y),
                    };
                    new_rect = Some(hit_rect);
                    // Fade only at rest (~no scroll): while scrolling it
                    // costs a few rects per frame for an effect hidden by motion.
                    let scrolled = ui.ctx().input(|i| i.smooth_scroll_delta.y.abs() > 0.0);
                    if !scrolled {
                        theme::paint_content_fade(ui, viewport);
                    }
                }
            });
        tab.doc_rect = new_rect;
        overlays::show_toast(root, tab);
        if let Some(id) = pending_anchor_click {
            // Applied on the next frame (this one is already rendered).
            tab.pending_anchor = Some(id);
        }
        if let Some(dest) = pending_link {
            if !dest.is_empty() {
                tab.navigate_to_text(&dest, true);
            }
        }
        return pending_new_tab.or(new_tab_url);
    }

    // FAILED: line-by-line log + retry (no per-frame clones).
    // Outer scroll so Retry stays reachable on short windows.
    // Retry honors the same 5s cooldown as Refresh (anti-hammer).
    // Local address rejections (BadUrl/NotHttps) can never succeed by
    // retrying: the button shows disabled with a fix-the-address hint
    // instead of a pointless countdown.
    if let TabState::Failed { lines, .. } = &tab.state {
        let kind = crate::ui::error_view::classify(lines);
        let local_error = kind == crate::ui::error_view::ErrorKind::BadAddress;
        let mut retry_asked: Option<String> = None;
        let can_retry = tab.can_retry() && !local_error;
        // No per-frame `format!`: the countdown is built inside the lazy
        // tooltip closure, so it only allocates while the button is actually
        // hovered. The button label stays static so a Failed page does not
        // re-layout every frame during cooldown.
        let retry_local_error = local_error;
        let retry_label = "Retry";
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                let scroll_out = egui::ScrollArea::vertical()
                    // Tuple salt: no per-frame `format!` allocation. The real
                    // id is recorded for `ui::forget_tab` (a `ScrollArea`
                    // derives it from the parent `Ui`, so the salt alone
                    // cannot address it).
                    .id_salt((crate::ui::ERROR_SCROLL_ID, tab.id))
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .content_margin(egui::Margin::same(gutter as i8))
                    .show(ui, |ui| {
                        if let TabState::Failed { asked, lines } = &tab.state {
                            crate::ui::error_view::show_error(ui, asked, lines, kind);
                            ui.add_space(theme::spacing::M);
                            let resp = theme::tonal_button_sized(
                                ui,
                                retry_label,
                                egui::vec2(160.0, 36.0),
                                can_retry,
                            );
                            let resp = crate::ui::hover_tooltip_lazy(resp, || {
                                if retry_local_error {
                                    "Fix the address first: retrying a local rejection cannot succeed"
                                        .to_string()
                                } else if can_retry {
                                    "Retry loading this page".to_string()
                                } else {
                                    format!("Retry in {}s (cooldown...)", tab.refresh_remaining())
                                }
                            });
                            if resp.clicked() && can_retry {
                                retry_asked = Some(asked.clone());
                            }
                            // Blank margin after the last element (same
                            // `spacing::PAGE_END` as a document): the error
                            // page scrolls too.
                            ui.add_space(theme::spacing::PAGE_END);
                        }
                    });
                crate::ui::remember_scroll(
                    ui.ctx(),
                    crate::ui::ERROR_SCROLL_ID,
                    tab.id,
                    scroll_out.id,
                );
            });
        overlays::show_toast(root, tab);
        if let Some(asked) = retry_asked {
            tab.navigate_to_text(&asked, false);
        }
    }
    None
}

/// Renders the centered reading column; returns its VIEWPORT rect so the
/// context menu attaches to the visible area (not the scrolled content).
///
/// `gutter` is `theme::content_gutter` for this panel: applied through the
/// scroll area's content margin so the always-visible bar hugs the sheet's
/// right edge while the column keeps its gutter and scrolled letters are
/// clipped at the flush top/bottom edges.
///
/// `reset_scroll` forces the offset to the top this frame (a fresh
/// navigation to a possibly already-seen document whose saved offset must
/// not be reused). A cross-page `#fragment` still wins: its
/// `scroll_to_rect` runs during content layout, after the initial offset.
#[allow(clippy::too_many_arguments)]
fn render_doc(
    ui: &mut egui::Ui,
    events: &[pulldown_cmark::Event<'static>],
    base: &Url,
    anchors: &AnchorIndex,
    dests: &crate::protocol::rewrite::DestCache,
    pending_anchor: &mut Option<String>,
    tab_id: usize,
    doc_generation: u64,
    reset_scroll: bool,
    gutter: f32,
    selected: bool,
    interactive: bool,
    images: &mut crate::media::cache::ImageCache,
    emoji: &mut crate::media::emoji::EmojiCache,
    links: &mut Vec<LinkHit>,
    find: &mut crate::viewer::find::FindState,
    on_link: &mut dyn FnMut(&str),
) -> egui::Rect {
    // Link hitboxes are rebuilt from scratch every frame: stale rects from
    // the previous document (or a scroll) would offer the wrong link.
    links.clear();
    // Centered max-width column, LEFT-aligned inside:
    // `Layout::Center` centered every paragraph/separator/table (short
    // centered separators looked like a stray middle rule + tables never
    // filled). Manual side pad keeps the block centered while text stays left.
    // Viewport captured BEFORE scroll (visible area, stable across scroll).
    // The reading width excludes the gutter on both sheet edges; the floating
    // bar overlays the right gutter, at the sheet edge.
    let viewport = ui.max_rect();
    let viewport_w = (viewport.width() - 2.0 * gutter).max(0.0);
    let max_w = theme::content_max_width(viewport_w);
    let type_scale = theme::type_scale(viewport_w);
    // Stable scroll id per document: a never-seen page starts at the top
    // instead of inheriting another page's offset. Hashed like the
    // table/code ids (a raw URL string would be up to 8 KiB per tab) and
    // MIXED WITH THE TAB ID: egui ids are global, so two tabs showing the
    // same document would otherwise share scroll/table/code state.
    // (A revisit of an already-seen document shares the id too: that case
    // is reset through `reset_scroll`, not through the salt.)
    // `doc_generation` re-keys this tab's code-block cache when the visible
    // document is replaced (refresh), without touching other tabs.
    let doc_salt =
        doc_id_salt(base) ^ (tab_id as u64) ^ doc_generation.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    // The SCROLL state uses the url-only salt (no generation): egui persists
    // one offset per (tab, document), so Back/Forward and refresh return to
    // the reading position exactly like a browser. A fresh navigation to an
    // already-seen document shares that salt, so it must force the offset
    // to the top instead (`reset_scroll`, from `Tab::scroll_top_requested`);
    // a DIFFERENT document gets a different salt and still starts at the
    // top on its own. The block caches keep using `doc_salt` so a refreshed
    // body can never be stale.
    let scroll_salt = doc_id_salt(base) ^ (tab_id as u64);
    let mut scroll = egui::ScrollArea::vertical();
    if reset_scroll {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll
        .id_salt(("doc-scroll", scroll_salt))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .content_margin(egui::Margin::same(gutter as i8))
        .show(ui, |ui| {
            let avail = ui.available_width();
            let col_w = max_w.min(avail).max(0.0);
            let pad = ((avail - col_w) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if pad > 0.5 {
                    ui.add_space(pad);
                }
                ui.vertical(|ui| {
                    ui.set_max_width(col_w);
                    ui.set_min_width(col_w.max(0.0));
                    show_markdown_with_find(
                        ui,
                        events,
                        anchors,
                        dests,
                        pending_anchor,
                        doc_salt,
                        selected,
                        interactive,
                        viewport_w,
                        type_scale,
                        images,
                        emoji,
                        on_link,
                        links,
                        find,
                        doc_generation,
                    );
                });
            });
        });
    viewport
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_find;
