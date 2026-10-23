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

# Semver base `X.Y.Z`: drops a `-prerelease` suffix and `+build` metadata.
# Apple's CFBundleVersion/CFBundleShortVersionString only accept numeric
# components, so the .app uses this while the artifact filename keeps the
# full version.
version_base() {
    printf '%s' "$1" | sed 's/[-+].*$//'
}

# Debian version: `-` is dpkg's revision separator, so a semver pre-release
# (`0.7.0-alpha.1`) becomes `0.7.0~alpha.1`. `~` also sorts BEFORE the final
# `0.7.0` release, which is what a pre-release must do; a literal `-` would
# sort after it. Semver `+build` metadata does not participate in precedence,
# so it is dropped instead of changing the package order.
version_deb() {
    printf '%s' "$1" | sed -e 's/+.*$//' -e 's/-/~/g'
}

# RPM version: the `Version:` tag rejects `-` outright. `~` is RPM's own
# pre-release marker (sorts first); `+build` metadata is dropped for the same
# precedence reason as in `version_deb`.
version_rpm() {
    printf '%s' "$1" | sed -e 's/+.*$//' -e 's/-/~/g'
}

# Renders `packaging/linux/dev.mdowser.app.metainfo.xml` to $1 with the
# NEWEST <release> version pinned to $2 and its date to today (UTC), or to the
# date of `SOURCE_DATE_EPOCH` when that is set (reproducible builds). The
# committed file keeps its own value so it stays valid standalone (Flatpak,
# appstream tooling read it from the tree); every PACKAGED copy matches the
# binary it ships with.
#
# Only the FIRST <release> entry (the current one, above the history) is
# rewritten: a sed over every matching line used to relabel the whole release
# history with the current version and date.
#
# NOTE: POSIX sh functions share one global scope. Every variable here is
# uniquely named and NOT `unset` at the end: an `unset _root` would delete a
# caller's variable of the same name (the reason this comment exists).
render_metainfo() {
    _metainfo_out="$1"
    _metainfo_ver="$2"
    _metainfo_date=""
    if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
        case "$SOURCE_DATE_EPOCH" in
            *[!0-9]*)
                echo "ERROR: SOURCE_DATE_EPOCH must be a Unix timestamp, not '$SOURCE_DATE_EPOCH'" >&2
                return 1
                ;;
        esac
        # GNU date takes `-d @<epoch>`, BSD date takes `-r <epoch>`. Try GNU
        # first: `-r` on GNU treats its argument as a FILE name, which could
        # silently use a same-named file's mtime.
        _metainfo_date="$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%d 2>/dev/null \
            || date -u -r "$SOURCE_DATE_EPOCH" +%Y-%m-%d 2>/dev/null \
            || true)"
        if [ -z "$_metainfo_date" ]; then
            echo "ERROR: SOURCE_DATE_EPOCH '$SOURCE_DATE_EPOCH' is not a representable date" >&2
            return 1
        fi
    else
        _metainfo_date="$(date -u +%Y-%m-%d 2>/dev/null || printf '1970-01-01')"
    fi
    # Fail closed on an unexpected version: it is interpolated into the XML
    # attribute (the allowed set excludes quotes, `&`, `\` and `/`).
    case "$_metainfo_ver" in
        '' | *[!0-9A-Za-z._+-]*)
            echo "ERROR: refusing to render metainfo with version '$_metainfo_ver'" >&2
            return 1
            ;;
    esac
    awk -v ver="$_metainfo_ver" -v date="$_metainfo_date" '
        !done && /^[[:space:]]*<release / {
            sub(/version="[^"]*"/, "version=\"" ver "\"")
            sub(/date="[^"]*"/, "date=\"" date "\"")
            done = 1
        }
        { print }
    ' "${ROOT:?ROOT must be set}/packaging/linux/dev.mdowser.app.metainfo.xml" \
        > "$_metainfo_out" || return 1
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
    # Pre-flight (fail closed before a partial tree): every source the tree
    # copies must exist, so a missing icon or man page aborts before the
    # destination is touched instead of leaving a half-installed prefix
    # (fatal for the RPM `%install`).
    if [ ! -f "${ROOT:?}/packaging/linux/mdowser.desktop" ]; then
        echo "ERROR: packaging/linux/mdowser.desktop not found" >&2
        return 1
    fi
    if [ ! -f "${ROOT}/packaging/linux/dev.mdowser.app.metainfo.xml" ]; then
        echo "ERROR: packaging/linux/dev.mdowser.app.metainfo.xml not found" >&2
        return 1
    fi
    if [ ! -f "${ROOT}/packaging/linux/mdowser.1" ]; then
        echo "ERROR: packaging/linux/mdowser.1 not found" >&2
        return 1
    fi
    if [ ! -f "${ROOT}/assets/icon.svg" ]; then
        echo "ERROR: assets/icon.svg not found" >&2
        return 1
    fi
    for _ilt_pre in 16 32 48 64 128 256 512; do
        if [ ! -f "${ROOT}/assets/icons/icon-$_ilt_pre.png" ]; then
            echo "ERROR: assets/icons/icon-$_ilt_pre.png not found (run make icons)" >&2
            return 1
        fi
    done
    unset _ilt_pre
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
