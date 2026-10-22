// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::error_view`: styled access error screen.
//!
//! REQUIREMENT:
//! - Network failures, local file failures and non-markdown answers need
//!   clear, well-formatted messages (not just a raw log).
//! - The route log is still shown (one line per candidate, in order),
//!   with the `https://`/`file://` scheme hidden for minimalism (the
//!   browser owns the transport). The Retry button lives in `viewer`.

use crate::ui::theme;
use crate::util::{strip_credentials_for_log, strip_scheme_everywhere};
use eframe::egui;

/// Error family for a friendly header + hints. Classified from the
/// line-by-line log (which stays visible below as details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    /// Transport / server unreachable (DNS, TLS, timeout, stall, 5xx...).
    Network,
    /// All candidates answered 404.
    NotFound,
    /// Server answered but without Markdown (wrong type, missing type,
    /// bad UTF-8, malformed frontmatter).
    NotMarkdown,
    /// Local address problem (bad URL, non-https). No network touched.
    BadAddress,
    /// Anything else (mixed log, cancellation, oversized body...).
    Other,
}

/// Classifies the log lines (case-insensitive substring scan).
///
/// Allocation-free: it used to `join("\n")` and `to_ascii_lowercase()` the
/// whole log on EVERY frame while the retry cooldown ticked. The scan
/// predicates keep their original evaluation order.
pub(crate) fn classify(lines: &[String]) -> ErrorKind {
    use crate::util::contains_ignore_ascii_case as has;
    let any = |needles: &[&str]| {
        lines
            .iter()
            .any(|line| needles.iter().any(|needle| has(line, needle)))
    };
    if any(&["invalid url:", "https only, blocked:"]) {
        return ErrorKind::BadAddress;
    }
    // Missing markdown signals beat generic HTTP: they explain *why*
    // every candidate was dropped even though the server answered.
    if any(&[
        "expected a markdown content-type",
        "(missing)",
        "not valid utf-8",
        "malformed yaml",
    ]) {
        return ErrorKind::NotMarkdown;
    }
    if any(&["does not exist (404)"]) {
        return ErrorKind::NotFound;
    }
    // Oversized body is a policy rejection, not a transport failure:
    // classify BEFORE the generic network scan (which matched "exceeds").
    if any(&["exceeds"]) {
        return ErrorKind::Other;
    }
    if any(&[
        "network error",
        "stalled",
        "timeout",
        "cancelled",
        "superseded",
        "no runtime",
        "returned http",
        "redirected off https",
        "error reading body",
    ]) {
        return ErrorKind::Network;
    }
    ErrorKind::Other
}

/// Title + explanation + hint bullets per family (ASCII only).
/// `local` selects the wording for a local-file load (no server involved).
/// Hints are `Cow`: static strings stay borrowed (no per-frame `Vec<String>`
/// on an error page that repaints during the retry cooldown), while the one
/// config-derived value stays owned so it can never drift from the constant
/// the validator actually uses.
fn copy_for(
    kind: ErrorKind,
    local: bool,
) -> (
    &'static str,
    &'static str,
    Vec<std::borrow::Cow<'static, str>>,
) {
    use std::borrow::Cow;
    let s = |text: &'static str| Cow::Borrowed(text);
    match kind {
        ErrorKind::Network => (
            "No connection",
            "The server could not be reached. Your address looks fine, \
             but the network or the server failed.",
            vec![
                s("Check your internet connection."),
                s("Retry in a few seconds (cooldown applies)."),
                s("Go back or try the site root (/)."),
            ],
        ),
        ErrorKind::NotFound if local => (
            "File not found",
            "None of the local candidates exists (the exact path, .md, \
             index.md and README.md).",
            vec![
                s("Check the spelling of the path."),
                s("Relative links resolve against the document's directory."),
                s("Use an absolute path like /Users/you/notes.md."),
            ],
        ),
        ErrorKind::NotFound => (
            "Page not found",
            "The server answered, but none of the markdown routes exists \
             (exact, /, .md, index.md, README.md).",
            vec![
                s("Check the spelling of the address."),
                s("Try the site root (/) and follow links."),
                s("The page may simply not exist yet."),
            ],
        ),
        ErrorKind::NotMarkdown if local => (
            "No markdown here",
            "The file could not be shown: it must be valid UTF-8 and its \
             YAML frontmatter (when present) must be valid.",
            vec![
                s("Only UTF-8 Markdown files can be opened."),
                s("Fix or remove the `---` frontmatter block."),
                s("See Details below for the exact reason."),
            ],
        ),
        ErrorKind::NotMarkdown => (
            "No markdown here",
            "The server answered, but it did not serve Markdown \
             (need a markdown Content-Type, valid UTF-8 and valid frontmatter).",
            vec![
                s("This browser only reads markdown documents."),
                s("Try another path or the site root (/)."),
                s("See Details below for the Content-Type found."),
            ],
        ),
        ErrorKind::BadAddress => (
            "Invalid address",
            "This address was rejected before touching the network \
             (HTTPS-only browser; local paths and file:// are also supported).",
            vec![
                s("Use host/path like example.com/love (no http://)."),
                s("Local files: /Users/you/notes.md, ~/notes.md or file:///path/notes.md."),
                s("mailto:, tel:, data:, ftp: and remote file://host/ are blocked."),
                Cow::Owned(format!(
                    "Keep it under {} characters.",
                    crate::config::MAX_URL_INPUT_CHARS
                )),
            ],
        ),
        ErrorKind::Other => (
            "Could not load",
            "This page could not be loaded. See the details below.",
            vec![
                s("Retry in a few seconds (cooldown applies)."),
                s("Go back or try the site root (/)."),
            ],
        ),
    }
}

