// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `gen_fonts`: build-time Brotli compression of the two largest embedded
//! fonts (`assets/fonts/NotoColorEmoji-Regular.ttf`,
//! `assets/fonts/NotoSansCJK-Regular.ttc`).
//!
//! Why build-time: `src/assets.rs` embeds the COMPRESSED blobs with
//! `include_bytes!` and decompresses them once at runtime, which saves
//! ~26.5 MB in the installed binary (the two large faces are 44.8 MB raw
//! and compress to 18.3 MB at quality 10). The raw files
//! under `assets/fonts/` stay the source of truth and keep feeding the icon
//! pipeline, tests and packaging untouched.
//!
//! - Regenerates only when NEEDED: a hash of both inputs (plus this
//!   generator's own source) is kept in `OUT_DIR`; a normal build reads the
//!   stamp and stops.
//! - Every blob is ROUND-TRIP VALIDATED (decompress == source) before the
//!   build succeeds, so the runtime decompression in `assets.rs` cannot
//!   fail on a binary this build produced.
//! - Emits `font_meta.rs` with the uncompressed lengths: the runtime
//!   preallocates exactly (no `Vec` growth doubling, so the transient peak
//!   is the final size) and the assets test pins the contract.

use std::fs;
use std::io::Read as _;
use std::path::Path;

/// Inputs as `(path relative to the manifest dir, generated const prefix)`.
pub const INPUTS: [(&str, &str); 2] = [
    (
        "assets/fonts/NotoColorEmoji-Regular.ttf",
        "NOTO_COLOR_EMOJI",
    ),
    ("assets/fonts/NotoSansCJK-Regular.ttc", "NOTO_SANS_CJK"),
];

/// Brotli quality: q10 costs ~90 s for both faces and saves ~1.8 MB over
/// q9; q11 would take ~2.5 minutes for ~0.6 MB more. The result is cached
/// in `OUT_DIR` (stamp), so the one-time cost is paid once per profile AND
/// target triple (`OUT_DIR` is keyed by both: a `make dist` over five
/// targets pays it once per target).
const QUALITY: i32 = 10;
/// Brotli window (`lgwin`): 22 = 4 MiB, the encoder default; measured
/// smaller on both faces than the maximum 16 MiB window at this quality.
const LGWIN: i32 = 22;

/// This file's own bytes, hashed into the stamp: changing the quality, the
/// window or the input list must regenerate the cached blobs.
const GENERATOR_SOURCE: &str = include_str!("gen_fonts.rs");

/// FNV-1a offset basis (the stamp's dependency-free hash).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One input's cheap change probe: `(length, mtime nanos)`.
#[derive(Debug, PartialEq, Eq, Clone)]
struct InputStat {
    name: &'static str,
    size: u64,
    mtime_nanos: u128,
}

/// The parsed stamp file (`gen-fonts.stamp`).
struct Stamp {
    /// FNV-1a over the input BYTES (the authoritative content identity).
    content_hash: u64,
    /// FNV-1a over the generator source (cheap, always current).
    source_hash: u64,
    stats: Vec<InputStat>,
}

impl Stamp {
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let content_hash = u64::from_str_radix(lines.next()?.trim(), 16).ok()?;
        let source_hash = u64::from_str_radix(lines.next()?.trim(), 16).ok()?;
        let mut stats = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let size = parts.next()?.parse().ok()?;
            let mtime_nanos = parts.next()?.parse().ok()?;
            let name = INPUTS.iter().map(|(n, _)| *n).find(|n| *n == name)?;
            stats.push(InputStat {
                name,
                size,
                mtime_nanos,
            });
        }
        Some(Self {
            content_hash,
            source_hash,
            stats,
        })
    }

    fn render(&self) -> String {
        let mut out = format!("{:016x}\n{:016x}\n", self.content_hash, self.source_hash);
        for stat in &self.stats {
            out.push_str(&format!(
                "{} {} {}\n",
                stat.name, stat.size, stat.mtime_nanos
            ));
        }
        out
    }
}

/// Ensures every compressed blob and `font_meta.rs` exist and match the
/// current inputs. Called by `build.rs` before the crate compiles.
pub fn ensure(manifest_dir: &Path, out_dir: &Path) {
    let stamp_path = out_dir.join("gen-fonts.stamp");
    let stats = input_stats(manifest_dir);
    let source_hash = fnv1a(GENERATOR_SOURCE.as_bytes(), FNV_OFFSET);
    let previous = fs::read_to_string(&stamp_path)
        .ok()
        .and_then(|text| Stamp::parse(&text));
    let mut cached_hash = None;
    if let Some(previous) = &previous {
        // FAST PATH: generator + input size/mtime unchanged and every output
        // on disk. The old code hashed 45 MB of font bytes on EVERY build
        // (`stamp_hash` ran before reading the stamp), so a warm build paid
        // that read for nothing.
        if previous.source_hash == source_hash
            && previous.stats == stats
            && all_outputs_present(out_dir)
        {
            return;
        }
        // A statistical change alone (`touch`, an mtime-only restore): the
        // content hash decides, so it never pays a ~90 s recompression for
        // identical bytes.
        let hash = *cached_hash.get_or_insert_with(|| content_hash(manifest_dir));
        if previous.content_hash == hash && all_outputs_present(out_dir) {
            write_stamp(&stamp_path, hash, source_hash, &stats);
            return;
        }
    }
    if let Err(e) = generate(manifest_dir, out_dir) {
        panic!("mdowser build: cannot compress embedded fonts: {e}");
    }
    let hash = cached_hash.unwrap_or_else(|| content_hash(manifest_dir));
    write_stamp(&stamp_path, hash, source_hash, &stats);
}

