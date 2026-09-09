// SPDX-License-Identifier: MPL-2.0

use std::path::{Path, PathBuf};

use crate::model::ProviderId;

#[derive(Debug, Clone, Copy)]
enum MarkerKind {
    Dir,
    File,
}

#[derive(Debug, Clone, Copy)]
struct Marker {
    relative_path: &'static str,
    kind: MarkerKind,
}

const fn dir(relative_path: &'static str) -> Marker {
    Marker {
        relative_path,
        kind: MarkerKind::Dir,
    }
}

const fn file(relative_path: &'static str) -> Marker {
    Marker {
        relative_path,
        kind: MarkerKind::File,
    }
}

fn markers(provider: ProviderId) -> &'static [Marker] {
    const CODEX: [Marker; 1] = [dir(".codex")];
    const CLAUDE: [Marker; 2] = [dir(".claude"), file(".claude.json")];
    const CURSOR: [Marker; 1] = [dir(".config/Cursor")];
    const GEMINI: [Marker; 1] = [file(".gemini/settings.json")];
    const ANTIGRAVITY: [Marker; 1] = [dir(".config/Antigravity")];
    const COPILOT: [Marker; 2] = [dir(".config/github-copilot"), dir(".copilot")];
    const MINIMAX: [Marker; 1] = [dir(".mmx")];
    const KIMI: [Marker; 0] = [];
    const OPENCODE_GO: [Marker; 1] = [file(".local/share/opencode/auth.json")];
    const GROK: [Marker; 2] = [dir(".grok"), file(".grok/auth.json")];
    match provider {
        ProviderId::Codex => &CODEX,
        ProviderId::Claude => &CLAUDE,
        ProviderId::Cursor => &CURSOR,
        ProviderId::Gemini => &GEMINI,
        ProviderId::Antigravity => &ANTIGRAVITY,
        ProviderId::Copilot => &COPILOT,
        ProviderId::Minimax => &MINIMAX,
        ProviderId::Kimi => &KIMI,
        ProviderId::OpenCodeGo => &OPENCODE_GO,
        ProviderId::Grok => &GROK,
    }
}

