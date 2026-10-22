// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `app::save`: write the page on screen to the downloads folder as Markdown.
//!
//! - `Ctrl/Cmd+S` and the document context menu's "Save page" row ask `App`
//!   for it; there is no dialog and no format choice: the captured Markdown
//!   source (frontmatter included) goes straight to disk as
//!   `~/Downloads/<page title>.md`.
//! - An existing file is NEVER overwritten: the write picks the first free
//!   browser-style copy name (`notes.md`, `notes (1).md`, ...), so a repeated
//!   save of the same page accumulates versions instead of destroying one.
//! - The destination directory is the platform's downloads folder: the
//!   Downloads known folder on Windows (so a location the user relocated in
//!   Explorer is honored), the XDG `XDG_DOWNLOAD_DIR` on Linux/BSD (read
//!   from `user-dirs.dirs`, so a localized name like `Descargas` works) when
//!   it exists, else a literal `~/Downloads` when that exists, else the home
//!   directory, else the current directory on a machine with no home.
//! - A destination that refuses the write (a permission prompt denied, a
//!   read-only folder, a filesystem policy) falls back to the home directory
//!   instead of failing behind an OS dialog; the toast names the location.
//! - The default file name derives from the page: its title slug, else the
//!   URL's last path segment (`.md`/`.markdown`/`.txt` stripped and
//!   percent-decoded), else `document`. The result is a safe file name on
//!   every platform (alphanumerics, `-` and `_` only), never empty and
//!   length-capped BEFORE it becomes a path component, so a hostile title or
//!   URL segment cannot add a separator or `..`, leave the downloads
//!   directory or name a Windows device.
//! - The write is ATOMIC and no-replace where the filesystem supports hard
//!   links: the bounded document (at most `MAX_BODY_BYTES`) lands in a
//!   unique sibling temp file (`create_new`, never following a planted
//!   link) that is synced to disk and published through a hard link, so a
//!   failure mid-write never leaves a truncated document and a file that
//!   appears after the name check is never overwritten. The directory entry
//!   is synced too, so a confirmed save survives a power loss. Filesystems
//!   without hard links fall back to an exclusive in-place create (also no
//!   replacement). A plain synchronous write on the UI thread is acceptable
//!   at the fetch cap, like the stores' own bounded writes.

use crate::config;
use std::path::{Path, PathBuf};

/// Directory preferred for the destination when it exists.
const DOWNLOADS_DIR: &str = "Downloads";
/// Extension of every saved page (always the Markdown source).
const MARKDOWN_EXTENSION: &str = "md";
/// Browser-style copy names tried before giving up (`notes (1).md` …).
const MAX_NAME_COPIES: u32 = 10_000;

/// One document captured for saving: the file-name stem plus the Markdown
/// source as fetched. The source is BORROWED: a direct save writes it before
/// the tab can change, so no body-sized copy is made.
pub(super) struct Payload<'a> {
    /// Page-title slug (or URL segment), no extension.
    stem: String,
    /// Raw markdown source as fetched (frontmatter included).
    source: &'a str,
}

impl<'a> Payload<'a> {
    /// Captures one document with [`default_stem`]'s result as its stem.
    pub(super) fn new(stem: String, source: &'a str) -> Self {
        Self { stem, source }
    }
}

/// A completed save.
pub(super) struct Saved {
    /// File written.
    path: PathBuf,
    /// True when the downloads folder refused the write and the home
    /// directory caught it (the toast names the different location).
    home_fallback: bool,
}

impl Saved {
    /// File NAME for the confirmation toast (`notes.md`), or the whole path
    /// for a destination with no file name at all.
    pub(super) fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Was the downloads folder unwritable and home used instead?
    pub(super) fn home_fallback(&self) -> bool {
        self.home_fallback
    }
}

/// Why one write attempt failed: the line for the toast plus the one error
/// kind the save falls back on.
#[derive(Debug)]
struct WriteError {
    message: String,
    permission_denied: bool,
}

impl WriteError {
    fn io(error: std::io::Error) -> Self {
        Self {
            message: format!("Could not save: {error}"),
            permission_denied: error.kind() == std::io::ErrorKind::PermissionDenied,
        }
    }

    fn plain(message: String) -> Self {
        Self {
            message,
            permission_denied: false,
        }
    }
}

