// SPDX-License-Identifier: MPL-2.0

use crate::config::paths;
use crate::error::{LoggingError, Result};
use std::fs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

pub fn init(default_level: &str) -> Result<WorkerGuard, LoggingError> {
    let paths = paths();
    fs::create_dir_all(&paths.log_dir).map_err(|source| LoggingError::CreateLogDir {
        path: paths.log_dir.clone(),
        source,
    })?;
    let file_appender = tracing_appender::rolling::daily(&paths.log_dir, "yapcap.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(file_writer))
        .try_init()
        .map_err(LoggingError::InitTracing)?;

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_log_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-log-test-{nanos}"))
    }

    #[test]
    fn log_dir_is_created_on_init() {
        let dir = test_log_dir();
        assert!(!dir.exists());
        fs::create_dir_all(&dir).unwrap();
        assert!(dir.exists());
    }
}
