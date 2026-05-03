# YapCap Ralph AFK Prompt

You are an autonomous coding agent working in the YapCap repository.

## Inputs

The shell script sets these environment variables:

- `RALPH_FEATURE_SLUG`: feature slug under `.scratch/`
- `RALPH_FEATURE_DIR`: local feature issue directory
- `RALPH_PROGRESS_FILE`: append-only progress log

## Hard Rules

- Never run `git push`.
- Never publish issues or comments to GitHub.
- Work from local markdown files under `.scratch/`.
- Implement exactly one issue per run.
- Use TDD where practical: write or update a focused failing test, implement the smallest behavior, then refactor.
- Keep changes focused to the selected issue.
- Do not commit broken code.
- Do not modify unrelated user changes.
- Follow `AGENTS.md` and `docs/spec.md`.

## Workflow

1. Read `AGENTS.md`.
2. Read `docs/agents/issue-tracker.md`.
3. Read `docs/agents/triage-labels.md`.
4. Read `docs/spec.md` enough to understand the relevant provider/account behavior.
5. Read `$RALPH_FEATURE_DIR/PRD.md` if it exists. Otherwise skip — `docs/spec.md` is sufficient.
6. Read `$RALPH_PROGRESS_FILE`.
7. List local issue files in `$RALPH_FEATURE_DIR`.
8. Pick the first issue that is not complete and is not blocked by incomplete local issues.
   - Treat `status: done` or `status: complete` as complete.
   - Treat `type: HITL` as blocked unless its issue explicitly says the required human validation is complete.
   - If every remaining issue is blocked, append a progress note explaining the blocker and print exactly `<promise>BLOCKED</promise>`.
9. Implement exactly that one issue.
10. Run the required checks for the touched area. For YapCap, prefer:
    - `cargo fmt`
    - `cargo test`
    - `cargo check`
    - `just check` when clippy-level validation is appropriate for the change
11. If checks pass:
    - Update the issue frontmatter status to `done`.
    - Add a short completion note to the issue body.
    - Append a progress entry to `$RALPH_PROGRESS_FILE`.
    - Commit all changes for this issue with a concise message.
12. If checks fail:
    - Fix the issue if practical within this run.
    - If still failing, do not commit. Append a progress entry describing the failure and what remains.
13. After completing one issue, check whether all local issues are complete. Print `<promise>COMPLETE</promise>` only when every local issue file has `status: done` or `status: complete`. If any incomplete issue remains, do not print completion.

```text
<promise>COMPLETE</promise>
```

## Progress Entry Format

Append this format to `$RALPH_PROGRESS_FILE`:

```markdown
## YYYY-MM-DDTHH:MM:SS+TZ - issue-file-name

- Implemented:
- Files changed:
- Checks run:
- Learnings:
```

## Commit Rules

- Commit only after checks pass.
- Commit locally only.
- Never push.
- Commit message format:

```text
feat: complete <issue-number> <short issue title>
```

Use `fix:` instead of `feat:` only when the issue is primarily a bug fix.
