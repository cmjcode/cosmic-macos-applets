# COSMIC macOS Top Bar

A macOS-style menu bar for the [COSMIC](https://system76.com/cosmic) desktop.
It adds three panel applets and a setup tool that restyles the existing top panel.

```
[◉]  Firefox                                 ▣ us ♿ 🔋  [⊜]  Wed, Sep 16, 4:06 PM
 │     │                                             │     └ clock at the far right
 │     │                                             └ Control Center
 │     └ focused application, bold (click: Hide / Quit)
 └ system menu: About, System Settings, App Store, Sleep, Restart, Shut Down, Lock, Log Out
```

The Control Center popup mirrors macOS:

```
┌───────────────────────────────┬───────────────┐
│ (◉) Wi-Fi      Home-Network   │ Now Playing   │
│ (◉) Bluetooth  AirPods        │ ⏮  ⏯  ⏭      │
├───────────────────────┬───────┼───────┬───────┤
│ (◎) Focus  Off        │  ☾    │  ⎙    │       │
├───────────────────────┴───────┴───────┴───────┤
│ Display   ☀ ━━━━━━━━━━━━━━━━━━━○──────  80%   │
│ Sound     🔊 ━━━━━━━━━━○──────────────────  ›  │
├───────┬───────┬───────┬───────────────────────┤
│  🔒   │  🖥   │  ⚙    │   🔋 92%              │
└───────┴───────┴───────┴───────────────────────┘
```

Clicking the Wi-Fi, Bluetooth or ⟩ labels opens a detail page: visible
networks, paired devices, or audio outputs.

## What you get

| Part | Kind | Notes |
|---|---|---|
| System menu | applet `io.github.jayuda.CosmicMacosMenu` | Restart, Shut Down and Log Out confirm through `cosmic-osd`. App Store is hidden when `cosmic-store` is missing. |
| Active application | applet `io.github.jayuda.CosmicMacosActiveApp` | Follows focus through the Wayland toplevel protocols, event driven with no polling. Resolves names from `.desktop` files, off the UI thread. |
| Control Center | applet `io.github.jayuda.CosmicMacosControlCenter` | Wi-Fi via NetworkManager, Bluetooth via BlueZ, volume and output via cosmic-settings-daemon, brightness, Now Playing via MPRIS, Do Not Disturb, dark mode, screenshot, lock, battery. Every service reconnects on its own. |
| `cosmic-macos-setup` | CLI | Backs up and then rewrites the panel config. The panel reloads live. `restore` undoes it. |

The tray, input source, battery and clock stay COSMIC's own applets. The
Control Center replaces COSMIC's audio, Bluetooth, network and notification
applets; pass `--keep-notifications` to keep notification history in the bar.

## Requirements

- COSMIC epoch 1.8 (Wayland session)
- Rust 1.93 or newer
- [`just`](https://github.com/casey/just) (`pacman -S just`)
- Build dependencies of libcosmic: `libxkbcommon`, `wayland`, `pkgconf`, `fontconfig`, `freetype2`, `expat`, `mesa`

## Install

```sh
just apply            # build, install to ~/.local, back up and apply the profile
```

Or step by step:

```sh
just install                          # ~/.local/bin, ~/.local/share/applications
cosmic-macos-setup apply --dry-run    # preview every config change
cosmic-macos-setup apply              # apply (options: --opacity 0.9, --no-weekday, --keep-notifications)
```

System-wide: `sudo just prefix=/usr install`.

## Undo and uninstall

```sh
cosmic-macos-setup restore            # undo the latest `apply`
cosmic-macos-setup restore --first    # back to the panel you had before this tool
just uninstall
```

Backups live in `~/.local/state/cosmic-macos-applet/backups/<timestamp>/`
as plain copies of the COSMIC config files.

## Configuration

Both applets hot-reload their settings from `~/.config/cosmic/<applet id>/v1/`.

| Applet | Key | Default | Meaning |
|---|---|---|---|
| Menu | `icon_name` | built-in glyph | Any icon theme name, e.g. `"distributor-logo-archlinux"` |
| Menu | `show_about`, `show_app_store` | `true` | Toggle entries |
| Menu | `confirm_power_actions` | `true` | Confirm restart, shut down and log out |
| Active app | `max_chars` | `32` | Longer names are ellipsized |
| Active app | `bold` | `true` | Bold label |
| Active app | `empty_label` | `""` | Label with nothing focused; empty means "Desktop" |
| Active app | `follow_panel_output` | `true` | On multi-monitor setups, show the last active app of this monitor |
| Control Center | `sections` | all | Order of `Connectivity`, `Toggles`, `Display`, `Sound`, `Shortcuts` |
| Control Center | `show_now_playing` | `true` | Show the media card |
| Control Center | `max_volume` | `100` | Slider limit, 100 to 150 |

Example:

```sh
echo '"distributor-logo-archlinux"' > ~/.config/cosmic/io.github.jayuda.CosmicMacosMenu/v1/icon_name
echo '[Connectivity, Sound, Shortcuts]' > ~/.config/cosmic/io.github.jayuda.CosmicMacosControlCenter/v1/sections
```

Set `COSMIC_MACOS_LOG=debug` in the session environment for verbose logs;
applet output goes to the `cosmic-panel` journal.

## Development

```sh
just check    # rustfmt, clippy -D warnings, tests
```

| Crate | Purpose |
|---|---|
| `macos-common` | Config types, desktop-entry index, logging |
| `macos-applet-menu` | System menu applet |
| `macos-applet-active-app` | Focused-application applet and its Wayland thread |
| `macos-applet-control-center` | Control Center applet; one service module per backend |
| `cosmic-macos-applets` | Multi-call binary, so libcosmic ships once |
| `macos-setup` | `cosmic-macos-setup` CLI |

To screenshot a Control Center page without clicking, launch the applet with
`COSMIC_MACOS_CC_OPEN_PAGE=main|wifi|bluetooth|sound` (e.g. by editing its
`Exec` line temporarily); the popup opens a few seconds after start.

COSMIC git dependencies are pinned in `[patch]` sections of `Cargo.toml` to the
commits shipped with COSMIC epoch 1.8.0. When upgrading COSMIC, update those
revisions together with the matching `pop-os/cosmic-applets` release.

## Limitations

- There is no global application menu (File, Edit, View). COSMIC has no
  AppMenu registrar, so the second applet shows the app name with Hide and Quit.
- Quit closes every window of the app. Background processes may keep running.
- Wi-Fi networks that need a new password open COSMIC Settings; saved and open
  networks connect directly from the popup. New Bluetooth devices are paired in
  Settings too.
- COSMIC's panel cannot blur what is behind it, so tiles use the theme colors
  with transparency instead of macOS' frosted glass. Active toggles use your
  theme's accent color.

## License

GPL-3.0-only, like libcosmic and cosmic-applets.
