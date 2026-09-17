// SPDX-License-Identifier: GPL-3.0-only
//! Thin wrapper around `systemctl --user` for the session services `apply` manages.

use anyhow::{Context, Result, bail};
use std::{path::PathBuf, process::Command};

fn systemctl(args: &[&str]) -> Result<std::process::Output> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("run systemctl --user")
}

fn run(args: &[&str]) -> Result<()> {
    let output = systemctl(args)?;
    if !output.status.success() {
        bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// `$XDG_CONFIG_HOME/systemd/user`, defaulting to `~/.config/systemd/user`.
pub fn unit_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config"),
    };
    Ok(base.join("systemd/user"))
}

/// `true` if systemd knows the unit (installed anywhere in the user search path).
#[must_use]
pub fn exists(unit: &str) -> bool {
    systemctl(&["list-unit-files", "--no-legend", unit])
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(unit))
}

#[must_use]
pub fn is_enabled(unit: &str) -> bool {
    systemctl(&["is-enabled", unit])
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
}

pub fn daemon_reload() -> Result<()> {
    run(&["daemon-reload"])
}

pub fn enable_now(unit: &str) -> Result<()> {
    run(&["enable", "--now", unit])
}

pub fn disable_now(unit: &str) -> Result<()> {
    run(&["disable", "--now", unit])
}
