#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# build-flatpak.sh: Flatpak build wrapper (manifest in packaging/flatpak/).
#
# Usage: ./scripts/build-flatpak.sh   (= `make flatpak`, part of `make dist`)
#
# Produces ONE distributable bundle for every Linux distribution:
#
#   dist/mdowser-<ver>.flatpak
#
# taken straight from the source tree (install it with
# `flatpak install mdowser-<ver>.flatpak`). A Flatpak build is OFFLINE, so
# cargo's dependencies must be vendored first
# (`flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json`, from
# https://github.com/flatpak/flatpak-builder-tools). Needs `flatpak-builder`
# plus the freedesktop runtime, SDK and rust-stable extension.
#
# Variables:
#   SKIP_SETUP=1  # do not install the runtimes (best-effort) before building.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Toolchain PATH without machine-specific absolute dirs (see lib.sh); the
# Flatpak SDK provides cargo inside the sandbox, but the generator may want
# the host toolchain. Shared helpers give the version and the file layout.
. "$SCRIPT_DIR/lib.sh"
ensure_toolchain_path
. "$SCRIPT_DIR/packaging.sh"

SKIP_SETUP="${SKIP_SETUP:-0}"
APP_ID="dev.mdowser.app"
VER="$(app_version)"
if [ -z "$VER" ]; then
    echo "ERROR: could not parse version from Cargo.toml" >&2
    exit 1
fi

MANIFEST="$ROOT/packaging/flatpak/dev.mdowser.app.yaml"
SOURCES="$ROOT/packaging/flatpak/cargo-sources.json"
# Runtime branch: single source of truth is the manifest's `runtime-version`
# (the `rust-stable` sandbox extension resolves against it automatically). A
# runtime bump is a one-line manifest change; `flatpak info`/`flatpak
# install` here and the error hints below all follow it.
FDO_VER="$(sed -n "s/^runtime-version:[[:space:]]*'\([0-9.]*\)'.*$/\1/p" "$MANIFEST")"
if [ -z "$FDO_VER" ]; then
    echo "ERROR: could not parse runtime-version from $MANIFEST" >&2
    exit 1
fi
# Build and state dirs live under dist/ (swept at the end): a build dir
# INSIDE the source path makes flatpak-builder recurse into its own state on
# some versions, and a root `.flatpak-builder/` left multi-GB scratch behind.
BUILD_DIR="$ROOT/dist/.flatpak-build"
STATE_DIR="$ROOT/dist/.flatpak-state"
REPO="$ROOT/dist/.flatpak-repo"
OUT="$ROOT/dist/mdowser-$VER.flatpak"
if ! command -v flatpak-builder >/dev/null 2>&1; then
    echo "ERROR: flatpak-builder not found." >&2
    echo "  Install the Flatpak tooling (flatpak + flatpak-builder) and the" >&2
    echo "  org.freedesktop.Platform / org.freedesktop.Sdk ($FDO_VER) runtimes" >&2
    echo "  and the org.freedesktop.Sdk.Extension.rust-stable extension, then" >&2
    echo '  re-run `make flatpak`.' >&2
    exit 1
fi
# `flatpak` (not just the builder) runs `info` and `build-bundle` below;
# without it the build used to fail late with a less helpful message.
if ! command -v flatpak >/dev/null 2>&1; then
    echo "ERROR: flatpak not found (flatpak-builder alone cannot query runtimes or bundle)." >&2
    exit 1
fi

