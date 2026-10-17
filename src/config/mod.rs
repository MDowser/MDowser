// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `config`: global browser constants.
//!
//! - Centralizes timeouts, headers and limits (documents, images, history).
//! - Everything in one place for easy protocol/anti-abuse auditing.

/// Exact `Accept` header we send: only `text/markdown`, q=0.8.
/// - We never ask for HTML or any other type.
/// - `q=0.8` states moderate preference; the server may ignore it,
///   which is why we strictly validate the answer (see `protocol::content_type`).
pub const ACCEPT_HEADER_VALUE: &str = "text/markdown;q=0.8";

/// `Accept` header name.
pub const ACCEPT_HEADER_NAME: &str = "accept";

/// Defined User-Agent for this browser (sent on every request).
/// Minimal and stable: platform-independent for reproducibility.
/// Version comes from `Cargo.toml` so dist binaries report themselves correctly.
pub const USER_AGENT: &str = concat!(
    "MDowser/",
    env!("CARGO_PKG_VERSION"),
    " (CommonMark; +https)"
);

/// Content types that IDENTIFY markdown and are accepted for documents.
/// - `text/markdown` is the standard; the `x-` prefixed forms and the
///   `application/*` spellings are the aliases real servers send.
/// - `text/plain` is deliberately NOT here: a plain-text response does not
///   identify a markdown document, so it never reaches the parser.
/// - Case-insensitive comparison and the UTF-8 charset rule live in
///   `protocol::content_type::is_markdown_content_type`.
pub const MARKDOWN_CONTENT_TYPES: &[&str] = &[
    "text/markdown",
    "text/x-markdown",
    "application/markdown",
    "application/x-markdown",
];

/// Favicon probe paths, in order, relative to the origin root.
/// - The two conventional locations first, then the modern SVG icon: many
///   sites only publish an SVG favicon.
/// - Each probe is bounded (`MAX_FAVICON_BYTES`) and failures are cheap.
pub const FAVICON_PATHS: [&str; 3] = ["favicon.ico", "favicon.png", "favicon.svg"];

/// Conventional directory indexes, in probe order: the standard spellings
/// only (`index.md`, `README.md`).
/// - Shared by the HTTPS resolver (`protocol::resolver`) and the local-file
///   reader (`protocol::local`): the two walked the same list from two
///   copies, so a local and a remote page of the same shape could disagree
///   after one was edited.
pub const DIR_INDEXES: [&str; 2] = ["index.md", "README.md"];

/// Total timeout per document download attempt (seconds).
/// - Each route candidate (`/love`, `/love.md`, ...) gets its own timeout.
/// - Sized for the 10 MiB body cap (`MAX_BODY_BYTES`) at ~4.2 Mbps, the
///   same "slow link" floor the old 5 MiB/10s pair encoded, while the
///   `MAX_ROUTE_CANDIDATES`-step cascade still fits `TOTAL_FETCH_TIMEOUT_SECS`.
pub const REQUEST_TIMEOUT_SECS: u64 = 20;

/// Total timeout per IMAGE download (seconds).
/// - Images are binary and legitimately larger than markdown: the 16 MiB
///   cap at ~2.2 Mbps already takes 60s, so the document budget would abort
///   big images on slow links even though they are still transferring.
/// - The per-chunk stall timeout (`CHUNK_TIMEOUT_SECS`) still aborts a
///   connection that stops making progress, and the running byte cap
///   still bounds the body.
pub const IMAGE_REQUEST_TIMEOUT_SECS: u64 = 60;

/// Total timeout per FAVICON probe (seconds).
/// - Each origin probes up to 3 paths SEQUENTIALLY (`FAVICON_PATHS`), all
///   sharing one image slot: at the image budget (60s each) a dead host
///   could pin a slot for 180s, past the 80s visual deadline and starving
///   document images. Icons are tiny; 15s is generous for a probe.
/// - Keeps `FAVICON_PATHS.len() * FAVICON_REQUEST_TIMEOUT_SECS` (45s)
///   safely below `IMAGE_JOB_TIMEOUT_SECS` (80s).
pub const FAVICON_REQUEST_TIMEOUT_SECS: u64 = 15;

/// Wall-clock age after which a queued/loading image entry is declared
/// failed and its loading placeholder clears (fetch + decode + upload).
/// - Decoders cannot be interrupted mid-call (`resvg`/`image` offer no
///   cancellation), but past this age the UI stops waiting and counting the
///   entry as pending, so a pathological image cannot spin forever or keep
///   the 120 ms repaint loop alive.
/// - The cooperative cancel flag is set too, which aborts a fetch still in
///   flight; a stuck decode finishes on its own and its result is ignored.
pub const IMAGE_JOB_TIMEOUT_SECS: u64 = IMAGE_REQUEST_TIMEOUT_SECS + 20;

/// Wall-clock budget a freshly fetched document waits for its images before
/// it is shown (seconds).
/// - The fetch already resolved, so the previous page (when there is one)
///   stays on screen with the loading indicator while the new document's
///   images settle in a separate cache: the swap is atomic, so the reader
///   never sees placeholder-to-bitmap reflow.
/// - A first load without a backdrop keeps the stable empty panel for the
///   same window instead of flashing half-loaded content.
/// - Past this age the page shows anyway (placeholders for whatever is still
///   pending): a slow gallery must not pin the old page forever. Text-only
///   pages settle instantly (nothing to wait for).
pub const DOC_IMAGE_GATE_SECS: u64 = 5;

/// Total wall-clock budget for the whole fallback cascade (seconds).
/// - `poll()` uses it as a visual timeout: a `Loading` older than this
///   becomes `Failed` instead of hanging forever (hung thread / panic).
/// - Derived from the worst-case path so the UI never abandons a load that
///   could still legitimately finish: waiting for a global slot
///   (`FETCH_SLOT_WAIT_SECS`) + every route candidate
///   (`MAX_ROUTE_CANDIDATES`) x `REQUEST_TIMEOUT_SECS` + 10s of scheduling
///   slack.
pub const TOTAL_FETCH_TIMEOUT_SECS: u64 =
    FETCH_SLOT_WAIT_SECS + MAX_ROUTE_CANDIDATES as u64 * REQUEST_TIMEOUT_SECS + 10;

/// Time the "Refresh" button stays disabled after each load.
/// - Stops compulsive F5 from hammering servers (spec requirement).
/// - Also applies right after navigating (see `browser::tab`).
pub const REFRESH_COOLDOWN_SECS: u64 = 5;

/// Redirect limit followed per attempt.
/// - `reqwest` follows them automatically; we cap it to avoid loops.
/// - 8 covers the long-but-legitimate www/canonical/CDN chains.
pub const MAX_REDIRECTS: usize = 8;

/// Max `Content-Type` header values inspected per response.
/// Hostile servers control the count; past this the response is rejected
/// instead of letting an unbounded header list grow the per-request `Vec`.
pub const MAX_CONTENT_TYPE_VALUES: usize = 8;

