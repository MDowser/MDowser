#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# validate.sh: best-effort validation of the packaging metadata.
#
# Usage: ./scripts/validate.sh   (= `make validate`)
#
# There are NO hard dependencies: a tool that is not installed is a SKIP with
# its install hint, never a failure. The exit status is non-zero only when a
# tool that IS installed reports an error.
#
# Checks:
#   - version consistency: Cargo.toml vs metainfo, RPM changelog,
#     CHANGELOG.md and SECURITY.md.
#   - dev.mdowser.app.metainfo.xml: XML well-formedness of the committed file
#     AND of the version-rendered copy the packages install (catches a broken
#     render), then appstreamcli (or appstream-util) validation.
#   - mdowser.desktop: desktop-file-validate.
#   - mdowser.spec: `rpmspec -P` (macro expansion = syntax check; a real RPM
#     needs a Linux/rpmbuild host) and rpmlint when installed.
#   - dev.mdowser.app.yaml: YAML parse when python3 has PyYAML.
#
# What this CANNOT check off-Linux: lintian (Debian-only), a real .deb/.rpm
# build and flatpak-builder. See docs/BUILDING.md for the container recipes.
# `set -u` (not `set -eu`) on purpose: best-effort means a failing probe must
# fall through to the SKIP path, never abort the script.
set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Shared version parse + metainfo renderer (single source of truth).
. "$SCRIPT_DIR/packaging.sh"

VER="$(app_version)"
if [ -z "$VER" ]; then
    echo "ERROR: could not parse version from Cargo.toml" >&2
    exit 1
fi

METAINFO="packaging/linux/dev.mdowser.app.metainfo.xml"
DESKTOP="packaging/linux/mdowser.desktop"
SPEC="packaging/rpm/mdowser.spec"
FLATPAK="packaging/flatpak/dev.mdowser.app.yaml"

FAILED=0

# Temp files from `mktemp` below are removed on any exit (a killed run must
# not leave them in `$TMPDIR`); the explicit `rm -f` calls stay as well.
cleanup_validate_tmp() {
    rm -f "${TMP_META:-}" "${TMP_OUT:-}" 2>/dev/null || true
}
trap cleanup_validate_tmp EXIT

have() { command -v "$1" >/dev/null 2>&1; }
step() { printf '\n==> %s\n' "$1"; }
ok() { printf '  ok    %s\n' "$1"; }
skip() { printf '  SKIP  %s (%s)\n' "$1" "$2"; }
bad() {
    printf '  FAIL  %s\n' "$1"
    FAILED=$((FAILED + 1))
}

# --- version consistency -----------------------------------------------------
# The cheap, tool-free gate shared with `make test`: Cargo.toml owns the
# version, and the committed metainfo, RPM changelog, CHANGELOG.md and
# security policy must name the same release.
step "version consistency"
if "$ROOT/scripts/check-version.sh"; then
    ok "Cargo.toml, metainfo, RPM changelog, CHANGELOG.md and SECURITY.md agree"
else
    bad "version consistency (see errors above)"
fi

# --- XML well-formedness (committed + version-rendered) ----------------------
step "XML well-formedness"
if have python3; then
    TMP_META="$(mktemp "${TMPDIR:-/tmp}/mdowser-metainfo.XXXXXX" 2>/dev/null)" || TMP_META=""
    if [ -z "$TMP_META" ]; then
        skip "rendered metainfo" "mktemp unavailable"
    elif render_metainfo "$TMP_META" "$VER"; then
        for f in "$METAINFO" "$TMP_META"; do
            if python3 -c 'import sys, xml.etree.ElementTree as ET; ET.parse(sys.argv[1])' "$f" 2>/dev/null; then
                ok "$f"
            else
                bad "$f (not well-formed XML)"
            fi
        done
        rm -f "$TMP_META"
    else
        bad "render_metainfo -> $TMP_META"
        rm -f "$TMP_META"
    fi
    unset TMP_META
else
    skip "XML well-formedness" "python3 not found"
fi

# --- AppStream ---------------------------------------------------------------
step "AppStream metadata"
if have appstreamcli; then
    # `appstreamcli validate` returns non-zero for WARNINGS too, so inspect its
    # own summary: only a non-zero "errors" count is a failure (the RPM %check
    # runs the same `appstreamcli validate --no-net` and tolerates warnings
    # with `|| :` for the same reason).
    TMP_OUT="$(mktemp "${TMPDIR:-/tmp}/mdowser-appstream.XXXXXX" 2>/dev/null)" || TMP_OUT=""
    if [ -z "$TMP_OUT" ]; then
        if appstreamcli validate --no-net "$METAINFO"; then
            ok "appstreamcli"
        else
            bad "appstreamcli validate $METAINFO"
        fi
    else
        if appstreamcli validate --no-net "$METAINFO" >"$TMP_OUT" 2>&1; then
            ok "appstreamcli"
        elif grep -q 'Validation was successful' "$TMP_OUT" \
            && ! grep -qE 'errors: [1-9]' "$TMP_OUT"; then
            # Non-zero exit with a successful summary = warnings/infos only
            # (appstreamcli lists non-zero counts only, so the old
            # `errors: 0` match never fired and warnings were hard FAILs).
            printf '  note  appstreamcli: warnings only (non-fatal)\n'
            sed 's/^/        /' "$TMP_OUT"
            ok "appstreamcli (warnings)"
        else
            sed 's/^/        /' "$TMP_OUT"
            bad "appstreamcli validate $METAINFO"
        fi
        rm -f "$TMP_OUT"
    fi
    unset TMP_OUT
