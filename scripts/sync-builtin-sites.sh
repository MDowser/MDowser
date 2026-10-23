#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# sync-builtin-sites.sh: sync the curated omnibox site list from the
# MDowser-Sites collaboration repo (via curl) and validate every entry the
# way the browser resolves it.
#
# Usage:
#   ./scripts/sync-builtin-sites.sh                     # download + validate + import every passing entry
#   ./scripts/sync-builtin-sites.sh --check-only [file] # validate one file, no download and no write
#
# A failing entry is DROPPED from the imported file and listed at the end
# with the counts; it never aborts the rest of the list. A sync exits
# non-zero only when nothing could be imported (download failure or every
# entry failing), and then the current assets/builtin_sites.txt stays
# untouched. A `--check-only` run is a strict linter: non-zero when any
# entry fails.
#
# Env:
#   SITES_UPSTREAM         raw file URL (default: MDowser-Sites main builtin_sites.txt)
#   SITES_CONNECT_TIMEOUT  per-candidate connect timeout, seconds (default 10)
#   SITES_MAX_TIME         per-candidate total timeout, seconds (default 25)
#
# Validation mirrors `protocol::resolver` + `protocol::content_type`:
# format (one `<url> | <title>` line, split on the first `|`; `#`/blank
# skipped; `https://` or `file://` in normalized store form: trailing `/`
# for HTTPS roots; no duplicate URLs) plus, for every `https://` entry, the
# browser's candidate cascade (`base`, `base/`, `base.md`, `base/index.md`,
# `base/README.md`; root: `/`, `/index.md`, `/README.md`) requested with
# `Accept: text/markdown;q=0.8`: at least one candidate must answer 2xx with
# a markdown Content-Type. `file://` entries are format-checked only.
#
# The Content-Type gate is a shell approximation of the strict Rust gate
# (every declared value must identify markdown; UTF-8 charset only; quoted
# `;` inside parameters is not handled); the authoritative check stays in
# `src/protocol/content_type.rs`.
#
# A manual maintainer step on purpose: no CI runs this (see MDowser-Sites).
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ASSET="assets/builtin_sites.txt"
UPSTREAM_DEFAULT="https://raw.githubusercontent.com/MDowser/MDowser-Sites/main/builtin_sites.txt"
SITES_UPSTREAM="${SITES_UPSTREAM:-$UPSTREAM_DEFAULT}"
CTO="${SITES_CONNECT_TIMEOUT:-10}"
MTO="${SITES_MAX_TIME:-25}"
UA="MDowser-sites-check/1 (CommonMark; +https)"

usage() {
    echo "usage: $(basename "$0") [--check-only [file]]" >&2
    echo "  (no args)            download \$SITES_UPSTREAM via curl, import the passing entries into $ASSET" >&2
    echo "  --check-only [file]  validate one file only (default $ASSET), no download, no write" >&2
}

trim_str() {
    printf '%s' "$1" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

strip_quotes() {
    printf '%s' "$1" | sed "s/^[\"']*//;s/[\"']*$//"
}

# True when a Content-Type VALUE identifies markdown (case-insensitive MIME
# + optional UTF-8 charset; any other type, a non-UTF-8 charset or a bare
# parameter without `=` rejects). Approximation of
# `protocol::content_type::is_markdown_content_type`.
is_markdown_ct() {
    _ct="$1"
    [ -n "$_ct" ] || return 1
    _lower="$(printf '%s' "$_ct" | tr '[:upper:]' '[:lower:]')" || return 1
    _mime="$(trim_str "$(printf '%s' "$_lower" | cut -d';' -f1)")"
    case "$_mime" in
        text/markdown|text/x-markdown|application/markdown|application/x-markdown) ;;
        *) return 1 ;;
    esac
    _rest="$(printf '%s' "$_lower" | cut -d';' -f2- -s)"
    [ -z "$_rest" ] && return 0
    if printf '%s' "$_rest" | tr ';' '\n' | while IFS= read -r _pp || [ -n "$_pp" ]; do
        _p="$(trim_str "$_pp")"
        if [ -n "$_p" ]; then
            case "$_p" in *=*) ;; *) exit 1 ;; esac
            _nm="$(trim_str "$(printf '%s' "$_p" | cut -d'=' -f1)")"
            if [ "$_nm" = "charset" ]; then
                _vv="$(trim_str "$(strip_quotes "$(printf '%s' "$_p" | cut -d'=' -f2-)")")"
                case "$_vv" in utf-8|utf8) ;; *) exit 1 ;; esac
            fi
        fi
    done; then
        return 0
    else
        return 1
    fi
}

