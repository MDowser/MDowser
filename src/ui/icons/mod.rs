// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::icons`: monochrome interface icons drawn as themed vector strokes.
//!
//! - Every toolbar/rail/chrome glyph is hand-drawn on ONE 24x24 design grid
//!   ([`Icon::draw`]) and stroked with the caller's color: one uniform,
//!   tintable line-icon set that follows the theme and stays crisp at every
//!   zoom. The old color-emoji textures (Noto Color Emoji through
//!   `media::emoji`) are gone from the chrome; that pipeline now serves only
//!   document emoji and user-chosen bookmark-tile glyphs.
//! - Button effect (`icon_button` / `icon_button_at`): hover and keyboard
//!   focus ease the glyph from the muted tone to the accent while it grows
//!   ~10%; pressing pulls it back ~6%. Each button animates independently
//!   through its own `Response::id`, and the tween only requests repaints
//!   while it is actually moving.
//! - `paint_text` remains for user content (bookmark custom icons): a
//!   letter/emoji cluster through `media::emoji`, with the themed
//!   monochrome text as fallback.
//! - `icon_button()` is the 32x32 toolbar button; `icon_button_at()` paints
//!   a button inside a caller-provided rect (omnibox clear, zoom pill).
//! - `BUTTON` / `GLYPH` are the chrome's shared sizes: the toolbar button
//!   box and the visible glyph side inside it. Surfaces that paint their
//!   own icons (the tab rail) size them from these constants, so every
//!   chrome icon matches the toolbar exactly.
//! - Disabled buttons draw the muted tone faded and use `Sense::hover`
//!   (never clickable, no false hover). The fade is a faded `Painter`
//!   clone, never a `ui.scope`: an empty scope advances the cursor one
//!   item spacing, so disabling a button used to push its neighbours 2px.

use crate::ui::theme;
use eframe::egui;
use std::sync::{Mutex, OnceLock};

/// Icons available to the UI (each maps to one vector glyph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// House: the Home / new tab page.
    Home,
    /// Left arrow: back.
    Back,
    /// Right arrow: forward.
    Forward,
    /// Circular arrow: refresh.
    Reload,
    /// Heavy minus: zoom out.
    ZoomOut,
    /// Heavy plus: zoom in.
    ZoomIn,
    /// Heavy plus: new tab.
    Plus,
    /// Globe: fallback page icon (no favicon yet).
    Globe,
    /// Padlock: secure HTTPS omnibox indicator.
    Lock,
    /// Page: local file omnibox indicator.
    File,
    /// Framed picture: image placeholder.
    Image,
    /// Warning sign: image failed to load.
    BrokenImage,
    /// Cross mark: close tab / clear field / stop loading.
    Close,
    /// Clock: browsing history.
    History,
    /// Magnifier: the history search field.
    Search,
    /// Circled i: internal informational pages (third-party licenses).
    Info,
    /// Gear: preferences.
    Settings,
    /// Hourglass: loading placeholder (favicon, image).
    Loading,
    /// Wastebasket: destructive actions (delete history).
    Delete,
    /// Bookmark ribbon outline: the Home bookmark section header and the
    /// toolbar bookmark button on a page that is not bookmarked (or when
    /// there is nothing to bookmark).
    Bookmark,
    /// Filled bookmark ribbon: the toolbar bookmark button on a bookmarked
    /// page.
    BookmarkFilled,
    /// Pencil: rename the Home bookmark tile.
    Edit,
    /// Check mark: save the inline bookmark rename.
    Check,
    /// Chevron pointing up: find-bar previous match (also the stepper shape
    /// pair with [`Icon::ChevronDown`]).
    ChevronUp,
    /// Chevron pointing down: find-bar next match (also the expanded state
    /// of an accordion header).
    ChevronDown,
    /// Chevron pointing right: the collapsed state of an accordion header.
    ChevronRight,
    /// Slashed cloud: an offline copy (no HTTP response arrived for the load).
    CloudOff,
    /// Archive box: a saved copy (the server answered without providing a
    /// fresh document).
    Archive,
}