/// Display-safe rendering of a log/address line: credentials stripped first
/// (a pasted `user:pass@` or a server redirect target must never reach the
/// screen or the clipboard), then the transport scheme hidden.
///
/// Single helper so the painted `Requested:`/details lines and the `Copy log`
/// clipboard text cannot drift apart again.
fn display_line(line: &str) -> String {
    strip_scheme_everywhere(&strip_credentials_for_log(line)).into_owned()
}

/// Shows the styled error with its attempt log (scheme hidden, credentials
/// stripped).
///
/// `kind` comes from [`classify`]: the caller already needed it (Retry
/// enablement), so the log is never scanned twice per frame.
pub fn show_error(ui: &mut egui::Ui, asked: &str, lines: &[String], kind: ErrorKind) {
    let local = crate::util::starts_with_ignore_ascii_case(asked.trim_start(), "file:");
    let (title, explain, hints) = copy_for(kind, local);
    let s = theme::scheme_ref(ui);
    ui.vertical(|ui| {
        ui.add_space(theme::spacing::M);
        // Header row: status dot + title + copy action.
        ui.horizontal(|ui| {
            // Status dot (error red for real failures). The dot slot only
            // reserves the horizontal room; the disc is painted centered on
            // the TITLE's laid-out rect. A horizontal row top-clamps any
            // item shorter than the row (the slot is 0.9x the title size,
            // the title is a full font line), so centering inside the slot
            // alone parked the disc several px above the title's center.
            // Title uses the dialog-title token (21px): the old 24px literal
            // drifted from the type scale.
            let title_size = theme::typography::DIALOG_TITLE;
            let (dot_slot, _) = ui.allocate_exact_size(
                egui::vec2(title_size * 0.7, title_size * 0.9),
                egui::Sense::hover(),
            );
            ui.add_space(theme::spacing::XS);
            let title_resp =
                ui.label(theme::strong(egui::RichText::new(title).size(title_size)).color(s.error));
            // Accessible label for the family title (egui has no heading
            // role; `Label` keeps it in the accesskit tree like every row).
            title_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, title));
            ui.painter().circle_filled(
                egui::pos2(dot_slot.center().x, title_resp.rect.center().y),
                title_size * 0.25,
                s.error,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let copy_log = ui.small_button("Copy log");
                let copy_log =
                    crate::ui::hover_tooltip(copy_log, "Copy the attempt log to the clipboard");
                if copy_log.clicked() {
                    let mut full = format!("Requested: {}\n", display_line(asked));
                    for l in lines {
                        full.push_str(&display_line(l));
                        full.push('\n');
                    }
                    ui.ctx().copy_text(full);
                }
            });
        });
        ui.add_space(theme::spacing::XS);
        ui.label(theme::weak(ui, explain));
        // Requested address wraps and tooltips: a long URL used to overflow
        // the sheet (vertical-only scroll, no wrap/truncate/tooltip).
        // Credential-stripped like the log lines: a pasted `user:pass@`
        // must never paint here either.
        let requested = format!("Requested: {}", display_line(asked));
        let requested_resp = ui.label(&requested);
        // Gated (not lazy): `requested` is borrowed, so hovered frames
        // alone allocate the tooltip card.
        let _ = crate::ui::hover_tooltip(requested_resp, &requested);
        // "No markdown here" is the one failure with a real escape hatch:
        // the target exists and answered, it just is not Markdown, so the OS
        // default handler can show it (the default browser for web pages,
        // the file's default app for local ones). A large accent button
        // right under the requested address: this is the action a reader
        // actually wants on this page.
        if kind == ErrorKind::NotMarkdown {
            ui.add_space(theme::spacing::M);
            let label = if local {
                "Open with the default app"
            } else {
                "Open in your browser"
            };
            let open = theme::primary_button_sized(ui, label, egui::vec2(240.0, 44.0));
            if open.clicked() {
                // Resolve ONCE on the click (not per frame): `parse_navigable`
                // accepts only targets this browser could have navigated to
                // and strips pasted credentials, so the OS never receives
                // anything else.
                if let Ok(target) = crate::protocol::client::parse_navigable(asked) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(target.as_str()));
                }
            }
        }
        ui.add_space(theme::spacing::M);
        // Hint card: what to do next.
        theme::card(ui).show(ui, |ui| {
            ui.label(theme::strong(egui::RichText::new("What you can do:")));
            ui.add_space(theme::spacing::XS);
            for h in hints {
                ui.horizontal(|ui| {
                    // Document bullet gutter (not `-`): matches the reader's
                    // list marker so hints read as a list, not code.
                    ui.label(egui::RichText::new("·").color(s.on_surface_variant));
                    ui.label(h.as_ref());
                });
            }
        });
        ui.add_space(theme::spacing::M);
        ui.label(theme::strong(egui::RichText::new("Details:")));
        ui.label(theme::weak(
            ui,
            if local {
                "Tried in order: exact path, .md, index.md and README.md."
            } else {
                "Tried in order: exact, with /, .md, index.md and README.md."
            },
        ));
        ui.add_space(theme::spacing::XS);
        // Bounded scroll log: readable mono, and it SHRINKS to its lines
        // (up to 320px). Reserving the full cap on every error pushed Retry
        // a hollow 320px below Details, off-screen on a short window; the
        // action sat at the page's bottom border instead of beside the log.
        // The outer page already scrolls: keep the inner bar automatic so
        // two always-visible bars never sit side by side.
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(320.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                if lines.is_empty() {
                    ui.monospace("(no details)");
                } else {
                    for l in lines {
                        ui.monospace(display_line(l));
                    }
                }
            });
        ui.add_space(theme::spacing::XS);
        ui.separator();
    });
}

