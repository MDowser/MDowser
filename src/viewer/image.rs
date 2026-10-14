// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::image`: image widgets (inline and standalone blocks).
//!
//! - Images are subresources of the document; the `media` pipeline fetches
//!   and decodes them in the background, so drawing never blocks.
//! - Loading: compact hourglass-emoji placeholder. Failed: warning emoji
//!   with the reason as tooltip. Both keep the alt text visible.
//! - SIZE POLICY: markdown carries no width/height, so raster images render
//!   at their NATURAL pixel size (`fit_to_width`), clamped to the available
//!   reading-column width when wider (aspect preserved, never upscaled).
//!   INLINE SVG is additionally clamped to `config::MAX_INLINE_VECTOR_H`
//!   (32px)
//!   because a vector's intrinsic size is usually a stylesheet decision
//!   (a 512px logo would otherwise dominate a text line); block SVG keeps
//!   its natural size.
//! - POSITION POLICY: the markdown position decides the widget. Standalone
//!   paragraphs render centered blocks; small inline occurrences stay in the
//!   text flow at natural size, while images too large for a text line
//!   (`config::MAX_INLINE_IMAGE_H`, or wider than most of the column) are promoted
//!   to centered blocks by `viewer::markdown` so they never open a tall
//!   blank line around the neighboring text. A run of 2+ consecutive large
//!   images renders as one centered gallery row (not one block each): photo
//!   strips keep their source order and horizontal position while loading.
//! - INLINE padding: a few px on each side keep neighboring glyphs off the
//!   bitmap.
//! - ICON BLOCKS: every inline SVG icon in one block (paragraph or link
//!   stack) renders at the tallest icon's height (aspect preserved, inline
//!   clamp applied), so assets declaring wildly different intrinsic sizes
//!   (a 512px logo next to a `1em` icon) look like the source site's single
//!   CSS icon size; a lone icon keeps its size.
//! - An image inside a link stays clickable and navigates internally.

use super::inline::ImageFrag;
use super::push_link_hit;
use super::LinkHit;
use crate::media::cache::{ImageCache, Status};
use crate::ui::icons;
use crate::ui::theme;
use eframe::egui;
use std::sync::Arc;

/// Canonical inline-image line caps live in [`crate::config`]; re-exported
/// here so existing `super::image::MAX_*` paths keep working.
pub(crate) use crate::config::{MAX_INLINE_IMAGE_H, MAX_INLINE_VECTOR_H};

/// Placeholder box while an image is loading/failed/limited (points).
/// The real widget swaps to the natural size once the texture is ready.
const INLINE_H: f32 = 24.0;

/// Horizontal breathing room around an inline image so neighboring glyphs
/// never touch the bitmap.
const INLINE_PAD_X: f32 = 3.0;

/// Size an inline image renders at: natural pixels clamped to `max_w`
/// (never upscaled); vector sources additionally clamped to
/// [`MAX_INLINE_VECTOR_H`].
pub(crate) fn inline_image_size(natural: egui::Vec2, vector: bool, max_w: f32) -> egui::Vec2 {
    let mut size = fit_to_width(natural, max_w);
    if vector && size.y > MAX_INLINE_VECTOR_H {
        let scale = MAX_INLINE_VECTOR_H / size.y;
        size = egui::vec2((size.x * scale).max(4.0), MAX_INLINE_VECTOR_H);
    }
    size
}

/// Width [`render_inline_image`] allocates for `img` right now: the natural
/// (clamped) size plus its horizontal breathing room, or the compact
/// placeholder box while loading/failed/unregistered.
///
/// Used by the gallery renderer (`viewer::markdown`) to plan centered
/// rows BEFORE drawing them, without duplicating the size policy.
pub(crate) fn inline_allocation_width(images: &ImageCache, img: &ImageFrag, max_w: f32) -> f32 {
    let inner = match images.status(&img.url) {
        Status::Ready { tex, vector } => inline_image_size(tex.size_vec2(), vector, max_w),
        _ => egui::vec2(INLINE_H, INLINE_H),
    };
    inner.x + 2.0 * INLINE_PAD_X
}

