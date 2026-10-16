# MDowser Makefile: thin shortcuts over cargo + scripts/.
# - `make`          : install deps + host release binary + current-OS dist
#                     into dist/. Per platform the dist artifact set is:
#                     macOS -> the universal .app.zip ONLY; Linux -> .tar.gz +
#                     .deb/.rpm + Flatpak; Windows -> the .exe zip.
# - `make debug`    : host debug binary (target/debug/mdowser).
# - `make test`     : full pipeline (sh syntax, fmt, clippy, test, doc, debug).
# - `make audit`    : supply-chain audit (cargo-audit + cargo-deny, best-effort).
# - `make dist`     : installs deps + host release binary + EVERY platform
#                     packaged into dist/ (build + --all), as wide as possible.
# - `make dist-*`   : one group (host, mac, linux, windows, rpm, flatpak).
# - `make install`  : runtime files under PREFIX (DESTDIR honored; RPM %install).
# - `make flatpak`  : Flatpak bundle (needs flatpak-builder).
# - `make validate` : best-effort packaging metadata checks (SKIPs when tools
#                     are missing, FAILs on real errors from installed tools).
# - `make deps`     : install build/packaging dependencies (best-effort, idempotent).
# - `make run`      : runs debug. `make clean`, `make help`, `make doctor`.
# - `make icons`    : force-regenerates assets/icons from assets/icon.svg.
# - `make sync-sites` : download assets/builtin_sites.txt from the MDowser-Sites
#                     collaboration repo (curl) + validate every URL the way
#                     the browser resolves it; passing entries are imported,
#                     failing ones are dropped and reported (manual step, no CI).
# - `make check-sites` : validate one list file without re-downloading the
#                     upstream list or writing the asset (each URL is still
#                     probed over the network; default assets/builtin_sites.txt).
# All real work lives in scripts/build-test.sh and scripts/build-dist.sh.
# Needs GNU make (macOS and Linux ship it; check with `make --version`).
# The toolchain PATH is fixed up below so recipes never depend on it.

SHELL := /bin/sh

