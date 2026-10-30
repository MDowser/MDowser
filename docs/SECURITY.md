# Security policy

See also: [`ARCHITECTURE.md`](ARCHITECTURE.md) for the internal contracts and
[`BUILDING.md`](BUILDING.md) for the packaging details.

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.7.x   | Yes       |
| < 0.7   | No        |

Only the latest `0.7.x` release line (see the `Cargo.toml` version and
`CHANGELOG.md`) receives security fixes. The table is checked against
`Cargo.toml` by `make check-version`, so a release cannot leave this policy
advertising an older line.

## Scope

MDowser is a viewer for Markdown (`text/markdown` and its common aliases)
served over HTTPS or opened from local files. Attack surface is deliberately
minimal.

### Network and transport

- Network transport is `https://` ONLY: only `GET`, no proxies, no cookies.
  `https_only(true)` rejects `http://` downgrades before the network, and an
  off-`https` redirect is rejected before the body is read.
- The encrypted disk cache (`cache/`) reuses validated responses within their
  HTTP freshness (`Cache-Control`/`Expires`, absolute 1-year ceiling). When no
  fresh copy can be obtained, the stored copy is served MARKED in the address
  bar (offline/saved), never presented as current.
- TLS uses `rustls` only (no `native-tls`); system `ca-certificates` is
  required on Linux for verification. `deny.toml` bans `openssl` and
  `native-tls`.
- Only a Markdown `Content-Type` (the standard type plus common aliases) with
  UTF-8 is rendered; everything else, including `text/plain`, is dropped
  and the next route candidate is tried. Headers beyond `Content-Type` are
  ignored except `Content-Length` (an early anti-abuse cutoff) and the cache
  set (`Cache-Control`, `Expires`, `ETag`, `Last-Modified`, `Date`, `Age`,
  `Vary`, `Pragma: no-cache`). `GET` works with any static server.
- Bodies are capped at 10 MiB with chunked reads and a 15s per-chunk stall
  timeout. Frontmatter YAML is capped at 64 KiB and aliases are never expanded
  (unknown fields skip with `IgnoredAny`); documents at 1M parsed events,
  10 000 rendered blocks, 10 000 rendered code lines and 512 KiB of rendered
  text per prose/HTML/table block. Tables are capped at 4096 CELLS per line
  (one past that is rejected BEFORE the eager parser allocates its tree; an
  escaped `\|` is content, not a cell), 5000 rows per block and 20 000
  collected cells per frame. One row spends a rendered-block unit and one cell
  a cell-budget unit, so a 4096-column table cannot lay out millions of
  widgets. The guard is fence-aware: a fenced block opened inside a blockquote
  or on a list item's first line covers only lines that stay in that container,
  so an unquoted or unindented table header cannot hide behind it.
- Other input caps: URL input at 8192 chars (including the address field
  itself), history stacks at 250 entries, titles at 256 chars (Unicode
  bidi/zero-width controls dropped), log lines truncated at 1000 chars, link
  context-menu hitboxes at 4096 per frame (navigation stays capped by the
  events/block budgets), in-page find at 256 query chars
  (`MAX_FIND_QUERY_CHARS`) and 2000 matches (`MAX_FIND_MATCHES`; extra hits
  render normally with an `N+` count), the outline at 2000 items / 120 title
  chars (`MAX_OUTLINE_ITEMS`/`MAX_OUTLINE_TITLE_CHARS`), the saved file's
  default name capped at 80 chars (`MAX_EXPORT_NAME_CHARS`), and the history
  filter at 512 chars (`MAX_HISTORY_FILTER_CHARS`).
- Redirects stop at 8 (`MAX_REDIRECTS`). Network concurrency is 6 fetches + 6
  local reads over 4 worker threads, with a 20s request timeout (60s per
  image, 15s per favicon, 5s image gate) and a 20s slot wait. The update check
  is one additional small request outside those slots (at most one in flight).
  Color-emoji raster jobs are capped at 4 (`MAX_CONCURRENT_EMOJI`).
