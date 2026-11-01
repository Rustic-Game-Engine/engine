# Third-party license records

Rust dependencies are resolved by the committed `Cargo.lock` and audited by
`cargo deny check` against `deny.toml`. The generated dependency inventory for a release
must be retained with its build evidence; [`../THIRD_PARTY.md`](../THIRD_PARTY.md)
documents the reviewed direct dependencies and policy.

Source copies of licenses required by a distributed binary belong in this directory.
No dependency may be added merely by copying a license here: its source, version,
features, engine-owned boundary, and failure behavior still require review.

The bundled Gradle 8.13 wrapper (`assets/android/gradle-wrapper.jar`) is covered by
[Apache License 2.0](Gradle-Apache-2.0.txt). Its verified SHA-256 is
`81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f`.
