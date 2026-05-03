#!/usr/bin/env python3
"""Hit OpenAI/Codex OAuth token and ChatGPT usage endpoints (same shapes as yapcap Codex provider).

Writes oauth_token_response.json and oauth_usage_response.json in this directory.
Captures may contain secrets; do not publish unredacted files.

Credentials (later wins per field: file, then environment):
  YapCap state: $XDG_STATE_HOME/yapcap/codex-accounts/<account-id>/tokens.json
    (falls back to ~/.local/state when XDG_STATE_HOME is unset).
    Optional ChatGPT-Account-Id for usage: provider_account_id from sibling metadata.json.
  Environment (optional overrides):
    CODEX_REFRESH_TOKEN or YAPCAP_CODEX_REFRESH_TOKEN
    CODEX_ACCESS_TOKEN or YAPCAP_CODEX_ACCESS_TOKEN
    CHATGPT_ACCOUNT_ID or YAPCAP_CHATGPT_ACCOUNT_ID (optional; matches ChatGPT-Account-Id header)
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
from urllib.parse import urlencode
from urllib.request import Request, urlopen

CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"
TOKEN_URL = "https://auth.openai.com/oauth/token"
USAGE_URL = "https://chatgpt.com/backend-api/wham/usage"

TOKEN_RESPONSE_FILE = "oauth_token_response.json"
USAGE_RESPONSE_FILE = "oauth_usage_response.json"
TOKENS_NAMES = ("tokens.json", "token.json")


def _default_codex_accounts_dir() -> Path:
    xdg = os.environ.get("XDG_STATE_HOME", "").strip()
    base = Path(xdg) if xdg else Path.home() / ".local" / "state"
    return base / "yapcap" / "codex-accounts"


def _pick_tokens_file(
    codex_accounts_dir: Path,
    account: str | None,
) -> Path:
    if account:
        for name in TOKENS_NAMES:
            candidate = codex_accounts_dir / account / name
            if candidate.is_file():
                return candidate
        msg = f"no tokens.json or token.json under {codex_accounts_dir / account}"
        raise FileNotFoundError(msg)

    if not codex_accounts_dir.is_dir():
        msg = f"codex accounts directory missing: {codex_accounts_dir}"
        raise FileNotFoundError(msg)

    hits: list[Path] = []
    for child in sorted(codex_accounts_dir.iterdir()):
        if not child.is_dir():
            continue
        for name in TOKENS_NAMES:
            candidate = child / name
            if candidate.is_file():
                hits.append(candidate)
                break
    if not hits:
        msg = f"no account tokens under {codex_accounts_dir}"
        raise FileNotFoundError(msg)
    if len(hits) > 1:
        ids = [p.parent.name for p in hits]
        raise OSError(
            f"multiple Codex accounts {ids}; pass --account <directory-name>"
        )
    return hits[0]


def _read_stored_tokens(path: Path) -> tuple[str | None, str | None]:
    data = json.loads(path.read_text(encoding="utf-8"))
    rr = data.get("refresh_token")
    aa = data.get("access_token")
    refresh = rr.strip() if isinstance(rr, str) and rr.strip() else None
    access = aa.strip() if isinstance(aa, str) and aa.strip() else None
    return refresh, access


def _provider_account_id_from_metadata(tokens_file: Path) -> str | None:
    meta = tokens_file.parent / "metadata.json"
    if not meta.is_file():
        return None
    data = json.loads(meta.read_text(encoding="utf-8"))
    pid = data.get("provider_account_id")
    if isinstance(pid, str) and pid.strip():
        return pid.strip()
    return None


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
    body = urlencode(
        {
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh_token,
        }
    ).encode("utf-8")
    headers = {
        "Content-Type": "application/x-www-form-urlencoded",
        "Accept": "application/json",
    }
    status, resp_headers, text = _request_json_record(
        method="POST",
        url=TOKEN_URL,
        headers=headers,
        body=body,
    )
    parsed: dict[str, Any] | list[Any] | str | None = None
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": "oauth_token",
        "method": "POST",
        "requested_at": _iso_now(),
        "url": TOKEN_URL,
        "status_code": status,
        "response_headers": resp_headers,
        "body_text": text,
        "body_json": parsed,
    }


def probe_usage(access_token: str, chatgpt_account_id: str | None) -> dict[str, Any]:
    headers: dict[str, str] = {
        "Authorization": f"Bearer {access_token}",
    }
    if chatgpt_account_id:
        headers["ChatGPT-Account-Id"] = chatgpt_account_id
    status, resp_headers, text = _request_json_record(
        method="GET",
        url=USAGE_URL,
        headers=headers,
        body=None,
    )
    parsed: dict[str, Any] | list[Any] | str | None = None
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": "wham_usage",
        "method": "GET",
        "requested_at": _iso_now(),
        "url": USAGE_URL,
        "status_code": status,
        "response_headers": resp_headers,
        "body_text": text,
        "body_json": parsed,
    }


def main() -> int:
    default_out = Path(__file__).resolve().parent
    default_accounts = _default_codex_accounts_dir()

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--out-dir",
        type=Path,
        default=default_out,
        help=f"Directory for JSON recordings (default: {default_out})",
    )
    p.add_argument(
        "--codex-accounts-dir",
        type=Path,
        default=default_accounts,
        help=f"YapCap codex-accounts root (default: {default_accounts})",
    )
    p.add_argument(
        "--tokens-file",
        type=Path,
        default=None,
        help="Use this tokens.json (or token.json) instead of discovering",
    )
    p.add_argument(
        "--account",
        default=None,
        metavar="ID",
        help="Account subdirectory name under codex-accounts (codex-…)",
    )
    p.add_argument(
        "--no-local-state",
        action="store_true",
        help="Do not read tokens or metadata from YapCap state; use environment only",
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
    tokens_path: Path | None = None
    chatgpt_id: str | None = None

    if not args.no_local_state:
        try:
            if args.tokens_file is not None:
                tf = args.tokens_file
                if not tf.is_file():
                    print(f"error: --tokens-file not found: {tf}", file=sys.stderr)
                    return 1
                tokens_path = tf
            else:
                tokens_path = _pick_tokens_file(args.codex_accounts_dir, args.account)
            refresh, access = _read_stored_tokens(tokens_path)
            chatgpt_id = _provider_account_id_from_metadata(tokens_path)
            print(f"using tokens from {tokens_path}", file=sys.stderr)
        except FileNotFoundError as e:
            if args.tokens_file is not None or args.account is not None:
                print(f"error: {e}", file=sys.stderr)
                return 1
            tokens_path = None
        except OSError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1

    refresh = (
        _env("CODEX_REFRESH_TOKEN", "YAPCAP_CODEX_REFRESH_TOKEN") or refresh
    )
    access = _env("CODEX_ACCESS_TOKEN", "YAPCAP_CODEX_ACCESS_TOKEN") or access
    chatgpt_id = (
        _env("CHATGPT_ACCOUNT_ID", "YAPCAP_CHATGPT_ACCOUNT_ID") or chatgpt_id
    )

    rec: dict[str, Any] | None = None

    try:
        if not args.usage_only:
            if not refresh:
                print(
                    "error: no refresh_token (log in with YapCap or set "
                    "CODEX_REFRESH_TOKEN / YAPCAP_CODEX_REFRESH_TOKEN)",
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
                "CODEX_ACCESS_TOKEN / YAPCAP_CODEX_ACCESS_TOKEN",
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
                "set CODEX_ACCESS_TOKEN or check token response)",
                file=sys.stderr,
            )
            return 1

        urec = probe_usage(access, chatgpt_id)
        upath = _save(out_dir, USAGE_RESPONSE_FILE, urec)
        print(f"wrote {upath}", file=sys.stderr)
        ok = int(urec["status_code"]) < 400
        return 0 if ok else 1
    except URLError as e:
        print(f"error: request failed: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
