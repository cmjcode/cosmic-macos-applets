// SPDX-License-Identifier: GPL-3.0-only
//! Shows the focused application's name in the COSMIC panel, macOS style.

mod global_menu;
mod localize;
mod model;
mod wayland;
mod window;

/// Run the applet. Blocks until the panel closes it.
pub fn run() -> cosmic::iced::Result {
    localize::localize();
    cosmic::applet::run::<window::ActiveAppApplet>(())
}
