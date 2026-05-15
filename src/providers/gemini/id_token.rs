// SPDX-License-Identifier: MPL-2.0

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdTokenClaims {
    pub email: String,
    pub sub: String,
    pub hd: Option<String>,
    pub name: Option<String>,
    pub email_verified: bool,
}

#[derive(Debug, Error)]
pub enum IdTokenError {
    #[error("id_token does not have three dot-separated segments")]
    MalformedStructure,
    #[error("id_token payload is not valid base64url: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("id_token payload is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("id_token payload is missing required claim `{0}`")]
    MissingClaim(&'static str),
    #[error("id_token payload is not a JSON object")]
    NotAnObject,
}

#[derive(Deserialize)]
struct RawClaims {
    email: Option<String>,
    sub: Option<String>,
    hd: Option<String>,
    name: Option<String>,
    #[serde(default)]
    email_verified: bool,
}

pub fn decode(id_token: &str) -> Result<IdTokenClaims, IdTokenError> {
    let payload_b64 = id_token
        .split('.')
        .nth(1)
        .ok_or(IdTokenError::MalformedStructure)?;
    if id_token.split('.').count() != 3 {
        return Err(IdTokenError::MalformedStructure);
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64)?;

    let value: serde_json::Value = serde_json::from_slice(&payload_bytes)?;
    if !value.is_object() {
        return Err(IdTokenError::NotAnObject);
    }
    let raw: RawClaims = serde_json::from_value(value)?;

    let email = raw.email.ok_or(IdTokenError::MissingClaim("email"))?;
    let sub = raw.sub.ok_or(IdTokenError::MissingClaim("sub"))?;

    Ok(IdTokenClaims {
        email,
        sub,
        hd: raw.hd,
        name: raw.name,
        email_verified: raw.email_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn make_token(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        format!("{header}.{payload}.signature")
    }

    fn captured_id_token() -> String {
        let fixture = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/gemini/oauth_token_response.json"
        ))
        .expect("fixture exists");
        let value: serde_json::Value = serde_json::from_str(&fixture).expect("valid json");
        value["body_json"]["id_token"]
            .as_str()
            .expect("id_token field")
            .to_string()
    }

    #[test]
    fn decodes_captured_personal_token() {
        let token = captured_id_token();
        let claims = decode(&token).expect("decoded");
        assert_eq!(claims.email, "user@example.com");
        assert_eq!(claims.sub, "1234567890");
        assert!(claims.email_verified);
        assert_eq!(claims.hd, None);
        assert_eq!(claims.name.as_deref(), Some("Test User"));
    }

    #[test]
    fn decodes_workspace_token_with_hd() {
        let token = make_token(
            r#"{"email":"alice@example.com","sub":"42","hd":"example.com","name":"Alice","email_verified":true}"#,
        );
        let claims = decode(&token).expect("decoded");
        assert_eq!(claims.hd.as_deref(), Some("example.com"));
        assert_eq!(claims.email, "alice@example.com");
        assert!(claims.email_verified);
    }

    #[test]
    fn missing_hd_yields_none() {
        let token = make_token(r#"{"email":"a@b.com","sub":"1"}"#);
        let claims = decode(&token).expect("decoded");
        assert_eq!(claims.hd, None);
        assert!(!claims.email_verified);
    }

    #[test]
    fn malformed_base64_is_an_error() {
        let token = "header.@@not_base64@@.signature";
        assert!(matches!(decode(token), Err(IdTokenError::Base64(_))));
    }

    #[test]
    fn missing_required_claim_is_an_error() {
        let token = make_token(r#"{"sub":"1"}"#);
        assert!(matches!(
            decode(&token),
            Err(IdTokenError::MissingClaim("email"))
        ));
        let token = make_token(r#"{"email":"a@b.com"}"#);
        assert!(matches!(
            decode(&token),
            Err(IdTokenError::MissingClaim("sub"))
        ));
    }

    #[test]
    fn payload_must_be_object() {
        let token = make_token("[1,2,3]");
        assert!(matches!(decode(&token), Err(IdTokenError::NotAnObject)));
    }

    #[test]
    fn missing_segments_is_an_error() {
        assert!(matches!(
            decode("only.two"),
            Err(IdTokenError::MalformedStructure)
        ));
    }
}
