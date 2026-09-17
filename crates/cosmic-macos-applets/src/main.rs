// SPDX-License-Identifier: GPL-3.0-only
//! Multi-call binary: the applet to run is chosen from the executable name,
//! so libcosmic is linked (and loaded into memory) only once on disk.

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn applet_name(argv0: &str) -> &str {
    argv0.rsplit('/').next().unwrap_or(argv0)
}

fn main() -> cosmic::iced::Result {
    macos_common::init_tracing();

    let argv0 = std::env::args().next().unwrap_or_default();
    // Allow `cosmic-macos-applets <applet>` for development.
    let name = match applet_name(&argv0) {
        "cosmic-macos-applets" => std::env::args().nth(1).unwrap_or_default(),
        other => other.to_owned(),
    };

    tracing::info!("starting `{name}` v{VERSION}");
    match name.as_str() {
        "cosmic-macos-menu" => macos_applet_menu::run(),
        "cosmic-macos-active-app" => macos_applet_active_app::run(),
        "cosmic-macos-control-center" => macos_applet_control_center::run(),
        "cosmic-macos-settings" => macos_settings::run(),
        _ => {
            eprintln!(
                "cosmic-macos-applets v{VERSION}\nusage: cosmic-macos-applets <cosmic-macos-menu|cosmic-macos-active-app|cosmic-macos-control-center|cosmic-macos-settings>"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_directories_from_argv0() {
        assert_eq!(
            super::applet_name("/usr/bin/cosmic-macos-menu"),
            "cosmic-macos-menu"
        );
        assert_eq!(super::applet_name("cosmic-macos-menu"), "cosmic-macos-menu");
    }
}
