# Pull request checks

`quality.yml` runs on every pull request (including forks and drafts), without
branch or path filters, on pushes to `main` and `work`, and in merge queues.
GitHub only discovers workflows under the repository-root `.github/workflows`.

The `Required checks` job fails if any Rust formatting, Clippy, unit/integration/
documentation test, compilation, headless smoke test, language adapter check,
dependency/license audit, secret scan, Windows installer build, or workflow
syntax check fails or is skipped.
The job summary links to the run; failed step logs contain diagnostic details.
The fresh Windows installer is saved as a workflow artifact for seven days.

Documentation site lint, type checking, static builds, dependency audits, and
Cloudflare Pages deployment live in the [docs repository](https://github.com/Rustic-Game-Engine/docs).

## Require checks before merging

For both `main` and `work`, configure a branch ruleset or branch protection to
require `Required checks` and require the branch to be up to date before merging.
The workflow must exist on each target branch. Merge queues run the same checks
on the merge group.

Repository rules are managed in
[GitHub settings](https://github.com/Rustic-Game-Engine/engine/settings/rules).

## Local validation

Validate workflow syntax with:

```sh
actionlint .github/workflows/*.yml
```

To test the pull request flow, create a branch from `work`, make a small
documentation change, and open a pull request targeting `work`. Documentation-only
pull requests also run the complete checks. Look for `Required checks` on the
pull request and inspect failed job logs when it does not pass.
