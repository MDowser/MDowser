# Interface

Desktop-browser layout, light/dark, monochrome vector icons.
Window chrome and rendering internals are in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## App icon

`assets/icon.svg`, raster ladder regenerated with `make icons`: one rounded
blue tile with the white Markdown `M↓`, used as the window/taskbar icon
(Windows/Linux), the `.exe` resource, the macOS `.app` icon and the Linux
hicolor icon.

## Theme

`src/ui/theme/mod.rs`: the README palette for the rendered document (light
`#f6f8fa` chrome / white page, dark `#151b23` / `#0d1117`, `#0969da` links,
`#d1d9e0` borders, current Primer syntax colors), tonal pill buttons and
error red for the chrome.

## Tabs

Vertical tab rail (`src/ui/tabs/mod.rs`): a 44px collapsed rail below the
toolbar with one favicon per tab, drawn at the toolbar's icon size
(active tab highlighted with a surface pill + accent bar in the rail's left
gutter). Hovering expands it to 248px as a floating overlay — never
reflowing the page — with favicon + title + close and two pinned actions at
the bottom: `+ New Tab` and `History` below it, their icons the same size
as the toolbar's. The expansion is animated (short `cubic_out` width tween,
titles fading in) instead of popping open, and closes the same way. The
overlay collapses only when the pointer moves into the document area:
moving up to the toolbar or leaving the window keeps it open. The overlay
merges with the toolbar (no top seam) and only its right edge is elevated.
A loading tab shows an hourglass icon where the favicon will be (a favicon
already cached stays, with a small progress dot over it).
New tabs are capped at `config::MAX_TABS` (128): opening one past the cap
(+ New Tab, a bookmark, `Ctrl/Cmd+T`) is refused with a "Tab limit reached"
toast instead of allocating an unbounded tab vector.

## Favicons

Favicons are probed per origin (`/favicon.ico`, then `/favicon.png`, then
`/favicon.svg`) and cached for the session (`src/media/favicon.rs`); origins
without one fall back to a home/globe icon.

## Home

The new-tab page keeps exactly three elements: a display wordmark, one
large address capsule (the same modern omnibox as the toolbar, schemeless
hint) and the encrypted bookmark "quick access" grid — the only place
bookmarks are LISTED. It lives at the internal `about:home` URL (typeable
like `about:history`) and is a real back/forward entry: Back and Forward
traverse it instead of losing it, while the capsule itself stays empty.
No hint rows, feature lists or shortcut catalogs on
the page. Each tile shows a stored custom glyph (letter or emoji) when one
exists, else the site favicon when it resolves (drawn at the
same size as the toolbar and rail icons), else the first letter of the
title on a stable color.
Clicking a tile opens the page here (its icon and its name alike),
`Cmd/Ctrl+click` opens it in a new tab, and dragging a tile reorders the
grid — as does `Alt+Left`/`Alt+Right` on a focused tile (keyboard-only
reordering). Editing happens ON the tile, with no window: the hover pencil opens an
inline rename field whose same spot becomes a check that saves; Enter saves
too and Esc drops the draft, and the hover X removes the tile.
The toolbar bookmark ribbon (or `Ctrl/Cmd+D`) adds/removes the page you are reading.

## Menu bar

Windows and Linux have no menu at all: the toolbar is the whole chrome
there. macOS always shows a system menu bar, so it is completed there
(`src/app/menu.rs`, the `muda` crate): an Application menu (About MDowser,
Preferences… `Cmd+,`, Services, Hide/Hide Others/Show All, Quit), File
(New Tab `Cmd+T`, Close Tab `Cmd+W`), Edit (Copy `Cmd+C`, Paste `Cmd+V`,
Select all `Cmd+A` — deliberately no more), View (Reload `Cmd+R`, Zoom In
`Cmd+=`, Zoom Out `Cmd+-`, Actual Size `Cmd+0`), History (Show History
`Cmd+Y`) and Window (Minimize, Zoom, Bring All to Front). Every item replays
the same shortcut the keyboard does, so the menu and the keys share one
handler; Paste reads the clipboard exactly like `Cmd+V` does. The bar shows
only MDowser's commands: automatic window tabbing is opted out at startup
(`app::menu::disable_automatic_tabbing`, which `main` calls before any window
exists), so AppKit's own "Show Tab Bar"/"Show All Tabs" items never enter
View — MDowser's tabs are the in-window rail — and the About MDowser panel
carries the app's own icon (the embedded 256px raster), never AppKit's
generic placeholder folder.

