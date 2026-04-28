# Repository Guidance

- Files should be ~300 lines as a soft rule; split large source files when there is a clear boundary.
- Functions should fit on one screen; split large functions around behavior.
- Do not add clippy exceptions by default. Prefer changing code, visibility, tests, or module structure. If needed, keep the exception narrow and justify it in the commit, PR, or final handoff.
- Do not add comments in source code. Prefer clear names, smaller functions, and tests over inline explanations.
- Remove existing comments from touched source code when they are no longer needed. MPL-2.0 license headers (`// SPDX-License-Identifier: MPL-2.0`) are fine and should be kept.
- Do not edit the template `justfile` unless explicitly asked.
- Before committing, update `docs/spec.md` when behavior or user-facing expectations change.
- Before committing, run `just check` and `cargo test` and `cargo fmt`, then fix all warnings, errors, and failures.
- Do not add agent or AI attribution to commit messages (no `Co-Authored-By: Claude` or similar).
