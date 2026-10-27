// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::theme::widgets`: the shared frame/space/type helpers built on the
//! active [`Scheme`].
//!
//! Cards, buttons, shadows, gutters, radii, focus rings, selection colors
//! and the responsive type scale: every browser surface composes these
//! instead of hardcoding a value, so the README look stays consistent.

use super::*;

/// Darken/lighten helper for hover/active states.
pub(crate) fn shade(c: egui::Color32, f: f32) -> egui::Color32 {
    let (r, g, b, a) = (c.r() as f32, c.g() as f32, c.b() as f32, c.a());
    egui::Color32::from_rgba_unmultiplied(
        (r * f).clamp(0.0, 255.0).round() as u8,
        (g * f).clamp(0.0, 255.0).round() as u8,
        (b * f).clamp(0.0, 255.0).round() as u8,
        a,
    )
}

/// One step past a hover fill for the PRESSED/emphasized state: darker in
/// light mode, LIGHTER in dark mode (darkening an already dark gray reads as
/// "returning toward rest" and gives no feedback).
pub(crate) fn press_shade(c: egui::Color32, dark: bool) -> egui::Color32 {
    shade(c, if dark { 1.25 } else { 0.94 })
}

/// Linear RGB blend of two opaque colors (`t` clamped to `0..=1`).
/// Single source for two-color interpolations (the Preferences switch track
/// carries the only caller): [`shade`] scales one color, this blends two.
pub fn mix(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    egui::Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

/// The same color at a low alpha (tinted hover/selected washes).
pub fn tint(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Tonal button with hover/active feedback + disabled handling.
/// Disabled buttons use muted FULL-OPACITY colors and a hover-sense button:
/// `add_enabled(false, ...)` fades the whole widget to 50%, which left
/// white-on-blue (light) and dark-on-dark (dark) labels nearly invisible.
pub fn tonal_button_sized(
    ui: &mut egui::Ui,
    text: &str,
    size: egui::Vec2,
    enabled: bool,
) -> egui::Response {
    // Borrowed palette: the 50-field `Scheme` is interned per theme, so a
    // per-button copy is pure waste on a surface with many actions.
    let s = scheme_ref(ui);
    let (fg, fill, hover, active) = if enabled {
        // Dark mode must LIGHTEN on hover/press: darkening an already dark
        // gray gave no feedback.
        let (hover_f, active_f) = if ui.visuals().dark_mode {
            (1.35, 1.6)
        } else {
            (0.92, 0.85)
        };
        (
            s.on_secondary_container,
            s.secondary_container,
            shade(s.secondary_container, hover_f),
            shade(s.secondary_container, active_f),
        )
    } else {
        (
            s.on_surface_variant,
            s.container_high,
            s.container_high,
            s.container_high,
        )
    };
    sized_button(ui, text, size, enabled, (fg, fill, hover, active))
}

/// Accent-filled button: the ONE emphasized action of a surface (the error
/// view's "open in your browser" escape hatch). Same geometry contract as
/// [`tonal_button_sized`]; the fill is the accent tone, never a widget gray.
///
/// The label is white, so every fill must keep it readable: the light accent
/// passes on its own (~5.9:1), while the dark accent (`#4493f8`, a light
/// blue) would render white at ~3.1:1, so dark mode fills with a darker
/// shade and LIGHTENS on hover/press like every other dark-mode control
/// (the whole ramp stays at or above WCAG AA, pressed included).
pub fn primary_button_sized(ui: &mut egui::Ui, text: &str, size: egui::Vec2) -> egui::Response {
    let s = scheme_ref(ui);
    let (fill, hover, active) = if ui.visuals().dark_mode {
        (
            shade(s.primary, 0.60),
            shade(s.primary, 0.72),
            shade(s.primary, 0.80),
        )
    } else {
        (s.primary, shade(s.primary, 0.92), shade(s.primary, 0.85))
    };
    sized_button(
        ui,
        text,
        size,
        true,
        (egui::Color32::WHITE, fill, hover, active),
    )
}

/// Shared body of [`tonal_button_sized`] and [`primary_button_sized`]:
/// fixed-size flat button whose visuals come from
/// `(fg, fill, hover, active)`, one 1px (transparent) stroke in every state
/// and readable disabled colors.
fn sized_button(
    ui: &mut egui::Ui,
    text: &str,
    size: egui::Vec2,
    enabled: bool,
    (fg, fill, hover, active): (egui::Color32, egui::Color32, egui::Color32, egui::Color32),
) -> egui::Response {
    // Enforce size via add_sized inside a scope with overridden visuals.
    ui.scope(|ui| {
        {
            let v = &mut ui.visuals_mut().widgets;
            v.inactive.bg_fill = fill;
            v.hovered.bg_fill = hover;
            v.active.bg_fill = active;
            v.inactive.weak_bg_fill = fill;
            v.hovered.weak_bg_fill = hover;
            v.active.weak_bg_fill = active;
            v.inactive.fg_stroke = egui::Stroke::new(stroke::HAIRLINE, fg);
            v.hovered.fg_stroke = egui::Stroke::new(stroke::HAIRLINE, fg);
            v.active.fg_stroke = egui::Stroke::new(stroke::HAIRLINE, fg);
            // Flat buttons: no visible outline ring. A TRANSPARENT 1px
            // stroke, never `Stroke::NONE`: `Frame::total_margin` adds the
            // stroke width back to the inner margin, so a 0-width stroke
            // would make these buttons 2px tighter than every other button.
            let flat = egui::Stroke::new(stroke::HAIRLINE, egui::Color32::TRANSPARENT);
            v.inactive.bg_stroke = flat;
            v.hovered.bg_stroke = flat;
            v.active.bg_stroke = flat;
            if !enabled {
                v.noninteractive.bg_fill = fill;
                v.noninteractive.weak_bg_fill = fill;
                v.noninteractive.fg_stroke = egui::Stroke::new(stroke::HAIRLINE, fg);
                v.noninteractive.bg_stroke = flat;
            }
        }
        if !enabled {
            ui.visuals_mut().interact_cursor = None;
        }
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        // `add_sized` (a `centered_and_justified` child ui): the label is
        // centered in the fixed frame whatever the PARENT layout is.
        // `Button::min_size` alone left it aligned to the parent's cross
        // axis — centered inside a centered/right-to-left parent (history
        // footer) but visibly left-parked inside the error view's plain
        // left-aligned column.
        let resp = ui.add_sized(
            size,
            egui::Button::new(strong(egui::RichText::new(text).color(fg))).sense(sense),
        );
        if enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            resp
        }
    })
    .inner
}

/// Card: subtle surface, 6px corners, 1px `#d0d7de` border
/// (table/image cards).
pub fn card(ui: &egui::Ui) -> egui::Frame {
    let s = scheme_ref(ui);
    card_with(s.container_low, s.outline_variant, 12)
}

/// Horizontal padding inside a code card, in px (see [`code_card`]).
/// Single source: the card frame and the block's inner width math share it,
/// so changing one cannot silently overflow the other.
pub const CODE_PAD_X: f32 = 16.0;

/// Square box of every chrome icon button (tab close, omnibox clear slot,
/// history capsule clear, navbar zoom buttons): 26px. Single source so the
/// click targets cannot drift apart across surfaces (all were 26px literals).
pub const ICON_BOX: f32 = 26.0;

/// Tile avatar hues for letter/emoji avatars: mid/dark tones with readable
/// white glyphs in BOTH themes.
///
/// Intentional fixed-hue exception to the `Scheme` tokens: these are
/// identity colors (the stored bookmark index names one of eight hues),
/// not chrome surfaces, so they must not shift with the theme the way
/// `alert_*`/surface tones do. `config::MAX_BOOKMARK_COLORS` keeps stored
/// indexes in range (a test pins the two together).
pub const AVATAR_PALETTE: [egui::Color32; 8] = [
    egui::Color32::from_rgb(9, 105, 218),  // blue
    egui::Color32::from_rgb(26, 127, 55),  // green
    egui::Color32::from_rgb(154, 103, 0),  // amber
    egui::Color32::from_rgb(207, 34, 46),  // red
    egui::Color32::from_rgb(130, 80, 223), // purple
    egui::Color32::from_rgb(191, 57, 137), // pink
    egui::Color32::from_rgb(5, 80, 174),   // dark blue
    egui::Color32::from_rgb(87, 96, 106),  // slate
];

/// Card for code blocks: muted surface, 6px corners, 16px padding and NO
/// border (a `pre` block has no stroke; the app's header row with language +
/// Copy stays).
///
/// The bottom padding is 4px, not 16: the horizontal bar's lane
/// ([`scrollbar_lane`], 12px) lives INSIDE the scroll content, so the bar
/// floats under the code instead of over the last line, while the card's
/// outer height stays exactly the uniform recipe's (4 + 12 = 16px).
pub fn code_card(ui: &egui::Ui) -> egui::Frame {
    let s = scheme_ref(ui);
    egui::Frame::new()
        .fill(s.container_low)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin {
            left: CODE_PAD_X as i8,
            right: CODE_PAD_X as i8,
            top: 16,
            bottom: 4,
        })
}

