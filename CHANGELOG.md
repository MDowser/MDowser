# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Offline reading: when no fresh copy of a page can be obtained (unreachable
  network, every route rejected, a failed revalidation after the cascade),
  the encrypted cache's stored copy is served instead of the error view,
  marked in the address bar and the page-info card as an "Offline copy"
  (nothing answered) or a "Saved copy" (the server answered without a
  document). Refresh falls back the same way and its result is marked too;
  cached images and favicons fall back to their stored bytes as well.
  (`src/protocol/client/`, `src/media/fetch.rs`, `src/browser/tab/`,
  `src/ui/omnibox/`, `src/ui/icons/`, `docs/`.)
- Reading aids. A **document outline panel** (`Ctrl/Cmd+Shift+O`, or the
  right-click menu's "Document outline" row; disabled when the document has no
  headings) docks to the reading sheet's right edge — a bottom sheet on narrow
  windows — never reflows the document, highlights the current section while
  scrolling, indents levels from the document's shallowest one, elides long
  titles (the full text stays in the tooltip) and scrolls its own list. The
  right-click menu's arrow keys skip the row when it is disabled. And
  **Export…** (`Ctrl/Cmd+S`, or the menu row) opens a floating card that
  writes the page on screen as Markdown source or plain text to an editable
  path (defaulted to `~/Downloads/<page title>`, `~` expansion, two-click
  confirmation before replacing an existing file, inline error on failure),
  with no native dialog and no new dependency. (`src/markdown/outline.rs`,
  `src/markdown/anchors.rs`, `src/viewer/outline.rs`,
  `src/viewer/overlays/outline.rs`, `src/ui/export.rs`, `src/app/`,
  `docs/`.)

## [0.6.0] - 2026-10-19

An encrypted HTTP disk cache with revalidation and hardening, `--help` and
`--version` command-line flags, and a dependency and supply-chain refresh.

### Added

- Command-line flags: `--help`/`-h` prints the synopsis and `--version`/`-V`
  the version to stdout and exits before a window opens. (`src/main.rs`,
  `README.md`, `docs/NAVIGATION.md`, `docs/BUILDING.md`,
  `docs/ARCHITECTURE.md`, `packaging/linux/mdowser.1`.)
- Encrypted HTTP disk cache (`src/cache/`, same `store.key` as
  history/bookmarks/settings): documents (one AEAD file per host, keyed by
  canonical page identity, so every route spelling shares a record), images (one
  AEAD file per URL, sharded) and favicons (one file for all probes) honor
  `Cache-Control`/`Expires` (+`Date`/`Age`) under a 1-year ceiling.
  `no-store`/`Vary: *` never persist and evict older records for the URL;
  `ETag`/`Last-Modified` revalidation refreshes on 304, replaces on 200; stale
  is never served on failed revalidation; `Refresh` forces it. Bounded at 512
  MiB / 50 000 entries, silent FIFO eviction (no error), per-class quotas; each
  class clears separately in Preferences/Data; "Forget on exit" disables it.
  (`src/cache/`, `src/config/mod.rs`, `src/protocol/client/`,
  `src/media/fetch.rs`, `src/app/`, `src/ui/preferences/`, `docs/`.)

### Changed

- README, the security/architecture docs and the contributing guide describe the
  encrypted cache accurately and state the persistence rule for new network
  resources.
- `CacheManager` refresh paths keep global byte accounting exact when
  revalidation metadata changes a record's size.
- Preferences/Data cache rows share the fixed-geometry two-click pattern of the
  other destructive actions.
- Cache freshness counts apparent age (`now - Date`) alongside `Age` for the
  `max-age` lifetime (RFC 9111 §4.2.3): a response generated long ago is not
  fresh for its full lifetime from arrival; a future `Date` adds no age.
- Docs synced with the code: `RENDERING.md` (single-label `host:port` sugar
  applies to link destinations, prose needs a dotted domain), `INTERFACE.md`
  (favicon probes persist in the encrypted cache), `NAVIGATION.md` (Refresh is a
  cache-bypassing re-fetch with `Cache-Control: no-cache` that never sends
  stored validators), `SECURITY.md` (`Pragma: no-cache` captured;
  `must-revalidate` honored trivially; `immutable` has no distinct effect) and
  `ARCHITECTURE.md` (`error`, `util`, `ui/row`).
- `windows.ps1` quotes the script path when it relaunches itself elevated, so a
  checkout under a directory with spaces (`C:\Users\me\My Projects\…`)
  elevates correctly.
- `make install` and `make help` no longer require a resolvable cargo (the RPM
  `%install` stage of an environment without the toolchain works); every other
  goal still fails fast with the install hint.
- `docs/BUILDING.md` and `gen_fonts.rs` state that the font-compression cache is
  keyed per profile AND target triple, so a multi-target `make dist` pays the
  ~90 s encoder pass once per target.
- Documentation pass: `AGENTS.md`, `docs/*.md` and this changelog were
  condensed, standardized and rewritten in English, with cross-document
  deduplication and no factual changes. (`AGENTS.md`, `CHANGELOG.md`, `docs/`.)
- Routine `cargo update` refresh (42 crates), including `aws-lc-rs` 1.18.2,
  `aws-lc-sys` 0.46.0, `rustls-native-certs` 0.8.5, `hyper` 1.12.0, `h2`
  0.4.20, `tokio` 1.53.3, `libc` 0.2.191, `uuid` 1.28.0 and `zeroize` 1.9.1.
  The refresh also collapses the redundant `windows-sys` 0.60 lane, dropping
  its nine `windows-targets` payload crates (duplicated crate families 42 ->
  33, none newly introduced).

### Fixed

- `make` builds its default current-OS dist through the complete `--host-only`
  path, so macOS no longer leaves an older version's `.app.zip` beside the new
  one in `dist/`.
- The dist script probes `cargo-zigbuild --version` (the subcommand rejects
  `--version`), so an installed Linux cross toolchain is not reported missing
  and the Linux artifacts build.
- A failed "Delete all data" wipe re-enables the cache too (it stayed silently
  read-only for the session).
- A redirect's landed URL stores its own revalidation target, so a direct visit
  to the landed page cannot revalidate (and re-store) a different resource.
- Corrupt or hash-collided cache records are dropped from the index instead of
  being retried on every navigation/render.
- A `304 Not Modified` that omits `Cache-Control` preserves the entry's original
  lifetime (a `no-cache` record stays no-cache), not the class default TTL.
- Corrupt cache index entries and crash-leftover `.clear.*` directories are
  swept at startup; cache indexes/paths reject malformed stems.
- A hostile `Expires`/`Date`/`Last-Modified` with an extreme year (`i32` bounds)
  no longer panics the cache's date math under `overflow-checks`: parsed years
  are held to 1900–9999 and the calendar math runs in `i64`.
- Cache byte accounting after a host-cap eviction is exact: evicted entries'
  index-charged bytes leave the global total (phantom bytes used to evict
  unrelated entries and drift `CacheStats`). A zero-size index row can no longer
  leave a phantom in the FIFO after a poisoned-record drop.
- A failed cache-index flush waits out the debounce before retrying (a per-frame
  flusher re-encrypted all three indexes under the manager lock).
- A favicon revalidated by 304 updates its byte accounting like images do
  (replaced validator fields change the record's size).
- A conditional revalidation of a stale record takes the same global fetch slot
  as any request, so stale hits cannot bypass the documented concurrency bound
  (each buffering a whole body on a 200).
- In-page find washes and the current-match scroll target land on the right
  characters when a case folding changes byte length before the match (`İ`
  folds to two bytes): a range landing on char boundaries fails the folding
  check and falls back to the whole-fragment highlight.
- An unreadable `history.bin`/`bookmarks.bin` (missing or regenerated key,
  corruption, tampering) says so where the data would appear (the Preferences
  caption; the Home grid), not a silent empty store, as the module docs
  promised; the `.bak` copy is kept for recovery.
- The RPM spec refuses to build without `--define "ver <version>"` instead of
  labeling the package with a stale hardcoded version, and the RPM source
  tarball excludes the Flatpak `cargo-sources.json` build product like the
  Flatpak manifest does.
- `scripts/build-dist.sh` proves `mk_app`/`mk_deb` outputs before reporting them
  BUILT (a failed heredoc/copy inside an `if`/`||` context, where `set -e` is
  inert, rode to a success report). Its `find_llvm_strip` discovers the brew
  prefix at runtime and accepts any versioned `llvm@N` keg; the Linux staging
  path no longer shares the global `stage` variable with `mk_deb`; a privacy
  gate that could not run (missing python3) is reported as such, not as a leak;
  single-group runs warn about leftover artifacts of another version.
- The release workflow sets `NO_LOCAL_RPM=1` on the two Ubuntu jobs: Ubuntu
  provides `rpmbuild`, so each host build also attached its own RPM (a second
  x86_64 package plus an undocumented aarch64 one) and paid a from-scratch LTO
  compile against the job timeout; the published RPM is the Fedora job's only.

### Security

- Cache stems loaded from encrypted indexes must match the 16-lowercase-hex
  filename shape before becoming path components; sharding is panic-free on
  malformed input.
- Cache sweeps never follow symlinked shard directories/files, and the shared
  store reader refuses symlinks (`O_NOFOLLOW` on Unix) instead of reading (or
  tightening permissions on) an arbitrary target.
- A response that becomes `no-store`/`Vary: *` evicts any older cached copy for
  the same URL in all three cache classes.
- Cache-relevant headers exceeding the bounded capture (repeated values or one
  oversized policy value) are non-storable, not cached from a partial view that
  could hide `no-store`/`Vary: *`.
- The cache directory is tightened to owner-only permissions like the rest of
  the data directory.
- The 512 MiB cache budget covers index metadata as it is created: a live
  per-entry estimate (key + stored URL + row framing) is charged on every store
  and released on every drop/eviction, not measured on disk after a flush. A
  hostile URL-heavy session can no longer hold an unbounded in-memory index (and
  a multi-hundred-MB serialization under the manager lock) that the body budget
  never sees, and `evict_while_over` can no longer be unable to satisfy the
  budget until the next flush.
- A cache shard directory swapped for a symlink after startup fails closed: the
  document/image body writers re-verify the shard is a real directory
  immediately before `atomic_write` (defense in depth under the same-user
  assumption).
- Supply chain: the three unmaintained-crate advisories of the Linux-only zbus
  5 accessibility stack (RUSTSEC-2026-0370 `zbus_names`, RUSTSEC-2026-0371
  `zvariant`, RUSTSEC-2026-0372 `zvariant_derive`) are recorded as accepted
  ignores in `deny.toml` and `.cargo/audit.toml` with the migration blocker
  documented; `make audit` stays green while any new advisory still fails.
  (`deny.toml`, `.cargo/audit.toml`, `docs/SECURITY.md`.)

## [0.5.4] - 2026-10-16

Find in page, a tabbed Preferences dialog, a history visual pass, tighter
favicon bounds, and standardization across the chrome and viewer.

### Added

- Shortcuts tab in Preferences: a read-only keyboard shortcut quick reference in
  English (Tabs / Navigation / Page / Zoom), curating the keys `app::shortcuts`
  handles, the find-bar stepping and the macOS menu accelerators down to the
  non-obvious ones (no bare `Esc` rows, no keyboard reorders; the full list
  stays in `docs/INTERFACE.md`). (`src/ui/preferences/`, `docs/`.)
- Find in page (`Ctrl/Cmd+F`, or the right-click menu's "Find in page" row) on
  document pages (loaded documents and `about:licenses`): a floating bar from
  the app's chrome kit (omnibox-style capsule with the magnifier, a fixed muted
  `n/m` count, circular previous/next/close icon buttons). Case-insensitive
  substring over the rendered text (prose, headings, lists, quotes, tables,
  fenced code), amber highlight with a stronger current match (code rows wash
  uniformly), `Enter`/`Shift+Enter` (`F3`/`Shift+F3`) navigation, the sheet
  scrolling the current match into view, `Esc` to close. Searching never
  re-wraps the page or monochromes color emoji: matches ride the laid-out text.
  (`src/viewer/find.rs`, `src/viewer/find/tests.rs`,
  `src/viewer/overlays/find.rs`, `src/app/shortcuts.rs`, `src/viewer/inline/`,
  `src/viewer/inline/tests_find.rs`, `src/viewer/code/`, `src/viewer/markdown/`,
  `src/viewer/tests_find.rs`, `src/viewer/table/`, `src/config/mod.rs`.)

### Changed

- The `about:history` page got a visual pass: visits group into rolling date
  sections (Today / Yesterday / Previous 7 days / Previous 30 days / Older),
  each row carries its site favicon with title, relative age and remove X, the
  search box is a capsule with magnifier and clear button, the header prints the
  total and the active retention window, a failed save/delete surfaces its error
  there, and the empty state is an illustrated call to action.
  (`src/ui/history_page/`, `src/ui/icons/mod.rs`, `src/viewer/mod.rs`.)
- Preferences is a tabbed dialog: General (theme, default zoom, restore tabs on
  startup, plus the version/own-license line and the "View third-party licenses"
  link), Data (history retention, "Delete history", "Delete all data and close")
  and Shortcuts. While open, the selected tab and each tab's scroll position are
  kept (neither is written to disk); every tab is content-sized as before.
  (`src/ui/preferences/`, `docs/`.)
- Favicon cache bounds: the per-session origin cap doubles to 512 hosts
  (`MAX_FAVICON_HOSTS`) and icons decode at `MAX_FAVICON_DIM` 160 px, not 384
  px. They never draw larger than the ~27 pt toolbar glyph (143 device px at the
  widest zoom plus a 300% display scale), so a full cache holds at most 50 MiB
  of textures instead of 144 MiB. (`src/config/mod.rs`, `src/media/favicon.rs`,
  `docs/`.)
- Standardization: find washes live in the `Scheme`
  (`find_match`/`find_current`) instead of hardcoded RGB branches, every chrome
  icon button shares `theme::ICON_BOX` (26px), the gallery gap and step gaps
  share `spacing::M`, headings rest on `spacing::XL`, and the content fade
  geometry is named constants. Same pixels except the step gap (10px → 8px)
  and H1/H2 top air (20/18px → 16px), now on the scale. (`src/ui/theme/`,
  `src/ui/tabs/`, `src/ui/navbar/`, `src/ui/omnibox/`, `src/ui/history_page/`,
  `src/viewer/markdown/`, `docs/`.)
- Performance: in-page find counts without the per-fragment `Vec` (new
  `find::count_occurrences`/`contains_match`), code blocks borrow the lowercased
  query instead of cloning it per block per frame, and the visible-row wash
  stops at the first hit. Emoji eviction finds the oldest tick in one scan and
  removes it O(1) (no clone, extra scan or queue memmove — same LRU policy).
  (`src/viewer/find.rs`, `src/viewer/code/`, `src/media/emoji/`.)
- Build/docs sync: `windows.ps1` skips NASM on ARM64 (like CI) and reads the
  toolchain channel from `rust-toolchain.toml`, not a hardcoded `1.98`;
  `README.md` says `.exe zip` and lists `Ctrl/Cmd+F`; `docs/BUILDING.md` lists
  every `make` target (`build`, `check-dist`, `fmt`, `clippy`, `sync-sites`,
  `check-sites`); `docs/ARCHITECTURE.md` names both find files;
  `docs/SECURITY.md` documents the find/history-filter caps and the SVG
  token/depth and bookmark byte numbers. (`windows.ps1`, `README.md`, `docs/`,
  `CHANGELOG.md`.)
- Hardening/standardization pass: the error view strips pasted/server
  credentials before painting `Requested:`/details and `Copy log` (one
  `display_line` helper for both); a response with more than
  `MAX_CONTENT_TYPE_VALUES` (8) `Content-Type` values is rejected, not read from
  a spoofed prefix; image failure tooltips strip credentials and bound at
  `MAX_LOG_LINE_CHARS`; shared bounds moved into `config`
  (`MAX_CONTENT_TYPE_VALUES`, `MAX_IMAGE_ALT/TITLE_CHARS`,
  `MAX_MARKDOWN_RENDER_DEPTH`, `LOCAL_READ_CHUNK_BYTES`,
  `MAX_CODE_HIGHLIGHT_*`); chrome gaps reuse `spacing::M`/`CHROME_*` tokens
  (same pixels); the toast announces its text to screen readers; ASCII find
  counting skips the lowercase allocation. Build scripts fail closed
  (`packaging.sh` `sed`, `validate.sh` temp trap, `build-dist.sh` `DIST` guard +
  `splitext` zip fallback, `lib.sh` `HOME` guard under `set -u`; `make help`
  lists `all`, `make clean` drops Flatpak staging). Docs/root sync covers the
  history shortcut, `+`/`=` zoom, shortcut scope, builtin-sites flow, find
  tests, `getrandom` pin and bounds wording. (`src/ui/error_view.rs`,
  `src/protocol/client/get.rs`, `src/config/mod.rs`, `src/protocol/local.rs`,
  `src/viewer/`, `src/ui/`, `scripts/`, `Makefile`, `docs/`, `README.md`,
  `assets/builtin_sites.txt`, `Cargo.toml`, `CHANGELOG.md`.)
- Standardization round 2: every bare `.strong()` goes through `theme::strong`
  (real Noto-700 face), every 1px/2px stroke through
  `stroke::HAIRLINE`/`EMPHASIS`, the bookmark header through
  `typography::ROW`/`HEADING`, the history H1 through the new
  `typography::PAGE_TITLE` (30px), hairline gaps through the new `spacing::XXS`
  (2px), placeholders/accordion headers through `ICON_BOX`, and tile avatar hues
  move to `theme::AVATAR_PALETTE` (documented fixed-hue exception). The dead
  owned `scheme_for` copy is removed (callers borrow `scheme_ref`), and icon
  buttons with one static text share `finish_static_icon_button`. Same pixels
  except the menu separator air (5px to 4px). (`src/ui/theme/`, `src/ui/`,
  `src/viewer/`, `docs/`.)
- Performance round 2: painted-per-frame labels are memoized — the find bar
  `n/m` count on `(current, total, capped)`, the history header/footer captions,
  the code Copy label lazily. The code match wash consumes the line iterator
  inline (no `Vec` per block per frame), `contains_match` gains an
  allocation-free ASCII path and gates all four range walks, `scan_pairs`
  compares close tags allocation-free, and the `"[image]"`/`"[x] "` frag
  spellings are `Cow::Borrowed`. Settled frames allocate nothing new here.
  (`src/viewer/`, `src/ui/`, `docs/ARCHITECTURE.md`.)
- Hardening round 2: local post-parse read errors echo the `file:` URL through
  the bounded credential-stripped helper (`local::echo_url`), mirroring the
  network log sinks. The remaining dispersed bounds move into `config`
  (`MAX_CODE_CACHE_BYTES`, `MAX_TABLE_WIDTHS_CACHE_BYTES`,
  `MAX_FONT_ASSET_GZIP_BYTES`, `MAX_SETTINGS_BYTES`) with sanitizer roles mapped
  in `util` docs. `docs/SECURITY.md` also names the redirect, concurrency,
  timeout, autolink/anchor/join, route, render-depth and cache caps.
  (`src/protocol/local.rs`, `src/config/mod.rs`, `src/viewer/`,
  `src/media/emoji/`, `src/settings/`, `src/util.rs`, `docs/`.)
- Refactor: the four image entry points share one `ImgCtx` bundle (built per
  call from the walk context via `RenderCtx::img_ctx`) instead of four
  `too_many_arguments` allows; the history clock and page-count rule are
  single-sourced (`util::now_secs`, shared count caption).
  (`src/viewer/image.rs`, `src/viewer/markdown/`, `src/viewer/inline/`,
  `src/ui/history_page/`, `src/ui/preferences/`, `src/ui/icons/`.)
- Docs/build sync: `docs/BUILDING.md` states the real `-SkipDist` meaning (still
  builds; setup-only needs `-SkipBuild` too), `make help` lists `help`,
  `docs/ARCHITECTURE.md` drops the removed `scheme_for`, and `docs/SECURITY.md`
  covers the local echo helper. (`docs/`, `Makefile`.)
- Standardization/hardening pass: the display sanitizer bounds its echo at the
  first ASCII whitespace (a second pasted line can never ride inside the
  address) and keeps a `host:port`-shaped token only when it parses as a
  credential-free URL. `strip_credentials_for_log` returns a `Cow` (clean lines
  allocate nothing; error echoes build only on failure). The scheme-syntax
  predicate is single-sourced in `util`; a hostile `Content-Type` parameter list
  materializes no `Vec` (lazy iterator); a rejected event-bomb line names the
  attempt URL like every other route error; and `logical_base` never collapses a
  directory whose spelling ends in `/` (`/docs.md/` keeps its base and the
  requested path is tried first, instead of the display URL and every relative
  link resolving under `/docs/`). `rewrite` resolves IP-literal `host:port`
  links (`127.0.0.1:8080/x`, `[::1]:8080/x`) as `https://` like the omnibox; an
  allowed-but-unparseable scheme spelling (`https://`, `file://`) goes inert.
  (`src/util.rs`, `src/protocol/`, `docs/`.)
- Graphics/rendering pass: in-page find scrolls the document to a match inside a
  code card or a table cell (the outer sheet re-issues the nested `ScrollArea`
  request) and marks the current fenced-block row with the stronger wash. `Esc`
  is one surface per press (focused fields, cards, popups and floating windows
  keep it). The bookmark grid keeps its documented `tile_w + GRID_GAP` stride (a
  4px-short title scope narrowed the gap to 6px). History rows keep their full
  `ROW_H` stride (52px rows painted at 46px left a blank scroll tail). The
  omnibox indicator uses `theme::ICON_BOX`; a closed tab drops the document
  scroll state of every URL it visited (`ui::remember_scroll` keeps the whole
  list). (`src/viewer/`, `src/ui/`, `docs/`.)
- Build/dist pass: `check-dist.sh --require` turns a missing inspector (no
  python3) into a failure and is wired into `build-dist.sh` and CI (the Fedora
  RPM job installs python3); `mk_deb` verifies the `.deb` was written;
  single-group `build-dist.sh` modes ADD to `dist/` (only `--all`/`--host-only`
  rebuild from scratch); Linux/Windows staging failures warn without aborting
  the multi-OS run; the RPM source tar and Flatpak manifest also exclude
  `.env*`/`.aider*`, and the Flatpak build remaps its sandbox paths; `lib.sh`
  guards `HOME` under `set -u`, `deps.sh` refreshes apt lists once per run,
  `windows.ps1` finds per-user/custom Git Bash installs, and the Makefile quotes
  the cargo path. (`scripts/`, `Makefile`, `windows.ps1`, `packaging/`,
  `.github/workflows/release.yml`, `docs/`.)

### Fixed

- Find bar count is centered again: the query field's inner scope left the
  parent cursor at the field's right edge, so `No matches` sat ~6px left,
  touching the capsule; the bar re-advances past the whole capsule like the
  omnibox, so `n/m` and `No matches` share one centered slot.
  (`src/viewer/overlays/find.rs`, `src/viewer/overlays/tests.rs`.)
- Omnibox and history search hover is coherent: the capsule keeps its hover wash
  whether the pointer is on padding or text (the field sits on top), and the
  whole capsule shows the text cursor — buttons on top (security indicator,
  clear/bookmark) keep their pointer cursor. Hovering never selects the address;
  a clean click selects all, a drag selects from the press point.
  (`src/ui/omnibox/mod.rs`, `src/ui/history_page/mod.rs`.)
- Supply chain: `rustls` 0.23.45 → 0.23.46 (fixes RUSTSEC-2026-0348 /
  RUSTSEC-2026-0349, low-severity TLS issues) plus its transitive `getrandom`
  0.4.3 → 0.3.4 pin (via the `tempfile` chain). `cargo audit` + `cargo deny`
  green. (`Cargo.lock`.)
- Preferences dropdowns (Theme, Keep visits) center in their rows: each field
  paints into an exact rect with the selected value and the shared chevron.
  egui's `ComboBox` is a `Frame`, and a frame in a right-to-left row anchors
  from the parent cursor's right-CENTER seed, which parked the box ~7px low; the
  popup stays egui's (click toggles, click-away/`Esc` close, `Enter`/`Space`
  opens). (`src/ui/preferences/`.)
- Keyboard focus no longer vanishes under the pointer: hand-painted rows (tabs,
  omnibox suggestions, history rows, generic `row`, bookmark tiles) paint the
  focus ring IN ADDITION to the hover wash, and the bookmark tile's `else if`
  that never ran now shows the ring. (`src/ui/tabs/rows.rs`, `src/ui/omnibox/`,
  `src/ui/history_page/`, `src/ui/row.rs`, `src/ui/bookmarks/tile.rs`.)
- Truncated titles/URLs tooltip their full text on hover (tabs, omnibox
  suggestions, history rows, bookmark tiles, error `Requested:`; code language
  chips already did): sighted hover recovers what only accesskit saw,
  allocation-free off-hover. History rows also announce title + URL + age.
  (`src/ui/`, `src/viewer/code/`.)
- History footer delete uses the shared tonal button (themed ramp) with an
  error-tone focus ring instead of a raw `Button`. (`src/ui/history_page/`.)
- Bookmark rename hint reads `Bookmark name` (not `Automatic`).
  (`src/ui/bookmarks/tile.rs`.)
- Error view uses the dialog-title token (21px, not a 24px literal), announces
  its title as a label, wraps the requested address with a hover tooltip, and
  lists hints with the document `·` bullet. (`src/ui/error_view.rs`.)
- Code Copy buttons announce `Copy <lang> code` (not N identical `Copy`s) with a
  hover tooltip. (`src/viewer/code/`.)
- The history page's armed "Delete history" note no longer promises to delete
  the shared `store.key` (it deletes visits; the key stays for
  bookmarks/settings/session), matching Preferences and `docs/SECURITY.md`.
  (`src/ui/history_page/mod.rs`.)
- The macOS `.app.zip` carries no AppleDouble `._*` sidecars (signing plants
  xattrs on every file): the staged bundle is swept before packing and `ditto`
  runs with `--norsrc --noqtn --noextattr`, so the extracted bundle verifies
  with `codesign --verify` and ships no builder metadata. `check-dist.sh` FAILS
  on AppleDouble members in any archive, so a packer that forgets the sweep
  cannot pass the gate. (`scripts/build-dist.sh`, `scripts/check-dist.sh`.)

## [0.5.3] - 2026-10-15

Windows on ARM64 artifacts, a curated-sites sync/validate flow, supply-chain
pinning, and native-platform fixes across the rail, menus, packaging and dark
chrome.

### Added

- Windows on ARM64: the release workflow also ships
  `mdowser-<ver>-windows-aarch64.zip`, built natively with MSVC on a
  GitHub-hosted `windows-11-arm` runner, with the embedded `VERSIONINFO`
  resource verified after the build. `README.md` and `docs/BUILDING.md` document
  both Windows downloads and the native arm64 source build.
- The release body also lists the Linux install commands (`dpkg`/`rpm`/`flatpak`
  and the `.tar.gz` alternative) and the Windows `x86_64`/`aarch64` zip choice,
  next to the macOS Gatekeeper and SmartScreen notes.
- `make sync-sites` / `make check-sites` (`scripts/sync-builtin-sites.sh`): the
  curated omnibox list is synced with `curl` from the MDowser-Sites
  collaboration repo, and every URL is validated the way the browser resolves it
  (candidate cascade plus the Markdown `Content-Type` gate). A failing entry is
  dropped from the imported file and reported at the end with the counts; the
  rest is still imported.

### Changed

- The curated omnibox list format is one `<url> | <title>` line per entry (split
  on the first `|`) instead of `<url><TAB><title>`: a literal TAB is untypable
  in the GitHub web editor, which mangled contributed lines.
  `assets/builtin_sites.txt`, its parser and `make sync-sites` validation moved
  together; mirror the same lines into MDowser-Sites.
- Tab rail reorder: a tab row can be dragged onto another slot (the list follows
  the pointer live and the dragged row carries a primary ring), and
  `Alt+Up`/`Alt+Down` moves the focused row one slot — the keyboard equivalent
  of the bookmark grid's `Alt+Left`/`Alt+Right`. The active tab follows its own
  page through a reorder, and the new order is what "restore tabs on startup"
  remembers. (`src/ui/tabs/`, `src/app/`.)
- `Cmd/Ctrl+click` on a body link opens it in a background tab (an in-page
  `#anchor` becomes its absolute URL plus fragment). (`src/viewer/`.)
- Release workflow supply chain: every `actions/*` step is pinned to the full
  commit SHA of the release tag in its trailing comment (a moved tag cannot
  change what runs, least of all in the `contents: write` publish job).
  (`.github/workflows/release.yml`.)
- `python3` is checked when the `.deb` writer or zip fallback is actually used,
  not up front: `--rpm`, `--flatpak`, `--mac` and `--windows` no longer abort on
  a host without it; `docs/BUILDING.md` documents the scoped requirement.
  (`scripts/build-dist.sh`.)
- `cargo-deny`'s target list includes `aarch64-pc-windows-msvc` (CI builds it
  natively). (`deny.toml`.)
- Preferences toggle switch gives hover feedback on its track.
  (`src/ui/preferences/widgets.rs`.)
- Code-block galley cache: a growing line window extends the laid-out prefix
  instead of rebuilding every galley of the block. (`src/viewer/code/`.)
- Hover tooltips on truncated tab/accordion/step labels are built only while
  hovered (`ui::hover_tooltip`), matching the app-wide convention: raw
  `on_hover_text` calls allocated one `String` per label per frame.
  (`src/viewer/markdown/components.rs`.)

### Fixed

- macOS first launch: right-click > Open no longer bypasses Gatekeeper for the
  ad-hoc signed, not notarized `.app` on current macOS. `README.md`,
  `docs/BUILDING.md` and the release notes document the working flow
  (double-click once, then System Settings > Privacy & Security >
  `Open Anyway`); the `.app.zip` is packed with `ditto --keepParent` when
  present so the signature survives, and `mk_app` verifies it
  (`codesign --verify`). (`README.md`, `docs/BUILDING.md`,
  `.github/workflows/release.yml`, `scripts/build-dist.sh`.)
