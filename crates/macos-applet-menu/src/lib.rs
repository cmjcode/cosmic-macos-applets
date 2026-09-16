// SPDX-License-Identifier: GPL-3.0-only
//! macOS-style system menu applet for the COSMIC panel.

mod localize;
mod window;

/// Run the applet. Blocks until the panel closes it.
pub fn run() -> cosmic::iced::Result {
    localize::localize();
    cosmic::applet::run::<window::MenuApplet>(())
}