/// Max accepted document size (bytes).
/// - Guards against giant responses; Markdown is lightweight text.
/// - When exceeded, the attempt is dropped and the next route is tried.
/// - 10 MiB is generous for CommonMark without images.
pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Images (markdown `![alt](url)` subresources + site favicons)
// ---------------------------------------------------------------------------

/// Image formats the decoder accepts. Kept in sync with `media::decode`:
/// PNG, JPEG, GIF, WebP (via `image`) + JPEG XL + AVIF + static SVG,
/// plus ICO for favicons. SVG is rasterized with scripts/foreign content
/// disabled (see `media::decode`); HTML is NEVER here.
/// Legacy aliases for supported formats (`image/x-png`, `image/pjpeg`) are
/// accepted too: old servers label PNG/JPEG that way and the magic sniff
/// still validates the real bytes before decoding.
pub const IMAGE_CONTENT_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    // Legacy aliases of the two above (older server spellings).
    "image/x-png",
    "image/pjpeg",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/jxl",
    // Static vector images (badges, logos, icons).
    "image/svg+xml",
    "image/svg",
    // Favicons only, but harmless for inline images too.
    "image/x-icon",
    "image/vnd.microsoft.icon",
];

/// `Accept` header sent for markdown image subresources.
pub const IMAGE_ACCEPT_HEADER_VALUE: &str = "image/png,image/jpeg,image/webp,image/avif,image/jxl,image/gif,image/svg+xml,image/x-icon;q=0.9,*/*;q=0.1";

/// `Accept` header sent for `/favicon.ico` / `/favicon.png` probes.
pub const FAVICON_ACCEPT_HEADER_VALUE: &str =
    "image/x-icon,image/png,image/webp,image/gif,image/jpeg;q=0.9,*/*;q=0.1";

/// Max accepted size of one image subresource (bytes).
/// - Larger than the markdown cap: photos are binary and legitimately bigger.
///   16 MiB covers camera/DSLR exports before the built-in downscale.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Max accepted size of a favicon (bytes). Icons are tiny; 1 MiB is already
/// absurdly generous for one.
pub const MAX_FAVICON_BYTES: usize = 1024 * 1024;

/// Max width/height per side of a decoded FAVICON texture.
/// - Favicons draw at the toolbar icon size (~27 px); decoding to
///   `MAX_TEXTURE_DIM` (3072) would let `MAX_FAVICON_HOSTS` icons approach
///   multiple GiB of VRAM outside the per-document budgets
///   (`media::favicon` also reserves its bytes in the process-wide texture
///   budget).
/// - 160 px covers the widest drawn size: the ~27 pt glyph at the 175%
///   toolbar zoom and a 300% display scale lands at ~143 device px, while
///   bounding each icon to 100 KiB RGBA.
pub const MAX_FAVICON_DIM: u32 = 160;

/// Max decoded pixels per image (anti decompression-bomb).
/// 48 MP covers full-frame/medium-format photos and 8K frames; larger
/// images become an error placeholder.
pub const MAX_IMAGE_PIXELS: u64 = 48 * 1024 * 1024;

/// Max width/height per side before decoding (anti decompression-bomb).
/// - 12288 admits high-res phone photos and panoramas; the 48 MP total cap
///   still rejects anything larger.
pub const MAX_IMAGE_DIM: u32 = 12288;

/// Longest side uploaded to the GPU. Larger images are downscaled with a
/// fast filter first: the reading column is at most 760 logical px, so even
/// at the 175% zoom a full-res upload beyond 3072 px would only waste VRAM
/// (4 bytes per pixel).
pub const MAX_TEXTURE_DIM: u32 = 3072;

/// Max concurrent image/favicon downloads+decodes (process-wide).
/// - Separate from `MAX_CONCURRENT_FETCHES` so document loads never queue
///   behind a gallery of images.
pub const MAX_CONCURRENT_IMAGES: usize = 6;

/// Max concurrent image jobs for ONE document.
/// - Without this, a single gallery (up to `MAX_IMAGES_PER_DOC` URLs) holds
///   every global slot until it drains: other tabs' images and favicon
///   probes sit on placeholders meanwhile. Three per document keeps the
///   shared slots fairly distributed; the jobs of one document simply
///   pipeline three at a time.
pub const MAX_CONCURRENT_IMAGES_PER_DOC: usize = 3;

/// Max image entries registered per document (anti-abuse: a markdown file
/// could reference thousands of URLs). Extra images render as placeholders.
pub const MAX_IMAGES_PER_DOC: usize = 512;

/// Automatic retries for one image subresource after a TRANSIENT failure
/// (network error, stall timeout, or a decode error likely caused by a
/// truncated transfer). Content-type rejections and cancellations are never
/// retried: those bytes will not change.
/// - Deliberately ONE retry: every retry re-downloads, and the job deadline
///   is re-armed per attempt, so a higher count would pin image slots for
///   minutes on a host that stalls without ever finishing.
pub const MAX_IMAGE_RETRIES: u8 = 1;

/// Backoff before a decoded image (or favicon) retries its GPU upload
/// after the process-wide texture budget refused its bytes.
/// - The budget is TRANSIENT (other documents release their share on
///   navigation), so a failed reservation is not a final verdict; this
///   bounds the re-decode rate while VRAM stays tight.
pub const IMAGE_BUDGET_RETRY_SECS: u64 = 2;

/// Max consecutive budget backoffs for ONE image before its placeholder
/// becomes a permanent failure. With `IMAGE_BUDGET_RETRY_SECS` this keeps
/// the retry window at ~16s: past it the entry fails like any other,
/// instead of re-decoding a large image every couple of seconds forever.
pub const MAX_IMAGE_BUDGET_RETRIES: u8 = 8;

/// Max favicon origins cached per session (hosts). Bounded like everything.
/// - 512 icons at `MAX_FAVICON_DIM` (100 KiB each) is at most 50 MiB
///   overall, well inside `MAX_TEXTURE_BYTES_TOTAL`, and far past any
///   session's working set.
pub const MAX_FAVICON_HOSTS: usize = 512;

/// Color emoji (`media::emoji`): Noto Color Emoji ships its glyphs as SVG
/// documents, rasterized per (cluster, size). The square side drawn for one
/// emoji, in multiples of the font size.
pub const EMOJI_SCALE: f32 = 1.15;

/// Emoji texture side (device px) is clamped and rounded up to a bucket so
/// zoom levels share cached textures.
pub const EMOJI_TEXTURE_PX_MIN: u32 = 16;
/// Upper bound per side: emoji draw at reading size, larger is wasted VRAM.
pub const EMOJI_TEXTURE_PX_MAX: u32 = 256;
/// Bucket granularity for the VIEWER's text-emoji textures (device px);
/// bookmark-tile glyphs (`ui::icons::paint_text`) use a finer 2 px grid.
pub const EMOJI_TEXTURE_PX_BUCKET: u32 = 8;

