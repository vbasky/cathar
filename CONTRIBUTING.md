# Contributing to cathar

Thanks for contributing! This project keeps the dev loop simple and the `main`
branch always green.

## Setup

```bash
git clone https://github.com/vbasky/cathar.git
cd cathar
just setup   # points git at .githooks (pre-commit: fmt + clippy)
```

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it
automatically on first build.

## Before you push

Run the same gate CI runs:

```bash
just check-all
```

This checks formatting, runs clippy with `-D warnings`, runs the tests, and
builds the docs with `RUSTDOCFLAGS="-D warnings"`.

## Conventions

- **Formatting** is enforced by `rustfmt` (see `rustfmt.toml`) and applied
  automatically by the pre-commit hook — never hand-format.
- **Clippy** (`-D warnings`) runs on every commit via the same pre-commit hook
  and in CI. `just check-all` is the full local gate (fmt + clippy + test + doc).
- **Lints** live in `[workspace.lints]` in the root `Cargo.toml` and are
  inherited by every crate via `[lints] workspace = true`. Add a new crate? Add
  that stanza. Relaxing a clippy lint? Do it in the workspace table with an
  inline comment explaining why.
- **Tests** live in `#[cfg(test)]` modules next to the code they cover.
- **Docs**: `//!` for module-level docs, `///` for items. Broken intra-doc
  links fail CI.
- **Dependencies**: declare shared versions once in `[workspace.dependencies]`
  and reference them with `foo.workspace = true`.
- **Changelog**: user-facing changes go under `## [Unreleased]` in
  `CHANGELOG.md`.
