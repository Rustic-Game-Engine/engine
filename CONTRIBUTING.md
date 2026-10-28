# Contributing to Rustic Engine

Thanks for your interest in contributing to Rustic Engine! Bug reports, documentation improvements, tests, and code contributions are welcome.

## Getting Started

1. Fork the repository and clone your fork.
2. Follow the setup instructions in the README.
3. Create a branch for your changes:

   git checkout -b your-change-name

Before starting a major feature or architectural change, open an issue to discuss the approach with the maintainers.

## Reporting Bugs

Search existing issues before opening a new report. Include:

- A clear description of the problem.
- Steps to reproduce it, ideally with a minimal example.
- Expected and actual behavior.
- Your operating system, relevant hardware, and engine version or commit.
- Relevant logs, error messages, or screenshots.

For rendering problems, include your GPU model and graphics driver version.

## Suggesting Features

Explain the problem your proposal solves, how it would be used, and any alternatives you considered. Small examples or sketches can help clarify the idea.

## Making Changes

- Keep each pull request focused on one change.
- Follow the conventions of the surrounding code.
- Prefer clear, maintainable solutions.
- Document public APIs and behavior changes.
- Add or update tests when changing behavior or fixing bugs.
- Avoid unrelated formatting changes and new dependencies without a clear reason.
- Explain safety requirements for any new `unsafe` code.
- Include measurements when claiming a performance improvement.

## Checking Your Work

Follow any repository-specific checks documented in the README or CI configuration.

For Rust code, run the applicable formatting, linting, and test checks:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets
    cargo test --workspace

Some components may require additional system dependencies, graphics support, or feature flags. Document any checks you could not run and explain why.

For changes that affect rendering, input, audio, or other interactive behavior, also test the affected example or application manually.

## Opening a Pull Request

Include:

- What changed and why.
- Any related issue, such as `Closes #123`.
- How you tested the change.
- Screenshots or recordings for visible changes.
- Any compatibility concerns, limitations, or follow-up work.

Keep the description up to date as the implementation changes. Address review feedback and ask for clarification when needed.

## Documentation Contributions

Documentation fixes are welcome, including typo corrections, clearer explanations, working examples, and setup instructions. Check that commands and code examples match the current engine behavior.

## Security Reports

Follow `SECURITY.md` if the repository provides one. Otherwise, contact the maintainers privately before publicly sharing details of a potential vulnerability.

## Community Expectations

Be respectful, constructive, and patient. Focus feedback on the work, explain your reasoning, and help keep discussions welcoming to contributors of all experience levels.

Follow the repository’s Code of Conduct where provided.

## Licensing

By submitting a contribution, you agree that it may be distributed under the repository’s license. Only submit work you have permission to contribute, and preserve required third-party notices.
