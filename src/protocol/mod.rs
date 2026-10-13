// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol`: networking, local files and route resolution.
//!
//! - `content_type`: validates a markdown `Content-Type` (the standard type
//!   plus its common aliases, with an optional UTF-8 charset) and the
//!   accepted image types for subresources.
//! - `resolver`: builds HTTPS candidates in order and normalizes to `/.../`.
//! - `rewrite`: absolutizes relative links before rendering.
//! - `client`: capped HTTPS GET (shared by documents and images) with the
//!   fallback cascade + line-by-line log, and the `parse_navigable` entry
//!   point (HTTPS or local file).
//! - `local`: local `file://` documents and subresources, with the same
//!   caps/cancel contract as the network path. Only user input and
//!   already-local documents can reach it (see `rewrite`).
//!
//! Everything touching the network goes through `protocol::client`
//! (`fetch_with_fallback` for documents, `get_bytes` for subresources).
//! Local files are read by `protocol::local`.

pub mod client;
pub mod content_type;
pub mod local;
pub mod resolver;
pub mod rewrite;
