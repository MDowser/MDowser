#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# check-dist.sh: release gate for the packaged artifacts in dist/.
#
# Usage: ./scripts/check-dist.sh [--require] [--allow-mixed] [--final] [dist-dir]
#        (= `make check-dist`, which uses --require --final)
#
#   --require      fail (exit 2) when the inspector cannot run (no python3 or
#                  no dist/) instead of skipping: `scripts/build-dist.sh` and
#                  CI use it, where a silent SKIP would report a leaking build
#                  as ready.
#   --allow-mixed  additive single-group builds (`--mac`, `--linux`, ...) may
#                  intentionally leave artifacts from an earlier version in
#                  dist/. Skip the version and update-marker comparisons; the
#                  privacy scan and checksum verification still run.
#   --final        require the finished release metadata: a SHA256SUMS.txt that
#                  covers every artifact, and latest.txt for a stable version
#                  (or no latest.txt for a pre-release). Implies --require and
#                  cannot be combined with --allow-mixed.
#
# Two independent gates live here:
#
# 1. Privacy. A release artifact must not carry the BUILDER's identity. Two
#    known channels, both checked:
#    - payload bytes: release binaries embed the absolute source path of every
#      panic location / `file!()`, including Cargo-registry dependencies
#      (`$HOME/.cargo/registry/src/...`), so the builder's username and home
#      layout travel in every binary (`strip` cannot remove them).
#    - archive metadata: Info-ZIP stores the Unix uid/gid extra field
#      (`0x7875`); bsdtar stores extended attributes as PAX records
#      (`LIBARCHIVE.xattr...`/`SCHILY.xattr...`) and could record a non-zero
#      owner. All three identify the build host.
#    The build host's own nodename is checked too (RPM's `BUILDHOST` tag).
#
# 2. Release integrity, so a stale or incomplete dist/ cannot pass as a set
#    ready to publish:
#    - every regular artifact is one of the names build-dist.sh generates and
#      (unless --allow-mixed) carries the version in Cargo.toml;
#    - SHA256SUMS.txt parses, lists no duplicated or metadata entries, and
#      every listed hash matches the file on disk; when the file exists it
#      must cover EVERY artifact, and --final requires the file itself;
#    - latest.txt contains exactly the stable version plus one newline when a
#      stable version is being checked, and is absent for a pre-release. A
#      present but wrong marker is always a failure; a missing one is allowed
#      before build-dist writes it, and required with --final.
#
# Only REGULAR, non-hidden files are release artifacts. Finder/editor
# droppings (`.DS_Store`) are ignored; a hidden symlink or a hidden leftover
# directory in dist/ is reported as a problem (publishing would follow or
# silently skip it), like any other symlink/directory.
#
# Exit status: 0 when dist/ is absent/empty without `--require` (nothing to
# check), or when every artifact passes; 1 when any privacy or integrity issue
# is found (including no artifacts at all under `--require`); 2 when the gate
# cannot run (missing dist/, inspector or Cargo version) or gets invalid flags.
set -u

REQUIRE=0
ALLOW_MIXED=0
FINAL=0
DIST_ARG=""

usage() {
    cat <<'EOF'
Usage: ./scripts/check-dist.sh [--require] [--allow-mixed] [--final] [dist-dir]

  --require      fail when the gate cannot run (no python3, no dist/)
  --allow-mixed  allow other versions in an additive dist/ (skip version and
                 update-marker comparisons; privacy/checksum checks still run)
  --final        require complete SHA256SUMS.txt/latest.txt metadata
                 (implies --require; cannot be combined with --allow-mixed)
EOF
}

for arg in "$@"; do
    case "$arg" in
        --require) REQUIRE=1 ;;
        --allow-mixed) ALLOW_MIXED=1 ;;
        --final) FINAL=1 ;;
        -h|--help)
            usage
            exit 0
            ;;
        --*)
            echo "ERROR: unknown option '$arg' (try --help)" >&2
            exit 2
            ;;
        *)
            if [ -n "$DIST_ARG" ]; then
                echo "ERROR: at most one dist directory may be given" >&2
                exit 2
            fi
            DIST_ARG="$arg"
            ;;
    esac
