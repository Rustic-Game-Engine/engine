# ADR-0002: External-editor-first Lua gameplay

Status: Accepted (2026-09-09)

## Decision

Rustic's first complete gameplay adapter is vendored Lua 5.4 through `mlua`. Gameplay
source is authored only in an external editor. The Rustic Editor creates, attaches,
validates, opens, and reports source; it contains no source editor, syntax highlighter,
terminal, web IDE, or language VM.

Lua was selected for its small MIT-licensed runtime, fast parse/reload cycle, mature
embedding model, portability, and lack of an unstable Rust dynamic-library ABI. The
adapter implements the engine-owned gameplay API 1.0. Scenes store only a stable
`ScriptId`, enabled state, and typed public values, so a future adapter can use the
same component and UI without changing scene identity or authoring workflows.

## Trust and isolation

All project source is untrusted. Validation compiles without invoking gameplay code.
Callbacks execute only in `rustic-runtime`, never in `rustic-editor`. The Lua global
environment removes filesystem, OS, process, environment, package loading, dynamic
file loading, debugging, and explicit garbage-collector controls. Host access is an
allow-list implemented by `GameplayHost`. Each callback has an instruction budget;
one exception or budget overrun disables that instance rather than the editor or play
session. Source size, IPC size, diagnostics, strings, public properties, and logs are
bounded. Immutable play snapshots verify every source hash before runtime startup.

Projects are trusted for gameplay only when Play is explicitly entered. There is no
native in-process editor execution and no dynamic-library ABI.

## Compatibility, reload, and determinism

The API uses `(major, minor)`: a major mismatch is rejected; compatible minor
additions are accepted. Manifest, component, diagnostics, exception, snapshot, and
reload messages carry explicit versions. Hot reload compiles a replacement first,
then swaps all matching instances at a simulation boundary. The shared engine-owned
property/world state survives compatible reload. A syntax error, hash mismatch, API
mismatch, or failed callback keeps the last-known-good behavior. Fixed updates use
the runtime's deterministic fixed delta and input is sampled into action states at
fixed boundaries; frame callbacks and external inputs are inherently nondeterministic
unless recorded by a future replay service.

## Dependencies and future adapters

`mlua`, Lua, and transitive glue are MIT-compatible and audited by `cargo deny`.
Future adapters implement the same versioned source/diagnostic/runtime boundary and
`GameplayHost` capabilities. They do not alter scenes, Inspector UI, external-editor
actions, snapshots, or console records. Native adapters remain separate processes and
must not depend on a Rust dynamic-library ABI.

## Consequences

Interactive breakpoint debugging is not claimed. Current debugging consists of
structured logs, compiler locations, runtime exceptions, and stack frames opened in
the configured external editor. Debug-adapter integration remains a future capability.