/// Design grid side: every glyph below is drawn in a 24x24 unit square.
const GRID: f32 = 24.0;
/// Stroke width in grid units (Lucide's 2/24 line weight).
const STROKE: f32 = 2.0;
/// Fraction of a target rect's shortest side the grid covers: the art keeps
/// a small optical margin, like every line-icon set.
const ART: f32 = 0.85;

/// Side of one toolbar icon button ([`icon_button`]) in points: the
/// chrome's reference control size. Surfaces that paint their own icons
/// (the tab rail) size them from this constant, so they match the toolbar.
pub const BUTTON: f32 = 32.0;
/// Visible side of one chrome glyph: a [`BUTTON`] box minus the line-icon
/// optical margin, the size `icon_button` draws at. Image icons in the
/// chrome (tab favicons) draw at this side too, so images and vector
/// glyphs read as one set.
pub const GLYPH: f32 = BUTTON * ART;

impl Icon {
    /// Draws this icon through `pen`.
    fn draw(self, pen: &Pen<'_>) {
        match self {
            Icon::Home => {
                // Roof, walls and door.
                pen.stroke(&[(3.6, 11.0), (12.0, 3.6), (20.4, 11.0)]);
                pen.stroke(&[(5.8, 9.6), (5.8, 20.4), (18.2, 20.4), (18.2, 9.6)]);
                pen.stroke(&[(9.6, 20.4), (9.6, 15.2), (14.4, 15.2), (14.4, 20.4)]);
            }
            Icon::Back => {
                pen.stroke(&[(19.2, 12.0), (5.4, 12.0)]);
                pen.stroke(&[(12.0, 5.2), (5.2, 12.0), (12.0, 18.8)]);
            }
            Icon::Forward => {
                pen.stroke(&[(4.8, 12.0), (18.6, 12.0)]);
                pen.stroke(&[(12.0, 5.2), (18.8, 12.0), (12.0, 18.8)]);
            }
            Icon::Reload => {
                // A ~285-degree clockwise arc with the arrowhead at its end
                // (upper left), leaving the gap at the top right.
                let (cx, cy, r) = (12.0, 12.0, 8.0);
                let (a0, a1) = (-40.0_f32, 245.0_f32);
                pen.arc(cx, cy, r, a0, a1);
                let end = a1.to_radians();
                let tip = (cx + r * end.cos(), cy + r * end.sin());
                // Clockwise travel: the tangent points along increasing angle.
                pen.arrow(tip, a1 + 90.0, 3.8);
            }
            Icon::ZoomOut => pen.stroke(&[(5.6, 12.0), (18.4, 12.0)]),
            Icon::ZoomIn | Icon::Plus => {
                pen.stroke(&[(12.0, 5.6), (12.0, 18.4)]);
                pen.stroke(&[(5.6, 12.0), (18.4, 12.0)]);
            }
            Icon::Globe => {
                pen.circle(12.0, 12.0, 8.2);
                pen.ellipse(12.0, 12.0, 3.6, 8.2);
                pen.stroke(&[(3.8, 12.0), (20.2, 12.0)]);
            }
            Icon::Lock => {
                // Padlock with a closed shackle: two straight legs rising
                // from the body's top edge, bridged by a half-circle
                // (Lucide lock proportions). The old open arc sat straight
                // on the body like a bag handle and read as a briefcase.
                pen.stroke(&[(7.4, 11.0), (7.4, 7.6)]);
                pen.arc(12.0, 7.6, 4.6, 180.0, 360.0);
                pen.stroke(&[(16.6, 7.6), (16.6, 11.0)]);
                // Body: rounded slab with the centered keyhole.
                pen.rounded_rect((3.8, 11.0), (20.2, 21.4), 2.4);
                pen.dot(12.0, 16.2, 1.1);
            }
            Icon::File => {
                pen.closed(&[
                    (6.0, 3.6),
                    (13.5, 3.6),
                    (18.0, 8.0),
                    (18.0, 20.4),
                    (6.0, 20.4),
                ]);
                pen.stroke(&[(13.5, 3.6), (13.5, 8.0), (18.0, 8.0)]);
                pen.stroke(&[(9.0, 12.6), (15.0, 12.6)]);
                pen.stroke(&[(9.0, 16.2), (15.0, 16.2)]);
            }
            Icon::Image => {
                pen.rounded_rect((4.0, 4.4), (20.0, 19.6), 2.2);
                pen.dot(8.8, 9.2, 1.3);
                pen.stroke(&[
                    (5.5, 17.6),
                    (10.0, 13.0),
                    (13.0, 16.0),
                    (15.8, 13.3),
                    (19.5, 17.2),
                ]);
            }
            Icon::BrokenImage => {
                pen.closed(&[(12.0, 3.6), (21.0, 19.8), (3.0, 19.8)]);
                pen.stroke(&[(12.0, 9.0), (12.0, 14.2)]);
                pen.dot(12.0, 17.0, 1.15);
            }
            Icon::Close => {
                pen.stroke(&[(6.2, 6.2), (17.8, 17.8)]);
                pen.stroke(&[(17.8, 6.2), (6.2, 17.8)]);
            }
            Icon::History => {
                pen.circle(12.0, 12.0, 8.2);
                pen.stroke(&[(12.0, 7.3), (12.0, 12.3), (15.7, 14.1)]);
            }
            Icon::Search => {
                // Magnifier: the ring plus the handle to the lower right.
                pen.circle(10.5, 10.5, 6.3);
                pen.stroke(&[(15.2, 15.2), (20.2, 20.2)]);
            }
            Icon::Info => {
                // Circled lowercase i: dot over the stem.
                pen.circle(12.0, 12.0, 8.2);
                pen.dot(12.0, 8.2, 1.15);
                pen.stroke(&[(12.0, 11.2), (12.0, 16.6)]);
            }
            Icon::Settings => {
                // Eight teeth around a hub, traced as one closed silhouette.
                let teeth = 8usize;
                let (cx, cy, outer, inner) = (12.0, 12.0, 9.7_f32, 7.3_f32);
                let mut cog = Vec::with_capacity(teeth * 4);
                for i in 0..teeth {
                    let base = i as f32 * std::f32::consts::TAU / teeth as f32
                        - std::f32::consts::FRAC_PI_2;
                    for (offset, radius) in [
                        (-0.17_f32, outer),
                        (0.17, outer),
                        (0.28, inner),
                        (0.50, inner),
                    ] {
                        let a = base + offset;
                        cog.push((cx + radius * a.cos(), cy + radius * a.sin()));
                    }
                }
                pen.closed(&cog);
                pen.circle(cx, cy, 3.3);
            }
            Icon::Loading => {
                // Hourglass: two rims and the crossing sides.
                pen.stroke(&[(6.6, 3.6), (17.4, 3.6)]);
                pen.stroke(&[(6.6, 20.4), (17.4, 20.4)]);
                pen.stroke(&[(7.8, 3.6), (7.8, 6.8), (16.2, 17.2), (16.2, 20.4)]);
                pen.stroke(&[(16.2, 3.6), (16.2, 6.8), (7.8, 17.2), (7.8, 20.4)]);
            }
            Icon::Delete => {
                pen.stroke(&[(3.8, 6.2), (20.2, 6.2)]);
                pen.stroke(&[
                    (8.6, 6.2),
                    (8.6, 4.2),
                    (10.4, 3.2),
                    (13.6, 3.2),
                    (15.4, 4.2),
                    (15.4, 6.2),
                ]);
                pen.stroke(&[
                    (5.8, 6.2),
                    (6.8, 19.4),
                    (8.0, 20.8),
                    (16.0, 20.8),
                    (17.2, 19.4),
                    (18.2, 6.2),
                ]);
                pen.stroke(&[(10.0, 10.2), (10.0, 16.8)]);
                pen.stroke(&[(14.0, 10.2), (14.0, 16.8)]);
            }
            Icon::Bookmark => pen.bookmark(false),
            Icon::BookmarkFilled => pen.bookmark(true),
            Icon::Edit => {
                // Pencil: tip triangle, body and the collar line between them.
                pen.closed(&[
                    (4.0, 20.0),
                    (5.6, 15.0),
                    (15.6, 5.0),
                    (19.0, 8.4),
                    (9.0, 18.4),
                ]);
                pen.stroke(&[(5.6, 15.0), (9.0, 18.4)]);
            }
            Icon::Check => {
                pen.stroke(&[(5.2, 12.8), (10.2, 17.6), (18.8, 7.2)]);
            }
            Icon::ChevronUp => pen.stroke(&[(6.6, 14.6), (12.0, 9.2), (17.4, 14.6)]),
            Icon::ChevronDown => pen.stroke(&[(6.6, 9.4), (12.0, 14.8), (17.4, 9.4)]),
            Icon::ChevronRight => pen.stroke(&[(9.4, 6.6), (14.8, 12.0), (9.4, 17.4)]),
            Icon::CloudOff => {
                // Cloud outline (left lobe, flat bottom, right lobe) with a
                // diagonal slash: no connection.
                pen.arc(9.4, 12.0, 6.2, 90.0, 343.1);
                pen.stroke(&[(15.33, 10.2), (16.6, 10.2)]);
                pen.arc(16.6, 14.2, 4.0, 270.0, 450.0);
                pen.stroke(&[(16.6, 18.2), (9.4, 18.2)]);
                pen.stroke(&[(3.2, 3.2), (20.8, 20.8)]);
            }
            Icon::Archive => {
                // Lid slab, box body and the centered handle line.
                pen.rounded_rect((3.4, 3.6), (20.6, 8.0), 1.2);
                pen.stroke(&[
                    (4.9, 8.0),
                    (4.9, 18.7),
                    (6.4, 20.6),
                    (17.6, 20.6),
                    (19.1, 18.7),
                    (19.1, 8.0),
                ]);
                pen.stroke(&[(9.8, 12.4), (14.2, 12.4)]);
            }
        }
    }
}

