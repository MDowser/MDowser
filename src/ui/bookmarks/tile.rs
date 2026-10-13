// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::bookmarks::tile`: one Home bookmark tile and its inline editor.
//!
//! - `bookmark_tile` paints the icon/favicon/letter, the two-row elided
//!   title and the hover actions (pencil/check/X) and reports a
//!   `TileAction` (open / new tab / remove / reorder drag).
//! - `inline_field` is the in-place rename editor (no dialog); on commit
//!   the store sanitizes the name. The draft paints at `TITLE_SIZE` (the
//!   tile name's own size), never the 16px `Body` default.
//!
//! The grid itself (geometry, layout, shortcuts) lives in the parent.

use super::*;

/// Tile outcome for the Home grid.
pub(super) enum TileAction {
    /// Nothing happened.
    None,
    /// Navigate the tab showing Home to this bookmark; `new_tab` asks `App`
    /// for a new tab instead (Cmd/Ctrl+click).
    Open { new_tab: bool },
    /// Pencil: open the inline rename field on this tile.
    Rename,
    /// The save check (the pencil while the field is open): commit the
    /// draft.
    Save,
    /// The open field just lost focus: apply its draft when `commit`
    /// (Enter or a click elsewhere), drop it on Esc.
    Finish { commit: bool },
    /// This tile just started being dragged (reorder).
    DragStart,
    /// The pointer is over this tile while another one is dragged: it is
    /// the drop target.
    DragOver,
    /// The dragged tile was released.
    DragStop,
    /// Keyboard reorder (`Alt+Left`/`Alt+Right` on the focused tile): move
    /// the tile `delta` display slots (negative = toward the newest tile),
    /// so a keyboard-only user can arrange the grid like a drag.
    Move { delta: i32 },
    /// Remove this bookmark from the store.
    Remove,
}

/// One tile's fixed layout slot and drag state for this pass. Bundled so
/// `bookmark_tile` stays under the `too_many_arguments` budget.
#[derive(Clone, Copy)]
pub(super) struct TileRef<'a> {
    /// Store index (position in `entries`, oldest first).
    pub(super) index: usize,
    /// The bookmark itself.
    pub(super) bookmark: &'a Bookmark,
    /// Column width for this pass.
    pub(super) tile_w: f32,
    /// Is this tile the one being dragged?
    pub(super) dragging: bool,
    /// Is some tile being dragged right now?
    pub(super) drag_active: bool,
    /// Extra right inset for the action buttons (the rightmost column keeps
    /// the floating scrollbar's lane clear).
    pub(super) right_inset: f32,
    /// Store revision (`BookmarkStore::generation`): invalidates the laid-out
    /// name memo when a title or glyph changes.
    pub(super) generation: u64,
}
/// Stable avatar color: the stored palette index, else a deterministic hash
/// of the URL so the same site always gets the same tile color.
pub(super) fn avatar_color(bookmark: &Bookmark) -> egui::Color32 {
    let index = match bookmark.color {
        Some(index) if (index as usize) < PALETTE.len() => index as usize,
        _ => (crate::media::fnv1a(&bookmark.url) % PALETTE.len() as u64) as usize,
    };
    PALETTE[index]
}

/// First alphanumeric character of the title (or URL) for the letter
/// avatar, uppercased.
///
/// Returns a `char` (encoded into a stack buffer at the call site): the
/// avatar is painted per frame per visible tile, and a `String` per tile
/// per frame was pure waste. `to_uppercase` can expand to several chars
/// (`ß` -> "SS"); the avatar keeps the first.
pub(super) fn avatar_letter(bookmark: &Bookmark) -> char {
    let source = bookmark.title.as_deref().unwrap_or(&bookmark.url);
    strip_scheme_prefix(source)
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .unwrap_or('?')
}