/// Max emoji textures kept per cache (clusters x sizes) and their byte
/// budget: each tab's document emoji and the shared `paint_text` cache
/// bound themselves with these. The bytes are reserved from the process-wide
/// texture pool (`media::try_reserve_texture_bytes`) and released on
/// eviction, so zooming cannot grow VRAM without bound.
pub const EMOJI_CACHE_ENTRIES: usize = 1024;
/// Bytes of the process-wide texture budget each emoji cache may hold.
pub const EMOJI_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Retry delay after a color-emoji texture could not reserve its bytes from
/// the process-wide budget (document images can saturate the pool). A
/// permanently cached miss would keep emoji monochrome for the whole
/// session; retrying every frame would re-rasterize the cluster. This bounds
/// both: the miss is remembered for this long, then retried.
pub const EMOJI_BUDGET_RETRY_SECS: u64 = 2;

/// Max concurrent color-emoji raster jobs (process-wide).
/// - A texture miss rasterizes on a background thread (see
///   `media::emoji::EmojiCache`), and a document (or a zoom/DPI change) that
///   misses on many clusters in one frame must not spawn one thread per
///   cluster: over the cap the miss stays unregistered and a later frame
///   retries it.
#[cfg_attr(test, allow(dead_code))]
pub const MAX_CONCURRENT_EMOJI: usize = 4;

/// GPU budget per document for uploaded image textures (bytes).
/// - A malicious page can reference `MAX_IMAGES_PER_DOC` huge images; without
///   a budget the process could try to upload gigabytes of VRAM.
/// - Images that would exceed the budget fail with a visible placeholder
///   instead of being uploaded.
pub const MAX_TEXTURE_BYTES: usize = 384 * 1024 * 1024;

/// Process-wide budget for uploaded image textures across ALL tabs (bytes).
/// - The per-document budget alone is not enough: `MAX_TABS` documents can
///   each keep their textures alive, so worst-case VRAM is
///   `MAX_TABS * MAX_TEXTURE_BYTES`. This global cap bounds the process.
/// - Reservations are released when a document's images are dropped
///   (navigation, tab close) or when the cache itself is dropped.
pub const MAX_TEXTURE_BYTES_TOTAL: usize = 768 * 1024 * 1024;

/// Max allocation (bytes) granted to a raster decoder while producing
/// decoded pixels. 48 MP RGBA is ~192 MiB; the rest is decoder scratch.
/// Generous enough for the biggest allowed image, well below the texture
/// budgets (decoded pixels are later downscaled before upload).
/// Worst case with every image slot decoding at once is bounded at
/// `MAX_CONCURRENT_IMAGES * MAX_DECODER_ALLOC_BYTES` (~3 GiB) of TRANSIENT
/// decoder memory; most scratch is freed before the texture upload.
/// Kept in `config` like every other anti-abuse bound.
/// `u64` because `image::Limits::max_alloc` takes `u64`.
pub const MAX_DECODER_ALLOC_BYTES: u64 = 512 * 1024 * 1024;

/// Max `<` characters accepted in one SVG source before it is rejected, and
/// max nodes the memoized `<use>` expansion may build (see
/// `media::decode::check_svg_structure`).
/// - `usvg` has no LOW node limit: a 10 MiB SVG full of elements could pin a
///   decode slot (and its threads) for minutes, and nested `<use>` references
///   expand exponentially from a few hundred source bytes before usvg's own
///   1M-node cutoff is reached. The structural pass bounds both.
/// - Real icons, logos and badges are orders of magnitude smaller, so this
///   only rejects pathological input.
///
/// SVG is additionally pre-scanned before `usvg` sees it: `<!DOCTYPE>` /
/// `<!ENTITY>` (roxmltree expands internal entities without a total-size
/// bound -> OOM) and `filter` / `fe*` / `marker` elements (filter parameters
/// such as `numOctaves` are unbounded and burn CPU per pixel; a marker's
/// subtree is re-converted per path vertex) are rejected.
pub const MAX_SVG_TOKENS: usize = 400_000;

/// Max element nesting depth accepted in one SVG source.
/// - `usvg` converts nested groups RECURSIVELY on the decode thread (2 MiB
///   stack): the token cap alone still allowed ~100k levels of `<g>` in one
///   small source, which overflows the stack (an uncatchable process abort)
///   before any size cap could trip. Real icons/logos nest a few dozen
///   levels at most; 512 is orders of magnitude above any legitimate
///   document.
pub const MAX_SVG_DEPTH: usize = 512;

/// Max number of `pulldown-cmark` events accepted for one document.
/// - A 10 MiB body of one-character paragraphs expands to ~16M events, each
///   `Event` carrying a `Tag` of up to ~80 bytes: several hundred MB to over
///   a GB on a background thread that a hostile `text/markdown` server fully
///   controls.
/// - Real documents stay far below this (a dense 1 MiB page is ~100k
///   events); the route is rejected with a clear line when exceeded.
pub const MAX_EVENTS_PER_DOC: usize = 1_000_000;

/// Max pipes accepted in ONE line before the table machinery is considered
/// hostile.
/// - `pulldown-cmark`'s first pass is EAGER: a 10 MiB header line of `|`s
///   builds one tree item per cell before `MAX_EVENTS_PER_DOC` can stop the
///   loop, and the delimiter-padding pass would first build a ~30 MB string
///   to match it. Rejected with a clear line before the parser runs.
/// - Real tables have a handful of columns; even dense ones stay far below
///   this bound.
pub const MAX_TABLE_COLS: usize = 4_096;

/// Max table ROWS collected and rendered per table.
/// - A table is ONE rendered block: without a row cap a ~400 KiB body can
///   hold ~100k rows under the event cap as a single block, and the
///   non-virtualized grid would lay out every row every frame. Past the cap
///   the remaining rows are consumed without collecting cells and the
///   truncation notice is shown (`Copy` keeps the full text). Every
///   collected row also spends one unit of `MAX_RENDERED_BLOCKS`, so rows
///   across many tables cannot bypass the per-frame block budget either.
pub const MAX_TABLE_ROWS: usize = 5_000;

/// Max table CELLS collected and rendered per document per frame (header
/// rows included).
/// - `MAX_TABLE_ROWS` charges one block per row, but a single row may hold
///   up to `MAX_TABLE_COLS` columns: 5 000 rows x 4 096 columns is ~20M
///   cells laid out every frame while the row budget sees only 5 000 units.
///   Each cell builds a widget subtree, so the cell count is what must be
///   bounded. Past the cap the remaining cells are consumed WITHOUT
///   collecting fragments (no layout, no widgets; `Copy` keeps the text)
///   and the table ends with the usual truncation notice.
pub const MAX_RENDERED_TABLE_CELLS: usize = 20_000;

