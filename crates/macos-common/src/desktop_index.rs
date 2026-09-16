// SPDX-License-Identifier: GPL-3.0-only
//! Maps Wayland `app_id`s to human readable application names.
//!
//! Scanning every `.desktop` file is comparatively expensive, so results are
//! memoised and a rescan is only requested when a lookup misses and the last
//! scan is older than [`RESCAN_INTERVAL`]. That covers apps installed while
//! the session is running without polling the filesystem. The scan itself is
//! a separate, blocking function so applets can run it off the UI thread.

pub use freedesktop_desktop_entry::DesktopEntry;
use freedesktop_desktop_entry::{self as fde, unicase::Ascii};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Minimum time between two full rescans triggered by cache misses.
pub const RESCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Display metadata resolved for an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMeta {
    pub name: String,
    pub icon: Option<String>,
    /// Path of the matching desktop file, if one was found.
    pub desktop_file: Option<std::path::PathBuf>,
}

/// Result of resolving an app id against the in-memory index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Hit(AppMeta),
    /// Unknown app id. `rescan` is `true` when the caller should refresh the
    /// entries (off the UI thread) with [`DesktopIndex::scan`] and
    /// [`DesktopIndex::install`], then resolve again.
    Miss {
        rescan: bool,
    },
}

#[derive(Debug)]
pub struct DesktopIndex {
    locales: Vec<String>,
    entries: Vec<DesktopEntry>,
    cache: HashMap<String, Option<AppMeta>>,
    last_scan: Option<Instant>,
    scan_pending: bool,
}

impl Default for DesktopIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopIndex {
    /// Create an empty index using the locales from the environment.
    #[must_use]
    pub fn new() -> Self {
        Self::with_entries(fde::get_languages_from_env(), Vec::new())
    }

    /// Create an index from pre-parsed entries.
    #[must_use]
    pub fn with_entries(locales: Vec<String>, entries: Vec<DesktopEntry>) -> Self {
        Self {
            locales,
            entries,
            cache: HashMap::new(),
            last_scan: None,
            scan_pending: false,
        }
    }

    #[must_use]
    pub fn locales(&self) -> &[String] {
        &self.locales
    }

    /// Read all application desktop entries from the XDG data directories.
    ///
    /// Blocking filesystem work: call it from a background thread.
    #[must_use]
    pub fn scan(locales: &[String]) -> Vec<DesktopEntry> {
        let started = Instant::now();
        let entries: Vec<DesktopEntry> = fde::Iter::new(fde::default_paths())
            .filter_map(|path| DesktopEntry::from_path(path, Some(locales)).ok())
            .filter(|entry| {
                entry
                    .desktop_entry("Type")
                    .is_none_or(|t| t == "Application")
            })
            .collect();
        tracing::debug!(
            entries = entries.len(),
            elapsed = ?started.elapsed(),
            "desktop entries scanned"
        );
        entries
    }

    /// Mark that a scan is about to start. Returns `false` if one is already
    /// running, so callers never start two scans concurrently.
    pub fn begin_scan(&mut self) -> bool {
        !std::mem::replace(&mut self.scan_pending, true)
    }

    /// Replace the entries with the result of [`DesktopIndex::scan`].
    pub fn install(&mut self, entries: Vec<DesktopEntry>) {
        self.entries = entries;
        self.cache.clear();
        self.last_scan = Some(Instant::now());
        self.scan_pending = false;
    }

    fn scan_is_stale(&self) -> bool {
        self.last_scan
            .is_none_or(|at| at.elapsed() >= RESCAN_INTERVAL)
    }

    /// Resolve an `app_id` using memoised results where possible.
    pub fn resolve(&mut self, app_id: &str) -> Resolution {
        if app_id.is_empty() {
            return Resolution::Miss { rescan: false };
        }
        let meta = match self.cache.get(app_id) {
            Some(cached) => cached.clone(),
            None => {
                let meta = self.lookup_cached_entries(app_id);
                self.cache.insert(app_id.to_owned(), meta.clone());
                meta
            }
        };
        match meta {
            Some(meta) => Resolution::Hit(meta),
            None => Resolution::Miss {
                rescan: !self.scan_pending && self.scan_is_stale(),
            },
        }
    }

