// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::outline`: the document outline sidebar's view state (reading aid).
//!
//! The outline ITSELF is built once per fetch (`Fetched.outline`, see
//! [`crate::markdown::outline`]); this module owns only the per-tab view
//! state the sidebar and the scrollspy read:
//!
//! - The sidebar is a permanent right-hand panel (`outline_panel`) on every
//!   document page. It is part of the layout, narrow, and wraps long titles
//!   instead of eliding them; a page without headings keeps the slot and
//!   shows a short empty state, so navigation never reflows the sheet.
//! - SCROLLSPY: the document walk records each rendered heading's rect keyed
//!   by the heading's own `Start` event index
//!   ([`OutlineState::record_rect`], into the state's reusable scratch
//!   buffer). [`OutlineState::update_current`] then picks the last outline
//!   item whose heading top reached the viewport top. Rects, not offsets: a
//!   heading inside an unselected `<Tabs>` panel (or one dropped by the
//!   block budget) simply renders no rect, so it can never shift another
//!   heading's position. At the page END the cursor points at the on-screen
//!   heading nearest the reading line instead of jumping to the deepest
//!   visible row: a cluster of headings near the foot of the page is thereby
//!   walked row by row while scrolling, and only a heading that truly cannot
//!   be reached keeps the cursor.
//! - Row clicks scroll through the SAME mechanism as a `#id` link
//!   (`Tab::pending_anchor`), so the sidebar needs no scroll code of its own.
//!   A clicked row becomes current immediately and the scrollspy keeps it
//!   pinned while the page settles.

use crate::config;
use crate::markdown::outline::Outline;
use eframe::egui;

/// Distance below the viewport top (px) within which a heading that just
/// scrolled past still counts as the CURRENT section: a heading scrolled
/// exactly to the top (`scroll_to_here`) or a few px above stays lit.
const SPY_PAD: f32 = 12.0;

/// Per-tab outline sidebar state. Lives on `Tab` (each tab keeps its own
/// section cursor); `clear_overlay` resets it on navigation/tab switch.
#[derive(Debug, Clone, Default)]
pub struct OutlineState {
    /// Index (into `Fetched.outline.items`) of the section the reader is in.
    /// Updated by the scrollspy after each document walk; the sidebar
    /// highlights this row.
    pub current: usize,
    /// Bring the current row into view on the next sidebar frame (the
    /// scrollspy moved the cursor, or a row was clicked). Cleared by the
    /// panel once the offset is applied, so the reader's own scrolling in
    /// the panel then wins.
    pub reveal: bool,
    /// A row the reader just clicked: the scrollspy keeps it current while
    /// the requested scroll settles (including a page-end heading that can
    /// never reach the top). Cleared once the heading reaches the spy line
    /// or a manual scroll takes over.
    pinned: Option<usize>,
    /// Heading rects collected by this frame's document walk: `(event index
    /// of the heading's `Start`, screen rect it painted)`, in document
    /// order. Cleared by [`Self::begin_frame`]; kept across frames so an
    /// open sidebar costs no per-frame allocation.
    rects: Vec<(usize, egui::Rect)>,
    /// Measured heights of the wrapped sidebar rows, in item order. Cached
    /// because the panel is always on screen: remeasuring every title every
    /// frame would be pure churn. Keyed by [`Self::row_heights_key`].
    pub(crate) row_heights: Vec<f32>,
    /// `(panel content width bits, document generation, row count)` the
    /// heights above were measured for.
    pub(crate) row_heights_key: Option<(u32, u64, usize)>,
    /// Screen rect of the sidebar this frame (`None` on pages without it),
    /// so the find bar can dock just left of it instead of over it.
    pub(crate) panel_rect: Option<egui::Rect>,
}

impl OutlineState {
    /// A row was clicked: make it current at once and keep it pinned while
    /// the requested scroll settles.
    pub(crate) fn select(&mut self, index: usize) {
        self.current = index;
        self.pinned = Some(index);
        self.reveal = true;
    }

    /// Drops a click pin: a manual scroll (the reader is no longer heading
    /// to the row they clicked) returns the cursor to the scrollspy.
    pub(crate) fn unpin(&mut self) {
        self.pinned = None;
    }