## Toolbar

`src/ui/navbar/mod.rs`: modern browser layout, no `Go`. Back / forward arrows,
reload (5s cooldown + countdown in its tooltip, becomes a Stop X while
loading), home (opens the new tab page; the page left behind stays on
Back), zoom pill and the preferences gear. Circular hover fills; every icon
is a monochrome vector glyph — muted at rest, easing to the accent and
growing slightly on hover.

## Omnibox

`src/ui/omnibox/mod.rs`: rounded capsule with a padlock security indicator (a
page indicator for local files), the address (schemeless for web pages, the
decoded path for files), an indeterminate loading sweep and ONE right slot
that holds the clear `X` while editing or the bookmark ribbon otherwise
(toolbar only): a filled ribbon when the shown page is bookmarked, an
outline ribbon when it is not, and a disabled ribbon when there is nothing
to bookmark
(a shared slot, so editing or a load resolving never shifts the field).
Enter navigates; `Esc` reverts to the shown page and leaves the field;
a clean click (or keyboard focus) selects the whole address so typing
replaces it, while a drag selects from the press point. Neither launching
the app nor opening a new tab focuses an address field on its own (no popup
over a fresh window or tab); only `Ctrl/Cmd+L` focuses it.

The left indicator is a button, not a field focus target: it names the page
kind at a glance (padlock for HTTPS, page for a local file, globe/clock/info
for the internal pages), and clicking it (or activating it from the
keyboard) opens a small page-info card under the capsule with the facts of
the page actually rendered — the kind, the document's own title when it has
one, the exact address WITH its scheme (the field hides `https://`), and one
line on where the content came from (fetched over HTTPS, read from this
device, or an internal page). The card is an overlay: it never shifts the
capsule, its exact address stays bounded
(`config::SITE_INFO_ADDRESS_CHARS`), and it closes on the next indicator
click, on `Esc`, on a click outside it, on `Ctrl/Cmd+L` and on any
navigation (it can never describe a page that is gone).

### Autocomplete

Both address fields (toolbar and Home) offer suggestions in a popup under
the capsule while the field is editing (`src/suggest.rs`), capped at
`config::MAX_OMNIBOX_SUGGESTIONS` (8):

- Bookmarks first (newest first), then local-history visits (newest first),
  then the curated builtin list compiled in from `assets/builtin_sites.txt`
  (`builtin_sites::builtin_suggestions()`; append-only: add one
  `<url><TAB><title>` line). Duplicates by exact URL keep
  the higher-priority source.
- Matching is case-insensitive on the URL and the title; a schemeless query
  (`ex.com/love`) matches the stored `https://` URL. An empty field offers
  recent bookmarks.
- No row is highlighted until the user picks one: the first `Down` selects
  the first row, the first `Up` the last, and later presses move through the
  list. `Tab` completes the highlighted row into the field without
  navigating (with nothing highlighted it completes the first row), `Enter`
  navigates to the typed address (only an explicit `Down`/`Up` or a pointer
  that MOVED onto a row routes `Enter` to a row — a pointer merely resting
  over the popup never picks one), clicking a row navigates to it, and `Esc`
  closes the popup first (a second `Esc` reverts the field). The popup is an
  overlay: it never shifts the capsule or its neighbours. A row's accessible
  name carries its URL as well as its title, so a screen reader can tell two
  pages with the same title apart.
