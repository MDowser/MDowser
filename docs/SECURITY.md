# Security policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.5.x   | Yes       |
| < 0.5   | No        |

Only the latest `0.5.x` release (see `Cargo.toml` version + `CHANGELOG.md`)
receives security fixes.

## Scope

MDowser is a viewer for Markdown (`text/markdown` and its common aliases)
served over HTTPS or opened from local files. It deliberately minimizes
attack surface:

- Network transport is `https://` ONLY: only `GET`, no proxies, no cookies,
  no cache. `https_only(true)` rejects `http://` downgrades before the
  network; a redirect that lands off-`https` is rejected before the body is
  read (never treated as a valid candidate).
- Local files (`file://` or an absolute/`~/` path) are reachable ONLY from
  user input (address bar, command line, OS hand-off) and from links inside
  an already-local document. A remote page can never name a local file:
  `protocol::rewrite` keeps `file:` destinations inert when the base
  document is not local. A `file://host/...` URL (remote share) is rejected
  before any read (`file://localhost/...` is the local machine and is
  accepted), only REGULAR files are readable (the open runs with
  `O_NONBLOCK` and the OPENED handle is re-checked, so a FIFO/device/socket
  swapped in after the metadata check is rejected without ever blocking the
  thread or pinning a fetch slot), there is no
  directory enumeration (a directory resolves
  through its conventional Markdown indexes only), local documents must be
  UTF-8, and every local read shares the network caps (10 MiB documents,
  image caps, metadata pre-check + running byte cap + cooperative cancel
  between 64 KiB chunks).
- TLS via `rustls` only (no `native-tls`); system `ca-certificates`
  required on Linux for verification. `deny.toml` bans `openssl` /
  `native-tls` so the contract cannot regress via new dependencies.
- Only a Markdown `Content-Type` (the standard type plus its common
  aliases) with UTF-8; everything else -- including `text/plain` -- is
  dropped and the next route candidate is tried. All other response headers
  are ignored (plus `Content-Length`, read only as an early anti-abuse
  cutoff), and `GET` works with any static server.
- Bodies are capped at 10 MiB with chunked reads + 15s per-chunk stall
  timeout; frontmatter YAML is capped at 64 KiB and YAML aliases are never
  expanded (unknown fields skip with `IgnoredAny`); documents are capped at
  1M parsed events, 10 000 rendered blocks, 10 000 rendered code lines and
  512 KiB of rendered text
  per prose/HTML/table block (a single multi-MB paragraph cannot become one
  giant label); tables are capped at 4096 CELLS per line (a line yielding
  one past that is rejected BEFORE the eager parser allocates its tree, and
  an escaped `\|` is content, not a cell): the guard is fence-aware — a
  fenced block opened inside a blockquote or on a list item's first line
  only covers lines that stay in that container, so an unquoted or
  unindented table header can never hide behind such an opener — 5000 rows
  per block and
  20 000 collected cells per frame — one row spends a rendered-block unit
  and one cell a cell-budget unit, so a 4096-column table cannot lay out
  millions of widgets while the row budget sees only thousands of units;
  URL input is capped at 8192 chars (including the address
  field itself); history stacks are capped at 250 entries; titles capped at
  256 chars (Unicode bidi/zero-width controls dropped); log lines truncated
  at 1000 chars; link context-menu hitboxes are capped at 4096 per frame
  (navigation itself stays capped by the events/block budgets); in-page find
  is capped at 256 query chars (`MAX_FIND_QUERY_CHARS`) and 2000 matches
  (`MAX_FIND_MATCHES`, extra hits render normally with an `N+` count), and
  the history filter field at 512 chars (`MAX_HISTORY_FILTER_CHARS`).
  Redirects stop at 8 (`MAX_REDIRECTS`); network concurrency at 6 fetches
  + 6 local reads over 4 worker threads, with a 20 s request timeout (60 s
  per image, 15 s per favicon, 5 s image gate) and a 20 s slot wait;
  color-emoji raster jobs at 4 (`MAX_CONCURRENT_EMOJI`); autolinks at
  20 000 per document, anchors at 20 000, link joins bounded
  (8192 joins / depth 2048), route candidates at 5, render nesting at depth
  48 with code highlighting capped at 4000 lines / 400 KiB; the code/table
  render caches at 32 MiB / 16 MiB and the embedded-font gzip expansion at
  64 MiB (all `config::MAX_*`, so one module owns every bound).
