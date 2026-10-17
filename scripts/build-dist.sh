#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# build-dist: multi-OS release build + packaging into dist/.
#
# Usage:
#   ./scripts/build-dist.sh --all        # host binary + everything (default with no flag)
#   ./scripts/build-dist.sh --host-only  # current-OS package only (no cross)
#   ./scripts/build-dist.sh --mac        # macOS: universal .app (arm64 + Intel)
#   ./scripts/build-dist.sh --linux      # Linux x86_64/arm64: tar.gz + .deb + .rpm + Flatpak
#   ./scripts/build-dist.sh --windows    # Windows x86_64: .exe in zip (needs mingw-w64)
#   ./scripts/build-dist.sh --rpm        # RPM built from source (needs rpmbuild, Linux host)
#   ./scripts/build-dist.sh --flatpak    # Flatpak bundle (needs flatpak-builder)
#
# dist/ semantics: `--all` and `--host-only` rebuild dist/ from scratch;
# a single-group flag (--mac, --linux, ...) ADDS its artifacts to whatever
# is already there, so focused reruns never destroy the other platforms'
# output. `make dist` stays the wide, from-scratch path.
#
# Variables:
#   SKIP_SETUP=1  # skip installing toolchains/linkers (needy groups fail)
#
# Artifacts in dist/ (ver = Cargo.toml version):
#   Host (current OS, no cross): the SAME set as that OS's group below.
#   macOS:   MDowser-<ver>-macos-universal.app.zip (universal .app ONLY)
#   Linux:   mdowser-<ver>-linux-<arch>.tar.gz + mdowser_<ver>_<debarch>.deb
#            (+ mdowser-<ver>-*.rpm when rpmbuild is available)
#            + mdowser-<ver>.flatpak (when flatpak-builder is available)
#   Windows: mdowser-<ver>-windows-<arch>.zip (with .exe; x86_64, or
#            aarch64 on Windows on ARM, native MSVC)
#   Always: SHA256SUMS.txt
#
# HONEST NOTE: GUI cross from macOS is best-effort (C + wgpu involved).
# The 100% guaranteed path is running this script natively on each
# target OS. It generates as much as possible locally with one command.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Toolchain + helper PATH without machine-specific absolute dirs (see lib.sh).
# System dirs (/usr/bin:/bin) stay as a fallback for tar/zip/shasum/ar.
. "$SCRIPT_DIR/lib.sh"
. "$SCRIPT_DIR/packaging.sh"
ensure_toolchain_path
export PATH="$PATH:/usr/bin:/bin"

# Privacy: shipped binaries must not embed the builder's absolute home /
# Cargo-registry paths (panic locations carry dependency source paths); remap
# them out of every release build this script runs (see lib.sh).
export_remap_rustflags

APP_LC="mdowser"          # binary name / deb+rpm package
APP_UI="MDowser"         # display name (.app, windows)
VER="$(app_version)"
# Fail loudly on an unparseable version instead of producing
# `mdowser--linux-*.tar.gz` artifacts with an empty tag.
if [ -z "$VER" ]; then
    echo "ERROR: could not parse version from Cargo.toml" >&2
    exit 1
fi
# macOS `cp`/`install` materialize AppleDouble `._name` sidecars for files
# carrying extended attributes (`com.apple.provenance`, ...); tar then packed
# them INTO the .deb payload and the .tar.gz artifacts as junk entries.
# Disabling copyfile(3)-based copies process-wide prevents their creation
# (Linux copies ignore the variable); `sweep_apple_double` below removes any
# sidecar that still appears.
COPYFILE_DISABLE=1
export COPYFILE_DISABLE
DIST="$ROOT/dist"
TARGETS="$ROOT/target"

MODE="${1:-"--all"}"
# NOTE: `${1:-word}` (default when unset/empty), not a substring expansion.
SKIP_SETUP="${SKIP_SETUP:-0}"

BUILT=""    # achieved artifacts
SKIPPED=""  # skipped groups (with reason)

log()  { printf '\n==> %s\n' "$1"; }
warn() { printf '··· SKIP: %s\n' "$1"; SKIPPED="$SKIPPED $1"; }
have() { command -v "$1" >/dev/null 2>&1; }

# bsdtar (macOS) archives extended attributes by default and would embed the
# builder's `com.apple.provenance` blob into `.tar.gz`/`.deb` payloads as PAX
# records; GNU tar does not unless asked. Probe once so the flag is used where
# supported and never breaks a build on a tar that lacks it.
TAR_NO_XATTRS=""
if tar --no-xattrs -cf /dev/null --files-from /dev/null 2>/dev/null; then
    TAR_NO_XATTRS="--no-xattrs"
fi

# Removes macOS AppleDouble sidecars (`._name`) and extended attributes from a
# packaging stage tree: they are xattr/resource-fork metadata, never package
# content. POSIX find; a tree without them (every Linux host) is a no-op.
sweep_apple_double() {
    find "$1" -name '._*' -exec rm -f {} \; 2>/dev/null || true
    sweep_xattrs "$1"
}

# Stage dirs live under dist/ and are removed on the success path; a failure
# under `set -e` (full disk, failed link) used to leave them behind. Best
# effort, so cleanup can never turn a successful run into a failure.
stage_dirs() {
    rm -rf "$DIST"/.deb-stage "$DIST"/.linux-x86_64 "$DIST"/.linux-aarch64 \
        "$DIST"/.rpmbuild "$DIST"/.win-x64 "$DIST"/.host-stage \
        "$DIST"/.universal-bin "$DIST"/.flatpak-repo \
        "$DIST"/.flatpak-build "$DIST"/.flatpak-state 2>/dev/null || true
}
# `EXIT` (not `0`) for readability: supported by dash, bash and zsh in `sh`
# mode; the handler is best-effort so it never masks the real exit status.
trap stage_dirs EXIT

# python3 packs the .deb (stdlib `ar` writer) and is the zip fallback.
# Checked at each use (not up front): `--rpm`, `--flatpak`, `--mac` and
# `--windows` never need it. The FINAL privacy gate (check-dist.sh --require)
# does need it on every mode, including the Fedora RPM container — the CI job
# installs python3 for exactly that reason (a silent SKIP there would report
# an unchecked release as ready).
need_python3() {
    if ! have python3; then
        echo "ERROR: $1 needs python3 (stdlib ar/zip writers); install it first" >&2
        return 1
    fi
}

