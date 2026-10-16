// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `protocol::local`: local Markdown documents (`file://` and OS paths).
//!
//! CONTRACT:
//! - Only reachable from user input (address bar, command line, OS "open
//!   with") and from links inside an ALREADY-LOCAL document. A remote
//!   (`https`) document can never reference the local filesystem:
//!   `protocol::rewrite` keeps `file:` destinations inert unless the base
//!   document itself is local.
//! - Only local paths: a `file://host/...` URL (a remote share) is rejected
//!   before any read.
//! - No directory listing: a directory resolves through its conventional
//!   Markdown indexes (`index.md`, `README.md`, ...), never by enumerating.
//! - Reads reuse the document anti-abuse caps (`MAX_BODY_BYTES` for
//!   documents, the media caps for images) and honor the cooperative cancel
//!   flag between chunks, like the network path.
//! - UTF-8 is mandatory for documents (same contract as `https` documents).
//! - No favicons and no network probe for local pages.

use crate::config;
use crate::error::BrowserError;
use crate::protocol::client::Fetched;
use crate::util::truncate_chars;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

/// Read chunk size (bytes): single source in [`config::LOCAL_READ_CHUNK_BYTES`].
const READ_CHUNK_BYTES: usize = config::LOCAL_READ_CHUNK_BYTES;

/// True when `url` is a local-file document URL.
pub(crate) fn is_file_url(url: &Url) -> bool {
    url.scheme() == "file"
}

/// True when a `file:` URL's authority names THIS device: empty/absent
/// (`file:///x`) or `localhost` (`file://localhost/x`). Any other host is a
/// remote filesystem share and is rejected. Shared with
/// `protocol::rewrite`, which must agree on what a local destination is.
pub(crate) fn is_local_host(url: &Url) -> bool {
    matches!(url.host_str(), None | Some("") | Some("localhost"))
}

/// Parses user input as a local target, when it is local-shaped.
///
/// Returns `None` for anything that should go through the HTTPS parser
/// (`example.com/x`), so `client::parse_navigable` stays the single entry
/// point. A local target is:
/// - `file:` with an absolute local path (host empty; `file://host/...` is
///   rejected as an unsupported remote share),
/// - an absolute path (`/Users/you/notes.md`),
/// - a home path (`~` / `~/notes.md`),
/// - an explicit relative path (`./notes.md`, `../notes.md`),
/// - a Windows drive path (`C:\notes.md`, `C:/notes.md`).
pub(crate) fn parse_input(input: &str) -> Option<Result<Url, BrowserError>> {
    // Allocation-free prefix test: the old whole-string lowercase made a
    // 8 KiB copy on every parse, even for plain URLs.
    if crate::util::starts_with_ignore_ascii_case(input, "file:") {
        return Some(parse_file_url(input));
    }
    if looks_like_path(input) {
        return Some(path_to_url(input));
    }
    None
}

/// True when the text is unambiguously a local filesystem path.
fn looks_like_path(s: &str) -> bool {
    if s == "~" || s.starts_with("~/") || s.starts_with("./") || s.starts_with("../") {
        return true;
    }
    // `//host/x` is protocol-relative HTTPS, never a local path.
    if s.starts_with('/') && !s.starts_with("//") {
        return true;
    }
    // Windows drive path (`C:\x`, `C:/x`). Gated so a Unix file literally
    // named `C:\x` is not misread as a path prefix on this platform.
    if cfg!(windows) {
        let b = s.as_bytes();
        if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            return true;
        }
    }
    false
}