/// Max bare URLs (`www.…`, `http(s)://…`) linkified per document by
/// `markdown::autolink`.
/// - Each autolink adds two events (a `Link` Start/End pair) around text that
///   was already there, so a hostile body repeating `www.a.co` would grow
///   the parsed event vector past `MAX_EVENTS_PER_DOC`; extra candidates
///   stay plain text (never lost).
/// - Real documents have far fewer bare URLs.
pub const MAX_AUTOLINKS_PER_DOC: usize = 20_000;

/// Max chars of a recognized bare-URL domain (roughly the DNS limit).
/// - The scanner must not treat a megabytes-long run of domain characters as
///   one host: past this the candidate is rejected and its first bytes are
///   not rescanned.
pub const AUTOLINK_MAX_DOMAIN_CHARS: usize = 253;

/// Max code-block lines RENDERED per document (highlighted or plain).
/// - A fenced block is one `Event::Text`: the event cap does not bound the
///   number of visual lines. Rendering millions of labels per frame would
///   freeze the UI on a few MB of newlines.
/// - Blocks beyond the budget render a one-line "hidden" notice; `Copy`
///   still copies the full body.
pub const MAX_RENDERED_CODE_LINES: usize = 10_000;

/// Max BLOCKS (paragraphs, headings, items, code cards, tables, quotes...)
/// rendered per document per frame.
/// - `MAX_EVENTS_PER_DOC` bounds parsing memory, not per-frame work: a
///   ~1.4 MB body of `x\n\n` repeated is ~1M events, and laying out every
///   block every frame freezes the UI. Real documents are far below this;
///   beyond it the walk consumes the remaining blocks without rendering
///   them (no layout, no widgets) and shows one truncation notice, while
///   `Copy` still yields the full text.
pub const MAX_RENDERED_BLOCKS: usize = 10_000;

/// Max characters of one rendered prose/HTML/table block (paragraph,
/// heading, HTML block, table cell).
/// - A single 10 MiB paragraph is one egui label: its layout job is rebuilt
///   and laid out every frame and the galley stays resident. Real documents
///   are orders of magnitude smaller; past the cap the block ends with a
///   bounded `… [truncated]` notice while `Copy` keeps the full body.
pub const MAX_RENDERED_BLOCK_CHARS: usize = 512 * 1024;

/// Max approximate bytes retained by the code-block galley cache across all
/// blocks.
/// - A body alone can reach `MAX_BODY_BYTES` (10 MiB); a handful of giant
///   blocks would otherwise sit in RAM for the process lifetime.
/// - The entry-count cap stays tied to `MAX_RENDERED_BLOCKS` at the call
///   site; this byte budget is the real memory bound.
pub const MAX_CODE_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// Max approximate bytes retained by the table-widths cache across all
/// cached tables (same contract as [`MAX_CODE_CACHE_BYTES`]).
pub const MAX_TABLE_WIDTHS_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// Max bytes a gzip-compressed embedded font asset may expand to.
/// - The asset is trusted (shipped in the binary), but a corrupt table must
///   never expand without limit.
pub const MAX_FONT_ASSET_GZIP_BYTES: u64 = 64 * 1024 * 1024;

/// Upper bound on the settings file read before it is rewritten from
/// scratch (a corrupt/hostile local file must not be loaded whole).
pub const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

/// Max link hitboxes collected per rendered document frame (right-click
/// "Open link in new tab").
/// - A link split into several inline fragments pushes one screen-rect
///   hitbox per rendered fragment; without a cap a hostile link farm could
///   retain one URL copy per rendered fragment on top of the fragment tree.
///   Past the cap the extra links stay clickable (navigation is never
///   affected), they just have no context-menu hitbox.
pub const MAX_LINK_HITS: usize = 4_096;

/// Max chars of an image `alt` shown as caption/tooltip, and of an image
/// `title` tooltip. Server/author-controlled; the parser does not bound them.
pub const MAX_IMAGE_ALT_CHARS: usize = 512;
pub const MAX_IMAGE_TITLE_CHARS: usize = 512;

/// Max recursion depth of the markdown block renderer. A hostile document
/// (`> > > …`, `- - - …`) nests arbitrarily; the renderer recurses on the UI
/// thread and a stack overflow aborts the process (uncatchable). Deeper
/// content renders flattened instead of recursing.
pub const MAX_MARKDOWN_RENDER_DEPTH: usize = 48;

/// Local-file read chunk (bytes): bounds the work between cooperative cancel
/// checks, shared by documents and image/favicon subresources.
pub const LOCAL_READ_CHUNK_BYTES: usize = 64 * 1024;

/// Huge code-block guard: above either bound the block renders as plain
/// selectable lines instead of per-line highlighted spans (tokenizing
/// megabytes per frame freezes the UI). `Copy` keeps the full body.
pub const MAX_CODE_HIGHLIGHT_LINES: usize = 4_000;
pub const MAX_CODE_HIGHLIGHT_BYTES: usize = 400_000;

// ---------------------------------------------------------------------------
// Content-shape heuristics (conventions the viewer applies to markdown that
// carries no structural information). Centralized here so every threshold is
// auditable and tunable in one place, like every other bound.
// ---------------------------------------------------------------------------

/// Minimum number of entries for a one-line paragraph to render as a menu.
pub const MENU_MIN_ITEMS: usize = 2;
/// Maximum number of entries for a one-line paragraph to render as a menu.
/// Longer runs are prose or a layout the menu row would distort.
pub const MENU_MAX_ITEMS: usize = 8;
/// Max chars of one menu entry (menu labels are short nav text).
pub const MENU_ITEM_MAX_CHARS: usize = 32;
/// Max words of one menu entry.
pub const MENU_ITEM_MAX_WORDS: usize = 4;
/// Max total chars of a menu line.
pub const MENU_TOTAL_MAX_CHARS: usize = 160;
/// Min chars of one link's text for a link-only paragraph to stack as a
/// card list (shorter runs keep their natural inline flow).
pub const LINK_STACK_MIN_TEXT_CHARS: usize = 40;
/// Max chars of a leaked accessibility skip-link label that may be dropped.
pub const SKIP_LINK_MAX_CHARS: usize = 64;
/// Max anchor targets (heading slugs + explicit ids + footnote names)
/// registered per document. Bounds the existence set for anchor links.
pub const MAX_ANCHORS_PER_DOC: usize = 20_000;
/// Max inline `[...](...)` spans the lax link-join pass
/// (`markdown::document::join_broken_links`) collects while scanning a
/// document. Bounds the transient `Vec` a hostile body can force before the
/// parser's own caps apply; extra spans are simply left untouched.
pub const MAX_LINK_JOINS: usize = 8_192;
/// Max open `[` brackets the link-join scanner tracks at once. A body with
/// deeper unmatched nesting is not joined (the pass stops pushing) instead
/// of growing the stack without bound.
pub const MAX_LINK_JOIN_DEPTH: usize = 2_048;
/// Max route candidates one logical page expands to (`/love`, `/love/`,
/// `/love.md`, `/love/index.md`, `/love/README.md`). Used to derive the
/// visual fetch budget; `resolver` tests keep it in sync with the cascade.
pub const MAX_ROUTE_CANDIDATES: usize = 5;

