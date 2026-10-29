# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# RPM spec for MDowser. Builds from the FULL source tree (the .deb, .tar.gz
# and .zip artifacts `make dist` produces ship the prebuilt binary instead).
#
# Build from the repository root with the version pinned by the caller in RPM
# form (single source of truth is Cargo.toml, scoped to `[package]` like
# `scripts/packaging.sh` `app_version`). RPM rejects `-` in Version, so a
# semver pre-release becomes `0.7.0~alpha.1` (the `~` sorts before 0.7.0):
#
#   VER=$(sed -n '/^\[package\]/,/^\[/s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' Cargo.toml | head -1)
#   RPM_VER=$(printf '%s' "$VER" | sed -e 's/+.*$//' -e 's/-/~/g')
#   rpmbuild -bb packaging/rpm/mdowser.spec \
#       --define "_topdir $PWD/dist/.rpmbuild" \
#       --define "ver $RPM_VER" \
#       --define "_sourcedir $PWD/dist/.rpmbuild/SOURCES" \
#       --define "_builddir $PWD/dist/.rpmbuild/BUILD" \
#       --define "_rpmdir $PWD/dist/.rpmbuild/RPMS" \
#       --define "_srcrpmdir $PWD/dist/.rpmbuild/SRPMS"
#
# `make dist-rpm` does all of that (and builds the source tarball first). The
# spec also builds on Fedora's packaging services once the tree is exported
# as mdowser-<ver>.tar.gz next to it.
#
# Network policy: `cargo build` fetches crates from crates.io during %build.
# A hermetic (no-network) build needs the crates vendored into the
# source tarball and a `.cargo/config.toml` pointing at them; that is the
# packager's choice and is NOT done here (a build service or local
# `rpmbuild` has network access).

Name:           mdowser
# `ver` is REQUIRED (see the header): a stale hardcoded fallback silently
# mislabeled manual builds, so refuse instead.
Version:        %{?ver}%{!?ver:%{error:ver is not defined; pass --define "ver <version>" (make dist-rpm does)}}
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

# Privacy: the RPM header records the build host in the BUILDHOST tag.
# Pin it to a neutral value so a locally built package does not disclose the
# builder's machine name (reproducible-builds practice).
%global _buildhost mdowser

%description
MDowser shows text/markdown documents served over HTTPS and local Markdown
files. It is a native desktop browser: one document format, one HTTP method,
no webview, no JavaScript, no cookies and no proxy. Repeat visits use a
bounded, encrypted local HTTP cache that can be cleared or disabled. It
registers as a Markdown viewer, so file managers can open .md documents with
it.

TLS verification uses the system trust store (rustls-platform-verifier).

%prep
%autosetup -n %{name}-%{version}

%build
# Privacy: remap the builder's absolute home / Cargo-registry paths out of the
# binary. Panic locations embed dependency source paths, and `strip` cannot
# remove them (they are runtime data). Mirrors `scripts/lib.sh`
# `export_remap_rustflags`; guard $HOME so an unset variable can never turn
# into an empty (catch-all) remap prefix.
if [ -n "${HOME:-}" ]; then
    RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/build"
    RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
    export RUSTFLAGS
fi
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
%defattr(-, root, root, -)
%license LICENSE
%doc README.md THIRD_PARTY_NOTICES.md
%{_bindir}/%{name}
%{_datadir}/applications/%{name}.desktop
%{_datadir}/metainfo/dev.mdowser.app.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/%{name}.png
%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg
%{_mandir}/man1/%{name}.1*

%changelog
* Thu Oct 29 2026 MDowser <noreply@mdowser.invalid> - 0.7.2-1
- Link destination previews, middle-click background tabs, callout and brand icons, and reader, cache and markup hardening (see CHANGELOG.md 0.7.2).
* Mon Oct 26 2026 MDowser <noreply@mdowser.invalid> - 0.7.1-1
- Flatpak build bridges the Rust 1.98.0 SDK extension with --ignore-rust-version (see CHANGELOG.md 0.7.1).
* Mon Oct 26 2026 MDowser <noreply@mdowser.invalid> - 0.7.0-1
- Browser navigation shortcuts, CLI relative paths, drag and drop, code-block Copy feedback, and a toolchain and dependency refresh (see CHANGELOG.md 0.7.0).
* Wed Oct 21 2026 MDowser <noreply@mdowser.invalid> - 0.6.3-1
- Offline cache reading, outline sidebar, Save page and reader hardening (see CHANGELOG.md 0.6.3).
* Mon Oct 19 2026 MDowser <noreply@mdowser.invalid> - 0.6.0-1
- Encrypted HTTP disk cache, --help/--version flags, dependency and supply-chain refresh (see CHANGELOG.md 0.6.0).
* Fri Oct 16 2026 MDowser <noreply@mdowser.invalid> - 0.5.4-1
- Find in page, tabbed Preferences, history refresh, hardening passes and UI fixes (see CHANGELOG.md 0.5.4).
* Thu Oct 15 2026 MDowser <noreply@mdowser.invalid> - 0.5.3-1
- Windows arm64 release build, tab-rail reorder, maintained omnibox list sync, and a security/UI hardening pass (see CHANGELOG.md 0.5.3).
* Thu Oct 15 2026 MDowser <noreply@mdowser.invalid> - 0.5.2-1
- Build-artifact privacy gate, CI release automation for every target, and Flatpak runtime 26.08 (see CHANGELOG.md 0.5.2).
* Wed Oct 14 2026 MDowser <noreply@mdowser.invalid> - 0.5.0-1
- Feature release: first-heading page titles, remembered window geometry, external-open action (see CHANGELOG.md 0.5.0).
* Wed Oct 14 2026 MDowser <noreply@mdowser.invalid> - 0.4.6-1
- Professionalization pass: shared theme scales, hardened packaging, docs sync (see CHANGELOG.md 0.4.6).
* Tue Oct 13 2026 MDowser <noreply@mdowser.invalid> - 0.4.5-1
- Security, rendering and packaging fixes (see CHANGELOG.md 0.4.5).
* Tue Oct 13 2026 MDowser <noreply@mdowser.invalid> - 0.4.4-1
- Package the desktop entry, AppStream metadata, hicolor icons and man page.
