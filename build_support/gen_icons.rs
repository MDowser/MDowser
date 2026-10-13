// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `gen_icons`: build-time rasterization of `assets/icon.svg` into
//! `assets/icons/` (PNG ladder + `icon.ico` + `icon.icns`).
//!
//! Why build-time: `src/assets.rs` embeds `icon-256.png` with
//! `include_bytes!`, `build.rs` reads `icon.ico` for the Windows resource
//! and `scripts/build-dist.sh` copies the ladder and `icon.icns` into the
//! packages — so the files must exist BEFORE the crate compiles. A fresh
//! checkout (or a deleted `assets/icons/`) therefore needs no manual step;
//! this replaced the standalone `tools/gen-icons` workspace package.
//!
//! - Regenerates only when NEEDED: an output is missing, or `assets/icon.svg`
//!   changed since the last run (a hash is kept in `OUT_DIR`). A normal build
//!   does one small file read and stops.
//! - The rasterizer is the same crate and features as the app's SVG
//!   pipeline (`resvg`), so regenerating reproduces the committed files byte
//!   for byte; packaging never has to run it.
//! - A generation failure with every output present is a warning (read-only
//!   source trees keep their stale but usable icons); missing outputs fail
//!   the build with a clear message instead of a confusing `include_bytes`
//!   error later.

use std::fs;
use std::path::Path;

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

/// Outputs of one generation, relative to `assets/icons/`. `build.rs` also
/// emits each as `cargo:rerun-if-changed`: deleting one with a warm target
/// directory must rerun that script so the file exists before `rustc`
/// compiles the `include_bytes!` sites.
pub const OUTPUT_NAMES: [&str; 10] = [
    "icon-16.png",
    "icon-32.png",
    "icon-48.png",
    "icon-64.png",
    "icon-128.png",
    "icon-256.png",
    "icon-512.png",
    "icon-1024.png",
    "icon.ico",
    "icon.icns",
];

/// Raster ladder written to `assets/icons/icon-<size>.png`.
const SIZES: [u32; 8] = [16, 32, 48, 64, 128, 256, 512, 1024];

/// Bump to force a regeneration after changing THIS generator (the stored
/// stamp also mixes the generator's own source bytes, so an edit to the
/// rasterization code invalidates it automatically; this constant stays as
/// the explicit escape hatch).
const GENERATOR_VERSION: u64 = 1;

/// This file's own bytes, hashed into the stamp: changing `SIZES`, the
/// `.icns` mapping or the encoder with a warm target directory must
/// regenerate the committed outputs, and a manual version bump was a fragile
/// convention (a forgotten bump left every committed icon stale while the
/// build "succeeded").
const GENERATOR_SOURCE: &str = include_str!("gen_icons.rs");

/// Ensures every icon output exists and matches the current `assets/icon.svg`.
///
/// Called by `build.rs` before anything reads the icons. Never panics when
/// a complete (possibly stale) icon set is on disk.
pub fn ensure(manifest_dir: &Path, out_dir: &Path) {
    match ensure_current(manifest_dir, out_dir) {
        Ok(()) => {}
        Err(e) => {
            if all_outputs_present(manifest_dir) {
                println!("cargo:warning=mdowser build: keeping existing icons: {e}");
            } else {
                panic!("mdowser build: cannot generate assets/icons: {e}");
            }
        }
    }
}

/// Regenerates when an output is missing, or when the SVG (or the generator
/// version) changed since the stamp was written.
fn ensure_current(manifest_dir: &Path, out_dir: &Path) -> Result<(), String> {
    let svg = fs::read(manifest_dir.join("assets/icon.svg"))
        .map_err(|e| format!("read assets/icon.svg: {e}"))?;
    let hash = stamp_hash(&svg);
    let stamp = out_dir.join("gen-icons.stamp");
    let current = fs::read_to_string(&stamp)
        .ok()
        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok());
    if !all_outputs_present(manifest_dir) || current != Some(hash) {
        generate(manifest_dir, &svg)?;
        fs::write(&stamp, format!("{hash:016x}")).map_err(|e| format!("write stamp: {e}"))?;
    }
    Ok(())
}