/// Parses an explicit `file:` URL.
///
/// Accepts `file:///abs/path`, `file:/abs/path` and `file://localhost/x`
/// (the common spellings). Rejects a remote host (a filesystem share), a
/// port and any credentials: this browser only ever opens files on THIS
/// device.
pub(crate) fn parse_file_url(input: &str) -> Result<Url, BrowserError> {
    let url = Url::parse(input).map_err(|e| BrowserError::BadUrl(e.to_string()))?;
    if !is_local_host(&url)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(BrowserError::BadUrl(format!(
            "file URL with a host is not supported: {}",
            crate::util::safe_input_echo(input)
        )));
    }
    if url.path().is_empty() || url.path() == "/" {
        return Err(BrowserError::BadUrl(format!(
            "no local path: {}",
            crate::util::safe_input_echo(input)
        )));
    }
    // The path must be absolute: the WHATWG parser rebases a relative
    // spelling (`file:notes.md`) onto the filesystem ROOT, so the check has
    // to read the ORIGINAL text (`url.path()` is already `/notes.md`).
    // Only `file:` followed by a slash is an absolute local target.
    let absolute_spelling = input
        .find(':')
        .map(|colon| input[colon + 1..].starts_with('/'))
        .unwrap_or(false);
    if !absolute_spelling || url.to_file_path().is_err() {
        return Err(BrowserError::BadUrl(format!(
            "not a local path: {}",
            crate::util::safe_input_echo(input)
        )));
    }
    Ok(url)
}

/// Converts an explicit path spelling (`/x`, `~/x`, `./x`, `C:\x`) to a
/// `file://` URL with a lexically normalized absolute path.
fn path_to_url(input: &str) -> Result<Url, BrowserError> {
    let expanded = if input == "~" || input.starts_with("~/") {
        let home = crate::util::home_dir()
            .ok_or_else(|| BrowserError::BadUrl("cannot resolve '~': no home directory".into()))?;
        if input == "~" {
            home
        } else {
            home.join(&input[2..])
        }
    } else {
        PathBuf::from(input)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|e| BrowserError::BadUrl(format!("no current directory: {e}")))?
            .join(expanded)
    };
    let normalized = normalize_path(&absolute);
    Url::from_file_path(&normalized).map_err(|_| {
        BrowserError::BadUrl(format!(
            "not a local path: {}",
            crate::util::safe_input_echo(input)
        ))
    })
}

/// Lexically normalizes `.` and `..` components without touching the
/// filesystem (no symlink resolution: the path the user typed stays visible).
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // At the root (or a Windows prefix) `..` cannot go higher.
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Bounded, credential-stripped echo of a parsed `file:` URL for error lines.
///
/// Mirrors the network path (`client::fetch` strips at every log sink):
/// `parse_file_url` already rejects userinfo upstream, but these sinks stay
/// safe even if a credentialed URL ever reaches them.
fn echo_url(url: &Url) -> String {
    crate::util::safe_input_echo(url.as_str())
}

/// Local path of a `file` URL, rejecting remote hosts.
///
/// The error echo is built INSIDE each failure arm: a successful read used
/// to allocate two full sanitized copies of the URL per document (and per
/// image) purely for messages that are never shown.
pub(crate) fn local_path(url: &Url) -> Result<PathBuf, String> {
    if !is_local_host(url) {
        let host = url.host_str().unwrap_or_default();
        return Err(format!(
            "access to {}: remote file host '{host}' is not supported",
            echo_url(url)
        ));
    }
    url.to_file_path()
        .map_err(|_| format!("access to {} is not a local path", echo_url(url)))
}

/// Candidate `(url, path)` pairs for a file URL, in resolution order.
///
/// - Existing file with a Markdown extension: itself.
/// - Existing file with another/no extension: itself first, then the `.md`
///   sibling (typing the stem finds `notes.md`).
/// - Existing directory (or a path ending in `/`): conventional indexes only.
fn candidates_for(start: &Url) -> Result<Vec<(Url, PathBuf)>, String> {
    let path = local_path(start)?;
    let mut out: Vec<(Url, PathBuf)> = Vec::new();
    let mut push = |p: PathBuf| {
        if let Ok(u) = Url::from_file_path(&p) {
            out.push((u, p));
        }
    };
    let is_dir = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
    let wants_dir = is_dir || start.path().ends_with('/');
    if wants_dir {
        for name in crate::config::DIR_INDEXES {
            push(path.join(name));
        }
        return Ok(out);
    }
    push(path.clone());
    let lower_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(lower_ext.as_deref(), Some("md") | Some("markdown")) {
        let mut name = path.clone().into_os_string();
        name.push(".md");
        push(PathBuf::from(name));
    }
    Ok(out)
}

