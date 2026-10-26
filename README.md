<div align="center">

<img src="assets/icons/icon-256.png" width="120" alt="MDowser icon">

# 🌐 MDowser: The Markdown Browser

**A cleaner web should not be a privilege reserved for AI agents. People deserve it, too.**

Read the web as text-first documents; without a webview, JavaScript, cookies or trackers.

[**⬇ Download MDowser**](https://github.com/MDowser/MDowser/releases)

<p>
  <a href="#download"><img src="https://img.shields.io/badge/download-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey" alt="Download for macOS, Linux and Windows"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.99%2B-orange" alt="Rust 1.99+"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="License: MPL-2.0"></a>
</p>

</div>

---

## The web is noisy. Reading it doesn't have to be.

Modern web pages often bring far more than the document you came to read: scripts, interfaces, trackers and layers of machinery between you and the information. AI agents are increasingly being given cleaner ways to access content. **That should not mean humans are left with the clutter.**

**MDowser brings the plain-text web to your desktop.** Enter an `https://` address or open a local Markdown file, and MDowser renders the document in a fast, native interface designed for reading. No embedded web browser. No JavaScript execution. No cookies or trackers. An encrypted, bounded disk cache keeps repeat visits fast and stays under your control. Just the content, presented clearly.

MDowser is deliberately focused: one document format, one HTTP method and one network protocol. There is no web stack to run scripts or reproduce an entire website. If a host serves a Markdown document over HTTPS, MDowser can read it.

**For people who write, learn, research, build and read. For anyone who wants information without the noise.**

[**Get MDowser from GitHub Releases →**](https://github.com/MDowser/MDowser/releases)

## Why MDowser?

- **Markdown, rendered natively.** CommonMark with GFM-style tables, footnotes, task lists and strikethrough, plus YAML frontmatter. Code blocks include syntax coloring and a Copy button; tables, callouts and documentation layouts are rendered by MDowser itself. HTML, JSX and `:::` containers are treated as markup rather than executable application code. MDX components such as `<Tabs>`, `<Accordion>`, `<Card>`, `<Steps>`, `<CodeGroup>` and `<Badge>` — along with supported `:::` equivalents — render natively, without running JavaScript. [See rendering details.](docs/RENDERING.md)

- **A real desktop-browser experience.** Vertical tabs, an address bar with autocomplete, favicons, encrypted local history and bookmarks, an encrypted HTTP cache for repeat visits (with clearly marked offline/saved copies), a document outline sidebar, one-key Markdown saving, zoom, light and dark themes, preferences and built-in `about:home`, `about:history` and `about:licenses` pages. Local files and folders can also be dropped straight onto the window. [Explore the interface.](docs/INTERFACE.md)

- **Your documents, not a stripped-down text dump.** Images, color emoji, CJK and right-to-left scripts, in-page anchors, footnotes and automatic links for bare URLs are supported. Image formats include PNG, JPEG, GIF, WebP, JPEG XL, AVIF, ICO and static SVG, decoded in pure Rust.

- **Privacy by design.** HTTPS only — never plain HTTP. GET requests only. No JavaScript, webview, cookies or proxy. Responses are cached only in an encrypted, bounded store that Preferences can clear per class and "Forget on exit" disables. Local files are opened only through user input or links inside a local document. A remote page cannot specify a local file path. [Read the security model.](docs/SECURITY.md)

- **Small enough to reason about.** One desktop application, `unsafe_code = "forbid"`, bounded inputs and a deliberately limited document format. Rendering is incremental: code blocks draw only visible lines, tables are fingerprinted once per document and the palette is interned. [See the architecture.](docs/ARCHITECTURE.md)

## Download

**Ready to try MDowser? Download the latest release for your operating system from the Releases page.** Each release includes the available prebuilt packages and a `SHA256SUMS.txt` file for checksum verification.

### [➡ Download MDowser from Releases](https://github.com/MDowser/MDowser/releases)

Choose the asset matching your system:

| Platform | Release asset |
| --- | --- |
| **macOS 13 or later** | `MDowser-<ver>-macos-universal.app.zip` — Apple Silicon and Intel |
| **Linux** | `mdowser-<ver>-linux-<arch>.tar.gz`, the matching `.deb` or `.rpm`, or the `mdowser-<ver>.flatpak` bundle |
| **Windows (x64)** | `mdowser-<ver>-windows-x86_64.zip` |
| **Windows on ARM** | `mdowser-<ver>-windows-aarch64.zip` |

Release filenames use the version number and architecture of the build. Open [**Releases**](https://github.com/MDowser/MDowser/releases), select a release and download the appropriate file from its **Assets** section.

### Platform notes

- **macOS:** The `.app` is ad-hoc signed, not notarized. Open it once so Gatekeeper registers the block, then allow it under **System Settings → Privacy & Security → Open Anyway**. On current macOS, right-clicking and choosing Open does not bypass this restriction.
- **Linux:** Install a Debian package with `sudo dpkg -i mdowser_<ver>_amd64.deb` (or `arm64`), an RPM with `sudo rpm -i mdowser-<ver>-*.rpm`, or the Flatpak bundle with `flatpak install mdowser-<ver>.flatpak`. The package installs the desktop entry, hicolor icon and AppStream metadata, so MDowser can appear in the applications menu and software center.
- **Windows:** Unzip the archive and run `mdowser.exe`. On Windows on ARM, use the `aarch64` archive; the x86_64 build can also run through emulation. The `.exe` is unsigned, so Windows SmartScreen may show a warning. If you choose to proceed, select **More info → Run anyway**. See the [navigation guide](docs/NAVIGATION.md) for registering `.md` files.

## Try it

Open a local document:

```sh
mdowser ~/notes.md
mdowser notes.md   # an existing relative file opens as a file, not a host
```

Or read a Markdown document hosted online:

```sh
mdowser https://example.com/readme.md
```

You can also type a URL or file path into the address bar, drop files or folders onto the window, or open a file from the application. Navigation stays inside MDowser, and `Esc` stops a load. `mdowser --help` prints the usage and `mdowser --version` the build, without opening a window.

For the URL-routing sequence, redirects and local-file rules, see [Navigation](docs/NAVIGATION.md).

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl/Cmd+T` · `Ctrl/Cmd+W` | New tab · close tab |
| `Ctrl/Cmd+Shift+T` | Reopen the most recently closed tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Alt+Left` / `Alt+Right` | Back / forward (macOS also `Cmd+[` / `Cmd+]`) |
| `Ctrl/Cmd+L` | Focus the address field |
| `Ctrl/Cmd+R` / `F5` | Refresh |
| `Ctrl/Cmd+F` | Find in page |
| `Ctrl/Cmd+S` | Save the page as Markdown in the downloads folder |
| `Ctrl+H` (Windows/Linux) · `Cmd+Y` (macOS) | Open history in a new tab |
| `Ctrl/Cmd+D` | Bookmark the current page |
| `Ctrl/Cmd` `+`/`=` / `-` / `0` | Zoom in / out / reset |
| `Ctrl/Cmd+,` | Open preferences |

See [Interface and shortcuts](docs/INTERFACE.md) for the complete list.

## Build from source

The Makefile handles dependencies and packaging:

```sh
make          # dependencies + current-OS packages in dist/
make dist     # dependencies + packages for macOS, Linux and Windows in dist/
make build    # host release binary in target/release/mdowser
make install  # install that binary into /usr/local (PREFIX/DESTDIR supported)
make run      # debug build and run
```

`make` and `make dist` install missing host dependencies first via `make deps`; set `SKIP_SETUP=1` to skip setup. If Rust itself is not installed yet, run `make deps` once first (the Makefile resolves the toolchain at startup). Requirements include stable Rust ≥ 1.99 and a C compiler. NASM is also required on x86/x86_64 hosts and for the x86_64 slice of a universal macOS build; ARM64-only builds use the system toolchain and do not need it. On native Windows without `make`, use `windows.ps1` as the equivalent entry point.

The build guide covers full requirements, the `make test` pipeline, multi-platform packaging and native Windows setup: [**Building MDowser**](docs/BUILDING.md#native-windows).

## Documentation

Want to understand a specific part of MDowser? Start here:

| Guide | What you'll find |
| --- | --- |
| [**Rendering**](docs/RENDERING.md) | Supported Markdown, code blocks, tables, callouts and native rendering behavior |
| [**Interface**](docs/INTERFACE.md) | Desktop UI, preferences and all keyboard shortcuts |
| [**Navigation**](docs/NAVIGATION.md) | URL handling, redirects, local files and file associations |
| [**Building**](docs/BUILDING.md) | Toolchain, Makefile pipeline, packages and runtime requirements |
| [**Architecture**](docs/ARCHITECTURE.md) | Module map, design contracts and performance model |
| [**Security**](docs/SECURITY.md) | Threat model, input limits and vulnerability reporting |
| [**Contributing**](docs/CONTRIBUTING.md) | Getting started and code standards |
| [**Changelog**](CHANGELOG.md) | Notable changes, improvements and bug fixes |
| [**Third-party notices**](THIRD_PARTY_NOTICES.md) | Licenses for bundled dependencies and fonts; also available in Preferences → View third-party licenses |

## Security by design

MDowser keeps its network and document-handling surface intentionally small:

- `https://` only, `GET` only, and a single Markdown content type.
- No JavaScript execution, webview, cookies or proxy. The only persistent network state is an encrypted, bounded HTTP cache that Preferences can clear per class and "Forget on exit" disables.
- Remote documents cannot name or load local paths; local files are opened only from user input or links in a local document.
- Hard limits on input and decoding: 10 MiB documents, 16 MiB / 48 MP images, 1 million events, and bounded encrypted history, bookmarks and cache.
- SVG is rasterized statically, and `unsafe_code = "forbid"`.

These restrictions are part of the design, not optional privacy switches. Read the full [**threat model and security limits**](docs/SECURITY.md).

## License

MDowser is distributed under the [Mozilla Public License 2.0 (`MPL-2.0`)](LICENSE).

## Trademarks

MDowser is an independent project and is not affiliated with, endorsed by or sponsored by any product, project or service it may mention. Third-party names and marks belong to their respective owners and are used only for identification or description. Bundled dependencies are listed in [Third-party notices](THIRD_PARTY_NOTICES.md).
