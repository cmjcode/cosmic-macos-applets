// SPDX-License-Identifier: GPL-3.0-only
//! Settings window for the COSMIC macOS top bar.
//!
//! Pages follow COSMIC Settings' layout and write the same config the applets
//! hot-reload, so changes show up in the panel immediately. Profile changes
//! and session services go through `macos-setup`, which backs up first.

mod app;
mod localize;
mod pages;

/// Open the settings window. Blocks until it is closed.
pub fn run() -> cosmic::iced::Result {
    localize::localize();
    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(900.0, 720.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(360.0)
                .min_height(300.0),
        );
    cosmic::app::run::<app::App>(settings, ())
}
