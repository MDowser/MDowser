# Interface

Desktop-browser layout, light/dark, monochrome vector icons. Chrome and
rendering internals in [`ARCHITECTURE.md`](ARCHITECTURE.md); Markdown mapping in
[`RENDERING.md`](RENDERING.md); URL resolution in
[`NAVIGATION.md`](NAVIGATION.md).

## App icon

`assets/icon.svg`: one rounded blue tile with the white Markdown `M↓`;
`make icons` regenerates its raster ladder. It is the window/taskbar icon on
Windows and Linux, the `.exe` resource, the macOS `.app` icon and Linux hicolor
icon.

## Theme

`src/ui/theme/mod.rs`: the README document palette (light `#f6f8fa` chrome /
white page, dark `#151b23` / `#0d1117`, `#0969da` links, `#d1d9e0` borders,
neutral syntax colors), tonal pill buttons and error red for the chrome.

## Tabs

Vertical tab rail (`src/ui/tabs/mod.rs`): a 44px collapsed rail below the
toolbar, one favicon per tab at toolbar icon size; the active tab shows a
surface pill and accent bar in the left gutter.

- Hovering expands it to 248px as a floating overlay that never reflows the
  page: favicon + title + close, plus pinned `+ New Tab` and `History` below it
  at the bottom, toolbar-sized.
- The expansion is a quintic ease-out width tween (fast start, gentle settle,
  mirrored on close), clipped to the growing edge; titles and action labels wait
  for width, then fade in while drifting their last pixels into column, each row
  a beat after the one above.
- It collapses only when the pointer enters the document area; moving to the
  toolbar or leaving the window keeps it open. It merges with the toolbar (no
  top seam), and only its right edge is elevated — a soft light penumbra, a dark
  hairline — smaller and fainter while collapsed, strengthening with the panel.
- Titles and action labels are regular chrome text; the rail never borrows the
  document bold face (`theme::strong`), reserved for the reading surface.
- A tab's title is the page's own (frontmatter `title`, else the first `#`
  heading), the schemeless URL only as a last resort.
- A loading tab shows an hourglass where the favicon will be; a cached favicon
  stays with a small progress dot.
- New tabs cap at `config::MAX_TABS` (128); one past the cap (`+ New Tab`, a
  bookmark, `Ctrl/Cmd+T`, a file drop) is refused with a "Tab limit reached"
  toast.
- Rows reorder: a drag reorders the rail live (list follows pointer, dragged
  row carries a primary ring, overlay stays open), `Alt+Up`/`Alt+Down` moves a
  focused row one slot. The active tab follows its page; the new order is what
  "restore tabs on startup" remembers.

## Drag and drop

Local files and folders can be dropped anywhere on the window. While the OS
reports files hovering, a translucent scrim and a centered target cover the
chrome and the page; dropping opens one tab per path in drop order, right after
the active tab, and selects the first dropped tab. The page that was open is
never replaced. Folders resolve through the local-file resolver's conventional
indexes (`index.md`, `README.md`), and a drop that exceeds the remaining tab
slots opens what fits and reports "Tab limit reached".

## Favicons

Favicons are probed per origin (`/favicon.ico`, `/favicon.png`, `/favicon.svg`)
and served from the encrypted disk cache on repeat visits and persist across
launches (`src/media/favicon.rs` holds the session per-origin textures). Origins
without one get a home/globe icon.

## Home

