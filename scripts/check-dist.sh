#!/bin/sh
# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# check-dist.sh: privacy gate for the packaged artifacts in dist/.
#
# Usage: ./scripts/check-dist.sh [dist-dir]   (= `make check-dist`)
#
# A release artifact must not carry the BUILDER's identity. Two known
# channels, both checked here:
#   - payload bytes: release binaries embed the absolute source path of every
#     panic location / `file!()`, including Cargo-registry dependencies
#     (`$HOME/.cargo/registry/src/...`), so the builder's username and home
#     layout travel in every binary (`strip` cannot remove them).
#   - archive metadata: Info-ZIP stores the Unix uid/gid extra field
#     (`0x7875`); bsdtar stores extended attributes as PAX records
#     (`LIBARCHIVE.xattr...`/`SCHILY.xattr...`) and could record a non-zero
#     owner. All three identify the build host.
# The build host's own nodename is checked too (RPM's `BUILDHOST` tag).
#
# Exit status: 0 when dist/ is absent/empty (nothing to check), or when every
# artifact is clean; 1 when any leak is found. `make validate` and
# `scripts/build-dist.sh` both call this (the latter before writing
# SHA256SUMS.txt, so a leaking build is never reported as ready).
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="${1:-$ROOT/dist}"

if [ ! -d "$DIST" ]; then
    echo "SKIP: no $DIST (build it with make dist first)"
    exit 0
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "SKIP: python3 not found (needed to inspect the archives)"
    exit 0
fi

exec python3 - "$DIST" <<'PY'
import io
import os
import socket
import struct
import sys
import tarfile
import zipfile

dist = sys.argv[1]

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
if len(host) >= 5 and host not in ("localhost", "localhost.localdomain"):
    add_needle("hostname", host)

problems = []
scanned = 0


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
        for key in member.pax_headers:
            if "xattr" in key.lower():
                problems.append(
                    f"{label}: {member.name} carries xattr PAX header {key!r}"
                )
        if member.isfile():
            handle = tf.extractfile(member)
            if handle is not None:
                scan_bytes(f"{label}!{member.name}", handle.read())


def scan_zip(label, path):
    with zipfile.ZipFile(path) as archive:
        for info in archive.infolist():
            if info.filename.startswith("__MACOSX/"):
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


def scan_path(path):
    label = os.path.basename(path)
    if path.endswith(".zip"):
        scan_zip(label, path)
    elif path.endswith(".deb"):
        scan_deb(label, path)
    elif path.endswith((".tar.gz", ".tgz", ".tar")):
        with tarfile.open(path, "r:*") as tf:
            scan_tar(label, tf)
    else:
        with open(path, "rb") as handle:
            scan_bytes(label, handle.read())


entries = sorted(os.listdir(dist))
checked = 0
for entry in entries:
    path = os.path.join(dist, entry)
    if os.path.isfile(path):
        checked += 1
        scan_path(path)

if problems:
    print(f"FAIL: {len(problems)} privacy issue(s) in {dist}:")
    for problem in problems:
        print(f"  - {problem}")
    sys.exit(1)

print(
    f"OK: {dist} clean ({checked} artifact(s) scanned, "
    f"{scanned} payload file(s) checked)"
)
PY