# `strip` that actually works for every artifact this script produces.
# Background: cargo ships `strip = "none"` (see Cargo.toml: toolchain
# `rust-objcopy` is broken on some installs), so dist strips here.
# The old version used the macOS `strip` for everything except `.exe`:
# that tool only understands Mach-O, so Linux ELFs shipped unstripped.
# Windows was fine via mingw strip.
# New behavior by file kind (detected via `file`, extension fallback):
#   Mach-O (macOS) -> macOS `strip` (always present with Xcode CLT).
#   ELF (Linux)    -> `llvm-strip` (multi-format, from brew llvm), else
#                     `x86_64/aarch64-linux-gnu-strip`, `eu-strip` or a
#                     GNU `strip` (binutils: present wherever `file` is).
#   PE (.exe)      -> `x86_64-w64-mingw32-strip` (mingw-w64), else
#                     `llvm-strip` (native Windows MSVC via LLVM).
# $STRIP still wins when set. Never fatal: warns and keeps going.
find_llvm_strip() {
    # Prints a working llvm-strip path, or nothing.
    # Brew llvm is keg-only (not in PATH): probe its opt prefixes.
    for _lp in \
        "/opt/homebrew/opt/llvm@21/bin/llvm-strip" \
        "/opt/homebrew/opt/llvm/bin/llvm-strip" \
        "/usr/local/opt/llvm@21/bin/llvm-strip" \
        "/usr/local/opt/llvm/bin/llvm-strip"; do
        if [ -x "$_lp" ]; then printf '%s' "$_lp"; return 0; fi
    done
    unset _lp
    if have llvm-strip; then command -v llvm-strip; return 0; fi
    return 1
}

pick_strip_for() {
    # $1 = file. Prints the strip tool to use, or nothing.
    _f="$1"
    if [ -n "${STRIP:-}" ]; then printf '%s' "$STRIP"; unset _f; return 0; fi
    case "$_f" in
        *.exe)
            if have x86_64-w64-mingw32-strip; then
                command -v x86_64-w64-mingw32-strip
            else
                find_llvm_strip 2>/dev/null || true
            fi
            unset _f
            return 0
            ;;
    esac
    _kind=""
    if have file; then _kind="$(file -b "$_f" 2>/dev/null || true)"; fi
    case "$_kind" in
        *ELF*)
            _ls="$(find_llvm_strip 2>/dev/null || true)"
            if [ -n "$_ls" ]; then printf '%s' "$_ls"; unset _f _kind _ls; return 0; fi
            for _es in x86_64-linux-gnu-strip aarch64-linux-gnu-strip eu-strip; do
                if have "$_es"; then command -v "$_es"; unset _f _kind _ls _es; return 0; fi
            done
            unset _es
            # Plain GNU `strip` last: on a stock Linux host it is the ELF
            # tool (binutils ships wherever `file` does), and dist used to
            # warn "ships unstripped" there even though the right tool was
            # one PATH lookup away. Probed, not assumed: Apple's Mach-O
            # `strip` also answers `strip`, and handing it an ELF only
            # printed a confusing "strip failed" warning.
            if have strip && strip --version 2>/dev/null | grep -qi 'gnu'; then
                command -v strip
                unset _f _kind _ls
                return 0
            fi
            ;;
        *Mach-O*|"")
            # Empty `file` output (or no `file` tool): assume macOS host
            # binary and use the macOS strip (historical behavior).
            if have strip; then command -v strip; fi
            ;;
        *PE32*|*MS\ Windows*)
            if have x86_64-w64-mingw32-strip; then
                command -v x86_64-w64-mingw32-strip
            else
                _ls="$(find_llvm_strip 2>/dev/null || true)"
                if [ -n "$_ls" ]; then
                    printf '%s' "$_ls"
                elif have strip; then
                    command -v strip
                fi
                unset _ls
            fi
            ;;
        *)
            if have strip; then command -v strip; fi
            ;;
    esac
    unset _f _kind _ls
}

# Highest `GLIBC_x.y` symbol version an ELF binary requires, or empty when
# no ELF reader is installed. Portable awk max (no GNU `sort -V` on macOS);
# versions are `major.minor` (`2.30`) and the key handles an optional patch.
elf_glibc_floor() {
    _e="$1"
    _symdump=""
    if have readelf; then
        _symdump="$(readelf --version-info "$_e" 2>/dev/null || true)"
    elif have llvm-readelf; then
        _symdump="$(llvm-readelf --version-info "$_e" 2>/dev/null || true)"
    elif have objdump; then
        _symdump="$(objdump -T "$_e" 2>/dev/null || true)"
    fi
    printf '%s\n' "$_symdump" \
        | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' \
        | sed 's/^GLIBC_//' \
        | awk -F. '{ key = $1 * 1000000 + $2 * 1000 + ($3 + 0); if (key > m) { m = key; v = $0 } } END { if (m) print v }'
    unset _e _symdump
}

try_strip() {
    _f="$1"
    _st="$(pick_strip_for "$_f")"
    if [ -z "$_st" ]; then
        case "$_f" in
            *linux*|*Linux*) warn "no ELF strip tool for $_f (ships unstripped; brew install llvm)";;
            *) warn "no strip tool for $_f (ships unstripped)";;
        esac
        unset _f _st
        return 0
    fi
    _before="$(wc -c < "$_f")"
    if "$_st" "$_f" 2>/dev/null; then
        _after="$(wc -c < "$_f")"
        if have file; then
            _stt="$(file -b "$_f" 2>/dev/null || true)"
            log "stripped $_f ($_before -> $_after bytes) [$_st: $_stt]"
        else
            log "stripped $_f ($_before -> $_after bytes) [$_st]"
        fi
    else
        warn "strip failed for $_f with $_st (ships unstripped)"
    fi
    unset _f _st _before _after _stt
}

# --- generic setup: adds a rustup target (idempotent) --------------------------
need_target() {
    # $1 = triple
    if rustup target list --installed 2>/dev/null | grep -qx "$1"; then
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        return 1
    fi
    log "rustup target add $1"
    rustup target add "$1"
}

# `rav1d` (pure-Rust AVIF decoder) assembles its x86_64 optimizations with
# `nasm` via `nasm_rs`: required by EVERY x86_64 target, macOS Intel
# included. arm64 targets use the system assembler, no nasm needed.
ensure_nasm() {
    have nasm && return 0
    if [ "$SKIP_SETUP" = "1" ]; then
        return 1
    fi
    log "brew install nasm"
    brew install nasm >/dev/null 2>&1 || return 1
    have nasm
}

