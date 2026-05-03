#!/usr/bin/env python3
import argparse
import base64
import json
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_DB = Path.home() / ".config/Cursor/User/globalStorage/state.vscdb"
REFRESH_URL = "https://api2.cursor.sh/oauth/token"
CLIENT_ID = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB"
AUTH_ME_URL = "https://cursor.com/api/auth/me"
USAGE_SUMMARY_URL = "https://cursor.com/api/usage-summary"


def read_db(db_path: Path) -> dict:
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        cur = conn.cursor()
        result = {}
        for key in ("cursorAuth/accessToken", "cursorAuth/refreshToken"):
            cur.execute("SELECT value FROM ItemTable WHERE key = ?", (key,))
            row = cur.fetchone()
            result[key] = row[0] if row else None
        return result
    finally:
        conn.close()


def decode_jwt_payload(token: str) -> dict:
    parts = token.split(".")
    if len(parts) != 3:
        raise ValueError("not a JWT")
    payload = parts[1]
    payload += "=" * (4 - len(payload) % 4)
    return json.loads(base64.urlsafe_b64decode(payload))


def build_session_cookie(user_id: str, access_token: str) -> str:
    return f"WorkosCursorSessionToken={urllib.parse.quote(user_id + '::' + access_token)}"


def post_json(url: str, payload: dict, headers: dict | None = None) -> tuple[int, dict | str]:
    req_headers = {"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}
    if headers:
        req_headers.update(headers)
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        method="POST",
        headers=req_headers,
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as err:
        text = err.read().decode("utf-8", errors="replace")
        try:
            return err.code, json.loads(text)
        except json.JSONDecodeError:
            return err.code, text


def get_json(url: str, headers: dict) -> tuple[int, dict | str]:
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0", **headers})
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as err:
        text = err.read().decode("utf-8", errors="replace")
        try:
            return err.code, json.loads(text)
        except json.JSONDecodeError:
            return err.code, text


def redact_value(key: str, value):
    lowered = key.lower()
    if any(p in lowered for p in ("token", "secret", "cookie", "password")):
        return "<redacted>"
    if lowered in {"id", "sub", "email", "name", "picture", "payment_id", "customer_id"}:
        return "<redacted>"
    if lowered.endswith("_id") or lowered.endswith("_uuid"):
        return "<redacted>"
    if key == "email":
        return "user@example.com"
    return redact(value)


def redact(value):
    if isinstance(value, dict):
        return {k: redact_value(k, v) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v) for v in value]
    return value


def save(capture_dir: Path, name: str, data: dict) -> None:
    capture_dir.mkdir(parents=True, exist_ok=True)
    path = capture_dir / f"{name}.json"
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    print(f"  wrote {path}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Probe Cursor token refresh and usage endpoints")
    parser.add_argument("--db", default=str(DEFAULT_DB), help="path to state.vscdb")
    parser.add_argument("--capture-dir", default="fixtures/cursor", help="output directory for fixtures")
    args = parser.parse_args()

    db_path = Path(args.db).expanduser()
    capture_dir = Path(args.capture_dir)

    if not db_path.exists():
        print(f"ERROR: {db_path} not found — is Cursor installed and has it been opened at least once?")
        return 1

    print(f"Reading {db_path}")
    tokens = read_db(db_path)

    refresh_token = tokens.get("cursorAuth/refreshToken")
    access_token = tokens.get("cursorAuth/accessToken")

    if not refresh_token:
        print("ERROR: cursorAuth/refreshToken not found in state.vscdb")
        return 1
    if not access_token:
        print("ERROR: cursorAuth/accessToken not found in state.vscdb")
        return 1

    try:
        jwt_payload = decode_jwt_payload(access_token)
        sub = jwt_payload.get("sub", "")
        user_id = sub.split("|")[-1] if "|" in sub else sub
        print(f"sub claim: {sub[:20]}... user_id prefix: {user_id[:10]}...")
    except Exception as exc:
        print(f"ERROR: failed to decode access token JWT: {exc}")
        return 1

    print()
    print("=== POST token refresh ===")
    started = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    status, body = post_json(REFRESH_URL, {
        "grant_type": "refresh_token",
        "client_id": CLIENT_ID,
        "refresh_token": refresh_token,
    })
    print(f"HTTP {status}")
    if isinstance(body, dict):
        print(f"keys: {sorted(body.keys())}")
        should_logout = body.get("shouldLogout", False)
        if should_logout:
            print("WARNING: shouldLogout=true — session may be expired")
        new_access_token = body.get("access_token")
        if new_access_token:
            print("access_token: present")
            access_token = new_access_token
    else:
        print(f"body: {str(body)[:200]}")

    save(capture_dir, "token_refresh", {
        "captured_at": started,
        "request": {
            "url": REFRESH_URL,
            "method": "POST",
            "body_keys": ["grant_type", "client_id", "refresh_token"],
        },
        "response": {
            "status": status,
            "keys": sorted(body.keys()) if isinstance(body, dict) else [],
            "redacted_body": redact(body) if isinstance(body, dict) else str(body)[:500],
        },
    })

    cookie = build_session_cookie(user_id, access_token)
    cookie_header = {"Cookie": cookie}

    print()
    print("=== GET /api/auth/me ===")
    status, body = get_json(AUTH_ME_URL, cookie_header)
    print(f"HTTP {status}")
    if isinstance(body, dict):
        print(f"keys: {sorted(body.keys())}")
    save(capture_dir, "auth_me", redact(body) if isinstance(body, dict) else {"error": str(body)})

    print()
    print("=== GET /api/usage-summary ===")
    status, body = get_json(USAGE_SUMMARY_URL, cookie_header)
    print(f"HTTP {status}")
    if isinstance(body, dict):
        print(f"keys: {sorted(body.keys())}")
    save(capture_dir, "usage_summary", redact(body) if isinstance(body, dict) else {"error": str(body)})

    print()
    print("Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
