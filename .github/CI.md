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
The installer job also runs Windows PowerShell 5.1 toolchain-manager regression tests
and silent packaged Setup tests. A local mock WinGet covers installed and missing
tools, ordinary package failures, and initialization errors `142`/`0xc0000142` without
downloading toolchains. Packaged tests verify a single PowerShell transcript for all
selected languages, that a failing WinGet is not retried for later languages, and
that bundled-only Setup does not start the toolchain manager.
The installer build also checks the GUI subsystem on the project manager and setup
helper. Native backend and PowerShell fixtures verify that helper children have no
console window, receive their arguments, capture stdout/stderr, and return exact
exit codes (including signed `0xc0000142`). Packaged tests require successful agent
integration registration and run the installed project manager's headless smoke path.
The visible Finish-screen launch still needs interactive qualification on Windows.

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

After the EXE upload and release publication succeed, the same job announces the
release in Discussions → **Change Log** (category slug `change-log`). Each post
includes direct EXE download links, a brief plain-text overview from the merged
PR title, and changed file names with addition/deletion counts and expandable
diff previews. File links open a comparison of the PR's recorded base commit and
release merge commit; the post also links the full file list and downloadable
patch. Announcements contain no PR links. Binary files or diffs omitted by GitHub have
links instead of previews. Large posts limit previews and direct readers to the
complete file list to stay within GitHub's size limit. GitHub's PR-files API
returns at most 3,000 files; the full diff and patch links remain available.

To associate the correct documentation update, add a line to the engine PR
description pointing to the affected **published page**, for example:

```text
Documentation: https://rustic-game-engine.pages.dev/docs/guides/gameplay-programming#hot-reload
```

Repeat the line for additional updated pages; duplicate URLs are removed. Use
the canonical documentation site's `/docs/` URLs, including section anchors when
helpful. Publish the documentation update before the release announcement. Do not
put a documentation PR URL in this field. The announcement copies only these
published-page URLs and never copies the PR description. For maintenance changes
without a documentation update, explicitly use `Documentation: none`; no docs
link is added. When there is no `Documentation:` line, canonical published docs
links elsewhere in the description are used instead. Missing or invalid mappings
do not block the announcement: it is posted without a docs link and the Actions
log shows a warning. Add the published page to the merged engine PR's description
and rerun publication to update the announcement. The automation does not guess a
matching update by title or choose the latest documentation PR, and does not
verify that the declared page has deployed.

The workflow token needs `discussions: write` alongside its existing permissions,
and Discussions must contain the `change-log` category. Retrying publication
updates the announcement identified by its hidden release-tag marker instead of
creating a duplicate. Keep that marker when editing an announcement manually.
The announcement copies all labels from the merged engine PR. Reruns synchronize
the Discussion's labels to the PR's current labels, adding missing labels and
removing labels no longer present on the PR (including manually added Discussion
labels). Label failures fail the job and can be repaired with a rerun.
An announcement failure fails the job but leaves the uploaded release available;
fix category access or permissions and rerun the job to repair the announcement.

The publishing workflow must be merged into the default branch before GitHub
can trigger it. To diagnose publication, open `Publish installer release` in
Actions. Missing/expired artifacts, missing merged-PR metadata, or insufficient
token permissions fail that job. After correcting the problem, rerun its failed
job while the artifact is still available. A standalone manual installer build
must select `main` or `work` at a merged PR commit.

If an EXE release exists but its change-log Discussion is missing, inspect the
publication step's traceback. Older tooling failed after upload when the PR
description lacked a `Documentation:` line. The corrected tooling warns and
continues. Existing failed runs use the workflow/tooling revision from their
original run: to recover one immediately, add a valid `Documentation:` line (or
`Documentation: none` when appropriate) to its merged PR description, then rerun
the failed publication job while its artifact is available. The existing tag is
reused, so no extra version is allocated. Future runs use the corrected tooling
once it is merged into the default branch.
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