- Single-flight per tab: a new navigation cancels the previous fetch
  cooperatively (cancel flag checked per candidate/chunk).
- Wall-clock deadlines never panic: slot waits, image/favicon budget
  backoffs and the history delete confirmation all go through
  `config::deadline_after` (`checked_add`, fail-closed to "now" on the
  unreachable overflow), so no UI or background thread can abort on
  `Instant` arithmetic. The OS/CLI open-target list is capped at `MAX_TABS`
  before any tab is built, so a hostile hand-off cannot pre-allocate an
  unbounded tab vector (each target is still length-checked by
  `parse_navigable`).
- Markdown links are absolutized before rendering; `mailto:` / `data:` /
  `javascript:` / `http:` destinations are inert (never navigate), and
  `file:` destinations are inert from a remote page (only a local document
  may follow them; a `file:` destination with credentials, a port or a
  remote host is inert even from a local document, because the reader would
  reject it at navigation time and credentials must never ride along a
  destination). A destination that carries authority credentials
  (`https://user:pass@host/x`, and the scheme-relative
  `//user:pass@host/x` spelling, which only reveals the real target after
  the join) is inert on EVERY scheme: the click must never land somewhere
  other than the visible label. A
  `#fragment` scrolls to an element already in the loaded document (an
  unknown target renders as text); it never triggers a network navigation.
  Bare URLs typed as text (`markdown::autolink`) follow the same policy:
  `www.…` becomes `https://…` (clickable), a bare `http://…` stays inert,
  and only candidates that parse as a real URL with a host and WITHOUT
  credentials ever become a link (a trailing dot before the `@` used to
  slip past the email guard and produce a link whose host was the part
  after the `@`).
  Markup is never EXECUTED: HTML and MDX JSX are not interpreted as code,
  no JavaScript, import or `{…}` expression is evaluated and nothing is
  fetched from a component. Before parsing, `normalize_markup` strips tags
  transparently (content stays), drops `<script>`/`<style>` regions, keeps
  `<pre>`/`<textarea>` inert verbatim, turns `<img src>` into a Markdown
  image and, in an MDX document, removes ESM/`{…}`; HTML comments are
  invisible. Inline `<br>` becomes a break and other inline tags a space,
  so viewer and clipboard (`plain_text`) agree. Structural components
  (`<Tabs>`, `<Accordion>`, `<Card>`, `<Steps>`) and their `:::` custom-
  container equivalents become inert
  `<mdowser-cmp …/>` layout markers the renderer draws; they carry no code
  and every panel's text still reaches `Copy`. Code spans and
  fenced/indented code are never touched.
