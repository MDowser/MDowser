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
//! - Document pages carry the always-visible outline sidebar
//!   (`outline_panel`, layout) on the right; pages without headings keep the
//!   slot with an empty state, and a loading page keeps it over its backdrop,
//!   so navigation never reflows the sheet.
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
// `pub(crate)`: `browser::tab` holds the per-tab state (the panel itself is
// private to the viewer).
pub(crate) mod outline;
mod outline_panel;
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
/// document renders so the right-click menu can offer "Open link in new tab"
/// and "Copy link address", and the hover chip can show the destination.
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
    /// Pointer is on this fragment this frame. Overlays sit on higher layers,
    /// so `Response::hovered` also turns this off when a floating surface
    /// (menu, find bar, toast) covers the link.
    pub(crate) hovered: bool,
}

/// Pushes one link hitbox, bounded by [`crate::config::MAX_LINK_HITS`].
/// Clicking and navigation never depend on the hitboxes: past the cap the
/// link still works, only the right-click link rows and the hover destination
/// chip are missing. This bounds the per-frame URL copies the hit list
/// retains.
pub(crate) fn push_link_hit(
    links: &mut Vec<LinkHit>,
    rect: egui::Rect,
    url: &std::sync::Arc<str>,
    hovered: bool,
) {
    if links.len() < crate::config::MAX_LINK_HITS {
        links.push(LinkHit {
            rect,
            url: std::sync::Arc::clone(url),
            hovered,
        });
    }
}

/// How the reader activated a rendered link. The gesture decides where the
/// destination opens: a primary click (or Enter on a focused link) navigates
/// the current tab, while a middle click opens a background tab, like a
/// browser. Cmd/Ctrl with the primary gesture (or Enter) asks for the same
/// background tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkActivation {
    CurrentTab,
    BackgroundTab,
}

