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

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Toolchain PATH without machine-specific absolute dirs (see lib.sh); the
# Flatpak SDK provides cargo inside the sandbox, but the generator may want
# the host toolchain. Shared helpers give the version and the file layout.
. "$(dirname "$0")/lib.sh"
ensure_toolchain_path
. "$(dirname "$0")/packaging.sh"

SKIP_SETUP="${SKIP_SETUP:-0}"
APP_ID="dev.mdowser.app"
VER="$(app_version)"
if [ -z "$VER" ]; then
    echo "ERROR: could not parse version from Cargo.toml" >&2
    exit 1
fi

MANIFEST="$ROOT/packaging/flatpak/dev.mdowser.app.yaml"
SOURCES="$ROOT/packaging/flatpak/cargo-sources.json"
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
    echo "  org.freedesktop.Platform / org.freedesktop.Sdk (24.08) runtimes" >&2
    echo "  and the org.freedesktop.Sdk.Extension.rust-stable extension, then" >&2
    echo '  re-run `make flatpak`.' >&2
    exit 1
fi

# Best-effort runtimes: add flathub and pull the runtime + SDK + rust
# extension when they are missing. Honors SKIP_SETUP; with no network or no
# remote the build simply fails with flatpak-builder's own message.
ensure_runtimes() {
    if [ "$SKIP_SETUP" = "1" ]; then
        return 0
    fi
    if ! command -v flatpak >/dev/null 2>&1; then
        return 0
    fi
    if flatpak info org.freedesktop.Sdk//24.08 >/dev/null 2>&1; then
        return 0
    fi
    printf '==> installing freedesktop runtimes (flathub)\n'
    flatpak remote-add --if-not-exists --user flathub \
        https://flathub.org/repo/flathub.flatpakrepo >/dev/null 2>&1 || true
    flatpak install -y --noninteractive --user flathub \
        org.freedesktop.Platform//24.08 \
        org.freedesktop.Sdk//24.08 \
        org.freedesktop.Sdk.Extension.rust-stable//24.08 || true
}
ensure_runtimes

# Offline build: the vendored crate sources must exist before flatpak-builder
# runs, or the manifest's `cargo build --offline` cannot resolve a crate.
if [ ! -f "$SOURCES" ]; then
    if command -v flatpak-cargo-generator.py >/dev/null 2>&1; then
        flatpak-cargo-generator.py "$ROOT/Cargo.lock" -o "$SOURCES"
    elif command -v flatpak-cargo-generator >/dev/null 2>&1; then
        flatpak-cargo-generator "$ROOT/Cargo.lock" -o "$SOURCES"
    else
        echo "ERROR: $SOURCES is missing and flatpak-cargo-generator was not found." >&2
        echo "  Get it from https://github.com/flatpak/flatpak-builder-tools, then:" >&2
        echo "    python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json" >&2
        exit 1
    fi
fi

# Build into a local repo and export a bundle: the DISTRIBUTABLE artifact is
# the .flatpak file, not an installed app.
mkdir -p "$ROOT/dist"
rm -rf "$REPO" "$BUILD_DIR" "$STATE_DIR"
printf '==> flatpak-builder %s\n' "$MANIFEST"
flatpak-builder --repo="$REPO" --state-dir="$STATE_DIR" --force-clean "$BUILD_DIR" "$MANIFEST"
printf '==> bundling %s\n' "$OUT"
flatpak build-bundle "$REPO" "$OUT" "$APP_ID"
rm -rf "$REPO" "$BUILD_DIR" "$STATE_DIR"

printf '\nOK: %s\n' "$OUT"
printf 'Install it with: flatpak install %s\n' "$OUT"