done

if [ "$FINAL" -eq 1 ]; then
    if [ "$ALLOW_MIXED" -eq 1 ]; then
        echo "ERROR: --final and --allow-mixed are mutually exclusive" >&2
        exit 2
    fi
    # A finished release must also fail when the gate itself cannot run.
    REQUIRE=1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="${DIST_ARG:-$ROOT/dist}"

if [ ! -d "$DIST" ]; then
    if [ "$REQUIRE" -eq 1 ]; then
        echo "ERROR: no $DIST to check (--require)" >&2
        exit 2
    fi
    echo "SKIP: no $DIST (build it with make dist first)"
    exit 0
fi

if ! command -v python3 >/dev/null 2>&1; then
    if [ "$REQUIRE" -eq 1 ]; then
        echo "ERROR: python3 not found (needed to inspect the archives)" >&2
        exit 2
    fi
    echo "SKIP: python3 not found (needed to inspect the archives)"
    exit 0
fi

# The expected version comes from the same helper build-dist.sh uses, so the
# gate and the artifact names can never disagree. --allow-mixed still needs no
# version: it accepts the other versions an additive dist/ may hold.
VER=""
DEB_VER=""
RPM_VER=""
if [ -f "$ROOT/Cargo.toml" ]; then
    # shellcheck source=packaging.sh
    . "$ROOT/scripts/packaging.sh"
    VER="$(app_version)"
    if [ -n "$VER" ]; then
        DEB_VER="$(version_deb "$VER")"
        RPM_VER="$(version_rpm "$VER")"
    fi
fi
if [ "$ALLOW_MIXED" -eq 0 ] && [ -z "$VER" ]; then
    echo "ERROR: could not parse the version from $ROOT/Cargo.toml" >&2
    exit 2
fi
case "$VER" in
    *[!0-9A-Za-z._+-]*)
        echo "ERROR: refusing to check dist/ against unsafe version '$VER'" >&2
        exit 2
        ;;
esac
case "$VER" in
    *-*|*+*) PRE_RELEASE=1 ;;
    *) PRE_RELEASE=0 ;;
esac

exec python3 - "$DIST" "$REQUIRE" "$ALLOW_MIXED" "$FINAL" \
    "$VER" "$DEB_VER" "$RPM_VER" "$PRE_RELEASE" <<'PY'
import hashlib
import io
import os
import re
import socket
import struct
import sys
import tarfile
import zipfile

dist = sys.argv[1]
require = sys.argv[2] == "1"
allow_mixed = sys.argv[3] == "1"
final = sys.argv[4] == "1"
ver = sys.argv[5]
deb_ver = sys.argv[6]
rpm_ver = sys.argv[7]
pre_release = sys.argv[8] == "1"

problems = []
scanned = 0
# Compressed package payloads (RPM's cpio, Flatpak's OSTree commit) cannot be
# unpacked with the Python stdlib. The raw byte scan still catches plaintext
# needles in their headers, but we must not REPORT those artifacts as fully
# inspected: track them so the final line says so explicitly.
header_only = set()

# --- needles: what the builder's identity looks like ------------------------
# The app intentionally ships example paths like `/Users/you/notes.md` and
# `/Users/me/...`; matching only the REAL home/cargo-home/userprofile (and the
# build host's nodename) avoids flagging those.
needles = {}
def add_needle(label, value):
    if not value or len(value) < 2:
        return
    needles[f"{label}={value}"] = value.encode()
    if "\\" in value:
        needles[f"{label}={value} (slashes)"] = value.replace("\\", "/").encode()

add_needle("HOME", os.environ.get("HOME", ""))
add_needle("CARGO_HOME", os.environ.get("CARGO_HOME", ""))
add_needle("USERPROFILE", os.environ.get("USERPROFILE", ""))
# socket.gethostname() is portable (os.uname() does not exist on Windows,
# where this checker also runs under Git Bash via windows.ps1).
host = socket.gethostname()
# `/build` and `/cargo` are the remap targets rustc writes into every
# release binary; a machine whose hostname happens to be one of these (or
# another generic build word) would otherwise flag every artifact. Those
# short generic names carry no identifying information anyway.
GENERIC_HOSTNAMES = {
    "build", "builder", "cargo", "home", "linux", "mac", "macos",
    "root", "runner", "user", "users", "windows", "workspace",
}
if (len(host) >= 5
        and host not in ("localhost", "localhost.localdomain")
        and host.lower() not in GENERIC_HOSTNAMES):
    add_needle("hostname", host)