# Runtimes: add flathub and pull the runtime + SDK + rust extension when any
# is missing. Honors SKIP_SETUP, but then verifies instead of assuming: a
# missing runtime must fail HERE with the exact refs, not deep inside
# flatpak-builder.
runtime_refs() {
    printf '%s\n' \
        "org.freedesktop.Platform//$FDO_VER" \
        "org.freedesktop.Sdk//$FDO_VER" \
        "org.freedesktop.Sdk.Extension.rust-stable//$FDO_VER"
}
missing_runtimes() {
    _missing=""
    for _ref in $(runtime_refs); do
        if ! flatpak info "$_ref" >/dev/null 2>&1; then
            _missing="$_missing $_ref"
        fi
    done
    printf '%s' "$_missing"
    unset _missing _ref
}
ensure_runtimes() {
    _missing="$(missing_runtimes)"
    if [ -z "$_missing" ]; then
        return 0
    fi
    if [ "$SKIP_SETUP" = "1" ]; then
        echo "ERROR: missing Flatpak runtime(s):$_missing" >&2
        echo "  Install them (or re-run without SKIP_SETUP=1), then retry." >&2
        unset _missing
        return 1
    fi
    printf '==> installing freedesktop runtimes (flathub)\n'
    flatpak remote-add --if-not-exists --user flathub \
        https://flathub.org/repo/flathub.flatpakrepo >/dev/null 2>&1 || true
    # shellcheck disable=SC2046: one ref per line, no spaces inside a ref.
    flatpak install -y --noninteractive --user flathub $(runtime_refs) || true
    _missing="$(missing_runtimes)"
    if [ -n "$_missing" ]; then
        echo "ERROR: Flatpak runtime install incomplete:$_missing" >&2
        unset _missing
        return 1
    fi
    unset _missing
}
ensure_runtimes || exit 1

# Offline build: the vendored crate sources must exist before flatpak-builder
# runs, or the manifest's `cargo build --offline` cannot resolve a crate.
# Missing is not the only stale case: a `Cargo.lock` newer than the committed
# JSON (a dependency bump) would otherwise pin the PREVIOUS dependency set,
# which fails offline or ships the wrong sources. Regenerate on either.
_regen_sources=0
if [ ! -f "$SOURCES" ]; then
    _regen_sources=1
elif [ -n "$(find "$ROOT/Cargo.lock" -newer "$SOURCES" -print 2>/dev/null | head -1)" ]; then
    printf '==> Cargo.lock is newer than cargo-sources.json: regenerating\n'
    _regen_sources=1
fi
if [ "$_regen_sources" = "1" ]; then
    if command -v flatpak-cargo-generator.py >/dev/null 2>&1; then
        flatpak-cargo-generator.py "$ROOT/Cargo.lock" -o "$SOURCES"
    elif command -v flatpak-cargo-generator >/dev/null 2>&1; then
        flatpak-cargo-generator "$ROOT/Cargo.lock" -o "$SOURCES"
    else
        echo "ERROR: $SOURCES is missing or stale and flatpak-cargo-generator was not found." >&2
        echo "  Get it from https://github.com/flatpak/flatpak-builder-tools, then:" >&2
        echo "    python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json" >&2
        exit 1
    fi
    if [ ! -s "$SOURCES" ]; then
        echo "ERROR: flatpak-cargo-generator produced no $SOURCES" >&2
        exit 1
    fi
fi
unset _regen_sources

# A failed build must leave neither multi-GB scratch nor an older/partial
# bundle at the output path that could be mistaken for this run's artifact.
# Installed only once the real build starts: a missing prerequisite above
# must not delete a previously valid bundle. The flag is set after the
# artifact is verified below.
_flatpak_ok=0
cleanup_flatpak() {
    rm -rf "$REPO" "$BUILD_DIR" "$STATE_DIR" 2>/dev/null || true
    if [ "${_flatpak_ok:-0}" != "1" ]; then
        rm -f "$OUT" 2>/dev/null || true
    fi
}
trap cleanup_flatpak EXIT

# Build into a local repo and export a bundle: the DISTRIBUTABLE artifact is
# the .flatpak file, not an installed app.
mkdir -p "$ROOT/dist"
rm -rf "$REPO" "$BUILD_DIR" "$STATE_DIR"
# A previous bundle at the same path must never survive a failed/partial run
# and be reported as this build's output.
rm -f "$OUT"
printf '==> flatpak-builder %s\n' "$MANIFEST"
flatpak-builder --repo="$REPO" --state-dir="$STATE_DIR" --force-clean "$BUILD_DIR" "$MANIFEST"
printf '==> bundling %s\n' "$OUT"
flatpak build-bundle "$REPO" "$OUT" "$APP_ID"
rm -rf "$REPO" "$BUILD_DIR" "$STATE_DIR"
if [ ! -s "$OUT" ]; then
    echo "ERROR: flatpak build-bundle produced no $OUT" >&2
    exit 1
fi
_flatpak_ok=1

printf '\nOK: %s\n' "$OUT"
printf 'Install it with: flatpak install %s\n' "$OUT"
