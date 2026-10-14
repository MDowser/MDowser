# Building MDowser

## Requirements

- Stable Rust toolchain ≥ 1.98 via `rustup` (provides `cargo`, `rustc`); the
  floor comes from the pure-Rust AVIF decoder (`avif-decode`). Everything
  else builds on older stable.
- System C compiler: Xcode Command Line Tools on macOS
  (`xcode-select --install`), `build-essential` on Debian/Ubuntu, MSVC or
  MinGW on Windows.
- NASM on x86/x86_64 hosts (Linux, Intel macOS, Windows): the AVIF decoder
  (`rav1d`) and the rustls crypto provider (`aws-lc-sys`) compile assembly.
  `brew install nasm`, `sudo apt-get install nasm` or (Windows, used by
  `windows.ps1`) `winget install NASM.NASM`.
- No `PATH` setup needed: the `Makefile` and `scripts/lib.sh` resolve the
  toolchain from `$HOME`-relative locations (`~/.cargo/env`, `~/.cargo/bin`,
  `rustup which cargo`, `brew --prefix`, toolchain glob) and keep cargo's own
  directory on `PATH` so it always finds its sibling `rustc`. Nothing
  machine-specific is committed. (Without `make`:
  `export PATH="$HOME/.cargo/bin:$PATH"`.)
- Crates are downloaded from crates.io into `~/.cargo/registry`, outside the
  repo.

Only for multi-OS `make dist` from macOS (auto-installed by the script,
unless `SKIP_SETUP=1`):

- macOS targets: `rustup target add x86_64-apple-darwin` on Apple Silicon
  (and `aarch64-apple-darwin` on an Intel host); both are download-only and
  the script installs whichever the host lacks.
- x86_64 assembly for the AVIF decoder (`rav1d`): `brew install nasm`
  (needed by every x86_64 target, Intel macOS included).
- Linux (dockerless linker): `brew install zig` + `cargo install cargo-zigbuild`.
- Windows (GNU linker): `brew install mingw-w64`.
- Packaging: `python3` (required: the `.deb` writer and the zip fallback),
  `tar`, and `shasum` or `sha256sum`; `zip` is used when present and falls
  back to `ditto` (macOS) or python3.

Native RPM and Flatpak (built only on Linux hosts; `make deps` attempts the
tools there, best-effort):

- RPM: `rpmbuild` (`rpm-build` on Fedora). `make dist-rpm` builds the
  package from the full source tree. Not cross-built from macOS.
- Flatpak: `flatpak-builder` plus the `org.freedesktop.Platform` /
  `org.freedesktop.Sdk` 24.08 runtimes and the
  `org.freedesktop.Sdk.Extension.rust-stable` extension (`make flatpak`).

## Installing dependencies

`make deps` installs what the host can use, best-effort and idempotent:
Rust (Homebrew's `rustup` + stable toolchain on macOS; on Linux the official
installer only with `ALLOW_RUSTUP=1`, never silently), `nasm`, `python3`, the
macOS cross tools (`zig`, `cargo-zigbuild`, `mingw-w64`, the rustup targets)
and the packaging tools (`desktop-file-utils`, `appstream`, `appstream-glib`,
`rpm`, `lintian`, PyYAML). Homebrew on macOS; `apt-get`/`dnf`/`pacman` on
Linux (via `sudo`).

```sh
make deps                    # everything this host can use
make deps DEPS=packaging     # one group: toolchain | cross | packaging
SKIP_SETUP=1 make deps       # report only, install nothing
```

A tool already on `PATH` is left alone; anything that cannot be installed is a
`SKIP` with the reason, never a failure. The Xcode Command Line Tools (the
macOS C compiler) cannot come from Homebrew, so the target prints
`xcode-select --install` instead. `make dist` already auto-installs the cross
tools it needs (`SKIP_SETUP=1` opts out), so `make deps` matters mostly for
Rust itself and the packaging/validation tools.

## Build, test and package

`make` shortcuts (delegate to cargo and `scripts/`, POSIX sh):

