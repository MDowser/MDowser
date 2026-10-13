<div align="center">

<img src="assets/icons/icon-256.png" width="120" alt="MDowser icon">

<h1>MDowser</h1>

<p><strong>The plain-text web, rendered natively.</strong><br>
A minimal, fast, cross-platform browser for Markdown over HTTPS and local files.<br>
No webview, no JavaScript, no cookies, no cache.</p>

<p>
  <a href="#install"><img src="https://img.shields.io/badge/platforms-macOS_%7C_Linux_%7C_Windows-lightgrey" alt="Platforms: macOS, Linux, Windows"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.98%2B-orange" alt="Rust 1.98+"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="License: MPL-2.0"></a>
  <a href="docs/CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen" alt="PRs welcome"></a>
</p>

</div>

MDowser is a native desktop browser for the plain-text web. Paste an
`https://` URL or open a local Markdown file (`file:///…`, an absolute path
or `~/notes.md`) and it fetches or reads it, parses it once, and renders it
with a clean, desktop-browser-style UI. The surface is deliberately small
and auditable: one document format, one HTTP method, one network protocol —
any static Markdown host just works.

## Highlights

- **GitHub-grade Markdown.** GFM + CommonMark (tables, footnotes, tasklists,
  strikethrough) and YAML frontmatter, drawn with GitHub's Primer palette —
  code cards with syntax colors and a Copy button, striped bordered tables,
  callouts and README typography. HTML, JSX and `:::` custom containers
  render at the markup level only: tags are transparent wrappers, MDX
  components (`<Tabs>`, `<Accordion>`, `<Card>`, `<Steps>`, `<CodeGroup>`,
  `<Badge>`) and their `:::` equivalents (`:::tabs`, `:::code-group`,
  `:::steps`, `:::card`, `:::accordion`/`:::details`) draw natively, and
  `:::`/`!!!` admonitions become callouts — no JavaScript is ever executed.
  → [`docs/RENDERING.md`](docs/RENDERING.md)
- **Real browser chrome.** Vertical tab rail, omnibox with address
  autocomplete (bookmarks, local history and a curated builtin list) and
  favicons, encrypted local history and bookmarks, zoom, light/dark theme,
  preferences and the internal `about:home` / `about:history` /
  `about:licenses` pages.
  → [`docs/INTERFACE.md`](docs/INTERFACE.md)
- **Documents as they are.** Color emoji, CJK and RTL scripts, images in
  PNG/JPEG/GIF/WebP/JPEG XL/AVIF/ICO/static SVG — decoded in pure Rust. In-page
  anchors, footnotes and bare-URL linkification included.
- **Private by design.** `https://` only (never `http://`), `GET` only, no
  cookies, cache, proxy, webview or JavaScript; local files only from user
  input. A remote page can never name or load a local path.
  → [`docs/SECURITY.md`](docs/SECURITY.md)
- **Small and auditable.** One binary, `unsafe_code = "forbid"`, hard caps
  on every input, and a single document format to reason about. Rendering
  is incremental: code blocks draw only the lines on screen, tables
  fingerprint once per document, and the palette is interned.
  → [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

## Install

Prebuilt archives for macOS, Linux and Windows are attached to every
[release](../../releases):

| Platform | Download |
| --- | --- |
| macOS 13+ | `MDowser-<ver>-macos-universal.app.zip` (Apple Silicon + Intel) |
| Linux | `mdowser-<ver>-linux-<arch>.tar.gz`, the matching `.deb` / `.rpm`, or the `mdowser-<ver>.flatpak` bundle |
| Windows | `mdowser-<ver>-windows-x86_64.zip` |

- **macOS**: the `.app` is ad-hoc signed, so the first launch may need
  right-click → Open.
- **Linux**: `sudo dpkg -i mdowser_<ver>_amd64.deb` (or `arm64`), or
  `sudo rpm -i mdowser-<ver>-*.rpm`, or `flatpak install mdowser-<ver>.flatpak`.
  The package installs the desktop entry, hicolor icon and AppStream metadata,
  so MDowser appears in the applications menu and its Software-center page.
- **Windows**: unzip and run `mdowser.exe`; see
  [`docs/NAVIGATION.md`](docs/NAVIGATION.md) to register `.md` files.

`SHA256SUMS.txt` travels with every release.

## Build from source

```sh
make        # deps + host release + current-OS dist into dist/
            #   macOS: universal .app.zip | Linux: tar.gz/.deb/.rpm/flatpak | Windows: .exe
make dist   # deps + host release + EVERY platform into dist/ (macOS, Linux, Windows)
make install   # install into /usr/local (PREFIX/DESTDIR honored; `make dist-rpm` for RPM)
make run    # debug build + run
```

`make` and `make dist` install what the host is missing first (`make deps`;
`SKIP_SETUP=1` opts out). Requires stable Rust ≥ 1.98 and a C compiler (plus
NASM on x86/x86_64). Full requirements, the `make test` pipeline and multi-OS
`make dist` packaging: [`docs/BUILDING.md`](docs/BUILDING.md).

## Usage

```sh
mdowser ~/notes.md                     # local document
mdowser https://example.com/readme.md  # remote document
```

Type an address or path in the omnibox, or open a file with the app. Every
navigation stays inside MDowser; `Esc` stops a load. See
[`docs/NAVIGATION.md`](docs/NAVIGATION.md) for the route cascade and local
file rules.

| Shortcut | Action |
| --- | --- |
| `Ctrl/Cmd+T` · `Ctrl/Cmd+W` | New tab · close tab |
| `Ctrl/Cmd+L` | Focus the address field |
| `Ctrl/Cmd+R` / `F5` | Refresh |
| `Ctrl+H` / `Cmd+Y` | History (opens in a new tab) |
| `Ctrl/Cmd+D` | Bookmark the current page |
| `Ctrl/Cmd` `+` / `-` / `0` | Zoom in / out / reset |
| `Ctrl/Cmd+,` | Preferences |

All shortcuts: [`docs/INTERFACE.md`](docs/INTERFACE.md).

## Documentation

- [`docs/RENDERING.md`](docs/RENDERING.md) — everything Markdown can put on screen.
- [`docs/INTERFACE.md`](docs/INTERFACE.md) — the desktop UI, element by element, plus all shortcuts.
- [`docs/NAVIGATION.md`](docs/NAVIGATION.md) — URL routes, redirects, local files and file associations.
- [`docs/BUILDING.md`](docs/BUILDING.md) — toolchain, `make` pipeline, packaging and runtime requirements.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — module map, contracts and the performance model.
- [`docs/SECURITY.md`](docs/SECURITY.md) — threat model, hard caps and vulnerability reporting.
- [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) — quick start and code standards.
- [`CHANGELOG.md`](CHANGELOG.md) — every notable change and fixed bug.
- [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) — licenses of the bundled dependencies and fonts (also rendered in-app: Preferences → View third-party licenses).

## Security

The network contract is deliberately tiny: `https://` only, `GET` only, one
Markdown content type, no cookies, cache or proxy, and local files only from
user input (or links inside a local document) — a remote page can never
name or load a local path. Every read
and decode path is hard-capped (10 MiB documents, 16 MiB / 48 MP images,
1M events, bounded encrypted history and bookmarks), SVG is rasterized
statically and `unsafe_code = "forbid"`. The full threat model and caps are
in [`SECURITY.md`](docs/SECURITY.md).

## Roadmap

- Optional cache (deliberately cacheless today).
- Per-block unique indent ids, tab drag-reorder.

## License

Mozilla Public License 2.0 (`MPL-2.0`) — see [`LICENSE`](LICENSE).