# rustup installs cargo into ~/.cargo/bin, which interactive shells add to
# PATH but make recipe shells frequently do NOT inherit (hence the classic
# "cargo: No such file or directory"). Resolve the binary to the first path
# that REALLY EXISTS, without hardcoding machine-specific absolute dirs
# (the repository stays portable across machines):
#   PATH lookup, `rustup which cargo`, ~/.cargo/bin, brew layout with the
#   prefix discovered at runtime, direct toolchain glob (HOME-relative).
export PATH := $(HOME)/.cargo/bin:$(PATH)
_WHICH_CARGO := $(shell command -v cargo 2>/dev/null)
_WHICH_RUSTUP := $(shell command -v rustup 2>/dev/null)
_RUSTUP_CARGO := $(if $(_WHICH_RUSTUP),$(shell $(_WHICH_RUSTUP) which cargo 2>/dev/null))
_BREW := $(shell command -v brew 2>/dev/null)
_BREW_PREFIX := $(if $(_BREW),$(shell $(_BREW) --prefix 2>/dev/null))
_BREW_CARGO := $(if $(_BREW_PREFIX),$(_BREW_PREFIX)/opt/rustup/bin/cargo)
# Prefer the live PATH lookup verbatim (it exists by definition, and `$(or)`
# never word-splits, so a home directory containing spaces survives). The
# wildcard fallbacks have to be single words: a spaced HOME still unsupported
# THERE is reported by `make doctor`, never by a confusing "No such file".
CARGO = $(or $(_WHICH_CARGO),$(firstword $(wildcard $(_RUSTUP_CARGO) $(HOME)/.cargo/bin/cargo $(HOME)/.rustup/toolchains/stable-*/bin/cargo $(HOME)/.rustup/toolchains/*/bin/cargo $(_BREW_CARGO))))
# `doctor` must stay runnable exactly when cargo cannot be found (it is the
# diagnosis path the error below points at), so the parse-time check skips
# it: every other goal fails immediately with the hint.
ifneq ($(MAKECMDGOALS),doctor)
ifeq ($(CARGO),)
$(error cargo not found (PATH lookup + rustup + ~/.cargo + brew all failed); install rustup, see README or run make doctor)
endif
endif
# cargo executes its sibling `rustc` on every build: lead PATH with cargo's
# own directory so `could not execute process rustc -vV` never happens on
# bare PATHs. Still relative: derived from the resolved binary, never hardcoded.
export PATH := $(dir $(CARGO)):$(PATH)

# Install prefix for `make install` (the RPM %install passes PREFIX=%{_prefix}).
PREFIX ?= /usr/local
# Dependency group for `make deps` (all | toolchain | cross | packaging).
DEPS ?= all
# Upstream collaboration repo for the curated omnibox list (see
# scripts/sync-builtin-sites.sh). Overridable: `make sync-sites SITES_UPSTREAM=<url>`.
SITES_UPSTREAM ?= https://raw.githubusercontent.com/MDowser/MDowser-Sites/main/builtin_sites.txt
# List file validated by `make check-sites` (no download).
SITES_FILE ?= assets/builtin_sites.txt

.PHONY: all debug build test dist dist-host dist-mac dist-linux dist-windows dist-rpm dist-flatpak install flatpak validate check-dist deps run clean fmt clippy audit icons sync-sites check-sites help doctor

# Default: install deps, build the host release binary, then the current-OS
# dist. macOS gets the universal .app (dist-mac); other systems the fast
# no-cross package (dist-host: Linux .tar.gz/.deb/.rpm/Flatpak, Windows .exe).
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
NATIVE_DIST = dist-mac
else
NATIVE_DIST = dist-host
endif
all: deps build $(NATIVE_DIST)

build:
	. ./scripts/lib.sh && export_remap_rustflags && "$(CARGO)" build --locked --release

# Host debug binary.
debug:
	"$(CARGO)" build --locked

# Verification pipeline (fmt + clippy + test + doc + debug).
test:
	./scripts/build-test.sh

# Install deps + host release binary + the WIDEST multi-OS dist into dist/
# (macOS universal .app, Linux tar.gz/.deb/.rpm/Flatpak, Windows .exe).
dist: deps build
	./scripts/build-dist.sh --all

dist-host:
	./scripts/build-dist.sh --host-only

dist-mac:
	./scripts/build-dist.sh --mac

dist-linux:
	./scripts/build-dist.sh --linux

dist-windows:
	./scripts/build-dist.sh --windows

# RPM built from source (needs rpmbuild, i.e. a Fedora/RHEL/openSUSE host).
dist-rpm:
	./scripts/build-dist.sh --rpm

# Flatpak bundle built from source (needs flatpak-builder).
dist-flatpak:
	./scripts/build-dist.sh --flatpak

# Install runtime files (binary + desktop entry + AppStream metadata + icons
# + man page) under PREFIX. `make install DESTDIR=/tmp/root PREFIX=/usr`
# stages a package; the RPM spec's %install calls exactly this target.
install:
	PREFIX="$(PREFIX)" DESTDIR="$(DESTDIR)" ./scripts/install.sh

# Flatpak bundle (needs flatpak-builder; see packaging/flatpak/).
flatpak:
	./scripts/build-flatpak.sh

# Best-effort packaging validation (desktop entry, AppStream metadata, RPM
# spec, Flatpak YAML) plus the dist/ privacy gate (builder paths, uid/gid,
# xattrs). Missing tools are SKIPs, never failures.
validate:
	./scripts/validate.sh

# Privacy gate for built artifacts: fails when any dist/ archive carries the
# builder's absolute paths or build-host metadata. Also run by `make dist`.
check-dist:
	./scripts/check-dist.sh

# Install build/packaging dependencies, best-effort and idempotent (Homebrew on
# macOS; apt-get/dnf/pacman on Linux). `SKIP_SETUP=1 make deps` reports only;
# `make deps DEPS=packaging` installs one group (all | toolchain | cross |
# packaging).
deps:
	./scripts/deps.sh $(DEPS)

run:
	"$(CARGO)" run --locked

fmt:
	"$(CARGO)" fmt --all

clippy:
	"$(CARGO)" clippy --locked --workspace --all-targets -- -D warnings

# Supply-chain audit (best-effort; run before releases).
# Skips with a hint when the tools are not installed.
audit:
	@if command -v cargo-audit >/dev/null 2>&1; then "$(CARGO)" audit; else echo "SKIP: cargo-audit not installed (cargo install cargo-audit)"; fi
	@if command -v cargo-deny >/dev/null 2>&1; then "$(CARGO)" deny check; else echo "SKIP: cargo-deny not installed (cargo install cargo-deny)"; fi

# Force-regenerate assets/icons/ (PNG ladder + .ico + .icns) from
# assets/icon.svg. The build itself regenerates them whenever they are
# missing or older than the SVG (build.rs + build_support/gen_icons.rs);
# removing them is the explicit way to force it.
icons:
	rm -rf assets/icons
	"$(CARGO)" build --locked

# Sync the curated omnibox list from the MDowser-Sites collaboration repo:
# curl download + per-URL validation the way the browser resolves it. The
# passing entries replace assets/builtin_sites.txt; a failing one is dropped
# and listed at the end with the counts (the sync still imports the rest).
# Manual maintainer step; never CI and never part of test/dist (the build
# stays offline and reproducible).
sync-sites:
	SITES_UPSTREAM="$(SITES_UPSTREAM)" ./scripts/sync-builtin-sites.sh

# Validate one list file without downloading (SITES_FILE, default
# assets/builtin_sites.txt). Same per-URL checks as sync-sites.
check-sites:
	./scripts/sync-builtin-sites.sh --check-only "$(SITES_FILE)"

clean:
	"$(CARGO)" clean
	rm -rf dist .flatpak-builder packaging/flatpak/.build-dir

# Diagnose the toolchain as THIS Makefile sees it (for "No such file" cases).
doctor:
	@echo "make:       $(shell make --version 2>/dev/null | head -1)"
	@echo "HOME:       $(HOME)"
	@echo "PATH:       $(PATH)"
	@echo "cargo:      $(CARGO)"
	@echo "rustc:      $(or $(firstword $(wildcard $(dir $(CARGO))rustc)),MISSING)"
	@echo "nasm:       $(or $(shell command -v nasm 2>/dev/null),MISSING - needed on x86/x86_64 for rav1d + aws-lc-sys)"
	@"$(CARGO)" --version || echo "ERROR: cargo not found; install rustup (see README)"

help:
	@echo "make [all|build|debug|test|dist|dist-host|dist-mac|dist-linux|dist-windows|dist-rpm|dist-flatpak|install|flatpak|validate|check-dist|deps|run|clean|fmt|clippy|audit|icons|sync-sites|check-sites|help|doctor]"
	@echo "  (no args)   deps + host release + current-OS dist into dist/"
	@echo "              (mac: universal .app | linux: tar.gz/.deb/.rpm/flatpak | win: .exe)"
	@echo "  build = host release binary"
	@echo "  debug = host debug"
	@echo "  test = sh+fmt+clippy+test+doc+debug   |  dist = deps + host + EVERY platform"
	@echo "  dist-host = current-OS dist only   |  dist-<mac|linux|windows> = one group"
	@echo "  dist-rpm = RPM from source (needs rpmbuild)   |  flatpak/dist-flatpak = Flatpak bundle"
	@echo "  install  = runtime files under PREFIX (DESTDIR/PREFIX honored; RPM %install)"
	@echo "  validate = best-effort packaging metadata checks (SKIPs when tools missing)"
	@echo "  check-dist = privacy gate: fail if dist/ carries builder paths/metadata"
	@echo "  deps     = install build/packaging deps (SKIP_SETUP=1 dry run; DEPS=<group>)"
	@echo "  audit = cargo audit + cargo deny (best-effort when installed)"
	@echo "  icons = force-regenerate assets/icons from assets/icon.svg"
	@echo "  sync-sites = download builtin_sites.txt from MDowser-Sites (curl) + validate every URL"
	@echo "  check-sites = validate one list file without downloading (SITES_FILE, default assets/builtin_sites.txt)"