/// Height [`render_inline_image`] would use for `img` when it is a decoded
/// VECTOR (SVG), or `None` while loading/failed or when it is a raster.
///
/// Used by the icon-strip normalization in `viewer::markdown`: the strip
/// gives every decoded SVG a common height (the tallest one), like the
/// source site's CSS does.
pub(crate) fn inline_vector_height(
    images: &ImageCache,
    img: &ImageFrag,
    max_w: f32,
) -> Option<f32> {
    match images.status(&img.url) {
        Status::Ready { tex, vector: true } => {
            Some(inline_image_size(tex.size_vec2(), true, max_w).y)
        }
        _ => None,
    }
}

/// Registers `img` for loading on the live document and returns its status.
/// Backdrops (`interactive == false`) never start fetches: they only show
/// whatever is already cached.
fn status_for<'a>(images: &'a mut ImageCache, img: &ImageFrag, interactive: bool) -> Status<'a> {
    if interactive && !images.ensure(&img.url) {
        return Status::Unavailable;
    }
    images.status(&img.url)
}

/// Accessible name for an image widget: author `alt` when present, else a
/// status description (screen readers otherwise get nothing at all).
/// Returns borrowed text when possible to avoid a per-frame allocation;
/// the caller passes it straight to `WidgetInfo`, which copies on use.
fn image_a11y<'a>(alt: &'a str, status: &Status<'_>) -> std::borrow::Cow<'a, str> {
    let alt = alt.trim();
    if !alt.is_empty() && alt != "[image]" {
        return std::borrow::Cow::Borrowed(alt);
    }
    std::borrow::Cow::Borrowed(match status {
        Status::Ready { vector, .. } => {
            if *vector {
                "Vector image"
            } else {
                "Image"
            }
        }
        Status::Loading => "Loading image",
        Status::Failed(_) => "Image could not be loaded",
        Status::Unavailable => "Image limit reached",
    })
}

/// Tooltip for an image: `title` first, then `alt`.
fn tooltip_for<'a>(img: &'a ImageFrag, alt: &'a str) -> Option<&'a str> {
    img.title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| Some(alt).filter(|a| !a.trim().is_empty()))
}

/// Names one image response for screen readers.
///
/// Always `enabled`: a passive image is not a disabled control, and a linked
/// one is a normal target. The old `clickable` flag announced every
/// non-linked image as *disabled* (accesskit `set_disabled`).
fn label_image(resp: &egui::Response, alt: &str, status: &Status<'_>) {
    let a11y = image_a11y(alt, status);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &*a11y));
}

/// Shared tail for the placeholder states (loading / failed / unavailable):
/// clickable interaction, keyboard focus ring (linked placeholders are
/// focusable exactly like a decoded bitmap), accessible name and the link
/// hooks.
fn finish_placeholder(
    ui: &egui::Ui,
    resp: egui::Response,
    clickable: bool,
    a11y: &str,
    link: Option<&Arc<str>>,
    on_link: &mut dyn FnMut(&str),
    links: &mut Vec<LinkHit>,
) -> egui::Response {
    let resp = if clickable {
        resp.interact(egui::Sense::click())
    } else {
        resp
    };
    if clickable && resp.has_focus() {
        theme::focus_ring(ui, resp.rect, theme::radius::SMALL);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, a11y));
    wire_link(&resp, link, clickable, on_link, links);
    resp
}

/// Draws one inline image (inside a paragraph/list line).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_inline_image(
    ui: &mut egui::Ui,
    img: &ImageFrag,
    alt: &str,
    link: Option<&Arc<str>>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    links: &mut Vec<LinkHit>,
) {
    render_inline_image_sized(
        ui,
        img,
        alt,
        link,
        selected,
        interactive,
        on_link,
        images,
        links,
        None,
    );
}

/// Draws one inline VECTOR image at an explicit height (aspect preserved,
/// column-clamped): used to give a strip of consecutive SVG icons a common
/// height, like the source site's CSS does. Raster images ignore `height`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_inline_image_at_height(
    ui: &mut egui::Ui,
    img: &ImageFrag,
    alt: &str,
    link: Option<&Arc<str>>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    links: &mut Vec<LinkHit>,
    height: f32,
) {
    render_inline_image_sized(
        ui,
        img,
        alt,
        link,
        selected,
        interactive,
        on_link,
        images,
        links,
        Some(height),
    );
}

