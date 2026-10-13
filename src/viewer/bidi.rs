// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::bidi`: display order for right-to-left text.
//!
//! epaint lays glyphs left-to-right and applies no Unicode bidi algorithm;
//! harfrust does reverse the glyphs inside one right-to-left font run, but
//! epaint splits font runs at face changes (spaces and digits belong to the
//! primary Noto Sans face), so the word order of an RTL run would stay
//! logical. This module supplies the missing run ordering:
//!
//! - [`is_rtl`] reports the paragraph base direction (Unicode P2/P3 on the
//!   first strong character), so callers can right-align the flow.
//! - [`visual_text`] returns the text in visual order, then reverses each
//!   run of RTL-script characters back to logical order: the shaper
//!   (harfrust) reverses those runs again, so the painted result is the
//!   visual order, including word order and mirrored punctuation. The
//!   whole-document copy path (`plain_text`, "Select all"/context menu)
//!   stays logical; a native DRAG selection copies the displayed string,
//!   so a reordered line comes back in visual order (known approximation).
//!
//! The approximation is display-layer only: content split across inline
//! elements (links, code spans) is reordered per fragment, not across the
//! whole paragraph.

use std::borrow::Cow;
use unicode_bidi::BidiInfo;

/// True when `text` contains any character of a right-to-left script.
pub(crate) fn contains_rtl(text: &str) -> bool {
    // No RTL script encodes below U+0590, so every UTF-8 lead byte of one is
    // >= 0xD6: this cheap byte scan skips the char decode for ASCII/Latin
    // text (runs per fragment per frame). It may pass some non-RTL
    // multi-byte chars; the char scan below is the precise check.
    if !text.bytes().any(|b| b >= 0xD6) {
        return false;
    }
    text.chars().any(is_rtl_script_char)
}

/// Right-to-left script ranges (Hebrew, Arabic and relatives, Thaana,
/// N'Ko, Samaritan, Mandaic, Syriac, Adlam, historic RTL blocks).
pub(crate) fn is_rtl_script_char(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x05FF // Hebrew
            | 0x0600..=0x06FF // Arabic
            | 0x0700..=0x074F // Syriac
            | 0x0750..=0x077F // Arabic Supplement
            | 0x0780..=0x07BF // Thaana
            | 0x07C0..=0x07FF // NKo
            | 0x0800..=0x083F // Samaritan
            | 0x0840..=0x085F // Mandaic
            | 0x0860..=0x086F // Syriac Supplement
            | 0x08A0..=0x08FF // Arabic Extended-A
            | 0xFB1D..=0xFB4F // Hebrew presentation forms
            | 0xFB50..=0xFDFF // Arabic presentation forms A
            | 0xFE70..=0xFEFF // Arabic presentation forms B
            | 0x10800..=0x10FFF // Cypriot, Phoenician, Kharoshthi, ...
            | 0x1E800..=0x1EFFF // Mende Kikakui, Adlam, ...
    )
}

/// Paragraph base direction: true when the first strong character is
/// right-to-left (rules P2/P3 of the Unicode bidi algorithm).
pub(crate) fn is_rtl(text: &str) -> bool {
    if !contains_rtl(text) {
        return false;
    }
    BidiInfo::new(text, None)
        .paragraphs
        .first()
        .is_some_and(|p| p.level.is_rtl())
}

/// Text prepared for epaint's left-to-right glyph placement.
///
/// The full character-level visual order is computed first (UBA L2 + L4:
/// mirrored punctuation included). Then each maximal run of RTL-script
/// characters is reversed back to logical order: epaint hands those runs
/// to harfrust, which reverses them again, so the painted result is the
/// visual order. Runs the shaper leaves alone (spaces, digits, Latin,
/// punctuation) keep their visual positions.
///
/// Returns borrowed text when nothing has to change (all-LTR text, or a
/// single RTL word).
pub(crate) fn visual_text(text: &str) -> Cow<'_, str> {
    if !contains_rtl(text) {
        return Cow::Borrowed(text);
    }
    let Some(visual) = uba_visual(text) else {
        return Cow::Borrowed(text);
    };
    let out = un_reverse_rtl_segments(&visual);
    if out == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(out)
    }
}