- Page images (and favicons) from a remote page are HTTPS subresources
  only: the declared
  Content-Type is checked BEFORE the body is read, so a concrete non-image
  type is never downloaded or decompressed (a generic
  `application/octet-stream` / `binary/octet-stream` (the object-store
  default) /
  missing header is allowed through to the sniff), and the bytes must then
  sniff + decode as PNG/JPEG/GIF/WebP/JPEG XL/AVIF/ICO/static SVG in pure
  Rust. An image referenced by a LOCAL document is read from disk instead
  (same magic sniff + decoder, same byte cap, no Content-Type gate);
  favicons are never probed for local pages.
  Caps: 16 MiB per image (1 MiB per favicon), 48 MP and 12288 px per side
  (checked from the header before decoding AND against the decoded frame),
  512 MiB decoder allocation, 384 MiB GPU texture budget per
  document plus a 768 MiB process-wide texture budget across tabs AND
  favicons (icons decode at `MAX_FAVICON_DIM`, 160 px, so `MAX_FAVICON_HOSTS`
  icons cannot approach the old multi-GiB worst case),
  512 images per document, 6 concurrent image jobs (at most 3 per document),
  60s network timeout per
  image (16 MiB at ~2.2 Mbps) with the same 15s per-chunk stall cutoff, and
  an 80s wall-clock deadline per job after which the entry fails instead of
  staying pending (decoders cannot be interrupted mid-call, but a stuck job
  neither hangs forever nor keeps the repaint loop alive).
  One automatic retry per image covers transient failures (network error,
  stall, truncated transfer, 5xx/429); content-type rejections, other 4xx
  and cancellations are never retried. A response declaring BOTH an image
  and a concrete non-image `Content-Type` is rejected (a misbehaving server
  gets no benefit from precedence games), and a process-wide texture-budget
  refusal is retried after a short backoff instead of failing the image.