/// Paints `icon` centered in `rect` as a monochrome vector glyph in `color`.
pub fn paint(ui: &egui::Ui, rect: egui::Rect, icon: Icon, color: egui::Color32) {
    paint_scaled(ui.painter(), rect, icon, color, 1.0);
}

/// `paint` with an extra scale factor (the button hover/press effect). The
/// glyph scales around the rect center, so a widget never moves. Takes the
/// painter (not the `Ui`) so the caller controls the paint target without
/// opening a `scope` (which would advance the cursor and shift the
/// following widgets).
fn paint_scaled(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: Icon,
    color: egui::Color32,
    scale: f32,
) {
    let side = rect.width().min(rect.height()).max(1.0) * ART * scale.clamp(0.5, 1.5);
    let pen = Pen {
        painter,
        origin: rect.center() - egui::vec2(side * 0.5, side * 0.5),
        u: side / GRID,
        // Proportional stroke, clamped only at the extremes: the old 3.0
        // ceiling thinned every glyph ~25% at 175% zoom (the ideal stroke
        // there is ~4.0), making chrome icons visibly lighter.
        w: (side * STROKE / GRID).clamp(1.0, 4.5),
        color,
    };
    icon.draw(&pen);
}

/// Paints an arbitrary glyph (a letter, digit or emoji cluster) centered
/// in `rect` through the color-emoji pipeline (`media::emoji`). Used by the
/// Home bookmark tiles for custom icons; a glyph without a color document
/// falls back to the themed monochrome text.
pub fn paint_text(ui: &egui::Ui, rect: egui::Rect, text: &str, color: egui::Color32) {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    paint_text_glyph(ui, rect, text, color, &mut cache);
}

