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
use crate::util::strip_scheme_everywhere;
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

/// Shows the styled error with its attempt log (scheme hidden).
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
            // Status dot (error red for real failures). Allocated as a real
            // widget rect and centered inside it: the old `+ (8,14)` offset
            // drifted with zoom/font changes.
            let title_size = 24.0;
            let (dot_rect, _) = ui.allocate_exact_size(
                egui::vec2(title_size * 0.7, title_size * 0.9),
                egui::Sense::hover(),
            );
            ui.painter()
                .circle_filled(dot_rect.center(), title_size * 0.25, s.error);
            ui.add_space(theme::spacing::XS);
            ui.label(theme::strong(egui::RichText::new(title).size(title_size)).color(s.error));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let copy_log = ui.small_button("Copy log");
                let copy_log =
                    crate::ui::hover_tooltip(copy_log, "Copy the attempt log to the clipboard");
                if copy_log.clicked() {
                    let mut full = format!("Requested: {}\n", strip_scheme_everywhere(asked));
                    for l in lines {
                        full.push_str(&strip_scheme_everywhere(l));
                        full.push('\n');
                    }
                    ui.ctx().copy_text(full);
                }
            });
        });
        ui.add_space(2.0);
        ui.label(theme::weak(ui, explain));
        ui.label(format!("Requested: {}", strip_scheme_everywhere(asked)));
        ui.add_space(theme::spacing::M);
        // Hint card: what to do next.
        theme::card(ui).show(ui, |ui| {
            ui.label(theme::strong(egui::RichText::new("What you can do:")));
            ui.add_space(theme::spacing::XS);
            for h in hints {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("-").color(s.on_surface_variant));
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
        // Bounded scroll log: readable mono, never pushes Retry off-screen.
        // The outer page already scrolls: keep the inner bar automatic so
        // two always-visible bars never sit side by side.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(320.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                if lines.is_empty() {
                    ui.monospace("(no details)");
                } else {
                    for l in lines {
                        ui.monospace(strip_scheme_everywhere(l));
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
