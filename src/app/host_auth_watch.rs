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
            let targets = WatchTargets::for_home(&home);
            let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(32);
            let tx_cb = tx.clone();
            let targets_cb = targets.clone();
            let Ok(mut watcher) = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res
                        && event_targets_cli_auth(&event, &targets_cb)
                    {
                        let _ = tx_cb.blocking_send(());
                    }
                },
                Config::default(),
            ) else {
                tracing::warn!("host CLI auth file watcher could not be created");
                return;
            };
            if !install_watches(&mut watcher, &targets) {
                tracing::warn!(
                    codex_auth = %targets.codex_auth.display(),
                    claude_json = %targets.claude_json.display(),
                    gemini_accounts = %targets.gemini_accounts.display(),
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

#[derive(Clone)]
struct WatchTargets {
    home: std::path::PathBuf,
    codex_auth: std::path::PathBuf,
    codex_dir: std::path::PathBuf,
    claude_json: std::path::PathBuf,
    gemini_accounts: std::path::PathBuf,
    gemini_dir: std::path::PathBuf,
}

impl WatchTargets {
    fn for_home(home: &Path) -> Self {
        let codex_dir = home.join(".codex");
        let codex_auth = codex_dir.join("auth.json");
        let claude_json = home.join(".claude.json");
        let gemini_dir = home.join(".gemini");
        let gemini_accounts = gemini_dir.join("google_accounts.json");
        Self {
            home: home.to_path_buf(),
            codex_auth,
            codex_dir,
            claude_json,
            gemini_accounts,
            gemini_dir,
        }
    }
}

fn install_watches(watcher: &mut RecommendedWatcher, targets: &WatchTargets) -> bool {
    let mut installed = false;
    if targets.codex_auth.exists() {
        installed |= install_watch(watcher, &targets.codex_auth);
    } else if targets.codex_dir.is_dir() {
        installed |= install_watch(watcher, &targets.codex_dir);
    }

    if targets.claude_json.exists() {
        installed |= install_watch(watcher, &targets.claude_json);
    }

    if targets.gemini_accounts.exists() {
        installed |= install_watch(watcher, &targets.gemini_accounts);
    } else if targets.gemini_dir.is_dir() {
        installed |= install_watch(watcher, &targets.gemini_dir);
    }

    installed |= install_watch(watcher, &targets.home);
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

fn event_targets_cli_auth(event: &Event, targets: &WatchTargets) -> bool {
    event.paths.iter().any(|p| {
        p == &targets.codex_auth
            || p == &targets.claude_json
            || p == &targets.gemini_accounts
            || codex_auth_in_dir_event(p, &targets.codex_auth)
            || claude_json_in_home_event(p, &targets.home, &targets.claude_json)
            || gemini_accounts_in_dir_event(p, &targets.gemini_accounts)
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

fn gemini_accounts_in_dir_event(path: &Path, gemini_accounts: &Path) -> bool {
    path.file_name() == Some(OsStr::new("google_accounts.json"))
        && path.parent().map(Path::to_path_buf) == gemini_accounts.parent().map(Path::to_path_buf)
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
    fn gemini_accounts_in_dir_event_matches_only_google_accounts_json_in_dot_gemini() {
        let home = PathBuf::from("/home/u");
        let gemini_accounts = home.join(".gemini").join("google_accounts.json");
        assert!(gemini_accounts_in_dir_event(
            &home.join(".gemini").join("google_accounts.json"),
            &gemini_accounts
        ));
        assert!(!gemini_accounts_in_dir_event(
            &home.join(".gemini").join("settings.json"),
            &gemini_accounts
        ));
        assert!(!gemini_accounts_in_dir_event(
            &home.join(".cache").join("google_accounts.json"),
            &gemini_accounts
        ));
    }

    #[test]
    fn cli_auth_event_matches_claude_json_rename_target() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);
        let event = Event::new(EventKind::Any)
            .add_path(home.join(".claude.json.tmp"))
            .add_path(home.join(".claude.json"));

        assert!(event_targets_cli_auth(&event, &targets));
    }

    #[test]
    fn cli_auth_event_matches_gemini_accounts_rename_target() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);
        let event = Event::new(EventKind::Any)
            .add_path(home.join(".gemini").join("google_accounts.json.tmp"))
            .add_path(home.join(".gemini").join("google_accounts.json"));

        assert!(event_targets_cli_auth(&event, &targets));
    }
}
