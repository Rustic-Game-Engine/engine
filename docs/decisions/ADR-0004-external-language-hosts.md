# ADR-0004: External gameplay language hosts

Status: Accepted (2026-09-09)

## Decision

Python, C#, C, C++, Java, and PHP gameplay programs use one engine-owned, versioned
JSON host protocol. Python and PHP are syntax-checked by their CLIs; C/C++ are compiled
with Clang, GCC, or MSVC; C# is built with the .NET SDK; and Java is compiled with
`javac`. Build success is required before a generation can replace the active one.

One persistent process is created per behavior instance. Each lifecycle invocation
sends one bounded newline-delimited state document on standard input and receives one
bounded newline-delimited command document on standard output. This preserves
language-global state across callbacks. The state includes the callback, delta times,
stable entity identity, translation, and typed public properties. Commands can update
translation/properties, log, or disable the instance. This deliberately avoids a Rust
dynamic-library ABI and pairwise VM interoperability.

HTML documents and CSS content use the bundled Web adapter. Structural HTML/CSS is
validated and inline JavaScript lifecycle code executes in the same restricted
QuickJS context as ordinary JavaScript behaviors. No Node or browser capability is
implicitly granted.

## Isolation and availability

External programs start only after the user explicitly enters Play. They inherit a
small environment allowlist, run in an adapter-owned temporary working directory,
receive no editor/backend objects, have a three-second callback deadline, and have
bounded stdout/stderr. Failure disables only that behavior. Runtime replacement builds
beside the active generation and preserves the previous program if compilation fails.

External toolchains are optional dependencies and are never silently emulated.
`cargo xtask doctor` reports availability and versions. A missing compiler/runtime is
an actionable validation error. CI installs the reference versions and builds and
executes every process adapter fixture.

## Source contract

Rustic generates a starter program for every language and a JSON schema at
`.rustic/generated/programming/rustic_host_protocol.schema.json`. Native and managed
programs may organize code freely as long as their entry point implements protocol
version 1. Hot reload restarts external processes rather than unloading arbitrary
native code.
