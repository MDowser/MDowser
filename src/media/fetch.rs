// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `media::fetch`: background download + decode of image subresources.
//!
//! - Remote images go over HTTPS through the shared `protocol::client` pool
//!   (no new TLS stacks, no cookies, no proxy). Images referenced by a LOCAL
//!   document are read from disk (`protocol::local`) with the same cap and
//!   cancel contract and are decoded through the same sniffing pipeline.
//! - The `Content-Type` is validated first: a present, non-image type
//!   (`text/html`, `application/pdf`, ...) is rejected before decoding.
//!   Generic `application/octet-stream` / `binary/octet-stream` (the
//!   object-store default) and a missing header are allowed because the
//!   magic-byte sniff + decoder still validate the payload.
//! - Cooperative cancellation reuses the document fetch contract.
//! - The async work runs on the shared process-wide runtime
//!   (`protocol::client::run_network`), never on a per-job runtime: the
//!   HTTP client's pooled connections must stay attached to a long-lived
//!   executor or later requests fail with `error sending request`.
//! - Images use their own total timeout (`IMAGE_REQUEST_TIMEOUT_SECS`,
//!   longer than the document one: binary payloads are bigger) while the
//!   per-chunk stall timeout keeps a stuck connection from lingering.
//! - Every public entry point is panic-guarded (`util::guard_panics`): a
//!   decoder panic becomes an error placeholder instead of killing the
//!   promise thread (which would panic the UI when polled).

use super::decode::{self, DecodedImage};
use crate::config;
use crate::protocol::client;
use crate::protocol::content_type::{is_image_content_type, is_octet_stream};
use crate::util::truncate_chars;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;

/// Downloads a binary resource with the shared capped, stall-bounded GET.
///
/// `accept` is the `Accept` header; `max_bytes` the anti-abuse cap; `timeout`
/// the per-request budget (documents own theirs, images a long one, favicon
/// probes a short one). Returns the raw bytes; image-type validation of the
/// header happens here, while format validation happens in `decode`.
pub(crate) async fn fetch_binary(
    url: &Url,
    accept: &str,
    max_bytes: usize,
    timeout: std::time::Duration,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let client = client::global_client().map_err(|e| e.to_string())?.clone();
    let gate = |cts: &[String]| image_type_gate(url, cts);
    // Sanitize like the document path: reqwest errors can embed a server
    // redirect URL with `user:pass@`, and image failure reasons reach the
    // UI as tooltips (`media::cache` -> `viewer::image`).
    let raw = client::get_bytes(&client, url, accept, max_bytes, timeout, &gate, cancel)
        .await
        .map_err(|e| crate::util::strip_credentials_for_log(&e))?;
    Ok(raw.bytes)
}

/// Header gate for image/favicon subresources: a declared concrete non-image
/// type is rejected BEFORE the body is read (never treat HTML/JS as a
/// possible image). Generic `application/octet-stream` and a missing header
/// are allowed: the magic-byte sniff + decoder still validate the payload.
/// A response naming BOTH an image and a concrete non-image type is rejected:
/// a misbehaving server gets no benefit from precedence games.
fn image_type_gate(url: &Url, content_types: &[String]) -> Result<(), String> {
    match content_types
        .iter()
        .find(|v| !is_image_content_type(Some(v)) && !is_octet_stream(v))
    {
        Some(found) => {
            let found = truncate_chars(found, config::MAX_LOG_LINE_CHARS);
            Err(format!(
                "access to {url} returned {found} (expected an image)"
            ))
        }
        None => Ok(()),
    }
}

/// Fetches + decodes one markdown image on a background thread.
///
/// Panic-guarded: image decoders (`image`, `jxl-oxide`, `avif-decode`) run
/// on this thread, and a panic would otherwise drop the promise sender and
/// panic the UI when polled (see `util::guard_panics`).
pub fn fetch_image_blocking(url: &str, cancel: Arc<AtomicBool>) -> Result<DecodedImage, String> {
    crate::util::guard_panics(move || fetch_image_blocking_inner(url, cancel)).and_then(|r| r)
}

/// Unguarded implementation of [`fetch_image_blocking`].
fn fetch_image_blocking_inner(url: &str, cancel: Arc<AtomicBool>) -> Result<DecodedImage, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    let parsed = client::parse_navigable(url).map_err(|e| e.to_string())?;
    let bytes = if crate::protocol::local::is_file_url(&parsed) {
        // Local image referenced by a local document: read from disk with
        // the image cap. There is no Content-Type header; the magic-byte
        // sniff + decoder validate the payload exactly like a remote
        // `application/octet-stream`.
        crate::protocol::local::read_bytes(&parsed, config::MAX_IMAGE_BYTES, &cancel)?
    } else {
        let job_cancel = cancel.clone();
        client::run_network(async move {
            fetch_binary(
                &parsed,
                config::IMAGE_ACCEPT_HEADER_VALUE,
                config::MAX_IMAGE_BYTES,
                std::time::Duration::from_secs(config::IMAGE_REQUEST_TIMEOUT_SECS),
                &job_cancel,
            )
            .await
        })??
    };
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    decode::decode(&bytes)
}

