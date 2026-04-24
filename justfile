name := 'yapcap'
appid := 'com.topi.YapCap'

rootdir := ''
prefix := home_directory() / '.local'

# Installation paths
base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
appdata-dst := base-dir / 'share' / 'appdata' / appid + '.metainfo.xml'
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / appid + '.desktop'
icon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Clears the app cache at ~/.cache/yapcap
clear-cache:
    rm -rf ~/.cache/yapcap

# Clears Cursor session dirs (cookie_header, managed profile Cookies DB) under ~/.local/state/yapcap/cursor-accounts

# Clears cookie/session dirs, snapshot cache, and full managed state (codex/claude/cursor/logs under ~/.local/state/yapcap)
clear-all-data: clear-config clear-cache clear-accounts

# Clears COSMIC config at $XDG_CONFIG_HOME/cosmic/<appid> (default ~/.config/...)
clear-config:
    rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/cosmic/{{ appid }}"

# Clears managed account state at ~/.local/state/yapcap
clear-accounts:
    rm -rf ~/.local/state/yapcap

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release {{args}}

# Runs with empty HOME/XDG dirs so provider discovery finds nothing
run-empty-discovery *args:
    rm -rf /tmp/yapcap-empty-home /tmp/yapcap-empty-config /tmp/yapcap-empty-state
    mkdir -p /tmp/yapcap-empty-home /tmp/yapcap-empty-config /tmp/yapcap-empty-state
    env RUST_BACKTRACE=full HOME=/tmp/yapcap-empty-home XDG_CONFIG_HOME=/tmp/yapcap-empty-config XDG_STATE_HOME=/tmp/yapcap-empty-state CARGO_HOME="${CARGO_HOME:-{{home_directory() / '.cargo'}}}" RUSTUP_HOME="${RUSTUP_HOME:-{{home_directory() / '.rustup'}}}" cargo run --release {{args}}

# Installs files
install: build-release
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 resources/app.desktop {{desktop-dst}}
    install -Dm0644 resources/app.metainfo.xml {{appdata-dst}}
    install -Dm0644 resources/icon.svg {{icon-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{appdata-dst}} {{icon-dst}}

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    echo >> .cargo/config.toml
    rm -rf .cargo vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    find -type f -name Cargo.toml -exec sed -i '0,/^version/s/^version.*/version = "{{version}}"/' '{}' \; -exec git add '{}' \;
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git tag -a {{version}} -m ''
