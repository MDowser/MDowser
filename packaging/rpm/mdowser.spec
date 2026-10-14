# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# RPM spec for MDowser. Builds from the FULL source tree (the .deb, .tar.gz
# and .zip artifacts `make dist` produces ship the prebuilt binary instead).
#
# Build from the repository root with the version pinned by the caller (single
# source of truth is Cargo.toml, scoped to `[package]` like
# `scripts/packaging.sh` `app_version`):
#
#   VER=$(sed -n '/^\[package\]/,/^\[/s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' Cargo.toml | head -1)
#   rpmbuild -bb packaging/rpm/mdowser.spec \
#       --define "_topdir $PWD/dist/.rpmbuild" \
#       --define "ver $VER" \
#       --define "_sourcedir $PWD/dist/.rpmbuild/SOURCES" \
#       --define "_builddir $PWD/dist/.rpmbuild/BUILD" \
#       --define "_rpmdir $PWD/dist/.rpmbuild/RPMS" \
#       --define "_srcrpmdir $PWD/dist/.rpmbuild/SRPMS"
#
# `make dist-rpm` does all of that (and builds the source tarball first). The
# spec also builds under COPR/`fedpkg` once the tree is exported as
# mdowser-<ver>.tar.gz next to it.
#
# Network policy: `cargo build` fetches crates from crates.io during %build.
# A hermetic (Koji-style, no-network) build needs the crates vendored into the
# source tarball and a `.cargo/config.toml` pointing at them; that is the
# packager's choice and is NOT done here (COPR and local `rpmbuild` have
# network access).

Name:           mdowser
Version:        %{?ver}%{!?ver:0.4.6}
Release:        1%{?dist}
Summary:        Browser for text/markdown (CommonMark) over HTTPS and local files

License:        MPL-2.0
URL:            https://github.com/MDowser/MDowser
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  nasm
BuildRequires:  make
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
Requires:       ca-certificates

# `strip = "none"` + `lto = true` in Cargo.toml ship an unstripped binary on
# purpose (the toolchain strip is broken on some installs; `make dist` strips
# explicitly). rpm's debuginfo extraction on that binary is pointless and
# huge, so the debuginfo subpackage is disabled; brp-strip still strips the
# installed binary.
%global debug_package %{nil}

%description
MDowser shows text/markdown documents served over HTTPS and local Markdown
files. It is a native desktop browser: one document format, one HTTP method,
no webview, no JavaScript, no cookies, no cache and no proxy. It registers as
a Markdown viewer, so file managers can open .md documents with it.

TLS verification uses the system trust store (rustls-platform-verifier).

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked

%install
%make_install PREFIX=%{_prefix}

%check
# Informational: `appstreamcli` reports warnings as a non-zero exit and distro
# toolchains differ, so a stricter validator must not make the package
# unbuildable. The strict gate is `make validate` / `scripts/validate.sh`.
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop || :
appstreamcli validate --no-net \
    %{buildroot}%{_datadir}/metainfo/dev.mdowser.app.metainfo.xml || :

%files
%license LICENSE
%doc README.md THIRD_PARTY_NOTICES.md
%{_bindir}/%{name}
%{_datadir}/applications/%{name}.desktop
%{_datadir}/metainfo/dev.mdowser.app.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/%{name}.png
%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg
%{_mandir}/man1/%{name}.1*

%changelog
* Wed Oct 14 2026 MDowser <noreply@mdowser.invalid> - 0.4.6-1
- Professionalization pass: shared theme scales, hardened packaging, docs sync (see CHANGELOG.md 0.4.6).
* Tue Oct 13 2026 MDowser <noreply@mdowser.invalid> - 0.4.5-1
- Security, rendering and packaging fixes (see CHANGELOG.md 0.4.5).
* Tue Oct 13 2026 MDowser <noreply@mdowser.invalid> - 0.4.4-1
- Package the desktop entry, AppStream metadata, hicolor icons and man page.
