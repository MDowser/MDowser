// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the parent module (kept out of the logic file so the
//! module stays reviewable).

use super::{fetch_one, global_client, parse_https, run_network};
use crate::config;
use std::sync::atomic::AtomicBool;

/// Runs ONE document attempt (`fetch_one`) through the production
/// client on the shared runtime, blocking the test thread.
fn fetch_one_sync(url: &str) -> Result<(String, url::Url), String> {
    let client = global_client().unwrap().clone();
    let url = parse_https(url).unwrap();
    run_network(async move {
        let cancel = AtomicBool::new(false);
        fetch_one(&client, &url, false, &cancel)
            .await
            .map(|(text, final_url, _headers)| (text, final_url))
            .map_err(|e| e.line)
    })
    .expect("shared network runtime")
}

#[test]
fn rejects_non_https() {
    assert!(parse_https("http://ex.com/").is_err());
    assert!(parse_https("mailto:a@b.com").is_err());
    assert!(parse_https("data:text/plain,hello").is_err());
    assert!(parse_https("ftp://ex.com/").is_err());
}

/// `parse_navigable` accepts local files and HTTPS, and still blocks
/// every other scheme (including remote `file://host/` shares).
#[test]
fn navigable_accepts_files_and_https_but_not_remote_hosts() {
    use super::parse_navigable;
    assert_eq!(
        parse_navigable("example.com/love").unwrap().scheme(),
        "https"
    );
    assert_eq!(parse_navigable("/tmp/notes.md").unwrap().scheme(), "file");
    assert_eq!(
        parse_navigable("file:///tmp/notes.md").unwrap().scheme(),
        "file"
    );
    assert!(parse_navigable("file://server/share/x.md").is_err());
    assert!(parse_navigable("http://ex.com/").is_err());
    assert!(parse_navigable("data:text/plain,hi").is_err());
    assert!(parse_navigable("javascript:alert(1)").is_err());
}

#[test]
fn accepts_schemeless_as_https() {
    let u = parse_https("example.com/love").unwrap();
    assert_eq!(u.scheme(), "https");
    assert_eq!(u.host_str(), Some("example.com"));
}

#[test]
fn accepts_host_port_without_scheme() {
    // BUG: this used to reject as "non-https" (`example.com` looked like a scheme).
    let u = parse_https("example.com:8080/love").unwrap();
    assert_eq!(u.scheme(), "https");
    assert_eq!(u.port(), Some(8080));
    let u = parse_https("localhost:3000/docs").unwrap();
    assert_eq!(u.host_str(), Some("localhost"));
    assert_eq!(u.port(), Some(3000));
}

#[test]
fn mailto_is_not_userinfo_but_userinfo_hosts_work() {
    // `mailto:a@b` must stay blocked even though it looks like userinfo.
    assert!(parse_https("mailto:a@b.com").is_err());
    assert!(parse_https("tel:+123").is_err());
    // `user:pass@host` pastes as a host: credentials are stripped, never sent.
    let u = parse_https("user:pass@example.com/x").unwrap();
    assert_eq!(u.host_str(), Some("example.com"));
    assert!(u.username().is_empty());
    assert!(u.password().is_none());
}

#[test]
fn rejects_bare_emails_instead_of_navigating_to_the_domain() {
    assert!(parse_https("john@example.com").is_err());
    assert!(parse_https("john.doe@example.co.uk").is_err());
    // Host with port and userinfo forms keep working.
    assert_eq!(
        parse_https("example.com:8080/x").unwrap().port(),
        Some(8080)
    );
    assert!(parse_https("user:pass@example.com/x").is_ok());
    // A colon (userinfo) or a path is not an email.
    assert!(parse_https("user@example.com/path").is_ok());
}

#[test]
fn accepts_protocol_relative_input() {
    let u = parse_https("//example.com/love").unwrap();
    assert_eq!(u.scheme(), "https");
    assert_eq!(u.host_str(), Some("example.com"));
    assert_eq!(u.path(), "/love");
}

#[test]
fn rejects_oversized_input() {
    let big = "a".repeat(config::MAX_URL_INPUT_CHARS + 1);
    assert!(parse_https(&big).is_err());
}