def scan_bytes(label, data):
    global scanned
    scanned += 1
    for name, needle in needles.items():
        if needle in data:
            problems.append(f"{label}: contains the builder's {name}")


def scan_tar(label, tf):
    for member in tf.getmembers():
        # Non-zero ownership or a recorded user/group name identifies the
        # build account; the packagers normalize to 0/0 with empty names.
        if member.uid != 0 or member.gid != 0 or member.uname or member.gname:
            problems.append(
                f"{label}: {member.name} owner uid/gid={member.uid}/{member.gid} "
                f"uname={member.uname!r} gname={member.gname!r}"
            )
        # AppleDouble sidecars (`._name`, `__MACOSX/...`) are macOS metadata
        # captured from the builder's xattrs (com.apple.provenance among
        # them): never package content. A packer that forgot to sweep the
        # staging tree ships them.
        base = os.path.basename(member.name)
        if member.name.startswith("__MACOSX/") or base.startswith("._"):
            problems.append(f"{label}: AppleDouble entry {member.name!r}")
        for key in member.pax_headers:
            if "xattr" in key.lower():
                problems.append(
                    f"{label}: {member.name} carries xattr PAX header {key!r}"
                )
        # A symlink's target is metadata, not payload, but an absolute
        # builder path there still leaks the build account (and points at a
        # file the target machine does not have).
        if member.issym() or member.islnk():
            scan_bytes(
                f"{label}!{member.name}",
                member.linkname.encode("utf-8", "surrogateescape"),
            )
        if member.isfile():
            handle = tf.extractfile(member)
            if handle is not None:
                scan_bytes(f"{label}!{member.name}", handle.read())


def scan_zip(label, path):
    with zipfile.ZipFile(path) as archive:
        for info in archive.infolist():
            base = os.path.basename(info.filename)
            if info.filename.startswith("__MACOSX/") or base.startswith("._"):
                problems.append(f"{label}: AppleDouble entry {info.filename!r}")
            pos = 0
            while pos + 4 <= len(info.extra):
                sub_id, size = struct.unpack_from("<HH", info.extra, pos)
                if sub_id == 0x7875:
                    payload = info.extra[pos + 4:pos + 4 + size].hex()
                    problems.append(
                        f"{label}: {info.filename!r} stores Unix uid/gid "
                        f"extra field ({payload})"
                    )
                pos += 4 + size
            scan_bytes(f"{label}!{info.filename}", archive.read(info))


def scan_deb(label, path):
    data = open(path, "rb").read()
    if data[:8] != b"!<arch>\n":
        problems.append(f"{label}: not an ar archive")
        return
    pos = 8
    while pos + 60 <= len(data):
        name = data[pos:pos + 16].decode("ascii", "replace").strip()
        try:
            size = int(data[pos + 48:pos + 58].decode("ascii").strip())
        except ValueError:
            break
        body = data[pos + 60:pos + 60 + size]
        try:
            with tarfile.open(fileobj=io.BytesIO(body), mode="r:*") as tf:
                scan_tar(f"{label}!{name}", tf)
        except tarfile.TarError:
            scan_bytes(f"{label}!{name}", body)
        pos += 60 + size + (size % 2)


def _scan_path(path):
    label = os.path.basename(path)
    if path.endswith(".zip"):
        scan_zip(label, path)
    elif path.endswith(".deb"):
        scan_deb(label, path)
    elif path.endswith((".tar.gz", ".tgz", ".tar")):
        with tarfile.open(path, "r:*") as tf:
            scan_tar(label, tf)
    elif path.endswith((".rpm", ".flatpak")):
        header_only.add(os.path.splitext(path)[1])
        with open(path, "rb") as handle:
            scan_bytes(label, handle.read())
    else:
        with open(path, "rb") as handle:
            scan_bytes(label, handle.read())


