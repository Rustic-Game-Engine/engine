# Third-party license records

Rust dependencies are resolved by the committed `Cargo.lock` and audited by
`cargo deny check` against `deny.toml`. The generated dependency inventory for a release
must be retained with its build evidence; [`../THIRD_PARTY.md`](../THIRD_PARTY.md)
documents the reviewed direct dependencies and policy.

Source copies of licenses required by a distributed binary belong in this directory.
No dependency may be added merely by copying a license here: its source, version,
features, engine-owned boundary, and failure behavior still require review.
