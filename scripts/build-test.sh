#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# build-test: local verification pipeline (fast, host only).
# Usage: ./scripts/build-test.sh
# Steps: sh syntax, fmt --check, clippy -D warnings, test, doc, debug build.
# Fails (exit != 0) at the first failing step. Untouched dist/.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Toolchain PATH without machine-specific absolute dirs (see lib.sh).
. "$SCRIPT_DIR/lib.sh"
ensure_toolchain_path

step() { printf '\n==> %s\n' "$1"; }

step "sh syntax"
for f in scripts/*.sh; do sh -n "$f" || exit 1; done

step "fmt --check"
cargo fmt --all --check

step "clippy -D warnings"
cargo clippy --locked --workspace --all-targets -- -D warnings

step "test"
cargo test --locked --workspace

step "doc (deny rustdoc warnings)"
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --document-private-items

step "debug build"
cargo build --locked

echo ""
echo "OK: build-test passed (host debug at target/debug/mdowser)."
