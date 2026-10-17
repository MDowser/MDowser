# Contributing

## Quick start

```sh
make test   # sh syntax + fmt --check + clippy -D warnings + test + doc + debug build
make run    # debug GUI
make audit  # cargo audit + cargo deny (best-effort when installed)
make check-dist  # privacy gate over dist/ (builder paths/metadata; after packaging)
make check-sites  # validate assets/builtin_sites.txt the way the browser resolves it (network; manual maintainer step)
```

Toolchain: stable Rust ≥ 1.98 via `rustup` (`Cargo.toml` `rust-version`;
the floor comes from the pure-Rust AVIF decoder). `make`/`scripts/lib.sh`
resolve `cargo` without hardcoded paths. Full toolchain, packaging and
runtime details: [`BUILDING.md`](BUILDING.md).

## Builtin sites list

The curated omnibox list (`assets/builtin_sites.txt`) is vendored from the
MDowser-Sites collaboration repo: propose new `<url> | <title>` lines there,
then run `make sync-sites` here. Every URL is validated the way the browser
resolves it (candidate cascade + markdown `Content-Type` gate, see
`scripts/sync-builtin-sites.sh`): the passing entries are imported and a
failing one is dropped and reported at the end with the counts.
`make check-sites` re-validates without re-downloading the upstream list or
writing the asset (each URL is still probed over the network, the same way
the browser resolves it). A manual maintainer step on purpose: no CI runs it.

## Standards

- `cargo fmt` (stable options in `rustfmt.toml`, max 100 cols) + `.editorconfig` (LF).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`. Pedantic/nursery stay off globally.
- English comments + user strings. Every `src/` module keeps its `//!` contract header.
- Every `.rs` and `.sh` file opens with the brief MPL-2.0 header: the
  copyright line plus the three-line Mozilla Exhibit-A notice, above the
  `//!` block (after the shebang in shell scripts); new files copy it
  verbatim. Full text: `LICENSE`.
- Layout: folders stay at most two levels deep under `src/`; a file that grows
  past a few hundred lines is split by functionality into sibling modules
  (unit tests live in sibling `tests*.rs` modules, never inline for large
  files).
- Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`). Update `CHANGELOG.md`
  `Unreleased` for user-visible changes.
- UI icons are monochrome vector strokes in `ui::icons` (the `Icon` enum
  maps each variant to one hand-drawn glyph on the 24x24 grid): add new ones
  there, never color-emoji textures — `media::emoji` serves only document
  emoji and stored bookmark-tile glyphs, so the chrome follows the theme
  while the document keeps its color emoji.
- Tests for every `protocol/` + `markdown/` + `util/` change (`cargo test`).
  Viewer changes need at least a unit test for the helper or a manual screenshot.

## Version control

The project lives at <https://github.com/MDowser/MDowser> (`origin`). Commits
use Conventional Commits and follow the `Unreleased` changelog rules above.
Create a commit, push, tag or history rewrite only when it is explicitly
requested — never as a side effect of another change — and never add AI
attribution ("Generated with…") to a message. Staging stays limited to project
deliverables: no `target/`, `dist/`, `.env*`, OS cruft or AI tooling scratch
(all gitignored).

## Security

Read `SECURITY.md` first. Never add `http://`, a `file:` destination
reachable from a remote page, a remote `file://host/` share, a local read
that bypasses the UTF-8/caps/cancel contract, an image fetch that bypasses
the Content-Type gate or the decode/caps pipeline, an unencrypted or
unbounded cache, cookies, or `unsafe`. Every network response that is
persisted must go through `cache/` and its HTTP freshness/eviction policy.
Keep new bounds in `config/` (`MAX_*`) with tests. Run `cargo audit` +
`cargo deny check` before large dep bumps.
