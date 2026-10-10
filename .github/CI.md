# Pull request checks

`quality.yml` runs on every pull request (including forks and drafts), without
branch or path filters, on pushes to `main` and `work`, and in merge queues.
GitHub only discovers workflows under the repository-root `.github/workflows`.

The `Required checks` job fails if any Rust formatting, Clippy, unit/integration/
documentation test, compilation, headless smoke test, language adapter check,
dependency/license audit, secret scan, Windows installer build, workflow syntax
check, or installer release automation test fails or is skipped.
The job summary links to the run; failed step logs contain diagnostic details.
The fresh Windows installer is saved as a workflow artifact for seven days.

## Installer releases

`installer-release.yml` publishes the installer artifact after a successful
`Code quality and security` or manually started `Windows installer` run on
`main` or `work`. It uses the completed run's exact commit and artifact; no
installer rebuild is needed. Pull request, fork, merge-queue, and feature-branch
builds keep their artifacts and do not publish releases.

The build commit must be the merge commit of exactly one pull request into that
branch. The release description copies that pull request's current description,
including Markdown, verbatim (an empty description stays empty). Direct pushes
without a matching merged PR fail the publication job with an explicit error.

Release titles and tags start at `v0.2-Pre-Alpha` and increase by 0.1 from the
highest existing pre-alpha release: `v0.3-Pre-Alpha`, through `v0.9-Pre-Alpha`,
then `v1.0-Pre-Alpha`. The existing `v0.1-pre-alpha` is recognized. Releases are
published as prereleases with the Windows `.exe` attached. Both build workflows
share one publication concurrency group, and retrying a build or publication
repairs the release for that commit rather than allocating another version.

The publishing workflow must be merged into the default branch before GitHub
can trigger it. To diagnose publication, open `Publish installer release` in
Actions. Missing/expired artifacts, missing merged-PR metadata, or insufficient
token permissions fail that job. After correcting the problem, rerun its failed
job while the artifact is still available. A standalone manual installer build
must select `main` or `work` at a merged PR commit.
Linux headless rendering uses Mesa's CPU Vulkan driver. Both engine and language
adapter jobs initialize the installed .NET SDK before launching parallel tests,
so C# fixtures do not race its first-run NuGet migrations. Windows gameplay unit
tests exercise audio mixing without opening a physical playback device; packaged
Windows applications continue to use the native audio output.

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

Run installer release automation tests and validate workflow syntax with:

```sh
python3 -m unittest discover -s .github/tests -v
actionlint .github/workflows/*.yml
```

To test the pull request flow, create a branch from `work`, make a small
documentation change, and open a pull request targeting `work`. Documentation-only
pull requests also run the complete checks. Look for `Required checks` on the
pull request and inspect failed job logs when it does not pass.