/// Writes `payload` into the downloads folder ([`download_dir`]).
///
/// A destination that refuses the write with a permission error (the macOS
/// "Downloads folder" prompt denied, a read-only folder, a filesystem policy)
/// is retried in the home directory, so the save still completes and the
/// caller can say where it landed. Any other failure is reported as-is: a
/// wrong path or a full disk must not silently move the file.
pub(super) fn write(payload: &Payload<'_>) -> Result<Saved, String> {
    write_with(&download_dir(), crate::util::home_dir().as_deref(), payload)
}

/// [`write()`] with an explicit downloads directory (tests exercise the real
/// write off the real downloads folder; `None` uses the platform one).
pub(super) fn write_to(directory: Option<&Path>, payload: &Payload<'_>) -> Result<Saved, String> {
    match directory {
        Some(directory) => write_with(directory, crate::util::home_dir().as_deref(), payload),
        None => write(payload),
    }
}

/// [`write()`] against an explicit downloads directory and home (tests
/// exercise the fallback without a real denied `~/Downloads`).
fn write_with(
    directory: &Path,
    home: Option<&Path>,
    payload: &Payload<'_>,
) -> Result<Saved, String> {
    match write_in(directory, payload) {
        Ok(path) => Ok(Saved {
            path,
            home_fallback: false,
        }),
        Err(error) if error.permission_denied => {
            let Some(home) = home.filter(|home| *home != directory) else {
                return Err(error.message);
            };
            match write_in(home, payload) {
                Ok(path) => Ok(Saved {
                    path,
                    home_fallback: true,
                }),
                // The downloads failure is the real one: the home attempt
                // only existed to soften it.
                Err(_) => Err(error.message),
            }
        }
        Err(error) => Err(error.message),
    }
}

/// [`write()`] into an explicit directory (tests exercise the browser-style
/// copy naming, the atomic write and the permission fallback without
/// touching the real downloads folder).
fn write_in(directory: &Path, payload: &Payload<'_>) -> Result<PathBuf, WriteError> {
    let path = unique_destination(directory, &payload.stem)?;
    write_file(&path, payload.source, false).map(|()| path)
}

/// First free destination for `stem` in `directory`: the page's own name when
/// nothing owns it, else the first `notes (N).md` that does not exist.
///
/// An existing file is never replaced, so a repeated Ctrl/Cmd+S accumulates
/// copies exactly like a browser download does.
fn unique_destination(directory: &Path, stem: &str) -> Result<PathBuf, WriteError> {
    let first = directory.join(format!("{stem}.{MARKDOWN_EXTENSION}"));
    if !destination_taken(&first) {
        return Ok(first);
    }
    for copy in 1..=MAX_NAME_COPIES {
        let candidate = directory.join(format!("{stem} ({copy}).{MARKDOWN_EXTENSION}"));
        if !destination_taken(&candidate) {
            return Ok(candidate);
        }
    }
    Err(WriteError::plain(format!(
        "Could not save: too many files named {stem}.{MARKDOWN_EXTENSION}"
    )))
}

/// Does a directory entry occupy `path`? `symlink_metadata` (lstat) counts a
/// symlink itself, including a DANGLING one, so a planted link is treated as
/// taken (the save moves to the next copy name) instead of being silently
/// replaced by `rename`. `exists()` followed the link and reported a dangling
/// one as free.
fn destination_taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Writes `bytes` to `path` through a unique sibling temp file.
///
/// `replace` is only for callers that explicitly own the destination (the
/// atomic-replacement test): a SAVE passes `false`, so a file that appeared
/// after [`unique_destination`] checked the name is never overwritten. The
/// no-replace publish is a hard link, which creates the destination only if
/// it is still free and exposes the completed temp file atomically; a
/// filesystem without hard links falls back to an exclusive create (still
/// no replacement). A replacement keeps the existing file's permissions.
fn write_file(path: &Path, bytes: &str, replace: bool) -> Result<(), WriteError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    // Unique per process + call: two saves never share a temp file, and a
    // leftover from a crash is never followed (create_new).
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut written = None;
    for _ in 0..8 {
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = dir.join(format!(".mdowser-save-{}-{seq}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&temp) {
            Ok(mut file) => {
                use std::io::Write as _;
                // Sync before the file can become visible at the
                // destination: a confirmed save then survives a power loss
                // (the stores' atomic writer has always done this).
                let result = file
                    .write_all(bytes.as_bytes())
                    .and_then(|()| file.sync_all());
                drop(file);
                if let Err(e) = result {
                    let _ = std::fs::remove_file(&temp);
                    return Err(WriteError::io(e));
                }
                written = Some(temp);
                break;
            }
            // A stale temp name (pid reuse) is skipped, never truncated.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(WriteError::io(e)),
        }
    }
    let Some(temp) = written else {
        return Err(WriteError::plain(
            "Could not save: no unique temporary file".to_string(),
        ));
    };
    if !replace {
        publish_new(path, &temp, bytes)?;
        sync_parent(dir);
        return Ok(());
    }
    // Replacing an existing file keeps its permissions (`fs::write` used to
    // truncate in place; the atomic rename must not silently widen or narrow
    // them).
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = meta.permissions().mode() & 0o777;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode));
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(WriteError::io(e));
    }
    sync_parent(dir);
    Ok(())
}