- Windows first run: the unsigned `.exe` trips SmartScreen, which shows only
  `Don't run` until `More info` is opened; `README.md`, `docs/BUILDING.md` and
  the release notes document the flow (`More info` > `Run anyway`). Linux
  formats need no bypass: the `.tar.gz` binary ships executable, the local
  `.deb`/`.rpm` install without a repository signature, and the unsigned
  `.flatpak` bundle installs from file (`scripts/packaging.sh` stages the binary
  `0755`; `scripts/build-flatpak.sh` exports an unsigned bundle).
- Windows builds driven from Git Bash (`--host-only`) remap `USERPROFILE` (both
  slash styles) like `windows.ps1` did, so the builder's home cannot leak into
  the binary through panic locations when the build skips `windows.ps1`.
- Blockquote indent widget ids are keyed by the block's own `Start(BlockQuote)`
  event index, not a per-render-pass counter, so two different quotes cannot
  alias onto one set of nested egui widget ids when a frame renders a different
  subset of blocks (an unselected `<Tabs>` panel, a closed accordion, the block
  budget). (`src/viewer/markdown/`.)
- `Content-Type` values reach the Markdown gate UNTRUNCATED: a server could pad
  the header past the log-line truncation and hide a non-UTF-8 `charset` inside
  the cut, defeating the "reject before reading the body" gate. Only the error
  line reports a bounded copy. (`src/protocol/`.)
- A pasted password containing `/`, `?` or `#` (`user:pa/ss@host`) is no longer
  echoed into the address field, the error view or "Copy log": the display
  sanitizer scans the whole whitespace-bounded token for the userinfo `@`
  instead of stopping at the first path separator. (`src/protocol/client/`.)
- The `finish_document` failure line is credential-stripped like its
  network-error sibling before it reaches the error view.
  (`src/protocol/client/fetch.rs`.)
- `file://localhost/x` and `file:///x` address the same document: the two
  spellings no longer produce duplicate history entries or a "not bookmarked"
  ribbon while the same local file is bookmarked under the other spelling.
  (`src/protocol/resolver.rs`.)
- Removing one of two bookmarks with the same normalized URL (a store written
  before normalization, or hand-edited) keeps the remaining tile reported as
  bookmarked, exactly like the toggle did. (`src/bookmarks/`.)
- Dark mode: the hover highlight of context-menu rows and Preferences dropdown
  rows was invisible (`container_high` equals the popup fill); both use the
  shared hover token, like the omnibox suggestion rows. (`src/viewer/overlays/`,
  `src/ui/preferences/`.)
- A bookmark tile's pencil and X are reachable with `Tab` once the tile has
  keyboard focus (the tile's frame left them unallocated on focus).
  (`src/ui/bookmarks/tile.rs`.)
- Scrolling a plain (huge/unsupported) code block vertically past a long line no
  longer clamps the horizontal offset to the left edge: the plain path keeps a
  monotonic widest-row memory like the highlighted path. (`src/viewer/code/`.)
- List markers inside a plain blockquote draw in the quote's muted tone instead
  of full-contrast `on_surface`. (`src/viewer/markdown/items.rs`.)
- Clicking the omnibox capsule padding shows the focus ring and fill in the SAME
  frame. (`src/ui/omnibox/`.)
- The open tab's close button is keyed by the tab, never its slot, so focus and
  hover follow the page through a reorder. (`src/ui/tabs/rows.rs`.)
- `.deb` packages declare `libc6 (>= <x.y>)`, derived from the binary's own
  required glibc symbols when an ELF reader is available: `dpkg -i` on a distro
  too old for the binary fails at install time with a clear dependency error
  instead of a "GLIBC_2.xx not found" crash at startup.
  (`scripts/build-dist.sh`, `docs/BUILDING.md`.)
- Linux `.tar.gz`/`.deb` payloads ship stripped on hosts without
  llvm/cross-binutils: the ELF branch falls back to a GNU `strip` (probed, never
  Apple's Mach-O one), the correct tool wherever binutils is present — release
  runners included. (`scripts/build-dist.sh`.)
- `make validate`: an appstreamcli run that exits non-zero with warnings only is
  a note, not a FAIL (appstreamcli lists non-zero counts only); the legacy
  appstream-util validator's stock-icon false positive is likewise a note.
  (`scripts/validate.sh`.)
- `make` scripts source `lib.sh`/`packaging.sh` through the script's own
  resolved directory, so invoking them from another directory
  (`sh ../scripts/validate.sh`) works instead of failing to source helpers.
  (`scripts/`.)
- `windows.ps1 -SkipSetup` installs nothing: the toolchain step no longer runs
  `rustup toolchain install` when setup is skipped. (`windows.ps1`.)
- `make check-sites`/`make sync-sites` no longer leak the first temp file when
  the second `mktemp` fails. (`scripts/sync-builtin-sites.sh`.)
- The desktop entry declares one main category (`Network;WebBrowser;`): two main
  categories tripped `desktop-file-validate`'s hint.
  (`packaging/linux/mdowser.desktop`.)

## [0.5.2] - 2026-10-15

Release automation and a `dist/` privacy gate, plus reproducible, identity-free
release artifacts.

### Added

- `make check-dist` (`scripts/check-dist.sh`): a privacy gate over `dist/` that
  fails when an artifact carries the builder's absolute home / Cargo-registry
  paths, the build host's nodename, a non-zero archive owner or an
  xattr/AppleDouble record. `scripts/build-dist.sh` runs it before writing
  `SHA256SUMS.txt`; `make validate` includes it.
- Release automation (`.github/workflows/release.yml`): pushing a `v*` tag
  builds every target on fresh GitHub-hosted runners (macOS universal
  `.app.zip`; Linux x86_64/arm64 `.tar.gz` + `.deb`; Windows x86_64 `.zip` via
  the mingw cross; `.rpm` from source in a Fedora container; `.flatpak` bundle),
  re-runs the `dist/` privacy gate over the merged set and publishes ONE GitHub
  Release titled with the tag, with a fixed body (changelog link and disclaimer;
  `-alpha`/`-beta`/`-rc` tags as pre-releases) and a single `SHA256SUMS.txt`. No
  Actions cache: every release compiles from scratch and only packaged files
  travel between jobs. A manual dispatch (`dry_run`) validates the pipeline
  without publishing.

### Changed

- Flatpak builds target the freedesktop SDK 26.08 runtime (24.08 is
  end-of-life); `runtime-version` in `packaging/flatpak/dev.mdowser.app.yaml` is
  the single source of truth and `scripts/build-flatpak.sh` derives the runtime
  and extension refs from it.

### Fixed

- Release binaries no longer embed the builder's absolute home and
  Cargo-registry paths (panic locations leak them past `strip`): `make build`,
  `scripts/build-dist.sh`, `windows.ps1` and the RPM spec's `%build` remap them
  at compile time with `--remap-path-prefix` (`scripts/lib.sh`
  `export_remap_rustflags`).
- macOS and Windows `.zip` artifacts no longer record the builder's Unix uid/gid
  (`zip -X` drops the Info-ZIP `0x7875` extra field).
- Linux `.tar.gz` and `.deb` payloads carry no `com.apple.provenance` extended
  attribute as PAX records: staging trees are xattr-swept and tarred with
  `--no-xattrs` where supported.
- The RPM spec no longer discloses the build hostname (`%_buildhost`) and its
  payload cannot inherit the builder's ownership (`%defattr(-, root, root, -)`).
- The Flatpak bundle builds again: one `install` targeted a directory that did
  not exist yet; each file installs with its own name now.

## [0.5.0] - 2026-10-14

A feature release on a full audited pass: untitled documents name themselves
from their first `#` heading, the window remembers its geometry and opens
centered, the "No markdown here" screen hands its target to the OS default
handler, plus a tab-rail reveal, neutral wording and fixes across stores,
packaging and the error view.

### Added

- Documents without a frontmatter `title` name their tab, the OS window title,
  the omnibox page-info card and the default bookmark/history entry with their
  first `#` heading; the schemeless URL stays the last resort. The title is
  derived once per fetch from the already-parsed events (background thread,
  never a second parse), bounded and sanitized like a frontmatter title, and a
  declared `title` always wins. (`src/markdown/document/title.rs`,
  `src/protocol/client/`, `src/browser/tab/`, `src/history/`, `src/bookmarks/`,
  `docs/`.)
- The native window remembers its geometry: last position, size and maximized
  state are stored encrypted in `settings.bin` (like the theme and zoom; written
  debounced after the window is still, and once more on exit) and restored on
  the next launch, with a centering fallback when its monitor is gone.
  (`src/settings/`, `src/app/window.rs`, `docs/`.)
- The error screen's "No markdown here" family gained a large accent "Open in
  your browser" action under the requested address (local files: "Open with the
  default app"): it hands the resolved target to the OS default handler (default
  browser for web pages, the file's default app for local ones) through egui's
  `OpenUrl`. The address is re-resolved through the app's navigability parser at
  click time, so pasted credentials are stripped before the OS sees anything; no
  other error family offers it. (`src/ui/error_view.rs`,
  `src/ui/theme/widgets.rs`, `docs/`.)

### Changed

- Assets layout: the bundled Noto faces moved to `assets/fonts/` (every
  `include_bytes!` site, the `build_support/gen_fonts.rs` inputs and the
  `THIRD_PARTY_NOTICES.md` paths updated), and the omnibox builtin suggestion
  list moved from a Rust `const` to `assets/builtin_sites.txt` (one
  `<url><TAB><title>` line per entry), embedded with `include_str!` and parsed
  once per process by `builtin_sites::builtin_suggestions()`. (`assets/`,
  `src/`, `build_support/`, `docs/`.)
- Tab rail labels (tab titles, `+ New Tab`, `History`) draw the regular face
  again: the rail is chrome, and `theme::strong` (the bundled Noto Sans at
  `wght = 700`) stays for document emphasis (headings, `**strong**` runs,
  callout titles, footnote refs) and the titled surfaces (History, Bookmarks,
  Preferences, the error view, omnibox suggestions, tonal buttons).
  (`src/ui/tabs/rows.rs`, `docs/INTERFACE.md`.)
- The tab rail's hover expansion settles like a native sidebar: a quintic
  ease-out width tween (fast start, gentle tail, mirrored when closing), with
  titles and action labels fading in while drifting the last few pixels into
  their column, each tab row a bounded beat after the one above. Geometry at
  rest is unchanged. (`src/ui/tabs/`, `docs/INTERFACE.md`.)
- Wording: user-facing copy (README, AppStream metadata, man page, docs and
  source comments) describes the Markdown engine, the palette and the layout in
  neutral terms rather than naming third-party products; the README gained a
  Trademarks note. Functional references that stay (CommonMark/GFM as a spec
  comparison, platforms, dependencies, fonts, curated links) are nominative
  only. (`README.md`, `docs/`, `packaging/`, `src/`, `CHANGELOG.md`.)

### Fixed

- Scrolling pages end with a small blank margin: every scrolling page closes
  with a shared `theme::spacing::PAGE_END` (24px) pad under its last element, so
  a long document (and the Home and error pages) no longer ends at the sheet's
  bottom edge; the top gutter and scrolled-edge clipping are unchanged.
  (`src/ui/theme/`, `src/ui/home.rs`, `src/viewer/`, `docs/`.)
- The internal egui id-salt parameters (`ui::omnibox::address_id`,
  `ui::scroll_id_key`, `ui::remember_scroll`, `ui::forget_scroll_state`) are
  named `id_salt` instead of `salt`, so static-analysis heuristics for
  hard-coded cryptographic values no longer misread a UI hash namespace (no
  behavior change; stored ids are identical).
- Initial window position on Windows is centered on the primary monitor while
  the window is created (`NativeOptions::centered`), and the first frame sizes
  it to 80% of the monitor, clamped between 640×480 and 1680×1050 (not a fixed
  1100×750). (`src/main.rs`, `src/config/`, `src/app/window.rs`, `docs/`.)
- The encrypted-store writer's bounded "newest wins" queue is keyed per target
  directory, so two writers to DIFFERENT data directories cannot drop each
  other's queued blob (latent; the app writes one directory, and the test suite
  is deterministic). (`src/store/writer.rs`, `docs/`.)
- The tab rail's right-edge separator no longer pops on or off when the hover
  overlay opens or closes: it is drawn collapsed too (smaller, fainter — a
  dimmer hairline in dark mode) and its strength follows the open tween, so the
  rail/page boundary is continuous. (`src/ui/tabs/`, `docs/INTERFACE.md`.)
- The error screen's red status dot sits centered with the family title: it is
  painted on the title's laid-out rect instead of its own shorter slot
  (top-clamped ~6px above the title). (`src/ui/error_view.rs`.)
- Fixed-size buttons (`theme::tonal_button_sized` and the new
  `theme::primary_button_sized`: Retry and the external-open action) center
  their label: the shared body uses `add_sized` (`centered_and_justified`), not
  `Button::min_size`, which aligned to the PARENT layout's cross axis and
  left-parked the label inside the error view's left-aligned column.
  (`src/ui/theme/widgets.rs`.)

## [0.4.6] - 2026-10-14

A professionalization pass: one type/stroke/spacing scale for the chrome
(pixel-identical), the interned palette on hot draw paths, fail-closed packaging
helpers, one version-parse recipe, the docs sync closed, and the native Windows
bootstrap.

### Added

- `windows.ps1`: a native Windows bootstrap (winget-only, idempotent),
  equivalent to `make` without `make`. It installs Rustup 1.98, MSVC Build Tools
  + the C++ workload, NASM, Git (Git Bash), Python 3.12 (with a `python3` shim)
  and LLVM (`llvm-strip`), then runs `cargo build --locked --release` plus the
  host `.exe` zip via `scripts/build-dist.sh --host-only`. Flags:
  `-Test`/`-Validate`/`-SkipBuild`/`-SkipDist`/`-SkipSetup`/`-NoElevate`.
  (`windows.ps1`, `docs/BUILDING.md`.)
- `.exe` strip falls back to `llvm-strip` when `x86_64-w64-mingw32-strip` is
  missing, so a native MSVC build (which ships LLVM, not mingw) no longer ships
  unstripped; `$STRIP` still wins. (`scripts/build-dist.sh`.)

### Changed

- Standardization: chrome type, strokes and gaps share one scale —
  `theme::typography`
  (`UI_COMPACT`/`UI_SMALL`/`EYEBROW`/`UI_LINK`/`BUTTON`/`DROPDOWN`/`ROW`/`HEADING`/`DIALOG_TITLE`/`DISPLAY`),
  `theme::stroke` (`HAIRLINE`/`ICON`/`EMPHASIS`) and `theme::spacing`
  (`XS`/`S`/`M`/`L`/`XL`) back the omnibox card, suggestion rows, history rows,
  Preferences dialog, task checkboxes, badges, cards and tables (same pixels;
  graphics stay pixel-identical). (`src/ui/theme/`, `src/ui/`, `src/viewer/`,
  `docs/ARCHITECTURE.md`.)
- Hot draw paths borrow the interned palette (`theme::scheme_ref`) instead of
  copying the 50-field `Scheme`: `tonal_button_sized`, `content_frame` and
  `paint_content_fade` no longer rebuild it per button/surface; the one-off
  `theme::scheme_for` copy stays for non-hot callers. (`src/ui/theme/`,
  `docs/ARCHITECTURE.md`.)
- Build tooling, fail-closed and best-effort: `render_metainfo` rejects a
  version outside `[0-9A-Za-z._+-]` and escapes `&`/`\` before the `sed`
  replacement; `install_linux_tree` pre-flights every source (binary, desktop
  entry, metainfo, icons, SVG, man page) before touching the destination;
  `mk_app` checks its binary/icon sources and returns instead of aborting;
  `do_mac` guards `lipo`/`mk_app` and `do_host` the host build, so `--all` warns
  and continues with a reason; `MODE` default and the `EXIT` trap read without
  POSIX trivia; intentional word-splits carry `shellcheck` disables;
  `deps.sh`/`validate.sh` document their `set -u` (best-effort, never abort).
  (`scripts/`, `docs/BUILDING.md`.)
- Single version-parse recipe: the RPM spec, the Flatpak manifest and the
  `BUILDING.md` container recipes use the same scoped `[package]` parse as
  `scripts/packaging.sh` `app_version`; the spec comment matches the real
  `build-dist.sh --rpm` defines and the Flatpak manual recipe matches
  `build-flatpak.sh` (`--state-dir`). (`packaging/`, `docs/BUILDING.md`.)
- Version-control policy is documented and active: the repository lives on
  GitHub (`origin`); commits only on explicit request, never automatically, with
  no AI attribution. (`AGENTS.md`, `docs/CONTRIBUTING.md`.)
- Package and crate metadata point at the upstream repository instead of a
  placeholder: the RPM `URL`, a `Homepage` field in the `.deb` control file, the
  AppStream `homepage`/`bugtracker`/`vcs-browser` links, the Cargo
  `homepage`/`repository` fields and the man page homepage. (`packaging/`,
  `scripts/build-dist.sh`, `Cargo.toml`.)
- README: dropped the "PRs welcome" badge; the build section points native
  Windows at `windows.ps1` and the security link uses the `docs/` label.
  (`README.md`.)
- Docs sync: `BUILDING.md` drops `choco` for NASM (winget, like `windows.ps1`),
  notes the `scripts/` equivalents skip `make deps` and the host build, names
  the RPM as copied verbatim from `rpmbuild`, and attributes the `.exe` strip
  fallback to `build-dist.sh`; `ARCHITECTURE.md` documents the new theme scales
  and the pre-flighted Linux tree. (`docs/`, `packaging/`.)

## [0.4.5] - 2026-10-13

A hardening, rendering and packaging pass on a performance pass: one parser pass
per document, bounded normalization scans on hostile input, O(1) settled store
paths, the fetch-slot counter fix, and fixes across security, the renderer and
every package format.

### Changed

- Copy text derives from the OWNED parse events
  (`markdown::document::plain_text_from_events`), not a second parser pass over
  the normalized body; a corpus test asserts byte-identical output against the
  body oracle across callouts (split markers included), code, skip links,
  tables, tasks, footnotes, comments, metadata, autolinks and emoji.
  (`src/markdown/document/plain.rs`, `src/protocol/client/fetch.rs`.)
- The structural `:::` and MDX/component normalization passes run under a
  per-document scan budget (`MAX_STRUCTURAL_SCAN_LINES` /
  `MAX_MARKUP_SCAN_LINES`), and `strip_line` never rescans a line for a closer
  it cannot find (`<` past the last `>`, `{` past the last `}`, plus a per-line
  budget), so unmatched openers cannot burn O(n²) CPU on the fetch thread.
  Malformed blocks still fall back to literal source.
  (`src/markdown/document/`.)
- `ImageCache::poll` returns in O(1) when nothing is queued or loading.
  (`src/media/cache/mod.rs`.)
- Settings loads the shared `store.key` once per process and startup validation
  seeds the plaintext body cache, so a valid `settings.bin` is read + decrypted
  once per launch, not twice. (`src/settings/file.rs`,
  `src/settings/session.rs`.)
- History keeps an incremental plaintext byte total: `trim` answers "under both
  caps" in O(1), so a recorded visit no longer rescans up to 20 000 entries.
  (`src/history/`.)
- The session snapshot is built only when its save is due, not every frame
  during a failed-save backoff. (`src/app/run.rs`.)
- `ui::tabs` no longer rebuilds the collapsed rail list while the open overlay
  covers it (pointer in or above the overlay) — up to `MAX_TABS` rows per
  frame. (`src/ui/tabs/mod.rs`.)
- Home bookmark tiles memoize their laid-out two-row name per URL (invalidated
  by the store generation, the theme and the tile width) instead of rebuilding
  one `String` + one `LayoutJob` per visible tile per frame.
  (`src/ui/bookmarks/tile.rs`, `src/ui/bookmarks/mod.rs`.)
- `markup_pass` re-masks into the SAME buffer after stripping comment spans
  (`mask_code_into`), saving one body-sized allocation per pass.
  (`src/markdown/document/normalize_links.rs`,
  `src/markdown/document/normalize_markup.rs`.)
- Color emoji rasterize on background workers: a texture miss (first sighting or
  a zoom/DPI change) no longer shapes + parses + rasterizes inside the frame.
  `EmojiCache::poll` uploads the finished texture, and a process-wide job cap
  (`config::MAX_CONCURRENT_EMOJI`) keeps a burst of new emoji from spawning one
  thread per cluster; the bookmark-tile glyph cache (`ui::icons`) shares the
  pipeline and is polled once per frame. (`src/media/emoji/mod.rs`,
  `src/ui/icons/mod.rs`, `src/browser/tab/`, `src/app/run.rs`.)
- `FaviconCache` answers `has_pending` and gates `poll` in O(1) from
  loading/backoff/failed counters; the due-backoff walk runs only once the
  earliest deadline passes, the saturated-cache flag is O(1), and a probe
  deferred by the image-slot limiter records a short backoff instead of
  re-parsing the URL every following frame. (`src/media/favicon.rs`.)
- Bookmark saves serialize + encrypt ON THE WRITER THREAD: the store hands the
  worker an entries snapshot (`WriteJob::build`) instead of building a blob of
  up to 48 MiB on the UI thread, so a save cannot copy + AEAD megabytes inside a
  frame. The synchronous test/exit path keeps the inline build.
  (`src/store/writer.rs`, `src/bookmarks/persist.rs`.)
- `history::format_age` returns a borrowed label for its fixed buckets ("just
  now", "yesterday", "1 week ago", ...), so rendering the history page allocates
  no `String` per visible row per frame; the no-pipe pre-reject keeps
  `has_oversized_table_line` from walking every line of a document that cannot
  hold a table. (`src/history/persist.rs`, `src/markdown/document/mod.rs`.)
- Allocation/copy trims on hot paths: code-block galleys paint the visible slice
  directly; `strip_scheme_everywhere` fast-paths without lowercasing;
  `norm_lang` and `is_badge_close` compare allocation-free; `read_regular_file`
  reads 64 KiB chunks; the Preferences palette is interned
  (`theme::scheme_ref_ctx`). (`src/util.rs`, `src/assets.rs`, `src/markdown/`,
  `src/ui/`, `src/bookmarks/persist.rs`, `src/settings/platform.rs`.)
- Emoji rendering walks text segments through a callback instead of building a
  `Vec` per fragment per repaint, and probes for a color emoji allocation-free.
  (`src/media/emoji/mod.rs`, `src/viewer/inline/mod.rs`.)
- Markdown normalization: `<script>`/`<style>`/`<pre>` region scanning compares
  case-insensitively without lowercasing every line, component markers are
  shape-gated before tag parsing (a non-marker 512 KiB block is not rescanned
  every frame), and component/callout expansion is depth-capped like the `:::`
  pass. (`src/markdown/document/normalize_markup.rs`.)
- Settled tabs return early from polling for non-`Loading` states instead of
  swapping their whole state enum. (`src/browser/tab/poll.rs`.)
- MDX tab labels elide to the sheet (per-cell cap) and carry accessible
  names/selected state plus a focus ring, as do accordion headers; the code
  card's language chip truncates and keeps the full value in a tooltip.
  (`src/viewer/markdown/components.rs`, `src/viewer/code/mod.rs`.)
- `theme::strong` is applied at every bare `.strong()` site (History H1 and
  rows, Bookmarks header, omnibox suggestion titles, tab titles, tonal buttons),
  so "bold" always means the bundled bold face. (`src/ui/`.)
- Build tooling: warm builds skip hashing 45 MB of font bytes (`gen_fonts`
  checks size/mtime first, falls back to the content hash); generated icons are
  written atomically (temp + rename) so concurrent cargo invocations cannot see
  a half-written PNG (`build_support/`). `build-dist.sh` cleans stage dirs on
  abort, normalizes tar ownership, and keeps macOS AppleDouble `._name` sidecars
  out of the `.deb` payload and `.tar.gz` artifacts; it also re-roots the RPM
  source tarball with bsdtar, `packaging.sh` scopes the version parse to
  `[package]` and chmods the rendered metainfo 0644, `make deps` installs
  `flatpak-builder` on apt and uses Arch's `python` package name, and
  `install.sh` sets `COPYFILE_DISABLE=1`. (`scripts/`.)
- `THIRD_PARTY_NOTICES.md` gained the full direct-dependency license table;
  `docs/ARCHITECTURE.md`, `docs/BUILDING.md` and `packaging/README.md` were
  updated for the changes above. A stale duplicate `[Unreleased]` section
  (released content, stranded mid-file) was removed and its one unreleased entry
  folded into 0.4.3.

### Fixed

- Pasted credentials in the no-separator spelling `scheme:user:pass@host` were
  echoed verbatim by the HTTPS and local-file parsers (error view and "Copy
  log"); the shared log sanitizer removes that userinfo too, and every "bad
  address" message goes through a bounded, credential-free echo. (`src/util.rs`,
  `src/protocol/client/mod.rs`, `src/protocol/local.rs`.)
- A relative `HOME`/`APPDATA`/`LOCALAPPDATA` resolved the per-user data
  directory — including `wipe_all_data`'s recursive delete and `~/` expansion
  — under the working directory; every base must now be absolute, like
  `XDG_DATA_HOME` already was. (`src/util.rs`, `src/settings/platform.rs`,
  `src/protocol/local.rs`.)
- Direct (no-hard-link) store-key creation could return a key that was no longer
  the file on disk: a concurrent launcher classifying the partial file as
  corrupt deletes and replaces it, and the writer kept encrypting with its
  orphaned in-memory key. The returned value is read back from the path,
  fail-closed. (`src/store/crypto/mod.rs`.)
- `.deb` packages were uninstallable: two `#` comment lines had been written
  into the binary control file ("field name '#' must be followed by colon"). The
  comments moved out and `mk_deb` refuses a control file with a comment line.
  (`scripts/build-dist.sh`.)
- Flatpak exports did not carry the application id (`rename-desktop-file` /
  `rename-icon` were missing), so an installed bundle never appeared in menus or
  software centers under its own name and icon; the manifest also shipped the
  committed metainfo release (stale after every bump) and let flatpak-builder
  copy `target/`/`dist/` (multi-GB) into its source tree. The manifest renames
  the exports, renders version/launchable/icon for the bundle, skips build
  output and strips the binary. (`packaging/flatpak/dev.mdowser.app.yaml`,
  `scripts/build-flatpak.sh`.)
- The footnote back-arrow works when the definition precedes its reference: the
  reference-rect memo is stamped with the per-document salt, not a per-FRAME
  generation. (`src/viewer/inline/mod.rs`, `src/viewer/markdown/mod.rs`.)
- Tiny images were stretched by independent per-axis size floors (a 2×40 became
  4×40); the 4px clickable floor scales both sides uniformly.
  (`src/viewer/image.rs`.)
- The Preferences dialog is content-sized and top-anchored, so arming a
  two-click confirmation never moves the control above it (it was a fixed 720pt
  with ~150px of slack). Long persistence errors are readable via hover tooltips
  and the history search field is vertically centered.
  (`src/ui/preferences/mod.rs`, `src/ui/history_page/mod.rs`.)
- Disabled chrome icons use the muted full-opacity tone of disabled text buttons
  (55% opacity gave ~2.3:1 on the light chrome, nearly invisible). Icon strokes
  do not thin out at 175% zoom. (`src/ui/icons/mod.rs`.)
- The global fetch/local-read slot counters could underflow to `usize::MAX`: a
  FAILED acquisition still ran the slot guard's `Drop` (`bool::then_some(Guard)`
  constructs the guard eagerly and drops it on `None`), decrementing a counter
  never incremented. After a handful of cancelled or queued navigations, every
  later document load waited out the 20s slot timeout and failed with
  `busy: too many concurrent fetches`, and the test suite starved its local
  reads. Both guards are constructed only on success; the full suite passes on
  the default thread count. (`src/protocol/client/mod.rs`.)

## [0.4.4] - 2026-10-13

A packaging pass: Linux gets real desktop integration (software-center page,
desktop entry, scalable icon, man page) shared by the `.deb`, source-built
`.rpm`, Flatpak manifest and `make install`, and the Windows `.exe` carries
version info and a DPI manifest in PE resources.

### Added

- Linux packaging is generated from ONE shared file set (`packaging/linux/`,
  staged by `scripts/packaging.sh` `install_linux_tree`), so the `.deb`, `.rpm`,
  Flatpak build and a manual install cannot drift: a desktop entry (registered
  categories, keywords, `StartupNotify`, `TryExec`, Markdown MIME types),
  AppStream metadata (`dev.mdowser.app.metainfo.xml`, the page GNOME Software
  and KDE Discover render, its `<release>` pinned to the `Cargo.toml` version at
  package time), a scalable hicolor icon beside the PNG ladder, and a
  `mdowser.1` man page. The `.deb` control gains `Installed-Size`,
  `Rules-Requires-Root: no` and `Section: web`. (`packaging/`,
  `scripts/packaging.sh`, `scripts/build-dist.sh`, `docs/BUILDING.md`,
  `docs/NAVIGATION.md`.)
- `packaging/rpm/mdowser.spec` plus `make dist-rpm`
  (`scripts/build-dist.sh --rpm`) build an RPM from the full source tree with
  the system `rpmbuild`; on a Fedora/RHEL/openSUSE host `make`/`make dist` also
  produce it. (`packaging/rpm/`, `Makefile`, `scripts/build-dist.sh`.)
- `make install` (`scripts/install.sh`) installs the binary, desktop entry,
  AppStream metadata, icons and man page under `DESTDIR`/`PREFIX`; it is the RPM
  spec's `%install`. (`scripts/install.sh`, `Makefile`.)
- `packaging/flatpak/dev.mdowser.app.yaml` plus `make flatpak`
  (`scripts/build-flatpak.sh`) build one artifact for every Linux distribution
  (needs `flatpak-builder` and vendored crate sources). (`packaging/flatpak/`,
  `scripts/build-flatpak.sh`, `Makefile`.)
- `make validate` (`scripts/validate.sh`) checks the packaging metadata
  best-effort: AppStream metadata (well-formedness and
  appstreamcli/appstream-util), the desktop entry (`desktop-file-validate`), the
  RPM spec (`rpmspec -P`, `rpmlint`) and the Flatpak manifest — skipping,
  never failing, when a tool is missing. (`scripts/validate.sh`, `Makefile`,
  `docs/BUILDING.md`.)
