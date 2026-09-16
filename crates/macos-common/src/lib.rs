// SPDX-License-Identifier: GPL-3.0-only
//! Shared building blocks for the COSMIC macOS top bar applets.

pub mod config;
pub mod desktop_index;
pub mod session;

/// App id (and `.desktop` file stem) of the system menu applet.
pub const MENU_APP_ID: &str = "io.github.jayuda.CosmicMacosMenu";
/// App id (and `.desktop` file stem) of the focused-application applet.
pub const ACTIVE_APP_APP_ID: &str = "io.github.jayuda.CosmicMacosActiveApp";
/// App id (and `.desktop` file stem) of the Control Center applet.
pub const CONTROL_CENTER_APP_ID: &str = "io.github.jayuda.CosmicMacosControlCenter";

/// Initialise logging once per process.
///
/// Defaults to `warn` so applets stay quiet in the session journal; override
/// with `COSMIC_MACOS_LOG=debug` (any `tracing` filter expression works).
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = std::env::var("COSMIC_MACOS_LOG")
        .ok()
        .and_then(|spec| EnvFilter::try_new(spec).ok())
        .unwrap_or_else(|| EnvFilter::new("warn"));

    // `try_init` so a second call (e.g. from tests) is harmless.
    let _ = fmt().with_env_filter(filter).try_init();
    let _ = tracing_log::LogTracer::init();
}

/// Return `true` if an executable called `program` exists in `$PATH`.
#[must_use]
pub fn program_in_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(program))
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_sh_but_not_nonsense() {
        assert!(super::program_in_path("sh"));
        assert!(!super::program_in_path(
            "definitely-not-a-real-program-4f2a"
        ));
    }
}