/// Shared body of [`render_inline_image`] /
/// [`render_inline_image_at_height`]; `forced_height` only affects vectors.
#[allow(clippy::too_many_arguments)]
fn render_inline_image_sized(
    ui: &mut egui::Ui,
    img: &ImageFrag,
    alt: &str,
    link: Option<&Arc<str>>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    links: &mut Vec<LinkHit>,
    forced_height: Option<f32>,
) {
    let tooltip = tooltip_for(img, alt);
    let clickable = interactive && link.is_some();
    let status = status_for(images, img, interactive);
    // Natural size clamped to the row width (reading column, table cell or
    // indent already subtracted by egui); `INLINE_PAD_X` keeps glyphs off
    // the bitmap on both sides. A wider image wraps to its own row inside
    // `horizontal_wrapped`.
    let max_w = (ui.max_rect().width() - 2.0 * INLINE_PAD_X).max(4.0);
    let inner = match &status {
        Status::Ready { tex, vector } => match forced_height {
            Some(h) if *vector => fit_to_height(tex.size_vec2(), h, max_w),
            _ => inline_image_size(tex.size_vec2(), *vector, max_w),
        },
        _ => egui::vec2(INLINE_H, INLINE_H),
    };
    let size = egui::vec2(inner.x + 2.0 * INLINE_PAD_X, inner.y);
    let sense = if clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let img_rect = egui::Rect::from_center_size(rect.center(), inner);
    let resp = resp.on_hover_cursor(if clickable {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::Default
    });
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::select_bg(ui),
        );
    }
    // Keyboard focus on linked images: hand-painted bitmaps get no
    // `widgets.active` outline on their own.
    if clickable && resp.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::SMALL);
    }
    // One palette lookup for the placeholder family below.
    let s = theme::scheme_ref(ui);
    match &status {
        Status::Ready { tex, .. } => {
            egui::Image::new(*tex)
                .corner_radius(crate::ui::theme::radius::SMALL)
                .paint_at(ui, img_rect);
        }
        Status::Loading => {
            // Intentionally `Icon::Loading` (hourglass), not the toolbar's
            // `Icon::Reload` arc: the toolbar glyph is an ACTION (click to
            // reload), the placeholder is STATUS (not clickable). Two
            // metaphors for two semantics.
            icons::paint(ui, img_rect, icons::Icon::Loading, s.on_surface_variant);
        }
        Status::Failed(_) => {
            icons::paint(ui, img_rect, icons::Icon::BrokenImage, s.on_surface_variant);
        }
        Status::Unavailable => {
            // Intentionally `Icon::Image` (generic), not `BrokenImage`: the
            // fetch was never attempted (over cap / inert destination), not
            // a broken download. Distinct states, distinct icons.
            icons::paint(ui, img_rect, icons::Icon::Image, s.on_surface_variant);
        }
    }
    let resp = match tooltip {
        Some(t) => crate::ui::hover_tooltip(resp, t),
        None => resp,
    };
    // Screen readers get an accessible name/state for every inline image
    // (hourglass, warning placeholder and decoded bitmap alike).
    label_image(&resp, alt, &status);
    wire_link(&resp, link, clickable, on_link, links);
}

