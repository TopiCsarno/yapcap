# Claude OAuth Probe

Scratch-only probe for testing Claude OAuth authorization-code exchange variants before changing product code.

Run:

```bash
python3 .scratch/claude-oauth-probe/probe.py all
```

The script prints an authorization URL for each variant, prompts for the returned `code#state` or callback URL, calls the token endpoint, and writes redacted captures to:

```text
.scratch/claude-oauth-probe/captures/
```

Do not commit raw tokens, raw auth codes, or real emails.

See `findings.md` for the current conclusions from the April 30, 2026 probe run.