- Markdown autolinks are capped at 20 000 per document and anchors at 20 000;
  link joins at 8192 joins / depth 2048; route candidates at 5; render nesting
  at depth 48; code highlighting at 4000 lines / 400 000 bytes (≈391 KiB). The
  code and table render caches are 32 MiB / 16 MiB, and the embedded-font gzip
  expansion is 64 MiB. All of these are `config::MAX_*`, so one module owns
  every bound.
- Single-flight per tab: a new navigation cooperatively cancels the previous
  fetch (the cancel flag is checked per candidate and per chunk).
- Wall-clock deadlines never panic: slot waits, image/favicon budget backoffs
  and the history delete confirmation go through `config::deadline_after`
  (`checked_add`, fail-closed to "now" on unreachable overflow), so no UI or
  background thread can abort on `Instant` arithmetic. The OS/CLI open-target
  list is capped at `MAX_TABS` before any tab is built; each target is still
  length-checked by `parse_navigable`.

### Update check (notification only)

- One fixed initial endpoint, HTTPS only:
  `https://github.com/MDowser/MDowser/releases/latest/download/latest.txt`
  (`config::UPDATE_ENDPOINT`). The initial URL never comes from a page or the
  marker; GitHub redirects once to its asset host, and the same HTTPS-only and
  8-redirect rules as every request apply.
- The GET has a 15s budget and a 1 KiB body cap
  (`UPDATE_REQUEST_TIMEOUT_SECS`, `MAX_UPDATE_MARKER_BYTES`), accepts a
  response whose declared `Content-Type` includes `text/plain` or
  `application/octet-stream` (checked BEFORE the body), sends
  `Cache-Control: no-cache`, and is not written to the encrypted disk cache.
- The body must be exactly one stable `X.Y.Z` semver (a single trailing
  newline tolerated). `v` prefixes, multiple lines, pre-release/build suffixes
  and non-UTF-8 are rejected; a failed parse is a failed check, never a
  navigation.
- The marker names a version and nothing else: it cannot supply a URL, a
  command, a signature or an installer. A newer version only paints the gear
  dot and the Updates card; "Open release page" builds
  `https://github.com/MDowser/MDowser/releases/tag/vX.Y.Z` locally and hands it
  to the system browser. MDowser never downloads, unpacks or replaces itself.
- A failed check does not persist a timestamp and cannot loop: the session
  waits out a 24h backoff, a restart follows the normal due time (the failure
  does not push it back), and `Check now` remains an explicit user action.
  Automatic checks can be turned off in Preferences and send no browsing data
  (GitHub sees the public endpoint request, the source IP and
  `MDowser/<version>`).
- Marker fetches share the process's network stack but not a tab's
  cancellation, history or cache entry.

### Local files

- Local files (`file://` or an absolute/`~/` path) are reachable ONLY from
  user input (address bar, command line, OS hand-off, a file or folder dropped
  onto the window) and from links inside an already-local document. A remote
  page can never name a local file: `protocol::rewrite` keeps `file:`
  destinations inert when the base document is not local.
- A dropped path is converted straight to a `file:` URL and never parsed as
  address text, so a bare name cannot become an HTTPS host. The same
  regular-file, UTF-8, size, cancellation and no-directory-listing rules apply
  as for a typed local path. The drop queue keeps at most `MAX_TABS + 1` paths,
  so an oversized drag cannot grow it without bound.
- A `file://host/...` URL (remote share) is rejected before any read;
  `file://localhost/...` is the local machine and is accepted. Only REGULAR
  files are readable: the open runs with `O_NONBLOCK` and the OPENED handle is
  re-checked, so a FIFO, device or socket swapped in after the metadata check
  is rejected without blocking or pinning a fetch slot.
- No directory enumeration (a directory resolves through its conventional
  Markdown indexes only), local documents must be UTF-8, and
  every local read shares the network caps (10 MiB documents, image caps,
  metadata pre-check + running byte cap + cooperative cancel between 64 KiB
  chunks).
- A `file:` destination with credentials, a port or a remote host is inert
  even from a local document. The history and bookmark stores and the session
  loader reject such URLs as the document reader does (`file://localhost/`
  stays local). A RELATIVE `file:` spelling (`file:notes.md`, which the WHATWG
  parser would rebase onto the filesystem root) is rejected; the session loader
  rejects credentials on `https` too, and session writes store an over-long tab
  URL as Home.
