// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::decode::raster`: raster image decoding (PNG/JPEG/GIF/WebP/ICO
//! through `image`, JPEG XL and AVIF through their pure-Rust decoders).
//!
//! Every path ends in the shared `finish` downscale (EXIF orientation
//! included for the `image` decoders); dimension/pixel caps live in
//! `config` and are re-checked on the decoded frame.

use super::*;

/// Standard `image` decoders with strict allocation limits.
///
/// The header dimensions are checked BEFORE decoding: `image::Limits` has no
/// pixel-count limit, so a 12288x12288 PNG/JPEG/GIF/WebP (over the 48 MP cap)
/// would otherwise be fully decoded — up to ~192 MiB of output plus decoder
/// scratch — only to be rejected by `finish` afterwards.
pub(super) fn decode_raster(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    let dims = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("image: {e}"))?
        .into_dimensions()
        .map_err(|e| format!("image: {e}"))?;
    check_dims(dims.0, dims.1)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(config::MAX_IMAGE_DIM);
    limits.max_image_height = Some(config::MAX_IMAGE_DIM);
    // 48 MP RGBA is ~192 MiB; the rest is decoder scratch (bound in config).
    limits.max_alloc = Some(config::MAX_DECODER_ALLOC_BYTES);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("image: {e}"))?;
    reader.limits(limits);
    // Decode through the trait so EXIF orientation is available before the
    // pixels are laid out: `image` never applies it automatically, so a
    // phone photo (JPEG/WebP/TIFF orientation tag) used to render rotated
    // with its layout dimensions swapped.
    use image::ImageDecoder as _;
    let mut decoder = reader.into_decoder().map_err(|e| format!("image: {e}"))?;
    let orientation = decoder.orientation().map_err(|e| format!("image: {e}"))?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| format!("image: {e}"))?;
    img.apply_orientation(orientation);
    finish(img, max_dim)
}

/// JPEG XL via `jxl-oxide` (pure Rust). Header dims are checked first.
pub(super) fn decode_jxl(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    use image::ImageDecoder as _;
    let mut decoder = jxl_oxide::integration::JxlDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("jxl: {e}"))?;
    let (w, h) = decoder.dimensions();
    check_dims(w, h)?;
    // `DynamicImage::from_decoder` allocates the full output buffer with no
    // fallible reservation (it aborts the process on OOM). Bound the frame
    // by the decoder allocation budget before handing it over.
    let out_bytes = u64::from(w)
        .saturating_mul(u64::from(h))
        .saturating_mul(u64::from(decoder.color_type().bytes_per_pixel()));
    if out_bytes > config::MAX_DECODER_ALLOC_BYTES {
        return Err("jxl: decoded frame exceeds the allocation budget".to_string());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(config::MAX_IMAGE_DIM);
    limits.max_image_height = Some(config::MAX_IMAGE_DIM);
    decoder
        .set_limits(limits)
        .map_err(|e| format!("jxl: {e}"))?;
    let img = image::DynamicImage::from_decoder(decoder).map_err(|e| format!("jxl: {e}"))?;
    finish(img, max_dim)
}

/// AVIF via `avif-decode` (pure-Rust `rav1d` AV1 decoder).
/// The container is parsed first so oversized images fail before decode.
/// Both the primary (color) and the auxiliary alpha item dimensions are
/// checked: `avif-decode` decodes the alpha frame without its own limit and
/// only validates its size after rav1d allocated the frame buffers.
pub(super) fn decode_avif(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    let mut cursor = std::io::Cursor::new(bytes);
    let data = avif_parse::read_avif(&mut cursor).map_err(|e| format!("avif: {e}"))?;
    let meta = data
        .primary_item_metadata()
        .map_err(|e| format!("avif: {e}"))?;
    check_dims(meta.max_frame_width.get(), meta.max_frame_height.get())?;
    if let Some(alpha) = data
        .alpha_item_metadata()
        .map_err(|e| format!("avif: {e}"))?
    {
        check_dims(alpha.max_frame_width.get(), alpha.max_frame_height.get())?;
    }
    // Free the parsed container copy before the heavy decode.
    drop(data);
    let img = avif_decode::Decoder::from_avif(bytes)
        .map_err(|e| format!("avif: {e}"))?
        .to_image()
        .map_err(|e| format!("avif: {e}"))?;
    color_from_avif(img, max_dim)
}

