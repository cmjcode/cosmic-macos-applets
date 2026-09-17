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
| Window controls on the left | opt-in, `--window-controls-left` | Close, minimize and maximize on the left in GTK and Chromium apps. See below. |
| Three-finger drag | opt-in, `--three-finger-drag` | Switches on [linux-3-finger-drag](https://github.com/lmr97/linux-3-finger-drag), installed separately. See below. |

The tray, input source, battery and clock stay COSMIC's own applets. The
Control Center replaces COSMIC's audio, Bluetooth, network and notification
applets; pass `--keep-notifications` to keep notification history in the bar.

## Global menu (experimental)

With `cosmic-macos-setup apply --global-menu`, the active application applet
also shows the focused app's menus (File, Edit, View, …) next to its name.
Clicking a title drops the menu down under it; submenus open in place.

How it works: the applet hosts `com.canonical.AppMenu.Registrar`. Apps that
see it stop drawing their own menu bar and export it over D-Bus
(`com.canonical.dbusmenu`) instead. Then:

- X11 apps register their window with the registrar; the applet matches the
  focused window by X11 window id.
- Qt on Wayland does **not** register: it announces menus through KDE's
  `org_kde_kwin_appmenu` Wayland protocol, which COSMIC lacks. It still
  exports `/MenuBar/<n>` objects, so the applet finds the bus connections of
  the focused app's process (executable, `argv[0]`, Flatpak id, or the
  desktop entry's `Exec=` launcher, e.g. `libreoffice` → `soffice.bin`) and
  uses the menu bar of the focused window.

| Expected to work | Not supported |
|---|---|
| Qt 5/6 apps with a menu bar (KDE apps, qBittorrent, VLC, …), on Wayland or X11 | GTK 3/4 apps (Firefox, GNOME apps); they need `appmenu-gtk-module`, which does not work on Wayland |
| X11 apps exporting dbusmenu: JetBrains IDEs, Electron apps started with `--ozone-platform=x11` | Electron apps on native Wayland (VS Code and forks, Discord, Slack) |
| | Apps without a native menu bar (Telegram, browsers) |

Notes:

- Apps look for the registrar only at startup: restart them after enabling.
- When enabled, supporting apps hide their own menu bar. Turn it off with
  `cosmic-macos-setup apply --no-global-menu` and restart those apps. Running
  `apply` without either flag keeps the current setting.
- Apps only export a classic menu bar. LibreOffice, for example, must use
  *View › User Interface › Standard Toolbar* (not the Tabbed/Notebookbar modes)
  and the Qt backend: fully quit it, then start `SAL_USE_VCLPLUGIN=qt6 libreoffice`.
- An app with several windows exports one menu per window. On Wayland the
  applet pairs them by creation order; if the counts differ (for example an
  extra dialog window), it uses the newest menu bar.
- Every call into an app has a 2 second timeout, so a frozen app cannot freeze
  the panel. If another registrar already owns the name (for example KDE's),
  the applet uses it instead and takes over if it goes away.

## Window controls on the left

```sh
cosmic-macos-setup apply --window-controls-left     # close, minimize, maximize on the left
cosmic-macos-setup apply --window-controls-right    # back to COSMIC's default
```

This sets GNOME's `button-layout` to `close,minimize,maximize:`. Buttons you
turned off in *Settings › Desktop › Windows* stay off.

cosmic-settings-daemon writes a right-side layout on every login and whenever
those toggles change, so a plain `gsettings set` does not last. The flag
installs the user service `cosmic-macos-window-controls.service`, which runs
`cosmic-macos-setup window-controls-watch` and puts the left layout back each
time. It sleeps between changes and uses no CPU.

| Moves to the left | Stays on the right |
|---|---|
| GTK 3/4 and libadwaita apps (Files from GNOME, Firefox, …) | COSMIC apps (Files, Terminal, Settings): libcosmic always draws them on the right |
| Chromium, Electron and VS Code when they use the GTK title bar | Windows with COSMIC's server-side title bar (most Qt apps) |

Both "stays on the right" cases need changes in COSMIC itself; see
[pop-os/cosmic-epoch#640](https://github.com/pop-os/cosmic-epoch/issues/640).
Running apps pick the change up live; a few need a restart.

## Three-finger drag

Hold three fingers on the touchpad and move to drag a window or select text,
as on a Mac. libinput supports this since 1.28, but only when the compositor
turns it on, and cosmic-comp has no setting for it yet. Until it does, the
separate project [linux-3-finger-drag](https://github.com/lmr97/linux-3-finger-drag)
does it below the compositor: it takes over the touchpad through evdev and
turns a three-finger touch into a left-button drag on a virtual device.

It lives in its own repository because it needs root to install and works
with any desktop. Install it once:

```sh
git clone https://github.com/lmr97/linux-3-finger-drag
cd linux-3-finger-drag
sudo ./install.sh     # udev rule for /dev/uinput, adds you to `input`, user service
reboot                # the new group only applies after logging in again
```

Then manage it together with the rest of the profile:

```sh
cosmic-macos-setup apply --three-finger-drag       # checks access, enables the service
cosmic-macos-setup apply --no-three-finger-drag    # disables it
```

`apply --three-finger-drag` lists anything that is still missing before it
changes anything. Notes:

- Four-finger swipes still switch workspaces. COSMIC does not use three-finger
  swipes, so nothing conflicts.
- With tap-to-click on, a three-finger touch waits about 50 ms to tell a tap
  from a drag. Tune `entryDebounce` and the other timings in
  `~/.config/linux-3-finger-drag/3fd-config.json`.
- Being in the `input` group lets your programs read every input device,
  keyboards included. That is how the tool works; do not add untrusted users.
- Its logs: `journalctl --user -u three-finger-drag`.

## Adding global menu support to your app

This section is for app developers. The applet shows a menu in the top bar when
the app **exports** its menu bar over D-Bus and the applet can tell which
window it belongs to.

### What the applet needs from your app

1. **A menu exported with `com.canonical.dbusmenu`** on the session bus, at
   `/MenuBar/<n>`: one object per window, numbered in window creation order
   (`/MenuBar/1`, `/MenuBar/2`, …).
2. **A way to pair the window with that menu:**
   - **Wayland (default for most apps):** nothing to call. The applet looks for
     `/MenuBar/<n>` on bus connections owned by the focused window's process.
     The process must be recognizable: its executable name, `argv[0]`, or
     Flatpak id matches the window's app id, or the app's `.desktop` entry
     `Exec=` launches it. Simplest rule: **app id = `.desktop` file name =
     executable name**.
   - **X11 / XWayland:** call `RegisterWindow(x11_window_id, "/MenuBar/<n>")`
     on `com.canonical.AppMenu.Registrar` (object
     `/com/canonical/AppMenu/Registrar`).
3. **Hide the in-window menu bar only when a host is present.** Check at
   startup whether `com.canonical.AppMenu.Registrar` has an owner. If it does
   not, keep your normal menu bar so the app still works on other desktops.

### `com.canonical.dbusmenu` in short

| Member | Kind | What to do |
|---|---|---|
| `GetLayout(i parentId, i depth, as props) → (u revision, (ia{sv}av) layout)` | method | Return the tree. The root has id `0`; children are variants wrapping `(ia{sv}av)` structs. Returning the whole tree regardless of arguments is fine. |
| `Event(i id, s eventId, v data, u timestamp)` | method | `eventId == "clicked"` means the entry was activated. `"opened"`/`"closed"` are informational. |
| `AboutToShow(i id) → b needUpdate` | method | Return `true` only if you changed the submenu and the panel must reload. |
| `LayoutUpdated(u revision, i parent)` | signal | Emit after any change (labels, checkmarks, enabled state) with an increased revision. |
| `ItemsPropertiesUpdated(a(ia{sv}) updated, a(ias) removed)` | signal | Optional finer-grained update; the applet treats it like `LayoutUpdated`. |
| `Version` (u, `3`), `Status` (s, `"normal"`) | properties | Expected by other hosts such as KDE. |

Entry properties (`a{sv}`), all optional:

| Key | Type | Meaning |
|---|---|---|
| `label` | s | Text; `_` marks the mnemonic (`_File`), `__` is a literal underscore |
| `type` | s | `"separator"` for a separator, otherwise omit |
| `enabled` / `visible` | b | Default `true` |
| `children-display` | s | `"submenu"` if the entry opens a submenu |
| `toggle-type` / `toggle-state` | s / i | `"checkmark"` or `"radio"`; state `1` = on, `0` = off |
| `shortcut` | aas | Display only, e.g. `[["Control", "S"]]` |
| `icon-name` | s | Ignored by this applet, used by other hosts |

For more interfaces beyond what this applet uses (`GetGroupProperties`,
`EventGroup`, `AboutToShowGroup`), see the dbusmenu specification in
[libdbusmenu](https://github.com/AyatanaIndicators/libdbusmenu/blob/master/libdbusmenu-glib/dbus-menu.xml).

### Per technology

"Tested here" means it was checked on COSMIC epoch 1.8 while building this
project; everything else follows from how the toolkit works but has not been
tried yet. Reports are welcome.

| Technology | Support | What to do | Tested here |
|---|---|---|---|
| **Qt 5 / Qt 6** (C++, PyQt, PySide, KDE Frameworks) | Built in | Use a normal `QMenuBar` (`QMainWindow::menuBar()`). Do not call `setNativeMenuBar(false)` or set `Qt::AA_DontUseNativeMenuBar`. Set `QGuiApplication::setDesktopFileName("your-app")` to match your `.desktop` file. Qt hides the in-window bar by itself. | Menus exported by LibreOffice's Qt 6 backend |
| **LibreOffice** | Built in (Qt backend only) | Users start it with `SAL_USE_VCLPLUGIN=qt6` and pick *View › User Interface › Standard Toolbar*. The Tabbed/Notebookbar modes have no menu bar. | Export verified |
| **Rust: egui / eframe** | Manual, ~200 lines | Follow the [tutorial below](#tutorial-egui--eframe). | Export, clicks and updates verified with `busctl` |
| **Rust: iced, libcosmic, Slint, Dioxus desktop, …** | Manual | Same module as the egui tutorial; turn the click callback into your framework's message (for iced/libcosmic, send it through a channel subscription). | No |
| **Electron** (VS Code, Discord, …) | X11 only | Chromium exports `Menu.setApplicationMenu` menus only on X11: start with `--ozone-platform=x11`. Apps with a custom title bar need a native one (VS Code: `"window.titleBarStyle": "native"`). | No |
| **Tauri** | Manual | Tauri's Linux menus are GTK widgets inside the window and are not exported. Build the menu with the Rust module from the egui tutorial instead and forward clicks to your frontend with events. | No |
| **JetBrains IDEs** | X11 only | Works when the IDE runs through XWayland (the default AWT toolkit); not with the Wayland toolkit (`-Dawt.toolkit.name=WLToolkit`). | No |
| **GTK 3** | X11 only, needs a module | Install `appmenu-gtk-module`, run with `GTK_MODULES=appmenu-gtk-module GDK_BACKEND=x11`. Nothing works on native Wayland. | No |
| **GTK 4 / libadwaita** | Not supported | GTK 4 exports `GMenuModel` over `org.gtk.Menus`, which this applet does not read. | No |
| **Java Swing / JavaFX** | Not supported | No built-in export. Community agents such as JAyatana exist for X11. | No |
| **Other languages** | Manual | Any D-Bus library works (`sdbus-c++`, `dasbus`/`dbus-next` for Python, `godbus` for Go, …): implement the table above and export `/MenuBar/1`. | No |

### Tutorial: egui / eframe

A complete, runnable version is in
[`examples/egui-global-menu`](examples/egui-global-menu)
(`cd examples/egui-global-menu && cargo run`). The steps:

**1. Dependencies** (tested with eframe 0.36 and zbus 5):

```toml
[dependencies]
eframe = "0.36"
serde = { version = "1", features = ["derive"] }
zbus = "5"
```

**2. Add `src/global_menu.rs`.** It exports a menu tree, forwards clicks to a
callback, and lets you update the menu. It has no egui dependency.

<details>
<summary><code>src/global_menu.rs</code> (click to expand)</summary>

```rust
//! Export a menu bar over `com.canonical.dbusmenu` so desktop panels
//! (COSMIC macOS Top Bar, KDE, Unity-style panels) can show it.
//!
//! Drop this file into your crate; it only depends on `zbus` and `serde`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use zbus::{
    blocking::{Connection, connection},
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Type, Value},
};

/// Object path panels look for. Use `/MenuBar/2`, `/MenuBar/3`, … for more
/// windows, numbered in the order the windows were created.
pub const MENU_PATH: &str = "/MenuBar/1";
const REGISTRAR: &str = "com.canonical.AppMenu.Registrar";

/// One entry of the menu tree. Ids must be unique and non-zero (0 is the root).
#[derive(Debug, Clone)]
pub enum MenuItem {
    Entry {
        id: i32,
        /// `_` marks the mnemonic: `"_File"`.
        label: String,
        enabled: bool,
        /// `Some(true/false)` shows a checkmark toggle.
        checked: Option<bool>,
        /// Display-only shortcut, e.g. `&["Control", "S"]`.
        shortcut: Vec<String>,
        children: Vec<MenuItem>,
    },
    Separator,
}

impl MenuItem {
    pub fn action(id: i32, label: &str) -> Self {
        Self::Entry {
            id,
            label: label.into(),
            enabled: true,
            checked: None,
            shortcut: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn submenu(id: i32, label: &str, children: Vec<MenuItem>) -> Self {
        Self::Entry {
            id,
            label: label.into(),
            enabled: true,
            checked: None,
            shortcut: Vec::new(),
            children,
        }
    }

    pub fn shortcut(mut self, keys: &[&str]) -> Self {
        if let Self::Entry { shortcut, .. } = &mut self {
            *shortcut = keys.iter().map(|k| (*k).to_owned()).collect();
        }
        self
    }
}

/// Wire format of `GetLayout`: `(ia{sv}av)`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Value, OwnedValue)]
#[zvariant(signature = "(ia{sv}av)")]
struct Layout {
    id: i32,
    properties: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(value.into()).expect("plain values never carry file descriptors")
}

fn to_layout(
    id: i32,
    item: Option<&MenuItem>,
    children: &[MenuItem],
    separator_id: &mut i32,
) -> Layout {
    let mut properties = HashMap::new();
    if let Some(MenuItem::Entry {
        label,
        enabled,
        checked,
        shortcut,
        ..
    }) = item
    {
        properties.insert("label".into(), owned(label.as_str()));
        properties.insert("enabled".into(), owned(*enabled));
        if let Some(state) = checked {
            properties.insert("toggle-type".into(), owned("checkmark"));
            properties.insert("toggle-state".into(), owned(i32::from(*state)));
        }
        if !shortcut.is_empty() {
            properties.insert("shortcut".into(), owned(vec![shortcut.clone()]));
        }
    }
    if !children.is_empty() {
        properties.insert("children-display".into(), owned("submenu"));
    }
    let children = children
        .iter()
        .map(|child| {
            let layout = match child {
                MenuItem::Entry { id, children, .. } => {
                    to_layout(*id, Some(child), children, separator_id)
                }
                MenuItem::Separator => {
                    // Separators need ids too; hand out negative ones.
                    *separator_id -= 1;
                    let mut properties = HashMap::new();
                    properties.insert("type".into(), owned("separator"));
                    Layout {
                        id: *separator_id,
                        properties,
                        children: Vec::new(),
                    }
                }
            };
            owned(layout)
        })
        .collect();
    Layout {
        id,
        properties,
        children,
    }
}

struct State {
    revision: u32,
    items: Vec<MenuItem>,
}

struct DbusMenu {
    state: Arc<Mutex<State>>,
    on_click: Box<dyn Fn(i32) + Send + Sync>,
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    async fn get_layout(
        &self,
        _parent_id: i32,
        _depth: i32,
        _properties: Vec<String>,
    ) -> (u32, Layout) {
        let state = self.state.lock().unwrap();
        (state.revision, to_layout(0, None, &state.items, &mut 0))
    }

    async fn event(&self, id: i32, event_id: String, _data: OwnedValue, _timestamp: u32) {
        if event_id == "clicked" {
            (self.on_click)(id);
        }
    }

    async fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    #[zbus(property)]
    async fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    async fn status(&self) -> String {
        "normal".into()
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

/// A menu bar exported on the session bus. Keep it alive as long as the window.
pub struct GlobalMenu {
    connection: Connection,
    state: Arc<Mutex<State>>,
}

impl GlobalMenu {
    /// Export `items`; `on_click` receives the id of a clicked entry (called
    /// from a D-Bus thread, so forward it to your UI thread).
    pub fn export(
        items: Vec<MenuItem>,
        on_click: impl Fn(i32) + Send + Sync + 'static,
    ) -> zbus::Result<Self> {
        let state = Arc::new(Mutex::new(State { revision: 1, items }));
        let menu = DbusMenu {
            state: state.clone(),
            on_click: Box::new(on_click),
        };
        let connection = connection::Builder::session()?
            .serve_at(MENU_PATH, menu)?
            .build()?;
        Ok(Self { connection, state })
    }

    /// `true` if a global menu host is running, i.e. the panel shows this menu
    /// and the in-window menu bar should be hidden. Checked at startup.
    pub fn panel_available(&self) -> bool {
        zbus::blocking::fdo::DBusProxy::new(&self.connection)
            .and_then(|dbus| Ok(dbus.name_has_owner(REGISTRAR.try_into()?)?))
            .unwrap_or(false)
    }

    /// Replace the menu (e.g. after toggling a checkmark) and notify the panel.
    pub fn set_items(&self, items: Vec<MenuItem>) -> zbus::Result<()> {
        let revision = {
            let mut state = self.state.lock().unwrap();
            state.items = items;
            state.revision += 1;
            state.revision
        };
        let emitter = SignalEmitter::new(self.connection.inner(), MENU_PATH)?;
        zbus::block_on(DbusMenu::layout_updated(&emitter, revision, 0))
    }

    /// Only needed when running under X11/XWayland: register the X11 window id.
    #[allow(dead_code)]
    pub fn register_x11_window(&self, x11_window_id: u32) -> zbus::Result<()> {
        self.connection.call_method(
            Some(REGISTRAR),
            "/com/canonical/AppMenu/Registrar",
            Some(REGISTRAR),
            "RegisterWindow",
            &(
                x11_window_id,
                zbus::zvariant::ObjectPath::try_from(MENU_PATH)?,
            ),
        )?;
        Ok(())
    }
}
```

</details>

**3. Describe your menu** with stable ids (they come back in clicks):

```rust
mod global_menu;
use global_menu::{GlobalMenu, MenuItem};

const OPEN: i32 = 10;
const QUIT: i32 = 11;

fn menu_items() -> Vec<MenuItem> {
    vec![
        MenuItem::submenu(1, "_File", vec![
            MenuItem::action(OPEN, "_Open…").shortcut(&["Control", "O"]),
            MenuItem::Separator,
            MenuItem::action(QUIT, "_Quit").shortcut(&["Control", "Q"]),
        ]),
    ]
}
```

**4. Export it when the app starts.** Clicks arrive on a D-Bus thread, so send
them through a channel and wake egui:

```rust
use std::sync::mpsc::{Receiver, channel};

struct App {
    global_menu: Option<GlobalMenu>,
    menu_in_panel: bool, // checked once; never call D-Bus every frame
    clicks: Receiver<i32>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, clicks) = channel();
        let ctx = cc.egui_ctx.clone();
        let global_menu = GlobalMenu::export(menu_items(), move |id| {
            let _ = tx.send(id);
            ctx.request_repaint();
        })
        .ok(); // no session bus: just keep the in-window menu
        let menu_in_panel = global_menu.as_ref().is_some_and(GlobalMenu::panel_available);
        Self { global_menu, menu_in_panel, clicks }
    }
}
```

**5. Handle clicks, and draw the egui menu bar only as a fallback:**

```rust
use eframe::egui;

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(id) = self.clicks.try_recv() {
            match id {
                QUIT => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                OPEN => { /* … */ }
                _ => {}
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.menu_in_panel {
            egui::Panel::top("menu").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| { /* same actions */ });
                });
            });
        }
        egui::CentralPanel::default_margins().show(ui, |ui| { /* your app */ });
    }
}
```

**6. Keep dynamic state in sync.** After toggling a checkmark or enabling an
entry, rebuild the items and call `global_menu.set_items(new_items)`; it bumps
the revision and emits `LayoutUpdated`.

**7. Give the window an app id that matches your `.desktop` file and binary:**

```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default().with_app_id("my-app"),
    ..Default::default()
};
eframe::run_native("my-app", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
```

```ini
# ~/.local/share/applications/my-app.desktop
[Desktop Entry]
Type=Application
Name=My App
Exec=my-app
```

**8. Multiple windows:** export one `GlobalMenu`-style object per window at
`/MenuBar/1`, `/MenuBar/2`, … in the order the windows are created (the module
uses a constant path; make it a parameter). **X11:** if you force XWayland,
also call `register_x11_window` with the window's X11 id.

### Checking your integration

```sh
# Is a global menu host running? (shows the applet's PID)
busctl --user status com.canonical.AppMenu.Registrar | grep PID

# Does your app export a menu? Replace <PID> with your app's process id.
for name in $(busctl --user list --no-pager | awk -v p=<PID> '$2==p{print $1}'); do
  busctl --user tree "$name" --list --no-pager | grep MenuBar
done

# Read the menu and simulate a click on entry 11
busctl --user -- call <name> /MenuBar/1 com.canonical.dbusmenu GetLayout iias 0 -1 0
busctl --user call <name> /MenuBar/1 com.canonical.dbusmenu Event isvu 11 clicked i 0 0
```

If the menu is exported but not shown, the window could not be paired with the
process: check that the app id, `.desktop` file name and executable name agree.

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
cosmic-macos-setup apply              # apply (options: --opacity 0.9, --no-weekday, --keep-notifications, --global-menu,
                                      #   --window-controls-left, --three-finger-drag)
```

System-wide: `sudo just prefix=/usr install`.

## Undo and uninstall

```sh
cosmic-macos-setup restore            # undo the latest `apply`
cosmic-macos-setup restore --first    # back to the panel you had before this tool
just uninstall
```

Backups live in `~/.local/state/cosmic-macos-applet/backups/<timestamp>/`
as plain copies of the COSMIC config files. Each backup also records whether
the window-controls and three-finger-drag services were on, and `restore`
switches them back to that state. `just uninstall` removes the window-controls
service; linux-3-finger-drag is uninstalled from its own repository.

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
| Active app | `global_menu` | `false` | Experimental global menu, see above |
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

The global menu has an end-to-end test that starts a private `dbus-daemon`,
registers a fake exporter, and checks matching, layout loading, click
forwarding and cleanup (`cargo test -p macos-applet-active-app global_menu`).
`COSMIC_MACOS_MENU_OPEN=<title index>` opens a global menu a few seconds after
the applet starts.

To screenshot a Control Center page without clicking, launch the applet with
`COSMIC_MACOS_CC_OPEN_PAGE=main|wifi|bluetooth|sound` (e.g. by editing its
`Exec` line temporarily); the popup opens a few seconds after start.

COSMIC git dependencies are pinned in `[patch]` sections of `Cargo.toml` to the
commits shipped with COSMIC epoch 1.8.0. When upgrading COSMIC, update those
revisions together with the matching `pop-os/cosmic-applets` release.

## Limitations

- The global menu is experimental and only covers apps listed above.
- Window controls move to the left only in GTK and Chromium-based apps.
- Three-finger drag depends on a separate tool with root-installed access
  rules, until cosmic-comp exposes libinput's native setting.
- Quit closes every window of the app. Background processes may keep running.
- Wi-Fi networks that need a new password open COSMIC Settings; saved and open
  networks connect directly from the popup. New Bluetooth devices are paired in
  Settings too.
- COSMIC's panel cannot blur what is behind it, so tiles use the theme colors
  with transparency instead of macOS' frosted glass. Active toggles use your
  theme's accent color.

## License

GPL-3.0-only, like libcosmic and cosmic-applets.