/// Shared card frame: one recipe for card/code/table surfaces.
pub fn card_with(fill: egui::Color32, stroke: egui::Color32, margin: i8) -> egui::Frame {
    // Note: the palette is read by callers; this stays `Ui`-free so tests
    // and non-Ui code can build the same frame deterministically.
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        // Keep the invariant from `apply`: every widget state owns a 1px
        // stroke width so controls never resize between states.
        .stroke(egui::Stroke::new(stroke::HAIRLINE, stroke))
        .inner_margin(egui::Margin::same(margin))
}

/// Table frame: page surface, 1px `#d1d9e0` border, 6px corners.
/// Striped rows come from `Visuals::faint_bg_color` (`#f6f8fa`).
///
/// No inner margin: the frame IS the table's outer cell border, so the
/// grid (and its collapsed interior lines) reaches it edge to edge. The
/// 5px/11px cell padding lives inside each cell (`viewer::table`).
pub fn table_card(ui: &egui::Ui) -> egui::Frame {
    let s = scheme_ref(ui);
    egui::Frame::new()
        .fill(s.surface)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .stroke(egui::Stroke::new(stroke::HAIRLINE, s.outline_variant))
        .inner_margin(egui::Margin::ZERO)
}

/// Document `<hr>`: a 4px `#d0d7de` bar (egui's separator is 1px).
/// Callers handle list/quote indentation around it.
pub fn rule(ui: &mut egui::Ui) {
    let s = scheme_ref(ui);
    let w = ui.available_width().max(0.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(radius::HAIRLINE),
        s.outline_variant,
    );
}

