# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2026-10-14

### Added

- `make check-dist` (`scripts/check-dist.sh`): privacy gate over `dist/` that
  fails when an artifact carries the builder's absolute home / Cargo-registry
  paths, the build host's nodename, a non-zero archive owner or an
  xattr/AppleDouble record. `scripts/build-dist.sh` runs it before writing
  `SHA256SUMS.txt`, and `make validate` includes it.
- Release automation (`.github/workflows/release.yml`): pushing a `v*` tag
  builds every distribution target on fresh GitHub-hosted runners (macOS
  universal `.app.zip`; Linux x86_64/arm64 `.tar.gz` + `.deb`; Windows
  x86_64 `.zip` via the mingw cross; `.rpm` from source in a Fedora
  container; `.flatpak` bundle), re-runs the `dist/` privacy gate over the
  merged set and publishes ONE GitHub Release with a single
  `SHA256SUMS.txt`. No Actions cache: every release compiles from scratch
  and only the packaged files travel between jobs. A manual dispatch
  (`dry_run`) validates the whole pipeline without publishing.

### Changed

- Flatpak builds now target the freedesktop SDK 26.08 runtime (24.08 is
  end-of-life); the `rust-stable` extension branch is pinned in the manifest
  and `scripts/build-flatpak.sh` derives the runtime branch from
  `packaging/flatpak/dev.mdowser.app.yaml` (single source of truth).

### Fixed

- Release binaries no longer embed the builder's absolute home and
  Cargo-registry paths (the username and home layout leaked through panic
  locations, which `strip` cannot remove): `make build`,
  `scripts/build-dist.sh`, `windows.ps1` and the RPM spec's `%build` now
  remap them at compile time with `--remap-path-prefix`
  (`scripts/lib.sh` `export_remap_rustflags`).
- macOS and Windows `.zip` artifacts no longer record the builder's Unix
  uid/gid (`zip -X` drops the Info-ZIP `0x7875` extra field).
- Linux `.tar.gz` and `.deb` payloads no longer carry the macOS build
  machine's `com.apple.provenance` extended attribute as PAX records:
  staging trees are xattr-swept and tarred with `--no-xattrs` where
  supported.
- The RPM spec no longer discloses the build hostname (`%_buildhost`) and its
  payload can no longer inherit the builder's ownership
  (`%defattr(-, root, root, -)`).

## [0.5.0] - 2026-10-14

A feature release on top of a full audited pass: documents without a
frontmatter `title` now name themselves from their first `#` heading, the
native window remembers its geometry (and opens centered instead of at the
OS cascade position), and the "No markdown here" screen gains an accent
action that hands the target to the OS default handler — plus a
native-feeling tab-rail reveal, neutral product wording across the docs,
metadata and comments, and fixes across the stores, the packaging and the
error view.

### Added

- Documents without a frontmatter `title` now name their tab, the OS window
  title, the omnibox page-info card and the default bookmark/history entry
  with their first `#` heading; the schemeless URL stays the last resort.
  The title is derived once per fetch from the already-parsed events
  (background thread, never a second parse), bounded and sanitized exactly
  like a frontmatter title, and a declared `title` always wins.
  (`src/markdown/document/title.rs`, `src/protocol/client/`,
  `src/browser/tab/`, `src/history/`, `src/bookmarks/`, `docs/`.)
- The native window now remembers its geometry: the last position, size and
  maximized state are stored encrypted in `settings.bin` (like the theme and
  zoom; written debounced after the window is still, and once more on exit)
  and restored on the next launch, with a centering fallback when the monitor
  it was on is gone. (`src/settings/`, `src/app/window.rs`, `docs/`.)
- The error screen's "No markdown here" family gained a large accent
  "Open in your browser" action under the requested address (local files say
  "Open with the default app"): it hands the resolved target to the OS
  default handler — the default browser for web pages, the file's default
  app for local ones — through egui's `OpenUrl` command (the already-embedded
  integration, no new dependency). The address is re-resolved through the
  app's own navigability parser at click time, so pasted credentials are
  stripped before the OS sees anything; no other error family offers it.
  (`src/ui/error_view.rs`, `src/ui/theme/widgets.rs`, `docs/`.)

### Changed

- Assets layout: the bundled Noto faces moved to `assets/fonts/` (every
  `include_bytes!` site, the `build_support/gen_fonts.rs` inputs and the
  `THIRD_PARTY_NOTICES.md` paths updated; embedded bytes unchanged), and the
  omnibox builtin suggestion list moved from a Rust `const` to
  `assets/builtin_sites.txt` (one `<url><TAB><title>` line per entry),
  embedded with `include_str!` and parsed once per process by
  `builtin_sites::builtin_suggestions()`. The list content and order are
  unchanged. (`assets/`, `src/`, `build_support/`, `docs/`.)
- Tab rail labels (tab titles, `+ New Tab` and `History`) draw the regular
  face again: the rail is chrome, and `theme::strong` (the bundled Noto
  Sans pinned to `wght = 700`) stays for document emphasis (headings,
  `**strong**` runs, callout titles, footnote refs) and the titled
  surfaces (History, Bookmarks, Preferences, the error view, omnibox
  suggestions, tonal buttons). (`src/ui/tabs/rows.rs`,
  `docs/INTERFACE.md`.)
- The tab rail's hover expansion now settles like a native sidebar: the
  width tween uses a quintic ease-out (fast start, gentle tail, mirrored
  when closing) instead of `cubic_out`, and the titles and action labels
  fade in while drifting the last few pixels into their column, each tab row
  a bounded beat after the one above it. Geometry at rest is unchanged.
  (`src/ui/tabs/`, `docs/INTERFACE.md`.)
- Wording: user-facing copy (README, AppStream metadata, man page, docs and
  source comments) now describes the Markdown engine, the palette and the
  layout in neutral terms instead of naming third-party products, and the
  README gained a Trademarks note. Functional references that stay
  (CommonMark/GFM as a spec comparison, platforms, dependencies, fonts and
  curated links) are nominative only.
  (`README.md`, `docs/`, `packaging/`, `src/`, `CHANGELOG.md`.)

### Fixed

- Scrolling pages now end with a small blank margin: the responsive content
  gutter is symmetric, so the last line of a long document (and of the Home
  and error pages) sat close to the sheet's bottom edge once scrolled all
  the way down. Every scrolling page closes with a shared
  `theme::spacing::PAGE_END` (24px) pad under its last element; the top
  gutter and the scrolled-edge clipping are unchanged. (`src/ui/theme/`,
  `src/ui/home.rs`, `src/viewer/`, `docs/`.)
- The internal egui id-salt parameters (`ui::omnibox::address_id`,
  `ui::scroll_id_key`, `ui::remember_scroll`, `ui::forget_scroll_state`) are
  named `id_salt` instead of `salt`, so static-analysis heuristics for
  hard-coded cryptographic values no longer misread a UI hash namespace as
  cryptographic material (no behavior change; the stored ids are identical).
- Initial window position on Windows: the window opened at the OS
  `CW_USEDEFAULT` cascade position (low and off to one side). It is now
  centered on the primary monitor while it is created
  (`NativeOptions::centered`), and the first frame sizes it to 80 % of the
  monitor, clamped between 640×480 and 1680×1050, instead of a fixed
  1100×750. (`src/main.rs`, `src/config/`, `src/app/window.rs`, `docs/`.)
- The encrypted-store background writer's bounded "newest wins" queue is now
  keyed per target directory, so two writers to DIFFERENT data directories
  can no longer drop each other's queued blob. The app writes a single data
  directory, so this is a latent-robustness fix (it also makes the parallel
  test suite deterministic). (`src/store/writer.rs`, `docs/`.)
- The tab rail's right-edge separator no longer pops on or off when the hover
  overlay opens or closes: it is now drawn in the collapsed state too
  (smaller and fainter — a dimmer hairline in dark mode) and its strength
  follows the open tween, so the rail/page boundary is continuous.
  (`src/ui/tabs/`, `docs/INTERFACE.md`.)
- The error screen's red status dot sits centered with the family title
  again: it is painted on the title's laid-out rect instead of on its own
  shorter slot, which the horizontal row top-clamps and parked ~6px above
  the title. (`src/ui/error_view.rs`.)
- Fixed-size buttons (`theme::tonal_button_sized` and the new
  `theme::primary_button_sized`: Retry and the external-open action) center
  their label again. `Button::min_size` aligned the label to the PARENT
  layout's cross axis — centered under a centered/right-to-left parent, but
  visibly left-parked inside the error view's left-aligned column; the
  shared body now uses `add_sized` (`centered_and_justified`), so the label
  centers whatever the surrounding layout is. (`src/ui/theme/widgets.rs`.)

## [0.4.6] - 2026-10-14

A professionalization pass: one type/stroke/spacing scale for the whole
chrome (pixel-identical), the interned palette borrowed on hot draw paths,
fail-closed packaging helpers, one version-parse recipe, and the root/docs
sync closed — plus the native Windows bootstrap.

### Added

- `windows.ps1`: native Windows bootstrap (winget-only, idempotent),
  equivalent to `make` without `make`: installs Rustup 1.98, MSVC Build Tools
  + the C++ workload, NASM, Git (Git Bash), Python 3.12 (with a `python3`
  shim) and LLVM (`llvm-strip`), then `cargo build --locked --release` plus
  the host `.exe` zip via `scripts/build-dist.sh --host-only`. Flags
  `-Test`/`-Validate`/`-SkipBuild`/`-SkipDist`/`-SkipSetup`/`-NoElevate`.
  (`windows.ps1`, `docs/BUILDING.md`.)
- `.exe` strip falls back to `llvm-strip` when `x86_64-w64-mingw32-strip` is
  missing, so a native MSVC build (which ships LLVM, not mingw) no longer
  ships unstripped; `$STRIP` still wins. (`scripts/build-dist.sh`.)

### Changed

- Standardization: chrome type, strokes and gaps share one scale —
  `theme::typography` (`UI_COMPACT`/`UI_SMALL`/`EYEBROW`/`UI_LINK`/`BUTTON`/
  `DROPDOWN`/`ROW`/`HEADING`/`DIALOG_TITLE`/`DISPLAY`), `theme::stroke`
  (`HAIRLINE`/`ICON`/`EMPHASIS`) and `theme::spacing` (`XS`/`S`/`M`/`L`/`XL`)
  back the omnibox card, suggestion rows, history rows, Preferences dialog,
  task checkboxes, badges, cards and tables instead of repeated literals
  (same pixel values; graphics stay pixel-identical). (`src/ui/theme/`,
  `src/ui/`, `src/viewer/`, `docs/ARCHITECTURE.md`.)
- Hot draw paths borrow the interned palette (`theme::scheme_ref`) instead of
  copying the 50-field `Scheme`: `tonal_button_sized`, `content_frame` and
  `paint_content_fade` no longer rebuild it per button/surface; the one-off
  `theme::scheme_for` copy stays for non-hot callers.
  (`src/ui/theme/`, `docs/ARCHITECTURE.md`.)
- Build tooling, fail-closed and best-effort: `render_metainfo` rejects a
  version outside `[0-9A-Za-z._+-]` and escapes `&`/`\` before the `sed`
  replacement; `install_linux_tree` pre-flights every source (binary,
  desktop entry, metainfo, icons, SVG, man page) before touching the
  destination; `mk_app` checks its binary/icon sources and returns instead of
  aborting; `do_mac` guards `lipo`/`mk_app` and `do_host` guards the host
  build so `--all` warns and continues with a reason; `MODE` default and the
  `EXIT` trap read without POSIX trivia; intentional word-splits carry
  `shellcheck` disables; `deps.sh`/`validate.sh` document their `set -u`
  (best-effort, never abort). (`scripts/`, `docs/BUILDING.md`.)
- Single version-parse recipe: the RPM spec, the Flatpak manifest and the
  `BUILDING.md` container recipes use the same scoped `[package]` parse as
  `scripts/packaging.sh` `app_version`; the spec comment matches the real
  `build-dist.sh --rpm` defines and the Flatpak manual recipe matches
  `build-flatpak.sh` (`--state-dir`). (`packaging/`, `docs/BUILDING.md`.)
- Version-control policy is documented and active: the repository lives on
  GitHub (`origin`) and commits are created only on explicit request, never
  automatically, with no AI attribution in messages. (`AGENTS.md`,
  `docs/CONTRIBUTING.md`.)
- Package and crate metadata point at the upstream repository instead of a
  placeholder: the RPM `URL`, a `Homepage` field in the `.deb` control file,
  the AppStream `homepage`/`bugtracker`/`vcs-browser` links, the Cargo
  `homepage`/`repository` fields and the man page homepage. (`packaging/`,
  `scripts/build-dist.sh`, `Cargo.toml`.)
- README: dropped the "PRs welcome" badge; the build section points native
  Windows at `windows.ps1` and the security link uses the `docs/` label like
  every other link. (`README.md`.)
- Docs sync: `BUILDING.md` no longer offers `choco` for NASM (winget, like
  `windows.ps1`), states that the `scripts/` equivalents skip `make deps`
  and the host build, names the RPM as copied verbatim from `rpmbuild`, and
  attributes the `.exe` strip fallback to `build-dist.sh`; `ARCHITECTURE.md`
  documents the new theme scales and the pre-flighted Linux tree.
  (`docs/`, `packaging/`.)

## [0.4.5] - 2026-10-13

A hardening, rendering and packaging pass on top of a performance pass: one
parser pass per document (the clipboard text is derived from the owned
events), bounded normalization scans on hostile input, O(1) settled paths in
the media/settings/history stores, the fetch-slot counter fix, and fixes
across security, the renderer and every package format.

### Fixed

- Pasted credentials in the no-separator spelling
  `scheme:user:pass@host` were echoed verbatim by the HTTPS and local-file
  parsers (error view and "Copy log"); the shared log sanitizer now removes
  that userinfo too, and every "bad address" message goes through a bounded,
  credential-free echo. (`src/util.rs`, `src/protocol/client/mod.rs`,
  `src/protocol/local.rs`.)
- A relative `HOME`/`APPDATA`/`LOCALAPPDATA` resolved the per-user data
  directory — including `wipe_all_data`'s recursive delete and `~/`
  expansion — under the current working directory; every base must now be
  absolute, exactly like `XDG_DATA_HOME` already was. (`src/util.rs`,
  `src/settings/platform.rs`, `src/protocol/local.rs`.)
- Direct (no-hard-link) store-key creation could return a key that was no
  longer the file on disk: a concurrent launcher classifying the partial
  file as corrupt deletes and replaces it, and the writer kept encrypting
  with its orphaned in-memory key. The returned value is now read back from
  the path, fail-closed. (`src/store/crypto/mod.rs`.)
- `.deb` packages were uninstallable: two `#` comment lines had been written
  into the binary control file ("field name '#' must be followed by
  colon"). The comments moved out and `mk_deb` now refuses a control file
  that contains a comment line. (`scripts/build-dist.sh`.)
- Flatpak exports did not carry the application id (`rename-desktop-file` /
  `rename-icon` were missing), so an installed bundle never appeared in
  menus or software centers under its own name and icon; the manifest also
  shipped the committed metainfo release (stale after every bump) and let
  flatpak-builder copy `target/`/`dist/` (multi-GB) into its source tree.
  The manifest now renames the exports, renders version/launchable/icon for
  the bundle, skips build output and strips the binary.
  (`packaging/flatpak/dev.mdowser.app.yaml`, `scripts/build-flatpak.sh`.)
- The footnote back-arrow did nothing when the definition precedes its
  reference: the reference-rect memo was stamped with a per-FRAME
  generation, so it was stale by the time the earlier back-arrow rendered.
  The stamp is now the per-document salt. (`src/viewer/inline/mod.rs`,
  `src/viewer/markdown/mod.rs`.)
- Tiny images were stretched by independent per-axis size floors (a 2×40
  became 4×40); the 4px clickable floor now scales both sides uniformly.
  (`src/viewer/image.rs`.)
- The Preferences dialog was a fixed 720pt tall and left ~150px of empty
  surface under the footer; it is content-sized now, top-anchored so arming
  a two-click confirmation never moves the control above it. Long
  persistence errors are readable via hover tooltips and the history search
  field is vertically centered. (`src/ui/preferences/mod.rs`,
  `src/ui/history_page/mod.rs`.)
- Disabled chrome icons painted at 55% opacity (~2.3:1 on the light chrome,
  nearly invisible); they use the muted full-opacity tone of disabled text
  buttons. Icon strokes no longer thin out at 175% zoom.
  (`src/ui/icons/mod.rs`.)
- The global fetch/local-read slot counters could underflow to
  `usize::MAX`: a FAILED acquisition still ran the slot guard's `Drop`
  (`bool::then_some(Guard)` constructs the guard eagerly and drops it on
  `None`), decrementing a counter that was never incremented. After a
  handful of cancelled or queued navigations, every later document load
  waited out the 20s slot timeout and failed with
  `busy: too many concurrent fetches`, and the test suite starved its local
  reads. Both guards are now constructed only on success, and the full
  suite passes on the default thread count (it used to fail 5 local tests
  after minutes of slot waits). (`src/protocol/client/mod.rs`.)

### Changed

- Copy text is derived from the OWNED parse events
  (`markdown::document::plain_text_from_events`) instead of running the
  Markdown parser a second time over the normalized body; a corpus test
  asserts byte-identical output against the body oracle across callouts
  (split markers included), code, skip links, tables, tasks, footnotes,
  comments, metadata, autolinks and emoji. (`src/markdown/document/plain.rs`,
  `src/protocol/client/fetch.rs`.)
- The structural `:::` and MDX/component normalization passes run under a
  per-document scan budget
  (`MAX_STRUCTURAL_SCAN_LINES` / `MAX_MARKUP_SCAN_LINES`), and `strip_line`
  never rescans a line for a closer it cannot find (`<` past the last `>`,
  `{` past the last `}`, plus a per-line budget): a body of unmatched
  openers can no longer burn O(n²) CPU on the fetch thread. Malformed
  blocks still fall back to literal source. (`src/markdown/document/`.)
- `ImageCache::poll` returns in O(1) when nothing is queued or loading: it
  used to walk every entry three times per tab per frame even for settled
  pages. (`src/media/cache/mod.rs`.)
- Settings loads the shared `store.key` once per process (it was re-read,
  re-validated and permission-checked on every preference and session
  write, on the UI thread) and the startup validation seeds the plaintext
  body cache, so a valid `settings.bin` is read + decrypted once per launch
  instead of twice. (`src/settings/file.rs`, `src/settings/session.rs`.)
- History keeps an incremental plaintext byte total: `trim` answers "under
  both caps" in O(1), so a recorded visit no longer rescans up to 20 000
  entries (and a save no longer scans a settled store). (`src/history/`.)
- The session snapshot is built only when its save is actually due, not on
  every frame during a failed-save backoff. (`src/app/run.rs`.)
- `ui::tabs` no longer rebuilds the collapsed rail list while the open
  overlay covers it (pointer inside the overlay OR above it, the toolbar
  area that keeps it open): the old rows were laid out under an opaque
  overlay for nothing, up to `MAX_TABS` rows per frame.
  (`src/ui/tabs/mod.rs`.)
- Home bookmark tiles memoize their laid-out two-row name per URL
  (invalidated by the store generation, the theme and the tile width): the
  grid rebuilt one `String` + one `LayoutJob` per visible tile per frame.
  (`src/ui/bookmarks/tile.rs`, `src/ui/bookmarks/mod.rs`.)
- `markup_pass` re-masks into the SAME buffer after stripping comment spans
  (`mask_code_into`), saving one body-sized allocation per pass.
  (`src/markdown/document/normalize_links.rs`,
  `src/markdown/document/normalize_markup.rs`.)
- Color emoji rasterize on background workers: a texture miss (first
  sighting, or a zoom/DPI change re-bucketing every visible glyph) no longer
  shapes + parses + rasterizes inside the frame. The monochrome fallback
  holds the line, `EmojiCache::poll` uploads the finished texture, and a
  process-wide job cap (`config::MAX_CONCURRENT_EMOJI`) keeps a burst of new
  emoji from spawning one thread per cluster; the bookmark-tile glyph cache
  (`ui::icons`) shares the same pipeline and is polled once per frame.
  (`src/media/emoji/mod.rs`, `src/ui/icons/mod.rs`, `src/browser/tab/`,
  `src/app/run.rs`.)
- `FaviconCache` answers `has_pending` and gates `poll` in O(1) from
  loading/backoff/failed counters (it used to walk up to 256 entries three
  to four times per frame, settled included); the due-backoff walk only runs
  once the earliest deadline passes, the saturated-cache flag is O(1), and a
  probe deferred by the image-slot limiter now records a short backoff
  instead of re-parsing the URL on every following frame.
  (`src/media/favicon.rs`.)
- Bookmark saves serialize + encrypt ON THE WRITER THREAD: the store hands
  the worker an entries snapshot (`WriteJob::build`) instead of building a
  blob of up to 48 MiB on the UI thread, so a save (and a failing-disk retry
  loop) can no longer copy + AEAD megabytes inside a frame. The synchronous
  test/exit path keeps the inline build. (`src/store/writer.rs`,
  `src/bookmarks/persist.rs`.)
- `history::format_age` returns a borrowed label for its fixed buckets
  ("just now", "yesterday", "1 week ago", ...), so rendering the history
  page no longer allocates one `String` per visible row per frame; the
  no-pipe pre-reject keeps `has_oversized_table_line` from walking every
  line of a document that cannot hold a table.
  (`src/history/persist.rs`, `src/markdown/document/mod.rs`.)
- Allocation/copy trims on hot paths: code-block galleys paint the visible
  slice directly; heading ids clone only while an anchor is pending;
  `strip_scheme_everywhere` fast-paths without lowercasing the input;
  `sanitize_title` returns its buffer instead of cloning a fitting title;
  `norm_lang` and `is_badge_close` compare without allocating; plain lines
  in a markup pass copy in place; the history search reuses the omnibox's
  ASCII in-place comparator; bookmark byte-trim drains in one pass;
  `read_regular_file` reads 64 KiB chunks; the Preferences palette is
  interned (`theme::scheme_ref_ctx`); the app icon decodes once per
  process; the autolink domain check no longer allocates a `Vec` per
  candidate word. (`src/util.rs`, `src/assets.rs`, `src/markdown/`,
  `src/ui/`, `src/bookmarks/persist.rs`, `src/settings/platform.rs`.)
- Emoji rendering walks text segments through a callback instead of building
  a `Vec` per fragment per repaint, and probes for a color emoji
  allocation-free. (`src/media/emoji/mod.rs`, `src/viewer/inline/mod.rs`.)
- Markdown normalization: `<script>`/`<style>`/`<pre>` region scanning
  compares case-insensitively without lowercasing every line, component
  markers are shape-gated before tag parsing (a non-marker 512 KiB block is
  no longer rescanned every frame), and component/callout expansion is
  depth-capped like the `:::` pass.
  (`src/markdown/document/normalize_markup.rs`.)
- Settled tabs no longer swap their whole state enum on every poll (early
  return for non-`Loading` states). (`src/browser/tab/poll.rs`.)
- MDX tab labels elide to the sheet (per-cell cap) and carry accessible
  names/selected state plus a focus ring, as do accordion headers; the code
  card's language chip truncates and keeps the full value in a tooltip.
  (`src/viewer/markdown/components.rs`, `src/viewer/code/mod.rs`.)
- `theme::strong` is applied at every remaining bare `.strong()` site
  (History H1 and rows, Bookmarks header, omnibox suggestion titles, tab
  titles, tonal buttons), so "bold" always means the bundled bold face.
  (`src/ui/`.)
- Build tooling: warm builds no longer hash 45 MB of font bytes
  (`gen_fonts` checks size/mtime first and falls back to the content hash);
  generated icons are written atomically (temp + rename) so concurrent
  cargo invocations cannot observe a half-written PNG (`build_support/`).
  `build-dist.sh` cleans stage dirs on abort, normalizes tar ownership, and
  no longer lets macOS AppleDouble `._name` sidecars (extended-attribute
  metadata created by `cp`/`install`) land in the `.deb` payload or the
  `.tar.gz` artifacts; it also re-roots the RPM source tarball with bsdtar
  too, and `packaging.sh` scopes the version parse to `[package]` and
  chmods the rendered metainfo 0644; `make deps` installs `flatpak-builder`
  on apt and uses Arch's `python` package name; `install.sh` sets
  `COPYFILE_DISABLE=1` for the same reason. (`scripts/`.)
- `THIRD_PARTY_NOTICES.md` gained the full direct-dependency license table;
  `docs/ARCHITECTURE.md`, `docs/BUILDING.md` and `packaging/README.md` were
  updated for the changes above. A stale duplicate `[Unreleased]` section
  (already released content, stranded mid-file) was removed and its one
  unreleased entry folded into 0.4.3.

## [0.4.4] - 2026-10-13

A packaging pass: Linux distributions get real desktop integration (a
Software-center page, a proper desktop entry, a scalable icon and a man page)
shared by the `.deb`, the new source-built `.rpm`, the new Flatpak manifest
and `make install`, and the Windows executable now carries its identity
(version info and a DPI/version manifest) in PE resources.

### Added

- Linux packaging is generated from ONE shared file set
  (`packaging/linux/`, staged by `scripts/packaging.sh` `install_linux_tree`),
  so the `.deb`, the `.rpm`, the Flatpak build and a manual install cannot
  drift: a polished desktop entry (registered categories, keywords,
  `StartupNotify`, `TryExec`, Markdown MIME types), AppStream metadata
  (`dev.mdowser.app.metainfo.xml`, the page GNOME Software and KDE Discover
  render, with its `<release>` pinned to the `Cargo.toml` version at package
  time), a scalable hicolor icon beside the PNG ladder, and a `mdowser.1` man
  page. The `.deb` control gains `Installed-Size`, `Rules-Requires-Root: no`
  and `Section: web`. (`packaging/`, `scripts/packaging.sh`,
  `scripts/build-dist.sh`, `docs/BUILDING.md`, `docs/NAVIGATION.md`.)
- `packaging/rpm/mdowser.spec` plus `make dist-rpm`
  (`scripts/build-dist.sh --rpm`) build an RPM from the full source tree with
  the system `rpmbuild`; on a Fedora/RHEL/openSUSE host `make`/`make dist`
  now also produce it. (`packaging/rpm/`, `Makefile`, `scripts/build-dist.sh`.)
- `make install` (`scripts/install.sh`) installs the binary, desktop entry,
  AppStream metadata, icons and man page under `DESTDIR`/`PREFIX`; it is the
  RPM spec's `%install`. (`scripts/install.sh`, `Makefile`.)
- `packaging/flatpak/dev.mdowser.app.yaml` plus `make flatpak`
  (`scripts/build-flatpak.sh`) build one artifact for every Linux
  distribution (needs `flatpak-builder` and vendored crate sources).
  (`packaging/flatpak/`, `scripts/build-flatpak.sh`, `Makefile`.)
- `make validate` (`scripts/validate.sh`) checks the packaging metadata
  best-effort — AppStream metadata (well-formedness and appstreamcli/
  appstream-util), the desktop entry (`desktop-file-validate`), the RPM spec
  (`rpmspec -P`, `rpmlint`) and the Flatpak manifest — skipping, never
  failing, when a tool is not installed. (`scripts/validate.sh`, `Makefile`,
  `docs/BUILDING.md`.)
- `make deps` (`scripts/deps.sh`) installs the build and packaging
  dependencies best-effort and idempotently: Rust (`rustup` + stable on
  macOS), `nasm`, `python3`, the macOS cross tools (`zig`, `cargo-zigbuild`,
  `mingw-w64`, rustup targets) and the packaging tools (`desktop-file-utils`,
  `appstream`, `appstream-glib`, `rpm`, `lintian`, PyYAML). Homebrew on macOS,
  `apt-get`/`dnf`/`pacman` on Linux; a tool already present is left alone, a
  package that cannot be installed is a `SKIP`, and `SKIP_SETUP=1` is a dry
  run. Groups: `toolchain`, `cross`, `packaging` (`make deps DEPS=packaging`).
  (`scripts/deps.sh`, `Makefile`, `docs/BUILDING.md`.)
- Windows executables embed a `VERSIONINFO` resource (product/file
  description, version and copyright, so Explorer's Details tab and the task
  manager show real values instead of blanks) and an application manifest
  (`RT_MANIFEST` id 1: per-monitor DPI awareness, supported Windows versions,
  UTF-8 active code page, long-path awareness, segment heap), on top of the
  icon already embedded. `main` sets `windows_subsystem = "windows"` so no
  console window opens behind the GUI. (`build.rs`, `src/main.rs`,
  `docs/ARCHITECTURE.md`, `docs/BUILDING.md`.)

### Changed

- `make` and `make dist` install every dependency the host can use
  (`make deps`) before building, and each entry point now produces exactly the
  platform's artifact set: macOS ships ONLY the universal `.app.zip` (the
  loose per-arch `.tar.gz` are gone), Linux a `.tar.gz` + `.deb`/`.rpm` + the
  Flatpak bundle, Windows the `.exe` zip. `make` is the current platform;
  `make dist` is every platform the host can cross-build. (`Makefile`,
  `scripts/build-dist.sh`, `scripts/build-flatpak.sh`, `docs/BUILDING.md`,
  `README.md`.)
