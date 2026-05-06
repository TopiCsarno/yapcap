// SPDX-License-Identifier: MPL-2.0

use super::Message;
use crate::config::host_user_home_dir;
use crate::demo_env;
use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::sink::SinkExt;
use cosmic::iced::stream;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

pub(super) fn subscription() -> Subscription<Message> {
    if demo_env::is_active() {
        return Subscription::none();
    }
    Subscription::run_with(0u8, |_| {
        stream::channel(32, move |mut output: mpsc::Sender<Message>| async move {
            let Some(home) = host_user_home_dir() else {
                return;
            };
            let codex_auth = home.join(".codex").join("auth.json");
            let claude_json = home.join(".claude.json");
            let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(32);
            let tx_cb = tx.clone();
            let codex_auth_cb = codex_auth.clone();
            let claude_json_cb = claude_json.clone();
            let home_cb = home.clone();
            let Ok(mut watcher) = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res
                        && event_targets_cli_auth(&event, &codex_auth_cb, &claude_json_cb, &home_cb)
                    {
                        let _ = tx_cb.blocking_send(());
                    }
                },
                Config::default(),
            ) else {
                tracing::warn!("host CLI auth file watcher could not be created");
                return;
            };
            if !install_watches(&mut watcher, &home, &codex_auth, &claude_json) {
                tracing::warn!(
                    codex_auth = %codex_auth.display(),
                    claude_json = %claude_json.display(),
                    "host CLI auth inotify watches could not be installed"
                );
                return;
            }
            while let Some(()) = rx.recv().await {
                tokio::time::sleep(Duration::from_millis(150)).await;
                while rx.try_recv().is_ok() {}
                if output.send(Message::HostCliAuthChanged).await.is_err() {
                    break;
                }
            }
        })
    })
}

fn install_watches(
    watcher: &mut RecommendedWatcher,
    home: &Path,
    codex_auth: &Path,
    claude_json: &Path,
) -> bool {
    let mut installed = false;
    let codex_dir = home.join(".codex");
    if codex_auth.exists() {
        installed |= install_watch(watcher, codex_auth);
    } else if codex_dir.is_dir() {
        installed |= install_watch(watcher, &codex_dir);
    }

    if claude_json.exists() {
        installed |= install_watch(watcher, claude_json);
    }
    installed |= install_watch(watcher, home);
    installed
}

fn install_watch(watcher: &mut RecommendedWatcher, path: &Path) -> bool {
    match watcher.watch(path, RecursiveMode::NonRecursive) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "host CLI auth path watch could not be installed");
            false
        }
    }
}

fn event_targets_cli_auth(
    event: &Event,
    codex_auth: &Path,
    claude_json: &Path,
    home: &Path,
) -> bool {
    event.paths.iter().any(|p| {
        p == codex_auth
            || p == claude_json
            || codex_auth_in_dir_event(p, codex_auth)
            || claude_json_in_home_event(p, home, claude_json)
    })
}

fn codex_auth_in_dir_event(path: &Path, codex_auth: &Path) -> bool {
    path.file_name() == Some(OsStr::new("auth.json"))
        && path.parent().map(Path::to_path_buf) == codex_auth.parent().map(Path::to_path_buf)
}

fn claude_json_in_home_event(path: &Path, home: &Path, claude_json: &Path) -> bool {
    path.file_name() == Some(OsStr::new(".claude.json"))
        && path.parent().map(Path::to_path_buf) == Some(home.to_path_buf())
        && claude_json.parent().map(Path::to_path_buf) == Some(home.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use std::path::PathBuf;

    #[test]
    fn codex_auth_in_dir_event_matches_only_auth_json_in_dot_codex() {
        let home = PathBuf::from("/home/u");
        let codex_auth = home.join(".codex").join("auth.json");
        assert!(codex_auth_in_dir_event(
            &home.join(".codex").join("auth.json"),
            &codex_auth
        ));
        assert!(!codex_auth_in_dir_event(
            &home.join(".codex").join("other.json"),
            &codex_auth
        ));
    }

    #[test]
    fn claude_json_home_event_matches_name_and_parent() {
        let home = PathBuf::from("/home/u");
        let claude = home.join(".claude.json");
        assert!(claude_json_in_home_event(&claude, &home, &claude));
        assert!(!claude_json_in_home_event(
            &home.join(".cache").join(".claude.json"),
            &home,
            &claude
        ));
    }

    #[test]
    fn cli_auth_event_matches_claude_json_rename_target() {
        let home = PathBuf::from("/home/u");
        let codex_auth = home.join(".codex").join("auth.json");
        let claude = home.join(".claude.json");
        let event = Event::new(EventKind::Any)
            .add_path(home.join(".claude.json.tmp"))
            .add_path(claude.clone());

        assert!(event_targets_cli_auth(&event, &codex_auth, &claude, &home));
    }
}
