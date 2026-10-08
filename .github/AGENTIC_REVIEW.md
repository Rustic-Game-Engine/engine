# Agentic pull request review

The `GPT-6 Luna review` status uses `gpt-6-luna` through the Responses API
with high reasoning effort. It reuses the existing `OPEN_AI_API_KEY` Actions
secret; no new secret or SDK dependency is needed.

The reviewer starts with the complete eligible diff and a tracked-file inventory
from the exact reviewed commit. Its structured responses can request up to eight
numbered file ranges per round, including unchanged callers, tests, manifests,
and configuration. Each range contains at most 400 lines. The trusted script
reads these Git blobs as data and supplies them on the next request, along with
the model's own next investigation question and findings so far. It does not
check out, import, execute, or install anything from the PR.

GPT-6 chooses when to finish. The first round must investigate rather than
immediately finish; the final response must explicitly mark the investigation
finished with no remaining file requests or follow-up question. Each round
rechecks and carries forward confirmed findings. Only concerns tied to changed
files are reported by this check; the separate full sweep covers existing bugs.

Resource limits are 12 rounds, 2 MB of accumulated review material, 1 MB per
requested file, and a 75-minute workflow timeout. Reaching a budget, an API
error, an invalid response, or unfinished investigation fails the status instead
of claiming success. Findings from completed rounds remain in the incomplete
report as provisional findings. Unavailable ranges are reported to the model
explicitly. Symlinks, submodules, generated files, actual `.env` files, and
private-key containers cannot be read through the file-request mechanism;
binary and non-UTF-8 contents are not supplied. Source content remains untrusted
regardless of whether it is presented in a diff or a requested file.

The review creates a **Review started** PR comment before reading the diff or
calling OpenAI, updates that same comment during investigation, and replaces it
with the final result or failure explanation. Findings use a severity count
table and collapsible sections with a location, impact, and suggested fix.
High and critical findings fail the status; medium and low findings remain
visible. The full sweep also posts a start comment and uses the same finding
format. Reports are available in the job summary and workflow artifact.

PR checks use trusted **base-branch** tooling. A PR targeting `work` uses the
version on `work`; a PR targeting `main` continues to use the version on `main`
until that branch receives the update. The full sweep's automatic `workflow_run`
uses tooling on the default branch; manually dispatching it on an updated branch
uses that branch's tooling. Publishing to feature branches alone does not
change tooling used by PRs targeting an unchanged base branch.

Validate locally without an API key or live requests:

```sh
python3 -m unittest discover -s .github/tests -v
actionlint .github/workflows/*.yml
```

The diff's existing 300 KB limit still applies. Additional context and multiple
high-effort requests increase review costs. AI findings supplement deterministic
CI and human review; they cannot guarantee that every bug is found.
