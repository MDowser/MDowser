// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::omnibox`: the address bar shared by the toolbar and the home page.
//!
//! Modern-browser shape: a fully rounded capsule with a left security
//! indicator (padlock for HTTPS, page for a local file), the field itself
//! (no `https://`, schemeless), and a right slot that holds the clear `X`
//! while editing or the bookmark ribbon otherwise (toolbar only): a filled
//! ribbon when the shown page is bookmarked, an outline ribbon when it is
//! not, disabled when there is nothing to bookmark. Enter navigates; Esc reverts
//! to the page actually shown and leaves the field; a clean click (or
//! keyboard focus) selects the whole address so typing replaces it, while a
//! drag selects from the press point.
//!
//! The indicator is a BUTTON, not a field focus target: clicking it (or
//! activating it from the keyboard) toggles a small page-info card under
//! the capsule with the facts of the page actually rendered
//! (`Tab::site_info`: kind, sanitized title, exact address with its
//! scheme). The card is an overlay; it closes on the next indicator
//! click, on Esc, on a click outside it, and on any navigation (the tab
//! clears its owner in `Tab::clear_overlay`), so it never describes a page
//! that is gone.
//!
//! Autocomplete: while the field owns the keyboard, a popup under the
//! capsule lists up to `config::MAX_OMNIBOX_SUGGESTIONS` matches from
//! bookmarks (newest first), history (newest first) and the curated builtin
//! list (`crate::suggest`), deduplicated in that priority. An empty field
//! offers recent bookmarks. No row is highlighted until the user picks one:
//! the first `Down` selects the first row, the first `Up` the last, and
//! later presses move through the list. `Tab` completes the highlighted row
//! into the field without navigating; with nothing highlighted it completes
//! the first row. `Enter` navigates to the typed address unless
//! `Down`/`Up`/a pointer that MOVED onto a row explicitly picked one (only
//! then the selection wins; a pointer that merely sits over the popup does
//! not), `Esc` closes the popup first and reverts on the next press, and
//! clicking a row navigates to it. The popup is an overlay: it never shifts
//! the capsule or its neighbours.
//!
//! The caller owns the surrounding layout and passes the exact width/height:
//! the toolbar sizes the capsule against the zoom controls, the home page
//! puts a large one under the title. `id_salt` keeps the ids of the two
//! instances (toolbar + home) unique for the same tab.
//!
//! While the tab is loading, a small indeterminate progress segment sweeps
//! under the capsule so a first load is never a blank wait.

use crate::bookmarks::BookmarkStore;
use crate::browser::tab::{SiteInfo, Tab, TabState};
use crate::history::HistoryStore;
use crate::suggest::{self, Suggestion};
use crate::ui::{icons, theme};
use crate::util::{strip_scheme_prefix, truncate_chars};
use eframe::egui;

/// Bookmark button presentation state, computed by `App` from the bookmark store (the
/// omnibox never WRITES the store; it only reads both stores for suggestions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookmarkState {
    /// No loaded page: the ribbon is shown but disabled.
    Unavailable,
    /// A loaded page that is not bookmarked.
    NotBookmarked,
    /// A loaded page that is already bookmarked.
    Bookmarked,
}

/// What the omnibox reported this frame.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OmniboxResponse {
    /// Enter (typed address or explicitly selected suggestion) or a
    /// suggestion click: the caller navigates via `tab.navigate_from_address()`.
    /// An explicit selection or click presets `tab.address` first, so the
    /// caller needs no new path.
    pub(crate) submitted: bool,
    /// The bookmark ribbon was clicked: the caller toggles the bookmark.
    pub(crate) bookmark_toggled: bool,
}

/// Stable id of an address field (one per tab + surface).
pub(crate) fn address_id(tab_id: usize, salt: &'static str) -> egui::Id {
    egui::Id::new((salt, tab_id))
}

