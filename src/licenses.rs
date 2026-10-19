// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `licenses`: the embedded third-party notices, rendered as an internal page.
//!
//! `THIRD_PARTY_NOTICES.md` is the single source of truth for the licenses of
//! the linked crates and the bundled fonts: it ships in every release archive
//! (see `scripts/build-dist.sh`) and is embedded into the binary by
//! [`crate::assets::THIRD_PARTY_NOTICES`], so the in-app page
//! (`about:licenses`, reached from Preferences) always shows the EXACT text
//! distributed with the running build.
//!
//! The document is parsed once per process ([`std::sync::OnceLock`]) through
//! the same pipeline as a fetched document
//! ([`crate::protocol::client::finish_document`] derives events, anchors,
//! copy text and destinations identically), so opening the page in several
//! tabs or repainting it never re-parses the Markdown. The base URL is the
//! internal `about:licenses` itself: absolute `https:` links inside the
//! notices stay live, while relative links cannot resolve against an
//! `about:` page and render inert (exactly like a broken link in a fetched
//! document).

use crate::protocol::client::Fetched;
use std::sync::OnceLock;

/// Parsed third-party notices, or `None` if the embedded Markdown ever fails
/// to parse (the page then renders empty instead of panicking the UI thread;
/// the `embedded_notices_parse` test keeps this from shipping unnoticed).
pub(crate) fn document() -> Option<&'static Fetched> {
    static DOC: OnceLock<Option<Fetched>> = OnceLock::new();
    DOC.get_or_init(parse).as_ref()
}

/// Parses [`crate::assets::THIRD_PARTY_NOTICES`] with `about:licenses` as
/// both the landed and the error URL (it never travels over the network).
fn parse() -> Option<Fetched> {
    let url = url::Url::parse(crate::config::LICENSES_URL).ok()?;
    // The embedded notices are a `'static` str; the pipeline takes the body
    // by value (it keeps it as the document's export source). One copy per
    // PROCESS: `document()` memoizes the result.
    crate::protocol::client::finish_document(
        crate::assets::THIRD_PARTY_NOTICES.to_string(),
        url.clone(),
        &url,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    /// The embedded notices must parse and stay a real document: the page has
    /// no fallback body, so a parse failure would ship an empty licenses page.
    #[test]
    fn embedded_notices_parse() {
        let doc = super::document().expect("embedded notices parse");
        assert!(
            doc.events.len() > 100,
            "notices are a full document, not a stub"
        );
        assert!(!doc.plain_text.is_empty(), "copy text derived");
        assert_eq!(doc.display_url.as_str(), crate::config::LICENSES_URL);
    }
}
