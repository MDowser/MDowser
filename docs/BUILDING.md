# Building MDowser

See also: [`CONTRIBUTING.md`](CONTRIBUTING.md) for standards and
[`SECURITY.md`](SECURITY.md#build-artifacts) for the release-artifact privacy
contract.

## Requirements

- Stable Rust toolchain ≥ 1.99 via `rustup` (provides `cargo` and `rustc`).
  `rust-toolchain.toml` pins the release line the project is verified against
  (`Cargo.toml` `rust-version`); it also satisfies the pure-Rust AVIF decoder
  (`avif-decode`), whose own floor is 1.98.
- System C compiler: Xcode Command Line Tools on macOS
  (`xcode-select --install`), `build-essential` on Debian/Ubuntu, MSVC or
  MinGW on Windows.
- NASM on x86/x86_64 hosts (Linux, Intel macOS, Windows), and on an Apple
  Silicon host when building the universal macOS `.app` (its x86_64 slice):
  the AVIF decoder (`rav1d`) and the rustls crypto provider (`aws-lc-sys`)
  compile assembly. Install with `brew install nasm`,
  `sudo apt-get install nasm` or, on Windows (used by `windows.ps1`),
  `winget install NASM.NASM`. ARM64-only builds (Apple Silicon, Linux arm64,
  Windows on ARM) need no NASM.
- No `PATH` setup: the `Makefile` and `scripts/lib.sh` resolve the toolchain
  from `$HOME`-relative locations (`~/.cargo/env`, `~/.cargo/bin`,
  `rustup which cargo`, `brew --prefix`, the toolchain glob) and keep cargo's
  own directory on `PATH` for its sibling `rustc`. Nothing machine-specific is
  committed. Without `make`, run `export PATH="$HOME/.cargo/bin:$PATH"`.
- Crates are downloaded from crates.io into `~/.cargo/registry`, outside the
  repo.

Only for a multi-OS `make dist` from macOS (auto-installed by the script
unless `SKIP_SETUP=1`):

- macOS targets: `rustup target add x86_64-apple-darwin` on Apple Silicon (and
  `aarch64-apple-darwin` on an Intel host). Both are download-only; the script
  installs whichever the host lacks.
- x86_64 assembly for `rav1d`: `brew install nasm` (needed by every x86_64
  target, Intel macOS included).
- Linux (dockerless linker): `brew install zig` +
  `cargo install cargo-zigbuild`.
- Windows (GNU linker): `brew install mingw-w64`.
- Packaging: `python3` for the `.deb` writer and the zip fallback (checked
  when used, so RPM/Flatpak/macOS/Windows-only runs do not need it), `tar`,
  and `shasum` or `sha256sum`. `zip` is used when present and falls back to
  `ditto` (macOS) or python3.

Native RPM and Flatpak builds run only on Linux hosts; `make deps` attempts
their tools there, best-effort:

- RPM: `rpmbuild` (`rpm-build` on Fedora). `make dist-rpm` builds the package
  from the full source tree. It is not cross-built from macOS.
- Flatpak: `flatpak-builder` plus the `org.freedesktop.Platform` /
  `org.freedesktop.Sdk` 26.08 runtimes and the
  `org.freedesktop.Sdk.Extension.rust-stable` extension (`make flatpak`). The
  branch is the manifest's `runtime-version`, the single source of truth
  `scripts/build-flatpak.sh` derives the refs from. The 26.08 extension still
  ships Rust 1.98.0, so the manifest passes `--ignore-rust-version` to cargo
  (the tree compiles on 1.98.0 and every dependency supports it); remove the
  flag when the extension ships Rust 1.99.

## Installing dependencies

`make deps` installs what the host can use, best-effort and idempotent: Rust
(Homebrew's `rustup` + stable toolchain on macOS; on Linux the official
installer only with `ALLOW_RUSTUP=1`, never silently), `nasm`, `python3`, the
macOS cross tools (`zig`, `cargo-zigbuild`, `mingw-w64`, the rustup targets)
and the packaging tools (`desktop-file-utils`, `appstream`, `appstream-glib`,
`mandoc`, `rpm`, `lintian`, PyYAML). It uses Homebrew on macOS and
`apt-get`/`dnf`/`pacman` on Linux (via `sudo`).

```sh
make deps                    # everything this host can use
make deps DEPS=packaging     # one group: toolchain | cross | packaging
SKIP_SETUP=1 make deps       # report only, install nothing
```

A tool already on `PATH` is left alone; anything uninstallable is a `SKIP`
with the reason, never a failure. The Xcode Command Line Tools (the macOS C
compiler) cannot come from Homebrew, so the target prints
`xcode-select --install`. `make dist` installs the cross tools through
`make deps` on macOS (`SKIP_SETUP=1` opts out); on Linux install `zig` and
`mingw-w64` with the package manager and `cargo-zigbuild` with
`cargo install`. `make deps` matters mostly for Rust itself and the
packaging/validation tools.

## Build, test and package

`make` shortcuts (delegating to cargo and `scripts/`, POSIX sh):

```sh
make              # deps + current-OS dist into dist/ (builds what it packages)
                  #   macOS: universal .app.zip ONLY
                  #   Linux: .tar.gz + .deb/.rpm + Flatpak bundle
                  #   Windows: .exe zip
make build        # host release binary only (target/release/mdowser)
make debug        # host debug (target/debug/mdowser)
make test         # pipeline: sh syntax + version consistency + fmt --check + clippy -D warnings + test + doc + debug
make audit        # supply-chain audit (cargo audit + cargo deny, best-effort)
make dist         # deps + EVERY platform into dist/ (+ SHA256SUMS.txt)
make dist-host | make dist-mac | make dist-linux | make dist-windows
make dist-rpm     # RPM from the full source tree (Linux host with rpmbuild)
make dist-flatpak # Flatpak bundle (needs flatpak-builder; see packaging/flatpak/)
make install      # runtime files under PREFIX (DESTDIR honored; RPM %install)
make flatpak      # alias of the Flatpak bundle build (needs flatpak-builder)
make validate     # best-effort packaging metadata checks (SKIPs when tools missing)
make check-version # Cargo.toml vs metainfo/RPM changelog/CHANGELOG.md/SECURITY.md
make check-dist   # privacy gate over dist/ (fails when dist/ is absent; after packaging)
make deps         # install build/packaging dependencies (SKIP_SETUP=1 dry run)
make fmt | make clippy | make run | make clean | make help | make icons | make doctor
make sync-sites   # sync curated omnibox list from MDowser-Sites (manual, network)
make check-sites  # validate one builtin-sites list file (SITES_FILE, manual, network)
```

`make doctor` prints the toolchain as the Makefile sees it (the `PATH` fixup,
the resolved `cargo`/`rustc`, whether `nasm` is present) and stays runnable
even when `cargo` cannot be found, as do the tool-free `deps` (the install
path for a missing toolchain), `install`, `help`, `validate`, `check-version`,
`check-dist`, `sync-sites` and `check-sites` targets. The cargo-driven targets
fail at parse time with the hint that points at `make doctor`. When no cargo
is resolved the Makefile does not prepend an empty PATH entry, so the current
directory is never searched for `install`/`tar`/similar tools first.

`make` (the default target) and `make dist` both run `make deps` first once
`cargo` is resolvable; `SKIP_SETUP=1 make ...` keeps it to what is already
present. A machine without Rust needs one explicit `make deps` first: the
Makefile resolves `cargo` at startup and cannot pick up a binary installed
later in the same run. Direct script invocations
(`./scripts/build-dist.sh`, `./scripts/build-test.sh`) skip that bootstrap.

`make` builds the current platform's set only (macOS: the universal
`.app.zip`; Linux: `.tar.gz` + `.deb`/`.rpm` + the Flatpak bundle; Windows:
the `.exe` zip), while `make dist` builds every platform the host can.

