# Navigation

How MDowser resolves what you type or open, and what local files mean.

## Routes (in order)

For a logical `/love` page:

1. `/love`
2. `/love/`
3. `/love.md`
4. `/love/index.md`
5. `/love/README.md`

Only the standard spellings are ever fetched: no `.markdown` extension and
no `readme`/`index.markdown` case variants. Legacy `.markdown` URLs still
normalize to the same logical page, so they resolve through this cascade
instead of failing.

Home (`/`): `/`, `/index.md`, `/README.md`.

The navbar shows the normalized form without scheme (`ex.com/love/`),
whichever candidate loaded. Requests carry `Accept: text/markdown;q=0.8`.
When nothing works, a line-by-line error explains each attempt (also without
`https://`).

Redirects (`301`, `302`, `303`, `307`, `308`) are followed transparently,
relative `Location` included, up to 8 hops per attempt
(`config::MAX_REDIRECTS`; a longer chain stops there). A redirect that leaves `https` is rejected, never fetched. The
**final** response must still be markdown: a redirect that lands on HTML is
dropped like any other wrong `Content-Type`. The navbar and history show
where the load landed (`ex.com/hola` found at `ex.com/hola/` shows
`ex.com/hola/`).

Repeat visits consult the encrypted HTTP cache first: a fresh record is
served without the network, a stale one revalidates with
`If-None-Match`/`If-Modified-Since`, and Refresh forces that revalidation
(or a full cascade when the response carried no validator). A failed
revalidation falls back to the normal route cascade; a stale body is never
served as if it were current.

## Autocomplete

Both address fields (the toolbar and the large Home capsule) suggest pages
while editing: bookmarks first (newest first), then local-history visits
(newest first), then the curated builtin list embedded in the binary
(`builtin_sites::builtin_suggestions()`, vendored from MDowser-Sites with
`make sync-sites` into `assets/builtin_sites.txt`: one `<url> | <title>`
line per entry).
Matching is case-insensitive on URL and title and works with or
without the scheme, up to `config::MAX_OMNIBOX_SUGGESTIONS` rows; an empty
field offers recent bookmarks. No row is highlighted until you pick one: the
first `Down` selects the first row, the first `Up` the last, and later
presses move through the list. `Tab` completes the highlighted row without
navigating (with nothing highlighted it completes the first row) and `Esc`
closes the popup before reverting the field. `Enter` submits the address you
TYPED unless you explicitly picked a row with `Down`/`Up` or moved the
pointer onto one (a pointer that merely rests over the popup is not a pick,
and typing clears an earlier pick), and a click on a row opens that row.
Suggestions come from the same encrypted stores as History and Bookmarks
(nothing new is read from disk), and the list is cached until the query or
a store changes.

## Local files

Local targets are accepted only from user input or from links inside an
already-local document: `file:///path/notes.md`,
an absolute path (`/Users/you/notes.md`), a home path (`~/notes.md`), an
explicit relative path (`./notes.md`, `../notes.md`) or a Windows drive path
(`C:\notes.md`). Resolution is simpler than the web cascade:

- An explicit file opens exactly that file; typing a stem without extension
  completes `.md` when the exact path does not exist.
- A `file:` URL must be written absolutely (`file:///x`, `file:/x`): the
  relative spelling `file:notes.md`, which the URL parser would silently
  rebase onto the filesystem root (`/notes.md`), is rejected as a bad
  address (and a `file:` link inside a local document stays inert).
- A directory (or a path ending in `/`) resolves through
  `index.md` and `README.md`.
- A remote `file://host/...` share is rejected: only files on this device.
- Relative links and images resolve against the document's directory;
  `https` links open normally (a local page is not sandboxed from the web).
- Reads share the network caps (10 MiB, UTF-8 mandatory, image caps) and the
  same cooperative cancellation; there is no directory listing.
- Local visits are recorded in History like web pages, and the nav bar shows
  the decoded path (with a page indicator instead of the padlock).

## Opening documents

Open targets passed on the command line (or by a file manager's "Open with")
are opened as documents: absolute paths, `file://` URLs and `https://` URLs
are all accepted, one tab each, and they take priority over session restore.

```sh
mdowser ~/notes.md            # from a terminal
mdowser file:///tmp/a.md      # file URL
open -a MDowser --args ~/notes.md   # macOS, while the app is not running
```

- Linux: the `.deb`, `.rpm`, Flatpak build and `make install` all ship one
  shared `.desktop` entry (`packaging/linux/mdowser.desktop`) that declares
  `MimeType=text/markdown;text/x-markdown;application/markdown;application/x-markdown;`
  and `Exec=mdowser %U`, so file managers list MDowser for `.md` files (choose
  it as default in the desktop's file-association settings). The tarball ships
  no `.desktop`, so the same integration needs the entry (and the hicolor
  icons) installed by hand.
- Windows: the zip has no installer, so register the file type once via
  "Open with > Choose another app > MDowser" and check "Always". Double-click
  then starts MDowser with the file path as its argument.
- macOS: the `.app` deliberately declares no document types. Finder
  double-click hands the file over as a macOS Apple Event, which
  eframe/winit do not expose (keep the app's `unsafe_code = "forbid"`), so
  use `mdowser path/notes.md` or `open -a MDowser --args path/notes.md`.
