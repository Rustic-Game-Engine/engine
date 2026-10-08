# Pull request checks

`quality.yml` runs on every pull request (including forks and drafts), without
branch or path filters, on pushes to `main` and `work`, and in merge queues.
GitHub only discovers workflows under the repository-root `.github/workflows`;
the older files under `Engine/.github/workflows` do not run automatically.

The `Required checks` job fails if any Rust formatting, Clippy, unit/integration/
documentation test, compilation, headless smoke test, language adapter check,
dependency/license audit, secret scan, website lint/type/build/audit check,
Windows installer build, or review automation test fails or is skipped.
The job summary links to the run; failed step logs contain diagnostic details.
The fresh Windows installer is saved as a workflow artifact for seven days.

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

Generated `Engine/target*`, `.tools`, `dist`, website `node_modules`, `.next`, and
TypeScript build-info files are excluded from AI review. Diffs above 300 KB,
binary changes outside these outputs, and commits without shared history within
the 1,024-commit fetch window fail review rather than silently accepting partial
coverage. Split large changes or arrange manual review. This review supplements
deterministic checks and does not guarantee correctness or security.

## Require checks before merging

For both `main` and `work`, configure a branch ruleset or branch protection to
require `Required checks` and `GPT-6 Luna review`, and require the branch to be
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