#[test]
fn blocks_schemes_with_an_authority_even_with_userinfo() {
    // BUG: `http://user:pass@host` was read as a host with userinfo and
    // sent to `https://http//user:pass@host`, leaking the credentials
    // into the request path, the address bar and the route log.
    assert!(parse_https("http://user:pass@example.com/x").is_err());
    assert!(parse_https("ftp://u:p@example.com/").is_err());
    assert!(parse_https("javascript:alert(1)").is_err());
    assert!(parse_https("data:text/plain,hello").is_err());
    // One slash is scheme syntax too: `http:/user:pass@host` used to be
    // read as host `http` with the credentials folded into the request
    // path (`@` after the first slash is NOT an authority userinfo).
    assert!(parse_https("http:/user:pass@example.com/x").is_err());
    assert!(parse_https("ftp:/u:p@example.com/").is_err());
    // The URL parser folds `\` into an authority separator for special
    // schemes, so a backslash spelling is scheme syntax too: it used to
    // bypass the block and land the credentials in the request path.
    assert!(parse_https(r"http:\user:pass@example.com/x").is_err());
    assert!(parse_https(r"http:\\user:pass@example.com/x").is_err());
    assert!(parse_https(r"ftp:\u:p@example.com/").is_err());
    // `https:\example.com/x` is accepted with the backslash normalized
    // away (never sent as part of the host/path).
    let u = parse_https(r"https:\example.com/x").unwrap();
    assert_eq!(u.host_str(), Some("example.com"));
    assert_eq!(u.path(), "/x");
    // `user:pass@host` (no scheme, no slash before the `@`) keeps
    // working, credentials stripped.
    let u = parse_https("user:pass@example.com/x").unwrap();
    assert_eq!(u.host_str(), Some("example.com"));
    assert!(u.username().is_empty());
    // `scheme:user:pass@host` (a SECOND colon before the `@`) is scheme
    // syntax, not a username: it used to be read as a userinfo host and
    // navigate to `https://host`, silently reinterpreting a spelling the
    // contract blocks.
    assert!(parse_https("ftp:user:pass@example.com/x").is_err());
    assert!(parse_https("http:user:pass@example.com/x").is_err());
    assert!(parse_https("gopher:u:p@example.com").is_err());
    // A userinfo username with colons in the PASSWORD keeps the one-colon
    // rule only for the user/password separator; the port form still works.
    let u = parse_https("user:pass@example.com:8080/x").unwrap();
    assert_eq!(u.host_str(), Some("example.com"));
    assert_eq!(u.port(), Some(8080));
    assert!(u.username().is_empty());
}

#[test]
fn accepts_single_label_host_with_port() {
    let u = parse_https("nas:5000/docs").unwrap();
    assert_eq!(u.scheme(), "https");
    assert_eq!(u.host_str(), Some("nas"));
    assert_eq!(u.port(), Some(5000));
    // Non-numeric tails stay blocked as schemes.
    assert!(parse_https("foo:bar").is_err());
}

/// Display sanitizer for local rejections: pasted passwords never reach
/// the omnibox, the error view or "Copy log" (docs/SECURITY.md contract).
#[test]
fn display_sanitizer_never_echoes_credentials() {
    use super::sanitize_for_display;
    // Schemeless paste with a password: the dangerous form `parse_https`
    // rejected while echoing it in the old code.
    assert_eq!(
        sanitize_for_display("user:pass@example.com:bad/"),
        "example.com:bad/"
    );
    // Schemed form: delegated to the shared log sanitizer.
    assert_eq!(
        sanitize_for_display("ftp://u:p@example.com/x"),
        "ftp://example.com/x"
    );
    // Blocked non-hierarchical scheme: `mailto:` is not a password and
    // stripping it would show a misleading bare domain.
    assert_eq!(sanitize_for_display("mailto:a@b.com"), "mailto:a@b.com");
    // ...but a password after the scheme colon is credential-shaped
    // input: only the domain survives.
    assert_eq!(
        sanitize_for_display("mailto:user:pass@b.com"),
        "b.com",
        "a pasted password must never be echoed"
    );
    // A bare `user@host` carries no secret.
    assert_eq!(
        sanitize_for_display("user@example.com/x"),
        "user@example.com/x"
    );
    // Ordinary rejected input is left as-is for context.
    assert_eq!(
        sanitize_for_display("example.com/a?q=1"),
        "example.com/a?q=1"
    );
    // Malformed authority spellings (one slash / extra slashes) used to
    // slip past both sanitizers; the password must never survive.
    assert_eq!(
        sanitize_for_display("http:/user:pass@example.com/x"),
        "http:/example.com/x"
    );
    assert_eq!(
        sanitize_for_display("http:///user:pass@example.com/x"),
        "http:///example.com/x"
    );
    // The NO-SEPARATOR spelling `scheme:user:pass@host` (blocked by
    // `parse_https` as scheme syntax) used to echo the whole password into
    // the error view and "Copy log".
    assert_eq!(
        sanitize_for_display("http:user:pass@example.com/x"),
        "http:example.com/x"
    );
    assert_eq!(
        sanitize_for_display("HTTPS only, blocked: http:user:secret@example.com"),
        "HTTPS only, blocked: http:example.com",
        "a whole error line must lose the password too"
    );
    // A pasted password may itself contain a `/`, `?` or `#`: the old
    // authority cut hid the `@` beyond it and echoed the password.
    assert_eq!(
        sanitize_for_display("user:pa/ss@host"),
        "host",
        "a password containing a slash must never be echoed"
    );
    assert_eq!(sanitize_for_display("mailto:user:pa/ss@b.com"), "b.com");
    assert_eq!(sanitize_for_display("nas:5000/x@y"), "nas:5000/x@y");
    // A host:port-shaped token the URL parser REJECTS (port out of range)
    // is not echoed verbatim: the password inside it is stripped.
    assert_eq!(
        sanitize_for_display("alice:99999/secret@example.com"),
        "example.com"
    );
    // The display echo stops at the token bound: text from a second pasted
    // line never rides along with an @-bearing token.
    assert_eq!(
        sanitize_for_display("mailto:user:pass@b.com\nhello"),
        "b.com"
    );
    assert_eq!(sanitize_for_display("a:b/c@d\nhello"), "d");
    // Path/query `@`s and host:port segments are not credentials: the
    // token stays readable.
    assert_eq!(sanitize_for_display("https://host/a@b"), "https://host/a@b");
    assert_eq!(
        sanitize_for_display("C:\\Users\\me\\a@b.md"),
        "C:\\Users\\me\\a@b.md"
    );
}

