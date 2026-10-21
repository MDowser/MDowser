# Architecture

Single binary (`src/main.rs` → `app::MdowserApp`), immediate-mode egui.
User-facing behavior lives in [`INTERFACE.md`](INTERFACE.md),
[`RENDERING.md`](RENDERING.md) and [`NAVIGATION.md`](NAVIGATION.md); the caps
and threat model are in [`SECURITY.md`](SECURITY.md).

```
main → app (tabs + active index + zoom + favicons + encrypted history and
           bookmarks + shortcuts + Markdown saving to the downloads folder
           (`save`) + the macOS-only native menu bar
           (`menu`, macOS only) + native window geometry (`window`:
           first-frame placement + persisted position/size/maximized) +
           OS/CLI open targets)
  ├─ error (the unified `BrowserError` every module converts its failures
  │          into: one message-shaped type for the error view and logs)
  ├─ util (shared helpers: panic guard for background jobs, display caps
  │         (`truncate_chars`), credential stripping for logs, stored-URL
  │         normalization)
  ├─ assets (embedded Noto faces: color emoji, monochrome emoji, CJK,
  │          Latin + script set; the two largest ship Brotli-compressed
  │          and are decompressed once into process-lifetime
  │          buffers; app icon PNG; OFL-1.1; embedded
  │          THIRD_PARTY_NOTICES.md)
  ├─ browser/tab (Home (`about:home`)/Loading/Settling/Loaded/History/
  │               Licenses/Failed, back/forward, throttle, poll; `state`
  │               holds the visible state, `poll` the per-frame handoff,
  │               `nav` the navigation/stack methods, `nav_moves`
  │               Back/Forward/Refresh/Stop and `info` the page summary
  │               behind the omnibox indicator card)
  ├─ config (single-sourced bounds/timeouts registry; one flat `mod.rs`
  │          on purpose: the shared anti-abuse audit reads it in one place)
  ├─ store (shared encrypted-store plumbing)
  │    ├─ crypto (AEAD file format + shared store.key handling)
  │    └─ writer (one serialized background writer per encrypted store)
  ├─ history (encrypted local visits: retention, key, atomic save;
  │            `persist` holds pruning + the file format)
  ├─ licenses (embedded third-party notices parsed once; about:licenses)
  ├─ bookmarks (encrypted local bookmarks: key, atomic save; `persist`
  │             holds the line format + background write)
  ├─ suggest (address-suggestion ranking shared by both omniboxes:
  │           bookmarks → history → builtin, deduped)
  ├─ builtin_sites (the single curated omnibox site list, vendored from
  │                 MDowser-Sites (`make sync-sites`) into
  │                 `assets/builtin_sites.txt`, embedded and parsed once)
  ├─ settings (encrypted preferences + session: theme, startup zoom,
  │            session restore, window geometry; settings.bin/session.bin
  │            + store.key, atomic file helpers; `session`, `file`,
  │            `platform` submodules)
  ├─ protocol/client (capped GET cascade, global rustls pool;
  │    │              `parse_navigable` = HTTPS or local file)
  │    ├─ get (shared capped streaming GET: documents + subresources)
  │    └─ fetch (fetch_with_fallback + finish_document: frontmatter,
  │               events, copy text, first-H1 title, anchors)
  ├─ protocol/resolver (.md candidates, display normalization, dedup)
  ├─ protocol/content_type (markdown Content-Type + accepted image types)
  ├─ protocol/rewrite (absolutize links pre-render; scheme policy per base:
  │                https-only from the web, https+file from a local doc)
  ├─ protocol/local (file:// documents: path candidates (exact, .md
  │                 completion, directory indexes), capped UTF-8
  │                 reads with cooperative cancel; reuses `finish_document`)
  ├─ cache (encrypted HTTP disk cache: documents/images/favicons;
  │         `policy` = Cache-Control/Expires/ETag freshness + canonical
  │         keys, `format` = bounded record encoding + validated hashed
  │         stems, `docs` = one encrypted file per host, `images` = one
  │         encrypted file per URL, `favicons` = one encrypted file for
  │         all probes; 512 MiB / 50k entries, FIFO, per-class clears)
  ├─ markdown (frontmatter split+cap (`Cow` body: no copy without a block),
  │            parser options, `:shortcode:` emoji,
  │            anchors (heading/footnote targets; a heading that slugs to
  │            nothing gets a reserved positional id), outline (the sidebar's
  │            heading list: level/title/id/event index, capped), `:::`/`!!!`
  │            admonitions +
  │            structural `:::` custom containers (same markers as the JSX
  │            equivalents, column-0 only),
  │            lax link join (blank line inside `[...](...)`), GFM-style
  │            bare-URL linkification (`www.`/`https://`/bare-domain/
  │            local `file:` text nodes; root-zone TLD allowlist +
  │            file-name guard; code, existing links and image alt text
  │            untouched; the scan lives in `autolink/scan`),
  │            lines yielding more than `MAX_TABLE_COLS` table CELLS
  │            rejected BEFORE the eager
  │            parser allocates one tree item per cell (fences are
  │            recognized inside blockquotes AND on a list item's first
  │            line, and only cover lines that STAY in that container: an
  │            unquoted/unindented line is scanned, so a hostile table can
  │            never hide behind such an opener),
  │            highlight ranges; `document/` splits the parser options and
  │            event walk (`mod`), the pre-parse repair pass (`normalize`
  │            + `normalize_links`/`normalize_tables`/`normalize_markup`),
  │            the Copy-text derivation (`plain`) and the first-H1 page-title
  │            fallback (`title`))
  ├─ media (pure-Rust image pipeline for subresources)
  │    ├─ fetch (background download via the encrypted disk cache;
  │    │         content-type gate + decode)
  │    ├─ emoji (color emoji: harfrust shaping + SVG-table extraction
  │    │         through resvg; bounded inline textures)
  │    ├─ decode (magic sniff + dispatch; `raster` = image/jxl-oxide/
  │    │         avif-decode, `svg` = usvg/resvg sanitize + rasterize)
  │    ├─ cache (per-document textures; url → Loading/Ready/Failed;
  │    │        one transient-failure retry)
  │    └─ favicon (session per-origin icon textures for the tab rail;
  │              probes served from the encrypted disk cache first)
  ├─ viewer (document rendering; the internal about:licenses page too)
  │    ├─ markdown (CommonMark block renderer: `walk` = recursive block
  │    │            walk, `blocks`/`headings`/`tables`/`html` = block
  │    │            kinds, `components` = native component blocks
  │    │            (tabs/accordion/card/steps from JSX or `:::`, plus
  │    │            `<CodeGroup>`/`:::code-group` desugared
  │    │            to tabs by the normalizer), `frags`/`items`/
  │    │            `links`/`menu`/`budget` = inline assembly and layout
  │    │            rules)
  │    ├─ inline (inline fragments + `code_span` painted code pills)
  │    ├─ table (measure/layout `mod` + paint `render`)
  │    ├─ image, `code/`, overlays (`overlays/find.rs` = the floating
  │    │   find bar UI), bidi, cache, `find.rs` + `find/tests.rs` (in-page
  │    │   search state + case-insensitive matching), `outline.rs` (the
  │    │   heading scrollspy: current section, page-end selection + reveal
  │    │   math) and `outline_panel.rs` (the permanent right sidebar:
  │    │   wrapped virtualized rows) with `inline/tests_find.rs`,
  │    │   `tests_find.rs` (fragment highlight + layout/scroll contract) and
  │    │   `tests_outline.rs` (outline sidebar end-to-end: wrapped rows, row
  │    │   click, scrollspy cursor, dense heading clusters, page end,
  │    │   headingless slot, responsive width)
  └─ ui (browser chrome: `theme/` document styling + widget kit, icons,
          tabs (`TabsResponse`, `rows`) with the live drag reorder, navbar,
          omnibox, home (with the
          Home-only encrypted bookmark list), bookmarks (grid + `tile`),
          preferences (tabbed Preferences window: General/Data/Shortcuts,
          incl. the licenses link; `widgets` kit), history_page (internal about:history
          page), error_view, `row` (the shared painted-row recipe behind
          menus/dropdowns/rows), `ui::forget_tab()` (single function registry
          for closed-tab egui-memory keys))
```
Key contracts per area (the module map above names every module):

## Network and documents

- Network runs only through `protocol::client` on background threads
  (`Promise::spawn_thread`), on ONE long-lived tokio runtime
  (`client::run_network`) that keeps the shared HTTP pool's executor between
  jobs (per-job `current_thread` runtimes failed with `error sending request`);
  the UI polls each frame.
- Documents go through `fetch_with_fallback`; image and favicon subresources
  through `media::fetch`, on the same `get_bytes` (cancellable connect,
  redirect/https check, per-chunk stall timeout, running byte cap).
- The `Content-Type` gate runs BEFORE the body streams, on EVERY declared value
  in full: a bounded copy could hide a `charset=` beyond the cut. A non-accepted
  type is rejected without download or decompression.
- Three slot limiters: `MAX_CONCURRENT_FETCHES` (network documents),
  `MAX_CONCURRENT_LOCAL_READS` (local reads; a hung filesystem never holds a
  network slot) and `MAX_CONCURRENT_IMAGES` (image/favicon jobs), plus
  `MAX_CONCURRENT_IMAGES_PER_DOC` per document: a gallery never queues a
  navigation or starves other tabs and favicon probes.
- Single-flight per tab: `cancel_inflight()` plus an `AtomicBool` checked per
  candidate and per chunk, during connect (`futures_util::select` race), with a
  15s per-chunk stall timeout (`CHUNK_TIMEOUT_SECS`).
- Transparent `gzip`/`brotli`/`deflate` decompression on the shared client;
  caps apply to decompressed bytes.
- Cancellation: every fetch checks the flag per candidate and races each body
  chunk wait (`futures_util::select`) against it, so a superseded navigation
  drops in ~20ms even mid-stall, not after the 15s `CHUNK_TIMEOUT_SECS`.
- Redirects are followed up to `MAX_REDIRECTS`; one that leaves `https` is
  rejected before the body is read.

## Local files

- Local files run through `protocol::local` on the same job thread (no
  runtime). `parse_navigable` is the ONLY navigation parser (https plus
  `file://`/absolute/`~`/relative paths).
- A `file://host/` share is rejected (`file://localhost/…` is this machine).
  Only regular files are read: `O_NONBLOCK` open, the OPENED handle re-checked,
  so a FIFO swapped in after the `metadata` check cannot block the thread or pin
  a fetch slot.