/// Persists the directory entry after a publish, so a power loss right after
/// a confirmed save cannot lose the file itself (the same best-effort
/// directory fsync the settings stores do). Windows cannot open a directory
/// this way; the synced file contents still survive there.
fn sync_parent(dir: &Path) {
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

/// Publishes the finished temp file at a destination that must still be
/// free. [`std::fs::hard_link`] is atomic and fails with `AlreadyExists`
/// instead of replacing the target, closing the check-then-rename race in
/// [`unique_destination`]. Filesystems that do not support hard links
/// (FAT/exFAT, some network shares) fall back to an exclusive create of the
/// same bytes: still no replacement, and it cleans up after a failed write.
fn publish_new(path: &Path, temp: &Path, bytes: &str) -> Result<(), WriteError> {
    match std::fs::hard_link(temp, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(temp);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(temp);
            Err(WriteError::plain(format!(
                "Could not save: {} already exists",
                path.display()
            )))
        }
        Err(_) => {
            let _ = std::fs::remove_file(temp);
            write_exclusive(path, bytes)
        }
    }
}

/// Creates `path` exclusively and writes `bytes` into it. The fallback for
/// filesystems without hard links: a destination that appeared in the
/// meantime is never replaced; a failed write removes the partial file.
fn write_exclusive(path: &Path, bytes: &str) -> Result<(), WriteError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(path).map_err(WriteError::io)?;
    use std::io::Write as _;
    let result = file
        .write_all(bytes.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(path);
        return Err(WriteError::io(e));
    }
    Ok(())
}

/// Default file-name stem for a page: its own title (slugified), else its
/// URL's last path segment, else `document`.
///
/// The result is a safe file name on every platform (alphanumerics, `-` and
/// `_` only) and never empty; `MAX_EXPORT_NAME_CHARS` bounds it so an
/// overlong title cannot produce a name the filesystem refuses.
pub(super) fn default_stem(title: Option<&str>, display_url: &url::Url) -> String {
    if let Some(title) = title {
        let stem = capped_stem(&crate::markdown::anchors::slugify(title));
        if !stem.is_empty() {
            return stem;
        }
    }
    let from_url = url_stem(display_url);
    if !from_url.is_empty() {
        return from_url;
    }
    "document".to_string()
}

/// Last path segment of `url` as a stem: `.md`/`.markdown`/`.txt` stripped
/// (so a `README.md` page does not save as `readme.md.md`), percent-decoded
/// (`Instalaci%C3%B3n` -> `instalación`) and slugified. Empty when the URL
/// has no usable segment (`https://example.com/`, `about:licenses`).
fn url_stem(url: &url::Url) -> String {
    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        .unwrap_or_default();
    let decoded = crate::markdown::anchors::percent_decode(segment);
    let stem = match Path::new(&decoded).extension().and_then(|e| e.to_str()) {
        Some(ext) if matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt") => {
            Path::new(&decoded)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or(decoded)
        }
        _ => decoded,
    };
    capped_stem(&crate::markdown::anchors::slugify(&stem))
}

/// Slug capped at `MAX_EXPORT_NAME_CHARS` chars, trimmed of the hyphens a
/// truncated run could leave trailing (`notes-` -> `notes`). Empty when the
/// slug was empty.
///
/// A Windows DEVICE name is never returned verbatim: `aux.md`, `con.md` and
/// `nul.md` still name the console/device there (any extension), so the stem
/// gets a `-file` suffix (`aux` -> `aux-file`). Applied on every platform, so
/// one page saves under the same name everywhere.
fn capped_stem(slug: &str) -> String {
    let trimmed = slug.trim_matches('-');
    let capped = crate::util::truncate_chars(trimmed, config::MAX_EXPORT_NAME_CHARS);
    let capped = capped.trim_matches('-');
    if capped.is_empty() {
        return String::new();
    }
    if is_windows_device_name(&capped.to_ascii_lowercase()) {
        return format!("{capped}-file");
    }
    capped.to_string()
}