/// Probes the conventional favicon paths (`config::FAVICON_PATHS`) for
/// `origin` and decodes the first usable icon. All probes share one runtime
/// and one cancel flag.
/// Panic-guarded like [`fetch_image_blocking`].
pub fn fetch_favicon_blocking(
    origin: &Url,
    cancel: Arc<AtomicBool>,
) -> Result<DecodedImage, String> {
    crate::util::guard_panics(move || fetch_favicon_blocking_inner(origin, cancel)).and_then(|r| r)
}

/// Unguarded implementation of [`fetch_favicon_blocking`].
fn fetch_favicon_blocking_inner(
    origin: &Url,
    cancel: Arc<AtomicBool>,
) -> Result<DecodedImage, String> {
    // Defense in depth: probe only https origins. `get_bytes` refuses a
    // non-https LANDING url, but the initial request must already be https
    // regardless of the caller.
    if origin.scheme() != "https" {
        return Err("favicon probe requires https".to_string());
    }
    let origin = origin.clone();
    client::run_network(async move {
        for name in config::FAVICON_PATHS {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            let mut probe = origin.clone();
            probe.set_path(&format!("/{name}"));
            probe.set_query(None);
            probe.set_fragment(None);
            let bytes = match fetch_binary(
                &probe,
                config::FAVICON_ACCEPT_HEADER_VALUE,
                config::MAX_FAVICON_BYTES,
                // Icons are tiny and the probes run back-to-back inside ONE
                // image slot: the short per-probe budget keeps the worst
                // case (`FAVICON_PATHS.len()` probes) under the entry
                // deadline instead of pinning a slot for 3x60s.
                std::time::Duration::from_secs(config::FAVICON_REQUEST_TIMEOUT_SECS),
                &cancel,
            )
            .await
            {
                Ok(b) => b,
                // 404 / network / wrong type: try the next probe.
                Err(_) => continue,
            };
            // Undecodable bytes: try the next probe too.
            // `decode_with_max_dim`: icons draw at the toolbar icon size
            // (~27 px), so decoding a full 3072 px texture would only waste
            // VRAM (see `MAX_FAVICON_DIM`).
            if let Ok(img) = decode::decode_with_max_dim(&bytes, config::MAX_FAVICON_DIM) {
                return Ok(img);
            }
        }
        Err("no favicon".to_string())
    })?
}

#[cfg(test)]
mod tests {
    use super::{fetch_image_blocking, image_type_gate};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use url::Url;

    /// Header gate matrix: concrete non-image types are refused before the
    /// body is read, while generic/missing types pass to the magic sniff.
    #[test]
    fn image_content_type_gate_matrix() {
        let url = Url::parse("https://ex.com/i.bin").unwrap();
        let cts = |vals: &[&str]| vals.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(image_type_gate(&url, &cts(&["image/png"])).is_ok());
        assert!(image_type_gate(&url, &cts(&[])).is_ok());
        assert!(image_type_gate(&url, &cts(&["application/octet-stream"])).is_ok());
        assert!(image_type_gate(&url, &cts(&["binary/octet-stream"])).is_ok());
        let err = image_type_gate(&url, &cts(&["text/html"])).unwrap_err();
        assert!(err.contains("text/html"), "{err}");
        // A concrete non-image wins even next to a generic value.
        assert!(
            image_type_gate(&url, &cts(&["application/octet-stream", "application/pdf"])).is_err()
        );
    }

    /// Regression (network, ignored): a fresh `current_thread` runtime per
    /// job dropped the HTTP pool's background tasks, so several parallel
    /// images on one host failed with `error sending request` and some
    /// stayed broken after the single retry. All concurrent jobs run on the
    /// shared runtime now; every image must decode.
    #[test]
    #[ignore = "requires network access"]
    fn concurrent_images_share_the_network_runtime() {
        // Generic public SVG endpoint, one query per job so the URLs differ.
        let urls: Vec<String> = (0..8)
            .map(|n| format!("https://httpbingo.org/image/svg?job={n}"))
            .collect();
        let handles: Vec<_> = urls
            .iter()
            .map(|u| {
                let u = u.to_string();
                std::thread::spawn(move || {
                    let result = fetch_image_blocking(&u, Arc::new(AtomicBool::new(false)));
                    (u, result.map(|d| d.pixels.size).map_err(|e| e.to_string()))
                })
            })
            .collect();
        for h in handles {
            let (url, result) = h.join().expect("job thread");
            assert!(result.is_ok(), "{url}: {result:?}");
        }
    }

    /// Network smoke test (ignored by default): exercises the REAL
    /// fetch + Content-Type gate + decode pipeline on generic public image
    /// endpoints. ICO/AVIF decode is covered locally in `media::decode`
    /// tests, so this stays vendor-neutral. Run manually with:
    /// `cargo test -- --ignored network_images --nocapture`
    #[test]
    #[ignore = "requires network access"]
    fn network_images() {
        let cases = [
            // (url, decodes successfully?)
            ("https://httpbingo.org/image/png", true),
            ("https://httpbingo.org/image/jpeg", true),
            ("https://httpbingo.org/image/webp", true),
            // Static SVG rasterizes (icons, logos, badge images).
            ("https://httpbingo.org/image/svg", true),
        ];
        for (url, want_ok) in cases {
            let result =
                fetch_image_blocking(url, Arc::new(AtomicBool::new(false))).map(|d| d.pixels.size);
            assert_eq!(result.is_ok(), want_ok, "{url}: {result:?}");
        }
    }
}
