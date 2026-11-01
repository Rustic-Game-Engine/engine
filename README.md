# Rustic Game Engine

A native 2D/3D game engine and editor built in Rust with `egui` and `wgpu`.

Rustic brings project management, scene editing, asset importing, and
playtesting into a native desktop application, with support for gameplay
programming across multiple languages.

**Rustic is currently in pre-alpha. Features, APIs, and project formats
may change as development continues.**

[Documentation](https://rustic-game-engine.pages.dev/) ·
[Downloads](https://github.com/Rustic-Game-Engine/engine/releases) ·
[Discussions](https://github.com/Rustic-Game-Engine/engine/discussions) ·
[Report a bug](https://github.com/Rustic-Game-Engine/engine/issues)

## Features

- Native project manager and dockable editor.
- 2D/3D scene authoring with entity hierarchies and undo support.
- GPU rendering through `wgpu`.
- Asset importing, caching, and background processing.
- Embedded playtesting and separate runtime windows.
- Live console output, script diagnostics, and hot reload.
- Gameplay scripting across Lua, JavaScript, Python, C#, C/C++, Java,
  PHP, and Web workflows.

See the [engine documentation](Engine/README.md) for implementation
details and current limitations.

## Download and Install

1. Choose a supported release from the table below.
2. Expand **Assets** on its release page.
3. Download and run the `RusticGameEngine-Setup-*.exe` installer.
4. Open the project manager to create or import a project.

The Windows installer includes the engine applications and bundled
language support. Optional language toolchains can be selected during
installation.

Installed Windows builds support automatic updates. See the
[update guide](Engine/docs/AUTOMATIC_UPDATES.md) for details.

## Supported Versions

The seven most recently published releases receive security updates,
including prereleases. Each new release replaces the oldest supported
release.

This table updates automatically through GitHub Actions.

<!-- supported-versions:start -->
| Version | Supported | Download |
| ------- | --------- | -------- |
| v3.8-Pre-Alpha | ✅ Latest | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.8-Pre-Alpha) |
| v3.7-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.7-Pre-Alpha) |
| v3.6-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.6-Pre-Alpha) |
| v3.5-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.5-Pre-Alpha) |
| v3.4-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.4-Pre-Alpha) |
| v3.3-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.3-Pre-Alpha) |
| v3.2-Pre-Alpha | ✅ | [Download](https://github.com/Rustic-Game-Engine/engine/releases/tag/v3.2-Pre-Alpha) |
<!-- supported-versions:end -->

Download links open the release pages containing installers and source
downloads. Versions outside this table are unsupported.

## Build from Source

Install Git and the Rust toolchain specified in
[Engine/rust-toolchain.toml](Engine/rust-toolchain.toml).

Clone the repository and run:

```sh
git clone https://github.com/Rustic-Game-Engine/engine.git
cd engine/Engine

cargo install cargo-deny --locked --version 0.20.2
cargo xtask doctor
cargo xtask test
cargo xtask run project-manager
```

The project manager creates or imports projects and launches the editor.

Run Cargo commands from `Engine/`; the repository root is not a Cargo
workspace. Check the [engine README](Engine/README.md) for platform
requirements and troubleshooting.

### Build the Windows Installer

From PowerShell on Windows, inside `Engine/`:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1
```

The installer is written to `Engine/dist/`.

## Documentation

- [Gameplay programming](Engine/docs/GAMEPLAY_PROGRAMMING.md)
- [Editor explorer](Engine/docs/EXPLORER.md)
- [Viewport navigation](Engine/docs/VIEWPORT_NAVIGATION.md)
- [Architecture](Engine/docs/ARCHITECTURE.md)
- [Roadmap](Engine/docs/ROADMAP.md)
- [Build and installer instructions](Engine/README.md)

The documentation website is maintained in the separate
[docs repository](https://github.com/Rustic-Game-Engine/docs).

## Repository Layout

| Path | Contents |
| ---- | -------- |
| `Engine/` | Rust workspace, applications, tools, and engine components |
| `Engine/docs/` | Engine guides and architecture documentation |
| `.github/` | GitHub Actions and repository automation |

## Contributing

Bug reports, documentation improvements, tests, and code contributions
are welcome.

Read the [contribution guide](CONTRIBUTING.md),
[engine contribution guide](Engine/CONTRIBUTING.md), and
[CI requirements](.github/CI.md) before submitting changes.

Contributors should follow [AGENTS.md](AGENTS.md),
[Engine/AGENTS.md](Engine/AGENTS.md), and the
[Code of Conduct](CODE_OF_CONDUCT.md).

Changes to engine behavior or scripting APIs should include updates to
the affected engine guides and website documentation.

## Security

Please report potential vulnerabilities privately.
See the [Security Policy](SECURITY.md) for reporting instructions and
response expectations.

## License

Rustic Game Engine is licensed under the
[Apache License 2.0](LICENSE).