/// Reads one local document candidate as UTF-8 text.
fn read_document(url: &Url, path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let bytes = read_bytes_at(url, path, config::MAX_BODY_BYTES, cancel)?;
    // `String::from_utf8` takes the owned body: no second full-body copy.
    String::from_utf8(bytes).map_err(|e| {
        BrowserError::InvalidUtf8 {
            url: url.to_string(),
            detail: e.utf8_error().to_string(),
        }
        .to_string()
    })
}

/// Reads one local file with a hard byte cap and cooperative cancellation.
///
/// Shared by documents and image/favicon subresources. Error strings mirror
/// the network path's wording, so the route log reads the same either way.
pub(crate) fn read_bytes(url: &Url, cap: usize, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let path = local_path(url)?;
    read_bytes_at(url, &path, cap, cancel)
}

/// Opens `path` for reading without ever blocking on the open itself.
///
/// The `metadata` check in [`read_bytes_at`] rejects FIFOs, but a local
/// attacker can swap one in between that check and `open`; `open` on a FIFO
/// with no writer then blocks forever (no timeout can interrupt it) and
/// pins one of only `MAX_CONCURRENT_LOCAL_READS` slots. `O_NONBLOCK` makes that
/// open return immediately; the opened-handle check rejects the FIFO before
/// any read. Reads from regular files ignore the flag.
#[cfg(unix)]
fn open_read_no_block(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// Non-Unix fallback: FIFOs are a Unix concept; `open` cannot block on one.
#[cfg(not(unix))]
fn open_read_no_block(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// [`read_bytes`] on an explicit path (candidate walking already has it).
fn read_bytes_at(
    url: &Url,
    path: &Path,
    cap: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    // Lazy echo: built only inside a failure arm (a successful read pays
    // nothing for messages that are never shown).
    let echo = || echo_url(url);
    if cancel.load(Ordering::Relaxed) {
        return Err(format!("access to {} cancelled", echo()));
    }
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("access to {} does not exist (404)", echo()));
        }
        Err(e) => return Err(format!("access to {}: {e}", echo())),
    };
    if meta.is_dir() {
        return Err(format!("access to {} is a directory", echo()));
    }
    // Only regular files are readable: a FIFO with no writer blocks `open`
    // forever (no timeout can interrupt it) and would permanently consume a
    // global fetch slot; sockets/devices have no business being a document.
    // `metadata` follows symlinks, so links to regular files still pass.
    if !meta.is_file() {
        return Err(format!("access to {} is not a regular file", echo()));
    }
    // Early cutoff on the metadata length (cheap anti-abuse, no read).
    if meta.len() > cap as u64 {
        return Err(format!("access to {} exceeds {cap} bytes", echo()));
    }
    let mut file = open_read_no_block(path).map_err(|e| format!("access to {}: {e}", echo()))?;
    // Re-check the OPENED handle's type: if the path was swapped between
    // `metadata` and `open`, the opened handle is what we actually read, and
    // `O_NONBLOCK` kept the open itself from blocking on a FIFO.
    match file.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(format!("access to {} is not a regular file", echo())),
        Err(e) => return Err(format!("access to {}: {e}", echo())),
    }
    let mut buf: Vec<u8> = Vec::with_capacity((meta.len() as usize).min(cap));
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(format!("access to {} cancelled", echo()));
        }
        let n = file
            .read(&mut chunk)
            .map_err(|e| format!("access to {} error reading file: {e}", echo()))?;
        if n == 0 {
            break;
        }
        // Running cap: the metadata check above can race a growing file.
        if buf.len().saturating_add(n) > cap {
            return Err(format!("access to {} exceeds {cap} bytes", echo()));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

/// Resolves a local file URL by trying candidates in order, with the same
/// `Fetched` contract (YAML frontmatter, parsed events, copy text, anchors)
/// as a network document.
///
/// A content error on one candidate (bad YAML, parse budget) does NOT end
/// the cascade: candidates are independent files (a directory's `index.md`
/// and `README.md` are different documents), so the next one is tried
/// exactly like the network path's route fallback. Every error is kept for
/// the final line-by-line log.
pub(crate) fn fetch_blocking(start: &Url, cancel: &AtomicBool) -> Result<Fetched, BrowserError> {
    let candidates = match candidates_for(start) {
        Ok(c) => c,
        Err(e) => return Err(BrowserError::BadUrl(e)),
    };
    let mut lines: Vec<String> = Vec::new();
    for (url, path) in candidates {
        if cancel.load(Ordering::Relaxed) {
            return Err(BrowserError::Cancelled);
        }
        match read_document(&url, &path, cancel) {
            Ok(text) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(BrowserError::Cancelled);
                }
                match super::client::finish_document(&text, url.clone(), &url) {
                    Ok(fetched) => return Ok(fetched),
                    Err(line) => {
                        lines
                            .push(truncate_chars(&line, config::MAX_ERROR_LINE_CHARS).into_owned());
                    }
                }
            }
            Err(line) => {
                lines.push(truncate_chars(&line, config::MAX_ERROR_LINE_CHARS).into_owned());
            }
        }
    }
    Err(BrowserError::NoCandidateWorked {
        asked: start.to_string(),
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::{candidates_for, is_file_url, local_path, normalize_path, parse_input, read_bytes};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use url::Url;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn explicit_file_urls_parse_and_reject_remote_hosts() {
        let ok = parse_input("file:///tmp/notes.md").unwrap().unwrap();
        assert_eq!(ok.scheme(), "file");
        assert_eq!(ok.path(), "/tmp/notes.md");
        // `file:/tmp` (no authority) is the same local file.
        let ok = parse_input("file:/tmp/notes.md").unwrap().unwrap();
        assert_eq!(ok.path(), "/tmp/notes.md");
        // A remote share is rejected before any read.
        assert!(parse_input("file://server/share/notes.md")
            .unwrap()
            .is_err());
        assert!(parse_input("file:///").unwrap().is_err());
        // A RELATIVE `file:` spelling is rejected instead of being silently
        // rebased onto the filesystem root (`file:notes.md` -> `/notes.md`).
        assert!(parse_input("file:notes.md").unwrap().is_err());
        assert!(parse_input("file:./notes.md").unwrap().is_err());
    }

    #[test]
    fn absolute_and_home_paths_become_file_urls() {
        let url = parse_input("/tmp/notes.md").unwrap().unwrap();
        assert!(is_file_url(&url));
        assert_eq!(url.path(), "/tmp/notes.md");
        // Relative paths are explicit about their relative nature.
        let url = parse_input("./notes.md").unwrap().unwrap();
        assert!(url.path().ends_with("/notes.md"), "{}", url.path());
        // A bare hostname is NEVER a path: it stays HTTPS.
        assert!(parse_input("example.com/notes.md").is_none());
        // Protocol-relative URLs stay HTTPS too.
        assert!(parse_input("//example.com/notes.md").is_none());
    }

    #[test]
    fn path_normalization_keeps_the_path_lexical() {
        assert_eq!(
            normalize_path(&PathBuf::from("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(normalize_path(&PathBuf::from("/../a")), PathBuf::from("/a"));
    }

    #[test]
    fn candidates_prefer_indexes_for_directories() {
        let dir = std::env::temp_dir();
        let url = Url::from_directory_path(&dir).unwrap();
        let cands = candidates_for(&url).unwrap();
        assert_eq!(cands.len(), 2, "indexes only, no listing");
        assert!(cands[0].1.ends_with("index.md"));
        assert!(cands.iter().all(|(u, _)| u.scheme() == "file"));
    }

    #[test]
    fn candidates_complete_a_missing_extension() {
        let dir = std::env::temp_dir().join("mdowser-local-tests");
        let _ = std::fs::create_dir_all(&dir);
        let url = Url::from_file_path(dir.join("stem")).unwrap();
        let cands = candidates_for(&url).unwrap();
        let names: Vec<String> = cands
            .iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["stem".to_string(), "stem.md".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_path_rejects_hosts() {
        assert!(local_path(&u("file://server/x.md")).is_err());
        assert!(local_path(&u("file:///x.md")).is_ok());
    }

    // -----------------------------------------------------------------------
    // End-to-end: reading real files through the production fetch entry.
    // -----------------------------------------------------------------------

    use crate::protocol::client::fetch_blocking;
    use std::path::Path;
    use std::sync::Arc;

    /// Fresh, unique scratch directory per test.
    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdowser-local-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fetch_reads_a_local_document_with_frontmatter() {
        let dir = tmp_dir("doc");
        let path = dir.join("notes.md");
        std::fs::write(
            &path,
            "---\ntitle: Notas\n---\n\n# Hola\n\n[web](https://ex.com/x)\n",
        )
        .unwrap();
        let url = Url::from_file_path(&path).unwrap();
        let fetched = fetch_blocking(&url, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(fetched.display_url, url, "the exact file path survives");
        assert_eq!(fetched.meta.title.as_deref(), Some("Notas"));
        assert!(fetched.plain_text.contains("Hola"));
        cleanup(&dir);
    }

    #[test]
    fn fetch_resolves_a_directory_index() {
        let dir = tmp_dir("dir");
        std::fs::write(dir.join("README.md"), "# Readme\n").unwrap();
        let url = Url::from_directory_path(&dir).unwrap();
        let fetched = fetch_blocking(&url, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(
            fetched.display_url.path().ends_with("/README.md"),
            "{}",
            fetched.display_url
        );
        assert!(fetched.plain_text.contains("Readme"));
        cleanup(&dir);
    }

    #[test]
    fn fetch_completes_a_missing_extension() {
        let dir = tmp_dir("ext");
        std::fs::write(dir.join("notes.md"), "# From stem\n").unwrap();
        let url = Url::from_file_path(dir.join("notes")).unwrap();
        let fetched = fetch_blocking(&url, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(fetched.display_url.path().ends_with("notes.md"));
        cleanup(&dir);
    }

    /// A content error on one candidate (malformed YAML frontmatter) must
    /// NOT end the cascade: the next directory index is tried, exactly like
    /// the network route fallback. The old code returned the first
    /// candidate's content error, so a broken `index.md` hid a perfectly
    /// readable `README.md`.
    #[test]
    fn fetch_falls_through_a_broken_candidate_to_the_next() {
        let dir = tmp_dir("fallthrough");
        // Malformed YAML: `[unclosed` never parses, so the frontmatter
        // split fails for this candidate only.
        std::fs::write(
            dir.join("index.md"),
            "---\ntitle: [unclosed\n---\n\n# Broken\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# Readme\n").unwrap();
        let url = Url::from_directory_path(&dir).unwrap();
        let fetched = fetch_blocking(&url, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(
            fetched.display_url.path().ends_with("/README.md"),
            "the readable sibling wins: {}",
            fetched.display_url
        );
        assert!(fetched.plain_text.contains("Readme"));
        cleanup(&dir);
    }

    /// A missing file reports the network-style 404 line so the error view
    /// classifies it as "not found" for local pages too.
    #[test]
    fn fetch_reports_a_missing_file_per_candidate() {
        let dir = tmp_dir("missing");
        let url = Url::from_file_path(dir.join("nope.md")).unwrap();
        let err = fetch_blocking(&url, Arc::new(AtomicBool::new(false))).unwrap_err();
        let lines = match err {
            crate::error::BrowserError::NoCandidateWorked { lines, .. } => lines,
            other => panic!("unexpected error: {other}"),
        };
        assert!(
            lines.iter().any(|l| l.contains("does not exist (404)")),
            "{lines:?}"
        );
        cleanup(&dir);
    }

    #[test]
    fn read_bytes_caps_and_honors_cancel() {
        let dir = tmp_dir("bytes");
        let path = dir.join("blob.bin");
        std::fs::write(&path, b"1234567890").unwrap();
        let url = Url::from_file_path(&path).unwrap();
        let running = AtomicBool::new(false);
        assert_eq!(read_bytes(&url, 10, &running).unwrap().len(), 10);
        assert!(read_bytes(&url, 9, &running)
            .unwrap_err()
            .contains("exceeds"));
        let cancelled = AtomicBool::new(true);
        assert!(read_bytes(&url, 10, &cancelled)
            .unwrap_err()
            .contains("cancelled"));
        let missing = Url::from_file_path(dir.join("nope.md")).unwrap();
        assert!(read_bytes(&missing, 10, &running)
            .unwrap_err()
            .contains("does not exist"));
        cleanup(&dir);
    }
}