- Candidates are the exact path, `.md` completion when the stem does not exist,
  and the conventional directory indexes.
- Reads reuse `MAX_BODY_BYTES` / `MAX_IMAGE_BYTES` with a cooperative cancel
  check between 64 KiB chunks. Bodies go through the SAME
  `client::finish_document` as HTTPS (frontmatter, events, copy text, first-H1
  title, anchors, display URL), so the viewer cannot tell them apart.
- `resolver::is_same_logical` compares local files by exact path (never the
  `/.../` collapsing rules); `display_string` shows the decoded path and
  `server_home_of` the containing directory.
- `rewrite` lets `file:` destinations out of a local base only, and only
  `https:`/`file:` through. Favicons are never probed for local pages.
- OS/CLI hand-off (`main` → `app::MdowserApp::new`) opens one tab per target
  and wins over session restore. `main::cli_action` answers `--help`/`-h` and
  `--version`/`-V` on stdout (`print_help`/`print_version`, the app's only
  stdout writes) and exits before any window; the remaining list is capped at
  `MAX_TABS` (`main::collect_initial_targets` skips flags/empties), so an
  OS-controlled argument list can never allocate an unbounded tab vector.
  Per-target length stays enforced by `parse_navigable`: oversized input
  becomes that tab's local error, never a truncated navigation.

## Encrypted disk cache

- `cache/` holds 512 MiB / 50 000 entries and shares `store.key` with
  history/bookmarks/settings. Documents: one AEAD file per host, keyed by
  `doc_key` so every route spelling hits one record; images: one per URL
  (sharded); favicons: one for all probes.
- `Cache-Control`/`Expires` (+`Date`/`Age`) set freshness under the absolute
  1-year ceiling. `no-store` (or `Vary: *`) skips storage and evicts older
  records for that URL; `no-cache` revalidates; a 304 refreshes
  `ETag`/`Last-Modified`. `Refresh` forces revalidation.
- When no fresh copy can be obtained (transport failure, every route
  rejected, a failed revalidation after the cascade), the stored record is
  served MARKED instead of the error view: `Fetched::stale_copy` drives the
  omnibox indicator (cloud-off offline / archive saved) and the page-info
  wording, while images/favicons fall back to their stored bytes unmarked. A
  stale body is never served as current.
- Past either bound the oldest entries evict FIFO (never an error); per-class
  clears live in Preferences/Data.
- Startup decrypts only the small indexes (bodies lazily per hit) plus a
  bounded RAM fast path. Body writes run synchronously on the fetching job
  thread; index/favicon writes use a debounced background flusher (exit drains
  it, bounded).
- "Forget on exit" disables the cache and wipes leftovers.

## Page transitions

- Stale-while-revalidate: `Loading::backdrop` keeps the old document and its
  images on screen, textured, until the new document resolves. Textures are
  never shared across pages (`Tab::images.cancel_all()` on
  resolve/failure/reset); the encrypted disk cache below avoids re-downloads.
- A visual timeout with a backdrop restores the page (history snapshots and
  textures intact) and reports a toast; only a first load without a backdrop
  settles into `Failed`.
- `Stop` and the visual timeout restore the stacks like a FAILED fetch
  (`settle_abandoned` takes `pushed_history`): the pushed entry stays
  reachable.
- After the fetch resolves, a `Settling` gate preloads the new document's images
  in a separate cache while the backdrop (or the stable empty panel on a first
  load) stays on screen with the loading indicator. The swap to `Loaded` is
  atomic once every image settles or `DOC_IMAGE_GATE_SECS` (5s) passes, so no
  placeholder-to-bitmap reflow reaches the screen. Text-only pages skip the
  gate.

## Images and favicons

- Acceptance: accepted `Content-Type` (image types plus legacy aliases, or
  `application/octet-stream`/`binary/octet-stream`/missing) + magic sniff +
  decoder validation.
- Caps: 16 MiB/image, 48 MP, 12288 px/side (re-checked on the decoded frame),
  512 MiB decoder allocation, 384 MiB texture budget per document plus a 768 MiB
  process-wide budget across tabs AND favicons, 512 images/document, 3
  concurrent jobs per document with 6 process-wide, a 60s total timeout per
  image (documents stay at 20s; the stall cutoff stays 15s for both).
- Layout caps: inline SVG icons clamp to 32px tall (`DecodedImage.vector`); an
  image sharing a line with text taller than `config::MAX_INLINE_IMAGE_H` (2
  body lines) or wider than most of the column becomes a centered block.
- Decoded pixels are downscaled to 3072 px (AVIF included: every decoder goes
  through the same `finish` downscale) and uploaded once; raster EXIF
  orientation is applied before layout. Favicons decode at `MAX_FAVICON_DIM`
  (160 px; drawn at the toolbar icon size, ~27 px) and reserve their bytes in
  the process-wide budget.
- A refused process-wide reservation is TRANSIENT for images and favicons: the
  entry backs off for `IMAGE_BUDGET_RETRY_SECS` (bounded by
  `MAX_IMAGE_BUDGET_RETRIES`) and retries rather than fail for the page's life
  or burn the network retry on VRAM. While every queued entry waits, `pump`
  returns until the earliest deadline (`next_budget_retry`, recomputed when the
  queue drains).
- A favicon backoff whose cooldown elapsed is retried by `FaviconCache::poll`
  ITSELF (its scheduled repaint is the only frame an unrendered origin — a
  closed tab, a scrolled-out tile — gets), and past the attempt cap it FAILS so
  it stops counting as pending. A budget-only failure is re-probed once VRAM
  frees. A saturated origin cache (`MAX_FAVICON_HOSTS`, nothing evictable)
  answers an unknown origin without parsing or allocating.
- An entry still queued past its deadline FAILS instead of staying pending; the
  renderer draws hourglass/warning-icon placeholders until then.
- A transient fetch failure (network/stall/truncated decode, 5xx/429/408) is
  retried once (`MAX_IMAGE_RETRIES`); content-type rejections, other 4xx and
  cancellations are final.
- Every image/favicon entry has a wall-clock deadline
  (`IMAGE_JOB_TIMEOUT_SECS`): past it the entry becomes a failure placeholder
  and stops counting as pending (cancel flag set; a decoder stuck mid-call
  finishes and its result is ignored). Image jobs are panic-guarded: a decoder
  panic becomes a placeholder, never a poisoned `poll-promise` thread.