The new-tab page has three elements: the wordmark lockup (the app mark — the
blue `M↓` tile, drawn as vectors so it stays crisp at any zoom — beside
"MDowser" in the display face; a window too narrow for both keeps the word),
one large address
capsule (the toolbar's omnibox, with a schemeless hint) and the encrypted
bookmark "quick access" grid — the only place bookmarks are LISTED. `about:home`
(typeable like `about:history`) is a real back/forward entry: Back and Forward
traverse it, the capsule stays empty. No hint rows, feature lists or shortcut
catalogs.

Each tile shows a stored custom glyph (letter or emoji), else the site favicon
when it resolves (at toolbar/rail size), else the title's first letter on a
stable color. A tile click (icon or name) opens the page here;
`Cmd/Ctrl+click` opens a new tab; dragging reorders the grid, as does
`Alt+Left`/`Alt+Right` on a focused tile. Editing is ON the tile, no window:
the hover pencil opens an inline rename field whose spot becomes a saving
check; Enter saves, Esc drops the draft, the hover X removes. With keyboard
focus, `Tab` reaches the pencil and X (tile, pencil, X: one focus chain per
cell). The toolbar bookmark ribbon (or `Ctrl/Cmd+D`) adds or removes the page
you are reading.

## Menu bar

Windows and Linux have no menu: the toolbar is the whole chrome. macOS always
shows a system menu bar (`src/app/menu.rs`, `muda`): Application (About MDowser,
Preferences… `Cmd+,`, Services, Hide/Hide Others/Show All, Quit), File (New Tab
`Cmd+T`, Close Tab `Cmd+W`, Save Page `Cmd+S`), Edit (Copy `Cmd+C`, Paste `Cmd+V`,
Select all `Cmd+A` — deliberately no more), View (Reload `Cmd+R`, Zoom In
`Cmd++`/`Cmd+=`, Zoom Out `Cmd+-`, Actual Size `Cmd+0`), History (Show
History `Cmd+Y`), Window
(Minimize, Zoom, Bring All to Front).

Every item replays the keyboard's shortcut through one handler; Paste reads the
clipboard exactly like `Cmd+V`. Only MDowser's commands appear: automatic window
tabbing is opted out at startup (`app::menu::disable_automatic_tabbing`, called
by `main` before any window exists), so AppKit's "Show Tab Bar"/"Show All Tabs"
never enter View — tabs are the in-window rail — and About MDowser carries the
app's own icon (the embedded 256px raster), not AppKit's placeholder folder.

## Toolbar

`src/ui/navbar/mod.rs`: modern layout, no `Go`. Back/forward, reload (5s
cooldown with a tooltip countdown, a Stop X while loading; it forces cache
revalidation instead of serving a fresh record, and falls back to the stored
copy — marked — when the network fails), home (opens the new-tab page;
the page left behind stays on Back), zoom pill and Preferences gear. Circular
hover fills; every icon is a monochrome vector glyph — muted at rest, easing to
the accent and growing slightly on hover.

## Omnibox

`src/ui/omnibox/mod.rs`: a rounded capsule with a padlock indicator (a page
indicator for local files, a cloud-off/archive for a copy served when no fresh
one could be obtained), the address (schemeless for web, decoded path for
files), an indeterminate loading sweep and ONE right slot: the clear `X` while
editing, else the bookmark ribbon (toolbar only) — filled when the page is
bookmarked, outline when not, disabled with nothing to bookmark. Sharing the
slot keeps editing and load resolution from shifting the field.

`Enter` navigates; `Esc` reverts to the shown page and leaves the field; a clean
click (or keyboard focus) selects the whole address so typing replaces it, a
drag selects from the press point. Launching the app or opening a tab never
focuses the field (no popup over a fresh window or tab); only `Ctrl/Cmd+L` does.

The left indicator is a button, not a focus target. It names the page kind
(padlock HTTPS, page for a local file, globe/clock/info internal, cloud-off for
an offline copy, archive for a saved one), and clicking
it (or keyboard activation) opens a page-info card under the capsule with the
rendered page's facts: kind, the document's own title when it has one
(frontmatter `title`, else the first `#` heading), the exact address WITH its
scheme (the field hides `https://`), and one line on content origin (fetched
over HTTPS, read from this device, an internal page, or why a stored copy is
shown). The card is an overlay
and never shifts the capsule; its address stays bounded
(`config::SITE_INFO_ADDRESS_CHARS`), and it closes on the next indicator click,
`Esc`, a click outside, `Ctrl/Cmd+L` or any navigation, never describing a gone
page.

### Autocomplete

Both address fields (toolbar and Home) offer suggestions in a popup under the
capsule while the field is editing (`src/suggest.rs`), capped at
`config::MAX_OMNIBOX_SUGGESTIONS` (8):

- Bookmarks first (newest first), then local-history visits (newest first),
  then the curated builtin list from `assets/builtin_sites.txt`
  (`builtin_sites::builtin_suggestions()`; curated upstream in MDowser-Sites,
  vendored with `make sync-sites`: one `<url> | <title>` line per entry).
  Exact-URL duplicates keep the higher-priority source.
- Matching is case-insensitive on URL and title; a schemeless query
  (`ex.com/love`) matches the stored `https://` URL. An empty field offers
  recent bookmarks.
- No row highlights until the user picks one: first `Down` selects the first
  row, first `Up` the last, later presses move through the list. `Tab` completes
  the highlighted row into the field without navigating (nothing highlighted:
  the first row). `Enter` navigates to the typed address; only an explicit
  `Down`/`Up` or a pointer that MOVED onto a row routes it there (resting over
  the popup never picks one), clicking a row navigates, `Esc` closes the popup
  first (a second `Esc` reverts the field). The popup is an overlay and never
  shifts the capsule or neighbours. A row's accessible name carries URL and
  title, letting a screen reader distinguish same-titled pages.
- The popup is height-bounded by the window (`ScrollArea`, scrollbar only when
  needed) and follows the selected row, so the full suggestion list stays
  reachable at the minimum window height instead of running off-screen.
- The list is cached per field in egui memory, rebuilt only when the query or
  the `HistoryStore`/`BookmarkStore` generations change, so typing filters live
  history and bookmarks without a per-frame rescan.

## Zoom

Zoom pill: a `-` / `%` / `+` segmented capsule (75–175%); clicking the readout
resets to 100%. `Ctrl/Cmd` with `+`/`=`/`-`/`0` drives the same factor.
Responsive: compact (no readout) on narrow windows, hidden on very narrow ones
so the omnibox keeps the space. Breakpoints use the unzoomed width, so zooming
in never hides the pill.

## Preferences

Gear icon, the toolbar's last button, after the zoom pill and always visible. It
opens a card-style dialog (custom header, tab bar, grouped cards, toggle switch)
with a FIXED WIDTH, content-sized height and top anchor: each tab's cards hug
their rows with no reserved bottom space, and arming a delete-confirmation note
only pushes the cards below it (inner scroll only past the viewport). The
selected tab and each tab's scroll position persist while the dialog stays open
(never written to disk).

- **General** holds the appearance card — theme (System/Light/Dark), default
  zoom (the toolbar's `- % +` pill; readout click resets to 100%) and "restore
  tabs on startup" (a caption under the switch explains when it is off because
  of "Forget on exit") — plus a compact footer with the version, the app's
  license (`MDowser <ver> · MPL-2.0`) and the "View third-party licenses" link,
  opening `about:licenses` in a new tab.
- **Data** holds the history card — the retention dropdown ("Forget on exit",
  "Keep 1 month", "Keep 3 months (default)", "Keep 6 months", "Keep 1 year",
  "Keep forever"), the destructive "Delete history" action (two-click
  confirmation) and the stored-count caption (the encrypted store location as
  its tooltip). "Forget on exit" also turns off "restore tabs on startup" and
  deletes `session.bin`, since the snapshot holds visited URLs. The tab also has
  the cache card (encrypted HTTP disk cache usage, one two-click "Clear …" per
  class: documents, images, favicons) and the local data card with "Delete all
  data and close" (two-click confirmation): it deletes the whole data directory
  (history, bookmarks, settings, session, cache, shared key) and exits
  immediately with `process::exit`, without further saves. A FAILED deletion
  (file open elsewhere, permissions) does NOT close the app or fake success: the
  card shows the error, writes resume, and a retry can succeed.
- **Shortcuts** holds the read-only keyboard shortcut reference in English
  (Tabs / Navigation / Page / Zoom), mirroring the table below.

Every control sits centered — dropdowns, zoom pill and switches share one right
edge — and every dropdown row keeps its geometry on hover, so nothing shifts.
`Esc` closes an open dropdown first, the dialog on the next press.

## History

Clock icon at the bottom of the tab rail: `about:history`, rendered in a tab
like a document. A capsule search field (magnifier, clear X) filters by URL and
title. Visits group into rolling date sections — Today / Yesterday / Previous 7
days / Previous 30 days / Older — each row with its favicon (shared session
cache; a globe for web pages, a file page for local ones while it resolves or
when absent), title, relative age and a remove X (the accessible name carries
the URL). The header prints the total and retention window; the footer repeats
the count ("N of M pages" while filtering) beside the two-click delete-all.

The button (and `Ctrl+H` / `Cmd+Y`) opens it in a NEW tab right after the
current one, never replacing it; clicking a row navigates that same history tab
to the visit, so Back returns to the list.

History is stored encrypted on this device only: the per-user data directory
(macOS `~/Library/Application Support/MDowser`, Linux `$XDG_DATA_HOME/mdowser`,
Windows `%LOCALAPPDATA%\MDowser`), bounded to 20 000 entries / 8 MiB, written
atomically, pruned by retention. "Forget on exit" keeps visits in memory for the
session only and wipes leftovers.

## Third-party licenses

"View third-party licenses" at the bottom of Preferences (or typing
`about:licenses`) opens the licenses of linked crates and bundled fonts: the
exact `THIRD_PARTY_NOTICES.md` in every release archive, embedded in the binary
and rendered with the browser's own Markdown engine. Selection, copy, the
right-click menu and links work like a document; relative links (to the
repository's license files) are inert offline, absolute `https:` links live. The
page opens in a new tab and session restore brings it back like `about:history`.

## Bookmarks

`Ctrl/Cmd+D` or the toolbar ribbon toggles the loaded page (a toast confirms;
the ribbon fills when bookmarked). Home shows a "quick access" tile grid (newest
first, responsive columns) whose interaction (glyph priority, rename, drag and
keyboard reorder, remove) is under [Home](#home). Names get up to two centered
lines, elided with `…` when they do not fit (long paths and CJK included); the
grid scrolls past three tile rows, its floating scrollbar appearing only then.
The editor is inline (no window): the pencil renames (check or Enter saves, Esc
cancels), a drag reorders (`Alt+Left`/`Alt+Right` moves a focused tile one
display slot), the X removes; any stored glyph or color keeps rendering.

Bookmarks are encrypted on this device only (`bookmarks.bin` + shared
`store.key`, ChaCha20-Poly1305, the same directory as History), kept until
removed (no retention window), bounded to 2000 entries; an add at the cap is
refused rather than dropping an older one. A first run starts with a curated set
of five (Markdown guides, an agent-readiness leaderboard and developer-service
docs); the rest lives only in the omnibox builtin suggestions, and deleting them
never brings them back. Favicons load through the usual tile pipeline (one probe
per origin on first display).

## Reading surface

Content: a fluid centered column (max 760px, 8/12/16px gutters) with an
always-visible scrollbar (a muted-text handle at 0.75 dormant opacity, at
least 3:1 against both themes: it is an operable control, not decoration). A
`spacing::PAGE_END` (24px) blank pad closes every
scrolling page under its last element (documents, Home, the error view), so the
end keeps a small margin. The previous page stays visible until the new one
resolves (no white flash).

Render: responsive H1–H6 in the README scale with H1/H2 bottom rules; selectable
wrapping paragraphs at README line height.

- One large `·` bullet hugging the text column inside a fixed 24px gutter for
  every unordered item (any source marker, `-`, `*` or `+`, at every depth),
  hanging-indented so wrapped rows stay in the text column and the bullet on the
  first text row.
- Compact item spacing, and `N.` lists with real read-only checkboxes for
  tasks.
- Inline code pills at body size: `.2em .4em` padding as layout air both sides,
  body baseline, linked code included, one tight pill per row when a span wraps;
  height capped at the body line minus 3px so the bundled mono cut's tall em box
  cannot inflate it into a full line.
- Code cards (language chip + Copy button that confirms with a brief `Copied`
  state + lightweight per-line highlighting, no line numbers) at the same body
  size as the prose.
- Striped bordered tables fill the drawn card (leftover width distributed
  proportionally); long cells (links included) shrink and wrap instead of
  forcing a horizontal scrollbar (a scrolling table keeps readable column
  widths), with unique ids, per-column alignment, collapsed interior separators
  reaching the card border and 5px/11px cell padding.
- Muted quotes in a soft full-width box with ONE thick bar (neutral for plain
  quotes, the alert accent for `> [!NOTE]`/`TIP`/`WARNING` callouts, whose body
  keeps normal text color), 8px of padding around the body and the same
  vertical air above the box as below it, so a quote right after a list or
  paragraph never crowds it.
- SVG badges/logos, emoji, CJK, RTL and bundled script faces, and real images
  (hourglass placeholder while loading, a warning icon with the reason as
  tooltip on failure, centered captions, natural-size images clamped to the
  reading column). Linked images draw the shared keyboard focus ring like any
  chrome control, in every load state, placeholders included.
- Footnote references are small raised NUMBER markers (linked to the note,
  numbered by first reference); notes form a footnotes section: one rule before
  the first note, one numbered entry per note whose marker shares the first line
  with the note body (continuation blocks hang under the text; a first block
  that cannot flow inline keeps the marker above, like a list item), and a
  back-arrow closing the note text and scrolling to the reference. Definitions
  the document never references still render. `Copy` keeps the source `[^label]`
  spelling. See [`RENDERING.md#footnotes`](RENDERING.md#footnotes).

Every surface draws bundled Noto typefaces only: proportional text (document,
chrome, menus) uses Noto Sans variable; inline code and code cards use the
bundled Noto Sans Mono CJK JP cut (face 5 of the CJK collection) at BODY size,
so code is exactly as wide as the prose (`theme::BODY_FONT_SIZE` backs both;
code cards are tall for the body 24px line). Bold is a real face
(`theme::strong`): headings, `**strong**` runs, callout titles and footnote refs
use Noto Sans variable pinned to `wght = 700`. Body text is the high-contrast
`on_surface` token; muted H6 and plain quote bodies opt into the muted tone. Raw
HTML is never painted or interpreted: every block-level or inline tag is a
transparent wrapper (the tag disappears, its content renders as Markdown).
Inert: `<pre>`/`<textarea>` (bytes are not Markdown; muted monospace) and
malformed/unbalanced markup, also literal. HTML comments (`<!-- ... -->`) are
invisible, block and inline: they never paint and `Copy` omits them.

## Error screen

A failed load (or local rejection) renders a styled error page in the reading
sheet, not a raw log:

- A colored status dot and a family title: No connection, Page not found, No
  markdown here, Invalid address, Could not load (classified from the failure
  log; local-file failures use the local wording). The dot is centered on the
  laid-out title. Then an explanation line, the requested address with its
  scheme hidden, a "What you can do" hint card and a Details section.
- "No markdown here" (the target answered but served no Markdown) offers a
  large accent "Open in your browser" action under the requested address: it
  hands the resolved address to the OS default handler — default browser for web
  pages, the file's default app for local files (labeled "Open with the default
  app"). The address passes back through the app's navigability parser, so
  pasted credentials are stripped before the OS sees it; no other family offers
  the action.
- Details is a bounded scroll of one monospace line per route candidate, in
  tried order, `https://`/`file://` stripped and userinfo removed. The box takes
  the log's own height (up to 320px), so a short log does not reserve empty
  space below it. A "Copy log" button copies that text prefixed by
  `Requested: <address>`.
- The whole error screen uses the same centered reading column as a document
  (up to 760px), so "Copy log" and Retry stay on the text's side of the sheet
  rather than on the window borders.
- A Retry button re-runs the page under the same 5s cooldown as Refresh
  (anti-hammer): while cooling down it is disabled and its tooltip counts the
  seconds. A local address rejection (invalid URL, non-`https`) can never
  succeed by retrying and shows a "fix the address" hint instead of a countdown.

## Right-click menu

Minimal: Copy (the drag selection when one exists, else the whole page), Select
all, Find in page (opens the find bar, same as `Ctrl/Cmd+F`) and Save page
(`Ctrl/Cmd+S`), below a separator; over a link it adds Open link in new tab at
the top. `Cmd/Ctrl+click` on a link opens it in a background tab directly (an
in-page `#anchor` becomes its absolute URL plus the fragment). Prose, headings,
lists, quotes and table cells are natively selectable (drag + `Cmd/Ctrl+C`)
with readable selection colors. A highlighted code block is virtualized and
painted row by row, so it has no per-line drag selection; its Copy button (or
menu Copy with no selection) copies the whole block verbatim and briefly
confirms with `Copied`. Right-clicking preserves an existing selection (its
press is intercepted before egui), so menu Copy copies exactly the selected
text.

Opening the menu with the pointer leaves NO row highlighted (only the keyboard
lights one). A primary press outside it — including the start of a drag
selection — closes it. The menu is keyboard-navigable: `Down`/`Up` move focus
into the rows (the first press lands on the first), `Enter` activates the
focused one, `Esc` closes.

## Keyboard shortcuts

The global shortcuts; field, menu and grid keys live in their sections above.
A focused bookmark tile keeps `Alt+Left`/`Alt+Right` for its own reorder, and
a focused text field keeps `Alt`+arrows for word movement. The Preferences
Shortcuts tab carries them in English as a curated quick reference, leaving
out bare `Esc` rows and keyboard reorders.

| Shortcut | Action |
| --- | --- |
| `Ctrl/Cmd+T` | New tab |
| `Ctrl/Cmd+W` | Close tab |
| `Ctrl/Cmd+Shift+T` | Reopen the most recently closed tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` (also `Ctrl+PageDown` / `Ctrl+PageUp`) | Next / previous tab (wraps) |
| `Alt+Left` / `Alt+Right` (macOS also `Cmd+[` / `Cmd+]`) | Back / forward |
| `Ctrl/Cmd+R` / `F5` | Refresh |
| `Ctrl/Cmd+L` | Focus address field (selects the URL) |
| `Esc` | One surface per press: a focused field/editor (revert / drop draft), else a popup or menu, a floating window, the find bar, or Stop loading |
| `Ctrl/Cmd+F` | Find in page (document pages only) |
| `Ctrl/Cmd+S` | Save the page as Markdown (document pages only) |
| `Enter` / `Shift+Enter` (field focused) · `F3` / `Shift+F3` (anywhere) | Next / previous find match |
| `Ctrl/Cmd+A` | Select all (page text; when no text field is focused) |
| `Ctrl/Cmd+C` | Copy (selection, or all while that highlight is active) |
| `Ctrl/Cmd+1..9` | Select tab |
| `Alt+Up` / `Alt+Down` | Move the focused tab one slot (reorder the rail) |
| `Ctrl/Cmd` `+`/`=` | Zoom in |
| `Ctrl/Cmd+-` | Zoom out |
| `Ctrl/Cmd+0` | Reset zoom to 100% |
| `Ctrl+H` on Windows/Linux, `Cmd+Y` on macOS | History (new tab after the current one; no-op if it already shows it) |
| `Ctrl/Cmd+D` | Bookmark the current page (toggle) |
| `Ctrl/Cmd+,` | Preferences window |

## Find in page

`Ctrl/Cmd+F` (or the right-click "Find in page" row) opens a floating bar over a
document page (a loaded document or the embedded licenses page; Home, history,
loading and error views have nothing to search). It uses the chrome kit: an
omnibox-style capsule (soft chrome fill, magnifier glyph, frameless field
focusing with the accent ring), the `n/m` count in a fixed muted slot and
circular previous/next/close buttons, identical in hover fill, glyph animation
and tooltips to the toolbar's (lighter shadow than menu popups, since the bar
stays on screen). On a document with an outline sidebar, the bar docks just
left of the sidebar instead of covering it. The query field shrinks on narrow
windows and high zoom to keep every control usable; if the fixed-width bar
still cannot fit beside the outline, it moves over the outline rather than
clipping the previous/next/close buttons.

Matching is a case-insensitive substring over rendered text — prose, headings,
list items, quotes, tables and fenced code bodies — every match washed amber,
the current one stronger, code blocks and table cells included (a code block
washes its matching rows and marks the current row with the stronger wash); the
sheet scrolls the current match into view even inside a code card or a table's
horizontal scroll area.

Searching never changes the layout, not by a pixel: matches ride on the text the
unsearched render lays out, the wash painted behind it (color emoji keep their
color, wash behind). `Enter` steps forward (`Shift+Enter` back; `F3`/`Shift+F3`
work unfocused) and the sheet scrolls the match into view — down or up —
retrying until on screen, unless the reader scrolls meanwhile. `Esc` closes the
bar — unless a focused field, the page-info card, a popup/menu or a floating
window owns that press, so one `Esc` never acts on two surfaces. The bar keeps
its query for reopening: `Ctrl/Cmd+F` focuses the field with it SELECTED, so
typing replaces it. The bar is an overlay (never shifts the document) and closes
on navigation or tab switch with the rest of the overlay state.

## Document outline

Every document page carries an **always-visible outline sidebar** on the
right: narrow (176-240px, proportional to the window), part of the reading
layout so the sheet reflows beside it, and full height. It lists the document's
headings in document order at the depth the document wrote them: every heading
starts one indent step below the shallowest level present (a document whose
headings start at `##` wastes no step), long titles WRAP onto as many lines as
they need (the full text also stays in the hover tooltip and the accessible
name), and a heading that slugs to nothing still gets a navigable row (anchors
reserve a positional id for it). The row of the section being read carries the
accent bar; the list keeps that row in view as the reader scrolls and scrolls
on its own when the outline is long. The cursor only names a heading that is
actually laid out: one hidden in a collapsed section or an unselected tab is
skipped until revealed, and a page whose headings are all hidden lights no
row. Across the page's last screenful the reading line descends from the
viewport top to its bottom as the sheet approaches its maximum offset, so
headings that can never reach the top (the document's final sections) still
become current in order; at the very bottom the deepest visible section is
current. Scrolling back up raises the line again, walking those last rows in
reverse.

The slot stays on every document page, including one without headings (a short
"No headings" state replaces the rows) and the page still visible while the
next one loads ("Loading..."), so navigation never widens or narrows the
reading column.

A row click scrolls its heading to the top of the sheet through the same
mechanism as a `#anchor` link click. If the heading lives in a closed
`<Accordion>` or an unselected `<Tabs>` panel, that container opens/selects
first and the heading then consumes the same request; a cross-page `#fragment`
into such content does the same. The scrollspy accounts for the page END: its
reading line descends through the last screenful, so a dense cluster at the
foot of the page is walked row by row and headings the sheet can no longer
scroll to the top still become current. A just-clicked row stays selected
while its requested scroll runs and, once the sheet stops, whenever the spy has
not landed on it: a page-end row the moving line crossed on the way down stays
selected even though the deepest visible section is current. The row is
released when the spy reaches it at rest after its requested scroll has
finished (its heading at the top, or the sheet at its maximum offset), or the
reader scrolls by hand; wheel and scrollbar scrolling then hand the cursor
back to the spy, including the reverse walk through the last rows.
The sidebar has no close control and does not own `Esc`; navigation and tab
switches reset its cursor with the rest of the page state. The find bar docks
just left of the sidebar instead of covering it.

## Save page

`Ctrl/Cmd+S` (or the right-click "Save page" row) writes the document page on
screen as **Markdown** — the source exactly as fetched, frontmatter included —
straight into the system downloads folder. There is no dialog and no format
choice: the file is `<page title>.md` in the OS downloads folder — the
Downloads known folder on Windows (so a location relocated in Explorer is
honored), the XDG downloads folder when one is configured and present
(`XDG_DOWNLOAD_DIR` in `user-dirs.dirs`, so a localized name like `Descargas` is
honored on Linux/BSD), else `~/Downloads` when that exists, else the home
directory. The name is the page title's slug, else the URL's last path
segment, else `document`.

An existing file is never overwritten. A repeated save of the same page picks
the first free browser-style copy name (`notes.md`, `notes (1).md`, …), so
versions accumulate instead of replacing each other, and a completed save names
the file in a toast (`Saved notes.md`). When the downloads folder refuses the
write — the macOS "Downloads folder" permission prompt denied, a read-only
folder, a filesystem policy — the save completes in the home directory instead
and the toast says where (`Saved notes.md in home (Downloads needs
permission)`), so a permission prompt never turns into a lost page. Any other
failure reports itself in the same toast. On filesystems with hard links the
save is atomic: the file is written to a unique sibling temp file, synced to
disk and published with a no-replace link, so an interrupted or failed save
never leaves a truncated document and a completed save survives a power loss.
Filesystems without hard links use an exclusive in-place create instead (also
no replacement). Either way a file that appeared after the name check is never
replaced. No native save dialog is involved.
