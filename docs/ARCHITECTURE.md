# Architecture

Single binary (`src/main.rs` → `app::MdowserApp`), immediate-mode egui.

```
main → app (tabs + active index + zoom + favicons + encrypted history and
           bookmarks + shortcuts + the macOS-only native menu bar
           (`menu`, macOS only) + native window geometry (`window`:
           first-frame placement + persisted position/size/maximized) +
           OS/CLI open targets)
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
  ├─ markdown (frontmatter split+cap (`Cow` body: no copy without a block),
  │            parser options, `:shortcode:` emoji,
  │            anchors (heading/footnote targets), `:::`/`!!!` admonitions +
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
  │    ├─ fetch (background download; content-type gate + decode)
  │    ├─ emoji (color emoji: harfrust shaping + SVG-table extraction
  │    │         through resvg; bounded inline textures)
  │    ├─ decode (magic sniff + dispatch; `raster` = image/jxl-oxide/
  │    │         avif-decode, `svg` = usvg/resvg sanitize + rasterize)
  │    ├─ cache (per-document textures; url → Loading/Ready/Failed;
  │    │        one transient-failure retry)
  │    └─ favicon (session per-origin icons for the tab rail)
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
  │    │   find bar UI), bidi,
  │    │   cache, `find.rs` + `find/tests.rs` (in-page search state +
  │    │   case-insensitive matching) with `inline/tests_find.rs` and
  │    │   `tests_find.rs` (fragment highlight + layout/scroll contract)
  └─ ui (browser chrome: `theme/` document styling + widget kit, icons,
          tabs (`TabsResponse`, `rows`) with the live drag reorder, navbar,
          omnibox, home (with the
          Home-only encrypted bookmark list), bookmarks (grid + `tile`),
          preferences (tabbed Preferences window: General/Data/Shortcuts,
          incl. the licenses link; `widgets` kit), history_page (internal about:history
          page), error_view, `ui::forget_tab()` (single function registry
          for closed-tab egui-memory keys))
```

Key contracts:

- Network only via `protocol::client` on background threads
  (`Promise::spawn_thread`); the async work itself runs on ONE long-lived
  tokio runtime (`client::run_network`), so the shared HTTP connection
  pool's idle connections keep their executor between jobs (per-job
  `current_thread` runtimes made pooled subresource requests fail with
  `error sending request`). UI polls each frame.
  Documents go through `fetch_with_fallback`; image/favicon subresources
  through `media::fetch`, which reuses the same `get_bytes` (cancellable
  connect, redirect/https check, per-chunk stall timeout, running byte cap).
  The `Content-Type` gate runs BEFORE the body is streamed, on EVERY
  declared value in full (untruncated: a bounded copy could hide a
  `charset=` beyond the cut): a concrete
  non-accepted type is rejected without downloading or decompressing it.
- Local files go through `protocol::local` on the same job thread (no
  runtime): `parse_navigable` is the ONLY navigation parser (https plus
  `file://`/absolute/`~`/relative paths); a `file://host/` share is
  rejected (`file://localhost/…` is this machine) and only regular files
  are read (the open runs with `O_NONBLOCK` and the OPENED handle is
  re-checked, so a FIFO swapped in after the `metadata` check can neither
  block the thread nor pin a fetch slot). Candidates are the
  exact path, `.md` completion
  when the stem does not exist, and the conventional directory indexes;
  reads reuse `MAX_BODY_BYTES` / `MAX_IMAGE_BYTES` with a cooperative
  cancel check between 64 KiB chunks, and successful bodies go through the
  SAME `client::finish_document` as HTTPS ones (frontmatter, parsed events,
  copy text, first-H1 title, anchors, display URL), so the viewer cannot
  tell them apart.
  `resolver::is_same_logical` compares local files by exact path (never by
  the `/.../` collapsing rules), `display_string` shows the decoded path
  and `server_home_of` returns the containing directory. `rewrite` only
  lets `file:` destinations out of a base document that is itself local,
  and only lets `https:`/`file:` through; favicons are never probed for
  local pages. OS/CLI hand-off (`main` → `app::MdowserApp::new`) opens one
  tab per target and wins over session restore. The hand-off list itself is
  capped at `MAX_TABS` (`main::collect_initial_targets` skips flags/empties
  and takes the first `MAX_TABS` targets): an OS-controlled argument list can
  never allocate an unbounded tab vector, and per-target length is still
  enforced by `parse_navigable` (oversized input becomes that tab's local
  error, never a truncated navigation).
- Three separate slot limiters: `MAX_CONCURRENT_FETCHES` for network
  documents, `MAX_CONCURRENT_LOCAL_READS` for local-file reads (a slow or
  hung filesystem holds its own limiter instead of a network slot) and
  `MAX_CONCURRENT_IMAGES` for image/favicon jobs, so a gallery never queues a
  navigation; each document additionally holds at most
  `MAX_CONCURRENT_IMAGES_PER_DOC` jobs, so one gallery cannot starve other
  tabs or the favicon probes.
- Single-flight per tab: `cancel_inflight()` + `AtomicBool` checked per
  candidate/chunk, during connect (`futures_util::select` race) and a 15s
  per-chunk stall timeout (`CHUNK_TIMEOUT_SECS`).
- Transparent `gzip`/`brotli`/`deflate` decompression on the shared client;
  caps apply to decompressed bytes.
- Stale-while-revalidate: `Loading::backdrop` keeps the old doc on screen;
  its images stay textured until the new document resolves (no cross-page
  image cache: `Tab::images.cancel_all()` on resolve/failure/reset). A
  visual timeout with a backdrop restores the page (history snapshots and
  textures intact) and reports the timeout as a toast; only a first load
  without one settles into `Failed`. `Stop` and the visual timeout restore
  the stacks like a FAILED fetch (`settle_abandoned` takes `pushed_history`):
  the entry the abandoned navigation pushed stays reachable (the restored
  backdrop heals off its own duplicate), so the same sequence behaves the
  same whether the load failed or was stopped. After the fetch resolves, a
  `Settling` gate preloads the new document's images in a separate cache
  while the backdrop (or the stable empty panel on a first load) stays on
  screen with the loading indicator; the swap to `Loaded` is atomic once
  every image is settled or `DOC_IMAGE_GATE_SECS` (5s) passes, so no
  placeholder-to-bitmap reflow ever reaches the screen. Text-only pages
  skip the gate on the same poll.
- Images: accepted `Content-Type` (image types + legacy aliases, or
  `application/octet-stream`/`binary/octet-stream`/missing) + magic sniff +
  decoder validation. Caps: 16 MiB/image, 48 MP, 12288 px/side (re-checked
  on the decoded frame), 512 MiB decoder alloc, 384 MiB texture budget per
  document + 768 MiB process-wide across tabs AND favicons, 512 images/doc,
  3 concurrent jobs per document with 6 process-wide (see the slot
  bullet above), 60s total timeout per image (documents stay at 20s;
  stall cutoff stays 15s for both). Decoded pixels are downscaled to 3072 px
  (AVIF included: every decoder goes through the same `finish` downscale)
  and uploaded once; a raster decoder applies the EXIF orientation before
  layout, so phone photos are never sideways with swapped dimensions;
  favicons decode at `MAX_FAVICON_DIM` (160 px, they
  draw at the toolbar icon size, ~27 px) and reserve their bytes in the
  process-wide budget. A
  refused process-wide reservation is TRANSIENT for both images and
  favicons: the entry backs off for `IMAGE_BUDGET_RETRY_SECS` (bounded by
  `MAX_IMAGE_BUDGET_RETRIES`) and retries instead of failing for the page's
  whole life or burning the network retry on a VRAM problem; while every
  queued entry waits on such a deadline, `pump` returns until the earliest
  one passes (`next_budget_retry`, recomputed when the queue drains) instead
  of re-walking the whole queue for each visible image every frame; a
  favicon backoff
  whose cooldown elapsed is retried by `FaviconCache::poll` ITSELF (the
  repaint it schedules is the only frame an origin nobody renders any more —
  a closed tab, a scrolled-out tile — ever gets), and past the attempt cap it
  FAILS so it stops counting as pending; a favicon that
  only failed on the budget is re-probed once VRAM frees, and a saturated
  origin cache (`MAX_FAVICON_HOSTS`, nothing evictable) answers an unknown
  origin without parsing or allocating. An entry still
  queued past its deadline FAILS (it used to stay pending, keeping the
  repaint loop alive forever). The
  renderer draws hourglass/warning-icon placeholders until then. Raster
  images render at their natural pixel size (never upscaled), clamped to
  the reading-column width when wider; a paragraph holding only an image
  centers it as a block in every context (blockquotes and list items
  included). Small inline occurrences stay in the text flow with a few px
  of breathing room; an image sharing a line with text that is taller
  than `config::MAX_INLINE_IMAGE_H` (2 body lines) or wider than most of the
  column is promoted to a centered block (the remaining text flows as
  its own paragraph) so it never opens a huge blank line around the text.
  A run of 2+ consecutive images with at least one block-sized (a site
  photo strip) renders as ONE centered, wrapping gallery row in source
  order instead of one block per image, so entries never move while their
  textures resolve.
  Inline SVG icons are clamped to 32px tall (`DecodedImage.vector`), block
  SVG keeps its natural size; every inline SVG icon in one BLOCK (paragraph
  or link-stack) shares the tallest one's height (aspect preserved), like a
  single CSS icon class on the source site, while a lone icon/badge keeps
  its intrinsic size. A light/dark logo pair shipped back to back
  (`logo-light.svg` + `logo-dark.svg`) collapses to the one variant matching
  the document theme (only a single differing `light`/`dark` URL token
  pairs), so a card shows one logo instead of two. A transient fetch failure (network/stall/
  truncated decode, 5xx/429/408) is retried once (`MAX_IMAGE_RETRIES`);
  content-type
  rejections, other 4xx and cancellations are final. Every image/favicon entry
  also has a wall-clock deadline (`IMAGE_JOB_TIMEOUT_SECS`): past it the
  entry becomes a failure placeholder and stops counting as pending (the
  cancel flag is set; a decoder stuck mid-call finishes on its own and its
  result is ignored). Image jobs are panic-guarded: a
  decoder panic becomes a failure placeholder instead of poisoning the
  `poll-promise` thread (which would panic the UI when polled).
