# AGENTS.md — rules for AI agents

Read this before creating or modifying files in this repository.

## Do not create versioned automation or AI artifacts

- **Never create or modify `.github/`** (workflows, Dependabot, issue/PR
  templates). CI and repository automation are intentionally out of scope
  for this project. Stale references to `.github/workflows` in docs or
  scripts are historic, not an invitation to regenerate workflows. If
  automation is genuinely needed, ask the user first.
- Do not commit AI tooling scratch: `.opencode/`, `.openchamber/`,
  `.claude/`, `.codex/`, `.cursor/`, `.aider*`, plan files or session
  logs. They are gitignored; do not force-add them or weaken
  `.gitignore` to let them through.

## Version control

- El repositorio git está operativo (remoto `origin` en GitHub); no recrearlo,
  no cambiar de remoto ni reescribir su historial sin petición expresa.
- Lectura libre de git (`git status`, `git log`, `git diff`). Los commits se
  crean **solo cuando el usuario lo pide explícitamente** ("haz commit",
  "commitea esto", …): nunca automáticos, nunca como paso intermedio de otra
  tarea, y no proponerlos por iniciativa propia.
- `git push`, tags, `amend`/`rebase`/`reset --hard` y force-push: solo si el
  usuario lo pide.
- Al preparar un commit, limitarse a entregables del proyecto: source, docs, scripts, `assets/`,
  build inputs (`Cargo.toml`, `Cargo.lock`, `deny.toml`,
  `.cargo/audit.toml`, `rust-toolchain.toml`, `.editorconfig`,
  `.gitattributes`).
- Nunca salida de build (`target/`, `dist/`), OS cruft (`.DS_Store`),
  secretos (`.env*`) ni config de CI generada.
- No añadir atribución AI a los commits ("Generated with…" trailers).
- Privacidad de rutas locales: no publicar rutas absolutas del equipo en respuestas, docs, logs ni mensajes de commit; usar rutas relativas al proyecto.

## Documentation stays current

- Any change that alters behavior, CLI, architecture, build/packaging,
  security posture or user-visible text must update the affected files in
  `docs/` in the same change (e.g. `ARCHITECTURE.md` for module
  contracts, `BUILDING.md` for toolchain/packaging, `SECURITY.md` for the
  threat model, `INTERFACE.md`/`NAVIGATION.md`/`RENDERING.md` for
  UI/URL/document behavior).
- User-visible changes also get an entry under `## [Unreleased]` in
  `CHANGELOG.md` (Keep a Changelog sections: `Added`/`Changed`/`Fixed`/
  `Removed`), as required by `docs/CONTRIBUTING.md`.
- Stale docs or a missing changelog entry make a change incomplete.

## Conventions

- Follow `docs/CONTRIBUTING.md` and keep changes consistent with the
  existing structure; run the relevant checks (`make fmt`, `make clippy`,
  `make test`) before claiming work is done.