/// One process-wide emoji cache shared by every `paint_text` caller
/// (bookmark tiles; bounded by `config::EMOJI_CACHE_*` and the process-wide
/// texture pool, like any other emoji cache).
fn cache() -> &'static Mutex<crate::media::emoji::EmojiCache> {
    static CACHE: OnceLock<Mutex<crate::media::emoji::EmojiCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(crate::media::emoji::EmojiCache::new()))
}

/// Uploads any finished chrome-glyph rasters. Called once per frame from
/// `App::logic`; `true` when a texture changed (extra repaint).
pub(crate) fn poll_text_glyphs(ctx: &egui::Context) -> bool {
    cache()
        .lock()
        .map(|mut cache| cache.poll(ctx))
        .unwrap_or(false)
}

/// `true` while a chrome glyph rasterizes on a worker (repaint scheduler).
pub(crate) fn text_glyphs_pending() -> bool {
    cache()
        .lock()
        .map(|cache| cache.has_pending())
        .unwrap_or(false)
}

/// Device-pixel side for one emoji texture: the drawn side times the
/// pixels-per-point, rounded to an EVEN pixel (a 2 px grid) and clamped to
/// the shared `config::EMOJI_TEXTURE_PX_*` bounds.
fn texture_px(side: f32, ppp: f32) -> u32 {
    let raw = (side * ppp / 2.0).round() * 2.0;
    (raw.max(crate::config::EMOJI_TEXTURE_PX_MIN as f32) as u32)
        .min(crate::config::EMOJI_TEXTURE_PX_MAX)
}