/// Draws a standalone image (paragraph that only contains the image).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_image_block(
    ui: &mut egui::Ui,
    img: &ImageFrag,
    alt: &str,
    link: Option<&Arc<str>>,
    selected: bool,
    interactive: bool,
    on_link: &mut dyn FnMut(&str),
    images: &mut ImageCache,
    links: &mut Vec<LinkHit>,
) {
    let clickable = interactive && link.is_some();
    let tooltip = tooltip_for(img, alt);
    let status = status_for(images, img, interactive);
    ui.add_space(theme::spacing::S);
    match &status {
        Status::Ready { tex, .. } => {
            // Natural size clamped to the column (block position: centered).
            let size = fit_to_width(tex.size_vec2(), ui.available_width());
            ui.vertical_centered(|ui| {
                let sense = if clickable {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (rect, resp) = ui.allocate_exact_size(size, sense);
                let resp = resp.on_hover_cursor(if clickable {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                });
                if selected {
                    ui.painter().rect_filled(
                        rect.expand(3.0),
                        egui::CornerRadius::same(theme::radius::TILE),
                        theme::select_bg(ui),
                    );
                }
                // Keyboard focus on linked images (see inline path above).
                if clickable && resp.has_focus() {
                    theme::focus_ring(ui, rect, theme::radius::MENU);
                }
                egui::Image::new(*tex)
                    .corner_radius(theme::radius::MENU)
                    .paint_at(ui, rect);
                let resp = match tooltip {
                    Some(t) => crate::ui::hover_tooltip(resp, t),
                    None => resp,
                };
                label_image(&resp, alt, &status);
                // Right-click on a linked image offers "Open link in new
                // tab"; anchors are jump targets, never new tabs.
                wire_link(&resp, link, clickable, on_link, links);
            });
        }
        Status::Loading => {
            let resp = image_placeholder(ui, icons::Icon::Loading, alt, "Loading image...", None);
            let a11y = image_a11y(alt, &status);
            finish_placeholder(ui, resp, clickable, &a11y, link, on_link, links);
        }
        Status::Failed(reason) => {
            let resp = image_placeholder(
                ui,
                icons::Icon::BrokenImage,
                alt,
                "Image could not be loaded",
                Some(reason),
            );
            let a11y = image_a11y(alt, &status);
            finish_placeholder(ui, resp, clickable, &a11y, link, on_link, links);
        }
        Status::Unavailable => {
            let resp = image_placeholder(
                ui,
                icons::Icon::Image,
                alt,
                "Image limit reached for this page",
                None,
            );
            let a11y = image_a11y(alt, &status);
            finish_placeholder(ui, resp, clickable, &a11y, link, on_link, links);
        }
    }
    // Caption: only when the author wrote real alt text.
    let caption = alt.trim();
    if !caption.is_empty() && caption != "[image]" {
        ui.add_space(2.0);
        ui.vertical_centered(|ui| {
            // Captions are document text: keep them readable (and visibly
            // highlighted) during "Select all" like every other string.
            let mut rt = theme::weak(ui, caption);
            if selected {
                rt = theme::selected_text(ui, rt);
            }
            ui.label(rt);
        });
    }
    ui.add_space(theme::spacing::M);
}

/// Wires a rendered image response to its enclosing link: clicking
/// navigates, and the screen rect feeds the right-click menu's "Open link in
/// new tab". Shared by the inline image, the decoded block image AND the
/// loading/failed placeholder (a linked image used to be dead until its
/// texture arrived, and after a permanent failure).
fn wire_link(
    resp: &egui::Response,
    link: Option<&Arc<str>>,
    clickable: bool,
    on_link: &mut dyn FnMut(&str),
    links: &mut Vec<LinkHit>,
) {
    if !clickable {
        return;
    }
    let Some(dest) = link else {
        return;
    };
    // In-document anchors are jump targets, never new tabs.
    if !dest.starts_with('#') {
        push_link_hit(links, resp.rect, dest);
    }
    if resp.clicked() {
        on_link(dest);
    }
}

/// Bordered placeholder card used while loading / on failure. Returns the
/// card's response so [`wire_link`] can keep a linked image clickable in
/// every state.
fn image_placeholder(
    ui: &mut egui::Ui,
    icon: icons::Icon,
    alt: &str,
    message: &str,
    reason: Option<&str>,
) -> egui::Response {
    let s = theme::scheme_ref(ui);
    let avail = ui.available_width().max(0.0);
    // The card frame adds 12px inner margin + 1px stroke per side; cap the
    // inner width by `avail - 26` so the bordered box never overflows the
    // reading column on narrow windows (it used to clip its right border).
    let box_w = theme::content_max_width(avail)
        .min(420.0)
        .min((avail - 26.0).max(1.0));
    ui.vertical_centered(|ui| {
        theme::card(ui)
            .show(ui, |ui| {
                ui.set_min_width(box_w.max(0.0));
                ui.set_max_width(box_w.max(0.0));
                ui.vertical_centered(|ui| {
                    ui.add_space(theme::spacing::XS);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    icons::paint(ui, rect, icon, s.on_surface_variant);
                    let label = if alt.trim().is_empty() || alt.trim() == "[image]" {
                        message.to_string()
                    } else {
                        format!("{message}: {}", alt.trim())
                    };
                    let rt = egui::RichText::new(label)
                        .small()
                        .color(s.on_surface_variant);
                    let resp = ui.add(egui::Label::new(rt).selectable(false).truncate());
                    if let Some(reason) = reason {
                        crate::ui::hover_tooltip(resp, reason);
                    }
                    ui.add_space(2.0);
                });
            })
            .response
    })
    .inner
}

/// Natural image size capped to `max_w`, preserving the aspect ratio.
///
/// - Markdown carries no width/height: the image's own pixel size is the
///   size to render (1:1), never upscaled (no blur, standard browser
///   behavior).
/// - Wider than the available width (reading column / cell): scaled down to
///   `max_w` so it never overflows the viewport horizontally.
/// - Degenerate textures (0 on a side) fall back to the compact placeholder
///   box so the allocation is never zero-sized.
fn fit_to_width(natural: egui::Vec2, max_w: f32) -> egui::Vec2 {
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return egui::vec2(INLINE_H, INLINE_H);
    }
    let scale = (max_w.max(1.0) / natural.x).min(1.0);
    // 4px floor on the SMALLER side keeps a clickable target for hairline
    // images; growing both sides by one factor preserves the aspect ratio
    // (independent per-axis floors stretched a 2x40 into a 4x40).
    floor_smaller_side(natural * scale, 4.0)
}

