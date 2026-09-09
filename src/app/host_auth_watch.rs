// SPDX-License-Identifier: MPL-2.0

use super::Message;
use crate::config::host_user_home_dir;
use crate::demo_env;
use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::sink::SinkExt;
use cosmic::iced::stream;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
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
                    grok_auth = %targets.grok_auth.display(),
                    opencode_auth = %targets.opencode_auth.display(),
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
    config_dir: std::path::PathBuf,
    codex_auth: std::path::PathBuf,
    codex_dir: std::path::PathBuf,
    claude_json: std::path::PathBuf,
    gemini_accounts: std::path::PathBuf,
    gemini_dir: std::path::PathBuf,
    gemini_settings: std::path::PathBuf,
    grok_auth: std::path::PathBuf,
    grok_dir: std::path::PathBuf,
    opencode_auth: std::path::PathBuf,
    opencode_dir: std::path::PathBuf,
}

impl WatchTargets {
    fn for_home(home: &Path) -> Self {
        let codex_dir = home.join(".codex");
        let codex_auth = codex_dir.join("auth.json");
        let claude_json = home.join(".claude.json");
        let config_dir = home.join(".config");
        let gemini_dir = home.join(".gemini");
        let gemini_accounts = gemini_dir.join("google_accounts.json");
        let gemini_settings = gemini_dir.join("settings.json");
        let grok_dir = home.join(".grok");
        let grok_auth = grok_dir.join("auth.json");
        let default_opencode_dir = home.join(".local/share/opencode");
        let opencode_auth = crate::providers::opencode_auth::auth_path()
            .unwrap_or_else(|| default_opencode_dir.join("auth.json"));
        let opencode_dir = opencode_auth
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or(default_opencode_dir);
        Self {
            home: home.to_path_buf(),
            config_dir,
            codex_auth,
            codex_dir,
            claude_json,
            gemini_accounts,
            gemini_dir,
            gemini_settings,
            grok_auth,
            grok_dir,
            opencode_auth,
            opencode_dir,
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
    }
    if targets.gemini_dir.is_dir() {
        installed |= install_watch(watcher, &targets.gemini_dir);
    }

    if targets.grok_auth.exists() {
        installed |= install_watch(watcher, &targets.grok_auth);
    } else if targets.grok_dir.is_dir() {
        installed |= install_watch(watcher, &targets.grok_dir);
    }

    if targets.opencode_dir.is_dir() {
        installed |= install_watch(watcher, &targets.opencode_dir);
    }
    if targets.opencode_auth.exists() {
        installed |= install_watch(watcher, &targets.opencode_auth);
    }

    installed |= install_watch(watcher, &targets.home);
    installed |= install_watch(watcher, &targets.config_dir);
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
    if !event_changes_cli_auth(&event.kind) {
        return false;
    }
    event.paths.iter().any(|p| {
        p == &targets.codex_auth
            || p == &targets.claude_json
            || p == &targets.gemini_accounts
            || p == &targets.gemini_settings
            || p == &targets.grok_auth
            || p == &targets.opencode_auth
            || codex_auth_in_dir_event(p, &targets.codex_auth)
            || claude_json_in_home_event(p, &targets.home, &targets.claude_json)
            || gemini_accounts_in_dir_event(p, &targets.gemini_accounts)
            || grok_auth_in_dir_event(p, &targets.grok_auth)
            || opencode_auth_in_dir_event(p, &targets.opencode_auth)
            || detection_marker_in_home_event(p, &targets.home)
            || detection_marker_in_config_event(p, &targets.config_dir)
            || gemini_settings_in_dir_event(p, &targets.gemini_settings)
    })
}

fn event_changes_cli_auth(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
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

fn grok_auth_in_dir_event(path: &Path, grok_auth: &Path) -> bool {
    path.file_name() == Some(OsStr::new("auth.json"))
        && path.parent().map(Path::to_path_buf) == grok_auth.parent().map(Path::to_path_buf)
}

fn opencode_auth_in_dir_event(path: &Path, opencode_auth: &Path) -> bool {
    path.file_name() == Some(OsStr::new("auth.json"))
        && path.parent().map(Path::to_path_buf) == opencode_auth.parent().map(Path::to_path_buf)
}

fn detection_marker_in_home_event(path: &Path, home: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".codex" | ".claude" | ".claude.json" | ".copilot" | ".mmx" | ".grok")
    ) && path.parent() == Some(home)
}

