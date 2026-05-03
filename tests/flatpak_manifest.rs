// SPDX-License-Identifier: MPL-2.0

use serde_json::Value;

fn manifest() -> Value {
    let path = format!(
        "{}/packaging/com.topi.YapCap.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("flatpak manifest should be readable");
    serde_json::from_str(&text).expect("flatpak manifest should be valid JSON")
}

fn strings_at<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .expect("manifest key should be an array")
        .iter()
        .map(|item| item.as_str().expect("array values should be strings"))
        .collect()
}

#[test]
fn flatpak_manifest_installs_cosmic_applet_metadata() {
    let manifest = manifest();
    assert_eq!(manifest["app-id"], "com.topi.YapCap");
    assert_eq!(manifest["command"], "yapcap");
    assert_eq!(manifest["runtime-version"], "25.08");
    assert!(
        strings_at(&manifest, "sdk-extensions")
            .contains(&"org.freedesktop.Sdk.Extension.rust-stable")
    );

    let commands = strings_at(&manifest["modules"][0], "build-commands").join("\n");
    assert!(commands.contains("/app/bin/yapcap"));
    assert!(commands.contains("/app/share/applications/com.topi.YapCap.desktop"));
    assert!(commands.contains("/app/share/metainfo/com.topi.YapCap.metainfo.xml"));
    assert!(commands.contains("/app/share/icons/hicolor/scalable/apps/com.topi.YapCap.svg"));
}

#[test]
fn flatpak_manifest_keeps_runtime_permissions_narrow() {
    let manifest = manifest();
    let finish_args = strings_at(&manifest, "finish-args");

    assert!(finish_args.contains(&"--share=network"));
    assert!(finish_args.contains(&"--share=ipc"));
    assert!(finish_args.contains(&"--socket=wayland"));
    assert!(finish_args.contains(&"--socket=session-bus"));
    assert!(finish_args.contains(&"--filesystem=xdg-config/cosmic:rw"));
    assert!(
        !finish_args
            .iter()
            .any(|arg| arg.starts_with("--filesystem=host"))
    );
    assert!(!finish_args.iter().any(|arg| arg == &"--filesystem=home"));
    assert!(
        !finish_args
            .iter()
            .any(|arg| arg == &"--talk-name=org.freedesktop.Flatpak")
    );
}