/// Converts `avif-decode` output (8/16-bit RGB/RGBA/gray) to `ColorImage`.
///
/// Routes through [`finish`] like every other decoder: it re-checks the REAL
/// decoded frame against the caps (defense in depth over the container
/// metadata) and downscales to `max_dim` before the GPU upload.
///
/// Every arm validates the frame dimensions BEFORE reserving its RGBA
/// buffer: the container metadata was checked earlier, but rav1d decoded
/// this frame, and `Vec::with_capacity` aborts the process on OOM instead
/// of returning an error.
fn color_from_avif(img: avif_decode::Image, max_dim: u32) -> Result<DecodedImage, String> {
    use avif_decode::Image as I;
    let (w, h, data): (u32, u32, Vec<u8>) = match img {
        I::Rgba8(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                *dst = [p.r, p.g, p.b, p.a];
            }
            (w, h, data)
        }
        I::Rgb8(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                *dst = [p.r, p.g, p.b, 255];
            }
            (w, h, data)
        }
        I::Gray8(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                let x = p.value();
                *dst = [x, x, x, 255];
            }
            (w, h, data)
        }
        // 16-bit formats: keep the high byte (8-bit preview).
        I::Rgba16(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                *dst = [
                    (p.r >> 8) as u8,
                    (p.g >> 8) as u8,
                    (p.b >> 8) as u8,
                    (p.a >> 8) as u8,
                ];
            }
            (w, h, data)
        }
        I::Rgb16(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                *dst = [(p.r >> 8) as u8, (p.g >> 8) as u8, (p.b >> 8) as u8, 255];
            }
            (w, h, data)
        }
        I::Gray16(v) => {
            let (w, h) = avif_frame_dims(v.width(), v.height())?;
            let mut data = vec![0u8; avif_rgba_bytes(w, h)];
            for (dst, p) in data.as_chunks_mut::<4>().0.iter_mut().zip(v.pixels()) {
                let x = (p.value() >> 8) as u8;
                *dst = [x, x, x, 255];
            }
            (w, h, data)
        }
    };
    let rgba = image::RgbaImage::from_raw(w, h, data)
        .ok_or_else(|| "avif: inconsistent frame buffer".to_string())?;
    finish(image::DynamicImage::ImageRgba8(rgba), max_dim)
}

/// Validates the REAL decoded frame before any allocation for it: the caps
/// are re-checked and the RGBA copy is bounded by the decoder budget, so a
/// frame whose size disagrees with the container metadata (already checked
/// before the decode) can never trigger a giant allocation.
fn avif_frame_dims(width: usize, height: usize) -> Result<(u32, u32), String> {
    let w = avif_dim(width)?;
    let h = avif_dim(height)?;
    check_dims(w, h)?;
    if avif_rgba_bytes(w, h) as u64 > config::MAX_DECODER_ALLOC_BYTES {
        return Err("avif: decoded frame exceeds the allocation budget".to_string());
    }
    Ok((w, h))
}

/// Exact RGBA buffer length of a validated frame (cannot overflow: the
/// dims went through `check_dims`, so both sides are <= `MAX_IMAGE_DIM`).
fn avif_rgba_bytes(w: u32, h: u32) -> usize {
    (w as usize) * (h as usize) * 4
}

/// `usize` frame side -> `u32`, erroring on the impossible overflow instead
/// of truncating (the caller's `finish` re-checks the real caps).
fn avif_dim(value: usize) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "avif: frame side too large".to_string())
}
