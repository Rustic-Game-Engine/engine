# Repository layout

`Engine/` contains the Rust game engine workspace and its engine-specific instructions.
Run engine commands from that folder; the repository root is not a Cargo project.

The Next.js documentation site lives at the root of the separate
[docs repository](https://github.com/Rustic-Game-Engine/docs). Its lint, build,
and deployment workflows run there.

# Documentation requirement for engine changes

Whenever you change a script function, callback, Script API behavior, or other engine functionality, update the affected **website documentation** in the same task. In the docs repository, the website serves pages from `docs/` through `lib/docs.ts` and generates API pages from `lib/api-docs.ts`; update the relevant sources and ensure new guides are reachable through `lib/docs-catalog.ts`. Keep corresponding engine-local documentation in `Engine/docs` accurate too. Include the companion docs pull request in the engine pull request when the change affects published documentation. Write for a first-time user: state what works, where it works, exact setup and attachment steps, copyable examples, expected results, current limitations, and how to diagnose common failures. Correct older claims that the change makes inaccurate. Check the website content against the implemented behavior before reporting the work complete.

# Pull requests and labels

When a feature is complete, finish the relevant validation and documentation, commit the changes on a feature branch, push it, and create a pull request against the branch the work started from. If the task already has a pull request, update it instead of creating a duplicate. Include a concise summary and the checks run in the PR description, then return the PR link to the user.

Before reporting the task complete, apply at least one change-type label and every relevant area label. Choose labels that describe the actual changes:

| Change type | Use for |
| --- | --- |
| `type:feature` | New functionality |
| `type:fix` | Bug fixes |
| `type:docs` | Documentation changes |
| `type:refactor` | Restructuring without changing behavior |
| `type:test` | Test additions or changes |
| `type:chore` | Maintenance, dependencies, or tooling |

| Area | Use for |
| --- | --- |
| `area:engine` | Changes to `Engine/`, including engine documentation |
| `area:website` | Changes to the documentation site (including its removal from this repository) |
| `area:ci` | GitHub Actions, CI, or repository automation |

For example, label an engine feature with `type:feature` and `area:engine`; add `type:docs` when its documentation changes, and `area:website` if the task also changes the documentation site. A root-level instructions-only change uses `type:docs` and needs no area label.

Use `gh pr create` / `gh pr edit --add-label`, or equivalent GitHub tools. If authentication or permissions prevent pushing, opening the PR, or labeling it, report the blocker and the remaining step explicitly.
