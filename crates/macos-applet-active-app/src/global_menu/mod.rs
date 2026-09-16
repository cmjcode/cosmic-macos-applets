// SPDX-License-Identifier: GPL-3.0-only
//! Experimental global application menu.
//!
//! * [`registrar`] hosts `com.canonical.AppMenu.Registrar`, where apps
//!   announce the D-Bus object exporting their menu bar.
//! * [`matcher`] decides which registration belongs to the focused window.
//!   X11 apps are matched by window id (`_NET_ACTIVE_WINDOW`), Wayland apps by
//!   process identity, because their window ids are meaningless on Wayland.
//! * [`dbusmenu`] / [`model`] read `com.canonical.dbusmenu` layouts.
//! * [`service`] ties it together off the UI thread.

pub mod dbusmenu;
pub mod matcher;
pub mod model;
pub mod procinfo;
pub mod registrar;
pub mod service;
pub mod x11;