```sh
make              # deps + host release + current-OS dist into dist/
                  #   macOS: universal .app.zip ONLY
                  #   Linux: .tar.gz + .deb/.rpm + Flatpak bundle
                  #   Windows: .exe zip
make debug        # host debug (target/debug/mdowser)
make test         # pipeline: sh syntax + fmt --check + clippy -D warnings + test + doc + debug
make audit        # supply-chain audit (cargo audit + cargo deny, best-effort)
make dist         # deps + host release + EVERY platform into dist/ (+ SHA256SUMS.txt)
make dist-host | make dist-mac | make dist-linux | make dist-windows
make dist-rpm     # RPM from the full source tree (Linux host with rpmbuild)
make dist-flatpak # Flatpak bundle (needs flatpak-builder; see packaging/flatpak/)
make install      # runtime files under PREFIX (DESTDIR honored; RPM %install)
make flatpak      # alias of the Flatpak bundle build (needs flatpak-builder)
make validate     # best-effort packaging metadata checks (SKIPs when tools missing)
make deps         # install build/packaging dependencies (SKIP_SETUP=1 dry run)
make run | make clean | make help | make icons | make doctor
```

`make doctor` prints the toolchain exactly as the Makefile sees it (the
`PATH` fixup, the resolved `cargo`/`rustc`, whether `nasm` is present) and
stays runnable even when `cargo` cannot be found — every other target fails
at parse time with the hint that points at it.

`make` (the default target) and `make dist` both run `make deps` first, so
the widest possible toolchain is installed before building; `SKIP_SETUP=1
make ...` keeps it to what is already present. `make` then builds the current
platform's artifact set only (macOS: the universal `.app.zip`; Linux:
`.tar.gz` + `.deb`/`.rpm` + the Flatpak bundle; Windows: the `.exe` zip),
while `make dist` builds every platform it can on the host.

`make icons` only forces a regeneration: the build itself generates
`assets/icons/` from `assets/icon.svg` whenever the committed files are
missing or outdated (`build.rs` + `build_support/gen_icons.rs`), so a fresh
checkout needs no manual step.

The build also Brotli-compresses the two largest embedded fonts
(`assets/fonts/NotoColorEmoji-Regular.ttf`, `assets/fonts/NotoSansCJK-Regular.ttc`)
into `OUT_DIR` (`build_support/gen_fonts.rs`), saving ~26.5 MB in the
installed binary (44.8 MB raw -> 18.3 MB at quality 10) with byte-identical
rendering; the runtime decompresses
each face once (CJK synchronously at startup, color emoji on the background
`media::emoji::prewarm` thread `App::new` spawns, so no first emoji ever
parses the face mid-frame). The result
is cached with a hash stamp, so only the first build of a profile pays the
~90 s encoder pass (quality 10). Build scripts run optimized
(`[profile.*.build-override] opt-level = 3` in `Cargo.toml`) precisely for
this: at the default `opt-level = 0` the encoder would take minutes. The
same tables set `strip = "none"` to opt out of cargo's automatic debuginfo
strip for build dependencies (since Rust 1.77, build scripts are built
without debug info and cargo passes `-C strip=debuginfo`): on macOS that
strip step invokes the toolchain's `rust-objcopy`, which is broken on some
installs (missing `libLLVM.dylib` -> `SIGABRT` warning once per build
script). Build scripts never ship, so keeping their debug info costs only
disk in `target/`. No manual step and no extra tooling: the encoder is a
build-dependency (`brotli`, already in the app's graph).

The Intel macOS build links `rav1d`'s NASM-assembled x86_64 objects, which
carry no `LC_BUILD_VERSION`, so `ld` prints ~46 "no platform load command
found, assuming macOS" notes. Benign upstream noise (the binary links and
runs fine): `[lints.rust] linker_messages = "allow"` in `Cargo.toml`
silences them until NASM/`rav1d` emit the load command.

Direct equivalents without make (the scripts alone skip the `make deps`
bootstrap and, for dist, the host `cargo build --locked --release` that
`make`/`make dist` run first; pass `SKIP_SETUP=1` to keep auto-install off):

```sh
./scripts/build-test.sh            # same as make test
./scripts/build-dist.sh --all      # same as make dist (every platform package)
./scripts/build-dist.sh --host-only   # current-OS package (fast, no cross)
SKIP_SETUP=1 ./scripts/build-dist.sh --all  # without auto-installing toolchains
```

`--host-only` generates just the current OS's artifact set with the same docs
every artifact carries: on macOS the universal `.app.zip`; on Linux
`mdowser-<ver>-linux-<arch>.tar.gz`, the matching `.deb` (and `.rpm` when
`rpmbuild` is present) and the Flatpak bundle; on Windows
`mdowser-<ver>-windows-<arch>.zip`. Plain `make` is the host binary in
`target/release/mdowser` plus this.

`--all` generates all of the above plus (the version comes from `Cargo.toml`; shown here as `<ver>`):

