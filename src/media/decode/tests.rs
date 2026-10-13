// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{check_dims, decode, sniff, Format};
use eframe::egui;
use image::ImageEncoder as _;

/// Encodes a tiny PNG in memory so the raster path is tested end to end.
fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), 3, 2, image::ExtendedColorType::Rgba8)
        .expect("png encode");
    out
}

#[test]
fn sniffs_supported_magics() {
    assert_eq!(sniff(&[0xFF, 0x0A, 0x00]), Some(Format::Jxl));
    assert_eq!(
        sniff(&[0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A]),
        Some(Format::Jxl)
    );
    assert_eq!(sniff(b"\x89PNG\r\n\x1a\nxx"), Some(Format::Png));
    assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Format::Jpeg));
    assert_eq!(sniff(b"GIF89a...."), Some(Format::Gif));
    assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(Format::WebP));
    assert_eq!(sniff(b"\x00\x00\x00\x18ftypavif...."), Some(Format::Avif));
    assert_eq!(sniff(b"\x00\x00\x00\x18ftypmif1...."), Some(Format::Avif));
    assert_eq!(sniff(&[0x00, 0x00, 0x01, 0x00, 0x01]), Some(Format::Ico));
    assert_eq!(
        sniff(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#),
        Some(Format::Svg)
    );
    assert_eq!(
        sniff(br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>"#),
        Some(Format::Svg)
    );
    assert_eq!(sniff(b"<html><body>"), None);
    assert_eq!(sniff(b"not an image at all"), None);
}

/// Comments and a DOCTYPE before the root are common in exported SVGs.
/// The sniffer skips well-formed prolog pieces instead of rejecting
/// the document as an unknown format.
#[test]
fn sniffs_svg_behind_prolog_and_comments() {
    assert_eq!(
        sniff(br#"<!-- exported by tool --><svg xmlns="http://www.w3.org/2000/svg"/>"#),
        Some(Format::Svg)
    );
    assert_eq!(
        sniff(
            br#"<?xml version="1.0"?><!-- a --><!-- b --><svg xmlns="http://www.w3.org/2000/svg"/>"#
        ),
        Some(Format::Svg)
    );
    assert_eq!(
        sniff(
            b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/x.dtd\">\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ),
        Some(Format::Svg)
    );
    // An unterminated comment hides no root: fail closed.
    assert_eq!(sniff(b"<!-- forever <svg"), None);
    assert_eq!(sniff(b"<!-- only a comment -->"), None);
}

#[test]
fn decodes_static_svg_to_color_image() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
        <rect width="10" height="10" fill="#ff0000"/>
    </svg>"##;
    let decoded = decode(svg).expect("decode svg");
    assert_eq!(decoded.pixels.size, [10, 10]);
    let px = decoded.pixels.pixels[0];
    assert_eq!((px.r(), px.g(), px.b()), (255, 0, 0));
}

/// SVG `<text>` in a non-Latin script must rasterize through the bundled
/// Noto faces (usvg's fallback walks the fontdb). Each script gets its
/// own text element: usvg reshapes a whole run per fallback font, so one
/// long mixed run is best-effort. Missing coverage would leave the
/// corresponding band fully transparent.
#[test]
fn svg_foreign_text_rasterizes_with_bundled_fonts() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="90">
        <text x="1" y="22" font-size="18">中文</text>
        <text x="1" y="52" font-size="18">Tiếng Việt</text>
        <text x="1" y="82" font-size="18">مرحبا</text>
    </svg>"##;
    let decoded = decode(svg.as_bytes()).expect("decode svg text");
    assert_eq!(decoded.pixels.size, [240, 90]);
    let ink_in = |y0: usize, y1: usize| {
        decoded.pixels.pixels[y0 * 240..y1 * 240]
            .iter()
            .any(|p| p.a() > 0)
    };
    assert!(ink_in(0, 30), "CJK text must draw pixels");
    assert!(ink_in(30, 60), "Vietnamese text must draw pixels");
    assert!(ink_in(60, 90), "Arabic text must draw pixels");
}

#[test]
fn svg_scale_is_capped_like_raster_images() {
    // A vector document declaring a huge viewport must rasterize at the
    // texture cap, never at the declared size.
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}">
            <rect width="100%" height="100%" fill="blue"/>
        </svg>"#,
        100_000u32, 20_000u32
    );
    let decoded = decode(svg.as_bytes()).expect("decode svg");
    let cap = crate::config::MAX_TEXTURE_DIM as usize;
    assert!(decoded.pixels.size[0] <= cap);
    assert!(decoded.pixels.size[1] <= cap);
}

#[test]
fn decodes_png_to_color_image() {
    let decoded = decode(&tiny_png()).expect("decode png");
    assert_eq!(decoded.pixels.size, [3, 2]);
    assert_eq!(
        decoded.pixels.pixels[0],
        egui::Color32::from_rgba_unmultiplied(10, 20, 30, 255)
    );
}

/// Real ICO decode from a LOCAL fixture (the app's own Windows icon):
/// no network and no third-party host.
#[test]
fn decodes_ico_fixture() {
    let bytes = include_bytes!("../../../assets/icons/icon.ico");
    assert_eq!(sniff(bytes), Some(Format::Ico));
    let decoded = decode(bytes).expect("decode ico");
    assert!(decoded.pixels.size[0] > 0 && decoded.pixels.size[1] > 0);
}

