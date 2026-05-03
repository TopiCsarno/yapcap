// SPDX-License-Identifier: MPL-2.0

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ClaudeAuth {
    pub access_token: String,
    pub scopes: Vec<String>,
    pub subscription_type: Option<String>,
}

pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let json = jwt_payload(id_token)?;
    json.get("email")?.as_str().map(str::to_string)
}

pub fn account_id_from_id_token(id_token: &str) -> Option<String> {
    let json = jwt_payload(id_token)?;
    json.get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::to_string)
}

pub fn user_id_from_token(token: &str) -> Option<String> {
    let json = jwt_payload(token)?;
    json.get("https://api.openai.com/auth")?
        .get("chatgpt_user_id")?
        .as_str()
        .map(str::to_string)
}

pub fn jwt_expiration(token: &str) -> Option<DateTime<Utc>> {
    let json = jwt_payload(token)?;
    let exp = json.get("exp")?.as_i64()?;
    DateTime::<Utc>::from_timestamp(exp, 0)
}

fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64url_decode(payload)?;
    serde_json::from_slice(&bytes).ok()
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    let mut table = [INVALID; 128];
    for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[c as usize] = u8::try_from(i).unwrap_or(INVALID);
    }
    table[b'-' as usize] = 62;
    table[b'_' as usize] = 63;

    let mut output = Vec::with_capacity(input.len() * 3 / 4 + 2);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &byte in input.as_bytes() {
        if byte == b'=' || byte > 127 {
            break;
        }
        let val = table[byte as usize];
        if val == INVALID {
            return None;
        }
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_email_from_id_token() {
        let payload = r#"{"email":"topi2236@gmail.com","sub":"auth0|abc"}"#;
        let encoded = {
            let b = payload.as_bytes();
            let mut out = String::new();
            let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i < b.len() {
                let b0 = b[i] as u32;
                let b1 = if i + 1 < b.len() { b[i + 1] as u32 } else { 0 };
                let b2 = if i + 2 < b.len() { b[i + 2] as u32 } else { 0 };
                out.push(table[((b0 >> 2) & 0x3f) as usize] as char);
                out.push(table[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
                out.push(if i + 1 < b.len() {
                    table[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char
                } else {
                    '='
                });
                out.push(if i + 2 < b.len() {
                    table[(b2 & 0x3f) as usize] as char
                } else {
                    '='
                });
                i += 3;
            }
            out.replace('+', "-").replace('/', "_").replace('=', "")
        };
        let token = format!("header.{encoded}.sig");
        assert_eq!(
            email_from_id_token(&token),
            Some("topi2236@gmail.com".to_string())
        );
    }

    #[test]
    fn extracts_account_id_and_expiration_from_id_token() {
        let payload = r#"{
            "exp": 1770000000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_123"
            }
        }"#;
        let encoded = encode_jwt_payload(payload);
        let token = format!("header.{encoded}.sig");

        assert_eq!(
            account_id_from_id_token(&token),
            Some("acct_123".to_string())
        );
        assert_eq!(
            jwt_expiration(&token),
            DateTime::<Utc>::from_timestamp(1_770_000_000, 0)
        );
    }

    fn encode_jwt_payload(payload: &str) -> String {
        let b = payload.as_bytes();
        let mut out = String::new();
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < b.len() {
            let b0 = b[i] as u32;
            let b1 = if i + 1 < b.len() { b[i + 1] as u32 } else { 0 };
            let b2 = if i + 2 < b.len() { b[i + 2] as u32 } else { 0 };
            out.push(table[((b0 >> 2) & 0x3f) as usize] as char);
            out.push(table[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
            out.push(if i + 1 < b.len() {
                table[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char
            } else {
                '='
            });
            out.push(if i + 2 < b.len() {
                table[(b2 & 0x3f) as usize] as char
            } else {
                '='
            });
            i += 3;
        }
        out.replace('+', "-").replace('/', "_").replace('=', "")
    }
}
