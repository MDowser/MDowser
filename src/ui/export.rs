// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::export`: the "Export…" card (the page on screen to a local file).
//!
//! - App-owned and document-scoped: `Ctrl/Cmd+S` or the document context
//!   menu's `Export…` asks `App` for it, and `App` captures BOTH renderings of
//!   the page ONCE ([`ExportPayload`]), so a navigation, a tab switch or a
//!   reload while the card is up can never change what a later save writes.
//!   The card owns the whole write: destination resolution, the two-click
//!   overwrite confirmation and the inline error all live here.
//! - A floating `Area` in `Order::Foreground` (the find bar's layer), anchored
//!   to the top center: above the document, the outline sidebar and the
//!   Preferences window, never draggable and never a layout participant.
//! - `Esc` closes it through `app::shortcuts` ([`ExportState::close`]), like
//!   the other app-owned surface; the primary action (its button, or `Enter`
//!   in the field) resolves the typed destination, and an EXISTING file
//!   reaches the disk only on the second press (`Replace`) — the same
//!   armed-confirm rule as the destructive Preferences actions.
//! - No native save dialog, no new dependency: the destination is a plain
//!   editable path, defaulted next to the document's own title in
//!   `~/Downloads` (else the home directory), with `~` expansion. `App`
//!   confirms the write with a toast from [`ExportActions::saved`].

use crate::config;
use crate::ui::{icons, theme};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Card width. Fixed like the Preferences dialog, so a long path or an error
/// line never resizes the surface under the pointer.
const CARD_W: f32 = 440.0;
/// Distance from the window top to the card's top edge (matches Preferences).
const TOP_MARGIN: f32 = 48.0;
/// Card corner radius (extra-large shape, the dialog recipe).
const RADIUS: u8 = 14;
/// Body horizontal padding.
const PAD: f32 = 18.0;
/// Footer action button size.
const BUTTON_W: f32 = 96.0;
const BUTTON_H: f32 = 32.0;
/// One format-choice segment.
const SEGMENT_W: f32 = 112.0;
const SEGMENT_H: f32 = 30.0;
/// Height of the destination field lane.
const FIELD_MIN_H: f32 = 30.0;
/// Seconds the "Replace" confirmation stays armed (shared with every other
/// two-click action).
const REPLACE_CONFIRM_SECS: u64 = config::DELETE_CONFIRM_SECS;
/// Directory name preferred for the default destination when it exists.
const DOWNLOADS_DIR: &str = "Downloads";

/// One rendering of the captured document: what the card writes to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// The markdown source exactly as fetched (frontmatter included).
    Markdown,
    /// The document's plain text, the same string "Select all" copies.
    PlainText,
}

impl ExportFormat {
    /// All formats, in choice order.
    const ALL: [Self; 2] = [Self::Markdown, Self::PlainText];

    /// Choice-button label.
    fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::PlainText => "Plain text",
        }
    }

    /// File extension (without the dot) of the default destination.
    fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::PlainText => "txt",
        }
    }

    /// The bytes this format writes for `payload`.
    fn bytes(self, payload: &ExportPayload) -> &str {
        match self {
            Self::Markdown => &payload.source,
            Self::PlainText => &payload.plain_text,
        }
    }
}

/// A document captured for export: the file-name stem plus both renderings.
///
/// Built ONCE when the card opens (owning copies, see the module docs): the
/// two body-sized strings are the top of the per-tab memory already, so the
/// copies only live while the card is up, and every later frame draws and
/// writes from them instead of re-deriving anything.
pub struct ExportPayload {
    /// Page title (or URL segment) slug, no extension: the default file name.
    stem: String,
    /// Raw markdown source as fetched.
    source: String,
    /// Plain text of the document.
    plain_text: String,
}

impl ExportPayload {
    /// Captures one document: `stem` is normally [`default_stem`]'s result.
    pub fn new(stem: String, source: String, plain_text: String) -> Self {
        Self {
            stem,
            source,
            plain_text,
        }
    }
}

/// Why a typed destination could not be resolved into a real path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathError {
    /// Nothing was typed.
    Empty,
    /// A `~` path with no home directory to expand it against.
    NoHome,
}

impl PathError {
    /// The line shown in the card.
    fn message(self) -> &'static str {
        match self {
            Self::Empty => "Enter a file name",
            Self::NoHome => "No home directory to expand ~",
        }
    }
}