/// Returns how `resp` activated its link this frame, if it did.
///
/// The one place that maps pointer gestures to link intent: every rendered
/// link (text, code, heading, footnote marker, image) calls this instead of
/// `Response::clicked`, so middle-click behaves the same everywhere and the
/// `on_link` callbacks only route the already-classified request.
pub(crate) fn link_activation(resp: &egui::Response) -> Option<LinkActivation> {
    if resp.middle_clicked() {
        Some(LinkActivation::BackgroundTab)
    } else if resp.clicked() {
        if resp.ctx.input(|i| i.modifiers.command) {
            Some(LinkActivation::BackgroundTab)
        } else {
            Some(LinkActivation::CurrentTab)
        }
    } else {
        None
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
        // The outline sidebar is layout, so it must claim its width BEFORE
        // the sheet (and before the find bar decides where to dock).
        show_document_sidebar(root, tab);
        // MENU FIRST, exactly like `Loaded` (it pushes `Event::Copy` for a
        // native selection before the labels render below).
        let new_tab_url = overlays::content_menu(root, tab);
        // Find bar before the document so a typed query highlights the same
        // frame (no one-frame lag between the bar and the page).
        overlays::find::show_find_bar(root, tab);
        let gutter = theme::content_gutter(root);
        let mut pending_link: Option<String> = None;
        // Background-tab target this frame (middle click or Cmd/Ctrl+click):
        // opened by the caller, like the context menu's "Open link in new
        // tab" and the bookmark tiles.
        let mut pending_new_tab: Option<String> = None;
        let mut new_rect: Option<egui::Rect> = None;
        let selected = tab.selected_all;
        let doc = crate::licenses::document();
        egui::CentralPanel::default()
            .frame(theme::content_frame(root))
            .show(root, |ui| {
                let Some(doc) = doc else {
                    return;
                };
                let mut on_link = |dest: &str, activation: LinkActivation| {
                    // The notices carry no `#` targets: every live
                    // destination is a navigation, never an anchor jump.
                    if !dest.is_empty() && !dest.starts_with('#') {
                        if activation == LinkActivation::BackgroundTab {
                            pending_new_tab = Some(dest.to_string());
                        } else {
                            pending_link = Some(dest.to_string());
                        }
                    }
                };
                // Manual scrolling (wheel or scrollbar drag) releases a
                // clicked row BEFORE the document walk runs: the sheet's
                // `ScrollArea` consumes the wheel delta and zeroes it.
                let manual_scroll = outline_manual_scroll(ui.ctx(), ui.max_rect());
                let viewport = render_doc(
                    ui,
                    &doc.events,
                    &doc.display_url,
                    &doc.anchors,
                    &doc.dests,
                    // The notices page has a real outline (its `#` headings):
                    // panel rows scroll it through the same `pending_anchor`
                    // mechanism as a `#id` link.
                    &mut tab.pending_anchor,
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
                    &mut tab.outline,
                    &mut on_link,
                );
                // Scrollspy: pick the section the reader is in from the
                // heading rects this frame's walk just recorded.
                if manual_scroll {
                    tab.outline.unpin();
                }
                tab.outline.update_current(
                    &doc.outline,
                    viewport.rect.top(),
                    viewport.rect.bottom(),
                    viewport.end_progress,
                    viewport.sheet_settled,
                );
                // Same hit rect as `Loaded`: backs the context menu and
                // excludes the vertical scrollbar.
                let bar = theme::scrollbar_width(ui);
                new_rect = Some(egui::Rect {
                    min: viewport.rect.min,
                    max: egui::pos2(
                        (viewport.rect.max.x - bar).max(viewport.rect.min.x),
                        viewport.rect.max.y,
                    ),
                });
                let scrolled = ui.ctx().input(|i| i.smooth_scroll_delta.y.abs() > 0.0);
                if !scrolled {
                    theme::paint_content_fade(ui, viewport.rect);
                }
            });
        tab.doc_rect = new_rect;
        overlays::show_link_status(root, tab);
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
        // Keep the sidebar slot while the previous page is still on screen:
        // the reader clicked a link, not a different layout, so the sheet
        // must not widen (and then narrow again on resolve). The panel shows
        // a short loading state; the backdrop's own outline belongs to the
        // page being replaced.
        show_document_sidebar(root, tab);
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
                    let mut noop = |_: &str, _: crate::viewer::LinkActivation| {};
                    // Backdrop is static until resolve: skip the fade
                    // overlay (16 extra rects per throttled repaint). Its
                    // anchors are inert (throwaway `pending_anchor`).
                    let mut no_anchor = None;
                    // The backdrop belongs to the page being replaced: its
                    // headings must not feed the new page's outline cursor
                    // (and the sidebar itself is not drawn here).
                    let mut no_outline = crate::viewer::outline::OutlineState::default();
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
                        &mut no_outline,
                        &mut noop,
                    );
                }
            });
        overlays::show_toast(root, tab);
        return None;
    }

    // LOADED: the document alone, no metadata or headers (minimal).
    if let TabState::Loaded { .. } = &tab.state {
        // The outline sidebar is layout, so it claims its width before the
        // sheet and before the find bar chooses its dock.
        show_document_sidebar(root, tab);
        // MENU FIRST: it uses a global secondary-click detector + foreground
        // `Area` (works on text, never blocks links) and, on `Copy`, pushes
        // `Event::Copy` into this frame's input so the text labels below --
        // which still hold the native selection -- perform the copy.
        let new_tab_url = overlays::content_menu(root, tab);
        // Find bar before the document (same reason as `Licenses`).
        overlays::find::show_find_bar(root, tab);
        let gutter = theme::content_gutter(root);
        // Render the document (no background `interact` behind: it either
        // ate link clicks or never saw secondary clicks on text).
        let mut pending_link: Option<String> = None;
        // Background-tab target this frame (middle click or Cmd/Ctrl+click):
        // opened by the caller (`app::run`), like the context menu's entry
        // and the bookmark tiles.
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
                    let mut on_link = |dest: &str, activation: LinkActivation| {
                        if dest.is_empty() {
                            return;
                        }
                        if activation == LinkActivation::BackgroundTab {
                            // Middle click or Cmd/Ctrl+click: a background
                            // tab. An in-document anchor becomes its own
                            // absolute URL plus fragment.
                            if let Some(id) = dest.strip_prefix('#').filter(|id| !id.is_empty()) {
                                let mut url = fetched.display_url.clone();
                                url.set_fragment(Some(id));
                                pending_new_tab = Some(url.to_string());
                            } else {
                                pending_new_tab = Some(dest.to_string());
                            }
                            return;
                        }
                        // Primary click: an in-document anchor jumps, every
                        // other destination navigates this tab.
                        match dest.strip_prefix('#') {
                            Some(id) if !id.is_empty() => {
                                pending_anchor_click = Some(id.to_string());
                            }
                            _ => {
                                pending_link = Some(dest.to_string());
                            }
                        }
                    };
                    // Manual scrolling (wheel or scrollbar drag) releases a
                    // clicked row BEFORE the document walk runs: the sheet's
                    // `ScrollArea` consumes the wheel delta and zeroes it.
                    let manual_scroll = outline_manual_scroll(ui.ctx(), ui.max_rect());
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
                        &mut tab.outline,
                        &mut on_link,
                    );
                    // Scrollspy: pick the section the reader is in from the
                    // heading rects this frame's walk just recorded. A frame
                    // that painted no heading keeps the previous cursor.
                    if manual_scroll {
                        tab.outline.unpin();
                    }
                    tab.outline.update_current(
                        &fetched.outline,
                        viewport.rect.top(),
                        viewport.rect.bottom(),
                        viewport.end_progress,
                        viewport.sheet_settled,
                    );
                    // Shrink right edge for hit-testing so clicks on the
                    // vertical scrollbar never open the text menu (same
                    // width the fade overlay reserves).
                    let bar = theme::scrollbar_width(ui);
                    let hit_rect = egui::Rect {
                        min: viewport.rect.min,
                        max: egui::pos2(
                            (viewport.rect.max.x - bar).max(viewport.rect.min.x),
                            viewport.rect.max.y,
                        ),
                    };
                    new_rect = Some(hit_rect);
                    // Fade only at rest (~no scroll): while scrolling it
                    // costs a few rects per frame for an effect hidden by motion.
                    let scrolled = ui.ctx().input(|i| i.smooth_scroll_delta.y.abs() > 0.0);
                    if !scrolled {
                        theme::paint_content_fade(ui, viewport.rect);
                    }
                }
            });
        tab.doc_rect = new_rect;
        overlays::show_link_status(root, tab);
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
        // A failed navigation replaced any loading backdrop: the sidebar slot
        // (and its panel rect) belongs to document pages only.
        tab.outline.panel_rect = None;
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
                        // Same centered reading column as a document: Copy log
                        // and Retry belong to the text, not to the sheet's
                        // borders (at the window edge Copy log could even share
                        // the floating scrollbar's hit lane, so pressing near
                        // its edge panned the page instead of copying).
                        let avail = ui.available_width();
                        let col_w = theme::content_max_width(avail).max(0.0);
                        let pad = ((avail - col_w) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if pad > 0.5 {
                                ui.add_space(pad);
                            }
                            ui.vertical(|ui| {
                                ui.set_max_width(col_w);
                                ui.set_min_width(col_w);
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
                                            format!(
                                                "Retry in {}s (cooldown...)",
                                                tab.refresh_remaining()
                                            )
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
                        });
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

/// Draws the always-visible outline sidebar for the page on screen.
///
/// Called BEFORE the reader's central panel: the sidebar is layout, not an
/// overlay. Every document page keeps the slot, including one without
/// headings and the backdrop still visible while the next page loads, so
/// navigation never widens the sheet. A clicked row asks for the same anchor
/// scroll a `#id` link does; the state was already updated by the panel for
/// immediate feedback.
fn show_document_sidebar(root: &mut egui::Ui, tab: &mut Tab) {
    // Field access keeps the borrow of `tab.state` disjoint from the mutable
    // sidebar state below.
    let outline = match &tab.state {
        TabState::Loaded { fetched } => &fetched.outline,
        TabState::Licenses => match crate::licenses::document() {
            Some(doc) => &doc.outline,
            None => {
                tab.outline.panel_rect = None;
                return;
            }
        },
        TabState::Loading {
            backdrop: Some(_), ..
        }
        | TabState::Settling {
            backdrop: Some(_), ..
        } => {
            // The old document is still visible as the backdrop but its
            // outline state was reset by the navigation. Reserve the slot
            // with a placeholder instead of reusing rows that no longer own
            // the cursor.
            let empty = crate::markdown::outline::Outline::default();
            let _ = outline_panel::show(
                root,
                &empty,
                &mut tab.outline,
                tab.id,
                tab.doc_generation,
                "Loading...",
            );
            return;
        }
        _ => {
            tab.outline.panel_rect = None;
            return;
        }
    };
    let response = outline_panel::show(
        root,
        outline,
        &mut tab.outline,
        tab.id,
        tab.doc_generation,
        "No headings",
    );
    if let Some(id) = response.clicked {
        tab.pending_anchor = Some(id);
    }
}

/// True when the reader is scrolling the sheet by hand: a wheel/trackpad
/// delta or a primary press/drag inside the visible sheet (which starts a
/// scrollbar drag). A click pin set by the outline sidebar must not survive
/// this: the reader is no longer heading to the row they clicked.
///
/// Call this BEFORE the document walk: the sheet's `ScrollArea` both reads
/// and WRITES `smooth_scroll_delta` and zeroes the axis it consumes, so a
/// check after the walk never sees a wheel scroll.
fn outline_manual_scroll(ctx: &egui::Context, viewport: egui::Rect) -> bool {
    ctx.input(|i| {
        i.smooth_scroll_delta.y.abs() > 0.0
            || (i.pointer.primary_down()
                && i.pointer
                    .hover_pos()
                    .is_some_and(|pos| viewport.contains(pos)))
    })
}

/// What one document walk tells the outline scrollspy about the viewport.
pub(crate) struct DocViewport {
    /// Visible sheet rect (screen coordinates).
    pub(crate) rect: egui::Rect,
    /// How far the sheet has travelled through its LAST screenful: `0` until
    /// the final viewport of scroll begins, `1` at the maximum offset. The
    /// scrollspy lowers its reading line from the viewport top to its bottom
    /// across this stretch, so headings that can never reach the top (the
    /// page-end tail) still become current in document order.
    pub(crate) end_progress: f32,
    /// The sheet's offset did not move since the previous walk and it has no
    /// kinetic velocity left. A clicked row's pin is released only on such a
    /// frame, so the reading line crossing that row while the click's own
    /// scroll is still running cannot hand the cursor to a deeper section.
    pub(crate) sheet_settled: bool,
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
///
/// `outline` is the tab's outline sidebar state: the walk records every
/// rendered heading's rect into it (keyed by the heading's own event index)
/// and the caller recomputes the scrollspy cursor from those rects right
/// after.
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
    outline: &mut crate::viewer::outline::OutlineState,
    on_link: &mut dyn FnMut(&str, LinkActivation),
) -> DocViewport {
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
    let output = scroll
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
                        Some(outline),
                    );
                });
            });
        });
    // Register this (tab, document) scroll state so closing the tab drops
    // it: egui keys the offset by (tab, URL) for Back/Forward and refresh,
    // and nothing else knows the real id of each visited document.
    crate::ui::remember_scroll(ui.ctx(), crate::ui::DOC_SCROLL_ID, tab_id, output.id);
    // Page end: within the sheet's LAST screenful the spy lowers its
    // reading line from the viewport top to its bottom, so headings that can
    // never reach the top (there is not enough content left below them) are
    // still selected in order. The stretch starts where the remaining scroll
    // equals one viewport (or at the top when the document scrolls less than
    // that), and reaches 1 at the maximum offset. A sheet that does not
    // scroll at all keeps the line at the top.
    let max_offset = (output.content_size.y - output.inner_rect.height()).max(0.0);
    let ramp = max_offset.min(viewport.height().max(0.0));
    let end_progress = if ramp > 1.0 {
        ((output.state.unclamped_offset().y - (max_offset - ramp)) / ramp).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sheet_settled =
        outline.sheet_settled(output.state.unclamped_offset().y, output.state.velocity().y);
    DocViewport {
        rect: viewport,
        end_progress,
        sheet_settled,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_find;
#[cfg(test)]
mod tests_outline;