/// Cached suggestion list + selection for one address field. Lives in egui
/// memory (per tab + surface), never in `Tab`: it is derived view state,
/// like the History page filter cache. The query and the list sit behind
/// `Arc`s so a state write on hover/selection clones pointers, not strings.
#[derive(Debug, Clone, Default)]
struct SuggestState {
    /// False until the list was built once: the default (`""` query, gens 0)
    /// would otherwise skip the first build on an empty field.
    built: bool,
    /// Query the cached `items` were built for (`tab.address` then).
    query: std::sync::Arc<str>,
    /// Store revisions the cache was built on (re-filter only on change).
    hist_gen: u64,
    bm_gen: u64,
    /// Ranked suggestions for `query`.
    items: std::sync::Arc<Vec<Suggestion>>,
    /// Keyboard/hover selection index into `items`. It also seeds the first
    /// `Down`/`Up`, but is only PAINTED once `explicit` is set, so an
    /// untouched list shows no highlighted row.
    selected: usize,
    /// The user explicitly picked a row (`Down`/`Up`, or the pointer MOVED
    /// onto it): only then is the highlighted row PAINTED and `Enter` routed
    /// to it. Without an explicit pick the typed address is submitted,
    /// otherwise pressing Enter silently replaced the typed URL with the
    /// default highlighted row (editing a URL could "reload" the page already
    /// shown).
    explicit: bool,
    /// `Esc` dismissed the popup for the current query (typing clears it).
    dismissed: bool,
}

/// Drops the suggestion cache a closed tab parked, plus the address
/// fields' own edit state. Ids are never reused, so without this every
/// closed tab would keep its last list forever. The `TextEdit` state lives
/// in the PERSISTED store (`TextEditState::load` uses `get_persisted`),
/// not the temp store with the suggestion cache, so it needs its own
/// removal by type.
pub(crate) fn forget_tab_data(ctx: &egui::Context, tab_id: usize) {
    for salt in ["address-bar", "home-address"] {
        let base = address_id(tab_id, salt);
        ctx.data_mut(|d| {
            d.remove::<SuggestState>(base.with("suggest"));
            d.remove::<egui::Rect>(base.with("suggest").with("popup-rect"));
            d.remove::<bool>(base.with("select-all-armed"));
            // The page-info card's last rect (click-outside test).
            d.remove::<egui::Rect>(base.with("site-info").with("rect"));
            d.remove::<egui::text_edit::TextEditState>(base);
        });
    }
}

/// Left-indicator glyph: padlock (secure HTTPS), page (local file) or the
/// internal-page glyph (globe/history/info). Shared by the indicator and
/// its page-info card, so the two can never disagree.
fn indicator_icon(tab: &Tab) -> icons::Icon {
    if tab.is_local_file() {
        return icons::Icon::File;
    }
    match &tab.state {
        TabState::Home => icons::Icon::Globe,
        TabState::History => icons::Icon::History,
        TabState::Licenses => icons::Icon::Info,
        _ => icons::Icon::Lock,
    }
}

