// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::outline`: the document outline panel's view state (reading aid).
//!
//! The outline ITSELF is built once per fetch (`Fetched.outline`, see
//! [`crate::markdown::outline`]); this module owns only the per-tab view
//! state the panel and the scrollspy read:
//!
//! - The panel is a floating overlay (`overlays::outline`: right-docked below
//!   the find bar, a bottom sheet on narrow windows). It NEVER reflows the
//!   document.
//! - SCROLLSPY: while the panel is open, the document walk records each
//!   rendered heading's rect keyed by the heading's own `Start` event index
//!   ([`OutlineState::record_rect`], into the state's reusable scratch
//!   buffer). [`OutlineState::update_current`] then picks the last outline
//!   item whose heading top reached the viewport top. Rects, not offsets: a
//!   heading inside an unselected `<Tabs>` panel (or one dropped by the
//!   block budget) simply renders no rect, so it can never shift another
//!   heading's position.
//! - Row clicks scroll through the SAME mechanism as a `#id` link
//!   (`Tab::pending_anchor`), so the outline needs no scroll code of its
//!   own.

use crate::config;
use crate::markdown::outline::Outline;
use eframe::egui;

/// Distance below the viewport top (px) within which a heading that just
/// scrolled past still counts as the CURRENT section: a heading scrolled
/// exactly to the top (`scroll_to_here`) or a few px above stays lit.
const SPY_PAD: f32 = 12.0;

/// Per-tab outline panel state. Lives on `Tab` (each tab keeps its own
/// section cursor); `clear_overlay` resets it on navigation/tab switch.
#[derive(Debug, Clone, Default)]
pub struct OutlineState {
    /// True while the panel is open (only on `Loaded`/`Licenses`).
    pub open: bool,
    /// Index (into `Fetched.outline.items`) of the section the reader is in.
    /// Updated by the scrollspy after each document walk while the panel is
    /// open; the panel highlights this row.
    pub current: usize,
    /// Bring the current row into view on the next panel frame (just opened,
    /// or the scrollspy moved the cursor). Cleared by the panel once the
    /// offset is applied, so the reader's own scrolling in the panel then
    /// wins.
    pub reveal: bool,
    /// True when ANOTHER surface owns Escape this frame (a focused field,
    /// the page-info card, an open menu/popup, a floating window or the
    /// find bar): the panel must not close on the same press the other
    /// surface acts on. Written by `app::shortcuts` before the viewer draws
    /// (the panel itself owns the close so it stays self-contained for
    /// direct rendering, like the find bar).
    pub esc_elsewhere: bool,
    /// Heading rects collected by this frame's document walk: `(event index
    /// of the heading's `Start`, screen rect it painted)`, in document
    /// order. Cleared by [`Self::begin_frame`]; kept across frames so an
    /// open panel costs no per-frame allocation.
    rects: Vec<(usize, egui::Rect)>,
}

impl OutlineState {
    /// Opens the panel and brings the reader's section into view.
    pub fn show(&mut self) {
        self.open = true;
        self.reveal = true;
    }

    /// Closes the panel (keeps `current`: reopening lands on the same
    /// section).
    pub fn close(&mut self) {
        self.open = false;
        self.reveal = false;
        self.esc_elsewhere = false;
    }

    /// Entry point of the context-menu row and the keyboard shortcut.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.show();
        }
    }

    /// Drops everything (navigation/tab switch: the cursor belongs to the
    /// page that was visible).
    pub fn reset(&mut self) {
        self.open = false;
        self.current = 0;
        self.reveal = false;
        self.esc_elsewhere = false;
        self.rects.clear();
    }

    /// Starts a document frame: drops the previous frame's rects. Callers
    /// must call this even while the panel is closed if a stale frame could
    /// otherwise be read (the viewer clears on every walk).
    pub fn begin_frame(&mut self) {
        self.rects.clear();
    }

    /// Records the rect `rect` a heading painted this frame, keyed by the
    /// index of the heading's own `Start` event. Bounded by
    /// `config::MAX_OUTLINE_ITEMS`: items past the cap have no row, so their
    /// rects would never be read.
    pub fn record_rect(&mut self, event_index: usize, rect: egui::Rect) {
        if self.rects.len() < config::MAX_OUTLINE_ITEMS {
            self.rects.push((event_index, rect));
        }
    }

    /// Recomputes [`Self::current`] from this frame's recorded rects: the
    /// last item whose heading top sits at or above `viewport_top + PAD`.
    ///
    /// Keeps the previous cursor when the walk recorded NO heading (the
    /// document was skipped whole by a `<Tabs>` selection or the block
    /// budget), and selects the first item while the reader is still above
    /// every heading that did render. Sets `reveal` only when the section
    /// actually moved, so the panel is never yanked around mid-scroll.
    pub fn update_current(&mut self, outline: &Outline, viewport_top: f32) {
        let Some(current) = current_section(outline, &self.rects, viewport_top) else {
            return;
        };
        if current != self.current {
            self.current = current;
            self.reveal = true;
        }
    }
}