- Sizing and grouping: [`RENDERING.md#images`](RENDERING.md#images).

## SVG

- SVG rasterizes STATICALLY (`usvg`/`resvg`, pure Rust). `<script>`,
  `<foreignObject>` and the SMIL elements
  (`animate`/`animateTransform`/`animateMotion`/`set`/`discard`) are rejected by
  the structural pre-pass, never executed; every `href` (network, local and
  `data:`) is never resolved. DTDs (`<!DOCTYPE>`/`<!ENTITY>`, unbounded entity
  expansion) are rejected by that parse (roxmltree pinned to `allow_dtd: false`,
  before usvg's entity-expanding parser).
- `filter`/`fe*` markup is STRIPPED (definitions, primitives, every
  reference): filters are ignored, so the cost model and filter pixmaps stay
  unreachable and usvg's dangling-link rule cannot drop the element.
  CSS-spelled filters (`style`/`<style>`, un-strippable without a CSS parser)
  are rejected in every parser spelling, including comments before the `:`
  (`filter/**/:blur(1px)`) and the IE star hack `*filter:`.
- `marker`/`pattern` markup (a marker subtree re-converts per path vertex; a
  pattern pixmap is sized by its own rect) is rejected before parsing. The pass
  matches LOCAL NAMES in the SVG namespace (or none, like usvg), so prefixed and
  namespace-less spellings cannot slip past a byte scan; duplicate element ids
  are rejected fail-closed.
- Fonts are the bundled Noto faces (never system fonts). The source is
  token-capped (`MAX_SVG_TOKENS`) and depth-capped (`MAX_SVG_DEPTH`, enforced
  before the XML parser, which recurses per level). Every `<use>` counts as a
  fresh expansion (an immediate self reference is the only skip, like usvg;
  whitespace-padded `#id` values trimmed like `svgtypes::IRI`) against the same
  cap by an ITERATIVE walk (no stack overflow on deep nesting), so a mutually
  recursive pair trips the cap before usvg's 1M-node limit. The raster is capped
  at `MAX_TEXTURE_DIM` per side.
- HTML and scripts are never interpreted as images. The bundled Noto Color Emoji
  font's SVG documents use the same hardened rasterizer. Threat model:
  [`SECURITY.md#svg`](SECURITY.md#svg).

## Navigation history and the local store

- `back`/`forward` are capped at 250 entries, snapshots restored on failure. The
  pending URL is committed as `current` before stack moves, so Back/Forward
  during `Loading` never create self-duplicates. A failed NEW navigation keeps
  the page it pushed (`pushed_history`) on `back`, so the previous page stays
  reachable; failed Back/Forward restore the original stacks. A superseding
  navigation bumps `doc_generation` (Loading → Loading keeps the same state
  KIND; the display memo is keyed on (kind, generation)), so the rail never
  keeps the superseded URL's title/favicon.
- A resolved page is handed to `App` exactly once (`Tab::pending_visit`) and
  recorded; failures and stopped loads never are. Consecutive visits to one URL
  update the newest entry.
- The store is bounded (20 000 entries, 8 MiB of plaintext, oldest dropped); the
  derived byte total is incremental, so a recorded visit answers the caps in
  O(1). Retention is `Retention::Days` (default 90), `Forever`, or `Session`
  (in-memory only: never read/written, leftovers wiped on load and exit).
- Saves are atomic (per-process unique `.tmp` + rename, 0600/0700 on Unix) at
  most every 2s while dirty and once on `App::on_exit`. The blob is built on the
  UI thread (bounded); write + fsync run on a worker thread, so a slow disk
  never stalls a frame.
- The worker queue holds one pending blob PER TARGET FILE (newest wins). A
  flush WRITES what is queued (never drops it; the store cleared `dirty` at
  enqueue) before waiting at most 5s for exit, so a hung disk cannot grow
  memory, lose the newest data or freeze a frame, delete or exit. A write,
  delete or exit flushes first, so a stale blob never lands over a newer one; a
  failed background write re-arms `dirty` for the synchronous retry.
- When a stalled disk outruns the flush timeout, a delete stays ARMED (the wipe
  completes on a later tick; only real write failures re-mark `dirty`), so it is
  never resurrected. A delete is a JOB on the same worker
  (`WriteJob::discard`), behind any in-flight write, so an exit-time wipe cannot
  be followed by the stalled worker renaming its temp blob.
- A job is handed to a worker that WILL run it or returned to the caller
  (`StoreWriter::push` checks the worker's `exited` flag under the same lock the
  worker takes for its exit decision); `store::writer::queue_or_respawn`
  replaces an exited worker and retries, so the newest blob — or a "Delete
  history" discard — is never left in a queue nobody reads. The discard sweeps
  `.bak` and `<store>.tmp.*` siblings (`settings::remove_store_file`), so
  preserved ciphertext cannot survive an explicit delete. A failed delete (e.g.
  a locked file) is reported in Preferences.
- Stored URLs are normalized (`util::normalize_stored_url`): only authority
  credentials are removed (path/query bytes, nested URLs included, preserved) on
  record, on load and for the bookmark-ribbon comparison. An undecryptable blob
  is kept as `.bak` before a later save could replace it (a wipe removes the
  backup too).
- Store loads go through `settings::read_regular_file` (`O_NONBLOCK` open, the
  OPENED handle re-checked): a FIFO swapped in after `metadata` cannot block the
  UI thread, and the streaming read enforces its cap. A world-readable store is
  tightened through that SAME handle (`fchmod`), never a fresh path lookup, so a
  symlink swapped in after the open is neither followed nor `chmod`ed.
  Crash-orphaned `<store>.tmp.*` files and key-creation temps
  (`store.key.tmp.<pid>.<seq>`, raw key bytes) are swept on load.
- File format: magic `MDHS`, version 1, 12-byte random nonce,
  ChaCha20-Poly1305 with the header as AAD. The 256-bit key lives in the single
  shared `store.key` (history, bookmarks, settings, session; created 0600;
  installed atomically via a private temp + `hard_link`, so two instances read
  only the winner's complete key; a pre-existing key with group/other bits is
  tightened on its OPENED handle, `O_NOFOLLOW` refuses a symlinked key, and a
  key whose permissions cannot be tightened is refused, never deleted).
- "Delete history" keeps the shared key; only "Delete all data and close"
  deletes the directory. A FAILED removal (open handles, permissions) does not
  exit pretending the data is gone: writes re-enable and Preferences shows the
  failure. Any IO/crypto error degrades to an empty in-memory store plus a
  Preferences warning; history never blocks or crashes the app.
- `ui::history_page` renders the local history as an INTERNAL PAGE
  (`about:history`) in a tab, in the same reading sheet as a document: a capsule
  search field over rolling date sections (Today / Yesterday / Previous 7 days /
  Previous 30 days / Older), one favicon per row from the shared session cache,
  per-row remove and two-click delete-all. `Tab::open_history` intercepts the
  URL before `parse_https` (never fetched, recorded or requested) and treats it
  as a real navigation so Back/Forward work. `App::open_history` (rail button /
  Ctrl+H / Cmd+Y) creates a NEW tab after the active one and activates it, so
  the page being read is never replaced; a row click navigates that same tab to
  the visit, Back returning to the list (a repeat request while the active tab
  shows the page is a no-op). Each row's remove control is keyed by the visit
  `(timestamp, URL)`, never its position, so deletion cannot move keyboard
  focus to a neighbouring row.
- The internal licenses page (`about:licenses`) shares that bookkeeping
  (`Tab::open_internal_page` backs `open_history`, `open_licenses` and
  `open_home`). The new-tab page is the third internal page (`about:home`,
  typeable, empty capsule): a real back/forward entry, so passing through Home
  never loses the Forward chain, while Reload stays disabled and the session
  file keeps the `None` Home marker (no format change).
- `licenses::document` embeds `THIRD_PARTY_NOTICES.md` and parses it once per
  process through `finish_document`; `viewer` renders it with the normal
  document pipeline (menu, selection, live links), and `App::open_licenses`
  backs the Preferences footer link.
- `ui::preferences` keeps only the floating, FIXED-SIZE dialog (borderless
  custom header, tab bar, grouped section cards, list rows, a toggle switch, and
  a read-only keyboard shortcut reference in English). General: theme, startup
  zoom, restore-tabs and the compact version/licenses footer ("View third-party
  licenses"); Data: the retention dropdown, "Delete history" (two-click
  confirmation) and "Delete all data and close" (two-click: wipes the data
  directory and `process::exit`s without further saves). Every dropdown row
  keeps its geometry on hover; a delete-confirmation note takes space only while
  armed (arming may push the cards below; the inner scroll covers small
  viewports). `App` owns its visibility.

## Tabs

- `ui::tabs` reports select/close/reorder/new/history through one
  `TabsResponse`. A row's interaction id is keyed by the TAB (never its slot),
  so focus, hover, tooltip and the in-flight drag follow the page when the list
  reorders (`Alt+Up`/`Alt+Down` move focus with the reordered row).
- The drag reorder follows the bookmark grid's contract (state in egui memory,
  applied live, persisted only on start/stop) with ONE difference: drop targets
  are GEOMETRIC (`Context::rect_contains_pointer` on THIS frame's row rect,
  which honors what is on top), not `Response::contains_pointer`, whose
  frame-start snapshot of the PREVIOUS frame's rects lags a live reorder and
  makes the dragged tab bounce between slots. The same read drives the hover
  wash.
- A drag keeps the overlay open wherever the pointer goes, the dragged tab's id
  self-heals when its row closes mid-gesture (so `ui::forget_tab` needs no
  entry), and `App::move_tab` is the grid's remove+insert with `active`
  re-indexed so the shown page never changes. The overlay's hit rectangle is
  stored at its CURRENT animated clipped width, so the space a row vacates
  during open/close never swallows a click.
- The session snapshot compares URLs per index, so a reorder persists through
  "restore tabs on startup" with no file-format change.

## Bookmarks

- Bookmarks are explicit user data, encrypted like history (`crypto`: magic
  `MDBM`, random 96-bit nonce, 256-bit shared key in `store.key`, 0600/0700 on
  Unix; `store::writer` for the debounced write). Plaintext: one
  `timestamp\turl\ttitle\ticon\tcolor` line per bookmark (icon and color
  optional; three-field lines from before the tile customization still parse).
- Ctrl/Cmd+D or the toolbar bookmark ribbon toggles the loaded page
  (`Tab::bookmark_target`; a toast confirms, and Home/Loading/History/Failed
  have nothing to bookmark). The "quick access" grid renders ONLY on the Home /
  new tab page (`ui::bookmarks`): responsive, virtualized tiles in a fixed-width
  block centered under the address capsule, newest first.
- The omnibox ribbon (`omnibox::BookmarkState`, computed by `App` from the
  store: unavailable / outline / filled) shares ONE right slot with the clear X,
  so editing or a load resolving never shifts the field.
- A tile shows the custom glyph (`icons::paint_text`, color emoji or monochrome
  fallback) when one exists, else the site favicon (the shared session
  `FaviconCache`, HTTPS origins only, requested by visible tiles through
  `texture_for_url`, which derives the origin key from the URL string so a
  settled tile costs no `Url::parse`), else the title's first letter on a stable
  palette color (stored index, else a hash of the URL).
- The name lays out to at most two centered rows, elided with `…` by the layout
  the galley is memoized per
  URL (invalidated by the store `generation`, the theme and the tile width), so
  a settled Home grid builds no `String`/`LayoutJob` per tile per frame. The
  floating scrollbar stays hidden until the rows exceed
  `config::BOOKMARK_GRID_MAX_VISIBLE_ROWS`.
- Clicking a tile — icon or name — opens it in that tab; Cmd/Ctrl+click asks
  `App` for a new tab; dragging reorders the grid (live, mirrored between
  display and store order); `Alt+Left`/`Alt+Right` on the FOCUSED tile move it
  one display slot (`move_indices` mirrors display slots onto store indices, so
  a keyboard user can arrange the grid like a drag).
- The tile editor is INLINE (egui memory keeps the open field; no dialog): the
  hover pencil opens the rename field whose same spot becomes a check that
  saves. Enter or a click elsewhere commits (sanitized by the store, saved
  debounced), Esc drops the draft (`Tab::tile_editing` keeps the shortcut
  handler off the key) and the hover X removes the tile. The pencil/X ids are
  keyed by the bookmark URL (not the grid slot), so a live reorder or removal
  keeps focus on the same bookmark. With a tile focused,
  `Tab` reaches the pencil and the X (the frame that moves focus off the tile
  keeps the hover-only buttons allocated: tile → pencil → X → next tile).
- The URL index forgets a URL only when NO entry carries it any more (`toggle`
  and `remove_entry` share the rule), so a hand-edited body with duplicate URLs
  cannot leave a visible tile reported as "not bookmarked".
- There is no retention: bookmarks persist until removed and survive restarts.
  The store is capped at `MAX_BOOKMARK_STORE_ENTRIES`, and an add at the cap is
  REFUSED with a toast instead of dropping the oldest. The body carries its own
  `MAX_BOOKMARK_STORE_BYTES` budget (`entry_bytes` is exact arithmetic, no
  throwaway buffer; an add OR a tile rename that would cross it is refused the
  same way, and an over-budget body from disk is trimmed oldest-first on load —
  derived index/byte/`generation` state is rebuilt only when `trim` dropped
  something).
- The debounced save hands the WRITER a snapshot (`WriteJob::build`) and the
  worker serializes + encrypts it, so the 48 MiB cap is never built inside a
  frame; the synchronous test/exit path keeps the inline build. A first run (no
  store file) starts with a curated default set (which accounts for its bytes
  like any other add); an existing store is never reseeded. A failed save
  surfaces as a note on Home.

## Preferences and session

- Preferences (`settings/`) live encrypted in `settings.bin` (`theme`, `zoom`,
  `restore_tabs`, magic `MDST`) alongside the history `retention` key, written
  key-by-key (decrypt + read-modify-write + encrypt + the same atomic rename),
  so history and the Preferences window never clobber each other. A missing or
  corrupt value falls back to its default.
- The read-modify-write uses a per-directory plaintext CACHE: a read body serves
  every later read AND write (four keys decrypt once; startup validation seeds
  it, so a valid `settings.bin` is decrypted once per launch). The shared
  `store.key` is loaded and validated once per process, so no preference or
  session write re-reads it.
- The write + `fsync` go to a dedicated background worker
  (`write_setting_async`), so `Prefs::set_*`, the retention switch and the
  per-change session save never stall a frame. `App::on_exit` drains the
  settings and session workers (one shared deadline, so the documented bounded
  wait is not doubled); only a `restore_tabs` toggle waits briefly (bounded) so
  the preference is durable before the session file is deleted.
- All four stores share the single `store.key`. Legacy `settings.conf` /
  `session.conf` and `history.key` / `bookmarks.key` are deleted on load, never
  read.
- A read failure is never an empty file: an
  unreadable or undecryptable `settings.bin` makes the WRITE REFUSE
  (`settings_body_with`) and is surfaced in Preferences instead of "reverting"
  next launch. An unreadable `settings.bin` is never treated as "restore tabs
  off", so it cannot delete the stored session.
- A failed background write (full disk, revoked permissions) surfaces the same
  way: `Prefs::tick` drains EACH worker's recorded outcome per frame (settings
  in `settings_error`, session in `session_save_error`), so a successful
  per-tab session save cannot clear a settings-write failure, and a session
  snapshot that failed ON THE WORKER cannot pass silently.
- A snapshot whose save could not be enqueued is not marked saved: the
  bookkeeping is invalidated, it stays pending and retries on a
  `SESSION_SAVE_RETRY_SECS` backoff (bounded, so a machine with no data
  directory does not allocate an error string per frame; the app schedules the
  repaint for the retry window).
- A job that could not be QUEUED at all is a failure too (the spawn-refusal
  path returns it), reported against ITS file: a session enqueue error lands in
  `session_save_error` (not the settings card) so `App` invalidates the snapshot
  and retries, and a settings enqueue error cannot clear a session warning or
  vice versa. A worker that a bounded `flush` retires (the "restore tabs off"
  path flushes mid-session, not only at exit) parks its last outcome first, so a
  failed write is still reported.
- A settings body that would cross `MAX_SETTINGS_BYTES` is REFUSED
  (`settings_body_with`), so a change never writes a file the reader would
  reject next launch (the reader's caps derive from `crypto::OVERHEAD_BYTES`).
- Theme maps to `egui::ThemePreference` (System/Light/Dark) and is applied
  idempotently; the startup zoom is clamped to `config::ZOOM_*` and applied to
  the live view when changed.
- "Restore tabs on startup" remembers one entry per tab (a URL, or a `Home`
  marker so tab count and active index survive) in encrypted `session.bin`
  (magic `MDSS`), written only while enabled and deleted when turned off through
  the SESSION WORKER as a discard job (a queued/in-flight snapshot cannot be
  renamed into place after it), then verified by a direct sweep (a failed delete
  is retried at every start and whenever the toggle is set off again; orphaned
  `.tmp.*`/`.bak` siblings removed too). `App::new` reopens them (navigating
  `about:history`/HTTPS pages) before the first frame. A tab whose load is in
  flight or failed reports the last real navigation target (`Tab::session_url` /
  `session_target`), so a slow or failed restore keeps the requested URL instead
  of a `Home` marker. OS/CLI targets WIN over a stored session for the current
  launch only: the file is not rewritten until the open tabs change, so
  double-clicking a document never destroys the remembered session.

## Rendering pipeline

### Parsing and copy text

- `pulldown-cmark` events are parsed ONCE at fetch time into owned events
  (`Fetched.events`); copy text (`Fetched.plain_text`) derives FROM THOSE EVENTS
  (`plain_text_from_events`), so the clipboard never pays a second parser pass
  (a corpus test pins it byte-identical to the body-based oracle).
- The page-title fallback (`Fetched.h1_title`, the document's first `#` heading)
  comes from the same events, read through `Fetched::page_title` (frontmatter
  `title` first), so a document without YAML still names its tab, window,
  page-info card and default bookmark/history entry.
- `:shortcode:` emoji expand for text nodes (never inside code) in the rendered
  events AND in `plain_text`, so the clipboard matches the viewer.
- The theme sets `override_text_color = on_surface` for plain labels; the muted
  tone is opt-in (`theme::weak`, plain quote bodies via the walk's
  `override_text_color` scope, H6). Bold text uses a named family whose first
  face is the bundled Noto Sans variable font pinned to `wght = 700`
  (`theme::strong`; epaint's `FontId` has no weight axis).

### Autolinks

- Bare URLs (`www.example.com`, `https://example.com`), bare domains
  (`celticos.com`, `agentready.md`) and local `file:` URLs are linkified in one
  pass (`markdown::autolink`, GFM rules plus bare-domain/`file:` sugar). A text
  node becomes `Link` only after a delimiter boundary (a run glued to a link's
  closing `)` or a code span does not qualify), needs a valid domain, and
  excludes trailing punctuation / unmatched `)` / entity-like `&x;`. A domain
  glued to `@` stays text in every candidate form (email spelling), so a URL
  typed as plain text renders, hit-tests and opens like a Markdown link (Copy
  pastes the URL text the document wrote).
- Only `https:` (typed, or inserted for `www.`/bare domains) and local `file:`
  URLs linkify. `http:`, `mailto:`, `ftp:`, `data:` and every other scheme stay
  text; emails never link.
- A bare domain must end in a real IANA root-zone TLD (`markdown::tld`, a
  generated allowlist), so `Cargo.toml`, `notes.txt`, `foo.local` and `v1.2`
  stay text. TLDs that double as common file extensions (`md`, `rs`, `sh`, `py`,
  `pl`, `ai`, `zip`, ...) also require the label before the TLD not to be a
  known file name (`README.md`, `config.rs`, `blog.readme.md`, `logo.ai` stay
  text; `agentready.md`, `mkplandeki.pl` link). Code spans/blocks, metadata
  blocks, existing links and image alt text are never touched.
- Bounded by `MAX_AUTOLINKS_PER_DOC` / `AUTOLINK_MAX_DOMAIN_CHARS` and a linear
  scan. Every generated destination must parse as a real `https:` URL with a
  host and NO credentials: the parser reads `example.com.` in
  `https://example.com.@evil.com` as userinfo, so a trailing dot before the `@`
  is rejected too.

### Rendering caps and caches

- The viewer walks events recursively (list items, quotes and footnotes nest
  correctly) under a DEPTH GUARD (`MAX_RENDER_DEPTH`): hostile nesting
  (`> > > …`, `- - - …`) renders flattened instead of overflowing the UI
  thread's stack.
- The event list is capped (`MAX_EVENTS_PER_DOC`): an over-complex document
  fails the route with a bounded line instead of a several-hundred-MB vector.
  Rendered blocks are capped (`MAX_RENDERED_BLOCKS`): past the budget the walk
  consumes blocks without rendering them, so ~1M tiny paragraphs cannot lay out
  every block each frame (Copy keeps the full text).
- Code blocks share a per-document line budget (`MAX_RENDERED_CODE_LINES`), so a
  few MB of newlines cannot render millions of labels per frame. The "hidden
  lines" notice is ADVANCED past the reserved body (rows are painted, not laid
  out), so it cannot cover the first visible line.
- Every rendered prose/HTML/table block is capped (`MAX_RENDERED_BLOCK_CHARS`):
  one multi-MB paragraph, heading or cell cannot become a single giant label (a
  bounded `… [truncated]` notice ends it; Copy keeps the full body). An HTML
  block's buffer is collected under the SAME cap (one char past it, so the
  notice still fires), not up to 4x it in bytes.
- Inline fragments borrow text from the events (`Frag<'a>` with `Cow`), so
  repaints do not copy the document.
- Code bodies and token spans are cached per (document salt, the block's
  `Start(CodeBlock)` EVENT index, language) in a bounded LRU cache with a byte
  budget. The per-document-stable event index stops a `<Tabs>` switch or
  accordion from handing a block a sibling's body; the SAME rule keys a
  blockquote's inner `ui.indent` (`quote_indent_id`: document salt + nesting
  level + the block's own `Start(BlockQuote)` event index, never a per-pass
  counter), so two quotes never alias onto one set of nested widget ids. Token
  spans fill LAZILY: a block rendered while the line budget was spent is cached
  body-only and highlights on a later frame.
- The salt mixes a per-tab `doc_generation` bumped when the visible document is
  replaced, so a reload cannot render or copy the previous body and resolving
  one tab keeps the others' cached blocks. The LRU eviction pass is shared with
  the table-width cache (`viewer::cache::evict_oldest`).
- Each block caches its laid-out line galleys (keyed by theme/selection/font
  size), so a repaint reuses an `Arc` per line instead of rebuilding a
  `LayoutJob`; a growing line window (a `<Tabs>` switch or accordion release)
  EXTENDS the prefix. The body is VIRTUALIZED vertically
  (`viewer::code::code_viewport`): only visible rows are built and painted, on a
  uniform measured row grid with the full height reserved, so widget count per
  frame does not scale with the line budget (plain blocks included). Highlighted
  rows are painted galleys (no per-line drag selection); Copy copies the block.
- Both paths keep the horizontal scroll range MONOTONIC: the highlighted path
  measures every laid-out line, the plain path remembers its widest painted row
  (`CachedBlock::plain_max_w`), so a vertical scroll past a long line cannot
  shrink the range and clamp the offset left. The viewport math uses the
  MEASURED row stride (epaint rounds to whole device pixels), not the design
  token, and bounds the visible rect with the WINDOW-LOCAL viewport
  (`InputState::viewport_rect`), never `ViewportInfo::inner_rect` (monitor
  space). Rows sit at their ABSOLUTE offset from the content origin (which
  carries the scroll translation), so a scrolled plain block cannot blank.

### Destinations and anchors

- Link/image destinations resolve ONCE per fetch
  (`protocol::rewrite::DestCache`, from the same events + anchors): per-frame
  fragment trees only hash-look-up, never `Url::parse`. One map serves links and
  derived images under a `MAX_DEST_CACHE_BYTES` budget, so a hostile link farm
  cannot retain unbounded strings past every other cap. Sources the base may not
  follow (e.g. `file:` from a remote page) stay inert alt text.
- A `host:port` spelling the omnibox accepts (`localhost:3000`,
  `example.com:8080/x`, IP literals such as `127.0.0.1:8080/x`, `[::1]:8080/x`)
  also resolves as `https://`; blocked schemes, userinfo forms and an ALLOWED
  scheme whose spelling did not parse (`https://`, `file://`) stay inert, never
  a bogus relative join under the base. Raw HTML is never interpreted.
- In-document anchors: `markdown::anchors` collects heading slugs (lowercase,
  punctuation dropped, spaces to hyphens), explicit `{#id}` heading ids and
  `fn-<label>` footnote names once per fetch (`MAX_ANCHORS_PER_DOC`). A heading
  id is looked up BY THE EVENT INDEX of its `Start`
  (`AnchorIndex::heading_id_at`), so skipped content (an unselected `<Tab>`
  panel, a closed accordion, the block budget) cannot shift a later heading onto
  a sibling's target.
- `#id` links resolve against that set — percent-encoded and mixed-case
  fragments included (`#caf%C3%A9`, `#Section`) — and scroll via
  `ui.scroll_to_rect`; unknown targets render as text. A cross-page `#fragment`
  rides `Tab::pending_anchor`, applied after the load resolves. A target that
  EXISTS but is not rendered this pass (an unselected `<Tab>` panel, a closed
  accordion, past the block budget) stays pending instead of being dropped, so
  opening that tab later still applies it (an unknown target is cleared).
- Footnotes are numbered by first reference (`AnchorIndex::footnote_number`):
  every REFERENCED label in first-reference order. The reference paints that
  number as a raised small marker
  (`viewer::inline::render_superscript_ref`, linked to the reserved
  `fn-<label>` target, its rect kept in egui memory); the definition paints
  `{number}. ` as a numbered entry, the marker sharing the first text line with
  the note body and continuation blocks hanging under the text column
  (`ItemState` markers with `note` style; a first block that cannot flow inline
  keeps the marker on its own row), plus a vector back-arrow scrolling to that
  rect.
- The back-arrow is the note's own last inline box (the next inline flow
  consumes `RenderCtx::pending_back_arrow`), so it ends the note's text on the
  same line and wraps like text; a note laying out no inline flow (or an empty
  one) gets the button on a trailing right-aligned row. ONE 1px rule precedes
  the first definition only (`RenderCtx::footnotes_started`); an unreferenced
  definition still renders, continuing the numbering
  (`RenderCtx::unreferenced_footnotes`). `plain_text` keeps the source
  `[^label]` spelling, like autolink copy keeps the URL the document wrote.