- SVG is rasterized STATICALLY (`usvg`/`resvg`, pure Rust): `<script>`,
  `<foreignObject>` and the SMIL animation elements
  (`animate`/`animateTransform`/`animateMotion`/`set`/`discard`) are
  rejected before parsing (the
  static pipeline would ignore them; refusing them structurally keeps a
  future parser from ever executing them), every `href` (network, local
  paths AND inline `data:` URLs) is never resolved, DTDs (`<!DOCTYPE>`/
  `<!ENTITY>` declarations, unbounded entity expansion) are rejected by the
  structural parse (roxmltree is pinned to `allow_dtd: false` explicitly —
  no library default can reopen it — before usvg's
  entity-expanding parser can see the source) and `filter`/`fe*` markup is
  STRIPPED before parsing (definitions, `fe*` primitives and every `filter`
  reference, by LOCAL NAME with namespace-prefixed spellings included; a
  byte scan alone was bypassable). Filters are therefore IGNORED, never
  rendered: an image that ships one (a status badge with a blur shadow)
  shows its unfiltered content, and the whole filter cost model — unbounded
  `numOctaves`, morphology and convolution, filter-region pixmaps — stays
  unreachable. Removing the references too is required: usvg DROPS an
  element whose `filter` link dangles. CSS-spelled filters
  (`style="filter:…"`, a `<style>` rule) cannot be stripped without a CSS
  parser and are rejected — in every spelling the parser actually accepts,
  including `filter/**/:…` (comments are skipped between the property name
  and the `:`) and the IE star hack `*filter:…` — and `marker`/`pattern`
  markup is rejected by the
  same structural pass: a marker subtree is re-converted per path vertex,
  and a pattern's pixmap is sized by the PATTERN's own rect (not the
  rendered target), so a tiny source could demand an allocation whose
  failure aborts the process. Duplicate element `id`s are rejected
  fail-closed (first- vs last-wins differs between parsers; counting a
  different `<use>` target than the renderer would undercount a bomb), and
  CSS `filter:` is checked in every `<style>` element regardless of `type`
  (the renderer may honor the text even for non-`text/css` types). Fonts are the app's own bundled faces (never
  system
  fonts), the source is token-capped (`MAX_SVG_TOKENS`, 400k) and nesting is
  DEPTH-capped (`MAX_SVG_DEPTH`, 512) before the
  XML parser runs (a deep `<g>` chain overflows the recursive parser/usvg
  stack even under the token count, which closing tags inflate without
  nesting), and the `<use>` expansion count is
  bounded by an ITERATIVE walk against the same token cap (nested `<use>` grows
  exponentially in the parsed tree from a tiny source, and a recursive walk
  could overflow the decode thread's stack). The
  raster buffer is capped at 3072 px per side (declared viewports can be
  arbitrarily large vectors). HTML and scripts are never interpreted as
  images. The SVG documents embedded in the bundled Noto Color Emoji font
  (`media::emoji`) go through the same hardened rasterizer and are bounded
  by the emoji texture caches (each `EMOJI_CACHE_ENTRIES` /
  `EMOJI_CACHE_BYTES`, reserved from the process-wide texture budget). Image
  jobs are panic-guarded: a decoder panic surfaces as a
  failure placeholder instead of killing the background thread
  (which would panic the UI on the next poll).
- Remote document titles — the frontmatter `title` or the first-H1
  fallback — are sanitized (control AND Unicode format/bidi
  characters stripped, whitespace collapsed, length-capped) before reaching
  window/tab titles, the page-info card and the bookmark/history stores.
- Local browsing history is stored in the OS per-user data directory only,
  bounded (20 000 entries / 8 MiB of plaintext; oldest dropped) and encrypted
  at rest with ChaCha20-Poly1305 (`aws-lc-rs`, the same crypto backend
  rustls links): random 96-bit nonce per write, 256-bit key in the single
  shared `store.key` (history, bookmarks, settings and session), mode 0600,
  directory 0700 on Unix. The key sits beside the
  data, so this protects the files against other users, backups and
  casual inspection, NOT against malware running as the same user. The key
  is installed atomically (private temp file + `hard_link`), so a second
  instance can only ever read the winner's complete key; a corrupt
  (wrong-size) key is repaired under a best-effort cross-process lock with
  stale-lock takeover. The lock is HELD until the replacement key is
  installed, and a lock timeout fails closed (the file is never deleted):
  two instances repairing at once can no longer destroy each other's fresh
  key. An oversized key file is rejected from its metadata without being
  read whole, and a key with
  permissions that cannot be tightened is refused fail-closed instead of
  trusted. Store loads read through a shared helper that opens with
  `O_NONBLOCK` and re-checks the OPENED handle (`settings::read_regular_file`,
  used by history, bookmarks and both settings reads): a FIFO swapped in
  between the metadata check and the open can neither block the UI thread
  nor be read, and the streaming read still enforces its cap if the file
  grows after the check. Only
  successfully loaded HTTPS pages and local files are recorded; retention is
  user
  configurable (default 3 months), "forget on exit" never reads or writes
  the files and wipes leftovers, and "Delete history now" removes the
  ciphertext (the shared key stays: bookmarks/settings/session need it);
  a delete waits for the background writer to drain
  (staying armed across a stalled disk and completing on a later tick), and
  a failed delete is reported in Preferences instead of looking successful.
  "Forget on exit" also disables "restore tabs on startup" and deletes any
  `session.bin`, so visited URLs cannot survive in that snapshot.
  Stored URLs lose authority credentials (`user:pass@`) but keep their exact
  path/query bytes, so a recorded page is the page that was loaded.
  Turning off "restore tabs" deletes the encrypted `session.bin`; a failed
  delete is surfaced in Preferences and retried at every start (and when the
  toggle is set off again), and the orphaned `.tmp.*`/`.bak` siblings are
  removed with it. An UNREADABLE
  `settings.bin` is reported in Preferences and never treated as "restore
  tabs off", so a transient read failure cannot delete the stored session;
  the same distinction guards WRITES: a file that exists but cannot be read
  or decrypted makes the read-modify-write REFUSE, because rewriting from an
  empty body would silently drop every other stored key (theme, zoom,
  restore-tabs, retention, window geometry), and a change that would push the
  plaintext past `MAX_SETTINGS_BYTES` is refused too (the reader's caps derive
  from `crypto::OVERHEAD_BYTES`, so a written file is always readable). The
  window geometry (position, size, maximized) is one of those encrypted
  preferences: the position is a weak "where you were" datum, so it stays
  encrypted with the rest and is removed by "Delete all data", never kept in a
  plaintext window-state file. A failed background preference/session write is
  surfaced in Preferences too (each worker's outcome is drained per frame
  into its own slot — a successful session save cannot clear a settings
  warning and vice versa — cleared only on a
  confirmed success), and a session snapshot that could not even be built
  is not recorded as saved, so it retries.
  Preferences (`settings.bin`, magic `MDST`) and session (`session.bin`,
  magic `MDSS`) use the same shared-key AEAD recipe; legacy plaintext
  `settings.conf` / `session.conf` and per-store `history.key` /
  `bookmarks.key` are deleted on load, never read (clean break, no
  migration). "Delete all data and close" (Preferences, two-click) suppresses
  every later store write and drains the writers before deleting the whole
  data directory and `process::exit`ing, so a queued blob cannot recreate it —
  and a removal that FAILS (open handles on Windows, permissions, read-only
  media) does not exit pretending the data is gone: writes are re-enabled,
  the failure is shown in the Local data card and the app keeps running.
  OS/CLI open targets supersede a stored session only for the current launch
  (an untouched window never overwrites it).
  File writes go through a temp file + rename with a
  per-process unique temp name (debounced writes run on a worker thread, so
  the UI thread never fsyncs; concurrent instances cannot interleave); the
  rename REPLACES the target atomically on every supported platform (no
  move-aside window), so a crash can never leave a missing or half-written
  store; any IO/crypto error degrades to
  an empty in-memory history instead of crashing. The worker queue is
  bounded to one pending blob per target file (newest wins; the app writes
  one data directory) and a UI-path flush writes what
  is queued before waiting at
  most 5s, so a hung disk can neither grow memory, lose the newest data nor
  freeze a frame, a delete or exit. A store wipe is enqueued as a DISCARD
  job on that same worker, so it runs strictly after any in-flight write:
  the exit-time forced wipe can no longer be overtaken by a stalled atomic
  rename that would resurrect the deleted ciphertext, a job is either handed
  to a worker that will run it or returned for a respawn (a liveness check
  followed by a push used to race the worker's exit and strand the blob),
  and the discard sweeps the store's `.bak` and `.tmp.*` siblings, so a
  preserved ciphertext cannot survive an explicit delete. Preferences and the session follow the
  same rule: `Prefs::set_*` and the per-change session save update an
  in-memory body and queue the encrypted write on dedicated background
  workers (one per file), and the exit path drains them; only a
  `restore_tabs` toggle waits briefly (bounded), so the preference is
  durable before the session file is deleted. The settings plaintext is
  cached per directory for the read-modify-write, so two preference changes
  cannot lose each other's keys; a concurrent instance's write to the same
  file is last-writer-wins (single-user preferences, not shared data). A
  store file that cannot be decrypted
  (missing/regenerated key, corruption) is preserved as `.bak` via an atomic
  `create_new` copy (`settings::preserve_blob_bytes`, shared by
  history/bookmarks/settings): never clobbers an earlier backup, no
  exists-then-rename TOCTOU, and the live file stays in place until a later
  save replaces it. Restored stores are tightened best-effort on load
  through the ALREADY OPENED handle (`fchmod`): group/other bits are
  stripped, owner bits never widened, and a symlink swapped in after the
  open can never be followed or `chmod`ed; data/key directories tighten the
  same way (never widened); the key file itself is opened with `O_NOFOLLOW`
  and tightened on that opened handle, so a symlinked `store.key`
  is refused instead of `chmod`-ing its target (the old check-then-chmod by
  PATH had a window where a swapped symlink was followed); crash-orphaned
  `<store>.tmp.*` files AND the key-creation temps
  (`store.key.tmp.<pid>.<seq>`, which hold raw key bytes) are swept.
- Local bookmarks use the same encrypted-store recipe (`bookmarks.bin`,
  magic `MDBM`, ChaCha20-Poly1305, random 96-bit nonce, 256-bit shared key
  in `store.key` mode 0600 / directory 0700 on Unix, atomic temp+rename,
  the same bounded background writer with a 2s debounce and the same
  write-then-exit flush, key installation and unreadable-blob `.bak`
  preservation as history): they are
  explicit user data with no retention window, capped at 2000 entries (an
  add at the cap is refused with a toast, never silently dropping an
  older bookmark) AND at 48 MiB of serialized
  plaintext (`MAX_BOOKMARK_STORE_BYTES`; an entry that would cross the byte budget — an ADD or a
  tile rename — is refused the same
  way; a body over it that arrived from disk is trimmed oldest-first on
  load, and the encrypted-file read cap is derived from the plaintext
  budget plus the AEAD overhead; the settings read cap
  (`config::MAX_SETTINGS_BYTES`, 256 KiB) lives in the same module), and
  listed ONLY on the Home page. A
  failed save is surfaced there instead of looking successful. The stored
  tile customizations (name, glyph, palette color) are sanitized whenever
  they enter the store or are read back (control characters dropped, glyph
  capped
  at 16 chars, color index bounded; the Home editor only renames today,
  while stored glyphs and colors keep rendering), and
  tile favicons reuse the existing bounded favicon pipeline (HTTPS
  origins only, the site's own `/favicon.*` over a plain GET).
- Credentials in pasted URLs (`user:pass@host`) are stripped before the
  network and never logged; any non-https scheme with an authority
  (`http://user:pass@host`, `http:/user:pass@host` with a single slash,
  `http:\user:pass@host` with a backslash, `http:///user:pass@host` with
  extra slashes, protocol-relative
  `//user:pass@host`, `ftp://…`, `data:`, `javascript:`) is rejected and
  SANITIZED in every displayed form (address field, error view, "Copy log")
  before parsing, so pasted credentials can never be folded into a request
  path or echoed back; a `scheme:user:pass@host` spelling (no slashes, a
  second colon before the `@`) is scheme syntax too and is rejected, with
  its userinfo removed by the same sanitizer, instead of being read as a
  userinfo host for `https://host`, while the plain `user:pass@host` and
  `user:pass@host:8080` forms keep working with credentials stripped. Local
  ("bad path") messages echo the rejected input through the same bounded,
  credential-free helper, and the post-parse local read errors echo the
  `file:` URL through it as well (`protocol::local::echo_url`): the parser
  already rejects userinfo upstream, but the log sink stays safe even if a
  credentialed URL ever reaches it, mirroring the network path. The stripping does not depend on the caller: the
  shared network entry point (`protocol::client::get_bytes`) removes
  authority credentials from the request URL itself, fail-closed, before
  anything is sent (and its own rejection message reuses the shared
  sanitizer instead of echoing the credential-bearing URL). Server-controlled
  redirect URLs embedded in
  transport error text are sanitized the same way before reaching the
  error view AND image failure tooltips, and any final URL that did not
  land on `https` is rejected
  before the body is read (defense in depth over `https_only(true)`).
  Userinfo on the LANDED redirect URL is stripped before display/history
  (fail-closed when it cannot be removed), collected `Content-Type`
  values are capped at 8 (`MAX_CONTENT_TYPE_VALUES`; more values are
  rejected instead of inspecting an attacker-chosen prefix), and route/display URLs enforce the SERIALIZED
  `config::MAX_URL_BYTES` cap after `.md`/index expansion (a byte cap:
  percent-encoding makes one 4-byte input char up to 12 ASCII bytes, so the
  old char-based check on the serialized URL silently dropped valid
  under-cap addresses, and history/bookmarks refused to record them).
  The error view's external-open action is no exception: at click time it
  re-resolves the asked address through the app's own navigability parser
  and hands ONLY the resulting sanitized `https:`/`file:` URL to the OS
  default handler (credentials stripped, blocked schemes and remote `file:`
  shares refused), so a pasted password can neither reach the browser's URL
  bar nor the launcher's argv.
- Key file reads are capped and handle-checked (`O_NONBLOCK | O_NOFOLLOW`
  open, the opened handle re-checked, at most 33 bytes read, permissions
  tightened on that same handle): a FIFO swapped in
  cannot block, a symlinked key is refused without ever being followed or
  `chmod`ed through, and a file grown after the check cannot force a large
  allocation. History/bookmark stores and the session loader reject
  `file:` URLs with credentials, ports or remote hosts exactly like the
  document reader (`file://localhost/` stays local), and the reader itself
  rejects a RELATIVE `file:` spelling (`file:notes.md`, which the WHATWG
  parser would silently rebase onto the filesystem root) instead of opening
  a file the user never named; the session loader
  rejects credentials on `https` too, and session writes
  store an over-long tab URL as Home instead of writing it unbounded.
  A relative `XDG_DATA_HOME` is ignored (the XDG base-directory spec
  requires an absolute path) and the same absolute-only rule now covers
  `HOME`, `APPDATA`/`LOCALAPPDATA` and `~/` expansion: the data directory
  (and therefore `wipe_all_data`'s recursive delete) can never resolve
  under the current working directory.
- `unsafe_code = "forbid"` in `Cargo.toml`. Supply chain: `Cargo.lock`
  committed, `cargo audit` + `cargo deny check` via `make audit`
  (configured by `.cargo/audit.toml` and `deny.toml`) before releases.
  `deny.toml` allows an explicit license set (the app itself is
  `MPL-2.0`, see `LICENSE`); the required notices (including bundled fonts)
  ship in `THIRD_PARTY_NOTICES.md` with every binary and are embedded in it
  for the internal `about:licenses` page. Image decoding is pure Rust
  (no C libraries): `avif-decode` (BSD-3-Clause), `avif-parse` (MPL-2.0,
  file-level copyleft, unmodified) and `jxl-oxide` (MIT/Apache-2.0). YAML
  uses the maintained `serde_norway` fork (archived `serde_yaml` and unsound
  `serde_yml` are not used).
- Desktop SSRF note: fetching `localhost` / private IPs is user-initiated
  (the user typed the URL or opened a document that links it), not
  server-side; allowed by design for local preview. Document subresources
  (images) inherit the same policy: they are HTTPS-only and still require a
  valid certificate, so a local service without one cannot be reached.
  No proxy is used on purpose (deterministic direct TLS).

## Build artifacts

Release binaries and packages must not carry the builder's identity. Rust
embeds the source path of every panic location — dependency sources included
(`$HOME/.cargo/registry/src/...`) — so `make`/`scripts/build-dist.sh`,
`windows.ps1` and the RPM spec's `%build` remap those prefixes at compile time
(`--remap-path-prefix`); the `.zip` writer drops the Info-ZIP Unix uid/gid
extra field; and the tar staging sweeps extended attributes / passes
`--no-xattrs`, so a macOS build does not embed its `com.apple.provenance` blob
in Linux packages. The RPM spec pins `%_buildhost` and
`%defattr(-, root, root, -)`. `scripts/check-dist.sh` (`make check-dist`, also
enforced by `make dist` and `make validate`) fails the build if any `dist/`
artifact still carries the builder's paths, hostname, uid/gid or xattrs. See
`BUILDING.md` ("Build-artifact privacy").

Release builds themselves run in CI (`.github/workflows/release.yml`) on
fresh GitHub-hosted runners from the tagged commit: macOS universal, Linux
x86_64/arm64, Windows x86_64 (MinGW cross) and Windows arm64 (native
MSVC), plus the RPM and Flatpak packages. The publish job re-runs
`check-dist.sh` over the merged artifact set, so no target can bypass the
gate before a release exists.

## Reporting a vulnerability

Open an issue in the project tracker with `[security]` in the title, or
contact the maintainer privately if the issue allows network exfiltration,
RCE, or TLS bypass. Include:

1. MDowser version (`Cargo.toml`) and OS/arch.
2. Minimal reproduction (URL or Markdown snippet, no credentials).
3. Expected vs actual behavior.

Do not include private hosts, tokens, or credentials in public reports;
redact them (`example.com`, `***`).

## What to expect

- Acknowledgement within a few days.
- Fix in `Unreleased` + patch release for TLS/network bypass, OOM,
  or arbitrary-file/credential leaks.
- Public disclosure notes in `CHANGELOG.md` once fixed.
