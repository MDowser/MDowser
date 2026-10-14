#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# deps.sh: install MDowser's build / packaging dependencies, best-effort.
#
# Usage:
#   ./scripts/deps.sh              # everything this host can use   (= make deps)
#   ./scripts/deps.sh toolchain    # Rust/cargo, nasm, python3
#   ./scripts/deps.sh cross        # macOS: zig, cargo-zigbuild, mingw-w64, rustup targets
#   ./scripts/deps.sh packaging    # desktop-file-utils, appstream(-glib), rpm, flatpak-builder, lintian, PyYAML
#   SKIP_SETUP=1 ./scripts/deps.sh # dry run: report only, install nothing
#
# Notes:
#   - macOS uses Homebrew; Debian/Ubuntu apt-get, Fedora/RHEL dnf, Arch pacman
#     (via sudo when not root).
#   - BEST-EFFORT and idempotent: a tool already on PATH is left alone, and a
#     package that cannot be installed is a SKIP with the reason, never a
#     failure. `SKIP_SETUP=1` is the same switch `make dist` honors.
#   - Rust >= 1.98 is required. On macOS `brew install rustup` + the stable
#     toolchain; on Linux the official rustup installer is NOT run silently —
#     set ALLOW_RUSTUP=1 to let this script run it, or install rustup yourself.
#   - The Xcode Command Line Tools (the macOS C compiler) cannot be installed
#     from Homebrew: this script only reports the command to run.
# `set -u` (not `set -eu`) on purpose: best-effort means a failing probe must
# fall through to the SKIP path, never abort the script.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Same toolchain PATH fixup as the rest of scripts/ (lib.sh): an existing
# install in ~/.cargo, ~/.rustup or brew is found, so `make deps` never
# reinstalls what is already there.
. "$(dirname "$0")/lib.sh"
ensure_toolchain_path

GROUP="${1:-all}"
SKIP_SETUP="${SKIP_SETUP:-0}"
ALLOW_RUSTUP="${ALLOW_RUSTUP:-0}"
OS="$(uname -s)"

have() { command -v "$1" >/dev/null 2>&1; }
step() { printf '\n==> %s\n' "$1"; }
ok() { printf '  ok    %s\n' "$1"; }
skip() { printf '  SKIP  %s (%s)\n' "$1" "$2"; }

# --- package manager ---------------------------------------------------------
PM=""
PM_SUDO=""
if have brew; then
    PM="brew"
elif have apt-get; then
    PM="apt"
    [ "$(id -u)" -eq 0 ] || PM_SUDO="sudo"
elif have dnf; then
    PM="dnf"
    [ "$(id -u)" -eq 0 ] || PM_SUDO="sudo"
elif have pacman; then
    PM="pacman"
    [ "$(id -u)" -eq 0 ] || PM_SUDO="sudo"
fi

# Installs one package for the detected manager and verifies the command.
#   $1 check command   $2 brew pkg   $3 apt pkg   $4 dnf pkg   $5 pacman pkg
#   $6 human note
ensure_pkg() {
    _ep_cmd="$1"
    if have "$_ep_cmd"; then
        ok "$_ep_cmd"
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        skip "$_ep_cmd" "$6 (SKIP_SETUP=1)"
        return 0
    fi
    _ep_pkg=""
    case "$PM" in
        brew) _ep_pkg="$2" ;;
        apt) _ep_pkg="$3" ;;
        dnf) _ep_pkg="$4" ;;
        pacman) _ep_pkg="$5" ;;
        *)
            skip "$_ep_cmd" "no supported package manager; $6"
            return 0
            ;;
    esac
    if [ -z "$_ep_pkg" ]; then
        skip "$_ep_cmd" "not packaged here; $6"
        return 0
    fi
    printf '  get   %s (%s)\n' "$_ep_pkg" "$PM"
    case "$PM" in
        brew) brew install "$_ep_pkg" >/dev/null 2>&1 || { skip "$_ep_cmd" "brew install $_ep_pkg failed"; return 0; } ;;
        # shellcheck disable=SC2086: `$PM_SUDO` is empty (root) or `sudo`;
        # splitting is intended.
        apt) $PM_SUDO apt-get install -y "$_ep_pkg" >/dev/null 2>&1 || { skip "$_ep_cmd" "apt-get install $_ep_pkg failed"; return 0; } ;;
        # shellcheck disable=SC2086: see above.
        dnf) $PM_SUDO dnf install -y "$_ep_pkg" >/dev/null 2>&1 || { skip "$_ep_cmd" "dnf install $_ep_pkg failed"; return 0; } ;;
        # shellcheck disable=SC2086: see above.
        pacman) $PM_SUDO pacman -S --noconfirm "$_ep_pkg" >/dev/null 2>&1 || { skip "$_ep_cmd" "pacman -S $_ep_pkg failed"; return 0; } ;;
    esac
    if have "$_ep_cmd"; then
        ok "$_ep_cmd"
    else
        skip "$_ep_cmd" "installed but not on PATH (open a new shell)"
    fi
}

