// SPDX-License-Identifier: GPL-3.0-only
//! Window controls (close, minimize, maximize) on the left, macOS style.
//!
//! GTK, libadwaita and Chromium-based apps read the GNOME `button-layout`
//! setting (directly or through the Settings portal). cosmic-settings-daemon
//! overwrites that key with a right-side layout whenever it starts or the
//! CosmicTk config changes, so a one-off `gsettings set` does not survive a
//! login. Instead a small user service (`cosmic-macos-setup
//! window-controls-watch`) follows the key and puts the left layout back.
//!
//! COSMIC's own server-side decorations and libcosmic apps draw their
//! controls on the right unconditionally; nothing here changes those.

use anyhow::{Context, Result, bail};
use cosmic_config::{Config, ConfigGet};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::user_service;

const SCHEMA: &str = "org.gnome.desktop.wm.preferences";
const KEY: &str = "button-layout";
const TK_COMPONENT: &str = "com.system76.CosmicTk";
pub const UNIT: &str = "cosmic-macos-window-controls.service";

/// macOS order on the left: close, minimize, zoom. Buttons the user turned
/// off in COSMIC Settings stay off.
#[must_use]
pub fn left_layout(show_minimize: bool, show_maximize: bool) -> String {
    let mut buttons = vec!["close"];
    if show_minimize {
        buttons.push("minimize");
    }
    if show_maximize {
        buttons.push("maximize");
    }
    format!("{}:", buttons.join(","))
}

/// The layout cosmic-settings-daemon writes (`set_gnome_button_layout`).
#[must_use]
pub fn right_layout(show_minimize: bool, show_maximize: bool) -> &'static str {
    match (show_maximize, show_minimize) {
        (true, true) => ":minimize,maximize,close",
        (true, false) => ":maximize,close",
        (false, true) => ":minimize,close",
        (false, false) => ":close",
    }
}

/// Strip the GVariant quoting from `gsettings get` output: `'close:'` → `close:`.
#[must_use]
pub fn parse_gsettings_string(output: &str) -> &str {
    let s = output.trim();
    s.strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(s)
}

/// systemd unit that runs the watcher for the graphical session.
#[must_use]
pub fn unit_contents(setup_binary: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=Keep window controls on the left (cosmic-macos-applet)\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={} window-controls-watch\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n",
        setup_binary.display()
    )
}

fn tk_buttons() -> (bool, bool) {
    // A missing config falls back to libcosmic's defaults (both shown).
    let Ok(tk) = Config::new(TK_COMPONENT, 1) else {
        return (true, true);
    };
    (
        tk.get("show_minimize").unwrap_or(true),
        tk.get("show_maximize").unwrap_or(true),
    )
}

fn gsettings(args: &[&str]) -> Result<String> {
    let output = Command::new("gsettings")
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .context("run gsettings (is glib2 installed?)")?;
    if !output.status.success() {
        bail!("gsettings {} failed: {}", args.join(" "), output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn current_layout() -> Result<String> {
    Ok(parse_gsettings_string(&gsettings(&["get", SCHEMA, KEY])?).to_owned())
}

fn set_layout(layout: &str) -> Result<()> {
    gsettings(&["set", SCHEMA, KEY, layout]).map(drop)
}

/// Put the left layout back if something else changed it. Returns whether it wrote.
fn enforce_left() -> Result<bool> {
    let (min, max) = tk_buttons();
    let desired = left_layout(min, max);
    if current_layout()? == desired {
        return Ok(false);
    }
    set_layout(&desired)?;
    Ok(true)
}

/// `window-controls-watch`: keep the layout on the left until killed.
pub fn watch() -> Result<()> {
    enforce_left()?;
    let mut child = Command::new("gsettings")
        .args(["monitor", SCHEMA, KEY])
        .stdout(Stdio::piped())
        .spawn()
        .context("run gsettings monitor")?;
    let stdout = child.stdout.take().context("gsettings monitor stdout")?;
    // Every change (including ours, which then compares equal) prints a line.
    // cosmic-settings-daemon writes after re-reading CosmicTk, so recomputing
    // the layout here also picks up minimize/maximize toggles.
    for line in BufReader::new(stdout).lines() {
        line.context("read gsettings monitor")?;
        match enforce_left() {
            Ok(true) => eprintln!("moved window controls back to the left"),
            Ok(false) => {}
            Err(error) => eprintln!("warning: could not restore button layout: {error:#}"),
        }
    }
    let status = child.wait()?;
    bail!("gsettings monitor exited: {status}")
}

fn unit_path() -> Result<PathBuf> {
    Ok(user_service::unit_dir()?.join(UNIT))
}

/// Whether the watcher service is installed and enabled.
#[must_use]
pub fn enabled() -> bool {
    unit_path().is_ok_and(|p| p.is_file()) && user_service::is_enabled(UNIT)
}

/// Install and start the watcher, or stop it and hand the layout back to COSMIC.
pub fn set_enabled(enable: bool) -> Result<()> {
    let path = unit_path()?;
    if enable {
        let binary = std::env::current_exe().context("locate cosmic-macos-setup")?;
        fs::create_dir_all(path.parent().context("unit dir")?)?;
        fs::write(&path, unit_contents(&binary))
            .with_context(|| format!("write {}", path.display()))?;
        user_service::daemon_reload()?;
        // Apply right away, so apps update before the service is even up.
        enforce_left()?;
        user_service::enable_now(UNIT)?;
    } else {
        if path.is_file() {
            user_service::disable_now(UNIT)?;
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            user_service::daemon_reload()?;
        }
        let (min, max) = tk_buttons();
        set_layout(right_layout(min, max))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_layout_follows_cosmic_toggles() {
        assert_eq!(left_layout(true, true), "close,minimize,maximize:");
        assert_eq!(left_layout(false, true), "close,maximize:");
        assert_eq!(left_layout(true, false), "close,minimize:");
        assert_eq!(left_layout(false, false), "close:");
    }

    #[test]
    fn right_layout_matches_settings_daemon() {
        assert_eq!(right_layout(true, true), ":minimize,maximize,close");
        assert_eq!(right_layout(false, true), ":maximize,close");
        assert_eq!(right_layout(true, false), ":minimize,close");
        assert_eq!(right_layout(false, false), ":close");
    }

    #[test]
    fn strips_gvariant_quotes() {
        assert_eq!(
            parse_gsettings_string("':maximize,close'\n"),
            ":maximize,close"
        );
        assert_eq!(parse_gsettings_string("close:"), "close:");
    }

    #[test]
    fn unit_runs_the_watcher_in_the_session() {
        let unit = unit_contents(Path::new("/home/u/.local/bin/cosmic-macos-setup"));
        assert!(
            unit.contains(
                "ExecStart=/home/u/.local/bin/cosmic-macos-setup window-controls-watch\n"
            )
        );
        assert!(unit.contains("WantedBy=graphical-session.target"));
    }
}