/// Real AVIF decode from bytes embedded below (from the `avif-decode`
/// test suite, BSD-3-Clause): no network, no third-party host and no
/// fixture file on disk.
#[test]
fn decodes_avif_fixture() {
    assert_eq!(sniff(TINY_AVIF), Some(Format::Avif));
    let decoded = decode(TINY_AVIF).expect("decode avif");
    assert!(decoded.pixels.size[0] > 0 && decoded.pixels.size[1] > 0);
}

/// A 16x16 AVIF image, embedded so the decode path stays covered without
/// a fixture file (bytes taken from the `avif-decode` test suite,
/// BSD-3-Clause).
#[rustfmt::skip]
const TINY_AVIF: &[u8] = &[
    0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66,
    0x00, 0x00, 0x00, 0x00, 0x61, 0x76, 0x69, 0x66, 0x6d, 0x69, 0x66, 0x31,
    0x6d, 0x69, 0x61, 0x66, 0x4d, 0x41, 0x31, 0x41, 0x00, 0x00, 0x00, 0xeb,
    0x6d, 0x65, 0x74, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x21,
    0x68, 0x64, 0x6c, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x70, 0x69, 0x63, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x70, 0x69, 0x74,
    0x6d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1e, 0x69,
    0x6c, 0x6f, 0x63, 0x00, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x13, 0x00, 0x00, 0x00,
    0x4a, 0x00, 0x00, 0x00, 0x28, 0x69, 0x69, 0x6e, 0x66, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1a, 0x69, 0x6e, 0x66, 0x65, 0x02,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x61, 0x76, 0x30, 0x31, 0x43,
    0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x00, 0x00, 0x00, 0x6a, 0x69, 0x70, 0x72,
    0x70, 0x00, 0x00, 0x00, 0x4b, 0x69, 0x70, 0x63, 0x6f, 0x00, 0x00, 0x00,
    0x14, 0x69, 0x73, 0x70, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x70, 0x69, 0x78,
    0x69, 0x00, 0x00, 0x00, 0x00, 0x03, 0x08, 0x08, 0x08, 0x00, 0x00, 0x00,
    0x0c, 0x61, 0x76, 0x31, 0x43, 0x81, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x13, 0x63, 0x6f, 0x6c, 0x72, 0x6e, 0x63, 0x6c, 0x78, 0x00, 0x01, 0x00,
    0x02, 0x00, 0x06, 0x80, 0x00, 0x00, 0x00, 0x17, 0x69, 0x70, 0x6d, 0x61,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x01,
    0x02, 0x83, 0x04, 0x00, 0x00, 0x00, 0x52, 0x6d, 0x64, 0x61, 0x74, 0x12,
    0x00, 0x0a, 0x08, 0x38, 0x0c, 0xff, 0xda, 0x40, 0x40, 0x81, 0xa4, 0x32,
    0x3c, 0x17, 0x82, 0x63, 0x04, 0xc2, 0x49, 0x24, 0x93, 0x20, 0x80, 0x00,
    0xc9, 0x63, 0xf0, 0x19, 0x92, 0x4f, 0xef, 0x7a, 0x8b, 0x93, 0x00, 0x3f,
    0x8b, 0x43, 0xf3, 0x10, 0x3b, 0xb1, 0xd2, 0x70, 0x66, 0x37, 0xd3, 0xe0,
    0x2e, 0x9f, 0x49, 0x14, 0x2f, 0xed, 0x39, 0x9c, 0xaf, 0xc7, 0xc4, 0x7f,
    0xc8, 0x6f, 0x83, 0xdd, 0xc1, 0x64, 0x99, 0x79, 0xa0, 0xa1, 0xc5, 0xd5,
    0x58,
];

#[test]
fn rejects_non_images_and_bombs() {
    assert!(decode(b"<html>hi</html>").is_err());
    assert!(decode(b"").is_err());
    assert!(check_dims(0, 10).is_err());
    assert!(check_dims(crate::config::MAX_IMAGE_DIM + 1, 10).is_err());
    assert!(check_dims(10_000, 10_000).is_err());
    assert!(check_dims(100, 100).is_ok());
}

#[test]
fn downscales_oversized_textures() {
    // A width over MAX_TEXTURE_DIM (but under the pixel cap): the
    // decoded texture must be capped on the long side.
    let wide = crate::config::MAX_TEXTURE_DIM + 1000;
    let img = image::RgbaImage::from_pixel(wide, 10, image::Rgba([1, 2, 3, 255]));
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), wide, 10, image::ExtendedColorType::Rgba8)
        .expect("png encode");
    let decoded = decode(&out).expect("decode");
    assert_eq!(
        decoded.pixels.size[0],
        crate::config::MAX_TEXTURE_DIM as usize
    );
    assert!(decoded.pixels.size[1] < 10);
}

/// Favicons decode at their own (much smaller) cap; the aspect ratio is
/// preserved and the result is never upscaled.
#[test]
fn explicit_cap_bounds_the_texture_side() {
    let img = image::RgbaImage::from_pixel(512, 256, image::Rgba([9, 9, 9, 255]));
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), 512, 256, image::ExtendedColorType::Rgba8)
        .expect("png encode");
    let decoded = super::decode_with_max_dim(&out, 128).expect("decode");
    assert_eq!(decoded.pixels.size, [128, 64]);
    // Smaller than the cap: never upscaled.
    let small = super::decode_with_max_dim(&tiny_png(), 256).expect("decode");
    assert_eq!(small.pixels.size, [3, 2]);
}
