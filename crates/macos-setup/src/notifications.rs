// SPDX-License-Identifier: GPL-3.0-only
//! Where notification popups appear.
//!
//! COSMIC's notification daemon puts popups next to its notifications applet
//! and, when no panel or dock holds that applet, at the top center of the
//! screen. The macOS profile drops the applet, so popups end up centered and
//! nothing in COSMIC Settings moves them. The daemon's config has an `anchor`
//! key that the daemon never reads; the patch in
//! `patches/cosmic-notifications-anchor-fallback.patch` makes it use that key
//! whenever the applet is absent. This module writes the key and reports
//! whether the daemon of the current session honors it.

use cosmic_config::{Config, ConfigGet};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::panel_profile::{Change, Options, diff};

pub const COMPONENT: &str = "com.system76.CosmicNotifications";
pub const KEY: &str = "anchor";
/// COSMIC's own daemon, as packaged.
pub const STOCK_DAEMON: &str = "/usr/bin/cosmic-notifications";
/// Where `just install-notifications` puts the patched daemon. `/usr/local/bin`
/// precedes `/usr/bin` in the session's PATH, so cosmic-session starts this one
/// and package updates never overwrite it.
pub const LOCAL_DAEMON: &str = "/usr/local/bin/cosmic-notifications";
/// Text the patched daemon logs. Finding it in a binary identifies the patch.
pub const PATCH_MARKER: &str = "cosmic-macos-applet: anchor from config";
/// Commands that build and install the patched daemon, then start it.
pub const INSTALL_COMMANDS: &str = "just install-notifications\njust restart-notifications";

/// A screen edge or corner. The variant names are the RON values of `Anchor`
/// in COSMIC's `cosmic-notifications-config` crate, so what this writes is
/// exactly what the daemon reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Position {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Position {
    pub const ALL: [Self; 8] = [
        Self::Top,
        Self::Bottom,
        Self::Left,
        Self::Right,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    /// The positions offered in the settings window: the corners and the top
    /// and bottom center. `Left` and `Right` stack popups from the middle of
    /// the edge, which looks odd, so they are only reachable from the CLI.
    pub const CHOICES: [Self; 6] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    /// Below the clock, as on macOS.
    pub const DEFAULT: Self = Self::TopRight;

    /// Stable name for the command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Every name, comma separated, for error messages.
    #[must_use]
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The position stored in `config`, if it holds a valid one.
#[must_use]
pub fn current(config: &Config) -> Option<Position> {
    config.get::<Position>(KEY).ok()
}

/// The position stored in the user's config, if any.
#[must_use]
pub fn stored() -> Option<Position> {
    Config::new(COMPONENT, 1).ok().and_then(|c| current(&c))
}

/// The config change for `options`. A requested position is written. Without
/// a request, a position that was never set becomes the macOS default and a
/// stored one is kept, so re-applying the profile never moves the popups.
#[must_use]
pub fn changes(config: &Config, options: &Options) -> Vec<Change> {
    let desired = options
        .notification_position
        .or_else(|| current(config).is_none().then_some(Position::DEFAULT));
    desired
        .and_then(|position| diff(config, COMPONENT, KEY, position))
        .into_iter()
        .collect()
}

/// The notification daemon of this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Daemon {
    NotRunning,
    /// COSMIC's daemon: popups sit next to the applet or at the top center.
    Stock(PathBuf),
    /// A build with the position patch.
    Patched(PathBuf),
    /// Some other build without the patch.
    Other(PathBuf),
}

/// `true` if `binary` is a build with the position patch.
pub fn has_patch(binary: &Path) -> io::Result<bool> {
    let bytes = fs::read(binary)?;
    let marker = PATCH_MARKER.as_bytes();
    Ok(bytes.windows(marker.len()).any(|window| window == marker))
}

fn strip_deleted(exe: PathBuf) -> PathBuf {
    exe.to_str()
        .and_then(|s| s.strip_suffix(" (deleted)"))
        .map_or(exe.clone(), PathBuf::from)
}

/// Find this user's `cosmic-notifications` process by scanning `/proc`.
///
/// `comm` holds only the first 15 characters of the name, so the executable
/// link decides. The binary is read through that link, which still works
/// after the file on disk has been replaced.
#[must_use]
pub fn daemon() -> Daemon {
    let Ok(uid) = fs::metadata("/proc/self").map(|m| m.uid()) else {
        return Daemon::NotRunning;
    };
    let Ok(entries) = fs::read_dir("/proc") else {
        return Daemon::NotRunning;
    };
    let name = "cosmic-notifications";
    for entry in entries.flatten() {
        let dir = entry.path();
        let is_pid = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid || fs::metadata(&dir).map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let Ok(comm) = fs::read_to_string(dir.join("comm")) else {
            continue;
        };
        if !name.starts_with(comm.trim_end()) {
            continue;
        }
        let link = dir.join("exe");
        let Ok(exe) = fs::read_link(&link).map(strip_deleted) else {
            continue;
        };
        if exe.file_name().and_then(|n| n.to_str()) != Some(name) {
            continue;
        }
        return match has_patch(&link) {
            Ok(true) => Daemon::Patched(exe),
            _ if exe == Path::new(STOCK_DAEMON) => Daemon::Stock(exe),
            _ => Daemon::Other(exe),
        };
    }
    Daemon::NotRunning
}