/// Max chars of a fenced code-block info string shown in the language chip.
/// - Server-controlled; the parser does not bound it.
pub const MAX_LANG_CHARS: usize = 64;

/// Max chars of a footnote label rendered inline.
/// - Server-controlled; `[^...]` labels arrive unbounded from the parser.
pub const MAX_FOOTNOTE_LABEL_CHARS: usize = 256;

/// Max chars of one MDX component label drawn in the document: a `<Tab>`
/// cell, an `<Accordion>` header or a `<Step>` title.
/// - Author-controlled and unbounded: a single 2 000-char label used to
///   allocate a cell thousands of px wide and push every later tab outside
///   the reading sheet (unreachable), or lay out a 512 KiB label per frame.
///   The full label stays in the hover tooltip and in `Copy`.
pub const MAX_COMPONENT_LABEL_CHARS: usize = 120;

/// Max history entries kept per tab (back + forward each).
/// - Bounds memory: URLs are tiny but unbounded `Vec<Url>` growth
///   over a long session is still a leak. Oldest entries are dropped.
pub const MAX_HISTORY_ENTRIES: usize = 250;

/// Max chars accepted in the `about:history` search field.
/// - The query is cloned into egui memory and lowercased per store change;
///   one URL's worth of text is more than any real filter needs.
pub const MAX_HISTORY_FILTER_CHARS: usize = 512;

/// Max chars of the in-page find query (`Ctrl/Cmd+F`).
/// - The query lives in per-tab state and is lowercased once per frame while
///   the bar is open; one line of text is more than any real search needs.
pub const MAX_FIND_QUERY_CHARS: usize = 256;

/// Max in-page find matches highlighted per document per frame.
/// - Each match splits one text fragment into extra widgets; without a cap a
///   one-character query on a large document would build tens of thousands of
///   labels per frame. Past the cap extra occurrences render normally and the
///   bar shows the capped total with a `+`.
pub const MAX_FIND_MATCHES: usize = 2_000;

// ---------------------------------------------------------------------------
// Local browsing history (encrypted at rest, see `history.rs`)
// ---------------------------------------------------------------------------

/// Default retention window for the persisted browsing history (days).
/// - 3 months, the documented default. The user can pick other periods,
///   `Forever`, or `Session` (forget on exit) in Preferences.
pub const HISTORY_DEFAULT_RETENTION_DAYS: u32 = 90;

/// Max visits kept in the local history store.
/// - Bounds memory and file size: a visit is a URL + optional title, but an
///   unbounded list would still grow forever. Oldest entries are dropped.
pub const MAX_HISTORY_STORE_ENTRIES: usize = 20_000;

/// Max serialized (plaintext) bytes of the history store.
/// - The entry cap alone does not bound bytes: URLs may be up to
///   `MAX_URL_INPUT_CHARS` each. Oldest entries are dropped until the
///   serialized body fits, so reads/writes stay small and predictable.
pub const MAX_HISTORY_STORE_BYTES: usize = 8 * 1024 * 1024;

/// Debounce before a dirty history store is written to disk (seconds).
/// - Navigations record at most one visit each; rewriting the (small) file
///   at most every couple of seconds keeps the UI thread free.
pub const HISTORY_SAVE_DEBOUNCE_SECS: u64 = 2;

/// How long a synchronous history flush waits for the background writer
/// before giving up (seconds).
/// - The writer does the file write + `fsync`; a hung disk (NFS/FUSE/
///   removable media) must not freeze a frame, "Delete history" or exit
///   forever. On timeout the caller keeps the worker and retries later
///   instead of writing a second blob behind its back (write ordering).
pub const HISTORY_SAVE_FLUSH_TIMEOUT_SECS: u64 = 5;

/// Max URL / address input length (chars, roughly bytes for ASCII URLs).
/// - Pasting megabytes into the address bar must not bloat `Tab` state,
///   logs or error views. Longer input is rejected as `BadUrl`.
/// - This is the cap on the RAW input a user can type/paste. Serialized
///   URLs are percent-encoded (one 4-byte UTF-8 char becomes 12 ASCII
///   characters), so derived candidates, display URLs and stored URLs are
///   bounded by [`MAX_URL_BYTES`] instead: capping them by chars silently
///   dropped valid, under-cap addresses.
pub const MAX_URL_INPUT_CHARS: usize = 8_192;

/// Max BYTES of a serialized/derived URL (candidates, display, stores).
/// - The worst-case percent-encoding of an input at `MAX_URL_INPUT_CHARS`:
///   `Url::to_string`/`Url::parse` write every non-ASCII byte as `%XX`, so a
///   capped input can never serialize past this bound, and a derived
///   candidate (`.md`/index expansion) is checked against it.
pub const MAX_URL_BYTES: usize = MAX_URL_INPUT_CHARS * 12;

/// Max bytes a `protocol::rewrite::DestCache` retains per document.
/// - A 10 MiB body may declare up to ~500k distinct destinations; without a
///   budget their resolved URLs would retain hundreds of MB (gigabytes with
///   a long base), long after the caps promised otherwise. Past the budget
///   the remaining destinations render as plain text / placeholders (the
///   same fail-safe as an unresolvable link), on top of the per-destination
///   `MAX_URL_INPUT_CHARS` limit.
pub const MAX_DEST_CACHE_BYTES: usize = 8 * 1024 * 1024;

/// Max YAML frontmatter size (bytes) passed to `serde_norway`.
/// - The body is already capped at 10 MiB, but a 10 MiB YAML mapping is a
///   parsing bomb. Larger frontmatter is treated as malformed so the
///   next route candidate is tried.
pub const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;

/// Max characters kept per log line in the error view.
/// - Servers control `Content-Type` values and redirect URLs; truncating
///   keeps the line-by-line log readable and bounded.
pub const MAX_LOG_LINE_CHARS: usize = 1_000;

/// Max characters kept per error line after server-controlled suffixes
/// (e.g. reqwest error text) are appended. Slightly larger than
/// `MAX_LOG_LINE_CHARS` so truncation never hides the URL prefix.
pub const MAX_ERROR_LINE_CHARS: usize = MAX_LOG_LINE_CHARS + 200;

/// Max chars kept for the address bar display after a rejected oversized
/// input. Small enough to keep `Tab` state bounded, large enough for context.
pub const MAX_ADDRESS_DISPLAY_CHARS: usize = 256;

/// Max chars kept for the `asked` echo on the error view after a rejected
/// oversized input. Larger than the address display so bug reports keep context.
pub const MAX_ASKED_DISPLAY_CHARS: usize = 512;

