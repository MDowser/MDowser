// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `error`: the browser's unified error type.
//!
//! - Every module converts its failures into `BrowserError`.
//! - `Display` yields one-line messages, fit for the line-by-line error view.

use std::fmt;

/// Generic browser error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserError {
    /// The URL is not `https://`. Never HTTP.
    NotHttps(String),
    /// The URL could not be parsed or normalized.
    BadUrl(String),
    /// Network/transport failure on one attempt.
    Fetch(String),
    /// Cooperative cancellation (superseded navigation, tab close, timeout).
    /// Typed so callers never match on the string `"cancelled"`.
    Cancelled,
    /// The server returned another Content-Type (e.g. `text/html`).
    /// The found value is kept for the log view.
    WrongContentType {
        /// Attempted URL.
        url: String,
        /// Received Content-Type (or "(missing)" when absent).
        found: String,
    },
    /// The body is not valid UTF-8 (UTF-8 is mandatory).
    InvalidUtf8 {
        /// Attempted URL.
        url: String,
        /// UTF-8 error detail.
        detail: String,
    },
    /// YAML frontmatter exists but is malformed.
    /// Per spec, a malformed document moves on to the next route.
    BadFrontmatter {
        /// Attempted URL.
        url: String,
        /// YAML parser detail.
        detail: String,
    },
    /// No route candidate worked. Holds the line-by-line log.
    NoCandidateWorked {
        /// Normalized requested URL (echoed in the error view; credentials
        /// are stripped by `parse_https` before this point).
        asked: String,
        /// One line per attempt (already formatted for the UI).
        lines: Vec<String>,
    },
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHttps(u) => write!(f, "HTTPS only, blocked: {u}"),
            Self::BadUrl(e) => write!(f, "Invalid URL: {e}"),
            Self::Fetch(e) => write!(f, "network error: {e}"),
            Self::Cancelled => write!(f, "navigation superseded by a newer one"),
            Self::WrongContentType { url, found } => write!(
                f,
                "access to {url} returned {found} (expected a markdown Content-Type)"
            ),
            Self::InvalidUtf8 { url, detail } => {
                write!(f, "access to {url} is not valid UTF-8: {detail}")
            }
            Self::BadFrontmatter { url, detail } => {
                write!(f, "access to {url} has malformed YAML: {detail}")
            }
            Self::NoCandidateWorked { asked, lines } => {
                writeln!(f, "could not load {asked}:")?;
                for l in lines {
                    writeln!(f, "{l}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for BrowserError {}

impl BrowserError {
    /// Returns `true` for cooperative cancellation (superseded load).
    /// Prefer over string matching on the display text.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}
