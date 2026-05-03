#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
AUTHORIZE_URL = "https://claude.ai/oauth/authorize"
TOKEN_URL = "https://console.anthropic.com/v1/oauth/token"
REDIRECT_URI = "https://console.anthropic.com/oauth/code/callback"

VARIANTS = {
    "minimal-separate": {
        "scope": "user:profile",
        "state_strategy": "separate",
    },
    "minimal-verifier": {
        "scope": "user:profile",
        "state_strategy": "verifier",
    },
    "broad-separate": {
        "scope": "org:create_api_key user:profile user:inference",
        "state_strategy": "separate",
    },
    "broad-verifier": {
        "scope": "org:create_api_key user:profile user:inference",
        "state_strategy": "verifier",
    },
}


def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")


def pkce_pair() -> tuple[str, str]:
    verifier = b64url(os.urandom(32))
    challenge = b64url(hashlib.sha256(verifier.encode("utf-8")).digest())
    return verifier, challenge


def build_authorize_url(scope: str, state_strategy: str, verifier: str, challenge: str) -> tuple[str, str]:
    state = verifier if state_strategy == "verifier" else secrets.token_urlsafe(32)
    params = {
        "code": "true",
        "client_id": CLIENT_ID,
        "response_type": "code",
        "redirect_uri": REDIRECT_URI,
        "scope": scope,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "state": state,
    }
    return f"{AUTHORIZE_URL}?{urllib.parse.urlencode(params, quote_via=urllib.parse.quote)}", state


def parse_code_state(raw: str) -> tuple[str, str | None]:
    value = raw.strip()
    if not value:
        raise ValueError("empty code input")
    if value.startswith("http://") or value.startswith("https://"):
        parsed = urllib.parse.urlparse(value)
        query = urllib.parse.parse_qs(parsed.query)
        code = query.get("code", [None])[0]
        state = query.get("state", [None])[0]
        if not code and parsed.fragment:
            code, state = parse_code_state(parsed.fragment)
        if not code:
            raise ValueError("URL did not contain code")
        return code, state
    if "#" in value:
        code, state = value.split("#", 1)
        return code, state or None
    return value, None


def exchange(code: str, state: str, verifier: str) -> tuple[int, dict | str]:
    payload = {
        "code": code,
        "state": state,
        "grant_type": "authorization_code",
        "client_id": CLIENT_ID,
        "redirect_uri": REDIRECT_URI,
        "code_verifier": verifier,
    }
    req = urllib.request.Request(
        TOKEN_URL,
        data=json.dumps(payload).encode("utf-8"),
        method="POST",
        headers={
            "Content-Type": "application/json",
            "User-Agent": "claude-code/2.0.32",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            text = resp.read().decode("utf-8", errors="replace")
            return resp.status, json.loads(text)
    except urllib.error.HTTPError as err:
        text = err.read().decode("utf-8", errors="replace")
        try:
            return err.code, json.loads(text)
        except json.JSONDecodeError:
            return err.code, text
    except urllib.error.URLError as err:
        return 0, {"network_error": str(err.reason)}


def redact_value(key: str, value):
    lowered = key.lower()
    if any(part in lowered for part in ["token", "code", "secret"]):
        return "<redacted>"
    if lowered in {"uuid", "id"} or lowered.endswith("_uuid") or lowered.endswith("_id"):
        return "<redacted-id>"
    if lowered == "name":
        return "<redacted-name>"
    if key in {"email", "email_address"}:
        return "user@example.com"
    return redact(value)


def redact(value):
    if isinstance(value, dict):
        return {key: redact_value(key, item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact(item) for item in value]
    return value


def response_shape(body):
    if isinstance(body, dict):
        return {
            "keys": sorted(body.keys()),
            "redacted_body": redact(body),
        }
    return {"text": str(body)[:500]}


def run_variant(name: str, capture_dir: Path) -> None:
    variant = VARIANTS[name]
    verifier, challenge = pkce_pair()
    auth_url, expected_state = build_authorize_url(
        variant["scope"],
        variant["state_strategy"],
        verifier,
        challenge,
    )

    print()
    print("=" * 72)
    print(f"Variant: {name}")
    print(f"Scope: {variant['scope']}")
    print(f"State strategy: {variant['state_strategy']}")
    print()
    print(auth_url)
    print()
    raw = input("Paste returned code#state or callback URL, or blank to skip: ").strip()
    if not raw:
        print("Skipped.")
        return

    code, returned_state = parse_code_state(raw)
    state_to_send = returned_state or expected_state
    started = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    status, body = exchange(code, state_to_send, verifier)

    capture = {
        "variant": name,
        "captured_at": started,
        "authorize_url_shape": {
            "host": urllib.parse.urlparse(AUTHORIZE_URL).netloc,
            "redirect_uri": REDIRECT_URI,
            "scope": variant["scope"],
            "code_true": True,
            "state_strategy": variant["state_strategy"],
            "state_matches_returned": returned_state == expected_state if returned_state else None,
        },
        "exchange_request_shape": {
            "endpoint": TOKEN_URL,
            "grant_type": "authorization_code",
            "redirect_uri": REDIRECT_URI,
            "has_code": bool(code),
            "code_length": len(code),
            "has_state": bool(state_to_send),
            "state_length": len(state_to_send),
            "has_code_verifier": bool(verifier),
            "code_verifier_length": len(verifier),
        },
        "response": {
            "status": status,
            **response_shape(body),
        },
    }

    capture_dir.mkdir(parents=True, exist_ok=True)
    path = capture_dir / f"{name}.json"
    path.write_text(json.dumps(capture, indent=2) + "\n", encoding="utf-8")
    print(f"HTTP {status}")
    print(f"Wrote {path}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "variants",
        nargs="*",
        choices=[*VARIANTS.keys(), "all"],
        default=["all"],
    )
    parser.add_argument(
        "--capture-dir",
        default=".scratch/claude-oauth-probe/captures",
    )
    args = parser.parse_args()

    variants = list(VARIANTS.keys()) if "all" in args.variants else args.variants
    capture_dir = Path(args.capture_dir)
    for name in variants:
        run_variant(name, capture_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