# --- release build for a triple (uses zigbuild when asked) ---------------------
build_for() {
    # $1 = triple, $2 = "zig" or "cargo"
    if [ "$2" = "zig" ]; then
        cargo zigbuild --locked --release --target "$1"
    else
        cargo build --locked --release --target "$1"
    fi
}

bin_of() {
    # $1 = triple -> resulting binary path
    if printf '%s' "$1" | grep -q "windows"; then
        printf '%s/%s/release/%s.exe' "$TARGETS" "$1" "$APP_LC"
    else
        printf '%s/%s/release/%s' "$TARGETS" "$1" "$APP_LC"
    fi
}

# --- .app macOS --------------------------------------------------------------
mk_app() {
    # $1 = .app dest dir, $2 = binary. Returns non-zero (never aborts the
    # caller under `set -e` when used as `if ! mk_app`): `do_mac` turns that
    # into a SKIP with a reason so `--all` keeps going.
    app="$1"
    if [ ! -f "$2" ]; then
        echo "ERROR: mk_app source binary not found: $2" >&2
        return 1
    fi
    if [ ! -f "$ROOT/assets/icons/icon.icns" ]; then
        echo "ERROR: assets/icons/icon.icns not found (run make icons)" >&2
        return 1
    fi
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" || return 1
    cp "$2" "$app/Contents/MacOS/$APP_UI" || return 1
    chmod +x "$app/Contents/MacOS/$APP_UI" || return 1
    # Bundle icon: Finder, the Dock and Cmd+Tab read it from here (a bare
    # binary has no icon on macOS; the runtime `with_icon` is ignored).
    cp "$ROOT/assets/icons/icon.icns" "$app/Contents/Resources/icon.icns" || return 1
    # License compliance: MPL-2.0 for the app, notices for bundled deps
    # and fonts, inside the .app bundle.
    cp "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/LICENSE" \
        "$app/Contents/Resources/" || return 1
    cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$APP_UI</string>
    <key>CFBundleIdentifier</key><string>dev.mdowser.app</string>
    <key>CFBundleName</key><string>$APP_UI</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundleVersion</key><string>$VER</string>
    <key>CFBundleShortVersionString</key><string>$VER</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    # Best-effort ad-hoc signing (no substitute for a Developer ID signature).
    codesign -s - --deep --force "$app" 2>/dev/null || true
    # Verify the signature survived staging: informational only (an unsigned
    # .app still opens via System Settings > Privacy & Security > Open Anyway).
    codesign --verify --deep --strict "$app" 2>/dev/null || \
        printf '··· WARN: ad-hoc signature did not verify for %s\n' "$app"
}

