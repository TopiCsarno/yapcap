#!/usr/bin/env python3
"""Hit Anthropic OAuth usage and token endpoints (same URLs and shapes as yapcap Claude provider).

Writes recordings into this directory as oauth_token_response.json and oauth_usage_response.json.
Output JSON may contain OAuth tokens; do not publish or commit unredacted captures.

Credentials (later wins per field: file, then environment):
  YapCap state: $XDG_STATE_HOME/yapcap/claude-accounts/<account-id>/tokens.json
    (same layout as the app; falls back to ~/.local/state if XDG_STATE_HOME is unset).
    Also accepts token.json in that directory.
  Environment (optional overrides):
    CLAUDE_REFRESH_TOKEN or YAPCAP_CLAUDE_REFRESH_TOKEN
    CLAUDE_ACCESS_TOKEN or YAPCAP_CLAUDE_ACCESS_TOKEN
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
USER_AGENT = "claude-code/2.0.32"
TOKEN_URL = "https://console.anthropic.com/v1/oauth/token"
USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
BETA_HEADER = "oauth-2025-04-20"

TOKEN_RESPONSE_FILE = "oauth_token_response.json"
USAGE_RESPONSE_FILE = "oauth_usage_response.json"
TOKENS_NAMES = ("tokens.json", "token.json")


def _default_claude_accounts_dir() -> Path:
    xdg = os.environ.get("XDG_STATE_HOME", "").strip()
    base = Path(xdg) if xdg else Path.home() / ".local" / "state"
    return base / "yapcap" / "claude-accounts"


def _pick_tokens_file(
    claude_accounts_dir: Path,
    account: str | None,
) -> Path:
    if account:
        for name in TOKENS_NAMES:
            candidate = claude_accounts_dir / account / name
            if candidate.is_file():
                return candidate
        msg = f"no tokens.json or token.json under {claude_accounts_dir / account}"
        raise FileNotFoundError(msg)

    if not claude_accounts_dir.is_dir():
        msg = f"claude accounts directory missing: {claude_accounts_dir}"
        raise FileNotFoundError(msg)

    hits: list[Path] = []
    for child in sorted(claude_accounts_dir.iterdir()):
        if not child.is_dir():
            continue
        for name in TOKENS_NAMES:
            candidate = child / name
            if candidate.is_file():
                hits.append(candidate)
                break
    if not hits:
        msg = f"no account tokens under {claude_accounts_dir}"
        raise FileNotFoundError(msg)
    if len(hits) > 1:
        ids = [p.parent.name for p in hits]
        raise OSError(
            f"multiple Claude accounts {ids}; pass --account <directory-name>"
        )
    return hits[0]


def _read_stored_tokens(path: Path) -> tuple[str | None, str | None]:
    data = json.loads(path.read_text(encoding="utf-8"))
    rr = data.get("refresh_token")
    aa = data.get("access_token")
    refresh = rr.strip() if isinstance(rr, str) and rr.strip() else None
    access = aa.strip() if isinstance(aa, str) and aa.strip() else None
    return refresh, access


def _env(*names: str) -> str | None:
    for name in names:
        v = os.environ.get(name)
        if v:
            return v.strip()
    return None


def _iso_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _headers_to_dict(msg: Any) -> dict[str, str]:
    out: dict[str, str] = {}
    for k, v in msg.items():
        out[k] = v
    return out


def _save(out_dir: Path, name: str, record: dict[str, Any]) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / name
    path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def _request_json_record(
    *,
    method: str,
    url: str,
    headers: dict[str, str],
    body: bytes | None,
) -> tuple[int, dict[str, str], str]:
    req = Request(url, data=body, method=method)
    for k, v in headers.items():
        req.add_header(k, v)
    try:
        with urlopen(req, timeout=120) as resp:
            status = getattr(resp, "status", resp.getcode())
            raw = resp.read().decode("utf-8", errors="replace")
            hdrs = _headers_to_dict(resp.headers)
            return int(status), hdrs, raw
    except HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        hdrs = _headers_to_dict(e.headers) if e.headers else {}
        return int(e.code), hdrs, raw


def probe_token(refresh_token: str) -> dict[str, Any]:
    payload = json.dumps(
        {
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        }
    ).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "User-Agent": USER_AGENT,
        "Accept": "application/json",
    }
    status, resp_headers, body = _request_json_record(
        method="POST",
        url=TOKEN_URL,
        headers=headers,
        body=payload,
    )
    parsed: dict[str, Any] | list[Any] | str | None = None
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": "oauth_token",
        "method": "POST",
        "requested_at": _iso_now(),
        "url": TOKEN_URL,
        "status_code": status,
        "response_headers": resp_headers,
        "body_text": body,
        "body_json": parsed,
    }


def probe_usage(access_token: str) -> dict[str, Any]:
    headers = {
        "Authorization": f"Bearer {access_token}",
        "anthropic-beta": BETA_HEADER,
        "User-Agent": USER_AGENT,
        "Accept": "application/json",
    }
    status, resp_headers, body = _request_json_record(
        method="GET",
        url=USAGE_URL,
        headers=headers,
        body=None,
    )
    parsed: dict[str, Any] | list[Any] | str | None = None
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": "oauth_usage",
        "method": "GET",
        "requested_at": _iso_now(),
        "url": USAGE_URL,
        "status_code": status,
        "response_headers": resp_headers,
        "body_text": body,
        "body_json": parsed,
    }


def main() -> int:
    default_out = Path(__file__).resolve().parent
    default_accounts = _default_claude_accounts_dir()

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--out-dir",
        type=Path,
        default=default_out,
        help=f"Directory for JSON recordings (default: {default_out})",
    )
    p.add_argument(
        "--claude-accounts-dir",
        type=Path,
        default=default_accounts,
        help=f"YapCap claude-accounts root (default: {default_accounts})",
    )
    p.add_argument(
        "--tokens-file",
        type=Path,
        default=None,
        help="Use this tokens.json (or token.json) instead of discovering under --claude-accounts-dir",
    )
    p.add_argument(
        "--account",
        default=None,
        metavar="ID",
        help="Account subdirectory name under claude-accounts (claude-…)",
    )
    p.add_argument(
        "--no-local-state",
        action="store_true",
        help="Do not read tokens from YapCap state; use environment only",
    )
    p.add_argument("--token-only", action="store_true", help="Only POST token endpoint")
    p.add_argument("--usage-only", action="store_true", help="Only GET usage endpoint")
    args = p.parse_args()
    out_dir: Path = args.out_dir

    if args.token_only and args.usage_only:
        print("error: use at most one of --token-only and --usage-only", file=sys.stderr)
        return 2

    refresh: str | None = None
    access: str | None = None

    if not args.no_local_state:
        try:
            if args.tokens_file is not None:
                tf = args.tokens_file
                if not tf.is_file():
                    print(f"error: --tokens-file not found: {tf}", file=sys.stderr)
                    return 1
            else:
                tf = _pick_tokens_file(args.claude_accounts_dir, args.account)
            refresh, access = _read_stored_tokens(tf)
            print(f"using tokens from {tf}", file=sys.stderr)
        except FileNotFoundError as e:
            if args.tokens_file is not None or args.account is not None:
                print(f"error: {e}", file=sys.stderr)
                return 1
        except OSError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1

    refresh = _env("CLAUDE_REFRESH_TOKEN", "YAPCAP_CLAUDE_REFRESH_TOKEN") or refresh
    access = _env("CLAUDE_ACCESS_TOKEN", "YAPCAP_CLAUDE_ACCESS_TOKEN") or access

    rec: dict[str, Any] | None = None

    try:
        if not args.usage_only:
            if not refresh:
                print(
                    "error: no refresh_token (log in with YapCap or set "
                    "CLAUDE_REFRESH_TOKEN / YAPCAP_CLAUDE_REFRESH_TOKEN)",
                    file=sys.stderr,
                )
                return 1
            rec = probe_token(refresh)
            path = _save(out_dir, TOKEN_RESPONSE_FILE, rec)
            print(f"wrote {path}", file=sys.stderr)
            if isinstance(rec.get("body_json"), dict):
                at = rec["body_json"].get("access_token")
                if isinstance(at, str) and at:
                    access = at
        elif not access:
            print(
                "error: --usage-only needs access_token in state or "
                "CLAUDE_ACCESS_TOKEN / YAPCAP_CLAUDE_ACCESS_TOKEN",
                file=sys.stderr,
            )
            return 1

        if args.token_only:
            if rec is None:
                return 2
            return 0 if int(rec["status_code"]) < 400 else 1

        if not access:
            print(
                "error: no access token (token response missing access_token; "
                "ensure state tokens.json has access_token or set CLAUDE_ACCESS_TOKEN)",
                file=sys.stderr,
            )
            return 1

        urec = probe_usage(access)
        upath = _save(out_dir, USAGE_RESPONSE_FILE, urec)
        print(f"wrote {upath}", file=sys.stderr)
        ok = int(urec["status_code"]) < 400
        return 0 if ok else 1
    except URLError as e:
        print(f"error: request failed: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