    /// Drops everything (navigation/tab switch: the cursor belongs to the
    /// page that was visible).
    pub fn reset(&mut self) {
        self.current = 0;
        // The list keeps its egui scroll offset per tab; reveal the first
        // section when the next document draws so a long outline left
        // scrolled on the previous page does not open mid-list.
        self.reveal = true;
        self.pinned = None;
        self.rects.clear();
        self.row_heights.clear();
        self.row_heights_key = None;
        self.panel_rect = None;
    }

    /// Starts a document frame: drops the previous frame's rects. Callers
    /// must call this even while no sidebar is shown if a stale frame could
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
    /// `at_bottom` says the sheet has reached its maximum offset: the
    /// on-screen heading nearest the reading line then wins instead of the
    /// deepest visible one, so a dense cluster of headings at the page end is
    /// not skipped in one jump (a heading that truly cannot reach the line
    /// still has a chance when it is the one on screen).
    ///
    /// Keeps the previous cursor when the walk recorded NO heading (the
    /// document was skipped whole by a `<Tabs>` selection or the block
    /// budget), and selects the first item while the reader is still above
    /// every heading that did render. Sets `reveal` only when the section
    /// actually moved, so the sidebar is never yanked around mid-scroll.
    pub fn update_current(
        &mut self,
        outline: &Outline,
        viewport_top: f32,
        viewport_bottom: f32,
        at_bottom: bool,
    ) {
        let Some(spied) = current_section(
            outline,
            &self.rects,
            viewport_top,
            viewport_bottom,
            at_bottom,
        ) else {
            return;
        };
        // A just-clicked row stays selected while its scroll is still in
        // flight. Once the spy agrees, the pin has done its job. At the end
        // of the page the pin keeps covering headings that can never reach
        // the top, which is exactly the case it exists for.
        if let Some(pinned) = self.pinned {
            if heading_rect(outline, &self.rects, pinned).is_none() || pinned == spied {
                // The heading did not render this frame (skipped content) or
                // already reached the spy line: the normal cursor takes over.
                self.pinned = None;
            } else {
                if self.current != pinned {
                    self.current = pinned;
                    self.reveal = true;
                }
                return;
            }
        }
        if spied != self.current {
            self.current = spied;
            self.reveal = true;
        }
    }
}

/// Rect of the heading at `index`, if it rendered this frame.
fn heading_rect(
    outline: &Outline,
    rects: &[(usize, egui::Rect)],
    index: usize,
) -> Option<egui::Rect> {
    let item = outline.items.get(index)?;
    rects
        .iter()
        .find_map(|(at, rect)| (*at == item.at).then_some(*rect))
}

/// Index of the section the reader is in, or `None` when no heading rendered
/// this frame (the caller keeps its previous cursor).
///
/// `rects` holds one `(event index, rect)` pair per heading the frame
/// actually painted, in document order; `outline.items` is in document order
/// too, so the two run through one merge pass. Items with no rect (skipped
/// content) neither qualify nor end the scan.
///
/// While the sheet can still scroll, the last heading above the reading line
/// wins. At the page END the sheet cannot put every heading on that line, so
/// the on-screen heading closest to it wins: in a cluster of headings near
/// the foot of the page the cursor then visits the rows one by one instead of
/// leaping straight to the deepest visible one (which made the intermediate
/// rows impossible to select by scrolling).
pub(crate) fn current_section(
    outline: &Outline,
    rects: &[(usize, egui::Rect)],
    viewport_top: f32,
    viewport_bottom: f32,
    at_bottom: bool,
) -> Option<usize> {
    if rects.is_empty() || outline.is_empty() {
        return None;
    }
    let line = viewport_top + SPY_PAD;
    let visible_bottom = viewport_bottom + SPY_PAD;
    let mut painted = 0usize;
    let mut current: Option<usize> = None;
    let mut nearest: Option<(usize, f32)> = None;
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
        let on_screen = rect.bottom() > viewport_top && rect.top() <= visible_bottom;
        if on_screen {
            let distance = (rect.top() - line).abs();
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((index, distance));
            }
        }
        if rect.top() <= line {
            current = Some(index);
        }
    }
    if at_bottom {
        // The sheet cannot scroll further. The deepest visible heading is
        // NOT the answer: with many headings together that jumped over every
        // intermediate row at once. Stay on the row nearest the reading line
        // instead; when no heading is on screen (a long tail after the last
        // one) keep the last heading that did cross the line.
        return nearest.map(|(index, _)| index).or(current);
    }
    // No heading passed the line yet: the reader is above the first section,
    // which is the one the sidebar should point at.
    (painted > 0).then(|| current.unwrap_or(0))
}