#[cfg(test)]
mod tests {
    use super::{classify, copy_for, ErrorKind};
    use eframe::egui;

    #[test]
    fn classifies_network() {
        let lines = vec!["access to https://a network error: dns".to_string()];
        assert_eq!(classify(&lines), ErrorKind::Network);
        let lines = vec!["access to https://a stalled (no data for 15s)".to_string()];
        assert_eq!(classify(&lines), ErrorKind::Network);
    }

    #[test]
    fn classifies_not_markdown() {
        let lines = vec![
            "access to https://a returned text/html (expected a markdown Content-Type)".to_string(),
        ];
        assert_eq!(classify(&lines), ErrorKind::NotMarkdown);
    }

    #[test]
    fn classifies_not_found_and_bad_address() {
        let lines = vec!["access to https://a does not exist (404)".to_string()];
        assert_eq!(classify(&lines), ErrorKind::NotFound);
        let lines = vec!["HTTPS only, blocked: http://a".to_string()];
        assert_eq!(classify(&lines), ErrorKind::BadAddress);
    }

    /// Every painted text run of one frame.
    fn texts(out: &egui::FullOutput) -> Vec<String> {
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Screen center of one painted label (the buttons center their label).
    fn text_center(out: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        out.shapes.iter().find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
    }

    /// One primary-pointer transition at `pos`.
    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// One rendered `show_error` frame. Returns the full output so callers
    /// can inspect shapes (dot/title geometry) and the queued OS commands
    /// (the external-open action), the same headless pattern the theme tests
    /// use.
    fn render(
        ctx: &egui::Context,
        asked: &str,
        lines: &[String],
        kind: ErrorKind,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 500.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            super::show_error(ui, asked, lines, kind);
        });
        out.textures_delta.clear();
        out
    }

    /// The URL of the first `OpenUrl` command in one frame's output.
    fn opened_url(out: &egui::FullOutput) -> Option<String> {
        out.platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::OpenUrl(u) => Some(u.url.clone()),
            _ => None,
        })
    }

    /// The status dot centers on the TITLE's laid-out rect. It used to be
    /// centered in its own (shorter) slot, which the horizontal layout
    /// top-clamps, parking the disc ~6px above the title's center.
    #[test]
    fn header_dot_is_centered_on_the_title() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let out = render(
            &ctx,
            "example.com/page",
            &["x".to_string()],
            ErrorKind::NotMarkdown,
            Vec::new(),
        );
        let circle = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Circle(c) => Some(c.center),
                _ => None,
            })
            .expect("the status dot painted");
        let title = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "No markdown here" => {
                    Some((t.pos, t.galley.size()))
                }
                _ => None,
            })
            .expect("the title painted");
        let title_center_y = title.0.y + title.1.y / 2.0;
        assert!(
            (circle.y - title_center_y).abs() <= 0.5,
            "dot center y {} vs title center {title_center_y}",
            circle.y
        );
        assert!(circle.x < title.0.x, "the dot sits left of the title");
    }

    /// "No markdown here" offers the escape hatch: a large accent button
    /// that hands the resolved address to the OS default handler (the
    /// `OpenUrl` command `egui-winit` executes through the embedded
    /// `webbrowser` integration). Clicking it queues the NORMALIZED URL:
    /// pasted credentials never reach the browser.
    #[test]
    fn not_markdown_opens_the_target_externally() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let lines = vec!["access to https://example.com/page returned text/html \
             (expected a markdown Content-Type)"
            .to_string()];
        let asked = "https://user:pass@example.com/page";
        let first = render(&ctx, asked, &lines, ErrorKind::NotMarkdown, Vec::new());
        assert!(
            texts(&first).iter().any(|t| t == "Open in your browser"),
            "the web button is painted"
        );
        let center = text_center(&first, "Open in your browser").expect("button label painted");
        // Hover + press, then release: egui's click lands on the release frame.
        let _ = render(
            &ctx,
            asked,
            &lines,
            ErrorKind::NotMarkdown,
            vec![egui::Event::PointerMoved(center), pointer(center, true)],
        );
        let out = render(
            &ctx,
            asked,
            &lines,
            ErrorKind::NotMarkdown,
            vec![pointer(center, false)],
        );
        assert_eq!(
            opened_url(&out).as_deref(),
            Some("https://example.com/page"),
            "the credential-free normalized URL is opened"
        );
    }

    /// A local non-markdown file gets the same escape hatch with the local
    /// wording (the OS default APP: for a local file the browser is not the
    /// handler the launcher picks) and its absolute `file:` URL.
    #[test]
    fn local_not_markdown_opens_with_the_default_app() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let lines = vec!["file is not valid utf-8".to_string()];
        let asked = "file:///tmp/notes.txt";
        let first = render(&ctx, asked, &lines, ErrorKind::NotMarkdown, Vec::new());
        assert!(
            texts(&first)
                .iter()
                .any(|t| t == "Open with the default app"),
            "the local button is painted"
        );
        let center =
            text_center(&first, "Open with the default app").expect("button label painted");
        let _ = render(
            &ctx,
            asked,
            &lines,
            ErrorKind::NotMarkdown,
            vec![egui::Event::PointerMoved(center), pointer(center, true)],
        );
        let out = render(
            &ctx,
            asked,
            &lines,
            ErrorKind::NotMarkdown,
            vec![pointer(center, false)],
        );
        assert_eq!(opened_url(&out).as_deref(), Some("file:///tmp/notes.txt"));
    }

    /// The escape hatch is scoped to the "No markdown here" family: every
    /// other failure keeps its page (there is nothing else that reliable to
    /// hand to the OS; a failed address must not be).
    #[test]
    fn other_kinds_offer_no_external_open() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let lines = vec!["access to https://a network error: dns".to_string()];
        for kind in [
            ErrorKind::Network,
            ErrorKind::NotFound,
            ErrorKind::BadAddress,
            ErrorKind::Other,
        ] {
            let out = render(&ctx, "example.com/page", &lines, kind, Vec::new());
            assert!(
                !texts(&out)
                    .iter()
                    .any(|t| t == "Open in your browser" || t == "Open with the default app"),
                "{kind:?} must not offer an external open"
            );
            assert!(opened_url(&out).is_none(), "{kind:?}: no command queued");
        }
    }

    #[test]
    fn every_kind_has_copy() {
        for k in [
            ErrorKind::Network,
            ErrorKind::NotFound,
            ErrorKind::NotMarkdown,
            ErrorKind::BadAddress,
            ErrorKind::Other,
        ] {
            for local in [false, true] {
                let (t, e, h) = copy_for(k, local);
                assert!(!t.is_empty() && !e.is_empty() && !h.is_empty());
            }
        }
    }
}