# --- minimal .deb with no external dependencies --------------------------------
mk_deb() {
    # $1 = ELF binary, $2 = deb arch ("amd64"|"arm64"), $3 = target .deb
    # deb layout: debian-binary + control.tar.gz (`control` at root)
    #             + data.tar.gz (`usr/...` tree).
    # macOS `ar` (BSD + automatic ranlib) DESTROYS non-object archives
    # (verified bug: leaves 96 bytes with only `__.SYMDEF`), so `ar`
    # is hand-written with python3 stdlib (trivial format, no dependencies).
    need_python3 "the .deb writer" || return 1
    #
    # The `/usr` tree (binary, desktop entry, AppStream metadata, hicolor
    # icons — ladder AND scalable — and the man page) comes from the SHARED
    # `install_linux_tree` (`scripts/packaging.sh`), the same function
    # `make install`/the RPM `%install` use: the .deb, the .rpm and a manual
    # install therefore ship one identical layout. Only the Debian-specific
    # pieces (control, /usr/share/doc) are added here.
    stage="$DIST/.deb-stage"
    rm -rf "$stage"
    mkdir -p "$stage/control" "$stage/data/usr/share/doc/$APP_LC"
    # Desktop integration: the hicolor icon ladder feeds the window list, app
    # menus and the GNOME/KDE launchers; `StartupWMClass` (in the shared
    # .desktop) matches the Wayland app id set in `main`.
    install_linux_tree "$stage/data" "/usr" "$1" "$VER" || {
        echo "ERROR: staging the Linux runtime tree for $3 failed" >&2
        return 1
    }
    # Debian convention: /usr/share/doc/<pkg>/copyright with the notices.
    {
        printf 'MDowser %s\nLicense: MPL-2.0\n\n' "$VER"
        cat "$ROOT/THIRD_PARTY_NOTICES.md"
    } > "$stage/data/usr/share/doc/$APP_LC/copyright"
    cp "$ROOT/LICENSE" "$stage/data/usr/share/doc/$APP_LC/"
    # `Installed-Size` is required by policy and read by apt; KiB, like `du -k`.
    _installed_kb="$(du -sk "$stage/data" 2>/dev/null | awk '{print $1}')"
    # `libc6 (>= x.y)`: the binary's OWN required glibc floor, the value
    # `dpkg-shlibdeps` would derive. Without it `dpkg -i` happily installs
    # on a distro too old for the binary and the app dies at startup with
    # "GLIBC_2.xx not found". The binary also NEEDs libdl/libm/libpthread,
    # all part of libc6. Best effort: a host with no ELF reader keeps the
    # plain dependency (no worse than before).
    _glibc="$(elf_glibc_floor "$1")"
    if [ -n "$_glibc" ]; then
        _deb_deps="libc6 (>= $_glibc), ca-certificates"
    else
        _deb_deps="ca-certificates"
    fi
    # Debian policy requires `Name <email>` in the control file; `.invalid`
    # is the reserved placeholder until the project has a real address.
    # (Comment lines are only legal in SOURCE control files: dpkg rejects a
    # `#` line here with "field name '#' must be followed by colon".)
    cat > "$stage/control/control" <<EOF
Package: $APP_LC
Version: $VER
Section: web
Priority: optional
Architecture: $2
Installed-Size: ${_installed_kb:-0}
Depends: $_deb_deps
Rules-Requires-Root: no
Maintainer: MDowser <noreply@mdowser.invalid>
Homepage: https://github.com/MDowser/MDowser
Description: Minimal browser for text/markdown (CommonMark)
 MDowser shows text/markdown documents served over HTTPS and local
 Markdown files. It registers as a Markdown viewer, so file managers can
 open .md documents with it.
 TLS verification uses the system trust store (rustls-platform-verifier).
EOF
    unset _installed_kb _glibc _deb_deps

    # dpkg's parser has no comment syntax in a binary control file: a `#`
    # line is a hard parse error ("field name '#' must be followed by
    # colon"). Catch a reintroduced one here, before the .deb is wrapped.
    if grep -q '^#' "$stage/control/control"; then
        echo "ERROR: comment line in the .deb control file for $3" >&2
        return 1
    fi

    # Ownership normalization: `tar` would record the BUILDER's uid/gid
    # (macOS often 501/20) and dpkg preserves archive ownership on install,
    # leaving the installed files owned by a UID that does not exist on the
    # target. GNU and BSD tar both accept the long owner flags. The sweep
    # drops any AppleDouble sidecar / xattr a copy left on a payload file
    # (bsdtar would otherwise archive `com.apple.provenance`).
    sweep_apple_double "$stage"
    (cd "$stage/control" && tar -czf "$stage/control.tar.gz" \
        --owner=0 --group=0 --numeric-owner $TAR_NO_XATTRS control)
    (cd "$stage/data" && tar -czf "$stage/data.tar.gz" \
        --owner=0 --group=0 --numeric-owner $TAR_NO_XATTRS usr)
    printf '2.0\n' > "$stage/debian-binary"
    # Verify the archives before wrapping: a silent/incomplete tar must fail
    # loudly here, never produce a broken .deb downstream. A macOS sidecar in
    # the payload is caught too (dpkg would install it as junk).
    if tar -tzf "$stage/data.tar.gz" | grep -qE '(^|/)\._'; then
        echo "ERROR: AppleDouble junk in the data payload for $3" >&2
        return 1
    fi
    tar -tzf "$stage/control.tar.gz" | grep -qx "control" || {
        echo "ERROR: bad control.tar.gz for $3" >&2
        return 1
    }
    tar -tzf "$stage/data.tar.gz" | grep -qx "usr/bin/$APP_LC" || {
        echo "ERROR: data.tar.gz misses usr/bin/$APP_LC for $3" >&2
        return 1
    }
    tar -tzf "$stage/data.tar.gz" | grep -qx "usr/share/applications/$APP_LC.desktop" || {
        echo "ERROR: data.tar.gz misses the desktop entry for $3" >&2
        return 1
    }
    tar -tzf "$stage/data.tar.gz" | grep -qx "usr/share/icons/hicolor/256x256/apps/$APP_LC.png" || {
        echo "ERROR: data.tar.gz misses the app icon for $3" >&2
        return 1
    }
    tar -tzf "$stage/data.tar.gz" \
        | grep -qx "usr/share/metainfo/dev.mdowser.app.metainfo.xml" || {
        echo "ERROR: data.tar.gz misses the AppStream metadata for $3" >&2
        return 1
    }
    tar -tzf "$stage/data.tar.gz" \
        | grep -qx "usr/share/icons/hicolor/scalable/apps/$APP_LC.svg" || {
        echo "ERROR: data.tar.gz misses the scalable icon for $3" >&2
        return 1
    }
    if ! tar -tzf "$stage/data.tar.gz" \
        | grep -Eq "usr/share/man/man1/$APP_LC\.1(\.gz)?$"; then
        echo "ERROR: data.tar.gz misses the man page for $3" >&2
        return 1
    fi
    _MKDEB_STAGE="$stage" _MKDEB_OUT="$3" python3 -c "
import os
stage = os.environ['_MKDEB_STAGE']; out = os.environ['_MKDEB_OUT']
def member(path, name, mode=0o100644):
    with open(os.path.join(stage, path), 'rb') as f:
        data = f.read()
    head = b'%-16s%-12d%-6d%-6d%-8o%-10d\x60\x0a' % (name.encode(), 0, 0, 0, mode, len(data))
    return head + data + (b'\n' if len(data) % 2 else b'')
with open(out, 'wb') as w:
    w.write(b'!<arch>\n')
    w.write(member('debian-binary', 'debian-binary'))
    w.write(member('control.tar.gz', 'control.tar.gz'))
    w.write(member('data.tar.gz', 'data.tar.gz'))
" || {
        # Called from an `if`/`||` list, where `set -e` does not fire: an
        # unchecked writer failure used to report the platform as BUILT with
        # no .deb on disk.
        echo "ERROR: could not write the .deb $3" >&2
        return 1
    }
    if [ ! -f "$3" ]; then
        echo "ERROR: no .deb written at $3" >&2
        return 1
    fi
    # Sanity: a .deb holding an ~18MB binary cannot weigh kilobytes.
    if [ "$(wc -c < "$3")" -lt 1000000 ]; then
        echo "ERROR: suspiciously small .deb $3" >&2
        return 1
    fi
    rm -rf "$stage"
}

zip_of() {
    # $1 = source (file or dir; a dir lands at the zip root: right for .app),
    # $2 = target zip. Verifies the result (fails loudly when empty).
    # `-X` drops the Unix uid/gid extra field (`0x7875`) Info-ZIP would
    # otherwise store, which would leak the builder's numeric uid/gid.
    if have zip; then
        rm -f "$2"
        if [ -d "$1" ]; then
            (cd "$(dirname "$1")" && zip -qrX "$2" "$(basename "$1")" -x '*.DS_Store')
        else
            (cd "$(dirname "$1")" && zip -qX "$2" "$(basename "$1")" -x '*.DS_Store')
        fi
    elif have ditto; then
        rm -f "$2"
        ditto -c -k --norsrc --noqtn "$1" "$2" 2>/dev/null || ditto -c -k "$1" "$2"
    else
        need_python3 "the zip fallback" || return 1
        rm -f "$2"
        _zipsrc="$1" _zipdst="$2" python3 -c \
            "import os,shutil;shutil.make_archive(os.path.splitext(os.environ['_zipdst'])[0],'zip',root_dir=os.path.dirname(os.environ['_zipsrc']),base_dir=os.path.basename(os.environ['_zipsrc']))"
    fi
    # Verification: the zip must exist and weigh something (else abort here
    # instead of soldiering on with dangling staging + broken checksums).
    if [ ! -s "$2" ]; then
        echo "ERROR: zip_of produced no $2" >&2
        return 1
    fi
}

