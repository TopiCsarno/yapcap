// SPDX-License-Identifier: MPL-2.0

use aes::Aes128;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes128Gcm, KeyInit, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use cbc::Decryptor;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use pbkdf2::pbkdf2_hmac;
use rusqlite::{Connection, OpenFlags};
use secret_service::{EncryptionType, SecretService};
use sha1::Sha1;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::NamedTempFile;

use crate::error::{BrowserError, Result};

const CURSOR_COOKIE_NAME: &str = "WorkosCursorSessionToken";
const CURSOR_COOKIE_DOMAIN: &str = "cursor.com";

pub async fn load_cursor_cookie_chromium(
    cookie_db_path: &Path,
    application: &str,
) -> Result<String, BrowserError> {
    load_chromium_cookie(
        cookie_db_path,
        application,
        CURSOR_COOKIE_NAME,
        CURSOR_COOKIE_DOMAIN,
    )
    .await
}

async fn load_chromium_cookie(
    cookie_db_path: &Path,
    application: &str,
    cookie_name: &str,
    cookie_domain: &str,
) -> Result<String, BrowserError> {
    let password = load_safe_storage_password(application).await?;
    let encrypted_cookie = read_cookie_blob(cookie_db_path, cookie_name, cookie_domain)?;
    let decrypted = decrypt_chromium_cookie(&encrypted_cookie, &password)?;
    Ok(format!("{cookie_name}={decrypted}"))
}

pub fn load_cursor_cookie_firefox(cookie_db_path: &Path) -> Result<String, BrowserError> {
    load_firefox_cookie(cookie_db_path, CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN)
}

