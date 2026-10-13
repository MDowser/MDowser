// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::decode`: pure-Rust image decoding to egui pixels.
//!
//! SUPPORTED FORMATS (product requirement):
//! PNG, JPEG (`.jpg`/`.jpeg`), GIF, WebP, JPEG XL and AVIF.
//! ICO is decoded too: it is how most sites serve `/favicon.ico`.
//! SVG is rasterized STATICALLY (no scripting, no network, no local file
//! access): vector logos/icons and badge images render; HTML and every
//! other type stay unsupported.
//!
//! SAFETY POSTURE:
//! - The format is sniffed from magic bytes; the decoder must then accept
//!   the bytes (content-type validation happens before this module).
//! - Dimensions and total pixels are capped (`config::MAX_IMAGE_DIM`,
//!   `MAX_IMAGE_PIXELS`); `image` decoders additionally get `Limits` so a
//!   decompression bomb is rejected before allocation.
//! - Everything is downscaled to `MAX_TEXTURE_DIM` before the GPU upload:
//!   the reading column is at most 760 logical px, so full-res textures
//!   would only waste VRAM. The downscale runs on the CPU (`image`'s box
//!   `thumbnail`, which averages every source pixel) rather than on the
//!   GPU: epaint's `TextureOptions` mipmaps are glow-only (this app runs
//!   wgpu), so a 3072-px texture drawn into the reading column would be
//!   plain-bilinear minified and alias while scrolling. The cap keeps the
//!   worst-case minification under ~1.2x at 200% DPI with the maximum zoom
//!   (760 logical px x 1.75 zoom x 2.0 scale = 2660 physical px).
//! - SVG: `usvg`/`resvg` parse + rasterize the static tree; `<script>` and
//!   `<foreignObject>` are ignored by design, `href` strings are never
//!   resolved (external, local and `data:` alike), DTDs are rejected, and
//!   `marker`/`pattern` elements (any namespace prefix) plus CSS `filter`
//!   declarations are rejected while `filter`/`fe*` markup is STRIPPED
//!   (ignored) before parsing (`sanitize_svg_structure`), so a badge that
//!   ships a blur shadow renders unfiltered instead of failing. `<use>`
//!   expansion is bounded, and the raster buffer is capped at
//!   `MAX_TEXTURE_DIM` per side. Fonts are the
//!   app's own bundled faces (Noto Sans plus its emoji/CJK/script
//!   fallbacks), never system fonts.
//! - GIF/JXL animations and AVIF sequences render their first frame only;
//!   there is no animation scheduler (single static texture per URL).

use crate::config;
use eframe::egui;

/// A decoded image ready for `Context::load_texture`.
pub struct DecodedImage {
    /// RGBA8 pixels, downscaled to `MAX_TEXTURE_DIM` on the longest side.
    pub pixels: egui::ColorImage,
    /// True for SVG sources: vector art whose intrinsic size is often a
    /// stylesheet decision (agent icons, logos). Inline vector images are
    /// clamped to an icon-sized box so a 512px logo does not dominate a
    /// text line; raster photos keep their natural size.
    pub vector: bool,
}

/// Image formats accepted by the decoder (magic-byte sniffed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    WebP,
    Jxl,
    Avif,
    Ico,
    Svg,
}

/// Identifies the format from magic bytes (no extension, no header trust).
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if looks_like_svg(bytes) {
        return Some(Format::Svg);
    }
    // JPEG XL bare codestream.
    if bytes.starts_with(&[0xFF, 0x0A]) {
        return Some(Format::Jxl);
    }
    // JPEG XL inside its container box.
    if bytes.len() >= 12
        && bytes[..12]
            == [
                0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
            ]
    {
        return Some(Format::Jxl);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Format::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Format::WebP);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // Any ISO-BMFF header is treated as a potential AVIF: the container
        // parser + decoder validate it (brands like `avif`, `avis` or the
        // `mif1` major brand with an `avif` compatible brand are all used).
        return Some(Format::Avif);
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some(Format::Ico);
    }
    None
}