- `make deps` (`scripts/deps.sh`) installs build and packaging dependencies
  best-effort and idempotently: Rust (`rustup` + stable on macOS), `nasm`,
  `python3`, the macOS cross tools (`zig`, `cargo-zigbuild`, `mingw-w64`, rustup
  targets) and the packaging tools (`desktop-file-utils`, `appstream`,
  `appstream-glib`, `rpm`, `lintian`, PyYAML). Homebrew on macOS,
  `apt-get`/`dnf`/`pacman` on Linux; a present tool is left alone, a package
  that cannot be installed is a `SKIP`, and `SKIP_SETUP=1` is a dry run. Groups:
  `toolchain`, `cross`, `packaging` (`make deps DEPS=packaging`).
  (`scripts/deps.sh`, `Makefile`, `docs/BUILDING.md`.)
- Windows executables embed a `VERSIONINFO` resource (product/file description,
  version and copyright) and an application manifest (`RT_MANIFEST` id 1:
  per-monitor DPI awareness, supported Windows versions, UTF-8 active code page,
  long-path awareness, segment heap), on top of the embedded icon. `main` sets
  `windows_subsystem = "windows"` so no console window opens behind the GUI.
  (`build.rs`, `src/main.rs`, `docs/ARCHITECTURE.md`, `docs/BUILDING.md`.)

### Changed

- `make` and `make dist` install every dependency the host can use (`make deps`)
  before building, and each entry point produces exactly the platform's artifact
  set: macOS ONLY the universal `.app.zip` (the loose per-arch `.tar.gz` are
  gone), Linux a `.tar.gz` + `.deb`/`.rpm` + the Flatpak bundle, Windows the
  `.exe` zip. `make` is the current platform; `make dist` is every platform the
  host can cross-build. (`Makefile`, `scripts/build-dist.sh`,
  `scripts/build-flatpak.sh`, `docs/BUILDING.md`, `README.md`.)
- The Flatpak build is part of `make dist` (`--flatpak` / `do_flatpak`) and
  exports the DISTRIBUTABLE bundle `dist/mdowser-<ver>.flatpak`
  (`flatpak-builder --repo` + `flatpak build-bundle`) instead of installing the
  app; freedesktop runtimes are pulled best-effort (`SKIP_SETUP=1` opts out).
  `make flatpak` / `make dist-flatpak` build the same bundle.
  (`scripts/build-flatpak.sh`, `scripts/build-dist.sh`, `Makefile`,
  `packaging/README.md`, `docs/BUILDING.md`.)
- All product-credit strings are plain `MDowser`: the Windows `VERSIONINFO`
  `CompanyName`/`LegalCopyright`, the `.deb` `Maintainer`, the AppStream
  `developer` name, the man page author and the MPL header of every source file.
  (`build.rs`, `packaging/`, `scripts/`, `src/`.)

### Fixed

- The `dist/` checksum loop no longer aborts the run (`set -e`) when the name
  that sorts LAST is the skipped `SHA256SUMS.txt`, which an uppercase
  `MDowser-…` `.app.zip` produces. The skip is an `if`, never a trailing `&&`.
  (`scripts/build-dist.sh`.)

## [0.4.3] - 2026-10-13

A code-size and release-hardening pass: inline code and code cards draw at the
BODY size on the shared 24px row grid, the footnote back-arrow closes the note
text on its last line, and shipped binaries get fail-closed arithmetic plus
doc/comment drift closed.

### Added

