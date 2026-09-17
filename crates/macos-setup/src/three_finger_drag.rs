// SPDX-License-Identifier: GPL-3.0-only
//! macOS-style three-finger drag through linux-3-finger-drag.
//!
//! libinput can do three-finger drag natively (since 1.28), but only when the
//! compositor turns it on, and cosmic-comp has no setting for it yet. The
//! separate project <https://github.com/lmr97/linux-3-finger-drag> proxies
//! the touchpad at the evdev level instead, which works under any compositor.
//! Installing it needs root (a udev rule for /dev/uinput and the `input`
//! group), so this tool only checks the prerequisites and switches its user
//! service on or off.

use anyhow::{Result, bail};
use std::fs::{self, OpenOptions};

use crate::user_service;

pub const UNIT: &str = "three-finger-drag.service";
pub const PROJECT_URL: &str = "https://github.com/lmr97/linux-3-finger-drag";

/// What stops the drag service from working, in the order to fix it.
#[must_use]
pub fn missing_prerequisites(
    unit_installed: bool,
    input_readable: bool,
    uinput_writable: bool,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !unit_installed {
        missing.push("linux-3-finger-drag is not installed (no three-finger-drag.service)");
    }
    if !input_readable {
        missing.push(
            "this session cannot read touchpads in /dev/input \
             (join the `input` group, then log out and back in)",
        );
    }
    if !uinput_writable {
        missing.push(
            "this session cannot write /dev/uinput \
             (udev rule 60-uinput.rules and the uinput module)",
        );
    }
    missing
}

/// Opening an evdev node read-only does not grab it, so this is side-effect free.
fn input_readable() -> bool {
    fs::read_dir("/dev/input").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|e| {
            e.file_name().to_string_lossy().starts_with("event")
                && OpenOptions::new().read(true).open(e.path()).is_ok()
        })
    })
}

/// Opening /dev/uinput creates no device until it is configured with ioctls.
fn uinput_writable() -> bool {
    OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

#[must_use]
pub fn enabled() -> bool {
    user_service::is_enabled(UNIT)
}

/// What is still missing on this machine. Runs `systemctl`; call off the UI thread.
#[must_use]
pub fn missing_now() -> Vec<&'static str> {
    missing_prerequisites(
        user_service::exists(UNIT),
        input_readable(),
        uinput_writable(),
    )
}

/// Shell commands that install linux-3-finger-drag.
pub const INSTALL_COMMANDS: &str = "git clone https://github.com/lmr97/linux-3-finger-drag\n\
                                    cd linux-3-finger-drag && sudo ./install.sh\n\
                                    reboot";

/// Fail early, before `apply` writes anything, when enabling cannot work.
pub fn check_can_enable() -> Result<()> {
    let missing = missing_now();
    if missing.is_empty() {
        return Ok(());
    }
    let list: String = missing.iter().map(|m| format!("\n  - {m}")).collect();
    bail!(
        "three-finger drag is not ready:{list}\n\n\
         Install linux-3-finger-drag once (needs sudo):\n\n    \
         git clone {PROJECT_URL}\n    \
         cd linux-3-finger-drag && sudo ./install.sh\n\n\
         then reboot and run `cosmic-macos-setup apply --three-finger-drag` again."
    )
}

pub fn set_enabled(enable: bool) -> Result<()> {
    if enable {
        user_service::enable_now(UNIT)
    } else if user_service::exists(UNIT) {
        user_service::disable_now(UNIT)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_every_missing_prerequisite_in_order() {
        assert!(missing_prerequisites(true, true, true).is_empty());
        let all = missing_prerequisites(false, false, false);
        assert_eq!(all.len(), 3);
        assert!(all[0].contains("not installed"));
        assert!(all[1].contains("input"));
        assert!(all[2].contains("uinput"));
        assert_eq!(missing_prerequisites(true, false, true).len(), 1);
    }
}