/// Scroll offset (px) that brings row `current` into view inside a sidebar
/// viewport `view_h` tall. Row heights are the panel's measured wrapped
/// heights (variable: a long title takes two or more lines). The row is
/// centered when it fits, and the offset is clamped to the real scrollable
/// range.
///
/// Computed instead of requested per row: the list renders only the visible
/// rows, so an off-screen row has no rect to scroll to.
pub(crate) fn reveal_offset(current: usize, heights: &[f32], view_h: f32) -> f32 {
    if heights.is_empty() || view_h <= 0.0 {
        return 0.0;
    }
    let index = current.min(heights.len() - 1);
    let top: f32 = heights[..index].iter().sum();
    let row_h = heights[index];
    let content_h: f32 = heights.iter().sum();
    let max_offset = (content_h - view_h).max(0.0);
    let centered = top - (view_h - row_h) * 0.5;
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
        assert_eq!(
            current_section(&outline, &rects, 100.0, 700.0, false),
            Some(1)
        );
        // Scrolled far DOWN: every heading is above the viewport top, so the
        // last one wins (the reader is inside the final section).
        assert_eq!(
            current_section(&outline, &rects, 1_000.0, 1_600.0, false),
            Some(2)
        );
        // Scrolled far UP: no heading has reached the line yet, so the first
        // section is the one the reader is heading into.
        assert_eq!(
            current_section(&outline, &rects, -500.0, 100.0, false),
            Some(0)
        );
        // Close to the first heading: the reader is already inside the
        // second section (it scrolled past the line), the third is ahead.
        assert_eq!(
            current_section(&outline, &rects, 0.0, 600.0, false),
            Some(1)
        );
        // A heading exactly at the viewport top qualifies (the pad allows for
        // the sub-pixel slack `scroll_to_here` leaves).
        assert_eq!(
            current_section(&outline, &rects, 300.0, 900.0, false),
            Some(2)
        );
        assert_eq!(
            current_section(&outline, &rects, 313.0, 913.0, false),
            Some(2)
        );
        // Below the line by itself: the previous heading stays current.
        assert_eq!(
            current_section(&outline, &rects, 200.0, 800.0, false),
            Some(1)
        );
    }

    /// At the END of the page a heading that never reached the viewport top
    /// must still become current: a heading in the page tail that is the one
    /// on screen is the section the reader can see.
    #[test]
    fn scrollspy_selects_a_visible_heading_at_the_page_end() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nTail\n",
            &[-2_000.0, -900.0, 300.0],
        );
        // Not at the bottom: the third heading is visible but still ahead, so
        // the middle one remains the reading section.
        assert_eq!(
            current_section(&outline, &rects, -500.0, 300.0, false),
            Some(1)
        );
        // At the bottom the same rects pick the only heading on screen, even
        // though its top never crossed the spy line.
        assert_eq!(
            current_section(&outline, &rects, -500.0, 300.0, true),
            Some(2)
        );
        // A skipped last heading (no rect) never wins: the last RENDERED
        // heading that crossed the line does.
        assert_eq!(
            current_section(&outline, &rects[..2], -500.0, 300.0, true),
            Some(1)
        );
    }

    /// Many headings together at the page end used to make the cursor leap
    /// straight to the deepest visible row, so the intermediate rows could
    /// never be selected by scrolling (the sheet cannot move further at the
    /// bottom). It must stay on the row nearest the reading line instead.
    #[test]
    fn scrollspy_does_not_skip_a_heading_cluster_at_the_page_end() {
        let md = "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nBody\n\n## Four\n\nBody\n\n## Five\n\nTail\n";
        let (outline, rects) = outline_and_rects(md, &[-4.0, 40.0, 84.0, 128.0, 172.0]);
        // The first heading sits on the line and is on screen: the deepest
        // visible row (the fifth) must NOT win.
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, true), Some(0));
        // Scrolled a little further, the first heading has left the viewport:
        // the next row (40) is closest to the line (12), still not the last.
        let (outline, rects) = outline_and_rects(md, &[-60.0, 20.0, 60.0, 100.0, 140.0]);
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, true), Some(1));
        // Halfway through the cluster, the row approaching the line takes the
        // cursor: scrolling through the group walks the intermediate rows.
        let (outline, rects) = outline_and_rects(md, &[-260.0, -180.0, -100.0, -20.0, 60.0]);
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, true), Some(3));
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
        assert_eq!(
            current_section(&outline, &rects, 200.0, 800.0, false),
            Some(2)
        );
        // Only the first heading rendered, and it is above the line.
        let (outline, all) =
            outline_and_rects("# One\n\n## Two\n\n## Three\n", &[-900.0, -200.0, 100.0]);
        assert_eq!(
            current_section(&outline, &all[..1], 200.0, 800.0, false),
            Some(0)
        );
        assert_eq!(current_section(&outline, &[], 0.0, 600.0, false), None);
        assert_eq!(current_section(&outline, &[], 0.0, 600.0, true), None);
    }

    #[test]
    fn state_updates_the_cursor_and_reveals_only_on_a_move() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n",
            &[-900.0, -100.0, 300.0],
        );
        let mut state = OutlineState::default();
        assert!(!state.reveal);
        // Frame 1: the panel just opened on section 2. The spy moves the
        // cursor and requests the reveal.
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 100.0, 800.0, false);
        assert_eq!(state.current, 1);
        assert!(state.reveal, "the cursor moved: the row is revealed");
        // Frame 2: the same place, no move, no reveal.
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 100.0, 800.0, false);
        assert_eq!(state.current, 1);
        assert!(!state.reveal);
        // Frame 3: nothing rendered (a whole-document skip): the cursor stays.
        state.begin_frame();
        state.update_current(&outline, 10_000.0, 10_600.0, false);
        assert_eq!(state.current, 1);
        // Frame 4: scrolled far past the last heading (every recorded rect
        // sits above the viewport top now).
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 2_000.0, 2_600.0, false);
        assert_eq!(state.current, 2);
        // Navigation drops everything; the first section is revealed again
        // even if the panel's egui scroll offset belonged to the old page.
        state.reset();
        assert_eq!(state.current, 0);
        assert!(state.reveal);
        assert!(state.row_heights.is_empty());
        assert!(state.panel_rect.is_none());
    }

    /// A row click survives the frames the requested scroll takes: the
    /// heading at the page END never reaches the top, but the row stays
    /// current instead of falling back to an earlier section.
    #[test]
    fn a_clicked_row_stays_current_at_the_page_end() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nTail\n",
            &[-2_000.0, -900.0, 300.0],
        );
        let mut state = OutlineState::default();
        state.select(2);
        assert_eq!((state.current, state.reveal), (2, true));
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, -500.0, 300.0, true);
        assert_eq!(state.current, 2, "the clicked last row stays current");
        assert!(!state.reveal, "at rest, no repeated reveal");
        // A heading that CAN reach the top clears the pin once the spy
        // agrees with it.
        state.select(1);
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, -50.0, 750.0, false);
        assert_eq!(state.current, 1);
        assert!(state.pinned.is_none(), "the spy landed on the target");
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
        let uniform: Vec<f32> = vec![20.0; 100];
        // 100 rows of 20px in a 200px viewport: row 0 stays at the top.
        assert_eq!(reveal_offset(0, &uniform, 200.0), 0.0);
        // Row 50 centers: 50*20 - (200-20)/2 = 910.
        assert_eq!(reveal_offset(50, &uniform, 200.0), 910.0);
        // The last row clamps to the real range: 2000 - 200 = 1800.
        assert_eq!(reveal_offset(99, &uniform, 200.0), 1800.0);
        // A short list has nothing to scroll.
        assert_eq!(reveal_offset(2, &uniform[..3], 200.0), 0.0);
        // Variable heights (a wrapped title) use the measured row tops.
        let heights = [30.0, 54.0, 30.0, 42.0];
        assert_eq!(reveal_offset(1, &heights, 100.0), 7.0);
        // Degenerate inputs never produce a NaN/negative offset.
        assert_eq!(reveal_offset(0, &[], 200.0), 0.0);
        assert_eq!(reveal_offset(5, &uniform, 0.0), 0.0);
    }
}
