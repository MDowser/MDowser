// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown`: CommonMark + table extensions + footnotes documents.
//!
//! - `frontmatter`: splits the optional YAML block.
//! - `document`: parser options, body normalization (including `:::`/`!!!`
//!   admonitions and structural `:::` custom containers) and utilities.
//! - `anchors`: heading/footnote targets for in-document navigation.
//! - `outline`: the document's heading outline (the reading-aid panel's
//!   source), built from the same events and reserved ids.
//! - `emoji`: `:shortcode:` expansion for text nodes.
//! - `autolink`: GFM-style bare-URL (`www.…`, `http(s)://…`) linkification of
//!   text nodes, so plain text URLs render like markdown links, plus bare
//!   domains (`celticos.com`) validated against the root-zone TLD list.
//! - `tld`: generated IANA root-zone TLD allowlist for `autolink`.
//! - `highlight`: syntax spans for rendered code blocks.
//!
//! Rendering lives in `viewer` (egui). This module only defines WHICH
//! extensions are active, so protocol and UI share the same criteria.

pub mod anchors;
pub mod autolink;
pub mod document;
pub mod emoji;
pub mod frontmatter;
pub mod highlight;
pub mod outline;
pub mod tld;
