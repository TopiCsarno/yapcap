---
status: done
type: AFK
blocked_by: []
---

# Fresh COSMIC Config Schema

## Parent

[QA Release Polish PRD](../PRD.md)

## What to build

Bump YapCap's COSMIC config schema for the next patch release so users start from a fresh app config, while leaving old YapCap account, cache, and log state untouched. The product spec and QA plan should explicitly describe this as a deliberate fresh-start boundary rather than data cleanup.

## Acceptance criteria

- [ ] The COSMIC config schema version is bumped to the new patch schema version.
- [ ] Startup does not automatically delete old account, cache, or log directories.
- [ ] Documentation states that old state may remain orphaned and users must re-add accounts after the fresh config boundary.
- [ ] Tests or static checks cover the new config schema version/default behavior where practical.

## Blocked by

None - can start immediately.

## Notes

2026-05-06T11:36:09+02:00 - Completed. Bumped COSMIC config schema to v400, documented the fresh-start boundary and orphaned old state behavior, and added focused coverage for the new schema/default account state.
