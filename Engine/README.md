# Rustic Game Engine

Rustic Game Engine is a native 2D/3D engine and editor under active development. The
repository currently implements the M0-M6 foundation: project/authoring applications,
an engine-owned RHI with a `wgpu` adapter, ECS scenes, an incremental asset pipeline,
and isolated playtesting through an authenticated child runtime.

Electron is not used. The desktop applications are native Rust executables rendered through `egui`/`wgpu`.

## Implemented slice

- Separate native project manager, authoring editor, runtime, and asset worker.
- Versioned projects, templates, scenes, sidecars, catalogs, settings, snapshots, and IPC.
- Transactional persistence, safe VFS paths, stable IDs, bounded jobs/logging, and recovery.
- Engine-owned RHI/resource model plus isolated `renderer-wgpu` WGSL rendering to both
  deterministic offscreen targets and live native `winit` surfaces.
- ECS hierarchy, deterministic scene migration, primitives, extraction, and centralized undo.
- SQLite/DDC asset pipeline with required baseline importers and isolated risky decoding.
- Dockable editor panels, project locking/read-only mode, workspace recovery, and play controls.
- Authenticated isolated runtime with bounded frames, embedded Play, distinct native New
  Window/Standalone shells, live console, exact frame advance, crash supervision, and
  explicit undoable runtime-change review/apply.
- External-editor-first Lua, JavaScript, Python, C#, C/C++, Java, PHP, and Web gameplay
  with stable script identities, generated VS Code
  workspace/API annotations, versioned manifests/components, isolated execution,
  deterministic input actions, clickable diagnostics, and last-known-good hot reload.
- Strict locked workspace gates, dependency/license/advisory policy, and Windows/Linux CI.

The architectural baseline and staged roadmap live in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and
[`docs/ROADMAP.md`](docs/ROADMAP.md). Current acceptance evidence and remaining manual
qualification are recorded in [`docs/M0-M6_STATUS.md`](docs/M0-M6_STATUS.md).

## Build

Install the Rust toolchain named in `rust-toolchain.toml`, then run:

```powershell
cargo install cargo-deny --locked --version 0.20.2
cargo xtask doctor
cargo xtask test

# Equivalent individual quality commands:
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

Launch the native project manager with `cargo xtask run project-manager`. It can create
or import a project and starts the editor as a separate native process.

## Windows installer

From PowerShell, build a self-contained `.exe` installer with:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1
```

The command installs the pinned Rust toolchain and Inno Setup locally under `.tools`
when either is missing, verifies rustup against its official SHA-256 checksum and
Inno Setup's Authenticode signature, then writes the installer to `dist`. The installed application
ships the project manager, editor, runtime, and asset worker and does not require Rust,
Cargo, or Inno Setup. Setup lets users select Python, C#, C/C++, Java, and PHP; selected
toolchains are installed through Windows Package Manager. Lua 5.4, JavaScript/QuickJS,
and HTML/CSS support are bundled. Users can install more toolchains later from the
Start-menu Language Toolchain Manager or by rerunning Setup; shared toolchains can be
removed through Windows Installed Apps. Because it is a desktop application, its
folder is not added to `PATH` by default; pass `-InstallDirectoryOnPath` to select that
installer option by default.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the decision-record and project-file safety
conventions.

To build an installer in the cloud, use the repository's **Windows installer**
GitHub Actions workflow (`.github/workflows/windows-installer.yml` at the repository
root). Once the workflow is on the default branch, select **Run workflow** and
choose the branch to build. It runs the same installer script on a Windows runner.
After a successful run, download the **RusticGameEngine-Windows-Installer** artifact
from the run page and extract the ZIP to obtain the setup `.exe`. Artifacts require
GitHub access to this repository and are retained for 30 days.

Gameplay authors should start with [`docs/GAMEPLAY_PROGRAMMING.md`](docs/GAMEPLAY_PROGRAMMING.md).