`make icons` forces a regeneration: the build generates `assets/icons/`
from `assets/icon.svg` whenever the committed files are missing or outdated
(`build.rs` + `build_support/gen_icons.rs`), so a fresh checkout needs no
  manual step.

The build Brotli-compresses the two largest embedded fonts
(`assets/fonts/NotoColorEmoji-Regular.ttf`,
`assets/fonts/NotoSansCJK-Regular.ttc`) into `OUT_DIR`
(`build_support/gen_fonts.rs`), saving ~26.5 MB (44.8 MB raw → 18.3 MB at
quality 10) with byte-identical rendering. The runtime decompresses each face
once (CJK synchronously at startup, color emoji on the background
`media::emoji::prewarm` thread `App::new` spawns), so no first emoji parses
  the face mid-frame; a machine that never shows the window still pays the
  one-time decompression on that thread. A hash stamp caches the
result: only the first build per profile and target triple pays the ~90 s
encoder pass; `OUT_DIR` is keyed by both. Build scripts run optimized
(`[profile.*.build-override] opt-level = 3` in `Cargo.toml`, versus
  `opt-level = 0` by default, at which the encoder would take minutes). Those
  tables set
`strip = "none"`: since Rust 1.77, build scripts are built without debug info
and cargo passes `-C strip=debuginfo`, and on macOS that strip invokes the
toolchain's `rust-objcopy`, which is broken on some installs (missing
`libLLVM.dylib` → one `SIGABRT` warning per build script). Build scripts never
ship, so their debug info costs only disk in `target/`. The encoder is a
build-dependency (`brotli`, already in the app's graph).

The Intel macOS build links `rav1d`'s NASM-assembled x86_64 objects, which
carry no `LC_BUILD_VERSION`, so `ld` prints ~46 "no platform load command
found, assuming macOS" notes. `[lints.rust] linker_messages = "allow"` in
`Cargo.toml` silences it until NASM/`rav1d` emit the load command.

Direct equivalents without make (the scripts alone skip the `make deps`
bootstrap; pass `SKIP_SETUP=1` to keep auto-install off):

```sh
./scripts/build-test.sh            # same as make test
./scripts/build-dist.sh --all      # same as make dist (every platform the host can build)
./scripts/build-dist.sh --host-only   # current-OS package (fast, no cross)
./scripts/build-dist.sh --mac | --linux | --windows | --rpm | --flatpak  # one group
./scripts/check-dist.sh --require  # privacy gate over dist/ (= make check-dist)
./scripts/validate.sh              # packaging metadata checks (= make validate)
./scripts/install.sh               # runtime files under PREFIX (= make install)
./scripts/deps.sh                  # build/packaging deps (= make deps)
SKIP_SETUP=1 ./scripts/build-dist.sh --all  # without auto-installing toolchains
```

`--host-only` generates just the current OS's artifact set: on macOS the
universal `.app.zip`; on Linux `mdowser-<ver>-linux-<arch>.tar.gz`, the
matching `.deb` (and `.rpm` when `rpmbuild` is present) and the Flatpak
bundle; on Windows `mdowser-<ver>-windows-<arch>.zip`. Plain `make` produces
this current-OS artifact set; `make build` produces just the host binary in
cargo's release output (`target/release/mdowser` by default).

On Linux, `--all` reuses the native host build for the host architecture's
`.tar.gz`/`.deb` (the same binary `make` ships) and Zig-cross-builds the other
architecture, so the host binary for those two artifacts is compiled once.
The host artifact follows the host toolchain's libc, as `--host-only` does.

`dist/` semantics: the COMPLETE modes (`--all`, the default, and
`--host-only`) rebuild `dist/` from scratch; a single-group flag (`--mac`,
`--linux`, `--windows`, `--rpm`, `--flatpak`) ADDS its artifacts to what is
already there. Make recipe execution is serialized (`.NOTPARALLEL`): the
goals share `target/` and `dist/`, so `make -j` must not race them (cargo
still compiles in parallel internally). `CARGO_TARGET_DIR` is honored by the
packaging and install paths (relative values resolve against the repository
root). For host packaging, `CARGO_BUILD_TARGET` must name the host triple;
use the platform flags for cross targets. The Flatpak wrapper regenerates
`cargo-sources.json` when `Cargo.lock` is newer and removes/verifies the
bundle path, so a stale dependency set or old bundle is never reused.

`--all` generates all of the above plus (the version comes from `Cargo.toml`;
shown here as `<ver>`):

- macOS: `MDowser-<ver>-macos-universal.app.zip` — the ONLY macOS artifact
  (arm64 + Intel universal binary via `lipo`, ad-hoc signature, app icon in
  `Contents/Resources`; no loose per-arch `.tar.gz`). The zip is versioned;
  the bundle inside is plain `MDowser.app`, so an upgrade replaces the
  previous copy in `/Applications`.
- Linux x86_64/arm64: `mdowser-<ver>-linux-<arch>.tar.gz` +
  `mdowser_<ver>_<amd64|arm64>.deb` (minimal deb from the python3 stdlib, no
  `cargo-deb`; installs the binary, the hicolor icon ladder plus a scalable
  SVG, AppStream metadata, a `.desktop` entry with `Icon=`/`StartupWMClass=`
  and the man page; `Depends` derives `libc6 (>= <x.y>)` from the binary's
  glibc symbols when an ELF reader is available). With `rpmbuild` present
  (Fedora/RHEL/openSUSE host) the same run builds the RPM from source (copied
  verbatim from `RPMS/*/` into `dist/`), and `flatpak-builder` present adds
  the `mdowser-<ver>.flatpak` bundle.
- Windows x86_64: `mdowser-<ver>-windows-x86_64.zip` (the `.exe` at the root;
  icon, `VERSIONINFO` block and the DPI/version manifest are embedded in its
  resources).
- Windows arm64: `mdowser-<ver>-windows-aarch64.zip`, the same `.exe` zip
  built natively (MSVC) on a Windows on ARM host. It is not cross-built, so
  only such a host — or CI, on a `windows-11-arm` runner — produces it.

Every Linux `.tar.gz` and Windows `.zip` carries `README.md`,
`THIRD_PARTY_NOTICES.md` and `LICENSE`; the macOS `.app` bundles the notices
and license under `Contents/Resources/`, the `.deb` under
`/usr/share/doc/mdowser/` (notices as `copyright`). A `SHA256SUMS.txt` sits
next to the artifacts. Before the checksums, `scripts/check-dist.sh --require`
enforces the privacy rules below.

A pre-release version such as `0.7.0-alpha.1` keeps its full spelling in the
artifact names and AppStream metadata, while each package format gets the form
it accepts: the `.app` plist uses the numeric base `0.7.0` (Apple's version
keys are numeric), Debian uses `0.7.0~alpha.1` and RPM uses the same `~` form
(RPM rejects `-` in `Version:` and `~` sorts before the final release). The
version gate compares the normalized RPM form with the spec changelog.

### Native Windows

On native Windows no `make` or `/bin/sh` is needed: `windows.ps1` (repo root)
is the winget-only, idempotent equivalent of `make` (deps + current-OS dist
into `dist/`). It self-elevates via UAC unless `-NoElevate` is
passed, and installs what is missing: Rustup at the channel in
`rust-toolchain.toml`, MSVC Build Tools + the C++ workload, NASM, Git with Git
Bash (the `sh`/`tar`/`sha256sum` runtime packaging needs), Python 3.12 with a
`python3` shim, and LLVM for `llvm-strip`. It then runs
`cargo build --locked --release` and calls `scripts/build-dist.sh --host-only`
through Git Bash for the `.exe` zip (`mdowser-<ver>-windows-<arch>.zip` —
`x86_64`, or `aarch64` on Windows on ARM) plus `SHA256SUMS.txt`.
`build-dist.sh` prefers `x86_64-w64-mingw32-strip` and falls back to
`llvm-strip` (arm64 has no MinGW strip).

```powershell
powershell -ExecutionPolicy Bypass -File windows.ps1
powershell -ExecutionPolicy Bypass -File windows.ps1 -Test
powershell -ExecutionPolicy Bypass -File windows.ps1 -SkipSetup   # use what is present, like SKIP_SETUP=1
```

Flags: `-Test` (also run `scripts/build-test.sh`), `-Validate` (also
`scripts/validate.sh`), `-SkipBuild` (setup/tests only: no release build and
no dist, because packaging always compiles the release binary), `-SkipDist`
(release build, no packaging), `-SkipSetup` (install nothing; also skips the
winget requirement, fail only when a required tool is missing), `-NoElevate`
(no UAC self-elevation; installs may fail). `-SkipBuild` with `-SkipDist`
(and neither `-Test` nor `-Validate`) is setup-only.

### Desktop integration and package formats

Linux packaging is generated from ONE shared set of files
([`../packaging/README.md`](../packaging/README.md)):

- `packaging/linux/mdowser.desktop` — the desktop entry (categories, keywords,
  `StartupWMClass`, Markdown MIME types).
- `packaging/linux/dev.mdowser.app.metainfo.xml` — AppStream metadata: the
  page GNOME Software and KDE Discover render (name, summary, description,
  links, icon, release). `scripts/packaging.sh` pins its newest `<release>` to
  the `Cargo.toml` version and today's date (or the date of
  `SOURCE_DATE_EPOCH` for a reproducible build) at package time.
