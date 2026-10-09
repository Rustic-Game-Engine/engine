# Repository layout

This repository is split into two independent project folders:

- `Engine/` contains the Rust game engine workspace and its engine-specific instructions.
- `Website/` contains the Next.js documentation website.

Run commands from the project folder they apply to. Do not assume the repository root is a Cargo or Node.js project, and do not move either project back to the root.

# Documentation requirement for engine changes

Whenever you change a script function, callback, Script API behavior, or other engine functionality, update the affected **website documentation** in the same task. The website serves pages from `Engine/docs` through `Website/lib/docs.ts` and also generates API pages from `Website/lib/api-docs.ts`; update the relevant sources and ensure new guides are reachable through `Website/lib/docs-catalog.ts`. Write for a first-time user: state what works, where it works, exact setup and attachment steps, copyable examples, expected results, current limitations, and how to diagnose common failures. Correct older claims that the change makes inaccurate. Check the website content against the implemented behavior before reporting the work complete.