/// Visibility and transient state of the export card.
pub struct ExportState {
    /// The open card, if any. Holds the captured payload.
    open: Option<ExportCard>,
    /// Deadline of the armed overwrite confirmation. Cleared whenever the
    /// card closes or the destination changes (the confirm belongs to the
    /// path that was armed, never to whatever is typed next).
    replace_until: Option<Instant>,
}

/// The open card's editable state.
struct ExportCard {
    /// Both renderings of the document, captured at open time.
    payload: ExportPayload,
    /// Chosen rendering.
    format: ExportFormat,
    /// Destination as typed (resolved on save, never rewritten by the card).
    path: String,
    /// Last failed write: shown inline, the card stays open.
    error: Option<String>,
    /// Select the destination field on the next real frame (the card opens
    /// with the default name ready to be replaced by typing).
    request_focus: bool,
}

/// What one frame of the card asked for; applied by [`ExportState::show`],
/// which owns the armed deadline and does the write.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CardIntent {
    /// Primary action: the button, or `Enter` in the destination field.
    save: bool,
    /// Destination or format changed: disarm any pending overwrite.
    edited: bool,
    /// Cancel button or the header's close.
    close: bool,
}

/// Side effects of one card frame, performed by `App`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportActions {
    /// File NAME written to disk this frame (`notes.md`): `App` confirms it
    /// with a toast. `None` on every other frame.
    pub saved: Option<String>,
}

impl ExportState {
    /// Card closed.
    pub fn new() -> Self {
        Self {
            open: None,
            replace_until: None,
        }
    }

    /// Is the card up? (`app::shortcuts` reads it to give Esc to the card
    /// before Stop; `App` uses it to keep a repeat request from resetting a
    /// destination the user is typing.)
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the card (or replaces an open one) for `payload`: Markdown
    /// source, default destination, no armed confirmation.
    pub fn open(&mut self, payload: ExportPayload) {
        let format = ExportFormat::Markdown;
        let path = default_path(&payload.stem, format);
        self.replace_until = None;
        self.open = Some(ExportCard {
            payload,
            format,
            path,
            error: None,
            request_focus: true,
        });
    }

    /// Closes the card (Cancel, its X, Esc, or a completed save) and disarms
    /// any pending overwrite.
    pub fn close(&mut self) {
        self.open = None;
        self.replace_until = None;
    }

    /// Draws the card when open and applies what it asked for. Returns the
    /// confirmation `App` shows.
    pub fn show(&mut self, ctx: &egui::Context) -> ExportActions {
        if self.open.is_none() {
            return ExportActions::default();
        }
        let now = Instant::now();
        let mut armed = self.replace_until.is_some_and(|until| until > now);
        let dark = ctx.theme() == egui::Theme::Dark;
        let s = theme::scheme_ref_ctx(ctx);
        let frame = egui::Frame::new()
            .fill(s.surface)
            .corner_radius(egui::CornerRadius::same(RADIUS))
            .shadow(theme::dialog_shadow(dark))
            .inner_margin(egui::Margin::same(0));
        let mut intent = CardIntent::default();
        // The card holds the payload and the typed destination, so this
        // borrow covers the whole draw; nothing below reads it again. The
        // early return above guarantees it is open (no `expect`: a UI
        // thread must never panic on state it can simply skip).
        let Some(card) = self.open.as_mut() else {
            return ExportActions::default();
        };
        egui::Area::new(egui::Id::new("export-card"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, TOP_MARGIN))
            // The find bar's layer: above the document and the outline
            // sidebar and the Preferences window, below the context menu and
            // the toasts it opened from and reports back through.
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(CARD_W);
                    // No gaps: the header/body stack stays flush, like the
                    // Preferences dialog (the theme's 8px item spacing would
                    // pile two gaps under the header divider).
                    ui.spacing_mut().item_spacing.y = 0.0;
                    header(ui, s, &mut intent);
                    body(ui, s, card, armed, &mut intent);
                });
            });
        // An edited destination (or format) disarms FIRST: the confirmation
        // must never carry over to a different file.
        if intent.edited {
            self.replace_until = None;
            // The frame-start `armed` belonged to the OLD destination: an
            // edit and the primary action can arrive in the same frame (a
            // paste and `Enter`), and that press must re-arm rather than
            // write to the new path on the strength of the old confirm.
            armed = false;
            if let Some(card) = self.open.as_mut() {
                card.error = None;
            }
        }
        if intent.close {
            self.close();
            return ExportActions::default();
        }
        let mut actions = ExportActions::default();
        if !intent.save {
            if armed {
                // Keep the armed label from going stale.
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            return actions;
        }
        let Some(card) = self.open.as_mut() else {
            return actions;
        };
        match resolve_path(&card.path) {
            Err(error) => {
                card.error = Some(error.message().to_string());
                self.replace_until = None;
            }
            Ok(path) => {
                // One `stat` per press, never per frame. An existing file
                // only reaches the disk on the SECOND press.
                if path.exists() && !armed {
                    self.replace_until = Some(config::deadline_after(REPLACE_CONFIRM_SECS));
                    ctx.request_repaint_after(Duration::from_millis(250));
                } else {
                    // Read the bytes BEFORE the match: the borrow of the
                    // payload would otherwise live through the arms that
                    // write `card.error`.
                    let result = write_file(&path, card.format.bytes(&card.payload));
                    match result {
                        Ok(name) => {
                            actions.saved = Some(name);
                            self.close();
                        }
                        Err(error) => {
                            card.error = Some(error);
                            self.replace_until = None;
                        }
                    }
                }
            }
        }
        actions
    }
}