- `packaging/linux/mdowser.1` — man page.
- the hicolor icon ladder (`assets/icons/icon-*.png`) plus the scalable
  `assets/icon.svg`.

`scripts/install.sh` (`make install`) lays those down under `DESTDIR`/`PREFIX`
and is the RPM spec's `%install`. It expects cargo's release output
(`target/release/mdowser` by default; `CARGO_TARGET_DIR` and
`CARGO_BUILD_TARGET` are honored, and `BIN=...` overrides the path):
`make install DESTDIR=/tmp/root PREFIX=/usr` stages a package tree.

RPM: `packaging/rpm/mdowser.spec` builds the crate from the full source tree.
`make dist-rpm` (or `./scripts/build-dist.sh --rpm`) creates the source tarball
and runs `rpmbuild` with the version from `Cargo.toml`. Offline builds need the
crates vendored; a host with network access (or a local `rpmbuild`) can build
directly.

Flatpak: `packaging/flatpak/dev.mdowser.app.yaml` builds the same source into
`dev.mdowser.app`. `make flatpak` / `make dist-flatpak` (and `make dist` when
`flatpak-builder` is present) vendors the crates
(`flatpak-cargo-generator.py`), runs `flatpak-builder` into a local repo, and
exports `dist/mdowser-<ver>.flatpak` (install with
`flatpak install mdowser-<ver>.flatpak`). The freedesktop runtimes are pulled
best-effort first.

