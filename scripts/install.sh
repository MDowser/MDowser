#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# install.sh: install MDowser's runtime files (binary + desktop integration).
#
# Usage:
#   ./scripts/install.sh                      # into /usr/local (may need sudo)
#   PREFIX=/usr      ./scripts/install.sh     # into /usr
#   DESTDIR=/tmp/root PREFIX=/usr ./scripts/install.sh   # staged/packaging
#
# Variables:
#   PREFIX  install prefix (default /usr/local)
#   DESTDIR staging root prefixed to every path (default empty)
#   BIN     binary to install (default: cargo's release output, honoring
#           CARGO_TARGET_DIR/CARGO_BUILD_TARGET); run `make build` first.
#
# This is the `%install` step of the RPM spec and the manual-install path; the
# `.deb` writer stages the SAME tree through `scripts/packaging.sh`
# (`install_linux_tree`), so all three ship one layout.
set -eu

# macOS `install`/`cp` can materialize AppleDouble `._name` sidecars for files
# with extended attributes; never let those land in an installed tree (Linux
# ignores the variable).
COPYFILE_DISABLE=1
export COPYFILE_DISABLE

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Shared destination layout (single source of truth).
. "$ROOT/scripts/packaging.sh"

PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
# Match where `make build`/cargo put the binary, including a caller-set
# CARGO_TARGET_DIR (relative values resolve against the repository root).
_target_dir="${CARGO_TARGET_DIR:-target}"
# Git Bash inherits a Windows spelling; convert it before resolving relative
# values (Linux/macOS hosts have no cygpath and need no conversion).
case "$_target_dir" in
    *\\*|[A-Za-z]:[\\/]*|\\\\*)
        if command -v cygpath >/dev/null 2>&1; then
            _target_dir="$(cygpath -u "$_target_dir")"
        fi
        ;;
esac
case "$_target_dir" in
    /*) ;;
    *) _target_dir="$ROOT/$_target_dir" ;;
esac
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    BIN="${BIN:-$_target_dir/$CARGO_BUILD_TARGET/release/mdowser}"
else
    BIN="${BIN:-$_target_dir/release/mdowser}"
fi
unset _target_dir
VER="$(app_version)"
if [ -z "$VER" ]; then
    echo "ERROR: could not parse version from Cargo.toml" >&2
    exit 1
fi

if [ ! -f "$BIN" ]; then
    echo "ERROR: $BIN not found; build first (make build, or cargo build --locked --release)" >&2
    exit 1
fi

# destdir semantics: the prefix is prefixed literally, never resolved, so a
# staged install never escapes into the real filesystem.
printf '==> installing MDowser %s to %s%s\n' "$VER" "$DESTDIR" "$PREFIX"
install_linux_tree "$DESTDIR" "$PREFIX" "$BIN" "$VER"

printf '\ninstalled:\n'
printf '  %s%s/bin/mdowser\n' "$DESTDIR" "$PREFIX"
printf '  %s%s/share/applications/mdowser.desktop\n' "$DESTDIR" "$PREFIX"
printf '  %s%s/share/metainfo/dev.mdowser.app.metainfo.xml\n' "$DESTDIR" "$PREFIX"
printf '  %s%s/share/icons/hicolor/{16..512}x{...}/apps/mdowser.png (+ scalable/apps/mdowser.svg)\n' "$DESTDIR" "$PREFIX"
# install_linux_tree ships the compressed page only when gzip exists, and the
# plain one otherwise; report the file that was actually installed.
if command -v gzip >/dev/null 2>&1; then
    _man_page="mdowser.1.gz"
else
    _man_page="mdowser.1"
fi
printf '  %s%s/share/man/man1/%s\n' "$DESTDIR" "$PREFIX" "$_man_page"
unset _man_page
exit 0
