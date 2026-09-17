# COSMIC macOS top bar — build and install recipes.
#
#   just              build release binaries
#   just install      install for the current user (~/.local)
#   just apply        install, then switch the panel to the macOS profile
#   just restore      undo `apply` from the latest backup
#   just uninstall    remove installed files (run `just restore` first)
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

# Install binaries, applet desktop entries and icons.
install: build
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

# Install and switch the COSMIC panel to the macOS profile (backs up first).
apply *args: install
    {{bindir}}/cosmic-macos-setup apply {{args}}

# Restore the panel configuration saved by the last `apply`.
restore:
    {{bindir}}/cosmic-macos-setup restore

# Remove everything `install` created. Backups in ~/.local/state are kept.
uninstall:
    -systemctl --user disable --now cosmic-macos-window-controls.service 2>/dev/null
    rm -f {{env('XDG_CONFIG_HOME', env('HOME') / '.config')}}/systemd/user/cosmic-macos-window-controls.service
    -systemctl --user daemon-reload
    rm -f {{bindir}}/{{multicall}} {{bindir}}/cosmic-macos-setup
    for applet in {{applets}}; do rm -f {{bindir}}/$applet; done
    rm -f {{appdir}}/{{menu_id}}.desktop {{appdir}}/{{active_id}}.desktop {{appdir}}/{{cc_id}}.desktop {{appdir}}/{{settings_id}}.desktop
    rm -f {{icondir}}/{{menu_id}}-symbolic.svg {{icondir}}/{{cc_id}}-symbolic.svg