def scan_path(path):
    # A corrupt/truncated archive is a release problem, not a crash: the gate
    # must say which artifact could not be inspected and fail closed. Python's
    # archive readers raise several types (`BadZipFile`, `ReadError`, xz/zstd
    # errors, ...), so catch them all here.
    try:
        _scan_path(path)
    except Exception as exc:  # noqa: BLE001 - fail closed on any inspector error
        problems.append(
            f"{os.path.basename(path)}: cannot inspect archive "
            f"({type(exc).__name__}: {exc})"
        )


# --- artifact names and expected version ------------------------------------
# Keep these in sync with the paths build-dist.sh writes. Each tuple is
# (label, compiled pattern, expected version); the pattern's first group is
# the version its filename encodes. Deb/RPM use the normalized forms (a
# semver `-pre` becomes `~pre`); every other format keeps the full version.
artifact_patterns = [
    ("macOS app zip",
     re.compile(r"^MDowser-(.+)-macos-universal\.app\.zip$"), ver),
    ("Linux tarball",
     re.compile(r"^mdowser-(.+)-linux-[A-Za-z0-9_]+\.tar\.gz$"), ver),
    ("Debian package",
     re.compile(r"^mdowser_([^_]+)_[A-Za-z0-9]+\.deb$"), deb_ver),
    # RPM normalizes a pre-release `-` to `~`, so the version itself never
    # contains a hyphen: everything after the first one is release/arch.
    ("RPM package",
     re.compile(r"^mdowser-([^-]+)-.+\.rpm$"), rpm_ver),
    ("Flatpak bundle",
     re.compile(r"^mdowser-(.+)\.flatpak$"), ver),
    ("Windows zip",
     re.compile(r"^mdowser-(.+)-windows-[A-Za-z0-9_]+\.zip$"), ver),
]


def classify(name):
    for label, pattern, expected in artifact_patterns:
        match = pattern.fullmatch(name)
        if match:
            return label, match.group(1), expected
    return None


def check_artifact(name):
    kind = classify(name)
    if kind is None:
        problems.append(f"{name}: not a release artifact name build-dist.sh generates")
        return
    label, found, expected = kind
    if allow_mixed:
        return
    if not expected:
        problems.append(f"{name}: cannot verify its {label} version")
    elif found != expected:
        problems.append(
            f"{name}: {label} version {found!r} does not match {expected!r} "
            f"from Cargo.toml (stale artifact from an earlier build?)"
        )


# --- metadata ---------------------------------------------------------------
def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_checksums(artifacts):
    path = os.path.join(dist, "SHA256SUMS.txt")
    if not os.path.isfile(path) or os.path.islink(path):
        if final:
            problems.append("SHA256SUMS.txt: missing (--final requires it)")
        return
    try:
        with open(path, "rb") as handle:
            data = handle.read()
    except OSError as exc:
        problems.append(f"SHA256SUMS.txt: cannot read: {exc}")
        return
    scan_bytes("SHA256SUMS.txt", data)
    seen = set()
    listed = set()
    for lineno, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1):
        if not line.strip():
            continue
        # sha256sum prints two spaces; BSD `shasum` too, and some tools use
        # ` *name` for binary mode. Accept both separators, nothing else.
        match = re.fullmatch(r"([0-9A-Fa-f]{64}) [ *](.+)", line)
        if not match:
            problems.append(f"SHA256SUMS.txt:{lineno}: not a sha256sum line")
            continue
        digest, name = match.group(1).lower(), match.group(2)
        if name in seen:
            problems.append(f"SHA256SUMS.txt:{lineno}: duplicate entry for {name!r}")
            continue
        seen.add(name)
        if name in ("latest.txt", "SHA256SUMS.txt"):
            problems.append(
                f"SHA256SUMS.txt:{lineno}: {name} is metadata, not a downloadable build"
            )
            continue
        if name.startswith(".") or os.path.basename(name) != name or "\\" in name:
            problems.append(
                f"SHA256SUMS.txt:{lineno}: {name!r} is not a plain file name"
            )
            continue
        if name not in artifacts:
            problems.append(
                f"SHA256SUMS.txt:{lineno}: {name!r} is not a checked artifact"
            )
        listed.add(name)
        target = os.path.join(dist, name)
        if os.path.islink(target) or not os.path.isfile(target):
            problems.append(
                f"SHA256SUMS.txt:{lineno}: {name!r} has no regular file in dist/"
            )
            continue
        try:
            actual = sha256_file(target)
        except OSError as exc:
            problems.append(f"SHA256SUMS.txt:{lineno}: cannot read {name!r}: {exc}")
            continue
        if actual != digest:
            problems.append(
                f"SHA256SUMS.txt:{lineno}: {name!r} hash mismatch "
                f"(listed {digest}, actual {actual})"
            )
    missing = sorted(artifacts - listed)
    if missing:
        problems.append(
            "SHA256SUMS.txt does not cover: " + ", ".join(missing)
        )