- The Flatpak build is now part of `make dist` (`--flatpak` / `do_flatpak`)
  and exports the DISTRIBUTABLE bundle `dist/mdowser-<ver>.flatpak`
  (`flatpak-builder --repo` + `flatpak build-bundle`) instead of installing
  the app; the freedesktop runtimes are pulled best-effort (`SKIP_SETUP=1`
  opts out). `make flatpak` / `make dist-flatpak` build the same bundle.
  (`scripts/build-flatpak.sh`, `scripts/build-dist.sh`, `Makefile`,
  `packaging/README.md`, `docs/BUILDING.md`.)
- All product-credit strings are now plain `MDowser`: the Windows
  `VERSIONINFO` `CompanyName`/`LegalCopyright`, the `.deb` `Maintainer`, the
  AppStream `developer` name, the man page author and the MPL header of every
  source file. (`build.rs`, `packaging/`, `scripts/`, `src/`.)

### Fixed

- The `dist/` checksum loop aborted the whole run (`set -e`) whenever the name
  that sorts LAST was the skipped `SHA256SUMS.txt`, which is exactly what a
  `.app.zip` starting with an uppercase `M` produces (`MDowser-…` sorts before
  `SHA256SUMS.txt`; the lowercase `mdowser-…` used to sort after it and mask
  the bug). The skip is an `if`, never a trailing `&&`.
  (`scripts/build-dist.sh`.)

## [0.4.3] - 2026-10-13

A code-size and release-hardening pass: inline code and code cards draw at the
BODY size (code is exactly as wide as the prose around it, on the shared 24px
row grid) and the footnote back-arrow closes the note text on its last line
instead of opening a row of its own, plus fail-closed arithmetic in shipped
binaries and a batch of doc/comment drift closed.

### Added