/// Writes the stamp; a failure is loud (the next build would recompress).
fn write_stamp(stamp: &Path, content_hash: u64, source_hash: u64, stats: &[InputStat]) {
    let rendered = Stamp {
        content_hash,
        source_hash,
        stats: stats.to_vec(),
    }
    .render();
    if let Err(e) = write_atomic(stamp, rendered.as_bytes()) {
        panic!("mdowser build: cannot write gen-fonts.stamp: {e}");
    }
}

/// Writes `bytes` through a sibling temp file and a rename. The stamp's fast
/// path only proves that an output EXISTS, so a plain `fs::write` interrupted
/// by a full disk or a killed build could leave a truncated blob that every
/// later warm build trusted. Renaming a complete temp file makes each output
/// either the old blob or the new one, never a partial write.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Current `(size, mtime)` of every input (missing inputs yield a stat no
/// recorded stamp can match, so generation runs and reports the real error).
fn input_stats(manifest_dir: &Path) -> Vec<InputStat> {
    INPUTS
        .iter()
        .map(|(name, _)| {
            let meta = fs::metadata(manifest_dir.join(name)).ok();
            let size = meta.as_ref().map_or(u64::MAX, |m| m.len());
            let mtime_nanos = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(u128::MAX, |d| d.as_nanos());
            InputStat {
                name,
                size,
                mtime_nanos,
            }
        })
        .collect()
}

/// Output file name inside `OUT_DIR` for one input: the base name plus
/// `.br` (`assets/fonts/NotoSansCJK-Regular.ttc` -> `NotoSansCJK-Regular.ttc.br`).
fn output_name(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    format!("{base}.br")
}

/// True when every expected generated file is already on disk and non-empty.
/// An empty file is never a valid blob (and would decompress to nothing), so
/// it must not satisfy the stamp's fast path.
fn all_outputs_present(out_dir: &Path) -> bool {
    INPUTS.iter().all(|(name, _)| {
        out_dir
            .join(output_name(name))
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() > 0)
    }) && out_dir
        .join("font_meta.rs")
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// FNV-1a over every input's bytes. Dependency-free: build scripts must not
/// pull a hashing crate for this. Only runs when the cheap probes moved.
fn content_hash(manifest_dir: &Path) -> u64 {
    let mut hash = FNV_OFFSET;
    for (name, _) in INPUTS {
        if let Ok(bytes) = fs::read(manifest_dir.join(name)) {
            hash = fnv1a(&bytes, hash);
        }
    }
    hash
}

/// Incremental FNV-1a over `bytes`.
fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compresses every input into `OUT_DIR/<name>.br`, validates each blob by
/// decompressing it back and comparing, and writes `font_meta.rs`.
fn generate(manifest_dir: &Path, out_dir: &Path) -> Result<(), String> {
    let mut meta = String::from(
        "// Generated by build.rs (build_support/gen_fonts.rs). Do not edit.\n\
         // Uncompressed lengths of the Brotli-compressed embedded fonts:\n\
         // the runtime preallocates exactly and asserts the result.\n\n",
    );
    for (name, prefix) in INPUTS {
        let src = fs::read(manifest_dir.join(name)).map_err(|e| format!("read {name}: {e}"))?;
        let compressed = compress(&src)?;
        // Round-trip BEFORE anything is written: a blob this build produced
        // must decompress to the exact source, or the runtime cannot trust it.
        let back = decompress(&compressed)?;
        if back != src {
            return Err(format!(
                "{name}: round-trip mismatch ({} bytes in, {} bytes out)",
                src.len(),
                back.len()
            ));
        }
        write_atomic(&out_dir.join(output_name(name)), &compressed)
            .map_err(|e| format!("write {}: {e}", output_name(name)))?;
        meta.push_str(&format!(
            "/// Uncompressed length of `{name}` (build-generated).\n\
             pub const {prefix}_UNCOMPRESSED: usize = {};\n",
            src.len()
        ));
    }
    write_atomic(&out_dir.join("font_meta.rs"), meta.as_bytes())
        .map_err(|e| format!("write font_meta.rs: {e}"))
}

/// Brotli-compresses one font (generic mode, the encoder defaults, which
/// measured smaller on both faces than the font mode / maximum window
/// combinations).
fn compress(src: &[u8]) -> Result<Vec<u8>, String> {
    use brotli::enc::BrotliEncoderParams;
    let params = BrotliEncoderParams {
        quality: QUALITY,
        lgwin: LGWIN,
        ..Default::default()
    };
    let mut out = Vec::with_capacity(src.len() / 2);
    brotli::BrotliCompress(&mut &src[..], &mut out, &params)
        .map_err(|e| format!("brotli compress: {e}"))?;
    Ok(out)
}

/// Decompresses one blob whole (validation only).
fn decompress(blob: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    brotli::Decompressor::new(blob, 16 * 1024)
        .read_to_end(&mut out)
        .map_err(|e| format!("brotli decompress: {e}"))?;
    Ok(out)
}