fn detection_marker_in_config_event(path: &Path, config_dir: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some("Cursor" | "Antigravity" | "github-copilot")
    ) && path.parent() == Some(config_dir)
}

fn gemini_settings_in_dir_event(path: &Path, gemini_settings: &Path) -> bool {
    path.file_name() == Some(OsStr::new("settings.json"))
        && path.parent() == gemini_settings.parent()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode, DataChange, ModifyKind};
    use std::path::PathBuf;

    #[test]
    fn auth_watch_ignores_file_access_events() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);
        let event = Event {
            kind: EventKind::Access(AccessKind::Close(AccessMode::Read)),
            paths: vec![targets.codex_auth.clone()],
            attrs: Default::default(),
        };

        assert!(!event_targets_cli_auth(&event, &targets));
    }

    #[test]
    fn auth_watch_accepts_auth_file_modify_events() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            paths: vec![targets.codex_auth.clone()],
            attrs: Default::default(),
        };

        assert!(event_targets_cli_auth(&event, &targets));
    }

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
    fn opencode_auth_in_dir_event_matches_only_auth_json_in_opencode_data_dir() {
        let home = PathBuf::from("/home/u");
        let opencode_auth = home.join(".local/share/opencode/auth.json");
        assert!(opencode_auth_in_dir_event(&opencode_auth, &opencode_auth));
        assert!(!opencode_auth_in_dir_event(
            &home.join(".local/share/opencode/other.json"),
            &opencode_auth
        ));
        assert!(!opencode_auth_in_dir_event(
            &home.join(".config/opencode/auth.json"),
            &opencode_auth
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

    #[test]
    fn grok_auth_in_dir_event_matches_only_auth_json_in_dot_grok() {
        let home = PathBuf::from("/home/u");
        let grok_auth = home.join(".grok").join("auth.json");
        assert!(grok_auth_in_dir_event(
            &home.join(".grok").join("auth.json"),
            &grok_auth
        ));
        assert!(!grok_auth_in_dir_event(
            &home.join(".grok").join("other.json"),
            &grok_auth
        ));
        assert!(!grok_auth_in_dir_event(
            &home.join(".cache").join("auth.json"),
            &grok_auth
        ));
    }

    #[test]
    fn detection_event_matches_home_marker_paths() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);

        for marker in [
            ".codex",
            ".claude",
            ".claude.json",
            ".copilot",
            ".mmx",
            ".grok",
        ] {
            let event = Event::new(EventKind::Create(notify::event::CreateKind::Any))
                .add_path(home.join(marker));
            assert!(event_targets_cli_auth(&event, &targets), "{marker}");
        }
    }

    #[test]
    fn detection_event_matches_config_and_gemini_marker_paths() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);

        for marker in [
            home.join(".config/Cursor"),
            home.join(".config/Antigravity"),
            home.join(".config/github-copilot"),
            home.join(".gemini/settings.json"),
        ] {
            let event = Event::new(EventKind::Remove(notify::event::RemoveKind::Any))
                .add_path(marker.clone());
            assert!(
                event_targets_cli_auth(&event, &targets),
                "{}",
                marker.display()
            );
        }
    }

    #[test]
    fn detection_event_ignores_unrelated_marker_lookalikes() {
        let home = PathBuf::from("/home/u");
        let targets = WatchTargets::for_home(&home);
        let event = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Any,
        )))
        .add_path(home.join(".cache/.codex"))
        .add_path(home.join(".config/Other"));

        assert!(!event_targets_cli_auth(&event, &targets));
    }
}
