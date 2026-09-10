# ADR-0003: QuickJS JavaScript gameplay adapter

Status: Accepted (2026-09-09)

## Decision

Rustic's second complete gameplay adapter is ECMAScript through QuickJS-NG and the
safe `rquickjs` bindings. It shares `ScriptId`, scene components, API versioning,
typed public properties, snapshots, external-editor navigation, lifecycle, and
last-known-good reload with Lua. A scene can attach Lua and JavaScript behaviors at
the same time; there is no project-language selector.

JavaScript source assigns its lifecycle object to `globalThis.behavior`. The adapter
compiles source without executing it by compiling it as a function body, then only
evaluates project code inside `rustic-runtime` after Play starts.

## Isolation and limits

The QuickJS context has no Node.js, browser, filesystem, network, process, registry,
environment, module loader, or native extension APIs. `rustic` is an engine-owned,
frozen command bridge. State enters as a bounded JSON-compatible snapshot and changes
leave as validated commands. The runtime enforces a 32 MiB heap limit, 512 KiB stack
limit, and an interrupt budget. A failing or over-budget callback disables its one
instance. QuickJS is never loaded in the editor process.

The adapter uses QuickJS's parallel runtime mode because Rustic's isolated simulation
worker owns and may move the behavior between OS threads. JavaScript and Lua never
exchange VM objects; they interact only through shared engine state and the versioned
engine API.

## Consequences

QuickJS-NG and `rquickjs` add build size and are covered by the repository dependency
audit. Async jobs, imports, DOM, and Node packages are intentionally unavailable.
Luau remains the next M7 adapter and requires a distinct isolated host because Cargo
cannot link Lua 5.4 and Luau through one feature-unified `mlua` package safely.