/// Index of the section the reader is in, or `None` when no heading rendered
/// this frame (the caller keeps its previous cursor).
///
/// `rects` holds one `(event index, rect)` pair per heading the frame
/// actually painted, in document order; `outline.items` is in document order
/// too, so the two run through one merge pass. Items with no rect (skipped
/// content) neither qualify nor end the scan.
pub(crate) fn current_section(
    outline: &Outline,
    rects: &[(usize, egui::Rect)],
    viewport_top: f32,
) -> Option<usize> {
    if rects.is_empty() || outline.is_empty() {
        return None;
    }
    let line = viewport_top + SPY_PAD;
    let mut painted = 0usize;
    let mut current: Option<usize> = None;
    let mut next_rect = 0usize;
    for (index, item) in outline.items.iter().enumerate() {
        while next_rect < rects.len() && rects[next_rect].0 < item.at {
            next_rect += 1;
        }
        let Some(&(at, rect)) = rects.get(next_rect) else {
            break;
        };
        if at != item.at {
            continue;
        }
        painted += 1;
        if rect.top() <= line {
            current = Some(index);
        }
    }
    // No heading passed the line yet: the reader is above the first section,
    // which is the one the panel should point at.
    (painted > 0).then(|| current.unwrap_or(0))
}

/// Scroll offset (px) that brings row `current` into view inside a panel
/// viewport `view_h` tall showing `rows` rows of `row_h` each (no item
/// spacing): the row is centered when it fits, and the offset is clamped to
/// the real scrollable range.
///
/// Computed instead of requested per row: `ScrollArea::show_rows` only runs
/// the VISIBLE rows, so a `scroll_to_rect` on the current row could never
/// reach one that is out of range.
pub(crate) fn reveal_offset(current: usize, rows: usize, view_h: f32, row_h: f32) -> f32 {
    if rows == 0 || row_h <= 0.0 || view_h <= 0.0 {
        return 0.0;
    }
    let content_h = rows as f32 * row_h;
    let max_offset = (content_h - view_h).max(0.0);
    let centered = current as f32 * row_h - (view_h - row_h) * 0.5;
    centered.clamp(0.0, max_offset)
}

#[cfg(test)]
mod tests {
    use super::{current_section, reveal_offset, Outline, OutlineState};
    use crate::markdown::{anchors, document::parse_events, outline};
    use eframe::egui;

    /// Outline of `md` plus one rect per heading, `y` from `tops`.
    fn outline_and_rects(md: &str, tops: &[f32]) -> (Outline, Vec<(usize, egui::Rect)>) {
        let events = parse_events(md).expect("parse");
        let anchors = anchors::build(&events);
        let outline = outline::build(&events, &anchors);
        let rects = outline
            .items
            .iter()
            .zip(tops)
            .map(|(item, y)| {
                (
                    item.at,
                    egui::Rect::from_min_size(egui::pos2(20.0, *y), egui::vec2(300.0, 40.0)),
                )
            })
            .collect();
        (outline, rects)
    }