/// Ready favicon texture for a bookmark (HTTPS only; local files have
/// none). Requests the probe on first call; the shared cache answers after
/// that. The cache keys by the origin derived from the URL string, so a
/// settled tile costs one hash probe per frame instead of a full
/// `Url::parse`.
pub(super) fn favicon_texture(
    bookmark: &Bookmark,
    favicons: &mut FaviconCache,
) -> Option<egui::TextureHandle> {
    favicons.texture_for_url(&bookmark.url)
}

/// Human name of one tile for labels and accessible names: the stored
/// title, or the schemeless URL when the bookmark has none. Borrowed
/// (`Cow`) so the per-frame path allocates nothing on the common case.
fn tile_display_name(bookmark: &crate::bookmarks::Bookmark) -> std::borrow::Cow<'_, str> {
    match &bookmark.title {
        Some(title) => std::borrow::Cow::Borrowed(title.as_str()),
        None => strip_scheme_prefix(&bookmark.url),
    }
}

/// One quick-access tile: icon (custom glyph / favicon / letter avatar)
/// above the title. Hovering reveals the pencil (inline rename) and the X
/// (remove); the whole tile -- icon and name -- opens the page, and
/// dragging it reorders the grid. All editing happens ON the tile.
pub(super) fn bookmark_tile(
    ui: &mut egui::Ui,
    tile: &TileRef<'_>,
    edit_id: egui::Id,
    mut edit: Option<&mut TileEdit>,
    favicons: &mut FaviconCache,
) -> TileAction {
    let TileRef {
        index,
        bookmark,
        tile_w,
        dragging,
        drag_active,
        right_inset,
        generation,
    } = *tile;
    let s = theme::scheme_ref(ui);
    let editing = edit.is_some();
    // The tile is clickable (open the page) and draggable (reorder) unless
    // the rename field is open: then a plain click saves/closes through the
    // field and must never start a drag.
    let sense = if editing {
        egui::Sense::click()
    } else {
        egui::Sense::click_and_drag()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(tile_w, TILE_H), sense);
    let response = if dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        response
    } else if editing {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    // Deferred: a drag event must not skip painting the tile this frame.
    let mut drag_action: Option<TileAction> = None;
    if response.drag_started() {
        drag_action = Some(TileAction::DragStart);
    } else if response.drag_stopped() {
        drag_action = Some(TileAction::DragStop);
    } else if drag_active && !dragging && response.contains_pointer()
    // `contains_pointer` is geometric, so it works under the captured
    // pointer where `hovered` is false.
    {
        drag_action = Some(TileAction::DragOver);
    }
    // `contains_pointer` (not `hovered`): the action buttons are separate
    // widgets on top, and the highlight must stay while the pointer sits on
    // any of them.
    if response.contains_pointer() || response.has_focus() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::TILE),
            s.container_high,
        );
    }
    if dragging {
        ui.painter().rect_stroke(
            rect.shrink(0.5),
            egui::CornerRadius::same(theme::radius::TILE),
            egui::Stroke::new(1.0, s.primary),
            egui::StrokeKind::Inside,
        );
    } else if response.has_focus() {
        // Keyboard focus is otherwise only the hover wash, which reads as
        // "the pointer happens to be here": draw the same primary ring every
        // other custom-painted row uses.
        theme::focus_ring_with(ui, rect, theme::radius::TILE, s);
    }

    // Icon slot: the custom glyph wins, then the site favicon, then a
    // colored circle with the first letter of the title.
    let icon_center = egui::pos2(rect.center().x, rect.min.y + ICON_TOP + ICON_SIZE * 0.5);
    let icon_rect = egui::Rect::from_center_size(icon_center, egui::vec2(ICON_SIZE, ICON_SIZE));
    let custom = bookmark.icon.as_deref();
    let favicon = if custom.is_none() {
        favicon_texture(bookmark, favicons)
    } else {
        None
    };
    let circle = if custom.is_some() || favicon.is_none() {
        avatar_color(bookmark)
    } else {
        s.container_high
    };
    ui.painter()
        .circle_filled(icon_center, ICON_SIZE * 0.5, circle);
    // Clip the glyph to the icon slot: a long custom string (or a wide
    // fallback font run) must never spill over the tile.
    ui.scope(|ui| {
        ui.set_clip_rect(icon_rect.expand(2.0));
        if let Some(glyph) = custom {
            icons::paint_text(ui, icon_rect.shrink(10.0), glyph, egui::Color32::WHITE);
        } else if let Some(texture) = favicon {
            // Favicons keep a comfortable margin inside the circle, at the
            // app's one favicon size (the tab rail's), fitted to their
            // aspect instead of stretched into the square slot.
            let art = egui::Rect::from_center_size(icon_center, egui::vec2(FAVICON, FAVICON));
            crate::ui::paint_texture_fit(ui, art, &texture, 0);
        } else {
            let letter = avatar_letter(bookmark);
            let mut buf = [0u8; 4];
            icons::paint_text(
                ui,
                icon_rect.shrink(10.0),
                letter.encode_utf8(&mut buf),
                egui::Color32::WHITE,
            );
        }
    });

    // Title: the inline rename field while open, else up to two centered
    // lines, ellipsized by the LAYOUT when they do not fit. The box holds
    // both rows of `TITLE_SIZE` text (see `TILE_H`/`TITLE_TOP`).
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 4.0, rect.min.y + TITLE_TOP),
        egui::pos2(rect.max.x - 4.0, rect.max.y - TITLE_BOTTOM_GUARD),
    );
    if let Some(state) = edit.as_mut() {
        let focus = std::mem::take(&mut state.focus);
        // The field is a ONE-LINE pill centered in the name slot: it hugs
        // the draft (the line plus `FIELD_PAD` above and below) instead of
        // filling the whole two-line slot, which left a wide band of empty
        // input around the text.
        let field_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(TITLE_SIZE)))
            + 2.0 * FIELD_PAD;
        let field_rect = egui::Rect::from_center_size(
            title_rect.center(),
            egui::vec2(title_rect.width(), field_h),
        );
        let field = inline_field(
            ui,
            field_rect,
            edit_id.with(("name-field", bookmark.url.as_str())),
            &mut state.draft,
            "Automatic",
            crate::config::MAX_TITLE_CHARS,
            focus,
        );
        if field.lost_focus() {
            return TileAction::Finish {
                commit: !ui.input(|i| i.key_pressed(egui::Key::Escape)),
            };
        }
    } else {
        let title = tile_display_name(bookmark);
        // Character cap first (cheap layout bound), then a real layout cap:
        // TWO rows with `…` appended by epaint when the text still does not
        // fit. The char cap alone is not enough (CJK/emoji and long paths
        // are wider than the estimate), and the old clip rect silently cut
        // the third row mid-glyph with no ellipsis.
        let title = truncate_chars(&title, TITLE_CHARS);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
        // Laid-out name memo: Home rebuilds every visible tile every frame,
        // and the old path allocated one String (`into_owned`) plus one
        // LayoutJob per tile per frame. Keyed by URL (one entry per bookmark,
        // whatever the window width) and invalidated by the store generation,
        // the theme and the tile width.
        let dark = ui.visuals().dark_mode;
        let width = title_rect.width();
        let memo_id = egui::Id::new(("bookmark-name-galley", bookmark.url.as_str()));
        type NameGalley = (u64, bool, u32, std::sync::Arc<egui::Galley>);
        let cached = ui.ctx().data(|d| d.get_temp::<NameGalley>(memo_id));
        let galley = match cached {
            Some((cached_generation, cached_dark, cached_w, galley))
                if cached_generation == generation
                    && cached_dark == dark
                    && cached_w == width.to_bits() =>
            {
                galley
            }
            _ => {
                let mut job = egui::text::LayoutJob::simple(
                    title.into_owned(),
                    egui::FontId::proportional(TITLE_SIZE),
                    s.on_surface,
                    width,
                );
                job.halign = egui::Align::Center;
                job.wrap.max_rows = 2;
                job.wrap.overflow_character = Some('\u{2026}');
                let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
                ui.ctx().data_mut(|d| {
                    d.insert_temp(memo_id, (generation, dark, width.to_bits(), galley.clone()))
                });
                galley
            }
        };
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(title_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                // Clip to the title box: a title taller than the two
                // laid-out rows (rounding, decorations) must never paint
                // over the next tile row.
                ui.set_clip_rect(title_rect);
                ui.add(egui::Label::new(galley).selectable(false));
            },
        );
    }

    // Hover/focus actions: pencil (rename; the save check while the field is
    // open) and X (remove) at the top-right. Hidden during a drag, so they
    // never flash over a reorder.
    // `contains_pointer` (not `hovered`): once the pointer sits on a button,
    // the tile stops being the topmost hovered widget, but the buttons must
    // stay alive to receive the click.
    let mut edit_rect = None;
    let mut remove_rect = None;
    if !drag_active && (response.contains_pointer() || response.has_focus() || editing) {
        // Stacked at the top-right corner: the X on top and the pencil (the
        // save check while the field is open) below, so both stay clear of
        // the tile icon on narrow tiles. `right_inset` keeps the rightmost
        // column clear of the floating scrollbar.
        let x = rect.max.x - right_inset - ACTION_INSET - ACTION_SIZE * 0.5;
        let remove_center = egui::pos2(x, rect.min.y + ACTION_INSET + ACTION_SIZE * 0.5);
        let edit_center = egui::pos2(x, remove_center.y + ACTION_SIZE + ACTION_GAP);
        let er = egui::Rect::from_center_size(edit_center, egui::vec2(ACTION_SIZE, ACTION_SIZE));
        let rr = egui::Rect::from_center_size(remove_center, egui::vec2(ACTION_SIZE, ACTION_SIZE));
        edit_rect = Some(er);
        remove_rect = Some(rr);
        let (icon, tip) = if editing {
            (icons::Icon::Check, "Save changes")
        } else {
            (icons::Icon::Edit, "Rename")
        };
        // Per-tile accessible names: a grid of identical "Rename"/"Remove
        // bookmark" buttons is unusable with a screen reader. Both closures
        // build their strings only when accesskit asks.
        let tile_name = tile_display_name(bookmark);
        if icons::icon_button_at_named_lazy(
            ui,
            er,
            ("bookmark-edit", index),
            icon,
            || {
                if editing {
                    format!("Save the new name for {tile_name}")
                } else {
                    format!("Rename {tile_name}")
                }
            },
            move || tip.to_string(),
            true,
        )
        .clicked()
        {
            return if editing {
                TileAction::Save
            } else {
                TileAction::Rename
            };
        }
        if icons::icon_button_at_named_lazy(
            ui,
            rr,
            ("bookmark-remove", index),
            icons::Icon::Close,
            || format!("Remove bookmark {tile_name}"),
            || "Remove bookmark".to_string(),
            true,
        )
        .clicked()
        {
            return TileAction::Remove;
        }
    }
    if let Some(action) = drag_action {
        return action;
    }
    // Keyboard reorder: Alt+Left/Right move the FOCUSED tile one display
    // slot (the grid's visual order), the keyboard equivalent of the drag
    // reorder. One slot per press keeps the mapping to the mirrored store
    // order trivial and needs no column count.
    if !editing && response.has_focus() {
        let delta = ui.input(|i| {
            if !i.modifiers.alt {
                0
            } else if i.key_pressed(egui::Key::ArrowLeft) {
                -1
            } else if i.key_pressed(egui::Key::ArrowRight) {
                1
            } else {
                0
            }
        });
        if delta != 0 {
            return TileAction::Move { delta };
        }
    }
    if response.clicked() {
        // Defensive: if an action hit test lost to the tile (overlapping
        // widgets), a click on the pencil/X must never open the page.
        if response.interact_pointer_pos().is_some_and(|pos| {
            edit_rect.is_some_and(|r| r.contains(pos))
                || remove_rect.is_some_and(|r| r.contains(pos))
        }) {
            return TileAction::None;
        }
        return TileAction::Open {
            new_tab: ui.input(|i| i.modifiers.command),
        };
    }
    TileAction::None
}

