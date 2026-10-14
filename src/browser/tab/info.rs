// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `browser::tab::info`: the brief page summary behind the omnibox
//! security-indicator popup.
//!
//! Facts only, read from the tab's own state: what the reader is looking
//! at (the sanitized page title, when the document carries one), where it
//! came from (the exact address, scheme included) and how it traveled
//! (HTTPS, local file or an internal page). Built only while the popup is
//! open; every string is sanitized and bounded like any other
//! remote-controlled display text.

use super::*;

/// Brief facts about the page a tab is showing, for the omnibox indicator
/// popup (`ui::omnibox`). Owned strings are built on demand and bounded;
/// nothing here touches the network or the stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteInfo {
    /// Short headline naming the page kind ("Secure connection", ...).
    pub title: &'static str,
    /// One muted sentence: where the page came from and what it is.
    pub detail: &'static str,
    /// The page's own title (sanitized, bounded), when the document
    /// carries one. `None` on internal pages, while loading and on
    /// failures.
    pub page_title: Option<String>,
    /// Exact address of the page, scheme included (`https://ex.com/a/`;
    /// a local file shows its decoded path). `None` when there is nothing
    /// addressable (the new-tab page).
    pub address: Option<String>,
}

impl Tab {
    /// Brief facts about the page on screen (or in flight / failed), for
    /// the omnibox security-indicator popup. Allocations happen only while
    /// the popup is open.
    pub fn site_info(&self) -> SiteInfo {
        match &self.state {
            TabState::Home => SiteInfo {
                title: "New tab page",
                detail: "Internal page: nothing is loaded and nothing is sent over the network.",
                page_title: None,
                address: None,
            },
            TabState::History => SiteInfo {
                title: "Local history",
                detail: "Internal page rendered from the encrypted store on this device.",
                page_title: None,
                address: Some(HISTORY_URL.to_string()),
            },
            TabState::Licenses => SiteInfo {
                title: "Third-party licenses",
                detail: "Internal page embedded in this build; nothing is fetched.",
                page_title: None,
                address: Some(LICENSES_URL.to_string()),
            },
            TabState::Loading { asked, .. } | TabState::Settling { asked, .. } => {
                if self.is_local_file() {
                    SiteInfo {
                        title: "Loading local file",
                        detail: "Read from this device; nothing is sent over the network.",
                        page_title: None,
                        address: Some(asked_local_for_display(asked)),
                    }
                } else {
                    SiteInfo {
                        title: "Loading over HTTPS",
                        detail:
                            "MDowser fetches pages over HTTPS only and renders them as Markdown.",
                        page_title: None,
                        address: Some(asked_for_display(asked)),
                    }
                }
            }
            TabState::Loaded { fetched } => {
                let page_title = fetched
                    .page_title()
                    .map(sanitize_title)
                    .filter(|title| !title.is_empty());
                if fetched.display_url.scheme() == "file" {
                    let path = resolver::display_string(&fetched.display_url);
                    SiteInfo {
                        title: "Local file",
                        detail: "Read from this device; nothing is sent over the network.",
                        page_title,
                        address: Some(bounded(&path)),
                    }
                } else {
                    SiteInfo {
                        title: "Secure connection",
                        detail:
                            "Fetched over HTTPS and rendered as Markdown; no scripts, no cookies.",
                        page_title,
                        // The LANDED URL, byte for byte: the fetch path
                        // already stripped any userinfo before display
                        // (docs/SECURITY.md), and re-sanitizing here would
                        // rewrite a nested URL inside the query.
                        address: Some(bounded(fetched.display_url.as_str())),
                    }
                }
            }
            TabState::Failed { asked, .. } => {
                if self.is_local_file() {
                    SiteInfo {
                        title: "Local file not loaded",
                        detail: "Nothing is rendered; the error page lists what was tried.",
                        page_title: None,
                        address: Some(asked_local_for_display(asked)),
                    }
                } else {
                    SiteInfo {
                        title: "Page not loaded",
                        detail: "Nothing is rendered; the error page lists every attempt.",
                        page_title: None,
                        address: Some(asked_for_display(asked)),
                    }
                }
            }
        }
    }
}

/// Display form of one REQUESTED address (user input, possibly a failed
/// paste): credentials stripped with the shared omnibox/error-view
/// sanitizer and bounded.
fn asked_for_display(asked: &str) -> String {
    bounded(&client::sanitize_for_display(asked))
}

/// Display form of one requested LOCAL address: the decoded path (what the
/// loaded state and the address field show) when the text still parses,
/// else the sanitized text.
fn asked_local_for_display(asked: &str) -> String {
    client::parse_navigable(asked)
        .map(|url| resolver::display_string(&url))
        .map_or_else(|_| asked_for_display(asked), |path| bounded(&path))
}