- SVG: `usvg`/`resvg` parse and rasterize a static tree. `<script>`,
  `<foreignObject>` and the SMIL animation elements
  (`animate`/`animateTransform`/`animateMotion`/`set`/`discard`) are
  rejected by the structural
  pre-pass (never executed), every `href` (network, local and `data:`
  URLs) is never resolved, DTDs (`<!DOCTYPE>`/`<!ENTITY>`, unbounded entity
  expansion) are rejected by that same parse (roxmltree pinned to
  `allow_dtd: false` explicitly, before usvg's entity-expanding parser runs) and
  `filter`/`fe*` markup is STRIPPED (definitions, primitives and every
  `filter` reference: filters are ignored, never rendered — the cost model
  and filter pixmaps stay unreachable, and usvg's dangling-link rule would
  otherwise DROP the element), while `marker`/`pattern` markup
  (a marker subtree is re-converted per path vertex; a pattern pixmap is
  sized by the pattern's own rect) and CSS `filter` declarations
  (`style`/`<style>`, which cannot be stripped without a CSS parser) are
  rejected before parsing, in every spelling the parser accepts (comments
  between the property name and the `:` — `filter/**/:blur(1px)` — and the
  IE star hack `*filter:`, included) — the structural pass
  matches LOCAL NAMES in the SVG namespace (or no namespace, like usvg),
  so namespace-prefixed and namespace-less spellings cannot slip past a
  byte scan — fonts come from the
  app's bundled Noto faces (Noto Sans plus its emoji/CJK/script
  fallbacks; never system fonts), the source is
  token-capped (`MAX_SVG_TOKENS`) and nesting depth-capped
  (`MAX_SVG_DEPTH`, enforced before the XML parser, which recurses per
  level), and every `<use>` reference is counted as a
  fresh expansion (an immediate self reference is the only skip, exactly
  like usvg; whitespace-padded `#id` values are trimmed like `svgtypes::IRI`)
  against the same cap by an ITERATIVE walk (no decode-thread stack overflow
  on deep nesting), so even a mutually recursive pair trips the cap instead
  of reaching usvg's own 1M-node limit, and the raster is capped at
  `MAX_TEXTURE_DIM` per side.
- History: `back`/`forward` capped at 250, snapshots restored on failure;
  the pending URL is committed as `current` before stack moves, so
  Back/Forward during `Loading` never create self-duplicates. A failed NEW
  navigation keeps the page it pushed (`pushed_history`) on `back`, so the
  page the user came from stays reachable; failed Back/Forward restore the
  original stacks. A navigation that supersedes an in-flight load bumps
  `doc_generation` (Loading -> Loading keeps the same state KIND, and the
  display memo is keyed on (kind, generation)), so the rail can never keep
  the superseded URL's title/favicon for the second load.
- Local history (`history/`): a successfully resolved page is handed to
  `App` exactly once (`Tab::pending_visit`) and recorded; failures and
  stopped loads never are. Consecutive visits to the same URL update the
  newest entry. The store is bounded (20 000 entries, 8 MiB of plaintext,
  oldest dropped; the derived byte total is maintained incrementally, so a
  recorded visit answers the caps in O(1) instead of rescanning the store),
  pruned by retention (`Retention::Days` default 90,
  `Forever`, or `Session` = in-memory only: never read/written, leftovers
  wiped on load and exit), saved atomically (per-process unique `.tmp` +
  rename, 0600/0700 on Unix) at most every 2s while dirty and once on
  `App::on_exit`. The encrypted blob is built on the UI thread (bounded) and
  the file write + fsync run on a worker thread, so a slow disk never
  stalls a frame; the worker queue is bounded to one pending blob PER TARGET
  FILE (newest wins; the app writes one data directory) and a flush lets the
  worker WRITE what is queued (never drops it:
  the store cleared `dirty` at enqueue time) before waiting at most 5s for
  the exit, so a hung disk can neither grow memory nor lose the newest data
  nor freeze a frame, a delete or exit; a write, delete or exit flushes the
  worker first, so a stale blob can never land over a newer one, and a
  failed background write re-arms `dirty` so the synchronous save retries.
  When a
  stalled disk outruns the flush timeout, a delete stays ARMED (the wipe
  completes on a later tick and the store is re-marked dirty only for real
  write failures), so it is still never resurrected. A delete is itself a
  JOB on the same worker (`WriteJob::discard`): the exit-time forced wipe
  used to delete the files directly while a stalled worker could still
  rename its temp blob afterwards, so the delete now runs behind any
  in-flight write. A job is handed to a worker that WILL run it or returned
  to the caller (`StoreWriter::push` checks the worker's `exited` flag
  under the same lock the worker takes for its exit decision), and
  `store::writer::queue_or_respawn` replaces an exited worker and retries:
  the old liveness pre-check raced the exit, so the newest blob — or a
  "Delete history" discard — could be left in a queue nobody reads. The
  discard also sweeps the `.bak` and `<store>.tmp.*` siblings
  (`settings::remove_store_file`), so a preserved ciphertext cannot survive
  an explicit delete. A failed delete (e.g. a
  locked file) is reported in Preferences instead of looking successful.
  Stored URLs are normalized (`util::normalize_stored_url`): only authority
  credentials are removed from the URL itself (path/query bytes, including
  nested URLs, are preserved) on record, on load and for the bookmark-ribbon
  comparison. An undecryptable blob is kept as `.bak` before a later save
  could replace it (a wipe removes the backup too). Store loads read
  through `settings::read_regular_file` (opened with `O_NONBLOCK`, the
  OPENED handle re-checked): a FIFO swapped in after the metadata check
  cannot block the UI thread, and the streaming read enforces its cap. A
  store restored world-readable is tightened through that SAME opened
  handle (`fchmod`), never a fresh path lookup: a symlink swapped in after
  the open is neither followed nor `chmod`ed. Crash-orphaned
  `<store>.tmp.*` files and the key-creation temps
  (`store.key.tmp.<pid>.<seq>`, which hold raw key bytes) are swept on
  load.
  File
  format: magic `MDHS`, version 1, 12-byte random nonce, ChaCha20-Poly1305
  ciphertext with the header as AAD; the 256-bit key lives in the single
  shared `store.key` (history, bookmarks, settings, session; created 0600;
  installed atomically through a private temp
  file + `hard_link`, so two instances can only ever read the winner's
  complete key; a pre-existing key with group/other bits is tightened on its
  OPENED handle, `O_NOFOLLOW` refuses a symlinked key without following it,
  and a key whose permissions cannot be tightened is refused, never
  deleted). "Delete history" keeps the shared key; only "Delete all data and
  close" deletes the whole directory — and a removal that FAILS (open
  handles, permissions) does not exit pretending the data is gone: writes are
  re-enabled and the failure is surfaced in Preferences. Any IO/crypto error
  degrades to an
  empty in-memory store plus a Preferences warning; history never blocks or
  crashes the app.
  `ui::history_page` renders the local history as an INTERNAL PAGE
  (`about:history`) in a tab, drawn in the same reading sheet as a document
  (a capsule search field over rolling date sections — Today / Yesterday /
  Previous 7 days / Previous 30 days / Older —, one favicon per row from the
  shared session cache, per-row remove, two-click delete-all); `Tab::open_history`
  intercepts the URL before `parse_https` (never fetched, never recorded,
  never a network request) and treats it as a real navigation so Back/Forward
  work. `App::open_history` (rail button / Ctrl+H / Cmd+Y) creates a NEW tab
  for it, inserted right after the active one and activated, so the page
  being read is never replaced; a row click then navigates that same tab to
  the visit, with Back returning to the list (a repeat request while the
  active tab already shows the page is a no-op). The internal third-party
  licenses page (`about:licenses`) shares that bookkeeping
  (`Tab::open_internal_page` backs `open_history`, `open_licenses` and
  `open_home`); the new-tab page itself is the third internal page
  (`about:home`, typeable, with an empty capsule): a real back/forward
  entry, so passing through Home never loses the Forward chain, while
  Reload stays disabled on it and the session file keeps the `None` Home
  marker for it (no format change).
  `licenses::document` embeds `THIRD_PARTY_NOTICES.md` and parses it once per
  process through `finish_document`, `viewer` renders it with the normal
  document pipeline (menu, selection, live links), and `App::open_licenses`
  backs the Preferences footer link. `ui::preferences` keeps only
  the floating, FIXED-SIZE Preferences dialog (borderless custom header,
  tab bar, grouped section cards, list rows, a toggle switch, and a
  read-only keyboard shortcut reference in English). The General tab holds
  theme, startup zoom and restore-tabs plus the compact version/licenses
  footer ("View third-party licenses"); the Data tab holds the retention
  dropdown and the destructive "Delete history" action (two-click
  confirmation) plus the "Delete all data and close" card (two-click: wipes
  the data directory and `process::exit`s without further saves). Every
  dropdown
  row keeps its geometry on hover; each delete-confirmation note only takes
  space while armed (fit content: arming may push the cards below, with the
  inner scroll covering small viewports). `App` owns its
  visibility.
