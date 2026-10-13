#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# packaging.sh: shared Linux packaging helpers (POSIX sh, no dependencies).
#
# Sourced by `scripts/install.sh` (`make install`, the RPM `%install`) and
# `scripts/build-dist.sh` (the `.deb` writer). The destination of every
# runtime file lives HERE and nowhere else, so the native installers, the RPM
# spec and a manual `make install` can never ship different layouts.
#
# The caller sets ROOT (the repository root) before sourcing. Helpers are
# plain functions, so a failed command is the caller's to handle.

# Prints the version from the `[package]` section of Cargo.toml (empty on
# failure — callers must fail loudly on an empty tag, like build-dist.sh).
# Scoped to the stanza: a future metadata table with its own `version` key
# must never be picked up.
app_version() {
    sed -n '/^\[package\]/,/^\[/s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' \
        "${ROOT:?ROOT must be set}/Cargo.toml" | head -1
}

# Renders `packaging/linux/dev.mdowser.app.metainfo.xml` to $1 with the
# <release> version pinned to $2 and its date to today (UTC). The committed
# file keeps its own value so it stays valid standalone (Flatpak, appstream
# tooling read it from the tree); every PACKAGED copy matches the binary it
# ships with.
#
# `sed` is scoped to the release line (the only place a version/date appears),
# so the description's own text can never be touched.
#
# NOTE: POSIX sh functions share one global scope. Every variable here is
# uniquely named and NOT `unset` at the end: an `unset _root` would delete a
# caller's variable of the same name (the reason this comment exists).
render_metainfo() {
    _metainfo_out="$1"
    _metainfo_ver="$2"
    _metainfo_date="$(date -u +%Y-%m-%d 2>/dev/null || printf '1970-01-01')"
    sed -e "/^[[:space:]]*<release /s/version=\"[^\"]*\"/version=\"$_metainfo_ver\"/" \
        -e "/^[[:space:]]*<release /s/date=\"[^\"]*\"/date=\"$_metainfo_date\"/" \
        "${ROOT:?ROOT must be set}/packaging/linux/dev.mdowser.app.metainfo.xml" \
        > "$_metainfo_out"
    # A restrictive caller umask must not ship a 0600 metainfo (desktop
    # software runs as other users): same explicit 0644 every other installed
    # runtime file gets.
    chmod 0644 "$_metainfo_out"
}

# Installs the platform-neutral runtime tree (the files an RPM/Debian package
# and a manual install share) under `<destdir><prefix>`:
#
#   bin/mdowser
#   share/applications/mdowser.desktop
#   share/metainfo/dev.mdowser.app.metainfo.xml   (version/date refreshed)
#   share/icons/hicolor/<sz>x<sz>/apps/mdowser.png   (16..512)
#   share/icons/hicolor/scalable/apps/mdowser.svg
#   share/man/man1/mdowser.1.gz
#
#   $1 destdir  staging root ("" for a live install, "/usr" prefix expected)
#   $2 prefix   install prefix (e.g. /usr, /usr/local)
#   $3 binary   path to the built mdowser executable
#   $4 version  version string for the metainfo
#
# `destdir` is prefixed literally (DESTDIR semantics), never resolved: an
# empty value installs into the real prefix. Variables are uniquely named and
# never `unset` (see the note in `render_metainfo`).
install_linux_tree() {
    _ilt_bin="$3"
    _ilt_appdir="$1$2"
    if [ ! -f "$_ilt_bin" ]; then
        echo "ERROR: install source binary not found: $_ilt_bin" >&2
        return 1
    fi
    install -d "$_ilt_appdir/bin" \
        "$_ilt_appdir/share/applications" \
        "$_ilt_appdir/share/metainfo" \
        "$_ilt_appdir/share/man/man1" || return 1
    install -m 0755 "$_ilt_bin" "$_ilt_appdir/bin/mdowser" || return 1
    install -m 0644 "${ROOT:?}/packaging/linux/mdowser.desktop" \
        "$_ilt_appdir/share/applications/mdowser.desktop" || return 1
    render_metainfo "$_ilt_appdir/share/metainfo/dev.mdowser.app.metainfo.xml" \
        "$4" || return 1
    for _ilt_sz in 16 32 48 64 128 256 512; do
        _ilt_icodir="$_ilt_appdir/share/icons/hicolor/${_ilt_sz}x${_ilt_sz}/apps"
        install -d "$_ilt_icodir" || return 1
        install -m 0644 "${ROOT}/assets/icons/icon-$_ilt_sz.png" \
            "$_ilt_icodir/mdowser.png" || return 1
    done
    _ilt_scalardir="$_ilt_appdir/share/icons/hicolor/scalable/apps"
    install -d "$_ilt_scalardir" || return 1
    install -m 0644 "${ROOT}/assets/icon.svg" \
        "$_ilt_scalardir/mdowser.svg" || return 1
    # `-n`: no timestamp/name in the gzip header, so identical input yields an
    # identical file across builds. No gzip on the host: ship the plain man
    # page (man reads both).
    if command -v gzip >/dev/null 2>&1; then
        gzip -9nc "${ROOT}/packaging/linux/mdowser.1" \
            > "$_ilt_appdir/share/man/man1/mdowser.1.gz" || return 1
        chmod 0644 "$_ilt_appdir/share/man/man1/mdowser.1.gz"
    else
        install -m 0644 "${ROOT}/packaging/linux/mdowser.1" \
            "$_ilt_appdir/share/man/man1/mdowser.1" || return 1
    fi
}