### Components and markup

- `:::`/`!!!` admonitions normalize to alert blockquotes in
  `document::normalize_body` (outside code fences; unknown types stay literal),
  markers stripped from `plain_text`.
- Structural `:::` containers (`:::tabs`/`:::tab`, `:::code-group`,
  `:::steps`/`:::step`, `:::card`, `:::accordion`/`:::details`; column-0 only,
  nesting-capped) become the same canonical markers as their JSX equivalents, so
  the renderer cannot tell them apart.
- The same pass recognizes MDX/HTML markup (`normalize_markup`): callout
  components become alert blockquotes, every other tag is a transparent wrapper,
  ESM/`{…}` are removed in an MDX document, `<script>`/`<style>` are dropped,
  `<pre>`/`<textarea>` stay inert and comments are invisible; nothing is
  executed or evaluated.
- Structural components (`<Tabs>`, `<Accordion>`, `<Card>`, `<Steps>`) reach the
  renderer as canonical `<mdowser-cmp data="…" />` markers (one HTML block each,
  blank-line separated so the content between parses as Markdown). A
  `<Tabs items={["A", "B"]}>` array labels its label-less `<Tab>` children
  (explicit `label`/`title` wins); `<CodeGroup>` desugars to `<Tabs>`/`<Tab>` in
  `normalize_markup` before parsing; inline `<Badge>` becomes a
  `<mdowser-badge>` pill.