Windows: the executable carries its identity in PE resources generated by
`build.rs` — the icon, a `VERSIONINFO` block (description, version, copyright)
and the application manifest (per-monitor DPI awareness, supported Windows
versions, UTF-8 code page, long paths, segment heap) — and `main` sets
`windows_subsystem = "windows"`. The distribution is a portable zip (no
installer). That windowed subsystem has no console, so `mdowser --help` and
`mdowser --version` cannot print there (the flags stay a no-op, never a
window); the `.exe` file properties (Details tab) carry the same version.

### Build-artifact privacy

Release artifacts must not identify the builder. Rust bakes
the source path of every panic location and `file!()` into a binary,
dependency sources under the Cargo registry included, so an unmodified release
binary ships the builder's home path (`/Users/<user>/.cargo/registry/src/...`
on macOS, `/home/<user>/...` on Linux); `strip` cannot remove them: they are
runtime data, not symbols. `scripts/lib.sh` `export_remap_rustflags` therefore
sets `CARGO_ENCODED_RUSTFLAGS` with `--remap-path-prefix=$HOME=/build` (plus
`USERPROFILE`, in both slash styles, on Windows) and the Cargo home to
`/cargo` before every DISTRIBUTABLE build: `make build`, `make dist` /
`scripts/build-dist.sh`, `windows.ps1`, the RPM spec's `%build` and the
Flatpak manifest (which remaps the sandbox paths
`/run/build/mdowser[/cargo]` the same way). Debug builds keep real paths for
debuggers.