/// One inline tile field (rename): a rounded input painted like the
/// omnibox (input surface, 1px accent ring while focused) with a frameless
/// `TextEdit` inside, so editing never leaves the tile. The draft paints
/// at `TITLE_SIZE`, the name's own size, so opening the editor does not
/// jump to a bigger font; `rect` is the one-line pill the caller sized
/// (`FIELD_PAD` above and below the line).
///
/// `select_all` focuses and selects the draft on the opening frame. The
/// draft stays CENTERED while it fits (matching the centered name it
/// replaces) and switches to a left-aligned input once it is wider than
/// the field: only that path runs egui's caret-follow scroll, so the end
/// being typed stays in view instead of the text sitting still with its
/// tail cut off.
pub(super) fn inline_field(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    char_limit: usize,
    select_all: bool,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let focused = ui.memory(|m| m.has_focus(id));
    let stroke = if focused {
        egui::Stroke::new(1.0, s.primary)
    } else {
        egui::Stroke::new(1.0, s.outline_variant)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(theme::radius::MENU),
        s.input_bg,
        stroke,
        egui::StrokeKind::Inside,
    );
    if select_all {
        focus_and_select_all(ui.ctx(), id, text);
    }
    let inner = rect.shrink(FIELD_PAD);
    let font = egui::FontId::proportional(TITLE_SIZE);
    // Layer the draft once to know whether it fits the field (the same
    // size/family it paints with); an empty draft lays out to 0 and stays
    // centered with its hint.
    let draft_w = ui.fonts_mut(|f| {
        f.layout_no_wrap(text.clone(), font.clone(), s.on_surface)
            .size()
            .x
    });
    let align = draft_align(draft_w, inner.width());
    let field = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .id(id)
                        // The draft keeps the tile name's size: the default
                        // resolves to `TextStyle::Body` (16px here), so
                        // opening the editor looked like a much bigger font
                        // than the name it replaces.
                        .font(font)
                        .frame(egui::Frame::new())
                        .margin(egui::Margin::ZERO)
                        .desired_width(inner.width())
                        .min_size(egui::vec2(0.0, inner.height()))
                        .char_limit(char_limit)
                        .text_color(s.on_surface)
                        .vertical_align(egui::Align::Center)
                        .horizontal_align(align)
                        .hint_text(egui::RichText::new(hint).color(s.on_surface_variant)),
                )
            },
        )
        .inner;
    // The TextEdit's own accessible info carries the draft value only: give
    // the field a purpose label for screen readers.
    field.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Bookmark name")
    });
    field
}

/// Alignment for the inline field's draft: centered while it fits `field_w`,
/// left-aligned once it is wider. Left alignment is what puts egui's
/// singleline field on its caret-follow path: centered text that no longer
/// fits sat still and showed a cut-off middle, so the end being typed was
/// invisible and never scrolled into view.
pub(super) fn draft_align(draft_w: f32, field_w: f32) -> egui::Align {
    if draft_w <= field_w {
        egui::Align::Center
    } else {
        egui::Align::Min
    }
}

/// Requests keyboard focus for `id` and selects the whole draft, so the
/// first keystroke replaces it (standard inline-rename behavior).
pub(super) fn focus_and_select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    ctx.memory_mut(|m| m.request_focus(id));
    crate::ui::select_all_in(ctx, id, text);
}
