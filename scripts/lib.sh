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
    # 1. Standard rustup env file when present.
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
    # 2. Standard location.
    case ":$PATH:" in
        *":$HOME/.cargo/bin:"*) ;;
        *) export PATH="$HOME/.cargo/bin:$PATH" ;;
    esac
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
    # 5. Direct toolchain glob, HOME-relative (no guessing the triple).
    if ! command -v cargo >/dev/null 2>&1; then
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
