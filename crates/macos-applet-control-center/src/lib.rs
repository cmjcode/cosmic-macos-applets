// SPDX-License-Identifier: GPL-3.0-only
//! macOS-style Control Center applet for the COSMIC panel.

mod brightness;
mod localize;
pub mod services;
mod style;
mod window;

/// Run the applet. Blocks until the panel closes it.
pub fn run() -> cosmic::iced::Result {
    localize::localize();
    cosmic::applet::run::<window::ControlCenter>(())
}

/// Shorten `s` to at most `max` characters with an ellipsis.
pub(crate) fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}