The archives are cleaned: `zip -X` drops the Info-ZIP Unix uid/gid extra
field, and the staging trees are xattr-swept (`sweep_xattrs`) plus tarred with
`--no-xattrs` where supported, so a macOS build does not embed its
`com.apple.provenance` blob in the Linux `.tar.gz`/`.deb`. The macOS `.app` is
swept before packing, and `ditto` runs with `--norsrc --noqtn --noextattr`, so
no AppleDouble `._*` member enters the zip. The RPM spec pins `%_buildhost`
and `%defattr(-, root, root, -)`.

`make check-dist` is the gate: it scans every file in `dist/` for the real
`$HOME`/`CARGO_HOME`/hostname and for archive owner/xattr metadata, and exits
non-zero on any hit (or when `dist/` is missing, so a SKIP can never pass for
a release gate). Generic hostnames that collide with the `/build` and `/cargo`
remap targets are not used as needles. `.tar.gz`, `.deb` and `.zip` are
unpacked and their payload files inspected; AppleDouble `._*`/`__MACOSX/`
members are a breach.
The `.rpm`'s xz cpio payload and the ostree `.flatpak` bundle are byte-scanned
only (best-effort). `scripts/build-dist.sh` runs the gate with `--require`
before writing `SHA256SUMS.txt`, so `make dist` fails instead of publishing,
and no python3 fails instead of skipping. The app's example paths
such as `/Users/you/notes.md` are not matched, and the two failure kinds are
reported separately. The `publish` job re-runs the gate over the merged
set from Linux (needle-based on ITS `$HOME`/hostname): macOS-only
`/Users/runner` or Windows-only `C:\Users\…` leaks are caught by each job's
own gate, not the merged one.

