// SPDX-License-Identifier: GPL-3.0-only
//! Snapshot and restore of the COSMIC config directories this tool touches.
//!
//! A backup is a plain copy of `~/.config/cosmic/<component>/v1/*`, so it can
//! also be inspected or restored by hand. Restoring removes keys that did not
//! exist at backup time, which lets COSMIC fall back to its system defaults.

use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Config components (cosmic-config ids) touched by `apply`.
pub const COMPONENTS: &[&str] = &[
    "com.system76.CosmicPanel.Panel",
    "com.system76.CosmicAppletTime",
];
const VERSION_DIR: &str = "v1";
/// Marker written last, so a half-written backup is never picked for restore.
const COMPLETE_MARKER: &str = ".complete";

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// `$XDG_CONFIG_HOME/cosmic`, defaulting to `~/.config/cosmic`.
pub fn cosmic_config_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home()?.join(".config"),
    };
    Ok(base.join("cosmic"))
}

/// `$XDG_STATE_HOME/cosmic-macos-applet/backups`, defaulting to `~/.local/state/...`.
pub fn backups_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home()?.join(".local/state"),
    };
    Ok(base.join("cosmic-macos-applet").join("backups"))
}

/// Format seconds since the Unix epoch as `YYYYMMDDTHHMMSSZ` (UTC).
#[must_use]
pub fn timestamp(unix_secs: u64) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(i64::MAX);
    let rem = unix_secs % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn copy_dir_files(from: &Path, to: &Path) -> Result<usize> {
    fs::create_dir_all(to).with_context(|| format!("create {}", to.display()))?;
    let mut copied = 0;
    if !from.is_dir() {
        return Ok(0);
    }
    for entry in fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), to.join(entry.file_name()))
                .with_context(|| format!("copy {}", entry.path().display()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Copy the current config of every component into a new timestamped backup.
pub fn create(config_dir: &Path, backups: &Path) -> Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut dir = backups.join(timestamp(now));
    // Two backups in the same second get distinct directories.
    let mut n = 1;
    while dir.exists() {
        dir = backups.join(format!("{}-{n}", timestamp(now)));
        n += 1;
    }
    for component in COMPONENTS {
        copy_dir_files(
            &config_dir.join(component).join(VERSION_DIR),
            &dir.join(component).join(VERSION_DIR),
        )?;
    }
    fs::write(dir.join(COMPLETE_MARKER), b"")?;
    Ok(dir)
}

/// All complete backups, oldest first.
pub fn list(backups: &Path) -> Result<Vec<PathBuf>> {
    if !backups.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(backups)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(COMPLETE_MARKER).is_file())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Write `contents` to `path` atomically so the panel never reads a partial file.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension("cosmic-macos-tmp");
    fs::write(&tmp, contents).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

/// Restore every component from `backup`. Returns the number of keys written or removed.
pub fn restore(config_dir: &Path, backup: &Path) -> Result<usize> {
    if !backup.join(COMPLETE_MARKER).is_file() {
        bail!("{} is not a complete backup", backup.display());
    }
    let mut changes = 0;
    for component in COMPONENTS {
        let saved = backup.join(component).join(VERSION_DIR);
        let live = config_dir.join(component).join(VERSION_DIR);
        fs::create_dir_all(&live)?;

        // Remove keys that were not present when the backup was made.
        for entry in fs::read_dir(&live)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && !saved.join(entry.file_name()).is_file() {
                fs::remove_file(entry.path())?;
                changes += 1;
            }
        }
        if saved.is_dir() {
            for entry in fs::read_dir(&saved)? {
                let entry = entry?;
                let target = live.join(entry.file_name());
                let contents = fs::read(entry.path())?;
                if fs::read(&target).ok().as_deref() != Some(contents.as_slice()) {
                    write_atomic(&target, &contents)?;
                    changes += 1;
                }
            }
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_formats_utc() {
        assert_eq!(timestamp(0), "19700101T000000Z");
        // 2026-09-16 10:30:05 UTC
        assert_eq!(timestamp(1_789_554_605), "20260916T103005Z");
        // Leap day.
        assert_eq!(timestamp(951_782_400), "20000229T000000Z");
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("cosmic");
        let backups = tmp.path().join("backups");
        let panel = config.join(COMPONENTS[0]).join(VERSION_DIR);
        fs::create_dir_all(&panel).unwrap();
        fs::write(panel.join("anchor"), "Top").unwrap();
        fs::write(panel.join("size"), "XS").unwrap();

        let backup = create(&config, &backups).unwrap();
        assert_eq!(list(&backups).unwrap(), vec![backup.clone()]);

        // Simulate `apply`.
        fs::write(panel.join("anchor"), "Bottom").unwrap();
        fs::write(panel.join("border_radius"), "0").unwrap();
        fs::remove_file(panel.join("size")).unwrap();

        let changes = restore(&config, &backup).unwrap();
        assert_eq!(changes, 3);
        assert_eq!(fs::read_to_string(panel.join("anchor")).unwrap(), "Top");
        assert_eq!(fs::read_to_string(panel.join("size")).unwrap(), "XS");
        assert!(!panel.join("border_radius").exists());
        // Restoring again is a no-op.
        assert_eq!(restore(&config, &backup).unwrap(), 0);
    }

    #[test]
    fn incomplete_backups_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("20260101T000000Z")).unwrap();
        assert!(list(tmp.path()).unwrap().is_empty());
        assert!(restore(tmp.path(), &tmp.path().join("20260101T000000Z")).is_err());
    }
}