/// Is `lower` one of the reserved Windows device names (compared
/// case-insensitively, extension included on the caller's side)?
fn is_windows_device_name(lower: &str) -> bool {
    matches!(
        lower,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

/// Directory a saved page lands in: the XDG downloads folder when one is
/// configured and exists (`XDG_DOWNLOAD_DIR` in `user-dirs.dirs`, so
/// localized names like `Descargas` work), else a literal `~/Downloads` when
/// that exists (where a browser puts saved files), else the home directory,
/// else an empty path (the current directory) on a machine with no home.
fn default_dir_in(home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::new();
    };
    default_dir_from(home, &xdg_config_home(home))
}

/// [`default_dir_in`] against an explicit config directory, so tests exercise
/// the XDG preference without touching the real environment.
fn default_dir_from(home: &Path, config: &Path) -> PathBuf {
    if let Some(platform) = platform_download_dir().filter(|dir| dir.is_dir()) {
        return platform;
    }
    let configured = xdg_download_dir_in(home, config).filter(|dir| dir.is_dir());
    if let Some(configured) = configured {
        return configured;
    }
    let literal = home.join(DOWNLOADS_DIR);
    if literal.is_dir() {
        literal
    } else {
        home.to_path_buf()
    }
}

/// The OS's own Downloads folder when it can name one.
///
/// Windows: the Downloads known folder, so a location the user moved in
/// Explorer (or a redirected profile) is honored. Other platforms: `None` —
/// macOS has no such API and its folder is the standard `~/Downloads`
/// (X11/Wayland desktops name theirs through `user-dirs.dirs`, read above).
/// `known-folders` is empty off Windows, so this costs nothing there.
fn platform_download_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        known_folders::get_known_folder_path(known_folders::KnownFolder::Downloads)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// [`default_dir_in`] against the real home directory.
fn download_dir() -> PathBuf {
    default_dir_in(crate::util::home_dir().as_deref())
}

/// `$XDG_CONFIG_HOME` when it is set and ABSOLUTE (the XDG spec requires
/// one; a relative value must never resolve the downloads folder under the
/// current working directory), else `~/.config`.
fn xdg_config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

/// `XDG_DOWNLOAD_DIR` from `<config>/user-dirs.dirs`, resolved against
/// `home`: `$HOME` and `${HOME}` expand (the form `xdg-user-dirs` writes,
/// e.g. `XDG_DOWNLOAD_DIR="$HOME/Descargas"`), a value that stays relative
/// is rejected, and a missing or unreadable file yields `None` (the caller
/// falls back to `~/Downloads`). Quoting and comments follow the format the
/// tool writes: one `KEY="value"` per line, `#` comments.
fn xdg_download_dir_in(home: &Path, config: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config.join("user-dirs.dirs")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "XDG_DOWNLOAD_DIR" {
            continue;
        }
        let value = value.trim().trim_matches('"');
        // `$HOME/…` and `${HOME}/…` both appear in the wild: expand the
        // prefix against the SAME home the caller handed in (never an env
        // lookup, so a relative HOME cannot sneak back in).
        let rest = value
            .strip_prefix("${HOME}")
            .or_else(|| value.strip_prefix("$HOME"));
        let path = match rest {
            Some(rest) => home.join(rest.trim_start_matches(['/', '\\'])),
            None => PathBuf::from(value),
        };
        if path.is_absolute() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh scratch directory for the file-system tests (the stores' own
    /// recipe: `pid`-tagged, so parallel test processes never share one).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdowser-save-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn page(url: &str) -> url::Url {
        url::Url::parse(url).expect("test url")
    }

    #[test]
    fn default_stem_prefers_the_page_title() {
        let url = page("https://example.com/guide/install/");
        assert_eq!(
            default_stem(Some("Instalación rápida"), &url),
            "instalación-rápida"
        );
        // A title that slugs to nothing (punctuation only) leaves the URL
        // segment to name the file.
        assert_eq!(default_stem(Some("—"), &url), "install");
        // No title and no last segment: the last resort, never an error.
        assert_eq!(
            default_stem(None, &page("https://example.com/")),
            "document"
        );
        // The stem is bounded: a title cannot build a file name the
        // filesystem would refuse.
        let long = default_stem(Some(&"a".repeat(500)), &url);
        assert_eq!(long.chars().count(), config::MAX_EXPORT_NAME_CHARS);
    }

    #[test]
    fn default_stem_reads_the_url_the_way_the_page_shows_it() {
        // Percent-encoded segments decode, and a document extension is
        // stripped: `README.md` must not save as `readme.md.md`.
        assert_eq!(
            default_stem(None, &page("https://example.com/docs/Instalaci%C3%B3n.md")),
            "instalación"
        );
        assert_eq!(
            default_stem(None, &page("https://example.com/docs/NOTES.markdown")),
            "notes"
        );
        assert_eq!(
            default_stem(None, &page("file:///Users/you/Notas%20sueltas.txt")),
            "notas-sueltas"
        );
        // An internal page has no path segment at all.
        assert_eq!(
            default_stem(None, &page(crate::config::LICENSES_URL)),
            "document"
        );
    }

    #[test]
    fn a_windows_device_name_never_names_the_default_file() {
        let url = page("https://example.com/guide/install/");
        // `aux.md`, `con.md` and `nul.md` still name the console or a device
        // there (with any extension): the stem is suffixed instead.
        assert_eq!(default_stem(Some("AUX"), &url), "aux-file");
        assert_eq!(default_stem(Some("Nul"), &url), "nul-file");
        assert_eq!(default_stem(Some("Com1"), &url), "com1-file");
        assert_eq!(
            default_stem(None, &page("https://example.com/docs/lpt3.md")),
            "lpt3-file"
        );
        // Only the exact device names: their longer siblings and the other
        // numbered units are ordinary file names.
        assert_eq!(default_stem(Some("Console"), &url), "console");
        assert_eq!(default_stem(Some("com10"), &url), "com10");
    }

    #[test]
    fn default_dir_lands_in_downloads_when_it_exists() {
        let dir = scratch("default-dir");
        let config = dir.join("config");
        assert_eq!(
            default_dir_from(&dir, &config).join("notes.md"),
            dir.join("notes.md"),
            "no Downloads directory and no XDG config: home itself"
        );
        std::fs::create_dir_all(dir.join(DOWNLOADS_DIR)).expect("Downloads");
        assert_eq!(
            default_dir_from(&dir, &config),
            dir.join(DOWNLOADS_DIR),
            "a browser saves into Downloads when it is there"
        );
        // No home directory at all: a relative name (the current directory).
        assert_eq!(default_dir_in(None), PathBuf::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The XDG download folder (localized or customized, what a Linux desktop
    /// actually uses) wins over the literal `~/Downloads`, and a relative or
    /// unknown entry is ignored instead of escaping home.
    #[test]
    fn configured_xdg_download_dir_wins_over_the_literal() {
        let dir = scratch("xdg");
        let config = dir.join("config");
        let home = dir.join("home");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::create_dir_all(home.join("Descargas")).expect("Descargas");
        std::fs::create_dir_all(home.join(DOWNLOADS_DIR)).expect("Downloads");
        std::fs::write(
            config.join("user-dirs.dirs"),
            "# generated by xdg-user-dirs\nXDG_DESKTOP_DIR=\"$HOME/Escritorio\"\nXDG_DOWNLOAD_DIR=\"$HOME/Descargas\"\n",
        )
        .expect("user-dirs.dirs");
        assert_eq!(
            xdg_download_dir_in(&home, &config),
            Some(home.join("Descargas"))
        );
        assert_eq!(
            default_dir_from(&home, &config),
            home.join("Descargas"),
            "the configured folder wins over the literal ~/Downloads"
        );
        // `${HOME}` is accepted too, and a value that stays relative (or a
        // config naming another key) is rejected.
        std::fs::write(
            config.join("user-dirs.dirs"),
            "XDG_DOWNLOAD_DIR=\"${HOME}/Bajadas\"\n",
        )
        .expect("user-dirs.dirs");
        assert_eq!(
            xdg_download_dir_in(&home, &config),
            Some(home.join("Bajadas"))
        );
        std::fs::write(
            config.join("user-dirs.dirs"),
            "XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n",
        )
        .expect("user-dirs.dirs");
        assert_eq!(xdg_download_dir_in(&home, &config), None);
        std::fs::write(
            config.join("user-dirs.dirs"),
            "XDG_DOWNLOAD_DIR=\"Descargas\"\n",
        )
        .expect("user-dirs.dirs");
        assert_eq!(
            xdg_download_dir_in(&home, &config),
            None,
            "a relative entry must never resolve under the working directory"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_writes_the_markdown_source_exactly_as_captured() {
        let dir = scratch("write");
        let payload = Payload::new("notes".to_string(), "# Title\n\nBody\n");
        let path = write_in(&dir, &payload).expect("save");
        assert_eq!(path, dir.join("notes.md"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("file written"),
            "# Title\n\nBody\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeated_save_never_overwrites_the_previous_file() {
        let dir = scratch("copies");
        let payload = Payload::new("notes".to_string(), "# Title\n");
        let first = write_in(&dir, &payload).expect("first save");
        let second = write_in(&dir, &payload).expect("second save");
        assert_eq!(first.file_name().expect("name"), "notes.md");
        assert_eq!(
            second.file_name().expect("name").to_string_lossy(),
            "notes (1).md",
            "the browser-style copy name is picked"
        );
        assert_eq!(
            std::fs::read_to_string(&first).expect("first kept"),
            "# Title\n",
            "the file already on disk stays untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dangling symlink still OCCUPIES the page's file name: the save
    /// picks the next browser-style copy instead of replacing the link (an
    /// `exists()` check followed the link and reported it as free).
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_at_the_page_name_is_preserved() {
        let dir = scratch("symlink");
        let target = dir.join("notes.md");
        std::os::unix::fs::symlink("missing-target", &target).expect("symlink");
        let payload = Payload::new("notes".to_string(), "# Title\n");
        let saved = write_in(&dir, &payload).expect("save");
        assert_eq!(saved, dir.join("notes (1).md"));
        assert_eq!(
            std::fs::read_link(&target).expect("link kept"),
            PathBuf::from("missing-target")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The save write is ATOMIC: it replaces the destination through a
    /// sibling temp file and never leaves that temp behind, so an existing
    /// file is only ever swapped whole.
    #[test]
    fn write_file_replaces_atomically_and_cleans_up() {
        let dir = scratch("atomic");
        let target = dir.join("notes.md");
        write_file(&target, "first\n", false).expect("write");
        write_file(&target, "second\n", true).expect("replace");
        assert_eq!(
            std::fs::read_to_string(&target).expect("file written"),
            "second\n"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .expect("scratch dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".mdowser-save-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The no-replace publish closes the check-then-rename race: a file that
    /// appears after `unique_destination` checked the name must survive, and
    /// the failed attempt must leave no temp file behind.
    #[test]
    fn an_existing_destination_is_never_replaced_by_a_new_save() {
        let dir = scratch("no-replace");
        let target = dir.join("notes.md");
        std::fs::write(&target, "already here\n").expect("seed");
        let error = write_file(&target, "new contents\n", false).expect_err("must not replace");
        assert!(
            error.message.contains("already exists"),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("original kept"),
            "already here\n"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .expect("scratch dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".mdowser-save-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_destination_directory_fails_without_a_file() {
        let dir = scratch("error");
        let missing = dir.join("missing");
        let payload = Payload::new("notes".to_string(), "# Title\n");
        let error = write_in(&missing, &payload).expect_err("no directory");
        assert!(
            error.message.starts_with("Could not save"),
            "{}",
            error.message
        );
        assert!(!missing.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A downloads folder that refuses the write (the macOS permission prompt
    /// denied, a read-only folder) sends the save to home instead of failing:
    /// the file completes and the caller learns it fell back.
    #[cfg(unix)]
    #[test]
    fn a_denied_downloads_folder_falls_back_to_home() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch("denied");
        let blocked = dir.join("blocked");
        let home = dir.join("home");
        std::fs::create_dir_all(&blocked).expect("blocked");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o555))
            .expect("read-only");
        // Packaged test environments can run as root, which ignores a
        // directory mode: the scenario cannot be built there, so skip it
        // instead of asserting on a write that legitimately succeeds.
        if std::fs::write(blocked.join(".probe"), b"").is_ok() {
            let _ = std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let payload = Payload::new("notes".to_string(), "# Title\n");
        let saved = write_with(&blocked, Some(&home), &payload).expect("fallback save");
        assert!(saved.home_fallback(), "the fallback is reported");
        assert_eq!(saved.file_name(), "notes.md");
        assert_eq!(
            std::fs::read_to_string(home.join("notes.md")).expect("home file"),
            "# Title\n"
        );
        assert!(
            !blocked.join("notes.md").exists(),
            "the denied folder stays untouched"
        );
        let _ = std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