fn load_firefox_cookie(
    cookie_db_path: &Path,
    cookie_name: &str,
    cookie_domain: &str,
) -> Result<String, BrowserError> {
    if !cookie_db_path.exists() {
        return Err(BrowserError::CookieDatabaseNotFound {
            path: cookie_db_path.to_path_buf(),
        });
    }

    let temp = NamedTempFile::new().map_err(BrowserError::CreateTempCookieDb)?;
    fs::copy(cookie_db_path, temp.path()).map_err(|source| BrowserError::CopyCookieDb {
        path: cookie_db_path.to_path_buf(),
        source,
    })?;
    let connection = Connection::open_with_flags(
        temp.path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(BrowserError::OpenCookieDb)?;

    let mut statement = connection
        .prepare(
            "SELECT value
             FROM moz_cookies
             WHERE name = ?1
               AND (
                    host = ?2
                 OR host = '.' || ?2
                 OR host LIKE '%.' || ?2
               )
             ORDER BY
               CASE
                 WHEN host = ?2 THEN 0
                 WHEN host = '.' || ?2 THEN 1
                 ELSE 2
               END
             LIMIT 1",
        )
        .map_err(BrowserError::PrepareCookieLookup)?;
    let value: String = statement
        .query_row((cookie_name, cookie_domain), |row| row.get(0))
        .map_err(BrowserError::CookieNotFound)?;

    Ok(format!("{cookie_name}={value}"))
}

async fn load_safe_storage_password(application: &str) -> Result<String, BrowserError> {
    let service = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(BrowserError::ConnectSecretService)?;
    let search = service
        .search_items(HashMap::from([("application", application)]))
        .await
        .map_err(BrowserError::SearchSecretService)?;
    let item = search
        .unlocked
        .first()
        .or_else(|| search.locked.first())
        .ok_or(BrowserError::MissingKeyringItem)?;
    let secret = item
        .get_secret()
        .await
        .map_err(BrowserError::ReadBrowserSecret)?;
    let password = String::from_utf8(secret).map_err(BrowserError::BrowserSecretNotUtf8)?;
    let password = password
        .trim_matches(|c| c == '\0' || c == '\n' || c == '\r')
        .to_string();
    tracing::debug!(
        application,
        password_len = password.len(),
        "loaded safe storage password from keyring"
    );
    Ok(password)
}

fn read_cookie_blob(
    cookie_db_path: &Path,
    cookie_name: &str,
    cookie_domain: &str,
) -> Result<Vec<u8>, BrowserError> {
    if !cookie_db_path.exists() {
        return Err(BrowserError::CookieDatabaseNotFound {
            path: cookie_db_path.to_path_buf(),
        });
    }

    let temp = NamedTempFile::new().map_err(BrowserError::CreateTempCookieDb)?;
    fs::copy(cookie_db_path, temp.path()).map_err(|source| BrowserError::CopyCookieDb {
        path: cookie_db_path.to_path_buf(),
        source,
    })?;
    let connection = Connection::open_with_flags(
        temp.path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(BrowserError::OpenCookieDb)?;

    let mut statement = connection
        .prepare(
            "SELECT value, encrypted_value
             FROM cookies
             WHERE name = ?1
               AND (
                    host_key = ?2
                 OR host_key = '.' || ?2
                 OR host_key LIKE '%.' || ?2
               )
             ORDER BY
               CASE
                 WHEN host_key = ?2 THEN 0
                 WHEN host_key = '.' || ?2 THEN 1
                 ELSE 2
               END
             LIMIT 1",
        )
        .map_err(BrowserError::PrepareCookieLookup)?;
    let (value, encrypted_value): (String, Vec<u8>) = statement
        .query_row((cookie_name, cookie_domain), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(BrowserError::CookieNotFound)?;

    if !value.is_empty() {
        tracing::warn!("browser cookie was stored in plaintext; using value column directly");
        return Ok(value.into_bytes());
    }
    Ok(encrypted_value)
}

fn derive_chromium_key(password: &str) -> [u8; 16] {
    let mut key = [0_u8; 16];
    pbkdf2_hmac::<Sha1>(password.as_bytes(), b"saltysalt", 1, &mut key);
    key
}

fn decrypt_chromium_cookie(blob: &[u8], secret: &str) -> Result<String, BrowserError> {
    if blob.is_empty() {
        return Err(BrowserError::EmptyCookieBlob);
    }
    if !blob.starts_with(b"v10") && !blob.starts_with(b"v11") {
        return String::from_utf8(blob.to_vec()).map_err(BrowserError::CookieBlobNotRecognized);
    }

    tracing::debug!(
        blob_len = blob.len(),
        blob_prefix = ?std::str::from_utf8(&blob[..blob.len().min(3)]).unwrap_or("?"),
        secret_len = secret.len(),
        "attempting chromium cookie decryption"
    );
    let mut errors = Vec::new();

    if let Ok(decoded_secret) = BASE64_STANDARD.decode(secret)
        && decoded_secret.len() == 16
        && blob.len() > 3 + 12 + 16
    {
        tracing::debug!("trying gcm/base64-key path");
        match decrypt_chromium_cookie_gcm(blob, &decoded_secret) {
            Ok(cookie) => return Ok(cookie),
            Err(error) => errors.push(format!("gcm/base64-key: {error}")),
        }
    }

    tracing::debug!("trying cbc/pbkdf2-key path");
    let cbc_key = derive_chromium_key(secret);
    match decrypt_chromium_cookie_cbc(blob, &cbc_key) {
        Ok(cookie) => return Ok(cookie),
        Err(error) => errors.push(format!("cbc/pbkdf2-key: {error}")),
    }

    if let Ok(decoded_secret) = BASE64_STANDARD.decode(secret)
        && let Ok(decoded_text) = std::str::from_utf8(&decoded_secret)
    {
        tracing::debug!("trying cbc/base64-decoded-pbkdf2-key path");
        let decoded_cbc_key = derive_chromium_key(decoded_text);
        match decrypt_chromium_cookie_cbc(blob, &decoded_cbc_key) {
            Ok(cookie) => return Ok(cookie),
            Err(error) => errors.push(format!("cbc/base64-decoded-pbkdf2-key: {error}")),
        }
    }

    Err(BrowserError::CookieDecryptFailed(errors.join(" | ")))
}

fn decrypt_chromium_cookie_cbc(blob: &[u8], key: &[u8; 16]) -> Result<String, BrowserError> {
    let ciphertext = &blob[3..];
    let iv = [b' '; 16];
    let mut buffer = ciphertext.to_vec();
    let decrypted = Decryptor::<Aes128>::new_from_slices(key, &iv)
        .map_err(|e| BrowserError::InitAesCbc(e.to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|e| BrowserError::DecryptCookieCbc(e.to_string()))?;
    normalize_decrypted_cookie(decrypted)
}

fn decrypt_chromium_cookie_gcm(blob: &[u8], key: &[u8]) -> Result<String, BrowserError> {
    let nonce = Nonce::from_slice(&blob[3..15]);
    let cipher =
        Aes128Gcm::new_from_slice(key).map_err(|e| BrowserError::InitAesGcm(e.to_string()))?;
    let decrypted = cipher
        .decrypt(nonce, &blob[15..])
        .map_err(|e| BrowserError::DecryptCookieGcm(e.to_string()))?;
    normalize_decrypted_cookie(&decrypted)
}

fn normalize_decrypted_cookie(decrypted: &[u8]) -> Result<String, BrowserError> {
    tracing::debug!(
        decrypted_len = decrypted.len(),
        "normalizing decrypted cookie"
    );
    match String::from_utf8(decrypted.to_vec()) {
        Ok(cookie) => Ok(cookie),
        Err(_) if decrypted.len() > 32 => String::from_utf8(decrypted[32..].to_vec())
            .map_err(BrowserError::CookieNotUtf8AfterPrefix),
        Err(source) => Err(BrowserError::CookieNotUtf8 {
            len: decrypted.len(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::Encryptor;
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use rusqlite::Connection;
    use tempfile::NamedTempFile;

    const CHROMIUM_SQL: &str = include_str!("../fixtures/browser/chromium_cookies.sql");
    const FIREFOX_SQL: &str = include_str!("../fixtures/browser/firefox_cookies.sql");

    fn chromium_cookies_db(
        name: &str,
        host_key: &str,
        plaintext_value: &str,
        encrypted_value: &[u8],
    ) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE cookies (
                 name            TEXT,
                 host_key        TEXT,
                 value           TEXT  NOT NULL DEFAULT '',
                 encrypted_value BLOB  NOT NULL DEFAULT X''
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cookies (name, host_key, value, encrypted_value) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, host_key, plaintext_value, encrypted_value],
        )
        .unwrap();
        file
    }

    fn firefox_cookies_db(name: &str, host: &str, value: &str) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_cookies (
                 name  TEXT,
                 host  TEXT,
                 value TEXT NOT NULL DEFAULT ''
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO moz_cookies (name, host, value) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, host, value],
        )
        .unwrap();
        file
    }

    fn make_chromium_cbc_blob(password: &str, plaintext: &str) -> Vec<u8> {
        let key = derive_chromium_key(password);
        let iv = [b' '; 16];
        let ciphertext = Encryptor::<aes::Aes128>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());
        let mut blob = b"v10".to_vec();
        blob.extend_from_slice(&ciphertext);
        blob
    }

    fn load_db(sql: &str) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(sql).unwrap();
        file
    }

    #[test]
    fn derive_key_has_expected_size() {
        assert_eq!(derive_chromium_key("secret").len(), 16);
    }

    #[test]
    fn rejects_empty_cookie_blob() {
        let err = decrypt_chromium_cookie(&[], "secret").unwrap_err();
        assert!(matches!(err, BrowserError::EmptyCookieBlob));
    }

    #[test]
    fn strips_domain_hash_prefix_before_utf8_decode() {
        let mut decrypted = vec![0xff_u8; 32];
        decrypted.extend_from_slice(b"session-value");
        let result = normalize_decrypted_cookie(&decrypted).unwrap();
        assert_eq!(result, "session-value");
    }

    #[test]
    fn chromium_read_cookie_blob_returns_encrypted_bytes() {
        let encrypted = vec![1u8, 2, 3, 4];
        let db = chromium_cookies_db(CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN, "", &encrypted);
        let blob = read_cookie_blob(db.path(), CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN).unwrap();
        assert_eq!(blob, encrypted);
    }

    #[test]
    fn chromium_read_cookie_blob_falls_back_to_plaintext_value() {
        let db = chromium_cookies_db(
            CURSOR_COOKIE_NAME,
            CURSOR_COOKIE_DOMAIN,
            "plain_session",
            &[],
        );
        let blob = read_cookie_blob(db.path(), CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN).unwrap();
        assert_eq!(blob, b"plain_session");
    }

    #[test]
    fn chromium_read_cookie_blob_matches_dot_prefixed_domain() {
        let encrypted = vec![5u8, 6];
        let db = chromium_cookies_db(
            CURSOR_COOKIE_NAME,
            &format!(".{CURSOR_COOKIE_DOMAIN}"),
            "",
            &encrypted,
        );
        let blob = read_cookie_blob(db.path(), CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN).unwrap();
        assert_eq!(blob, encrypted);
    }

    #[test]
    fn chromium_read_cookie_blob_returns_error_when_missing() {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE cookies (
                 name TEXT, host_key TEXT,
                 value TEXT NOT NULL DEFAULT '',
                 encrypted_value BLOB NOT NULL DEFAULT X''
             );",
        )
        .unwrap();
        drop(conn);
        let err =
            read_cookie_blob(file.path(), CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN).unwrap_err();
        assert!(matches!(err, BrowserError::CookieNotFound(_)));
    }

    #[test]
    fn chromium_cbc_decrypt_roundtrip() {
        let password = "test_safe_storage_password";
        let plaintext = "user%3AtestWorkosSessionToken";
        let blob = make_chromium_cbc_blob(password, plaintext);
        let result = decrypt_chromium_cookie(&blob, password).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn chromium_fixture_loads_without_firefox_cookie_value() {
        let db = load_db(CHROMIUM_SQL);
        let result = load_cursor_cookie_firefox(db.path());
        assert!(result.is_err());
    }

    #[test]
    fn firefox_fixture_returns_cursor_session_token() {
        let db = load_db(FIREFOX_SQL);
        let result = load_cursor_cookie_firefox(db.path()).unwrap();
        assert_eq!(result, "WorkosCursorSessionToken=cursor-test-session-token");
    }

    #[test]
    fn firefox_extracts_cookie_exact_domain() {
        let db = firefox_cookies_db(
            CURSOR_COOKIE_NAME,
            CURSOR_COOKIE_DOMAIN,
            "firefox_session_value",
        );
        let result = load_cursor_cookie_firefox(db.path()).unwrap();
        assert_eq!(
            result,
            format!("{CURSOR_COOKIE_NAME}=firefox_session_value")
        );
    }

    #[test]
    fn firefox_extracts_cookie_dot_prefixed_domain() {
        let db = firefox_cookies_db(
            CURSOR_COOKIE_NAME,
            &format!(".{CURSOR_COOKIE_DOMAIN}"),
            "dot_domain_value",
        );
        let result = load_cursor_cookie_firefox(db.path()).unwrap();
        assert_eq!(result, format!("{CURSOR_COOKIE_NAME}=dot_domain_value"));
    }

    #[test]
    fn firefox_returns_error_when_cookie_absent() {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_cookies (name TEXT, host TEXT, value TEXT NOT NULL DEFAULT '');",
        )
        .unwrap();
        drop(conn);
        let err = load_cursor_cookie_firefox(file.path()).unwrap_err();
        assert!(matches!(err, BrowserError::CookieNotFound(_)));
    }

    #[test]
    fn firefox_returns_error_for_missing_db() {
        let err = load_cursor_cookie_firefox(Path::new("/tmp/nonexistent_cookies_yapcap.sqlite"))
            .unwrap_err();
        assert!(matches!(err, BrowserError::CookieDatabaseNotFound { .. }));
    }
}
