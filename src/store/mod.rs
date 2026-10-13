// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `store`: shared plumbing for the AEAD-encrypted local stores.
//!
//! - [`crypto`]: the single on-disk file format (magic/version header,
//!   random nonce, ChaCha20-Poly1305) plus the one shared `store.key`
//!   (created 0600, installed atomically); used by `history`, `bookmarks`
//!   and `settings`.
//! - [`writer`]: the serialized background writer (`fsync` off the UI
//!   thread, one bounded queue per store) those stores enqueue into.
//!
//! The stores themselves live in their own top-level modules (`history`,
//! `bookmarks`, `settings`); this module only holds what they share.

pub(crate) mod crypto;
pub(crate) mod writer;