/// Shared text-glyph painter: one color emoji cluster at 0.78 of the rect's
/// shortest side, falling back to monochrome text.
fn paint_text_glyph(
    ui: &egui::Ui,
    rect: egui::Rect,
    glyph: &str,
    color: egui::Color32,
    cache: &mut crate::media::emoji::EmojiCache,
) {
    let size = rect.width().min(rect.height()).max(1.0);
    let side = size * 0.78;
    let ctx = ui.ctx();
    let px = texture_px(side, ctx.pixels_per_point());
    let texture = cache.texture(ctx, glyph, px);
    // Emoji textures are not perfectly square (they carry the em
    // ascent/descent), so the drawn rect keeps the texture's aspect; the
    // fallback text uses the square box.
    let art = match &texture {
        Some(texture) => {
            let tex = texture.size_vec2();
            let height = if tex.x > 0.0 {
                side * tex.y / tex.x
            } else {
                side
            };
            egui::Rect::from_center_size(rect.center(), egui::vec2(side, height))
        }
        None => egui::Rect::from_center_size(rect.center(), egui::vec2(side, side)),
    };
    if let Some(texture) = texture {
        egui::Image::new(&texture).paint_at(ui, art);
        return;
    }
    // Monochrome Noto Emoji fallback: the glyph always renders.
    ui.painter().text(
        art.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(side),
        color,
    );
}

/// 32x32 toolbar icon button. Returns the `Response`; the caller checks
/// `enabled && resp.clicked()`.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, enabled: bool) -> egui::Response {
    let size = egui::vec2(BUTTON, BUTTON);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    finish_static_icon_button(ui, rect, resp, icon, enabled, tooltip)
}

/// [`icon_button`] whose tooltip is built on hover only: the closure runs on
/// hover frames, so a live countdown ("Reload (available in 3s)") costs
/// nothing on settled frames. `name` is the static accessible label (screen
/// readers must not get a string that changes every second).
pub fn icon_button_lazy(
    ui: &mut egui::Ui,
    icon: Icon,
    name: &str,
    tooltip: impl FnOnce() -> String,
    enabled: bool,
) -> egui::Response {
    let size = egui::vec2(BUTTON, BUTTON);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    finish_icon_button(ui, rect, resp, icon, enabled, || name.to_string(), tooltip)
}

/// Icon button painted inside a caller-provided `rect` (omnibox clear,
/// zoom pill): same hover fill, cursor, accessible label, tooltip and
/// hover/press animation as `icon_button`. `id_salt` keeps overlapping
/// buttons' ids unique.
pub fn icon_button_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let resp = ui.interact(rect, ui.id().with(id_salt), sense);
    finish_static_icon_button(ui, rect, resp, icon, enabled, tooltip)
}

/// [`icon_button_at`] whose tooltip is built on hover only: the closure runs
/// on hover frames, so a dynamic tooltip (`Zoom in (125%)`) allocates
/// nothing on settled frames. `name` is the static accessible label.
pub fn icon_button_at_lazy(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    name: &str,
    tooltip: impl FnOnce() -> String,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let resp = ui.interact(rect, ui.id().with(id_salt), sense);
    finish_icon_button(ui, rect, resp, icon, enabled, || name.to_string(), tooltip)
}