/// Draws the address capsule of `tab` at the given size.
///
/// Returns an [`OmniboxResponse`]: `submitted` on Enter or suggestion click
/// (the caller navigates via `tab.navigate_from_address()`) and
/// `bookmark_toggled` when the ribbon was clicked (the caller toggles the
/// bookmark in its store).
///
/// `focus`: request keyboard focus this frame (new tab / Ctrl+Cmd+L).
/// `id_salt`: `"address-bar"` (toolbar) / `"home-address"` (home), so both
/// fields of the same tab stay distinct widgets.
/// `bookmark`: ribbon state, or `None` to draw no ribbon (the home capsule).
/// `history`/`bookmarks`: read-only suggestion sources (never mutated here).
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_omnibox(
    ui: &mut egui::Ui,
    tab: &mut Tab,
    width: f32,
    height: f32,
    focus: bool,
    id_salt: &'static str,
    bookmark: Option<BookmarkState>,
    history: &HistoryStore,
    bookmarks: &BookmarkStore,
) -> OmniboxResponse {
    let mut bookmark_toggled = false;
    let s = theme::scheme_ref(ui);
    let id = address_id(tab.id, id_salt);
    if focus {
        ui.memory_mut(|m| m.request_focus(id));
        // Cmd/Ctrl+L on an already-focused field must select all again
        // (a plain focus request alone changes nothing then).
        crate::ui::select_all_in(ui.ctx(), id, &tab.address);
        // An explicit "edit the address" drops the page-info card: the
        // user is done asking what is on screen.
        tab.site_info = None;
    }
    // `Sense::CLICK` (not `click()`): clicks anywhere on the capsule focus
    // the address field, but the capsule itself never joins the keyboard
    // tab order — it draws no focus ring of its own, so a focusable capsule
    // was a silent, invisible stop before the field.
    let (rect, pill) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::CLICK);
    // Focus read BEFORE the field exists: a pointer press grants focus
    // INSIDE the `TextEdit`'s own code, so this value is one frame stale on
    // the click frame. It is used only to RESERVE the right slot (below),
    // never to paint; the capsule fill/ring is painted after the field with
    // the fresh state.
    let focused = ui.memory(|m| m.has_focus(id));

    // Capsule background + ring are painted with a `Noop` placeholder and
    // filled in AFTER the field, so a pointer focus shows its ring in the
    // SAME frame (the old fill/stroke used the stale pre-field focus and the
    // ring appeared one frame late).
    //
    // The ring is INSIDE the allocated rect: painted outside (the old
    // `StrokeKind::Outside`) it bled into the neighbouring toolbar buttons
    // (the bar uses 2px item spacing) and could never be reserved by the
    // layout, so focus looked like a 2px overlap glitch.
    let bg_idx = ui.painter().add(egui::Shape::Noop);

    // Left indicator: padlock (secure HTTPS), page (local file) or globe on
    // Home.
    let pad = 7.0;
    let indicator_box = 26.0;
    let indicator_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + pad + indicator_box * 0.5, rect.center().y),
        egui::vec2(indicator_box, indicator_box),
    );
    let local = tab.is_local_file();
    let indicator = indicator_icon(tab);
    let indicator_tip = if local {
        "Local file (on this device)"
    } else {
        match &tab.state {
            TabState::Home => "MDowser: HTTPS and local files",
            TabState::History => "Internal page: local history (on this device)",
            TabState::Licenses => "Internal page: third-party licenses (shipped with this build)",
            _ => "Secure connection (HTTPS only)",
        }
    };
    // Interact first, then paint: the hover fill sits BEHIND the glyph.
    let indicator_resp = ui
        .interact(indicator_rect, id.with("indicator"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // Lazy hint: the "click for page details" suffix is built only while
    // the pointer actually rests on the indicator.
    let indicator_resp = crate::ui::hover_tooltip_lazy(indicator_resp, || {
        format!("{indicator_tip}. Click for page details")
    });
    if indicator_resp.hovered() {
        // The capsule itself lightens on hover, so the indicator's own
        // feedback is one step stronger, matching `icons::icon_button`.
        ui.painter().rect_filled(
            indicator_rect.shrink(2.0),
            egui::CornerRadius::same(theme::radius::MENU),
            theme::press_shade(s.omnibox_hover, ui.visuals().dark_mode),
        );
    } else if indicator_resp.has_focus() {
        // The ring follows the SHRUNK indicator rect, so it cannot use the
        // whole-row `theme::focus_ring` helper directly.
        theme::focus_ring_stroke(
            ui,
            indicator_rect.shrink(2.0),
            theme::radius::MENU,
            s.primary,
        );
    }
    icons::paint(
        ui,
        indicator_rect.shrink(5.0),
        indicator,
        s.on_surface_variant,
    );
    // Accessible name: the painted indicator otherwise announces nothing
    // (the tooltip is not exposed to accesskit on its own).
    indicator_resp
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, indicator_tip));
    // Click (or Space/Enter on the focused indicator) toggles the
    // page-info card. It never focuses the field: the capsule padding and
    // the field itself already do that, and the indicator answers "what am
    // I looking at?".
    if indicator_resp.clicked() {
        tab.site_info = if tab.site_info == Some(id_salt) {
            None
        } else {
            Some(id_salt)
        };
    }

    // Page-info card: facts of the page actually rendered. Esc closes it
    // (App leaves the key while `Tab::site_info_open` reported it open),
    // a press anywhere outside it closes it in the same frame, and any
    // navigation closes it through `Tab::clear_overlay`.
    let mut site_info_escape = false;
    if tab.site_info == Some(id_salt) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.site_info = None;
        site_info_escape = true;
    }
    if tab.site_info == Some(id_salt) {
        let card_key = id.with("site-info").with("rect");
        // The click-outside test runs against the PREVIOUS frame's card
        // rect (the fresh one does not exist yet on the frame of the
        // press), so a press elsewhere closes the card immediately instead
        // of after one more painted frame.
        let last_card = ui.data_mut(|d| d.get_temp::<egui::Rect>(card_key));
        let pressed_outside = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|pos| {
                    !indicator_rect.contains(pos)
                        && !last_card.is_some_and(|card| card.contains(pos))
                })
        });
        if pressed_outside {
            tab.site_info = None;
        } else {
            let card = site_info_card(ui, tab, rect, id_salt);
            ui.data_mut(|d| d.insert_temp(card_key, card));
        }
    }

    // Right slot: ONE reserved box that holds the clear X while editing a
    // non-empty address, or the bookmark ribbon otherwise (toolbar only).
    // The reservation must not depend on the CURRENT frame's focus (read
    // before the field, so it is stale on the frame a pointer press grants
    // it): reserving on `focused || !address.is_empty()` keeps the text
    // area's right edge fixed across editing and focus changes. On Home the
    // address is empty until the field is focused, so the slot appears
    // exactly when the field gains focus (no clear X is drawn before that
    // anyway).
    let slot_needed = bookmark.is_some() || focused || !tab.address.is_empty();
    let slot_box = 26.0;
    let slot_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - pad - slot_box * 0.5, rect.center().y),
        egui::vec2(slot_box, slot_box),
    );
    let right_inset = if slot_needed { pad + slot_box } else { 2.0 };

    // The field itself: transparent, no frame of its own (the capsule is the
    // frame), vertically centered, leaving room for indicator + clear.
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(indicator_rect.max.x + 2.0, rect.min.y + 2.0),
        egui::pos2(rect.max.x - right_inset, rect.max.y - 2.0),
    );
    let field = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(text_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut tab.address)
                        .id(id)
                        .frame(egui::Frame::new())
                        .margin(egui::Margin::symmetric(2, 0))
                        .desired_width(text_rect.width().max(16.0))
                        .min_size(egui::vec2(0.0, (height - 4.0).max(16.0)))
                        // Bound typed/pasted text at the source: no amount of
                        // input can bloat `Tab.address` or the per-frame
                        // layout (submit-time validation still reports it).
                        .char_limit(crate::config::MAX_URL_INPUT_CHARS)
                        .text_color(s.on_surface)
                        .vertical_align(egui::Align::Center)
                        .hint_text(
                            egui::RichText::new("example.com/love").color(s.on_surface_variant),
                        ),
                )
            },
        )
        .inner;

    // Screen readers need a purpose label: the visible hint text is not
    // exposed as the accessible name.
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Address"));

    // Real edit detection: sets the dirty flag so `poll()` never clobbers
    // typing (robust even if the user types and reverts).
    if field.changed() {
        tab.mark_address_edited();
    }
    // Capsule fill + focus ring with the FRESH focus state: a pointer press
    // grants focus inside the `TextEdit`'s own code, so the pre-field read
    // above was a frame behind (the ring/fill used to appear one frame late).
    let focused_now = field.has_focus() || ui.memory(|m| m.has_focus(id));
    let fill = if focused_now {
        s.input_bg
    } else if pill.hovered() {
        s.omnibox_hover
    } else {
        s.omnibox_bg
    };
    let stroke = if focused_now {
        egui::Stroke::new(2.0, s.primary)
    } else {
        egui::Stroke::NONE
    };
    ui.painter().set(
        bg_idx,
        egui::Shape::Rect(egui::epaint::RectShape::new(
            rect,
            theme::pill_radius(height),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        )),
    );
    let show_clear = focused_now && !tab.address.is_empty();
    // Focusing selects the whole address (standard browser behavior) so
    // typing replaces the URL instead of appending to it. A POINTER focus
    // fires on the click's release instead: running `crate::ui::select_all_in` at the
    // end of the press frame overwrote the cursor the TextEdit had just
    // placed at the press point, re-anchoring every drag-selection at
    // character 0 — dragging could never select "from where the user
    // pressed". A drag now keeps the range built from the press point; a
    // clean click (no drag) still selects all.
    let armed_key = id.with("select-all-armed");
    if field.gained_focus() {
        if ui.input(|i| i.pointer.any_down()) {
            ui.data_mut(|d| d.insert_temp(armed_key, true));
        } else {
            crate::ui::select_all_in(ui.ctx(), id, &tab.address);
        }
    }
    if ui.input(|i| i.pointer.any_released()) {
        let armed = ui
            .data_mut(|d| d.remove_temp::<bool>(armed_key))
            .unwrap_or(false);
        if armed && field.clicked() {
            crate::ui::select_all_in(ui.ctx(), id, &tab.address);
        }
    }
    // Clicking the capsule padding (not the field itself) focuses the field.
    // The clear-X / ribbon slot is excluded: its own button handles that
    // pointer (focusing would swap the ribbon for the X).
    if pill.clicked() {
        if let Some(pos) = pill.interact_pointer_pos() {
            let in_slot = slot_needed && slot_rect.contains(pos);
            if !in_slot && !indicator_rect.contains(pos) {
                ui.memory_mut(|m| m.request_focus(id));
            }
        } else {
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    // --- Autocomplete: cached ranked list for this field. Re-filtered only
    // when the query or either store revision changes (the 20000-entry scan
    // runs on keystrokes, never per frame). The rebuild is gated on the field
    // EDITING: on Home both the toolbar and the capsule share one `address`,
    // and without the gate each keystroke rebuilt the whole list twice (a
    // second full history/bookmark scan per keystroke for a popup nobody saw).
    let editing = field.has_focus() || field.lost_focus();
    let suggest_id = id.with("suggest");
    let mut suggest: SuggestState = ui.data_mut(|d| d.get_temp(suggest_id).unwrap_or_default());
    let hist_gen = history.generation();
    let bm_gen = bookmarks.generation();
    if editing
        && (!suggest.built
            || suggest.query.as_ref() != tab.address.as_str()
            || suggest.hist_gen != hist_gen
            || suggest.bm_gen != bm_gen)
    {
        let query_changed = suggest.query.as_ref() != tab.address.as_str();
        suggest.built = true;
        suggest.query = std::sync::Arc::from(tab.address.as_str());
        suggest.hist_gen = hist_gen;
        suggest.bm_gen = bm_gen;
        suggest.items = std::sync::Arc::new(suggest::suggest(
            &tab.address,
            history.entries(),
            bookmarks.entries(),
        ));
        if query_changed {
            // A new query rebuilds the list for it: park the selection
            // cursor on the first row and re-arm a popup `Esc` dismissed
            // while typing.
            suggest.selected = 0;
            suggest.dismissed = false;
            // Typing invalidates an earlier choice: nothing is highlighted
            // (the cursor above is only a seed for the next `Down`/`Up`) and
            // `Enter` submits the typed address again until the user picks a
            // row.
            suggest.explicit = false;
        } else {
            // Same query, new store generation (a bookmark or history change
            // while the popup is open): keep the highlighted row, clamped to
            // the possibly shorter list.
            let last = suggest.items.len().saturating_sub(1);
            suggest.selected = suggest.selected.min(last);
        }
        ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
    }
    // The field counts as "editing" while it owns the keyboard OR lost it on
    // this very frame: egui surrenders a TextEdit's focus when the release of
    // a click lands elsewhere, and on Enter; both happen INSIDE the field
    // widget, before this code runs. Without the `lost_focus` arm the popup
    // would vanish exactly on the frame its row click or Enter completes.
    let mut popup_open = editing && !suggest.dismissed && !suggest.items.is_empty();

    // `Down`/`Up` move the selection while the popup is open. Consumed so
    // the keys never scroll the page behind the field; egui already queued a
    // focus-direction move for them during `begin_pass`, so that is cancelled
    // too (the field keeps the keyboard).
    if popup_open {
        let mut handled = false;
        let down = ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        if down && !suggest.items.is_empty() {
            // With nothing picked yet the first `Down` lands on the FIRST
            // row; the cursor already sits there, so incrementing would skip
            // it (and highlight a row the user never chose).
            suggest.selected = if suggest.explicit {
                (suggest.selected + 1) % suggest.items.len()
            } else {
                0
            };
            suggest.explicit = true;
            ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
            handled = true;
        }
        let up = ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        if up && !suggest.items.is_empty() {
            // Symmetric: the first `Up` picks the LAST row.
            suggest.selected = if suggest.explicit {
                (suggest.selected + suggest.items.len() - 1) % suggest.items.len()
            } else {
                suggest.items.len() - 1
            };
            suggest.explicit = true;
            ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
            handled = true;
        }
        // `Tab` completes the highlighted row (the first one when nothing is
        // highlighted yet) into the field without navigating (standard): keep
        // editing from the completed text. The TextEdit does
        // not lock `Tab`, so egui's own focus traversal must be cancelled or
        // the keyboard would leave the field along with the completion.
        // `Shift+Tab` (reverse traversal) is left alone.
        let shift = ui.ctx().input(|i| i.modifiers.shift);
        let tab_key = !shift
            && ui
                .ctx()
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
        if tab_key && !suggest.items.is_empty() {
            if let Some(picked) = suggest.items.get(suggest.selected) {
                tab.address = strip_scheme_prefix(&picked.url).into_owned();
                tab.mark_address_edited();
            }
            handled = true;
        }
        if handled {
            ui.memory_mut(|m| {
                m.move_focus(egui::FocusDirection::None);
                m.request_focus(id);
            });
        }
    }

    // Enter submits. `lost_focus() + Enter` is the standard singleline
    // signal; `has_focus() + Enter` covers toolkits that keep focus on
    // submit, so Enter never silently does nothing. The popup selection only
    // wins after an EXPLICIT pick (`Down`/`Up`, or a pointer that moved onto
    // a row): otherwise Enter submits the typed address as-is. Presetting
    // the default highlighted row used to replace the typed URL, so editing
    // an address could navigate to (and reload) the page already shown.
    let enter_key = ui.input(|i| i.key_pressed(egui::Key::Enter));
    if enter_key && popup_open && suggest.explicit && !suggest.items.is_empty() {
        if let Some(picked) = suggest.items.get(suggest.selected) {
            let display = strip_scheme_prefix(&picked.url);
            if tab.address.trim() != display {
                tab.address = display.into_owned();
                tab.mark_address_edited();
            }
        }
    }
    let mut submitted = enter_key && (field.lost_focus() || field.has_focus());
    if submitted {
        // Enter ends the edit session: the popup must not survive it, even
        // where the toolkit keeps the keyboard on the field.
        suggest.dismissed = true;
        ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
        popup_open = false;
    }
    // Esc: the popup gets the first press (close, keep editing); the next
    // one reverts to the page shown and leaves the field. egui clears the
    // focus on Esc before the UI runs, so `has_focus` is already false here;
    // `lost_focus` still fires on that frame (`Focus` remembers the previous
    // ids), and `App::handle_shortcuts` skips Stop whenever the address had
    // the keyboard last frame (`Tab::address_focused`). An Esc that closed
    // the page-info card this frame is spent: the field keeps its text.
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape && !site_info_escape && (field.has_focus() || field.lost_focus()) {
        let was_open = !suggest.dismissed && !suggest.items.is_empty();
        if was_open {
            suggest.dismissed = true;
            ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
            popup_open = false;
            ui.memory_mut(|m| m.request_focus(id));
        } else {
            tab.revert_address();
            field.surrender_focus();
        }
    }

    // Popup rows (overlay: never shifts the capsule or its neighbours).
    // Click navigates; hover adopts the selection so `Enter` picks what the
    // pointer is on.
    let mut clicked: Option<usize> = None;
    let mut hovered_row: Option<usize> = None;
    if popup_open {
        let ctx = ui.ctx().clone();
        let frame = egui::Frame::popup(ui.style());
        // `Frame::popup` pads the content with the theme menu margin on
        // every side (plus its stroke): size the CONTENT so the framed
        // popup ends up exactly as wide as the capsule (otherwise it sticks
        // out on the right and the rows never line up with the field).
        let pad_x = f32::from(frame.inner_margin.left.max(0))
            + f32::from(frame.inner_margin.right.max(0))
            + 2.0 * frame.stroke.width;
        let content_w = (rect.width() - pad_x).max(80.0);
        // The Area id is scoped to the SURFACE (`id_salt`), not the tab:
        // egui keeps one `AreaState` per id for the whole process and never
        // prunes it, so a per-tab id leaked one area (and its layer) per tab
        // that ever opened the popup, growing every later hit test linearly.
        // At most one field edits at a time and the position/size are
        // re-specified every frame, so sharing is behavior-preserving.
        let area_id = egui::Id::new((id_salt, "suggest-popup"));
        let area = egui::Area::new(area_id)
            .fixed_pos(egui::pos2(rect.min.x, rect.max.y + 4.0))
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_min_width(content_w);
                    ui.set_max_width(content_w);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (index, item) in suggest.items.iter().enumerate() {
                        // Paint the highlight only for an EXPLICIT pick: an
                        // untouched list shows no selected row, so the
                        // highlight always means "Enter opens this".
                        let picked = suggest.explicit && index == suggest.selected;
                        let row = suggest_row(ui, item, picked);
                        if row.clicked() {
                            clicked = Some(index);
                        }
                        if row.hovered() {
                            hovered_row = Some(index);
                        }
                    }
                });
            });
        // Exposed for the width test below (and harmless otherwise): the
        // Area response carries the framed popup rect.
        ui.data_mut(|d| {
            d.insert_temp(suggest_id.with("popup-rect"), area.response.rect);
        });
    }
    if let Some(index) = hovered_row {
        // Only a pointer that MOVED this frame counts as a hover pick:
        // `hovered()` is positional, so a pointer merely sitting over the
        // popup (it opened under it, or the list rebuilt while typing) would
        // otherwise turn the highlighted row into an explicit pick and steal
        // Enter from the typed address. A `PointerMoved` event (or a non-zero
        // frame delta) is that movement; the event arm also covers the
        // pointer's first appearance, where `delta()` is zero (no previous
        // position).
        let pointer_moved = ui.input(|i| {
            i.pointer.delta() != egui::Vec2::ZERO
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::PointerMoved(_)))
        });
        if pointer_moved && suggest.selected != index {
            suggest.selected = index;
            // The pointer moved the highlight onto this row: Enter picks it.
            suggest.explicit = true;
            ui.data_mut(|d| d.insert_temp(suggest_id, suggest.clone()));
        }
    }
    if let Some(index) = clicked {
        if let Some(picked) = suggest.items.get(index) {
            tab.address = strip_scheme_prefix(&picked.url).into_owned();
            tab.mark_address_edited();
            submitted = true;
        }
    }

    // Right-slot button: clear the field while editing, else toggle the
    // bookmark (filled ribbon when the page is bookmarked, outline ribbon
    // otherwise; faded/disabled when there is nothing to bookmark).
    if show_clear {
        if icons::icon_button_at(
            ui,
            slot_rect,
            id.with("clear"),
            icons::Icon::Close,
            "Clear address",
            true,
        )
        .clicked()
        {
            tab.address.clear();
            tab.mark_address_edited();
            ui.memory_mut(|m| m.request_focus(id));
        }
    } else if let Some(state) = bookmark {
        let (icon, tip, enabled) = match state {
            BookmarkState::Unavailable => (icons::Icon::Bookmark, "Nothing to bookmark", false),
            BookmarkState::NotBookmarked => (
                icons::Icon::Bookmark,
                "Bookmark this page (Ctrl/Cmd+D)",
                true,
            ),
            BookmarkState::Bookmarked => (
                icons::Icon::BookmarkFilled,
                "Remove bookmark (Ctrl/Cmd+D)",
                true,
            ),
        };
        if icons::icon_button_at(ui, slot_rect, id.with("bookmark"), icon, tip, enabled).clicked() {
            bookmark_toggled = true;
        }
    }

    // Loading: indeterminate sweep under the capsule. The 60ms tick keeps
    // the animation smooth without a full 60fps continuous repaint.
    if tab.is_loading() {
        animate_progress(ui, rect);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(60));
    }

    // The inner `scope_builder` (field layout) advanced the parent cursor to
    // the text rect's right edge -- LESS than the capsule's when the clear
    // button is reserved, which would pull following toolbar widgets into
    // the capsule. Re-advance past the whole capsule.
    ui.advance_cursor_after_rect(rect);
    // Remember whether the field owns the keyboard for the next frame's
    // Esc handling (see above). `|=`: the toolbar and the home page both
    // draw an omnibox and each frame one shared flag is accumulated
    // (`App::handle_shortcuts` resets it at the frame start).
    tab.address_focused |= field.has_focus();
    OmniboxResponse {
        submitted,
        bookmark_toggled,
    }
}