/// Full character-level visual order (UBA L2 + L4): visual runs in order,
/// odd (RTL) runs reversed with paired punctuation mirrored.
///
/// Every paragraph is reordered, not just the first: a flattened hostile
/// subtree (`render_block_flat`) joins blocks with `\n` into one fragment,
/// and `visual_runs` only covers the paragraph passed to it. Dropping the
/// rest used to silently erase every line after the first.
fn uba_visual(text: &str) -> Option<String> {
    let info = BidiInfo::new(text, None);
    if info.paragraphs.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    // Paragraph ranges include their separator (UBA P1), so the `\n`s are
    // preserved by the concatenation.
    for para in &info.paragraphs {
        let (levels, runs) = info.visual_runs(para, para.range.clone());
        for run in runs {
            let Some(slice) = text.get(run.clone()) else {
                continue;
            };
            if levels.get(run.start).is_some_and(|l| l.is_rtl()) {
                for c in slice.chars().rev() {
                    out.push(mirror(c));
                }
            } else {
                out.push_str(slice);
            }
        }
    }
    Some(out)
}

/// Reverses each maximal run of RTL-script characters back to logical
/// order. The shaper (harfrust, through epaint's per-face font runs)
/// reverses exactly those runs again, so the final painting matches the
/// visual order. Segments it never reverses (spaces, punctuation, digits
/// and Latin picked from the primary face) keep their visual positions.
fn un_reverse_rtl_segments(visual: &str) -> String {
    let mut out = String::with_capacity(visual.len());
    let mut segment = String::new();
    for c in visual.chars() {
        if is_rtl_script_char(c) {
            segment.push(c);
        } else {
            if !segment.is_empty() {
                out.extend(segment.chars().rev());
                segment.clear();
            }
            out.push(c);
        }
    }
    if !segment.is_empty() {
        out.extend(segment.chars().rev());
    }
    out
}