/// Full-bleed 1px separator: the dialog/card rule and the header underline.
/// Single helper so every floating surface draws the same hairline instead
/// of repeating the allocate + `rect_filled` recipe.
pub fn hairline(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), stroke::HAIRLINE),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, color);
}

/// Elevation helper: a soft shadow in light mode, none in dark mode.
///
/// A black shadow over dark chrome reads as a dirty outline around panels
/// (the "black frame" glitch), so dark mode stays flat and relies on the
/// fill contrast between chrome and page.
pub fn soft_shadow(ui: &egui::Ui, offset: [i8; 2], blur: u8, alpha_light: u8) -> egui::Shadow {
    shadow_recipe(ui.visuals().dark_mode, offset, blur, alpha_light)
}

/// Floating-dialog shadow: the elevation used by the app's centered cards
/// (Preferences). Light mode gets a soft drop shadow; dark mode is
/// flat (black blur over dark chrome reads as dirt). Single source so the
/// two dialogs cannot drift apart.
pub fn dialog_shadow(dark: bool) -> egui::Shadow {
    shadow_recipe(dark, [0, 8], 28, 40)
}

/// Core shadow recipe, shared by [`soft_shadow`] and the floating surfaces
/// set up in `apply_visuals` (which has no `Ui`). `dark` = flat.
pub(crate) fn shadow_recipe(
    dark: bool,
    offset: [i8; 2],
    blur: u8,
    alpha_light: u8,
) -> egui::Shadow {
    if dark {
        egui::Shadow::NONE
    } else {
        egui::Shadow {
            offset,
            blur,
            spread: 0,
            color: egui::Color32::from_black_alpha(alpha_light),
        }
    }
}