- macOS: `MDowser-<ver>-macos-universal.app.zip` — the ONLY macOS artifact
  (arm64+Intel universal binary via `lipo`, ad-hoc signature, app icon in
  `Contents/Resources`; no loose per-arch `.tar.gz`).
- Linux x86_64/arm64: `mdowser-<ver>-linux-<arch>.tar.gz` +
  `mdowser_<ver>_<amd64|arm64>.deb` (minimal deb with python3 stdlib, no
  `cargo-deb`; installs the binary, the hicolor icon ladder plus a scalable
  SVG, AppStream metadata and a `.desktop` entry with
  `Icon=`/`StartupWMClass=`, and the man page). When `rpmbuild` is present
  (Fedora/RHEL/openSUSE host) the same run also builds the RPM from source
  (copied verbatim from `rpmbuild`'s `RPMS/*/` output into `dist/`), and when
  `flatpak-builder` is present it adds the `mdowser-<ver>.flatpak` bundle.
- Windows x86_64: `mdowser-<ver>-windows-x86_64.zip` (with `.exe` at root;
  the icon, the `VERSIONINFO` block and the DPI/version manifest are embedded
  in the executable's resources, so the shell, the taskbar, Alt-Tab and
  Explorer's Details tab all read them).

Every `.tar.gz`/`.zip` archive carries `README.md`, `THIRD_PARTY_NOTICES.md`
and `LICENSE`; the macOS `.app` bundles the notices and license under
`Contents/Resources/` and the `.deb` under
`/usr/share/doc/mdowser/` (the notices as `copyright`); a `SHA256SUMS.txt` is
written next to the artifacts.

### Native Windows

On native Windows no `make` or `/bin/sh` is needed: `windows.ps1` (repo root)
is the winget-only, idempotent equivalent of `make` (deps + host release +
current-OS dist into `dist/`). It self-elevates via UAC unless `-NoElevate`
is passed, installs what is missing (Rustup 1.98, MSVC Build Tools + the C++
workload, NASM, Git with Git Bash — the `sh`/`tar`/`sha256sum` runtime the
packaging needs — Python 3.12 with a `python3` shim, LLVM for `llvm-strip`),
runs `cargo build --locked --release`, then calls
`scripts/build-dist.sh --host-only` through Git Bash for the `.exe` zip
(`mdowser-<ver>-windows-x86_64.zip` + `SHA256SUMS.txt`). `build-dist.sh`
prefers `x86_64-w64-mingw32-strip` and falls back to `llvm-strip`; the
`windows.ps1` path installs LLVM, so a native MSVC build strips with
`llvm-strip`.

```powershell
powershell -ExecutionPolicy Bypass -File windows.ps1
powershell -ExecutionPolicy Bypass -File windows.ps1 -Test
powershell -ExecutionPolicy Bypass -File windows.ps1 -SkipSetup   # use what is present, like SKIP_SETUP=1
```

Flags: `-Test` (also run `scripts/build-test.sh`), `-Validate` (also run
`scripts/validate.sh`), `-SkipBuild` (deps + dist only), `-SkipDist` (deps +
build only), `-SkipSetup` (install nothing, fail when a tool is missing),
`-NoElevate` (no UAC self-elevation; installs may fail). With `-SkipBuild`
and `-SkipDist` (or `-SkipDist` without `-Test`/`-Validate`) it is
setup-only.

### Desktop integration and package formats

Linux packaging is generated from ONE shared set of files, so the `.deb`, the
`.rpm`, Flatpak and a manual install cannot drift (`packaging/README.md`):

- `packaging/linux/mdowser.desktop` — the desktop entry (categories,
  keywords, `StartupWMClass`, Markdown MIME types).
- `packaging/linux/dev.mdowser.app.metainfo.xml` — AppStream metadata: the
  page GNOME Software / KDE Discover render (name, summary, description,
  links, icon, release). `scripts/packaging.sh` pins its `<release>` to the
  `Cargo.toml` version at package time.
- `packaging/linux/mdowser.1` — man page.
- the hicolor icon ladder (`assets/icons/icon-*.png`) plus the scalable
  `assets/icon.svg`.

`scripts/install.sh` (`make install`) lays those down under `DESTDIR`/`PREFIX`
and is the RPM spec's `%install`:
`make install DESTDIR=/tmp/root PREFIX=/usr` stages a package tree.

RPM: `packaging/rpm/mdowser.spec` builds the crate from the full source tree.
`make dist-rpm` (or `./scripts/build-dist.sh --rpm`) creates the source
tarball and runs `rpmbuild` with the version from `Cargo.toml`. Offline
(no-network) builds need the crates vendored; a build host with network
access (or a local `rpmbuild`) does.

Flatpak: `packaging/flatpak/dev.mdowser.app.yaml` builds the same source into
`dev.mdowser.app`; `make flatpak` / `make dist-flatpak` (and `make dist` when
`flatpak-builder` is present) vendors the crates
(`flatpak-cargo-generator.py`), runs `flatpak-builder` into a local repo and
exports the DISTRIBUTABLE bundle `dist/mdowser-<ver>.flatpak` (install it
with `flatpak install mdowser-<ver>.flatpak`). The freedesktop runtimes are
pulled best-effort first.

Windows: the executable carries its identity in PE resources generated by
`build.rs` — the icon, a `VERSIONINFO` block (description, version, copyright)
and the application manifest (per-monitor DPI awareness, supported Windows
versions, UTF-8 code page, long paths, segment heap) — and `main` sets
`windows_subsystem = "windows"` so no console window opens behind the GUI.
The distribution stays a portable zip (no installer): the shell reads the
icon and version from the `.exe`.

### Validating the packaging metadata

`make validate` checks what the host can check and never fails on a missing
tool (a SKIP names the install command): XML well-formedness of the AppStream
metadata and of the version-rendered copy the packages install, `appstreamcli`
or `appstream-util` for the metadata, `desktop-file-validate` for the desktop
entry, `rpmspec -P` for the RPM spec and `rpmlint` when present, and a YAML
parse of the Flatpak manifest.

On macOS everything except a real `.deb`/`.rpm` build and `flatpak-builder` is
installable with Homebrew:

```sh
brew install desktop-file-utils appstream appstream-glib rpm
```

`rpm` on macOS is useful only for `rpmspec -P`/`rpmlint` (a real RPM needs a
Linux host; the binary would be a Mach-O). `lintian` is Debian-only and
`flatpak-builder` is Linux-only. To validate those for real, use a container:

```sh
# .desktop + AppStream + lintian on a built .deb (from dist/)
docker run --rm -v "$PWD:/w" -w /w debian:bookworm bash -lc '
  apt-get update && apt-get install -y --no-install-recommends \
    desktop-file-utils appstream lintian &&
  desktop-file-validate packaging/linux/mdowser.desktop &&
  appstreamcli validate packaging/linux/dev.mdowser.app.metainfo.xml &&
  lintian dist/mdowser_*_amd64.deb'

# RPM spec syntax + rpmlint (real build: `./scripts/build-dist.sh --rpm`)
docker run --rm -v "$PWD:/w" -w /w fedora:latest bash -lc '
  dnf -y install rpm-build rpmlint &&
  VER=$(sed -n '/^\[package\]/,/^\[/s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' Cargo.toml | head -1) &&
  rpmspec -P packaging/rpm/mdowser.spec --define "ver $VER" >/dev/null &&
  rpmlint packaging/rpm/mdowser.spec'
```

Cross from macOS self-installs any missing tooling (rustup target, `zig` +
`cargo-zigbuild`, `mingw-w64`, `nasm`); any failing group is skipped with a
reason and the script moves on. For fully reliable GUI artifacts, build
natively on each target OS: this script is the best-effort single-machine
equivalent.

Publishing: attach the files in `dist/` to a GitHub Release (web UI, or
`gh release create v<ver> dist/*`). Release assets are served directly
(`…/releases/latest/download/<file>`), so `dist/` itself stays untracked
and out of the repository history.

## Runtime

- macOS: macOS 13+ (the `.app` carries an ad-hoc signature, not a Developer
  ID: on first launch use right-click > Open if macOS warns).
- Linux: system graphics libraries (X11/Wayland, Mesa) and `ca-certificates`
  for TLS verification; the `.deb` and the `.rpm` depend only on
  `ca-certificates` and install with `sudo dpkg -i mdowser_*_amd64.deb` or
  `sudo rpm -i mdowser-*.rpm`.
- Windows: the GNU `.exe` needs nothing extra in practice; if some old
  Windows asks for MinGW DLLs, build with the MSVC toolchain instead.

## Development commands

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo test --locked --workspace -- --ignored network_images   # optional: real fetch/decode smoke test
cargo run --locked
cargo build --locked --release
```

Standards and contribution workflow: [`CONTRIBUTING.md`](CONTRIBUTING.md).
Module map and internal contracts: [`ARCHITECTURE.md`](ARCHITECTURE.md).
