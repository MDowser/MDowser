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
//!   item whose heading top reached the reading line. Rects, not offsets: a
//!   heading inside an unselected `<Tabs>` panel (or one dropped by the
//!   block budget) simply renders no rect, so it can never shift another
//!   heading's position. During the page's LAST screenful the line descends
//!   from the viewport top to its bottom as the sheet approaches its maximum
//!   offset, so headings that can never reach the top are still selected in
//!   order (and in reverse while scrolling back up), and the deepest visible
//!   section is current at the very end.
//! - Row clicks scroll through the SAME mechanism as a `#id` link
//!   (`Tab::pending_anchor`), so the sidebar needs no scroll code of its own.
//!   A clicked row becomes current immediately and the scrollspy keeps it
//!   pinned while the page settles; the pin is released only once that scroll
//!   has stopped at its target (the top, or the page end) and the spy agrees,
//!   so a page-end click is not overtaken by a deeper section after the sheet
//!   reaches the bottom.

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
    /// never reach the top). Cleared once that scroll has settled at its
    /// target (the heading at the top, or the sheet at its maximum offset)
    /// and the spy agrees, or when a manual scroll takes over.
    pinned: Option<usize>,
    /// Heading rects collected by this frame's document walk: `(event index
    /// of the heading's `Start`, screen rect it painted)`, in document
    /// order. Cleared by [`Self::begin_frame`]; kept across frames so an
    /// open sidebar costs no per-frame allocation.
    rects: Vec<(usize, egui::Rect)>,
    /// The sheet's scroll offset last frame, used by [`Self::sheet_settled`]
    /// to keep a clicked row pinned until the requested scroll has stopped.
    last_scroll_offset: Option<f32>,
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

    /// Records this frame's sheet scroll offset and reports whether the sheet
    /// has come to rest: the offset has not moved since the previous frame
    /// and there is no kinetic velocity left.
    ///
    /// A click pin may only be released once this is `true`. A page-end row
    /// that cannot reach the top is crossed by the moving reading line while
    /// the click's smooth scroll is still running; clearing the pin at that
    /// moment would hand the cursor to a deeper section by the time the sheet
    /// stops. Waiting for the settled frame lets the pin outlive the scroll,
    /// so the clicked row stays selected at the bottom.
    pub(crate) fn sheet_settled(&mut self, offset: f32, velocity: f32) -> bool {
        let settled = self
            .last_scroll_offset
            .is_some_and(|previous| (offset - previous).abs() <= 0.5)
            && velocity.abs() <= 0.5;
        self.last_scroll_offset = Some(offset);
        settled
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
        self.last_scroll_offset = None;
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
    /// last item whose heading top sits at or above the reading line.
    ///
    /// `end_progress` is how far the sheet has travelled through its LAST
    /// screenful (0 at the start of that stretch, 1 at the maximum offset).
    /// It lowers the reading line from the viewport top to its bottom, so a
    /// heading that can never reach the top still becomes current as the
    /// reader scrolls the page end into view, and the deepest visible section
    /// wins at the very bottom. The line walks back up with the sheet, so the
    /// tail headings are selected in reverse on the way up too.
    ///
    /// Keeps the previous cursor when the walk recorded NO heading (a
    /// manually unselected `<Tabs>` panel, a closed accordion or the block
    /// budget skipped them all), and selects the first heading that DID
    /// render while the reader is still above every painted one.
    /// [`Self::current_is_rendered`] tells the panel whether that cursor has
    /// a row to light this frame. Sets `reveal` only when the section
    /// actually moved, so the sidebar is never yanked around mid-scroll.
    ///
    /// `sheet_settled` reports whether the sheet's scroll came to rest
    /// ([`Self::sheet_settled`]). A click pin is only released on a settled
    /// frame AND after the clicked heading reached its target (its top at the
    /// viewport top, or the sheet at its maximum offset). While the requested
    /// scroll is still running (or has not started yet), the reading line may
    /// cross the clicked row and keep going, so the pin must not hand the
    /// cursor over until the scroll has finished where it was going.
    pub fn update_current(
        &mut self,
        outline: &Outline,
        viewport_top: f32,
        viewport_bottom: f32,
        end_progress: f32,
        sheet_settled: bool,
    ) {
        let Some(spied) = current_section(
            outline,
            &self.rects,
            viewport_top,
            viewport_bottom,
            end_progress,
        ) else {
            return;
        };
        // A just-clicked row stays selected while its scroll is still in
        // flight. Once the sheet has stopped AND the spy agrees, the pin has
        // done its job ONLY if the row actually reached its scroll target:
        // its top is at the viewport top, or the sheet is at its maximum
        // offset. A page-end row the line moved past never agrees at rest, so
        // it stays current until a manual scroll takes over; a row already
        // current when clicked is not released before its own scroll starts.
        if let Some(pinned) = self.pinned {
            let done = match heading_rect(outline, &self.rects, pinned) {
                None => true, // did not render this frame (skipped content)
                Some(rect) => {
                    let at_top = rect.top() <= viewport_top + SPY_PAD;
                    let at_end = end_progress >= 1.0;
                    sheet_settled && pinned == spied && (at_top || at_end)
                }
            };
            if done {
                // The requested scroll is complete and the normal cursor can
                // take over (or the heading is gone).
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

    /// Whether [`Self::current`] has a heading rect from the last document
    /// walk. The panel draws BEFORE the sheet, so these are the rects that
    /// produced the current cursor. A page whose headings are all hidden (a
    /// manually unselected `<Tabs>` panel, a closed accordion) keeps the
    /// cursor internally but must not light a row that is not on the page.
    pub(crate) fn current_is_rendered(&self, outline: &Outline) -> bool {
        heading_rect(outline, &self.rects, self.current).is_some()
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
/// content) neither qualify nor end the scan, and can never become current:
/// a heading hidden in a closed section has no reading position to point at.
///
/// While the sheet can still scroll, the last heading above the reading line
/// wins. `end_progress` lowers that line through the viewport during the
/// page's last screenful (0 at the start of the stretch, 1 at the maximum
/// offset), so headings the sheet can no longer put on the top line still
/// become current in document order, and the deepest visible one wins at the
/// very end.
pub(crate) fn current_section(
    outline: &Outline,
    rects: &[(usize, egui::Rect)],
    viewport_top: f32,
    viewport_bottom: f32,
    end_progress: f32,
) -> Option<usize> {
    if rects.is_empty() || outline.is_empty() {
        return None;
    }
    let progress = if end_progress.is_finite() {
        end_progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let bottom = viewport_bottom.max(viewport_top);
    let line = viewport_top + SPY_PAD + (bottom - viewport_top - SPY_PAD).max(0.0) * progress;
    let mut first_painted: Option<usize> = None;
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
        first_painted.get_or_insert(index);
        if rect.top() <= line {
            current = Some(index);
        }
    }
    // No heading passed the line yet: the reader is above the first section,
    // which is the one the sidebar should point at. Headings skipped this
    // pass (a closed accordion, an unselected tab) are not candidates.
    current.or(first_painted)
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
            current_section(&outline, &rects, 100.0, 700.0, 0.0),
            Some(1)
        );
        // Scrolled far DOWN: every heading is above the viewport top, so the
        // last one wins (the reader is inside the final section).
        assert_eq!(
            current_section(&outline, &rects, 1_000.0, 1_600.0, 0.0),
            Some(2)
        );
        // Scrolled far UP: no heading has reached the line yet, so the first
        // section is the one the reader is heading into.
        assert_eq!(
            current_section(&outline, &rects, -500.0, 100.0, 0.0),
            Some(0)
        );
        // Close to the first heading: the reader is already inside the
        // second section (it scrolled past the line), the third is ahead.
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, 0.0), Some(1));
        // A heading exactly at the viewport top qualifies (the pad allows for
        // the sub-pixel slack `scroll_to_here` leaves).
        assert_eq!(
            current_section(&outline, &rects, 300.0, 900.0, 0.0),
            Some(2)
        );
        assert_eq!(
            current_section(&outline, &rects, 313.0, 913.0, 0.0),
            Some(2)
        );
        // Below the line by itself: the previous heading stays current.
        assert_eq!(
            current_section(&outline, &rects, 200.0, 800.0, 0.0),
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
            current_section(&outline, &rects, -500.0, 300.0, 0.0),
            Some(1)
        );
        // At the bottom the same rects pick the only heading on screen, even
        // though its top never crossed the spy line.
        assert_eq!(
            current_section(&outline, &rects, -500.0, 300.0, 1.0),
            Some(2)
        );
        // A skipped last heading (no rect) never wins: the last RENDERED
        // heading that crossed the line does.
        assert_eq!(
            current_section(&outline, &rects[..2], -500.0, 300.0, 1.0),
            Some(1)
        );
    }

    /// The reading line descends through the last screenful instead of
    /// jumping to the deepest row at once: a cluster of headings at the foot
    /// of the page is still selected in document order as the sheet scrolls
    /// to the bottom.
    #[test]
    fn scrollspy_walks_a_heading_cluster_at_the_page_end() {
        let md = "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nBody\n\n## Four\n\nBody\n\n## Five\n\nTail\n";
        let (outline, rects) = outline_and_rects(md, &[-4.0, 40.0, 84.0, 128.0, 172.0]);
        // Line = 12 + 588 * progress on a 600px viewport.
        let seen: Vec<usize> = [0.0, 0.05, 0.13, 0.2, 0.3, 1.0]
            .into_iter()
            .filter_map(|p| current_section(&outline, &rects, 0.0, 600.0, p))
            .collect();
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 4]);
    }

    /// A heading hidden by collapsed content (a closed `<Accordion>`, an
    /// unselected `<Tabs>` panel) has an outline row but no rect. While the
    /// reader is still above every rendered heading, the cursor must point at
    /// the first row that actually renders, never at the hidden one above it:
    /// that row can never be reached by scrolling.
    #[test]
    fn scrollspy_skips_unrendered_headings_before_the_first_painted_one() {
        let (outline, rects) = outline_and_rects(
            "# Hidden\n\n## Visible\n\n### Later\n",
            &[0.0, 300.0, 900.0],
        );
        // `# Hidden` did not render (closed section), the reader is above the
        // first visible heading.
        assert_eq!(
            current_section(&outline, &rects[1..], 0.0, 600.0, 0.0),
            Some(1),
            "the hidden first row must not become current"
        );
        // A hidden heading between rendered ones keeps the merge scan honest.
        let (outline, rects) = outline_and_rects(
            "# Hidden\n\n## Visible\n\n### Later\n",
            &[-900.0, 0.0, 300.0],
        );
        assert_eq!(
            current_section(&outline, &[rects[0], rects[2]], 350.0, 950.0, 0.0),
            Some(2),
            "the last rendered heading above the line wins"
        );
    }

    /// A heading that never reaches the viewport top must still become
    /// current at the page end: the reading line reaches the bottom of the
    /// viewport at the maximum offset, so the last visible section wins
    /// instead of an earlier heading that merely sits above the top.
    #[test]
    fn scrollspy_reaches_the_deepest_visible_heading_at_the_page_end() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nTail\n",
            &[-30.0, 30.0, 80.0],
        );
        // At the top of the last screenful the line is still at the top.
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, 0.0), Some(0));
        // At the max offset every visible heading has been reached.
        assert_eq!(current_section(&outline, &rects, 0.0, 600.0, 1.0), Some(2));
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
            current_section(&outline, &rects, 200.0, 800.0, 0.0),
            Some(2)
        );
        // Only the first heading rendered, and it is above the line.
        let (outline, all) =
            outline_and_rects("# One\n\n## Two\n\n## Three\n", &[-900.0, -200.0, 100.0]);
        assert_eq!(
            current_section(&outline, &all[..1], 200.0, 800.0, 0.0),
            Some(0)
        );
        assert_eq!(current_section(&outline, &[], 0.0, 600.0, 0.0), None);
        assert_eq!(current_section(&outline, &[], 0.0, 600.0, 1.0), None);
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
        state.update_current(&outline, 100.0, 800.0, 0.0, true);
        assert_eq!(state.current, 1);
        assert!(state.reveal, "the cursor moved: the row is revealed");
        // Frame 2: the same place, no move, no reveal.
        state.reveal = false;
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 100.0, 800.0, 0.0, true);
        assert_eq!(state.current, 1);
        assert!(!state.reveal);
        // Frame 3: nothing rendered (a whole-document skip): the cursor stays.
        state.begin_frame();
        state.update_current(&outline, 10_000.0, 10_600.0, 0.0, true);
        assert_eq!(state.current, 1);
        // Frame 4: scrolled far past the last heading (every recorded rect
        // sits above the viewport top now).
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 2_000.0, 2_600.0, 0.0, true);
        assert_eq!(state.current, 2);
        // Navigation drops everything; the first section is revealed again
        // even if the panel's egui scroll offset belonged to the old page.
        state.reset();
        assert_eq!(state.current, 0);
        assert!(state.reveal);
        assert!(state.row_heights.is_empty());
        assert!(state.panel_rect.is_none());
    }

    /// The panel lights a row only when its heading painted in the last
    /// walk. A frame that skipped every heading (all content folded away or
    /// dropped by the block budget) keeps the internal cursor but reports no
    /// rendered row, so the sidebar cannot highlight a section that is not
    /// on the page.
    #[test]
    fn a_frame_that_painted_no_heading_lights_no_row() {
        let (outline, rects) = outline_and_rects("# Only\n\nBody\n", &[20.0]);
        let mut state = OutlineState::default();
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 0.0, 600.0, 0.0, true);
        assert!(
            state.current_is_rendered(&outline),
            "the painted heading is the current row"
        );
        state.begin_frame();
        state.update_current(&outline, 0.0, 600.0, 0.0, true);
        assert_eq!(state.current, 0, "the internal cursor is kept");
        assert!(
            !state.current_is_rendered(&outline),
            "but a skipped heading must not light a row"
        );
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
        // Halfway through the last screenful the spy would pick `## Two`,
        // but the clicked `## Three` is visible and stays pinned.
        state.update_current(&outline, -500.0, 300.0, 0.5, true);
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
        state.update_current(&outline, -50.0, 750.0, 0.0, true);
        assert_eq!(state.current, 1);
        assert!(state.pinned.is_none(), "the spy landed on the target");
    }

    /// The settled guard needs a baseline frame, an unchanged offset and no
    /// kinetic velocity: any of the three failing keeps a click pin alive.
    #[test]
    fn sheet_settled_needs_a_still_offset_and_no_velocity() {
        let mut state = OutlineState::default();
        assert!(
            !state.sheet_settled(100.0, 0.0),
            "the first frame has no previous offset to compare"
        );
        assert!(state.sheet_settled(100.0, 0.0), "same offset, at rest");
        assert!(
            !state.sheet_settled(140.0, 0.0),
            "a moving sheet is not settled"
        );
        assert!(
            !state.sheet_settled(140.0, 8.0),
            "kinetic velocity is not settled"
        );
        assert!(state.sheet_settled(140.0, 0.0), "stopped and at rest");
    }

    /// A click pin is released by the spy only once the requested scroll has
    /// SETTLED and reached its target (the top, or the page end). The page-end
    /// reading line crosses a clicked second-to-last row while the sheet is
    /// still heading to the bottom; releasing the pin at that crossing would
    /// hand the cursor to the deeper last section by the time it stops
    /// (Cloudflare's `Getting Started` -> `Resources`).
    #[test]
    fn a_click_pin_waits_for_the_scroll_to_settle() {
        let (outline, rects) = outline_and_rects(
            "# One\n\nBody\n\n## Two\n\nBody\n\n## Three\n\nBody\n\n## Four\n\nTail\n",
            &[-2_000.0, -900.0, 300.0, 340.0],
        );
        let mut state = OutlineState::default();
        // The reading line reaches the clicked `## Three` while the sheet is
        // still moving toward `## Four`: the pin survives the crossing.
        state.select(2);
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 300.0, 800.0, 0.0, false);
        assert_eq!(state.current, 2);
        assert!(
            state.pinned.is_some(),
            "the pin waits for the requested scroll to settle"
        );
        // The sheet settles with the spy still on `## Three`: the pin has
        // done its job and is released.
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 300.0, 800.0, 0.0, true);
        assert_eq!(state.current, 2);
        assert!(
            state.pinned.is_none(),
            "the settled spy lands on the target"
        );
        // At the bottom the spy picks the deeper `## Four`, but the clicked
        // `## Three` stays current after its scroll has settled: only a
        // manual scroll (or another click) may take the cursor away.
        state.select(2);
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 300.0, 800.0, 1.0, true);
        assert_eq!(state.current, 2, "the clicked row outlives the scroll");
        assert!(state.pinned.is_some());
        // A row that is ALREADY current when clicked is not released on the
        // settled click frame either: its top is far below the viewport top
        // and the sheet is mid-ramp, so the click still requests a scroll to
        // the page end. Only reaching that target may release the pin.
        state.select(3);
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 0.0, 800.0, 0.5, true);
        assert_eq!(state.current, 3);
        assert!(
            state.pinned.is_some(),
            "the click's scroll target has not been reached"
        );
        // At the page end the spy IS the clicked last row: the pin is
        // released so a later keyboard scroll can move the cursor again.
        state.begin_frame();
        for (at, rect) in &rects {
            state.record_rect(*at, *rect);
        }
        state.update_current(&outline, 0.0, 800.0, 1.0, true);
        assert_eq!(state.current, 3);
        assert!(state.pinned.is_none(), "the page end completes the pin");
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
