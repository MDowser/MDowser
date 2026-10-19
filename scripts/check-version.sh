#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# check-version.sh: fail when the release version drifts between the places
# a user or a package can read it.
#
# Usage: ./scripts/check-version.sh   (= `make check-version`)
#
# `Cargo.toml` is the single source of truth. This cheap, tool-free gate runs
# as part of `make test`/`scripts/build-test.sh` and `make validate`:
#   - the committed AppStream metainfo advertises the current release,
#   - the RPM spec's newest changelog entry names it,
#   - CHANGELOG.md has a released section for it,
#   - docs/SECURITY.md supports its major.minor line.
# Packaged copies are rendered from `Cargo.toml` at package time (see
# `scripts/packaging.sh`), but the committed files are read directly from the
# tree by Flatpak and by people, so they must not lag behind.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Shared `[package]` version parse (the same one every packager uses).
. "$SCRIPT_DIR/packaging.sh"

VER="$(app_version)"
if [ -z "$VER" ]; then
    echo "ERROR: could not parse the version from Cargo.toml [package]" >&2
    exit 1
fi

FAILED=0
fail() {
    echo "ERROR: $1" >&2
    FAILED=$((FAILED + 1))
}

# Numeric major.minor only (a pre-release suffix stays on the full version).
MINOR="$(printf '%s' "$VER" | cut -d. -f1-2)"

METAINFO="packaging/linux/dev.mdowser.app.metainfo.xml"
INSTALLER_VER="$(sed -n 's/.*<release[^>]*version="\([^"]*\)".*/\1/p' "$METAINFO" | head -1)"
if [ "$INSTALLER_VER" != "$VER" ]; then
    fail "$METAINFO advertises '${INSTALLER_VER:-<none>}', expected '$VER'"
fi

SPEC="packaging/rpm/mdowser.spec"
RPM_VER="$(sed -n 's/^\* .* - \([0-9][^ ]*\)-.*$/\1/p' "$SPEC" | head -1)"
if [ "$RPM_VER" != "$VER" ]; then
    fail "$SPEC's newest changelog entry is '${RPM_VER:-<none>}', expected '$VER'"
fi

CHANGELOG="CHANGELOG.md"
if ! grep -Fq "## [$VER]" "$CHANGELOG"; then
    fail "$CHANGELOG has no '## [$VER]' release section"
fi

SECURITY="docs/SECURITY.md"
if ! grep -Fq "| $MINOR.x" "$SECURITY"; then
    fail "$SECURITY does not list '$MINOR.x' as supported"
fi
if ! grep -Fq "| < $MINOR" "$SECURITY"; then
    fail "$SECURITY does not mark releases before '$MINOR' unsupported"
fi

if [ "$FAILED" -ne 0 ]; then
    echo "FAILED: $FAILED version consistency check(s)." >&2
    exit 1
fi
echo "OK: version $VER is synchronized (Cargo.toml, metainfo, RPM changelog, CHANGELOG.md, SECURITY.md $MINOR.x)."