### Validating the packaging metadata

`make validate` checks what the host can check and never fails on a missing
tool (a SKIP names the install command): version consistency between
`Cargo.toml` and the committed AppStream metainfo, the RPM changelog, the
released `CHANGELOG.md` section and the supported line in `SECURITY.md`
(`make check-version` runs the same gate on its own); XML well-formedness of
the AppStream metadata and of the version-rendered copy the packages install,
`appstreamcli` or `appstream-util`, `desktop-file-validate`, a man-page lint
(`mandoc -T lint -W error`) when available, `rpmspec -P` and `rpmlint` when
present, a YAML parse of the Flatpak manifest, and the `dist/` privacy gate
when a build is present.

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
  RPM_VER=$(printf '%s' "$VER" | sed -e 's/+.*$//' -e 's/-/~/g') &&
  rpmspec -P packaging/rpm/mdowser.spec --define "ver $RPM_VER" >/dev/null &&
  rpmlint packaging/rpm/mdowser.spec'
```

Cross-building from macOS self-installs any missing tooling (rustup target,
`zig` + `cargo-zigbuild`, `mingw-w64`, `nasm`); any failing group (including
the host build in `--all`) is skipped with a reason and the script moves on.
For reliable GUI artifacts, build natively on each target OS.

### Releases in CI (GitHub Actions)

`.github/workflows/release.yml` builds every target and publishes ONE GitHub
Release per tag (`v<version>`, which must match `Cargo.toml`). Each job runs
on a fresh GitHub-hosted VM with NO Actions cache: releases compile from
scratch, and only packaged files travel between jobs (Actions artifacts,
1-day retention). The scripts build, so privacy matches `make dist`: the
publishing job re-runs `scripts/check-dist.sh` over the merged set and writes
the single `SHA256SUMS.txt`.

| Job | Runner | Builds |
| --- | --- | --- |
| `verify` | ubuntu-24.04 | `scripts/build-test.sh` (sh syntax, version, fmt, clippy, tests, doc, debug) + pinned `cargo audit`/`cargo deny check`; every build job needs it |
| `linux-x64` | ubuntu-24.04 | Linux x86_64 `.tar.gz` + `.deb`, Windows x86_64 `.zip` (mingw cross) |
| `linux-arm64` | ubuntu-24.04-arm | Linux arm64 `.tar.gz` + `.deb` |
| `windows-arm64` | windows-11-arm | Windows arm64 `.zip` (native MSVC) |
| `macos` | macos-15 | universal `.app.zip` (arm64 + Intel, `lipo`) |
| `rpm` | Fedora container | `.rpm` from source (`rpmbuild`) |
| `flatpak` | ubuntu-24.04 | `.flatpak` bundle |
| `publish` | ubuntu-24.04 | merged privacy gate, `SHA256SUMS.txt`, Release |

A tag cannot publish a tree that fails `make test`: the `verify` job runs the
full pipeline and every build job declares `needs: verify`. It also installs
`cargo-audit` 0.22.2 and `cargo-deny` 0.20.2 (pinned, no Actions cache) and
runs both checks, so a new advisory, yanked crate, banned dependency or
unapproved license blocks the release even when the local, best-effort
`make audit` was skipped. The publish job
also checks that the merged set contains every advertised target (macOS,
Linux x86_64/arm64 archives and debs, both Windows zips, RPM and Flatpak)
before writing checksums; the x86_64 Windows build asserts its `.rsrc` section
(icon/version/manifest) like the native arm64 job does.

Artifact names are identical to `make dist`, plus the Windows arm64 `.zip`
(native MSVC on a Windows on ARM runner; not cross-built, so only CI or a
Windows on ARM host running `--host-only` produces it). The two Ubuntu Linux
jobs run `--host-only` with `NO_LOCAL_RPM=1`: Ubuntu's runner provides
`rpmbuild`, and without the opt-out each job would also attach its own `.rpm`
(a duplicate x86_64 package and an undocumented aarch64 one); the published
RPM is always Fedora's. The Release is titled with the tag and its
body is fixed text substituted with the release version (CHANGELOG link,
per-platform first-run and install notes, assets pointer and the
use-at-your-own-risk disclaimer); tags carrying
`-alpha`/`-beta`/`-rc` are published as pre-releases. A manual run
(`workflow_dispatch`) with `dry_run: true` compiles everything without
publishing; a `dry_run: false` run must be dispatched from the matching
`v<version>` tag, and the `verify` job rejects a branch, a tag without the
`v` prefix, or a mismatched tag before any build starts. Standard runners are
free for public repositories; every job is capped with `timeout-minutes` and
no cache is kept between runs.

Push a `v<version>` tag matching `Cargo.toml`; the workflow attaches the
files to one GitHub Release. A local `make dist` set can be attached by hand
(`gh release create v<ver> dist/*`). Release assets are served directly
(`…/releases/latest/download/<file>`), so `dist/` stays untracked.

## Runtime

- **macOS 13+:** the `.app` carries an ad-hoc signature, not a Developer ID,
  and is not notarized. Double-click once so Gatekeeper registers the block,
  then allow it in System Settings > Privacy & Security > `Open Anyway`
  (right-click > Open no longer bypasses this on current macOS).
- **Linux:** system graphics libraries (X11/Wayland, Mesa) and
  `ca-certificates` for TLS verification. The `.deb` depends on
  `ca-certificates` plus the `libc6` floor derived from the binary; the `.rpm`
  depends on `ca-certificates`. Install with
  `sudo dpkg -i mdowser_*_amd64.deb` or `sudo rpm -i mdowser-*.rpm`.
- **Windows x86_64:** the GNU `.exe` needs nothing extra. It is unsigned, so
  on the SmartScreen warning click `More info` > `Run anyway`. If some old
  Windows asks for MinGW DLLs, build with the MSVC toolchain instead.
  **Windows on ARM64:** use the MSVC `-windows-aarch64.zip` (Windows 11 on
  ARM); the x86_64 `.exe` also runs there under emulation.

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
