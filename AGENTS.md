# Repository Guidance

- Files should be ~300 lines (soft rule), break up large surce files. 
- Functions should fit on one screen, break up large functions
- Treat `~/projects/yapcap_old` as proof-of-concept reference material, not target architecture.
- Preserve intentional user-facing behavior, but refactor boundaries during the port when the template shape is cleaner.
- Do not add comments in source code. Prefer clear names, smaller functions, and tests over inline explanations.
- Remove existing comments from source code.
- Do not edit the template `justfile` unless explicitly asked.
- Leave changes uncommitted until they are reviewed and explicitly reviewed.