/// Explicit central-panel frame with responsive gutters (8/12/16px).
/// (Relying on panel defaults made margins uneven across themes/versions.)
/// Borderless sheet + soft shadow so the page reads as a sheet sunk below
/// the chrome without a hard frame around it.
///
/// The sheet is FLUSH with ALL FOUR window edges (the tab rail included: the
/// reading area wins those pixels) while the rounded rail-side corner keeps
/// the sheet look.
///
/// The frame itself carries NO margin: the gutter ([`content_gutter`]) and
/// the always-visible scrollbar travel INSIDE the scroll content. With the
/// gutter in the frame, the bar floated one gutter short of the sheet's
/// right edge and a scrolled document was clipped one gutter short of the
/// flush top/bottom edges (its letters "vanished" before the edge).
pub fn content_frame(ui: &egui::Ui) -> egui::Frame {
    let s = scheme_ref(ui);
    egui::Frame::new()
        .fill(s.surface)
        .stroke(egui::Stroke::NONE)
        .corner_radius(egui::CornerRadius {
            nw: radius::SHEET,
            ne: 0,
            sw: 0,
            se: 0,
        })
        .shadow(soft_shadow(ui, [0, 1], 8, 18))
        .inner_margin(egui::Margin::ZERO)
}

/// Responsive gutter (8/12/16px) every sheet applies INSIDE its scroll
/// content (`ScrollArea::content_margin`): the left/right insets keep the
/// reading column off the sheet edges while the always-visible scrollbar
/// hugs the flush right edge, and the top/bottom pad matches that spacing at
/// rest so letters can still scroll all the way to the flush edges (the END
/// of a long page additionally carries [`spacing::PAGE_END`] below its last
/// element, so a document scrolled to its bottom does not read cramped).
///
/// Must be measured on the same `Ui` [`content_frame`] is built from (the
/// central panel's root, before any frame margin), so the two cannot drift.
pub fn content_gutter(ui: &egui::Ui) -> f32 {
    content_margin(ui.available_width()) as f32
}

/// Horizontal width the floating scrollbar covers inside a `ScrollArea`
/// (`bar_width` plus its outer margins). Single source for the top fade
/// (never paint under the bar) and the document hit rect (clicks on the bar
/// must not open the text menu).
pub fn scrollbar_width(ui: &egui::Ui) -> f32 {
    let scroll = &ui.spacing().scroll;
    scroll.bar_width + 2.0 * scroll.bar_outer_margin
}

/// Bottom lane a horizontal inner scroll (code and table cards) reserves
/// under its content: the floating bar's full width plus a 2px gap, so the
/// expanded handle lies UNDER the text instead of covering the last code
/// line / table row. Applied through `ScrollArea::content_margin`.
pub fn scrollbar_lane(ui: &egui::Ui) -> i8 {
    (scrollbar_width(ui).ceil() as i8).saturating_add(2)
}