elif have appstream-util; then
    # Legacy validator fallback (when appstreamcli is not installed). It flags
    # the committed `<icon type="stock">` line ("stock icon is not valid") — a
    # false positive that the current appstreamcli accepts; only THAT complaint is
    # downgraded to a note, any other finding still fails. The filter is
    # fail-closed on plain phrases (never the localized problem bullet): a
    # line the whitelist does not recognize stays a FAIL.
    _as_out="$(appstream-util validate-relax --nonet "$METAINFO" 2>&1)"
    _as_rc=$?
    _as_other="$(printf '%s\n' "$_as_out" \
        | grep -v 'stock icon is not valid' \
        | grep -v 'Validation of files failed' \
        | grep -v ': FAILED:' \
        | grep -v '^[[:space:]]*$' || true)"
    if [ "$_as_rc" -eq 0 ]; then
        ok "appstream-util"
    elif printf '%s\n' "$_as_out" | grep -q 'stock icon is not valid' \
        && [ -z "$_as_other" ]; then
        printf '  note  appstream-util: only the legacy stock-icon hint (non-fatal)\n'
        ok "appstream-util (legacy stock-icon hint)"
    else
        printf '%s\n' "$_as_out" | sed 's/^/        /'
        bad "appstream-util validate-relax $METAINFO"
    fi
    unset _as_out _as_rc _as_other
else
    skip "AppStream metadata" "install appstreamcli (brew install appstream) or appstream-util (brew install appstream-glib)"
fi

# --- Desktop entry -----------------------------------------------------------
step "desktop entry"
if have desktop-file-validate; then
    if desktop-file-validate "$DESKTOP"; then
        ok "desktop-file-validate"
    else
        bad "desktop-file-validate $DESKTOP"
    fi
else
    skip "desktop entry" "install desktop-file-validate (brew install desktop-file-utils)"
fi

# --- RPM spec ----------------------------------------------------------------
step "RPM spec"
if have rpmspec; then
    # `-P` only parses/expands macros: a portable SYNTAX check (a real build
    # needs rpmbuild on Linux, see docs/BUILDING.md).
    if rpmspec -P "$SPEC" --define "ver $VER" >/dev/null 2>&1; then
        ok "rpmspec -P (ver=$VER)"
    else
        bad "rpmspec -P $SPEC"
    fi
else
    skip "rpmspec" "install rpm (brew install rpm, or dnf install rpm-build)"
fi
if have rpmlint; then
    if rpmlint "$SPEC"; then
        ok "rpmlint"
    else
        bad "rpmlint $SPEC (distro-specific warnings/errors; check with a real package)"
    fi
else
    skip "rpmlint" "pipx install rpmlint (needs the rpm Python bindings)"
fi

# --- Flatpak manifest --------------------------------------------------------
step "Flatpak manifest"
if have python3 && python3 -c 'import yaml' >/dev/null 2>&1; then
    if python3 -c 'import sys, yaml; yaml.safe_load(open(sys.argv[1]))' "$FLATPAK"; then
        ok "YAML: $FLATPAK"
    else
        bad "YAML: $FLATPAK"
    fi
else
    skip "Flatpak manifest" "PyYAML not available (pip install pyyaml)"
fi

# --- dist artifacts (privacy) ------------------------------------------------
# A built dist/ must not carry the builder's absolute paths or build-host
# metadata; the check is best-effort only in that dist/ need not exist yet.
# When the inspector itself cannot run (no python3), the result is a SKIP
# with its reason, never a green `ok`.
step "dist artifacts (privacy)"
if [ -d "$ROOT/dist" ] && [ -n "$(ls -A "$ROOT/dist" 2>/dev/null)" ]; then
    _cd_out="$("$ROOT/scripts/check-dist.sh" "$ROOT/dist" 2>&1)"
    _cd_status=$?
    printf '%s\n' "$_cd_out"
    if [ "$_cd_status" -ne 0 ]; then
        bad "check-dist found build-host information in dist/"
    elif printf '%s\n' "$_cd_out" | grep -q '^SKIP'; then
        skip "dist artifacts (privacy)" "the inspector could not run (see above)"
    else
        ok "check-dist (no builder paths / build-host metadata)"
    fi
    unset _cd_out _cd_status
else
    skip "dist artifacts (privacy)" "no dist/ yet (run make dist)"
fi

# --- summary -----------------------------------------------------------------
echo ""
if [ "$FAILED" -eq 0 ]; then
    echo "OK: packaging validation passed (see the SKIP lines for what is not installed)."
    exit 0
fi
echo "FAILED: $FAILED packaging check(s) reported errors." >&2
exit 1
