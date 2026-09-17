// SPDX-License-Identifier: MPL-2.0

pub fn discover_api_key() -> Option<String> {
    match crate::providers::opencode_auth::discover("synthetic")? {
        crate::providers::opencode_auth::OpenCodeCredential::Api { key } if !key.is_empty() => {
            Some(key)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn discovers_synthetic_api_key() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(&path, r#"{"synthetic":{"type":"api","key":"test-key"}}"#).unwrap();
        let mut env = crate::test_support::test_env();
        env.set(
            crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
            &path,
        );

        assert_eq!(discover_api_key().as_deref(), Some("test-key"));
    }

    #[test]
    fn missing_auth_file_has_no_api_key() {
        let temp = tempdir().unwrap();
        let mut env = crate::test_support::test_env();
        env.set(
            crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
            temp.path().join("auth.json"),
        );

        assert_eq!(discover_api_key(), None);
    }

    #[test]
    fn malformed_auth_file_has_no_api_key() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(&path, "not-json").unwrap();
        let mut env = crate::test_support::test_env();
        env.set(
            crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
            &path,
        );

        assert_eq!(discover_api_key(), None);
    }

    #[test]
    fn wrong_auth_shape_has_no_api_key() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(&path, r#"{"synthetic":{"token":"test-key"}}"#).unwrap();
        let mut env = crate::test_support::test_env();
        env.set(
            crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
            &path,
        );

        assert_eq!(discover_api_key(), None);
    }
}