/// Max chars kept for a sanitized document title: the frontmatter `title`
/// or the first-H1 fallback. Defense in depth: callers also truncate for
/// tab (30) / window (60) display, but the helper itself never returns an
/// unbounded string from remote YAML or heading text.
pub const MAX_TITLE_CHARS: usize = 256;

/// Per-chunk stall timeout (seconds) while streaming the body.
/// - `reqwest`'s request timeout covers connect + full response, but a
///   trickling server could otherwise hold `bytes_stream().next()` open
///   indefinitely, pinning a background thread past the visual timeout.
/// - Each stalled chunk aborts the attempt so the next route is tried.
pub const CHUNK_TIMEOUT_SECS: u64 = 15;

/// Max concurrent background fetches across all tabs.
/// - Single-flight per tab already holds, but many tabs navigating at once
///   would spawn one thread each with up to a 10 MiB buffer + tokio runtime.
/// - `fetch_blocking` blocks on a slot (with cancel + timeout) instead of
///   spawning unbounded network load.
pub const MAX_CONCURRENT_FETCHES: usize = 6;

/// Max concurrent LOCAL-file document reads across all tabs.
/// - Deliberately separate from `MAX_CONCURRENT_FETCHES`: a slow or hung
///   filesystem (NFS/FUSE) holds its own limiter without starving HTTPS
///   navigations, and heavy network activity never queues a local open
///   behind a network fetch slot.
/// - Still bounded, so restoring a session of many local files cannot
///   spawn unbounded threads/transient read buffers.
pub const MAX_CONCURRENT_LOCAL_READS: usize = 6;

/// Worker threads of the one shared network runtime
/// (`protocol::client::run_network`). Bound on purpose: the HTTP/2 connection
/// pool needs a runtime that outlives individual jobs; a handful of workers
/// keep document parsing and connection tasks moving without a thread per
/// core.
pub const NETWORK_WORKER_THREADS: usize = 4;

/// How long a fetch thread waits for a global slot before giving up (seconds).
/// Keeps fast abandon responsive instead of queueing for the full visual budget.
pub const FETCH_SLOT_WAIT_SECS: u64 = 20;

/// Poll interval while waiting for a fetch slot / cooperative cancel (ms).
pub const FETCH_SLOT_POLL_MS: u64 = 20;

/// Max simultaneous tabs.
/// - Every `Loaded` tab keeps its document body + parsed events in memory
///   (up to ~2 x `MAX_BODY_BYTES`); an unbounded tab list is an unbounded
///   memory leak. New tabs beyond this show a toast instead of allocating.
/// - 128 leaves room for a power user's whole session; the tab list itself
///   costs almost nothing until a tab actually loads a document.
pub const MAX_TABS: usize = 128;

/// Internal local-history page (`about:history`). One definition shared by
/// the navigation intercept (`browser::tab`), the session restore validator
/// (`settings`) and the tab tests, so no path can accept a different
/// `about:` string.
pub const HISTORY_URL: &str = "about:history";

/// Internal third-party licenses page (`about:licenses`): the embedded
/// `THIRD_PARTY_NOTICES.md` (`licenses::document`) rendered like a document,
/// reached from Preferences or by typing the URL. Same single-definition
/// rule as [`HISTORY_URL`].
pub const LICENSES_URL: &str = "about:licenses";

/// Internal new-tab page (`about:home`): the landing page with the large
/// omnibox and the bookmark grid. A real navigation for the back/forward
/// stacks (like [`HISTORY_URL`]), so Back/Forward traverse it instead of
/// losing it; the address field still shows empty on it (revert clears).
pub const HOME_URL: &str = "about:home";

/// Every internal `about:` page, for the session restore validator: whatever
/// the app can write into `session.bin` it must also be able to load back.
pub const INTERNAL_PAGES: [&str; 3] = [HISTORY_URL, LICENSES_URL, HOME_URL];

// ---------------------------------------------------------------------------
// Local bookmarks (encrypted at rest, see `bookmarks.rs`)
// ---------------------------------------------------------------------------

/// Max bookmarks kept in the local bookmark store.
/// - Bounds memory and file size. Unlike history, an add beyond the cap is
///   REFUSED (with a toast) instead of silently dropping the oldest
///   bookmark: a bookmark is explicit user data, not a disposable visit.
pub const MAX_BOOKMARK_STORE_ENTRIES: usize = 2_000;

/// Max serialized (plaintext) bytes of the bookmark store.
/// - The entry count alone does not bound bytes: URLs may be up to
///   `MAX_URL_INPUT_CHARS` chars each. The store keeps the caller's URL
///   spelling (no re-encoding) so an entry is bounded by 4 bytes per char,
///   but the guard is enforced on the EXACT serialized body
///   (`bookmarks::entry_bytes`): an add that would cross this budget is
///   REFUSED like the entry cap, and a body that arrived from disk over it
///   is trimmed oldest-first on load. Together with the entry cap this
///   bounds memory, file size and the read path.
pub const MAX_BOOKMARK_STORE_BYTES: usize = 48 * 1024 * 1024;

/// Max bytes READ for the encrypted bookmark file: the plaintext budget
/// plus the AEAD overhead (magic/version header, nonce, tag). The reader
/// refuses anything larger, so this MUST stay at or above what a bounded
/// writer (`MAX_BOOKMARK_STORE_BYTES` + the same overhead) can produce.
pub const MAX_BOOKMARK_STORE_FILE_BYTES: usize =
    MAX_BOOKMARK_STORE_BYTES + crate::store::crypto::OVERHEAD_BYTES;

/// Max chars of a bookmark's custom tile icon (Home "quick access" tile).
/// - A letter, digit or short emoji cluster. Long enough for every standard
///   ZWJ emoji sequence (family/role/flags) and tag sequences, short enough
///   that a tile glyph can never overflow its box.
pub const MAX_BOOKMARK_ICON_CHARS: usize = 16;

/// Number of tile colors the bookmark editor offers (palette indexes are
/// stored per bookmark; the palette itself lives in `ui::bookmarks`, and a
/// test keeps the two in sync).
pub const MAX_BOOKMARK_COLORS: u8 = 8;

/// Debounce before a dirty bookmark store is written to disk (seconds).
/// - Bookmarks change only on an explicit user action (Ctrl/Cmd+D, per-row
///   remove); the short debounce merges bursts and keeps the UI thread off
///   the `fsync` path like history.
pub const BOOKMARKS_SAVE_DEBOUNCE_SECS: u64 = 2;

/// Backoff before a FAILED session save is retried (seconds).
/// - "Restore tabs on startup" snapshots the open tabs; a failed save (no
///   data directory, full disk) must not retry on every frame (which would
///   allocate an error string per frame) nor silently give up: the pending
///   snapshot stays pending and lands on the next retry window.
pub const SESSION_SAVE_RETRY_SECS: u64 = 5;

