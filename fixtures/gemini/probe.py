#!/usr/bin/env python3
"""Hit Google Code Assist (Gemini) endpoints the same way @google/gemini-cli does.

Writes recordings into this directory as:
  oauth_token_response.json          POST oauth2.googleapis.com/token (refresh_token grant)
  load_code_assist_response.json     POST cloudcode-pa.googleapis.com/v1internal:loadCodeAssist
  retrieve_user_quota_response.json  POST cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota
  oauth_token_400_response.json      POST oauth2.googleapis.com/token with a bogus refresh_token
                                     (only written when --simulate-bad-refresh is passed)
  oauth_token_429_response.json      captured opportunistically if Google rate-limits the probe;
                                     not produced by any flag — rename a recorded 429
                                     oauth_token_response.json into this slot when one occurs.

Output JSON may contain OAuth tokens and account PII; do not publish or commit unredacted captures.

Credentials (later wins per field: file, then environment):
  Gemini CLI state: ~/.gemini/oauth_creds.json
    Fields used: access_token, refresh_token, expiry_date (Unix ms), id_token.
  Environment (optional overrides):
    GEMINI_REFRESH_TOKEN or YAPCAP_GEMINI_REFRESH_TOKEN
    GEMINI_ACCESS_TOKEN  or YAPCAP_GEMINI_ACCESS_TOKEN
    GEMINI_PROJECT_ID    or YAPCAP_GEMINI_PROJECT_ID  (skips loadCodeAssist discovery)

The OAuth client_id / client_secret are the public values embedded in the official
@google/gemini-cli build (extracted from its OAUTH_CLIENT_ID / OAUTH_CLIENT_SECRET
constants). They are the same values used by every gemini-cli installation; this
probe hardcodes them to avoid a scan-the-node_modules step.
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

OAUTH_CLIENT_ID = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com"
OAUTH_CLIENT_SECRET = "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"

TOKEN_URL = "https://oauth2.googleapis.com/token"
LOAD_CODE_ASSIST_URL = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
QUOTA_URL = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota"

USER_AGENT = "GeminiCLI/0.0.0 (linux; x64)"

IDE_METADATA = {
    "ideType": "IDE_UNSPECIFIED",
    "platform": "PLATFORM_UNSPECIFIED",
    "pluginType": "GEMINI",
    "duetProject": "default",
}

TOKEN_RESPONSE_FILE = "oauth_token_response.json"
LOAD_CODE_ASSIST_FILE = "load_code_assist_response.json"
QUOTA_RESPONSE_FILE = "retrieve_user_quota_response.json"
TOKEN_400_RESPONSE_FILE = "oauth_token_400_response.json"
TOKEN_429_RESPONSE_FILE = "oauth_token_429_response.json"

INVALID_REFRESH_TOKEN_PLACEHOLDER = (
    "1//0eINVALID-PROBE-FORCED-INVALID-REFRESH-TOKEN-FOR-ERROR-CAPTURE"
)


def _default_creds_path() -> Path:
    return Path.home() / ".gemini" / "oauth_creds.json"


def _read_stored_creds(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise OSError(f"{path}: expected JSON object")
    return data


def _env(*names: str) -> str | None:
    for name in names:
        v = os.environ.get(name)
        if v:
            return v.strip()
    return None


def _iso_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _headers_to_dict(msg: Any) -> dict[str, str]:
    return {k: v for k, v in msg.items()}


def _save(out_dir: Path, name: str, record: dict[str, Any]) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / name
    path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def _request_record(
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


def _record(endpoint: str, method: str, url: str, status: int, headers: dict[str, str], body: str) -> dict[str, Any]:
    parsed: Any
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        parsed = None
    return {
        "endpoint": endpoint,
        "method": method,
        "requested_at": _iso_now(),
        "url": url,
        "status_code": status,
        "response_headers": headers,
        "body_text": body,
        "body_json": parsed,
    }


def probe_token(refresh_token: str) -> dict[str, Any]:
    form = urlencode({
        "client_id": OAUTH_CLIENT_ID,
        "client_secret": OAUTH_CLIENT_SECRET,
        "refresh_token": refresh_token,
        "grant_type": "refresh_token",
    }).encode("utf-8")
    headers = {
        "Content-Type": "application/x-www-form-urlencoded",
        "Accept": "application/json",
        "User-Agent": USER_AGENT,
    }
    status, resp_headers, body = _request_record(
        method="POST", url=TOKEN_URL, headers=headers, body=form,
    )
    return _record("oauth_token", "POST", TOKEN_URL, status, resp_headers, body)


def probe_load_code_assist(access_token: str) -> dict[str, Any]:
    payload = json.dumps({"metadata": IDE_METADATA}).encode("utf-8")
    headers = {
        "Authorization": f"Bearer {access_token}",
        "Content-Type": "application/json",
        "Accept": "application/json",
        "User-Agent": USER_AGENT,
    }
    status, resp_headers, body = _request_record(
        method="POST", url=LOAD_CODE_ASSIST_URL, headers=headers, body=payload,
    )
    return _record("load_code_assist", "POST", LOAD_CODE_ASSIST_URL, status, resp_headers, body)


def probe_quota(access_token: str, project_id: str | None) -> dict[str, Any]:
    body_obj: dict[str, Any] = {"project": project_id} if project_id else {}
    payload = json.dumps(body_obj).encode("utf-8")
    headers = {
        "Authorization": f"Bearer {access_token}",
        "Content-Type": "application/json",
        "Accept": "application/json",
        "User-Agent": USER_AGENT,
    }
    status, resp_headers, body = _request_record(
        method="POST", url=QUOTA_URL, headers=headers, body=payload,
    )
    return _record("retrieve_user_quota", "POST", QUOTA_URL, status, resp_headers, body)


def _extract_project_id(load_code_assist_body: Any) -> str | None:
    if not isinstance(load_code_assist_body, dict):
        return None
    direct = load_code_assist_body.get("cloudaicompanionProject")
    if isinstance(direct, str) and direct.strip():
        return direct.strip()
    tier = load_code_assist_body.get("currentTier")
    if isinstance(tier, dict):
        inner = tier.get("cloudaicompanionProject")
        if isinstance(inner, str) and inner.strip():
            return inner.strip()
    for t in load_code_assist_body.get("allowedTiers") or []:
        if isinstance(t, dict):
            inner = t.get("cloudaicompanionProject")
            if isinstance(inner, str) and inner.strip():
                return inner.strip()
    return None


def _ms_to_iso(ms: Any) -> str | None:
    try:
        seconds = int(ms) / 1000.0
    except (TypeError, ValueError):
        return None
    return datetime.fromtimestamp(seconds, tz=timezone.utc).isoformat()


def _token_expired_soon(expiry_date_ms: Any, skew_seconds: int = 300) -> bool:
    try:
        expiry = int(expiry_date_ms) / 1000.0
    except (TypeError, ValueError):
        return True
    return datetime.now(timezone.utc).timestamp() + skew_seconds >= expiry


def main() -> int:
    default_out = Path(__file__).resolve().parent
    default_creds = _default_creds_path()

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out-dir", type=Path, default=default_out,
                   help=f"Directory for JSON recordings (default: {default_out})")
    p.add_argument("--creds-file", type=Path, default=default_creds,
                   help=f"Path to gemini-cli oauth_creds.json (default: {default_creds})")
    p.add_argument("--no-local-state", action="store_true",
                   help="Do not read tokens from ~/.gemini; use environment only")
    p.add_argument("--force-refresh", action="store_true",
                   help="Always refresh even if stored access token has not expired")
    p.add_argument("--token-only", action="store_true",
                   help="Only probe the OAuth token endpoint")
    p.add_argument("--load-only", action="store_true",
                   help="Only probe loadCodeAssist (skip retrieveUserQuota)")
    p.add_argument("--quota-only", action="store_true",
                   help="Only probe retrieveUserQuota (skip loadCodeAssist; needs --project-id or env)")
    p.add_argument("--project-id", default=None,
                   help="Override cloudaicompanionProject (skips/augments loadCodeAssist discovery)")
    p.add_argument("--simulate-bad-refresh", action="store_true",
                   help=("POST a deliberately invalid refresh_token to the OAuth token endpoint "
                         "and save the 4xx response as oauth_token_400_response.json. "
                         "Does not touch loadCodeAssist or retrieveUserQuota."))
    args = p.parse_args()

    if args.simulate_bad_refresh:
        rec = probe_token(INVALID_REFRESH_TOKEN_PLACEHOLDER)
        path = _save(args.out_dir, TOKEN_400_RESPONSE_FILE, rec)
        print(f"wrote {path}  (status {rec['status_code']})", file=sys.stderr)
        return 0
    out_dir: Path = args.out_dir

    exclusive = sum(1 for v in (args.token_only, args.load_only, args.quota_only) if v)
    if exclusive > 1:
        print("error: use at most one of --token-only / --load-only / --quota-only", file=sys.stderr)
        return 2

    refresh: str | None = None
    access: str | None = None
    expiry_ms: Any = None

    if not args.no_local_state:
        try:
            creds = _read_stored_creds(args.creds_file)
            refresh = (creds.get("refresh_token") or None) if isinstance(creds.get("refresh_token"), str) else None
            access = (creds.get("access_token") or None) if isinstance(creds.get("access_token"), str) else None
            expiry_ms = creds.get("expiry_date")
            print(f"using creds from {args.creds_file} (expiry {_ms_to_iso(expiry_ms)})", file=sys.stderr)
        except FileNotFoundError:
            print(f"note: {args.creds_file} not found; relying on environment", file=sys.stderr)
        except OSError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1

    refresh = _env("GEMINI_REFRESH_TOKEN", "YAPCAP_GEMINI_REFRESH_TOKEN") or refresh
    access = _env("GEMINI_ACCESS_TOKEN", "YAPCAP_GEMINI_ACCESS_TOKEN") or access
    project_override = args.project_id or _env("GEMINI_PROJECT_ID", "YAPCAP_GEMINI_PROJECT_ID")

    try:
        should_refresh = args.token_only or args.force_refresh or not access or _token_expired_soon(expiry_ms)
        if should_refresh:
            if not refresh:
                print(
                    "error: no refresh_token (log into gemini-cli or set "
                    "GEMINI_REFRESH_TOKEN / YAPCAP_GEMINI_REFRESH_TOKEN)",
                    file=sys.stderr,
                )
                return 1
            rec = probe_token(refresh)
            path = _save(out_dir, TOKEN_RESPONSE_FILE, rec)
            print(f"wrote {path}  (status {rec['status_code']})", file=sys.stderr)
            if isinstance(rec.get("body_json"), dict):
                at = rec["body_json"].get("access_token")
                if isinstance(at, str) and at:
                    access = at
            if args.token_only:
                return 0 if int(rec["status_code"]) < 400 else 1

        if not access:
            print("error: no access token available after refresh", file=sys.stderr)
            return 1

        project_id = project_override
        if not args.quota_only:
            lrec = probe_load_code_assist(access)
            lpath = _save(out_dir, LOAD_CODE_ASSIST_FILE, lrec)
            print(f"wrote {lpath}  (status {lrec['status_code']})", file=sys.stderr)
            if not project_id:
                project_id = _extract_project_id(lrec.get("body_json"))
                if project_id:
                    print(f"discovered cloudaicompanionProject={project_id}", file=sys.stderr)
            if args.load_only:
                return 0 if int(lrec["status_code"]) < 400 else 1

        qrec = probe_quota(access, project_id)
        qpath = _save(out_dir, QUOTA_RESPONSE_FILE, qrec)
        print(f"wrote {qpath}  (status {qrec['status_code']})", file=sys.stderr)
        return 0 if int(qrec["status_code"]) < 400 else 1
    except URLError as e:
        print(f"error: request failed: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