zip_contents() {
    # Like zip_of but packs a dir's CONTENTS at the zip root
    # (for .exe + README, no staging prefix). $1 = dir, $2 = dest,
    # remaining args = files inside $1 (passed as "$@" so spaces survive).
    dest="$2"; dir="$1"; shift 2
    if have zip; then
        rm -f "$dest"
        # `-X`: no Unix uid/gid extra field (see zip_of).
        (cd "$dir" && zip -qX "$dest" "$@" -x '*.DS_Store')
    else
        # `zip`/`ditto` missing (or ditto, which cannot select files):
        # python stdlib with argv (no `split(' ')`, so `My File.exe` survives).
        need_python3 "the zip fallback" || return 1
        rm -f "$dest"
        _zdir="$dir" _zdst="$dest" python3 - "$@" <<'PY'
import os, sys, zipfile
zdir = os.environ['_zdir']; zdst = os.environ['_zdst']
with zipfile.ZipFile(zdst, 'w', zipfile.ZIP_DEFLATED) as z:
    for f in sys.argv[1:]:
        z.write(os.path.join(zdir, f), f)
PY
    fi
    if [ ! -s "$dest" ]; then
        echo "ERROR: zip_contents produced no $dest" >&2
        return 1
    fi
}

# --- groups ------------------------------------------------------------------
do_mac() {
    # Runs at most once per invocation: `--all` calls it directly AND via
    # `do_host` on a macOS host.
    if [ "${_DID_MAC:-0}" = "1" ]; then return 0; fi
    _DID_MAC=1
    log "macOS: universal .app (arm64 + x86_64)"
    need_target "aarch64-apple-darwin" || { warn "mac-app-universal (no aarch64-apple-darwin target)"; return 0; }
    need_target "x86_64-apple-darwin" || { warn "mac-app-universal (no x86_64-apple-darwin target)"; return 0; }
    # Intel needs nasm for rav1d's AVX kernels; the universal .app needs BOTH
    # arches, so a missing tool or a failed build skips the group (best-effort,
    # no abort: the script moves on to Linux/Windows and the summary lists it).
    if ! ensure_nasm; then
        warn "mac-app-universal (nasm missing for the x86_64 build; brew install nasm)"
        return 0
    fi
    if ! build_for "aarch64-apple-darwin" "cargo"; then
        warn "mac-arm64 (build failed)"
        return 0
    fi
    if ! build_for "x86_64-apple-darwin" "cargo"; then
        warn "mac-x86_64 (build failed)"
        return 0
    fi
    try_strip "$(bin_of aarch64-apple-darwin)"
    try_strip "$(bin_of x86_64-apple-darwin)"
    arm_bin="$(bin_of aarch64-apple-darwin)"
    x64_bin="$(bin_of x86_64-apple-darwin)"
    # macOS ships ONE artifact: the universal .app (arm64 + Intel), zipped. No
    # loose per-arch binaries: the `.app` is what Finder/Dock launch, and the
    # license/notice set travels inside `Contents/Resources` (`mk_app`).
    if ! have lipo; then
        warn "mac-app-universal (no lipo; install the Xcode Command Line Tools)"
        return 0
    fi
    # Guarded like the builds above: a failed `lipo`/`mk_app` skips the group
    # instead of aborting the whole `--all` run under `set -e`.
    if ! lipo -create -output "$DIST/.universal-bin" "$arm_bin" "$x64_bin"; then
        warn "mac-app-universal (lipo failed)"
        rm -f "$DIST/.universal-bin"
        unset arm_bin x64_bin
        return 0
    fi
    if ! mk_app "$DIST/$APP_UI-$VER-macos-universal.app" "$DIST/.universal-bin"; then
        warn "mac-app-universal (staging the .app failed)"
        rm -f "$DIST/.universal-bin"
        rm -rf "$DIST/$APP_UI-$VER-macos-universal.app"
        unset arm_bin x64_bin
        return 0
    fi
    rm -f "$DIST/.universal-bin"
    # Artifact hygiene: signing and the build plant `com.apple.provenance`
    # xattrs on every file, and ditto/Info-ZIP would then add AppleDouble
    # `._name` members (builder metadata, and they made the extracted bundle
    # fail `codesign --verify`). Strip them from the staged bundle BEFORE
    # packing — the ad-hoc signature does not seal xattrs (verified: it still
    # passes `codesign --verify --deep --strict` after the sweep).
    sweep_apple_double "$DIST/$APP_UI-$VER-macos-universal.app"
    # Guarded like the deb/zip steps: a failed packaging step skips the
    # artifact with a reason instead of aborting the whole `--all` run
    # (`set -e` would otherwise kill the script on a plain `return 1`).
    # `ditto` preserves the .app signature/resource metadata better than
    # Info-ZIP, so it packs the macOS artifact when present; the metadata
    # flags keep the AppleDouble sidecars out even if an xattr escapes the
    # sweep.
    if have ditto; then
        if (cd "$DIST" && rm -f "$DIST/$APP_UI-$VER-macos-universal.app.zip" && ditto -c -k --keepParent --norsrc --noqtn --noextattr "$APP_UI-$VER-macos-universal.app" "$APP_UI-$VER-macos-universal.app.zip"); then
            BUILT="$BUILT mac-app-universal"
        else
            warn "mac-app-universal (zip failed)"
        fi
    elif (cd "$DIST" && zip_of "$DIST/$APP_UI-$VER-macos-universal.app" "$DIST/$APP_UI-$VER-macos-universal.app.zip"); then
        BUILT="$BUILT mac-app-universal"
    else
        warn "mac-app-universal (zip failed)"
    fi
    rm -rf "$DIST/$APP_UI-$VER-macos-universal.app"
    unset arm_bin x64_bin
}