/// `finish_document` derives the first-H1 title fallback once, next to the
/// copy text and the anchors; a declared frontmatter `title` always wins,
/// and an EMPTY one falls through to the heading.
#[test]
fn finish_document_derives_the_first_h1_title() {
    let url = url::Url::parse("https://ex.com/a/").unwrap();
    let fetched =
        super::finish_document("Intro.\n\n# Carla\n\nBody.", url.clone(), &url).expect("parse");
    assert_eq!(fetched.h1_title.as_deref(), Some("Carla"));
    assert_eq!(fetched.page_title(), Some("Carla"));
    assert!(fetched.meta.title.is_none());

    // A frontmatter title wins; the heading still arrives as the fallback.
    let fetched = super::finish_document("---\ntitle: Meta\n---\n# Carla\n", url.clone(), &url)
        .expect("parse");
    assert_eq!(fetched.meta.title.as_deref(), Some("Meta"));
    assert_eq!(fetched.page_title(), Some("Meta"));
    assert_eq!(fetched.h1_title.as_deref(), Some("Carla"));

    // `title: ""` is "no title": the heading names the page.
    let fetched = super::finish_document("---\ntitle: \"\"\n---\n# Carla\n", url.clone(), &url)
        .expect("parse");
    assert_eq!(fetched.page_title(), Some("Carla"));
}

// -----------------------------------------------------------------------
// Redirect contract, network smoke tests (ignored by default).
//
// 301/302/303/307/308 are followed automatically by `reqwest` (relative
// `Location` included) up to `config::MAX_REDIRECTS`; the FINAL response
// still has to be markdown; a `Location` that leaves https is rejected;
// and the landed URL is the one the UI shows. Run manually with:
// `cargo test -- --ignored network_redirect --nocapture`
// -----------------------------------------------------------------------

/// A real 302 chain that ends in markdown is followed, and the value
/// handed to the UI is where the request LANDED, not what was asked.
#[test]
#[ignore = "requires network access"]
fn network_redirect_follows_to_markdown() {
    // unpkg answers `302` with a relative Location for the unversioned
    // path; the target serves `text/markdown`.
    let (text, landed) =
        fetch_one_sync("https://unpkg.com/left-pad/README.md").expect("302 -> markdown");
    assert_eq!(
        landed.as_str(),
        "https://unpkg.com/left-pad@1.3.0/README.md"
    );
    assert!(text.contains("left-pad"), "unexpected body");
}

/// The hop cap bounds a chain: `MAX_REDIRECTS` hops fit, one more does
/// not.
#[test]
#[ignore = "requires network access"]
fn network_redirect_hop_cap_stops_long_chains() {
    // 8 hops complete, so the failure is the markdown gate rejecting
    // the JSON body, NOT the redirect layer.
    let err = fetch_one_sync("https://httpbingo.org/redirect/8").unwrap_err();
    assert!(err.contains("application/json"), "{err}");
    // 9 hops exceed the cap: the redirect layer stops the chain.
    let err = fetch_one_sync("https://httpbingo.org/redirect/9").unwrap_err();
    assert!(err.contains("error following redirect"), "{err}");
}

/// A redirect that leaves https is rejected: the http target is never
/// requested (never HTTP).
#[test]
#[ignore = "requires network access"]
fn network_redirect_off_https_is_rejected() {
    let err = fetch_one_sync(
        "https://httpbingo.org/redirect-to?url=http%3A%2F%2Fexample.com%2F&status_code=302",
    )
    .unwrap_err();
    assert!(err.contains("error following redirect"), "{err}");
}
