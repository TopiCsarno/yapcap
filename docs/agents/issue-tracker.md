# Issue Tracker

Issues for this repository live as local markdown files under `issues/`.

Engineering skills that create or update issues should write markdown files under a feature-specific `issues/<feature-slug>/` directory.

Use this layout:

```text
issues/<feature-slug>/
  README.md
  PRD.md
  issues/
    001-short-title.md
    002-short-title.md
```

`PRD.md` is the local PRD. If a PRD already exists elsewhere in the repo, future issue files may link to it, but new PRDs should default to this local path.

Implementation issues live under `issues/`. Each issue should include YAML frontmatter:

```yaml
---
status: needs-triage
type: AFK
blocked_by:
  - 001-short-title
---
```

`status` is the local equivalent of an issue tracker label. Use the configured triage vocabulary from `docs/agents/triage-labels.md`.

When a skill says "publish to the issue tracker", write or update these local markdown files. Do not create GitHub issues unless the user explicitly asks for public GitHub publication in that turn.

When a skill says "apply a label", update the `status` field in the issue frontmatter. If multiple states would apply, keep only one state and describe any nuance in the body.

When a skill says "comment on an issue", append a timestamped `## Notes` entry to the issue file instead of posting to GitHub.

GitHub Issues may be used manually when the maintainer explicitly requests publication, but local markdown is the default tracker for agent-created issues.
