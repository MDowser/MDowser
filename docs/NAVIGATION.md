# Navigation

How MDowser resolves what you type or open, and what local files mean.

See also: [`INTERFACE.md`](INTERFACE.md) for omnibox UI and suggestion
interaction, and [`SECURITY.md`](SECURITY.md) for caps and scheme policy.

## Routes (in order)

For a logical `/love` page, candidates in order:

1. `/love`
2. `/love/`
3. `/love.md`
4. `/love/index.md`
5. `/love/README.md`

Only these spellings are fetched: no `.markdown` extension or
`readme`/`index.markdown` case variants. Legacy `.markdown` URLs normalize to
the same page and resolve here. Home (`/`) resolves `/`, `/index.md` and
`/README.md`.

The navbar shows the normalized form without the scheme (`ex.com/love/`),
whichever loaded; requests carry `Accept: text/markdown;q=0.8`. A line-by-line
error explains each attempt, also without `https://`.

Redirects (`301`, `302`, `303`, `307`, `308`) follow transparently, relative
`Location` included, up to 8 hops per attempt (`config::MAX_REDIRECTS`; a
longer chain stops there); one leaving `https` is rejected, never fetched. The **final** response must still be
Markdown; one landing on HTML is dropped like any wrong `Content-Type`. Navbar
and history show the landing URL (`ex.com/hola` found at `ex.com/hola/` shows
`ex.com/hola/`).

Repeat visits consult the encrypted disk cache: a fresh record is served
without the network; a stale one with validators revalidates with
`If-None-Match`/`If-Modified-Since` (a 304 refreshes it). Refresh bypasses the
cache record, re-fetching through the route cascade with
`Cache-Control: no-cache` and never sending stored validators; a fresh copy
always comes from the network. A failed revalidation falls back to the
cascade; a stale body is never served as current.

## Autocomplete

Both address fields (toolbar, Home) suggest pages while editing: bookmarks first
(newest first), then local-history visits (newest first), then the curated
builtin list embedded in the binary (`builtin_sites::builtin_suggestions()`,
vendored from MDowser-Sites by `make sync-sites` into
`assets/builtin_sites.txt`, one `<url> | <title>` line per entry). Matching is
case-insensitive on URL and title, scheme optional, up to
`config::MAX_OMNIBOX_SUGGESTIONS` rows; an empty field offers recent bookmarks.
Suggestions use only the encrypted History and Bookmarks stores (nothing new
from disk) and cache until the query or a store changes.

The full interaction contract (no initial highlight, `Down`/`Up`, `Tab`,
`Enter`, click, `Esc`):
[`INTERFACE.md#autocomplete`](INTERFACE.md#autocomplete).

## Local files

Local targets come only from user input or links inside an already-local
document:

- `file:///path/notes.md`
- an absolute path (`/Users/you/notes.md`)
- a home path (`~/notes.md`)
- an explicit relative path (`./notes.md`, `../notes.md`)
- a Windows drive path (`C:\notes.md`)

Resolution is simpler:

- An explicit file opens exactly that file; a stem without extension gets
  `.md` when the exact path does not exist.
- A `file:` URL must be written absolutely (`file:///x`, `file:/x`); the
  relative `file:notes.md`, which the URL parser would silently rebase onto the
  filesystem root (`/notes.md`), is rejected as a bad address, and a `file:`
  link in a local document stays inert.
- A directory (or a path ending in `/`) resolves through `index.md` and
  `README.md`.
- A remote `file://host/...` share is rejected: only files on this device.
- Relative links and images resolve against the document's directory;
  `https` links open normally (a local page is not sandboxed from the web).
- Reads share the network caps (10 MiB, UTF-8 mandatory, image caps) and
  cooperative cancellation; no directory listing.
- Local visits land in History like web pages; the navbar shows the decoded
  path with a page indicator instead of the padlock.

## Opening documents

Command-line targets (or a file manager's "Open with") open as documents:
absolute paths and `file://`/`https://` URLs, one tab each, taking priority
over session restore.

```sh
mdowser ~/notes.md                  # from a terminal
mdowser file:///tmp/a.md            # file URL
open -a MDowser --args ~/notes.md   # macOS, while the app is not running
```

Platform integration:

- **Linux:** the `.deb`, `.rpm`, Flatpak build and `make install` ship one
  shared `.desktop` entry (`packaging/linux/mdowser.desktop`) declaring
  `MimeType=text/markdown;text/x-markdown;application/markdown;application/x-markdown;`
  and `Exec=mdowser %U`, so file managers list MDowser for `.md` files (set it
  as default in file associations). The tarball ships no `.desktop`; that needs
  the entry and hicolor icons installed by hand.
- **Windows:** the zip has no installer, so register the file type once via
  "Open with > Choose another app > MDowser" with "Always" checked;
  double-clicking a document then starts MDowser with the path as argument.
- **macOS:** the `.app` declares no document types: Finder double-click hands it
  over as a macOS Apple Event, which eframe/winit do not expose (the app keeps
  `unsafe_code = "forbid"`), so use `mdowser path/notes.md` or
  `open -a MDowser --args path/notes.md`.