- `viewer::markdown::components` draws them, with tab AND accordion state in
  egui's temp store keyed by document salt + the marker's event index +
  labels/title (identical label sets or same-titled/untitled accordions never
  share a selection or open state). A component label (tab cell, accordion
  header, step title) is capped at `MAX_COMPONENT_LABEL_CHARS`, full text in the
  hover tooltip, so one long label cannot push the strip outside the reading
  sheet. While an anchor request targets a heading inside a container, the
  tab strip selects the panel that holds it and an accordion opens before its
  content is walked, so outline rows and cross-page fragments reveal hidden
  headings. A `<Badge>` inside a heading draws the same pill, scaled with the
  heading (capped).
- Every display cap that elides text — suggestion/history/bookmark titles and
  URLs, footnote labels, window titles, block text, component labels — goes
  through `util::truncate_chars`, which never cuts inside a grapheme cluster (a
  combining mark, ZWJ family or flag stays whole); `plain_text` skips markers.
- Conservative product heuristics (deliberate; thresholds in `config/`): a
  leaked first-block fragment skip link is dropped (a plain paragraph is not);
  long link-only paragraphs stack one link per row (card entries with a logo
  included); one-line pipe/dot menus render as a centered nav row (measured, not
  `vertical_centered`: a wrapping child spans the full width); both menu
  separator families keep a prose guard.
- `normalize_body` repairs a link an exporter split with a blank line (`[label`
  blank line `body](url)`) into one link, and rewrites the docs-generator
  `[#id]` heading marker (`## Title [#custom-id]`) to `{#custom-id}`, so the
  marker never renders and the explicit id is the anchor. README
  headings/rules/quotes/tables come from `ui::theme` tokens.

### Tables

- `viewer::table` measures every cell line and caches the widths (keyed by
  content hash + column caps + selection + the image/emoji geometry epochs, so a
  resolution transition re-measures once and settled frames hit, images and
  color emoji included). The content fingerprint is memoized per (doc salt,
  table EVENT index), so settled frames do not re-hash the table text, and a
  `<Tabs>` switch cannot alias two tables.
- How many CELLS one frame may collect is capped
  (`config::MAX_RENDERED_TABLE_CELLS`): one row spends one block but can hold up
  to `MAX_TABLE_COLS` columns, so a 5 000-row × 4 096-column table would
  otherwise lay out ~20M cells while the row budget saw only 5 000 units. Past
  the cap remaining cells are consumed without fragments (no layout, no
  widgets), and the table ends with the usual truncation notice (Copy keeps the
  text).
- Cells render at their column's real width (the Grid reports `max_col_width` as
  the available width of non-last columns; aligning against it inflates
  right/center columns). `distribute_columns` grows measured widths to fill the
  card (no blank strip) or shrinks them with a `min_col` floor so wide cells
  wrap instead of forcing a horizontal scrollbar. When even the floors do not
  fit,
  the table scrolls with every column at its measured width up to a wider
  `scroll_max` cap. The pass paints collapsed 1px interior separators; the card
  frame stays the outer border.
- A fitting layout never sums past the usable width, not even by a rounding ulp
  (the residual comes off the widest column, never below `min_col`), so the
  overflow test cannot mistake an f32 artifact for a real "even the floors do
  not fit" (a two-column table grown to fill 757 summed to `757.00006` and
  collapsed its second column to `scroll_max` while the card stayed full width).
- A table that still scrolls — and every horizontally scrolling code card —
  reserves the floating bar's lane under the content (`theme::scrollbar_lane`
  via `ScrollArea::content_margin`), so the expanded handle passes below the
  last row/line. The code card keeps its uniform 16px outer bottom (4px padding
  + 12px lane); a fitting table keeps its grid flush with the card border.

### Fonts

- `ui::theme::install_fonts` and `media::decode::svg::build_svg_options` build
  the embedded Noto stack: the bundled Noto Sans faces lead every UI family.
  Noto Sans serves proportional text (chrome, menus, documents), its
  true-monospace cut (TTC face 5, Noto Sans Mono CJK JP) serves code, and Noto
  Sans CJK face 0 is the proportional CJK fallback. The complete monochrome
  emoji face and every face in `assets::SCRIPT_FONTS` (Arabic, Hebrew, Indic,
  Southeast Asian, ...) follow as glyph fallbacks. epaint's bundled defaults are
  never registered.
- In the SVG database every generic family — `serif`/`sans-serif`/`cursive`/
  `fantasy` and its no-family default — maps to Noto Sans, `monospace` to the
  Noto Sans Mono CJK cut, so SVG text can never pull a system font. A named
  `theme::strong_family` family pins the SAME Noto Sans variable asset to
  `wght = 700` (`FontTweak::coords`) as its first face for bold (CJK and emoji
  fall through to the regular faces: no variable CJK face is bundled).
- The two largest faces ship BROTLI-COMPRESSED (`build_support/gen_fonts.rs`
  compresses into `OUT_DIR`; `assets::noto_sans_cjk()` /
  `assets::noto_color_emoji()` decompress once into one leaked process-lifetime
  buffer each): CJK synchronously at startup, color emoji on a background
  PREWARM thread (`media::emoji::prewarm`, spawned by `App::new`), so neither
  the first emoji nor a chrome icon parses the ~24 MB face mid-frame.
- Exactly one copy exists in memory: epaint borrows the slice through
  `FontData`, the SVG fontdb wraps the SAME slice in an `Arc`, `media::emoji`
  borrows it; the build round-trip validates every blob before the crate
  compiles.
- Noto Color Emoji is deliberately NOT in those chains. `media::emoji` shapes
  clusters with harfrust, extracts the `glyph<id>` element and its `#id`
  references from the font's SVG table (read-fonts/roxmltree) and rasterizes it
  with the hardened resvg pipeline into a texture, cached per (cluster,
  device-px size) under `config::EMOJI_CACHE_*` and the process-wide texture
  budget; the viewer buckets its text sizes. A miss rasterizes on a background
  worker (`MAX_CONCURRENT_EMOJI` process-wide: over the cap the miss stays
  unregistered and a later frame retries), the monochrome fallback holds the
  line, and `EmojiCache::poll` uploads the texture (up to
  `MAX_IMAGE_BUDGET_RETRIES` refusals, then a permanent fallback). The
  bookmark-tile glyph cache (`ui::icons`) shares the pipeline, polled from
  `App::logic`. The SVG database shares static slices through `Arc` instead of
  copying the ~19 MB CJK collection per decode thread.
- epaint rasterizes with fontations (`skrifa`) and shapes with `harfrust`.
  Since epaint applies no bidi, `viewer::bidi` resolves the paragraph base
  direction, visual run order, RTL word order and mirrored punctuation before
  layout, and `inline_flow` lays RTL lines out from the right. Whole-document
  copy stays logical (a native drag selection copies the displayed string);
  content split across inline elements is reordered per fragment.

## Chrome and widgets

