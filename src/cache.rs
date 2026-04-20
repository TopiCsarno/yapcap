// SPDX-License-Identifier: MPL-2.0

use crate::config::paths;
use crate::error::{CacheError, Result};
use crate::model::AppState;
use std::fs;
use std::path::Path;

pub fn load_cached_state() -> Result<Option<AppState>, CacheError> {
    load_cached_state_from(&paths().snapshot_file)
}

pub fn save_cached_state(state: &AppState) -> Result<(), CacheError> {
    let paths = paths();
    save_cached_state_to(state, &paths.cache_dir, &paths.snapshot_file)
}

fn load_cached_state_from(file: &Path) -> Result<Option<AppState>, CacheError> {
    if !file.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(file).map_err(|source| CacheError::ReadCache {
        path: file.to_path_buf(),
        source,
    })?;
    let state = serde_json::from_str(&raw).map_err(CacheError::ParseCache)?;
    Ok(Some(state))
}

fn save_cached_state_to(
    state: &AppState,
    cache_dir: &Path,
    snapshot_file: &Path,
) -> Result<(), CacheError> {
    fs::create_dir_all(cache_dir).map_err(|source| CacheError::CreateCacheDir {
        path: cache_dir.to_path_buf(),
        source,
    })?;
    let mut cached_state = state.clone();
    for provider in &mut cached_state.providers {
        provider.is_refreshing = false;
    }
    let payload = serde_json::to_vec_pretty(&cached_state).map_err(CacheError::EncodeCache)?;
    fs::write(snapshot_file, payload).map_err(|source| CacheError::WriteCache {
        path: snapshot_file.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderId;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-cache-{name}-{nanos}"))
    }

    #[test]
    fn missing_cache_returns_none() {
        let dir = test_dir("missing");

        assert!(
            load_cached_state_from(&dir.join("snapshots.json"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cache_round_trips_app_state() {
        let dir = test_dir("round-trip");
        let file = dir.join("snapshots.json");
        let mut state = AppState::empty();
        state.mark_provider_refreshing(ProviderId::Codex, true);

        save_cached_state_to(&state, &dir, &file).unwrap();
        let loaded = load_cached_state_from(&file).unwrap().unwrap();
        let codex = loaded.provider(ProviderId::Codex).unwrap();

        assert!(!codex.is_refreshing);
        assert_eq!(loaded.providers.len(), state.providers.len());
    }

    #[test]
    fn invalid_cache_json_reports_parse_error() {
        let dir = test_dir("invalid-json");
        let file = dir.join("snapshots.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, "{").unwrap();

        assert!(matches!(
            load_cached_state_from(&file),
            Err(CacheError::ParseCache(_))
        ));
    }
}
