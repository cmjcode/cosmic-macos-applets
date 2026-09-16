// SPDX-License-Identifier: GPL-3.0-only
//! `com.canonical.dbusmenu` wire types and client proxy.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zvariant::{OwnedValue, Type, Value};

/// One node of `GetLayout`'s `(ia{sv}av)` result. Children are variants
/// wrapping further `LayoutItem` structures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Value, OwnedValue)]
#[zvariant(signature = "(ia{sv}av)")]
pub struct LayoutItem {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

#[zbus::proxy(interface = "com.canonical.dbusmenu", gen_blocking = false)]
pub trait DBusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[&str],
    ) -> zbus::Result<(u32, LayoutItem)>;

    fn event(&self, id: i32, event_id: &str, data: &Value<'_>, timestamp: u32) -> zbus::Result<()>;

    fn about_to_show(&self, id: i32) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn layout_updated(&self, revision: u32, parent: i32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn items_properties_updated(
        &self,
        updated_props: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}