/// How long a synchronous bookmark flush waits for the background writer
/// before giving up (seconds).
/// - Same contract as `HISTORY_SAVE_FLUSH_TIMEOUT_SECS`: a hung disk must
///   not freeze a frame or exit forever, and on timeout the caller keeps
///   the worker and retries later instead of reordering writes.
pub const BOOKMARKS_SAVE_FLUSH_TIMEOUT_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// Omnibox autocomplete (toolbar + Home share `ui::omnibox`)
// ---------------------------------------------------------------------------

/// Max suggestions shown under the address capsule.
/// - Small enough to stay a popup (never a second page), large enough to
///   cover bookmarks + recent history + the curated builtin list.
pub const MAX_OMNIBOX_SUGGESTIONS: usize = 8;

/// Max chars of a suggestion/history row title before the ellipsis
/// (display bound only; the stored title stays capped by `MAX_TITLE_CHARS`).
/// Single source for `ui::omnibox::suggest_row` and `ui::history_page`.
pub const ROW_TITLE_CHARS: usize = 160;

/// Max chars of a suggestion/history row URL before the ellipsis
/// (display bound only; the stored URL stays capped by `MAX_URL_INPUT_CHARS`).
pub const ROW_URL_CHARS: usize = 220;

/// Max chars of the exact address shown in the omnibox page-info popup
/// (display bound only; the loaded URL itself stays capped by
/// `MAX_URL_BYTES`). Small enough for a brief card, large enough to name
/// the real page.
pub const SITE_INFO_ADDRESS_CHARS: usize = 160;

/// Seconds a destructive two-click confirmation stays armed
/// ("Delete history", "Delete all data"). One token so every surface
/// arms/expires identically.
pub const DELETE_CONFIRM_SECS: u64 = 4;

/// Standard chrome row heights (toolbar, tab action rows, dropdown rows).
/// Centralized so the rail, navbar and dialogs share one rhythm.
pub const CHROME_ROW_H: f32 = 34.0;
/// Height of one dropdown popup row (fixed, so hover never moves it).
pub const CHROME_DROPDOWN_ROW_H: f32 = 30.0;
/// Height of the toolbar zoom pill.
pub const CHROME_ZOOM_H: f32 = 30.0;

// ---------------------------------------------------------------------------
// UI zoom (toolbar magnifiers + the Preferences "Default zoom" setting)
// ---------------------------------------------------------------------------

/// Smallest UI zoom factor (75%).
pub const ZOOM_MIN: f32 = 0.75;
/// Largest UI zoom factor (175%).
pub const ZOOM_MAX: f32 = 1.75;
/// Zoom step of the toolbar magnifiers (25%).
pub const ZOOM_STEP: f32 = 0.25;

// ---------------------------------------------------------------------------
// UI layout (chrome geometry shared by the viewer and the Home grid)
// ---------------------------------------------------------------------------

/// Creation-time window size (logical points), used by `main`'s
/// `ViewportBuilder` before the first frame applies the remembered geometry
/// or the monitor-proportional [`initial_window_size`]. Centralized here so
/// `main` and tests share the same geometry instead of local magic numbers.
pub const WINDOW_INITIAL_W: f32 = 1100.0;
/// Creation-time window height, see [`WINDOW_INITIAL_W`].
pub const WINDOW_INITIAL_H: f32 = 750.0;
/// Minimum window size: below this the toolbar and the reading column
/// cannot lay out without clipping.
pub const WINDOW_MIN_W: f32 = 640.0;
/// Minimum window height, see [`WINDOW_MIN_W`].
pub const WINDOW_MIN_H: f32 = 480.0;

/// Initial window size as a fraction of the monitor's logical size.
///
/// A fraction (instead of a fixed size) keeps the window proportionate on a
/// small laptop screen and a large desktop alike; [`WINDOW_MIN_W`]/
/// [`WINDOW_MIN_H`] win on a monitor too small for the result, and
/// [`WINDOW_INITIAL_MAX_W`]/[`WINDOW_INITIAL_MAX_H`] cap it on a very large,
/// 100 %-scale one. Used by [`initial_window_size`] (first run only: once a
/// geometry is stored, the app restores it).
pub const WINDOW_INITIAL_FRACTION: f32 = 0.8;

/// Largest initial window width (logical points), see
/// [`WINDOW_INITIAL_FRACTION`].
pub const WINDOW_INITIAL_MAX_W: f32 = 1680.0;
/// Largest initial window height, see [`WINDOW_INITIAL_MAX_W`].
pub const WINDOW_INITIAL_MAX_H: f32 = 1050.0;

/// Quiet period before a changed window geometry is persisted.
///
/// Dragging or resizing a window emits many changes; the encrypted
/// `settings.bin` write happens once the window has been still for this long
/// (and once more on a clean exit), instead of on every frame.
pub const WINDOW_GEOMETRY_SAVE_DEBOUNCE_SECS: u64 = 1;

/// Initial inner size (logical points) for a monitor of `monitor_w` ×
/// `monitor_h` points.
///
/// [`WINDOW_INITIAL_FRACTION`] of the monitor, never below
/// [`WINDOW_MIN_W`]/[`WINDOW_MIN_H`] and never above
/// [`WINDOW_INITIAL_MAX_W`]/[`WINDOW_INITIAL_MAX_H`] nor the monitor itself.
/// A monitor smaller than the minimum still gets the minimum (the window
/// then overflows it, as it always did). Non-finite input falls back to the
/// minimum.
pub fn initial_window_size(monitor_w: f32, monitor_h: f32) -> (f32, f32) {
    let clamp = |monitor: f32, min: f32, max: f32| {
        if monitor.is_finite() {
            (monitor * WINDOW_INITIAL_FRACTION)
                .clamp(min, max)
                .min(monitor.max(min))
        } else {
            min
        }
    };
    (
        clamp(monitor_w, WINDOW_MIN_W, WINDOW_INITIAL_MAX_W),
        clamp(monitor_h, WINDOW_MIN_H, WINDOW_INITIAL_MAX_H),
    )
}

/// Top-left position (logical points, monitor space) that centers a window
/// whose OUTER size is `outer_w` × `outer_h` inside a monitor at
/// `(origin_x, origin_y)` sized `monitor_w` × `monitor_h`.
///
/// A negative margin (window larger than its monitor) is clamped to the
/// monitor origin, so an oversized window opens at the top-left corner
/// instead of hanging off the top/left edge. Non-finite input falls back to
/// the monitor origin.
pub fn centered_window_pos(
    origin_x: f32,
    origin_y: f32,
    monitor_w: f32,
    monitor_h: f32,
    outer_w: f32,
    outer_h: f32,
) -> (f32, f32) {
    let center = |origin: f32, monitor: f32, outer: f32| {
        if origin.is_finite() && monitor.is_finite() && outer.is_finite() {
            origin + ((monitor - outer) * 0.5).max(0.0)
        } else {
            origin
        }
    };
    (
        center(origin_x, monitor_w, outer_w),
        center(origin_y, monitor_h, outer_h),
    )
}

