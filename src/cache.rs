use crate::config::paths;
use crate::error::{CacheError, Result};
use crate::model::AppState;
use std::fs;

pub fn load_cached_state() -> Result<Option<AppState>> {
    let file = paths().snapshot_file;
    if !file.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&file).map_err(|source| CacheError::ReadCache {
        path: file.clone(),
        source,
    })?;
    let state = serde_json::from_str(&raw).map_err(CacheError::ParseCache)?;
    Ok(Some(state))
}

pub fn save_cached_state(state: &AppState) -> Result<()> {
    let paths = paths();
    fs::create_dir_all(&paths.cache_dir).map_err(|source| CacheError::CreateCacheDir {
        path: paths.cache_dir.clone(),
        source,
    })?;
    let payload = serde_json::to_vec_pretty(state).map_err(CacheError::EncodeCache)?;
    fs::write(&paths.snapshot_file, payload).map_err(|source| CacheError::WriteCache {
        path: paths.snapshot_file.clone(),
        source,
    })?;
    Ok(())
}
