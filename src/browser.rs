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
use tracing::{debug, warn};

use crate::error::{BrowserError, Result};

const CURSOR_COOKIE_NAME: &str = "WorkosCursorSessionToken";
const CURSOR_COOKIE_DOMAIN: &str = "cursor.com";

pub async fn load_cursor_cookie_from_brave(
    cookie_db_path: &Path,
    application: &str,
) -> Result<String> {
    let password = load_safe_storage_password(application).await?;
    let encrypted_cookie = read_cookie_blob(cookie_db_path)?;
    let decrypted = decrypt_chromium_cookie(&encrypted_cookie, &password)?;
    Ok(format!("{CURSOR_COOKIE_NAME}={decrypted}"))
}

async fn load_safe_storage_password(application: &str) -> Result<String> {
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
    // Some secret service implementations (e.g. KWallet, COSMIC) append a
    // trailing terminators. Strip them once here so cookie decryption can
    // assume it receives a clean secret.
    let password = password
        .trim_matches(|char| char == '\0' || char == '\n' || char == '\r')
        .to_string();
    debug!(
        application,
        password_len = password.len(),
        "loaded safe storage password from keyring"
    );
    Ok(password)
}

fn read_cookie_blob(cookie_db_path: &Path) -> Result<Vec<u8>> {
    if !cookie_db_path.exists() {
        return Err(BrowserError::CookieDatabaseNotFound {
            path: cookie_db_path.to_path_buf(),
        }
        .into());
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
        .query_row((CURSOR_COOKIE_NAME, CURSOR_COOKIE_DOMAIN), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(BrowserError::CookieNotFound)?;

    if !value.is_empty() {
        warn!("browser cookie was stored in plaintext; using value column directly");
        return Ok(value.into_bytes());
    }
    Ok(encrypted_value)
}

fn derive_chromium_key(password: &str) -> [u8; 16] {
    let mut key = [0_u8; 16];
    // Chromium on Linux really uses one PBKDF2 iteration here for OSCrypt compatibility.
    pbkdf2_hmac::<Sha1>(password.as_bytes(), b"saltysalt", 1, &mut key);
    key
}

fn decrypt_chromium_cookie(blob: &[u8], secret: &str) -> Result<String> {
    if blob.is_empty() {
        return Err(BrowserError::EmptyCookieBlob.into());
    }
    if !blob.starts_with(b"v10") && !blob.starts_with(b"v11") {
        return String::from_utf8(blob.to_vec())
            .map_err(BrowserError::CookieBlobNotRecognized)
            .map_err(Into::into);
    }

    debug!(
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
        debug!("trying gcm/base64-key path");
        match decrypt_chromium_cookie_gcm(blob, &decoded_secret) {
            Ok(cookie) => return Ok(cookie),
            Err(error) => errors.push(format!("gcm/base64-key: {error}")),
        }
    }

    debug!("trying cbc/pbkdf2-key path");
    let cbc_key = derive_chromium_key(secret);
    match decrypt_chromium_cookie_cbc(blob, &cbc_key) {
        Ok(cookie) => return Ok(cookie),
        Err(error) => errors.push(format!("cbc/pbkdf2-key: {error}")),
    }

    if let Ok(decoded_secret) = BASE64_STANDARD.decode(secret)
        && let Ok(decoded_secret_text) = std::str::from_utf8(&decoded_secret)
    {
        debug!("trying cbc/base64-decoded-pbkdf2-key path");
        let decoded_cbc_key = derive_chromium_key(decoded_secret_text);
        match decrypt_chromium_cookie_cbc(blob, &decoded_cbc_key) {
            Ok(cookie) => return Ok(cookie),
            Err(error) => errors.push(format!("cbc/base64-decoded-pbkdf2-key: {error}")),
        }
    }

    Err(BrowserError::CookieDecryptFailed(errors.join(" | ")).into())
}

fn decrypt_chromium_cookie_cbc(blob: &[u8], key: &[u8; 16]) -> Result<String> {
    let ciphertext = &blob[3..];
    let iv = [b' '; 16];
    let mut buffer = ciphertext.to_vec();
    let decrypted = Decryptor::<Aes128>::new_from_slices(key, &iv)
        .map_err(|error| BrowserError::InitAesCbc(error.to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|error| BrowserError::DecryptCookieCbc(error.to_string()))?;
    normalize_decrypted_cookie(decrypted)
}

fn decrypt_chromium_cookie_gcm(blob: &[u8], key: &[u8]) -> Result<String> {
    let nonce = Nonce::from_slice(&blob[3..15]);
    let cipher = Aes128Gcm::new_from_slice(key)
        .map_err(|error| BrowserError::InitAesGcm(error.to_string()))?;
    let decrypted = cipher
        .decrypt(nonce, &blob[15..])
        .map_err(|error| BrowserError::DecryptCookieGcm(error.to_string()))?;
    normalize_decrypted_cookie(&decrypted)
}

fn normalize_decrypted_cookie(decrypted: &[u8]) -> Result<String> {
    debug!(
        decrypted_len = decrypted.len(),
        "normalizing decrypted cookie"
    );
    match String::from_utf8(decrypted.to_vec()) {
        Ok(cookie) => Ok(cookie),
        Err(_) if decrypted.len() > 32 => String::from_utf8(decrypted[32..].to_vec())
            .map_err(BrowserError::CookieNotUtf8AfterPrefix)
            .map_err(Into::into),
        Err(source) => Err(BrowserError::CookieNotUtf8 {
            len: decrypted.len(),
            source,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_has_expected_size() {
        let key = derive_chromium_key("secret");
        assert_eq!(key.len(), 16);
    }

    #[test]
    fn rejects_empty_cookie_blob() {
        let error = decrypt_chromium_cookie(&[], "secret")
            .unwrap_err()
            .to_string();
        assert!(error.contains("empty"));
    }

    #[test]
    fn strips_domain_hash_prefix_before_utf8_decode() {
        let mut decrypted = vec![0xff_u8; 32];
        decrypted.extend_from_slice(b"session-value");
        let normalized = normalize_decrypted_cookie(&decrypted).unwrap();
        assert_eq!(normalized, "session-value");
    }
}