ensure_rust() {
    if have cargo; then
        ok "cargo (Rust >= 1.98)"
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        skip "cargo" "Rust toolchain (SKIP_SETUP=1)"
        return 0
    fi
    if [ "$OS" = "Darwin" ] && have brew; then
        printf '  get   rustup + stable toolchain (brew)\n'
        brew install rustup >/dev/null 2>&1 || { skip "cargo" "brew install rustup failed"; return 0; }
        if have rustup; then
            rustup toolchain install stable --profile minimal >/dev/null 2>&1 || true
            rustup default stable >/dev/null 2>&1 || true
        elif have rustup-init; then
            rustup-init -y --profile minimal --no-modify-path >/dev/null 2>&1 || true
        fi
        if have cargo; then
            ok "cargo (Rust)"
        else
            skip "cargo" "rustup installed; open a new shell (or run rustup default stable)"
        fi
        return 0
    fi
    if [ "$ALLOW_RUSTUP" = "1" ] && have curl; then
        printf '  get   rustup (official installer, ALLOW_RUSTUP=1)\n'
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null 2>&1 \
            && have cargo && ok "cargo (Rust)" || skip "cargo" "rustup installer failed"
        return 0
    fi
    skip "cargo" "install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  (or ALLOW_RUSTUP=1 ./scripts/deps.sh)"
}

ensure_cargo_zigbuild() {
    if have cargo-zigbuild; then
        ok "cargo-zigbuild"
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        skip "cargo-zigbuild" 'needed for `make dist` Linux cross (SKIP_SETUP=1)'
        return 0
    fi
    if ! have cargo; then
        skip "cargo-zigbuild" "install the Rust toolchain first (make deps toolchain)"
        return 0
    fi
    printf '  get   cargo-zigbuild (cargo install)\n'
    if cargo install cargo-zigbuild >/dev/null 2>&1 && have cargo-zigbuild; then
        ok "cargo-zigbuild"
    else
        skip "cargo-zigbuild" "cargo install cargo-zigbuild failed"
    fi
}

ensure_rustup_targets() {
    if ! have rustup; then
        skip "rustup targets" "rustup not installed"
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        skip "rustup targets" 'cross targets for `make dist` (SKIP_SETUP=1)'
        return 0
    fi
    for _ert_t in aarch64-apple-darwin x86_64-apple-darwin \
        x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
        x86_64-pc-windows-gnu; do
        if rustup target list --installed 2>/dev/null | grep -qx "$_ert_t"; then
            ok "target $_ert_t"
        elif rustup target add "$_ert_t" >/dev/null 2>&1; then
            ok "target $_ert_t"
        else
            skip "target $_ert_t" "rustup target add failed"
        fi
    done
}

ensure_pyyaml() {
    if have python3 && python3 -c 'import yaml' >/dev/null 2>&1; then
        ok "PyYAML"
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        skip "PyYAML" 'Flatpak YAML check in `make validate` (SKIP_SETUP=1)'
        return 0
    fi
    if have python3; then
        printf '  get   PyYAML (python3 -m pip install --user)\n'
        if python3 -m pip install --user pyyaml >/dev/null 2>&1 \
            && python3 -c 'import yaml' >/dev/null 2>&1; then
            ok "PyYAML"
        else
            skip "PyYAML" "pip refused (PEP 668?); run in a venv or skip the YAML check"
        fi
    else
        skip "PyYAML" "python3 not installed"
    fi
}