impl Default for ExportState {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes `bytes` to `path` and returns the file NAME for the confirmation
/// toast (`notes.md`), or a line for the card.
///
/// The write is ATOMIC: the bytes land in a unique sibling temp file that is
/// renamed over the destination, so a failure mid-write (full disk, revoked
/// permission, a crash) never truncates or corrupts an existing file the
/// user chose to replace. A plain synchronous write on the UI thread is
/// acceptable because the payload is bounded by `MAX_BODY_BYTES` (the fetch
/// cap), the same land the encrypted stores already do for their own
/// summaries.
fn write_file(path: &Path, bytes: &str) -> Result<String, String> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    // Unique per process + call: two exports never share a temp file, and a
    // leftover from a crash is never followed (create_new).
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut written = None;
    for _ in 0..8 {
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = dir.join(format!(".mdowser-export-{}-{seq}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&temp) {
            Ok(mut file) => {
                use std::io::Write as _;
                let result = file.write_all(bytes.as_bytes());
                drop(file);
                if let Err(e) = result {
                    let _ = std::fs::remove_file(&temp);
                    return Err(format!("Could not save: {e}"));
                }
                written = Some(temp);
                break;
            }
            // A stale temp name (pid reuse) is skipped, never truncated.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Could not save: {e}")),
        }
    }
    let Some(temp) = written else {
        return Err("Could not save: no unique temporary file".to_string());
    };
    // Replacing an existing file keeps its permissions (`fs::write` used to
    // truncate in place; the atomic rename must not silently widen or narrow
    // them). New files keep the process umask.
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = meta.permissions().mode() & 0o777;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode));
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Could not save: {e}"));
    }
    Ok(match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        // A destination with no file name at all (`/`, `~`): echo the path.
        None => path.display().to_string(),
    })
}

/// Default file-name stem for a page: its own title (slugified), else its
/// URL's last path segment, else `document`.
///
/// The result is a safe file name on every platform (alphanumerics, `-` and
/// `_` only) and never empty; `MAX_EXPORT_NAME_CHARS` bounds it so an
/// overlong title cannot produce a name the filesystem refuses.
pub fn default_stem(title: Option<&str>, display_url: &url::Url) -> String {
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
/// (so a `README.md` page does not export as `readme.md.md`), percent-decoded
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
/// one page exports under the same name everywhere.
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

/// Resolves the typed destination into a real path:
///
/// - `~` and `~/…` (or `~\…` on Windows) expand against `home`;
/// - an absolute path is used as typed;
/// - a relative path is taken from `home` (`notes.md` -> `~/notes.md`), or
///   from the current directory when there is no home.
///
/// Nothing is canonicalized (the file usually does not exist yet) and no
/// directory is created: a missing parent is the writer's error to report,
/// like a native save dialog would.
fn resolve_path_in(field: &str, home: Option<&Path>) -> Result<PathBuf, PathError> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(PathError::Empty);
    }
    let rest = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"));
    if trimmed == "~" || rest.is_some() {
        let Some(home) = home else {
            return Err(PathError::NoHome);
        };
        return Ok(match rest {
            Some(rest) if !rest.is_empty() => home.join(rest),
            // `~` alone is the home directory: the write reports whatever the
            // filesystem says about saving over a directory.
            _ => home.to_path_buf(),
        });
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(match home {
        Some(home) => home.join(path),
        None => path.to_path_buf(),
    })
}