/// Sniffs an SVG document: optional UTF-8 BOM/whitespace, then XML prolog
/// components (declaration, comments, DOCTYPE) and finally the root `<svg`
/// element. Only the first KiB is inspected (the tree parser validates the
/// rest). Lossy decoding: a multi-byte character straddling the 1 KiB cut
/// must not misclassify a valid SVG as an unknown format.
///
/// Missing a real SVG here only costs a "unrecognized image format"
/// placeholder (fail-closed), but comments before the root are common in
/// hand-written and exported files, so they are accepted.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let mut t = text.trim_start_matches('\u{feff}').trim_start();
    // Skip well-formed prolog pieces. A malformed piece (no terminator)
    // means the file is not a decodable SVG anyway: fail closed.
    loop {
        if let Some(rest) = t.strip_prefix("<?xml") {
            match rest.find("?>") {
                Some(end) => t = rest[end + 2..].trim_start(),
                None => return false,
            }
        } else if let Some(rest) = t.strip_prefix("<!--") {
            match rest.find("-->") {
                Some(end) => t = rest[end + 3..].trim_start(),
                None => return false,
            }
        } else if t.len() >= 9 && t.as_bytes()[..9].eq_ignore_ascii_case(b"<!DOCTYPE") {
            // The DTD itself is rejected by the decoder, but its presence
            // must not hide the root tag from the sniffer. The internal
            // subset (`[ ... ]`) may contain `>` (entity declarations), so
            // the closing `>` is the first one at bracket depth zero.
            let rest = &t[9..];
            let mut depth = 0i32;
            let mut end = None;
            for (i, c) in rest.char_indices() {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    '>' if depth <= 0 => {
                        end = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            match end {
                Some(end) => t = rest[end + 1..].trim_start(),
                None => return false,
            }
        } else {
            break;
        }
    }
    t.starts_with("<svg")
}

/// Decodes `bytes` into RGBA8 pixels after validating format + size caps.
pub fn decode(bytes: &[u8]) -> Result<DecodedImage, String> {
    decode_with_max_dim(bytes, config::MAX_TEXTURE_DIM)
}

/// Decodes like [`decode`] with an explicit longest-side texture cap.
///
/// - Document images use `MAX_TEXTURE_DIM`; favicons use a much smaller cap
///   (`config::MAX_FAVICON_DIM`): they draw at the toolbar icon size
///   (~27 px), so a full-size texture would only waste VRAM (the favicon
///   cache is not part of the per-document image budget).
/// - The cap is clamped to `MAX_TEXTURE_DIM` (never larger).
pub fn decode_with_max_dim(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    let max_dim = max_dim.clamp(1, config::MAX_TEXTURE_DIM);
    match sniff(bytes) {
        Some(Format::Jxl) => decode_jxl(bytes, max_dim),
        Some(Format::Avif) => decode_avif(bytes, max_dim),
        Some(Format::Svg) => decode_svg(bytes, max_dim),
        Some(Format::Png | Format::Jpeg | Format::Gif | Format::WebP | Format::Ico) => {
            decode_raster(bytes, max_dim)
        }
        None => Err("unrecognized image format".to_string()),
    }
}

/// Rejects images over the per-side / total-pixel caps before heavy work.
fn check_dims(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("image has zero size".to_string());
    }
    if width > config::MAX_IMAGE_DIM || height > config::MAX_IMAGE_DIM {
        return Err(format!(
            "image is too large ({}x{}, max {} px per side)",
            width,
            height,
            config::MAX_IMAGE_DIM
        ));
    }
    if u64::from(width) * u64::from(height) > config::MAX_IMAGE_PIXELS {
        return Err("image has too many pixels".to_string());
    }
    Ok(())
}

/// Pixels -> `ColorImage`, downscaling to the texture cap when needed.
fn finish(img: image::DynamicImage, max_dim: u32) -> Result<DecodedImage, String> {
    check_dims(img.width(), img.height())?;
    let max_dim = max_dim.clamp(1, config::MAX_TEXTURE_DIM);
    let img = if img.width().max(img.height()) > max_dim {
        img.thumbnail(max_dim, max_dim)
    } else {
        img
    };
    let rgba = img.into_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Ok(DecodedImage {
        pixels: egui::ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()),
        vector: false,
    })
}

mod raster;
mod svg;

use raster::{decode_avif, decode_jxl, decode_raster};
use svg::decode_svg;

pub(crate) use svg::decode_svg_pixels;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_svg;