do_linux() {
    log "Linux: x86_64 + arm64 (via zig)"
    # zig as linker: no sysroot/docker needed.
    if ! have zig; then
        if [ "$SKIP_SETUP" = "1" ]; then warn "linux (no zig)"; return 0; fi
        log "brew install zig"
        brew install zig || { warn "linux (zig install failed)"; return 0; }
    fi
    # `cargo-zigbuild --version` (the binary), not `cargo zigbuild
    # --version`: cargo subcommands do not implement `--version`, so the
    # old probe reported an installed tool as missing and silently skipped
    # the whole Linux group.
    if ! cargo-zigbuild --version >/dev/null 2>&1; then
        if [ "$SKIP_SETUP" = "1" ]; then warn "linux (no cargo-zigbuild)"; return 0; fi
        log "cargo install cargo-zigbuild"
        cargo install cargo-zigbuild || { warn "linux (cargo-zigbuild install failed)"; return 0; }
    fi
    for triple in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
        need_target "$triple" || { warn "linux-$triple (no target)"; continue; }
        case "$triple" in
            x86_64-*) if ! ensure_nasm; then
                warn "linux-$triple (nasm missing; brew install nasm)"
                continue
            fi ;;
        esac
        if ! build_for "$triple" "zig"; then
            warn "linux-$triple (build failed)"
            continue
        fi
        try_strip "$(bin_of "$triple")"
        arch="${triple%%-*}"
        [ "$arch" = "x86_64" ] && debarch="amd64" || debarch="arm64"
        b="$(bin_of "$triple")"
        stage="$DIST/.linux-$arch"
        rm -rf "$stage"; mkdir -p "$stage"
        # Guarded: an unguarded `cp`/`tar` failure (full disk, a missing
        # README) used to abort the WHOLE multi-OS run under `set -e` before
        # the later groups and the summary.
        if cp "$b" "$stage/$APP_LC" \
            && cp "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" \
                "$ROOT/LICENSE" "$stage/" \
            && sweep_apple_double "$stage" \
            && (cd "$stage" && tar -czf "$DIST/$APP_LC-$VER-linux-$arch.tar.gz" \
                --owner=0 --group=0 --numeric-owner $TAR_NO_XATTRS \
                "$APP_LC" README.md THIRD_PARTY_NOTICES.md LICENSE); then
            # Best-effort packaging: a failed .deb keeps the .tar.gz and
            # skips only this artifact (never aborts the multi-OS run).
            mk_deb "$b" "$debarch" "$DIST/${APP_LC}_${VER}_${debarch}.deb" ||
                warn "linux-$arch (.deb failed; .tar.gz kept)"
            BUILT="$BUILT linux-$arch"
        else
            warn "linux-$arch (staging/tar.gz failed)"
        fi
        rm -rf "$stage"
    done
}