/// [`resolve_path_in`] against the real home directory.
fn resolve_path(field: &str) -> Result<PathBuf, PathError> {
    resolve_path_in(field, crate::util::home_dir().as_deref())
}

/// Directory the default destination starts in: `~/Downloads` when it
/// exists (where a browser puts saved files), else the home directory, else
/// an empty path (the current directory) on a machine with no home at all.
fn default_dir_in(home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::new();
    };
    let downloads = home.join(DOWNLOADS_DIR);
    if downloads.is_dir() {
        downloads
    } else {
        home.to_path_buf()
    }
}

/// The default destination as shown in the field.
fn default_path(stem: &str, format: ExportFormat) -> String {
    default_dir_in(crate::util::home_dir().as_deref())
        .join(format!("{stem}.{}", format.extension()))
        .to_string_lossy()
        .into_owned()
}

/// The typed destination with its extension retargeted after a format
/// switch: only the two formats' own extensions are swapped
/// (`notes.md` -> `notes.txt`), so a name the user typed is otherwise left
/// exactly as it is (no extension is invented, none is dropped).
fn retarget_extension(field: &str, format: ExportFormat) -> String {
    let trimmed = field.trim();
    let path = Path::new(trimmed);
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("txt") => path
            .with_extension(format.extension())
            .to_string_lossy()
            .into_owned(),
        _ => trimmed.to_string(),
    }
}

/// Card header: document icon + "Export" on the left, close X on the right,
/// over a full-bleed divider (the Preferences header recipe).
fn header(ui: &mut egui::Ui, s: &theme::Scheme, intent: &mut CardIntent) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD as i8,
            right: 12,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    icons::paint(ui, icon_rect, icons::Icon::File, s.primary);
                    ui.add_space(10.0);
                    ui.label(
                        theme::strong(
                            egui::RichText::new("Export").size(theme::typography::DIALOG_TITLE),
                        )
                        .color(s.on_surface),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_button(ui, icons::Icon::Close, "Close (Esc)", true).clicked()
                        {
                            intent.close = true;
                        }
                    });
                },
            );
        });
    theme::hairline(ui, s.outline_variant);
}

/// Card body: format choice, destination field, the error/confirm line and
/// the action row.
fn body(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    card: &mut ExportCard,
    armed: bool,
    intent: &mut CardIntent,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD as i8,
            right: PAD as i8,
            top: 14,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // ---- Format -------------------------------------------------
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), SEGMENT_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = theme::spacing::S;
                    for format in ExportFormat::ALL {
                        let selected = format == card.format;
                        if format_button(ui, s, format, selected).clicked() && !selected {
                            card.format = format;
                            // The destination follows the format's extension:
                            // typed or defaulted, it stays the file the user
                            // is looking at.
                            card.path = retarget_extension(&card.path, format);
                            intent.edited = true;
                        }
                    }
                },
            );
            ui.add_space(theme::spacing::L);
            // ---- Destination --------------------------------------------
            caption(ui, s, "SAVE AS");
            ui.add_space(theme::spacing::XS);
            path_field(ui, s, card, intent);
            // Fit content: the error and the confirmation lines only take
            // space while they have something to say.
            if let Some(error) = &card.error {
                ui.add_space(theme::spacing::S);
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(error)
                            .size(theme::typography::UI_SMALL)
                            .color(s.error),
                    )
                    .truncate()
                    .selectable(false),
                );
                crate::ui::hover_tooltip_lazy(resp, || error.clone());
            }
            if armed {
                ui.add_space(theme::spacing::S);
                ui.label(theme::weak(ui, "Click again to replace the existing file."));
            }
            ui.add_space(theme::spacing::XL);
            actions_row(ui, armed, intent);
        });
}