def check_latest():
    path = os.path.join(dist, "latest.txt")
    exists = os.path.isfile(path) and not os.path.islink(path)
    if allow_mixed:
        # An additive dist/ may still hold other versions, so a marker naming
        # this run's version cannot be compared against the mixed set.
        return
    if pre_release:
        if exists:
            problems.append(
                f"latest.txt: pre-release {ver} must not publish an update marker"
            )
        return
    if not exists:
        if final:
            problems.append("latest.txt: missing (--final requires it for a stable version)")
        return
    try:
        with open(path, "rb") as handle:
            data = handle.read()
    except OSError as exc:
        problems.append(f"latest.txt: cannot read: {exc}")
        return
    expected = (ver + "\n").encode()
    if data != expected:
        problems.append(
            f"latest.txt: must contain exactly {ver!r} plus one newline, "
            f"found {data[:80]!r}"
        )


# --- walk dist/ -------------------------------------------------------------
artifacts = set()
checked = 0
metadata = 0
ignored = 0
for entry in sorted(os.listdir(dist)):
    path = os.path.join(dist, entry)
    # Hidden regular files are not release artifacts (Finder's `.DS_Store`,
    # an editor swap file); shell globs skip them when publishing too. Hidden
    # directories and symlinks are NOT ignorable: an uploader that globs `*`
    # would silently drop a staged tree or follow a link out of dist/.
    if entry.startswith(".") and os.path.isfile(path) and not os.path.islink(path):
        ignored += 1
        continue
    # A symlink would be followed here but not necessarily by the uploader:
    # never let a release point at a file outside dist/.
    if os.path.islink(path):
        problems.append(f"{entry}: release artifacts must be regular files, not symlinks")
        continue
    if os.path.isfile(path):
        if entry == "SHA256SUMS.txt":
            # Parsed after the walk, once every artifact name is known.
            metadata += 1
            continue
        if entry == "latest.txt":
            metadata += 1
            scan_path(path)
            continue
        artifacts.add(entry)
        checked += 1
        check_artifact(entry)
        scan_path(path)
    else:
        # Staging directories belong to a failed/interrupted build; they must
        # never sit in the final artifact set.
        problems.append(f"{entry}: not a regular file (leftover directory or special file)")

check_checksums(artifacts)
check_latest()

if problems:
    print(f"FAIL: {len(problems)} release-gate issue(s) in {dist}:")
    for problem in problems:
        print(f"  - {problem}")
    sys.exit(1)

if require and checked == 0:
    print(f"FAIL: no release artifacts in {dist} (--require; metadata alone is not a build)")
    sys.exit(1)

ignored_note = f", {ignored} hidden file(s) ignored" if ignored else ""
metadata_note = f", {metadata} metadata file(s)" if metadata else ""
# Be explicit about what the stdlib cannot inspect: a green result must never
# be read as a claim that a compressed .rpm/.flatpak payload was unpacked.
header_note = (
    f"; HEADER-ONLY (payload not unpacked): {' '.join(sorted(header_only))}"
    if header_only
    else ""
)
mode_note = " (mixed versions allowed)" if allow_mixed else ""
print(
    f"OK: {dist} clean{mode_note} ({checked} artifact(s) scanned, "
    f"{scanned} payload file(s) checked{metadata_note}{ignored_note}){header_note}"
)
PY