- Tabs (`ui::tabs`): the rail reports select/close/reorder/new/history
  through one `TabsResponse`. A row's interaction id is keyed by the TAB
  (never by its slot), so focus, hover, tooltip and the in-flight drag follow
  the page when the list reorders (`Alt+Up`/`Alt+Down` would otherwise walk
  the focus onto whichever tab arrived at the slot). The drag reorder is the
  bookmark grid's contract (state in egui memory, applied live while the
  pointer moves, persisted only on start/stop) with ONE deliberate
  difference: drop targets are found GEOMETRICALLY (`Context::rect_contains_pointer`
  on THIS frame's row rect, which still honors what is on top), not with
  `Response::contains_pointer` — that snapshot is computed at frame start
  from the PREVIOUS frame's widget rects, and a live reorder moves the rows,
  so the lagged hit test kept targeting the slot a row no longer occupied and
  the dragged tab bounced between two slots instead of advancing. The same
  geometric read drives the row's hover wash. A drag also keeps the overlay open
  wherever the pointer goes (the rail is hover-driven and a drag leaves it),
  the dragged tab's id self-heals when its row is closed mid-gesture (so
  `ui::forget_tab` needs no entry), and `App::move_tab` is the grid's
  remove+insert with `active` re-indexed so the shown page never changes. The
  session snapshot compares URLs per index, so a reorder persists through
  "restore tabs on startup" with no file-format change.
