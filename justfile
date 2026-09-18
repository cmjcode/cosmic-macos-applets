# COSMIC macOS top bar — build and install recipes.
#
#   just              build release binaries
#   just install      install for the current user (~/.local)
#   just apply        install, then switch the panel to the macOS profile
#   just restore      undo `apply` from the latest backup
#   just uninstall    remove installed files (run `just restore` first)
#
#   just install-notifications   patched notification daemon, so popups can
#                                be placed (sudo, /usr/local/bin)
#
# System-wide:  sudo just prefix=/usr install

prefix := env('PREFIX', env('HOME') / '.local')
bindir := prefix / 'bin'
sharedir := prefix / 'share'
appdir := sharedir / 'applications'
icondir := sharedir / 'icons/hicolor/scalable/apps'
target := env('CARGO_TARGET_DIR', 'target') / 'release'

multicall := 'cosmic-macos-applets'
applets := 'cosmic-macos-menu cosmic-macos-active-app cosmic-macos-control-center cosmic-macos-settings'
menu_id := 'io.github.jayuda.CosmicMacosMenu'
active_id := 'io.github.jayuda.CosmicMacosActiveApp'
cc_id := 'io.github.jayuda.CosmicMacosControlCenter'
settings_id := 'io.github.jayuda.CosmicMacosSettings'

default: build

# Compile optimized binaries with the pinned dependency set.
build:
    cargo build --release --locked

# Format, lint and test.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked

# Install binaries, applet desktop entries, icons, and the patched notification daemon.
install: build install-notifications
    install -Dm0755 {{target}}/{{multicall}} {{bindir}}/{{multicall}}
    install -Dm0755 {{target}}/cosmic-macos-setup {{bindir}}/cosmic-macos-setup
    for applet in {{applets}}; do ln -sfn {{multicall}} {{bindir}}/$applet; done
    install -d {{appdir}}
    sed 's|@bindir@|{{bindir}}|g' crates/macos-applet-menu/data/{{menu_id}}.desktop.in > {{appdir}}/{{menu_id}}.desktop
    sed 's|@bindir@|{{bindir}}|g' crates/macos-applet-active-app/data/{{active_id}}.desktop.in > {{appdir}}/{{active_id}}.desktop
    sed 's|@bindir@|{{bindir}}|g' crates/macos-applet-control-center/data/{{cc_id}}.desktop.in > {{appdir}}/{{cc_id}}.desktop
    sed 's|@bindir@|{{bindir}}|g' crates/macos-settings/data/{{settings_id}}.desktop.in > {{appdir}}/{{settings_id}}.desktop
    install -Dm0644 crates/macos-applet-menu/data/icons/scalable/apps/{{menu_id}}-symbolic.svg {{icondir}}/{{menu_id}}-symbolic.svg
    install -Dm0644 crates/macos-applet-control-center/data/icons/scalable/apps/{{cc_id}}-symbolic.svg {{icondir}}/{{cc_id}}-symbolic.svg
    -gtk-update-icon-cache -qtf {{sharedir}}/icons/hicolor 2>/dev/null
    @echo "Installed to {{prefix}}. Next: just apply"

# Install all components, restart notification daemon, and switch panel to macOS profile.
apply *args: install restart-notifications
    {{bindir}}/cosmic-macos-setup apply {{args}}

# Restore the panel configuration saved by the last `apply`.
restore:
    {{bindir}}/cosmic-macos-setup restore

# --- Notification position ---------------------------------------------------
# COSMIC's notification daemon ignores its own `anchor` config key and puts
# popups at the top center when the notifications applet is not in a panel.
# patches/cosmic-notifications-anchor-fallback.patch makes it use that key in
# that case. The daemon is built from the COSMIC release the panel ships with,
# because the two share a private socket protocol: rebuild after upgrades.
notif_tag := 'epoch-1.8.0'
notif_src := absolute_path(env('CARGO_TARGET_DIR', 'target') / 'cosmic-notifications')
notif_patch := justfile_directory() / 'patches/cosmic-notifications-anchor-fallback.patch'
notif_bin := '/usr/local/bin/cosmic-notifications'

# Fetch cosmic-notifications at the pinned tag, apply the position patch, build.
build-notifications:
    [ -d {{notif_src}} ] || git clone --depth 1 --branch {{notif_tag}} https://github.com/pop-os/cosmic-notifications {{notif_src}}
    git -C {{notif_src}} checkout -- .
    git -C {{notif_src}} apply {{notif_patch}}
    rm -f {{notif_src}}/rust-toolchain.toml
    cd {{notif_src}} && CARGO_TARGET_DIR={{notif_src}}/target RUSTFLAGS="${RUSTFLAGS:-} --cfg tokio_unstable" cargo build --release --locked

# Install the patched daemon to /usr/local/bin (asks for sudo). It starts at the
# next login, or right away with `just restart-notifications`.
install-notifications: build-notifications
    sudo install -Dm0755 {{notif_src}}/target/release/cosmic-notifications {{notif_bin}}

# Stop the running daemon; cosmic-session starts the installed one and reloads the panel.
restart-notifications:
    -kill $(pidof cosmic-notifications) 2>/dev/null || true

# Remove the patched daemon. COSMIC's own is back after the next login or restart.
uninstall-notifications:
    sudo rm -f {{notif_bin}}

# Remove everything `install` created. Backups in ~/.local/state are kept.
uninstall: uninstall-notifications
    -systemctl --user disable --now cosmic-macos-window-controls.service 2>/dev/null
    rm -f {{env('XDG_CONFIG_HOME', env('HOME') / '.config')}}/systemd/user/cosmic-macos-window-controls.service
    -systemctl --user daemon-reload
    rm -f {{bindir}}/{{multicall}} {{bindir}}/cosmic-macos-setup
    for applet in {{applets}}; do rm -f {{bindir}}/$applet; done
    rm -f {{appdir}}/{{menu_id}}.desktop {{appdir}}/{{active_id}}.desktop {{appdir}}/{{cc_id}}.desktop {{appdir}}/{{settings_id}}.desktop
    rm -f {{icondir}}/{{menu_id}}-symbolic.svg {{icondir}}/{{cc_id}}-symbolic.svg
