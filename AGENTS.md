# AGENTS.md — rules for AI agents

Read this before creating or modifying files in this repository.

## Automation and AI artifacts stay out of the tree

- `.github/workflows/release.yml` is the only automation exception. It is a
  project deliverable: it builds every release target and publishes one
  release per `v*` tag. Create or modify it only for that purpose, never as
  a side effect of another task.
- Everything else under `.github/` (other workflows, Dependabot, issue and
  PR templates) is out of scope. Historic references to those paths in docs
  or scripts are not an invitation to recreate them. Ask the user before
  adding any other automation.
- Never commit AI-tooling scratch: `.opencode/`, `.openchamber/`,
  `.claude/`, `.codex/`, `.cursor/`, `.aider*`, plan files or session logs.
  They are gitignored; do not force-add them or weaken `.gitignore` to let
  them through.

## Version control

- The Git repository is live, with `origin` on GitHub. Do not recreate it,
  change the remote, or rewrite history without an explicit request.
- Read-only Git is always fine (`git status`, `git log`, `git diff`).
  Create commits only when the user explicitly asks ("commit this", "make a
  commit", …). Never commit automatically or as an intermediate step of
  another task, and do not propose a commit on your own initiative.
- `git push`, tags, `amend`/`rebase`/`reset --hard` and force-push only on
  request.
- When preparing a commit, stage only project deliverables: source, docs,
  scripts, `assets/` and build inputs (`Cargo.toml`, `Cargo.lock`,
  `deny.toml`, `.cargo/audit.toml`, `rust-toolchain.toml`, `.editorconfig`,
  `.gitattributes`).
- Never stage build output (`target/`, `dist/`), OS cruft (`.DS_Store`),
  secrets (`.env*`) or generated CI configuration.
- Do not add AI attribution to commits ("Generated with…" trailers).
- Keep local paths private: never publish absolute paths from this machine
  in replies, docs, logs or commit messages. Use project-relative paths.

## Documentation stays current

- Any change to behavior, the CLI, architecture, build/packaging, security
  posture or user-visible text must update the affected `docs/` files in the
  same change:
  - `ARCHITECTURE.md` for module contracts.
  - `BUILDING.md` for the toolchain and packaging.
  - `SECURITY.md` for the threat model and caps.
  - `INTERFACE.md`, `NAVIGATION.md` and `RENDERING.md` for UI, URL and
    document behavior.
- User-visible changes also need an entry under `## [Unreleased]` in
  `CHANGELOG.md`, using the Keep a Changelog sections (`Added`, `Changed`,
  `Fixed`, `Removed`), as required by `docs/CONTRIBUTING.md`.
- Stale docs or a missing changelog entry make a change incomplete.

## Conventions

- Follow `docs/CONTRIBUTING.md` and keep changes consistent with the existing
  structure.
- Run the relevant checks (`make fmt`, `make clippy`, `make test`) before
  claiming work is done.
