// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Seeded robustness sweep: deterministic pseudo-random documents built from
//! the markup the normalizer, parser and renderer special-case (components,
//! callouts, `:::` containers, tables, fences, autolinks, shortcodes) mixed
//! with multibyte text, run through the whole document pipeline and the
//! renderer. Any panic (a byte slice inside a UTF-8 character, an index past
//! a bound) fails with the offending document. The seeds are fixed so a
//! failure reproduces; `MDOWSER_SWEEP_ITERS` raises the document count for a
//! longer local run.

use super::{doc_id_salt, show_markdown};
use eframe::egui;

/// Building blocks: structural markup plus text that stresses char
/// boundaries (combining marks, ZWJ sequences, RTL, CJK, zero-width).
const PIECES: &[&str] = &[
    "é",
    "日本語",
    "🙂",
    "👩‍👩‍👧",
    "a\u{301}",
    "\u{200b}",
    "\u{202e}",
    "ß",
    "İ",
    "\t",
    "\r\n",
    "\n",
    "\n\n",
    " ",
    "    ",
    "# ",
    "## ",
    "> ",
    "- ",
    "1. ",
    "- [ ] ",
    "- [x] ",
    "```",
    "```rust\n",
    "~~~",
    "`",
    "|",
    "| a | b |\n|---|---|\n",
    ":-:",
    "---\n",
    "***",
    "+++\n",
    "[",
    "]",
    "(",
    ")",
    "![",
    "](",
    "[^1]",
    "[^é]: ",
    "<",
    ">",
    "</",
    "/>",
    "<Tabs>",
    "</Tabs>",
    "<Tab title=\"é\">",
    "</Tab>",
    "<Note>",
    "</Note>",
    "<Card title=\"x\" href=\"/y\">",
    "</Card>",
    "<Steps>",
    "<Step title=\"日\">",
    "</Step>",
    "</Steps>",
    "<Accordion title=\"🙂\">",
    "</Accordion>",
    "<CodeGroup>",
    "</CodeGroup>",
    "<Badge>",
    "</Badge>",
    "<details>",
    "<summary>",
    "</details>",
    "<br>",
    "<!--",
    "-->",
    "<img src=\"é.png\">",
    ":::",
    ":::note",
    ":::tip[é]",
    ":::tabs",
    "::: code-group",
    "!!! note \"日\"",
    "> [!NOTE]",
    "> [!WARNING]é",
    "https://",
    "https://ex.com/é",
    "www.",
    "www.日本.com",
    "example.com",
    "ex.io/a_(b)",
    "file:///tmp/é.md",
    ":smile:",
    ":é:",
    ":+1:",
    "*",
    "**",
    "_",
    "~~",
    "\\",
    "&amp;",
    "&#x1F600;",
    "{#id}",
    "{",
    "}",
    "==\n",
    "word",
    "Home | Docs | About",
    "Inicio · Docs · Más",
    "\"",
    "title:",
    "key: [",
];

/// xorshift64: tiny, deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn document(rng: &mut Rng) -> String {
    let mut doc = String::new();
    if rng.below(6) == 0 {
        doc.push_str("---\ntitle: é\n---\n");
    }
    for _ in 0..=rng.below(120) {
        doc.push_str(PIECES[rng.below(PIECES.len())]);
    }
    doc
}

fn iterations(default: u64) -> u64 {
    std::env::var("MDOWSER_SWEEP_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn document_pipeline_never_panics_on_generated_markup() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let web = url::Url::parse("https://ex.com/a/").unwrap();
    let local = url::Url::parse("file:///tmp/a/doc.md").unwrap();
    for n in 0..iterations(1_500) {
        let doc = document(&mut rng);
        let base = if n % 2 == 0 { &web } else { &local };
        let outcome = std::panic::catch_unwind(|| {
            let _ = crate::protocol::client::finish_document(doc.clone(), base.clone(), base);
            for line in doc.lines() {
                for lang in ["rust", "py", "js", "sh", "json", "toml", "yaml"] {
                    let lang = crate::markdown::highlight::norm_lang(lang);
                    let _ = crate::markdown::highlight::spans(lang, line);
                }
                let _ = crate::markdown::highlight::info_title(line);
                let _ = crate::protocol::rewrite::absolutize(base, line);
                let _ = crate::markdown::emoji::expand_shortcodes(line);
                let _ = crate::markdown::anchors::slugify(line);
            }
        });
        assert!(
            outcome.is_ok(),
            "pipeline panicked on document {n}: {doc:?}"
        );
    }
}

#[test]
fn renderer_never_panics_on_generated_markup() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let ctx = egui::Context::default();
    crate::ui::theme::install_test_fonts(&ctx);
    let base = url::Url::parse("https://ex.com/a/").unwrap();
    for n in 0..iterations(150) {
        let doc = document(&mut rng);
        let Ok(fetched) =
            crate::protocol::client::finish_document(doc.clone(), base.clone(), &base)
        else {
            continue;
        };
        // Narrow, phone-like and desktop columns exercise different wraps.
        let width = [120.0f32, 400.0, 1100.0][(n % 3) as usize];
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 750.0),
            )),
            ..Default::default()
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut images = crate::media::cache::ImageCache::new();
            let mut emoji = crate::media::emoji::EmojiCache::new();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut on_link = |_: &str, _: crate::viewer::LinkActivation| {};
                    let mut links = Vec::new();
                    // Inert: no network from tests (see `render_probe`).
                    show_markdown(
                        ui,
                        &fetched.events,
                        &fetched.anchors,
                        &fetched.dests,
                        &mut None,
                        doc_id_salt(&base),
                        true,
                        false,
                        width,
                        1.0,
                        &mut images,
                        &mut emoji,
                        &mut on_link,
                        &mut links,
                    );
                });
            });
            out.textures_delta.clear();
        }));
        assert!(
            outcome.is_ok(),
            "renderer panicked on document {n}: {doc:?}"
        );
    }
}