/// Draws the indicator's page-info card under the capsule (`anchor`):
/// the kind of page on screen, its sanitized title when it has one, the
/// exact address (scheme included) and one line on where it came from.
/// An overlay `Area` (never shifts the capsule); returns the card's screen
/// rect, kept for the next frame's click-outside test. Text is painted as
/// non-selectable labels, like every other chrome surface.
fn site_info_card(
    ui: &mut egui::Ui,
    tab: &Tab,
    anchor: egui::Rect,
    id_salt: &'static str,
) -> egui::Rect {
    let info: SiteInfo = tab.site_info();
    let s = theme::scheme_ref(ui);
    let frame = egui::Frame::popup(ui.style());
    // Size the CONTENT so the framed card lands at the target width
    // (`Frame::popup` pads with the theme menu margin + its stroke).
    let pad_x = f32::from(frame.inner_margin.left.max(0))
        + f32::from(frame.inner_margin.right.max(0))
        + 2.0 * frame.stroke.width;
    let content_w = (anchor.width().clamp(240.0, 320.0) - pad_x).max(160.0);
    // Keep the card inside the window: a narrow (zoomed-in) window must
    // not push the card's right half past the edge.
    let card_w = content_w + pad_x;
    let screen = ui.ctx().viewport_rect();
    let x = anchor
        .min
        .x
        .min((screen.right() - card_w).max(screen.left()));
    // Area id scoped to the SURFACE, like the suggestion popup: egui keeps
    // one `AreaState` per id for the whole process, and the position/size
    // are re-specified every frame, so sharing is behavior-preserving.
    let area_id = egui::Id::new((id_salt, "site-info-popup"));
    let area = egui::Area::new(area_id)
        .fixed_pos(egui::pos2(x, anchor.max.y + 4.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            let inner = frame.show(ui, |ui| {
                ui.set_min_width(content_w);
                ui.set_max_width(content_w);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    icons::paint(ui, icon_rect, indicator_icon(tab), s.primary);
                    ui.add(
                        egui::Label::new(theme::strong(
                            egui::RichText::new(info.title).color(s.on_surface),
                        ))
                        .selectable(false),
                    );
                });
                if let Some(title) = &info.page_title {
                    ui.add(
                        egui::Label::new(egui::RichText::new(title).size(14.0).color(s.on_surface))
                            .wrap()
                            .selectable(false),
                    );
                }
                if let Some(address) = &info.address {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(address)
                                .size(11.5)
                                .color(s.on_surface_variant),
                        )
                        .wrap()
                        .selectable(false),
                    );
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(info.detail)
                            .size(11.5)
                            .color(s.on_surface_variant),
                    )
                    .wrap()
                    .selectable(false),
                );
            });
            // Swallow clicks over the card: the frame itself only senses
            // hover, so without this a press on the card would fall
            // through to the document behind the floating area (text
            // selection, link clicks). `Sense::CLICK` (not `click()`): the
            // invisible cover must never join the keyboard tab order.
            let _ = ui.interact(
                inner.response.rect,
                area_id.with("cover"),
                egui::Sense::CLICK,
            );
        });
    area.response.rect
}