- Layout is the toolbar (top, full width), then the collapsible tab rail (44px;
  it expands on hover as a floating overlay with a short animated width tween
  and fading titles, no content reflow; it collapses only when the pointer moves
  into the document area, staying open over the toolbar and outside the window;
  the overlay merges with the toolbar and only its right edge is elevated), then
  the central content panel.
- The rail reads each tab's title, favicon origin and OS window title from the
  per-tab display memo (`Tab::display`, keyed by `(kind, doc_generation)`): a
  settled frame allocates nothing per tab and builds no `Vec` of cloned
  origins, since the favicon loop borrows the memo and `FaviconCache` looks up
  with a borrowed key slice. `show_tabs` takes `&mut [Tab]` while mutating
  nothing but the memo.
- Chrome icons are monochrome vector strokes (`ui::icons`), each hand-drawn on
  one 24x24 grid and tinted with the active theme. Buttons ease the glyph from
  muted to accent while it grows ~10% on hover (pulling back on press); focus
  draws `theme::focus_ring`.
- Flat filled buttons share one body (`theme::widgets::sized_button`, labels
  centered with `add_sized` whatever the parent layout, where `Button::min_size`
  alone left them on the parent's cross axis):
  `theme::tonal_button_sized` (neutral `secondary_container`) everywhere, plus
  `theme::primary_button_sized`, the single accent-filled white-label action
  (the error view's "open in your browser" escape hatch), whose dark-mode fill
  is darker so the white label keeps WCAG AA on every pointer state.
- Corners share the `theme::radius` scale
  (`SMALL/HAIRLINE/CARD/MENU/TILE/SHEET/FULL` + `theme::pill_radius`; rows use
  `row::ROW_RADIUS = radius::MENU`, switch/zoom tracks use `pill_radius`). Type
  shares `theme::typography`
  (`UI_COMPACT/UI_SMALL/EYEBROW/UI_LINK/BUTTON/DROPDOWN/ROW/HEADING/DIALOG_TITLE/PAGE_TITLE/DISPLAY`),
  strokes share `theme::stroke` (`HAIRLINE/ICON/EMPHASIS`), gaps share
  `theme::spacing` (`XXS/XS/S/M/L/XL/PAGE_END`), and every chrome icon button
  shares `theme::ICON_BOX` (26px: tab close, omnibox clear, history clear, zoom
  buttons, image placeholders, accordion headers). Full-bleed separators use
  `theme::hairline`; the floating cards share `theme::dialog_shadow`.
- Tile avatar hues live in `theme::AVATAR_PALETTE` (documented fixed-hue
  exception: identity colors, not chrome surfaces). The find washes live in the
  `Scheme` (`find_match`/`find_current`, pale/strong amber per theme), so
  `find_match_bg`/`find_current_bg` borrow the interned palette.
- Hand-painted rows paint the focus ring IN ADDITION to the hover wash (never
  `else if`); every truncated title/URL tooltips its full text on hover
  (`ui::hover_tooltip_lazy`, allocation-free off-hover), and accesskit gets the
  same string via `widget_info`.
- Suggestion/history row caps (`config::ROW_TITLE_CHARS` / `ROW_URL_CHARS`), the
  page-info card's address cap (`config::SITE_INFO_ADDRESS_CHARS`), delete
  confirmations (`config::DELETE_CONFIRM_SECS`) and chrome heights
  (`config::CHROME_ROW_H` / `CHROME_DROPDOWN_ROW_H` / `CHROME_ZOOM_H`) are
  single-sourced in `config`. Display helpers
  (`util::strip_scheme_prefix`, `util::truncate_chars`) return `Cow` (borrowed
  on the fit path; the scheme check is case-insensitive without a whole-URL
  lowercase allocation).
- The toolbar is the navigation cluster plus the shared `ui::omnibox` capsule
  (padlock indicator with hover/focus feedback; a click-to-open page-info card
  with the facts of the rendered page — `Tab::site_info` in
  `browser::tab::info`, read-only and bounded — address autocomplete, Enter
  submit, Esc revert, select-all on focus, loading sweep, and ONE right slot
  shared by the clear X while editing and the bookmark ribbon otherwise: filled
  when the shown page is bookmarked, outline when not, disabled when nothing is
  loaded) and a responsive zoom pill; there is no `Go`. Home opens the NEW TAB
  page; the page left behind moves to `back`. While loading, the reload slot
  becomes Stop (`Tab::stop` restores the backdrop page and its images). Every
  top-bar button owns a fixed slot, so a swap (Reload/Stop, ribbon states) never
  shifts its neighbours.
- Favicons are probed per origin (`/favicon.ico` → `/favicon.png` →
  `/favicon.svg`, each bounded by `FAVICON_REQUEST_TIMEOUT_SECS`), decoded into
  session textures and persisted in `favicons.bin`, so a relaunch skips the
  probes. The cache gates `poll`/`has_pending` in O(1) from loading/backoff/
  failed counters; the budget-backoff walk runs only past the earliest deadline;
  a probe deferred by the image-slot limiter records a short backoff instead of
  re-parsing its URL every frame.
- The page sheet is flush with ALL FOUR window edges (the rail included; only
  its rail-side corner stays rounded). The responsive gutter and the
  always-visible scrollbar travel INSIDE the scroll content
  (`theme::content_gutter` + `ScrollArea::content_margin`), so the bar hugs the
  sheet's right edge and a scrolled document clips at the flush edges. Every
  scrolling page ends with `theme::spacing::PAGE_END` (24px) under its last
  element (documents, Home, the error view).

## Omnibox autocomplete

- `suggest.rs` + `ui::omnibox`: while either address field is editing, a popup
  lists up to `config::MAX_OMNIBOX_SUGGESTIONS` suggestions ranked
  bookmarks → history → builtin (`builtin_sites::builtin_suggestions()`, the
  curated `(url, title)` list vendored from MDowser-Sites via `make sync-sites`
  into `assets/builtin_sites.txt` and embedded at compile time), deduplicated by
  exact normalized URL and matched case-insensitively on URL + title (schemeless
  queries included; ASCII candidates compare in place, without a lowercase
  `String` per candidate per keystroke). An empty field offers recent bookmarks.
- No row is highlighted until picked: the first `Down`/`Up` selects the
  first/last row, `Tab` completes the highlighted row without navigating (the
  first one when nothing is highlighted), and `Enter` submits the typed address
  (only an explicit `Down`/`Up` pick or a pointer that MOVED onto a row routes
  it to the selection through `Tab::navigate_from_address`; a stationary pointer
  over the popup is not a pick, and typing clears the pick so a URL edit can
  never be silently replaced by the page already shown). `Esc` closes the popup
  before reverting the field.
- A clean click on the field selects the whole address; a drag keeps the range
  built from the press point. The popup is a foreground `Area`: it never shifts
  the capsule and its row clicks survive the frame egui surrenders the
  TextEdit's focus on. Its rows live in a vertical `ScrollArea` capped by the
  window content rect (scrollbar only when needed), and the selected row calls
  `scroll_to_me`, so keyboard navigation stays visible at the minimum window
  height.
- The list is cached per field in egui memory, rebuilt only while the field is
  EDITING and only when the query or the
  `HistoryStore`/`BookmarkStore` `generation()` changes, so the 20 000-entry
  scan runs on keystrokes, never per frame, and never twice per keystroke on
  Home. A closed tab drops its cache (`ui::forget_tab`, the single
  registry for every per-tab egui-memory key: omnibox caches + field edit
  states, history filter/cache/clear + search-field state, the
  Home/bookmarks/history-page scroll offsets, the document reading sheet's
  per-(tab, URL) offsets, the error-view scroll offset, and the Home
  rename/drag drafts).
- Scroll offsets are registered through `ui::remember_scroll` with the REAL id
  their `ScrollArea` returned (one surface can record several ids per tab — the
  document sheet keeps one per visited URL — and the close path removes them
  all). A `ScrollArea` derives its persistent id from the parent `Ui`
  (`make_persistent_id`), which the removal uses.

## Omnibox capsule state

The right slot is reserved from
`bookmark.is_some() || focused || !address.is_empty()`, and the fill/ring is
painted after the field with the FRESH focus, so a pointer press shows its ring
in the same frame and neither editing nor focus can shift the text area's right
edge.

## Application identity

- Every raster icon derives from `assets/icon.svg` through `build.rs`
  (`build_support/gen_icons.rs`, the integrated gen-icons tool; it runs
  automatically when the committed outputs are missing or outdated, and
  `make icons` forces it). `main` decodes `assets/icons/icon-256.png`
  (`assets::window_icon`) into `ViewportBuilder::with_icon` for the
  Windows/Linux window, taskbar and Alt-Tab.
- Windows executables also embed three PE resources (`build.rs`, `windres` for
  GNU, `rc.exe` for MSVC, best-effort: a missing compiler warns and builds
  without them): `assets/icons/icon.ico` as the shell icon, a `VERSIONINFO`
  block (product/file description, version, copyright) and the application
  manifest (`RT_MANIFEST` id 1:
  per-monitor DPI awareness, supported Windows versions, UTF-8 code page, long
  paths, segment heap). `main` also sets `windows_subsystem = "windows"` for
  that target, so no console opens behind the GUI.
- macOS reads `Contents/Resources/icon.icns` from the `.app`. Linux uses the
  hicolor ladder (`.deb`, `.rpm`, Flatpak, `make install`), a shared `.desktop`
  with `Icon=`/`StartupWMClass=` and AppStream metadata (`packaging/linux/`,
  staged by `scripts/packaging.sh` `install_linux_tree`: pre-flights every
  source, renders metainfo fail-closed via `render_metainfo`).

## Shortcuts

`app::handle_shortcuts` uses `consume_key`, so typing never triggers a shortcut:
Ctrl/Cmd+T/W/R/L/A, Ctrl/Cmd+1..9, F5, Esc (stop loading when no
field/menu/find-bar owns the key; a focused omnibox reverts its text instead),
Ctrl/Cmd+D (toggle the loaded page's bookmark, with a toast), Ctrl/Cmd+,
(Preferences), the platform History key (Ctrl+H/Cmd+Y), zoom
`+`/`=`/`-`/`0`, Ctrl/Cmd+F (in-page find on document pages, even while a field
is focused) with `F3`/`Shift+F3` stepping through matches, Ctrl/Cmd+S (save
the page as Markdown in the downloads folder, likewise a document action that
never types an `s` into a field), and Ctrl/Cmd+A selecting the whole page with
Ctrl/Cmd+C copying it while that highlight is active (egui only emits
`Event::Copy`; no key event to consume). The Esc cascade gives the first press
to a focused field/editor, then a popup/menu, a floating window and the find
bar, and only then Stop. The full table
is [`INTERFACE.md#keyboard-shortcuts`](INTERFACE.md#keyboard-shortcuts).

## Right-click menu

- `viewer/overlays`: minimal Copy / Select all / Find in page / Save page,
  plus "Open link in new tab" when the click hit a link (link hitboxes are
  collected by the renderer each frame). `Cmd/Ctrl+click` on a link is the
  direct route: the renderer's `on_link` callback hands the viewer a
  background-tab request instead of a same-tab navigation (an in-page
  `#anchor` becomes its absolute URL plus fragment). The rows that need `App`
  (Save page) or the viewer's own state flag what they want (`Tab::save_asked`)
  instead of reaching outside their layer.
- The rows carry explicit widget ids and are keyboard-navigable: opening with
  the pointer leaves NO row focused, `Up`/`Down` move focus into the rows (the
  first press lands on the first one, egui's arrow traversal cancelled so it
  cannot skip it), and `Enter` activates the focused one. A primary press
  outside the popup closes it (a press-drag selection never reaches
  `clicked`).
- The menu renders BEFORE the document so a Copy click can push `Event::Copy`
  into the same frame; whole-page copy clones the precomputed
  `Fetched.plain_text` (no UI-thread reparse).
- `MdowserApp::raw_input_hook` intercepts a secondary PRESS on the document
  while a native text selection exists (egui would otherwise collapse it as the
  start of a drag). The press is dropped and its release arrives as
  `Tab::pending_context_click`, which opens the menu; the selection (and its
  highlight) survives, so the menu's Copy copies exactly the selected text.

## macOS menu bar

- `app::menu` (macOS only; Windows and Linux have NO menu — the toolbar is the
  whole chrome). Startup installs a `muda`-built `NSMenu` over winit's bare
  default (About/Services/Hide/Quit only): an Application menu (About MDowser,
  Preferences… `Cmd+,`, Services, Hide/Hide Others/Show All, Quit — the first
  submenu, which AppKit titles with the app name), File (New Tab, Close Tab,
  Save Page), Edit (Copy, Paste, Select all — deliberately no more), View
  (Reload, Zoom In/Out/Actual Size), History (Show History) and Window
  (Minimize, Zoom, Bring All to Front).
- The bar owns no logic: each custom item replays the shortcut the keyboard
  would (`MenuAction::shortcut`: `Event::Copy`, a `Cmd+<key>` `Event::Key`, or a
  clipboard-built `Event::Paste`), queued by the muda handler (which calls
  `Context::request_repaint`, a menu activation not being a winit event),
  drained by `App::poll_menu` (re-scheduling the injecting pass) and injected
  into the raw input in `App::raw_input_hook`: a command raised while the window
  is minimized survives in eframe's pending input, and menu and real keys land
  in the SAME `handle_shortcuts` and focused `TextEdit`. An item whose key
  equivalent matches is consumed by AppKit, so the real key never reaches
  egui.
- Two AppKit defaults are switched off/overridden so the bar shows only
  MDowser's commands: startup opts the process out of automatic window tabbing
  (`app::menu::disable_automatic_tabbing`, called by `main` BEFORE any window
  exists), keeping the injected "Show Tab Bar"/"Show All Tabs" items out of
  View, and About MDowser carries the app's own icon (`AboutMetadata::icon`, the
  embedded 256px raster) instead of AppKit's generic placeholder folder.
- `muda`/`arboard`/`objc2`/`objc2-app-kit` are macOS-only dependencies
  (`arboard` only backs Paste; the objc2 crates only make the one safe AppKit
  call muda does not surface).

## Window geometry

- `app::window`: the FIRST frame sizes and positions the window once. `main`
  sets `NativeOptions::centered`, so eframe centers the window on the primary
  monitor while CREATING it (Windows otherwise opens it at the `CW_USEDEFAULT`
  cascade position). On the first frame the app restores the geometry remembered
  in the encrypted preferences, or (first run) opens at
  `config::initial_window_size` —
  `WINDOW_INITIAL_FRACTION` (80%) of the current monitor, clamped to
  `WINDOW_MIN_*`/`WINDOW_INITIAL_MAX_*` — centered on that monitor.
- The monitor list and window rects come from eframe's winit window
  (`Frame::winit_window`, inherent methods only: no direct `winit` dependency).
  A stored position that no longer overlaps any attached monitor (unplugged
  screen) falls back to centering, and the size is clamped to the monitor it
  opens on.
- Later moves/resizes are persisted (encrypted `settings.bin` key `window`,
  debounced by `WINDOW_GEOMETRY_SAVE_DEBOUNCE_SECS` and flushed on exit), so the
  next launch reopens where the user left it; while maximized only the flag is
  updated (the stored rect stays the last WINDOWED one, so un-maximizing
  restores it).
- The geometry is kept in PHYSICAL pixels, converted to native-scale points only
  for viewport commands: egui applies a `set_zoom_factor` only at the start of
  the NEXT pass, so the first frame's commands run at the default zoom, and
  physical pixels survive a DPI or zoom change without drifting (a window
  remembered on a monitor at a different scale reopens at the same physical
  size).

## Identifiers and repaint scheduling

- Address typing is tracked with `Tab::address_dirty` (set by the fields on
  `changed()`), so `poll()` never clobbers user edits.
- Tables/code-scroll/document-scroll ids are salted by `doc_id_salt(base_url)`
  MIXED WITH THE TAB ID (egui ids are global, so two tabs on one document would
  otherwise share scroll/table/code state), bounded. The document SCROLL salt
  excludes the per-load generation: the offset belongs to the (tab, URL) pair,
  so Back/Forward and refresh return to the reading position while a different
  document starts at the top. A fresh navigation to an already-seen document
  shares that salt, so it forces the offset to the top instead
  (`Tab::scroll_top_requested`, consumed by the first `Loaded` frame); failed or
  abandoned loads clear it so the restored page keeps its position.
- Jobs registered during `ui` (image `ensure`, favicon probes) schedule
  their own 120ms repaint tick, so a loading placeholder never waits for the
  next input event.

## Find in page

- The render walks the SAME events the viewer paints and assigns match ORDINALS
  in document order (`viewer::find::FindRender` `claim`, capped at
  `MAX_FIND_MATCHES`, per fragment as well as globally), so highlight, the bar's
  `n/m` count and Prev/Next share one source of truth.
- A searched fragment keeps the EXACT layout of the unsearched one:
  prose/headings lay out ONE UNSHAKEN `LayoutJob` (a single section, so the
  shaping run and every advance match the unsearched render) and the wash is
  painted as RECTANGLES behind the glyphs (`range_row_rects`, one band per
  galley row a match touches). Links and inline code stay single widgets; fenced
  blocks claim their cached body and wash matching rows without touching the
  galley cache. `render_plain_split` documents why: per-piece widgets re-wrap
  each piece, and per-match SECTIONS lose kerning across boundaries (~1px over
  a long line).
- A fragment holding color emoji never takes the glyph split (a galley cannot
  draw a texture): it keeps the emoji COLOR path with the wash behind the
  textures, highlighted whole. Fenced blocks use the stronger CURRENT wash on
  the row that owns the current ordinal too (found once per frame by counting
  occurrences line by line, early-exiting past the target).
- The CURRENT match's painted rect is recorded on the frame's cursor (the union
  of its row bands, `range_row_rects`) and its `scroll_to_rect` request is
  RETRIED until that rect shows up in the sheet's clip rect: egui's `ScrollArea`
  DROPS a scroll target on any frame that also sees scroll input. A match inside
  a code card or
  table cell sits under a NESTED horizontal `ScrollArea`, which consumes the
  frame's target and applies only its own axis: the retry re-issues the request
  from the outer sheet, whose `ScrollArea` ends after the walk, so the document
  scrolls there too. New wheel/drag input cancels the retry.
- The bar's own focus request is deferred past the first frame too: an `Area`'s
  first frame is an invisible sizing pass with a DISABLED `Ui`, where a
  `TextEdit` cannot take (and would surrender) focus. `Esc` closes the bar only
  when no other surface owns the press: `app::shortcuts` records a focused
  field, the page-info card, an open popup/menu or a floating window in
  `FindState::esc_elsewhere` before the viewer draws, so one press never closes
  the bar AND acts on the surface behind it.

## Document outline

- `markdown::outline::build` runs ONCE per fetch, next to the anchor index and
  over the same owned events: one item per heading (level, whitespace-collapsed
  title, the heading's RESERVED anchor id and its own `Start` event index),
  bounded by `MAX_OUTLINE_ITEMS` / `MAX_OUTLINE_TITLE_CHARS`.
  `anchors::build` reserves a positional `heading-N` id for a heading that
  slugs to nothing, so such an item is still navigable; an item's `at` keyed to
  the `Start` index means content the viewer SKIPS (a closed accordion, an
  unselected `<Tabs>` panel, the block budget) can never shift the scrollspy of
  a later heading. `Fetched.outline` carries the result to the sidebar.
- `viewer::outline` is the scrollspy state: the document walk records each
  rendered heading's screen rect by that event index, `current_section` merges
  the heading list and the recorded rects in ONE ordered pass (the last heading
  above the reading line: the viewport top plus a small pad, lowered through
  the page's last screenful). A heading with no rect (collapsed
  content, an unselected tab, the block budget) is never a candidate; when no
  heading has reached the line yet the first heading that DID render wins, not
  the first row of the outline. A frame that rendered no heading lights no row
  (the cursor is kept for when content renders). Across the page's last
  screenful the reading line descends from the viewport top to its bottom
  (`DocViewport::end_progress`, 0 at the start of that stretch and 1 at the
  maximum offset), so headings the sheet can never put on the top line are
  still walked in document order, the deepest visible section wins at the
  very bottom, and scrolling back up walks them in reverse. A dense cluster
  at the foot of the page is therefore still selected row by row. A clicked
  row is pinned until the sheet has SETTLED at the row's scroll target (its
  top at the viewport top, or the sheet at its maximum offset) and the spy
  reaches it, or the reader scrolls by hand. The settled check
  (`DocViewport::sheet_settled`: the offset has not moved since the previous
  walk and there is no kinetic velocity) keeps a page-end pin through the
  requested scroll, so the descending line crossing that row on the way to
  the bottom cannot hand the cursor to a deeper section; the target check
  keeps a row that is ALREADY current from being released on the click frame,
  before that scroll starts. The viewer checks for a wheel/scrollbar scroll
  BEFORE the document walk because the sheet's `ScrollArea` consumes and
  zeroes `smooth_scroll_delta`.
  `reveal_offset` computes the sidebar list offset from the measured variable
  row heights.
- `viewer::outline_panel` draws the permanent sidebar BEFORE the central
  panel, so it is layout, not an overlay: `Panel::right`, proportional width
  176-240px, full height, on every document page (a headingless page or the
  backdrop while the next one loads keeps the slot with a short empty/loading
  state, so navigation never reflows the sheet). Titles WRAP (the complete
  heading stays in the tooltip and the accessible name); row heights are
  measured once per panel width/document and cached in `OutlineState`, and only
  the visible rows are laid out on each frame. The current row carries the
  accent bar. A row click selects the row at once and sets `Tab::pending_anchor`,
  the same request a `#id` link click makes; the sheet's own walk consumes it
  and scrolls. A target inside a closed `<Accordion>` or an unselected
  `<Tabs>` panel first reveals that content (see
  `AnchorIndex::heading_event_at` and `viewer::markdown::components`), so the
  row can never leave the request pending on content that is not laid out.
  The sidebar publishes its screen rect so the find bar docks just left of it.
  It has no close state and owns no keyboard key.

## Save page

- The context menu's "Save page" row only flags the request (`Tab::save_asked`);
  `App::ui` reads and clears it right after the viewer draw and calls
  `save_page` — the same path as Ctrl/Cmd+S — so the viewer never reaches
  across its own layer for the document payload.
- `save_page` reads `Fetched.source` (or the `about:licenses` document;
  everywhere else a "Nothing to save" toast) and names the file through
  `app::save::default_stem`: the page title's slug, else the URL's last
  non-empty path segment with `.md`/`.markdown`/`.txt` stripped and
  percent-decoded, else `document`, all capped; a Windows device name (`aux`,
  `nul`, `com1`…) gets a `-file` suffix, so the default is a real file on every
  platform. The source is borrowed, not copied: the write happens in the same
  frame the shortcut or menu row was read, and nothing reads the tab again.
- `app::save` owns the write: destination is the OS downloads folder — the
  Downloads known folder on Windows (`known-folders`, a Windows-only
  dependency wrapping `SHGetKnownFolderPath`, so a location the user relocated
  in Explorer is honored), the XDG `XDG_DOWNLOAD_DIR` from `user-dirs.dirs` on
  Linux/BSD (localized names work) — when it exists, else `~/Downloads`, else
  home, else the current directory; the first free name wins (`notes.md`,
  `notes (1).md`, …), and the publish itself is no-replace (see below), so an
  existing file is NEVER overwritten even if one appears after the name
  check. A destination that refuses the write with a permission error (the
  macOS Downloads prompt denied, a read-only folder) falls back to the home
  directory, and the toast distinguishes it
  (`Saved <name> in home (Downloads needs permission)`); any
  other failure surfaces as `Could not save: …`. Success, fallback and failure
  all go through the shared toast, so no dialog ever blocks the save.
  On filesystems with hard links the write is atomic (unique `create_new`
  temp beside the destination, then a no-replace hard-link publish);
  filesystems without hard links fall back to an exclusive create. Either way
  a failed or interrupted save neither truncates a file nor replaces one that
  appeared after the name check; a replacement, used only by the explicit
  atomic-replacement path, keeps the existing file's permissions.
  The file name derives from the page and is slugified and capped BEFORE it
  becomes a path component, so a hostile title or URL segment cannot add a
  separator, leave the downloads directory or name a Windows device.

## Bounds and conventions

- Shared anti-abuse bounds live in `config/` (`MAX_SETTINGS_BYTES`, the
  code/table render-cache byte budgets, the font-asset gzip bound and
  `LOCAL_READ_CHUNK_BYTES`). The structural/MDX scan-line budgets live beside
  their passes in `markdown::document`, the store repair-lock timeouts in
  `store::crypto`, and the display helpers in `util.rs`.
- The `image::ImgCtx` parameter bundle (the five inputs shared by all four image
  entry points, built per call from the walk context) lives beside its owner in
  `viewer::image`.
- Window geometry constants
  (`WINDOW_INITIAL_*`/`WINDOW_INITIAL_FRACTION`/`WINDOW_INITIAL_MAX_*`/`WINDOW_MIN_*`
  and the save debounce) live in `config/`, not as magic numbers in
  `main`/`app::window`. Every wall-clock deadline goes through
  `config::deadline_after` (fail-closed `checked_add`: the unreachable overflow
  expires immediately instead of panicking the thread).

## Performance

- A reused global HTTP client (TLS/HTTP2 pool, no proxy/cookies; only state is
  the encrypted disk cache plus the visible tab). Transparent
  `gzip`/`brotli`/`deflate` decompression shrinks Markdown and error bodies; the
  anti-abuse caps run on the decompressed stream.
- One background thread per navigation polls a shared long-lived multi-thread
  `tokio` runtime (`NETWORK_WORKER_THREADS`, 4 workers), keeping the HTTP pool's
  executor and the UI unblocked. Early `Content-Length` cutoff plus capped
  chunked reads with a 15s per-chunk stall timeout.
- Images download and decode on background threads under
  `MAX_CONCURRENT_IMAGES` (separate from document fetches) and upload once as
  textures; local reads use `MAX_CONCURRENT_LOCAL_READS`, so neither queue waits
  behind the other.
- No continuous repaint: only while loading (60ms omnibox progress ticks,
  100/300ms fetch poll), while images/favicons are pending, during the 500ms
  cooldown ticks, or on interaction. Parsing to owned events happens once per
  fetch (copy text and the first-H1 fallback included); repaints never re-run
  the parser nor copy the text, and code blocks reuse a bounded per-document
  body/span cache.
- Per-frame allocation stays down: egui id salts are tuples (no `format!` per
  document/block/table/error-scroll/menu/toast); link hitboxes hold interned
  `Arc<str>` handles shared by every fragment; unordered bullets are borrowed
  (`Cow::Borrowed`, only the ordered prefix allocates); context-menu hints are
  `&'static str`; the history page rebuilds its lowercase needle only on a cache
  miss (the cache also holds date sections and flat offsets); error-view hints
  are borrowed `Cow`s; the code-block language chip skips the allocation when
  already lowercase; the quote-alert look-ahead and `detect_callout` read only
  the marker-length prefix of each run (not an up-to-512-KiB uppercase pass per
  quoted paragraph per frame); image-alt and flattened-subtree collectors stop
  at the per-block char cap; `cap_block_text` counts only up to the cap; each
  cached code block stores its line total once.
- The tab rail reads the per-tab display memo (`Tab::display`) instead of
  allocating a title and parsing the asked URL per tab per frame, and requests
  favicons from that borrowed memo (no `Vec` of cloned origins). Bookmark tiles
  resolve favicons by URL string (`FaviconCache::texture_for_url`) instead of a
  `Url::parse` per tile per frame: with the borrowed key slice
  (`String: Borrow<str>`) every lookup is one hash probe, and parsing happens
  only on the first frame of an unknown origin.
- The palette (`theme::scheme_ref`, borrowed per surface) is interned per theme
  rather than rebuilt per element; the OS window title comes from the same
  per-tab memo (`Tab::window_title_cached`, a borrowed `&str`).
- The omnibox suggestion cache rebuilds only while its field is editing (Home
  never runs the doubly-scheduled 20 000-entry scan per keystroke) and compares
  ASCII in place (`util::contains_ignore_ascii_case`) without a lowercase copy
  per candidate. Bookmark tiles and history rows borrow their titles (`Cow`);
  the tile avatar returns a stack-encoded `char`.
- The image cache keeps an incremental in-flight count
  (`ImageCache::loading_count`) instead of rescanning entries while the queue
  drains; the emoji LRU evicts in one pass per victim; preference/session writes
  queue their `fsync` on background workers (the UI thread never `fsync`s);
  tooltips/accesskit labels (icon buttons and their lazy `Reload` countdown, tab
  close buttons, color swatches, the Preferences data-directory path, both
  address fields, the history search field) are built only while hovered or
  announced (`ui::hover_tooltip_lazy` for `format!`-ed text).
- Painted labels are memoized: the find bar's `n/m` on
  `(current, total, capped)` in `FindState`; the toolbar zoom readout in a
  thread-local buffer keyed by the rounded percent; the history header/footer
  captions on `(total, retention)` /
  `(total, shown)` with a shared singular/plural rule Preferences reuses; the
  code Copy label for the accesskit node and hover card. Tab titles borrow with
  a gated tooltip, the code match wash consumes the visible-line iterator inline
  (no `Vec` per block per frame), `contains_match` keeps range walks
  allocation-free on the ASCII path, inline code skips its wrap-measure probe on
  fresh rows, and the component close-tag scan compares allocation-free
  (`is_close_tag`, also in `scan_pairs`).
- Settled tables cost NO byte scan per frame: the content fingerprint memo is
  keyed by the table's `Start` event index (stable per document) instead of
  probing the table text; the overflow/lane decision compares against the width
  the inner `ScrollArea` really sees.
- A code block's widest line is computed when its galleys are built; the plain
  path keeps a monotonic widest-row memory, so the horizontal range cannot
  shrink while a long line is off screen. Token spans are tokenized on first
  need; the error view's scheme-stripping helper borrows (`Cow`) when nothing is
  stripped.
- One parser pass per document: clipboard text derives from the owned events
  (`plain_text_from_events`); the structural `:::`/MDX passes run under
  `MAX_STRUCTURAL_SCAN_LINES` / `MAX_MARKUP_SCAN_LINES`, and `strip_line` never
  rescans for a closer it cannot find. Unmatched openers stay literal instead of
  costing O(n²) on the fetch thread.
- Settled O(1) paths: `ImageCache::poll` returns immediately when nothing is
  queued or loading; `settings` loads `store.key` once per process; `history`
  keeps its plaintext byte total incrementally.
- Slot accounting is leak-free: `try_acquire` increments once and the guard
  decrements on `Drop`, constructed ONLY on success (`bool::then`, never
  `then_some`), so a cancelled or timed-out wait cannot underflow the counters
  (an underflow made later loads time out and report "busy").