/// Vector size scaled so its height is `height` (aspect preserved), still
/// capped to `max_w` so a very wide icon never overflows the row.
/// Degenerate textures fall back to the compact placeholder box.
fn fit_to_height(natural: egui::Vec2, height: f32, max_w: f32) -> egui::Vec2 {
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return egui::vec2(INLINE_H, INLINE_H);
    }
    let scale = (height.max(1.0) / natural.y).min(max_w.max(1.0) / natural.x);
    floor_smaller_side(natural * scale, 4.0)
}

/// Grows `size` uniformly, when needed, so its SMALLER side reaches
/// `min_side`; the aspect ratio never changes.
fn floor_smaller_side(size: egui::Vec2, min_side: f32) -> egui::Vec2 {
    let smallest = size.x.min(size.y);
    if smallest >= min_side || smallest <= 0.0 {
        return size;
    }
    size * (min_side / smallest)
}

#[cfg(test)]
mod tests {
    use super::{fit_to_width, INLINE_H};
    use eframe::egui;

    #[test]
    fn natural_size_is_kept_and_clamped_to_width() {
        // Fits the column: exact natural size, no upscale.
        assert_eq!(
            fit_to_width(egui::vec2(100.0, 50.0), 760.0),
            egui::vec2(100.0, 50.0)
        );
        // Smaller than the column: NEVER upscaled.
        assert_eq!(
            fit_to_width(egui::vec2(16.0, 16.0), 760.0),
            egui::vec2(16.0, 16.0)
        );
        // Wider than the column: scaled to the width, aspect preserved.
        assert_eq!(
            fit_to_width(egui::vec2(1520.0, 380.0), 760.0),
            egui::vec2(760.0, 190.0)
        );
        // Tall image narrower than the column keeps its natural size.
        assert_eq!(
            fit_to_width(egui::vec2(300.0, 2000.0), 760.0),
            egui::vec2(300.0, 2000.0)
        );
        // Hairline images grow uniformly to the 4px clickable floor: the
        // aspect ratio must survive (a 2x40 used to become 4x40).
        assert_eq!(
            fit_to_width(egui::vec2(2.0, 40.0), 760.0),
            egui::vec2(4.0, 80.0)
        );
    }

    #[test]
    fn degenerate_sizes_fall_back_to_the_placeholder() {
        assert_eq!(
            fit_to_width(egui::vec2(0.0, 0.0), 760.0),
            egui::vec2(INLINE_H, INLINE_H)
        );
        assert_eq!(
            fit_to_width(egui::vec2(10.0, 0.0), 760.0),
            egui::vec2(INLINE_H, INLINE_H)
        );
        // Degenerate available width still yields a non-zero box.
        let s = fit_to_width(egui::vec2(100.0, 50.0), 0.0);
        assert!(s.x >= 4.0 && s.y >= 1.0);
    }
}
