#!/usr/bin/env python3
"""Hit Cursor token refresh and cookie-authenticated Cursor web API (same shapes as yapcap Cursor provider).

Writes in this directory:
  oauth_token_response.json   POST token refresh on api2.cursor.sh
  usage_summary_response.json GET /api/usage-summary on cursor.com
  auth_me_response.json       GET /api/auth/me on cursor.com

Captures may contain secrets; do not publish unredacted files.

Credentials (later wins per field: file, then environment):
  YapCap state: $XDG_STATE_HOME/yapcap/cursor-accounts/<account>/tokens.json
    token_id builds WorkosCursorSessionToken per refresh.rs when present; otherwise
    access_token alone is sent as Cookie (legacy path).

  Environment (optional overrides):
    CURSOR_REFRESH_TOKEN or YAPCAP_CURSOR_REFRESH_TOKEN
    CURSOR_ACCESS_TOKEN or YAPCAP_CURSOR_ACCESS_TOKEN
    CURSOR_TOKEN_ID / YAPCAP_CURSOR_TOKEN_ID (WorkosCursorSessionToken prefix when set)
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

CLIENT_ID = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB"
TOKEN_URL = "https://api2.cursor.sh/oauth/token"
USAGE_SUMMARY_URL = "https://cursor.com/api/usage-summary"
AUTH_ME_URL = "https://cursor.com/api/auth/me"

TOKEN_RESPONSE_FILE = "oauth_token_response.json"
USAGE_SUMMARY_RESPONSE_FILE = "usage_summary_response.json"
AUTH_ME_RESPONSE_FILE = "auth_me_response.json"
TOKENS_NAMES = ("tokens.json", "token.json")


def _default_cursor_accounts_dir() -> Path:
    xdg = os.environ.get("XDG_STATE_HOME", "").strip()
    base = Path(xdg) if xdg else Path.home() / ".local" / "state"
    return base / "yapcap" / "cursor-accounts"


def _pick_tokens_file(
    cursor_accounts_dir: Path,
    account: str | None,
) -> Path:
    if account:
        for name in TOKENS_NAMES:
            candidate = cursor_accounts_dir / account / name
            if candidate.is_file():
                return candidate
        msg = f"no tokens.json or token.json under {cursor_accounts_dir / account}"
        raise FileNotFoundError(msg)

    if not cursor_accounts_dir.is_dir():
        msg = f"cursor accounts directory missing: {cursor_accounts_dir}"
        raise FileNotFoundError(msg)

    hits: list[Path] = []
    for child in sorted(cursor_accounts_dir.iterdir()):
        if not child.is_dir():
            continue
        for name in TOKENS_NAMES:
            candidate = child / name
            if candidate.is_file():
                hits.append(candidate)
                break
    if not hits:
        msg = f"no account tokens under {cursor_accounts_dir}"
        raise FileNotFoundError(msg)
    if len(hits) > 1:
        ids = [p.parent.name for p in hits]
        raise OSError(
            f"multiple Cursor accounts {ids}; pass --account <directory-name>"
        )
    return hits[0]


def _read_cursor_tokens(path: Path) -> tuple[str | None, str | None, str | None]:
    data = json.loads(path.read_text(encoding="utf-8"))
    rr = data.get("refresh_token")
    aa = data.get("access_token")
    tt = data.get("token_id")
    refresh = rr.strip() if isinstance(rr, str) and rr.strip() else None
    access = aa.strip() if isinstance(aa, str) and aa.strip() else None
    token_id = tt.strip() if isinstance(tt, str) and tt.strip() else None
    return refresh, access, token_id


def cookie_header(access_token: str, token_id: str | None) -> str:
    if token_id:
        return f"WorkosCursorSessionToken={token_id}%3A%3A{access_token}"
    return access_token


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


def probe_refresh(refresh_token: str) -> dict[str, Any]:
    payload = json.dumps(
        {
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh_token,
        }
    ).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    status, resp_headers, text = _request_json_record(
        method="POST",
        url=TOKEN_URL,
        headers=headers,
        body=payload,
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


def probe_cookie_get(url: str, tag: str, cookie_value: str) -> dict[str, Any]:
    headers = {"Cookie": cookie_value}
    status, resp_headers, text = _request_json_record(
        method="GET",
        url=url,
        headers=headers,
        body=None,
    )
    parsed: dict[str, Any] | list[Any] | str | None = None
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": tag,
        "method": "GET",
        "requested_at": _iso_now(),
        "url": url,
        "status_code": status,
        "response_headers": resp_headers,
        "body_text": text,
        "body_json": parsed,
    }


def main() -> int:
    default_out = Path(__file__).resolve().parent
    default_accounts = _default_cursor_accounts_dir()

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--out-dir",
        type=Path,
        default=default_out,
        help=f"Directory for JSON recordings (default: {default_out})",
    )
    p.add_argument(
        "--cursor-accounts-dir",
        type=Path,
        default=default_accounts,
        help=f"YapCap cursor-accounts root (default: {default_accounts})",
    )
    p.add_argument(
        "--tokens-file",
        type=Path,
        default=None,
        help="Explicit tokens.json (or token.json) path",
    )
    p.add_argument(
        "--account",
        default=None,
        metavar="ID",
        help="Account subdirectory under cursor-accounts (cursor-…)",
    )
    p.add_argument(
        "--no-local-state",
        action="store_true",
        help="Do not load YapCap state; credentials from environment only",
    )
    p.add_argument(
        "--skip-refresh",
        action="store_true",
        help="Skip POST token; use access_token (+ token_id when set) only",
    )
    p.add_argument(
        "--token-only",
        action="store_true",
        help="Only POST token endpoint",
    )
    args = p.parse_args()
    out_dir: Path = args.out_dir

    refresh: str | None = None
    access: str | None = None
    token_id: str | None = None

    if not args.no_local_state:
        try:
            if args.tokens_file is not None:
                tf = args.tokens_file
                if not tf.is_file():
                    print(f"error: --tokens-file not found: {tf}", file=sys.stderr)
                    return 1
            else:
                tf = _pick_tokens_file(args.cursor_accounts_dir, args.account)
            refresh, access, token_id = _read_cursor_tokens(tf)
            print(f"using tokens from {tf}", file=sys.stderr)
        except FileNotFoundError as e:
            if args.tokens_file is not None or args.account is not None:
                print(f"error: {e}", file=sys.stderr)
                return 1
        except OSError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1

    refresh = _env("CURSOR_REFRESH_TOKEN", "YAPCAP_CURSOR_REFRESH_TOKEN") or refresh
    access = _env("CURSOR_ACCESS_TOKEN", "YAPCAP_CURSOR_ACCESS_TOKEN") or access
    token_id = _env("CURSOR_TOKEN_ID", "YAPCAP_CURSOR_TOKEN_ID") or token_id

    rec: dict[str, Any] | None = None

    try:
        if not args.skip_refresh:
            if not refresh:
                print(
                    "error: no refresh_token (use YapCap Cursor account or set "
                    "CURSOR_REFRESH_TOKEN / YAPCAP_CURSOR_REFRESH_TOKEN; "
                    "or pass --skip-refresh with access token)",
                    file=sys.stderr,
                )
                return 1
            rec = probe_refresh(refresh)
            _save(out_dir, TOKEN_RESPONSE_FILE, rec)
            print(f"wrote {out_dir / TOKEN_RESPONSE_FILE}", file=sys.stderr)
            if isinstance(rec.get("body_json"), dict):
                at = rec["body_json"].get("access_token")
                if isinstance(at, str) and at:
                    access = at

        if args.token_only:
            if rec is None:
                print(
                    "error: --token-only implies refresh; omit --skip-refresh",
                    file=sys.stderr,
                )
                return 2
            return 0 if int(rec["status_code"]) < 400 else 1

        if args.skip_refresh and not access:
            print(
                "error: --skip-refresh needs CURSOR_ACCESS_TOKEN or stored access_token",
                file=sys.stderr,
            )
            return 1

        if not access:
            print(
                "error: no access_token (refresh missing access_token field or stored token)",
                file=sys.stderr,
            )
            return 1

        cookie = cookie_header(access, token_id)

        usr = probe_cookie_get(
            USAGE_SUMMARY_URL, "usage_summary", cookie
        )
        path_u = _save(out_dir, USAGE_SUMMARY_RESPONSE_FILE, usr)
        print(f"wrote {path_u}", file=sys.stderr)

        me = probe_cookie_get(AUTH_ME_URL, "auth_me", cookie)
        path_m = _save(out_dir, AUTH_ME_RESPONSE_FILE, me)
        print(f"wrote {path_m}", file=sys.stderr)

        ok = int(usr["status_code"]) < 400 and int(me["status_code"]) < 400
        return 0 if ok else 1
    except URLError as e:
        print(f"error: request failed: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