/// [`icon_button_at_lazy`] with a lazily-built ACCESSIBLE NAME too.
///
/// Row-scoped buttons (a history row's X, a tile's remove) must announce
/// WHICH row they act on; a static label made every row's button identical
/// to a screen reader. Both closures run only when accesskit asks for the
/// info, so a settled frame allocates nothing.
pub fn icon_button_at_named_lazy(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    name: impl Fn() -> String,
    tooltip: impl FnOnce() -> String,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let resp = ui.interact(rect, ui.id().with(id_salt), sense);
    finish_icon_button(ui, rect, resp, icon, enabled, name, tooltip)
}

/// Shared button visuals: circular hover/press fill, the animated glyph
/// effect, cursor, accessible name and tooltip. `a11y_name`/`tooltip` are
/// built lazily (accesskit may be off; a tooltip is built only while
/// hovered).
/// Shared entry for icon buttons whose accessible name and tooltip are the
/// same static text: one `&str` in, the two lazy closures out. (Spelling two
/// `|| tooltip.to_string()` closures at every call site invited eager
/// `on_hover_text` regressions by copy-paste.)
fn finish_static_icon_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    resp: egui::Response,
    icon: Icon,
    enabled: bool,
    text: &str,
) -> egui::Response {
    finish_icon_button(
        ui,
        rect,
        resp,
        icon,
        enabled,
        || text.to_string(),
        || text.to_string(),
    )
}

fn finish_icon_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    resp: egui::Response,
    icon: Icon,
    enabled: bool,
    a11y_name: impl Fn() -> String,
    tooltip: impl FnOnce() -> String,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let radius = theme::pill_radius(rect.height());
    let pressed = enabled && resp.is_pointer_button_down_on();
    if pressed {
        ui.painter().rect_filled(
            rect,
            radius,
            theme::press_shade(s.omnibox_hover, ui.visuals().dark_mode),
        );
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, radius, s.omnibox_hover);
    } else if enabled && resp.has_focus() {
        // Keyboard focus is otherwise invisible on hand-painted buttons.
        // Shared token so every chrome control focuses identically.
        theme::focus_ring(ui, rect, theme::pill_radius(rect.height()).nw);
    }
    // Hover/focus effect: the glyph eases from the muted tone to the accent
    // and lifts a touch; pressing pulls it back. Both tweens are keyed to
    // this button's id and only request repaints while they are moving.
    let hot = enabled && (resp.hovered() || resp.has_focus());
    let t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("icon-hot"), hot, 0.14);
    let press = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("icon-press"), pressed, 0.07);
    let fg = if enabled {
        s.on_surface_variant.lerp_to_gamma(s.primary, t)
    } else {
        s.on_surface_variant
    };
    if enabled {
        paint_scaled(ui.painter(), rect, icon, fg, 1.0 + 0.10 * t - 0.06 * press);
    } else {
        // Disabled: the muted tone at FULL opacity, painted in place. egui's
        // own disabled widgets fade through the painter opacity, but a faded
        // clone at 55% measured ~2.3:1 against the light chrome (almost
        // invisible) and a `ui.scope` would advance the cursor past its
        // (empty) rect plus one item spacing, pushing every following widget
        // 2px right (the omnibox visibly jumped when Reload entered its
        // cooldown). Mirrors `theme::tonal_button_sized`'s disabled tone.
        paint_scaled(ui.painter(), rect, icon, fg, 1.0);
    }
    // Accessible name: an icon-only button otherwise announces nothing to a
    // screen reader (accesskit is part of the default eframe stack). Built
    // lazily inside the closure: accesskit may be off and then no String is
    // allocated per button per frame.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, a11y_name()));
    // Lazy tooltip: no String built per button per frame (see
    // `crate::ui::hover_tooltip`).
    let resp = crate::ui::hover_tooltip_lazy(resp, tooltip);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// One icon drawn on the 24x24 design grid: converts grid coordinates to
/// screen points and emits the stroked/filled shapes.
struct Pen<'a> {
    painter: &'a egui::Painter,
    /// Screen point of grid (0, 0).
    origin: egui::Pos2,
    /// Screen points per grid unit.
    u: f32,
    /// Stroke width in screen points.
    w: f32,
    /// The themed tint every shape uses.
    color: egui::Color32,
}

