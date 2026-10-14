// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::autolink`: GFM-style bare-URL linkification.
//!
//! `pulldown-cmark` does not implement the GFM autolink extension (bare
//! `www.` / `https://` URLs typed as plain text), and `Options::ENABLE_GFM`
//! only populates blockquote kinds — turning it on would consume the
//! `> [!NOTE]` callout markers the viewer renders manually. This pass adds
//! the extension WITHOUT touching the parser: after `parse_events` it
//! rewrites TEXT nodes that contain a bare URL into `Link` events, so the
//! viewer's existing link rendering, hit-testing, clipboard and right-click
//! menu apply unchanged (the destination is absolutized/validated like any
//! other link, and the label text stays byte-for-byte the source).
//!
//! Rules (GFM "Autolinks (extension)", adjusted to what this browser can
//! actually navigate: `https:` and local `file:` only):
//! - Recognition starts at the beginning of a text run, after whitespace, or
//!   after one of `*`, `_`, `~`, `(`.
//! - Three recognized forms, and NOTHING else: `https://` followed by a
//!   valid domain, `www.` followed by a valid domain (with `https://`
//!   inserted — GFM inserts `http://`), and a local `file:` URL the reader
//!   accepts (`file:///x`, `file:/x`, `file://localhost/x`). `http:`,
//!   `mailto:`, `ftp:`, `data:` and every other scheme stay plain text.
//!   A valid domain is dot-separated segments of alphanumerics, `_` and
//!   `-`, with at least one period and no `_` in its last two segments.
//! - After the domain/path, every non-space non-`<` character is part of the
//!   candidate; trailing punctuation (`?`, `!`, `.`, `,`, `:`, `*`, `_`,
//!   `~`), unmatched `)` and a trailing entity reference (`&hl;`) are
//!   excluded, per the GFM spec's "extended autolink path validation".
//! - Email addresses never link (`user@example.com`,
//!   `user.name@example.com`, `mailto:...`): a domain followed by `@` is
//!   rejected, and `mailto:` is not a recognized scheme.
//! - SUGAR beyond GFM: a bare domain WITHOUT `www.` (`celticos.com`,
//!   `scholar-sidekick.com`, `agentready.md`) is linkified anywhere, with
//!   `https://` inserted. Two guards keep filesystem and prose tokens out:
//!   1. the last label must be a REAL TLD of the IANA root zone
//!      (`markdown::tld`), which excludes `Cargo.toml`, `notes.txt`,
//!      `data.csv`, `v1.2`, `foo.local` and any other dotted non-host;
//!   2. when that TLD doubles as a common file extension (`md`, `rs`, `sh`,
//!      `py`, `pl`, `ai`, `zip`, ...), the name before it must not be a
//!      known file stem: `README.md`, `config.rs`, `script.pl`, `logo.ai`
//!      stay text, while real hosts (`agentready.md`, `mkplandeki.pl`) link.
//! - Code spans/blocks, metadata blocks, existing links and image alt text
//!   are never touched.
//! - Bounded by `config::MAX_AUTOLINKS_PER_DOC` (each link adds two events
//!   around text that was already there) and by
//!   `config::AUTOLINK_MAX_DOMAIN_CHARS`. The scan is linear: a candidate
//!   the scanner rejects is still consumed whole, so no byte is rescanned.
//!
//! This pass only rewrites the RENDERED events. `plain_text_from_events`
//! derives "Copy" from those same events and copies each text node as
//! written, so the linkification wrappers are transparent: the clipboard
//! pastes the URL exactly as the page shows it.

use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use url::Url;

/// One recognized bare URL inside a text node.
struct AutoLink {
    /// Byte range of the visible text (`[start..end]`).
    start: usize,
    end: usize,
    /// Destination the link event carries.
    dest: String,
}

/// TLDs that double as common file extensions: paired with a file-like stem
/// (`config` + `rs`) the token is a file name, not a host. See
/// `looks_like_filename`.
const FILE_EXT_TLDS: &[&str] = &[
    "ai", "cc", "fm", "gg", "md", "ml", "mov", "pl", "py", "rs", "sh", "so", "st", "to", "tv",
    "zip", "zone",
];

/// Stems of well-known file names, checked (case-insensitively) before an
/// extension-like TLD. Curated, ASCII: the classic README/config/main family
/// plus common code, doc and asset names.
const FILE_STEMS: &[&str] = &[
    "authors",
    "avatar",
    "backup",
    "babel",
    "banner",
    "bar",
    "baz",
    "bin",
    "build",
    "cache",
    "cargo",
    "changelog",
    "changes",
    "client",
    "code_of_conduct",
    "component",
    "components",
    "composer",
    "config",
    "constants",
    "contributing",
    "copy",
    "cover",
    "data",
    "demo",
    "deploy",
    "dist",
    "dockerfile",
    "doc",
    "docs",
    "documentation",
    "draft",
    "editorconfig",
    "env",
    "error",
    "errors",
    "eslint",
    "example",
    "examples",
    "favicon",
    "fixture",
    "fixtures",
    "file",
    "files",
    "final",
    "foo",
    "gemfile",
    "gitattributes",
    "gitignore",
    "helper",
    "helpers",
    "icon",
    "icons",
    "image",
    "images",
    "img",
    "index",
    "input",
    "install",
    "layout",
    "lib",
    "license",
    "licence",
    "log",
    "logs",
    "logo",
    "logos",
    "main",
    "makefile",
    "manual",
    "media",
    "mock",
    "mocks",
    "mod",
    "model",
    "models",
    "movie",
    "node",
    "note",
    "notes",
    "notice",
    "output",
    "package",
    "path",
    "paths",
    "photo",
    "photos",
    "podfile",
    "prettier",
    "public",
    "qux",
    "readme",
    "release",
    "report",
    "requirements",
    "result",
    "results",
    "route",
    "routes",
    "router",
    "run",
    "sample",
    "samples",
    "schema",
    "screenshot",
    "script",
    "scripts",
    "security",
    "server",
    "service",
    "settings",
    "setup",
    "source",
    "spec",
    "specs",
    "src",
    "start",
    "static",
    "stop",
    "store",
    "style",
    "styles",
    "summary",
    "temp",
    "template",
    "test",
    "tests",
    "theme",
    "thumb",
    "thumbnail",
    "tmp",
    "todo",
    "tool",
    "tools",
    "types",
    "util",
    "utils",
    "vendor",
    "video",
    "videos",
    "view",
    "views",
    "vite",
    "webpack",
    "worker",
    "yarn",
];

/// Rewrites bare URLs in TEXT nodes into `Link` events.
///
/// - `events` are the owned events from `parse_events` (emoji already
///   expanded); the output is the same stream with `Start(Link)` /
///   `End(Link)` pairs inserted around recognized URL text.
/// - Tracks `CodeBlock`, `MetadataBlock`, `Link` and `Image` depth so code,
///   metadata, existing link labels and image alt text stay literal.
/// - `last` carries the last emitted character across adjacent text runs, so
///   a URL glued to a previous run (`a&amp;www…`, a code span, a footnote
mod scan;

pub use scan::linkify;

#[cfg(test)]
mod tests;