/// What the position setting does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// The running daemon reads the setting; changes apply at once.
    Active,
    /// The patched daemon is installed, but this session still runs another.
    InstalledNotRunning,
    /// Only COSMIC's daemon is available; the setting has no effect yet.
    Missing,
}

/// Combine the running daemon and the installed file into one verdict.
/// Reads two binaries, so call it off the UI thread.
#[must_use]
pub fn support() -> Support {
    if matches!(daemon(), Daemon::Patched(_)) {
        Support::Active
    } else if has_patch(Path::new(LOCAL_DAEMON)).unwrap_or(false) {
        Support::InstalledNotRunning
    } else {
        Support::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_choices_are_positions() {
        for position in Position::ALL {
            assert_eq!(Position::from_name(position.name()), Some(position));
        }
        assert_eq!(Position::from_name("middle"), None);
        assert!(Position::names().contains("top-right"));
        for choice in Position::CHOICES {
            assert!(Position::ALL.contains(&choice));
        }
        assert!(Position::CHOICES.contains(&Position::DEFAULT));
    }

    #[test]
    fn writes_upstream_ron_and_keeps_a_stored_choice() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config::with_custom_path(COMPONENT, 1, tmp.path().to_owned()).unwrap();

        // Never set: the macOS default is written.
        let plan = changes(&config, &Options::default());
        assert_eq!(plan.len(), 1);
        plan[0].apply().unwrap();
        assert_eq!(current(&config), Some(Position::TopRight));
        let raw = fs::read_to_string(
            tmp.path()
                .join("cosmic")
                .join(COMPONENT)
                .join("v1")
                .join(KEY),
        )
        .unwrap();
        assert_eq!(
            raw.trim(),
            "TopRight",
            "must match cosmic-notifications-config's Anchor"
        );

        // Set: kept unless requested.
        assert!(changes(&config, &Options::default()).is_empty());
        let request = Options {
            notification_position: Some(Position::BottomLeft),
            ..Options::default()
        };
        let plan = changes(&config, &request);
        assert_eq!(plan.len(), 1);
        plan[0].apply().unwrap();
        assert_eq!(current(&config), Some(Position::BottomLeft));
        assert!(changes(&config, &request).is_empty());
    }

    #[test]
    fn patch_is_detected_by_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plain");
        let patched = tmp.path().join("patched");
        fs::write(&plain, b"\x7fELF nothing to see").unwrap();
        fs::write(
            &patched,
            [b"\x7fELF ", PATCH_MARKER.as_bytes(), b": {anchor:?}"].concat(),
        )
        .unwrap();
        assert!(!has_patch(&plain).unwrap());
        assert!(has_patch(&patched).unwrap());
        assert!(has_patch(&tmp.path().join("missing")).is_err());
        assert_eq!(
            strip_deleted(PathBuf::from("/usr/bin/x (deleted)")),
            PathBuf::from("/usr/bin/x")
        );
    }

    #[test]
    fn position_default_is_top_right() {
        assert_eq!(Position::DEFAULT, Position::TopRight);
        assert_eq!(Position::DEFAULT.name(), "top-right");
        assert!(Position::CHOICES.contains(&Position::TopRight));
    }

    #[test]
    fn position_all_variants_ron_serialization() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config::with_custom_path(COMPONENT, 1, tmp.path().to_owned()).unwrap();

        let expected_ron_map = [
            (Position::Top, "Top"),
            (Position::Bottom, "Bottom"),
            (Position::Left, "Left"),
            (Position::Right, "Right"),
            (Position::TopLeft, "TopLeft"),
            (Position::TopRight, "TopRight"),
            (Position::BottomLeft, "BottomLeft"),
            (Position::BottomRight, "BottomRight"),
        ];

        for (position, expected_ron) in expected_ron_map {
            let req = Options {
                notification_position: Some(position),
                ..Options::default()
            };
            let plan = changes(&config, &req);
            if let Some(change) = plan.first() {
                change.apply().unwrap();
            }
            assert_eq!(current(&config), Some(position));

            let raw = fs::read_to_string(
                tmp.path()
                    .join("cosmic")
                    .join(COMPONENT)
                    .join("v1")
                    .join(KEY),
            )
            .unwrap();

            assert_eq!(
                raw.trim(),
                expected_ron,
                "Position {:?} RON must match cosmic-notifications-config Anchor RON variant",
                position
            );
        }
    }
}