- macOS completes the system menu bar with the browser's commands: Application
  (About MDowser, Preferences… `Cmd+,`, Services, Hide/Hide Others/Show All,
  Quit), File (New Tab `Cmd+T`, Close Tab `Cmd+W`), Edit (Copy `Cmd+C`, Paste
  `Cmd+V`, Select all `Cmd+A`), View (Reload `Cmd+R`, Zoom In/Out/Actual Size),
  History (Show History `Cmd+Y`) and Window. Each item replays the keyboard
  shortcut, so menu and keys share one handler. (`src/app/menu.rs`,
  `Cargo.toml`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Omnibox builtin suggestions gain `https://modelcontextprotocol.io/` ("Model
  Context Protocol"). (`src/builtin_sites.rs`.)

### Changed

- Inline code and code cards draw at the BODY type size (16px), not 85% of it:
  code is exactly as wide as the prose around it, the code font and text font
  share one size token (`theme::BODY_FONT_SIZE`) and one 24px line grid, and the
  inline pill's `.2em .4em` padding and height cap are re-derived from that
  size. The pill's baseline is POSITIONED, not re-metricized through a synthetic
  `line_height`, which no longer fits the bundled mono cut's face metrics.
  (`src/ui/theme/`, `src/viewer/code/`, `src/viewer/inline/`,
  `docs/RENDERING.md`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- The omnibox security indicator is a real padlock (closed shackle over the
  body); clicking it opens a page-info card under the capsule with the kind, the
  document's own title when it has one, the exact address WITH its scheme. The
  card is an overlay (it never shifts the capsule) and closes on the next
  indicator click, `Esc`, a click outside, `Ctrl/Cmd+L` and any navigation; its
  address stays bounded (`config::SITE_INFO_ADDRESS_CHARS`). (`src/ui/omnibox/`,
  `src/ui/icons/`, `src/browser/tab/`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Release binaries compile with `overflow-checks = true`: arithmetic that could
  overflow already used `wrapping_*`/`saturating_*`, so a silent wrap in new
  code (a budget, a size, an offset into a body) panics loudly instead of
  feeding a wrong number to a cap check. `panic = "abort"` stays off on purpose
  (the image pipeline catches decoder panics). (`Cargo.toml`.)
- Docs and comments synced: the CI-only phrasings in lint comments (this
  repository has no workflow automation) name the real gate
  (`scripts/build-test.sh`), and the OS/CLI open-target cap is documented as the
  `MAX_TABS` list it is. (`Cargo.toml`, `rust-toolchain.toml`,
  `docs/ARCHITECTURE.md`.)

### Fixed

- The footnote back-arrow closes the note's TEXT: drawn as the note's own last
  inline box, it sits right after the final word on the same line (and wraps
  like a word when no room is left), not on a row of its own that read as a
  separate paragraph. A note whose last block cannot flow inline (a code fence,
  list, quote or table) keeps a small right-aligned trailing row, so the
  back-reference is never lost. (`src/viewer/markdown/`, `src/viewer/inline/`,
  `docs/RENDERING.md`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- The macOS menu bar no longer carries AppKit's "Show Tab Bar"/"Show All Tabs"
  items: automatic window tabbing is opted out at startup
  (`NSWindow.setAllowsAutomaticWindowTabbing(false)`, before any window exists),
  so the system cannot inject window-tab-group entries into the View menu —
  MDowser's tabs are the in-window rail. "About MDowser" shows the app's own
  embedded 256px icon, not AppKit's placeholder folder. (`src/main.rs`,
  `src/app/menu.rs`, `src/app/mod.rs`, `Cargo.toml`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Home bookmark tiles no longer cut their own name: the tile reserved 30px for
  the name while two 12.5pt rows need ~34px at the bundled font's metrics, so
  the second row spilled past the bottom edge — the text clip rect shaved its
  descenders (and an elided title's `…`) and the hover highlight (the tile
  rect) cut through the line. The tile is 96px tall and the name block starts
  2px higher (`TILE_H`, `TITLE_TOP`), keeping the icon, both rows and the hover
  wash inside with real air below. (`src/ui/bookmarks/`.)
- The bookmark grid's section header ribbon aligns with the "Bookmarks" title:
  one point down on the painted glyph (the 18px slot stays in the layout) lines
  up the optical centers (`HEADER_ICON_DY`), since the ink is a touch taller
  than the 15pt caps. (`src/ui/bookmarks/`.)
- The bookmark grid's reserved header block includes the item spacing after the
  header row, so the grid's ScrollArea gets its full height (the viewport came
  out 4px short and clipped the last tile row's bottom — the hover wash lost
  its rounded corners and the rename field its bottom border).
  (`src/ui/bookmarks/`.)
- The inline bookmark rename is a compact one-line field: it paints the draft at
  the tile name's own size (12.5pt, not the 16pt `Body` fallback), hugs the text
  instead of filling the two-line name slot, stays centered while the draft fits
  and scrolls with the caret once wider than the field. (`src/ui/bookmarks/`.)
- The omnibox suggestion popup no longer paints its first row as selected while
  `Enter` submits the typed address: nothing is highlighted until the user picks
  a row, so the highlight always means "`Enter` opens this", and the first
  `Down`/`Up` selects the first/last row instead of skipping it. The row fill
  uses a token visible on the popup in dark mode (not `container_high`,
  identical to the popup's own `window_fill` there). (`src/ui/omnibox/`,
  `docs/INTERFACE.md`, `docs/NAVIGATION.md`, `docs/ARCHITECTURE.md`.)
- A fitting table is no longer mistaken for an overflowing one by a
  floating-point ulp: the grow branch of `distribute_columns` summed a hair past
  the usable width (`86.25 + 340` grown to fill 757 came out `757.00006 > 757`),
  and `flush_table` reads `sum > usable` as "even the floors do not fit",
  re-laying every column for horizontal scrolling capped at `scroll_max` (a
  pricing table's second column collapsed from ~604px to 128px). A rounding
  residual is taken off the widest column, never below `min_col`, so a REAL
  overflow still scrolls. (`src/viewer/table/render.rs`,
  `docs/ARCHITECTURE.md`.)
- A fresh navigation to an already-seen page starts at the top: the saved scroll
  offset belongs to the (tab, URL) pair, so only Back/Forward/Refresh restore
  the reading position (a cross-page `#fragment` still scrolls to its target).
  (`src/browser/tab/`, `src/viewer/mod.rs`, `docs/ARCHITECTURE.md`.)

## [0.4.2] - 2026-10-13

A hardening, rendering and rendering-cost pass: the store key is tightened on
its OPENED handle (`O_NOFOLLOW`, never a path-based `chmod`), a failed "Delete
all data and close" keeps the app open, a corrupt `restore_tabs` value cannot
delete the encrypted session, table CELLS are capped per frame, and several
rendering bugs are fixed (the hidden-code-lines notice over row 0, same-titled
accordions sharing one state, a known `#fragment` dropped, heading badges
flattening, grapheme-split truncation).

### Added

- Structural `:::` custom containers render as the same natives as their JSX
  equivalents: `:::tabs`/`:::tab`, `:::code-group`, `:::steps`/`:::step`,
  `:::card` and `:::accordion`/`:::details` become the same inert
  `<mdowser-cmp …/>` layout markers (tab/step labels via bare text or
  `label=`/`title=`, cards via `title=`/`href=`). They are markup-level only —
  no JavaScript runs, unknown names, missing closers and fenced code stay
  literal, and column-0-only openers never break out of a list item or quote.
  (`src/markdown/document/`, `README.md`, `docs/RENDERING.md`,
  `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- The bookmark grid is keyboard-reorderable: `Alt+Left`/`Alt+Right` on the
  FOCUSED tile move it one display slot (the keyboard equivalent of the drag).
  The move mirrors the newest-first display order onto the insertion-ordered
  store (`ui::bookmarks::move_indices`) and never opens the page.
  (`src/ui/bookmarks/`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)

### Changed

- The shared `store.key` is tightened on its OPENED handle and opened with
  `O_NOFOLLOW`: a symlinked key is refused fail-closed without being followed or
  `chmod`-ing its target, closing the check-then-chmod window of a path-based
  `std::fs::set_permissions`. `settings::tighten_opened_perms` reports whether
  the handle is user-only, so the key path fails closed when the filesystem
  refuses. (`src/store/crypto/`, `src/settings/platform.rs`,
  `docs/SECURITY.md`.)
- `StoreWriter` spawns its worker with `thread::Builder`: a refused thread start
  is reported through the store's own queue-error path instead of panicking the
  UI thread, and the "two spawns refused" branch is reachable.
  (`src/store/writer.rs`.)
- `parse_https` rejects a `scheme:user:pass@host` spelling (a second colon
  before the `@`), which was read as a userinfo host and navigated to
  `https://host`, silently reinterpreting a spelling the contract blocks. The
  plain `user:pass@host` and `user:pass@host:8080` forms keep working with
  credentials stripped. (`src/protocol/client/mod.rs`, `docs/SECURITY.md`.)
- `util::truncate_chars` never cuts inside a grapheme cluster: a combining mark,
  ZWJ emoji family or flag stays whole (or is dropped whole when it cannot fit),
  so a display cap cannot produce a bare base character with a dangling joiner.
  Every cap that uses it (image alt/title, block text, footnote labels, log
  lines) inherits the fix. (`src/util.rs`, `docs/ARCHITECTURE.md`.)
- A component label (tab cell, accordion header, step title) is capped at
  `config::MAX_COMPONENT_LABEL_CHARS` with the full text in the hover tooltip:
  one 2 000-char label allocated a cell thousands of px wide and pushed every
  later tab outside the reading sheet. (`src/viewer/markdown/components.rs`,
  `src/config/mod.rs`, `docs/RENDERING.md`.)
- The tab rail's close X uses the shared chrome icon-button recipe
  (`icons::icon_button_at_named_lazy`): the hand-rolled copy had drifted (CARD
  radius hover fill, no pressed state, a re-implemented tooltip).
  (`src/ui/tabs/rows.rs`, `docs/ARCHITECTURE.md`.)
- The omnibox suggestion popup's `Area` id is scoped to the SURFACE, not the
  tab: egui never prunes `AreaState`, so a per-tab id leaked one area and one
  layer per tab that ever opened the popup, and every later hit test scanned a
  linearly growing layer order. (`src/ui/omnibox/mod.rs`.)
- `detect_callout` clamps its prefix scan to a char boundary: when byte 12 split
  a multi-byte character, the fallback uppercased the WHOLE (up to 512 KiB)
  quoted paragraph per frame. (`src/viewer/markdown/links.rs`.)
- Preferences delete confirmations arm through `config::deadline_after`
  (panic-free `Instant` arithmetic, the one source of every surface's deadline)
  and the dialog card uses `theme::radius::SHEET` instead of a hard-coded 12.
  (`src/ui/preferences/`, `docs/ARCHITECTURE.md`.)
- `settings::cleanup_legacy_files` runs at most once per directory per process:
  the sweep sat on settings-hot read paths and, off Linux, ran `ps` per
  crash-orphaned temp on the UI thread. (`src/settings/platform.rs`.)
- Standardization: exact-match `add_space` literals in the error view, the
  History page, Preferences and the Home grid use the shared `theme::spacing`
  scale (`XS`/`S`/`M`/`L`) (same pixel values; graphics stay pixel-identical).
  (`src/ui/error_view.rs`, `src/ui/history_page/`, `src/ui/preferences/`,
  `src/ui/bookmarks/`.)
- Docs sync: `RENDERING.md` documents that an escaped `\:shortcode:` or
  `\[!NOTE]` still expands (the passes match the RENDERED text; an inline code
  span keeps them literal). (`docs/RENDERING.md`.)

### Fixed

- The address bar no longer navigates to a suggestion the pointer merely sits
  over: `hovered()` is positional, so a stationary pointer over the popup marked
  the highlighted row as an explicit pick and `Enter` went to the full-path
  suggestion instead of the typed intermediate address. A hover pick now
  requires a `PointerMoved` this frame (or a non-zero pointer delta);
  `Down`/`Up` and clicks are unchanged. (`src/ui/omnibox/`, `docs/INTERFACE.md`,
  `docs/NAVIGATION.md`.)
- `<Tabs items={[…]}>` labels reach the tab strip: the `items` array was
  ignored, so such tab sets fell back to `Tab 1…N`. The string-array attribute
  labels the `<Tab>` children with no `label`/`title` of their own (explicit
  labels win, nested sets use their own parent). The per-instance selection key
  also includes the `<Tabs>` marker's event index, so two sets with identical
  labels cannot share one selection (the "ghost" tab of the second list).
  (`src/markdown/document/`, `src/viewer/markdown/`, `docs/RENDERING.md`.)
- Headings no longer show their `[#anchor]` markers: the docs-generator
  convention (`## Title [#custom-id]`) is rewritten before parsing to the
  heading attribute `{#custom-id}`, which sets the explicit anchor id and never
  renders as text. Code, escaped markers and non-heading lines keep their
  literal `[#…]`. (`src/markdown/document/normalize.rs`, `docs/RENDERING.md`.)
- Right-clicking a text selection no longer destroys it: egui's label-selection
  plugin reads any press over a selectable label as the start of a drag, so the
  secondary press collapsed the selection. `raw_input_hook` intercepts that
  press before egui and delivers its release as a pending context click, so the
  selection and highlight survive and Copy copies exactly the selected text.
  (`src/app/run.rs`, `src/browser/tab/`, `src/viewer/overlays/`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Code blocks keep their rows while the page scrolls them toward the top edge:
  virtualization bounds its visible rect with the window-local
  `InputState::viewport_rect()` instead of `ViewportInfo::inner_rect` (MONITOR
  space, whose intersection with the window-local clip rect shifted the bound by
  the window's screen position — a window 300px down culled ~15 rows). Plain
  (huge/unsupported) blocks paint at the absolute row offset like the
  highlighted path, not `row - visible.start`, which parked them at the block
  top. (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- The hidden-code-lines notice no longer paints over the first code row: rows
  are painted, not laid out, so the layout cursor was still at the block's top
  and the notice landed on row 0 of any block past the line budget. It is
  advanced past the reserved body. (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- Two `<Accordion>`s with the same title (or two untitled ones, both shown as
  `Details`) no longer share one open/closed state: the state key is the
  marker's own event index + title, like the `<Tabs>` selection key. The title
  alone made toggling one section toggle the other.
  (`src/viewer/markdown/components.rs`, `docs/RENDERING.md`.)
- A `#fragment` whose target EXISTS but is not rendered in the current pass (an
  unselected `<Tab>` panel, a closed `<Accordion>`, past the block budget) stays
  pending instead of being dropped at the end of the frame, so opening that tab
  later still applies the request; only a target the anchor index does not know
  is cleared. (`src/viewer/markdown/mod.rs`, `docs/ARCHITECTURE.md`.)
- An inline `<Badge>` inside a heading renders as the same pill as in a
  paragraph, scaled with the heading (capped at the H3 step), not flattened into
  plain heading text with no pill and no accent.
  (`src/viewer/markdown/headings.rs`, `src/viewer/inline/mod.rs`,
  `docs/RENDERING.md`.)
- `Copy` strips the alert marker at ANY quote depth (`> > [!WARNING]`): the
  viewer renders the same badge at any depth; the clipboard pass peeled only one
  `>` prefix and pasted the marker. (`src/markdown/document/plain.rs`,
  `docs/RENDERING.md`.)
- A wide table can no longer bypass the per-frame block budget one row at a time
  (one row spends a single block, so a 4096-column table laid out every cell of
  every row, ~20M cells for 5 000 rows). Rendered cells are capped per document
  per frame (`config::MAX_RENDERED_TABLE_CELLS`); cells past it are consumed
  without fragments, with the usual truncation notice.
  (`src/viewer/markdown/walk.rs`, `src/viewer/markdown/budget.rs`,
  `src/config/mod.rs`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- The pre-parse table guard counts CELLS, not pipes: a header row with exactly
  `MAX_TABLE_COLS` pipes yielded `MAX_TABLE_COLS + 1` columns (one past the
  documented cap) and is rejected now. (`src/markdown/document/mod.rs`,
  `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- A same-frame `Ctrl+H` (or `Cmd+Y`) plus `Ctrl/Cmd+W` no longer closes the
  freshly opened History tab: the close target is captured before the
  History/Licenses tab is inserted and activated. (`src/app/shortcuts.rs`.)
- Bookmark `trim` no longer over-evicts: after the entry-cap drain the byte
  counter still counted the dropped entries, so the byte loop dropped extra live
  bookmarks to compensate for phantom bytes (silent data loss at the cap).
  (`src/bookmarks/persist.rs`.)
- A corrupt/unknown `restore_tabs` value keeps the DEFAULT (restore ON) instead
  of reading as "off": the off path deletes the encrypted `session.bin`, so a
  garbage value could destroy the visited-URL memory. Only an explicit
  `false`/`0` disables restore. (`src/settings/mod.rs`, `docs/SECURITY.md`.)
- A worker retired by a bounded settings/session `flush` keeps its last outcome:
  the error is parked and reported by `Prefs::tick` like a live worker's (a
  mid-session flush could drop a failed write with the worker).
  (`src/settings/file.rs`, `docs/ARCHITECTURE.md`.)
- A settings/session job that could not be QUEUED is reported against its own
  file: a session enqueue error landed in the settings slot (wrong Preferences
  card) while the snapshot was marked saved and never retried.
  `save_session_async` now returns `Err` when the job was refused.
  (`src/settings/file.rs`, `src/settings/session.rs`, `docs/ARCHITECTURE.md`.)
- A failed "Delete all data and close" no longer exits as if the data were gone:
  the app re-enables writes, shows the error in the Local data card and stays
  open (the `remove_dir_all` result was discarded). (`src/settings/platform.rs`,
  `src/app/mod.rs`, `src/ui/preferences/`, `docs/SECURITY.md`,
  `docs/INTERFACE.md`.)
- A failed session save now wakes the app when its retry window opens: the
  `SESSION_SAVE_RETRY_SECS` backoff was armed but nothing requested a repaint,
  so an idle window retried only on the next input event. (`src/app/run.rs`.)
- The table-bomb guard can no longer be hidden behind a QUOTED or list-item
  fence: the fence state only remembered its marker, so a fence opened after a
  quote marker or a list item made every following line count as code — an
  unquoted (or unindented) 4097-column header reached the eager parser
  unguarded. The state now carries the quote depth and the list item's content
  column, and a line that leaves the container ends the fence.
  (`src/markdown/document/normalize.rs`, `docs/SECURITY.md`.)
- A local image read failure (permission, vanished file, I/O error on a
  read-only mount) is FINAL, not transient: the same path and bytes come back,
  so the automatic retry only wasted a read. The verdict is scoped to `file:`
  targets; a NETWORK error keeps its retry even though it also carries an
  `(os error N)` suffix. (`src/media/cache/mod.rs`.)
- An ELAPSED Preferences delete confirmation is dropped when the dialog writes
  its state back: a stale `Some(Instant)` survived and re-armed the armed label
  on a later visit without a new click. (`src/ui/preferences/mod.rs`.)
- A favicon budget backoff can no longer keep an idle window repainting at 8 fps
  forever with the icon unresolved: `FaviconCache::poll` retries a backoff whose
  cooldown elapsed itself (its repaint is the only frame that origin gets) and
  past the attempt cap the entry FAILS, so it stops counting as pending.
  (`src/media/favicon.rs`, `docs/ARCHITECTURE.md`.)

### Performance

- A queue whose entries all wait for a texture-budget deadline is not re-walked
  per visible image per frame: `ImageCache::pump` remembers the earliest
  `not_before` and returns until it passes (a VRAM-tight gallery walked it up to
  512 x 512 times a frame). (`src/media/cache/mod.rs`, `docs/ARCHITECTURE.md`.)
- An HTML block's buffer is collected under the render cap (one char past it, so
  the `… [truncated]` notice still fires), not up to 4x the cap in bytes per
  frame before the renderer trimmed it. (`src/viewer/markdown/walk.rs`,
  `docs/ARCHITECTURE.md`.)
- `ImageCache::has_pending` is O(1) (`queue` + the cached `loading_count`)
  rather than rescanning every entry for every tab every frame (up to ~65k
  matches per frame with many tabs). (`src/media/cache/mod.rs`,
  `docs/ARCHITECTURE.md`.)
- A saturated favicon origin cache answers an unknown origin without
  `Url::parse` + key allocation per tile per frame (`FaviconCache`'s
  `at_capacity` flag, recomputed once per `poll`). (`src/media/favicon.rs`.)

## [0.4.1] - 2026-10-12

A hardening and standardization pass: fail-closed UTF-8 handling on the markup
path, allocation-free component scans, one document spacing scale, and the
root/docs version drift closed.

### Changed

- Standardization: test-only store writers (`settings::write_setting`,
  `settings::save_session_to`) keep one
  `#[cfg_attr(not(test), allow(dead_code))]` instead of a blanket allow, so the
  intent reads at the definition; the embedded-font corruption path funnels
  through a single `#[cold]` panic site. No behavior change. (`src/settings/`,
  `src/assets.rs`.)
- Standardization, no behavior change: the shared `theme::spacing` scale
  (`XS 4.0` / `S 6.0` / `M 8.0` / `L 12.0` / `XL 16.0`) backs heading air,
  footnote rules and the MDX tab-strip padding (same pixels; graphics stay
  pixel-identical). (`src/ui/theme/`, `src/viewer/markdown/`.)
- Perf, no behavior change: `viewer::markdown::components::find_close` compares
  close tags allocation-free (`is_close_tag`), not `format!`-ing `/kind` per
  scan step. (`src/viewer/markdown/components.rs`.)

### Fixed

- The markup pre-parse no longer indexes mid-character on hostile input:
  `decode_field`, `strip_line` and `strip_comment_spans` advance through
  `str::get` + `chars().next()` and stop fail-closed instead of `unwrap`-ing
  `s[i..]`, and the `<` fast path resolves its tag from the checked slice. No
  behavior change on valid documents.
  (`src/markdown/document/normalize_markup.rs`.)
- Docs sync: `SECURITY.md` supports `0.4.x`; `RENDERING.md` lists
  `<AccordionGroup>` and the rejected SVG `pattern`/SMIL (`animate`,
  `animateTransform`, `animateMotion`, `set`, `discard`) markup;
  `ARCHITECTURE.md` nests `protocol/resolver`, `protocol/content_type` and
  `protocol/rewrite` beside `protocol/client` and notes `<CodeGroup>` desugars
  to `<Tabs>`/`<Tab>`; `README.md` lists `about:home`. (`README.md`, `docs/`.)

## [0.4.0] - 2026-10-12

A consolidation release: two after-interaction rendering bugs are fixed (a code
card painting a sibling's body after a `<Tabs>` switch; heading anchors pointing
at the wrong heading), store writers cannot strand or silently lose their newest
blob, URL caps are byte-based on serialized URLs, and per-frame costs,
accessibility names and doc drifts are cleaned up.

### Added

- Native MDX components (`viewer::markdown::components` +
  `markdown::document::normalize_markup` markers): `<Tabs>`/`<Tab>` a clickable
  tab bar (only the selected panel lays out; the choice is remembered per
  document/tab), `<Accordion>` a collapsible section (toggled by its title,
  collapsed by default), `<Card>` a bordered box (a `title` becomes the first
  line, a link when `href` is set), `<Steps>`/`<Step>` numbered rows and
  `<CodeGroup>` a code tab per fenced child (label from the fence `title=`, else
  the language). Inline `<Badge>` is a small pill;
  `<CardGroup>`/`<Frame>`/`<Tiles>` are transparent wrappers. Tab and accordion
  controls keep a fixed geometry (hover never shifts the page). The normalizer
  carries canonical `<mdowser-cmp …/>` markers (blank-line separated so the
  Markdown between parses) and `<mdowser-badge>` inline pills; `plain_text`
  skips them and `Copy` includes every panel's text.
  (`src/viewer/markdown/components.rs`, `src/viewer/inline/mod.rs`,
  `src/markdown/document/normalize_markup.rs`, `docs/RENDERING.md`,
  `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- MDX and raw-tag handling in the Markdown pipeline
  (`markdown::document::normalize_markup`, run inside `normalize_body` before
  parsing): known components (`<Note>`, `<Tip>`, `<Warning>`, `<Caution>`,
  `<Important>`, `<Info>`, `<Danger>`, `<Check>`, `<Success>`, `<Hint>`,
  `<Attention>`, `<Error>` and the `type`/`title` containers
  `<Callout>`/`<Alert>`/`<Aside>`/`<Admonition>`) render as callouts; every
  other JSX/HTML tag is a transparent wrapper (tag dropped, content rendered as
  Markdown); `<img>` becomes a Markdown image and inline `<br>` a break. In an
  MDX document (`import`/`export` or a capitalized component), ESM statements
  and `{…}` expressions are removed; `<script>`/`<style>` regions are dropped
  and `<pre>`/`<textarea>` stay inert. Nothing is executed, imported or fetched;
  code spans and fenced/indented code are never touched.
  (`src/markdown/document/`, `docs/RENDERING.md`, `docs/SECURITY.md`,
  `docs/ARCHITECTURE.md`.)

### Changed

- MDX tab strips look like tabs: labels sit over a full-width rule with the
  active one underlined in the accent (not filled buttons); hover changes only
  colors/underline, so the fixed-geometry controls never shift the page.
  `<Badge>` pills use a faint accent tint as fill, the accent as ink and a
  hairline border, so they read in both themes. Callout bodies put paragraph air
  between blocks, tightening the box's bottom edge.
  (`src/viewer/markdown/components.rs`, `src/viewer/inline/mod.rs`,
  `src/viewer/markdown/{walk,blocks}.rs`.)
- Raw HTML tags are no longer painted as inert monospace text: block and inline
  tags are transparent wrappers, so exported HTML-wrapped content
  (`<div align="center">`, HTML badges wrapped in `<a>`/`<img>`, card links with
  `<h3>` titles) renders instead of leaking tags.
  (`src/markdown/document/normalize_markup.rs`.)
- HTML comments (`<!-- ... -->`) are invisible, exactly like in a browser: the
  block form no longer paints as inert monospace, and the inline form leaves no
  gap (`foo<!-- x -->bar` reads `foobar`), in the viewer and in `Copy`/plain
  text. This covers comments the pre-parse markup pass leaves literal
  (multi-line or unterminated); a literal `<!--` inside code is untouched.
  (`src/markdown/document/mod.rs`, `src/markdown/document/plain.rs`,
  `src/viewer/markdown/`, `docs/RENDERING.md`, `docs/INTERFACE.md`.)
- Serialized/derived URLs are capped in BYTES (`config::MAX_URL_BYTES`, the
  worst-case percent-encoding of a capped input): route candidates, the display
  URL, the landed redirect URL and the history/bookmark recordability gate no
  longer compare a CHAR count against a percent-encoded spelling. A valid
  under-cap address whose path encoded to more bytes than chars failed with an
  EMPTY "no candidate worked" log (and could be neither recorded nor
  bookmarked); the raw address input keeps its char cap. (`src/config/`,
  `src/protocol/`, `src/util.rs`, `src/settings/`, `docs/SECURITY.md`.)
- Accessible names for row-scoped actions: a history row's X announces the
  visited page, a bookmark tile's rename/remove announce the tile, and the
  Preferences dropdowns ("Keep visits", "Theme") announce what they set — they
  were N identical buttons and an unnamed combo box. All names are built lazily,
  so a settled frame allocates nothing while accesskit is off. (`src/ui/icons/`,
  `src/ui/history_page/`, `src/ui/bookmarks/`, `src/ui/preferences/`.)
- The toolbar's reserved width charges one item spacing per allocation, so the
  Preferences gear lands exactly on the panel's right edge (it stopped 2-4px
  short; the omnibox absorbs the difference). (`src/ui/navbar/`.)
- Turning "Restore tabs on startup" off deletes `session.bin` through the
  session writer as a discard job (a queued or in-flight snapshot cannot be
  renamed into place after the delete) and then verifies the result, so the URL
  ciphertext cannot outlive the preference. (`src/settings/`,
  `docs/ARCHITECTURE.md`.)
- `make doctor` stays runnable exactly when needed: the parse-time "cargo not
  found" error skips the `doctor` goal, so the diagnosis command the message
  points at does not fail with the same error. (`Makefile`, `docs/BUILDING.md`.)
- Docs sync: `INTERFACE.md` describes the raw-HTML transparent wrappers (the one
  inert case left is `<pre>`/`<textarea>` plus malformed markup), the Error
  screen section (families, Copy log, Retry cooldown) and the `MAX_TABS` toast;
  `RENDERING.md` calls `<CardGroup>` a transparent wrapper like
  `<Frame>`/`<Tiles>`, lists the extra callout aliases
  (`success`/`hint`/`attention`/`error`) and documents `+++` metadata blocks;
  `NAVIGATION.md` matches the omnibox Enter behavior; `ARCHITECTURE.md`'s
  image-retry policy matches the code (5xx/429/408 retry, other 4xx final).
  (`README.md`, `docs/`, `Cargo.toml`, `src/assets.rs`.)

### Fixed

- Editing the address field and pressing Enter no longer navigates to the
  popup's default highlighted row: typing a URL that matched the page already
  shown (e.g. trimming its path) "reloaded" that page and the field snapped
  back. Enter submits the typed address; the popup selection only wins after an
  explicit `Down`/`Up`/hover pick, and typing clears that pick.
  (`src/ui/omnibox/`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Selecting part of the address with the mouse works again: select-all-on-focus
  ran at the end of the press frame, re-anchoring every drag selection at the
  first character. A clean click still selects the whole address; a drag selects
  from the press point and following clicks place the caret. (`src/ui/omnibox/`,
  `docs/INTERFACE.md`.)
- Code blocks can never render a SIBLING's body: the per-document cache was
  keyed by a render-order index, and `<Tabs>`/`<Accordion>` change which blocks
  the walk renders, so switching a tab or opening an accordion made the newly
  visible card hit the old block's cache entry (body and syntax state). The key
  — and the horizontal scroll id — is now the block's `Start(CodeBlock)`
  EVENT index, and its token spans fill lazily so a block first rendered while
  the per-document line budget was spent highlights as soon as a frame can show
  it. (`src/viewer/code/`, `src/viewer/markdown/`, `docs/ARCHITECTURE.md`.)
- Heading anchors no longer shift when the walk skips content: ids came from a
  render-order list, so an unselected `<Tab>` panel, a collapsed accordion or a
  budget-dropped block handed every later heading a sibling's target (an
  in-document `#link` scrolled to the wrong heading or nowhere).
  `AnchorIndex::heading_id_at` looks an id up by the heading's OWN event index.
  (`src/markdown/anchors.rs`, `src/viewer/markdown/`, `src/viewer/mod.rs`,
  `docs/ARCHITECTURE.md`.)
- Code blocks scrolled well into a long listing show no growing blank band at
  the top: the virtualized viewport divided by the 19.6px design token while
  epaint rounds every row to a whole device pixel (20.0 at scale 1), so the
  computed first visible row sat past the real one. The viewport math, the
  reserved height and the painted offsets share the measured row stride.
  (`src/viewer/code/`, `docs/ARCHITECTURE.md`.)
- A table overflowing by 1-2px shows its horizontal bar inside the reserved lane
  instead of covering the last row: the overflow test now uses the inner
  `ScrollArea` width (the card frame's inner area), like the lane.
  (`src/viewer/table/render.rs`.)
- A background `session.bin` write failure no longer passes silently: only
  failures caught at enqueue time were reported, so a full disk lost the
  restored tab list while Preferences stayed quiet. `Prefs::tick` drains each
  worker into its own slot (`session_save_error` is separate, so a successful
  session save cannot clear a settings warning and vice versa).
  (`src/settings/`, `src/app/`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- A store writer can no longer strand a job: `push` accepted jobs for a worker
  that had already decided to exit, leaving the newest blob — or a "Delete
  history" discard, whose ciphertext then survived — in a queue nobody reads.
  The worker publishes its exit under the job lock, `push` returns the job
  instead of swallowing it, and `store::writer::queue_or_respawn` replaces the
  worker and retries; the discard also sweeps the store's `.bak` and `.tmp.*`
  siblings. (`src/store/writer.rs`, `src/settings/`, `src/history/`,
  `src/bookmarks/`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- A relative `file:` spelling is rejected wherever it appears: `file:notes.md`
  (no leading slash) was parsed by the WHATWG rules as `/notes.md`, silently
  opening a file at the filesystem root, on navigation and through a `file:`
  link inside a local document. Both the address parser and the link rewrite
  require the absolute spelling (`file:/x`, `file:///x`, `file://localhost/x`).
  (`src/protocol/local.rs`, `src/protocol/rewrite.rs`, `docs/NAVIGATION.md`,
  `docs/SECURITY.md`.)
- `ui::forget_tab` really drops a closed tab's scroll offsets: a `ScrollArea`
  derives its persistent id from the parent `Ui` (`make_persistent_id`), so the
  five `Id::new((salt, tab_id))` removals never matched and every closed tab
  leaked one scroll state per surface. Each surface records the real id it got
  back (`ui::remember_scroll`) and the registry removes that. (`src/ui/`,
  `src/viewer/`, `docs/ARCHITECTURE.md`.)
- The omnibox capsule paints its fill and focus ring AFTER the field with the
  fresh focus state, so a pointer press shows the ring in the same frame, and
  the right slot is reserved from `bookmark || focused || !address.is_empty()`,
  so on Home the text area's right edge no longer shifts 31px on the frame the
  empty field gains focus. Suggestion rows share the popup's own 80px width
  floor (the row's 120px floor could grow the frame wider than a very narrow
  capsule). (`src/ui/omnibox/`.)
- Row highlights survive their own action buttons: the history row and the
  expanded tab row test `contains_pointer` like the bookmark tile, so the wash
  no longer blinks off when the pointer reaches the row's X (egui's hover set
  holds only the topmost widget). (`src/ui/history_page/`, `src/ui/tabs/`.)
- The favicon string-key fast path refuses a non-lowercase authority
  (`https://EX.com`), which keyed a different origin than the parser's lowercase
  one, splitting one site into two cache entries and two probes.
  (`src/media/favicon.rs`.)
- `settings.bin` can no longer be written past what the reader accepts: a
  read-modify-write that would cross `MAX_SETTINGS_BYTES` is refused (it was
  rewritten one key over budget, then silently fell back to defaults on the next
  launch), and every store reader derives its file cap from
  `crypto::OVERHEAD_BYTES`. (`src/settings/`, `src/history/`,
  `docs/SECURITY.md`.)

### Performance

- A settled table costs no byte scan per frame per table: the content
  fingerprint memo is keyed by the table's `Start` event index (stable whatever
  the frame renders), not re-validated with a probe of the whole table text.
  (`src/viewer/table/`, `docs/ARCHITECTURE.md`.)
- The widest code line is computed when the line galleys are built, not from the
  visible rows each frame, so the horizontal scroll range (and the clamped
  offset) no longer shrinks while a long line is scrolled out of view.
  (`src/viewer/code/`.)
- The error view allocates much less per frame: the scheme-hiding helper returns
  `Cow` (borrowed when nothing is stripped), the local-file check no longer
  lowercases the whole address to test a prefix, and an oversized HTML block's
  truncation has a byte fast path and builds one `String` instead of
  `format!`-ing a second copy. (`src/util.rs`, `src/ui/error_view.rs`,
  `src/viewer/markdown/html.rs`.)

## [0.3.4] - 2026-10-12

This release serializes store deletes behind the writer, keeps failed session
snapshots pending for retry, and preserves requested tab URLs across restores.
It also virtualizes code blocks, adds real bold text and footnotes, and hardens
credential handling, store permissions and SVG filter rejection.

### Added

- Store deletes: `store::writer::WriteJob::discard` and `StoreWriter::discard`
  run a wipe ("Delete history now", forget-on-exit) on the store's writer worker
  behind any in-flight write, so a stalled worker cannot rename its temp blob
  and resurrect deleted ciphertext (`src/store/writer.rs`, `src/history/`,
  `docs/ARCHITECTURE.md`, `docs/SECURITY.md`).
- Session saves: `config::SESSION_SAVE_RETRY_SECS` keeps a failed save's
  snapshot PENDING and retries on a 5s backoff; with no data directory, no error
  string is allocated per frame and the snapshot is not recorded as saved
  (`src/app/`, `src/config/mod.rs`).
- Tab URLs: `Tab::session_url` (with `Tab::session_target`, `app::session`)
  remembers a restored tab's requested URL while its load is in flight or failed
  instead of serializing the `None` Home marker, which lost the URL from
  `session.bin` on the next save (`src/browser/tab/`, `src/app/session.rs`).

### Changed

- Code blocks: `viewer::code::code_viewport` builds and paints only the rows in
  the scroll viewport on a uniform measured row grid within the per-document
  budget (once one `LayoutJob`/`Label` per row of a 10 000-line log, ~20 rows on
  screen); highlighted rows are painted galleys, so a code line cannot be
  drag-selected inside one and the Copy button (with select-all + Ctrl/Cmd+C)
  copies the whole block (`src/viewer/code/`, `docs/ARCHITECTURE.md`).
- Table widths: the content fingerprint is memoized per
  `(doc_salt, table index)` and validated by a cheap content probe
  (`viewer::table::cached_fingerprint`), so measurements no longer re-hash the
  table text each frame and an edited table re-measures (`src/viewer/table/`,
  `docs/ARCHITECTURE.md`).
- `ui::error_view::classify` is allocation-free: a per-line case-insensitive
  scan replaces `join` + `to_ascii_lowercase` of the whole log every frame
  (`src/ui/error_view.rs`).
- `settings::cleanup_legacy_files` leaves a temp file alone while its creator
  runs (pid from the `<file>.tmp.<pid>.<seq>` name), so a second instance's live
  write survives the legacy/crash sweep (`src/settings/platform.rs`).
- Reload button: the accessible name is the static `"Reload"` in every state
  (`src/ui/navbar/`).
- Omnibox capsule: `Sense::CLICK` senses clicks without joining the keyboard tab
  order and draws no focus ring, removing the silent focus stop before the
  address field (`src/ui/omnibox/`).
- Tab ids: closing the LAST tab allocates a fresh id instead of reusing the
  closed one, preserving the "ids are never reused" invariant the egui-memory
  cleanup relies on (`src/app/mod.rs`).
- `ui::forget_tab` also drops the Home, bookmarks and history-page scroll states
  and the error view's scroll state (`src/ui/`).
- Footnote back-references carry the document generation, so a later document
  reusing a `#fn-...` label cannot scroll to the previous document's painted
  rect (`src/viewer/inline/`, `src/viewer/markdown/walk.rs`).
- Address autocomplete: the builtin list grows from 9 to 82 curated entries
  (developer-service and docs sites plus the agent-ready and personal-site
  lists) and moves to `src/builtin_sites.rs`, imported by `src/suggest.rs`
  (`src/builtin_sites.rs`, `src/suggest.rs`).
- Corner radius: `ui::theme::radius` and `ui::theme::focus_ring` give cards,
  pills, tiles, dialogs and chrome icon buttons one shared scale instead of
  hard-coded `3/6/8/10/12` values (`src/ui/theme/`, `src/ui/icons/`,
  `src/viewer/image.rs`, `src/viewer/inline.rs`).
- Address fields: both address fields and the `about:history` search field get
  accessible names ("Address" / "Search history"), not only hint text
  (`src/ui/omnibox/`, `src/ui/history_page/`, `docs/ARCHITECTURE.md`).
- Viewer caches: `viewer::cache::evict_oldest` provides one shared LRU eviction
  pass for the code-block and table-width caches (`src/viewer/cache.rs`,
  `src/viewer/code.rs`, `src/viewer/table.rs`, `docs/ARCHITECTURE.md`).
- Unreadable stores: `settings::preserve_blob_bytes` makes `.bak` copies with
  `create_new` (no clobber, no exists-then-rename TOCTOU) and leaves the live
  file in place (`src/settings/`, `src/history/`, `src/bookmarks/`,
  `docs/SECURITY.md`).
- Chrome tokens: `config::ROW_TITLE_CHARS/ROW_URL_CHARS`,
  `config::DELETE_CONFIRM_SECS`, `config::CHROME_*`, `theme::radius::MENU/FULL`
  and `theme::pill_radius` give suggestion and history rows, delete
  confirmations, the tab rail, navbar, dialogs and viewer pills one scale
  (`src/config/`, `src/ui/theme/`, `src/ui/row.rs`, `src/ui/omnibox/`,
  `src/ui/preferences/`, `src/ui/history_page/`, `src/ui/navbar/`,
  `src/ui/tabs/`, `src/ui/bookmarks/`, `src/viewer/`).
- Image settling: `TabState::Settling` and `config::DOC_IMAGE_GATE_SECS` preload
  a fetched page's images in a separate cache while the previous page (or a
  stable empty panel on first load) keeps the loading indicator, swapping
  atomically when every image settles or 5s pass, so text no longer jumps;
  text-only pages show instantly, while `Stop`/new navigations keep the backdrop
  (`src/browser/tab/`, `src/app/`, `src/viewer/mod.rs`, `src/config/`,
  `docs/ARCHITECTURE.md`, `docs/RENDERING.md`).
- Bold text: `theme::strong` and `theme::strong_family` name a family whose
  first face is the bundled Noto Sans variable font pinned to `wght = 700` via
  `FontTweak::coords` (epaint 0.36 has no weight axis in `FontId`) and inherits
  the proportional fallbacks; headings, `**strong**` runs, callout titles,
  footnote refs, the Preferences title and section labels, error-view headings
  and the Home wordmark are visibly heavier, not color-only (`src/ui/theme/`,
  `src/viewer/`, `src/ui/error_view.rs`, `src/ui/preferences/`,
  `docs/RENDERING.md`).
- Footnotes: a `[^label]` reference renders as a small raised NUMBER marker
  linked to its note (never the `[^label]` spelling), numbered in
  first-reference order (`[^note]` before `[^2]` shows 1 then 2; repeats keep
  their number, `AnchorIndex::footnote_number`). Notes form one section with a
  single 1px rule before the FIRST definition; each note is a numbered entry
  whose marker shares the body's first text line (`1. text`; a first block that
  cannot flow inline keeps the marker above), with a small vector back-arrow
  (`Icon::Back`) scrolling to the painted reference. Unreferenced definitions
  still render and continue the numbering , and `Copy` keeps the source spelling
  (`src/markdown/anchors.rs`, `src/viewer/markdown/`, `src/viewer/inline/`,
  `docs/RENDERING.md`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`).
- Typeface: bundled Noto Sans leads the proportional family (chrome, menus,
  documents) and its true-monospace cut (Noto Sans Mono CJK JP, face 5 of the
  CJK collection) leads the monospace family; epaint's default faces are no
  longer registered in the UI stack or loaded into the SVG font database, and
  SVG text defaults and generic families resolve to the same bundled faces
  (`serif`/`sans-serif`/`cursive`/`fantasy` to Noto Sans, `monospace` to the
  mono cut). Regular and bold share one face's metrics, the bold face keeping
  the pinned `wght = 700` Noto Sans bytes without a separate `y_offset_factor`
  tweak (`src/ui/theme/`, `src/media/decode/svg.rs`, `src/assets.rs`,
  `docs/RENDERING.md`, `docs/ARCHITECTURE.md`, `docs/INTERFACE.md`,
  `THIRD_PARTY_NOTICES.md`, `deny.toml`).
- Source layout: domain folders at most two levels deep: files split by
  function, unit tests in sibling `tests*.rs` modules. `ui::viewer` becomes
  top-level `viewer/` (with `viewer/markdown`, `viewer/table`, `viewer/inline`),
  shared encrypted-store plumbing moves to `store/` (`store::crypto`,
  `store::writer`), and `ui::history` becomes `ui::preferences`. Submodules:
  `browser::tab` (`state`/`poll`/`nav`/`nav_moves`), `media::decode`
  (`raster`/`svg`), `protocol::client` (`get`/`fetch`), `markdown::autolink`
  (`scan`), `markdown::document` (`normalize_links`/`normalize_tables`),
  `viewer::markdown` (`walk`/`blocks`/`headings`/`tables`/`html`),
  `viewer::table` (`render`), `viewer::inline` (`code_span`), `ui::theme`
  (`widgets`), `ui::tabs` (`rows`), `ui::bookmarks` (`tile`) and
  `ui::preferences` (`widgets`); `config`, `ui::icons`, `ui::navbar`,
  `ui::omnibox` and `ui::history_page` keep one logic file with tests in
  `tests.rs`. No logic, format or behavior changes (`src/`,
  `docs/ARCHITECTURE.md`, `docs/INTERFACE.md`, `Cargo.toml`).
- Startup: "Restore tabs on startup" is ON by default, so a fresh profile with
  no stored `restore_tabs` value reopens the last session; an explicit `false`
  stays off (`src/settings/`).
- Preferences "Default zoom": uses the toolbar's `- % +` capsule, and clicking
  the readout resets to 100% (`src/ui/navbar/`, `src/ui/preferences/`).
- Zoom range: now 75-175% in 25% steps, down from 50-300% (`src/config/`,
  `docs/INTERFACE.md`).
- Dependencies: `tokio` 1.53.1 → 1.53.2 via `cargo update -p tokio`;
  `skrifa`/`harfrust` stay pinned by `eframe 0.36`.
- Refactor: `markdown/mod.rs` shrinks by pure moves (block-budget helpers to new
  `markdown/budget.rs`, list walker to `markdown/items.rs`); the tab rail
  returns `TabsResponse`, mirroring `OmniboxResponse`; closed-tab cleanup is one
  registry, `ui::forget_tab`, which also drops the address/search `TextEdit`
  states that leaked from the persisted store, one per closed tab. No behavior
  change (`src/viewer/markdown/`, `src/ui/tabs/`, `src/app/`, `src/ui/mod.rs`,
  `src/ui/omnibox/`, `src/ui/history_page/`, `docs/ARCHITECTURE.md`).
- Home: the capsule floor grows 80 → 90px (`src/ui/home.rs`).
- Refactor: `theme::apply` splits into `apply_spacing_type` and `apply_visuals`;
  failed navigations share `Tab::restore_failed_navigation` instead of
  duplicating six statements; `navbar` reads zoom limits from `config` directly.
  No behavior change (`src/ui/theme/`, `src/browser/tab/mod.rs`,
  `src/ui/navbar/`, `docs/ARCHITECTURE.md`).
- Graphics: linked inline and block images paint the shared keyboard focus ring;
  status icons stay intentional (`Loading` status vs `Reload` action, `Image`
  unavailable vs `BrokenImage` failed) (`src/viewer/image.rs`,
  `docs/INTERFACE.md`).
- Graphics: the floating scrollbar paints no track; the reserved
  `theme::scrollbar_lane` keeps the handle alone; the focused omnibox ring is
  painted INSIDE the capsule instead of bleeding 2px into neighbouring toolbar
  buttons; the tab rail's active bar, the omnibox progress sweep and the `<hr>`
  bar share `theme::radius::HAIRLINE`; rail, history, dropdown, suggestion and
  bookmark rows share `theme::focus_ring`, and the bookmark tile no longer uses
  a 0.5px vs 1px inset (`src/ui/theme/`, `src/ui/omnibox/`, `src/ui/tabs/`,
  `src/ui/preferences/`, `src/ui/history_page/`, `src/ui/row.rs`,
  `src/ui/bookmarks/`).
- `about:history`: the "Local, encrypted. Newest first." subtitle is removed and
  the delete footer gains 8px of breathing room (`src/ui/history_page/`).
- Preferences and session writes no longer `fsync` on the UI thread:
  `Prefs::set_*`, the per-change session save in `App::logic` and the history
  retention key update an in-memory body and queue the encrypted write on
  dedicated background workers (`settings::write_setting_async`,
  `save_session_async`), with settings plaintext cached per directory; the exit
  path drains both workers and only a `restore_tabs` toggle waits briefly
  (bounded) for durability (`src/settings/`, `src/history/`, `src/app/`,
  `docs/SECURITY.md`).
- Refactor: `theme::mix` and `theme::tint` are the only color-blending helpers
  `TabStateKind` names the state discriminant instead of four inline matches;
  `theme::weak` drops its no-op `.weak()` flag, since an explicit color always
  wins. No behavior change (`src/ui/theme/`, `src/ui/preferences/`,
  `src/browser/tab/mod.rs`).
- Chrome ramp: `Scheme::container_hover` and `Scheme::container_active` replace
  two near-identical literals that drifted by 1/255; flat tonal buttons keep a
  transparent 1px `bg_stroke` instead of `Stroke::NONE` (the "one stroke width
  in every state" invariant: `Frame::total_margin` adds the width back); the
  code card exposes `theme::CODE_PAD_X` instead of a `34.0` literal (`16+16+2`);
  `content_max_width`, heading-scale and code-font-size comments now state what
  the code does (`src/ui/theme/`, `src/viewer/code/`,
  `src/viewer/markdown/headings.rs`).
- Inline code pills are height-capped at `viewer::inline::MAX_PILL_H`, the body
  line minus 3px: the bundled mono cut's em box is taller than a typical UI
  face, so an uncapped pill grew to a 24px line box; the cap shrinks the box
  about its center, keeping glyphs optically centered (`src/viewer/inline/`,
  `docs/INTERFACE.md`).
- Hardening: `Tab::display` falls back to a static empty display instead of an
  `expect`, and the test-only `Tab::title`, `window_title` and `favicon_origin`
  accessors are `#[cfg(test)]`, with production reading the display memo only.
  No behavior change (`src/browser/tab/`).
- Docs: `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/RENDERING.md` and
  `docs/INTERFACE.md` describe the rendering, storage and rendering-safety
  behavior above (bold family, body text token, quoted code fences, CSS filter
  spellings, unreadable-settings refusal, handle-based permission tightening,
  credential-free destinations, write suppression on wipe).
- License: MDowser moves from `MIT OR Apache-2.0` to Mozilla Public License 2.0
  (`MPL-2.0`, file-level weak copyleft); `THIRD_PARTY_NOTICES.md` still covers
  every bundled crate and font. One `LICENSE` with the MPL 2.0 text replaces
  `LICENSE-MIT`/`LICENSE-APACHE`; every `.rs`/`.sh` file opens with the brief
  Mozilla Exhibit-A header plus the copyright line (rule in
  `docs/CONTRIBUTING.md`), and the crate metadata, README badge and license
  section, Preferences footer and artifacts follow (`Cargo.toml`, `LICENSE`,
  `README.md`, `THIRD_PARTY_NOTICES.md`, `deny.toml`, `src/`, `build.rs`,
  `build_support/`, `scripts/`, `docs/INTERFACE.md`, `docs/BUILDING.md`,
  `docs/CONTRIBUTING.md`).

### Fixed

- Scrollbar contrast: the handle now paints `fg_stroke`
  (`foreground_color = true`), the ramp's border tone, at 0.75 dormant opacity,
  pinned by a contrast test (`src/ui/theme/`, `src/ui/theme/tests.rs`), instead
  of `widgets.inactive.bg_fill` at ~1.1:1.
- Theme tokens: `widgets.open` now sets `weak_bg_fill`, `bg_stroke` and
  `fg_stroke`, so the open Preferences ComboBox no longer paints epaint's
  default gray fill, `gray(160)` border and gray text (`src/ui/theme/mod.rs`).
- Preferences warning: `Prefs::tick` drains only the settings worker's outcome
  (session failures surface through `save_session`'s own error), so a successful
  session write cannot clear a settings-write failure (`src/settings/file.rs`).
- History on stop: `settle_abandoned` takes `pushed_history`, so Stop and the
  visual timeout keep the pushed history entry like a failed fetch instead of
  restoring the pre-navigation snapshot and dropping the page the user came from
  (`src/browser/tab/`, `docs/ARCHITECTURE.md`).
- Autolink guard: the domain label immediately before the extension-like TLD is
  read instead of the host's first label, so `blog.readme.md` stays text (it
  ends in the file name `readme.md`) while `readme.blog.md` links as a host
  under `.md` (`src/markdown/autolink/scan.rs`, `docs/RENDERING.md`,
  `docs/ARCHITECTURE.md`).
- Preferences header: the title keeps equal air above and below (symmetric 12px
  padding, a vertically centered 32px lane, a flush header/divider/scroll stack)
  (`src/ui/preferences/`).
- macOS Intel build: `[lints.rust] linker_messages = "allow"` in `Cargo.toml`
  silences ~46 benign `ld: no platform load command found` notes from `rav1d`'s
  NASM-assembled x86_64 objects (no `LC_BUILD_VERSION`), until NASM/`rav1d` emit
  the load command (`Cargo.toml`, `docs/BUILDING.md`).
- Tab rail docs: the doc comment lists the real favicon cascade
  (`config::FAVICON_PATHS`: `/favicon.ico`, `/favicon.png`, `/favicon.svg`)
  (`src/ui/tabs/`).
- Corrupt-key repair: `acquire_repair_lock` gives up at once on a stale
  `<store.key>.repair` that cannot be removed (a directory squatting the name,
  an immutable file, a permission quirk) instead of re-observing it forever on
  the UI thread (`src/store/crypto/`).
- Directory fallback: a local `index.md` with malformed frontmatter or an
  over-budget parse falls through to `README.md` exactly like the network route
  fallback instead of ending the cascade with a content error
  (`src/protocol/local.rs`).
- Favicon backoff: `FaviconCache::has_pending` counts `BudgetWait`, so the
  repaint scheduler stays awake (`src/media/favicon.rs`).
- Image failures: classification matches the real decoder strings; the dead
  `svg: filter elements...` and `svg: DTD...` markers are gone, the
  deterministic SVG verdicts (`CSS filter declarations`,
  `duplicate element ids`) and credential/over-long redirect blocks are final
  with no pointless retry (`src/media/cache/`).
- Store writer: `StoreWriter::worker_dead` (the thread exited or panicked
  outside a guarded write) makes `save_async` and `flush_writer` replace a dead
  worker instead of waiting forever, and `Drop` asks a live worker to stop and
  joins only one that already finished, so exit never blocks
  (`src/store/writer.rs`, `src/history/`, `src/bookmarks/`).
- Context menu: opening with the pointer leaves NO row focused; the first
  `Down`/`Up` moves focus into the rows (the first press lands on the first row;
  egui's arrow traversal is cancelled), `Enter` activates the focused row, `Esc`
  closes, and a primary press outside the popup (including a drag-selection
  start) closes it, not only a completed click; Copy, Select all and "Open link
  in new tab" still need no pointer (`src/viewer/overlays/`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`).
- Suggestion highlight: a non-query store-generation rebuild now clamps the old
  selection to the possibly shorter list, and only a new query starts at the
  top; the clamp branch was dead after an unconditional reset, which sent the
  highlight to the first row when a page was bookmarked or removed with the
  popup open (`src/ui/omnibox/mod.rs`).
- Docs: the root README links `docs/SECURITY.md` under its own label;
  `docs/ARCHITECTURE.md` lists the `Settling` tab state and `bookmarks/` module
  path and the bookmark-tile icon order of `ui::bookmarks::tile` (custom glyph →
  favicon → letter avatar); `docs/INTERFACE.md` quotes the real retention labels
  ("Keep 1 year", not "12 months"); the `THIRD_PARTY_NOTICES.md` font-stack note
  says `viewer::bidi` (moved out of `ui::viewer`) (`README.md`, `docs/`,
  `THIRD_PARTY_NOTICES.md`).
- Body text: `Visuals::text_color()` falls back to
  `widgets.noninteractive.fg_stroke`, which the theme set to
  `on_surface_variant` , so paragraphs, list items, table cells and inline code
  pills rendered `#59636e`/`#9198a1` while bold runs, headings and bullets
  rendered `on_surface`; the theme now sets `override_text_color = on_surface`,
  and muted text opts in explicitly (`theme::weak`, quote bodies), so the quote
  box renders muted and body text meets the palette's WCAG target
  (`src/ui/theme/`, `src/viewer/`).
- Tab rail title: `navigate_to_url` bumps `doc_generation` when it replaces an
  in-flight load, so a superseding `Loading` → `Loading` navigation no longer
  shows the old page's title and favicon for the whole second load (the memo is
  keyed on state kind and generation) (`src/browser/tab/`).
- Tab rail keys: the settled-collapsed branch returns the actions the collapsed
  rail collected instead of a fresh `TabsResponse::default()`, so
  `Enter`/`Space` on a focused tab or action row is not dropped
  (`src/ui/tabs/`).
- Reload tooltip: the disabled tooltip says "Nothing to reload" when no
  countdown exists, instead of "Reload (available in 0s)" forever on Home and
  fresh local rejections; the countdown is built lazily on hover instead of
  every frame (`src/ui/navbar/`).
- Zoom readout: the `100%` click-to-reset control now has a keyboard focus
  indicator like every other hand-painted chrome control (`src/ui/navbar/`).
- Settings write: writing refuses when `settings.bin` exists but cannot be read
  or decrypted; treating it as an EMPTY body let the next preference change
  rewrite one key and drop the rest (theme, zoom, restore-tabs, retention) for
  the session, the exact failure `settings_file_error` exists to prevent
  (`src/settings/`).
- Background writes: writers respawn a stopped-and-drained worker, so the queue
  cannot strand the newest `settings.bin`/`session.bin` blob; a background write
  failure is now surfaced, `Prefs::tick` drains every finished write's outcome
  into the Preferences warning (cleared only on confirmed success), and a failed
  session snapshot is not recorded as saved, so it retries and surfaces
  (`src/settings/`, `src/app/`).
- Bookmark budget: `set_title` refuses a rename that would push the serialized
  store past `MAX_BOOKMARK_STORE_BYTES`, where the next save's `trim` silently
  dropped the OLDEST bookmark; the first-run seed now updates byte accounting,
  so the first save no longer trims the just-seeded store by one entry
  (`src/bookmarks/`).
- Delete all data: the wipe suppresses every later store write
  (`settings::suppress_writes`) and drains the writers before removing the
  directory, so a background writer that already dequeued its blob cannot
  recreate the data directory after the wipe (`src/app/`, `src/settings/`).
- Quoted code fences: `fence_update` strips the blockquote prefix first, so
  `> ``` ` is visible to pre-parse passes; the table-bomb guard no longer counts
  pipes
  inside QUOTED CODE as cells (rejecting legitimate documents with a "4096 table
  columns" error) and the link-join mask no longer treats code lines as prose
  (`src/markdown/document/`).
- Bare-URL candidates: the destination validator rejects credentials, so
  `https://example.com.@evil.com` or `www.example.com.@evil.com` no longer
  linkifies to host `evil.com`; the trailing dot slipped past the one-byte `@`
  email guard and the click landed elsewhere than the visible label
  (`src/markdown/autolink/`).
- Link rewriting: `protocol::rewrite` keeps credential-bearing `https:`
  destinations inert, both absolute (`https://user:pass@host/x`, which kept its
  userinfo) and scheme-relative (`//user:pass@host/x`, which only checked the
  scheme) (`src/protocol/rewrite.rs`).
- Bold baseline: the single-face stack (regular and bold are the same Noto Sans
  bytes at two weights, so their metrics agree) fixes bold runs sitting 2px low
  at body size (1px at small marker size), since epaint lays each section on its
  own font's `ascent + half the line gap`; pinned by
  `bold_runs_share_the_body_baseline` (`src/ui/theme/`,
  `src/viewer/markdown/tests_layout.rs`).
- Delete button: `DELETE_W` grows to 168px, so the armed "Delete history now" no
  longer grows 0.7px or overflows the fixed 164px slot,
  (`src/ui/history_page/`).
- Back reference: the arrow is the chrome's monochrome `Icon::Back` vector with
  a hover fill, the shared focus ring, an accessible name and a tooltip, not `↩`
  (U+21A9), which the emoji fallback face drew as a boxed return key
  (`src/viewer/markdown/`).

### Security

- Key file reads: `crypto::read_key_problem` opens with `O_NONBLOCK`, re-checks
  the opened handle and reads at most 33 bytes (oversize detection without
  allocation), closing the last uncapped local `read` and its metadata/open
  TOCTOU (`src/store/crypto/`, `docs/SECURITY.md`).
- `file:` URLs: history/bookmark stores (`util::is_recordable_url`) and the
  session loader (`settings::load_session_from`) reject credentials, ports and
  remote hosts exactly like `protocol::local::parse_file_url`, while
  `file://localhost/` stays local (`src/util.rs`, `src/settings/`,
  `docs/SECURITY.md`).
- Redirects: userinfo is stripped from the landed URL before display, history or
  further hops (fail-closed if it cannot be removed), and collected
  `Content-Type` values are capped at 8; route candidates and display URLs
  re-enforce `MAX_URL_INPUT_CHARS` after `.md`/index expansion, and session
  writes store over-long tab URLs as Home instead of unbounded values
  (`src/protocol/client/`, `src/protocol/resolver.rs`, `src/settings/`,
  `docs/SECURITY.md`).
- Store permissions: world-readable restored stores are tightened on load
  (group/other bits stripped, owner bits never widened); data and key
  directories tighten the same way (a `0500` admin-hardened dir is not widened
  to `0700`); a symlinked `store.key` is refused; crash-orphaned `<store>.tmp.*`
  files are swept for all four stores; SVG rejects duplicate `id`s fail-closed
  (a first- vs last-wins parser divergence would undercount a `<use>` bomb) and
  checks CSS `filter:` in every `<style>` element regardless of `type`
  (`src/settings/`, `src/store/crypto/`, `src/media/decode/`,
  `docs/SECURITY.md`).
- Network entry: `protocol::client::get_bytes` strips URL authority credentials
  itself, fail-closed, before sending anything, so "credentials never reach the
  wire" no longer depends on every caller going through `parse_navigable`
  (`src/protocol/client/`, `docs/SECURITY.md`).
- Bookmark budget: `toggle` refuses an entry that would push the serialized body
  past `MAX_BOOKMARK_STORE_BYTES` (a URL spelling can serialize larger than its
  char count); a body over budget from disk is trimmed oldest-first on load; the
  encrypted-file read cap derives from the plaintext budget plus
  `crypto::OVERHEAD_BYTES` (`src/bookmarks/`, `src/config/`,
  `src/store/crypto/`, `docs/SECURITY.md`).
- SVG filters: the scan skips comments and whitespace between the property name
  and `:` (`style="filter/**/:blur(5px)"`, which `simplecss` reads as a `filter`
  declaration) and accepts the IE star hack (`*filter:blur(5px)`), rejecting
  every spelling the CSS parser accepts and closing the last bypass of "no
  filter can ever run" (`src/media/decode/`, `docs/SECURITY.md`).
- Store mode: a store restored world-readable is tightened through the already
  opened handle (`fchmod`) instead of a fresh path lookup, and a
  credential-bearing `settings.bin` or store path is never `chmod`ed through a
  swapped-in symlink (`src/settings/`).
- Key temps: crash-orphaned `store.key.tmp.<pid>.<seq>` files, which hold RAW
  key bytes, join the orphan sweep, so a crash between the private temp write
  and its `hard_link` no longer leaves key material on disk (`src/settings/`,
  `docs/SECURITY.md`).
- `XDG_DATA_HOME`: a relative value is ignored as the XDG spec requires an
  absolute path, so stores and `wipe_all_data`'s `remove_dir_all` cannot resolve
  under the current working directory (`src/settings/platform.rs`).
- Session restore: URLs with credentials are rejected on every scheme (only
  `file:` was checked), so a tampered session file cannot seed a
  credential-bearing URL into restore (`src/settings/`).
- Credential errors: `get_bytes`' fail-closed "credentials (blocked)" error
  reuses `util::strip_credentials_for_log` instead of interpolating the
  credential-bearing URL it just failed to sanitize, and the credential-free
  request path no longer clones the URL (`src/protocol/client/`, `src/util.rs`).
- Atomic writes: `atomic_write` refuses every write once the data directory was
  wiped (`settings::suppress_writes`), and "Delete all data and close" sets it
  before draining, so a worker draining its queue cannot resurrect a deleted
  store (`src/settings/`, `src/app/`).

### Performance

- Code chips: the code-block language chip no longer allocates per block per
  frame when the tag is already lowercase, and H6 headings keep readable
  select-all colors (`src/viewer/code.rs`, `src/viewer/markdown/blocks.rs`,
  `src/viewer/inline.rs`).
- Media tables: image and emoji tables are cached like text tables, keyed by new
  geometry epochs (`ImageCache::resolved_epoch`, `EmojiCache::resolved_epoch`),
  so resolution, evict or registration re-measures once; image URLs join the
  table fingerprint so swapped sources cannot reuse another image's widths
  (`src/media/cache/`, `src/media/emoji/`, `src/viewer/table.rs`,
  `docs/ARCHITECTURE.md`).
- URL and text handling: `strip_scheme_prefix` uses a case-insensitive prefix
  check with `Cow` borrowed on the common path instead of lowercasing the URL
  per row per frame; `truncate_chars` returns `Cow`; theme-twin URL tokens
  compare borrowed via `eq_ignore_ascii_case`; the Failed retry button keeps a
  static label with its countdown in a lazy tooltip; bookmark tile edit/drag
  state persists only on change or while open (`src/util.rs`,
  `src/viewer/markdown/frags.rs`, `src/viewer/mod.rs`, `src/ui/bookmarks/`,
  `src/browser/tab/`).
- Theme lookup: `theme::scheme_ref` interns the palette per theme (`OnceLock`),
  so ~40 per-frame `scheme_for` call sites no longer rebuild the 50-field
  `Scheme` per element; `hover_tooltip_lazy` builds `format!` tooltips only on
  hover; the tab rail reads a per-tab display memo (`Tab::display`: title and
  favicon origin, invalidated by state kind and `doc_generation`); bookmark
  tiles resolve favicons by URL string (`FaviconCache::texture_for_url`) instead
  of a `Url::parse` per tile per frame (`src/ui/theme/`, `src/ui/mod.rs`,
  `src/browser/tab/mod.rs`, `src/ui/tabs/`, `src/media/favicon.rs`,
  `src/ui/bookmarks/`, `docs/ARCHITECTURE.md`).
- Media caches: image caches no longer rescan every entry per frame
  (`ImageCache::loading_count`, recomputed in `poll`); the emoji LRU evicts with
  one O(n) pass per victim instead of two; `DestCache` charges a fixed per-entry
  container overhead so a link farm cannot retain several times its documented
  byte budget (`src/media/cache/`, `src/media/emoji/`,
  `src/protocol/rewrite.rs`, `docs/ARCHITECTURE.md`).
- Render caps: the per-frame path stays within its documented caps: the
  quote-alert look-ahead scans only the longest-marker prefix of each text run;
  the image-alt collector and depth-guard flatten buffer stop at
  `MAX_RENDERED_BLOCK_CHARS`; `cap_block_text` counts chars only up to the cap;
  each cached code block stores its line total once instead of rescanning the
  body per repaint; and `FaviconCache::texture_for_url` derives the origin key
  from the URL string, so a settled tile avoids a `Url::parse` per frame
  (`src/viewer/markdown/`, `src/viewer/code/`, `src/media/favicon.rs`,
  `docs/ARCHITECTURE.md`).
- Favicons: the tab rail requests probes from the borrowed per-tab display memo
  (no `Vec<Option<Url>>` plus one `Url` clone per tab per frame), and
  `FaviconCache` keys every lookup with a borrowed slice of the serialization
  (`String: Borrow<str>`), so a settled frame costs one hash probe per tab or
  tile; parsing is limited to the FIRST frame per unknown origin
  (`src/ui/tabs/`, `src/media/favicon.rs`, `docs/ARCHITECTURE.md`).
- Window title: the OS window title is memoized with the tab display data
  (`TabDisplay::window_title`, `Tab::window_title_cached`), so the per-frame
  check compares borrowed strings and allocates only when the title changes
  (`src/browser/tab/`, `src/app/`).
- Autocomplete: the omnibox autocomplete cache rebuilds only while its field is
  EDITING (on Home one address is shared, so each keystroke ran the scan twice
  for a closed popup), and `suggest::matches_candidate` compares ASCII
  candidates in place via `util::contains_ignore_ascii_case` instead of one
  `to_lowercase()` `String` per candidate per keystroke, tens of thousands of
  allocations (`src/ui/omnibox/`, `src/suggest.rs`, `src/util.rs`).
- Rows and avatars: bookmark tiles and history rows borrow their title (`Cow`,
  like suggestion rows) instead of cloning it per frame; the tile letter avatar
  returns a `char` encoded into a stack buffer instead of a `String` per tile
  per frame; `bookmarks::entry_bytes` is arithmetic instead of allocating a
  throwaway `Vec` per entry on every mutation, trim and save
  (`src/ui/bookmarks/`, `src/ui/history_page/`, `src/bookmarks/`).
- Settings: `read_setting` reuses the per-directory plaintext cache, so loading
  four keys decrypts once, not four times; the legacy-file sweep runs once per
  write path instead of on every session save; `settings_writer().flush` shares
  one deadline across both workers (the documented bounded wait was silently
  twice as long); a store's plaintext body is rebuilt only when `trim` actually
  dropped an entry (`src/settings/`, `src/bookmarks/`).
- List markers and tooltips: the ordered-list marker-width cache keys on
  `(ordinal, body size)` instead of a `String` key per item per frame (the
  marker text is `"{ordinal}. "`); dynamic tooltips (the Reload countdown, the
  zoom-in percentage) are built on hover only; the rendered code card's Copy
  button gets an explicit 24px minimum height instead of a ~17px sliver
  (`small()` zeroes vertical padding and skips the `interact_size` floor)
  (`src/viewer/markdown/items.rs`, `src/ui/navbar/`, `src/ui/icons/`,
  `src/viewer/code/mod.rs`).

## [0.3.3] - 2026-10-10

This release adds first-run bookmarks, address autocomplete in both address
fields, a navigable `about:home` entry and a Preferences action that deletes
all local data. It also retunes the tab rail, consolidates store encryption
under one key, trims the fetch cascade and the release binary, and hardens
deadlines, SVG parsing and encrypted store reads.

### Added

- First-run bookmarks: a fresh profile with no bookmark store starts with five
  curated defaults (Markdown guides, an agent-readiness leaderboard and
  developer-service docs), seeded once and never recurrently, so an existing
  store is never reseeded even when emptied (existing profiles keep the older
  eight). Favicons load through the Home tile pipeline, one probe per origin on
  first display.
- Address autocomplete: both address fields (toolbar omnibox and Home capsule)
  rank bookmarks, then local history, then a curated builtin list compiled in
  (`suggest::BUILTIN_SUGGESTIONS`: the five seeded bookmarks, developer-service
  docs and a developers-portal entry, grown by one `(url, title)` line),
  deduplicated by exact URL and matched case-insensitively on URL and title with
  or without scheme; an empty field offers recent bookmarks. `Down`/`Up` move
  the selection, `Tab` completes it, `Enter` or a click opens the row, `Esc`
  closes the popup before reverting the field, and the popup never shifts the
  capsule. The list is capped at `config::MAX_OMNIBOX_SUGGESTIONS` and cached
  per field in egui memory, rebuilt only when the query or the
  `HistoryStore`/`BookmarkStore` generation changes
  (`BookmarkStore::generation()`). Only `Ctrl/Cmd+L` focuses an address field,
  so no popup opens over a fresh window or tab. (`src/suggest.rs`,
  `src/ui/omnibox.rs`, `src/bookmarks.rs`, `src/ui/navbar.rs`, `src/ui/home.rs`,
  `src/ui/viewer/mod.rs`, `src/app.rs`, `docs/INTERFACE.md`,
  `docs/NAVIGATION.md`, `docs/ARCHITECTURE.md`.)
- New-tab page: Home is a real navigation entry (`about:home`, typeable like
  `about:history`), so Back and Forward traverse Home instead of dropping the
  Forward chain, while the capsule stays empty and Reload stays disabled on it;
  the session file keeps the `None` Home marker with no format change.
  (`src/config.rs`, `src/browser/tab/`, `src/app.rs`, `docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Preferences "Delete all data and close": the action uses a two-click
  confirmation like "Delete history", deletes the whole per-user data directory
  (`history.bin`, `bookmarks.bin`, `settings.bin`, `session.bin` and the shared
  `store.key`) and exits through `process::exit` without further saves
  (`app::MdowserApp::wipe_all_and_exit`, `settings::wipe_all_data`).

### Changed

- Tab rail: the expanded overlay collapses only when the pointer moves into the
  document area, so moving up to the toolbar or leaving the window keeps it
  open, and the open/close tween is slightly slower with a more decisive
  `cubic_out` easing (`EXPAND_ANIM_SECS` 0.16 → 0.22 s). (`src/ui/tabs.rs`,
  `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)
- Hardening and slimmer frames, no behavior change: window geometry lives in
  `config.rs` (`WINDOW_INITIAL_*`/`WINDOW_MIN_*`), OS/CLI open targets are
  capped at `MAX_TABS` before any tab is built
  (`main::collect_initial_targets`), every wall-clock deadline goes through the
  fail-closed `checked_add` in `config::deadline_after`, egui id salts are
  tuples everywhere, right-click link hitboxes hold `Arc<str>` handles,
  unordered list bullets are borrowed (`Cow`) with only the ordered prefix
  allocating, and context-menu shortcut hints are `&'static str`.
  (`src/config.rs`, `src/main.rs`, `src/protocol/client.rs`,
  `src/media/cache.rs`, `src/media/favicon.rs`, `src/ui/history_page.rs`,
  `src/ui/viewer/`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`.)
- Single shared key for every encrypted store: `store.key` now encrypts
  `history.bin` (magic `MDHS`), `bookmarks.bin` (`MDBM`), `settings.bin`
  (`MDST`) and `session.bin` (`MDSS`). Clean break without migration: legacy
  `settings.conf`/`session.conf` and `history.key`/`bookmarks.key` are deleted
  on load and never read. "Delete history" keeps the shared key (bookmarks,
  settings and session still need it); only the wipe-all path removes it with
  the whole directory. (`src/settings.rs`, `src/history.rs`, `src/bookmarks.rs`,
  `src/crypto.rs`, `docs/SECURITY.md`, `docs/ARCHITECTURE.md`,
  `docs/INTERFACE.md`.)
- Fetch: only the standard spellings are tried — directory indexes are
  `index.md` + `README.md` (`config::DIR_INDEXES`), and the `.markdown`
  extension and the `readme`/`index.markdown` variants are never requested,
  shrinking the cascade from 9 to 5 candidates, while legacy `.markdown` URLs
  still normalize to the same logical page. (`src/protocol/resolver.rs`,
  `src/protocol/local.rs`, `src/ui/error_view.rs`, `docs/NAVIGATION.md`.)
- `make`: builds the host release binary and the current-OS dist into `dist/` —
  on macOS the native group (`build` + `dist-mac`, the universal `.app.zip` plus
  per-arch `.tar.gz`), elsewhere `build` + `dist-host`. `make dist` is the host
  release binary plus the full multi-OS dist (`build` + `build-dist.sh --all`,
  starting with the host group). `build-dist.sh --host-only` packs only the
  current OS archive (Unix `.tar.gz`, Windows `.zip`, plus the matching `.deb`
  on Linux when packaging succeeds), reusing the per-OS names so `make` output
  is a subset of `make dist`. (`Makefile`, `scripts/build-dist.sh`,
  `docs/BUILDING.md`, `README.md`.)
- UI/chrome bounds: standardized in `src/config.rs`
  (`BOOKMARK_GRID_MAX_VISIBLE_ROWS`, `MAX_INLINE_VECTOR_H`,
  `MAX_INLINE_IMAGE_H`, `TAB_TITLE_CHARS`, `WINDOW_TITLE_CHARS`,
  `WINDOW_TITLE_ERROR_URL_CHARS`), so the Home grid, inline-image promotion, tab
  titles and OS window titles share auditable caps, not local magic numbers
  (`ui::viewer::image` re-exports the canonical values, `docs/ARCHITECTURE.md`
  points at `config::`). No behavior change.
- Per-frame work trimmed without changing rendering: the menu detector rejects
  oversized paragraphs on a byte sum before joining them (no body-sized `String`
  per paragraph), `Tab::favicon_origin` borrows the loaded URL, image accessible
  names use `Cow` with borrowed hover tooltips, the toolbar reload branch avoids
  building the countdown string off-cooldown, `theme::card`/`display`/`weak`
  share the canonical constructors (`ui::history` cards reuse
  `theme::card_with`), and `RichText::new` takes `&str` directly.
- Tooltips: all 16 call sites use one lazy helper (`ui::hover_tooltip`) that
  gates on `hovered()`, since `Response::on_hover_text` converts its argument
  eagerly, every icon button, tab row, tab close button, image and hand-painted
  control built one `String` per frame. The allocation now happens only on
  frames that show the tooltip.
- Embedded fonts: the two largest now ship Brotli-compressed and decompress once
  at runtime — `NotoColorEmoji-Regular.ttf` (24.2 MB) and
  `NotoSansCJK-Regular.ttc` (18.6 MB) become 6.3 MB + 11.2 MB at build time
  (`build_support/gen_fonts.rs`, quality 10, cached in `OUT_DIR` with a hash
  stamp), shrinking the release binary from 81 MB to 55 MB. `src/assets.rs`
  exposes `noto_color_emoji()`/`noto_sans_cjk()` backed by `OnceLock`: the color
  emoji face decompresses lazily on the first color emoji a session renders
  (pages without emoji never allocate it) and CJK at startup, each into one
  process-lifetime buffer borrowed without copies by epaint, the SVG fontdb and
  `media::emoji`. The build round-trip validates every blob (decompress ==
  source), and the generated `font_meta.rs` records the uncompressed lengths.
  The SVG fontdb shares epaint's four bundled faces (~1 MB) through the `Arc`
  instead of copying them. Build scripts run optimized
  (`[profile.*.build-override] opt-level = 3`), so the q10 encoder needs ~90 s
  instead of ~10 minutes at `opt-level = 0`. (`Cargo.toml`, `build.rs`,
  `build_support/gen_fonts.rs`, `src/assets.rs`, `src/ui/theme.rs`,
  `src/media/decode.rs`, `src/media/emoji.rs`, `docs/ARCHITECTURE.md`,
  `docs/BUILDING.md`.)
- Documentation hygiene: `docs/BUILDING.md` dev commands now match the canonical
  `make`/`scripts/build-test.sh` flags (`--locked --workspace`),
  `docs/ARCHITECTURE.md` references the canonical `config::` bounds, and
  `.cargo/audit.toml`/`deny.toml` wording no longer implies a CI service
  (`make audit` is the gate; there is no `.github/` automation).
- Code organization, no behavior change: the three largest sources are now
  directories of focused submodules, split by pure moves — `ui/viewer/markdown/`
  (`mod` the walk and block budget, `frags` the inline collection, `items` the
  list markers, `blocks` paragraphs/headings/tables/HTML, `links` link runs and
  callouts, `menu` the inline nav heuristic), `browser/tab/` (`mod` keeps the
  state, accessors and poll loop; `nav` every navigation/stack method) and
  `markdown/document/` (`mod` parser options and event walk, `normalize` the
  pre-parse repair pass, `plain` the Copy-text derivation).
  (`docs/ARCHITECTURE.md` module map updated.)
- Preferences dialog: compacted in `src/ui/history.rs` to a shorter fixed
  480x720 size with a tighter header bottom margin, and the History/Local data
  cards fit their rows with no reserved bottom space, since each
  delete-confirmation note takes space only while armed. (`docs/INTERFACE.md`,
  `docs/ARCHITECTURE.md`.)
- Toolbar bookmark: a ribbon instead of a star — the omnibox right slot shows
  the outline `Icon::Bookmark` when the page is not bookmarked and the new
  filled `Icon::BookmarkFilled` when it is, and `Icon::Star`/`StarOutline` are
  gone. Every top-bar button owns a fixed slot (Reload/Stop share one, the
  ribbon shares the clear-X slot, the zoom pill keeps its per-mode width), so
  swapping an icon never shifts its neighbours. Root cause fixed: a disabled
  button faded through an empty `ui.scope`, which advances the cursor one item
  spacing and pushed the omnibox 2px right when Reload was disabled; the fade is
  now a faded `Painter` clone with zero layout effect, locked by an icons test
  and a navbar test. (`src/ui/icons.rs`, `src/ui/omnibox.rs`,
  `src/ui/navbar.rs`, `src/app.rs`, `src/bookmarks.rs`,
  `src/browser/tab/mod.rs`, `docs/INTERFACE.md`, `docs/ARCHITECTURE.md`.)

### Fixed

- Home bookmark rename: while the inline name field is open the blue ring paints
  only the editable text field (1px), not the whole tile, and the drag-reorder
  ring is 1px too. (`src/ui/bookmarks.rs`.)
- Build scripts no longer trigger the macOS `rust-objcopy` warning: cargo >=
  1.77 strips debuginfo from build dependencies automatically
  (`-C strip=debuginfo`), while the toolchain's `rust-objcopy` is broken on some
  installs (missing `libLLVM.dylib` -> `SIGABRT`), so every build printed
  "stripping debug info with `rust-objcopy` failed" once per build script. Both
  build-override profiles now set `strip = "none"`, the opt-out the release
  profile already uses; only `target/` grows. (`Cargo.toml`,
  `docs/BUILDING.md`.)

### Security

- Fail-closed wall-clock deadlines: slot waits, image/favicon budget backoffs
  and the history delete confirmation go through `config::deadline_after`
  (`checked_add`, expiring immediately on the unreachable overflow) instead of
  `Instant + Duration`, so no thread can panic on time arithmetic; the OS/CLI
  open-target list is capped at `MAX_TABS` before any tab is built.
  (`src/config.rs`, `src/main.rs`, `src/protocol/client.rs`,
  `src/media/cache.rs`, `src/media/favicon.rs`, `src/ui/history_page.rs`,
  `docs/SECURITY.md`.)
- SVG structural pre-pass: `allow_dtd: false` is pinned explicitly instead of
  relying on the XML library default, so a DTD reaching usvg's entity-expanding
  parser is rejected first. The SMIL rejection list also covers `animateMotion`
  and `discard` on top of `animate`/`animateTransform`/`set`.
  (`src/media/decode.rs`, `docs/SECURITY.md`.)
- `file:` destinations carrying credentials or a port
  (`file://user:pass@localhost/x`, `file://localhost:8080/x`) are inert in the
  link rewriter like remote hosts: the reader rejects them at navigation time,
  so a local document no longer renders a live link whose only outcome is
  destroying the page, and credentials can never ride along a destination.
  (`src/protocol/rewrite.rs`, `docs/SECURITY.md`.)
- Encrypted stores: `history.bin`, `bookmarks.bin` and both `settings.bin` reads
  load through a shared `read_regular_file` helper that opens with `O_NONBLOCK`
  and re-checks the handle before reading, so a FIFO swapped in between the
  metadata check and the open cannot block the UI thread, and the streaming read
  enforces its cap even if the file grows after the metadata check.
  (`src/settings.rs`, `src/history.rs`, `src/bookmarks.rs`.)

## [0.3.2] - 2026-10-08

This release simplifies the OS window title and ignores SVG filters instead of
failing the whole image. It also fixes the Home bookmark grid scrollbar,
bookmark tile title elision and `Esc` handling in Preferences.

### Changed

- OS window title: now just the open page's own title (frontmatter `title`, else
  the schemeless URL, `Error: …` on failures) with no app name or `Tab N:`
  prefix; on Home — nothing open or a new tab — it is `MDowser`
  (`browser::tab::window_title`).
- SVG filters: ignored instead of failing the whole image (`media::decode`) —
  `<filter>`/`fe*` definitions, primitives and every `filter` reference (element
  and attribute spellings, namespace prefixes included) are stripped from the
  source before `usvg` parses it, and the unfiltered content renders, so a
  status badge carrying a blur shadow shows its flat badge. Removing the
  references too is required (usvg drops an element whose `filter` link
  dangles). No filter can run (the `numOctaves`/morphology/convolution cost and
  filter pixmaps stay unreachable), while CSS-spelled `filter` declarations
  (`style="filter:…"`, a `<style>` rule) still reject the image and
  `marker`/`pattern` stay rejected; `docs/SECURITY.md` documents the new
  posture.

### Fixed

- Home bookmark grid: the floating scrollbar is hidden while the grid fits
  (`ui::bookmarks`) — the inner scroll area reported its trailing item gap as
  overflow, so a full-height bar sat next to the last tile; it now appears only
  once the grid exceeds `MAX_VISIBLE_ROWS`.
- Bookmark tile titles: elided by the layout instead of clipped — a title
  needing more than the tile's two centered lines (long paths, CJK) now lays out
  with `max_rows = 2` and an `…` on the second row instead of a mid-glyph cut.
- Preferences: `Esc` with an open dropdown closes only the dropdown
  (`app::handle_shortcuts` checks `egui::Popup::is_any_open`, tooltips excluded)
  instead of the whole dialog.

## [0.3.1] - 2026-10-07

This release fixes image texture-budget retries and hardens SVG parsing, key
repair, atomic writes and autolinks. It also retunes the safety caps,
integrates the icon generator into the build, and polishes graphics and
documentation.

### Changed

- Image texture-budget retries: the per-URL counter is cleared only by a
  successful upload (and on permanent failure), so a saturated process-wide pool
  no longer re-downloads and re-decodes every image until the 80s job deadline
  and `MAX_IMAGE_BUDGET_RETRIES` applies as documented (`media/cache.rs`). The
  color-emoji cache gets the same bounded attempt count (after it, the cluster
  keeps the monochrome fallback instead of re-rasterizing forever), and the
  emoji grapheme split is skipped for text without any emoji candidate, saving
  one `Vec` per plain fragment per repaint.
- Encrypt: builds the output buffer once (`header | nonce | plaintext`, then
  seals the tail in place), copying the plaintext exactly once, which halves the
  transient allocation on the UI thread (bookmark saves are bounded by the store
  cap).
- Bookmark star state: no longer re-parses the URL and linearly scans the store
  every frame — `BookmarkStore` keeps an exact URL index in lockstep with its
  entries and the toolbar asks it through the already-parsed `display_url` (a
  borrowed `Tab::loaded_url`), and the loaded-URL accessor also stops cloning
  the URL and title for a state check.
- `host:port` links: destinations the omnibox accepts (`localhost:3000`,
  `example.com:8080/x`) resolve as `https://` instead of rendering inert, while
  blocked schemes (`javascript:`, `data:`, `mailto:`) and userinfo forms still
  never become host:port.
- Graphics polish: code inside a heading link renders monospace; the dark-mode
  pressed state on toolbar and omnibox controls lightens; the Preferences "View
  third-party licenses" link shows keyboard focus; task checkboxes announce
  done/pending; bookmark tiles draw the shared focus ring; the inline rename
  field and image placeholders carry accessible names; context-menu and dropdown
  rows share one corner radius (6, not 8); the expanded tab rail's action block
  height is derived from the same constants the layout uses; and the floating
  scrollbar's lane is reserved by the history rows' remove button, the tab close
  button and the Home grid's rightmost tile actions.
- Build hygiene: the icon generator's own source is hashed into the regeneration
  stamp (editing `build_support/gen_icons.rs` cannot leave stale committed
  icons), the Windows resource-compiler variables (`RC`, `RC_PATH`,
  `RC_<target>`, `ProgramFiles*`) are part of the rebuild fingerprint, and the
  PATH lookup requires the executable bit on Unix (a non-executable `windres`
  otherwise downgraded the build to "no icon" silently).
- Icon generator: integrated into the build — the standalone `tools/gen-icons`
  workspace package is gone, and `build.rs` with `build_support/gen_icons.rs`
  rasterizes `assets/icon.svg` into `assets/icons/` whenever the committed
  outputs are missing or outdated (an SVG content hash in `OUT_DIR` lets a
  normal build read one small file and stop). `src/assets.rs` embeds
  `icon-256.png` with `include_bytes!`, so a fresh checkout or a deleted
  `assets/icons/` bootstraps itself, and `make icons` forces a regeneration. The
  same `resvg` version and features as the app's SVG pipeline reproduce the
  committed files byte for byte.
- Safety caps: retuned for longer documents, bigger images and deeper local
  history, every bound explicit in `src/config.rs` — documents 5→10 MiB; images
  10→16 MiB / 24→48 MP (12288 px per side, 3072 px uploaded); favicons 512 KiB→1
  MiB at 384 px; parsed events 500k→1M; rendered blocks and code lines 5k→10k; a
  rendered prose/HTML/table block 256→512 KiB; table columns 1024→4096; bare-URL
  autolinks 10k→20k; anchors 10k→20k; link joins 4096→8192; route timeout 10→20s
  and streamed-chunk stall 10→15s; redirects 5→8; concurrent document
  fetches/local reads 4→6; network workers 2→4; image jobs 4→6 (3 per document);
  texture budgets 256→384 MiB per document and 512→768 MiB process-wide; emoji
  caches 512→1024 entries / 32→64 MiB; per-tab Back/Forward 100→250; local
  history 5000→20 000 entries and 2→8 MiB; bookmarks 1000→2000 entries (tile
  glyph 8→16 chars); URL input 4096→8192 chars; dest-cache budget 4→8 MiB;
  settings read cap 64→256 KiB. The recursion-depth, refresh-cooldown and
  image-retry caps stay.

### Fixed

- Session restore: opening a file (`mdowser notes.md`, a `file://` URL) with
  "restore tabs on startup" enabled no longer overwrites the stored session on
  the first frame — OS/CLI targets win for the current launch and the session
  file is rewritten only once the open tabs change (the exit hook follows the
  same rule), so double-clicking a document no longer discards the remembered
  session.
- Autolinks: a link run glued to a link's closing `)` (`[a](b)www.evil.com`) no
  longer linkifies (not at a GFM start boundary); a domain glued to `@` stays
  text in every candidate form (`www.example.com@evil.com`,
  `https://example.com@evil.com`), matching the documented email rule, since the
  URL parser would otherwise read the domain as userinfo; and multibyte
  candidates are capped by characters like every other URL path.
- Documentation drift: `CONTRIBUTING.md` points new UI icons at the monochrome
  vector set; `NAVIGATION.md` documents the 8-redirect cap and the true
  local-file entry points; `SECURITY.md` and `ARCHITECTURE.md` describe the SVG
  pre-pass as rejecting `<script>`/`<foreignObject>`/SMIL elements (not
  "ignoring" them) and attribute `avif-decode` to BSD-3-Clause (`avif-parse`
  stays MPL-2.0); `BUILDING.md` lists the real archive contents and packaging
  tools; `INTERFACE.md` documents the tile icon priority (stored glyph, then
  favicon) and the cached-favicon loading dot.
- `THIRD_PARTY_NOTICES.md`: now credits the Hack font bundled by
  `epaint_default_fonts` (MIT + Bitstream Vera License) with the full Bitstream
  Vera text and no longer calls the color-emoji pipeline an interface-icon
  source; the in-app `about:licenses` page picks the file up on the next build.

### Security

- SVG `<pattern>`: rejected by the structural pre-pass, because a pattern's
  pixmap is sized by its own rect, not the rendered target, so a tiny remote SVG
  could demand an allocation whose failure aborts the process through
  `tiny_skia::Pixmap::new`'s infallible allocator, bypassing every raster-size
  cap. DTD rejection moved into that same structural parse (roxmltree with
  `allow_dtd: false`, before usvg's entity-expanding parser runs), so a comment
  or CDATA section merely containing `<!DOCTYPE`/`<!ENTITY` no longer rejects a
  valid exported SVG; `docs/SECURITY.md` and `docs/ARCHITECTURE.md` list
  `pattern` and the new DTD wording.
- Corrupt-key repair: serialized end to end in `src/crypto.rs` — the best-effort
  cross-process lock is held until the replacement key is installed, and a lock
  timeout fails closed (the file is never deleted) instead of unlinking a path
  another instance may have just filled with its complete fresh key. Two
  instances repairing at once can no longer leave a winner whose key is not on
  disk, and an oversized key file is rejected from its metadata without being
  read whole.
- `atomic_write`: replaces the destination in one rename on every platform
  (`std::fs::rename` replaces an existing target on Windows too, through
  `MoveFileExW`/`FileRenameInfoEx`), so a crash between two renames cannot leave
  the store missing on Windows. The unreadable-blob `.bak` preservation no
  longer clobbers an earlier backup: a second unreadable blob needs the same old
  key, and the first one kept is the oldest data.
- Unreadable `settings.conf` (permissions, invalid UTF-8, over the cap) is
  reported in Preferences and never treated as "restore tabs off", so a
  transient read failure can no longer delete the plaintext `session.conf`.
- "Delete history now": also sweeps orphaned `history.key.tmp.*` copies left
  behind by an interrupted key installation, because a full 32-byte key must not
  survive an explicit delete.
- Fence guard: a fence-like line whose backtick info string contains a backtick
  no longer opens fence state in the pre-parse table-bomb guard; pulldown-cmark
  keeps it as paragraph text, so treating it as a fence let a 4097-column table
  hide behind it.
- Missing bounds: added with the retune — SVG nesting is depth-capped
  (`MAX_SVG_DEPTH`) before the XML parser runs, tables are capped at
  `MAX_TABLE_ROWS` rows per block (each row also spends the per-frame
  `MAX_RENDERED_BLOCKS` budget), right-click link hitboxes at `MAX_LINK_HITS`
  per frame, and the `about:history` search field at `MAX_HISTORY_FILTER_CHARS`.
- Link/image destinations: interned (`DestCache` and the fragment tree hold
  `Arc<str>` handles), so a link split into thousands of fragments or a document
  repeating one long URL shares one allocation instead of copying it per
  fragment, closing a memory amplification (~1000x the body) that no other cap
  bounded.
- Code-block cache: galleys now enforce the cache byte budget on cache hits — a
  theme or select-all change rebuilds every visible block's galleys without a
  miss, otherwise bypassing eviction and keeping laid-out glyphs past the
  budget.

## [0.3.0] - 2026-10-07

This release adds encrypted local bookmarks and history, local file documents, a
modern toolbar, GFM extensions, static SVG images and RTL/multilingual
rendering, and hardens URL, SVG and encrypted-store handling while trimming
per-frame allocations.

### Added

- Third-party licenses page: `about:licenses` renders the embedded
  `THIRD_PARTY_NOTICES.md` — the file shipped in every release archive — through
  the browser's Markdown engine as a read-only page with document text
  selection, copy, right-click menu and live links; Preferences adds a footer
  line with the version, app license and a "View third-party licenses" link that
  opens it in a new tab, and session restore restores it like `about:history`.
- Encrypted local bookmarks (`src/bookmarks.rs`): the omnibox star (filled when
  bookmarked, outline otherwise, disabled when nothing is loaded) or
  `Ctrl/Cmd+D` toggles the page with a toast; the Home / new tab page is the
  only place they appear, in a virtualized "quick access" grid, newest first,
  with a stored glyph, else the site favicon when it resolves, else the first
  letter of the title on a stable color. Clicking a tile opens it in that tab,
  `Cmd/Ctrl+click` asks for a new tab, and dragging reorders the grid live.
  Editing is ON the tile with NO extra window: the hover pencil opens an inline
  rename field whose spot becomes a save check (Enter saves, Esc drops the
  draft), and the hover X removes the tile. `bookmarks.bin` is ChaCha20-Poly1305
  (magic `MDBM`, random 96-bit nonce, 256-bit key in `bookmarks.key` mode 0600 /
  directory 0700 on Unix), written by atomic temp+rename off the UI thread on
  the shared store writer (2s debounce, 5s flush timeout), kept until removed
  and bounded to 1000 entries, where an add at the cap is refused with a toast;
  the plaintext keeps the optional tile customization
  (`timestamp\turl\ttitle\ticon\tcolor`) and still parses three-field lines, and
  the AEAD format and key handling (`src/crypto.rs`) plus the store writer
  (`src/store_writer.rs`) are shared with history.
- Local Markdown files: `file://` URLs and absolute/`~`/explicit-relative paths
  open as documents, with `.md`/`.markdown` completion for a stem and
  conventional `index.md`/`README.md` directory indexes (no directory listing);
  relative links and images resolve against the document's directory, the
  omnibox shows the decoded path with a page indicator, History records local
  visits, "home" goes to the containing folder, and reads share the network caps
  (5 MiB / image caps, UTF-8 mandatory, cooperative cancel), while a remote page
  can never link to or load a local file and a `file://host/` remote share is
  rejected (`protocol::local`, `client::parse_navigable`, the `rewrite` scheme
  policy).
- Open targets from the OS or command line (`mdowser notes.md`,
  `file:///path/a.md`, `https://example.com/x`): one tab each, taking priority
  over session restore (`main` → `app::tabs_from_initial`).
- Linux `.deb`: the `.desktop` entry registers MDowser for `text/markdown` and
  `text/x-markdown` (`Exec=… %U`), so file managers can open `.md` files with
  it.
- GFM-style bare-URL links (`markdown::autolink`): URLs typed as plain text
  (`www.celticos.com`, `https://example.com/docs`) render as clickable links,
  and beyond GFM a bare domain without `www.` linkifies anywhere with `https://`
  inserted, as does a local `file:` URL the reader accepts
  (`file:///Users/me/notes.md`, `file://localhost/…`). Only those forms link:
  `http:`, `mailto:`, `ftp:`, `data:` and every other scheme stay plain text,
  and emails never link (a domain followed by `@` is rejected); the `www.` form
  gets `https://` inserted, recognition needs a delimiter boundary and a valid
  domain, and trailing punctuation, unmatched `)` and entity-like `&x;` suffixes
  are excluded. A bare domain must end in a real IANA root-zone TLD
  (`markdown::tld`), so `Cargo.toml`, `notes.txt`, `data.csv`, `foo.local` and
  version tokens (`v1.2`, `1.2.3.4`) stay text; when that TLD doubles as a
  common file extension (`md`, `rs`, `sh`, `py`, `pl`, `ai`, `zip`) a file-like
  stem (`README.md`, `config.rs`, `script.pl`, `logo.ai`) stays text while real
  hosts (`agentready.md`, `mkplandeki.pl`) link. Code spans/blocks, metadata
  blocks, existing links and image alt text are never touched; the pass is
  bounded by `config::MAX_AUTOLINKS_PER_DOC` / `AUTOLINK_MAX_DOMAIN_CHARS` and a
  linear scan, and Copy pastes the URL as written.
- Preferences: a fixed-size, card-style dialog; the History card holds the
  retention dropdown and the destructive "Delete history" action (two-click
  confirmation), and the Appearance card holds a Theme dropdown
  (System/Light/Dark, applied immediately), a Default zoom slider and a "Restore
  tabs on startup" toggle that reopens the last session's tabs (HTTPS pages and
  `about:history`); settings persist in `settings.conf` key-by-key
  (read-modify-write) so history retention and the new keys never clobber each
  other, while the session lives in `session.conf`, written only while restore
  is enabled and deleted when it is turned off.
- README rendering styles: neutral palette (light/dark), H1/H2 bottom rules,
  muted H6 and blockquotes with alert-accent bars, bordered code cards with
  syntax colors (no line-number gutter), striped bordered tables, README body
  line height, 4px `<hr>` and icon-sized inline SVG.
- `:shortcode:` emoji expansion in text (never in code spans or blocks), plus
  `assets/NotoEmoji-Variable.ttf` (OFL-1.1, complete monochrome Noto Emoji)
  registered as the first fallback for UI and SVG text, so recent emoji no
  longer render as tofu boxes.
- Multilingual rendering through the OFL-1.1 Noto family:
  `assets/NotoSansCJK-Regular.ttc` (Chinese, Japanese, Korean,
  kana/Hangul/Bopomofo, CJK Ideographs + Ext A), `assets/NotoSans-Variable.ttf`
  (Latin Extended-A/Additional, Vietnamese) and `assets::SCRIPT_FONTS`, one face
  per major non-CJK script (Arabic, Hebrew, the Indic scripts, Thai, Lao, Khmer,
  Myanmar, Ethiopic, Armenian, Georgian, Thaana, N'Ko, Cherokee, Canadian
  Aboriginal, Symbols 2); all register between the emoji face and epaint's
  bundled fonts in the proportional and monospace families, and the SVG font
  database loads them with zero-copy `Arc` sharing for `<text>`.
- Color emoji (`media::emoji`): document emoji render in color from
  `assets/NotoColorEmoji-Regular.ttf` (SVG-in-OpenType, OFL-1.1); because epaint
  cannot paint color fonts, the viewer shapes each emoji cluster with harfrust,
  isolates its `glyph<id>` element plus the transitive closure of its `#id`
  references from the font's shared SVG document, injects the em-square viewBox
  and rasterizes it through the hardened resvg pipeline, with textures cached
  per (cluster, pixel bucket) under `config::EMOJI_CACHE_*` and the process-wide
  texture budget; any failure falls back to the monochrome Noto Emoji text path,
  which links and code spans keep.
- Right-to-left display order (`ui::viewer::bidi`): epaint applies no Unicode
  bidi algorithm, so the viewer reorders RTL runs before layout (base direction,
  visual run order, word order, mirrored punctuation) and lays RTL paragraphs
  out from the right; harfrust still shapes the logical runs and copy text stays
  logical, while mixed-direction content split across inline elements remains
  approximate.
- Static SVG image support (`resvg`/`usvg`, pure Rust): icons, logos and badge
  images render; `<script>`/`<foreignObject>` are never executed (rejected by
  the structural pre-pass), no `href` (network, local or inline `data:`) is ever
  resolved, fonts are the app's own, and the raster is capped at 2048 px/side.
- One automatic retry per image after a transient network/stall failure,
  re-queued behind pending jobs; wrong content types and cancellations are never
  retried.
- Long link-only paragraphs (site card lists flattened by an exporter) stack one
  link per row while short link runs stay inline; entries that carry a logo
  (`[![icon] Vendor<h3>Name</h3>Body](url)`) stack too, so a joined card list
  reads one card per row.
- Lax link joining (`markdown::document::normalize_body`): an inline link/image
  whose text an exporter split with a blank line (`[label` blank line
  `body](url)`, common in some docs-site `.md` exports) is repaired instead of
  leaking a literal `[` and a stray `](url)`; only blank lines inside a balanced
  `[...](...)` span are removed, and the pass is bounded
  (`config::MAX_LINK_JOINS` / `MAX_LINK_JOIN_DEPTH` plus a scan budget).
- Uniform inline SVG icon sizing: every inline SVG icon in one BLOCK (paragraph
  or link stack) renders at the tallest one's clamped height, not just
  consecutive icons on one line, so a card list whose logos declare wildly
  different intrinsic sizes (`width="1em"` next to a 512px logo) shows them all
  at one icon size.
- Light/dark logo pairs (`logo-light.svg` + `logo-dark.svg` adjacent) collapse
  to the single variant matching the document theme, so a docs card shows one
  logo; only URLs differing in exactly one `light`/`dark` token pair up.
- Accessibility skip links leaked by `.md` variants (`[Skip to
  content](#main-content)`) are dropped instead of rendering.
- In-document anchors navigate: `[text](#heading)` scrolls to the heading whose
  slug or explicit `{#id}` matches and `[^N]` jumps to its footnote; unknown
  anchors render as text, a `#fragment` on a cross-page link is applied after
  the load resolves, and heading/footnote targets are collected once per fetch
  (`markdown::anchors`).
- `:::` and `!!!` admonitions render as callouts; unknown types stay literal,
  code fences are never touched, and markers are stripped from the copied text.
- Markdown `Content-Type` aliases (`text/x-markdown`, `application/markdown`,
  `application/x-markdown`) are accepted alongside `text/markdown`, while
  `text/plain` and everything else are still rejected.
- `.markdown` route candidates (`/love.markdown`, `/love/index.markdown`, home
  `/index.markdown`).
- Favicon probing also tries `/favicon.svg` after `.ico`/`.png`.
- Modern top toolbar (`ui::navbar` + `ui::omnibox`): back/forward arrows with a
  tail, circular hover fills, and one shared address capsule with a padlock
  security indicator, a clear `X` while editing, select-all on focus, Enter to
  navigate and `Esc` to revert to the shown page; no `Go` button anywhere.
- Reload/Stop slot: while a fetch is in flight the reload icon becomes a Stop
  `X` that cancels it and restores the page that was on screen (`Tab::stop`,
  backdrop + decoded images kept), and `Esc` does the same when no field or
  context menu owns the key.
- Indeterminate loading sweep under the omnibox (60ms ticks) so a first load is
  never a blank wait.
- Compact zoom pill (`-` / `%` / `+`): the readout resets to 100% when clicked;
  the pill drops the readout on narrow windows and hides entirely on very narrow
  ones so the omnibox keeps the space.
- `Icon::Lock` and `icons::icon_button_at`, a custom-rect icon button (omnibox
  clear, zoom pill) with the same hover, cursor, tooltip and accessible-name
  rules as `icon_button`.
- App identity, everywhere the OS looks for it: `assets/icon.svg` (blue rounded
  tile with the white Markdown `M↓`) feeds a generated raster ladder
  (`assets/icons/`, rebuilt with `make icons`): PNG 16-1024 for Linux hicolor
  themes and the runtime window/taskbar icon (`assets::window_icon`), a
  multi-size `.ico` embedded into Windows `.exe` resources by `build.rs`, and an
  `.icns` installed as `Contents/Resources/icon.icns` with `CFBundleIconFile` in
  the macOS `.app`; the `.deb` installs the hicolor icons and a desktop entry
  with `Icon=`/`StartupWMClass=`.
- Encrypted local browsing history with configurable retention
  (`src/history.rs`): every page that actually loads is recorded; the default
  window is 3 months and Preferences offers forget on exit, 1/3/6 and 12 months,
  or forever, plus "Delete history now"; the store is bounded (5000 entries / 2
  MiB of plaintext) and lives in the standard per-user data directory (macOS
  `~/Library/Application Support/MDowser`, Linux `$XDG_DATA_HOME/mdowser`,
  Windows `%LOCALAPPDATA%\MDowser`) as `history.bin` encrypted with
  ChaCha20-Poly1305 (random 96-bit nonce, 256-bit key in `history.key`, mode
  0600; directory 0700), and nothing leaves the machine.
- History and Preferences UI: the local history opens as an INTERNAL PAGE
  (`about:history`) in a tab, rendered in the same reading sheet as a document
  (`src/ui/history_page.rs`) with newest first, search, per-row remove, a footer
  entry count and the two-click "Delete history now"; the page is a real
  navigation (Back/Forward work) that never touches the network and never
  records itself, and the address bar shows `about:history`; Preferences stays a
  floating overlay, the History button is pinned at the bottom of the tab rail,
  `Ctrl+H` / `Cmd+Y` opens the history page, `Ctrl/Cmd+,` toggles Preferences,
  and `Esc` closes the window (or stops a load).

### Changed

- Browser zoom keyboard shortcuts: `Ctrl/Cmd` `+`/`=` (in), `-` (out) and `0`
  (reset) drive the same factor as the toolbar pill and work while a field is
  focused; listed in the README and `docs/INTERFACE.md` shortcut tables
  (`README.md`, `docs/INTERFACE.md`).
- Menu and action rows: hand-painted menu/action rows now show a pressed state
  one step past hover, matching the icon buttons.
- Graphics (`ui::icons`): interface icons are a monochrome vector set, with
  every toolbar/rail/omnibox glyph hand-drawn on one 24x24 design grid and
  tinted with the active theme — muted at rest, easing to the accent while
  growing ~10% on hover and pulling back on press; the color-emoji chrome is
  gone, that pipeline now serves only document emoji and user-chosen bookmark
  tiles, and every icon variant paints vector shapes.
- Chrome icon sizing (`ui::tabs`, `ui::bookmarks`): favicons and the rail's
  icons (Home/globe placeholders, `+ New Tab`, `History`) paint at the toolbar's
  glyph size (`ui::icons::BUTTON`/`GLYPH`), and the Home bookmark tiles draw
  their favicons at that same size; the selected tab's accent bar moved into the
  rail's left gutter (a row-filling favicon covered the old in-row bar).
- Toolbar Home button: it opens the new tab page (the landing page with the
  large omnibox and the bookmark grid) instead of the server root, the page left
  behind moves to `back` so Back returns to it, and a pending load is abandoned.
- Omnibox indicator: the padlock/file indicator shows hover and keyboard-focus
  feedback and a pointing cursor, and focuses the address field when clicked,
  matching every other painted button.
- Per-frame allocations: egui id salts are tuples instead of `format!` strings;
  the history page filter cache compares the raw query and rebuilds the
  lowercase needle only on a miss; error-view hints are borrowed `Cow`s;
  tooltips and accesskit labels (icon buttons, tab close buttons) are built
  lazily only while hovered/announced; the Preferences data-directory tooltip
  builds its path only on hover.
- Home / new tab page: decluttered to exactly three elements — the wordmark, the
  large address capsule and the bookmark grid — with calmer spacing (84px above
  the wordmark, 36px before the capsule); the subtitle, "Press Enter to open"
  hint, examples / local-file notes, image-format line and keyboard-shortcut
  catalog are gone.
- Documentation: `README.md` is now a project overview, the detailed catalogs
  moved to `docs/RENDERING.md`, `docs/INTERFACE.md`, `docs/NAVIGATION.md` and
  `docs/BUILDING.md`, and the performance model now lives in `ARCHITECTURE.md`
  (`README.md`, `docs/RENDERING.md`, `docs/INTERFACE.md`, `docs/NAVIGATION.md`,
  `docs/BUILDING.md`, `ARCHITECTURE.md`).
- History navigation: opening History (`about:history`) creates a NEW tab,
  inserted right after the active one and activated, instead of replacing the
  page being read (tab rail button and `Ctrl+H`/`Cmd+Y` alike; at `MAX_TABS` it
  shows the usual toast); clicking a row then navigates that same history tab to
  the visit so Back returns to the list, and a repeat request while the active
  tab already shows the page is a no-op.
- Interface icons became color emoji (`ui::icons`): every chrome glyph —
  toolbar, tab rail, omnibox indicator and clear action, zoom pill,
  `about:history` row remove, image placeholders and loading states — is one
  emoji cluster rendered through the same `media::emoji` pipeline as document
  emoji, served by one process-wide cache with the monochrome Noto Emoji text
  face as fallback, each rasterized on a fine 2px grid.
- Content-shape heuristic thresholds (menu entries/chars/words, link-stack gate,
  skip-link cap, anchor cap, route-candidate budget) are centralized in
  `config.rs` instead of inline literals.
- Skip-link filter: it is structural — only the document's first block, and only
  when it is a link to a fragment — so a plain paragraph reading "Skip
  navigation" is no longer dropped.
- Dot menus: the dot menu family keeps the same navigation guard as pipes (a
  link or an uppercase/digit label is required), so `término · definición` stays
  a paragraph.
- Network tests: ignored network tests use generic public endpoints and ICO/AVIF
  decode is covered by local fixtures, so no test names a third-party site.
- Bullet lists render like a document: every unordered item (source marker `-`,
  `*` or `+`, any depth) draws ONE large `·` in a fixed 24px gutter aligned with
  the FIRST text row, and the item text is a real hanging indent — every wrapped
  row and later fragment stays in the text column; ordered `N.` items share the
  same text column.
- Document typography follows the README scale: body 16px at 1.5 line height,
  headings 2em/1.5em/1.25em/1em/.875em/.85em, and inline and block code at 85%.
- README rendering palette: a neutral documentation palette (light `#d1d9e0`
  borders; dark `#151b23` chrome, `#f0f6fc` text, `#9198a1` muted, `#3d444d`
  borders, updated syntax colors), inline code pills with `.2em .4em` padding
  and a 6px radius, code cards without the 1px border at 16px padding and 1.45
  line height, tables with collapsed interior cell separators and a tight
  5px/11px cell padding, and task checkboxes that paint like the native control
  (blue fill + white check when done, bordered box when pending).
- Quotes and alerts are ONE thick bar in a soft full-width box: egui's indent
  guide line is disabled (`Visuals::indent_has_left_vline`) so the bar is not
  doubled, the bar is 5px, the whole quote block carries a background (neutral
  `container_low` wash for plain quotes, ~9% alert accent for callouts), and
  alert bodies keep the normal text color while plain quotes stay muted.
- Tables fill the drawn card border: measured content widths grow proportionally
  to the card's inner width, or shrink proportionally down to `min_col` when
  they do not fit, so long cells wrap instead of pushing a horizontal scrollbar
  and columns pinned at the floor leave the remaining space to the others
  (`ui::viewer::table::distribute_columns`); the grid spans the card edge to
  edge (cells adjacent, no Grid spacing) with a tight 5px/11px padding INSIDE
  each cell, and quote/alert boxes span the full reading column with consecutive
  boxes keeping vertical air (`QUOTE_GAP`).
- Vertical tab rail: the collapsed rail is 44px so it hugs the favicons; the
  hover overlay reads as an extension of the toolbar instead of a floating card
  (square corners, no shadow above, only the right edge elevated — a soft
  gradient in light, a 1px hairline in dark), and the expanded `+ New Tab` is
  the same borderless action row as the collapsed `+`.
- Home page: the card + field + `Go` are replaced by one large centered omnibox
  capsule (same widget and behavior as the toolbar), with a "Press Enter to
  open" hint.
- Toolbar button hovers are neutral circles (the icon keeps its on-surface
  color) instead of a blue tinted glyph.
- Toolbar icons repainted with cleaner joins: the home house draws its roof,
  walls/floor and door as polylines, the reload arrow opens a wider gap at the
  top with its head at the clockwise end of the arc, and the zoom handle starts
  on the lens rim.
- Removed the now-unused primary `filled_button_sized` helper and the
  `on_primary` token (the toolbar has no filled primary button anymore).
- One-line menus (`Uno | Dos | Tres`) render as a CENTERED nav row with muted
  separators (`vertical_centered` never centered a wrapping child); the dot
  family (`·`, `•`, `∙`, `⋅`) is recognized too (a dot only splits as its own
  token, so `col·legi` stays a word), pipes keep the all-lowercase prose guard
  while dots do not, and delimited tables are never mistaken for paragraphs
  (they arrive as `Tag::Table`).
- Inline images: images sharing a line with text stay inline only while small
  (icons, badges ≤2 body lines and ≤80% of the reading column), and taller/wider
  ones are promoted to centered blocks, so a photo never vertically centers the
  neighboring words or opens a huge blank line around them; inline images get
  3px of breathing room on each side.
- Page sheet: it now reaches the top, right and bottom window edges (was 6/8/8px
  of outer margin) while keeping its 12px rounded corners (the chrome shows
  through the corner cutouts), and the left keeps its 8px gutter against the tab
  rail, so the reading area wins those pixels.
- Inline fragments borrow text from the parsed events (`Frag<'a>` + `Cow`), so a
  repaint no longer copies the whole document; copy text is derived once at
  fetch time (`Fetched.plain_text`), so `Ctrl/Cmd+C` never re-parses on the UI
  thread.
- Code blocks cache body + token spans per (document, block index, language), so
  repaints neither re-copy nor re-hash a block.
- Favicon origins are resolved once per tab per frame and texture handles are no
  longer cloned.
- The top fade and the document hit-test share `theme::scrollbar_width`.

### Fixed

- Separator-dense paragraphs (`a|` repeated tens of thousands of times) no
  longer build one `Vec` and `String` per pipe per frame: the menu check rejects
  on joined length and a capped separator count BEFORE splitting fragments.
- Footnote references with a colliding heading id now land on the footnote:
  `AnchorIndex` remembers the RESERVED id (`fn-a-1` when a heading claimed
  `fn-a`), used by both the reference link and the definition.
- Pipe-heavy lines inside fenced/indented code no longer reject the whole
  document: the table-bomb pre-scan is fence-aware, so a JSON/CSV/ASCII-art line
  with more than `MAX_TABLE_COLS` pipes counts as legitimate code, not a table.
- A `:::` line INSIDE a fence in a `:::` admonition body no longer closes the
  admonition early: the body collector is fence-aware, so the fence stays intact
  and the trailing lines remain in the callout.
- Back/Forward and refresh restore the reading position: document scroll state
  is keyed by (tab, URL) instead of the per-load generation, while a DIFFERENT
  document still starts at the top (the code/table caches keep the
  generation-mixed salt).
- Dark-mode floating surfaces are readable: tooltips, context menus, toasts and
  dropdown popups use the elevated `container` fill with a hairline
  `outline_variant` border (they were `#0d1117` on the `#0d1117` page with no
  border and no shadow).
- The zoom pill no longer disappears when zooming in: responsive breakpoints
  measure the unzoomed window width, so the only visible way back stays visible.
- Non-square favicons (wordmarks, 32x16 `.ico` frames) keep their aspect in the
  tab rail and bookmark tiles instead of being stretched into a square slot.
- Select-all highlights inline emoji images too: the emoji painter fills its
  slot with the selection background instead of chopping the band.
- `Ctrl/Cmd+A` / `Ctrl/Cmd+C` are no longer swallowed by a focused button: the
  whole-page copy path checks `text_edit_focused`.
- Preferences controls yield to the dialog at high zoom / narrow windows: the
  dropdown and the zoom-track widths cap to the available row instead of
  overrunning the card.
- `settings.conf` reads scan every line (the file is already byte-capped), so a
  file with more than 64 keys can no longer hide values the writer itself
  preserved.
- Closed tabs drop their egui-memory state (history search + filter cache,
  bookmark rename/drag drafts): ids are never reused, so otherwise every closed
  tab parks its caches for the whole process.
- The debounced store writer no longer drops the newest queued blob on `flush`:
  `save_async` clears `dirty` before the write, so a failed background write
  re-arms `dirty` and the synchronous exit save writes what is queued.
- Images stuck in the queue past their deadline become failures instead of
  staying "pending" forever and keeping the 120 ms repaint loop alive for the
  whole session.
- The global texture budget is transient for both images and favicons: a refused
  reservation backs off (`IMAGE_BUDGET_RETRY_SECS`) and retries, bounded by
  `MAX_IMAGE_BUDGET_RETRIES`/its favicon equivalent; favicons that only failed
  on the budget are retried once VRAM frees up.
- A response declaring BOTH an image and a concrete non-image Content-Type is
  now rejected for image subresources.
- Local-file document reads use their own slot limiter
  (`MAX_CONCURRENT_LOCAL_READS`), separate from network fetches, so a slow or
  hung filesystem (NFS/FUSE) cannot hold a network fetch slot and starve HTTPS
  navigation, and network load never queues a local open.
- A `#fragment` navigation that fails resolution no longer leaves the pending
  anchor behind to be applied to a document restored later by Back/Retry.
- `file:///a.md?x` and `file:///a.md` are the same document for history dedup,
  matching what the address bar displays.
- `atomic_write` on Windows moves the previous file aside to `.bak` and restores
  it when the swap fails, instead of deleting the target first (a failed rename
  or a crash could lose the whole store); on Unix the directory entry is
  `fsync`ed after the rename, and decryption no longer copies the whole
  plaintext a second time.
- Preferences no longer clips its bottom option: the dialog is taller (480x640)
  so the full card stack fits with the "Delete history" confirmation line and
  the persistence-error notes, and the confirmation line claims a fixed-height
  slot; the inner scroll remains only for viewports smaller than the dialog.
- Session restore: the `session.conf` byte cap is derived from the
  percent-encoded form of a capped URL (`Url::to_string` can triple the byte
  size of non-ASCII input), and the reader accepts exactly what the writer can
  produce.
- "Delete history now" followed by quitting before the deferred wipe ran left
  the ciphertext AND its key on disk, so deleted visits reappeared on the next
  launch; exit now completes a pending wipe before any save, and falls back to
  deleting anyway when a stalled disk exhausts the flush timeout.
- State machine: a locally rejected address while a page is on screen (loaded OR
  kept as a loading backdrop) shows a toast and lets the pending load run
  instead of destroying the page with an error log, and a fetch that already
  produced a result is delivered even when the visual deadline passed between
  two polls (UI stall/suspend).
- Graphics: dark-mode idle/hover/pressed widget states are distinct and keyboard
  focus is visible (neutral active border + focus rings on hand-painted
  rows/buttons); the tab rail gap is the documented 2px; the navbar reserves the
  right width when the zoom pill is hidden; the restore-tabs switch announces
  its label and close-tab buttons carry the tab title.
- Inline code spans: the pill padding (`.4em` / `.2em`, 6px radius) is real
  layout air on BOTH sides instead of paint over the neighboring words, the code
  baseline lands on the body baseline, a soft-wrapped span paints one tight pill
  per row instead of one full-width box spanning the whole two-row label, a span
  that does not fit after the previous word moves to the next row as a unit
  instead of breaking mid-word and reserving an empty first row, and code inside
  links uses the same tight pill instead of the full-line-box `RichText::code()`
  background with its gray band below the text.
- Tab rail: hovering no longer flashes the full 248px overlay in one frame; it
  grows out of the rail with a short `quadratic_out` width tween and collapses
  the same way, with egui's built-in area fade disabled.
- Omnibox/error view: malformed pasted credentials (`http:/user:pass@host`,
  `http:///user:pass@host`, `//user:pass@host`) are stripped by the log
  sanitizer instead of being echoed.
- `file://localhost/...` is accepted as the local machine by the reader and the
  link rewriter alike.
- `session.conf` requires its magic AND layout version, restores only the
  internal `about:history` page for `about:` URLs, and is size-capped before it
  is read at startup.
- Graphics: the omnibox focus ring no longer eats text (`StrokeKind::Outside`),
  the padlock click focuses the field, and clear-X is no longer double-handled;
  the zoom readout uses `TextStyle::Small` so it scales; the page sheet spans
  the full central panel with its always-visible scrollbar hugging the sheet's
  right edge; tables cap columns at 60% of the card; inline code wraps instead
  of overflowing under the scrollbar; the error log uses an auto inner scrollbar
  at 320px; photo galleries center vertically; bullets at 1.8x stay inside the
  24px gutter; and the Preferences dialog clamps to 92%x90% of the viewport with
  an inner scroll.
- Tables: a table that cannot fit and scrolls horizontally keeps every column at
  its measured width up to a wider scroll cap (128px; 112px on narrow windows)
  instead of collapsing all of them to the 48/64px fit floor; fitting tables
  keep the shrink-to-fit behavior.
- Code/table scrollbars: the horizontal bar of a code card or a scrolling table
  now owns a lane UNDER the content; code cards keep their previous outer height
  (16px bottom = 4px padding + 12px lane) and a table card grows by the lane
  only when it actually scrolls (`theme::scrollbar_lane` applied through
  `ScrollArea::content_margin`).
- Tables: centered/right-aligned cells were laid out against the column cap, so
  every such column grew to `max_col_width` (340px) and pushed the rest of the
  table off the card; columns are measured once per frame and rendered at that
  width, and a right-aligned multi-run line keeps its source order (egui's
  wrapping flow flips to RTL inside a right-aligned scope, painting `**a** b` as
  `b a`).
- Code health: shared `ui::row::painted_row` (`RowSpec`) for menu rows; removed
  `eprintln!/println!` leftovers in tests; fixed the `7 routes` doc to
  `MAX_ROUTE_CANDIDATES`; unified `config::tests` imports; `sanitize_title` uses
  `MAX_TITLE_CHARS`; `doc_id_salt` reuses `media::fnv1a`; `card/code_card` share
  `card_with`; `build.rs` watches itself; `debug = 0` in release; `deny.toml`
  denies yanked crates; `.editorconfig` covers `*.toml`; CI `build-test` runs
  under `bash`.
- CommonMark/GFM "insecure characters": `U+0000` is replaced with `U+FFFD` in
  `normalize_body` before parsing, so the raw NUL can reach neither the rendered
  text nor the clipboard (viewer, `plain_text` and anchors share it).
- Table bombs: a single line with more than `MAX_TABLE_COLS` unescaped pipes is
  rejected BEFORE the eager `pulldown-cmark` first pass allocates one tree item
  per cell, and the same cap stops the delimiter padding from building a
  body-sized string; escaped pipes (`\|`) count as cell content, so a valid
  escaped-pipe table is no longer padded into an unparseable one.
- SVG `<use>` expansion: every reference is counted exactly like usvg expands it
  (the old "visiting" set exempted the mutual-recursion graphs usvg still
  expands) and whitespace-padded `href=" #id "` values are trimmed like
  `svgtypes::IRI`, so both shapes no longer slip past `MAX_SVG_TOKENS`.
- Media: a decode that finishes between the deadline snapshot and the failure
  insertion is uploaded on the next poll instead of being discarded as "image
  timed out" (images and favicons); raster decoders apply the EXIF orientation
  before layout; the RGBA conversion moves instead of copying; each document
  runs at most `MAX_CONCURRENT_IMAGES_PER_DOC` jobs so one gallery cannot hold
  every global slot.
- History: a stalled flush can no longer respawn a second writer; a wipe stays
  armed until the draining writer exits; a failed `session.conf` delete is
  surfaced in Preferences instead of leaving plaintext URLs silently behind.
- Protocol: `http:\user:pass@host` (backslash authority) is rejected and
  sanitized like its slash spellings; fetched document bodies are no longer
  copied twice; `./#anchor` destinations resolve to the in-page anchor.
- Markdown: a code span ending in a GFM delimiter (a backtick, `*`, `_`, `~` or
  space) no longer opens a linkified URL right after it; footnote targets are
  collision-reserved against heading ids; frontmatter returns `Cow` (no
  full-body copy when no block is present).
- List markers align by their painted INK, not by their oversized line box: the
  document bullet `·` sits where a body-size middot would on the first text row,
  hugs the text column instead of floating mid-gutter, and keeps its larger
  printed size (2.5x), while ordered `N. ` markers share the first row's
  baseline; ink bounds come from the glyph atlas (`Glyph::uv_rect`), so no font
  metrics are baked in.
- List items keep a compact rhythm: the explicit per-item and per-paragraph gaps
  inside lists are gone (the implicit item spacing alone separates items).
- Inline code pills painted a gray band under the text: epaint sized a
  `RichText::code()` background from the glyph's `line_height`, pinned by the
  viewer to the 24px body line box; code spans now render from a hand-built
  layout job (the baseline lands on the body baseline while the full body line
  box is kept) and paint their own rounded pill tight around the glyphs, while
  select-all and newline-bearing spans keep the full-row background.
- Preferences dropdown rows no longer shift on hover: the default
  `selectable_label` grew its frame on hover, moving the label and pushing the
  whole list; rows are painted into a fixed rect now, and the window has a fixed
  size so toggling any option leaves every control where it was.
- SVG filter rejection was bypassable with namespace-prefixed elements
  (`<s:filter>`, `<s:feTurbulence>` carry neither `<filter` nor `<fe`): a
  structural pass rejects filter elements by LOCAL NAME in the SVG namespace and
  bounds the memoized `<use>` expansion count against `MAX_SVG_TOKENS`.
- The color-emoji cache released nothing to the process-wide texture budget when
  it evicted an entry, shrinking `MAX_TEXTURE_BYTES_TOTAL` until every image
  failed with "global image budget reached"; evictions now release immediately,
  and a full global pool is remembered for `EMOJI_BUDGET_RETRY_SECS` instead of
  caching a permanent miss.
- Alert-marker stripping sliced document text at fixed byte offsets and panicked
  when a multi-byte character straddled `marker.len()` (`> [!NOTE]` + `> Hello
  é` failed the whole route through the panic guard); the prefix test is
  boundary-safe.
- `parse_https` folded `http:/user:pass@host` (one slash) into host `http` with
  the credentials in the request path, the address bar and the route log; the
  userinfo `@` check is now scoped to the authority.
- A local rejection while `Loading` (`mailto:`, oversized input, bad URL)
  dropped the history snapshots, silently losing Forward entries that a failed
  network load preserves; local failures now restore
  `saved_back`/`saved_forward` (and stamp the refresh cooldown) like every other
  failed load.
- "Delete history now" ignored `remove_file` failures and reported success while
  the ciphertext/key survived: failures surface in Preferences, atomic writes
  use per-process unique temp names (two instances sharing one `.tmp` could
  interleave a store that fails to decrypt and wipes the history), and queued
  writes are flushed before a delete or exit.
- Alert callout bars were always the default gray: the look-ahead gave up on the
  first `Text` event, and `pulldown-cmark` splits `[!NOTE]` into `"["`,
  `"!NOTE"`, `"]"`; consecutive text is concatenated before matching, so the bar
  takes the alert accent.
- Percent-encoded and mixed-case fragments never resolved to anchors
  (`#caf%C3%A9` for `café`, `#Section` for `section`): fragments are matched
  percent-decoded and case-insensitively (headings, footnotes and cross-page
  fragments).
- Heading ids desynchronized from the anchor index when the depth guard
  flattened a heading subtree, shifting every later anchor target; the flatten
  path consumes one heading slot per heading.
- Image failure tooltips could show server redirect URLs with credentials
  (`media::fetch` did not sanitize like the document path); reasons are now
  credential-stripped centrally.
- `logical_base("/love//index.md")` returned a trailing-slash base (candidates
  and display URLs showed `/love//`).
- Deterministic decode-cap failures (`image is too large`, `svg: more than N
  markup tokens`, allocation-budget verdicts) were treated as transient and
  re-downloaded the same body once; they are final.
- The frontmatter contract now matches the code in the docs: only a block that
  PARSES as YAML but is not a mapping renders as content, while malformed YAML
  still moves to the next route.
- Several images of one page could fail permanently with `error sending
  request`: every job built its own short-lived tokio runtime while sharing one
  HTTP client, so the pool's idle connections lost the executor that drove them;
  all network futures now run on ONE long-lived runtime
  (`protocol::client::run_network`).
- The document renderer was clipped one gutter short of the sheet's flush
  top/bottom edges: the frame keeps only the horizontal inner margin and the
  vertical margin travels inside the scroll content
  (`theme::content_vertical_pad`), so scrolled letters reach the edge while the
  at-rest spacing is unchanged.
- A run of consecutive large images (site photo strips) was promoted to one
  centered block PER image, so the strip stacked vertically and entries moved
  while their textures resolved; a run of 2+ consecutive block-sized images now
  renders as ONE centered, wrapping gallery row in source order.
- A strip of consecutive inline SVG icons used each asset's intrinsic size (a
  `1em` icon stayed at 16px next to 256px logos clamped to 32px): the strip now
  shares the tallest decoded icon height, like the source site's CSS, while a
  lone icon/badge keeps its size.
- Reloading the same URL could render and copy the PREVIOUS code block from the
  thread-local cache (keys are URL-based and the document can change
  server-side): the cache is dropped when a fetch resolves and has a byte
  budget, and at capacity it evicts one entry instead of clearing everything.
- A failed NEW navigation dropped the page the user came from from `back`; the
  pushed entry now survives, while failed Back/Forward still restore the
  original stacks.
- Copy text dropped the content lines of block-level HTML while the viewer
  showed them, and expanded `:shortcode:` emoji inside fenced/indented code
  blocks (`Select all` disagreed with the code card's Copy); both now keep the
  literal source.
- Frontmatter closing delimiters were detected on `trim()`ed lines, so an
  indented `---` inside a YAML block scalar truncated the metadata early; only a
  column-0 closer counts now.
- A frontmatter `title` written as a map/sequence failed the whole route
  although the docs promised it coerced to `None`; it is now drained correctly.
- Nested list content used a constant indent, so a wide parent marker (`10. `,
  `[x] `) left nested lists hanging to the LEFT of the parent text column; the
  measured marker offset is now honored.
- `Ctrl+W` in the same frame as `Ctrl+T`/`Ctrl+1..9` closed the newly
  selected/created tab instead of the one active when the key was read; `Ctrl+T`
  also left the outgoing page's context menu latched, and the expanded tab rail
  did not share the collapsed rail's scroll offset (different `ScrollArea`
  parents) despite the docs claiming it did.
- `Address` typing/pasting was unbounded until submit (`MAX_URL_INPUT_CHARS` is
  now enforced by the field itself); block-level HTML copy agrees with the
  rendered inert source.
- Hostile markdown (`> > > …`, `- - - …`, `***…`) could overflow the UI thread's
  stack (an uncatchable abort) on a few KB of input: the renderer now has a
  depth guard (`MAX_RENDER_DEPTH`) and renders deeper content flattened instead
  of recursing.
- The table-delimiter normalization padded pipe lines INSIDE fenced or indented
  code, corrupting the rendered code and the copied text; code regions are now
  left byte-for-byte.
- AVIF images skipped the `MAX_TEXTURE_DIM` downscale and could upload up to 24
  MP textures as-is; every decoder now goes through the same `finish` downscale
  (the real decoded frame is re-checked first).
- Jobs registered during a frame (image `ensure`, favicon probes) did not
  schedule a repaint tick, so their spinners could hang on an idle window until
  the next input event; `App::ui` now schedules the 120ms tick.
- Cancelling a fetch while waiting for a body chunk could linger up to the 10s
  stall timeout; the chunk wait now races the cancel flag (~20ms).
- Document/table/code/error scroll state was shared between tabs showing the
  same URL; ids now mix the tab id.
- Comments and docs no longer quote a fixed fallback budget: the visual timeout
  derives from `FETCH_SLOT_WAIT_SECS` + `MAX_ROUTE_CANDIDATES` x
  `REQUEST_TIMEOUT_SECS` + slack (`TOTAL_FETCH_TIMEOUT_SECS`).
- Copy (`plain_text`) now expands `:shortcode:` emoji exactly like the renderer
  does for text nodes, while code spans keep the literal source, so the
  clipboard matches what is on screen.
- An image/favicon job that resolved between the readiness and deadline
  snapshots was declared failed and its decoded pixels were dropped: ready jobs
  are uploaded first, the deadline re-checks readiness, and the race
  re-insertion keeps the ORIGINAL `since`.
- An AVIF frame whose real dimensions disagreed with the container metadata was
  copied into a `Vec::with_capacity` sized from the frame BEFORE the caps were
  re-checked, so a huge frame could abort the process on OOM; the real frame is
  validated (dims + decoder budget) before any allocation.
- Favicon probes ran three sequential requests on the image timeout (45s each),
  pinning one of four image slots for up to 135s, past the 65s entry deadline
  and starving document images on a dead host; probes now use their own 10s
  budget (`FAVICON_REQUEST_TIMEOUT_SECS`).
- A visual timeout dropped the page that was on screen and replaced it with an
  error log; when a backdrop exists it now comes back (history snapshots and
  decoded textures intact) and the timeout is reported as a toast, while a first
  load without a backdrop still settles into `Failed`.
- Nested `.md`/`.markdown` dotfiles (`/a/.md`) collapsed to the directory route:
  the extension is stripped only when a real file name precedes it.
- The SVG sniffer rejected documents with comments or a DOCTYPE before the root
  (common in exported files); well-formed prolog pieces are skipped.
- The SVG pre-scan (DTD/filter detection) compared case-insensitively at every
  byte offset over up to 10 MiB; it now scans for the first byte first.
- A local address error (`mailto:`, bad URL, oversized input) stored the raw
  unbounded input in `address`/`asked`; both are bounded like every other branch
  now.
- The History window re-lowercased every URL and title of up to 5000 entries on
  each frame; the filtered index list is cached on a store generation counter
  now.
- Two threads racing the first request could build two HTTP clients / two
  network runtimes and drop one unused; first-use construction is serialized
  now.
- A flattened hostile subtree joins blocks with `\n` into one fragment, but the
  bidi reorder only rewrote the FIRST paragraph, so everything after the newline
  disappeared on screen; every paragraph is reordered now.
- A locally rejected URL (bad input, oversized paste) echoed pasted `user:pass@`
  credentials into the omnibox, the error view and "Copy log"; both stored
  strings and the parser's error text are credential-sanitized.
- A failed background history write dropped the store's dirty flag at enqueue
  time, so the visit was never rewritten and a later success could not clear the
  Preferences warning; the store stays dirty until a write lands and the warning
  clears on success.
- A `history.key` copied in with wider permissions was silently trusted; it is
  tightened to 0600 on load (fail closed when it cannot be).
- History recorded over-long URLs truncated with `...`, so clicking the row
  opened a different (usually 404) page; over-long URLs are skipped whole.
- A standalone linked image was not clickable while loading or after a permanent
  failure; every placeholder state is wired like the decoded image now.
- The `[image: alt]` placeholder always rendered italic, even without source
  emphasis; it keeps the enclosing emphasis like the real image.
- "Select all"/Copy pasted the leaked first-block skip link that the viewer
  drops; clipboard extraction drops exactly the same block.

### Security

- Remote memory amplification: `protocol::rewrite::DestCache` keeps ONE map (the
  image form is derived from the link form) under a `MAX_DEST_CACHE_BYTES`
  budget, and a resolved destination past `MAX_URL_INPUT_CHARS` is inert; a
  hostile link farm (~250k distinct destinations in a 5 MiB body) retained
  hundreds of MB per tab past every documented cap, and past the budget the
  remaining links render as plain text.
- Same-document anchor normalization now requires the same ORIGIN:
  `//other.example/same/path#x` collapsed to `#x` and scrolled the current page
  instead of navigating, and `//server/share/x.md#s` from a local document could
  bypass the remote-file-share rejection the same way.
- Credential sanitization covers blocked non-hierarchical pastes:
  `mailto:user:pass@host` displays as `host` (the `mailto:` spelling stays
  intact only when no password follows the scheme colon).
- `atomic_write` creates its temp file with `create_new` (O_EXCL) and a fresh
  sequence on collision, so a symlink pre-planted at a predictable temp path can
  no longer redirect settings/session/store writes.
- A redirect landing on a URL past `MAX_URL_INPUT_CHARS` is rejected before the
  body is read: `Location` is server-controlled while the display, history and
  rewrite paths assume the input cap.
- Document fetches require EVERY declared `Content-Type` value to be a markdown
  type (a `text/html` + `text/markdown` label got a precedence game), matching
  the image gate.
- "Forget on exit" covers the whole disk surface: the plaintext `session.conf`
  snapshot is never written while that retention mode is active, selecting the
  mode turns "restore tabs on startup" off and deletes any leftover, and startup
  refuses to restore a stored session before this normalization runs.
- Store-writer error reporting recovers poisoned locks instead of silently
  dropping "could not save history/bookmarks".
- `normalize_stored_url` fails closed when credentials cannot be removed
  (`set_username`/`set_password` errors) instead of storing the URL.
- Store keys are installed atomically (`src/crypto.rs`): the 32 bytes are
  written and synced to a unique private temp file and linked into place with
  `hard_link`, so two instances creating the key at once can only read the
  winner's COMPLETE key; a corrupt (wrong-size) key is repaired under a
  best-effort cross-process lock with stale-lock takeover, so two instances
  repairing at once cannot delete each other's fresh key, and a valid key that
  cannot be tightened to user-only permissions is refused fail-closed, never
  deleted.
- Encrypted stores keep an undecryptable blob as a `.bak` (missing or
  regenerated key, corruption, tampering) before a later save could replace it,
  so restoring the matching key still recovers the data; `Delete history now`
  removes the backup too.
- Turning "Restore tabs on startup" off deletes the plaintext `session.conf`
  even when a previous delete failed, and removes the orphaned `.tmp.*`/`.bak`
  siblings a crash can leave behind.
- `settings.conf` is never rewritten from a failed read: a read error
  (permissions, invalid UTF-8) no longer looks like an empty file and silently
  drops the other preferences, and persistence failures surface in Preferences
  instead of "reverting" on the next launch.
- Stored history/bookmark URLs keep their exact path and query: only authority
  credentials (`user:pass@`) are removed from the URL, while loading a store
  normalizes the URLs too and the bookmark star compares the same way.
- SVG `<use>` expansion-bomb counter bypass: character references
  (`href="#bomb&#9;"`, `&#xA0;`) survive XML attribute normalization while
  `svgtypes::IRI` stops an id only at an ASCII space, so a `trim()`-based
  resolver matched no `id` while usvg still expanded the graph un-counted; the
  structural pass now mirrors `IRI::from_str` byte for byte (ASCII whitespace
  only, trailing data rejected like usvg does).
- Credential sanitization covers every authority spelling (one slash, extra
  slashes, backslash, protocol-relative), so malformed pastes can no longer
  reach the address field, the error view or "Copy log".
- Local reads accept only REGULAR files: a FIFO/device can no longer block
  `open` forever and permanently consume a global fetch slot; settings, session
  and history reads refuse non-regular files too, and the history file length
  check is 64-bit (a 32-bit wrap could read a multi-GB file).
- `atomic_write` removes its partial temp file on any create/write/sync error (a
  full disk no longer litters a `.tmp` per history retry).
- History `record` strips credentials defense-in-depth; tab/window titles
  sanitize `asked` before display; the title filter drops variation selectors,
  tags and extra format chars; and the SVG structural pass also rejects
  `script`, `foreignObject`, `animate`, `animateTransform` and `set`.
- Local file access is confined to user-opened documents: a remote page can
  neither render nor navigate `file:` destinations, `file://host/` remote shares
  are rejected, and every local read reuses the document/image caps and the
  mandatory UTF-8 rule.
- Namespace-prefixed SVG filters (`<s:filter>`, `<s:feTurbulence>`) bypassed the
  byte pre-scan and could burn a decode slot for years on a ~300-byte document
  (`numOctaves` has no resvg-side bound); filters are now rejected by local name
  in a structural XML pass.
- Nested `<use>` references expand exponentially inside usvg from a tiny source
  (a 939-byte document parsed for 5.5 s and hundreds of MB before usvg's own
  node limit); the memoized expansion count is capped at `MAX_SVG_TOKENS` before
  parsing.
- `http:/user:pass@host` (single slash) no longer folds pasted credentials into
  a request path to a single-label host.
- Image failure reasons (tooltips) are credential-sanitized like the document
  error view.
- SVG decoding is hardened against parser and CPU bombs: `<!DOCTYPE>` /
  `<!ENTITY>` (roxmltree expands internal entities without a total-size bound)
  and `filter`/`fe*` markup (`numOctaves`, morphology and convolution parameters
  have no resvg-side limits) are rejected before parsing, inline `data:` hrefs
  are no longer resolved, and a UTF-8 character split at the 1 KiB sniff window
  no longer misclassifies valid SVGs.
- AVIF alpha items are dimension-checked before decoding: `avif-decode`
  validated the alpha size only after rav1d allocated the frame, so a tiny color
  item plus a huge alpha item could exhaust memory.
- Raster decoders check the header dimensions before `decode()` (image's
  `Limits` has no pixel-count limit, so an 8192x8192 67 MP image was fully
  decoded before being rejected), and JXL checks its output byte size before
  `DynamicImage::from_decoder` (which aborts on OOM instead of failing).
- The Content-Type gate now runs BEFORE the body is streamed for documents and
  images: a hostile page can no longer force up to 256 x 10 MiB of wrong-type
  downloads dropped only after buffering.
- YAML frontmatter aliases are never expanded: the root check reads the mapping
  directly and skips unknown fields with `IgnoredAny`, closing a 32 KiB alias
  fan-out that materialized gigabytes through `serde_norway::Value`.
- Document complexity is bounded: `MAX_EVENTS_PER_DOC` (500k) rejects
  event-stream bombs and `MAX_RENDERED_CODE_LINES` (5000 per document) stops a
  few MB of newlines from rendering millions of labels per frame.
- `parse_https` recognizes `scheme://…` as a scheme before the userinfo
  heuristic: `http://user:pass@host` is blocked instead of being sent to host
  `http` with the credentials in the request path, address bar and route log;
  non-hierarchical schemes (`data:`, `javascript:`, ...) are blocked, and
  single-label hosts with ports (`nas:5000`) work.
- Image/favicon retries classify the real error strings (network errors, stalls,
  truncated transfers, 5xx/429 retry; 4xx, content-type rejections and caps do
  not), and every job has a wall-clock deadline so a decoder that cannot be
  interrupted stops counting as pending.
- Remote titles drop Unicode bidi/zero-width format controls (RLO spoofing) and
  `truncate_chars` respects its documented total cap; fenced language chips and
  footnote labels are bounded before rendering (a 4 MB fence no longer allocates
  a lowercased copy per frame).
- Failed favicon probes no longer permanently exhaust the 128-origin cap.
- Favicons now decode at `MAX_FAVICON_DIM` (256 px; they draw at 20 px) and
  reserve their bytes in the process-wide texture budget, so 128 hosts can no
  longer approach ~2 GiB of VRAM outside the documented bound.
- SVG sources are token-capped (`MAX_SVG_TOKENS`) before `usvg` parsing: the
  upstream parser has no node limit, so a 10 MiB markup bomb could pin a decode
  slot for minutes.
- Image retry docs match the code: truncated transfers surfacing as decode
  errors are retried once, while content-type rejections and cancellations are
  final.
- The SVG structural pass could overflow the decode thread's stack on deep
  nesting: the expansion walk is iterative now, bounded only by
  `MAX_SVG_TOKENS`.
- The structural pass mirrors usvg's namespace acceptance (`None | SVG_NS`): a
  namespace-less `<use>` expansion bomb passed as ordinary elements; it also
  follows a `<use>` whose target is itself a `<use>` and resolves `href` with
  usvg's precedence (unprefixed first), closing a forged foreign-namespace
  `href` decoy.
- SVG `marker` elements are rejected like filters: usvg re-converts a marker's
  subtree for EVERY path vertex (no cache), so a small marker plus a long path
  built an unbounded render tree no source token cap could see.
- Rendered block count is bounded (`MAX_RENDERED_BLOCKS`): `MAX_EVENTS_PER_DOC`
  bounds parsing memory, but a ~0.7 MB body of tiny paragraphs laid out ~500k
  labels every frame and froze the UI; past the budget the walk consumes blocks
  without rendering them and `Copy` still yields the full text.
- Switching to "Forget on exit" flushes the queued history writer and wipes the
  ciphertext/key immediately.
- Small buttons no longer jump when pressed or keyboard-focused: the `active`
  widget state had lost its 1px `bg_stroke` width (`Stroke::NONE`), and a
  `small` button subtracts that width from vertical padding it has already
  zeroed, so the pressed (or keyboard-focused) code `Copy` and error `Copy log`
  allocated 2px less than their idle state; every widget state now keeps ONE
  stroke width (a transparent 1px active stroke paints nothing), and the
  `about:history` footer's delete button keeps one fixed width while the
  two-click confirmation swaps its label ("Delete history now" / "Confirm
  delete").

### Performance

- `DestCache` stores one map instead of two and skips inert destinations
  entirely, halving its retained strings on link-dense pages.
- Code/table width LRU eviction is one sorted pass instead of a `min_by_key`
  rescan per victim (`O(cap²)` inside a single frame when a new generation
  refilled a full cache).
- The media timeout sweep prunes its queue with a `HashSet` instead of a
  `Vec::contains` per entry (O(n²) on a page-wide timeout).
- Viewer per-frame allocations are trimmed: code cards no longer re-truncate an
  already-bounded info string, and an unlabeled fence borrows `"code"` instead
  of allocating it per frame.
- Link/image destinations resolve ONCE per fetch
  (`protocol::rewrite::DestCache`, built from the parsed events + anchors)
  instead of `Url::parse` + `join` in the per-frame fragment tree; an
  unknown-anchor or over-cap destination is stored inert.
- Table column widths are measured once and cached (content hash + column caps +
  selection; bounded LRU with a byte budget), while tables holding images or
  color emoji bypass the cache since their widths change when a texture
  resolves; the invisible per-frame sizing pass no longer re-runs `render_frag`
  over every cell line of every table.
- Highlighted code blocks cache their laid-out line galleys (keyed by
  theme/selection/font size), so a repaint clones an `Arc` per line instead of
  rebuilding a `LayoutJob` and copying every span's text.
- Image cache: `pump()` returns immediately when no URL is queued, so the
  per-frame `ensure` for every visible image no longer rescans the whole entry
  map (O(N²) per frame on a page of settled images).
- Color-emoji split: a scalar pre-scan (pictograph range, VS16, ZWJ, keycap)
  skips the grapheme segmentation and emoji-table lookups for emoji-free
  non-ASCII text (CJK, Cyrillic, Arabic).
- Code-block cache keys: a per-tab document generation re-keys only that tab;
  normalized table delimiters, bare-URL linkification and menu detection each
  scan once; the history filter cache is shared behind an `Arc` (a hit no longer
  copies the index); errors are classified once per frame; color-emoji textures
  are keyed without a per-lookup `String` allocation, evicted LRU instead of
  FIFO, and a budget retry schedules its own repaint; a global texture budget
  miss gets one bounded retry instead of a permanent failure.
- RTL fast path for pure-ASCII lines; stable quote ids avoid per-frame `format!`
  churn; ordered-marker widths cached per (marker, size) with a 512-entry bound;
  inline-code font metrics cached per (body, mono) size; theme-pair logos
  compared token-by-token without `Vec<String>` allocs; image expiry scan avoids
  cloning the map.
- The `usvg` options (the ~25 MB bundled font database) are built once per
  process (`SVG_OPTIONS`) instead of once per image-decode thread.
- The color-emoji font, its ~14 MB shared SVG document index and the SVG
  rasterizer are prewarmed on a background thread at startup, so the first
  document emoji never parses them mid-frame.
- One rendered prose/HTML/table block is capped at
  `config::MAX_RENDERED_BLOCK_CHARS` (256 KiB): a crafted multi-MB paragraph
  became one giant label, laid out (and its layout job rebuilt) every frame.
- `coalesce_text` appends same-style runs in place (`to_mut().push_str`) instead
  of `format!`-reallocating the whole run on every merge (O(n2) per frame on a
  paragraph of hundreds of thousands of fragments).
- The code-block cache is a true LRU: a document with more blocks than
  `CACHE_MAX` no longer re-copies or re-tokenizes the blocks on screen.
- History saves: the encrypted blob is built on the UI thread and written (with
  `fsync`) by a serialized worker thread, so a slow disk never stalls a frame.
- `plain_text` runs on the already-normalized body at fetch time instead of
  normalizing the document a second time.
- The code-block cache holds at least as many blocks as one document can render
  (`MAX_RENDERED_BLOCKS`): the old fixed 64-entry LRU evicted every block before
  the next frame asked for it again (sequential-scan thrash); tokenization is
  also skipped once the per-document line budget is spent.
- Per-frame copies of server-controlled strings are bounded at capture: image
  `title`/`alt`, link/image destinations and HTML block buffers no longer
  concatenate or `Url::parse` multi-MB values before their display caps.

## [0.2.1] - 2026-10-04

Adds a vertical tab rail with favicons, full markdown image rendering with
anti-abuse caps, keyboard shortcuts and a right-click menu. Reworks fetching,
parsing and texture budgeting, and fixes selection, tables, lists and
address-bar behavior.

### Added

- Vertical tab rail: a 52px collapsed rail shows one favicon per tab and expands
  on hover into a 248px overlay (favicon + title + close + `+ New Tab`) without
  reflowing the page; active pill + accent bar, loading spinner per tab and
  tooltips.
- Site favicons: a background per-origin probe of `/favicon.ico` then
  `/favicon.png` uses a bounded session cache and falls back to a home/globe
  glyph.
- Markdown images: PNG, JPEG (`.jpg`/`.jpeg`), GIF, WebP, JPEG XL and AVIF
  decode 100% in Rust (`image`, `jxl-oxide`, `avif-decode`) on background
  threads through a per-document texture cache; every image renders at its
  natural pixel size (never upscaled) and clamps to the reading-column width
  when wider; standalone paragraphs center as blocks while inline occurrences
  stay in the text flow, images inside links stay clickable,
  loading/failed/budget states show accessible placeholders, and alt text
  becomes a caption/tooltip.
- Image anti-abuse caps: 10 MiB per image, 24 MP and 8192 px per side, 512 MiB
  decoder allocation, 256 MiB GPU texture budget, 256 images per document and 4
  concurrent image jobs kept separate from document fetches.
- Keyboard shortcuts: Ctrl/Cmd+T (new tab), Ctrl/Cmd+W (close), Ctrl/Cmd+R / F5
  (refresh), Ctrl/Cmd+L (focus address), Ctrl/Cmd+A (select all page text) and
  Ctrl/Cmd+1..9 (select tab); new tabs focus the address field.
- Right-click menu: Copy (the drag selection when present, else the whole page
  text) and Select all, plus Open link in new tab at the top when the click
  lands on a link; link hitboxes are collected while the document renders.
- Compression: transparent `gzip`/`brotli`/`deflate` decompression via reqwest
  features cuts markdown transfer size several times, with anti-abuse caps
  applied to the decompressed stream.
- New modules: `media/{fetch,decode,cache,favicon}`, `ui/icons`,
  `ui/viewer/image` and `ui/viewer/overlays`, with unit tests for format
  sniffing, PNG decode/downscale, bomb caps, table headers and a headless rail
  render.
- `MdowserApp` self-heal: unit tests cover an empty tab vec, an out-of-bounds
  index and closing the last tab.
- `scripts/build-test.sh`: a `cargo doc` step with `RUSTDOCFLAGS="-D warnings"`
  fails the pipeline on broken intra-doc links.
- Fetch limiter: a global concurrent-fetch limiter
  (`config::MAX_CONCURRENT_FETCHES`, `FETCH_SLOT_WAIT_SECS`) with cancel-aware
  slot acquisition stops rapid multi-tab navigation from accumulating unbounded
  threads/buffers.
- `Fetched.events`: the document is parsed once at fetch time into owned
  `pulldown-cmark` events (normalization + parsing off the UI thread), so
  repaints (scroll, selection) never re-run the parser on up to 5 MiB.
- Token-span cache: a bounded multi-entry cache in `ui::viewer::code` keyed by
  (block hash, length, language) stops a document with several code blocks from
  re-highlighting every block on every frame.
- `markdown::document::parse_events()`: returns owned `'static` events with unit
  tests, plus a headless renderer smoke test covering loose lists, nested
  quotes, rules inside quotes, tables, footnotes, code blocks and more.
- `Tab::address_dirty` + `mark_address_edited()`: real edit tracking from the
  navbar/home fields, so a type-and-revert is no longer clobbered on resolve.
- `THIRD_PARTY_NOTICES.md`: license summary of the dependency graph plus the
  full OFL-1.1, Ubuntu-font-1.0, Unicode-3.0 and BSL-1.0 texts; shipped in every
  dist archive and inside the macOS `.app` `Contents/Resources/`.
- Accessibility: painted widgets that screen readers cannot infer now carry
  names — every toolbar icon button reports its tooltip as the label plus
  enabled state, and tab rows report title + selected state.
- Ignored network smoke test: `cargo test -- --ignored network_images` exercises
  the real fetch → Content-Type gate → decode pipeline for PNG, JPEG, WebP, ICO
  and AVIF.

### Changed

- Version: bumped to `0.2.1` in `Cargo.toml` and `Cargo.lock`, with `USER_AGENT`
  following via `CARGO_PKG_VERSION`.
- `protocol::client`: now exposes a shared `get_bytes` (cancellable
  connect/status/content-type/streaming caps) used by both document candidates
  and image/favicon subresources; `global_client` is crate-visible.
- Tab labels: dropped the `Tab N:` prefix (favicons identify tabs now) and
  capped at 30 chars, while the OS window title keeps the prefix.
- MSRV: the minimum Rust version is 1.98 for the pure-Rust AVIF decoder, and
  `deny.toml` documents the MPL-2.0 file-level copyleft of `avif-parse` while
  GPL/AGPL stay denied.
- Docs: README/ARCHITECTURE/SECURITY/CHANGELOG document the image pipeline, the
  rail, shortcuts and the new caps.
- `README.md`: restructured as a product overview and the historical bug-by-bug
  audit list was removed (the change record lives in this file).
- `deny.toml`: the license allowlist now covers the permissive + Unicode + font
  set used by the GUI/TLS stack (`CC0-1.0`, `MIT-0`, `Zlib`, `Unicode-3.0`,
  `BSL-1.0`, OFL-1.1, Ubuntu-font-1.0, LLVM exception) while copyleft stays
  denied.
- CI `audit` job: installs `cargo-audit`/`cargo-deny` only when missing, so no
  more `|| true` masking install failures.
- `scripts/build-dist.sh`: fails loudly when the `Cargo.toml` version cannot be
  parsed (no empty-tag artifacts).
- Address bar: `Enter` submits whether the field kept or lost focus (navbar +
  home).
- Pipeline docs: synced `Makefile`, `README.md`, `CONTRIBUTING.md` and the
  `scripts/build-test.sh` header so `make test` is sh syntax + fmt + clippy +
  test + doc + debug.
- Repaint cadence: while loading, a resolved load repaints immediately, a
  backdrop repaints every 300 ms and a first load every 100 ms.
- `ui::viewer::markdown::show_markdown`: renders recursively from the pre-parsed
  events (correct nesting), and the top fade overlay is drawn only at rest
  (scroll idle) instead of every throttled repaint.
- Blockquotes: the viewer renders one indent + bar per depth level, and rules
  and H1/H2 separators inside a quote/list item stay indented.
- YAML: `serde_yaml` (archived upstream) is replaced by the maintained
  `serde_norway` fork (same API; `serde_yml` is unsound, RUSTSEC-2025-0068).
- `frontmatter::split`: never errors on a closerless leading `---` and never
  swallows a delimited block without a YAML mapping — both stay CommonMark
  content.
- Tab cap: `config::MAX_TABS` makes new tabs beyond the cap show a toast instead
  of growing document memory without bound.
- Navbar: degrades below 720 px (`+ New` shrinks to 64 px, `Go` to 44 px, the
  `%` readout hides and the zoom-in tooltip reports the factor) so the URL field
  and zoom controls never clip off-screen.
- Decoder allocation cap: the `512 MiB` literal in `media::decode` moved to
  `config::MAX_DECODER_ALLOC_BYTES`, like every other anti-abuse bound.
- CI `audit` job: installs `cargo-audit`/`cargo-deny` from prebuilt binaries;
  the workflow also runs weekly (fresh RustSec DB) and on manual dispatch.
- Release CI: publishes `SHA256SUMS.txt` next to every artifact, the same
  guarantee `make dist` gives locally.
- `scripts/build-dist.sh`: checksums accept `shasum` (macOS/Perl) or `sha256sum`
  (GNU coreutils) and fail loudly when neither exists.
- `.deb`: now ships `/usr/share/doc/mdowser/copyright` and declares `Depends:
  ca-certificates` (the system trust store TLS verification actually needs).
- Image policy: markdown carries no width/height, so every image renders at its
  NATURAL pixel size (never upscaled) and scales down to the reading-column
  width when wider; standalone paragraphs center as blocks and inline
  occurrences stay in the text flow; `fit_inline` is replaced by the tested
  `fit_to_width`.

### Fixed

- Selection visibility: native (drag) text selection was invisible because egui
  paints selected glyphs with `selection.stroke.color` and the theme set
  `Stroke::NONE` (transparent); the stroke is now width-0 with the readable
  `on_surface` color and the same pair drives the "Select all" highlight.
- Copy selection: the right-click "Copy selected text" always copied the WHOLE
  page and never read the native selection; `Copy` now copies the real selection
  through egui, Select all + Copy / Ctrl/Cmd+C copies the full plain text,
  `Ctrl/Cmd+A` selects the page.
- Select-all highlight: footnote markers, HTML blocks and callout badges were
  skipped and now highlight like the rest of the document.
- Context menu: the anchor was clamped with global `viewport().inner_rect`
  coordinates against local pointer positions and a hardcoded 240px menu size;
  the popup now relies on the Area's own window constraint and never spawns
  off-screen at any size.
- Table headers: pulldown-cmark closes `TableHead` without a `TableRow`, so
  accumulated header cells were cleared by the first body row; headers now
  commit on `End(TableHead)` (regression test `renders_table_headers`).
- Expanded tab overlay: it mixed global screen coordinates
  (`viewport().inner_rect`) with local layout rects and grew past the window,
  dropping the pinned `+ New Tab` off-screen; it now uses the local
  `viewport_rect()`.
- Collapsed rail: it kept drawing under the expanded overlay, so old rows and
  the old `+` showed through; rail content is skipped while the overlay covers
  it.
- Tab expansion: tab rows and `+ New Tab` moved when expanding; both states now
  share paddings, row height/gap, icon column, action height and scroll offset,
  so expanding only reveals title/close.
- Window background: the clear color was eframe's default dark gray;
  `clear_color` now returns the theme `panel_fill`.
- Scrollbar: the floating scrollbar used `fg_stroke` (near-black) for its
  handle; handles now use the gray container tone with tuned opacities.
- Content frame: no longer draws the hard 1px stroke plus black shadow in dark
  mode.
- Focus cue: toolbar/home address fields showed none (`selection.stroke` was
  width-0 to avoid stray lines), so a primary focus ring is now painted
  explicitly.
- Buttons and shadows: flat buttons no longer draw the default outline ring, and
  dark-mode popup/window/overlay shadows were removed.
- AVIF safety: images now re-check the REAL decoded frame against the
  dimension/pixel caps after `to_image()`, not just the container `ispe`
  metadata, so a container whose header lies about its size cannot slip a bomb
  past the pre-decode check.
- Image paragraphs: the path no longer carries a production `expect`, so the
  renderer can never panic the UI thread if `image_only`'s invariant regresses.
- MSRV docs: `CONTRIBUTING.md` stated Rust ≥ 1.85 while `Cargo.toml` requires
  1.98; the note now matches the manifest.
- `deny.toml`: it was broken on current cargo-deny; `cargo deny check` now
  passes with only the documented accepted advisories ignored, and every
  non-MIT/Apache component is listed in `THIRD_PARTY_NOTICES.md`.
- `scripts/build-dist.sh`: one failed macOS arch aborted the whole multi-OS run
  (`set -e` + unguarded `build_for`); macOS now skips the group with a reason
  like Linux/Windows, and the missing `nasm` requirement of `rav1d`'s x86_64
  kernels is detected and auto-installed.
- Content-Type gate: images served as `binary/octet-stream` or labeled with the
  legacy `image/x-png` / `image/pjpeg` aliases were rejected; `is_octet_stream`
  now covers both octet-stream spellings and the image type list includes the
  legacy aliases.
- Image timeouts: images shared the 10s document request timeout and aborted
  5-10 MiB images mid-transfer on slow links; `get_bytes` now takes a
  per-request timeout (documents keep 10s, images get
  `config::IMAGE_REQUEST_TIMEOUT_SECS`, 45s), while the 10s per-chunk stall
  cutoff still aborts dead connections.
- Heading images: an image inside a heading (`# ![Logo](logo.png) Title`)
  rendered as alt text and was never registered for fetching; heading fragments
  now render image frags through the shared image widget.
- Decoder panics: a panic inside an image decoder (`image`, `jxl-oxide`,
  `avif-decode`) and the YAML parser dropped the promise sender without a value,
  and polling such a promise PANICS the UI (`poll-promise` behavior);
  `util::guard_panics` now wraps every background fetch entry point
  (`fetch_blocking`, `fetch_image_blocking`, `fetch_favicon_blocking`) and turns
  the panic into a bounded error placeholder.
- `SECURITY.md`: supported versions corrected to `0.2.x` (was stale `0.1.x`).
- `MdowserApp::active_tab_mut`: now panic-free — an empty tab vec recreates the
  initial tab and an out-of-bounds index clamps.
- Route logs: all lines are now bounded, with the success `Content-Type` echo
  and `BadFrontmatter` details truncated like every other server-influenced
  line.
- Code blocks: line counting scans the buffer once per frame (was twice).
- Cancelled navigations: they abort during connect/headers
  (`futures_util::select` race against the cancel flag).
- Tab rows: hover no longer flickers over the title/close button (hover is the
  union of row + children), the row background fills through a reserved
  `Shape::Noop` slot so it stays behind the text.
- Right-click menu: clamps to the screen (never spawns off-window), closes on
  scroll/zoom, and clicks on the content scrollbar no longer open it.
- Error view: the status dot allocates its rect (stays aligned at any zoom), and
  oversized-body logs classify as `Other` instead of a misleading "No
  connection".
- Zoom: headings cap at 40 pt so 3x zoom cannot force horizontal scrolling, and
  the quote-level bar derives its gutter from `spacing.indent` instead of the
  hardcoded `-18`.
- Ordered lists: use the parsed `is_ordered` flag instead of inferring it from
  `marker.contains('.')`.
- Pipe-menu heuristic: rejects all-lowercase prose (`pan | vino | queso`) while
  keeping mixed-case menus (`Home | docs`).
- Table columns: max column width is capped by an equal share of the card, so
  narrow windows no longer overflow just because one column is wide.
- Loose lists: paragraphs inside list items render with the item marker/checkbox
  on the same row and continuation paragraphs are indented, so bullets are no
  longer lost.
- Leading `---`: such documents no longer fail or lose content — a closerless
  `---` (thematic break) is content, and a delimited block without a YAML
  mapping renders instead of being swallowed by the metadata skipper.
- History: Back/Forward during a pending load no longer corrupts history because
  the in-flight URL becomes the current position before the stacks move;
  pressing Back/Forward onto the already-loading URL is a no-op instead of
  mutating the stacks.
- Visual timeout: derived from the real worst case (slot wait + 7 candidates +
  slack) so a legitimate load is never abandoned at 75 s.
- Right-click menu lifetime: it no longer survives a tab switch and closes when
  the window moves.
- Code blocks: the trailing newline no longer renders as a phantom last line,
  and blank lines keep their line number.
- List continuation: lines align to the measured marker width (ordered `10. `
  and checkboxes included), not a fixed 14 px.
- Ordered-list numbering of loose items: checkbox/text/marker are consistent and
  the marker is drawn exactly once.
- `parse_https`: rejects bare emails (`user@domain.tld`) instead of navigating
  to the domain, and `NotHttps` errors no longer echo credentials.
- YAML `title`: accepts i128/u128 integers, so huge IDs no longer fail the whole
  frontmatter.
- Nested lists: render structurally indented (one step per level) with
  continuation lines aligned to their own first line.
- Item markers: drawn at the item's own indentation when the first block is a
  blockquote or a nested list.
- YAML blocks: a delimited block whose root is not a mapping (sequence, scalar
  containing `:`, comment-only, `null`) renders as CommonMark content instead of
  failing the route.
- Home and fresh-error pages: no longer repaint every 500 ms forever; cooldown
  ticks are scheduled only while a real countdown is pending.
- Disabled buttons: disabled primary/tonal buttons (`Go`, `Retry`) keep a
  readable full-opacity label.
- Refresh/server-home: after a failed load they act on the failed `asked` (and
  on the pending URL while loading), not on the previous page that `current`
  still pointed at.
- Window title: the URL fallback is truncated like every other title path.
- History duplicates: self-duplicates are healed on load, and navigation commits
  the pending URL before the reload check.
- `Content-Type`: parameters with quoted values containing `;`
  (`boundary="a;b"`) are accepted instead of rejected.
- Retry: now disabled with a fix-the-address hint on local rejections
  (`BadUrl`/`NotHttps`), where retrying can never succeed.
- H1/H2 rules: the rule inside a list item aligns with the heading text, not the
  item edge.
- URL cap hint: the error-view hint derives from `config::MAX_URL_INPUT_CHARS`
  (no hardcoded 4096 drift).
- Toasts: copy/share toasts are English like the rest of the UI.

### Security

- Texture budget: enforced twice — per document (`config::MAX_TEXTURE_BYTES`,
  256 MiB) and process-wide (`config::MAX_TEXTURE_BYTES_TOTAL`, 512 MiB);
  reservations are released on navigation, tab close and cache drop (unit test
  covers reserve/release/saturation).
- AVIF: the post-decode dimension re-check (see Fixed) closes the
  container-vs-bitstream mismatch window left by the pre-decode `ispe` check.

## [0.2.0] - 2026-10-02

Introduces typed error classification, centralized display bounds and the
project documentation set (architecture, contributing, security, license texts),
plus audit tooling and CI hardening. Splits the viewer into focused modules,
removes dead code and fixes stalls, highlighting and unbounded inputs.

### Added

- `config::USER_AGENT`: documented as the defined browser User-Agent, with a
  unit test.
- `ui::error_view`: classification (`Network` / `NotFound` / `NotMarkdown` /
  `BadAddress` / `Other`) with unit tests.
- Tab titles: `Tab::title` / `window_title` are always prefixed (`Tab N:`) with
  an `Untitled` fallback.
- Docs: `ARCHITECTURE.md` (module map + contracts) and `CONTRIBUTING.md` (quick
  start, standards).
- Supply chain: `deny.toml` (cargo-deny policy), automated weekly cargo/actions
  dependency updates and a CI `audit` job (`cargo audit`, `cargo deny check`).
- Bounds: `config::CHUNK_TIMEOUT_SECS` (10s per-chunk stall timeout),
  `MAX_ERROR_LINE_CHARS`, `MAX_ADDRESS_DISPLAY_CHARS`, `MAX_ASKED_DISPLAY_CHARS`
  and `MAX_TITLE_CHARS`.
- `SECURITY.md`: supported-versions table, TLS/rustls notes, SSRF note,
  credential-strip note and supply-chain notes.
- Tests: title bound plus highlighter multibyte strings/escapes.
- Formatting: `rustfmt.toml` and `.editorconfig` (stable rustfmt options only).
- Licensing: `LICENSE-MIT` / `LICENSE-APACHE` texts for the declared `MIT OR
  Apache-2.0` license.
- `src/util.rs`: shared display helpers (`strip_scheme_prefix`,
  `strip_https_everywhere`, `truncate_chars`, `sanitize_title`) with unit tests.
- More tests: `ui::viewer::inline` hard-break splitting, resolver
  query/fragment/port/IPv6, rewrite scheme guards, frontmatter CRLF/BOM and size
  cap, theme layout bounds.
- `SECURITY.md` (scope, caps, reporting) and `.gitattributes` (LF, diff).
- Error view: a `Copy log` button.
- Retry: `Tab::can_retry()` honors the 5s cooldown with a countdown.
- Errors: a typed `BrowserError::Cancelled` variant with an `is_cancelled()`
  helper.
- Tooling: `make audit` runs the supply-chain audit (`cargo audit` plus `cargo
  deny check`, best-effort locally, enforced in CI), and `scripts/build-test.sh`
  starts with a POSIX `sh -n` syntax check of `scripts/*.sh`.

### Changed

- Top bar: `+ New` is now FIRST with the same 188px width as tab rows (fusion
  with the tab strip); the URL field reserve was updated.
- Navigation: re-navigation to the same logical URL reloads instead of returning
  early.
- Version: bumped to `0.2.0` in `Cargo.toml` and `Cargo.lock`, with `USER_AGENT`
  following via `CARGO_PKG_VERSION`.
- `USER_AGENT`: now derives from `CARGO_PKG_VERSION` (no version drift).
- `config`: centralizes all display/error bounds (`MAX_ERROR_LINE_CHARS`,
  `MAX_ADDRESS/ASKED_DISPLAY_CHARS`, `MAX_TITLE_CHARS`).
- `Cargo.toml`: `tokio/time` for stall timeouts, plus `missing_docs` +
  `broken_intra_doc_links` warnings.
- `scripts/build-dist.sh`: the `zip_contents` python fallback uses `argv`
  (filenames with spaces survive), and a missing `zip`/`ditto` is handled
  uniformly.
- Widget ids: table/code widget ids are salted by document URL hash (no
  cross-page leaks).
- Theme: `theme::shade` rounds (no truncation bias) and `content_max_width` is
  simplified.
- Cleanup: dead `Fetched::loaded_url` and its `allow(dead_code)` were removed;
  the canonical URL is `display_url` only.
- Language: all code comments and user-visible strings standardized to English.
- Viewer split: `ui::viewer` is split into focused modules (`mod`, `markdown`,
  `inline`, `code`, `table`) with no behavior change.
- Dead code: dropped unused `Fetched` fields (`raw_markdown`, `attempts`), the
  `Attempt` struct, unused frontmatter fields (only `title` is surfaced), the
  dead `has_link` helper, all `allow(dead_code)` attributes.
- Buttons: helpers are unified on `&str` (`tonal_button_sized` took `String`).
- `Cargo.toml`: completed package metadata (`publish = false`), documented the
  lint policy (`unsafe_code = forbid`; `clippy::all` enforced through `cargo
  clippy -- -D warnings`) and set `strip = "none"` so only
  `scripts/build-dist.sh` strips binaries (matches the documented `try_strip`
  flow).
- CI: now tests all three OSes (Linux, macOS, Windows) with a dependency cache,
  and the Intel macOS runner moved to the current runner image; the release flow
  is unchanged.
- CI hardening: top-level `permissions: contents: read` (the release job
  re-escalates to `write`), per-job `timeout-minutes` and `concurrency`
  cancel-in-progress on the same ref.
- `deny.toml`: `[bans] deny` pins the rustls-only contract (`openssl`,
  `openssl-sys`, `native-tls` rejected).
- Scroll state: the document scroll id uses the hashed `doc_id_salt` like
  tables/code.
- `scripts/build-dist.sh`: the version parse tolerates indentation and checksums
  are space-safe (no `xargs` splitting).
- Link resolution: uses `display_url` (directory semantics: `other` from
  `/love/` goes to `/love/other`), and unresolvable destinations are inert.
- Loading/error strips: use bounded, truncated log lines.
- `README.md`: fixed the stale `viewer.rs` path, the `+ New` location,
  refresh-disabled wording, the clippy invocation and a duplicated compiler
  bullet.

### Fixed

- Stalled trickle responses: a per-chunk 10s stall timeout now aborts the
  attempt instead of pinning a background thread to the 75s visual budget.
- Highlighter: stepped mid-char on multibyte text/strings/escapes; it now
  advances by whole UTF-8 chars.
- HTML tags: the viewer rendered raw `<br/>`/`<kbd>` as code pills; `<br>` is
  now a break and other tags a space.
- `zip_contents`: the python fallback split on spaces, breaking names like `My
  File.exe`.
- Logs: the missing-content-type log now reports `(missing)` instead of
  `(ausente)`.
- Packaging: `.deb` / desktop-file descriptions translated to English.
- `image_only`: fixed a false positive where plain text starting with `[image`
  rendered as an image box.
- `rewrite::absolutize`: a join failure returned garbage for navigation; it is
  now inert (`""`).
- `code` viewer: it sliced highlighter ranges directly (`&line[s..e]`); it now
  uses a `get()` fallback.
- `content_type`: fixed mixed-quote `charset="'utf-8'"` unwrapping.
- `highlight`: byte-wise `as char` casts replaced by `is_ascii_*`, and C-family
  fences (`c`, `cpp`, `c#`) no longer mis-highlight as JS/Rust (now `Plain`).
- Superseded loads: detection matched on the `"cancelled"` substring; it is now
  typed `BrowserError::Cancelled` end-to-end.
- Retry: it bypassed the 5s cooldown and allowed hammering; it is now gated by
  `can_retry()` with a countdown label.
- Frontmatter titles: remote titles could inject newlines/control chars; they
  are now sanitized (`sanitize_title`) and truncated.
- `App`: indexed `tabs[active]`; it now clamps with a safe getter and a
  `debug_assert`.
- History: unbounded `back`/`forward` stacks are capped at 100 entries.
- URL input: unbounded URL/address input is capped at 4096 chars (`BadUrl`
  locally).
- Frontmatter: unbounded YAML is capped at 32 KiB.
- Logs: unbounded `Content-Type` values are truncated to 500 chars.
- Window: a minimum size (640x480) is set so chrome never collapses.
- `is_html_break`: it missed `</br>` and treated `<bracket>` as a break; it now
  delimiter-checks (`<br>`, `<br/>`, `<br />`, `</br>`, attributes tolerated) in
  a single shared helper (`markdown::document::is_html_break`) used by the
  viewer and the clipboard `plain_text` alike.
- Frontmatter scan: it buffered the whole body as YAML before the 32 KiB size
  check; it now aborts early once clearly over the cap.
- HTTPS: a final URL that did not land on `https` is rejected before the body is
  read (defense in depth over `https_only(true)`).
- Transport errors: error text is credential-sanitized before reaching the
  line-by-line error view.
- Error view: it now shows clear styled messages per family (`No connection` /
  `Page not found` / `No markdown here` / `Invalid address`).
- Titles: tab titles are always prefixed and window titles match.
- Refresh/retry/re-navigation: never reuses cached results or errors (it always
  re-downloads).

## [0.1.0] - 2026-09-30

First release: an HTTPS-only `text/markdown` browser with a route fallback
cascade, a tabbed egui UI and release packaging for macOS, Linux and Windows.

### Added

- Browser: HTTPS-only rendering of `text/markdown` (CommonMark + tables,
  footnotes, task lists, strikethrough).
- Routing: a fallback cascade (`/love`, `/love/`, `/love.md`, `/love/index.md`,
  README variants) with a line-by-line error view.
- UI: a tabbed interface (eframe/egui) with home, navbar, viewer, theme and
  error view.
- Packaging: a macOS universal app, a Linux tarball + deb and a Windows zip,
  plus native CI releases.