impl Marker {
    fn exists_in(self, home: &Path) -> bool {
        let path = home.join(self.relative_path);
        match self.kind {
            MarkerKind::Dir => path.is_dir(),
            MarkerKind::File => path.is_file(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DetectionSnapshot {
    detected: [bool; ProviderId::ALL.len()],
}

impl DetectionSnapshot {
    #[must_use]
    pub fn detected(&self, provider: ProviderId) -> bool {
        self.detected[provider_index(provider)]
    }

    #[must_use]
    pub fn detected_providers(&self) -> Vec<ProviderId> {
        ProviderId::ALL
            .into_iter()
            .filter(|provider| self.detected(*provider))
            .collect()
    }
}

fn provider_index(provider: ProviderId) -> usize {
    ProviderId::ALL
        .iter()
        .position(|candidate| *candidate == provider)
        .expect("ProviderId::ALL contains every provider")
}

#[must_use]
pub fn detect(home: &Path) -> DetectionSnapshot {
    let mut snapshot = DetectionSnapshot::default();
    for provider in ProviderId::ALL {
        snapshot.detected[provider_index(provider)] = markers(provider)
            .iter()
            .any(|marker| marker.exists_in(home));
    }
    snapshot
}

#[must_use]
pub fn startup_snapshot(home: Option<PathBuf>) -> DetectionSnapshot {
    home.map_or_else(DetectionSnapshot::default, |home| detect(&home))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp home")
    }

    fn touch(home: &Path, relative_path: &str) {
        let path = home.join(relative_path);
        fs::create_dir_all(path.parent().expect("marker paths have a parent"))
            .expect("create parent dirs");
        fs::write(path, b"").expect("write marker file");
    }

    fn mkdir(home: &Path, relative_path: &str) {
        fs::create_dir_all(home.join(relative_path)).expect("create marker dir");
    }

    fn assert_only_detected(snapshot: &DetectionSnapshot, expected: ProviderId) {
        for provider in ProviderId::ALL {
            assert_eq!(
                snapshot.detected(provider),
                provider == expected,
                "unexpected detection state for {provider:?}"
            );
        }
    }

    #[test]
    fn startup_snapshot_without_home_detects_nothing() {
        let snapshot = startup_snapshot(None);
        assert_eq!(snapshot, DetectionSnapshot::default());
    }

    #[test]
    fn startup_snapshot_with_home_runs_detection() {
        let home = home();
        mkdir(home.path(), ".codex");
        let snapshot = startup_snapshot(Some(home.path().to_path_buf()));
        assert_only_detected(&snapshot, ProviderId::Codex);
    }

    #[test]
    fn detected_providers_lists_only_detected() {
        let home = home();
        mkdir(home.path(), ".codex");
        touch(home.path(), ".gemini/settings.json");
        let snapshot = detect(home.path());
        assert_eq!(
            snapshot.detected_providers(),
            vec![ProviderId::Codex, ProviderId::Gemini]
        );
    }

    #[test]
    fn default_detects_nothing() {
        let snapshot = DetectionSnapshot::default();
        for provider in ProviderId::ALL {
            assert!(!snapshot.detected(provider));
        }
    }

    #[test]
    fn empty_home_detects_nothing() {
        let home = home();
        let snapshot = detect(home.path());
        for provider in ProviderId::ALL {
            assert!(!snapshot.detected(provider));
        }
    }

    #[test]
    fn codex_dir_detects_codex() {
        let home = home();
        mkdir(home.path(), ".codex");
        assert_only_detected(&detect(home.path()), ProviderId::Codex);
    }

    #[test]
    fn claude_dir_detects_claude() {
        let home = home();
        mkdir(home.path(), ".claude");
        assert_only_detected(&detect(home.path()), ProviderId::Claude);
    }

    #[test]
    fn claude_json_file_detects_claude() {
        let home = home();
        touch(home.path(), ".claude.json");
        assert_only_detected(&detect(home.path()), ProviderId::Claude);
    }

    #[test]
    fn gemini_settings_file_detects_gemini() {
        let home = home();
        touch(home.path(), ".gemini/settings.json");
        assert_only_detected(&detect(home.path()), ProviderId::Gemini);
    }

    #[test]
    fn gemini_bare_dir_is_not_detected() {
        let home = home();
        mkdir(home.path(), ".gemini");
        let snapshot = detect(home.path());
        assert!(!snapshot.detected(ProviderId::Gemini));
    }

    #[test]
    fn gemini_settings_dir_is_not_detected() {
        let home = home();
        mkdir(home.path(), ".gemini/settings.json");
        let snapshot = detect(home.path());
        assert!(!snapshot.detected(ProviderId::Gemini));
    }

    #[test]
    fn opencode_auth_file_detects_opencode_go() {
        let home = home();
        touch(home.path(), ".local/share/opencode/auth.json");
        assert_only_detected(&detect(home.path()), ProviderId::OpenCodeGo);
    }

    #[test]
    fn cursor_config_dir_detects_cursor() {
        let home = home();
        mkdir(home.path(), ".config/Cursor");
        assert_only_detected(&detect(home.path()), ProviderId::Cursor);
    }

    #[test]
    fn copilot_config_dir_detects_copilot() {
        let home = home();
        mkdir(home.path(), ".config/github-copilot");
        assert_only_detected(&detect(home.path()), ProviderId::Copilot);
    }

    #[test]
    fn copilot_home_dir_detects_copilot() {
        let home = home();
        mkdir(home.path(), ".copilot");
        assert_only_detected(&detect(home.path()), ProviderId::Copilot);
    }

    #[test]
    fn antigravity_config_dir_detects_antigravity() {
        let home = home();
        mkdir(home.path(), ".config/Antigravity");
        assert_only_detected(&detect(home.path()), ProviderId::Antigravity);
    }

    #[test]
    fn minimax_dir_detects_minimax() {
        let home = home();
        mkdir(home.path(), ".mmx");
        assert_only_detected(&detect(home.path()), ProviderId::Minimax);
    }

    #[test]
    fn file_where_dir_expected_is_not_detected() {
        let home = home();
        touch(home.path(), ".codex");
        touch(home.path(), ".claude");
        touch(home.path(), ".config/Cursor");
        touch(home.path(), ".config/github-copilot");
        touch(home.path(), ".copilot");
        touch(home.path(), ".config/Antigravity");
        touch(home.path(), ".mmx");
        touch(home.path(), ".grok");
        let snapshot = detect(home.path());
        for provider in ProviderId::ALL {
            assert!(
                !snapshot.detected(provider),
                "{provider:?} detected from a file where a dir was expected"
            );
        }
    }

    #[test]
    fn multiple_providers_detected_together() {
        let home = home();
        mkdir(home.path(), ".codex");
        touch(home.path(), ".gemini/settings.json");
        let snapshot = detect(home.path());
        assert!(snapshot.detected(ProviderId::Codex));
        assert!(snapshot.detected(ProviderId::Gemini));
        assert!(!snapshot.detected(ProviderId::Claude));
    }

    #[test]
    fn detects_grok_from_directory() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".grok")).unwrap();
        let snapshot = detect(home.path());
        assert!(snapshot.detected(ProviderId::Grok));
    }

    #[test]
    fn detects_grok_from_auth_file() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".grok");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("auth.json"), "{}").unwrap();
        let snapshot = detect(home.path());
        assert!(snapshot.detected(ProviderId::Grok));
    }
}