- Bookmarks (`bookmarks/`): explicit user data, encrypted like history
  with the shared recipe (`crypto`: magic `MDBM`, random 96-bit nonce,
  256-bit shared key in `store.key`, 0600/0700 on Unix; `store::writer` for the
  debounced write). Plaintext: one `timestamp\turl\ttitle\ticon\tcolor`
  line per bookmark (icon/color optional; three-field lines written before
  the tile customization still parse). Ctrl/Cmd+D or the toolbar bookmark ribbon
  toggles the loaded page (`Tab::bookmark_target`; a toast confirms, and
  Home/Loading/History/Failed have nothing to bookmark), and the "quick
  access" grid is rendered ONLY on the Home / new tab page
  (`ui::bookmarks`): responsive, virtualized tiles in a fixed-width block
  centered under the address capsule, newest first. The omnibox ribbon
  (`omnibox::BookmarkState`, computed by `App` from the store: unavailable
  / outline / filled) shares ONE right slot with the clear X, so editing or
  a load resolving never shifts the field. A tile shows the custom glyph
  (`icons::paint_text`, color emoji or monochrome
  fallback) when one exists, else the site favicon (the shared session
  `FaviconCache`, HTTPS origins only, requested by visible tiles through
  `texture_for_url`, which derives the origin key from the URL string so a
  settled tile costs no `Url::parse` per frame), else
  the first letter of the title on a stable palette color (stored index, else a hash of the URL). The name lays out to at most two
  centered rows, elided with `…` by the layout (the char cap alone cannot
  know the real line count); the laid-out galley is memoized per URL
  (invalidated by the store `generation`, the theme and the tile width), so
  a settled Home grid builds no `String`/`LayoutJob` per tile per frame. The
  grid's floating scrollbar stays hidden
  until the rows actually exceed `config::BOOKMARK_GRID_MAX_VISIBLE_ROWS` (the trailing item gap
  made egui report a scrollable area with nothing to scroll). Clicking a tile -- icon or name --
  opens it in that tab, Cmd/Ctrl+click asks `App` for a new tab, dragging a
  tile reorders the grid (live, mirrored between display and store order),
  `Alt+Left`/`Alt+Right` on the FOCUSED tile move it one display slot
  (`move_indices` mirrors display slots onto store indices, so a keyboard
  user can arrange the grid like a drag),
  and the tile editor is INLINE (egui memory keeps the open field; no
  dialog): the hover pencil opens the rename field whose same spot becomes
  a check that saves. Enter or a click elsewhere commits (sanitized by the
  store, saved debounced), Esc drops the draft (`Tab::tile_editing` keeps
  the shortcut handler off the key) and the hover X removes the tile. Once a
  tile has keyboard focus, `Tab` reaches the pencil and the X (the frame
  that moves focus off the tile keeps the hover-only buttons allocated, so
  the chain is tile → pencil → X → next tile). The URL index only forgets a
  URL when NO entry carries it any more (`toggle` and `remove_entry` share
  the rule), so a hand-edited body with duplicate URLs can never leave a
  visible tile reported as "not bookmarked".
  There is no
  retention: bookmarks persist until removed and survive restarts; the
  store is capped at `MAX_BOOKMARK_STORE_ENTRIES` and an add at the cap is
  REFUSED with a toast instead of dropping the oldest bookmark (unlike
  history's disposable visits); the serialized body carries its own
  `MAX_BOOKMARK_STORE_BYTES` budget (`entry_bytes` is exact arithmetic, no
  throwaway buffer; an add OR a tile rename that would cross the budget is
  refused the same way, and an over-budget body from
  disk is trimmed oldest-first on load — the derived index/byte/`generation`
  state is rebuilt only when `trim` actually dropped something, since it
  runs on every save). The debounced save hands the WRITER a snapshot
  (`WriteJob::build`) and the worker serializes + encrypts it, so the
  48 MiB byte cap is never built inside a frame; the synchronous
  test/exit path keeps the inline build. A first run (no store file yet)
  starts with
  a curated default set (which accounts for its bytes like any other add);
  an existing store is never reseeded. A failed save
  surfaces as a note on Home.
- Preferences (`settings/`): encrypted `settings.bin` (`theme`, `zoom`,
  `restore_tabs`, magic `MDST`) alongside the history `retention` key, written
  key-by-key (decrypt + read-modify-write + encrypt + the same atomic
  rename), so the history module and the Preferences window never clobber
  each other; a missing/corrupt value falls back to its default. The
  read-modify-write runs against a per-directory plaintext CACHE (a
  successfully read body serves every later key read AND write, so loading
  four keys decrypts once; the startup validation seeds the cache, so a
  valid `settings.bin` is read + decrypted once per launch) and the
  shared `store.key` is loaded + validated once per process (a preference
  or session write never re-reads it). The file write + `fsync` go to a
  dedicated background worker
  (`write_setting_async`), so `Prefs::set_*`, the retention switch and the
  per-change session save never stall a frame; `App::on_exit` drains the
  settings and session workers (one shared deadline, so the documented
  bounded wait is not silently doubled), and only a `restore_tabs` toggle
  waits
  briefly (bounded) so the preference is durable before the session file is
  deleted. All four
  stores share the single `store.key`. Legacy `settings.conf` /
  `session.conf` and `history.key` / `bookmarks.key` are deleted on load,
  never read (clean break). A read failure is never treated as an empty
  file (it would silently drop the other keys): a `settings.bin` that
  exists but cannot be read/decrypted makes the WRITE REFUSE
  (`settings_body_with`), and the failure
  is surfaced in Preferences instead of "reverting" on the next launch. An
  unreadable `settings.bin` is likewise surfaced and never treated as
  "restore tabs off", so it cannot delete the stored session by accident.
  A background write that fails (full disk, revoked permissions) surfaces
  the same way: `Prefs::tick` drains EACH worker's recorded outcome per
  frame — settings in `settings_error`, session in `session_save_error` —
  so a successful per-tab session save cannot clear a settings-write
  failure, and a session snapshot that failed ON THE WORKER cannot pass
  silently (the session worker's outcome used to be unreachable: only the
  enqueue error was reported). A snapshot whose save could not even be
  enqueued is not marked as saved either: the snapshot bookkeeping is
  invalidated, it
  stays pending and retries on a `SESSION_SAVE_RETRY_SECS` backoff (bounded,
  so a machine with no data directory does not allocate an error string per
  frame; the app schedules the repaint for the retry window, so the save
  lands on an idle window too). A job that could not be QUEUED at all is a failure too (the
  spawn-refusal path returns it), and it is reported against ITS file: a
  session enqueue error lands in `session_save_error` (not the settings
  card) so `App` invalidates the snapshot and retries, and a settings enqueue
  error cannot clear a session warning or vice versa. A worker that a
  bounded `flush` retires (the "restore tabs off" path flushes mid-session,
  not only at exit) parks its last outcome first, so a failed write is still
  reported to Preferences instead of dying with the dropped worker.
  A settings body that would cross `MAX_SETTINGS_BYTES` is REFUSED
  (`settings_body_with`), so a change can never write a file the reader
  would reject next launch (the reader's caps derive from
  `crypto::OVERHEAD_BYTES`).
  Theme maps to `egui::ThemePreference`
  (System/Light/Dark) and is applied idempotently; the startup zoom is
  clamped to `config::ZOOM_*` and applied to the live view when changed.
  "Restore tabs on startup" remembers one entry per tab (a URL, or a `Home`
  marker so the tab count and active index survive) in encrypted
  `session.bin` (magic `MDSS`),
  written only while enabled and deleted when turned off through the
  SESSION WORKER as a discard job (so a queued/in-flight snapshot can never
  be renamed into place after the delete), then verified with a direct
  sweep (a failed delete is
  retried at every start and whenever the toggle is set off again, and the
  orphaned `.tmp.*`/`.bak` siblings are removed too); `App::new`
  reopens them (navigating `about:history`/HTTPS pages) before the first
  frame. A tab whose load is still in flight, or has already failed,
  reports the last real navigation target as its session entry
  (`Tab::session_url` / `session_target`), so a slow or failed restore keeps
  the URL the user asked for instead of being rewritten as a `Home` marker
  by the first save. OS/CLI open targets WIN over a stored session, but only
  for the current launch: the file is not rewritten until the open tabs
  actually change, so double-clicking a document never destroys the
  remembered session.
- Rendering: `pulldown-cmark` events are parsed ONCE at fetch time into
  owned events (`Fetched.events`); copy text (`Fetched.plain_text`) is
  derived FROM THOSE EVENTS (`plain_text_from_events`, so the clipboard
  never pays a second parser pass; a corpus test pins it byte-identical to
  the body-based oracle). The page-title fallback (`Fetched.h1_title`, the
  document's first `#` heading) is derived from the same events and read
  through `Fetched::page_title` (frontmatter `title` first), so a document
  without YAML metadata still names its tab, window, page-info card and
  default bookmark/history entry instead of showing its URL.
  `:shortcode:` emoji are expanded for text nodes
  (never inside code) in the rendered events AND in `plain_text`, so the
  clipboard matches the viewer. The theme sets
  `override_text_color = on_surface`, so plain labels paint the readable
  body token; the muted tone is opt-in (`theme::weak`, plain quote bodies
  via the walk's `override_text_color` scope, H6), and bold text uses a
  named font family whose first face is the bundled Noto Sans variable
  font pinned to `wght = 700` (`theme::strong`; epaint's `FontId` has no
  weight axis). Bare URLs (`www.example.com`,
  `https://example.com`) and bare domains (`celticos.com`,
  `agentready.md`) and local `file:` URLs are linkified in the same pass
  (`markdown::autolink`, GFM rules + the bare-domain/`file:` sugar): a text
  node is rewritten into `Link` events only after a delimiter boundary (a run
  glued to a link's closing `)` or a code span does not qualify), with
  a valid domain required and trailing punctuation / unmatched `)` /
  entity-like `&x;` excluded, and a domain glued to `@` stays text in every
  candidate form (email spelling), so a URL typed as plain text renders,
  hit-tests and opens exactly like a markdown link (Copy still pastes the
  URL text the document wrote). Only `https:` (typed, or inserted for
  `www.`/bare domains) and local `file:` URLs linkify — `http:`, `mailto:`,
  `ftp:`, `data:` and every other scheme stay text, and emails never link.
  A bare domain must end in a real IANA root-zone TLD (`markdown::tld`, a
  generated allowlist), so `Cargo.toml`, `notes.txt`, `foo.local` and
  `v1.2` stay text; TLDs that double as common file extensions (`md`, `rs`,
  `sh`, `py`, `pl`, `ai`, `zip`, ...) also require the label immediately
  before the TLD not to be a known file name (`README.md`, `config.rs`,
  `blog.readme.md`, `logo.ai` stay text; `agentready.md`, `mkplandeki.pl`
  link). Code spans/blocks, metadata
  blocks, existing links and image alt text are never touched. Bounded by
  `MAX_AUTOLINKS_PER_DOC` / `AUTOLINK_MAX_DOMAIN_CHARS` and a linear scan,
  and every generated destination must parse as a real `https:` URL with a
  host and NO credentials (a trailing dot before the `@` used to slip an
  `https://example.com.@evil.com` candidate past the email guard: the
  parser read `example.com.` as userinfo, so the click would have landed on
  `evil.com`). The viewer walks the events
  recursively (list items, quotes and footnotes nest correctly) with a
  DEPTH GUARD
  (`MAX_RENDER_DEPTH`): hostile nesting (`> > > …`, `- - - …`) renders
  flattened instead of overflowing the UI thread's stack. The event list
  itself is capped (`MAX_EVENTS_PER_DOC`): an over-complex document fails
  the route with a bounded line instead of building a several-hundred-MB
  vector. Rendered blocks are capped too (`MAX_RENDERED_BLOCKS`): a crafted
  body of ~1M tiny paragraphs used to lay out every block each frame; past
  the budget the walk consumes blocks without rendering them (Copy keeps the
  full text). Code blocks share a per-document line budget
  (`MAX_RENDERED_CODE_LINES`) so a few MB of newlines cannot render millions
  of labels per frame — the "hidden lines" notice is ADVANCED past the
  reserved body (the rows are painted, not laid out, so it used to sit at the
  block's top and cover the first visible line) — and every rendered
  prose/HTML/table block is capped
  (`MAX_RENDERED_BLOCK_CHARS`) so one multi-MB paragraph/heading/cell cannot
  become a single giant label (a bounded `… [truncated]` notice ends it;
  `Copy` keeps the full body). An HTML block's buffer is collected under the
  SAME cap (one char past it, so the notice still fires) instead of up to 4x
  it in bytes before the renderer trimmed it. Inline fragments
  borrow text from the events (`Frag<'a>` with `Cow`), so repaints do not
  copy the document. Code bodies + token spans are cached per (document salt,
  the block's `Start(CodeBlock)` EVENT index, language) in a bounded LRU
  cache with a byte budget; the event index is stable per document, so
  switching a `<Tabs>` panel or opening an accordion (which change what the
  walk renders, not the document) can never hand a block a sibling's cached
  body. The SAME rule keys a blockquote's inner `ui.indent`
  (`quote_indent_id`: document salt + nesting level + the block's own
  `Start(BlockQuote)` event index, never a per-pass counter), so two quotes
  can never alias onto one set of nested widget ids when the visible block
  set changes. Token spans fill LAZILY: a block first rendered while the line
  budget was spent is cached body-only and highlights on a later frame that
  can show it. The salt
  mixes a per-tab `doc_generation` bumped whenever the visible document is
  replaced, so a reloaded URL can never render or copy the previous body and
  resolving one tab does not flush the others' cached blocks. The LRU
  eviction pass is shared with the table-width cache
  (`viewer::cache::evict_oldest`). Each block also
  caches its laid-out line galleys (keyed by theme/selection/font size), so a
  repaint reuses an `Arc` per line instead of rebuilding a `LayoutJob`; a
  growing line window (a `<Tabs>` switch or accordion release handing the
  block more budget) EXTENDS the laid-out prefix instead of rebuilding it,
  and the block body is VIRTUALIZED on the vertical axis
  (`viewer::code::code_viewport`): only the rows the scroll viewport shows
  are built and painted, placed on a uniform measured row grid with the
  full height reserved, so the widget count per frame no longer scales with
  the line budget, plain blocks included (highlighted rows are painted
  galleys, so per-line drag selection is gone inside them; Copy copies the
  whole block). Both paths keep the horizontal scroll range MONOTONIC: the
  highlighted path measures every laid-out line, and the plain path
  remembers the widest row it has ever painted (`CachedBlock::plain_max_w`),
  so a vertical scroll past a long line cannot shrink the range and clamp
  the offset back to the left edge. The viewport math uses the MEASURED row stride (epaint
  rounds a row to a whole device pixel), not the design token, or a
  long block scrolled past a couple of screens left an unpainted band. It
  also bounds the visible rect with the WINDOW-LOCAL viewport
  (`InputState::viewport_rect`), never `ViewportInfo::inner_rect` (which is
  in monitor space): intersecting that with the local clip rect shifted the
  bound by the window's position on screen and culled the rows near the top
  early, so a block scrolling to the top edge grew a blank band. Both
  renderers place rows at their ABSOLUTE row offset from the content origin
  (which already carries the scroll translation); the plain path's relative
  offset used to park the visible rows at the block top, blanking a
  scrolled plain block.
  Link/image destinations are resolved ONCE per fetch
  (`protocol::rewrite::DestCache`, built from the same events+anchors):
  the per-frame fragment tree only does a hash lookup, never a `Url::parse`;
  one map serves links and (derived) images under a
  `MAX_DEST_CACHE_BYTES` storage budget, so a hostile link farm cannot
  retain unbounded strings past every other cap;
  sources the base may not follow (e.g. `file:` from a remote page) stay as
  inert alt text; a `host:port` spelling the omnibox accepts
  (`localhost:3000`, `example.com:8080/x`, IP literals such as
  `127.0.0.1:8080/x` and `[::1]:8080/x`) resolves as `https://` too, while
  blocked schemes, userinfo forms and an ALLOWED scheme whose spelling did
  not parse (`https://`, `file://`) stay inert — never a bogus relative join
  under the base; raw HTML is never
  interpreted.
  In-document anchors: `markdown::anchors` collects heading slugs (lowercase,
  punctuation dropped, spaces to hyphens), explicit `{#id}` heading ids and
  `fn-<label>` footnote names
  once per fetch (`MAX_ANCHORS_PER_DOC`); each heading id is looked up BY THE
  EVENT INDEX of its `Start` (`AnchorIndex::heading_id_at`), so content the
  walk skips (an unselected `<Tab>` panel, a closed accordion, the block
  budget) can never shift a later heading onto a sibling's target; `#id`
  links resolve against that
  set — percent-encoded and mixed-case fragments included (`#caf%C3%A9`,
  `#Section`) — and scroll the heading/footnote into view via
  `ui.scroll_to_rect`,
  unknown targets render as text; a `#fragment` on a cross-page link is
  carried as `Tab::pending_anchor` and applied after the load resolves — and
  a pending target that EXISTS but is not rendered this pass (inside an
  unselected `<Tab>` panel, a closed accordion, past the block budget) stays
  pending instead of being dropped at the end of the frame, so opening that
  tab later still applies the request (only a target the anchor index does
  not know is cleared).
  Footnotes are numbered by first reference
  (`AnchorIndex::footnote_number`): the same
  pass numbers every REFERENCED label in first-reference order, the
  reference paints that number as a raised small marker
  (`viewer::inline::render_superscript_ref`, linked to the reserved
  `fn-<label>` target and remembering its painted rect in egui memory), and
  the definition paints `{number}. ` as a numbered entry — the marker
  shares the first text line with the note body and continuation blocks
  hang under the text column (`ItemState` markers with `note` style; a
  first block that cannot flow inline keeps the marker on its own row,
  like a list item) — plus a vector back-arrow that scrolls to that
  remembered rect, drawn as the note's own last inline box (the next inline
  flow consumes `RenderCtx::pending_back_arrow`), so it sits at the END of
  the note's text on the same line and wraps like text; a note whose blocks
  never lay out an inline flow (or an empty one) gets the button on a
  trailing right-aligned row instead, so it is never lost. ONE 1px rule
  precedes the first definition only
  (`RenderCtx::footnotes_started`), and a definition the document never
  references still renders, continuing the numbering
  (`RenderCtx::unreferenced_footnotes`) instead of being dropped.
  `plain_text` keeps the source `[^label]` spelling, like an autolink's copy
  text keeps the URL the document wrote.
  `:::`/`!!!` admonitions are normalized to alert blockquotes in
  `document::normalize_body` (outside code fences; unknown types stay
  literal) and their markers are stripped from `plain_text`. Structural
  `:::` custom containers (`:::tabs`/`:::tab`, `:::code-group`,
  `:::steps`/`:::step`, `:::card`, `:::accordion`/`:::details`, column-0
  only, nesting-capped) become the same canonical markers as their JSX
  equivalents before that, so the renderer cannot tell them apart. The same pass
  recognizes MDX/HTML markup (`normalize_markup`): callout components become
  alert blockquotes, every other tag is a transparent wrapper, ESM/`{…}` are
  removed in an MDX document, `<script>`/`<style>` are dropped,
  `<pre>`/`<textarea>` stay inert and comments are invisible — nothing is
  ever executed or evaluated. Structural components (`<Tabs>`, `<Accordion>`,
  `<Card>`, `<Steps>`) are carried to the renderer as canonical
  `<mdowser-cmp data="…" />` markers (one HTML block each, blank-line
  separated so the content between parses as Markdown); a `<Tabs
  items={["A", "B"]}>` string array labels its label-less `<Tab>` children
  (explicit `label`/`title` wins), `<CodeGroup>` is
  desugared to `<Tabs>`/`<Tab>` markers by `normalize_markup` before parsing, inline `<Badge>` as a
  `<mdowser-badge>` pill; `viewer::markdown::components` draws them, with
  tab AND accordion state in egui's temp store keyed by document salt + the
  marker's own event index + labels/title (so two tab sets with identical
  labels — or two same-titled/untitled accordions — never share a
  selection or an open/closed state); a component label (tab cell, accordion
  header, step title) is capped at `MAX_COMPONENT_LABEL_CHARS` with the full
  text in the hover tooltip, so one long label cannot push the rest of the
  strip outside the reading sheet. A `<Badge>` inside a heading draws the
  same pill as in a paragraph, scaled with the heading (capped) — it used to
  flatten into plain heading text. Every display cap that elides text —
  suggestion/history/bookmark titles and URLs, footnote labels, window
  titles, block text, component labels — goes through
  `util::truncate_chars`, which never cuts inside a
  grapheme cluster (a combining mark, ZWJ family or flag stays whole), and
  `plain_text` skips the markers. Conservative
  product heuristics (deliberate, thresholds in `config/`): a leaked
  first-block fragment skip link is dropped (a plain paragraph is not),
  long link-only paragraphs stack one link per row (card entries carrying a
  logo included), one-line pipe/dot menus
  render as a centered nav row (measured, not `vertical_centered`: a
  wrapping child spans the full width); both menu separator families keep a
  prose guard. `normalize_body` also repairs a link an exporter split with a
  blank line (`[label` blank line `body](url)`) so it parses as one link,
  and rewrites the docs-generator `[#id]` heading marker
  (`## Title [#custom-id]`) to the heading attribute `{#custom-id}`, so the
  marker never renders and the explicit id is the anchor.
  README headings/rules/quotes/tables come from
  `ui::theme` tokens. Tables (`viewer::table`) measure every cell line
  and cache the measured widths (keyed by content hash + column caps +
  selection + the image/emoji geometry epochs, so a resolution transition
  re-measures exactly once and settled frames hit — including tables that
  hold images or color emoji, whose widths depend on resolved textures; the
  content fingerprint itself is memoized per (doc salt, table EVENT index),
  so settled frames do not re-hash the whole table text at all — the index
  is stable whatever the frame renders, so a `<Tabs>` switch cannot alias
  two tables), and how many CELLS one frame may collect is capped
  (`config::MAX_RENDERED_TABLE_CELLS`): one collected row spends one block,
  but a row can hold up to `MAX_TABLE_COLS` columns, so a 5 000-row x
  4 096-column table used to lay out ~20M cells per frame while the row
  budget saw only 5 000 units — past the cell cap the remaining cells are
  consumed without fragments (no layout, no widgets) and the table ends with
  the usual truncation notice (`Copy` keeps the text)
  and render each cell at
  its column's real width (the Grid
  reports `max_col_width` as the available width of non-last columns, so
  aligning against it inflated right/center columns); `distribute_columns`
  then grows the measured widths proportionally to fill the card (no blank
  strip) or shrinks them with a `min_col` floor so wide cells wrap instead
  of forcing a horizontal scrollbar; when even the floors do not fit, the
  table scrolls with every column kept at its measured width up to a wider
  `scroll_max` cap (the floors wrapped words mid-word), and the pass paints
  collapsed 1px interior row/column separators between the cell
  rects of that same pass; the card frame stays the outer border. A fitting
  layout never sums past the usable width, not even by a rounding ulp (the
  residual is taken off the widest column, never below `min_col`): the
  overflow test that selects the scrolling path cannot mistake an f32
  artifact for a real "even the floors do not fit" — a two-column table
  grown to fill 757 summed to `757.00006` and collapsed its second column
  to `scroll_max` while the card stayed full width. A table that still
  scrolls — and every horizontally scrolling code card — reserves the
  floating bar's lane under the content (`theme::scrollbar_lane` via
  `ScrollArea::content_margin`), so the expanded handle passes below the
  last row/line instead of covering it; the code card keeps its uniform
  16px outer bottom (4px padding + 12px lane) and a fitting table keeps its
  grid flush with the card border.
- Fonts (`ui::theme::install_fonts`, `media::decode::svg::build_svg_options`):
  the embedded Noto stack — the bundled Noto Sans faces lead every UI
  family: Noto Sans for proportional text (chrome, menus, documents) and
  its true-monospace cut (TTC face 5, Noto Sans Mono CJK JP) for code,
  with Noto Sans CJK face 0 as the proportional CJK fallback. The complete
  monochrome emoji face and
  every face in `assets::SCRIPT_FONTS` (Arabic, Hebrew, Indic, Southeast
  Asian, ...) follow as glyph fallbacks. epaint's bundled default
  faces are never registered; in the SVG database every generic
  family — `serif`/`sans-serif`/`cursive`/`fantasy` and its no-family
  default to Noto Sans, `monospace` to the Noto Sans Mono CJK cut — so
  SVG text can never pull a system font. A named
  `theme::strong_family` family pins the SAME Noto Sans variable asset to
  `wght = 700` (`FontTweak::coords`) as its first face for bold text (CJK
  and emoji fall through to the regular faces: no variable CJK face is
  bundled), because epaint's `FontId` carries no weight axis. The two
  largest faces ship BROTLI-COMPRESSED (`build_support/gen_fonts.rs`
  compresses at build time into `OUT_DIR`; `assets::noto_sans_cjk()` /
  `assets::noto_color_emoji()` decompress once into one leaked
  process-lifetime buffer each): CJK synchronously at startup, color emoji on
  a background PREWARM thread (`media::emoji::prewarm`, spawned by
  `App::new`), so neither the first emoji nor a chrome icon ever parses the
  ~24 MB face mid-frame; a machine that never shows the window still pays
  the one-time decompression on that thread. Exactly one copy exists in memory — epaint
  borrows the slice through `FontData`, the SVG fontdb wraps the SAME slice
  in an `Arc`, `media::emoji` borrows it — and the build round-trip
  validates every blob before the crate compiles. Noto Color
  Emoji is deliberately NOT in those chains: `media::emoji` shapes its
  clusters with harfrust, extracts the `glyph<id>` element and its `#id`
  references from the font's SVG table (read-fonts/roxmltree) and rasterizes
  it with the hardened resvg pipeline into a texture, cached per
  (cluster, device-px size) under `config::EMOJI_CACHE_*` and the
  process-wide texture budget; the viewer buckets its text sizes. A miss
  rasterizes on a background worker (`MAX_CONCURRENT_EMOJI` process-wide:
  over the cap the miss stays unregistered and a later frame retries), the
  monochrome fallback holds the line, and `EmojiCache::poll` uploads the
  finished texture (up to `MAX_IMAGE_BUDGET_RETRIES` budget refusals, then a
  permanent fallback); the bookmark-tile glyph cache (`ui::icons`) shares
  the same pipeline and is polled from `App::logic`. The SVG
  database shares the static slices through `Arc` instead of copying the
  ~19 MB CJK collection per decode thread. epaint rasterizes with
  fontations (`skrifa`) and shapes with `harfrust`; since epaint applies no
  bidi, `viewer::bidi` resolves the paragraph base direction, the
  visual run order, RTL word order and mirrored punctuation before layout,
  and `inline_flow` lays RTL lines out from the right. Whole-document copy
  stays logical (a native drag selection copies the displayed string);
  content split across inline elements is reordered per fragment.
- UI: toolbar (top, full width), then the collapsible tab rail (44px;
  expands on hover as a floating overlay with a short animated width tween
  and fading titles, no content reflow; it collapses only when the pointer
  moves into the document area, staying open over the toolbar and outside
  the window; the overlay merges with the toolbar
  at the top and only its right edge is elevated), then the central content
  panel. The rail reads each tab's title, favicon origin and OS window
  title from the
  per-tab display memo (`Tab::display`: `(kind, doc_generation)` keyed, so
  a settled frame allocates nothing per tab — no `Vec` of cloned origins
  either: the favicon request loop borrows the memo, and `FaviconCache`
  looks up with a borrowed key slice, so a settled frame performs hash
  probes only; `show_tabs` therefore takes `&mut [Tab]` while mutating
  nothing but the memo). Chrome icons are
  monochrome vector strokes
  (`ui::icons`): every glyph is hand-drawn on one 24x24 design grid and
  tinted with the active theme, so the whole chrome shares one uniform,
  crisp line-icon set; buttons ease the glyph from the muted tone to the
  accent while it grows ~10% on hover (and pulls back on press), and
  keyboard focus draws the shared `theme::focus_ring`. Flat filled buttons
  share one body (`theme::widgets::sized_button`, labels centered with
  `add_sized` whatever the parent layout — `Button::min_size` alone left
  them on the parent's cross axis): `theme::tonal_button_sized`
  (neutral `secondary_container`) everywhere, plus
  `theme::primary_button_sized`, the single accent-filled white-label action
  (the error view's "open in your browser" escape hatch), whose dark-mode
  fill is a darker accent shade so the white label keeps WCAG AA on every
  pointer state. Corners share the
  `theme::radius` scale (`SMALL/HAIRLINE/CARD/MENU/TILE/SHEET/FULL` + `theme::pill_radius`;
  rows use `row::ROW_RADIUS = radius::MENU`, switch/zoom tracks use
  `pill_radius`), type shares `theme::typography` (`UI_COMPACT/UI_SMALL/EYEBROW/UI_LINK/BUTTON/DROPDOWN/ROW/HEADING/DIALOG_TITLE/PAGE_TITLE/DISPLAY`),
  strokes share `theme::stroke` (`HAIRLINE/ICON/EMPHASIS`) and gaps share
  `theme::spacing` (`XXS/XS/S/M/L/XL/PAGE_END`); every chrome icon button shares
  `theme::ICON_BOX` (26px: tab close, omnibox clear, history clear, zoom
  buttons, image placeholders, accordion headers); tile avatar hues live in
  `theme::AVATAR_PALETTE` (documented fixed-hue exception: identity colors,
  not chrome surfaces); the find washes live in the `Scheme` (`find_match`/`find_current`,
  pale/strong amber per theme) so `find_match_bg`/`find_current_bg` borrow
  the interned palette; hand-painted rows paint the focus ring IN ADDITION
  to the hover wash (never `else if`), and every truncated title/URL
  tooltips its full text on hover (`ui::hover_tooltip_lazy`, allocation-free
  off-hover) while accesskit gets the same string via `widget_info`; suggestion/history row caps (`config::ROW_TITLE_CHARS /
  ROW_URL_CHARS`), the page-info card's address cap
  (`config::SITE_INFO_ADDRESS_CHARS`), delete confirmations
  (`config::DELETE_CONFIRM_SECS`) and
  chrome heights (`config::CHROME_ROW_H / CHROME_DROPDOWN_ROW_H /
  CHROME_ZOOM_H`) are single-sourced in `config`. Display helpers
  (`util::strip_scheme_prefix`, `util::truncate_chars`) return `Cow`
  (borrowed on the common fit path; the scheme check is case-insensitive
  without a whole-URL lowercase alloc). The toolbar
  is the navigation
  cluster plus the shared `ui::omnibox` capsule (padlock indicator with
  hover/focus feedback, a click-to-open page-info card with the facts of
  the page actually rendered — `Tab::site_info` in `browser::tab::info`,
  read-only and bounded — address autocomplete, Enter
  submit, Esc revert, select-all on focus, loading sweep, and ONE right slot
  shared by the clear X while editing and the bookmark ribbon otherwise:
  filled when the shown page is bookmarked, outline when not, disabled when
  nothing is loaded) and a
  responsive zoom pill; no `Go`. The Home button opens the NEW TAB page
  (the landing page with the large omnibox and the bookmark grid); the page
  left behind moves to `back`, so Back returns to it. While loading, the
  reload slot becomes Stop
  (`Tab::stop` restores the backdrop page and its images). Every top-bar
  button owns a fixed slot whatever icon it shows, so a swap (Reload/Stop,
  ribbon states) never shifts its neighbours. Favicons are
  probed per origin (`/favicon.ico` → `/favicon.png` → `/favicon.svg`, each
  probe bounded by `FAVICON_REQUEST_TIMEOUT_SECS`) and cached for the
  session; the cache gates `poll`/`has_pending` in O(1) from
  loading/backoff/failed counters (settled frames do not walk its
  entries), the budget-backoff walk only runs once the earliest deadline
  passes, and a probe the image-slot limiter defers records a short
  backoff instead of re-parsing its URL on every following frame. The page
  sheet inside the central panel is flush with ALL FOUR
  window edges (the rail included; only its rail-side corner stays rounded)
  so the reading area wins those pixels; the responsive gutter and the
  always-visible scrollbar travel INSIDE the scroll content
  (`theme::content_gutter` + `ScrollArea::content_margin`) so the bar hugs
  the sheet's right edge and a scrolled document is clipped at the flush
  edges instead of one gutter short of them. Every scrolling page closes
  with a shared `theme::spacing::PAGE_END` (24px) blank pad under its last
  element (documents, Home, the error view), so a page scrolled to its end
  keeps a small blank margin under the text; the top gutter is unchanged.
- Omnibox autocomplete (`suggest.rs` + `ui::omnibox`): while either
  address field is editing, a popup lists up to
  `config::MAX_OMNIBOX_SUGGESTIONS` suggestions ranked bookmarks → history →
  builtin (`builtin_sites::builtin_suggestions()`, the curated `(url, title)`
  list vendored from MDowser-Sites (`make sync-sites`) into
  `assets/builtin_sites.txt` and embedded at compile time), deduplicated by exact
  normalized URL and matched case-insensitively on URL + title (schemeless
  queries included; ASCII candidates compare in place, so the scan does
  not allocate a lowercase `String` per candidate per keystroke); an empty
  field offers recent bookmarks. No row is highlighted until picked: the
  first `Down`/`Up` selects the first/last row, `Tab` completes the
  highlighted row without navigating (the first one when nothing is
  highlighted), `Enter` submits the typed
  address (only an explicit `Down`/`Up` pick or a pointer that MOVED onto a
  row routes it to the
  selection through the normal `Tab::navigate_from_address` path; a
  stationary pointer over the popup is
  not a pick, and typing clears the pick so a URL
  edit can never be silently replaced by the page already shown), and `Esc`
  closes the popup before reverting the field. A clean click on the field
  selects the whole address, while a drag keeps the range built from the
  press point (the deferred select-all). The popup is a foreground
  `Area`: it never shifts the capsule and its row clicks survive the frame
  egui surrenders the TextEdit's focus on. The list is cached per field in
  egui memory, rebuilt only while the field is EDITING and only when the
  query or the `HistoryStore`/`BookmarkStore` `generation()` changes, so
  the 20 000-entry scan runs on keystrokes, never per frame, and never
  twice per keystroke on Home (where the toolbar field and the capsule
  share one address and only one of them can own the keyboard); a closed
  tab drops its cache
  (`ui::forget_tab`, the single registry for every per-tab egui-memory
  key: omnibox caches + field edit states, history filter/cache/clear +
  search-field state, the Home/bookmarks/history-page scroll offsets, the
  document reading sheet's per-(tab, URL) offsets, the
  error-view scroll offset, and the Home rename/drag drafts). Scroll
  offsets are registered through `ui::remember_scroll` with the REAL id
  their `ScrollArea` returned (one surface can record several ids per tab —
  the document sheet keeps one per visited URL — and the close path removes
  them all): a `ScrollArea` derives its persistent id
  from the parent `Ui` (`make_persistent_id`), so the old
  `Id::new((salt, tab_id))` removals never matched and every closed tab
  leaked one scroll state per surface.
- Omnibox capsule state (`ui::omnibox`): the right slot is reserved from
  `bookmark.is_some() || focused || !address.is_empty()` and the fill/ring
  is painted after the field with the FRESH focus, so a pointer press shows
  its ring in the same frame and neither editing nor focus can shift the
  text area's right edge (the pre-field focus read was a frame behind).
- App identity: every raster icon derives from `assets/icon.svg` through
  `build.rs` (`build_support/gen_icons.rs`: the integrated gen-icons tool;
  it runs automatically when the committed outputs are missing or outdated,
  and `make icons` forces it). `main` decodes
  `assets/icons/icon-256.png` (`assets::window_icon`) into
  `ViewportBuilder::with_icon` for the Windows/Linux window, taskbar and
  Alt-Tab; Windows executables also embed three PE resources (`build.rs`,
  windres for GNU, `rc.exe` for MSVC, best-effort: a missing compiler warns
  and builds without them): `assets/icons/icon.ico` as the shell icon, a
  `VERSIONINFO` block (product/file description, version, copyright — what
  Explorer's Details tab and installers read) and the application manifest
  (`RT_MANIFEST` id 1: per-monitor DPI awareness, supported Windows versions,
  UTF-8 code page, long paths, segment heap). `main` additionally sets
  `windows_subsystem = "windows"` for that target, so no console window opens
  behind the GUI. macOS reads `Contents/Resources/icon.icns` from the `.app`;
  Linux reads the hicolor ladder (`.deb`, `.rpm`, Flatpak, `make install`)
  plus a shared `.desktop` with `Icon=`/`StartupWMClass=` and AppStream
  metadata the Software centers render
  (`packaging/linux/`, staged by `scripts/packaging.sh` `install_linux_tree`,
  which pre-flights every source and renders the metainfo fail-closed via
  `render_metainfo`).
- Shortcuts in `app::handle_shortcuts` (consume_key so typing never
  triggers them): Ctrl/Cmd+T/W/R/L/A, Ctrl/Cmd+1..9, F5, Esc (stop loading
  when no field/menu/find-bar owns the key; a focused omnibox reverts its text
  instead), Ctrl/Cmd+D (toggle the bookmark of the loaded page, with a
  toast confirmation), Ctrl/Cmd+, (Preferences), the platform History key
  (Ctrl+H / Cmd+Y), zoom `+`/`=`/`-`/`0`, Ctrl/Cmd+F (in-page find on document
  pages, even while a field is focused) with `F3` / `Shift+F3` stepping
  through its matches, and Ctrl/Cmd+A
  selects the whole page and Ctrl/Cmd+C copies it while that highlight is
  active (egui only emits `Event::Copy`; no key event to consume). The
  full table is `docs/INTERFACE.md`.
- Right-click menu (`viewer/overlays`): minimal Copy / Select all, plus
  "Open link in new tab" when the click hit a link (link hitboxes are
  collected by the renderer each frame). `Cmd/Ctrl+click` on a link is the
  direct route: the renderer's `on_link` callback hands the viewer a
  background-tab request instead of a same-tab navigation (an in-page
  `#anchor` becomes its absolute URL plus fragment). The rows carry explicit widget ids
  and are keyboard-navigable: opening with the pointer leaves NO row
  focused (the old pending-focus seed ringed the first row on every open),
  `Up`/`Down` move focus into the rows — the first press lands on the first
  one, with egui's own arrow traversal cancelled so it cannot skip it — and
  `Enter` activates the focused one. A primary press outside the popup
  closes it (a completed click did before; a press-drag selection never
  reached `clicked` and left the menu floating). The menu renders
  BEFORE the
  document so a Copy click can push `Event::Copy` into the same frame and
  the labels copy the native selection; whole-page copy clones the
  precomputed `Fetched.plain_text` (no UI-thread reparse).
  `MdowserApp::raw_input_hook` intercepts a secondary PRESS that lands on
  the document while a native text selection exists — egui's
  label-selection plugin reads any press over a selectable label as the
  start of a drag and would collapse (or clear) the selection before the
  menu could copy it. The press is dropped and its release arrives as
  `Tab::pending_context_click`, which opens the menu; the selection (and
  its highlight) survives, so the menu's Copy copies exactly the selected
  text.
- Native macOS menu bar (`app::menu`; macOS only — Windows and Linux have NO
  menu at all, the toolbar is the whole chrome). Startup installs a
  `muda`-built `NSMenu` over winit's bare default (About/Services/Hide/Quit
  only): an Application menu (About MDowser, Preferences… `Cmd+,`, Services,
  Hide/Hide Others/Show All, Quit — the first submenu, which AppKit titles
  with the app name), File (New Tab, Close Tab), Edit (Copy, Paste, Select
  all — deliberately no more), View (Reload, Zoom In/Out/Actual Size),
  History (Show History) and Window (Minimize, Zoom, Bring All to Front). The
  bar owns no logic: each custom item replays the shortcut the keyboard would
  (`MenuAction::shortcut`: `Event::Copy`, a `Cmd+<key>` `Event::Key`, or a
  clipboard-built `Event::Paste`), queued by the muda handler (which calls
  `Context::request_repaint`, since a menu activation is not a winit event
  and must wake an idle window), drained by `App::poll_menu` (which
  re-schedules the injecting pass) and injected
  into the raw input in `App::raw_input_hook` — so a command raised while the
  window is minimized survives in eframe's pending input, and both the menu
  and the real keys land in the SAME `handle_shortcuts` and focused
  `TextEdit`. There is no double handling: an item whose key equivalent
  matches is consumed by AppKit, so the real key never reaches egui.
  Two AppKit defaults are switched off/overridden so the bar shows only
  MDowser's commands: startup opts the process out of automatic window
  tabbing (`app::menu::disable_automatic_tabbing`, called by `main` BEFORE
  any window exists), which keeps the system's injected "Show Tab Bar"/
  "Show All Tabs" items out of View (MDowser's tabs are the in-window
  rail), and About MDowser carries the app's own icon
  (`AboutMetadata::icon`, the embedded 256px raster) instead of AppKit's
  generic placeholder folder.
  `muda`/`arboard`/`objc2`/`objc2-app-kit` are macOS-only dependencies
  (`arboard` only backs Paste; the objc2 crates only make the one safe
  AppKit call muda does not surface).
- Native window geometry (`app::window`): the FIRST frame sizes and positions
  the window once. `main` sets `NativeOptions::centered`, so eframe centers
  the window on the primary monitor while CREATING it — Windows otherwise
  opens it at the `CW_USEDEFAULT` cascade position (low and off to one side).
  On the first frame the app restores the geometry remembered in the
  encrypted preferences, or (first run) opens at `config::initial_window_size`
  — `WINDOW_INITIAL_FRACTION` (80 %) of the current monitor, clamped to
  `WINDOW_MIN_*`/`WINDOW_INITIAL_MAX_*` — centered on that monitor. The
  monitor list and the window rects come from the winit window eframe exposes
  (`Frame::winit_window`, inherent methods only: no direct `winit`
  dependency). A stored position that no longer overlaps any attached monitor
  (unplugged screen) falls back to centering, and the size is clamped to the
  monitor it opens on. Later moves/resizes are persisted (encrypted
  `settings.bin` key `window`, debounced by
  `WINDOW_GEOMETRY_SAVE_DEBOUNCE_SECS` and flushed on exit), so the next
  launch reopens where the user left the window; while maximized only the flag
  is updated (the stored rect stays the last WINDOWED one, so un-maximizing
  restores it). The geometry is kept in PHYSICAL pixels and converted to
  native-scale points only for the viewport commands: egui applies a
  `set_zoom_factor` only at the start of the NEXT pass, so the first frame's
  commands run at the default zoom, and physical pixels survive a DPI or zoom
  change without drifting (a window remembered on a monitor at a different
  scale reopens at the same physical size).
- Address typing is tracked with `Tab::address_dirty` (set by the fields on
  `changed()`), so `poll()` never clobbers user edits.
- Ids: tables/code-scroll/document-scroll salted by `doc_id_salt(base_url)`
  MIXED WITH THE TAB ID (egui ids are global, so two tabs on the same
  document would otherwise share scroll/table/code state), while staying
  bounded. The document SCROLL salt additionally excludes the per-load
  generation: the offset belongs to the (tab, URL) pair, so Back/Forward and
  refresh return to the reading position while a different document still
  starts at the top. A fresh navigation to an already-seen document shares
  that salt, so it forces the offset to the top instead
  (`Tab::scroll_top_requested`, consumed by the first `Loaded` frame);
  failed or abandoned loads clear it so the restored page keeps its own
  position. Jobs registered during `ui` (image `ensure`, favicon
  probes) schedule their own 120ms repaint tick, so a loading placeholder
  never waits for the next input event.
- Find in page: the render walks the SAME events the viewer paints and
  assigns match ORDINALS in document order (`viewer::find::FindRender`
  `claim`, capped at `MAX_FIND_MATCHES`, per fragment as well as globally),
  so highlight, the bar's `n/m` count and Prev/Next share one source of
  truth. A searched fragment keeps the EXACT layout of the unsearched one:
  prose/headings lay out ONE UNSHAKEN `LayoutJob` (a single section, so the
  shaping run and every advance match the unsearched render) and the wash is
  painted as RECTANGLES behind the glyphs (`range_row_rects`, one band per
  galley row a match touches, the cursor math egui's own selection painting
  uses); links/inline code stay single widgets, and fenced blocks claim their
  cached body and wash matching rows without touching the galley cache. Both
  earlier shapes moved the page and are documented at `render_plain_split`:
  one widget per piece re-wrapped each piece on its own, and one job with
  per-match SECTIONS lost the kerning across boundaries (~1px over a long
  line, enough to push a borderline word to the next row). A fragment holding
  color emoji never takes the glyph split (a galley cannot draw a texture):
  it keeps the emoji COLOR path with the wash painted behind the textures,
  highlighted whole. Fenced blocks use the stronger CURRENT wash on the row
  that owns the current ordinal too (it is found once per frame by counting
  occurrences line by line, early-exiting past the target). The CURRENT
  match's painted rect is recorded on the
  frame's cursor (the union of its row bands, `range_row_rects`) and
  its `scroll_to_rect` request is RETRIED until that
  rect shows up in the sheet's clip rect: egui's `ScrollArea` DROPS a scroll
  target on any frame that also sees scroll input (the wheel's smoothing
  tail keeps one alive for a few frames), so consuming the request on the
  first frame lost it. A match inside a code card or a table cell sits under
  a NESTED horizontal `ScrollArea`, which consumes the frame's target and
  applies only its own axis: the retry re-issues the request from the outer
  sheet, whose `ScrollArea` ends after the walk, so the document scrolls to
  such a row too. New wheel/drag input cancels the retry for good (the
  user's scroll wins). The bar's own focus request is deferred past the
  first frame too: an `Area`'s first frame is an invisible sizing pass with
  a DISABLED `Ui`, where a `TextEdit` cannot take (and would surrender)
  focus. `Esc` closes the bar only when no other surface owns the press:
  `app::shortcuts` records a focused field, the page-info card, an open
  popup/menu or a floating window in `FindState::esc_elsewhere` before the
  viewer draws, so one press never closes the bar AND acts on the surface
  behind it.
- Cancellation: every fetch checks the flag per candidate and, during the
  body, races each chunk wait (`futures_util::select`) against it, so a
  superseded navigation drops in ~20ms even mid-stall (not after the 15s
  `CHUNK_TIMEOUT_SECS`).
- Shared anti-abuse bounds live in `config/` (`MAX_SETTINGS_BYTES`, the
  code/table render-cache byte budgets, the font-asset gzip bound and
  `LOCAL_READ_CHUNK_BYTES`); the structural/MDX scan-line budgets live
  beside their passes in `markdown::document`, the store repair-lock
  timeouts in `store::crypto`, and the display helpers in `util.rs`. The
  `image::ImgCtx` parameter bundle (the five inputs shared by all four
  image entry points, built per call from the walk context) lives beside
  its owner in `viewer::image`.
  Window geometry constants (`WINDOW_INITIAL_*`/`WINDOW_INITIAL_FRACTION`/
  `WINDOW_INITIAL_MAX_*`/`WINDOW_MIN_*` and the save debounce) live in
  `config/` (not as magic numbers in `main`/`app::window`), and every
  wall-clock deadline goes through
  `config::deadline_after` (fail-closed `checked_add`: the unreachable
  overflow expires immediately instead of panicking the thread).

## Performance

- Reused global HTTP client (TLS/HTTP2 pool, no proxy/cookies/cache).
- Transparent `gzip`/`brotli`/`deflate` decompression: markdown and error
  bodies shrink several times over the wire; the anti-abuse caps run on the
  decompressed stream.
- One background thread per navigation polling a shared, long-lived
  multi-thread `tokio` runtime (`NETWORK_WORKER_THREADS`, 4 workers) so the
  HTTP connection pool keeps its executor between jobs; the UI never blocks.
- Early `Content-Length` cutoff + capped chunked reads, with a 15s
  per-chunk stall timeout.
- Images download + decode on background threads with a process-wide limit
  (`MAX_CONCURRENT_IMAGES`, separate from document fetches) and upload once
  as textures; the renderer never blocks and placeholders animate. Local
  file reads use their own limiter (`MAX_CONCURRENT_LOCAL_READS`), so a slow
  filesystem and the network never queue behind each other.
- No continuous repaint: only while loading (60ms ticks for the omnibox
  progress sweep, 100/300ms for the fetch poll), while images/favicons are
  pending, during the 500ms cooldown ticks (counter), or on interaction.
  The document is parsed to owned events once per fetch and copy text and
  the first-H1 title fallback are derived there too; repaints (scroll,
  selection) never re-run the parser
  nor copy the document text, and code blocks reuse a bounded
  per-document body/span cache. Jobs registered mid-frame also schedule
  their own 120ms tick, so loading placeholders never wait for the next
  input event. Per-frame allocation is kept down on purpose: egui id salts
  are tuples (no `format!` per document/block/table/error-scroll/menu/toast),
  link hitboxes hold interned `Arc<str>` handles (a link split into many
  fragments shares one allocation instead of copying the URL per fragment
  per frame), unordered list bullets are borrowed (`Cow::Borrowed`, only the
  ordered prefix allocates), context-menu shortcut hints are `&'static str`,
  the history page compares the raw filter text and rebuilds the lowercase
  needle only on a cache miss (the same cache carries the rows' date sections
  and their flat offsets, so grouping costs no per-frame walk), error-view
  hints are borrowed `Cow`s, the
  code-block language chip avoids the lowercase allocation when already
  lowercase, the quote-alert look-ahead and `detect_callout` read only the
  marker-length prefix
  of each text run (never the whole quoted paragraph; `detect_callout` used
  to uppercase a whole up-to-512-KiB run per quoted paragraph per frame), the image-alt and
  flattened-subtree collectors stop at the per-block char cap before
  copying a fragment, `cap_block_text` counts chars only up to the cap
  instead of rescanning an oversized fragment, and each cached code block
  stores its line total once instead of rescanning the body per frame,
  the tab rail reads the per-tab display memo (`Tab::display`) instead of
  allocating a title and parsing the asked URL per tab per frame (and
  requests favicon probes from that borrowed memo, so a settled frame does
  not build a `Vec` of cloned origins), bookmark
  tiles resolve favicons by URL string (`FaviconCache::texture_for_url`)
  instead of a `Url::parse` per tile per frame — with the cache's borrowed
  key slice (`String: Borrow<str>`) every lookup, tile or tab, is one hash
  probe with no `String` allocation and no parse, and parsing happens on
  the first frame of an unknown origin alone — the palette
  (`theme::scheme_ref`, borrowed per surface; the former owned
  `theme::scheme_for` copy is removed, every caller borrows) is interned per theme instead of rebuilt per element, the OS window title comes from the same per-tab
  memo (`Tab::window_title_cached`, compared as a borrowed `&str`);
  the omnibox suggestion cache is rebuilt only while its field is editing
  (so Home never runs the doubly-scheduled 20 000-entry scan per keystroke)
  and the scan itself compares ASCII in place
  (`util::contains_ignore_ascii_case`) instead of allocating a lowercase
  copy per candidate per keystroke; bookmark tiles and history rows borrow
  their titles (`Cow`) and the tile avatar returns a stack-encoded `char`;
  the image cache keeps an incremental in-flight
  count (`ImageCache::loading_count`) instead of rescanning its entries
  per frame while the queue drains, the emoji LRU evicts in one pass per
  victim, preference/session writes queue their `fsync` on dedicated
  background workers (the UI thread never `fsync`s), and
  tooltips/accesskit labels (icon buttons and their lazy `Reload` countdown,
  tab close buttons, color
  swatches, the Preferences data-directory path, both address fields and
  the history search field) are built lazily only
  while hovered or announced (`ui::hover_tooltip_lazy` for `format!`-ed
  text). Painted-per-frame labels are memoized instead: the find bar's
  `n/m` count on `(current, total, capped)` in `FindState`, the toolbar
  zoom readout in a thread-local buffer keyed by the rounded percent, the
  history header/footer captions on `(total, retention)` / `(total, shown)`
  with a shared singular/plural rule Preferences reuses, and the code Copy
  label lazily for both the accesskit node and the hover card; tab
  titles are borrowed (no per-row clone) with a gated tooltip, the code
  match wash consumes the visible-line iterator inline (no `Vec` per block
  per frame), `contains_match` gates every range walk allocation-free on
  the ASCII path, inline code skips its wrap-measure probe on fresh rows, and
  the component close-tag scan compares allocation-free (`is_close_tag`,
  also in `scan_pairs`).
- Settled tables cost NO byte scan per frame: the content fingerprint memo
  is keyed by the table's `Start` event index (stable per document) instead
  of re-validating itself with a probe over the whole table text, and a
  table's overflow/lane decision compares against the width the inner
  `ScrollArea` really sees. A code block's widest line is computed when its
  galleys are built (not from the visible rows each frame) and the plain
  path keeps a monotonic widest-row memory, so the
  horizontal scroll range cannot shrink while a long line is off screen;
  its token spans are tokenized on first need, and the error view's
  scheme-stripping helper borrows (`Cow`) when nothing is stripped.
- One parser pass per document: the clipboard text is derived from the
  owned events (`plain_text_from_events`), and the structural `:::`/MDX
  passes run under per-document line budgets (`MAX_STRUCTURAL_SCAN_LINES`,
  `MAX_MARKUP_SCAN_LINES`), with `strip_line` never rescanning a line for a
  closer it cannot find. Unmatched openers (a body of `:::tabs` or `<Tabs>`
  lines without closers) stay literal instead of costing O(n²) on the fetch
  thread.
- Settled O(1) paths: `ImageCache::poll` returns immediately when nothing is
  queued or loading, `settings` loads the shared `store.key` once per
  process and seeds its body cache from the startup validation, and
  `history` maintains its plaintext byte total incrementally so a recorded
  visit never rescans the store.
- Slot accounting is leak-free: `try_acquire` increments once and the guard
  decrements on `Drop`, and the guard is constructed ONLY on success
  (`bool::then`, never `then_some`), so a cancelled or timed-out wait cannot
  drive the process-wide counters below zero — that underflow used to make
  every later load wait out the slot timeout and report "busy".