/// One suggestion row: title strong on the first line, schemeless URL muted
/// below. Painted labels (never `Button` widgets) so rows keep exact
/// geometry on hover/selection and text never joins a document selection.
fn suggest_row(ui: &mut egui::Ui, item: &Suggestion, selected: bool) -> egui::Response {
    const ROW_H: f32 = 46.0;
    const TITLE_CHARS: usize = crate::config::ROW_TITLE_CHARS;
    const URL_CHARS: usize = crate::config::ROW_URL_CHARS;
    let s = theme::scheme_ref(ui);
    // Floor at the SAME 80px the popup itself is floored at (`content_w`
    // above): a larger row floor than the popup makes the rows grow the
    // frame wider than the capsule for a very narrow host (the capsule is
    // never narrower than ~160pt in practice, but the contract is "exactly
    // as wide as the capsule").
    let width = ui.available_width().max(80.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected || resp.hovered() {
        // `container_hover`, not `container_high`: the popup's fill is
        // `window_fill`, which in dark mode equals `container_high`, so the
        // old highlight was invisible there (and the pick is now the ONLY
        // row highlight the popup paints).
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::MENU),
            s.container_hover,
        );
    } else if resp.has_focus() {
        theme::focus_ring_with(ui, rect, theme::radius::MENU, s);
    }
    let title = match &item.title {
        Some(title) => std::borrow::Cow::Borrowed(title.as_str()),
        None => strip_scheme_prefix(&item.url),
    };
    let title = truncate_chars(&title, TITLE_CHARS);
    // Accessible name: title + URL (the painted URL line is invisible to
    // accesskit, and two suggestions can share a title). Built inside the
    // closure, only while accesskit actually needs it.
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{title} — {}", strip_scheme_prefix(&item.url)),
        )
    });
    let text_left = rect.min.x + 10.0;
    let text_right = rect.max.x - 10.0;
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.min.y + 5.0),
        egui::pos2(text_right.max(text_left + 40.0), rect.min.y + 24.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(theme::strong(
                    egui::RichText::new(title).color(s.on_surface),
                ))
                .truncate()
                .selectable(false),
            );
        },
    );
    let stripped_url = strip_scheme_prefix(&item.url);
    let url = truncate_chars(&stripped_url, URL_CHARS);
    let url_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.min.y + 24.0),
        egui::pos2(text_right.max(text_left + 40.0), rect.min.y + 43.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(url_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(url)
                        .size(11.5)
                        .color(s.on_surface_variant),
                )
                .truncate()
                .selectable(false),
            );
        },
    );
    resp
}

/// Indeterminate progress segment under the capsule.
fn animate_progress(ui: &egui::Ui, pill: egui::Rect) {
    let s = theme::scheme_ref(ui);
    let t = ui.input(|i| i.time) as f32;
    let phase = (t / 1.6).fract();
    // Ping-pong 0..1..0: the segment sweeps forward and back (two-way).
    let pos = if phase < 0.5 {
        phase * 2.0
    } else {
        (1.0 - phase) * 2.0
    };
    let seg_w = (pill.width() * 0.3).max(24.0).min(pill.width());
    let x = pill.min.x + pos * (pill.width() - seg_w).max(0.0);
    let bar = egui::Rect::from_min_size(egui::pos2(x, pill.max.y + 2.0), egui::vec2(seg_w, 2.5));
    ui.painter().rect_filled(
        bar,
        egui::CornerRadius::same(theme::radius::HAIRLINE),
        s.primary,
    );
}

#[cfg(test)]
mod tests;
