// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser`: tab state and history.
//!
//! - Each `Tab` is independent: its URL, its back/forward history,
//!   its document or its error, its image textures, and its "Refresh"
//!   throttle.
//! - No global cache: every navigation re-downloads ("no cache" requirement).
//!   Refresh, retry and re-navigation to the same URL always fetch anew;
//!   neither results nor errors are reused. History only stores URLs,
//!   never bodies. Document images live only while their document is on
//!   screen (see `media::cache`).
//! - Tab persistence ships (`settings::save_session` / `load_session`, used
//!   by `app` when "restore tabs on startup" is on). Planned: prefetching,
//!   optional cache.

pub mod tab;
