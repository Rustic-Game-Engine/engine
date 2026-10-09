# Rustic Game Engine

Rustic Game Engine is a native 2D/3D engine and editor built in Rust with
`egui`/`wgpu`. This repository contains the engine workspace in [`Engine/`](Engine/).
See the [engine README](Engine/README.md) for implemented features, build details,
and Windows installer instructions.

## Repository layout

- [`Engine/`](Engine/) contains the Rust workspace, applications, tooling, and
  engine-local documentation in [`Engine/docs/`](Engine/docs/).
- The Next.js documentation website lives at the root of the separate
  [docs repository](https://github.com/Rustic-Game-Engine/docs). Run its lint,
  build, and deployment workflows there.

Run engine commands from `Engine/`; the root of this repository is not a Cargo
project.

## Build and run

Install the Rust toolchain specified in
[`Engine/rust-toolchain.toml`](Engine/rust-toolchain.toml), then run from this
repository's root:

```sh
cd Engine
cargo install cargo-deny --locked --version 0.20.2
cargo xtask doctor
cargo xtask test
cargo xtask run project-manager
```

The project manager creates or imports projects and launches the native editor.
Start with the [gameplay programming guide](Engine/docs/GAMEPLAY_PROGRAMMING.md)
for scripting, and read the [architecture](Engine/docs/ARCHITECTURE.md) and
[roadmap](Engine/docs/ROADMAP.md) before changing subsystem boundaries or scope.

## Contribution rules

See [pull request checks and review automation](.github/CI.md) for CI requirements
and troubleshooting.

Follow the repository's [`AGENTS.md`](AGENTS.md), the engine-specific
[`Engine/AGENTS.md`](Engine/AGENTS.md), and
[`Engine/CONTRIBUTING.md`](Engine/CONTRIBUTING.md).

Whenever a change affects a script function, callback, Script API behavior, or
other engine functionality, update the affected website documentation in the same
task. In the docs repository, guides live in `docs/` and are served through
`lib/docs.ts`; API pages are generated through `lib/api-docs.ts`. Make new guides
reachable through `lib/docs-catalog.ts`. Keep the corresponding documentation in
`Engine/docs` accurate, and include the companion docs pull request in the engine
pull request when published documentation changes.

Write for first-time users: explain what works and where, exact setup and
attachment steps, copyable examples, expected results, current limitations, and
how to diagnose common failures. Correct outdated claims and verify the website
content against the implemented behavior before reporting completion.

After code changes, build the Windows installer from PowerShell on Windows in
`Engine/`:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1
```

Confirm that a fresh `Engine/dist/RusticGameEngine-Setup-*.exe` was produced. If
the build fails, report the failure and do not describe the task as fully complete.

Finish the relevant validation and documentation, commit on a feature branch,
push it, and open a pull request against the branch the work started from. Update
an existing task pull request instead of creating a duplicate. Include a concise
summary and the checks run in its description, then return its link.

Before reporting completion, apply at least one change-type label and every
relevant area label:

| Change type | Use for |
| --- | --- |
| `type:feature` | New functionality |
| `type:fix` | Bug fixes |
| `type:docs` | Documentation changes |
| `type:refactor` | Restructuring without changing behavior |
| `type:test` | Test additions or changes |
| `type:chore` | Maintenance, dependencies, or tooling |

| Area | Use for |
| --- | --- |
| `area:engine` | Changes to `Engine/`, including engine documentation |
| `area:website` | Changes to the documentation site, including its removal from this repository |
| `area:ci` | GitHub Actions, CI, or repository automation |

Choose labels for the actual changes. Root-level instructions-only changes use
`type:docs` and need no area label. If authentication or permissions prevent
pushing, opening the pull request, or labeling it, report the blocker and the
remaining step explicitly.