/// Bounds one address for the popup (`config::SITE_INFO_ADDRESS_CHARS`).
fn bounded(address: &str) -> String {
    truncate_chars(address, config::SITE_INFO_ADDRESS_CHARS).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{super::tests::fetched_for, Tab, TabState};

    /// A loaded HTTPS page reports its real identity: the sanitized
    /// frontmatter title and the LANDED URL, scheme included.
    #[test]
    fn loaded_https_page_reports_title_and_landed_url() {
        let mut tab = Tab::new(1);
        let mut fetched = fetched_for("https://ex.com/page/");
        fetched.meta.title = Some("  My \n Page  ".to_string());
        tab.state = TabState::Loaded { fetched };
        let info = tab.site_info();
        assert_eq!(info.title, "Secure connection");
        assert_eq!(info.page_title.as_deref(), Some("My Page"));
        assert_eq!(info.address.as_deref(), Some("https://ex.com/page/"));
        assert!(info.detail.contains("HTTPS"));
    }

    /// A local file shows its decoded path and the local wording.
    #[test]
    fn local_file_reports_the_decoded_path() {
        let mut tab = Tab::new(1);
        tab.state = TabState::Loaded {
            fetched: fetched_for("file:///tmp/notes.md"),
        };
        let info = tab.site_info();
        assert_eq!(info.title, "Local file");
        assert_eq!(info.address.as_deref(), Some("/tmp/notes.md"));
        assert!(info.detail.contains("this device"));
    }

    /// Internal pages and Home describe themselves; Home has no address.
    #[test]
    fn internal_pages_describe_themselves() {
        let home = Tab::new(1).site_info();
        assert_eq!(home.title, "New tab page");
        assert!(home.address.is_none());

        let mut history = Tab::new(2);
        history.open_history(true);
        let info = history.site_info();
        assert_eq!(info.title, "Local history");
        assert_eq!(info.address.as_deref(), Some("about:history"));

        let mut licenses = Tab::new(3);
        licenses.open_licenses(true);
        let info = licenses.site_info();
        assert_eq!(info.title, "Third-party licenses");
        assert_eq!(info.address.as_deref(), Some("about:licenses"));
    }

    /// A `Loading` state with an already-resolved (never polled) promise.
    fn loading(asked: &str) -> TabState {
        TabState::Loading {
            promise: poll_promise::Promise::from_ready(Err(crate::error::BrowserError::Cancelled)),
            asked: asked.to_string(),
            address_at_start: String::new(),
            started: std::time::Instant::now(),
            backdrop: None,
            saved_back: Vec::new(),
            saved_forward: Vec::new(),
            pushed_history: false,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Loading and failed states name the requested address, with
    /// credentials stripped from a pasted one.
    #[test]
    fn requested_addresses_never_echo_credentials() {
        let mut tab = Tab::new(1);
        tab.state = TabState::Failed {
            asked: "https://user:pass@ex.com/x".to_string(),
            lines: Vec::new(),
        };
        let info = tab.site_info();
        assert_eq!(info.title, "Page not loaded");
        assert_eq!(info.address.as_deref(), Some("https://ex.com/x"));

        let mut tab = Tab::new(2);
        tab.state = loading("https://user:pass@ex.com/y");
        let info = tab.site_info();
        assert_eq!(info.title, "Loading over HTTPS");
        assert_eq!(info.address.as_deref(), Some("https://ex.com/y"));
    }

    /// A local failure uses the local wording and the decoded path (like
    /// the loaded state and the address field), while an absolute-path
    /// rejection keeps the web wording like the indicator does.
    #[test]
    fn failed_states_keep_the_indicator_family() {
        let mut local = Tab::new(1);
        local.state = TabState::Failed {
            asked: "file:///tmp/missing.md".to_string(),
            lines: Vec::new(),
        };
        let info = local.site_info();
        assert_eq!(info.title, "Local file not loaded");
        assert_eq!(info.address.as_deref(), Some("/tmp/missing.md"));

        let mut loading_local = Tab::new(2);
        loading_local.state = loading("file:///tmp/notes.md");
        let info = loading_local.site_info();
        assert_eq!(info.title, "Loading local file");
        assert_eq!(info.address.as_deref(), Some("/tmp/notes.md"));

        let mut web = Tab::new(3);
        web.state = TabState::Failed {
            asked: "https://ex.com/missing".to_string(),
            lines: Vec::new(),
        };
        assert_eq!(web.site_info().title, "Page not loaded");
    }

    /// Remote-controlled titles are sanitized (no control/bidi spoofing)
    /// and the address is bounded.
    #[test]
    fn remote_title_and_address_are_sanitized_and_bounded() {
        let mut tab = Tab::new(1);
        let mut fetched = fetched_for("https://ex.com/a/");
        fetched.meta.title = Some("Evil\u{202E}txt\u{0007}".to_string());
        tab.state = TabState::Loaded { fetched };
        let info = tab.site_info();
        assert_eq!(info.page_title.as_deref(), Some("Eviltxt"));

        let long = format!("https://ex.com/{}", "a".repeat(400));
        let mut tab = Tab::new(2);
        tab.state = TabState::Failed {
            asked: long,
            lines: Vec::new(),
        };
        let address = tab.site_info().address.expect("address");
        assert!(address.chars().count() <= crate::config::SITE_INFO_ADDRESS_CHARS);
    }
}
