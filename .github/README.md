# Pull request checks

`quality.yml` runs on every pull request (including forks and drafts), without
branch or path filters, on pushes to `main` and `work`, and in merge queues.
GitHub only discovers workflows under the repository-root `.github/workflows`;
the older files under `Engine/.github/workflows` do not run automatically.

The `Required checks` job fails if any Rust formatting, Clippy, unit/integration/
documentation test, compilation, headless smoke test, language adapter check,
dependency/license audit, secret scan,
Windows installer build, or review automation test fails or is skipped.
The job summary links to the run; failed step logs contain diagnostic details.
The fresh Windows installer is saved as a workflow artifact for seven days.

Documentation site lint, type checking, static builds, dependency audits, and
Cloudflare Pages deployment live in the [docs repository](https://github.com/Rustic-Game-Engine/docs).

## OpenAI setup

The repository Actions secret is named `OPEN_AI_API_KEY` at:

https://github.com/JamieW105/Rustic-Game-Engine/settings/secrets/actions

The workflow maps this secret to the script's `OPENAI_API_KEY` environment variable.
Use a project API key with Responses API permission, billing enabled, and access
to [`gpt-6-luna`](https://developers.openai.com/api/docs/models/gpt-6-luna).
The review uses the [Responses API with structured output](https://developers.openai.com/api/docs/guides/structured-outputs).
There is no substitute model. A missing key, unavailable model, refusal,
incomplete response, or API failure produces a failed status and an explanation.
After adding the secret, rerun the failed review from the Actions tab.

`ai-review.yml` runs on every pull request via `pull_request_target`, including
forks and drafts, on pushes to `main` and `work`, and for merge groups. It checks out only trusted
base-branch tooling. PR commit objects are fetched and compared as data; submitted
code, dependency installers, and scripts are never executed in this privileged
job. Checkout credentials are not persisted. The API key is only available in
the review step, and response storage is disabled. The text diff is sent to
OpenAI; GitHub job logs are not sent. Review costs depend on diff size.

Each review posts concerns with severity, file, line, impact, and suggested fix,
adds a job summary and report artifact, and publishes the `GPT-6 Luna review`
commit status on the exact PR head SHA. High/critical concerns fail the status;
medium/low findings remain visible without blocking. A clean report is also
posted. On pushes, results appear in the run summary and commit status.

Generated `Engine/target*`, `.tools`, `dist`, and TypeScript build-info files
are excluded from AI review. Diffs above 300 KB,
binary changes outside these outputs, and commits without shared history within
the 1,024-commit fetch window fail review rather than silently accepting partial
coverage. Split large changes or arrange manual review. This review supplements
deterministic checks and does not guarantee correctness or security.

## Require checks before merging

For both `main` and `work`, configure a branch ruleset or branch protection to
require `Required checks`, `GPT-6 Luna review`, and `GPT-6 Luna full sweep`, and require the branch to be
up to date before merging. The AI commit status is needed because
`pull_request_target` workflow check runs attach to the trusted base commit.
Do not substitute the similarly named workflow check for the head commit status.

Settings are at:

https://github.com/JamieW105/Rustic-Game-Engine/settings/rules

The workflows must exist on both branches to review pull requests targeting
either one. They must also be installed on any additional target branch for
the privileged review to run there. Changes to trusted review tooling should be
reviewed by a maintainer. Merge queues run deterministic CI and AI review on the
merge group; the AI job uses trusted base-branch tooling for this event too.

## Full repository sweep

`full-review.yml` runs when `Code quality and security` completes, including
failed runs, for pull requests, branch pushes, manual CI runs, and merge groups.
Cancelled CI runs are skipped; their required CI checks remain unsatisfied.
The quick diff review is a separate result; a clean diff review does not mean
the complete repository or CI passed.

The sweep reads all tracked UTF-8 text files at the exact target commit,
including unchanged code, tests, documentation, configuration, and dependency
lockfiles. It partitions complete numbered source lines into bounded batches and
uses `gpt-6-luna` with high reasoning effort. Shared complete files supply the
installer, manifests, architecture, and repository requirements to every batch.
It checks normal behavior, failure/recovery paths, cache and version interactions,
security, missing tests, and repository validation requirements. Existing bugs
can appear in findings even when the PR did not change their files.

CI job results and failed step names must match the reviewed SHA. A successful
Windows installer job counts as evidence of a build, while CI failures make the
overall sweep fail even if the AI found no blocking code issue. Pending or absent
CI evidence keeps the sweep status pending. Medium/low concerns remain visible;
high/critical concerns fail the sweep. All code concerns and the CI results appear
in a separate PR comment, the run summary, the console log, and the
`gpt-6-luna-full-sweep` artifact.

The coverage artifact lists text files reviewed, omitted binary/non-UTF-8 or
sensitive files, generated-file exclusions, and completed/total batch counts.
Generated outputs are excluded as in the diff review. Actual `.env` files and
private-key/certificate containers are not sent to the model; the secret scanner
checks credentials. The maximum source budget is 10 MB and 64 batches. Oversized
source lines, unsupported submodules, API errors, or unfinished batches fail the
sweep rather than claiming full coverage. Binary assets require separate review.
The sweep makes multiple API requests and costs more than a diff-only review.
Even complete text coverage cannot guarantee that Luna finds every bug.
Source batches have a larger response budget than quick diff reviews. A batch
that reaches the output limit is retried once with twice that budget. Completed
findings are saved continuously in `review-findings.json`, so an incomplete sweep
still reports the concerns it found while keeping the overall status failed.
During a sweep, its PR comment and pending commit status update as batches
complete. The same comment is updated with the final result, so concerns appear
before the entire scan finishes without generating one comment per batch.

The privileged job checks out only trusted sweep tooling. It reads Git blobs as
data and uses GitHub job metadata; it never executes PR source or downloads CI
artifacts as executable inputs. After changing trusted tooling, test an existing
PR from the Actions tab by running `GPT-6 Luna full sweep` on `main` or `work`
and setting `pr_number` to the PR number. Leave it blank to review the selected
branch commit. Review source and requirements are treated as data, not executable
instructions.

## Local validation

To test the pull request flow, create a branch from `work`, make a small
documentation change, and open a pull request targeting `work`. Documentation-only
pull requests also run the complete checks. Look for `Required checks`, the
`GPT-6 Luna review` status on the PR's head commit, and Luna's review comment.
The full report is also available in the run summary and the
`gpt-6-luna-review` artifact; the review step's console log does not print it.

```sh
python3 -m unittest discover -s .github/tests -v
actionlint .github/workflows/*.yml
```