/// Deadline `secs` seconds from now, fail-closed on overflow.
///
/// `Instant + Duration` panics on overflow. The overflow is unreachable in
/// practice (it needs ~500M years of seconds), but every caller of this
/// helper is on a background or UI thread that must never panic: on the
/// (unreachable) overflow the deadline is "now", so the wait expires
/// immediately instead of aborting the process.
pub fn deadline_after(secs: u64) -> std::time::Instant {
    std::time::Instant::now()
        .checked_add(std::time::Duration::from_secs(secs))
        .unwrap_or_else(std::time::Instant::now)
}

// ---------------------------------------------------------------------------
// Encrypted HTTP disk cache (`cache/`): documents, images, favicons
// ---------------------------------------------------------------------------

/// Max total plaintext bytes held by the encrypted HTTP disk cache
/// (documents + images + favicons, all classes combined).
/// - Past this the OLDEST entries are evicted first (FIFO): a store never
///   fails and never toasts, it makes room. 512 MiB caps a full session of
///   heavy reading while staying small next to the 768 MiB process-wide
///   texture budget.
pub const MAX_CACHE_BYTES: usize = 512 * 1024 * 1024;

/// Max total entries held by the encrypted HTTP disk cache (all classes).
/// - FIFO like the byte budget: the oldest entry goes first, never an error.
/// - 50 000 entries of small metadata + bodies stay well under the byte cap
///   on their own; the two bounds are enforced together (whichever trips
///   first evicts).
pub const MAX_CACHE_ENTRIES: usize = 50_000;

/// Max plaintext bytes of ONE cached document class shard: documents are
/// stored one encrypted file per host, and a single host must not be able
/// to eat the whole cache (a hostile mirror of 10 MiB pages).
pub const MAX_CACHE_DOMAIN_BYTES: usize = 32 * 1024 * 1024;

/// Max plaintext bytes of the single favicon store file (`favicons.bin`).
/// - Favicons decode at `MAX_FAVICON_DIM` and are small; 32 MiB still holds
///   thousands of icons. Past it the oldest probes are evicted FIFO.
pub const MAX_CACHE_FAVICON_BYTES: usize = 32 * 1024 * 1024;

/// Max plaintext bytes of ONE cached image entry (mirrors the fetch cap:
/// an entry can never hold more than the network would deliver).
pub const MAX_CACHE_IMAGE_BYTES: usize = MAX_IMAGE_BYTES;

/// Max entries kept in the favicon disk store (origins x probes).
/// - The session cache already caps origins at `MAX_FAVICON_HOSTS`; the disk
///   store keeps the same order of magnitude, FIFO-evicted past the cap.
pub const MAX_CACHE_FAVICON_ENTRIES: usize = 2_048;

/// Absolute freshness ceiling for ANY cached entry (seconds): even an
/// `immutable` response with `max-age=31536000` is revalidated after a year.
/// - HTTP caches must bound heuristic/declared freshness; a year is the
///   longest honest TTL, and it keeps "Delete all data" honest too.
pub const MAX_CACHE_FRESHNESS_SECS: u64 = 365 * 24 * 60 * 60;

/// Default freshness (seconds) when a cacheable response carries no
/// explicit freshness (`max-age`/`Expires`): documents change often.
pub const CACHE_DEFAULT_DOC_TTL_SECS: u64 = 5 * 60;

/// Default freshness (seconds) for cacheable image responses without
/// explicit freshness.
pub const CACHE_DEFAULT_IMAGE_TTL_SECS: u64 = 60 * 60;

/// Default freshness (seconds) for cacheable favicon responses without
/// explicit freshness (icons change rarely).
pub const CACHE_DEFAULT_FAVICON_TTL_SECS: u64 = 24 * 60 * 60;

/// Debounce before dirty cache indexes are flushed to disk (seconds).
/// - Body files are written synchronously on the background job thread that
///   fetched them; only the small index files are debounced, so a burst of
///   loads rewrites them at most once per window.
pub const CACHE_INDEX_FLUSH_DEBOUNCE_SECS: u64 = 10;

/// How long a synchronous cache flush waits for the background flusher
/// before giving up (seconds). A hung disk must not freeze a frame or exit.
pub const CACHE_FLUSH_TIMEOUT_SECS: u64 = 5;

/// How long one cached document body is kept VERIFIED-fresh in the RAM
/// fast path after its last use is irrelevant: the RAM copy is keyed by the
/// same expiry as disk, so this only bounds process memory, not correctness.
/// - 64 MiB of recently used document bodies avoids re-reading + re-decrypting
///   the domain file on Back/Forward.
pub const CACHE_RAM_DOC_BYTES: usize = 64 * 1024 * 1024;

/// Max chars kept per captured HTTP cache header (`ETag`, `Last-Modified`,
/// `Expires`, …). Servers control these values; the cap bounds the index
/// entries and log lines derived from them.
pub const MAX_CACHE_HEADER_CHARS: usize = 512;

/// Max repeated values captured per cache-policy header (`Cache-Control`,
/// `Vary`). Past this the response is treated as non-storable: the values
/// that did not fit could forbid caching.
pub const MAX_CACHE_HEADER_VALUES: usize = 8;

/// Most visible bookmark-tile rows before the Home grid itself scrolls.
/// Below this the floating scrollbar stays hidden (the trailing item gap
/// would otherwise report overflow with nothing to scroll).
pub const BOOKMARK_GRID_MAX_VISIBLE_ROWS: usize = 3;

/// Max chars of the short tab title shown in the expanded tab rail.
/// A remote frontmatter title cannot bloat the rail.
pub const TAB_TITLE_CHARS: usize = 30;

/// Max chars of the OS window title (page title or schemeless URL).
/// Bounds what reaches the window manager; the stored title itself stays
/// capped by `MAX_TITLE_CHARS`.
pub const WINDOW_TITLE_CHARS: usize = 60;

/// Max chars of the URL shown after the `Error: ` prefix in the OS window
/// title on failed loads. Keeps the prefixed title within
/// `WINDOW_TITLE_CHARS`.
pub const WINDOW_TITLE_ERROR_URL_CHARS: usize = 50;

/// Tallest INLINE vector image (SVG) that may share a text line.
/// Vector sources declare stylesheet-sized dimensions; without CSS the
/// intrinsic size would let a logo dominate the paragraph. Raster images
/// keep their natural size.
pub const MAX_INLINE_VECTOR_H: f32 = 32.0;

/// Tallest image that may still share a text line (2 body lines). Taller
/// images are promoted to centered blocks, so text is never vertically
/// centered next to a photo.
pub const MAX_INLINE_IMAGE_H: f32 = 48.0;

#[cfg(test)]
mod tests;