- The list is cached per field in egui memory, rebuilt only when the query
  or `HistoryStore`/`BookmarkStore` generations change, so typing filters
  against history/bookmarks live without a per-frame rescan.

## Zoom

Zoom pill: `-` / `%` / `+` segmented capsule (75-175%); clicking the readout
resets to 100%. `Ctrl/Cmd+`/`=`/`-`/`0` drive the same factor. Responsive:
compact (no readout) on narrow windows, hidden on very narrow ones so the
omnibox keeps the space. Breakpoints use the unzoomed window width, so
zooming in never hides the pill itself.

## Preferences

Gear icon, the last button in the toolbar, after the zoom pill and always
visible. Opens a Material-style dialog (custom header, grouped cards, toggle
switch) with a FIXED WIDTH and a content-sized height, top-anchored: the
History and Local data cards hug their rows with no reserved bottom space,
and arming a delete-confirmation note only pushes the cards below it (the
inner scroll appears only when the armed stack exceeds the viewport).

- The History card holds the retention dropdown ("Forget on exit", "Keep 1
  month", "Keep 3 months (default)", "Keep 6 months", "Keep 1 year", "Keep
  forever") and the destructive "Delete
  history" action (two-click confirmation); the encrypted store location
  shows as a
  tooltip on the stored-count caption. Selecting "Forget on exit" also turns
  off "restore tabs on startup" and deletes `session.bin`: that encrypted
  snapshot holds visited URLs, and the mode promises nothing on disk.
- The Appearance card holds the theme (System/Light/Dark), the default zoom
  (the same `- % +` pill as the toolbar; clicking the readout resets to
  100%) and "restore tabs on startup" (a caption under the switch explains when
  it is off because of "Forget on exit").
- The Local data card holds "Delete all data and close" (two-click
  confirmation): deletes the whole data directory (history, bookmarks,
  settings, session and the shared key) and exits immediately with
  `process::exit` without further saves. A deletion that FAILS (a file still
  open elsewhere, permissions) does NOT close the app and does not pretend
  the data is gone: the card shows the error, writes resume and the next
  attempt can succeed.
- A compact footer line shows the version and the app's own license
  (`MDowser <ver> · MPL-2.0`) and holds the "View third-party
  licenses" link, which opens `about:licenses` in a new tab.

Every dropdown row keeps its geometry on hover, so the controls never shift.
Esc closes an open dropdown first, and the dialog on the next press.

## History

Clock icon at the bottom of the tab rail: the internal `about:history`
page, rendered in a tab like a document — a searchable list of pages that
actually loaded, newest first, with each row's age and a remove X (the
row's accessible name carries the URL too). The
button (and `Ctrl+H` / `Cmd+Y`) opens it in a NEW tab right after the
current one (which is never replaced); clicking a row then navigates that
same history tab to the visit, so Back returns to the list.

History is stored encrypted on this device only: the standard per-user data
directory (macOS `~/Library/Application Support/MDowser`, Linux
`$XDG_DATA_HOME/mdowser`, Windows `%LOCALAPPDATA%\MDowser`), bounded to
20 000 entries / 8 MiB, written atomically and pruned by the retention
window. "Forget on exit" keeps visits in memory for the session only and
wipes any leftovers.

## Third-party licenses

"View third-party licenses" at the bottom of Preferences (or typing
`about:licenses`) opens the licenses of the linked crates and bundled
fonts: the exact `THIRD_PARTY_NOTICES.md` shipped in every release archive,
embedded in the binary and rendered with the browser's own Markdown engine.
Text selection, copy, the right-click menu and links behave like a document;
relative links (to the repository's license files) are inert offline, while
absolute `https:` links are live. The page opens in a new tab and session
restore brings it back like `about:history`.

## Bookmarks

`Ctrl/Cmd+D` or the toolbar bookmark ribbon toggles the loaded page (a toast confirms;
the ribbon fills while the shown page is bookmarked). The Home page shows them
as a "quick access" tile grid (newest first, responsive columns): a stored
custom glyph (a letter or emoji) when one exists, else the site favicon
when it resolves, else the first letter of the title on a stable color. The
name is up to two centered lines, elided with `…` by the layout when it
does not fit (long paths and CJK included); the grid scrolls past three
tile rows, and its floating scrollbar only appears once it actually does.
The tile editor is inline (no
window): the pencil renames (its check or Enter saves, Esc cancels), a drag
reorders the grid (`Alt+Left`/`Alt+Right` on the focused tile moves it one
display slot) and the X removes; any stored glyph or color keeps
rendering. Stored
encrypted on this device only (`bookmarks.bin` + shared `store.key`,
ChaCha20-Poly1305, the same per-user data directory as History), kept until
removed (no retention window), bounded to 2000 entries with an add at the
cap refused instead of dropping an older bookmark. A first run starts with
a curated default set of five (Accept Markdown, AgentReady leaderboard,
Cloudflare, Stripe docs, Is It Agent Ready); Vercel, Sentry and Supabase
docs live only in the omnibox builtin suggestions. Deleting them never brings
them back. Their favicons load through the usual tile pipeline (one probe
per origin on first display). The grid is deliberately
the only place the bookmarks themselves are listed.

## Reading surface

Content: fluid centered column (max 760px, 8/12/16px gutters),
always-visible scrollbar (a border-tone handle at 0.75 dormant opacity, so
the bar reads against both themes instead of blending into the chrome). The
previous page stays visible until the new one resolves (no white flash).

Render: responsive H1-H6 in the README type scale with H1/H2 bottom rules,
wrapping selectable paragraphs with README line height,

- one large `·` document bullet hugging the text column inside a fixed
  24px gutter for every unordered item (whatever the source marker: `-`,
  `*` or `+`, at every depth) with a hanging indent that keeps wrapped rows
  in the text column and the bullet on the first text row,
- compact item spacing, and `N.` lists with real read-only checkboxes for
  tasks,
- inline code pills at the body size (GitHub `.2em .4em` padding as real
  layout air on both
  sides, body baseline, linked code included, and one tight pill per row
  when a span wraps; the pill's height is capped at the body line minus 3px
  so the bundled mono cut's tall em box cannot inflate it into a full line
  box),
- code cards (language chip + Copy button + lightweight per-line
  highlighting, no line numbers, at the same body size as the prose),
- striped bordered tables that fill the drawn card (leftover width
  distributed proportionally), shrink and wrap long cells (links included)
  instead of forcing a horizontal scrollbar (a table that still scrolls
  keeps its columns at a readable width instead of collapsing them), unique
  ids, per-column alignment, GitHub-like interior cell separators that reach
  the card border and 5px/11px cell padding so text never touches a line,
- muted quotes in a soft full-width box with ONE thick bar (neutral for
  plain quotes, the alert accent for `> [!NOTE]`/`TIP`/`WARNING` callouts,
  whose body keeps the normal text color) and vertical air between
  consecutive boxes,
- SVG badges/logos, emoji, CJK, RTL and the bundled script
  faces, and real images (hourglass placeholder while loading, warning icon
  with the reason as tooltip on failure, centered captions, natural-size
  images clamped to the reading column; linked images draw the shared
  keyboard focus ring like every other chrome control — in every load
  state, placeholders included).
- Footnote references render as small raised NUMBER markers (linked to the
  note, numbered by first reference) and the notes themselves as a footnotes
  section: one rule before the first note, one numbered entry per note whose
  marker shares the first line with the note body (continuation blocks hang
  under the text; a first block that cannot flow inline keeps the marker
  above it, like a list item), and a back-arrow that closes the note text
  (the last inline box, so it sits right after the final word on the same
  line; a note ending in a block that cannot flow inline keeps it on a
  small trailing row) and scrolls to the
  reference. Definitions
  the document never references still render. `Copy` keeps the source
  `[^label]` spelling. See [`RENDERING.md`](RENDERING.md#footnotes).

Every surface draws bundled Noto typefaces only: proportional text
(document text, chrome, menus) draws the Noto Sans variable font, while
inline code and code cards draw the bundled Noto Sans Mono CJK JP cut (face
5 of the CJK collection) so code keeps true monospace alignment — at the
BODY size, so code is exactly as wide as the prose around it (`theme::BODY_FONT_SIZE`
backs both text styles; the code cards are tall for the body 24px line).
Bold text
is a real bold face (`theme::strong`): headings, `**strong**` runs, callout
titles and footnote refs draw the same Noto Sans variable font pinned to
`wght = 700`, so weight carries the emphasis instead of a color shift.
Body text is the high-contrast `on_surface` token; muted H6 and plain
quote bodies opt into the muted tone on purpose. Raw HTML is never painted
as markup and never interpreted: every block-level or inline tag is a
transparent wrapper (the tag disappears, its content renders as Markdown),
so exported HTML-wrapped content reads normally. The one inert case left is
`<pre>`/`<textarea>` (their bytes are not Markdown; they paint as muted
monospace) and malformed/unbalanced markup, which also stays literal.
HTML comments (`<!-- ... -->`) are invisible, block and
inline alike: they never paint and `Copy` never includes them.

## Error screen

A failed load (or a local rejection) renders in the reading sheet as a
styled error page instead of a raw log:

- A colored status dot and a family title: No connection, Page not found,
  No markdown here, Invalid address, Could not load (the family is
  classified from the failure log; a local-file failure uses the local
  wording). An explanation line, the requested address with its scheme
  hidden, a "What you can do" hint card and a Details section follow.
- Details is a bounded scroll of one monospace line per route candidate,
  in the order they were tried, with `https://`/`file://` stripped and any
  userinfo credentials removed; a "Copy log" button copies the same text
  prefixed by `Requested: <address>`.
- A Retry button re-runs the page. It honors the same 5s cooldown as
  Refresh (anti-hammer): while cooling down it is disabled and its tooltip
  counts the seconds; a local address rejection (invalid URL, non-https)
  can never succeed by retrying and shows a "fix the address" hint instead
  of a countdown.

## Right-click menu

Minimal: Copy (the drag selection when there is one, else the whole page)
and Select all; over a link it adds Open link in new tab at the top. All
body text is natively selectable (drag + Cmd/Ctrl+C) with readable selection
colors. Right-clicking never disturbs an existing selection (its press is
intercepted before egui, which would otherwise collapse it), so Copy from
the menu copies exactly the selected text. Opening the menu with the pointer
leaves NO row highlighted (only
the keyboard lights one), a primary press outside it — including the start
of a drag selection — closes it, and the menu is keyboard-navigable:
`Down`/`Up` move focus into the rows (the first press lands on the first
one), `Enter` activates the focused one and `Esc` closes it.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl/Cmd+T` | New tab |
| `Ctrl/Cmd+W` | Close tab |
| `Ctrl/Cmd+R` / `F5` | Refresh |
| `Ctrl/Cmd+L` | Focus address field (selects the URL) |
| `Esc` | Stop loading (reverts the field when editing it) |
| `Ctrl/Cmd+A` | Select all (page text) |
| `Ctrl/Cmd+C` | Copy (selection, or all while selected) |
| `Ctrl/Cmd+1..9` | Select tab |
| `Ctrl/Cmd` `+` / `=` | Zoom in |
| `Ctrl/Cmd+-` | Zoom out |
| `Ctrl/Cmd+0` | Reset zoom to 100% |
| `Ctrl+H` / `Cmd+Y` | History (new tab) |
| `Ctrl/Cmd+D` | Bookmark the current page (toggle) |
| `Ctrl/Cmd+,` | Preferences window |