/// Top inner fade (recessed-sheet effect): paints a subtle vertical
/// gradient strip at the top of `rect` (container tone fading to
/// transparent). `Frame` cannot do gradients, so this draws thin
/// 1px rects. Call AFTER the scroll content, as a non-interactive overlay.
pub fn paint_content_fade(ui: &egui::Ui, rect: egui::Rect) {
    let s = scheme_ref(ui);
    let base = s.container_low;
    // Single source for the fade geometry: height, peak alpha and steps.
    const FADE_HEIGHT: f32 = 20.0;
    const FADE_PEAK_ALPHA: f32 = 26.0;
    const FADE_STEPS: i32 = 16;
    let steps = FADE_STEPS;
    let height = FADE_HEIGHT;
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
        return;
    }
    // Never cover the vertical scrollbar: shrink the right edge by the bar
    // + its floating margins (the bar is the only thing painted there).
    let bar_use = scrollbar_width(ui);
    let rect = egui::Rect {
        min: rect.min,
        max: egui::pos2((rect.max.x - bar_use).max(rect.min.x), rect.max.y),
    };
    let p = ui.painter();
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let alpha = ((1.0 - t) * FADE_PEAK_ALPHA)
            .round()
            .clamp(0.0, FADE_PEAK_ALPHA) as u8;
        if alpha == 0 {
            continue;
        }
        let y0 = rect.min.y + t * height;
        let strip = egui::Rect {
            min: egui::pos2(rect.min.x, y0),
            max: egui::pos2(rect.max.x, y0 + height / steps as f32 + 1.0),
        };
        let c = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
        p.rect(
            strip,
            egui::CornerRadius::ZERO,
            c,
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    }
}

/// Document body font size (16px). The single source for `TextStyle::Body`
/// and for the monospace family, which draws at the SAME size so inline code
/// pills and code cards are exactly as wide as the body text.
pub const BODY_FONT_SIZE: f32 = 16.0;

/// Document body line height (`1.5 x 16px`, the README line-height).
/// Applied to body/inline text in `viewer::inline` and to code lines; headings
/// keep the font's natural line height (their glyphs are taller than 24px).
pub const BODY_LINE_HEIGHT: f32 = 24.0;

/// Type scale for chrome and document helpers: one token per size so the
/// omnibox, Preferences, history rows and the display face cannot drift.
/// Values are pixel-identical to the literals they replace.
pub mod typography {
    /// Compact muted lines (omnibox card detail, suggestion URLs): 11.5px.
    pub const UI_COMPACT: f32 = 11.5;
    /// Small chrome text (`TextStyle::Small`, captions, section labels): 12px.
    pub const UI_SMALL: f32 = 12.0;
    /// Preferences section label (uppercase eyebrow): 11px.
    pub const EYEBROW: f32 = 11.0;
    /// Link/list measure in Preferences (`text_link` galley): 13px.
    pub const UI_LINK: f32 = 13.0;
    /// Buttons (`TextStyle::Button`) and omnibox titles: 14px.
    pub const BUTTON: f32 = 14.0;
    /// Dropdown rows: 14.5px.
    pub const DROPDOWN: f32 = 14.5;
    /// Setting rows and danger rows: 15px.
    pub const ROW: f32 = 15.0;
    /// Headings (`TextStyle::Heading`): 18px.
    pub const HEADING: f32 = 18.0;
    /// Preferences dialog title: 21px.
    pub const DIALOG_TITLE: f32 = 21.0;
    /// Full-page titles (History landing, error view): 30px.
    pub const PAGE_TITLE: f32 = 30.0;
    /// Landing display face: 38px.
    pub const DISPLAY: f32 = 38.0;
}