/// Small uppercase section caption (the Preferences `section_title`, muted:
/// the card has one emphasized color and it belongs to the primary action).
fn caption(ui: &mut egui::Ui, s: &theme::Scheme, text: &str) {
    ui.label(
        theme::strong(egui::RichText::new(text).size(theme::typography::EYEBROW))
            .color(s.on_surface_variant),
    );
}

/// One format choice: fixed geometry in every state (the app's stable-hover
/// rule), label centered, hover wash while unselected and the accent wash +
/// underline while selected — the Preferences tab language at action size.
fn format_button(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    format: ExportFormat,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SEGMENT_W, SEGMENT_H), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::CARD),
            theme::tint(s.primary, 24),
        );
        let underline = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 10.0, rect.max.y - 2.0),
            egui::vec2(rect.width() - 20.0, 2.0),
        );
        ui.painter().rect_filled(
            underline,
            egui::CornerRadius::same(theme::radius::HAIRLINE),
            s.primary,
        );
    } else {
        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(theme::radius::CARD),
                s.container_hover,
            );
        }
        // The unselected segment keeps the shared hairline, so switching the
        // format never moves a pixel of the row.
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(theme::radius::CARD),
            egui::Stroke::new(theme::stroke::HAIRLINE, s.outline_variant),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        theme::focus_ring(ui, rect, theme::radius::CARD);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format.label(),
        egui::FontId::new(
            theme::typography::BUTTON,
            if selected {
                theme::strong_family()
            } else {
                egui::FontFamily::Proportional
            },
        ),
        if selected { s.primary } else { s.on_surface },
    );
    let label = format.label();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
    });
    response
}

/// The destination field. It opens focused with the default name selected,
/// so typing replaces it (the omnibox / inline-rename contract).
fn path_field(
    ui: &mut egui::Ui,
    s: &theme::Scheme,
    card: &mut ExportCard,
    intent: &mut CardIntent,
) {
    let id = egui::Id::new("export-path");
    // An `Area`'s FIRST frame runs as an invisible sizing pass with a
    // DISABLED Ui: a focus request made there is swallowed (the field cannot
    // take it), so the flag stays set and the request is retried on the next,
    // real frame — the same rule as the find bar and the inline tile editor.
    if card.request_focus && ui.is_enabled() {
        card.request_focus = false;
        ui.memory_mut(|m| m.request_focus(id));
        crate::ui::select_all_in(ui.ctx(), id, &card.path);
    }
    let field = ui.add_sized(
        egui::vec2(ui.available_width(), FIELD_MIN_H),
        egui::TextEdit::singleline(&mut card.path)
            .id(id)
            .desired_width(ui.available_width())
            .char_limit(config::MAX_EXPORT_PATH_CHARS)
            .font(egui::FontId::proportional(theme::typography::ROW))
            .text_color(s.on_surface)
            .hint_text("File name"),
    );
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Save as"));
    if field.changed() {
        intent.edited = true;
    }
    // Enter in the field runs the primary action. A singleline `TextEdit`
    // SURRENDERS focus when Enter is pressed, so by this point `has_focus` is
    // already false: the documented `lost_focus` + `key_pressed` pair is the
    // reliable read (the find bar's rule). The keyboard is handed back so an
    // armed overwrite takes the next Enter too.
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        intent.save = true;
        ui.memory_mut(|m| m.request_focus(id));
    }
}