/// Mirrors paired punctuation for RTL runs (rule L4). Common pairs only;
/// the full BidiMirroring table is larger than this app needs.
fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        '‘' => '’',
        '’' => '‘',
        '“' => '”',
        '”' => '“',
        '（' => '）',
        '）' => '（',
        '【' => '】',
        '】' => '【',
        '《' => '》',
        '》' => '《',
        '「' => '」',
        '」' => '「',
        '≤' => '≥',
        '≥' => '≤',
        '≪' => '≫',
        '≫' => '≪',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_rtl, mirror, visual_text};
    use unicode_bidi::BidiInfo;

    /// Ground truth: the full character-level visual order (UBA L2 + L4),
    /// which is what a correct bidi engine would paint. Every paragraph
    /// (not just the first) so multi-line text is covered.
    fn uba_visual(text: &str) -> String {
        let info = BidiInfo::new(text, None);
        let mut out = String::new();
        for para in &info.paragraphs {
            let (levels, runs) = info.visual_runs(para, para.range.clone());
            for run in runs {
                let slice = &text[run.clone()];
                if levels[run.start].is_rtl() {
                    for c in slice.chars().rev() {
                        out.push(mirror(c));
                    }
                } else {
                    out.push_str(slice);
                }
            }
        }
        out
    }

    /// Simulates epaint + harfrust on the prepared display string: text is
    /// segmented into font runs (RTL script vs everything else, matching
    /// how the bundled faces split words from spaces/digits/Latin), each
    /// RTL font run is shaped with harfrust (glyphs reversed), and glyphs
    /// are painted left-to-right.
    fn simulated_screen(text: &str) -> String {
        let display = visual_text(text);
        let mut out = String::new();
        let mut run = String::new();
        let mut run_rtl = None;
        for c in display.chars() {
            let rtl = super::is_rtl_script_char(c);
            if run_rtl.is_some_and(|r| r != rtl) {
                flush_run(&mut out, &run, run_rtl == Some(true));
                run.clear();
            }
            run_rtl = Some(rtl);
            run.push(c);
        }
        flush_run(&mut out, &run, run_rtl == Some(true));
        out
    }

    fn flush_run(out: &mut String, run: &str, rtl: bool) {
        if rtl {
            out.extend(run.chars().rev());
        } else {
            out.push_str(run);
        }
    }

    const CORPUS: &[&str] = &[
        "سلام",
        "مرحبا بالعالم",
        "Hola سلام mundo",
        "سلام Hola",
        "سنة 2026 سنة",
        "שלום עולם",
        "abc שלום def",
        "שלום (טוב)",
        "مرحبا، بالعالم!",
        "العربية English mixed نص",
        "مرحبا 123 abc",
        "مرحبا بالعالم، كيف حالك؟",
        "שלום (טוב) עולם",
    ];

    /// The prepared text must paint exactly the UBA visual order through
    /// epaint's left-to-right glyph placement.
    #[test]
    fn prepared_text_matches_uba_visual_order() {
        for text in CORPUS {
            assert_eq!(
                simulated_screen(text),
                uba_visual(text),
                "visual order mismatch for {text:?} (prepared: {:?})",
                visual_text(text)
            );
        }
    }

    #[test]
    fn detects_base_direction() {
        assert!(is_rtl("مرحبا بالعالم"));
        assert!(is_rtl("שלום"));
        assert!(is_rtl("123 مرحبا"));
        assert!(!is_rtl("Hola mundo"));
        assert!(!is_rtl("Hola سلام")); // first strong char is Latin
        assert!(!is_rtl("123 ()"));
        assert!(!is_rtl(""));
    }

    #[test]
    fn ltr_text_is_borrowed_untouched() {
        for text in ["Hola mundo", "sin acentos 123 ()"] {
            assert!(matches!(visual_text(text), std::borrow::Cow::Borrowed(t) if t == text));
        }
    }

    #[test]
    fn single_rtl_word_is_borrowed() {
        let text = "سلام";
        assert!(matches!(visual_text(text), std::borrow::Cow::Borrowed(t) if t == text));
    }

    #[test]
    fn multi_word_rtl_reverses_word_order_only() {
        // Words swapped, characters inside each word still logical (the
        // shaper reverses those): epaint must receive "بالعالم مرحبا".
        assert_eq!(visual_text("مرحبا بالعالم"), "بالعالم مرحبا");
        // Single spaces stay single spaces.
        assert_eq!(visual_text("a b c").as_ref(), "a b c"); // all-LTR: borrowed
        assert_eq!(visual_text("שלום עולם"), "עולם שלום");
    }

    /// A flattened hostile subtree joins blocks with `\n` into one fragment:
    /// the reorder must cover every paragraph. Only the first used to be
    /// rewritten, silently deleting everything after the newline.
    #[test]
    fn every_paragraph_is_reordered_not_only_the_first() {
        let cases = [
            "hello\nمرحبا بالعالم",
            "مرحبا بالعالم\nhello",
            "one\ntwo مرحبا\nthree שלום עולם",
            "عربي\n\nعربي",
        ];
        for text in cases {
            let display = visual_text(text).into_owned();
            assert_eq!(
                display.chars().count(),
                text.chars().count(),
                "no content may be lost for {text:?} (got {display:?})"
            );
            assert_eq!(
                simulated_screen(text),
                uba_visual(text),
                "visual order for {text:?}"
            );
        }
        assert_eq!(visual_text("hello\nمرحبا بالعالم"), "hello\nبالعالم مرحبا");
    }

    #[test]
    fn mirrors_paired_punctuation_in_rtl_runs() {
        // Visual order: the RTL run is reversed with mirrored parens, then
        // the Hebrew letters are handed back logical for the shaper.
        assert_eq!(visual_text("שלום (טוב)"), "(טוב) שלום");
        assert_eq!(mirror('('), ')');
        assert_eq!(mirror(')'), '(');
        assert_eq!(mirror('x'), 'x');
    }

    #[test]
    fn embedded_rtl_in_ltr_paragraph_keeps_run_position() {
        // LTR base: the Arabic run stays between the Latin words.
        let display = visual_text("Hola سلام mundo");
        assert_eq!(display, "Hola سلام mundo");
    }

    /// End-to-end through the real font stack: epaint paints the prepared
    /// display text so that the glyph `chr` sequence (one glyph per
    /// character here) equals the UBA visual order.
    #[test]
    fn painted_glyph_order_matches_uba() {
        use eframe::egui;

        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 100.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            root.ctx().fonts_mut(|f| {
                for text in CORPUS {
                    if !super::contains_rtl(text) {
                        continue;
                    }
                    let display = visual_text(text).into_owned();
                    let galley = f.layout_no_wrap(
                        display.clone(),
                        egui::FontId::proportional(18.0),
                        egui::Color32::WHITE,
                    );
                    // Only glyphs that advance the pen are painted text:
                    // epaint leaves zero-advance continuation glyphs in
                    // RTL runs (an upstream quirk) whose bitmaps are empty
                    // or duplicated.
                    let painted: String = galley.rows[0]
                        .glyphs
                        .iter()
                        .filter(|g| g.advance_width > 0.0)
                        .map(|g| g.chr)
                        .collect();
                    assert_eq!(
                        painted,
                        uba_visual(text),
                        "painted glyph order for {text:?}"
                    );
                }
            });
        });
        out.textures_delta.clear();
    }
}