/// Stroke-width tokens: one scale so frames, dividers and focus rings keep
/// the 1px invariant documented in `apply` (a 0-width state shrinks `small`
/// buttons by 2px and shifts neighbours).
pub mod stroke {
    /// Hairline frames, dividers, focus rings, table borders: 1px.
    pub const HAIRLINE: f32 = 1.0;
    /// Hand-painted icon ink (close X, check marks, switch ring): 1.8px.
    pub const ICON: f32 = 1.8;
    /// Check-mark ink over its tile wash: 2px.
    pub const EMPHASIS: f32 = 2.0;
}

/// Corner-radius tokens: one scale for the whole chrome so cards, pills,
/// dialogs and sheets stay consistent. Callers must use these instead of
/// hard-coded `CornerRadius::same(N)`.
pub mod radius {
    /// Inline pills, image placeholders, checkboxes (3px).
    pub const SMALL: u8 = 3;
    /// Hairline accents: the tab rail's active bar, the omnibox progress
    /// sweep, the document `<hr>` bar (2px; too thin for `SMALL`).
    pub const HAIRLINE: u8 = 2;
    /// Cards, code/table surfaces, widgets, quote boxes (6px).
    pub const CARD: u8 = 6;
    /// Hand-painted rows, menus, suggestion rows, danger rows (8px).
    /// Covers the former `same(8)` sites and `row::ROW_RADIUS`.
    pub const MENU: u8 = 8;
    /// Tiles, menus, windows (10px).
    pub const TILE: u8 = 10;
    /// Dialogs, sheets (12-14px handled at the call site).
    pub const SHEET: u8 = 12;
    /// Fully-rounded tracks/pills (switch track, zoom pill backgrounds).
    pub const FULL: u8 = 16;
}

/// Pill/track radius for a control of height `h`: half height, rounded.
/// Single source for the former `(h * 0.5).round() as u8` duplicates in
/// `icons`, `omnibox`, `navbar` and `history`.
pub fn pill_radius(h: f32) -> egui::CornerRadius {
    egui::CornerRadius::same((h * 0.5).round().clamp(0.0, 255.0) as u8)
}

/// Vertical air tokens: one scale for document/chrome gaps so headings,
/// rules, footnotes and cards stay consistent. Callers must prefer these
/// over hard-coded `add_space(N)`.
pub mod spacing {
    /// Hairline gap (2px): divider air, caption nudges, superscript fudge.
    pub const XXS: f32 = 2.0;
    /// Tight gap inside a block (4px): empty heading lines, footnote tails.
    pub const XS: f32 = 4.0;
    /// Small gap (6px): heading rules, compact card padding.
    pub const S: f32 = 6.0;
    /// Block gap (8px): after headings, around rules and lists.
    pub const M: f32 = 8.0;
    /// Section gap (12px): before footnote rules, card sections.
    pub const L: f32 = 12.0;
    /// Loose gap (16px): heading top air, hero sections.
    pub const XL: f32 = 16.0;
    /// Page-end pad (24px): blank air after the LAST element of a scrolling
    /// page, so its final line never sits flush against the sheet's bottom
    /// edge when scrolled all the way down. Bottom-only: the top keeps its
    /// symmetric gutter, and scrolled content still clips at the flush
    /// edges. Applied by the document renderer, the Home landing page and
    /// the error view.
    pub const PAGE_END: f32 = 24.0;
}

/// Shared keyboard-focus ring: a 1px accent inset stroke that does not
/// affect layout (painted, not framed). Every hand-painted control that
/// can own keyboard focus should call this when `has_focus` is true.
pub fn focus_ring(ui: &egui::Ui, rect: egui::Rect, radius: u8) {
    focus_ring_with(ui, rect, radius, scheme_ref(ui));
}

/// [`focus_ring`] for callers that already resolved the scheme (repeated
/// rows: one palette lookup per list instead of one per row).
pub fn focus_ring_with(ui: &egui::Ui, rect: egui::Rect, radius: u8, s: &Scheme) {
    focus_ring_stroke(ui, rect, radius, s.primary);
}