# Walks the browser's candidate cascade for one https URL ($1). On success
# prints the winning candidate and returns 0; on failure returns 1 and
# leaves the last attempt in P_LAST / P_CT for the caller's report.
probe_url() {
    _u="$1"
    _nf="${_u%%#*}"
    case "$_nf" in
        *\?*) _st="${_nf%%\?*}"; _qq="?${_nf#*\?}" ;;
        *) _st="$_nf"; _qq="" ;;
    esac
    _rh="${_st#https://}"
    case "$_rh" in
        */*) _pa="/${_rh#*/}" ;;
        *) _pa="" ;;
    esac
    _or="https://${_rh%%[/?]*}"
    if [ -z "$_pa" ] || [ "$_pa" = "/" ]; then
        _stems="/ /index.md /README.md"
    else
        _b="$_pa"
        while [ -n "$_b" ] && [ "${_b%/}" != "$_b" ]; do _b="${_b%/}"; done
        _stems="$_b $_b/ $_b.md $_b/index.md $_b/README.md"
    fi
    P_LAST="no request made"
    P_CT="-"
    # Word splitting on $_stems is intended: candidates never hold spaces
    # (whitespace in the URL already failed the format check).
    # shellcheck disable=SC2086
    for _s in $_stems; do
        _cand="$_or$_s$_qq"
        _wo="$(curl -sSL --proto '=https' --max-redirs 8 \
            --connect-timeout "$CTO" --max-time "$MTO" --max-filesize 1048576 \
            -o /dev/null -H 'Accept: text/markdown;q=0.8' -A "$UA" \
            -w 'code=%{http_code} type=%{content_type}' "$_cand" 2>/dev/null)"
        _rc=$?
        if [ -z "$_wo" ] && [ "$_rc" -ne 0 ]; then
            P_LAST="$_cand -> curl error $_rc"
            P_CT="-"
            continue
        fi
        _code="${_wo#code=}"
        _code="${_code%% *}"
        _ct="${_wo#*type=}"
        P_LAST="$_cand -> ${_code:-?}"
        P_CT="${_ct:-(missing)}"
        case "$_code" in
            2[0-9][0-9]) ;;
            *) continue ;;
        esac
        if is_markdown_ct "$_ct"; then
            printf 'ok   %s\n       <- %s  [%s; %s]\n' "$_u" "$_cand" "$_code" "$_ct"
            return 0
        fi
    done
    return 1
}

# Records one dropped entry for the end-of-run report ($1 line number,
# $2 the offending text, $3 the reason).
note_fail() {
    FAIL_CNT="$((FAIL_CNT + 1))"
    FAILURES="$FAILURES  - line $1: $2
      reason: $3
"
}