    /// Resolve an `app_id` against the entries already in memory.
    #[must_use]
    pub fn lookup_cached_entries(&self, app_id: &str) -> Option<AppMeta> {
        let entry = fde::find_app_by_id(&self.entries, Ascii::new(app_id))?;
        let name = entry
            .name(&self.locales)
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())?;
        Some(AppMeta {
            name,
            icon: entry.icon().map(str::to_owned),
            desktop_file: Some(entry.path.clone()).filter(|p| !p.as_os_str().is_empty()),
        })
    }
}

/// Turn a raw app id into something presentable when no desktop entry exists,
/// e.g. `org.gnome.Nautilus` becomes `Nautilus` and `my-tool` becomes `My Tool`.
#[must_use]
pub fn humanize_app_id(app_id: &str) -> String {
    let last = app_id
        .trim()
        .trim_end_matches(".desktop")
        .rsplit('.')
        .next()
        .unwrap_or_default();
    last.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(file: &str, body: &str) -> DesktopEntry {
        DesktopEntry::from_str(
            format!("/usr/share/applications/{file}"),
            body,
            None::<&[&str]>,
        )
        .expect("valid desktop entry")
    }

    fn index() -> DesktopIndex {
        let firefox = entry(
            "firefox.desktop",
            "[Desktop Entry]\nType=Application\nName=Firefox\nName[id]=Peramban Firefox\nIcon=firefox\nExec=firefox %u\n",
        );
        let files = entry(
            "com.system76.CosmicFiles.desktop",
            "[Desktop Entry]\nType=Application\nName=COSMIC Files\nIcon=com.system76.CosmicFiles\nExec=cosmic-files\n",
        );
        let code = entry(
            "code.desktop",
            "[Desktop Entry]\nType=Application\nName=Visual Studio Code\nStartupWMClass=Code\nExec=/usr/bin/code\n",
        );
        let mut idx = DesktopIndex::with_entries(vec!["en".into()], vec![firefox, files, code]);
        // Pretend we just scanned so tests never touch the real filesystem.
        idx.last_scan = Some(Instant::now());
        idx
    }

    #[test]
    fn resolves_by_desktop_file_id() {
        let idx = index();
        let meta = idx
            .lookup_cached_entries("com.system76.CosmicFiles")
            .unwrap();
        assert_eq!(meta.name, "COSMIC Files");
        assert_eq!(meta.icon.as_deref(), Some("com.system76.CosmicFiles"));
    }

    #[test]
    fn resolves_by_wm_class_case_insensitively() {
        let idx = index();
        assert_eq!(
            idx.lookup_cached_entries("code").unwrap().name,
            "Visual Studio Code"
        );
    }

    #[test]
    fn prefers_requested_locale() {
        let mut idx = index();
        idx.locales = vec!["id".into()];
        assert_eq!(
            idx.lookup_cached_entries("firefox").unwrap().name,
            "Peramban Firefox"
        );
    }

    #[test]
    fn unknown_app_is_memoised_and_rescan_is_rate_limited() {
        let mut idx = index();
        // Fresh scan: a miss must not trigger another scan.
        assert_eq!(
            idx.resolve("does.not.Exist"),
            Resolution::Miss { rescan: false }
        );
        assert!(idx.cache.contains_key("does.not.Exist"));
        assert_eq!(idx.resolve(""), Resolution::Miss { rescan: false });

        // Stale scan: a miss asks for exactly one rescan at a time.
        idx.last_scan = Some(Instant::now() - RESCAN_INTERVAL);
        assert_eq!(
            idx.resolve("does.not.Exist"),
            Resolution::Miss { rescan: true }
        );
        assert!(idx.begin_scan());
        assert!(!idx.begin_scan());
        assert_eq!(
            idx.resolve("does.not.Exist"),
            Resolution::Miss { rescan: false }
        );

        idx.install(Vec::new());
        assert!(idx.begin_scan(), "install clears the pending flag");
    }

    #[test]
    fn hits_are_returned_from_cache() {
        let mut idx = index();
        match idx.resolve("firefox") {
            Resolution::Hit(meta) => assert_eq!(meta.name, "Firefox"),
            other => panic!("expected hit, got {other:?}"),
        }
    }

    #[test]
    fn humanizes_reverse_dns_and_dashes() {
        assert_eq!(humanize_app_id("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(humanize_app_id("my-cool_tool"), "My Cool Tool");
        assert_eq!(humanize_app_id("  "), "");
    }
}