/// Focus ring in an arbitrary color (the destructive rows ring in `error`
/// tone instead of the accent).
pub fn focus_ring_stroke(ui: &egui::Ui, rect: egui::Rect, radius: u8, color: egui::Color32) {
    ui.painter().rect_stroke(
        rect.shrink(stroke::HAIRLINE),
        egui::CornerRadius::same(radius),
        egui::Stroke::new(stroke::HAIRLINE, color),
        egui::StrokeKind::Inside,
    );
}

/// "Select all" background (same token as native drag-select).
pub fn select_bg(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().selection.bg_fill
}

/// Applies the "select all" highlight to `RichText`: selection background
/// plus a forced high-contrast foreground (`on_surface`).
/// Without the forced fg, links (primary blue) and code tokens keep their
/// colors on top of the pale selection bg and become unreadable.
pub fn selected_text(ui: &egui::Ui, rt: egui::RichText) -> egui::RichText {
    rt.background_color(select_bg(ui))
        .color(scheme_ref(ui).on_surface)
}

/// In-page find match background: pale amber in light mode, deep amber in
/// dark (light text stays readable on it, like `selected_text` forces the
/// `on_surface` foreground for the same reason).
pub fn find_match_bg(ui: &egui::Ui) -> egui::Color32 {
    scheme_ref(ui).find_match
}

/// In-page find CURRENT match background: stronger amber than `find_match_bg`.
pub fn find_current_bg(ui: &egui::Ui) -> egui::Color32 {
    scheme_ref(ui).find_current
}

/// Applies the find highlight to `RichText`: amber background plus a forced
/// high-contrast foreground (links keep their blue on pale yellow otherwise).
pub fn find_text(ui: &egui::Ui, rt: egui::RichText, current: bool) -> egui::RichText {
    let bg = if current {
        find_current_bg(ui)
    } else {
        find_match_bg(ui)
    };
    rt.background_color(bg).color(scheme_ref(ui).on_surface)
}

/// Responsive reading-column width: fluid on small windows, capped at 760.
/// Guarantees at least 8px of breathing room so narrow windows never clip.
///
/// Callers pass the width INSIDE the content gutter (see [`content_gutter`]);
/// this reserves another ~12px per side on top of it, so text never touches
/// the gutter edge even when the window is narrower than the cap.
pub fn content_max_width(avail: f32) -> f32 {
    if !avail.is_finite() {
        return 760.0;
    }
    // Usable width minus gutters, capped at the reading measure.
    (avail - 24.0).clamp(0.0, 760.0)
}

/// Responsive content gutter: 8px mobile, 12px tablet, 16px desktop.
/// Applied inside the scroll content (see [`content_gutter`]).
pub fn content_margin(avail: f32) -> i8 {
    if avail < 600.0 {
        8
    } else if avail < 1024.0 {
        12
    } else {
        16
    }
}

/// Responsive type scale from the viewport width: 0.95 / 1.0 / 1.06.
/// (Headings only; body uses `TextStyle::Body` directly.)
pub fn type_scale(avail: f32) -> f32 {
    if avail < 480.0 {
        0.95
    } else if avail < 700.0 {
        1.0
    } else {
        1.06
    }
}

/// Display type (landing wordmark): large, bold, main text color. The brand
/// blue lives in the mark beside it (`ui::icons::paint_brand_mark`), so the
/// lockup pairs one colored tile with a neutral word instead of two blues.
pub fn display(ui: &egui::Ui, text: &str) -> egui::RichText {
    strong(egui::RichText::new(text).size(typography::DISPLAY)).color(scheme_ref(ui).on_surface)
}

/// Muted secondary text.
///
/// The explicit `on_surface_variant` color IS the muted tone: egui resolves
/// an explicit `text_color` before the `weak` flag, so a `.weak()` call here
/// would be a no-op (it used to be one).
pub fn weak(ui: &egui::Ui, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .small()
        .color(scheme_ref(ui).on_surface_variant)
}