    #[test]
    fn scrollspy_picks_the_last_heading_above_the_top() {
        // Viewport top at y = 100; pad 12 => the line sits at 112.
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n",
            &[-900.0, -100.0, 300.0],
        );
        // The second heading sits above the line, the third below it.
        assert_eq!(current_section(&outline, &rects, 100.0), Some(1));
        // Scrolled far DOWN: every heading is above the viewport top, so the
        // last one wins (the reader is inside the final section).
        assert_eq!(current_section(&outline, &rects, 1_000.0), Some(2));
        // Scrolled far UP: no heading has reached the line yet, so the first
        // section is the one the reader is heading into.
        assert_eq!(current_section(&outline, &rects, -500.0), Some(0));
        // Close to the first heading: the reader is already inside the
        // second section (it scrolled past the line), the third is ahead.
        assert_eq!(current_section(&outline, &rects, 0.0), Some(1));
        // A heading exactly at the viewport top qualifies (the pad allows for
        // the sub-pixel slack `scroll_to_here` leaves).
        assert_eq!(current_section(&outline, &rects, 300.0), Some(2));
        assert_eq!(current_section(&outline, &rects, 313.0), Some(2));
        // Below the line by itself: the previous heading stays current.
        assert_eq!(current_section(&outline, &rects, 200.0), Some(1));
    }

    /// A skipped heading (an unselected `<Tabs>` panel, the block budget)
    /// renders no rect: the spy still resolves every later heading, and a
    /// frame that painted nothing reports `None` so the caller keeps its
    /// cursor instead of jumping to the first section.
    #[test]
    fn scrollspy_survives_unrendered_headings() {
        let (outline, mut rects) =
            outline_and_rects("# One\n\n## Two\n\n## Three\n", &[-900.0, -200.0, 100.0]);
        rects.remove(1); // `## Two` did not render this frame
        assert_eq!(current_section(&outline, &rects, 200.0), Some(2));
        // Only the first heading rendered, and it is above the line.
        let (outline, all) =
            outline_and_rects("# One\n\n## Two\n\n## Three\n", &[-900.0, -200.0, 100.0]);
        assert_eq!(current_section(&outline, &all[..1], 200.0), Some(0));
        assert_eq!(current_section(&outline, &[], 0.0), None);
    }

    #[test]
    fn state_updates_the_cursor_and_reveals_only_on_a_move() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n",
            &[-900.0, -100.0, 300.0],
        );
        let mut state = OutlineState::default();
        state.show();
        assert!(state.open && state.reveal);
        // Frame 1: the panel just opened on section 2. The reveal flag set by
        // `show` must be consumed by the panel, not re-set by the spy.
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 100.0);
        assert_eq!(state.current, 1);
        assert!(state.reveal, "the cursor moved: the row is revealed");
        // Frame 2: the same place, no move, no reveal.
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 100.0);
        assert_eq!(state.current, 1);
        assert!(!state.reveal);
        // Frame 3: nothing rendered (a whole-document skip): the cursor stays.
        state.begin_frame();
        state.update_current(&outline, 10_000.0);
        assert_eq!(state.current, 1);
        // Frame 4: scrolled far past the last heading (every recorded rect
        // sits above the viewport top now).
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 2_000.0);
        assert_eq!(state.current, 2);
        // Reopening keeps the place; navigation drops everything.
        state.close();
        assert!(!state.open && !state.reveal && state.current == 2);
        state.show();
        state.reset();
        assert!(!state.open && state.current == 0 && !state.reveal);
        // Toggle is the shortcut/menu entry.
        state.toggle();
        assert!(state.open);
        state.toggle();
        assert!(!state.open);
    }

    /// The rect scratch is bounded: a hostile heading list cannot grow it
    /// past the item cap (rects past it could never be read anyway).
    #[test]
    fn recorded_rects_are_capped() {
        let mut state = OutlineState::default();
        state.begin_frame();
        for i in 0..crate::config::MAX_OUTLINE_ITEMS + 10 {
            state.record_rect(
                i * 2,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
            );
        }
        assert_eq!(state.rects.len(), crate::config::MAX_OUTLINE_ITEMS);
        state.begin_frame();
        assert!(state.rects.is_empty());
    }

    #[test]
    fn reveal_offset_centers_the_row_and_clamps() {
        // 100 rows of 20px in a 200px viewport: row 0 stays at the top.
        assert_eq!(reveal_offset(0, 100, 200.0, 20.0), 0.0);
        // Row 50 centers: 50*20 - (200-20)/2 = 910.
        assert_eq!(reveal_offset(50, 100, 200.0, 20.0), 910.0);
        // The last row clamps to the real range: 2000 - 200 = 1800.
        assert_eq!(reveal_offset(99, 100, 200.0, 20.0), 1800.0);
        // A short list has nothing to scroll.
        assert_eq!(reveal_offset(2, 3, 200.0, 20.0), 0.0);
        // Degenerate inputs never produce a NaN/negative offset.
        assert_eq!(reveal_offset(0, 0, 200.0, 20.0), 0.0);
        assert_eq!(reveal_offset(5, 10, 0.0, 20.0), 0.0);
        assert_eq!(reveal_offset(5, 10, 200.0, 0.0), 0.0);
    }
}