/// Action row: Cancel to the left of the primary action, right-aligned (the
/// primary takes the far corner, like every dialog).
///
/// While an overwrite is armed the primary action's label states the outcome
/// of the NEXT press ("Replace"), so the two-click confirmation reads as one
/// action instead of a bare repeat, and the armed state is visible, not only
/// implied by the still-open card.
fn actions_row(ui: &mut egui::Ui, armed: bool, intent: &mut CardIntent) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), BUTTON_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = theme::spacing::M;
            // The accent fill is reserved for the UNARMED primary action:
            // once the confirm is about to destroy an existing file, it must
            // not look like the ordinary path forward.
            let primary = if armed {
                theme::tonal_button_sized(ui, "Replace", egui::vec2(BUTTON_W, BUTTON_H), true)
            } else {
                theme::primary_button_sized(ui, "Export", egui::vec2(BUTTON_W, BUTTON_H))
            };
            if primary.clicked() {
                intent.save = true;
            }
            if theme::tonal_button_sized(ui, "Cancel", egui::vec2(BUTTON_W, BUTTON_H), true)
                .clicked()
            {
                intent.close = true;
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh scratch directory for the file-system tests (the stores' own
    /// recipe: `pid`-tagged, so parallel test processes never share one).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdowser-export-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn page(url: &str) -> url::Url {
        url::Url::parse(url).expect("test url")
    }

    fn input_at(pointer: Option<egui::Pos2>) -> egui::RawInput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        if let Some(pos) = pointer {
            input.events.push(egui::Event::PointerMoved(pos));
        }
        input
    }

    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
                _ => None,
            })
            .collect()
    }

    /// Bottom-most text shape reading exactly `text`: the card's header title
    /// and its primary button share the word "Export", and the button is the
    /// lower one.
    fn bottom_text(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
        texts(shapes)
            .into_iter()
            .filter(|(t, _)| t == text)
            .map(|(_, pos)| pos)
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap_or_else(|| panic!("{text:?} rendered"))
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// Runs the card until it settles (an anchored `Area` lays out on its
    /// first frames), returning the last frame's shapes.
    fn settle(
        ctx: &egui::Context,
        state: &mut ExportState,
        frames: usize,
    ) -> Vec<egui::epaint::ClippedShape> {
        let mut shapes = Vec::new();
        for _ in 0..frames {
            let mut out = ctx.run_ui(input_at(None), |root| {
                state.show(root.ctx());
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        shapes
    }

    /// Clicks `pos` (press + release in the click frame, like the
    /// Preferences tests) and returns the actions of the frame that handled
    /// it: the card may close on that very frame, so only its non-default
    /// result is kept.
    fn click(ctx: &egui::Context, state: &mut ExportState, pos: egui::Pos2) -> ExportActions {
        let mut total = ExportActions::default();
        for frame in 0..3 {
            let mut input = input_at(Some(pos));
            if frame == 0 {
                input.events.push(pointer(pos, true));
                input.events.push(pointer(pos, false));
            }
            let mut out = ctx.run_ui(input, |root| {
                let actions = state.show(root.ctx());
                if actions.saved.is_some() {
                    total.saved = actions.saved;
                }
            });
            out.textures_delta.clear();
        }
        total
    }

    /// A captured page whose two renderings differ, so a test can tell which
    /// one reached the disk.
    fn payload(stem: &str) -> ExportPayload {
        ExportPayload::new(
            stem.to_string(),
            "# Title\n\nBody\n".to_string(),
            "Title\nBody\n".to_string(),
        )
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
        // stripped: `README.md` must not export as `readme.md.md`.
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
    fn resolve_path_handles_tilde_absolute_and_relative() {
        let home = Path::new("/home/you");
        assert_eq!(
            resolve_path_in("~/notes.md", Some(home)).expect("resolves"),
            PathBuf::from("/home/you/notes.md")
        );
        // `~` alone is the home directory itself.
        assert_eq!(
            resolve_path_in("~", Some(home)).expect("resolves"),
            PathBuf::from("/home/you")
        );
        // An absolute path is used as typed, never rebased on home.
        assert_eq!(
            resolve_path_in("  /tmp/notes.md  ", Some(home)).expect("resolves"),
            PathBuf::from("/tmp/notes.md")
        );
        // A relative path is taken from home (so a bare `notes.md` does not
        // land in whatever directory the process happens to run in).
        assert_eq!(
            resolve_path_in("notes.md", Some(home)).expect("resolves"),
            PathBuf::from("/home/you/notes.md")
        );
        assert_eq!(
            resolve_path_in("sub/notes.md", Some(home)).expect("resolves"),
            PathBuf::from("/home/you/sub/notes.md")
        );
        // No home at all: a relative path stays relative, `~` fails closed.
        assert_eq!(
            resolve_path_in("notes.md", None).expect("resolves"),
            PathBuf::from("notes.md")
        );
    }

    #[test]
    fn resolve_path_rejects_an_empty_field_and_a_homeless_tilde() {
        let home = Path::new("/home/you");
        assert_eq!(resolve_path_in("", Some(home)), Err(PathError::Empty));
        assert_eq!(resolve_path_in("   ", Some(home)), Err(PathError::Empty));
        assert_eq!(resolve_path_in("~/notes.md", None), Err(PathError::NoHome));
        // Both failures have a line for the card.
        assert!(!PathError::Empty.message().is_empty());
        assert!(!PathError::NoHome.message().is_empty());
    }

    #[test]
    fn format_switch_retargets_only_the_formats_own_extensions() {
        assert_eq!(
            retarget_extension("/tmp/notes.md", ExportFormat::PlainText),
            "/tmp/notes.txt"
        );
        assert_eq!(
            retarget_extension("~/Notas.MD", ExportFormat::PlainText),
            "~/Notas.txt"
        );
        assert_eq!(
            retarget_extension("/tmp/notes.txt", ExportFormat::Markdown),
            "/tmp/notes.md"
        );
        // A name the user typed is otherwise left exactly as it is: no
        // extension is invented and none is dropped.
        assert_eq!(
            retarget_extension("/tmp/notes", ExportFormat::PlainText),
            "/tmp/notes"
        );
        assert_eq!(
            retarget_extension("/tmp/notes.bak", ExportFormat::PlainText),
            "/tmp/notes.bak"
        );
        assert_eq!(retarget_extension("", ExportFormat::Markdown), "");
    }

    #[test]
    fn default_path_lands_in_downloads_when_it_exists() {
        let dir = scratch("default-path");
        assert_eq!(
            default_dir_in(Some(&dir)).join("notes.md"),
            dir.join("notes.md"),
            "no Downloads directory: home itself"
        );
        std::fs::create_dir_all(dir.join(DOWNLOADS_DIR)).expect("Downloads");
        assert_eq!(
            default_dir_in(Some(&dir)),
            dir.join(DOWNLOADS_DIR),
            "a browser saves into Downloads when it is there"
        );
        // No home directory at all: a relative name (the current directory).
        assert_eq!(default_dir_in(None), PathBuf::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The export write is ATOMIC: it replaces the destination through a
    /// sibling temp file and never leaves that temp behind, so an existing
    /// file is only ever swapped whole.
    #[test]
    fn write_file_replaces_atomically_and_cleans_up() {
        let dir = scratch("atomic");
        let target = dir.join("notes.md");
        assert_eq!(
            write_file(&target, "first\n").expect("write"),
            "notes.md",
            "the confirmation names the file"
        );
        assert_eq!(
            write_file(&target, "second\n").expect("replace"),
            "notes.md"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("file written"),
            "second\n"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .expect("scratch dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".mdowser-export-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_card_writes_the_captured_document() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let dir = scratch("save");
        let target = dir.join("notes.md");
        let mut state = ExportState::new();
        state.open(payload("notes"));
        state.open.as_mut().expect("open").path = target.display().to_string();

        let shapes = settle(&ctx, &mut state, 4);
        let actions = click(&ctx, &mut state, bottom_text(&shapes, "Export"));

        assert_eq!(actions.saved.as_deref(), Some("notes.md"));
        assert!(!state.is_open(), "a completed save closes the card");
        assert_eq!(
            std::fs::read_to_string(&target).expect("file written"),
            "# Title\n\nBody\n",
            "the markdown source, exactly as captured"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_destination_needs_the_second_press() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let dir = scratch("replace");
        let target = dir.join("notes.md");
        std::fs::write(&target, "old\n").expect("seed");
        let mut state = ExportState::new();
        state.open(payload("notes"));
        state.open.as_mut().expect("open").path = target.display().to_string();

        // First press: the card stays up, arms the confirmation and does NOT
        // touch the file.
        let shapes = settle(&ctx, &mut state, 4);
        let actions = click(&ctx, &mut state, bottom_text(&shapes, "Export"));
        assert_eq!(actions.saved, None, "the first press never writes");
        assert!(state.is_open(), "the card waits for the confirmation");
        assert_eq!(std::fs::read_to_string(&target).expect("seeded"), "old\n");
        let shapes = settle(&ctx, &mut state, 1);
        assert!(
            texts(&shapes).iter().any(|(t, _)| t == "Replace"),
            "the armed button states the outcome of the next press"
        );
        assert!(
            texts(&shapes)
                .iter()
                .any(|(t, _)| t.starts_with("Click again")),
            "the armed note explains the second press"
        );

        // Second press: the file is replaced and the save reported.
        let actions = click(&ctx, &mut state, bottom_text(&shapes, "Replace"));
        assert_eq!(actions.saved.as_deref(), Some("notes.md"));
        assert!(!state.is_open());
        assert_eq!(
            std::fs::read_to_string(&target).expect("replaced"),
            "# Title\n\nBody\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_in_the_primary_actions_own_frame_re_arms_instead_of_writing() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let dir = scratch("armed-edit");
        // `dir/notes.md` and `dir/./notes.md` are the SAME file reached by
        // two different strings, so the edit below only changes the typed
        // destination — the armed confirmation must still not carry over.
        let target = dir.join("notes.md");
        std::fs::write(&target, "old\n").expect("seed");
        let mut state = ExportState::new();
        state.open(payload("notes"));
        state.open.as_mut().expect("open").path = target.display().to_string();

        // Enter in the destination field is the primary action: the first
        // one arms the confirmation without touching the file.
        settle(&ctx, &mut state, 4);
        let mut frame = input_at(None);
        frame.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let mut out = ctx.run_ui(frame, |root| {
            state.show(root.ctx());
        });
        out.textures_delta.clear();
        assert!(state.is_open(), "the first Enter arms, never writes");
        assert_eq!(std::fs::read_to_string(&target).expect("seeded"), "old\n");

        // ONE frame carrying both the edit (a text event, which replaces the
        // selected default) and the primary action: the frame-start confirm
        // belonged to the old destination, so this press must re-arm instead
        // of writing the file the new text points at.
        let mut frame = input_at(None);
        frame
            .events
            .push(egui::Event::Text(format!("{}/./notes.md", dir.display())));
        frame.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let mut out = ctx.run_ui(frame, |root| {
            let actions = state.show(root.ctx());
            assert_eq!(actions.saved, None, "an edited destination never writes");
        });
        out.textures_delta.clear();
        assert_eq!(
            std::fs::read_to_string(&target).expect("kept"),
            "old\n",
            "the file the OLD confirm was armed for is untouched"
        );
        assert!(state.is_open(), "the press re-arms, the card waits");
        let shapes = settle(&ctx, &mut state, 1);
        assert!(
            texts(&shapes).iter().any(|(t, _)| t == "Replace"),
            "the new destination is armed for its own confirmation"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_keeps_the_card_open_with_its_error() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let dir = scratch("error");
        let mut state = ExportState::new();
        state.open(payload("notes"));
        // A destination whose parent directory does not exist: the card must
        // report it, never fail silently or pretend the save happened.
        let target = dir.join("missing").join("notes.md");
        state.open.as_mut().expect("open").path = target.display().to_string();

        let shapes = settle(&ctx, &mut state, 4);
        let actions = click(&ctx, &mut state, bottom_text(&shapes, "Export"));
        assert_eq!(actions.saved, None);
        assert!(state.is_open(), "a failed save leaves the card up");
        assert!(
            state
                .open
                .as_ref()
                .and_then(|card| card.error.as_deref())
                .is_some_and(|e| e.starts_with("Could not save")),
            "the failure is stated inline: {:?}",
            state.open.as_ref().and_then(|card| card.error.as_deref())
        );
        let shapes = settle(&ctx, &mut state, 1);
        assert!(
            texts(&shapes)
                .iter()
                .any(|(t, _)| t.starts_with("Could not save")),
            "the error line is rendered"
        );
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hovering_an_action_moves_nothing_in_the_card() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let mut state = ExportState::new();
        state.open(payload("notes"));

        let shapes = settle(&ctx, &mut state, 4);
        let mut base = texts(&shapes);
        let button = bottom_text(&shapes, "Export");
        // Same frame with the pointer over the primary action.
        let mut out = ctx.run_ui(input_at(Some(button)), |root| {
            state.show(root.ctx());
        });
        out.textures_delta.clear();
        let mut hovered = texts(&out.shapes);
        let sort = |v: &mut Vec<(String, egui::Pos2)>| {
            v.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.1.y.total_cmp(&b.1.y))
                    .then_with(|| a.1.x.total_cmp(&b.1.x))
            });
        };
        sort(&mut base);
        sort(&mut hovered);
        assert_eq!(base, hovered, "hovering must not move any text in the card");
    }
}
