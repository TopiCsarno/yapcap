# Triage Labels

The repo uses this triage state vocabulary. For local markdown issues, store the current state in the issue frontmatter as `status: <state>`.

- `needs-info`: waiting on more information before the issue can be acted on.
- `ready-for-agent`: fully specified and ready for an AFK agent.
- `ready-for-human`: ready for human implementation.
- `wontfix`: will not be actioned.

There is no `needs-triage` step — this is a solo project. New issues should be filed directly as `ready-for-agent` or `ready-for-human`. Use `needs-info` only when genuinely blocked on missing information.

Each issue should have exactly one current state. Skills should update the `status` field exactly when moving issues through the triage flow.

For issue types, use:

- `AFK`: can be implemented by an agent without human interaction once it is `ready-for-agent`.
- `HITL`: requires human interaction, manual validation, or a design decision during implementation.