- macOS completes the system menu bar — which macOS always shows, whether or
  not the app declares one — with the browser's commands: an Application menu
  (About MDowser, Preferences… `Cmd+,`, Services, Hide/Hide Others/Show All,
  Quit), File (New Tab `Cmd+T`, Close Tab `Cmd+W`), Edit (Copy `Cmd+C`,
  Paste `Cmd+V`, Select all `Cmd+A` — deliberately no more), View (Reload
  `Cmd+R`, Zoom In/Out/Actual Size), History (Show History `Cmd+Y`) and
  Window. Each item replays the very shortcut the keyboard does, so the menu
  and the keys share one handler; the bar replaces the bare
  About/Hide/Quit the toolkit left behind. Windows and Linux, which have no
  menu bar at all, are unchanged. (`src/app/menu.rs`, `Cargo.toml`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Omnibox builtin suggestions gain `https://modelcontextprotocol.io/`
  ("Model Context Protocol"). (`src/builtin_sites.rs`.)

### Changed

- Inline code and code cards draw at the BODY type size (16px) instead of
  85% of it: code is exactly as wide as the prose around it, the code font
  and the text font share one size token (`theme::BODY_FONT_SIZE`) and one
  24px line grid, and the inline pill's `.2em .4em` padding and the pill
  height cap are re-derived from that size. The pill's baseline is now
  POSITIONED (the run is translated onto the body baseline) instead of
  re-metricized through a synthetic `line_height`, because the bundled mono
  cut's face metrics no longer fit the cap that way. (`src/ui/theme/`,
  `src/viewer/code/`, `src/viewer/inline/`, `docs/RENDERING.md`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- The omnibox security indicator is a real padlock now (a closed shackle
  over the body; the old open arc read as a briefcase handle) and clicking
  it opens a brief page-info card under the capsule with the facts of the
  page actually rendered: the kind, the document's own title when it has
  one, the exact address WITH its scheme (the field hides `https://`) and
  where the content came from (HTTPS, local file or an internal page). The
  card is an overlay (it never shifts the capsule) and closes on the next
  indicator click, on `Esc`, on a click outside it, on `Ctrl/Cmd+L` and on
  any navigation; its exact address stays bounded
  (`config::SITE_INFO_ADDRESS_CHARS`). (`src/ui/omnibox/`, `src/ui/icons/`,
  `src/browser/tab/`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Release binaries compile with `overflow-checks = true`: every arithmetic
  that could overflow already used `wrapping_*`/`saturating_*`, so a silent
  wrap in new code (a budget, a size, an offset into a body) now panics
  loudly instead of feeding a wrong number to a cap check. `panic = "abort"`
  stays off on purpose (the image pipeline catches decoder panics).
  (`Cargo.toml`.)
- Docs and comments synced with the code: the CI-only phrasings in the lint
  comments (there is no workflow automation in this repository) now name the
  real gate (`scripts/build-test.sh`), and the OS/CLI open-target cap is
  documented as the `MAX_TABS` list it is.
  (`Cargo.toml`, `rust-toolchain.toml`, `docs/ARCHITECTURE.md`.)

### Fixed

- The footnote back-arrow now closes the note's TEXT: it is drawn as the
  note's own last inline box, so it sits right after the final word on the
  same line (and wraps like a word when the line has no room left) instead
  of opening a row of its own under the note, which read as a separate
  paragraph. A note whose last block cannot flow inline (a code fence, a
  list, a quote, a table) keeps a small right-aligned trailing row, so the
  back-reference is never lost. (`src/viewer/markdown/`,
  `src/viewer/inline/`, `docs/RENDERING.md`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- The macOS menu bar no longer carries AppKit's own "Show Tab Bar"/"Show All
  Tabs" items: automatic window tabbing is opted out at startup
  (`NSWindow.setAllowsAutomaticWindowTabbing(false)`, called before any
  window exists), so the system cannot inject its window-tab-group entries
  into the View menu — MDowser's tabs are the in-window rail, not macOS
  window tabs. "About MDowser" also shows the app's own icon now (the
  embedded 256px raster) instead of AppKit's generic placeholder folder.
  (`src/main.rs`, `src/app/menu.rs`, `src/app/mod.rs`, `Cargo.toml`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Home bookmark tiles no longer cut their own name: the tile reserved only
  30px for the name while two 12.5pt rows need ~34px at the bundled font's
  metrics, so the second row spilled past the tile's bottom edge — the text
  clip rect shaved its descenders (and the `…` of an elided title) and the
  hover highlight, which IS the tile rect, cut straight through the line.
  The tile is 96px tall and the name block starts 2px higher (`TILE_H`,
  `TITLE_TOP`), so the icon, both rows and the hover wash stay inside the
  tile with real air at the bottom. (`src/ui/bookmarks/`.)
- The bookmark grid's section header ribbon is now vertically aligned with
  the "Bookmarks" title: the line box centered the 18px icon slot on the
  row while its ink is a touch taller than the 15pt caps, so it read a hair
  high. One point down on the painted glyph (the slot stays in the layout)
  lines the two optical centers up (`HEADER_ICON_DY`). (`src/ui/bookmarks/`.)
- The bookmark grid's reserved header block now includes the item spacing
  that follows the header row, so the grid's ScrollArea gets its full
  height: the viewport used to come out 4px short and clipped the last
  tile row's bottom — the hover wash (the tile rect) lost its rounded
  bottom corners and the rename field lost its bottom border.
  (`src/ui/bookmarks/`.)
- The inline bookmark rename is a compact one-line field now: it paints the
  draft at the tile name's own size (12.5pt, not the 16pt `Body` fallback
  that looked like a different, much bigger font), hugs the text instead of
  filling the whole two-line name slot, stays centered while the draft fits
  and scrolls with the caret once it is wider than the field (centered text
  that no longer fit simply sat still, so the end being typed was never
  visible). (`src/ui/bookmarks/`.)
- The omnibox suggestion popup no longer paints its first row as selected
  while `Enter` still submits the typed address: nothing is highlighted
  until the user picks a row, so the highlight always means "`Enter` opens
  this", and the first `Down`/`Up` now selects the first/last row instead of
  skipping it. The row fill also uses a token visible on the popup in dark
  mode (it was `container_high`, identical to the popup's own `window_fill`
  there, so no row highlight was visible at all). (`src/ui/omnibox/`,
  `docs/INTERFACE.md`, `docs/NAVIGATION.md`, `docs/ARCHITECTURE.md`.)
- A fitting table is no longer mistaken for an overflowing one by a
  floating-point rounding ulp: the grow branch of `distribute_columns`
  summed a hair past the usable width (`86.25 + 340` grown to fill 757 came
  out `757.00006 > 757`), and `flush_table` reads `sum > usable` as "even
  the floors do not fit", re-laying every column out for horizontal
  scrolling capped at `scroll_max`. On a real-world pricing table
  the second column collapsed from ~604px to 128px and wrapped every couple
  of words while its card stayed full width. A rounding residual is now
  taken off the widest column (never below `min_col`, so a REAL overflow
  still scrolls). (`src/viewer/table/render.rs`, `docs/ARCHITECTURE.md`.)
- A fresh navigation to an already-seen page no longer reuses its saved
  scroll offset: the offset belongs to the (tab, URL) pair, so following a
  link back to a visited page landed mid-document. New navigations start
  at the top now; only Back/Forward/Refresh restore the reading position
  (a cross-page `#fragment` still scrolls to its target). (`src/browser/tab/`,
  `src/viewer/mod.rs`, `docs/ARCHITECTURE.md`.)

## [0.4.2] - 2026-10-13
A hardening, rendering and rendering-cost pass: the shared store key is
tightened on its OPENED handle (`O_NOFOLLOW`, never a path-based `chmod`), a
failed "Delete all data and close" keeps the app open instead of exiting as
if the data were gone, a corrupt `restore_tabs` value can no longer delete the
encrypted session, table CELLS are capped per frame (a 4096-column table used
to bypass the block budget and lay out ~20M cells per frame), and a batch of
rendering bugs is fixed (the hidden-code-lines notice painted over the first
row, same-titled accordions sharing one open state, a known `#fragment` being
dropped before it rendered, heading badges flattening to text, grapheme-split
truncation).

### Added

- Structural `:::` custom containers render as the same natives as their
  JSX equivalents: `:::tabs`/`:::tab`, `:::code-group`, `:::steps`/`:::step`,
  `:::card` and `:::accordion`/`:::details` become the same inert
  `<mdowser-cmp …/>` layout markers (tab/step labels via bare text or
  `label=`/`title=`, cards via `title=`/`href=`), markup-level only like
  every other component — no JavaScript is executed, unknown names, missing
  closers and fenced code stay literal, and column-0-only openers never
  break out of a list item or quote. The attribute scanners also terminate
  on malformed input now (garbage between pairs is skipped instead of
  rescanning forever). (`src/markdown/document/`, `README.md`,
  `docs/RENDERING.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- The bookmark grid is keyboard-reorderable: `Alt+Left`/`Alt+Right` on the
  FOCUSED tile move it one display slot (the keyboard equivalent of the
  drag, which was the only way to arrange the grid). The move mirrors the
  newest-first display order onto the insertion-ordered store
  (`ui::bookmarks::move_indices`) and never opens the page.
  (`src/ui/bookmarks/`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)

### Fixed

- The address bar no longer navigates to a suggestion the pointer merely
  sits over: `hovered()` is positional, so a stationary pointer over the
  popup (it opened under it, or the list rebuilt while typing) marked the
  highlighted row as an explicit pick and Enter went to the full-path
  suggestion instead of the typed intermediate address. A hover pick now
  requires a `PointerMoved` this frame (or a non-zero pointer delta);
  `Down`/`Up` and clicks are unchanged. (`src/ui/omnibox/`,
  `docs/INTERFACE.md`, `docs/NAVIGATION.md`.)
- `<Tabs items={[…]}>` labels now reach the tab strip: the `items` array was
  ignored, so tab sets that rely on it fell back to `Tab 1…N`. The
  string-array attribute labels the `<Tab>` children that carry no
  `label`/`title` of their own (explicit labels win, nested sets use their
  own parent). The per-instance selection key also includes the `<Tabs>`
  marker's event index, so two sets with identical labels can no longer
  share one selection (the "ghost" tab that appeared selected in the second
  list and vanished on the first click). (`src/markdown/document/`,
  `src/viewer/markdown/`, `docs/RENDERING.md`.)
- Headings no longer show their `[#anchor]` markers: the docs-generator
  convention (`## Title [#custom-id]`) is rewritten before parsing to the
  heading attribute `{#custom-id}`, which sets the explicit anchor id and
  never renders as text. Code, escaped markers and non-heading lines keep
  their literal `[#…]`. (`src/markdown/document/normalize.rs`,
  `docs/RENDERING.md`.)
- Right-clicking a text selection no longer destroys it: egui's
  label-selection plugin reads any press over a selectable label as the
  start of a drag, so the secondary press collapsed the selection (and the
  menu's Copy then copied the whole label/paragraph). `raw_input_hook`
  intercepts that press before egui and delivers its release as a pending
  context click, so the selection and its highlight survive and Copy copies
  exactly the selected text. (`src/app/run.rs`, `src/browser/tab/`,
  `src/viewer/overlays/`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Code blocks no longer lose their rows while the page scrolls them toward
  the top edge. Two causes: the virtualization bound its visible rect with
  `ViewportInfo::inner_rect`, which is in MONITOR space, and intersecting it
  with the window-local clip rect shifted the bound by the window's position
  on screen (a window 300px down the monitor culled ~15 rows); it now uses
  the window-local `InputState::viewport_rect()`. Plain (huge/unsupported)
  blocks also painted their visible rows at a RELATIVE offset
  (`row - visible.start`), which parked them at the block top and left the
  scrolled viewport blank; they now paint at the absolute row offset like
  the highlighted path. (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- The hidden-code-lines notice no longer paints over the first code row: the
  rows are painted, not laid out, so the layout cursor was still at the
  block's top and the notice landed on row 0 of any block past the line
  budget. It is advanced past the reserved body now.
  (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- Two `<Accordion>`s with the same title (or two untitled ones, both shown
  as `Details`) no longer share one open/closed state: the state key is the
  marker's own event index + title, exactly like the `<Tabs>` selection key
  (it was the title alone, so toggling one section toggled the other).
  (`src/viewer/markdown/components.rs`, `docs/RENDERING.md`.)
- A `#fragment` whose target EXISTS but is not rendered in the current pass
  (inside an unselected `<Tab>` panel, a closed `<Accordion>`, past the block
  budget) stays pending instead of being dropped at the end of the frame, so
  opening that tab later still applies the request; only a target the anchor
  index does not know is cleared. (`src/viewer/markdown/mod.rs`,
  `docs/ARCHITECTURE.md`.)
- An inline `<Badge>` inside a heading renders as the same pill as in a
  paragraph, scaled with the heading (capped at the H3 step): it used to
  flatten into plain heading text with no pill and no accent.
  (`src/viewer/markdown/headings.rs`, `src/viewer/inline/mod.rs`,
  `docs/RENDERING.md`.)
- `Copy` strips the alert marker at ANY quote depth (`> > [!WARNING]`): the
  viewer renders the same badge at any depth, but the clipboard pass peeled
  only one `>` prefix and pasted the marker.
  (`src/markdown/document/plain.rs`, `docs/RENDERING.md`.)
- A wide table can no longer bypass the per-frame block budget one row at a
  time: one row spends a single block, so a 4096-column table laid out every
  cell of every row (~20M cells for 5 000 rows). Rendered cells are now
  capped per document per frame (`config::MAX_RENDERED_TABLE_CELLS`) and the
  cells past it are consumed without fragments, with the usual truncation
  notice. (`src/viewer/markdown/walk.rs`, `src/viewer/markdown/budget.rs`,
  `src/config/mod.rs`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- The pre-parse table guard counts CELLS, not pipes: a header row with
  exactly `MAX_TABLE_COLS` pipes yielded `MAX_TABLE_COLS + 1` columns (one
  past the documented cap) and is rejected now.
  (`src/markdown/document/mod.rs`, `docs/ARCHITECTURE.md`,
  `docs/SECURITY.md`.)
- A same-frame `Ctrl+H` (or `Cmd+Y`) plus `Ctrl/Cmd+W` no longer closes the
  freshly opened History tab: the close target is captured before the
  History/Licenses tab is inserted and activated (the capture ran after them
  and saw the new tab's index). (`src/app/shortcuts.rs`.)
- Bookmark `trim` no longer over-evicts: after the entry-cap drain the byte
  counter still counted the dropped entries, so the byte loop dropped extra
  live bookmarks to compensate for phantom bytes (silent data loss at the
  cap). (`src/bookmarks/persist.rs`.)
- A corrupt/unknown `restore_tabs` value keeps the DEFAULT (restore ON)
  instead of being read as "off": the off path deletes the encrypted
  `session.bin`, so a garbage value could destroy the visited-URL memory.
  Only an explicit `false`/`0` disables restore.
  (`src/settings/mod.rs`, `docs/SECURITY.md`.)
- A worker retired by a bounded settings/session `flush` no longer takes its
  last outcome with it: the error is parked and reported by `Prefs::tick`
  like a live worker's (the "restore tabs off" path flushes mid-session, so
  a failed write could disappear with the dropped worker).
  (`src/settings/file.rs`, `docs/ARCHITECTURE.md`.)
- A settings/session job that could not even be QUEUED is reported against
  its own file: a session enqueue error used to land in the settings slot
  (wrong Preferences card) while the snapshot was marked as saved and never
  retried. `save_session_async` now returns `Err` when the job was refused.
  (`src/settings/file.rs`, `src/settings/session.rs`,
  `docs/ARCHITECTURE.md`.)
- A failed "Delete all data and close" no longer exits as if the data were
  gone: the app re-enables writes, shows the error in the Local data card and
  stays open (the old `remove_dir_all` result was discarded and the process
  exited anyway). (`src/settings/platform.rs`, `src/app/mod.rs`,
  `src/ui/preferences/`, `docs/SECURITY.md`, `docs/INTERFACE.md`.)
- A failed session save now wakes the app when its retry window opens: the
  `SESSION_SAVE_RETRY_SECS` backoff was armed but nothing requested a
  repaint, so an idle window retried only on the next input event.
  (`src/app/run.rs`.)
- The table-bomb guard can no longer be hidden behind a QUOTED or list-item
  fence: the fence state only remembered its marker, so `> ``` ` (or
  `- ```json`) made every following line count as code — an unquoted (or
  unindented) 4097-column header reached the eager parser unguarded. The
  state now carries the quote depth and the list item's content column, and
  a line that leaves the container ends the fence (pulldown ends the code
  block and the quote/item there too). (`src/markdown/document/normalize.rs`,
  `docs/SECURITY.md`.)
- A local image read failure (permission, vanished file, I/O error on a
  read-only mount) is FINAL instead of transient: the same path and bytes
  come back, so the automatic retry only wasted a read. The verdict is
  scoped to `file:` targets; a NETWORK error keeps its retry even though it
  also carries an `(os error N)` suffix. (`src/media/cache/mod.rs`.)
- An ELAPSED Preferences delete confirmation is dropped when the dialog
  writes its state back: a stale `Some(Instant)` survived and re-armed the
  armed label on a later visit without a new click.
  (`src/ui/preferences/mod.rs`.)
- A favicon budget backoff can no longer keep an idle window repainting at
  8 fps forever with the icon unresolved: `FaviconCache::poll` retries a
  backoff whose cooldown elapsed itself (the repaint it schedules is the only
  frame a closed tab's origin ever gets) and past the attempt cap the entry
  FAILS, so it stops counting as pending.
  (`src/media/favicon.rs`, `docs/ARCHITECTURE.md`.)

### Changed

- The shared `store.key` is tightened on its OPENED handle and opened with
  `O_NOFOLLOW`: a symlinked key is refused fail-closed without being followed
  or `chmod`-ing its target, closing the check-then-chmod window a
  path-based `std::fs::set_permissions` had. `settings::tighten_opened_perms`
  reports whether the handle is user-only, so the key path fails closed when
  the filesystem refuses (the store files already used the handle recipe).
  (`src/store/crypto/`, `src/settings/platform.rs`, `docs/SECURITY.md`.)
- `StoreWriter` spawns its worker with `thread::Builder`: a refused thread
  start is reported through the store's own queue-error path instead of
  panicking the UI thread, and the "two spawns refused" branch is reachable.
  (`src/store/writer.rs`.)
- `parse_https` rejects a `scheme:user:pass@host` spelling (a second colon
  before the `@`): it used to be read as a userinfo host and navigate to
  `https://host`, silently reinterpreting a spelling the contract blocks.
  The plain `user:pass@host` and `user:pass@host:8080` forms keep working
  with credentials stripped. (`src/protocol/client/mod.rs`,
  `docs/SECURITY.md`.)
- `util::truncate_chars` never cuts inside a grapheme cluster: a combining
  mark, ZWJ emoji family or flag stays whole (or is dropped whole when it
  cannot fit), so a display cap can no longer produce a bare base character
  with a dangling joiner. Every cap that uses it (image alt/title, block
  text, footnote labels, log lines) inherits the fix.
  (`src/util.rs`, `docs/ARCHITECTURE.md`.)
- A component label (tab cell, accordion header, step title) is capped at
  `config::MAX_COMPONENT_LABEL_CHARS` with the full text in the hover
  tooltip: one 2 000-char label allocated a cell thousands of px wide and
  pushed every later tab outside the reading sheet (unreachable).
  (`src/viewer/markdown/components.rs`, `src/config/mod.rs`,
  `docs/RENDERING.md`.)
- The tab rail's close X uses the shared chrome icon-button recipe
  (`icons::icon_button_at_named_lazy`): the hand-rolled copy had drifted
  (CARD-radius hover fill where every other X uses the pill radius, no
  pressed state, a re-implemented tooltip). (`src/ui/tabs/rows.rs`,
  `docs/ARCHITECTURE.md`.)
- The omnibox suggestion popup's `Area` id is scoped to the SURFACE, not the
  tab: egui never prunes `AreaState`, so a per-tab id leaked one area and one
  layer per tab that ever opened the popup, and every later hit test scanned
  a linearly growing layer order. (`src/ui/omnibox/mod.rs`.)
- `detect_callout` clamps its prefix scan to a char boundary: when byte 12
  split a multi-byte character, the fallback uppercased the WHOLE (up to
  512 KiB) quoted paragraph per frame. (`src/viewer/markdown/links.rs`.)
- Preferences delete confirmations arm through `config::deadline_after`
  (panic-free `Instant` arithmetic, the one place every surface derives its
  deadline from) and the dialog card uses `theme::radius::SHEET` instead of a
  hard-coded 12. (`src/ui/preferences/`, `docs/ARCHITECTURE.md`.)
- `settings::cleanup_legacy_files` runs at most once per directory per
  process: the sweep sat on the settings-hot read paths and, off Linux, ran
  `ps` per crash-orphaned temp on the UI thread.
  (`src/settings/platform.rs`.)
- Standardization: exact-match `add_space` literals in the error view, the
  History page, Preferences and the Home grid use the shared
  `theme::spacing` scale (`XS`/`S`/`M`/`L`) instead of repeated numbers
  (same pixel values; graphics stay pixel-identical).
  (`src/ui/error_view.rs`, `src/ui/history_page/`, `src/ui/preferences/`,
  `src/ui/bookmarks/`.)
- Docs sync: `RENDERING.md` documents that an escaped `\:shortcode:` or
  `\[!NOTE]` still expands (the passes match the RENDERED text; an inline
  code span keeps them literal). (`docs/RENDERING.md`.)

### Performance

- A queue whose entries all wait for a texture-budget deadline is no longer
  re-walked per visible image per frame: `ImageCache::pump` remembers the
  earliest `not_before` and returns until it passes (the renderer calls
  `ensure` for every visible image, so a VRAM-tight gallery walked the queue
  up to 512 x 512 times a frame).
  (`src/media/cache/mod.rs`, `docs/ARCHITECTURE.md`.)

- An HTML block's buffer is collected under the render cap (one char past
  it, so the `… [truncated]` notice still fires) instead of up to 4x the cap
  in bytes per frame before the renderer trimmed it.
  (`src/viewer/markdown/walk.rs`, `docs/ARCHITECTURE.md`.)
- `ImageCache::has_pending` is O(1) (`queue` + the cached `loading_count`):
  it used to rescan every entry for every tab every frame (up to ~65k
  matches per frame with many tabs).
  (`src/media/cache/mod.rs`, `docs/ARCHITECTURE.md`.)
- A saturated favicon origin cache answers an unknown origin without
  `Url::parse` + key allocation per tile per frame (`FaviconCache`'s
  `at_capacity` flag, recomputed once per `poll`). (`src/media/favicon.rs`.)

## [0.4.1] - 2026-10-12

A hardening and standardization pass: fail-closed UTF-8 handling on the
markup path, allocation-free component scans, one spacing scale for the
document, and the root/docs version drift closed.

### Fixed

- The markup pre-parse no longer indexes mid-character on hostile input:
  `decode_field`, `strip_line` and `strip_comment_spans` advance through
  `str::get` + `chars().next()` and stop fail-closed instead of
  `unwrap`-ing `s[i..]`, and the `<` fast path resolves its tag from the
  checked slice. No behavior change on valid documents.
  (`src/markdown/document/normalize_markup.rs`.)
- Docs sync: `SECURITY.md` supports `0.4.x`; `RENDERING.md` lists
  `<AccordionGroup>` with the transparent wrappers and documents the
  rejected SVG `pattern`/SMIL (`animate`, `animateTransform`,
  `animateMotion`, `set`, `discard`) markup; `ARCHITECTURE.md` nests
  `protocol/resolver`, `protocol/content_type` and `protocol/rewrite`
  beside (not under) `protocol/client` and notes that `<CodeGroup>` is
  desugared to `<Tabs>`/`<Tab>` markers before parsing; `README.md`
  lists the typeable `about:home` page with the other internal pages.
  (`README.md`, `docs/`.)

### Changed

- Standardization: test-only store writers (`settings::write_setting`,
  `settings::save_session_to`) keep one `#[cfg_attr(not(test),
  allow(dead_code))]` instead of a blanket allow, so the intent reads at
  the definition; the embedded-font corruption path funnels through a
  single `#[cold]` panic site. No behavior change.
  (`src/settings/`, `src/assets.rs`.)
- Standardization, no behavior change: the shared `theme::spacing`
  scale (`XS 4.0` / `S 6.0` / `M 8.0` / `L 12.0` / `XL 16.0`) backs heading
  air, footnote rules and the MDX tab-strip padding instead of repeated
  literals (same pixel values; graphics stay pixel-identical).
  (`src/ui/theme/`, `src/viewer/markdown/`.)
- Perf, no behavior change: `viewer::markdown::components::find_close`
  compares close tags allocation-free (`is_close_tag`) instead of
  `format!`-ing `/kind` per scan step.
  (`src/viewer/markdown/components.rs`.)

## [0.4.0] - 2026-10-12

A consolidation release: two rendering-correctness bugs that only showed up
after interacting with a document are fixed (a code card could paint a
sibling's body after a `<Tabs>` switch, and heading anchors could point at
the wrong heading), the encrypted-store writers can no longer strand or
silently lose their newest blob, the URL caps are byte-based on serialized
URLs, and a batch of per-frame costs, accessibility names and doc drifts
are cleaned up.

### Added

- Native MDX components (`viewer::markdown::components` +
  `markdown::document::normalize_markup` markers): `<Tabs>`/`<Tab>` render a
  clickable tab bar (only the selected panel is laid out; the choice is
  remembered per document/tab), `<Accordion>` a collapsible section (toggled
  by its title, collapsed by default), `<Card>` a bordered box
  (a `title` becomes the first line, a real link when `href` is set),
  `<Steps>`/`<Step>` numbered rows and `<CodeGroup>` one code tab per fenced
  child (label from the fence `title=`, else the language); inline `<Badge>`
  is a small pill and `<CardGroup>`/`<Frame>`/`<Tiles>` are transparent
  wrappers. Tab and
  accordion controls keep a fixed geometry, so hovering never shifts the
  page. The normalizer carries components as canonical `<mdowser-cmp …/>`
  markers (blank-line separated so the Markdown between parses) and
  `<mdowser-badge>` inline pills; `plain_text` skips the markers and `Copy`
  still includes every panel's text. (`src/viewer/markdown/components.rs`,
  `src/viewer/inline/mod.rs`, `src/markdown/document/normalize_markup.rs`,
  `docs/RENDERING.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- MDX and raw-tag handling in the Markdown pipeline
  (`markdown::document::normalize_markup`, run inside `normalize_body`
  before parsing): known components (`<Note>`, `<Tip>`, `<Warning>`,
  `<Caution>`, `<Important>`, `<Info>`, `<Danger>`, `<Check>`, `<Success>`,
  `<Hint>`, `<Attention>`, `<Error>` and the
  `type`/`title` containers `<Callout>`/`<Alert>`/`<Aside>`/`<Admonition>`)
  render as the existing callouts; every other JSX/HTML tag is a transparent
  wrapper (the tag is dropped and the content renders as Markdown); `<img>`
  becomes a Markdown image and inline `<br>` a break. In an MDX document
  (`import`/`export` or a capitalized component) ESM statements and `{…}`
  expressions are removed; `<script>`/`<style>` regions are dropped and
  `<pre>`/`<textarea>` stay inert. Nothing is executed, imported or fetched.
  Code spans and fenced/indented code are never touched.
  (`src/markdown/document/`, `docs/RENDERING.md`, `docs/SECURITY.md`,
  `docs/ARCHITECTURE.md`.)

### Changed

- MDX tab strips look like tabs now: labels sit over a full-width rule with
  the active one underlined in the accent (not filled buttons), and the
  hover state changes only colors/underline — the fixed-geometry controls
  never shift the page (the old `selectable_label` grew its frame on hover).
  `<Badge>` pills use a faint tint of the accent as fill with the accent as
  ink plus a hairline border, so they read in both light and dark themes
  (the old solid accent fill with body-colored text read dark-on-dark in
  light mode and light-on-light in dark). Callout bodies put their paragraph
  air between blocks instead of after the last one, tightening the box's
  bottom edge. (`src/viewer/markdown/components.rs`,
  `src/viewer/inline/mod.rs`, `src/viewer/markdown/{walk,blocks}.rs`.)
- Raw HTML tags are no longer painted as inert monospace text: block and
  inline tags are transparent wrappers now, so exported HTML-wrapped content
  (`<div align="center">`, HTML badges wrapped in `<a>`/`<img>`, card links
  with `<h3>` titles) renders its content instead of leaking the tags.
  (`src/markdown/document/normalize_markup.rs`.)
- HTML comments (`<!-- ... -->`) are invisible, exactly like in a browser:
  the block form no longer paints as inert monospace text (a comment-only
  list item keeps its marker row, like an empty `<li>`) and the inline form
  leaves no gap (`foo<!-- x -->bar` reads `foobar`), in the viewer and in
  `Copy`/plain text alike. This covers the comments the pre-parse markup
  pass leaves literal (multi-line or unterminated ones); a literal `<!--`
  inside code is untouched. (`src/markdown/document/mod.rs`,
  `src/markdown/document/plain.rs`, `src/viewer/markdown/`,
  `docs/RENDERING.md`, `docs/INTERFACE.md`.)
- Serialized/derived URLs are capped in BYTES (`config::MAX_URL_BYTES`,
  the worst-case percent-encoding of a capped input): route candidates, the
  display URL, the landed redirect URL and the history/bookmark
  recordability gate no longer compare a CHAR count against a
  percent-encoded spelling. A valid under-cap address whose path encoded to
  more bytes than chars used to fail with an EMPTY "no candidate worked"
  log (and could be neither recorded nor bookmarked); the raw address input
  keeps its char cap. (`src/config/`, `src/protocol/`, `src/util.rs`,
  `src/settings/`, `docs/SECURITY.md`.)
- Accessible names for row-scoped actions: a history row's X announces the
  visited page, a bookmark tile's rename/remove announce the tile, and the
  Preferences dropdowns ("Keep visits", "Theme") announce what they set —
  they were N identical buttons and an unnamed combo box. All names are
  built lazily, so a settled frame allocates nothing while accesskit is
  off. (`src/ui/icons/`, `src/ui/history_page/`, `src/ui/bookmarks/`,
  `src/ui/preferences/`.)
- The toolbar's reserved width charges one item spacing per allocation, so
  the Preferences gear lands exactly on the panel's right edge in every
  zoom mode (it used to stop 2-4px short; the omnibox absorbs the
  difference). (`src/ui/navbar/`)
- Turning "Restore tabs on startup" off deletes `session.bin` through the
  session writer as a discard job (a queued or in-flight snapshot can no
  longer be renamed into place after the delete) and then verifies the
  result, so the URL ciphertext cannot outlive the preference.
  (`src/settings/`, `docs/ARCHITECTURE.md`)
- `make doctor` stays runnable exactly when it is needed: the parse-time
  "cargo not found" error now skips the `doctor` goal, so the diagnosis
  command the message points at no longer fails with the same error.
  (`Makefile`, `docs/BUILDING.md`)
- Docs sync: `INTERFACE.md` describes the raw-HTML transparent wrappers
  (the one inert case left is `<pre>`/`<textarea>` plus malformed markup),
  the new Error screen section (families, Copy log, Retry cooldown) and the
  `MAX_TABS` toast; `RENDERING.md` no longer calls `<CardGroup>` a bordered
  box (it is a transparent wrapper like `<Frame>`/`<Tiles>`), lists the
  extra callout aliases (`success`/`hint`/`attention`/`error`) and
  documents `+++` metadata blocks; `NAVIGATION.md` matches the omnibox
  Enter behavior; `ARCHITECTURE.md`'s image-retry policy matches the code
  (5xx/429/408 retry, other 4xx are final); the color-emoji face is
  documented as decompressed on the `media::emoji::prewarm` background
  thread at startup instead of "lazily on the first emoji".
  (`README.md`, `docs/`, `Cargo.toml`, `src/assets.rs`.)

### Fixed

- Editing the address field and pressing Enter no longer navigates to the
  popup's default highlighted row: typing a URL that matched the page
  already shown (e.g. trimming its path) "reloaded" that page and the field
  snapped back to it. Enter now submits the typed address; the popup
  selection only wins after an explicit `Down`/`Up`/hover pick, and typing
  clears that pick. (`src/ui/omnibox/`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Selecting part of the address with the mouse works again: the
  select-all-on-focus ran at the end of the press frame, which re-anchored
  every drag selection at the first character (dragging could never select
  from where the user pressed). A clean click still selects the whole
  address; a drag now selects from the press point and the following
  clicks place the caret. (`src/ui/omnibox/`, `docs/INTERFACE.md`.)
- Code blocks can never render a SIBLING's body: the per-document cache
  was keyed by a render-order index, and `<Tabs>`/`<Accordion>` change which
  blocks the walk renders, so switching a tab or opening an accordion made
  the newly visible card hit the old block's cache entry (body and syntax
  state). The key — and the horizontal scroll id — is now the block's
  `Start(CodeBlock)` EVENT index, and its token spans fill lazily so a block
  first rendered while the per-document line budget was spent highlights as
  soon as a frame can show it. (`src/viewer/code/`,
  `src/viewer/markdown/`, `docs/ARCHITECTURE.md`.)
- Heading anchors no longer shift when the walk skips content: ids were
  consumed from a render-order list, so an unselected `<Tab>` panel, a
  collapsed accordion or a budget-dropped block handed every later heading a
  sibling's target (an in-document `#link` scrolled to the wrong heading or
  nowhere). `AnchorIndex::heading_id_at` looks an id up by the heading's OWN
  event index. (`src/markdown/anchors.rs`, `src/viewer/markdown/`,
  `src/viewer/mod.rs`, `docs/ARCHITECTURE.md`.)
- Code blocks scrolled well into a long listing no longer show a growing
  blank band at the top: the virtualized viewport divided by the 19.6px
  design token while epaint rounds every row to a whole device pixel (20.0
  at scale 1), so the computed first visible row sat past the real one. The
  viewport math, the reserved height and the painted offsets now share the
  measured row stride. (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- A table overflowing by 1-2px showed its horizontal bar without the
  reserved lane (the bar covered the last row): overflow was tested against
  the outer width while the inner `ScrollArea` sees the card frame's inner
  area. Both the scroll decision and the lane now use that same width.
  (`src/viewer/table/render.rs`.)
- A background `session.bin` write failure no longer passes silently: only
  failures caught at enqueue time were reported, so a full disk lost the
  restored tab list while Preferences stayed quiet. `Prefs::tick` drains
  each worker into its own slot (`session_save_error` is separate, so a
  successful session save cannot clear a settings warning and vice versa)
  and `App` invalidates the saved snapshot so the retry lands.
  (`src/settings/`, `src/app/`, `docs/ARCHITECTURE.md`,
  `docs/SECURITY.md`.)
- A store writer can no longer strand a job: `push` accepted jobs for a
  worker that had already decided to exit, leaving the newest blob — or a
  "Delete history" discard, whose ciphertext then survived — in a queue
  nobody reads. The worker publishes its exit under the job lock, `push`
  returns the job instead of swallowing it, and
  `store::writer::queue_or_respawn` replaces the worker and retries; the
  discard also sweeps the store's `.bak` and `.tmp.*` siblings.
  (`src/store/writer.rs`, `src/settings/`, `src/history/`,
  `src/bookmarks/`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- A relative `file:` spelling is rejected wherever it appears:
  `file:notes.md` (no leading slash) used to be parsed by the WHATWG rules
  as `/notes.md` and silently opened a file at the filesystem root, on
  navigation and through a `file:` link inside a local document alike. Both
  the address parser and the link rewrite require the absolute spelling
  (`file:/x`, `file:///x`, `file://localhost/x`). (`src/protocol/local.rs`,
  `src/protocol/rewrite.rs`, `docs/NAVIGATION.md`, `docs/SECURITY.md`.)
- `ui::forget_tab` really drops a closed tab's scroll offsets: a
  `ScrollArea` derives its persistent id from the parent `Ui`
  (`make_persistent_id`), so the five `Id::new((salt, tab_id))` removals
  never matched and every closed tab leaked one scroll state per surface.
  Each surface records the real id it got back (`ui::remember_scroll`) and
  the registry removes that. (`src/ui/`, `src/viewer/`,
  `docs/ARCHITECTURE.md`.)
- The omnibox capsule paints its fill and focus ring AFTER the field with
  the fresh focus state, so a pointer press shows the ring in the same
  frame instead of one frame late, and the right slot is reserved from
  `bookmark || focused || !address.is_empty()`, so on Home the text area's
  right edge no longer shifts 31px on the frame the empty field gains
  focus. Suggestion rows also share the popup's own 80px width floor
  (the row's 120px floor could grow the frame wider than a very narrow
  capsule). (`src/ui/omnibox/`)
- Row highlights survive their own action buttons: the history row and the
  expanded tab row test `contains_pointer` like the bookmark tile already
  did, so the wash no longer blinks off the moment the pointer reaches the
  row's X (egui's hover set holds only the topmost widget).
  (`src/ui/history_page/`, `src/ui/tabs/`.)
- The favicon string-key fast path refuses a non-lowercase authority
  (`https://EX.com`): it used to key a different origin than the parser's
  lowercase one, splitting one site into two cache entries and two probes.
  (`src/media/favicon.rs`)
- `settings.bin` can no longer be written past what the reader accepts: a
  read-modify-write that would cross `MAX_SETTINGS_BYTES` is refused (the
  file used to be rewritten one key over budget, then silently fall back to
  defaults on the next launch), and every store reader derives its file cap
  from `crypto::OVERHEAD_BYTES`. (`src/settings/`, `src/history/`,
  `docs/SECURITY.md`.)

### Performance

- A settled table no longer costs a byte scan per frame per table: the
  content fingerprint memo is keyed by the table's `Start` event index
  (stable whatever the frame renders) instead of re-validating itself with
  a probe of the whole table text. (`src/viewer/table/`,
  `docs/ARCHITECTURE.md`.)
- The widest code line is computed when the line galleys are built, not
  from the visible rows each frame: the horizontal scroll range (and the
  clamped offset) used to shrink while a long line was scrolled out of
  view. (`src/viewer/code/`)
- The error view allocates much less per frame: the scheme-hiding helper
  returns `Cow` (borrowed when nothing has to be stripped), the
  local-file check no longer lowercases the whole address to test a
  prefix, and an oversized HTML block's truncation has a byte fast path and
  builds one `String` instead of `format!`-ing a second copy.
  (`src/util.rs`, `src/ui/error_view.rs`, `src/viewer/markdown/html.rs`.)

## [0.3.4] - 2026-10-12

### Added

- Serialized store deletes (`store::writer::WriteJob::discard` +
  `StoreWriter::discard`): a store file wipe ("Delete history now",
  forget-on-exit) is now a job on the SAME worker that writes the store, so
  it runs behind any in-flight write. The forced exit-time wipe used to
  delete the files directly while a stalled worker could still rename its
  temp blob afterwards, resurrecting the deleted ciphertext.
  (`src/store/writer.rs`, `src/history/`, `docs/ARCHITECTURE.md`,
  `docs/SECURITY.md`.)
- `config::SESSION_SAVE_RETRY_SECS`: a failed session save keeps the
  snapshot PENDING and retries on a 5s backoff (a machine with no data
  directory no longer allocates an error string per frame, and no longer
  records the failed snapshot as saved). (`src/app/`,
  `src/config/mod.rs`.)
- `Tab::session_url` (+ `Tab::session_target`, `app::session`): a restored
  tab whose load is still in flight, or has already failed, remembers the
  URL the user asked for. The session snapshot used to serialize such a tab
  as the `None` Home marker, so the first save after any other tab resolved
  erased its URL from `session.bin`. (`src/browser/tab/`,
  `src/app/session.rs`.)

### Changed

- Code blocks are virtualized: only the rows the scroll viewport shows are
  built and painted (`viewer::code::code_viewport` + per-row placement on a
  uniform measured row grid), any
  line count up to the per-document budget. A 10 000-line log used to build
  one `LayoutJob` and one `Label` per row per frame while ~20 rows were on
  screen. Highlighted rows are now painted galleys instead of per-row
  labels, so drag-selecting an individual code line no longer works inside
  a highlighted block: the Copy button (and select-all + Ctrl/Cmd+C) copies
  the whole block. (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- Table width measurements no longer re-hash the whole table text on every
  frame: the content fingerprint is memoized per `(doc_salt, table index)`
  and validated by a cheap content probe, so an edited table still
  re-measures (`viewer::table::cached_fingerprint`).
  (`src/viewer/table/`, `docs/ARCHITECTURE.md`.)
- `ui::error_view::classify` is allocation-free (per-line case-insensitive
  scan instead of `join` + `to_ascii_lowercase` of the whole log on every
  frame). (`src/ui/error_view.rs`.)
- `settings::cleanup_legacy_files` leaves a temp file alone while the
  process that created it is still running (pid parsed from the
  `<file>.tmp.<pid>.<seq>` name): a second instance's live atomic write is
  no longer deleted mid-flight by the legacy/crash sweep.
  (`src/settings/platform.rs`.)
- The Reload button's accessible name is the static `"Reload"` in every
  state (it used to announce the tooltip sentence, parentheses included).
  (`src/ui/navbar/`.)
- The omnibox capsule senses clicks without joining the keyboard tab order
  (`Sense::CLICK`): it draws no focus ring of its own, so the old focusable
  capsule was a silent stop before the address field. (`src/ui/omnibox/`.)
- Closing the LAST tab allocates a fresh tab id instead of reusing the
  closed one, keeping the "ids are never reused" invariant the egui-memory
  cleanup relies on. (`src/app/mod.rs`.)
- `ui::forget_tab` also drops the Home/bookmarks/history-page scroll states
  and the error view's scroll state. (`src/ui/`.)
- Footnote back-references carry the document generation: a later document
  that reuses a `#fn-...` label can no longer scroll to the previous
  document's painted rect. (`src/viewer/inline/`,
  `src/viewer/markdown/walk.rs`.)

### Fixed

- The document scrollbar was effectively invisible in both themes: the
  handle painted `widgets.inactive.bg_fill` (`container_high`, the same
  gray as the chrome behind it) at ~1.1:1 contrast. It now paints
  `fg_stroke` (`foreground_color = true`), which the ramp sets to the
  border tone, at a 0.75 dormant opacity; pinned by a contrast test.
  (`src/ui/theme/`, `src/ui/theme/tests.rs`.)
- `widgets.open` now sets `weak_bg_fill`/`bg_stroke`/`fg_stroke`: the open
  Preferences dropdown button (ComboBox) painted epaint's default gray fill
  with a `gray(160)` border and gray text instead of the app's tokens.
  (`src/ui/theme/mod.rs`.)
- A settings-write failure can no longer be cleared by a successful SESSION
  write: `Prefs::tick` drains the settings worker's outcome only (session
  failures already surface through `save_session`'s own error), so the
  Preferences warning reflects the file it names. (`src/settings/file.rs`.)
- Stop and the visual timeout keep the pushed history entry like a failed
  fetch (`settle_abandoned` now takes `pushed_history`): the stacks are no
  longer restored to the pre-navigation snapshot, which dropped the page
  the user came from and made the same sequence behave differently
  depending on whether the load failed or was stopped.
  (`src/browser/tab/`, `docs/ARCHITECTURE.md`.)
- The file-name autolink guard reads the domain label IMMEDIATELY before
  the extension-like TLD instead of the host's first label:
  `blog.readme.md` now stays text (it ends in the file name `readme.md`)
  and `readme.blog.md` links (a host under `.md`).
  (`src/markdown/autolink/scan.rs`, `docs/RENDERING.md`,
  `docs/ARCHITECTURE.md`.)

- Address autocomplete builtin list grows from 9 to 82 curated entries
  (developer-service and docs sites plus the
  agent-ready and personal-site lists) and moves to its own module
  (`src/builtin_sites.rs`, imported by `src/suggest.rs`), so growing it
  never touches ranking code or tests.
- Corner-radius tokens and a shared focus ring (`ui::theme::radius`,
  `ui::theme::focus_ring`): cards, pills, tiles, dialogs and the chrome
  icon buttons now share one scale instead of hard-coded `3/6/8/10/12`
  values. (`src/ui/theme/`, `src/ui/icons/`,
  `src/viewer/image.rs`, `src/viewer/inline.rs`.)
- Accessible names for both address fields and the `about:history` search
  field (screen readers now announce "Address" / "Search history" instead
  of only the hint text). (`src/ui/omnibox/`,
  `src/ui/history_page/`, `docs/ARCHITECTURE.md`.)
- Shared LRU eviction pass (`viewer::cache::evict_oldest`, covered by
  unit tests) backing both the code-block and table-width caches.
  (`src/viewer/cache.rs`, `src/viewer/code.rs`,
  `src/viewer/table.rs`, `docs/ARCHITECTURE.md`.)
- Atomic unreadable-store preservation (`settings::preserve_blob_bytes`):
  `.bak` copies use `create_new` (no clobber, no exists-then-rename TOCTOU)
  and the live file stays in place. (`src/settings/`, `src/history/`,
  `src/bookmarks/`, `docs/SECURITY.md`.)
- Shared chrome tokens (`config::ROW_TITLE_CHARS/ROW_URL_CHARS`,
  `config::DELETE_CONFIRM_SECS`, `config::CHROME_*`, `theme::radius::MENU/FULL`,
  `theme::pill_radius`): suggestion/history rows, delete confirmations, tab
  rail, navbar, dialogs and viewer pills/radii share one scale.
  (`src/config/`, `src/ui/theme/`, `src/ui/row.rs`, `src/ui/omnibox/`,
  `src/ui/preferences/`, `src/ui/history_page/`, `src/ui/navbar/`,
  `src/ui/tabs/`, `src/ui/bookmarks/`, `src/viewer/`.)
- Image-settling gate (`TabState::Settling`, `config::DOC_IMAGE_GATE_SECS`):
  a fetched page preloads all its images in a separate cache while the
  previous page (or the stable empty panel on a first load) stays on screen
  with the loading indicator, then swaps atomically once every image
  settles or 5s pass — no more placeholder-to-bitmap jumps shifting the
  text. Text-only pages show instantly; `Stop`/new navigations abandon the
  gate and keep the original backdrop. (`src/browser/tab/`, `src/app/`,
  `src/viewer/mod.rs`, `src/config/`, `docs/ARCHITECTURE.md`,
  `docs/RENDERING.md`.)
- Real bold text (`theme::strong`, `theme::strong_family`): a named family
  whose first face is the BUNDLED Noto Sans variable font pinned to
  `wght = 700` through `FontTweak::coords` (epaint 0.36 has no weight axis
  in `FontId`), inheriting the proportional family's fallbacks. Headings,
  `**strong**` runs, callout titles, footnote refs, the Preferences title
  and section labels, the error-view headings and the Home wordmark are
  now visibly heavier instead of color-only. (`src/ui/theme/`,
  `src/viewer/`, `src/ui/error_view.rs`, `src/ui/preferences/`,
  `docs/RENDERING.md`.)
- Footnotes: a `[^label]` reference renders as a small raised
  NUMBER marker linked to its note (never the source `[^label]` spelling),
  numbered in FIRST-REFERENCE order — so `[^note]` before `[^2]` shows 1 and
  2, and a repeat keeps its number (`AnchorIndex::footnote_number`). The
  notes render as one section: a single 1px rule before the FIRST definition
  (it used to draw one rule before every note), each note as a numbered
  entry whose marker shares the first text line with the body (`1. text`,
  wrapped lines and continuation blocks hanging under the text; a first
  block that cannot flow inline keeps the marker above it, like a list
  item), and a small vector back-arrow (`Icon::Back`) that scrolls to the
  painted reference.
  Definitions the document never references still render — common renderers
  drop them, MDowser never hides what the author wrote — continuing the numbering
  after the referenced ones. `Copy` keeps the source `[^label]` spelling.
  (`src/markdown/anchors.rs`, `src/viewer/markdown/`,
  `src/viewer/inline/`, `docs/RENDERING.md`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- One typeface family everywhere: the bundled Noto Sans is now the primary
  face of the UI — the proportional family leads with Noto Sans (chrome,
  menus, documents) and the monospace family leads with its bundled
  true-monospace cut (Noto Sans Mono CJK JP, face 5 of the CJK
  collection), so code keeps its alignment — instead of both sitting
  behind epaint's bundled default faces, which are no longer
  registered in the UI stack or loaded into the SVG font
  database; SVG text defaults and generic families resolve to the same
  bundled faces (`serif`/`sans-serif`/`cursive`/`fantasy` to Noto Sans,
  `monospace` to the mono cut). The bold face keeps the
  same pinned `wght = 700` Noto Sans bytes; its Ubuntu-era
  `y_offset_factor` baseline tweak is gone (regular and bold share one
  face's metrics now). (`src/ui/theme/`,
  `src/media/decode/svg.rs`, `src/assets.rs`, `docs/RENDERING.md`,
  `docs/ARCHITECTURE.md`, `docs/INTERFACE.md`,
  `THIRD_PARTY_NOTICES.md`, `deny.toml`.)
- Refactor, no behavior change: the source tree is reorganized into
  domain folders (at most two levels deep) and every oversized file is
  split by functionality, with unit tests moved out of the logic files
  into sibling `tests*.rs` modules. `ui::viewer` moves to a top-level
  `viewer/` (document rendering, `viewer/markdown`, `viewer/table`,
  `viewer/inline` included); the shared encrypted-store plumbing moves to
  `store/` (`store::crypto`, `store::writer`); `ui::history` (the
  Preferences window) becomes `ui::preferences`. `app`, `history`,
  `bookmarks`, `settings`, `browser::tab` (`state`/`poll`/`nav`/
  `nav_moves`), `media::decode` (`raster`/`svg`), `protocol::client`
  (`get`/`fetch`), `markdown::autolink` (`scan`), `markdown::document`
  (`normalize_links`/`normalize_tables`), `viewer::markdown` (`walk`/
  `blocks`/`headings`/`tables`/`html`), `viewer::table` (`render`),
  `viewer::inline` (`code_span`), `ui::theme` (`widgets`), `ui::tabs`
  (`rows`), `ui::bookmarks` (`tile`) and `ui::preferences` (`widgets`)
  gain functional submodules; `config`, `ui::icons`, `ui::navbar`,
  `ui::omnibox` and `ui::history_page` keep their single logic file with
  the tests in a sibling `tests.rs`; no logic/format/behavior change.
  (`src/`, `docs/ARCHITECTURE.md`, `docs/INTERFACE.md`, `Cargo.toml`.)
- "Restore tabs on startup" is now ON by default: a fresh profile (no
  stored `restore_tabs` value) reopens the last session. An explicit
  `false` still stays off. (`src/settings/`.)
- Preferences "Default zoom" now uses the same `- % +` capsule as the
  toolbar (clicking the readout resets to 100%) instead of a slider.
  (`src/ui/navbar/`, `src/ui/preferences/`.)
- Zoom range is now 75-175% in 25% steps (was 50-300%).
  (`src/config/`, `docs/INTERFACE.md`.)
- Dependency patch: `tokio` 1.53.1 → 1.53.2 (`cargo update -p tokio`).
  `skrifa`/`harfrust` stay pinned by `eframe 0.36` (upgrading them
  directly would duplicate the shaper `epaint` already links).
- Perf: the code-block language chip no longer allocates per block per
  frame when the tag is already lowercase; H6 headings keep readable
  select-all colors. (`src/viewer/code.rs`,
  `src/viewer/markdown/blocks.rs`, `src/viewer/inline.rs`.)
- Perf: media tables (images, color emoji) are cached like text tables,
  keyed by new image/emoji geometry epochs (`ImageCache::resolved_epoch`,
  `EmojiCache::resolved_epoch`): every resolution/evict/registration
  transition re-measures exactly once while settled frames hit, instead of
  re-measuring every frame. Image URLs join the table fingerprint so
  swapped sources cannot reuse another image's widths.
  (`src/media/cache/`, `src/media/emoji/`, `src/viewer/table.rs`,
  `docs/ARCHITECTURE.md`.)
- Refactor, no behavior change: `markdown/mod.rs` shrinks via pure moves
  (block-budget helpers → new `markdown/budget.rs`, list walker →
  `markdown/items.rs`); the tab rail returns `TabsResponse` instead of
  four out-params (mirrors `OmniboxResponse`); closed-tab cleanup is one
  registry (`ui::forget_tab`, which also drops the address/search
  `TextEdit` states that lived in the persisted store and leaked one per
  closed tab). (`src/viewer/markdown/`,
  `src/ui/tabs/`, `src/app/`, `src/ui/mod.rs`, `src/ui/omnibox/`,
  `src/ui/history_page/`, `docs/ARCHITECTURE.md`.)
- Home capsule floor 80 → 90px so the two-column bookmark grid cannot
  overflow in narrow windows. (`src/ui/home.rs`.)
- Perf, no behavior change: `strip_scheme_prefix` no longer lowercases the
  whole URL per row per frame (case-insensitive prefix check, `Cow`
  borrowed on the common path); `truncate_chars` returns `Cow` (borrowed
  when the input fits); theme-twin URL tokens compare borrowed
  (`eq_ignore_ascii_case`, no per-token `String`); the Failed retry button
  keeps a static label with the countdown in the (lazy) tooltip instead of
  a per-frame `format!`; bookmark tile edit/drag state persists only on
  change or while open. (`src/util.rs`, `src/viewer/markdown/frags.rs`,
  `src/viewer/mod.rs`, `src/ui/bookmarks/`, `src/browser/tab/`.)
- Refactor, no behavior change: `theme::apply` splits into
  `apply_spacing_type`/`apply_visuals`; failed navigations share
  `Tab::restore_failed_navigation` instead of duplicating six statements;
  `navbar` reads zoom limits from `config` directly. (`src/ui/theme/`,
  `src/browser/tab/mod.rs`, `src/ui/navbar/`, `docs/ARCHITECTURE.md`.)
- Graphics: linked inline/block images paint the shared keyboard focus ring;
  image status icons documented as intentional (`Loading` status vs `Reload`
  action, `Image` unavailable vs `BrokenImage` failed). (`src/viewer/image.rs`,
  `docs/INTERFACE.md`.)
- Graphics: the floating scrollbar no longer paints its track (epaint used
  `extreme_bg_color`, washing a white band over the gray code cards and
  Preferences surfaces, and a darker one in dark mode) — the reserved
  `theme::scrollbar_lane` keeps the handle alone; the focused omnibox ring
  is painted INSIDE the capsule instead of bleeding 2px into the
  neighbouring toolbar buttons; the tab rail's active bar, the omnibox
  progress sweep and the `<hr>` bar share a new `theme::radius::HAIRLINE`
  token; the rail, history, dropdown, suggestion and bookmark rows all draw
  the shared `theme::focus_ring` instead of five hand-rolled variants (the
  bookmark tile no longer used a 0.5px vs 1px inset). (`src/ui/theme/`,
  `src/ui/omnibox/`, `src/ui/tabs/`, `src/ui/preferences/`,
  `src/ui/history_page/`, `src/ui/row.rs`, `src/ui/bookmarks/`.)
- `about:history`: removed the "Local, encrypted. Newest first." subtitle
  and added 8px of breathing room below the delete footer.
  (`src/ui/history_page/`.)
- Perf, no behavior change: `theme::scheme_ref` interns the palette per
  theme (`OnceLock`), so the ~40 per-frame `scheme_for` call sites no longer
  rebuild the 50-field `Scheme` per element (heading fragments, list
  markers, table cells, tiles); `hover_tooltip_lazy` builds `format!`-ed
  tooltips only while the widget is hovered; the tab rail reads a per-tab
  display memo (`Tab::display`: title + favicon origin, invalidated by the
  state kind and `doc_generation`) instead of allocating a title and
  re-parsing the asked URL per tab per frame; bookmark tiles resolve their
  favicon by URL string (`FaviconCache::texture_for_url`) instead of a
  `Url::parse` per tile per frame. (`src/ui/theme/`, `src/ui/mod.rs`,
  `src/browser/tab/mod.rs`, `src/ui/tabs/`, `src/media/favicon.rs`,
  `src/ui/bookmarks/`, `docs/ARCHITECTURE.md`.)
- Perf, no behavior change: image caches no longer rescan every entry per
  frame (`ImageCache::loading_count`, recomputed in `poll`), the emoji LRU
  evicts with one O(n) pass per victim instead of two (the old scan made a
  full cache ~1M comparisons per insert on the UI thread), and
  `DestCache` charges a fixed per-entry container overhead so a link farm
  cannot retain several times its documented byte budget.
  (`src/media/cache/`, `src/media/emoji/`, `src/protocol/rewrite.rs`,
  `docs/ARCHITECTURE.md`.)
- Perf, no behavior change: the per-frame render path no longer touches
  bytes past its documented caps. The quote-alert look-ahead scans only
  the longest-marker prefix of each text run instead of copying (and
  uppercasing) the whole quoted paragraph; the image-alt collector and the
  depth-guard flatten buffer stop at `MAX_RENDERED_BLOCK_CHARS` before
  appending a multi-MB fragment; `cap_block_text` counts chars only up to
  the cap instead of rescanning an oversized fragment; each cached code
  block stores its line total once instead of rescanning the body on every
  repaint; and `FaviconCache::texture_for_url` now really derives the origin
  key from the URL string (the earlier string-lookup intent was not
  implemented: every settled tile still ran a `Url::parse` per frame).
  (`src/viewer/markdown/`, `src/viewer/code/`,
  `src/media/favicon.rs`, `docs/ARCHITECTURE.md`.)
- Preferences and session writes no longer `fsync` on the UI thread:
  `Prefs::set_*`, the per-change session save in `App::logic` and the
  history retention key update an in-memory body and queue the encrypted
  write on dedicated background workers (`settings::write_setting_async`,
  `save_session_async`), with the settings plaintext cached per directory so
  two changes cannot lose each other's keys; the exit path drains both
  workers and only a `restore_tabs` toggle waits briefly (bounded) so the
  preference is durable before the session file is deleted.
  (`src/settings/`, `src/history/`, `src/app/`, `docs/SECURITY.md`.)
- Refactor, no behavior change: `theme::mix`/`theme::tint` are the single
  color-blending helpers (the Preferences switch track's local copies are
  gone); `TabStateKind` names the state discriminant instead of four
  inline matches; `theme::weak` drops its no-op `.weak()` flag (an explicit
  color always wins). (`src/ui/theme/`, `src/ui/preferences/`,
  `src/browser/tab/mod.rs`.)
- Standardization: the neutral chrome ramp gains `Scheme::container_hover` /
  `Scheme::container_active` (the omnibox pill and the widget ramp shared two
  near-identical literals that had already drifted by 1/255); the flat
  tonal buttons keep a transparent 1px `bg_stroke` instead of
  `Stroke::NONE` (the theme's own "one stroke width in every state"
  invariant: `Frame::total_margin` adds the width back, so those buttons
  were 2px tighter inside than every other button); the code card exposes
  `theme::CODE_PAD_X` instead of a `34.0` literal whose `16+16+2` breakdown
  had to be re-derived here; `content_max_width`, the heading-scale and the
  code-font-size comments now state what the code does. (`src/ui/theme/`,
  `src/viewer/code/`, `src/viewer/markdown/headings.rs`.)
- Perf, no behavior change: the tab rail requests favicon probes from the
  borrowed per-tab display memo (no `Vec<Option<Url>>` plus one `Url` clone
  per tab per frame), and `FaviconCache` keys every lookup with a borrowed
  slice of the serialization (`String: Borrow<str>`), so a settled frame
  costs one hash probe per tab/tile instead of a `String` allocation
  (parsing is now limited to the FIRST frame per unknown origin).
  (`src/ui/tabs/`, `src/media/favicon.rs`, `docs/ARCHITECTURE.md`.)
- Perf, no behavior change: the OS window title is memoized with the rest
  of the tab display data (`TabDisplay::window_title`,
  `Tab::window_title_cached`): the per-frame title check compares borrowed
  strings and allocates only when the title actually changes, where every
  frame used to build a sanitized/truncated `String` for the window
  manager. (`src/browser/tab/`, `src/app/`.)
- Perf, no behavior change: the omnibox autocomplete cache rebuilds only
  while its field is EDITING (on Home both the toolbar field and the
  capsule share one address, so every keystroke ran the full
  bookmarks+history scan twice for a popup that was closed), and
  `suggest::matches_candidate` compares ASCII candidates in place
  (`util::contains_ignore_ascii_case`) instead of allocating one
  `to_lowercase()` `String` per candidate per keystroke — tens of thousands
  of allocations on a full history. (`src/ui/omnibox/`, `src/suggest.rs`,
  `src/util.rs`.)
- Perf, no behavior change: bookmark tiles and history rows borrow their
  title (`Cow`, like the suggestion rows) instead of cloning it per frame,
  the tile letter avatar returns a `char` encoded into a stack buffer
  instead of a `String` per tile per frame, and `bookmarks::entry_bytes` is
  arithmetic (it allocated a throwaway `Vec` per entry on every mutation,
  trim and save). (`src/ui/bookmarks/`, `src/ui/history_page/`,
  `src/bookmarks/`.)
- Perf, no behavior change: `read_setting` reuses the per-directory
  plaintext cache (loading four keys decrypts once, not four times), the
  legacy-file sweep runs once per write path instead of on every session
  save, `settings_writer().flush` shares one deadline across both workers
  (the documented bounded wait was silently twice as long), and a store's
  plaintext body is only rebuilt when `trim` actually dropped an entry.
  (`src/settings/`, `src/bookmarks/`.)
- Perf, no behavior change: the ordered-list marker-width cache keys on
  `(ordinal, body size)` instead of allocating a `String` key per item per
  frame (the marker text is by construction `"{ordinal}. "`), dynamic
  tooltips (the Reload countdown, the zoom-in percentage) are built on
  hover only, and the rendered code card's Copy button gets an explicit
  24px minimum height instead of a ~17px sliver (`small()` zeroes the
  vertical padding and skips the `interact_size` floor).
  (`src/viewer/markdown/items.rs`, `src/ui/navbar/`, `src/ui/icons/`,
  `src/viewer/code/mod.rs`.)
- Inline code pills are height-capped (`viewer::inline::MAX_PILL_H`, the
  body line minus 3px): the bundled mono cut's em box is taller than a
  typical UI face, and without the cap a pill grew to a whole line box
  (24px) instead of reading as a tight tag. The cap shrinks the box
  symmetrically about its own center, so the glyphs stay optically
  centered. (`src/viewer/inline/`, `docs/INTERFACE.md`.)
- Hardening, no behavior change: `Tab::display` falls back to a static
  empty display instead of an `expect` (the UI thread never panics on a
  defensive invariant), and the test-only `Tab::title`/`window_title`/
  `favicon_origin` accessors are `#[cfg(test)]` (production reads the
  display memo exclusively). (`src/browser/tab/`.)
- Docs sync: `docs/ARCHITECTURE.md`, `docs/SECURITY.md`,
  `docs/RENDERING.md` and `docs/INTERFACE.md` describe the rendering,
  storage and rendering-safety behavior changed above (bold family, body
  text token, quoted code fences, CSS filter spellings, unreadable-settings
  refusal, handle-based permission tightening, credential-free
  destinations, write suppression on wipe).
- License change: MDowser is now distributed under the Mozilla Public
  License 2.0 (`MPL-2.0`) instead of `MIT OR Apache-2.0` (file-level weak
  copyleft; `THIRD_PARTY_NOTICES.md` still covers every bundled crate and
  font). `LICENSE-MIT` / `LICENSE-APACHE` are replaced by a single
  `LICENSE` with the MPL 2.0 text; every `.rs`/`.sh` file now opens with
  the brief Mozilla Exhibit-A header plus the copyright line (rule in
  `docs/CONTRIBUTING.md`) and the crate metadata, README badge and license
  section, the Preferences footer, the deb/archive metadata and the copies
  shipped with every artifact follow. (`Cargo.toml`, `LICENSE`,
  `README.md`, `THIRD_PARTY_NOTICES.md`, `deny.toml`, `src/`, `build.rs`,
  `build_support/`, `scripts/`, `docs/INTERFACE.md`, `docs/BUILDING.md`,
  `docs/CONTRIBUTING.md`.)

### Fixed

- Preferences header: the title now keeps equal air above and below
  (symmetric 12px padding, vertically-centered 32px lane, flush
  header/divider/scroll stack). (`src/ui/preferences/`.)
- macOS Intel build no longer prints ~46 `ld: no platform load command
  found` notes from `rav1d`'s NASM-assembled x86_64 objects (no
  `LC_BUILD_VERSION`): `[lints.rust] linker_messages = "allow"` in
  `Cargo.toml` silences the benign upstream noise — the binary always
  linked and ran fine — until NASM/`rav1d` emit the load command.
  (`Cargo.toml`, `docs/BUILDING.md`.)
- Tab-rail doc comment now lists the real favicon cascade
  (`config::FAVICON_PATHS`: `/favicon.ico`, `/favicon.png`,
  `/favicon.svg`). (`src/ui/tabs/`.)
- The corrupt-key repair lock can no longer spin forever on the UI thread:
  a stale `<store.key>.repair` that cannot be removed (a directory squatting
  the name, an immutable file, a permission quirk) now makes
  `acquire_repair_lock` give up at once instead of re-observing the same
  stale lock with no sleep and no wait check. (`src/store/crypto/`.)
- A broken directory candidate no longer hides its readable siblings: a
  local `index.md` with malformed frontmatter (or an over-budget parse)
  falls through to `README.md` exactly like the network route fallback,
  instead of ending the cascade with a content error.
  (`src/protocol/local.rs`.)
- Favicon budget backoff keeps the repaint scheduler awake
  (`FaviconCache::has_pending` counts `BudgetWait`): an idle window used to
  leave the icon unresolved until the next input event.
  (`src/media/favicon.rs`.)
- Image failure classification matches the real decoder strings: the
  dead `svg: filter elements...` / `svg: DTD...` markers are gone, the
  actual deterministic SVG verdicts (`CSS filter declarations`, `duplicate
  element ids`) and the credential/over-long redirect blocks are final
  (no pointless retry). (`src/media/cache/`.)
- A dead background writer no longer wedges a store: `StoreWriter::worker_dead`
  (the thread exited or panicked outside the guarded write) makes
  `save_async`/`flush_writer` replace it instead of waiting forever on a
  worker that can never drain, and `Drop` asks a live worker to stop and
  joins only when it already finished (a blocking drop would freeze the
  exit path). (`src/store/writer.rs`, `src/history/`,
  `src/bookmarks/`.)
- The document context menu no longer rings its first row when opened with
  the pointer (it read as a pre-selected entry): opening leaves NO row
  focused, the first `Down`/`Up` moves focus into the rows (the first press
  lands on the first row; egui's own arrow traversal is cancelled so it
  cannot skip it), `Enter` activates the focused row and `Esc` closes it,
  so Copy / Select all / "Open link in new tab" still need no pointer — and
  a primary press outside the popup (including the start of a drag
  selection) closes the menu instead of only a completed click.
  (`src/viewer/overlays/`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- The omnibox suggestion highlight survives a store-generation rebuild:
  bookmarking or removing a page while the popup is open used to reset the
  highlighted row to the first one (the clamp branch was dead code after an
  unconditional reset); a non-query rebuild now clamps the old selection to
  the possibly shorter list, and only a new query starts at the top.
  (`src/ui/omnibox/mod.rs`.)
- Docs sync: the root README links to `docs/SECURITY.md` under its own
  label like every other doc link; `docs/ARCHITECTURE.md` lists the real
  `Settling` tab state and the `bookmarks/` module path, and documents the
  bookmark-tile icon order implemented by `ui::bookmarks::tile`
  (custom glyph → favicon → letter avatar); `docs/INTERFACE.md` quotes the
  real retention labels ("Keep 1 year", not "12 months"); the
  `THIRD_PARTY_NOTICES.md` font-stack note says `viewer::bidi` (the module
  moved out of `ui::viewer`). (`README.md`, `docs/`,
  `THIRD_PARTY_NOTICES.md`.)
- Document text painted in the MUTED tone: `Visuals::text_color()` falls
  back to `widgets.noninteractive.fg_stroke`, which the theme sets to
  `on_surface_variant` (the separator/hint tone) — so every plain
  paragraph, list item, table cell and inline code pill rendered
  `#59636e`/`#9198a1` while bold runs, headings and bullets rendered the
  readable `on_surface`, and `**bold**` showed as "a different color"
  instead of heavier. The theme now sets
  `override_text_color = on_surface`; muted text opts in explicitly
  (`theme::weak`, quote bodies), so the plain quote box finally renders
  muted as documented AND body text meets the palette's own WCAG target.
  (`src/ui/theme/`, `src/viewer/`.)
- A superseding navigation left the tab rail showing the OLD page's title
  and favicon for the whole second load: `Loading` -> `Loading` keeps the
  same state kind, and the display memo is keyed on (kind, generation), so
  `navigate_to_url` now bumps `doc_generation` when it replaces an
  in-flight load. (`src/browser/tab/`.)
- Keyboard activation of a tab or a rail action row (`Enter`/`Space` on the
  focused row) was silently dropped: the settled-collapsed branch returned
  a fresh `TabsResponse::default()` instead of the actions the collapsed
  rail had just collected. (`src/ui/tabs/`.)
- The Reload button's disabled tooltip read "Reload (available in 0s)"
  forever on Home and on fresh local rejections (no cooldown is running):
  it now says "Nothing to reload" when no countdown exists, and the
  countdown itself is built lazily on hover instead of every frame.
  (`src/ui/navbar/`.)
- The zoom readout (`100%`, click to reset) had no keyboard focus
  indicator while every other hand-painted chrome control does.
  (`src/ui/navbar/`.)
- An unreadable `settings.bin` was treated as an EMPTY body by the
  read-modify-write, so the next preference change rewrote the file with
  one key and silently dropped the rest (theme, zoom, restore-tabs,
  retention) for the session — the exact failure `settings_file_error`
  exists to prevent. Writing now refuses when the file exists but cannot
  be read or decrypted. (`src/settings/`.)
- A background `settings.bin`/`session.bin` write failure was never
  surfaced (only failures detected synchronously were), and the queue could
  strand the newest blob on a worker that `flush` had already asked to stop:
  the writers now respawn a stopped-and-drained worker, `Prefs::tick`
  drains every finished write's outcome into the Preferences warning
  (cleared only on a confirmed success), and a failed session snapshot is
  no longer recorded as saved (so it retries and surfaces).
  (`src/settings/`, `src/app/`.)
- A bookmark RENAME could push the store past `MAX_BOOKMARK_STORE_BYTES`
  with no check, and the next save's `trim` then silently dropped the
  OLDEST bookmark (the budget contract promises an explicit refusal for
  user data); `set_title` now refuses an over-budget rename. The first-run
  seed also never updated the byte accounting, so the first save trimmed
  the just-seeded store by one entry. (`src/bookmarks/`.)
- "Delete all data and close" could leave data behind: a background writer
  that had already dequeued its blob recreated the data directory after the
  wipe. The wipe now suppresses every later store write
  (`settings::suppress_writes`) and drains the writers before removing the
  directory. (`src/app/`, `src/settings/`.)
- Fenced code inside a blockquote (`> ``` `) was invisible to the
  pre-parse passes: the table-bomb guard counted pipes inside QUOTED CODE
  as table cells (rejecting legitimate documents with a
  "4096 table columns" error) and the link-join mask treated code lines as
  prose. `fence_update` now strips the blockquote prefix first.
  (`src/markdown/document/`.)
- A bare-URL candidate whose domain hides a real target in the userinfo
  (`https://example.com.@evil.com`, `www.example.com.@evil.com`) linkified:
  the trailing dot slipped past the one-byte `@` email guard and the
  destination parsed as host `evil.com`, so the click landed somewhere other
  than the visible label. The destination validator now rejects
  credentials. (`src/markdown/autolink/`.)
- `protocol::rewrite` passed credential-bearing `https:` destinations
  through verbatim (an absolute `https://user:pass@host/x` kept its
  userinfo in the link) and the scheme-relative join
  (`//user:pass@host/x`) only checked the scheme; both now stay inert.
  (`src/protocol/rewrite.rs`.)
- Bold runs painted BELOW the text around them: epaint lays every section
  out on its own font's baseline (`ascent + half the line gap`), so a bold
  face with different metrics sat 2px low at the body size (1px at the
  small marker size) and mixed bold/regular text looked like it had slid
  off the line. Fixed by the single-face stack above (regular and bold are
  the same Noto Sans bytes at two weights, so their metrics agree), and
  pinned by `bold_runs_share_the_body_baseline`, which compares the PAINTED
  ink of the same letter in the regular, bold and trailing runs of one line.
  (`src/ui/theme/`, `src/viewer/markdown/tests_layout.rs`.)
- The `about:history` footer's destructive button could grow by 0.7px when
  armed ("Delete history now" no longer fitted the fixed 164px slot under
  the bundled primary face, which is wider than epaint's default face):
  `DELETE_W` is now 168px, so the two-click swap never nudges the footer.
  (`src/ui/history_page/`.)
- The back-reference arrow was the `↩` character (U+21A9), which the emoji
  fallback face drew as its boxed "return key" symbol instead of a text
  arrow. It is now the chrome's own monochrome `Icon::Back` vector, with a
  hover fill, the shared focus ring, an accessible name and a tooltip.
  (`src/viewer/markdown/`.)

### Security

- Key file reads are capped and handle-checked: `crypto::read_key_problem`
  opens with `O_NONBLOCK`, re-checks the opened handle and reads at most
  33 bytes (oversize detection without allocation), closing the last
  uncapped local `read` + its metadata/open TOCTOU. (`src/store/crypto/`,
  `docs/SECURITY.md`.)
- `file:` URL validation is consistent everywhere: history/bookmark
  stores (`util::is_recordable_url`) and the session loader
  (`settings::load_session_from`) now reject credentials, ports and
  remote hosts exactly like `protocol::local::parse_file_url`
  (`file://localhost/` stays local). (`src/util.rs`, `src/settings/`,
  `docs/SECURITY.md`.)
- Redirect userinfo is stripped from the landed URL before display,
  history or further hops (fail-closed when it cannot be removed), and
  collected `Content-Type` values are capped at 8. Route candidates and
  display URLs re-enforce `MAX_URL_INPUT_CHARS` after `.md`/index
  expansion, and session writes store over-long tab URLs as Home instead
  of writing them unbounded. (`src/protocol/client/`,
  `src/protocol/resolver.rs`, `src/settings/`, `docs/SECURITY.md`.)
- Store hardening: world-readable restored stores are tightened best-effort
  on load (group/other bits stripped, owner bits never widened); data/key
  directories tighten the same way (a `0500` admin-hardened dir is no longer
  widened to `0700`); a symlinked `store.key` is refused instead of
  `chmod`-ing its target; crash-orphaned `<store>.tmp.*` files are swept for
  all four stores; SVG rejects duplicate `id`s fail-closed (parser
  first- vs last-wins divergence would undercount a `<use>` bomb) and checks
  CSS `filter:` in every `<style>` element regardless of `type`.
  (`src/settings/`, `src/store/crypto/`, `src/media/decode/`,
  `docs/SECURITY.md`.)
- The network entry point (`protocol::client::get_bytes`) strips URL
  authority credentials itself, fail-closed, before anything is sent: the
  documented "credentials never reach the wire" rule no longer depends on
  every caller going through `parse_navigable`. (`src/protocol/client/`,
  `docs/SECURITY.md`.)
- Bookmark store byte budget: `toggle` now refuses an entry that would push
  the SERIALIZED body past `MAX_BOOKMARK_STORE_BYTES` (the store keeps the
  caller's URL spelling, so an entry can serialize larger than its char
  count), and a body over the budget that arrived from disk is trimmed
  oldest-first on load; the encrypted-file read cap is derived from the
  plaintext budget plus `crypto::OVERHEAD_BYTES` instead of a hardcoded
  slack. (`src/bookmarks/`, `src/config/`, `src/store/crypto/`,
  `docs/SECURITY.md`.)
- CSS-spelled SVG filters are rejected in every spelling the CSS parser
  actually accepts: the scan now skips comments and whitespace between the
  property name and the `:` (`style="filter/**/:blur(5px)"`, the spelling
  `simplecss` itself reads as a `filter` declaration) and accepts the IE
  star hack (`*filter:blur(5px)`), closing the last bypass of the
  "no filter can ever run" contract. (`src/media/decode/`,
  `docs/SECURITY.md`.)
- A store file restored world-readable is tightened through the ALREADY
  OPENED handle (`fchmod`) instead of a fresh path lookup, and a
  credentials-bearing `settings.bin`/store path is never `chmod`ed through
  a swapped-in symlink. (`src/settings/`.)
- Crash-orphaned key-creation temps (`store.key.tmp.<pid>.<seq>`, which
  hold RAW key bytes) join the orphan sweep, so a crash between the private
  temp write and its `hard_link` no longer leaves key material on disk.
  (`src/settings/`, `docs/SECURITY.md`.)
- A relative `XDG_DATA_HOME` is ignored (the XDG spec requires an absolute
  path): the stores (and `wipe_all_data`'s `remove_dir_all`) can no longer
  resolve under the current working directory.
  (`src/settings/platform.rs`.)
- Session restore rejects URLs with credentials on EVERY scheme (the
  `https` arm accepted them; only `file:` was checked), so a tampered
  session file cannot seed a credential-bearing URL into the restore path.
  (`src/settings/`.)
- `get_bytes`' own fail-closed "credentials (blocked)" error no longer
  interpolates the credential-bearing URL it just failed to sanitize (it
  reuses the shared `util::strip_credentials_for_log`), and the common
  credential-free request path no longer clones the URL.
  (`src/protocol/client/`, `src/util.rs`.)
- `atomic_write` refuses every write once the data directory was wiped
  (`settings::suppress_writes`), so a background worker draining its queue
  cannot resurrect a deleted store; the "Delete all data and close" path
  sets it before draining. (`src/settings/`, `src/app/`.)

## [0.3.3] - 2026-10-10

### Added

- First-run bookmarks: a fresh profile (no bookmark store yet) starts with
  a curated default set of five (Markdown guides, an agent-readiness
  leaderboard and developer-service docs). Seeded once, never
  recurrently: an existing store — even one the user emptied — is never
  reseeded (existing profiles keep the older eight). Favicons load through
  the usual Home tile pipeline (one probe per origin on first display).
- Address autocomplete in BOTH address fields (toolbar omnibox and the
  Home capsule), ranked bookmarks → local history → a curated builtin list
  compiled into the binary (`suggest::BUILTIN_SUGGESTIONS`: the five seeded
  bookmarks plus developer-service docs — kept out of the first-run
  grid — plus a developers-portal entry; append one `(url, title)` line
  to grow it), deduplicated by exact URL and matched
  case-insensitively on URL + title with or without scheme; an empty field
  offers recent bookmarks. `Down`/`Up` move the selection, `Tab` completes
  it, `Enter` or a click opens the selected row (standard), `Esc` closes the
  popup before reverting the field, and the popup is a foreground overlay
  that never shifts the capsule. The list is capped at
  `config::MAX_OMNIBOX_SUGGESTIONS` and cached per field in egui memory,
  rebuilt only when the query or the `HistoryStore`/`BookmarkStore`
  generation changes (the new `BookmarkStore::generation()` mirror of the
  history one), so typing filters live without a per-frame rescan; closed
  tabs drop their cache. Neither launching the app nor opening a new tab
  focuses an address field on its own, so no suggestion popup opens over a
  fresh window or tab (only `Ctrl/Cmd+L` focuses it). (`src/suggest.rs`,
  `src/ui/omnibox.rs`, `src/bookmarks.rs`, `src/ui/navbar.rs`,
  `src/ui/home.rs`, `src/ui/viewer/mod.rs`, `src/app.rs`,
  `docs/INTERFACE.md`, `docs/NAVIGATION.md`, `docs/ARCHITECTURE.md`.)
- The new-tab page is a real navigation entry (`about:home`, typeable like
  `about:history`): Back and Forward traverse Home instead of dropping the
  Forward chain when passing through it, while the capsule stays empty and
  Reload stays disabled on it. The session file keeps the `None` Home
  marker (no format change; existing restores behave the same).
  (`src/config.rs`, `src/browser/tab/`, `src/app.rs`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Preferences gains "Delete all data and close" (Local data card,
  two-click confirmation like "Delete history"): deletes the whole per-user
  data directory (`history.bin`, `bookmarks.bin`, `settings.bin`,
  `session.bin` and the shared `store.key`) and exits immediately with
  `process::exit` without further saves
  (`app::MdowserApp::wipe_all_and_exit`, `settings::wipe_all_data`).

### Changed

- Tab rail: the expanded overlay now collapses only when the pointer moves
  into the document area — moving up to the toolbar or leaving the window
  keeps it open — and the open/close tween is slightly slower with a more
  decisive `cubic_out` easing (`EXPAND_ANIM_SECS` 0.16 → 0.22 s).
  (`src/ui/tabs.rs`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Hardening + slimmer frames, no behavior change: window geometry lives in
  `config.rs` (`WINDOW_INITIAL_*`/`WINDOW_MIN_*`, shared by `main` and
  tests) and OS/CLI open targets are capped at `MAX_TABS` before any tab is
  built (`main::collect_initial_targets`); every wall-clock deadline goes
  through `config::deadline_after` (fail-closed `checked_add`); egui id
  salts are tuples everywhere (error scroll, menu, toast); right-click link
  hitboxes hold interned `Arc<str>` handles instead of one `String` copy per
  fragment per frame; unordered list bullets are borrowed (`Cow`), only the
  ordered prefix allocates; context-menu shortcut hints are `&'static str`.
  (`src/config.rs`, `src/main.rs`, `src/protocol/client.rs`,
  `src/media/cache.rs`, `src/media/favicon.rs`, `src/ui/history_page.rs`,
  `src/ui/viewer/`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- Single shared key for every encrypted store: `store.key` now encrypts
  `history.bin` (magic `MDHS`), `bookmarks.bin` (`MDBM`), `settings.bin`
  (`MDST`) and `session.bin` (`MDSS`). Settings and session are no longer
  plaintext. Clean break, no migration: legacy `settings.conf` /
  `session.conf` and `history.key` / `bookmarks.key` are deleted on load,
  never read. "Delete history" now keeps the shared key (bookmarks/settings/
  session still need it); only the wipe-all path removes it with the whole
  directory. (`src/settings.rs`, `src/history.rs`, `src/bookmarks.rs`,
  `src/crypto.rs`, `docs/SECURITY.md`, `docs/ARCHITECTURE.md`,
  `docs/INTERFACE.md`.)
- Fetch tries the standard spellings only: directory indexes are
  `index.md` + `README.md` (`config::DIR_INDEXES`), the `.markdown`
  extension and `readme`/`index.markdown` variants are never requested
  (legacy `.markdown` URLs still normalize to the same logical page).
  The cascade shrinks from 9 to 5 candidates
  (`src/protocol/resolver.rs`, `src/protocol/local.rs`,
  `src/ui/error_view.rs`, `docs/NAVIGATION.md`.)
- `make` now builds the host release binary AND the current-OS dist into
  `dist/`: on macOS the full native group (`build` + `dist-mac`, i.e. the
  universal `.app.zip` + per-arch `.tar.gz`); elsewhere `build` + `dist-host`
  (fast, no cross). `make dist`
  is the host release binary plus the full multi-OS dist (`build` +
  `build-dist.sh --all`, which now starts with the host group, so a direct
  script run also leaves `target/release/mdowser` behind).
  `build-dist.sh --host-only` no longer drops a raw
  `mdowser-<ver>-host` file: it only packs the current OS archive with the
  same docs every other artifact carries (Unix `.tar.gz`, Windows `.zip`,
  plus the matching `.deb` on Linux when packaging succeeds), reusing the
  per-OS names so `make` output is a subset of `make dist`.
  (`Makefile`, `scripts/build-dist.sh`, `docs/BUILDING.md`, `README.md`.)
- Standardized all UI/chrome bounds in `src/config.rs`
  (`BOOKMARK_GRID_MAX_VISIBLE_ROWS`, `MAX_INLINE_VECTOR_H`,
  `MAX_INLINE_IMAGE_H`, `TAB_TITLE_CHARS`, `WINDOW_TITLE_CHARS`,
  `WINDOW_TITLE_ERROR_URL_CHARS`): the Home grid, inline-image promotion,
  tab titles and OS window titles now share the same auditable caps instead
  of local magic numbers (`ui::viewer::image` re-exports the canonical
  values, `docs/ARCHITECTURE.md` points at `config::`). No behavior change.
- Trimmed per-frame work without changing rendering: the menu detector
  rejects oversized paragraphs on a byte sum before joining them (no
  body-sized `String` per paragraph per frame), `Tab::favicon_origin`
  borrows the loaded URL instead of cloning it, image accessible names use
  `Cow` (one allocation saved per image) with borrowed hover tooltips, the
  toolbar reload branch avoids building the countdown string off-cooldown,
  `theme::card`/`display`/`weak` share the canonical constructors
  (`ui::history` cards reuse `theme::card_with`), and `RichText::new` takes
  `&str` directly.
- Tooltips are shown through one lazy helper (`ui::hover_tooltip`): egui's
  `Response::on_hover_text` converts its argument EAGERLY, so every icon
  button, tab row, tab close button, image and hand-painted control built
  one `String` per frame whether or not a tooltip was displayed. The helper
  gates on `hovered()`, keeping the allocation to the frames that actually
  show the tooltip (egui's hover delay is unchanged). All 16 call sites
  converted.
- The two largest embedded fonts now ship Brotli-compressed and are
  decompressed once at runtime: `NotoColorEmoji-Regular.ttf` (24.2 MB) and
  `NotoSansCJK-Regular.ttc` (18.6 MB) compress to 6.3 MB + 11.2 MB at build
  time (`build_support/gen_fonts.rs`, quality 10, cached in `OUT_DIR` with
  a hash stamp), shrinking the release binary from 81 MB to 55 MB with
  byte-identical rendering. `src/assets.rs` exposes
  `noto_color_emoji()`/`noto_sans_cjk()` backed by `OnceLock`: the color
  emoji face is decompressed LAZILY (the first color emoji a session
  renders — pages without emoji never allocate it), CJK once at startup,
  each into exactly ONE process-lifetime buffer that epaint, the SVG fontdb
  and `media::emoji` all borrow (no copies). The build round-trip validates
  every blob (decompress == source) before the crate compiles, and the
  generated `font_meta.rs` records the uncompressed lengths so the
  allocation is exact. The SVG fontdb also stops copying epaint's four
  bundled faces (~1 MB) and shares them through the `Arc` instead. Build
  scripts now run optimized (`[profile.*.build-override] opt-level = 3`):
  the q10 encoder needs ~90 s instead of ~10 minutes at the default
  `opt-level = 0`, and the icon rasterizer benefits too.
  (`Cargo.toml`, `build.rs`, `build_support/gen_fonts.rs`,
  `src/assets.rs`, `src/ui/theme.rs`, `src/media/decode.rs`,
  `src/media/emoji.rs`, `docs/ARCHITECTURE.md`, `docs/BUILDING.md`.)
- Documentation hygiene: `docs/BUILDING.md` dev commands now match the
  canonical `make`/`scripts/build-test.sh` flags (`--locked --workspace`),
  `docs/ARCHITECTURE.md` references the canonical `config::` bounds, and
  `.cargo/audit.toml`/`deny.toml` wording no longer implies a CI service
  (the repo intentionally has no `.github/` automation; `make audit` is the
  gate).
- Code organization, no behavior change: the three largest sources are now
  directories of focused submodules, split by pure moves (the suite covers
  every one): `ui/viewer/markdown/` (`mod` holds the walk and the block
  budget; `frags` the inline collection, `items` the list markers,
  `blocks` paragraphs/headings/tables/HTML, `links` link runs and callouts,
  `menu` the inline nav heuristic), `browser/tab/` (`mod` keeps the state,
  accessors and poll loop; `nav` every navigation/stack method) and
  `markdown/document/` (`mod` parser options + event walk; `normalize` the
  pre-parse repair pass; `plain` the Copy-text derivation).
  (`docs/ARCHITECTURE.md` module map updated.)
- Preferences dialog compacted (`src/ui/history.rs`): shorter fixed size
  (480x720), tighter header bottom margin, and History/Local data cards
  fit their rows with no reserved bottom space — each delete-confirmation
  note only takes space while armed, pushing the cards below it
  (`docs/INTERFACE.md`, `docs/ARCHITECTURE.md`).
- Toolbar bookmark is a ribbon, not a star: the omnibox right slot shows the
  outline `Icon::Bookmark` when the page is not bookmarked (or there is
  nothing to bookmark) and the new filled `Icon::BookmarkFilled` when it is,
  matching the Home bookmark header; `Icon::Star`/`StarOutline` are gone.
  Every top-bar button now owns a fixed slot whatever it renders
  (Reload/Stop share one slot, the ribbon shares the clear-X slot, the zoom
  pill keeps its per-mode width), so swapping an icon never shifts its
  neighbours. Root cause fixed: a disabled button faded through an empty
  `ui.scope`, which advances the cursor one item spacing — disabling Reload
  on cooldown pushed the omnibox 2px right on every load. The fade is now a
  faded `Painter` clone with zero layout effect, locked by an icons test
  (enabled/disabled marker) and a navbar test over reload/cooldown/loading,
  ribbon and zoom states. (`src/ui/icons.rs`, `src/ui/omnibox.rs`,
  `src/ui/navbar.rs`, `src/app.rs`, `src/bookmarks.rs`,
  `src/browser/tab/mod.rs`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)

### Fixed

- Home bookmark rename: while the inline name field is open the blue ring
  paints only around the editable text field (1px), no longer around the
  whole tile; the drag-reorder ring is 1px too. (`src/ui/bookmarks.rs`.)
- Build scripts no longer trigger the macOS `rust-objcopy` warning: cargo
  >= 1.77 strips debuginfo from build dependencies automatically
  (`-C strip=debuginfo`), and the toolchain's `rust-objcopy` is broken on
  some installs (missing `libLLVM.dylib` -> `SIGABRT`), so every build
  printed "stripping debug info with `rust-objcopy` failed" once per build
  script. Both build-override profiles now set `strip = "none"`, the same
  opt-out the release profile already uses; build scripts never ship, so
  only `target/` grows. (`Cargo.toml`, `docs/BUILDING.md`.)

### Security

- Fail-closed wall-clock deadlines: slot waits, image/favicon budget
  backoffs and the history delete confirmation go through
  `config::deadline_after` (`checked_add`, expiring immediately on the
  unreachable overflow) instead of `Instant + Duration`, so no UI or
  background thread can panic on time arithmetic. The OS/CLI open-target
  list is capped at `MAX_TABS` before any tab is built. (`src/config.rs`,
  `src/main.rs`, `src/protocol/client.rs`, `src/media/cache.rs`,
  `src/media/favicon.rs`, `src/ui/history_page.rs`, `docs/SECURITY.md`.)
- The SVG structural pre-pass pins `allow_dtd: false` explicitly instead of
  relying on the XML library's default: a DTD that reaches usvg's
  entity-expanding parser is rejected first, and a library-default change
  can no longer reopen unbounded entity expansion silently. The SMIL
  rejection list also covers `animateMotion` and `discard` on top of
  `animate`/`animateTransform`/`set` (all are ignored by the static
  pipeline today; refusing them structurally keeps a future parser from
  ever executing them). (`src/media/decode.rs`, `docs/SECURITY.md`.)
- `file:` destinations carrying credentials or a port
  (`file://user:pass@localhost/x`, `file://localhost:8080/x`) are inert in
  the link rewriter, exactly like remote hosts: the reader rejects them at
  navigation time, so a local document no longer renders a live link whose
  only outcome is destroying the page, and credentials can never ride along
  a destination. (`src/protocol/rewrite.rs`, `docs/SECURITY.md`.)
- The encrypted stores (`history.bin`, `bookmarks.bin`, both `settings.bin`
  reads) now load through a shared `read_regular_file` helper that opens
  with `O_NONBLOCK` and re-checks the OPENED handle before reading: a FIFO
  swapped in between the metadata check and the open can no longer block
  the UI thread forever (the same recipe local document reads already
  used), and the streaming read enforces its cap even if the file grows
  after the metadata check. (`src/settings.rs`, `src/history.rs`,
  `src/bookmarks.rs`.)

## [0.3.2] - 2026-10-08

### Changed

- The OS window title is now just the open page's own title (frontmatter
  `title`, else the schemeless URL, `Error: …` on failures) with no app name
  or `Tab N:` prefix; on Home — nothing open or a new tab — it is `MDowser`
  (`browser::tab::window_title`).
- SVG filters are IGNORED instead of failing the whole image
  (`media::decode`): `<filter>`/`fe*` definitions, primitives and every
  `filter` reference (element and attribute spellings, namespace prefixes
  included) are stripped from the source before `usvg` parses it, and the
  unfiltered content renders — a status badge carrying a blur shadow
  now shows its flat badge instead of the failure placeholder. Removing the
  references as well is required: usvg DROPS an element whose `filter` link
  dangles. No filter can run (the `numOctaves`/morphology/convolution cost
  and filter pixmaps stay unreachable): CSS-spelled `filter` declarations
  (`style="filter:…"`, a `<style>` rule) still reject the image, and
  `marker`/`pattern` stay rejected. `docs/SECURITY.md` documents the new
  posture.

### Fixed

- Home bookmark grid: the floating scrollbar is hidden while the grid fits
  (`ui::bookmarks`): the inner scroll area reported its trailing item gap as
  overflow, so a full-height bar sat next to the last tile (and lit up on
  hover) with nothing to scroll. It now appears only once the grid actually
  exceeds `MAX_VISIBLE_ROWS`.
- Bookmark tile titles are elided by the layout instead of clipped: a title
  that needs more than the tile's two centered lines (long paths, CJK) now
  lays out with `max_rows = 2` and an `…` on the second row; the old clip
  rect cut the third row mid-glyph with no ellipsis.
- Preferences: `Esc` with an open dropdown closes only the dropdown
  (`app::handle_shortcuts` checks `egui::Popup::is_any_open`, tooltips
  excluded) instead of closing the whole dialog along with it.

## [0.3.1] - 2026-10-07

### Changed

- Image texture-budget retries actually count now (`media/cache.rs`): the
  per-URL counter was cleared before the budget check, so a saturated
  process-wide pool re-downloaded and re-decoded every image until the 80s
  job deadline; the counter is only cleared by a successful upload (and on
  permanent failure), so `MAX_IMAGE_BUDGET_RETRIES` applies as documented.
  The color-emoji cache gets the same bounded attempt count (after it, the
  cluster keeps the monochrome fallback instead of re-rasterizing forever),
  and the emoji grapheme split is skipped for text without any emoji
  candidate, removing one `Vec` per plain fragment per repaint.
- Encrypt builds the output buffer once (`header | nonce | plaintext`, then
  seals the tail in place): the plaintext is copied exactly once instead of
  into a body buffer and again into the result, halving the transient
  allocation on the UI thread (bookmark saves are bounded by the store cap).
- The bookmark star state no longer re-parses the URL and linearly scans the
  store every frame: `BookmarkStore` keeps an exact URL index in lockstep
  with its entries and the toolbar asks it through the already-parsed
  `display_url` (a new borrowed `Tab::loaded_url`); the loaded-URL accessor
  also stops cloning the URL and title for a state check.
- `host:port` link destinations the omnibox accepts (`localhost:3000`,
  `example.com:8080/x`) resolve as `https://` instead of rendering inert;
  blocked schemes (`javascript:`, `data:`, `mailto:`) and userinfo forms
  still never become host:port.
- Graphics polish: code inside a heading link renders monospace like code in
  a heading; the dark-mode pressed state on toolbar and omnibox controls
  lightens (it used to darken toward rest, giving no feedback); the
  Preferences "View third-party licenses" link shows keyboard focus; task
  checkboxes announce done/pending, bookmark tiles draw the shared focus
  ring, the inline rename field and image placeholders carry accessible
  names; context-menu and dropdown rows share one corner radius (6 vs 8
  before); the expanded tab rail's action block height is DERIVED from the
  same constants the layout uses, so the separator can no longer cut across
  the bottom of the tab list; and the floating scrollbar's lane is reserved
  by the history rows' remove button, the tab close button and the Home
  grid's rightmost tile actions, so the bar can neither cover nor steal
  their clicks.
- Build hygiene: the icon generator's own source is hashed into the
  regeneration stamp (editing `build_support/gen_icons.rs` can no longer
  leave stale committed icons while the build "succeeds"), the Windows
  resource-compiler variables (`RC`, `RC_PATH`, `RC_<target>`,
  `ProgramFiles*`) are part of the rebuild fingerprint, and the PATH lookup
  requires the executable bit on Unix (a non-executable file named
  `windres` used to downgrade the build to "no icon" silently).
- The icon generator is integrated into the build. The standalone
  `tools/gen-icons` workspace package is gone: `build.rs` and
  `build_support/gen_icons.rs` rasterize `assets/icon.svg` into
  `assets/icons/` whenever the committed outputs are missing or outdated
  (a content hash of the SVG is kept in `OUT_DIR`, so a normal build only
  reads one small file and stops). `src/assets.rs` embeds `icon-256.png`
  with `include_bytes!`, so a fresh checkout or a deleted `assets/icons/`
  now bootstraps itself with no manual step; `make icons` forces a
  regeneration. Same `resvg` version and features as the app's own SVG
  pipeline, so regenerating reproduces the committed files byte for byte,
  and the script never links into the binary (build dependencies run on the
  host).
- Safety caps retuned across the board so the browser handles longer
  documents, bigger images and deeper local history, keeping every bound
  explicit in `src/config.rs`: documents 5→10 MiB; images 10→16 MiB /
  24→48 MP (12288 px per side, 3072 px uploaded); favicons 512 KiB→1 MiB
  at 384 px; parsed events 500k→1M; rendered blocks and code lines
  5k→10k; a rendered prose/HTML/table block 256→512 KiB; table columns
  1024→4096; bare-URL autolinks 10k→20k; anchors 10k→20k; link joins
  4096→8192; route timeout 10→20s and streamed-chunk stall 10→15s (the
  visual cascade budget derives from them); redirects 5→8; concurrent
  document fetches/local reads 4→6; network workers 2→4; image jobs
  4→6 (3 per document); texture budgets 256→384 MiB per document and
  512→768 MiB process-wide; emoji caches 512→1024 entries / 32→64 MiB;
  per-tab Back/Forward 100→250; local history 5000→20 000 entries and
  2→8 MiB; bookmarks 1000→2000 entries (tile glyph 8→16 chars); URL
  input 4096→8192 chars; dest-cache budget 4→8 MiB; settings read cap
  64→256 KiB. The recursion-depth, refresh-cooldown and image-retry
  caps stay as they were: those bound stack safety and request
  amplification rather than capacity.

### Security

- SVG `<pattern>` elements are rejected by the structural pre-pass: a
  pattern's pixmap is sized by the PATTERN's own rect, not by the rendered
  target, so a tiny remote SVG could demand an allocation whose failure
  aborts the process through `tiny_skia::Pixmap::new`'s infallible
  allocator, bypassing every raster-size cap. DTD rejection moved into that
  same structural parse (roxmltree with `allow_dtd: false`, before usvg's
  entity-expanding parser runs): a comment or CDATA section that merely
  contains the text `<!DOCTYPE`/`<!ENTITY` no longer rejects a valid
  exported SVG, and the guarantee no longer depends on a raw byte scan.
  `docs/SECURITY.md` and `docs/ARCHITECTURE.md` list `pattern` and the new
  DTD wording.
- Corrupt-key repair is now serialized END TO END (`src/crypto.rs`): the
  best-effort cross-process lock is held until the replacement key is
  installed, and a lock timeout fails closed (the file is never deleted)
  instead of unlinking a path another instance may just have filled with its
  complete fresh key. Two instances repairing at once can no longer leave a
  winner whose key is not on disk (every blob it encrypted would have been
  silently undecryptable). An oversized key file is rejected from its
  metadata without being read whole.
- `atomic_write` replaces the destination in ONE rename on every platform
  (`std::fs::rename` replaces an existing target on Windows too, through
  `MoveFileExW`/`FileRenameInfoEx`): the old Windows move-aside `.bak` dance
  could leave the store missing after a crash between the two renames. The
  unreadable-blob `.bak` preservation no longer clobbers an earlier backup
  either: a second unreadable blob needs the same old key, and the first one
  kept is the oldest recoverable data.
- An unreadable `settings.conf` (permissions, invalid UTF-8, over the cap)
  is reported in Preferences and never treated as "restore tabs off": a
  transient read failure can no longer delete the plaintext `session.conf`.
- "Delete history now" also sweeps orphaned `history.key.tmp.*` copies an
  interrupted key installation can leave behind (a full 32-byte key must not
  survive an explicit delete).
- A fence-like line whose backtick info string contains a backtick
  (```` ``` x` ````) no longer opens fence state in the pre-parse table-bomb
  guard: pulldown-cmark keeps it as paragraph text, so treating it as a
  fence let a 4097-column table hide behind it.
- Missing bounds added with the retune: SVG nesting is DEPTH-capped
  (`MAX_SVG_DEPTH`) before the XML parser runs (a deep `<g>` chain
  overflows the recursive parser/usvg stack even under the token count,
  which closing tags inflate without nesting), tables are capped at
  `MAX_TABLE_ROWS` rows per block (each row also spends the per-frame
  `MAX_RENDERED_BLOCKS` budget, so many tables cannot bypass it),
  right-click link hitboxes at
  `MAX_LINK_HITS` per frame and the `about:history` search field at
  `MAX_HISTORY_FILTER_CHARS`.
- Link/image destinations are interned (`DestCache` and the fragment
  tree hold `Arc<str>` handles): a link split into thousands of
  fragments or a document repeating one long URL shares one allocation
  instead of copying the destination per fragment, closing a memory
  amplification (~1000x the body) no other cap bounded.
- Code-block cache galleys now enforce the cache byte budget on cache
  HITS: a theme/select-all change rebuilds every visible block's
  galleys without a miss, which used to bypass eviction and keep the
  whole document's laid-out glyphs past the declared budget.

### Fixed

- Opening a file (`mdowser notes.md`, a `file://` URL) with "restore tabs on
  startup" enabled no longer overwrites the stored session on the first
  frame: OS/CLI targets win for the current launch, and the session file is
  only rewritten once the open tabs actually change (the exit hook follows
  the same rule). Double-clicking a document can no longer discard the
  remembered session.
- Autolink correctness: a link run glued to a link's closing `)`
  (`[a](b)www.evil.com`) no longer linkifies (it is not at a GFM start
  boundary), a domain glued to `@` stays text in every candidate form
  (`www.example.com@evil.com`, `https://example.com@evil.com`) matching the
  documented email rule (the URL parser would otherwise read the domain as
  userinfo and send the click to the part after the `@`), and multibyte
  candidates are capped by CHARACTERS like every other URL path.
- Documentation drift corrected across the tree: `CONTRIBUTING.md` points
  new UI icons at the monochrome vector set; `NAVIGATION.md` documents the
  8-redirect cap and the true local-file entry points; `SECURITY.md` and
  `ARCHITECTURE.md` describe the SVG pre-pass as rejecting
  `<script>`/`<foreignObject>`/SMIL elements (not "ignoring" them) and
  attribute `avif-decode` to BSD-3-Clause (`avif-parse` stays MPL-2.0);
  `BUILDING.md` lists the real archive contents and packaging tools;
  `INTERFACE.md` documents the tile icon priority (stored glyph, then
  favicon) and the cached-favicon loading dot; the README roadmap no longer
  lists the already-shipped tab persistence.
- `THIRD_PARTY_NOTICES.md` now credits the Hack font bundled by
  `epaint_default_fonts` (MIT + Bitstream Vera License) with the full
  Bitstream Vera text, and no longer calls the color-emoji pipeline an
  interface-icon source; the in-app `about:licenses` page picks the file
  up on the next build.

## [0.3.0] - 2026-10-07

### Added

- Internal third-party licenses page (`about:licenses`): the embedded
  `THIRD_PARTY_NOTICES.md` (the exact file shipped in every release archive)
  is rendered by the browser's own Markdown engine as a read-only page, with
  the same text selection, copy, right-click menu and live links as a
  document. Preferences gains a compact footer line with the version and the
  app license plus a "View third-party licenses" link that opens the page in
  a new tab; typing `about:licenses` works too and session restore brings it
  back like `about:history`.

### Security

- Remote memory amplification closed: `protocol::rewrite::DestCache` keeps
  ONE map (the image form is derived from the link form) under a
  `MAX_DEST_CACHE_BYTES` budget, and a resolved destination past
  `MAX_URL_INPUT_CHARS` is inert. A hostile link farm (~250k distinct
  destinations in a 5 MiB body) used to retain hundreds of MB per tab
  (gigabytes with a long base) long after every documented cap had passed;
  past the budget the remaining links render as plain text exactly like an
  unresolvable one.
- Same-document anchor normalization now requires the same ORIGIN:
  `//other.example/same/path#x` collapsed to `#x` and scrolled the current
  page instead of navigating, and `//server/share/x.md#s` from a local
  document could bypass the remote-file-share rejection the same way.
- Credential sanitization covers blocked non-hierarchical pastes:
  `mailto:user:pass@host` displays as `host` (the `mailto:` spelling stays
  intact only when no password follows the scheme colon).
- `atomic_write` creates its temp file with `create_new` (O_EXCL) and a
  fresh sequence on collision: a symlink pre-planted at a predictable temp
  path can no longer redirect settings/session/store writes.
- A redirect landing on a URL past `MAX_URL_INPUT_CHARS` is rejected before
  the body is read: `Location` is server-controlled while the display,
  history and rewrite paths all assume the input cap.
- Document fetches require EVERY declared `Content-Type` value to be a
  markdown type (a server labeling a payload `text/html` first and
  `text/markdown` second got a precedence game), matching the image gate.
- "Forget on exit" now covers the whole disk surface: the plaintext
  `session.conf` snapshot is never written while that retention mode is
  active, selecting the mode turns "restore tabs on startup" off and
  deletes any leftover, and startup refuses to restore a stored session
  before this normalization runs.
- Store-writer error reporting recovers poisoned locks instead of silently
  dropping "could not save history/bookmarks" (the store's retry contract
  depends on that notice).
- `normalize_stored_url` fails closed when credentials cannot be removed
  (`set_username`/`set_password` errors) instead of storing the URL.
- Store keys are now installed atomically (`src/crypto.rs`): the 32 bytes are
  written and synced to a unique private temp file and linked into place with
  `hard_link`, so two instances creating the key at the same moment can only
  read the winner's COMPLETE key — the old `create_new` + write could expose
  an empty file that the loser deleted, leaving the winner with an unlinked
  key and an undecryptable store. A corrupt (wrong-size) key is repaired
  under a best-effort cross-process lock with stale-lock takeover, so two
  instances repairing at once cannot delete each other's fresh key. A valid
  key that cannot be tightened to
  user-only permissions is still refused fail-closed, never deleted.
- Encrypted stores keep an undecryptable blob as a `.bak` (history/complex
  case: missing or regenerated key, corruption, tampering) before a later
  save could replace it, so restoring the matching key still recovers the
  data; `Delete history now` removes the backup too.
- Turning "Restore tabs on startup" off deletes the plaintext `session.conf`
  even when a previous delete failed, and also removes the orphaned
  `.tmp.*`/`.bak` siblings a crash can leave behind (all of them hold
  visited URLs in the clear).
- `settings.conf` is never rewritten from a failed read: a read error
  (permissions, invalid UTF-8) no longer looks like an empty file and
  silently drops the other preferences; persistence failures now surface in
  Preferences instead of "reverting" on the next launch.
- Stored history/bookmark URLs keep their exact path and query: only
  authority credentials (`user:pass@`) are removed from the URL itself.
  The previous log-line sanitizer also rewrote every nested
  `scheme://user:pass@` inside the URL (`?next=...`), recording — and later
  opening — a different page than the one loaded. Loading a store
  normalizes the URLs too, and the bookmark star compares the same way.

### Fixed

- A separator-dense paragraph (`a|` repeated tens of thousands of times) no
  longer builds one `Vec` and `String` per pipe per frame: the menu check
  rejects on joined length and a capped separator count BEFORE splitting
  fragments (the split-then-check order froze the UI on a ~100 KB
  paragraph).
- Footnote references with a colliding heading id now land on the footnote:
  `AnchorIndex` remembers the RESERVED id (`fn-a-1` when a heading claimed
  `fn-a`) and both the reference link and the definition use it; previously
  the click scrolled to `# Head {#fn-a}` and the definition was unreachable.
- Pipe-heavy lines inside fenced/indented code no longer reject the whole
  document: the table-bomb pre-scan is fence-aware (a JSON/CSV/ASCII-art
  line with more than `MAX_TABLE_COLS` pipes is legitimate code, not a
  table).
- A `:::` line INSIDE a fence in a `:::` admonition body no longer closes
  the admonition early: the body collector is fence-aware, so the fence
  stays intact and the trailing lines remain in the callout.
- Back/Forward and refresh restore the reading position: the document
  scroll state is keyed by (tab, URL) instead of the per-load generation,
  while a DIFFERENT document still starts at the top (the code/table caches
  keep the generation-mixed salt).
- Dark-mode tooltips, context menus, toasts and dropdown popups are
  readable: floating surfaces use the elevated `container` fill with a
  hairline `outline_variant` border (they were `#0d1117` on the `#0d1117`
  page with no border and no shadow in dark mode).
- The zoom pill no longer disappears when zooming in: responsive
  breakpoints measure the unzoomed window width (egui points shrink with
  the zoom factor), so the only visible way back stays visible.
- Non-square favicons (wordmarks, 32x16 `.ico` frames) keep their aspect in
  the tab rail and bookmark tiles instead of being stretched into a square
  slot.
- Select-all highlights inline emoji images too: the emoji painter fills
  its slot with the selection background instead of chopping the band.
- `Ctrl/Cmd+A` / `Ctrl/Cmd+C` are no longer swallowed by a focused button:
  the whole-page copy path checks `text_edit_focused` (a focused non-text
  widget used to count as "a field owns the keyboard").
- Preferences controls yield to the dialog at high zoom / narrow windows:
  the dropdown and the zoom-track widths cap to the available row instead
  of overrunning the card.
- `settings.conf` reads scan every line (the file is already byte-capped),
  so a file with more than 64 keys can no longer hide values the writer
  itself preserved.
- Closed tabs drop their egui-memory state (history search + filter cache,
  bookmark rename/drag drafts): ids are never reused, so every closed tab
  used to park its caches for the whole process.
- The debounced store writer no longer drops the newest queued blob on
  `flush`: the exit path could silently lose the last history visit or
  bookmark because `save_async` had already cleared `dirty`. The worker now
  writes what is queued before exiting, and a failed background write re-arms
  `dirty` so the synchronous exit save retries instead of trusting a clean
  flag.
- Images stuck in the queue past their deadline now become failures instead
  of staying "pending" forever (which kept the 120 ms repaint loop alive for
  the whole session when no image slot ever freed up).
- The global texture budget is treated as transient for both images and
  favicons: a refused reservation now backs off (`IMAGE_BUDGET_RETRY_SECS`)
  and retries — bounded by `MAX_IMAGE_BUDGET_RETRIES`/its favicon
  equivalent — instead of failing the image for the page's whole life or
  burning the network retry on a VRAM problem. Favicons that only failed on
  the budget are retried once VRAM frees up.
- A response declaring BOTH an image and a concrete non-image Content-Type is
  now rejected for image subresources (the gate accepted it as soon as any
  value looked like an image).
- Local-file document reads use their own slot limiter
  (`MAX_CONCURRENT_LOCAL_READS`), separate from network fetches: a slow or
  hung filesystem (NFS/FUSE) can no longer hold a network fetch slot and
  starve HTTPS navigation, and heavy network activity never queues a local
  open behind a network slot.
- A `#fragment` navigation that fails resolution no longer leaves the
  pending anchor behind (it could be applied to a document restored later by
  Back/Retry).
- `file:///a.md?x` and `file:///a.md` are the same document for history
  dedup, matching what the address bar displays (the query has no
  filesystem meaning).
- `atomic_write` on Windows moves the previous file aside to `.bak` and
  restores it when the swap fails, instead of deleting the target first (a
  failed rename or a crash could lose the whole store); on Unix the
  directory entry is `fsync`ed after the rename so a power loss right after
  a confirmed save cannot lose it. Decryption no longer copies the whole
  plaintext a second time.
- Preferences no longer clips its bottom option: the dialog is taller
  (480x640) so the full card stack fits together with the "Delete history"
  confirmation line and the persistence-error notes, and the confirmation
  line now claims a fixed-height slot, so arming (or expiring) it neither
  pushes nor cuts off the rows below. The inner scroll remains only for
  viewports smaller than the dialog.

### Changed

- Browser zoom keyboard shortcuts: `Ctrl/Cmd` `+`/`=` (in), `-` (out) and
  `0` (reset) drive the same factor as the toolbar pill and work while a
  field is focused, like in a desktop browser. Listed in the README and
  `docs/INTERFACE.md` shortcut tables.
- Hand-painted menu/action rows now show a pressed state (one step past
  hover), matching the icon buttons.
- Graphics: the interface icons are now a monochrome vector set
  (`ui::icons`): every toolbar/rail/omnibox glyph is hand-drawn on one
  24x24 design grid and tinted with the active theme — muted at rest,
  easing to the accent while growing ~10% on hover and pulling back on
  press. The color-emoji chrome (Noto Color Emoji textures through
  `media::emoji`) is gone; that pipeline now serves only document emoji
  and user-chosen bookmark tiles. Every icon variant paints vector shapes
  (a fallback to the text path fails the suite) and the button effect is
  covered by a dedicated test.
- Chrome icon sizing: favicons and the rail's icons (`ui::tabs`:
  Home/globe placeholders, `+ New Tab`, `History`) now paint at the
  toolbar's glyph size (`ui::icons::BUTTON`/`GLYPH`) and the Home bookmark
  tiles (`ui::bookmarks`) draw their favicons at that same size, so every
  icon and favicon in the UI reads as one set. The selected tab's accent
  bar moved into the rail's left gutter (a row-filling favicon covered the
  old in-row bar), the pinned action rows grew to the toolbar's rhythm and
  the expanded row's close X matches the omnibox slot.
- The toolbar Home button opens the new tab page (the landing page with the
  large omnibox and the bookmark grid) instead of the server root. The page
  left behind moves to `back`, so Back returns to it; a pending load is
  abandoned (cancelled).
- The omnibox padlock/file indicator now shows hover and keyboard-focus
  feedback and a pointing cursor (it focuses the address field when
  clicked), matching every other painted button.
- Performance (per-frame allocations): egui id salts are tuples instead of
  `format!` strings; the history page filter cache compares the raw query
  and rebuilds the lowercase needle only on a miss; error-view hints are
  borrowed `Cow`s; tooltips and accesskit labels (icon buttons, tab close
  buttons) are built lazily only while hovered/announced; the Preferences
  data-directory tooltip builds its path only on hover.

### Added

- Encrypted local bookmarks (`src/bookmarks.rs`): the toolbar star in the
  omnibox (filled while the shown page is bookmarked, outline otherwise,
  disabled when nothing is loaded) or `Ctrl/Cmd+D` toggles the page (toast
  confirmation), and the Home / new tab page shows them as a "quick
  access" tile grid — the only place the bookmarks themselves are listed.
  Tiles are responsive and virtualized, newest first, and use a stored
  custom glyph (letter/emoji) when one exists, else the site favicon when
  it resolves, else the first letter of the title on a stable color.
  Clicking a tile (its icon or
  its name) opens it in that tab and `Cmd/Ctrl+click` asks for a new tab;
  dragging a tile reorders the grid live. Editing happens ON the tile, with
  NO extra window (the earlier icon/color pickers are gone; stored glyphs
  and colors still render): the hover pencil opens an inline rename field
  whose same spot becomes a check that saves, Enter saves too and Esc drops
  the draft, and the hover X removes the tile. Same recipe as history:
  `bookmarks.bin` encrypted with
  ChaCha20-Poly1305 (magic `MDBM`, random 96-bit nonce, 256-bit key in
  `bookmarks.key` mode 0600 / directory 0700 on Unix), atomic temp+rename
  writes off the UI thread on the shared store writer (2s debounce, 5s
  flush timeout), kept until removed (no retention) and bounded to 1000
  entries — an add at the cap is refused with a toast instead of dropping
  an older bookmark. The store plaintext keeps the optional tile
  customization (`timestamp\turl\ttitle\ticon\tcolor`) and still parses
  three-field lines. The AEAD file format + key handling
  (`src/crypto.rs`) and the encrypted-store writer (`src/store_writer.rs`)
  are now shared with the history store.

### Changed

- Home / new tab page decluttered to exactly three elements — the wordmark,
  the large address capsule and the bookmark grid — with calmer spacing
  (84px above the wordmark, 36px before the capsule). The subtitle, the
  "Press Enter to open" hint, the examples / local-file notes, the
  image-format line and the keyboard-shortcut catalog are gone.
- Documentation: `README.md` is now a project overview (highlights, install,
  usage, docs index). The detailed catalogs moved to `docs/RENDERING.md`,
  `docs/INTERFACE.md`, `docs/NAVIGATION.md` and `docs/BUILDING.md`, and the
  performance model now lives in `ARCHITECTURE.md`.

### Fixed

- Session restore: the `session.conf` byte cap is derived from the
  percent-encoded form of a capped URL (`Url::to_string` can triple the byte
  size of non-ASCII input), so a session the app itself wrote is no longer
  silently rejected at startup (tabs lost after a restart); the reader
  accepts exactly what the writer can produce.
- "Delete history now" followed by quitting before the deferred wipe ran
  (`clear` arms the wipe while the background writer is busy) left the
  ciphertext AND its key on disk, so the deleted visits reappeared on the
  next launch; exit now completes a pending wipe before any save, and falls
  back to deleting anyway when a stalled disk exhausts the flush timeout.
- State machine: a locally rejected address while a page is on screen (loaded
  OR kept as a loading backdrop) shows a toast and lets the pending load run
  instead of destroying the page with an error log; a fetch that already
  produced a result is delivered even when the visual deadline passed
  between two polls (UI stall/suspend), matching the media caches.
- Graphics: dark-mode idle/hover/pressed widget states are now distinct and
  keyboard focus is visible (neutral active border + focus rings on
  hand-painted rows/buttons); the tab rail gap is the
  documented 2px (egui's default item spacing doubled it); the navbar
  reserves the right width when the zoom pill is hidden; the restore-tabs
  switch announces its label and close-tab buttons carry the tab title.
- Inline code spans: the pill padding (`.4em` / `.2em`, 6px radius) is now
  real layout air on BOTH sides instead of paint over the neighboring
  words, the code baseline lands on the body baseline, a soft-wrapped span
  paints one tight pill per row instead of one full-width box spanning the
  whole two-row label, a span that does not fit after the previous word
  moves to the next row as a unit instead of breaking mid-word and
  reserving an empty first row, and code inside links (and inert/backdrop
  link runs) uses the same tight pill instead of the full-line-box
  `RichText::code()` background with its gray band below the text.
- Tab rail: hovering no longer flashes the full 248px overlay in one frame.
  It grows out of the rail with a short `quadratic_out` width tween (the
  expanded-only titles/labels fade in, the right-edge separator follows the
  animated edge) and collapses the same way; egui's built-in area fade is
  disabled so it cannot stack a second animation on top.
- Omnibox/error view: malformed pasted credentials (`http:/user:pass@host`,
  `http:///user:pass@host`, `//user:pass@host`) are stripped by the log
  sanitizer instead of being echoed.
- `file://localhost/...` is accepted as the local machine by the reader and
  the link rewriter alike (previously rewritten as a live link and then
  rejected on navigation).
- `session.conf` requires its magic AND layout version, restores only the
  internal `about:history` page for `about:` URLs, and is size-capped before
  it is read at startup.
- Graphics: omnibox focus ring no longer eats text (`StrokeKind::Outside`),
  padlock click focuses the field, clear-X no longer double-handled;
  zoom readout uses `TextStyle::Small` so it scales; page sheet spans the
  full central panel (flush against the rail, rail-side corner still
  rounded) and its always-visible scrollbar hugs the sheet's right edge,
  with the responsive gutter applied inside the scroll content; reading
  column uses a single gutter source;
  tables cap columns at 60% of the card (no more 148px slivers); inline code
  wraps instead of overflowing under the scrollbar; error log uses an
  auto inner scrollbar at 320px; error dot scales with the title; photo
  galleries center vertically; bullets at 1.8x stay inside the 24px gutter;
  Preferences dialog clamps to 92%x90% of the viewport with an inner scroll.
- Tables: a table that cannot fit and scrolls horizontally now keeps every
  column at its measured width up to a wider scroll cap (128px; 112px on
  narrow windows) instead of collapsing all of them to the 48/64px fit
  floor, which broke words like "Header" mid-word. Fitting tables keep the
  existing shrink-to-fit behavior.
- Code/table scrollbars: the horizontal bar of a code card or a scrolling
  table now owns a lane UNDER the content, so the handle no longer passes
  over the last code line / table row while scrolling. Code cards keep their
  previous outer height (16px bottom = 4px padding + 12px lane) and a table
  card grows by the lane only when it actually scrolls (`theme::scrollbar_lane`
  applied through `ScrollArea::content_margin`).
- Tables: centered/right-aligned cells were laid out against the column cap,
  so every such column grew to `max_col_width` (340px) and pushed the rest
  of the table off the card (an `#`/Score/Grade leaderboard rendered as
  three 340px columns). Columns are now measured once per frame and
  rendered at that width, and a right-aligned multi-run line keeps its
  source order (egui's wrapping flow flips to RTL inside a right-aligned
  scope, which painted `**a** b` as `b a`).
- Code health: shared `ui::row::painted_row` (`RowSpec`) for menu rows;
  removed `eprintln!/println!` leftovers in tests, fixed the
  `7 routes` doc to `MAX_ROUTE_CANDIDATES`, unified `config::tests` imports,
  `sanitize_title` uses `MAX_TITLE_CHARS`, `doc_id_salt` reuses
  `media::fnv1a`, `card/code_card` share `card_with`, `build.rs` watches
  itself, `debug = 0` in release, `deny.toml` denies yanked crates,
  `.editorconfig` covers `*.toml`, CI `build-test` runs under `bash`.
- CommonMark/GFM "insecure characters": `U+0000` is replaced with `U+FFFD`
  in `normalize_body` before parsing. `pulldown-cmark` passes the raw NUL
  through its `Text`/`Code` events, so it used to reach the rendered text
  and the clipboard; the swap now happens on the body, before every parse
  (viewer, `plain_text` and anchors share it).
- Table bombs: a single line with more than `MAX_TABLE_COLS` unescaped pipes
  is rejected BEFORE the eager `pulldown-cmark` first pass allocates one
  tree item per cell (the event cap runs after that tree exists); the same
  cap stops the delimiter padding from building a body-sized string.
  Escaped pipes (`\|`) count as cell content, so a valid escaped-pipe table
  is no longer padded into an unparseable one.
- SVG `<use>` expansion: every reference is counted exactly like usvg
  expands it (the old "visiting" set exempted the mutual-recursion graphs
  usvg still expands) and whitespace-padded `href=" #id "` values are
  trimmed like `svgtypes::IRI`, so both shapes used to slip past
  `MAX_SVG_TOKENS`.
- Media: a decode that finishes between the deadline snapshot and the
  failure insertion is uploaded on the next poll instead of being discarded
  as "image timed out" (images and favicons); raster decoders apply the EXIF
  orientation before layout (phone photos no longer render sideways with
  swapped dimensions); the RGBA conversion moves instead of copying; each
  document runs at most `MAX_CONCURRENT_IMAGES_PER_DOC` jobs so one gallery
  cannot hold every global slot.
- History: a stalled flush can no longer respawn a second writer (which
  could reorder a stale blob over a newer one); a wipe stays armed until the
  draining writer exits; a failed `session.conf` delete is surfaced in
  Preferences instead of leaving plaintext URLs silently behind.
- Protocol: `http:\user:pass@host` (backslash authority) is rejected and
  sanitized like its slash spellings; fetched document bodies are no longer
  copied twice; `./#anchor` destinations resolve to the in-page anchor.
- Markdown: a code span ending in a GFM delimiter (`(`*`_`~` or space) no
  longer opens a linkified URL right after it; footnote targets are
  collision-reserved against heading ids; frontmatter returns `Cow` (no
  full-body copy when no block is present).

### Security

- SVG `<use>` expansion-bomb counter bypass closed: character references
  (`href="#bomb&#9;"`, `&#xA0;`) survive XML attribute normalization while
  `svgtypes::IRI` stops an id only at an ASCII space, so the old
  `trim()`-based resolver matched no `id` while usvg still expanded the
  graph un-counted. The structural pass now mirrors `IRI::from_str` byte for
  byte (ASCII whitespace only, trailing data rejected like usvg does).
- Credential sanitization covers every authority spelling (one slash, extra
  slashes, backslash, protocol-relative); malformed pastes can no longer
  reach the address field, the error view or "Copy log".
- Local reads accept only REGULAR files: a FIFO/device can no longer block
  `open` forever and permanently consume a global fetch slot; settings,
  session and history reads refuse non-regular files too, and the history
  file length check is 64-bit (a 32-bit wrap could read a multi-GB file).
- `atomic_write` removes its partial temp file on any create/write/sync
  error (a full disk no longer litters a `.tmp` per history retry).
- History `record` strips credentials defense-in-depth; tab/window titles
  sanitize `asked` before display; title filter drops variation selectors,
  tags and extra format chars; SVG structural pass also rejects `script`,
  `foreignObject`, `animate`, `animateTransform` and `set`.
- Local file access is confined to user-opened documents: a remote page can
  neither render nor navigate `file:` destinations, `file://host/` remote
  shares are rejected, and every local read reuses the document/image caps
  and the mandatory UTF-8 rule.

### Performance

- `DestCache` stores one map instead of two and skips inert destinations
  entirely, halving its retained strings on link-dense pages.
- Code/table width LRU eviction is one sorted pass instead of a
  `min_by_key` rescan per victim (`O(cap²)` inside a single frame when a
  new generation refilled a full cache).
- The media timeout sweep prunes its queue with a `HashSet` instead of a
  `Vec::contains` per entry (O(n²) on a page-wide timeout).
- Viewer per-frame allocations trimmed: code cards no longer re-truncate an
  already-bounded info string, and an unlabeled fence borrows `"code"`
  instead of allocating it per frame.
- Link/image destinations resolve ONCE per fetch (`protocol::rewrite::DestCache`,
  built from the parsed events + anchors) instead of `Url::parse` + `join` in
  the per-frame fragment tree; an unknown-anchor or over-cap destination is
  stored inert exactly as before.
- Table column widths are measured once and cached (content hash + column
  caps + selection; bounded LRU with a byte budget). Tables holding images or
  color emoji bypass the cache, since their widths change when a texture
  resolves. The invisible per-frame sizing pass no longer re-runs
  `render_frag` over every cell line of every table.
- Highlighted code blocks cache their laid-out line galleys (keyed by
  theme/selection/font size); a repaint clones an `Arc` per line instead of
  rebuilding a `LayoutJob` and copying every span's text.
- Image cache: `pump()` returns immediately when no URL is queued, so the
  per-frame `ensure` for every visible image no longer rescans the whole
  entry map (O(N²) per frame on a page of settled images).
- Color-emoji split: a scalar pre-scan (pictograph range, VS16, ZWJ, keycap)
  skips the grapheme segmentation + emoji-table lookups for emoji-free
  non-ASCII text (CJK, Cyrillic, Arabic), which ran per fragment per frame.
- Code-block cache is keyed by a per-tab document generation: resolving a
  page re-keys only that tab instead of flushing every tab's cached bodies
  and spans; normalized table delimiters, bare-URL linkification and menu
  detection each scan once (no duplicate pass per frame); the history
  filter cache is shared behind an `Arc` (a cache hit no longer copies the
  index); errors are classified once per frame; color-emoji textures are
  keyed without a per-lookup `String` allocation, evicted LRU instead of
  FIFO, and a budget retry schedules its own repaint; a global texture
  budget miss gets one bounded retry instead of a permanent failure.
- RTL fast path for pure-ASCII lines (no alloc/scan per paragraph);
  stable quote ids avoid per-frame `format!` churn; ordered-marker widths
  cached per (marker, size) with a 512-entry bound; inline-code font metrics
  cached per (body, mono) size; theme-pair logos compared token-by-token
  without `Vec<String>` allocs; image expiry scan avoids cloning the map.

### Added

- Local Markdown files: `file://` URLs and absolute/`~`/explicit-relative
  paths open as documents, with `.md`/`.markdown` completion for a stem and
  conventional `index.md`/`README.md` directory indexes (no directory
  listing). Relative links and images in a local document resolve against
  its directory and `https` links open normally. The omnibox shows the
  decoded path with a page indicator, History records local visits, "home"
  goes to the containing folder, and reads share the network caps (5 MiB /
  image caps, UTF-8 mandatory, cooperative cancel). A remote page can never
  link to or load a local file, and a `file://host/` remote share is
  rejected (`protocol::local`, `client::parse_navigable`, the `rewrite`
  scheme policy).
- Open targets from the OS/command line (`mdowser notes.md`,
  `file:///path/a.md`, `https://example.com/x`): one tab each, taking
  priority over session restore (`main` → `app::tabs_from_initial`).
- Linux `.deb`: the `.desktop` entry registers MDowser for
  `text/markdown` / `text/x-markdown` MIME types (`Exec=… %U`), so file
  managers can open `.md` files with it.
- GFM-style bare-URL links (`markdown::autolink`): URLs typed as plain text
  (`www.celticos.com`, `https://example.com/docs`) render as clickable links
  like markdown ones, and SUGAR beyond GFM: a bare domain without `www.`
  (`celticos.com`, `scholar-sidekick.com`, `agentready.md`) linkifies
  anywhere with `https://` inserted, and a local `file:` URL the reader
  accepts (`file:///Users/me/notes.md`, `file://localhost/…`) linkifies as a
  local file. Only those forms: `http:`, `mailto:`, `ftp:`, `data:` and
  every other scheme stay plain text, and emails never link (a domain
  followed by `@` is rejected). The `www.` form gets `https://` inserted;
  recognition needs a delimiter boundary and a valid domain, and trailing
  punctuation, unmatched `)` and entity-like `&x;` suffixes are excluded. A
  bare domain must end in a real IANA root-zone TLD (`markdown::tld`), so
  `Cargo.toml`, `notes.txt`, `data.csv`, `foo.local` and version tokens
  (`v1.2`, `1.2.3.4`) stay text; when that TLD doubles as a common file
  extension (`md`, `rs`, `sh`, `py`, `pl`, `ai`, `zip`, ...) a file-like
  stem (`README.md`, `config.rs`, `script.pl`, `logo.ai`) stays text while
  real hosts (`agentready.md`, `mkplandeki.pl`) link. Code spans/blocks,
  metadata blocks, existing links and image alt text are never touched;
  bounded by `config::MAX_AUTOLINKS_PER_DOC` / `AUTOLINK_MAX_DOMAIN_CHARS`
  and a linear scan, and Copy still pastes the URL as written.
- Preferences: a fixed-size, card-style dialog (custom header with a
  title and close button, grouped section cards, list rows, a toggle switch)
  that no longer resizes when an option changes. The History card holds the
  retention dropdown and the destructive "Delete history" action (two-click
  confirmation); the Appearance card holds a Theme dropdown
  (System/Light/Dark, applied immediately), a Default zoom slider and a
  "Restore tabs on startup" toggle that reopens the last session's tabs
  (HTTPS pages and `about:history`). Preferences persist in
  `settings.conf` key-by-key (read-modify-write), so the history retention
  and the new keys never clobber each other; the session lives in
  `session.conf`, written only while restore is enabled and deleted when it
  is turned off.

- README rendering styles: neutral palette (light/dark), H1/H2 bottom
  rules, muted H6 and blockquotes with alert-accent bars, bordered code cards
  with syntax colors (no line-number gutter), striped bordered tables,
  README body line height, 4px `<hr>` and icon-sized inline SVG.
- `:shortcode:` emoji expansion in text (never in code spans/blocks)
  plus `assets/NotoEmoji-Variable.ttf` (OFL-1.1, complete monochrome Noto
  Emoji) registered as the first fallback for UI and SVG text: recent emoji
  no longer render as tofu boxes.
- Multilingual rendering through the OFL-1.1 Noto family:
  `assets/NotoSansCJK-Regular.ttc` (the full Noto Sans CJK collection:
  Chinese, Japanese and Korean with kana/Hangul/Bopomofo, CJK Unified
  Ideographs + Ext A), `assets/NotoSans-Variable.ttf` (Latin
  Extended-A/Additional, Vietnamese) and `assets::SCRIPT_FONTS`, one face
  per major non-CJK script (Arabic, Hebrew, the Indic scripts, Thai, Lao,
  Khmer, Myanmar, Ethiopic, Armenian, Georgian, Thaana, N'Ko, Cherokee,
  Canadian Aboriginal, Symbols 2). All register between the emoji face and
  epaint's bundled fonts in the proportional and monospace families, and
  the SVG font database loads them with zero-copy `Arc` sharing, so
  `<text>` in those scripts rasterizes too.
- Color emoji (`media::emoji`): document emoji now render in color from
  `assets/NotoColorEmoji-Regular.ttf` (SVG-in-OpenType, OFL-1.1). epaint
  cannot paint color fonts, so the viewer shapes each emoji cluster with
  harfrust, isolates its `glyph<id>` element plus the transitive closure of
  its `#id` references from the font's shared SVG document, injects the
  em-square viewBox and rasterizes it through the hardened resvg pipeline.
  Textures are cached per (cluster, pixel bucket) under `config::EMOJI_CACHE_*`
  and the process-wide texture budget, and any failure falls back to the
  monochrome Noto Emoji text path. Links and code spans keep the monochrome
  text path.
- Right-to-left display order (`ui::viewer::bidi`): epaint applies no
  Unicode bidi algorithm, so the viewer now reorders RTL runs before layout
  (base direction from the first strong character, visual run order, word
  order, mirrored punctuation) and lays RTL paragraphs out from the right.
  harfrust still shapes the logical runs (Arabic joining, Indic
  reordering) and copy text stays logical; mixed-direction content split
  across inline elements (a link inside an Arabic sentence) remains
  approximate.
- Static SVG image support (`resvg`/`usvg`, pure Rust): icons, logos and
  badge images render. `<script>`/`<foreignObject>` are never executed
  (rejected by the structural pre-pass), no `href`
  (network, local or inline `data:`) is ever resolved, fonts are the app's
  own, and the raster is capped at 2048 px/side.
- One automatic retry per image after a transient network/stall failure,
  re-queued behind pending jobs; wrong content types and cancellations are
  never retried.
- Long link-only paragraphs (site card lists flattened by an exporter) stack
  one link per row; short link runs stay inline. Entries that carry a logo
  (`[![icon] Vendor<h3>Name</h3>Body](url)`) stack too, so a joined card list
  reads one card per row.
- Lax link joining (`markdown::document::normalize_body`): an inline
  link/image whose text an exporter split with a blank line (`[label` blank
  line `body](url)`, common in some docs-site `.md` exports) is repaired
  instead of leaking a literal `[` and a stray `](url)`. Only blank lines
  inside a balanced `[...](...)` span are removed (a whitespace-only line
  counts); ordinary paragraph breaks, fenced/indented blocks and inline code
  spans are never touched, and the pass is bounded
  (`config::MAX_LINK_JOINS` / `MAX_LINK_JOIN_DEPTH` plus a scan budget).
- Uniform inline SVG icon sizing: every inline SVG icon in one BLOCK
  (paragraph or link stack) renders at the tallest one's clamped height, not
  just consecutive icons on one line. A card list whose logos declare wildly
  different intrinsic sizes (`width="1em"` next to a 512px logo) now shows
  them all at one icon size.
- Light/dark logo pairs (`logo-light.svg` + `logo-dark.svg` adjacent) collapse
  to the single variant matching the document theme, so a docs card shows
  one logo instead of two. Only URLs differing in exactly one `light`/`dark`
  token pair up.
- Accessibility skip links leaked by `.md` variants (`[Skip to
  content](#main-content)`) are dropped instead of rendering.
- In-document anchors navigate: `[text](#heading)` scrolls to the heading
  whose slug or explicit `{#id}` matches, and `[^N]` jumps to
  its footnote; unknown anchors render as text. A `#fragment` on a
  cross-page link is applied after the load resolves. Heading/footnote
  targets are collected once per fetch (`markdown::anchors`).
- `:::` and `!!!` admonitions render as callouts; unknown types stay
  literal, code fences are never touched, and markers are stripped from the
  copied text.
- Markdown `Content-Type` aliases (`text/x-markdown`,
  `application/markdown`, `application/x-markdown`) are accepted alongside
  `text/markdown`; `text/plain` and everything else are still rejected.
- `.markdown` route candidates (`/love.markdown`, `/love/index.markdown`,
  home `/index.markdown`).
- Favicon probing also tries `/favicon.svg` after `.ico`/`.png`.
- Modern top toolbar (`ui::navbar` + `ui::omnibox`), closer to modern
  desktop browsers:
  back/forward arrows with a tail, circular hover fills, and one shared
  address capsule with a padlock security indicator, a clear `X` while
  editing, select-all on focus, Enter to navigate and `Esc` to revert to the
  shown page. No `Go` button anywhere.
- Reload/Stop slot: while a fetch is in flight the reload icon becomes a Stop
  `X` that cancels it and restores the page that was on screen
  (`Tab::stop`, backdrop + decoded images kept). `Esc` does the same when no
  field or context menu owns the key.
- Indeterminate loading sweep under the omnibox (60ms ticks) so a first load
  is never a blank wait.
- Compact zoom pill (`-` / `%` / `+`): the readout resets to 100% when
  clicked; the pill drops the readout on narrow windows and hides entirely
  on very narrow ones so the omnibox keeps the space.
- `Icon::Lock` and `icons::icon_button_at`, a custom-rect icon button
  (omnibox clear, zoom pill) with the same hover, cursor, tooltip and
  accessible-name rules as `icon_button`.
- App identity, everywhere the OS looks for it: `assets/icon.svg` (blue
  rounded tile with the white Markdown `M↓`) is the source for a generated
  raster ladder (`assets/icons/`, rebuilt with `make icons`): PNG 16-1024
  for Linux hicolor themes and the runtime window/taskbar icon
  (`assets::window_icon`), a multi-size `.ico` embedded into Windows `.exe`
  resources by `build.rs` (the shell, taskbar, Alt-Tab), and an `.icns`
  installed as `Contents/Resources/icon.icns` with `CFBundleIconFile` in
  the macOS `.app`. The `.deb` installs the hicolor icons and a desktop
  entry with `Icon=`/`StartupWMClass=`.
- Encrypted local browsing history with configurable retention
  (`src/history.rs`): every page that actually loads is recorded; the
  default window is 3 months and Preferences offers forget on exit, 1/3/6
  and 12 months, or forever, plus "Delete history now". The store is
  bounded (5000 entries / 2 MiB of plaintext) and lives in the standard
  per-user data directory (macOS `~/Library/Application Support/MDowser`,
  Linux `$XDG_DATA_HOME/mdowser`, Windows `%LOCALAPPDATA%\MDowser`) as
  `history.bin` encrypted with ChaCha20-Poly1305 (random 96-bit nonce,
  256-bit key in `history.key`, mode 0600; directory 0700). Nothing leaves
  the machine.
- History and Preferences UI: the local history opens as an INTERNAL PAGE
  (`about:history`) in a tab, rendered in the same reading sheet as a
  document (`src/ui/history_page.rs`): newest first, searchable, per-row
  remove, footer entry count and the two-click "Delete history now". The
  page is a real navigation (Back/Forward work) that never touches the
  network and never records itself, and the address bar shows
  `about:history`. The Preferences window (retention, storage and
  appearance) stays a floating overlay; the History button is
  pinned at the bottom of the tab rail (below `+ New Tab`),
  `Ctrl+H` / `Cmd+Y` opens the history page and `Ctrl/Cmd+,` toggles
  Preferences, and `Esc` closes the window (or stops a load).

### Changed

- Opening History (`about:history`) now creates a NEW tab, inserted right
  after the active one and activated, instead of replacing the page being
  read (tab rail button and `Ctrl+H`/`Cmd+Y` alike; at `MAX_TABS` it shows
  the usual toast). Clicking a row then navigates that same history tab to
  the visit, so Back returns to the list, and a repeat request while the
  active tab already shows the page is a no-op.
- Interface icons are color emoji now (`ui::icons`): every chrome glyph —
  toolbar, tab rail, omnibox indicator and clear action, zoom pill,
  `about:history` row remove, image placeholders and loading states — is one
  emoji cluster rendered through the same `media::emoji` pipeline as document
  emoji, served by one process-wide cache, with the monochrome Noto Emoji
  text face as fallback. Each glyph gets an optical size (solid signs draw
  smaller, narrow silhouettes larger) and rasterizes on a fine 2 px grid, so
  the set reads at one visual size and stays crisp. The hand-drawn vector
  path set and every
  `egui::Spinner` placeholder are gone, so the chrome and the content share
  one uniform set.
- Content-shape heuristic thresholds (menu entries/chars/words, link-stack
  gate, skip-link cap, anchor cap, route-candidate budget) are centralized in
  `config.rs` instead of inline literals.
- The skip-link filter is structural: only the document's first block, and
  only when it is a link to a fragment, so a plain paragraph reading
  "Skip navigation" is no longer dropped.
- The dot menu family now keeps the same navigation guard as pipes (a link
  or an uppercase/digit label is required), so `término · definición` stays
  a paragraph.
- Ignored network tests use generic public endpoints, and ICO/AVIF decode is
  covered by local fixtures, so no test names a third-party site.
- Bullet lists render like a document: every unordered item (source marker
  `-`, `*` or `+`, any depth) draws ONE large `·` in a fixed 24px gutter,
  aligned with the FIRST text row. The item text is a real hanging indent:
  every wrapped row and every later fragment stays in the text column (was:
  the literal source marker `- `/`* `/`+ ` chosen by depth, which read as
  raw markdown and let later rows fall back to the item's left edge).
  Ordered `N.` items share the same text column.
- Document typography follows the README scale: body 16px at 1.5 line height,
  headings 2em/1.5em/1.25em/1em/.875em/.85em, inline and block code at 85%.
- README rendering refreshed with a neutral documentation palette
  (light `#d1d9e0` borders; dark `#151b23` chrome, `#f0f6fc` text,
  `#9198a1` muted, `#3d444d` borders, updated syntax colors), inline code
  pills use `.2em .4em` padding and a 6px radius, code cards drop the
  1px border and use 16px padding and 1.45 line height, tables carry
  collapsed interior cell separators and a tight 5px/11px cell padding
  (slightly tighter than the common 6px/13px), task
  checkboxes paint like the native control (blue fill + white check when
  done, bordered box when pending), and alert titles read in title
  case at body size ("Note", not "NOTE").
- Quotes and alerts are ONE thick bar in a soft full-width box: egui's
  indent guide line is disabled (`Visuals::indent_has_left_vline`) so the
  bar is no longer doubled, the bar is 5px, the whole quote block carries a
  background (neutral `container_low` wash for plain quotes, ~9% alert
  accent for callouts), and alert bodies keep the normal text color while
  plain quotes stay muted.
- Tables fill the drawn card border: the measured content widths are grown
  proportionally to the card's inner width (no more blank strip on the
  right), or shrunk proportionally down to `min_col` when they do not fit,
  so long cells (links included) wrap instead of pushing a horizontal
  scrollbar; columns pinned at the floor leave the remaining space to the
  others (`ui::viewer::table::distribute_columns`). The grid now spans the
  card edge to edge (cells adjacent, no Grid spacing) and carries a tight
  5px/11px padding INSIDE each cell, so the interior lines touch the outer
  border while the text never touches a line. Quote/alert boxes span
  the full reading column and consecutive boxes keep vertical air
  (`QUOTE_GAP`) instead of touching.
- Vertical tab rail tuned: the collapsed rail is 44px (was 52px) so it hugs
  the favicons (12px of breathing room per side, same `+` glyph as the tabs).
  The hover overlay now reads as an extension of the toolbar instead of a
  floating card: square corners, no shadow above (which painted a gray seam
  under the toolbar) and only the right edge elevated (soft gradient in
  light, 1px hairline in dark, where a black blur reads as dirt). The
  expanded `+ New Tab` is the same borderless action row as the collapsed
  `+` (it used to be a bordered button).
- Home page: the card + field + `Go` are replaced by one large centered
  omnibox capsule (same widget and behavior as the toolbar), with a
  "Press Enter to open" hint.
- Toolbar button hovers are neutral circles (the icon keeps its on-surface
  color) instead of a blue tinted glyph.
- Repainted the toolbar icons with cleaner joins: the home house draws its
  roof, walls/floor and door as polylines (the roof meets the wall tops
  exactly, no seams or floating eaves) and the reload arrow opens a wider
  gap at the top with its head at the clockwise end of the arc; the zoom
  handle now starts on the lens rim instead of beside it.
- Removed the now-unused primary `filled_button_sized` helper and the
  `on_primary` token (the toolbar has no filled primary button anymore).
- One-line menus (`Uno | Dos | Tres`) render as a CENTERED nav row with
  muted separators; `vertical_centered` never centered a wrapping child, so
  they used to look like a left-aligned paragraph. The dot family (`·`,
  `•`, `∙`, `⋅`) is now recognized too (a dot only splits as its own token,
  so `col·legi` stays a word); pipes keep the all-lowercase prose guard,
  dots do not. Never mistaken for a table: delimited tables arrive as
  `Tag::Table`, not paragraphs.
- Images sharing a line with text stay inline only while small (icons,
  badges ≤2 body lines and ≤80% of the reading column); taller/wider ones
  are promoted to centered blocks (the remaining text flows as its own
  paragraph), so a photo never vertically centers the neighboring words or
  opens a huge blank line around them. Inline images get 3px of breathing
  room on each side.
- The page sheet now reaches the top, right and bottom window edges (was
  6/8/8px of outer margin) while keeping its 12px rounded corners (the
  chrome shows through the corner cutouts); the left keeps its 8px gutter
  against the tab rail, so the reading area wins those pixels.
- Inline fragments now borrow text from the parsed events
  (`Frag<'a>` + `Cow`): a repaint no longer copies the whole document.
  Copy text is derived once at fetch time (`Fetched.plain_text`), so
  Ctrl/Cmd+C never re-parses on the UI thread.
- Code blocks cache body + token spans per (document, block index,
  language): repaints neither re-copy nor re-hash a block.
- Favicon origins are resolved once per tab per frame (the request loop and
  the row drawing share them) and texture handles are no longer cloned.
- The top fade and the document hit-test share `theme::scrollbar_width`.

### Performance

- The `usvg` options (the ~25 MB bundled font database) are built once per
  process (`SVG_OPTIONS`) instead of once per image-decode thread.
- The color-emoji font, its ~14 MB shared SVG document index and the SVG
  rasterizer are prewarmed on a background thread at startup, so the first
  document emoji never parses them mid-frame.
- One rendered prose/HTML/table block is capped at
  `config::MAX_RENDERED_BLOCK_CHARS` (256 KiB): a crafted multi-MB paragraph
  used to become one giant label, laid out (and its layout job rebuilt)
  every frame.
- `coalesce_text` appends same-style runs in place (`to_mut().push_str`)
  instead of `format!`-reallocating the whole run on every merge (O(n2)
  per frame on a paragraph of hundreds of thousands of fragments).
- The code-block cache is a true LRU: a document with more blocks than
  `CACHE_MAX` no longer re-copies/re-tokenizes the blocks on screen.
- History saves: the encrypted blob is built on the UI thread and written
  (with `fsync`) by a serialized worker thread, so a slow disk never stalls
  a frame.
- `plain_text` runs on the already-normalized body at fetch time instead of
  normalizing the document a second time.
- The code-block cache holds at least as many blocks as one document can
  render (`MAX_RENDERED_BLOCKS`): the old fixed 64-entry LRU evicted every
  block before the next frame asked for it again (sequential-scan thrash),
  so a document with more than 64 blocks re-copied and re-tokenized every
  block on every frame. Tokenization is also skipped once the per-document
  line budget is spent.
- Per-frame copies of server-controlled strings are bounded at capture:
  image `title`/`alt`, link/image destinations and HTML block buffers no
  longer concatenate or `Url::parse` multi-MB values before their display
  caps.

### Fixed

- List markers align by their painted INK, not by their oversized line
  box: the document bullet `·` sits where a body-size middot would on the
  first text row (box centering let the scaled glyph descend to the
  baseline, about 4px low), hugs the text column instead of floating
  mid-gutter, and keeps its larger printed size (2.5x); ordered `N. `
  markers share the first row's baseline (was: ~3px low). Ink bounds come
  from the glyph atlas (`Glyph::uv_rect`), so no font metrics are baked
  in.
- List items keep a compact rhythm: the explicit per-item and
  per-paragraph gaps inside lists are gone (the implicit item spacing
  alone separates items), so a list reads as one block instead of a loose,
  airy stack.
- Inline code pills painted a gray band under the text: epaint sizes a
  `RichText::code()` background from the glyph's `line_height`, which the
  viewer pins to the 24px body line box, so the pill filled the whole row
  while the smaller code font's baseline also sat above the neighboring
  words. Code spans now render from a hand-built layout job (the baseline
  lands on the body baseline while the full body line box is kept) and paint
  their own rounded pill tight around the glyphs; select-all and
  newline-bearing spans keep the full-row background.
- Preferences dropdown rows no longer shift on hover: the default
  `selectable_label` only grew its frame (and inner padding) on hover, which
  moved the label and pushed the whole list. Rows are painted into a fixed
  rect now, and the window has a fixed size so toggling any option leaves
  every control where it was.
- SVG filter rejection was bypassable with namespace-prefixed elements
  (`<s:filter>`, `<s:feTurbulence>` carry neither `<filter` nor `<fe`): a
  structural pass rejects filter elements by LOCAL NAME in the SVG
  namespace, and bounds the memoized `<use>` expansion count against
  `MAX_SVG_TOKENS` (a 939-byte `<use>` chain expanded to usvg's 1M-node
  limit in seconds before).
- The color-emoji cache released nothing to the process-wide texture budget
  when it evicted an entry, shrinking `MAX_TEXTURE_BYTES_TOTAL` for the
  process lifetime until every image failed with "global image budget
  reached"; evictions now release immediately, and a full global pool is
  remembered for `EMOJI_BUDGET_RETRY_SECS` instead of caching a permanent
  miss.
- Alert-marker stripping sliced document text at fixed byte offsets and
  panicked when a multi-byte character straddled `marker.len()` (`>
  [!NOTE]` + `> Hello é` failed the whole route through the panic guard);
  the prefix test is boundary-safe.
- `parse_https` folded `http:/user:pass@host` (one slash) into host `http`
  with the credentials in the request path, the address bar and the route
  log; the userinfo `@` check is now scoped to the authority.
- A local rejection while `Loading` (`mailto:`, oversized input, bad URL)
  dropped the history snapshots, silently losing Forward entries that a
  failed network load preserves; local failures now restore
  `saved_back`/`saved_forward` (and stamp the refresh cooldown) like every
  other failed load.
- "Delete history now" ignored `remove_file` failures and reported success
  while the ciphertext/key survived (it reappeared on the next launch):
  failures surface in Preferences. Atomic writes use per-process unique
  temp names (two instances sharing one `.tmp` could interleave a
  store that fails to decrypt and wipes the history), and queued writes are
  flushed before a delete or exit.
- Alert callout bars were always the default gray: the look-ahead gave up
  on the first `Text` event, and `pulldown-cmark` splits `[!NOTE]` into
  `"["`, `"!NOTE"`, `"]"`. Consecutive text is concatenated before matching,
  so the bar takes the alert accent.
- Percent-encoded and mixed-case fragments never resolved to anchors
  (`#caf%C3%A9` for `café`, `#Section` for `section`): fragments are
  matched percent-decoded and case-insensitively (headings, footnotes and
  cross-page fragments).
- Heading ids desynchronized from the anchor index when the depth guard
  flattened a heading subtree, shifting every later anchor target; the
  flatten path consumes one heading slot per heading.
- Image failure tooltips could show server redirect URLs with credentials
  (`media::fetch` did not sanitize like the document path); reasons are now
  credential-stripped centrally.
- `logical_base("/love//index.md")` returned a trailing-slash base
  (candidates and display URLs showed `/love//`).
- Deterministic decode-cap failures (`image is too large`, `svg: more than N
  markup tokens`, allocation-budget verdicts) were treated as transient and
  re-downloaded the same body once; they are final.
- The frontmatter contract now matches the code in the docs: only a block
  that PARSES as YAML but is not a mapping renders as content; malformed
  YAML still moves to the next route.

- Several images of one page could fail permanently with
  `error sending request`: every job built its own short-lived tokio runtime
  while sharing one HTTP client, so the pool's idle connections lost the
  executor that drove them. All network futures now run on ONE long-lived
  runtime (`protocol::client::run_network`), and the one automatic retry is
  only needed for real transient failures (page subresources no longer drop
  out).
- The document renderer was clipped one gutter short of the sheet's flush
  top/bottom edges: the frame now keeps only the horizontal inner margin and
  the vertical margin travels inside the scroll content
  (`theme::content_vertical_pad`), so scrolled letters reach the edge while
  the at-rest spacing is unchanged.
- A run of consecutive large images (site photo strips) was promoted to one
  centered block PER image: the strip stacked vertically and entries moved
  while their textures resolved. A run of 2+ consecutive block-sized images
  now renders as ONE centered, wrapping gallery row in source order.
- A strip of consecutive inline SVG icons used each asset's intrinsic size
  (a `1em` icon stayed at 16px next to 256px logos clamped to 32px): the
  strip now shares the tallest decoded icon height (aspect preserved), like
  the source site's CSS, while a lone icon/badge keeps its size.
- Reloading the same URL could render and copy the PREVIOUS code block from
  the thread-local cache (keys are URL-based and the document can change
  server-side): the cache is now dropped when a fetch resolves and has a
  byte budget; at capacity it evicts one entry instead of clearing
  everything (which re-copied every block on every frame for >64-block
  documents).
- A failed NEW navigation dropped the page the user came from from `back`
  (the pre-push snapshot was restored): the pushed entry now survives, while
  failed Back/Forward still restore the original stacks.
- Copy text dropped the content lines of block-level HTML while the viewer
  showed them, and expanded `:shortcode:` emoji inside fenced/indented code
  blocks (`Select all` disagreed with the code card's Copy); both now keep
  the literal source.
- Frontmatter closing delimiters were detected on `trim()`ed lines, so an
  indented `---` inside a YAML block scalar truncated the metadata early;
  only a column-0 closer counts now.
- A frontmatter `title` written as a map/sequence failed the whole route
  although the docs promised it coerced to `None` (the visitors were dead);
  it is now drained correctly.
- Nested list content used a constant indent, so a wide parent marker
  (`10. `, `[x] `) left nested lists hanging to the LEFT of the parent text
  column; the measured marker offset is now honored.
- `Ctrl+W` in the same frame as `Ctrl+T`/`Ctrl+1..9` closed the newly
  selected/created tab instead of the one active when the key was read;
  `Ctrl+T` also left the outgoing page's context menu latched, and the
  expanded tab rail did not actually share the collapsed rail's scroll
  offset (different `ScrollArea` parents) despite the docs claiming it did.
- `Address` typing/pasting was unbounded until submit
  (`MAX_URL_INPUT_CHARS` is now enforced by the field itself); block-level
  HTML copy agrees with the rendered inert source.
- Hostile markdown (`> > > …`, `- - - …`, `***…`) could overflow the UI
  thread's stack (an uncatchable abort) on a few KB of input: the renderer
  now has a depth guard (`MAX_RENDER_DEPTH`) and renders deeper content
  flattened instead of recursing.
- The table-delimiter normalization padded pipe lines INSIDE fenced or
  indented code, corrupting the rendered code and the copied text; code
  regions are now left byte-for-byte (a table right after a closed fence is
  still padded).
- AVIF images skipped the `MAX_TEXTURE_DIM` downscale and could upload up
  to 24 MP textures as-is; every decoder now goes through the same
  `finish` downscale (the real decoded frame is re-checked first).
- Jobs registered during a frame (image `ensure`, favicon probes) did not
  schedule a repaint tick: their spinners could hang on an idle window
  until the next input event. `App::ui` now schedules the 120ms tick.
- Cancelling a fetch while waiting for a body chunk could linger up to the
  10s stall timeout; the chunk wait now races the cancel flag (~20ms).
- Document/table/code/error scroll state was shared between tabs showing
  the same URL (egui ids are global); ids now mix the tab id.
- Comments/docs no longer quote a fixed fallback budget: the visual
  timeout derives from `FETCH_SLOT_WAIT_SECS` + `MAX_ROUTE_CANDIDATES` x
  `REQUEST_TIMEOUT_SECS` + slack (`TOTAL_FETCH_TIMEOUT_SECS`).
- Copy (`plain_text`) now expands `:shortcode:` emoji exactly like the
  renderer does for text nodes; code spans keep the literal source, so the
  clipboard matches what is on screen.
- An image/favicon job that resolved between the readiness and deadline
  snapshots was declared failed and its decoded pixels were dropped: ready
  jobs are now uploaded first, the deadline re-checks readiness, and the
  race re-insertion keeps the ORIGINAL `since` (a flapping promise could
  otherwise postpone the timeout forever).
- An AVIF frame whose real dimensions disagreed with the container metadata
  was copied into a `Vec::with_capacity` sized from the frame BEFORE the
  caps were re-checked: a huge frame could abort the process on OOM. The
  real frame is validated (dims + decoder budget) before any allocation.
- Favicon probes ran three sequential requests on the image timeout (45s
  each), pinning one of four image slots for up to 135s, past the 65s entry
  deadline and starving document images on a dead host. Probes now use
  their own 10s budget (`FAVICON_REQUEST_TIMEOUT_SECS`).
- A visual timeout dropped the page that was on screen and replaced it with
  an error log; when a backdrop exists it now comes back (history snapshots
  and decoded textures intact) and the timeout is reported as a toast. A
  first load without a backdrop still settles into `Failed`.
- Nested `.md`/`.markdown` dotfiles (`/a/.md`) collapsed to the directory
  route: the extension is stripped only when a real file name precedes it.
- The SVG sniffer rejected documents with comments or a DOCTYPE before the
  root (common in exported files); well-formed prolog pieces are skipped.
- The SVG pre-scan (DTD/filter detection) compared case-insensitively at
  every byte offset over up to 10 MiB; it now scans for the first byte first.
- A local address error (`mailto:`, bad URL, oversized input) stored the raw
  unbounded input in `address`/`asked`; both are bounded like every other
  branch now.
- The History window re-lowercased every URL and title of up to 5000
  entries on each frame; the filtered index list is cached on a store
  generation counter now.
- Two threads racing the first request could build two HTTP clients / two
  network runtimes and drop one unused; first-use construction is
  serialized now.
- A flattened hostile subtree joins blocks with `\n` into one fragment, but
  the bidi reorder only rewrote the FIRST paragraph: everything after the
  newline silently disappeared on screen. Every paragraph is reordered now.
- A locally rejected URL (bad input, oversized paste) echoed pasted
  `user:pass@` credentials into the omnibox, the error view and "Copy log";
  both stored strings and the parser's error text are credential-sanitized.
- A failed background history write dropped the store's dirty flag at
  enqueue time, so the visit was never rewritten and a later success could
  not clear the Preferences warning; the store stays dirty until a write
  lands and the warning clears on success.
- A `history.key` copied in with wider permissions was silently trusted; it
  is tightened to 0600 on load (fail closed when it cannot be).
- History recorded over-long URLs truncated with `...`, so clicking the row
  opened a different (usually 404) page; over-long URLs are skipped whole.
- A standalone linked image was not clickable while loading or after a
  permanent failure (the placeholder lost the link); every placeholder
  state is wired like the decoded image now.
- The `[image: alt]` placeholder always rendered italic, even without
  source emphasis; it keeps the enclosing emphasis like the real image.
- "Select all"/Copy pasted the leaked first-block skip link that the viewer
  drops; clipboard extraction drops exactly the same block (the phrase list
  is shared, so viewer and clipboard can never disagree).

### Security

- Namespace-prefixed SVG filters (`<s:filter>`, `<s:feTurbulence>`) bypassed
  the byte pre-scan and could burn a decode slot for years on a ~300-byte
  document (`numOctaves` has no resvg-side bound): filters are now rejected
  by local name in a structural XML pass.
- Nested `<use>` references expand exponentially inside usvg from a tiny
  source (a 939-byte document parsed for 5.5 s and hundreds of MB before
  usvg's own node limit): the memoized expansion count is capped at
  `MAX_SVG_TOKENS` before parsing.
- `http:/user:pass@host` (single slash) no longer folds pasted credentials
  into a request path to a single-label host.
- Image failure reasons (tooltips) are credential-sanitized like the
  document error view.
- SVG decoding is hardened against parser and CPU bombs: `<!DOCTYPE>` /
  `<!ENTITY>` (roxmltree expands internal entities without a total-size
  bound) and `filter`/`fe*` markup (`numOctaves`, morphology and convolution
  parameters have no resvg-side limits) are rejected before parsing; inline
  `data:` hrefs are no longer resolved (a nested SVG bypassed the markup cap
  and could recurse); a UTF-8 character split at the 1 KiB sniff window no
  longer misclassifies valid SVGs.
- AVIF alpha items are dimension-checked before decoding: `avif-decode`
  validated the alpha size only after rav1d allocated the frame, so a tiny
  color item plus a huge alpha item could exhaust memory.
- Raster decoders check the header dimensions before `decode()` (image's
  `Limits` has no pixel-count limit, so an 8192x8192 67 MP image was fully
  decoded before being rejected) and JXL checks its output byte size before
  `DynamicImage::from_decoder` (which aborts on OOM instead of failing).
- The Content-Type gate now runs BEFORE the body is streamed for documents
  and images: a hostile page can no longer force up to 256 x 10 MiB of
  wrong-type downloads that were dropped only after buffering (and
  decompressing).
- YAML frontmatter aliases are never expanded: the root check reads the
  mapping directly and skips unknown fields with `IgnoredAny`, closing a
  32 KiB alias fan-out that materialized gigabytes through
  `serde_norway::Value`.
- Document complexity is bounded: `MAX_EVENTS_PER_DOC` (500k) rejects
  event-stream bombs and `MAX_RENDERED_CODE_LINES` (5000 per document) stops
  a few MB of newlines from rendering millions of labels per frame.
- `parse_https` recognizes `scheme://…` as a scheme before the userinfo
  heuristic: `http://user:pass@host` is now blocked instead of being sent
  to host `http` with the credentials in the request path, address bar and
  route log; non-hierarchical schemes (`data:`, `javascript:`, ...) are
  blocked, and single-label hosts with ports (`nas:5000`) work.
- Image/favicon retries now classify the real error strings (network
  errors, stalls, truncated transfers, 5xx/429 retry; 4xx, content-type
  rejections and caps do not), and every job has a wall-clock deadline so a
  decoder that cannot be interrupted stops counting as pending.
- Remote titles drop Unicode bidi/zero-width format controls (RLO spoofing)
  and `truncate_chars` now respects its documented total cap; fenced
  language chips and footnote labels are bounded before rendering (a 4 MB
  fence no longer allocates a lowercased copy per frame).
- Failed favicon probes no longer permanently exhaust the 128-origin cap
  (one failed entry is evicted instead).
- Favicons now decode at `MAX_FAVICON_DIM` (256 px; they draw at 20 px) and
  reserve their bytes in the process-wide texture budget: 128 hosts can no
  longer approach ~2 GiB of VRAM outside the documented bound.
- SVG sources are token-capped (`MAX_SVG_TOKENS`) before `usvg` parsing:
  the upstream parser has no node limit, so a 10 MiB markup bomb could pin
  a decode slot for minutes.
- Image retry docs now match the code: truncated transfers surfacing as
  decode errors are retried once; content-type rejections and
  cancellations are final.
- The SVG structural pass could overflow the decode thread's stack (a
  stack overflow aborts the process and cannot be caught) on deep nesting:
  the expansion walk is iterative now, bounded only by `MAX_SVG_TOKENS`.
- The structural pass mirrors usvg's namespace acceptance (`None |
  SVG_NS`): a namespace-less `<use>` expansion bomb used to pass as
  ordinary elements. It also follows a `<use>` whose target is itself a
  `<use>` and resolves `href` with usvg's precedence (unprefixed first),
  closing a forged foreign-namespace `href` decoy.
- SVG `marker` elements are rejected like filters: usvg re-converts a
  marker's subtree for EVERY path vertex (no cache), so a small marker plus
  a long path built an unbounded render tree no source token cap could see.
- Rendered block count is bounded (`MAX_RENDERED_BLOCKS`):
  `MAX_EVENTS_PER_DOC` bounds parsing memory, but a ~0.7 MB body of tiny
  paragraphs laid out ~500k labels every frame and froze the UI; past the
  budget the walk consumes blocks without rendering them and `Copy` still
  yields the full text.
- Switching to "Forget on exit" now flushes the queued history writer and
  wipes the ciphertext/key immediately (a blob already encrypted could land
  after the policy change, and a kill before exit left the store on disk).
- Small buttons no longer jump when pressed or keyboard-focused. The
  `active` widget state had lost its 1px `bg_stroke` width (`Stroke::NONE`),
  and a `small` button subtracts that width from vertical padding it has
  already zeroed, so the pressed (or keyboard-focused) rendered code `Copy`
  and error `Copy log` allocated 2px less than their idle state and pulled
  the rows below them up. Every widget state now keeps ONE stroke width (a
  transparent 1px active stroke paints nothing), so no themed control can
  resize between states; the `about:history` footer's delete button also
  keeps one fixed width while the two-click confirmation swaps its label
  ("Delete history now" / "Confirm delete").

## [0.2.1] - 2026-10-04

### Added

- Vertical tab rail: 52px collapsed rail with one favicon per
  tab that expands on hover into a 248px overlay (favicon + title + close +
  `+ New Tab`) without reflowing the page; active pill + accent bar, loading
  spinner per tab and tooltips.
- Site favicons: per-origin probe of `/favicon.ico` then `/favicon.png`
  (background, bounded session cache) with a home/globe fallback glyph.
- Markdown images: PNG, JPEG (`.jpg`/`.jpeg`), GIF, WebP, JPEG XL and AVIF,
  decoded 100% in Rust (`image`, `jxl-oxide`, `avif-decode`) on background
  threads through a per-document texture cache. Every image renders at its
  natural pixel size, never upscaled, and clamped to the reading-column
  width when wider; standalone paragraphs center as blocks (quotes and list
  items included) while inline occurrences stay in the text flow. Images
  inside links stay clickable, loading/failed/budget states show accessible
  placeholders, and alt text becomes a caption/tooltip.
- Image anti-abuse caps: 10 MiB per image, 24 MP and 8192 px per side,
  512 MiB decoder allocation, 256 MiB GPU texture budget, 256 images per
  document, 4 concurrent image jobs (separate from document fetches).
- Keyboard shortcuts: Ctrl/Cmd+T (new tab), Ctrl/Cmd+W (close),
  Ctrl/Cmd+R / F5 (refresh), Ctrl/Cmd+L (focus address),
  Ctrl/Cmd+A (select all page text), Ctrl/Cmd+1..9 (select tab);
  new tabs focus the address field.
- Minimal right-click menu: Copy (the drag selection when there is one,
  else the whole page text) and Select all, plus Open link in new tab at
  the top when the click landed on a link (link hitboxes are collected
  while the document renders).
- Transparent `gzip`/`brotli`/`deflate` decompression (reqwest features),
  cutting markdown transfer size several times; anti-abuse caps run on the
  decompressed stream.
- New modules: `media/{fetch,decode,cache,favicon}`, `ui/icons`,
  `ui/viewer/image`, `ui/viewer/overlays`; unit tests for format sniffing,
  PNG decode/downscale, bomb caps, table headers and a headless rail
  render.
- `MdowserApp` self-heal unit tests (empty vec, out-of-bounds index,
  last-tab close resets instead of emptying).
- `scripts/build-test.sh` `cargo doc` step with `RUSTDOCFLAGS="-D warnings"`
  (broken intra-doc links fail the pipeline).
- Global concurrent-fetch limiter (`config::MAX_CONCURRENT_FETCHES`,
  `FETCH_SLOT_WAIT_SECS`) with cancel-aware slot acquisition: rapid
  multi-tab navigation can no longer accumulate unbounded threads/buffers.
- `Fetched.events`: the document is parsed ONCE at fetch time into owned
  `pulldown-cmark` events (normalization + parsing off the UI thread);
  repaints (scroll, selection) never re-run the parser on up to 5 MiB.
- Bounded MULTI-entry token-span cache in `ui::viewer::code` keyed by
  (block hash, length, language): a document with several code blocks no
  longer re-highlights every block on every frame.
- `markdown::document::parse_events()` (owned `'static` events) with unit
  tests, plus a headless renderer smoke test covering loose lists, nested
  quotes, rules inside quotes, tables, footnotes, code blocks and more.
- `Tab::address_dirty` + `mark_address_edited()`: real edit tracking from
  the navbar/home fields (type-and-revert no longer clobbered on resolve).
- `THIRD_PARTY_NOTICES.md`: license summary of the dependency graph plus
  the full OFL-1.1, Ubuntu-font-1.0, Unicode-3.0 and BSL-1.0 texts that
  must accompany the binary. Shipped in every dist archive (and inside
  the macOS `.app` `Contents/Resources/`).
- Accessible names for the painted widgets that screen readers cannot
  infer: every toolbar icon button now reports its tooltip as the label
  plus enabled state, and tab rows report title + selected state
  (accesskit is part of the default eframe stack).
- Ignored network smoke test (`cargo test -- --ignored network_images`)
  that exercises the real fetch → Content-Type gate → decode pipeline for
  PNG, JPEG, WebP, ICO and AVIF, plus the deliberate SVG rejection. It is
  opt-in so the normal suite never touches the network.

### Changed

- Version bumped to `0.2.1` (`Cargo.toml` + `Cargo.lock`; `USER_AGENT`
  follows via `CARGO_PKG_VERSION`).
- `protocol::client` now exposes a shared `get_bytes` (cancellable
  connect/status/content-type/streaming caps) used by both document
  candidates and image/favicon subresources; `global_client` is
  crate-visible.
- Tab labels dropped the `Tab N:` prefix (favicons identify tabs now) and
  are capped at 30 chars; the OS window title keeps the prefix.
- Minimum Rust version is 1.98 (pure-Rust AVIF decoder); `deny.toml`
  documents the MPL-2.0 file-level copyleft of `avif-parse` while GPL/AGPL
  stay denied.
- README/ARCHITECTURE/SECURITY/CHANGELOG document the image pipeline, the
  rail, shortcuts and the new caps.
- `README.md` restructured as a product overview; the historical bug-by-bug
  audit list was removed (the change record lives in this file).
- `deny.toml` license allowlist covers the permissive + Unicode + font set
  used by the GUI/TLS stack (`CC0-1.0`, `MIT-0`, `Zlib`, `Unicode-3.0`,
  `BSL-1.0`, OFL-1.1, Ubuntu-font-1.0, LLVM exception); copyleft stays
  denied.
- CI `audit` job installs `cargo-audit`/`cargo-deny` only when missing
  (no more `|| true` masking install failures).
- `scripts/build-dist.sh` fails loudly when `Cargo.toml` version
  cannot be parsed (no empty-tag artifacts).
- Address-bar `Enter` submits whether the field kept or lost focus
  (navbar + home); previously focus-kept submits could be ignored.
- Pipeline docs synced (`Makefile`, `README.md`, `CONTRIBUTING.md`,
  `scripts/build-test.sh` header): `make test` is now
  sh syntax + fmt + clippy + test + doc + debug.
- Repaint cadence while loading: immediate repaint when a load resolves;
  300 ms with a backdrop on screen, 100 ms for a first load (nothing to
  re-render yet).
- `ui::viewer::markdown::show_markdown` now renders recursively from the
  pre-parsed events (correct nesting), and the top fade overlay is drawn
  only at rest (scroll idle) instead of every throttled repaint.
- The viewer renders one blockquote indent + bar PER depth level; rules and
  H1/H2 separators inside a quote/list item stay indented.
- `serde_yaml` (archived upstream) replaced by the maintained `serde_norway`
  fork (same API; `serde_yml` is unsound, RUSTSEC-2025-0068).
- `frontmatter::split` never errors on a closerless leading `---` and never
  swallows a delimited block without a YAML mapping: both stay CommonMark
  content. The parser no longer enables YAML metadata blocks, so `split` is
  the single source of truth for frontmatter.
- Tab cap (`config::MAX_TABS`): new tabs beyond the cap show a toast instead
  of growing document memory without bound.
- Navbar degrades below 720 px: `+ New` shrinks to 64 px, `Go` to 44 px
  and the `%` readout is hidden (zoom-in tooltip reports the factor), so
  the URL field and zoom controls never clip off-screen.
- Decoder allocation cap (`512 MiB`) moved from a literal in
  `media::decode` to `config::MAX_DECODER_ALLOC_BYTES`, like every other
  anti-abuse bound (docs reference the constant).
- CI `audit` job installs `cargo-audit`/`cargo-deny` from prebuilt
  binaries (an install action), cutting minutes per run; the
  workflow also runs weekly (fresh RustSec DB) and on manual dispatch.
- Release CI now publishes `SHA256SUMS.txt` next to every artifact, the
  same guarantee `make dist` gives locally.
- `scripts/build-dist.sh` checksums accept `shasum` (macOS/Perl) or
  `sha256sum` (GNU coreutils) and fail loudly when neither exists.
- The `.deb` now ships `/usr/share/doc/mdowser/copyright` and declares
  `Depends: ca-certificates` (the system trust store TLS verification
  actually needs).
- Image size/position policy: markdown carries no width/height, so every
  image renders at its NATURAL pixel size (never upscaled) and is scaled
  down to the reading-column width when wider. Position follows the
  markdown: standalone paragraphs center as blocks in every context
  (blockquotes included, previously inline at 24px), inline occurrences
  stay in the text flow at natural size (previously forced to 24px text
  height). `fit_inline` is replaced by the tested `fit_to_width`.

### Fixed

- Native (drag) text selection was INVISIBLE: egui paints the selected
  glyphs with `selection.stroke.color`, and the theme set `Stroke::NONE`
  (transparent). The stroke is now width-0 with the readable `on_surface`
  color, so selected text stays legible in both themes (WCAG-contrast
  tested); the same pair drives the "Select all" highlight.
- The right-click menu's "Copy selected text" always copied the WHOLE page
  (it never read the native selection). `Copy` now copies the real
  selection through egui itself, and Select all + Copy / Ctrl/Cmd+C copies
  the full plain text. `Ctrl/Cmd+A` selects the page; a drag selection
  replaces the page-wide highlight on press.
- The "Select all" highlight skipped footnote markers, HTML blocks and
  callout badges: they now highlight like the rest of the document.
- The context-menu anchor was clamped with global `viewport().inner_rect`
  coordinates against local pointer positions and a hardcoded 240px menu
  size; the popup now relies on the Area's own window constraint, so it
  never spawns off-screen at any size.
- Table headers were silently dropped: pulldown-cmark closes
  `TableHead` without a `TableRow`, so the accumulated header cells were
  cleared by the first body row. Headers now commit on `End(TableHead)`
  (regression test `renders_table_headers`).
- The expanded tab overlay mixed global screen coordinates
  (`viewport().inner_rect`) with local layout rects, so it grew past the
  window: the pinned `+ New Tab` fell off-screen and the rail/list could
  disagree. It now uses the local `viewport_rect()`.
- The collapsed rail kept drawing under the expanded overlay, so old rows
  and the old `+` showed through and overlapped the expanded list. Rail
  content is skipped while the overlay covers it.
- Tab rows and the `+ New Tab` moved when expanding: both states now share
  paddings, row height/gap, icon column and action height, and the scroll
  offset is shared, so expanding only reveals title/close.
- The window clear color was eframe's default dark gray, visible through the
  page frame's rounded corners and outer margins as an ugly black frame:
  `clear_color` now returns the theme `panel_fill`.
- The floating scrollbar used `fg_stroke` (near-black) for its handle and
  drew a hard black line along the reading pane: handles now use the gray
  container tone with tuned opacities.
- The content frame had a hard 1px stroke plus a black shadow in dark mode:
  borderless sheet with a soft shadow in light mode and none in dark.
- Toolbar/home address fields showed no focus cue (`selection.stroke` was
  width-0 to avoid stray lines): a primary focus ring is painted explicitly.
- Flat buttons no longer draw the default outline ring, and dark-mode
  popup/window/overlay shadows were removed (dirty black halos).
- AVIF images now re-check the REAL decoded frame against the dimension /
  pixel caps after `to_image()`, not just the container `ispe` metadata:
  a container whose header lies about its size cannot slip a bomb past the
  pre-decode check (defense in depth).
- The image paragraph path no longer carries a production `expect`: the
  renderer can never panic the UI thread if `image_only`'s invariant ever
  regresses (falls through to normal paragraph rendering instead).
- `CONTRIBUTING.md` stated Rust ≥ 1.85 while `Cargo.toml` requires 1.98;
  the MSRV note now matches the manifest (AVIF decoder floor).
- `deny.toml` was broken on current cargo-deny (`unlicensed` was removed
  upstream, and the bundled fonts / Windows crates were rejected:
  OFL-1.1, Ubuntu-font-1.0, BSL-1.0 were missing). `cargo deny check` now
  passes with only the documented accepted advisories ignored; every
  non-MIT/Apache component is listed in `THIRD_PARTY_NOTICES.md`.
- `scripts/build-dist.sh` aborted the whole multi-OS run when one macOS
  arch failed (`set -e` + unguarded `build_for`), contradicting the
  documented best-effort behavior; macOS now skips the group with a reason
  like Linux/Windows. The missing `nasm` requirement of `rav1d`'s x86_64
  kernels is now detected and auto-installed (it broke Intel macOS,
  Linux x86_64 and Windows builds with a raw cargo error).
- Images served as `binary/octet-stream` (the object-store default) or labeled with
  the legacy `image/x-png` / `image/pjpeg` aliases were rejected by the
  Content-Type gate even though the decoder handles the real bytes.
  `is_octet_stream` now covers both octet-stream spellings and the image
  type list includes the legacy aliases (magic sniff still validates).
- Images shared the 10s document request timeout, aborting 5-10 MiB
  images mid-transfer on slow links. `get_bytes` now takes a per-request
  timeout: documents keep 10s, images get
  `config::IMAGE_REQUEST_TIMEOUT_SECS` (45s, ~1.9 Mbps for the 10 MiB
  cap). The 10s per-chunk stall cutoff still aborts dead connections.
- An image inside a heading (`# ![Logo](logo.png) Title`) rendered as
  alt text and was never registered for fetching: heading fragments now
  render image frags through the shared image widget (regression test
  `heading_image_renders_as_image_not_alt_text`).
- A panic inside an image decoder (`image`, `jxl-oxide`, `avif-decode`)
  or the YAML parser dropped the promise sender without a value; polling
  such a promise PANICS the UI (documented `poll-promise` behavior).
  `util::guard_panics` now wraps every background fetch entry point
  (`fetch_blocking`, `fetch_image_blocking`, `fetch_favicon_blocking`),
  turning the panic into a bounded error placeholder.
- `SECURITY.md` supported versions corrected to `0.2.x` (was stale `0.1.x`).
- `MdowserApp::active_tab_mut` is now panic-free: an empty tab vec
  recreates the initial tab and an out-of-bounds index clamps (was a
  residual `expect` that could panic the UI thread).
- Route log lines are now all bounded: success `Content-Type` echo and
  `BadFrontmatter` details are truncated like every other
  server-influenced line.
- Code-block line counting scans the buffer once per frame (was twice).
- Cancelled navigations abort during connect/headers (`futures_util::select`
  race against the cancel flag) instead of holding the thread up to 10 s
  before the per-chunk checks ran.
- Tab rows: hover no longer flickers when the cursor sits over the title /
  close button (hover is the union of row + children); the row background
  is filled through a reserved `Shape::Noop` slot so it stays behind the
  text; the row scrollbar is hidden so the active-tab continuity is never
  cut by a 1 px strip.
- Right-click menu clamps to the screen (never spawns off-window), closes
  on scroll/zoom, and clicks on the content scrollbar no longer open it.
- Error view: status dot allocates its rect (stays aligned at any zoom);
  oversized-body logs classify as `Other` instead of a misleading
  "No connection".
- Headings cap at 40 pt so 3x zoom cannot force horizontal scrolling;
  quote-level bar derives its gutter from `spacing.indent` instead of the
  hardcoded `-18`, so zoom no longer drifts it over the text.
- Ordered lists use the parsed `is_ordered` flag instead of inferring it
  from `marker.contains('.')`.
- Pipe-menu heuristic rejects all-lowercase prose (`pan | vino | queso`)
  while keeping mixed-case menus (`Home | docs`).
- Table max column width capped by an equal share of the card: narrow
  windows no longer overflow just because one column is wide.
- LOOSE LISTS no longer lose their bullets: paragraphs inside list items
  render with the item marker/checkbox (same row) and continuation
  paragraphs are indented; no more stray empty bullets, and loose task
  lists keep checkbox + text together.
- Leading `---` documents no longer fail or lose content: a closerless
  `---` (thematic break) is content, and a delimited block without a YAML
  mapping renders instead of being swallowed by the metadata skipper.
- Back/Forward during a pending load no longer corrupt history: the
  in-flight URL becomes the current position before the stacks move (was:
  stale page pushed again -> self-duplicates). Pressing Back/Forward onto
  the URL already loading is a no-op instead of mutating the stacks.
- Visual timeout derived from the real worst case (slot wait + 7
  candidates + slack) so a legitimate load is never abandoned at 75 s.
- The right-click menu does not survive a tab switch anymore and closes
  when the window moves (screen-coordinate anchor no longer goes stale).
- Code blocks: the trailing newline no longer renders as a phantom last
  line, and blank lines keep their line number.
- List continuation lines align to the measured marker width (ordered
  `10. ` and checkboxes included), not a fixed 14 px.
- Ordered-list numbering of loose items: checkbox/text/marker are
  consistent (the marker is drawn exactly once).
- `parse_https` rejects bare emails (`user@domain.tld`) instead of
  navigating to the domain; `NotHttps` errors no longer echo credentials.
- YAML `title` accepts i128/u128 integers (huge IDs no longer fail the
  whole frontmatter).
- Nested lists render structurally indented (one step per level) and
  continuation lines align with their own first line (was: all nested
  markers at the item edge while continuations were pushed a level
  deeper).
- An item marker is drawn at the item's own indentation when the first
  block is a blockquote or a nested list (was: the bullet appeared after
  the nested list, or inside the quote next to the quoted text).
- A delimited YAML block whose root is not a mapping (sequence, scalar
  containing `:`, comment-only, `null`) renders as CommonMark content
  instead of failing the route.
- Home and fresh-error pages no longer repaint every 500 ms forever: the
  cooldown ticks are scheduled only while a real countdown is pending.
- Disabled primary/tonal buttons (`Go`, `Retry`) keep a readable
  full-opacity label (was: enabled colors faded by egui's 50% disabled
  alpha, nearly invisible in both themes).
- Refresh and server-home after a failed load act on the failed `asked`
  (and on the pending URL while loading), not on the previous page that
  `current` still pointed at.
- Window title URL fallback is truncated like every other title path
  (was: up to 4 KiB straight to the window manager).
- History self-duplicates are healed on load, and navigation commits the
  pending URL before the reload check, so Back/Forward never look like
  no-op reloads after an interrupted navigation.
- `Content-Type` parameters with quoted values containing `;`
  (`boundary="a;b"`) are accepted instead of rejected.
- Retry is disabled with a fix-the-address hint on local rejections
  (`BadUrl`/`NotHttps`), where retrying can never succeed.
- The H1/H2 rule inside a list item aligns with the heading text, not the
  item edge.
- Error-view hint about the URL cap derives from
  `config::MAX_URL_INPUT_CHARS` (no hardcoded 4096 drift).
- Copy/share toasts are English like the rest of the UI (were Spanish).

### Security

- The image texture budget is now enforced twice: per document
  (`config::MAX_TEXTURE_BYTES`, 256 MiB) and process-wide
  (`config::MAX_TEXTURE_BYTES_TOTAL`, 512 MiB). Before, `MAX_TABS`
  image-heavy documents could each keep up to 256 MiB of textures alive,
  adding up to unbounded VRAM; reservations are released on navigation,
  tab close and cache drop. Unit test covers reserve/release/saturation.
- AVIF post-decode dimension re-check (see Fixed) closes the
  container-vs-bitstream mismatch window left by the pre-decode `ispe`
  check.

## [0.2.0] - 2026-10-02

### Added
- `config::USER_AGENT` documented as the defined browser User-Agent + unit test.
- `ui::error_view` classification (`Network` / `NotFound` / `NotMarkdown` /
  `BadAddress` / `Other`) with unit tests.
- `Tab::title` / `window_title` always prefixed (`Tab N:`) with `Untitled` fallback.
- `ARCHITECTURE.md` (module map + contracts) and `CONTRIBUTING.md` (quick start, standards).
- `deny.toml` (cargo-deny policy) + automated weekly cargo/actions
  dependency updates + CI `audit` job (`cargo audit`, `cargo deny check`).
- `config::CHUNK_TIMEOUT_SECS` (10s per-chunk stall timeout),
  `MAX_ERROR_LINE_CHARS`, `MAX_ADDRESS_DISPLAY_CHARS`,
  `MAX_ASKED_DISPLAY_CHARS`, `MAX_TITLE_CHARS`.
- `SECURITY.md` supported-versions table, TLS/rustls notes, SSRF note,
  credential-strip note, supply-chain notes.
- Unit tests for title bound, highlighter multibyte strings/escapes.
- `rustfmt.toml` and `.editorconfig` (stable rustfmt options only).
- `LICENSE-MIT` / `LICENSE-APACHE` texts for the declared
  `MIT OR Apache-2.0` license.
- `src/util.rs`: shared display helpers (`strip_scheme_prefix`,
  `strip_https_everywhere`, `truncate_chars`, `sanitize_title`) with unit tests.
- Unit tests for `ui::viewer::inline` hard-break splitting, resolver
  query/fragment/port/IPv6, rewrite scheme guards, frontmatter CRLF/BOM
  and size cap, theme layout bounds, and URL input caps.
- `SECURITY.md` (scope, caps, reporting) and `.gitattributes` (LF, diff).
- `Copy log` button on the error view.
- `Tab::can_retry()` (Retry honors the 5s cooldown with countdown).
- `BrowserError::Cancelled` typed variant with `is_cancelled()` helper.
- `make audit` (supply-chain audit: `cargo audit` + `cargo deny check`,
  best-effort locally, enforced in CI); `scripts/build-test.sh` starts
  with a POSIX `sh -n` syntax check of `scripts/*.sh`.

### Changed
- `+ New` is now FIRST in the top bar with the same 188px width as tab
  rows (fusion with the tab strip); URL field reserve updated.
- Re-navigation to the same logical URL reloads instead of returning early
  (no result/error cache); same-document reload leaves history alone like
  refresh.
- Version bumped to `0.2.0` (`Cargo.toml` + `Cargo.lock`; `USER_AGENT`
  follows via `CARGO_PKG_VERSION`).
- `USER_AGENT` now derives from `CARGO_PKG_VERSION` (no version drift).
- `config` centralizes all display/error bounds (`MAX_ERROR_LINE_CHARS`,
  `MAX_ADDRESS/ASKED_DISPLAY_CHARS`, `MAX_TITLE_CHARS`); `sanitize_title`
  caps internally (defense in depth).
- `Cargo.toml`: `tokio/time` for stall timeouts; `missing_docs` +
  `broken_intra_doc_links` warnings.
- `scripts/build-dist.sh` `zip_contents` python fallback uses `argv`
  (filenames with spaces survive); `zip`/`ditto` missing handled uniformly.
- Table/code widget ids salted by document URL hash (no cross-page leaks).
- `theme::shade` rounds (no truncation bias); `content_max_width` simplified.
- Removed dead `Fetched::loaded_url` + its `allow(dead_code)`; canonical
  URL is `display_url` only.
- Standardized all code comments and user-visible strings to English.
- Split `ui::viewer` (1700+ lines) into focused modules: `mod` (entry),
  `markdown` (blocks), `inline` (fragments), `code` (code blocks),
  `table` (grids). No behavior change.
- Removed dead code: dropped unused `Fetched` fields (`raw_markdown`,
  `attempts`), the `Attempt` struct, unused frontmatter fields (only
  `title` is surfaced; unknown fields are still ignored), the dead
  `has_link` helper, the unreachable table-width fallback, and all
  `allow(dead_code)` attributes.
- Unified button helpers on `&str` (`tonal_button_sized` took `String`).
- `Cargo.toml`: completed package metadata (`publish = false`), documented
  lint policy (`unsafe_code = forbid`; `clippy::all` via
  `cargo clippy -- -D warnings`), and set `strip = "none"` so only
  `scripts/build-dist.sh` strips binaries (matches the documented
  `try_strip` flow).
- CI now tests all three OSes (Linux, macOS, Windows) with a dependency
  cache; the Intel macOS runner moved to the current runner image;
  release flow unchanged.
- CI hardening: top-level `permissions: contents: read` (release job
  re-escalates to `write`), per-job `timeout-minutes`, and `concurrency`
  cancel-in-progress on the same ref.
- `deny.toml` `[bans] deny` pins the rustls-only contract (`openssl`,
  `openssl-sys`, `native-tls` rejected).
- Document scroll id uses the hashed `doc_id_salt` like tables/code
  (was the raw URL string, up to 4 KiB per tab).
- `scripts/build-dist.sh`: version parse tolerates indentation, checksums
  are space-safe (no `xargs` splitting).
- Link resolution uses `display_url` (directory semantics: `other` from
  `/love/` goes to `/love/other`); unresolvable destinations are inert.
- Loading/error strips use bounded, truncated log lines.
- README: fixed stale `viewer.rs` path, `+ New` location, refresh-disabled
  wording, clippy invocation, and a duplicated compiler bullet.

### Fixed
- Stalled trickle responses could pin a background thread to the 75s visual
  budget: per-chunk 10s stall timeout aborts the attempt (next route tried).
- Highlighter stepped mid-char on multibyte text/strings/escapes: now advances
  by whole UTF-8 chars + `debug_assert` boundaries + tests.
- Viewer rendered raw `<br/>`/`<kbd>` as code pills, diverging from clipboard
  `plain_text` (which strips): `<br>` is now a break, other tags a space.
- `zip_contents` python fallback split on spaces (broke `My File.exe` names).
- Missing-content-type log now reports `(missing)` instead of `(ausente)`.
- `.deb` / desktop-file descriptions translated to English.
- `image_only` false positive: plain text starting with `[image` no longer
  renders as an image box (requires italic placeholder shape).
- `rewrite::absolutize` join failure returned garbage for navigation;
  now inert (`""`).
- `code` viewer sliced highlighter ranges directly (`&line[s..e]`);
  now uses `get()` fallback (no UI-thread panic on bad ranges).
- `content_type` mixed-quote `charset="'utf-8'"` unwrapping.
- `highlight`: byte-wise `as char` casts replaced by `is_ascii_*`; C-family
  fences (`c`, `cpp`, `c#`) no longer mis-highlight as JS/Rust (now `Plain`).
- Superseded-load detection matched on `"cancelled"` substring; now typed
  `BrowserError::Cancelled` end-to-end (fetch, tab poll, timeout).
- `Retry` bypassed the 5s cooldown and allowed hammering; now gated by
  `can_retry()` with countdown label.
- Window/tab titles from remote frontmatter could inject newlines/control
  chars; now sanitized (`sanitize_title`) and truncated.
- `App` indexed `tabs[active]` directly (panic on broken invariant);
  now clamps + safe getter with `debug_assert`.
- Unbounded `back`/`forward` stacks capped at 100 entries (oldest dropped).
- Unbounded URL/address input capped at 4096 chars (`BadUrl` locally).
- Unbounded YAML frontmatter capped at 32 KiB (tries next route).
- Unbounded `Content-Type` log values truncated to 500 chars.
- Minimum window size (640x480) set so chrome never collapses.
- `is_html_break` missed `</br>` (despite the comment) and treated
  `<bracket>` as a break; now delimiter-checked (`<br>`, `<br/>`,
  `<br />`, `</br>`, attributes tolerated) in a single shared helper
  (`markdown::document::is_html_break`) used by the viewer and the
  clipboard `plain_text` alike.
- Frontmatter scan buffered the whole body as YAML before the 32 KiB
  size check; now aborts early once clearly over the cap (same error).
- A final URL that did not land on `https` is rejected before the body
  is read (defense in depth over `https_only(true)`).
- Transport error text (which can embed server redirect URLs such as
  `https://user:pass@host/`) is credential-sanitized before reaching
  the line-by-line error view.
- Error view now shows clear styled messages per family (`No connection` /
  `Page not found` / `No markdown here` / `Invalid address`) with hints + details card.
- Tab titles always prefixed (`Tab N: Home/.../Error`, `Untitled` fallback); window titles match.
- Refresh/retry/re-navigation never reuses cached results or errors (always re-downloads).

## [0.1.0] - 2026-09-30

### Added
- HTTPS-only browser for `text/markdown` (CommonMark + tables,
  footnotes, task lists, strikethrough).
- Route fallback cascade (`/love`, `/love/`, `/love.md`, `/love/index.md`,
  README variants) with line-by-line error view.
- Tabbed UI (eframe/egui): home, navbar, viewer, theme, error view.
- Release packaging: macOS universal app, Linux tarball + deb,
  Windows zip, plus native CI releases.