# Validates one list file ($1). When $2 is non-empty, the FILTERED list is
# written there: comment/blank lines are copied verbatim, entry lines only
# when they pass. A failing entry is dropped and reported, never aborting
# the walk. Sets TOTAL/OK_CNT/FAIL_CNT for the caller.
process_file() {
    _f="$1"
    _out="$2"
    if [ ! -f "$_f" ]; then
        echo "ERROR: no such file: $_f" >&2
        return 1
    fi
    TOTAL=0; OK_CNT=0; FAIL_CNT=0; SEEN=""; FAILURES=""; LN=0
    # Newline delimiter for the seen-URL list (URLs never hold one).
    NL="$(printf '\nX')"; NL="${NL%X}"
    if [ -n "$_out" ]; then
        : > "$_out" || return 1
    fi
    while IFS= read -r _line || [ -n "$_line" ]; do
        LN="$((LN + 1))"
        _t="$(trim_str "$_line")"
        case "$_t" in
            ''|\#*)
                [ -n "$_out" ] && printf '%s\n' "$_line" >> "$_out"
                continue
                ;;
        esac
        TOTAL="$((TOTAL + 1))"
        case "$_line" in
            *"|"*) ;;
            *)
                note_fail "$LN" "$_t" 'expected `<url> | <title>`'
                continue
                ;;
        esac
        # Split the RAW line on the first `|`, then trim each side: a
        # `url | (spaces)` line reports an empty title instead of vanishing
        # into the whole-line trim above. Titles may hold further `|`; the
        # URL (before the first one) never does.
        _url="$(trim_str "${_line%%"|"*}")"
        _title="$(trim_str "${_line#*"|"}")"
        if [ -z "$_url" ] || [ -z "$_title" ]; then
            note_fail "$LN" "$_t" 'empty URL or title'
            continue
        fi
        case "$_url" in
            *[[:space:]]*)
                note_fail "$LN" "$_url" 'whitespace in URL'
                continue
                ;;
        esac
        case "$_url" in
            https://*|file://*) ;;
            *)
                note_fail "$LN" "$_url" 'scheme must be https:// or file://'
                continue
                ;;
        esac
        case "$SEEN" in
            *"$NL$_url$NL"*)
                note_fail "$LN" "$_url" 'duplicate URL'
                continue
                ;;
        esac
        SEEN="$SEEN$NL$_url$NL"
        case "$_url" in
            https://*)
                _rh2="${_url#https://}"
                case "$_rh2" in
                    */*) ;;
                    *)
                        note_fail "$LN" "$_url" 'HTTPS root needs trailing `/` (normalized store form)'
                        continue
                        ;;
                esac
                if probe_url "$_url"; then
                    OK_CNT="$((OK_CNT + 1))"
                    [ -n "$_out" ] && printf '%s\n' "$_line" >> "$_out"
                else
                    note_fail "$LN" "$_url" "no candidate answered markdown (last: $P_LAST [type: $P_CT])"
                fi
                ;;
            file://*)
                printf 'ok   %s\n       (local file, no probe)\n' "$_url"
                OK_CNT="$((OK_CNT + 1))"
                [ -n "$_out" ] && printf '%s\n' "$_line" >> "$_out"
                ;;
        esac
    done <"$_f"
    if [ -n "$FAILURES" ]; then
        printf '\nfailures (%d):\n%s' "$FAIL_CNT" "$FAILURES"
    fi
    if [ -n "$_out" ]; then
        printf 'entries: imported %d of %d (%d dropped)\n' "$OK_CNT" "$TOTAL" "$FAIL_CNT"
    else
        printf 'sites: checked %d, ok %d, failed %d (%s)\n' "$TOTAL" "$OK_CNT" "$FAIL_CNT" "$_f"
    fi
}

MODE="sync"
FILE=""
case "${1:-}" in
    '') ;;
    --check-only) MODE="check"; FILE="${2:-$ASSET}" ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
esac

if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl not found (install curl to sync/validate the site list)" >&2
    exit 1
fi

if [ "$MODE" = "check" ]; then
    process_file "$FILE" "" || exit 1
    [ "$FAIL_CNT" -eq 0 ] || exit 1
    exit 0
fi

# Stage next to the asset, not in $TMPDIR: the final `mv` is then a
# same-filesystem rename (atomic), so an interrupted sync can never leave a
# truncated or half-written builtin_sites.txt behind.
ASSET_DIR="$(dirname "$ASSET")"
TMP_LIST="$(mktemp "$ASSET_DIR/.builtin_sites-download.XXXXXX")" || exit 1
# Trap right after the FIRST mktemp: if the second one fails, the first
# file must still be removed.
trap 'rm -f "${TMP_LIST:-}" "${TMP_OUT:-}"' EXIT INT TERM
TMP_OUT="$(mktemp "$ASSET_DIR/.builtin_sites-filtered.XXXXXX")" || exit 1

if ! curl -fsSL --proto '=https' --proto-redir '=https' --max-time 60 \
    -A "$UA" "$SITES_UPSTREAM" -o "$TMP_LIST"; then
    echo "ERROR: download failed: $SITES_UPSTREAM (keeping $ASSET)" >&2
    exit 1
fi
if [ ! -s "$TMP_LIST" ]; then
    echo "ERROR: empty download: $SITES_UPSTREAM (keeping $ASSET)" >&2
    exit 1
fi
process_file "$TMP_LIST" "$TMP_OUT" || exit 1
if [ "$OK_CNT" -eq 0 ]; then
    echo "ERROR: not a single entry imported: $SITES_UPSTREAM (keeping $ASSET)" >&2
    exit 1
fi
# Keep the asset's permission bits: `mv` would install the mktemp file's
# 0600. `stat -f` is BSD/macOS, `stat -c` is GNU. A brand-new asset gets the
# normal 0644 so it is readable by every checkout.
ASSET_MODE="644"
if [ -e "$ASSET" ]; then
    _mode="$(stat -f '%Lp' "$ASSET" 2>/dev/null || stat -c '%a' "$ASSET" 2>/dev/null || true)"
    [ -n "$_mode" ] && ASSET_MODE="$_mode"
    unset _mode
fi
if ! mv "$TMP_OUT" "$ASSET"; then
    echo "ERROR: could not replace $ASSET (keeping the previous file)" >&2
    exit 1
fi
chmod "$ASSET_MODE" "$ASSET" 2>/dev/null || true
echo "synced $ASSET from $SITES_UPSTREAM"
