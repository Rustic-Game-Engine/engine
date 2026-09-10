# Contributing to Rustic Game Engine

Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before changing subsystem
boundaries and [`docs/ROADMAP.md`](docs/ROADMAP.md) before expanding milestone scope.
Rustic keeps the launcher, editor, and runtime as distinct native Rust processes;
Electron and browser-hosted editor shells are outside the architecture.

## Local verification

Install `cargo-deny` 0.20.2, then run:

```powershell
cargo xtask doctor
cargo xtask test
```

The second command runs formatting, Clippy with warnings denied, workspace tests,
workspace checks, the dependency/license audit, and both non-UI process smoke tests.

## Architecture decisions

A change to a decision in `docs/ARCHITECTURE.md` requires a short record in
`docs/decisions/`. Copy the template in that directory and document the observed
problem, replacement, migration effect, and rollback plan. Small implementation
choices that preserve an existing boundary do not need a decision record.

Do not overwrite project descriptors, scenes, catalogs, or other user data in place.
Persistent writes need a temporary-file, flush, atomic-install strategy and a tested
failure path that preserves the prior valid data.
