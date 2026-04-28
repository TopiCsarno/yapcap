// SPDX-License-Identifier: MPL-2.0

use crate::model::ProviderId;
use crate::providers::claude::external_claude_config_dir_candidate;
use cosmic::iced::Subscription;
use cosmic::iced::futures::SinkExt;
use inotify::{Inotify, WatchMask};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WatcherKey;

#[derive(Debug, Clone)]
pub enum WatcherEvent {
    AuthFileChanged(ProviderId),
}

pub fn subscription() -> Subscription<WatcherEvent> {
    Subscription::run_with(WatcherKey, |_| {
        cosmic::iced::stream::channel(16, |mut output| async move {
            loop {
                if let Err(e) = run_once(&mut output).await {
                    tracing::warn!(error = %e, "auth watcher restarting");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        })
    })
}

async fn run_once(
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<WatcherEvent>,
) -> Result<(), String> {
    use cosmic::iced::futures::StreamExt;

    let inotify = Inotify::init().map_err(|e| format!("inotify init: {e}"))?;
    let mut watches = inotify.watches();
    let mut active = 0usize;

    for (dir, _) in watch_dirs() {
        if !dir.exists() {
            continue;
        }
        if let Err(e) = watches.add(
            &dir,
            WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::CLOSE_WRITE,
        ) {
            tracing::warn!(path = %dir.display(), error = %e, "could not watch dir");
        } else {
            active += 1;
        }
    }

    if active == 0 {
        tokio::time::sleep(Duration::from_secs(30)).await;
        return Ok(());
    }

    let mut buf = vec![0u8; 4096];
    let mut stream = inotify
        .into_event_stream(&mut buf)
        .map_err(|e| format!("inotify stream: {e}"))?;

    let mut pending: Option<ProviderId> = None;
    let mut deadline: Option<tokio::time::Instant> = None;

    loop {
        let sleep = deadline.map(tokio::time::sleep_until);
        tokio::select! {
            event = stream.next() => {
                match event {
                    None => return Err("event stream ended".into()),
                    Some(Err(e)) => return Err(format!("event error: {e}")),
                    Some(Ok(ev)) => {
                        if let Some(provider) = provider_from_event(&ev) {
                            pending = Some(provider);
                            deadline = Some(tokio::time::Instant::now() + DEBOUNCE);
                        }
                    }
                }
            }
            () = async {
                match sleep {
                    Some(s) => s.await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(provider) = pending.take() {
                    let _ = output.send(WatcherEvent::AuthFileChanged(provider)).await;
                }
                deadline = None;
            }
        }
    }
}

fn provider_from_event(event: &inotify::Event<OsString>) -> Option<ProviderId> {
    provider_from_filename(event.name.as_deref()?)
}

fn provider_from_filename(name: &std::ffi::OsStr) -> Option<ProviderId> {
    if name == "auth.json" {
        return Some(ProviderId::Codex);
    }
    if name == ".credentials.json" {
        return Some(ProviderId::Claude);
    }
    None
}

fn watch_dirs() -> Vec<(PathBuf, ProviderId)> {
    let mut dirs = Vec::new();
    if let Ok(dir) = crate::auth::codex_home() {
        dirs.push((dir, ProviderId::Codex));
    }
    if let Some(dir) = external_claude_config_dir_candidate() {
        dirs.push((dir, ProviderId::Claude));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    fn name(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn filename_matches_auth_json_to_codex() {
        assert_eq!(
            provider_from_filename(name("auth.json").as_os_str()),
            Some(ProviderId::Codex)
        );
    }

    #[test]
    fn filename_matches_credentials_json_to_claude() {
        assert_eq!(
            provider_from_filename(name(".credentials.json").as_os_str()),
            Some(ProviderId::Claude)
        );
    }

    #[test]
    fn filename_returns_none_for_other_files() {
        assert_eq!(
            provider_from_filename(name("config.toml").as_os_str()),
            None
        );
    }

    #[test]
    fn watch_dirs_includes_codex_when_env_set() {
        let _guard = test_support::env_lock();
        unsafe {
            std::env::set_var("CODEX_HOME", "/tmp/test-codex");
        }
        let dirs = watch_dirs();
        unsafe {
            std::env::remove_var("CODEX_HOME");
        }
        assert!(
            dirs.iter()
                .any(|(p, provider)| *provider == ProviderId::Codex
                    && p == std::path::Path::new("/tmp/test-codex"))
        );
    }

    #[test]
    fn watch_dirs_includes_claude_when_env_set() {
        let _guard = test_support::env_lock();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/tmp/test-claude");
        }
        let dirs = watch_dirs();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        assert!(
            dirs.iter()
                .any(|(p, provider)| *provider == ProviderId::Claude
                    && p == std::path::Path::new("/tmp/test-claude"))
        );
    }
}