do_rpm() {
    # Runs at most once per invocation (`--all` and the Linux host path both
    # reach it).
    if [ "${_DID_RPM:-0}" = "1" ]; then return 0; fi
    _DID_RPM=1
    # RPM built from SOURCE (rpmbuild), unlike the prebuilt-binary .tar.gz/
    # .deb/.zip: `packaging/rpm/mdowser.spec` compiles the crate in %build.
    # Meaningful only on a host with rpmbuild (Fedora/RHEL/openSUSE);
    # everywhere else the group is skipped with a reason. The spec's %install
    # runs `make install`, which stages the SAME packaging/linux/ desktop
    # entry, AppStream metadata, icons and man page the .deb carries, so the
    # two packages cannot drift.
    log "RPM: $APP_LC $VER (rpmbuild, from source)"
    if ! have rpmbuild; then
        warn "rpm (rpmbuild not found; install rpm-build, or run make dist-rpm on a Fedora/RHEL host)"
        return 0
    fi
    _top="$DIST/.rpmbuild"
    rm -rf "$_top"
    mkdir -p "$_top/BUILD" "$_top/BUILDROOT" "$_top/RPMS" "$_top/SOURCES" \
        "$_top/SPECS" "$_top/SRPMS"
    # Full source tarball (the spec builds it), with the tree re-rooted
    # under mdowser-<ver>/ for `%autosetup`. GNU tar and bsdtar take
    # different re-root flags (`--transform` is GNU-only; bsdtar, the macOS
    # default, rejects it and takes sed-style `-s`), so probe the
    # implementation instead of failing `--rpm` on a brew-rpmbuild host.
    # Excludes are the build output and the AI/editor scratch AGENTS.md
    # keeps untracked, plus the local-secret files `.gitignore` already
    # treats as sensitive (`.env*`) and the Aider scratch dir.
    _pkg="$APP_LC-$VER"
    if tar --version 2>/dev/null | grep -qi 'gnu tar'; then
        _tar_root="--transform=s,^\.,$_pkg,"
    else
        _tar_root="-s,^\.,$_pkg,"
    fi
    # Word-splitting on $_tar_root is intended: it is exactly one flag.
    # shellcheck disable=SC2086
    if ! tar -C "$ROOT" \
        --exclude='./target' --exclude='./dist' --exclude='./.git' \
        --exclude='./.opencode' --exclude='./.openchamber' \
        --exclude='./.claude' --exclude='./.codex' --exclude='./.cursor' \
        --exclude='./.env*' --exclude='./.aider*' \
        --exclude='./packaging/flatpak/.build-dir' \
        --exclude='./.flatpak-builder' \
        --exclude='.DS_Store' \
        --exclude='./._*' --exclude='*/._*' \
        $TAR_NO_XATTRS \
        $_tar_root \
        -czf "$_top/SOURCES/$_pkg.tar.gz" .; then
        warn "rpm (source tarball failed)"
        rm -rf "$_top"
        unset _top _pkg _tar_root
        return 0
    fi
    unset _tar_root
    cp "$ROOT/packaging/rpm/$APP_LC.spec" "$_top/SPECS/$APP_LC.spec"
    if rpmbuild -bb "$_top/SPECS/$APP_LC.spec" \
        --define "_topdir $_top" \
        --define "ver $VER" \
        --define "_sourcedir $_top/SOURCES" \
        --define "_builddir $_top/BUILD" \
        --define "_rpmdir $_top/RPMS" \
        --define "_srcrpmdir $_top/SRPMS"; then
        _rpm_count=0
        for _rpm in "$_top"/RPMS/*/*.rpm; do
            [ -f "$_rpm" ] || continue
            cp "$_rpm" "$DIST/"
            _rpm_count=$((_rpm_count + 1))
        done
        if [ "$_rpm_count" -gt 0 ]; then
            BUILT="$BUILT linux-rpm"
        else
            warn "linux-rpm (rpmbuild produced no .rpm)"
        fi
        unset _rpm _rpm_count
    else
        warn "linux-rpm (rpmbuild failed)"
    fi
    rm -rf "$_top"
    unset _top _pkg
}

do_flatpak() {
    # Runs at most once per invocation (`--all`, `--linux` and the Linux host
    # path all reach it).
    if [ "${_DID_FLATPAK:-0}" = "1" ]; then return 0; fi
    _DID_FLATPAK=1
    # ONE distributable bundle for every Linux distribution (dev.mdowser.app),
    # built from source by `scripts/build-flatpak.sh` into
    # dist/mdowser-<ver>.flatpak. Needs flatpak-builder + the freedesktop
    # runtimes; skipped with a reason everywhere else (macOS/Windows).
    log "Flatpak: $APP_LC (dev.mdowser.app)"
    if ! have flatpak-builder; then
        warn "flatpak (flatpak-builder not found; install it or run make deps)"
        return 0
    fi
    if "$ROOT/scripts/build-flatpak.sh"; then
        BUILT="$BUILT linux-flatpak"
    else
        warn "linux-flatpak (build failed; see scripts/build-flatpak.sh)"
    fi
}

do_windows() {
    log "Windows: x86_64 (.exe via mingw-w64)"
    triple="x86_64-pc-windows-gnu"
    need_target "$triple" || { warn "windows (no $triple target)"; return 0; }
    if ! have x86_64-w64-mingw32-gcc; then
        if [ "$SKIP_SETUP" = "1" ]; then warn "windows (no mingw-w64)"; return 0; fi
        log "brew install mingw-w64"
        brew install mingw-w64 || { warn "windows (mingw-w64 install failed)"; return 0; }
    fi
    # rav1d's x86_64 assembly needs nasm (see ensure_nasm).
    if ! ensure_nasm; then
        warn "windows (nasm missing; brew install nasm)"
        return 0
    fi
    # cc/link for the GNU target (covers aws-lc-sys).
    export CC_x86_64_pc_windows_gnu="x86_64-w64-mingw32-gcc"
    export AR_x86_64_pc_windows_gnu="x86_64-w64-mingw32-ar"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="x86_64-w64-mingw32-gcc"
    build_for "$triple" "cargo" || { warn "windows (build failed)"; return 0; }
    # The icon, VERSIONINFO and manifest are embedded by build.rs
    # (windres/rc.exe -> .rsrc). Check the section survived the link;
    # informational only (the binaries are fine either way).
    _exe="$(bin_of "$triple")"
    if have x86_64-w64-mingw32-objdump; then
        if x86_64-w64-mingw32-objdump -h "$_exe" 2>/dev/null | grep -q '\.rsrc'; then
            log "resources embedded in $APP_LC.exe (icon, version info, manifest)"
        else
            printf '··· WARN: %s has no .rsrc section (check mingw-w64 windres)\n' "$_exe"
        fi
    fi
    unset _exe
    try_strip "$(bin_of "$triple")"
    stage="$DIST/.win-x64"
    rm -rf "$stage"; mkdir -p "$stage"
    # Guarded like the Linux staging: a failed copy must warn, not abort the
    # multi-OS run before the summary.
    if cp "$(bin_of "$triple")" "$stage/$APP_LC.exe" \
        && cp "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" \
            "$ROOT/LICENSE" "$stage/" \
        && sweep_apple_double "$stage"; then
        # Content at the zip root (no staging prefix). A failed zip skips the
        # group with a reason instead of aborting the run.
        if zip_contents "$stage" "$DIST/$APP_LC-$VER-windows-x86_64.zip" \
            "$APP_LC.exe" README.md THIRD_PARTY_NOTICES.md LICENSE; then
            BUILT="$BUILT windows-x86_64"
        else
            warn "windows-x86_64 (zip failed)"
        fi
    else
        warn "windows-x86_64 (staging failed)"
    fi
    rm -rf "$stage"
}

do_host() {
    # Current-OS package (no cross toolchains), matching the per-platform
    # artifact set: macOS -> the universal .app, Linux -> .tar.gz + .deb/.rpm +
    # Flatpak, Windows -> the .exe zip. No raw binary: dist/ holds just the
    # packaged archive(s) with the same docs every other artifact carries,
    # reusing the per-OS names so the output stays a subset of `make dist`.
    _host_triple="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
    log "host: ${_host_triple:-unknown}"
    # macOS's only artifact is the universal .app, which needs BOTH arches and
    # lipo: hand the whole job to `do_mac` (never a host-only tarball).
    case "$_host_triple" in
        *darwin*|*macos*)
            do_mac
            unset _host_triple
            return 0
            ;;
    esac
    # Guarded like the cross builds: a failed host build warns instead of
    # aborting under `set -e` with no summary.
    if ! cargo build --locked --release; then
        warn "host (${_host_triple:-unknown} build failed)"
        unset _host_triple
        return 0
    fi
    _host_bin="$TARGETS/release/$APP_LC"
    [ -f "$_host_bin.exe" ] && _host_bin="$_host_bin.exe"
    try_strip "$_host_bin"
    # Derive OS/arch labels from the host triple (fallback: uname). macOS
    # never reaches this point: the early return above handed it to `do_mac`.
    _host_os=""; _host_arch=""
    case "$_host_triple" in
        *linux*)
            _host_os="linux"
            _host_arch="${_host_triple%%-*}"
            ;;
        *windows*|*mingw*)
            _host_os="windows"
            _host_arch="${_host_triple%%-*}"
            [ -z "$_host_arch" ] && _host_arch="x86_64"
            ;;
    esac
    if [ -z "$_host_os" ] || [ -z "$_host_arch" ]; then
        _u_s="$(uname -s 2>/dev/null || printf 'unknown')"
        _u_m="$(uname -m 2>/dev/null || printf 'unknown')"
        case "$_u_s" in
            Darwin) [ -z "$_host_os" ] && _host_os="macos" ;;
            Linux) [ -z "$_host_os" ] && _host_os="linux" ;;
            MINGW*|MSYS*|CYGWIN*|Windows_NT) [ -z "$_host_os" ] && _host_os="windows" ;;
            *) [ -z "$_host_os" ] && _host_os="$(printf '%s' "$_u_s" | tr '[:upper:]' '[:lower:]')" ;;
        esac
        if [ -z "$_host_arch" ]; then
            case "$_u_m" in
                aarch64|arm64)
                    if [ "$_host_os" = "linux" ]; then _host_arch="aarch64"; else _host_arch="arm64"; fi
                    ;;
                x86_64|amd64) _host_arch="x86_64" ;;
                *) _host_arch="$_u_m" ;;
            esac
        fi
        unset _u_s _u_m
    fi
    # Package the host binary with the same docs every other artifact carries.
    _hstage="$DIST/.host-stage"
    rm -rf "$_hstage"; mkdir -p "$_hstage"
    if [ "$_host_os" = "windows" ]; then
        cp "$_host_bin" "$_hstage/$APP_LC.exe"
        cp "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" \
            "$ROOT/LICENSE" "$_hstage/"
        sweep_apple_double "$_hstage"
        if zip_contents "$_hstage" "$DIST/$APP_LC-$VER-windows-$_host_arch.zip" \
            "$APP_LC.exe" README.md THIRD_PARTY_NOTICES.md LICENSE; then
            BUILT="$BUILT host-zip"
        else
            warn "host-windows-$_host_arch (zip failed)"
        fi
    else
        cp "$_host_bin" "$_hstage/$APP_LC"
        cp "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" \
            "$ROOT/LICENSE" "$_hstage/"
        sweep_apple_double "$_hstage"
        if (cd "$_hstage" && tar -czf "$DIST/$APP_LC-$VER-$_host_os-$_host_arch.tar.gz" \
            --owner=0 --group=0 --numeric-owner $TAR_NO_XATTRS \
            "$APP_LC" README.md THIRD_PARTY_NOTICES.md LICENSE); then
            BUILT="$BUILT host-tar"
        else
            warn "host-$_host_os-$_host_arch (tar failed)"
        fi
        # On Linux also offer the .deb for the host arch (no zig needed:
        # mk_deb only needs python3 + icons). Same name as do_linux, so
        # `make` output stays a subset of `make dist`.
        if [ "$_host_os" = "linux" ]; then
            case "$_host_arch" in
                x86_64) _hdeb="amd64" ;;
                aarch64|arm64) _hdeb="arm64" ;;
                *) _hdeb="" ;;
            esac
            if [ -n "$_hdeb" ]; then
                if mk_deb "$_host_bin" "$_hdeb" "$DIST/${APP_LC}_${VER}_${_hdeb}.deb"; then
                    BUILT="$BUILT host-deb"
                else
                    warn "host-linux-$_host_arch (.deb failed; .tar.gz kept)"
                fi
            fi
            unset _hdeb
            # An RPM only when the host can actually build one (Fedora/RHEL/
            # openSUSE); skipped silently elsewhere so `make` on Debian/Arch
            # stays quiet.
            if have rpmbuild; then
                do_rpm
            fi
            # Flatpak too (best-effort): one bundle for every distribution.
            do_flatpak
        fi
    fi
    rm -rf "$_hstage"
    unset _host_triple _host_bin _host_os _host_arch _hstage
}

# --- main --------------------------------------------------------------------
: "${DIST:?DIST must be set}"
mkdir -p "$DIST"
# dist/ semantics: the COMPLETE modes rebuild it from scratch; a single-group
# flag ADDS its artifacts to whatever is already there. The old unconditional
# wipe destroyed the other platforms' packages on every focused rerun (CI had
# to stash dist/ and re-merge it around `--host-only`), so `make dist-linux`
# after `make dist-mac` now keeps both sets.
case "$MODE" in
    --all|--host-only)
        rm -rf "$DIST"
        ;;
esac
mkdir -p "$DIST"

case "$MODE" in
    --host-only) do_host ;;
    --mac)       do_mac ;;
    --linux)     do_linux; do_rpm; do_flatpak ;;
    --windows)   do_windows ;;
    --rpm)       do_rpm ;;
    --flatpak)   do_flatpak ;;
    --all)       do_host; do_mac; do_linux; do_rpm; do_flatpak; do_windows ;;
    *) echo "unknown flag: $MODE (use --all|--host-only|--mac|--linux|--windows|--rpm|--flatpak)"; exit 2 ;;
esac

[ -z "$BUILT" ] && { echo "ERROR: nothing was generated."; exit 1; }

# Sweep staging leftovers before checksums (a loose .app from a failed zip
# used to break `shasum` and abort the summary under `set -e`: bug). The
# EXIT trap covers aborts; this covers success and keeps `dist/` final-only.
stage_dirs
rm -f "$DIST"/.DS_Store
# If the versioned .app survived zipping (it should not), out too:
if [ -f "$DIST/$APP_UI-$VER-macos-universal.app.zip" ]; then
    rm -rf "$DIST/$APP_UI-$VER-macos-universal.app"
fi

# Privacy gate: no artifact may carry the builder's absolute paths or
# build-host metadata (uid/gid, xattrs). Fail BEFORE writing checksums so a
# leaking build is never reported as ready (see scripts/check-dist.sh).
# `--require`: a missing python3 is a FAILURE here (exit 2), never a silent
# SKIP that would report an unchecked build as ready.
if ! "$ROOT/scripts/check-dist.sh" --require "$DIST"; then
    echo "ERROR: dist/ artifacts leak build-host information (see above)" >&2
    exit 1
fi

# Checksums of FINAL files only (no dirs, no dotfiles, no self-inclusion).
# Space-safe: no `xargs` splitting (filenames with spaces stay intact).
# `shasum` is macOS/Perl, `sha256sum` is GNU coreutils: accept either.
if have shasum; then
    _HASH="shasum -a 256"
elif have sha256sum; then
    _HASH="sha256sum"
else
    echo "ERROR: no SHA-256 tool (need shasum or sha256sum)" >&2
    exit 1
fi
rm -f "$DIST/SHA256SUMS.txt"
(cd "$DIST" && {
    for f in *; do
        # `if`, not `[ ] && [ ] &&`: with `set -e` a failing last member of an
        # `&&` list would abort the loop (and the script) when the name that
        # sorts last is skipped (`SHA256SUMS.txt`).
        if [ -f "$f" ] && [ "$f" != "SHA256SUMS.txt" ]; then
            # shellcheck disable=SC2086: `_HASH` is a command plus its args
            # (`shasum -a 256` vs `sha256sum`); splitting is intended.
            $_HASH "$f"
        fi
    done
} > SHA256SUMS.txt)
unset _HASH

echo ""
echo "dist/ ready (ver=$VER):"
ls -la "$DIST" | sed -n '2,20p'
echo ""
echo "OK:$BUILT"
[ -n "$SKIPPED" ] && echo "SKIPPED:$SKIPPED (retry after installing the noted tools, or run this script natively on that OS)"
exit 0
