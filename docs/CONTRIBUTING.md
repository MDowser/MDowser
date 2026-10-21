# Contributing

## Quick start

```sh
make test          # sh syntax + version consistency + fmt --check + clippy -D warnings + test + doc + debug build
make run           # debug GUI
make audit         # cargo audit + cargo deny (best-effort when installed)
make check-version # Cargo.toml vs metainfo/RPM changelog/CHANGELOG.md/SECURITY.md
make check-dist    # privacy gate over dist/ (fails if dist/ is absent; after packaging)
make check-sites   # validate assets/builtin_sites.txt as the browser resolves it (network; manual)
```

Stable Rust ≥ 1.98 via `rustup` (`Cargo.toml` `rust-version`, floored by the
pure-Rust AVIF decoder); `make` and `scripts/lib.sh` resolve `cargo` without
hardcoded paths. Toolchain, packaging, runtime: [`BUILDING.md`](BUILDING.md).

## Builtin sites list

`assets/builtin_sites.txt` is vendored from MDowser-Sites: propose new
`<url> | <title>` lines there, then `make sync-sites` here. URLs are validated
as the browser resolves them (candidate cascade + Markdown `Content-Type` gate,
`scripts/sync-builtin-sites.sh`); passing entries imported, failing ones
dropped and reported at the end with the counts. `make check-sites`
re-validates without downloading the upstream list or writing the asset (each
URL is still probed over the network). Manual maintainer step: no CI.

## Standards

- `cargo fmt` with the stable `rustfmt.toml` options (max 100 columns) and
  `.editorconfig` (LF).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`; no
  pedantic/nursery lints globally. The shipped binary denies
  `clippy::unwrap_used`, `expect_used`, `panic` and `unreachable` (tests are
  exempt); new failure paths return typed errors or skip gracefully, and an
  embedded-asset invariant guard needs a documented `#[allow]`.
- English comments and user strings; every `src/` module keeps its `//!`
  contract header.
- Every `.rs`/`.sh` file carries the MPL-2.0 header (copyright line +
  three-line Mozilla Exhibit-A notice) above the `//!` block, after the shebang
  in shell scripts; copy it verbatim from `LICENSE`.
- Folders stay ≤ 2 levels deep under `src/`; split files past a few hundred
  lines by function into sibling modules; tests in sibling `tests*.rs`, never
  inline for large files.
- Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`); user-visible
  changes update `CHANGELOG.md` under `Unreleased`.
- UI icons are monochrome vector strokes in `ui::icons`: one hand-drawn 24x24
  glyph per `Icon` variant; add new icons there, never color-emoji textures.
  `media::emoji` serves only document emoji and stored bookmark-tile glyphs, so
  chrome follows the theme while documents keep color emoji.
- Add tests for every `protocol/`, `markdown/` and `util/` change
  (`cargo test`); viewer changes need a helper unit test or a manual screenshot.

## Version control

The project lives at <https://github.com/MDowser/MDowser> (`origin`). Create a
commit, push, tag or rewrite history only when explicitly requested — never as
a side effect of another change — and never add AI attribution ("Generated
with…"). Stage only project deliverables: no `target/`, `dist/`, `.env*`, OS
cruft or AI-tooling scratch (all gitignored).

## Security

Read [`SECURITY.md`](SECURITY.md) first. Never add `http://`, a `file:`
destination reachable from a remote page, a remote `file://host/` share, a
local read bypassing the UTF-8/caps/cancel contract, an image fetch bypassing
the Content-Type gate or decode/caps pipeline, an unencrypted or unbounded
cache, cookies, or `unsafe`. Persisted network responses go through `cache/`
with its HTTP freshness/eviction policy; new bounds live in `config/` (`MAX_*`)
with tests. Run `cargo audit` and `cargo deny check` before large dependency
bumps.
