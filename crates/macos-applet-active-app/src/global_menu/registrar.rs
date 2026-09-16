// SPDX-License-Identifier: GPL-3.0-only
//! `com.canonical.AppMenu.Registrar` server and client proxy.
//!
//! Registrations are keyed by `(sender, window id)`, not only the window id:
//! on Wayland, Qt reports toolkit-internal ids that can collide between
//! processes. Entries of a client that leaves the bus are dropped.

use std::sync::{Arc, Mutex};

use zbus::{
    fdo,
    message::Header,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedObjectPath},
};

pub const BUS_NAME: &str = "com.canonical.AppMenu.Registrar";
pub const OBJECT_PATH: &str = "/com/canonical/AppMenu/Registrar";

/// Upper bound on stored registrations, so a misbehaving client cannot grow
/// the registrar without limit.
const MAX_REGISTRATIONS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub window_id: u32,
    /// Unique bus name of the exporting client.
    pub service: String,
    pub path: OwnedObjectPath,
    /// Monotonic registration order; higher is newer.
    pub seq: u64,
}

/// Pure registration table.
#[derive(Debug, Default)]
pub struct Registry {
    entries: Vec<Registration>,
    next_seq: u64,
}

impl Registry {
    /// Add or replace the registration of `window_id` by `service`.
    pub fn register(&mut self, window_id: u32, service: &str, path: OwnedObjectPath) {
        self.entries
            .retain(|r| !(r.window_id == window_id && r.service == service));
        if self.entries.len() >= MAX_REGISTRATIONS {
            self.entries.remove(0);
        }
        self.next_seq += 1;
        self.entries.push(Registration {
            window_id,
            service: service.to_owned(),
            path,
            seq: self.next_seq,
        });
    }

    /// Remove `window_id` for `service`; returns `true` if something was removed.
    pub fn unregister(&mut self, window_id: u32, service: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|r| !(r.window_id == window_id && r.service == service));
        before != self.entries.len()
    }

    /// Drop everything registered by a client that left the bus; returns the
    /// window ids that disappeared.
    pub fn remove_service(&mut self, service: &str) -> Vec<u32> {
        let mut removed = Vec::new();
        self.entries.retain(|r| {
            let keep = r.service != service;
            if !keep {
                removed.push(r.window_id);
            }
            keep
        });
        removed
    }

    /// The newest registration for `window_id`.
    #[must_use]
    pub fn for_window(&self, window_id: u32) -> Option<&Registration> {
        self.entries
            .iter()
            .filter(|r| r.window_id == window_id)
            .max_by_key(|r| r.seq)
    }

    #[must_use]
    pub fn all(&self) -> &[Registration] {
        &self.entries
    }
}

/// D-Bus object implementing the registrar interface.
#[derive(Debug, Clone, Default)]
pub struct Registrar {
    pub registry: Arc<Mutex<Registry>>,
}

impl Registrar {
    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        // A poisoned lock only means another handler panicked mid-update; the
        // table itself is always structurally valid.
        self.registry.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forget a client that disconnected and announce its windows as gone.
    pub async fn client_vanished(&self, emitter: &SignalEmitter<'_>, service: &str) {
        let removed = self.lock().remove_service(service);
        for window_id in removed {
            let _ = Self::window_unregistered(emitter, window_id).await;
        }
    }
}

#[zbus::interface(name = "com.canonical.AppMenu.Registrar")]
impl Registrar {
    async fn register_window(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        window_id: u32,
        menu_object_path: OwnedObjectPath,
    ) -> fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("message has no sender".into()))?
            .to_string();
        if menu_object_path.as_str() == "/" {
            return Err(fdo::Error::InvalidArgs(
                "menu object path must not be /".into(),
            ));
        }
        tracing::debug!(window_id, %sender, path = %menu_object_path, "RegisterWindow");
        self.lock()
            .register(window_id, &sender, menu_object_path.clone());
        let _ =
            Self::window_registered(&emitter, window_id, &sender, menu_object_path.as_ref()).await;
        Ok(())
    }

    async fn unregister_window(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        window_id: u32,
    ) -> fdo::Result<()> {
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        if self.lock().unregister(window_id, &sender) {
            let _ = Self::window_unregistered(&emitter, window_id).await;
        }
        Ok(())
    }

    async fn get_menu_for_window(&self, window_id: u32) -> fdo::Result<(String, OwnedObjectPath)> {
        self.lock()
            .for_window(window_id)
            .map(|r| (r.service.clone(), r.path.clone()))
            .ok_or_else(|| fdo::Error::Failed(format!("no menu registered for window {window_id}")))
    }

    async fn get_menus(&self) -> Vec<(u32, String, OwnedObjectPath)> {
        self.lock()
            .all()
            .iter()
            .map(|r| (r.window_id, r.service.clone(), r.path.clone()))
            .collect()
    }

    #[zbus(signal)]
    async fn window_registered(
        emitter: &SignalEmitter<'_>,
        window_id: u32,
        service: &str,
        menu_object_path: ObjectPath<'_>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn window_unregistered(emitter: &SignalEmitter<'_>, window_id: u32) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "com.canonical.AppMenu.Registrar",
    default_service = "com.canonical.AppMenu.Registrar",
    default_path = "/com/canonical/AppMenu/Registrar",
    gen_blocking = false
)]
pub trait RegistrarClient {
    fn get_menus(&self) -> zbus::Result<Vec<(u32, String, OwnedObjectPath)>>;

    #[zbus(signal)]
    fn window_registered(
        &self,
        window_id: u32,
        service: String,
        menu_object_path: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn window_unregistered(&self, window_id: u32) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(p).unwrap()
    }

    #[test]
    fn same_window_id_from_different_clients_does_not_collide() {
        let mut r = Registry::default();
        r.register(1, ":1.10", path("/MenuBar/1"));
        r.register(1, ":1.20", path("/MenuBar/1"));
        assert_eq!(r.all().len(), 2);
        assert_eq!(r.for_window(1).unwrap().service, ":1.20", "newest wins");

        r.register(1, ":1.10", path("/MenuBar/2"));
        assert_eq!(r.all().len(), 2, "re-registering replaces");
        assert_eq!(r.for_window(1).unwrap().path.as_str(), "/MenuBar/2");
    }

    #[test]
    fn unregister_is_scoped_to_the_sender() {
        let mut r = Registry::default();
        r.register(7, ":1.10", path("/m"));
        assert!(!r.unregister(7, ":1.99"), "other clients cannot remove it");
        assert!(r.unregister(7, ":1.10"));
        assert!(r.all().is_empty());
    }

    #[test]
    fn vanished_clients_are_forgotten() {
        let mut r = Registry::default();
        r.register(1, ":1.10", path("/a"));
        r.register(2, ":1.10", path("/b"));
        r.register(3, ":1.11", path("/c"));
        let mut gone = r.remove_service(":1.10");
        gone.sort_unstable();
        assert_eq!(gone, [1, 2]);
        assert_eq!(r.all().len(), 1);
    }

    #[test]
    fn table_is_bounded() {
        let mut r = Registry::default();
        for id in 0..(MAX_REGISTRATIONS as u32 + 10) {
            r.register(id, ":1.10", path("/m"));
        }
        assert_eq!(r.all().len(), MAX_REGISTRATIONS);
        assert!(
            r.for_window(0).is_none(),
            "oldest entries are evicted first"
        );
    }
}
