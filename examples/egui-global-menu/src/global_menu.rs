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
