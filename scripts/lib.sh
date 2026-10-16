#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# lib.sh: shared toolchain bootstrap for scripts/ (POSIX sh, no deps).
# Sourced as: . "$(dirname "$0")/lib.sh"  (path relative to the script itself,
# so the repo moves between machines cleanly).
#
# Goal: make `cargo` (and `rustup` when needed) reachable WITHOUT hardcoding
# machine-specific absolute dirs like /opt/homebrew or /usr/local.
# Strategy, in order:
#   1. rustup's standard env file (portable, lives in $HOME).
#   2. ~/.cargo/bin (standard rustup location).
#   3. `rustup which cargo` (rustup knows its own toolchains).
#   4. `brew --prefix` + /opt/rustup/bin (brew layouts, prefix discovered
#      at runtime, never hardcoded).
#   5. ~/.rustup/toolchains/*/bin (direct glob, HOME-relative).

# Adds toolchain dirs to PATH. Idempotent and quiet on failure (callers
# decide with `command -v cargo` whether to proceed or skip with a reason).
ensure_toolchain_path() {
    # 0. OS-conventional tool homes (identical on every Mac/Linux box, NOT
    #    machine-specific paths): only consulted when the tool is missing.
    #    Covers non-interactive shells whose profiles never added them.
    for _bd in /opt/homebrew/bin /usr/local/bin; do
        case ":$PATH:" in
            *"$_bd"*) ;;
            *) [ -d "$_bd" ] && export PATH="$_bd:$PATH" ;;
        esac
    done
    unset _bd
    # 1. Standard rustup env file when present (`HOME` guarded: callers like
    #    `check-dist` run under `set -u`, where an unset `HOME` aborts).
    if [ -n "${HOME:-}" ] && [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
    # 2. Standard location (`HOME` guarded like step 1: callers run under
    #    `set -u`).
    if [ -n "${HOME:-}" ]; then
        case ":$PATH:" in
            *":$HOME/.cargo/bin:"*) ;;
            *) export PATH="$HOME/.cargo/bin:$PATH" ;;
        esac
    fi
    # 3. Ask rustup itself (covers layouts whose shims are elsewhere).
    if ! command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
        _RC="$(rustup which cargo 2>/dev/null)" || _RC=""
        if [ -n "$_RC" ]; then
            export PATH="$(dirname "$_RC"):$PATH"
        fi
        unset _RC
    fi
    # 4. Brew layout, prefix discovered at runtime (macOS Intel/ARM differ).
    # Adds both the rustup shims AND brew's own bin (the real `rustup`
    # binary lives there, not next to the shims).
    if command -v brew >/dev/null 2>&1; then
        _BP="$(brew --prefix 2>/dev/null)" || _BP=""
        if [ -n "$_BP" ]; then
            [ -d "$_BP/opt/rustup/bin" ] && case ":$PATH:" in
                *"$_BP/opt/rustup/bin"*) ;;
                *) export PATH="$_BP/opt/rustup/bin:$PATH" ;;
            esac
            case ":$PATH:" in
                *"$_BP/bin"*) ;;
                *) export PATH="$_BP/bin:$PATH" ;;
            esac
        fi
        unset _BP
    fi
    # 5. Direct toolchain glob, HOME-relative (no guessing the triple);
    #    `HOME` guarded like the steps above.
    if ! command -v cargo >/dev/null 2>&1 && [ -n "${HOME:-}" ]; then
        for _TC in "$HOME"/.rustup/toolchains/*/bin/cargo; do
            if [ -x "$_TC" ]; then
                export PATH="$(dirname "$_TC"):$PATH"
                break
            fi
        done
        unset _TC
    fi
    # cargo executes its sibling `rustc` on every build: lead PATH with
    # cargo's own directory, or `could not execute process rustc -vV`
    # strikes on bare PATHs. Derived, never hardcoded.
    if command -v cargo >/dev/null 2>&1; then
        _CD="$(dirname "$(command -v cargo)")"
        case ":$PATH:" in
            *"$_CD"*) ;;
            *) export PATH="$_CD:$PATH" ;;
        esac
        unset _CD
    fi
}

# --- build-artifact privacy --------------------------------------------------
#
# Rust bakes the source path of every panic location / `file!()` into the
# binary, including dependency sources under the Cargo registry. On a release
# machine those are absolute paths under the builder's home
# (`~/.cargo/registry/src/...`), so even a `strip`ped binary ships the
# builder's username and home layout; `strip` cannot remove them (they are
# runtime data, not symbols). The only fix is to remap the prefixes at compile
# time. Call `export_remap_rustflags` once before a DISTRIBUTABLE
# `cargo build`/`cargo zigbuild`; debug builds should keep their real paths so
# debuggers can still resolve sources.
#
# CARGO_ENCODED_RUSTFLAGS (0x1f-separated, preferred by cargo over RUSTFLAGS)
# is used so a `$HOME` containing spaces survives. An existing RUSTFLAGS is
# preserved (cargo itself splits it on whitespace). Idempotent.
export_remap_rustflags() {
    [ -n "${_MDOWSER_REMAP_DONE:-}" ] && return 0
    _MDOWSER_REMAP_DONE=1
    _r_sep="$(printf '\037')"
    _r_enc="${CARGO_ENCODED_RUSTFLAGS:-}"
    if [ -z "$_r_enc" ] && [ -n "${RUSTFLAGS:-}" ]; then
        for _r_f in $RUSTFLAGS; do
            _r_enc="${_r_enc:+$_r_enc$_r_sep}$_r_f"
        done
    fi
    if [ -n "${HOME:-}" ]; then
        _r_enc="${_r_enc:+$_r_enc$_r_sep}--remap-path-prefix=$HOME=/build"
        _r_cargo="${CARGO_HOME:-$HOME/.cargo}"
        [ "$_r_cargo" = "$HOME" ] ||
            _r_enc="$_r_enc$_r_sep--remap-path-prefix=$_r_cargo=/cargo"
    fi
    # Windows: rustc emits the USERPROFILE form (`C:\Users\...`), which the
    # MSYS-style $HOME above never matches (Git Bash HOME is `/c/Users/...`).
    # Mirror windows.ps1 and remap both slash styles, so a Windows build
    # driven from Git Bash (`--host-only` in CI included) stays clean.
    if [ -n "${USERPROFILE:-}" ]; then
        for _r_up in "$USERPROFILE" "$(printf '%s' "$USERPROFILE" | tr '\\' '/')"; do
            _r_enc="$_r_enc$_r_sep--remap-path-prefix=$_r_up=/build"
            _r_enc="$_r_enc$_r_sep--remap-path-prefix=$_r_up/.cargo=/cargo"
        done
        unset _r_up
    fi
    if [ -n "$_r_enc" ]; then
        CARGO_ENCODED_RUSTFLAGS="$_r_enc"
        export CARGO_ENCODED_RUSTFLAGS
    fi
    unset _r_sep _r_enc _r_f _r_cargo
}

# Removes extended attributes from a packaging stage tree. bsdtar (macOS)
# archives xattrs by default as PAX records (`LIBARCHIVE.xattr...` /
# `SCHILY.xattr...`), which would embed the builder's `com.apple.provenance`
# blob into Linux `.tar.gz`/`.deb` payloads. `xattr -c` is macOS-only; on
# Linux the tree carries no such attributes, so this is a no-op. Best-effort:
# never fails the caller.
sweep_xattrs() {
    command -v xattr >/dev/null 2>&1 || return 0
    xattr -cr "$1" 2>/dev/null || true
}