/// FNV-1a over the SVG plus the generator source (and version). Dependency-
/// free: this runs inside the build script for a few KB on every build, and
/// a hashing crate would be a build-dependency for nothing.
fn stamp_hash(svg: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash
    }
    let hash = fnv1a(GENERATOR_SOURCE.as_bytes(), OFFSET);
    let hash = fnv1a(svg, hash);
    hash ^ GENERATOR_VERSION.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// True when every expected file is already on disk.
fn all_outputs_present(manifest_dir: &Path) -> bool {
    let dir = manifest_dir.join("assets/icons");
    OUTPUT_NAMES.iter().all(|name| dir.join(name).is_file())
}

/// Rasterizes the ladder and both container icons, writing every file.
fn generate(manifest_dir: &Path, svg: &[u8]) -> Result<(), String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())
        .map_err(|e| format!("parse assets/icon.svg: {e}"))?;
    let out = manifest_dir.join("assets/icons");
    fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
    // A previous run killed between write and rename can leave a hidden
    // temp: sweep our own `.tmp.<pid>` files before regenerating.
    if let Ok(entries) = fs::read_dir(&out) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') && name.contains(".tmp.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let mut pngs: Vec<(u32, Vec<u8>)> = Vec::with_capacity(SIZES.len());
    for size in SIZES {
        let png = render_png(&tree, size)?;
        write_atomic(&out.join(format!("icon-{size}.png")), &png)?;
        pngs.push((size, png));
    }
    write_atomic(&out.join("icon.ico"), &ico(&pngs))?;
    write_atomic(&out.join("icon.icns"), &icns(&pngs))?;
    Ok(())
}

/// Writes one generated icon ATOMICALLY (temp + rename in the same
/// directory). Two builds with different profiles (a debug/release pair, or
/// `cargo build` racing `cargo clippy`) share the COMMITTED `assets/icons/`
/// tree; a truncate-then-write let one observe the other's half-written PNG.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("icon");
    let tmp = path.with_file_name(format!(".{name}.tmp.{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", path.display()));
    }
    Ok(())
}

/// Rasterizes the SVG at `size` (longest side) to PNG bytes.
fn render_png(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>, String> {
    let src = tree.size();
    let scale = size as f32 / src.width().max(src.height());
    let w = (src.width() * scale).round().max(1.0) as u32;
    let h = (src.height() * scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| format!("raster {w}x{h} refused"))?;
    resvg::render(
        tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| format!("png encode at {size}: {e}"))
}

/// Builds a PNG-based `.ico` (Vista+): 16..256 px, 0 means 256 in the header.
fn ico(pngs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<(u32, &Vec<u8>)> = pngs
        .iter()
        .filter(|(size, _)| *size <= 256)
        .map(|(size, png)| (*size, png))
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (size, png) in &entries {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(dim);
        out.push(dim);
        out.push(0); // palette size
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // color planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &entries {
        out.extend_from_slice(png);
    }
    out
}

/// Builds an `.icns` with PNG payloads (macOS 10.7+), 1x and @2x types.
fn icns(pngs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    // (icns type, source pixel size): icp4..ic10 are the 1x ladder;
    // ic11..ic14 the @2x duplicates (16@2x = 32px, 32@2x = 64px, ...).
    const TYPES: [(&[u8; 4], u32); 11] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic13", 256),
        (b"ic14", 512),
    ];
    let mut chunks: Vec<(&[u8; 4], &Vec<u8>)> = Vec::with_capacity(TYPES.len());
    for (kind, size) in TYPES {
        if let Some((_, png)) = pngs.iter().find(|(s, _)| *s == size) {
            chunks.push((kind, png));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"icns");
    let total = 8 + chunks.iter().map(|(_, data)| 8 + data.len()).sum::<usize>();
    out.extend_from_slice(&(total as u32).to_be_bytes());
    for (kind, png) in chunks {
        out.extend_from_slice(kind);
        out.extend_from_slice(&((8 + png.len()) as u32).to_be_bytes());
        out.extend_from_slice(png);
    }
    out
}