- A relative `XDG_DATA_HOME` is ignored, and the same absolute-only rule covers
  `HOME`, `APPDATA`/`LOCALAPPDATA` and `~/` expansion. The data directory (and
  therefore `wipe_all_data`'s recursive delete) can never resolve under the
  current working directory.

### Links and markup

- Markdown links are absolutized before rendering. `mailto:`, `data:`,
  `javascript:` and `http:` destinations are inert (never navigate), and
  `file:` destinations are inert from a remote page (only a local document may
  follow them).
- A destination carrying authority credentials (`https://user:pass@host/x`
  and the scheme-relative `//user:pass@host/x` spelling) is inert on EVERY
  scheme.
- A `#fragment` scrolls to an element already in the loaded document (an
  unknown target renders as text); it never triggers a network navigation.
- Bare URLs typed as text (`markdown::autolink`) follow the same policy:
  `www.…` becomes `https://…` (clickable), a bare `http://…` stays inert,
  and only a candidate that parses as a real URL with a host and WITHOUT
  credentials becomes a link; a trailing dot before the `@` cannot slip
  past the email guard.
- Markup is never EXECUTED: HTML and MDX JSX are not interpreted as code, no
  JavaScript, import or `{…}` expression is evaluated, and nothing is fetched
  from a component. Before parsing, `normalize_markup` strips tags
  transparently (content stays), drops `<script>`/`<style>` regions, keeps
  `<pre>`/`<textarea>` inert verbatim, turns `<img src>` into a Markdown image
  and, in an MDX document, removes ESM/`{…}`; HTML comments are invisible.
  Inline `<br>` becomes a break and other inline tags a space, so the viewer
  and the clipboard (`plain_text`) agree. Structural components (`<Tabs>`,
  `<Accordion>`, `<Card>`, `<Steps>`) and their `:::` custom-container
  equivalents become inert `<mdowser-cmp …/>` layout markers the renderer
  draws; they carry no code and every panel's text reaches `Copy`. Code
  spans and fenced/indented code are never touched.

### Images and favicons

- Remote page images and favicons are HTTPS subresources only. The declared
  Content-Type is checked BEFORE the body is read, so a concrete
  non-image type is never downloaded or decompressed; a generic
  `application/octet-stream` / `binary/octet-stream` (the object-store default)
  or a missing header is allowed through to the sniff. The bytes must then
  sniff and decode as PNG/JPEG/GIF/WebP/JPEG XL/AVIF/ICO/static SVG in pure
  Rust.
- An image in a LOCAL document is read from disk instead (same magic sniff +
  decoder, byte cap, no Content-Type gate); favicons are
  never probed for local pages.
- Caps: 16 MiB per image (1 MiB per favicon), 48 MP and 12288 px per side
  (checked from the header before decoding AND against the decoded frame),
  512 MiB decoder allocation, a 384 MiB GPU texture budget per document plus a
  768 MiB process-wide budget across tabs AND favicons, 512 images per
  document, 6 concurrent image jobs (at most 3 per document), a 60s network
  timeout per image (16 MiB at ~2.2 Mbps) with the same 15s per-chunk stall
  cutoff, and an 80s wall-clock deadline per job after which the entry fails
  instead of staying pending (decoders cannot be interrupted mid-call, but a
  stuck job neither hangs forever nor keeps the repaint loop alive). Icons
  decode at `MAX_FAVICON_DIM` (160 px), so `MAX_FAVICON_HOSTS` icons cannot
  approach a multi-GiB worst case.
- One automatic retry per image covers transient failures (network error,
  stall, truncated transfer, 5xx/429); content-type rejections, other 4xx and
  cancellations are never retried. A response declaring BOTH an image and a
  concrete non-image Content-Type is rejected; a process-wide
  texture-budget refusal is retried after a short backoff.

### SVG

- SVG is rasterized STATICALLY (`usvg`/`resvg`, pure Rust). `<script>`,
  `<foreignObject>` and the SMIL animation elements (`animate`,
  `animateTransform`, `animateMotion`, `set`, `discard`) are rejected before
  parsing: the static pipeline ignores them, and rejecting them structurally
  keeps a future parser from executing them.
- Every `href` (network, local path and inline `data:` URL) is never resolved.
  DTDs (`<!DOCTYPE>`/`<!ENTITY>` declarations, unbounded entity expansion) are
  rejected by the structural parse: roxmltree is pinned to `allow_dtd: false`
  explicitly, before usvg's entity-expanding parser can see the source.
- `filter`/`fe*` markup is STRIPPED before parsing — definitions, `fe*`
  primitives and every `filter` reference, by LOCAL NAME with
  namespace-prefixed spellings included. Filters are therefore IGNORED, never
  rendered: an image that ships one shows its unfiltered content, and the whole
  filter cost model — unbounded `numOctaves`, morphology and convolution,
  filter-region pixmaps — stays unreachable. Removing the references too is
  required, because usvg DROPS an element whose `filter` link dangles.
- CSS-spelled filters (`style="filter:…"`, a `<style>` rule) cannot be
  stripped without a CSS parser and are rejected in every spelling the parser
  accepts, including `filter/**/:…` (comments between the property name and
  the `:`) and the IE star hack `*filter:…`. CSS `filter:` is checked in every
  `<style>` element regardless of `type`, since the renderer may honor the
  text even for non-`text/css` types.
- `marker`/`pattern` markup is rejected by the same structural pass: a marker
  subtree is re-converted per path vertex, and a pattern's pixmap is sized by
  the PATTERN's own rect (not the rendered target), so a tiny source could
  demand an allocation whose failure aborts the process.
- Duplicate element `id`s are rejected fail-closed (first- vs last-wins differs
  between parsers; counting a different `<use>` target than the renderer would
  undercount a bomb).
- Fonts are the app's own bundled faces, never system fonts. The source is
  token-capped (`MAX_SVG_TOKENS`, 400k) and nesting DEPTH-capped
  (`MAX_SVG_DEPTH`, 512) before the XML parser runs: a deep `<g>` chain
  overflows the recursive parser/usvg stack even under the token count. The
  `<use>` expansion count is bounded by an ITERATIVE walk against the same
  token cap (nested `<use>` grows exponentially in the parsed tree, and a
  recursive walk could overflow the decode thread's stack). The raster buffer
  is capped at 3072 px per side.
- HTML and scripts are never interpreted as images. The SVG documents embedded
  in the bundled Noto Color Emoji font (`media::emoji`) go through the same
  hardened rasterizer and are bounded by the emoji texture caches (each
  `EMOJI_CACHE_ENTRIES` / `EMOJI_CACHE_BYTES`, reserved from the process-wide
  texture budget). Image jobs are panic-guarded: a decoder panic surfaces as a
  failure placeholder instead of killing the background thread.

### Titles

Remote document titles (frontmatter `title` or the first-H1 fallback) are
sanitized — control AND Unicode format/bidi characters stripped, whitespace
collapsed, length-capped — before reaching window/tab titles, the page-info
card and the bookmark/history stores.

### Local data and encrypted stores

- **Shared recipe.** History, bookmarks, settings, the session and the disk
  cache are encrypted at rest with ChaCha20-Poly1305 (`aws-lc-rs`,
  the same crypto backend rustls links): a random 96-bit nonce per write and a
  256-bit key in the single shared `store.key` (created 0600; directory 0700
  on Unix). The key sits beside the data, protecting against other users,
  backups and casual inspection but NOT malware running as the same user.
- **Key handling.** The key is installed atomically (private temp file +
  `hard_link`), so a second instance can only read the winner's complete key.
  A corrupt (wrong-size) key is repaired under a best-effort cross-process lock with stale-lock takeover; the lock is HELD until the
  replacement key is installed, and a lock timeout fails closed (the file is
  never deleted). An oversized key file is rejected from its metadata without
  being read whole, and a key whose permissions cannot be tightened is refused
  fail-closed.
- **Store I/O.** Store loads open with `O_NONBLOCK` through a shared helper
  that re-checks the OPENED handle (`settings::read_regular_file`,
  used by history, bookmarks and both settings reads): a FIFO swapped in
  between the metadata check and the open can neither block the UI thread nor
  be read, and the streaming read still enforces its cap if the file grows
  after the check. A store restored world-readable is tightened through that
  SAME opened handle (`fchmod`), never a fresh path lookup, and owner bits are
  never widened; a symlink swapped in after the open is neither followed nor
  `chmod`ed. The key file opens with `O_NOFOLLOW` and is tightened on that
  handle, so a symlinked `store.key` is refused. Crash-orphaned
  `<store>.tmp.*` files AND the key-creation temps
  (`store.key.tmp.<pid>.<seq>`, which hold raw key bytes) are swept.
- **Atomic writes and backups.** Writes use a temp file + rename with a
  per-process unique temp name (debounced writes run on a worker thread, so
  the UI thread never fsyncs; concurrent instances cannot interleave). The
  rename REPLACES the target atomically on every supported platform (no
  move-aside window), so a crash cannot leave a missing or half-written store;
  any IO/crypto error degrades to an empty in-memory history instead of
  crashing. A store file that cannot be decrypted (missing/regenerated key,
  corruption) is preserved as `.bak` via an atomic `create_new` copy
  (`settings::preserve_blob_bytes`, shared by history/bookmarks/settings): no
  earlier backup is clobbered, no exists-then-rename TOCTOU, and the live file
  stays in place until a later save replaces it.
- **Bounded writer.** The worker queue holds one pending blob per target file
  (newest wins; the app writes one data directory) and a UI-path flush writes
  what is queued before waiting at most 5s, so a hung disk cannot grow memory,
  lose the newest data or freeze a frame, a delete or exit. A store wipe is
  enqueued as a DISCARD job on the same worker, so it runs strictly after any
  in-flight write, and a stalled rename cannot resurrect the deleted
  ciphertext. A job is either handed to a worker that will run it or returned
  for a respawn; the discard sweeps the store's `.bak` and `.tmp.*` siblings.
- **History.** Local browsing history stays in the OS per-user data directory
  only, bounded at 20 000 entries / 8 MiB of plaintext (oldest dropped) and
  encrypted with the shared recipe. Only successfully loaded HTTPS pages and
  local files are recorded. Retention is user-configurable (default 3 months);
  "Forget on exit" never reads or writes the files and wipes leftovers;
  "Delete history now" removes the ciphertext (the shared key stays for
  bookmarks/settings/session). A delete waits for the background writer to
  drain (armed across a stalled disk, completing on a later tick); a
  failed delete is reported in Preferences. "Forget on exit" also disables
  "restore tabs on startup" and deletes any `session.bin`. Stored URLs lose
  authority credentials (`user:pass@`) but keep their exact path/query
  bytes.
- **Settings and session.** Preferences (`settings.bin`, magic `MDST`) and the
  session (`session.bin`, magic `MDSS`) use the same shared-key AEAD recipe.
  `Prefs::set_*` and the per-change session save update an in-memory body and
  queue the encrypted write on dedicated background workers (one per file);
  the exit path drains them. Legacy plaintext `settings.conf` / `session.conf`
  and per-store `history.key` / `bookmarks.key` are deleted on load, never read
  (clean break, no migration). An UNREADABLE `settings.bin` is reported in
  Preferences and never treated as "restore tabs off". WRITES follow the same
  distinction: a file that exists but cannot be read or decrypted makes the
  read-modify-write REFUSE, because rewriting from an empty body would
  drop every other stored key (theme, zoom, restore-tabs, check-updates, the
  last update result, retention, window geometry); a change that would push
  the plaintext past `MAX_SETTINGS_BYTES` is refused too (the reader's caps
  derive from `crypto::OVERHEAD_BYTES`, so a written file is always readable).
  Window geometry (position, size, maximized) is one of those encrypted
  preferences, removed by "Delete all data", never kept in a plaintext
  window-state file. A failed background write is surfaced in Preferences
  (each worker's outcome is drained per frame into its own slot, so a session
  save cannot clear a settings warning and vice versa, and cleared only on
  confirmed success), and a snapshot that could not be built is not recorded as
  saved and retries.
- **Session restore.** Turning off `restore_tabs` deletes the encrypted
  `session.bin`; a failed delete is surfaced in Preferences and retried at
  every start (and when the toggle is set off again), and the orphaned
  `.tmp.*`/`.bak` siblings are removed with it. OS/CLI open targets supersede a
  stored session only for the current launch (an untouched window never
  overwrites it).
- **Delete all data.** "Delete all data and close" (Preferences, two-click)
  suppresses every later store write and drains the writers before deleting the
  whole data directory and calling `process::exit`. A removal that FAILS (open
  handles on Windows, permissions, read-only media) does not exit pretending
  the data is gone: writes are re-enabled, the failure is shown in the Local
  data card and the app keeps running.
- **Bookmarks.** Bookmarks use the same recipe (`bookmarks.bin`, magic
  `MDBM`) with the same bounded background writer (a 2s debounce). They are
  explicit user data with no retention window, capped at 2000 entries (an add
  at the cap is refused with a toast, never a silent drop of an older
  bookmark) AND at 48 MiB of serialized plaintext
  (`MAX_BOOKMARK_STORE_BYTES`); an entry that would cross the byte budget — an
  ADD or a tile rename — is refused the same way, a body over it from disk is
  trimmed oldest-first on load, and the encrypted-file read cap is the
  plaintext budget plus the AEAD overhead. The settings read cap
  (`config::MAX_SETTINGS_BYTES`, 256 KiB) lives in the same module. The store
  is listed ONLY on the Home page, and a failed save is surfaced there. Stored
  tile customizations (name, glyph, palette color) are sanitized on entry and
  read-back (control characters dropped, glyph capped
  at 16 chars, color index bounded; the Home editor only renames today, while
  stored glyphs and colors keep rendering), and tile favicons reuse the
  bounded favicon pipeline (HTTPS origins only, the site's own `/favicon.*`
  over a plain GET).
- **Disk cache.** The encrypted HTTP disk cache (`cache/`, 512 MiB / 50 000
  entries) reuses the same store key and AEAD recipe. Documents are one
  encrypted file per host, images one per URL (sharded), favicons one for all
  probes. Filenames are key hashes, never hostnames/URLs, and every record
  carries its key, verified on read, so a listing or a hash collision cannot
  leak or mix pages. Index stems must be exactly the 16
  lowercase hex chars the cache writes before they become path components,
  and startup sweeps remove symlinks instead of following them, so a planted entry
  cannot walk outside `cache/`. A
  policy-header set beyond the bounded capture (repeated values, an over-long
  value) is never stored: a partial view could hide `no-store`/`Vary: *`.
- **Cache freshness.** Freshness is HTTP: `max-age` / `Expires` (+`Date`/`Age`;
  the response's age is the larger of the `Age` header and the apparent age
  `now - Date`), `no-store` (never persisted, and evicts an older record for
  the same URL), `no-cache` (always revalidated), `must-revalidate` (honored:
  an expired entry is never served as current) and `immutable` (no distinct
  effect: a fresh entry is never revalidated unless Refresh asks), all inside
  the absolute 1-year ceiling. `ETag`/`Last-Modified` revalidation refreshes
  the entry on 304 and replaces it on 200. `Vary: *` (or over anything but
  `Accept`/`Accept-Encoding`) is never stored, and `Refresh` forces a
  revalidation. When none of that yields a fresh copy (unreachable network,
  every route rejected, a failed revalidation after the cascade), a stored
  copy is served only MARKED as an offline/saved copy in the address bar and
  the page-info card — never as current, and only when one exists; without a
  stored copy the error view stands. Past either bound the OLDEST entries
  evict FIFO (never an error, never a toast), with per-class quotas.
  Local files and internal pages are never cached; "Forget on exit" disables
  the cache entirely (neither read nor written, leftovers wiped); each class
  clears separately from Preferences and "Delete all data" removes the whole
  directory.
- **Save page.** `Ctrl/Cmd+S` writes the page on screen as Markdown into the
  OS downloads folder — the Windows Downloads known folder, the XDG one when
  configured and present, else `~/Downloads`, else home, else the current
  directory: nothing is typed and no directory is created. A permission denial in the downloads folder falls
  back to home (announced in the toast), so the write never leaves the user's
  home tree; any other failure is reported instead of moving the file. The
  file name derives from the page, so it is
  slugified (alphanumerics, `-`, `_`) and length-capped BEFORE it becomes a
  path component: a hostile title or URL segment cannot add a separator or
  `..`, cannot leave the downloads directory and cannot produce a name the
  filesystem refuses (a Windows device name such as `aux` or `nul` gets a
  `-file` suffix). An existing file is NEVER overwritten: the first free
  browser-style copy name (`notes.md`, `notes (1).md`, …) is picked, and the
  publish is no-replace itself (a hard link that fails if the destination
  appeared after the name check; an exclusive create on filesystems without
  hard links), so a racing file is preserved instead of silently replaced.
  On filesystems with hard links the write is ATOMIC: the bounded document
  (at most `MAX_BODY_BYTES`) goes to a unique sibling temp file (`create_new`,
  never following a planted link) that is published whole, so a failure
  mid-write cannot leave a truncated document. The temp file is fsynced
  before it is published and the directory entry afterwards (best effort), so
  a confirmed save also survives a power loss. Filesystems without hard links
  fall back to an exclusive in-place create (never a replacement; a failed
  write removes the partial file). A new file gets the process umask. The
  write happens on the UI thread, like the stores' own bounded writes.

### Credentials and URL sanitization

- Credentials in pasted URLs (`user:pass@host`) are stripped before the
  network and never logged. Any non-https scheme with an authority
  (`http://user:pass@host`, `http:/user:pass@host` with a single slash,
  `http:\user:pass@host` with a backslash, `http:///user:pass@host` with extra
  slashes, protocol-relative `//user:pass@host`, `ftp://…`, `data:`,
  `javascript:`) is rejected and SANITIZED in every displayed form (address
  field, error view, "Copy log") before parsing. A `scheme:user:pass@host`
  spelling (no slashes, a second colon before the `@`) is scheme syntax too and
  is rejected, with its userinfo removed by the same sanitizer, instead of
  being read as a userinfo host for `https://host`; the plain `user:pass@host`
  and `user:pass@host:8080` forms keep working with credentials stripped.
- The display echo is bounded at the first ASCII-whitespace character (one
  address token: a second-line paste cannot ride along), and a
  host-port-shaped token (`nas:5000/x@y`) is kept verbatim only when it parses
  as a credential-free URL, so a password-shaped `name:digits/path@…` input is
  stripped instead.
- Local ("bad path") messages echo the input through the same bounded,
  credential-free helper, and post-parse local read errors echo the
  `file:` URL through it as well (`protocol::local::echo_url`).
- The stripping does not depend on the caller: the shared network entry point
  (`protocol::client::get_bytes`) removes authority credentials from the
  request URL itself, fail-closed, before anything is sent, and its own
  rejection message reuses the shared sanitizer. Server-controlled redirect
  URLs embedded in transport error text are sanitized the same way before
  the error view AND image failure tooltips, and a final URL off
  `https` is rejected before the body is read (defense in
  depth over `https_only(true)`).
- Userinfo on the LANDED redirect URL is stripped before display/history
  (fail-closed when it cannot be removed). Collected `Content-Type` values are
  capped at 8 (`MAX_CONTENT_TYPE_VALUES`; more values are rejected instead of
  inspecting an attacker-chosen prefix); route/display URLs enforce the
  SERIALIZED `config::MAX_URL_BYTES` cap after `.md`/index expansion (a byte
  cap: percent-encoding makes one 4-byte input char up to 12 ASCII bytes).
- The error view's external-open action re-resolves the address at click time
  through the app's own navigability parser and hands ONLY the resulting
  sanitized `https:`/`file:` URL to the OS default handler (credentials
  stripped, blocked schemes and remote `file:` shares refused), so a pasted
  password never reaches the browser's URL bar or the launcher's argv.
- Key file reads are capped and handle-checked (`O_NONBLOCK | O_NOFOLLOW`
  open, the opened handle re-checked, at most 33 bytes read, permissions
  tightened on that same handle): a FIFO swapped in cannot block, a symlinked
  key is refused without being followed or `chmod`ed through, and a file
  grown after the check cannot force a large allocation.

### Supply chain and dependencies

- `unsafe_code = "forbid"` in `Cargo.toml`. `Cargo.lock` is committed, and
  `cargo audit` + `cargo deny check` run via `make audit` (configured by
  `.cargo/audit.toml` and `deny.toml`) before releases, and the release
  workflow's `verify` job runs the same two tools at pinned versions with no
  Actions cache, so a tagged build fails closed. Accepted transitive
  advisories are listed in both files with their rationale and dependency
  chain; any advisory not listed fails the gate.
- `deny.toml` allows an explicit license set (the app itself is `MPL-2.0`, see
  `LICENSE`). The required notices, bundled fonts included, ship in
  `THIRD_PARTY_NOTICES.md` with every binary and are embedded in it for the
  internal `about:licenses` page.
- Image decoding is pure Rust (no C libraries): `avif-decode` (BSD-3-Clause),
  `avif-parse` (MPL-2.0, file-level copyleft, unmodified) and `jxl-oxide`
  (MIT/Apache-2.0).
- YAML uses the maintained `serde_norway` fork; the archived `serde_yaml` and
  unsound `serde_yml` are not used.
- Platform integrations stay target-scoped and small: `known-folders`
  (Windows only) is a safe wrapper over `SHGetKnownFolderPath` whose only
  dependency, `windows-sys`, is already in the graph through winit/muda, so
  the `unsafe_code = "forbid"` crate rule holds and other platforms compile
  none of it.

### SSRF note

Fetching `localhost` or private IPs is user-initiated (the user typed the URL
or opened a document that links it), not server-side; it is allowed by design
for local preview. Document subresources (images) inherit the same policy:
they are HTTPS-only and still require a valid certificate, so a local service
without one cannot be reached. No proxy is used (deterministic direct TLS).

## Build artifacts

Rust embeds the source path of every panic location, dependency sources
included (`$HOME/.cargo/registry/src/...`), which would identify the builder.
`make`/`scripts/build-dist.sh`, `windows.ps1`, the RPM spec's `%build` and the
Flatpak manifest (`/run/build/mdowser[/cargo]`) remap those prefixes at
compile time
(`--remap-path-prefix`). The `.zip` writer drops the Info-ZIP Unix uid/gid
extra field, and tar staging sweeps extended attributes / passes
`--no-xattrs`, so `com.apple.provenance` stays out of Linux packages. The RPM
spec pins `%_buildhost` and `%defattr(-, root, root, -)`.

`scripts/check-dist.sh` (`make check-dist`, also enforced by `make dist` and
`make validate`) fails the build if any `dist/` artifact carries the builder's
paths, hostname, uid/gid, xattrs or an AppleDouble `._*`/`__MACOSX/` member.
Only regular, non-hidden files count as artifacts: Finder's `.DS_Store` is
ignored, while a symlink (hidden or not) or a leftover staging directory in
`dist/` fails the gate (a release must not point outside it or ship a stale
tree). `make check-dist`, `make dist` and CI pass `--require`, so a missing
`dist/`, no artifact at all, or a missing inspector (no python3) is a hard
failure, not a silent SKIP. Generic hostnames that collide with the `/build`
and `/cargo` remap targets are never used as needles.
See [`BUILDING.md#build-artifact-privacy`](BUILDING.md#build-artifact-privacy).

Release builds run in CI (`.github/workflows/release.yml`) on fresh
GitHub-hosted runners from the tagged commit: macOS universal, Linux
x86_64/arm64, Windows x86_64 (MinGW cross) and Windows arm64 (native MSVC),
plus RPM and Flatpak. The publish job re-runs `check-dist.sh` over the merged
artifact set, so no target can bypass the gate before a release exists.

## Reporting a vulnerability

Open an issue in the project tracker with `[security]` in the title, or contact
the maintainer privately if the issue allows network exfiltration, RCE, or TLS
bypass. Include:

1. MDowser version (`Cargo.toml`) and OS/arch.
2. A minimal reproduction (URL or Markdown snippet, no credentials).
3. Expected vs actual behavior.

Do not include private hosts, tokens, or credentials in public reports; redact
them (`example.com`, `***`).

## What to expect

- Acknowledgement within a few days.
- A fix in `Unreleased` plus a patch release for TLS/network bypass, OOM, or
  arbitrary-file/credential leaks.
- Public disclosure notes in `CHANGELOG.md` once fixed.