impl Pen<'_> {
    /// Grid coordinates to screen points.
    fn p(&self, x: f32, y: f32) -> egui::Pos2 {
        self.origin + egui::vec2(x, y) * self.u
    }

    /// Open stroked polyline through the given grid points.
    fn stroke(&self, pts: &[(f32, f32)]) {
        debug_assert!(pts.len() >= 2);
        self.painter.add(egui::Shape::line(
            pts.iter().map(|&(x, y)| self.p(x, y)).collect(),
            egui::Stroke::new(self.w, self.color),
        ));
    }

    /// Closed stroked outline.
    fn closed(&self, pts: &[(f32, f32)]) {
        self.painter.add(egui::Shape::closed_line(
            pts.iter().map(|&(x, y)| self.p(x, y)).collect(),
            egui::Stroke::new(self.w, self.color),
        ));
    }

    /// Stroked circle at grid center `(cx, cy)` with grid radius `r`.
    fn circle(&self, cx: f32, cy: f32, r: f32) {
        self.painter.circle_stroke(
            self.p(cx, cy),
            r * self.u,
            egui::Stroke::new(self.w, self.color),
        );
    }

    /// Stroked ellipse (vertical meridians and the like).
    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.painter.add(egui::Shape::ellipse_stroke(
            self.p(cx, cy),
            egui::vec2(rx * self.u, ry * self.u),
            egui::Stroke::new(self.w, self.color),
        ));
    }

    /// Filled dot.
    fn dot(&self, cx: f32, cy: f32, r: f32) {
        self.painter
            .circle_filled(self.p(cx, cy), r * self.u, self.color);
    }

    /// Stroked rounded rectangle between two grid corners.
    fn rounded_rect(&self, min: (f32, f32), max: (f32, f32), r: f32) {
        let rect = egui::Rect::from_min_max(self.p(min.0, min.1), self.p(max.0, max.1));
        let rounding = (r * self.u).round().clamp(0.0, 255.0) as u8;
        self.painter.rect_stroke(
            rect,
            egui::CornerRadius::same(rounding),
            egui::Stroke::new(self.w, self.color),
            egui::StrokeKind::Middle,
        );
    }

    /// Stroked arc clockwise (screen: increasing angle) from `a0` to `a1`
    /// degrees, 0 at 3 o'clock.
    fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
        let sweep = a1 - a0;
        let steps = ((sweep.abs() / 7.5).ceil() as usize).max(2);
        let pts: Vec<(f32, f32)> = (0..=steps)
            .map(|i| {
                let a = (a0 + sweep * i as f32 / steps as f32).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.stroke(&pts);
    }

    /// Two barbs forming an arrowhead at `tip`, opening back from `dir_deg`
    /// (the travel direction, degrees).
    fn arrow(&self, tip: (f32, f32), dir_deg: f32, len: f32) {
        const SPREAD_DEG: f32 = 28.0;
        for sign in [-1.0_f32, 1.0] {
            let a = (dir_deg + 180.0 + sign * SPREAD_DEG).to_radians();
            self.stroke(&[tip, (tip.0 + len * a.cos(), tip.1 + len * a.sin())]);
        }
    }

    /// Bookmark ribbon with the notch at the bottom; filled (bookmarked)
    /// or hollow (not bookmarked).
    fn bookmark(&self, fill: bool) {
        let outline: Vec<egui::Pos2> = [
            (6.4, 3.6),
            (17.6, 3.6),
            (17.6, 20.6),
            (12.0, 15.4),
            (6.4, 20.6),
        ]
        .iter()
        .map(|&(x, y)| self.p(x, y))
        .collect();
        if fill {
            // Concave at the notch, so the generic path fill would smear:
            // fan the mesh from an interior point instead.
            let center = self.p(12.0, 11.0);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(center, self.color);
            for point in &outline {
                mesh.colored_vertex(*point, self.color);
            }
            for i in 0..5u32 {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 5);
            }
            self.painter.add(mesh);
        }
        // The outline both hollows the ribbon and feathers the mesh edge.
        self.painter.add(egui::Shape::closed_line(
            outline,
            egui::Stroke::new(self.w * if fill { 0.8 } else { 1.0 }, self.color),
        ));
    }
}

#[cfg(test)]
mod tests;