# --- groups ------------------------------------------------------------------
group_toolchain() {
    step "toolchain (Rust, nasm, python3)"
    ensure_rust
    # nasm: rav1d's x86/x86_64 assembly (every x86_64 target, macOS Intel too).
    ensure_pkg nasm nasm nasm nasm nasm "required on x86/x86_64 (AVIF + rustls assembly)"
    if have python3; then
        ok "python3"
    else
        ensure_pkg python3 python3 python3 python3 python 'required for .deb/zip packaging'
    fi
    # The C compiler is not a Homebrew formula; only report it.
    if [ "$OS" = "Darwin" ]; then
        if xcode-select -p >/dev/null 2>&1; then
            ok "Xcode Command Line Tools (C compiler)"
        else
            skip "C compiler" "run: xcode-select --install"
        fi
    elif ! have cc && ! have gcc; then
        skip "C compiler" "install build-essential / gcc / base-devel for your distribution"
    else
        ok "C compiler"
    fi
}

group_cross() {
    step 'cross-build tools (multi-OS `make dist`)'
    if [ "$OS" != "Darwin" ]; then
        skip "cross tools" "only needed when crossing FROM macOS; build natively instead"
        return 0
    fi
    # zig as the Linux linker (with cargo-zigbuild) and mingw-w64 for Windows.
    ensure_pkg zig zig "" "" "" 'Linux cross linker for `make dist --linux`'
    ensure_pkg x86_64-w64-mingw32-gcc mingw-w64 "" "" "" 'Windows cross linker for `make dist --windows`'
    ensure_cargo_zigbuild
    ensure_rustup_targets
}

group_packaging() {
    step "packaging + validation tools"
    ensure_pkg desktop-file-validate desktop-file-utils desktop-file-utils desktop-file-utils desktop-file-utils 'desktop entry check (`make validate`)'
    ensure_pkg appstreamcli appstream appstream appstream appstream 'AppStream metadata check (`make validate`)'
    ensure_pkg appstream-util appstream-glib appstream-util libappstream-glib appstream-glib 'AppStream check, RPM %check'
    ensure_pkg rpmspec rpm rpm rpm-build "" 'RPM spec check (`make validate`, `make dist-rpm`)'
    # Flatpak (Linux only): the tool; the freedesktop runtimes are installed
    # best-effort by `scripts/build-flatpak.sh` at build time.
    ensure_pkg flatpak-builder flatpak-builder flatpak-builder flatpak-builder flatpak-builder 'Flatpak bundle (`make dist` / `make flatpak`)'
    # lintian is Debian-only; harmless SKIP elsewhere.
    ensure_pkg lintian "" lintian "" "" 'Debian package lint (Debian only)'
    ensure_pyyaml
}

# --- main --------------------------------------------------------------------
printf '==> deps: group=%s os=%s package-manager=%s%s\n' \
    "$GROUP" "$OS" "${PM:-none}" "$([ "$SKIP_SETUP" = "1" ] && printf ' SKIP_SETUP=1' || true)"

case "$GROUP" in
    all)
        group_toolchain
        group_cross
        group_packaging
        ;;
    toolchain) group_toolchain ;;
    cross) group_cross ;;
    packaging) group_packaging ;;
    *)
        echo "unknown group: $GROUP (use all|toolchain|cross|packaging)" >&2
        exit 2
        ;;
esac

echo ""
if [ "$SKIP_SETUP" = "1" ]; then
    echo "Dry run (SKIP_SETUP=1): nothing was installed."
fi
echo "Done. Re-read the SKIP lines for anything still missing, then:"
echo "  make            # host release + current-OS package"
echo "  make test       # full verification pipeline"
echo "  make validate   # packaging metadata checks"
exit 0
