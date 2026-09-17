// SPDX-License-Identifier: GPL-3.0-only
//! The macOS-style panel profile and a diff/apply engine on top of cosmic-config.

use anyhow::{Context, Result};
use cosmic_config::{Config, ConfigGet, ConfigSet};
use cosmic_panel_config::{AutoHide, CosmicPanelBackground, PanelAnchor, PanelSize};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;

pub const PANEL_COMPONENT: &str = "com.system76.CosmicPanel.Panel";
pub const PANEL_LIST_COMPONENT: &str = "com.system76.CosmicPanel";
pub const TIME_COMPONENT: &str = "com.system76.CosmicAppletTime";
pub const ACTIVE_APP_COMPONENT: &str = macos_common::ACTIVE_APP_APP_ID;

/// Left side of the bar: system menu, then the focused app's name.
pub const LEFT: &[&str] = &[macos_common::MENU_APP_ID, macos_common::ACTIVE_APP_APP_ID];

/// Right side, in macOS order: status icons, the Control Center (which
/// replaces COSMIC's audio, Bluetooth, network and notification applets),
/// then the clock at the far edge. Applets that are not installed are skipped.
pub const RIGHT: &[&str] = &[
    "com.system76.CosmicAppletStatusArea",
    "com.system76.CosmicAppletInputSources",
    "com.system76.CosmicAppletA11y",
    "com.system76.CosmicAppletTiling",
    "com.system76.CosmicAppletBattery",
    NOTIFICATIONS,
    macos_common::CONTROL_CENTER_APP_ID,
    "com.system76.CosmicAppletTime",
];

/// COSMIC's notification applet. Dropped unless `keep_notifications` is set,
/// since the Control Center covers Do Not Disturb.
pub const NOTIFICATIONS: &str = "com.system76.CosmicAppletNotifications";

/// Applets this project provides; `apply` requires them to be installed.
pub const OWN_APPLETS: &[&str] = &[
    macos_common::MENU_APP_ID,
    macos_common::ACTIVE_APP_APP_ID,
    macos_common::CONTROL_CENTER_APP_ID,
];

/// User-tunable knobs of the profile.
#[derive(Debug, Clone)]
pub struct Options {
    /// Panel opacity, 0.0 – 1.0. macOS menu bars are translucent.
    pub opacity: f32,
    /// Show the weekday in the clock ("Wed 16 Sep 10:30").
    pub clock_weekday: bool,
    /// Keep COSMIC's notification applet (notification history) in the bar.
    pub keep_notifications: bool,
    /// Turn the experimental global menu on or off; `None` leaves it as is,
    /// so re-running `apply` never silently disables it.
    pub global_menu: Option<bool>,
    /// Window controls on the left for GTK/Chromium apps; `None` leaves it as is.
    pub window_controls_left: Option<bool>,
    /// Three-finger drag through linux-3-finger-drag; `None` leaves it as is.
    pub three_finger_drag: Option<bool>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            opacity: 0.8,
            clock_weekday: true,
            keep_notifications: false,
            global_menu: None,
            window_controls_left: None,
            three_finger_drag: None,
        }
    }
}

type Writer = Box<dyn Fn() -> Result<(), cosmic_config::Error>>;

/// One key that differs from the profile.
pub struct Change {
    pub component: &'static str,
    pub key: &'static str,
    pub current: String,
    pub desired: String,
    write: Writer,
}

impl Change {
    /// Write the desired value into the config this change was computed from.
    pub fn apply(&self) -> Result<()> {
        (self.write)().with_context(|| format!("write {}/{}", self.component, self.key))
    }
}

fn diff<T>(
    config: &Config,
    component: &'static str,
    key: &'static str,
    desired: T,
) -> Option<Change>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug + Clone + 'static,
{
    let current = config.get::<T>(key).ok();
    if current.as_ref() == Some(&desired) {
        return None;
    }
    Some(Change {
        component,
        key,
        current: current.map_or_else(|| "(default)".to_owned(), |v| format!("{v:?}")),
        desired: format!("{desired:?}"),
        write: {
            let config = config.clone();
            Box::new(move || config.set(key, desired.clone()))
        },
    })
}

/// Right-side applets for `options`, keeping only those whose desktop file
/// can be found, in order.
#[must_use]
pub fn right_applets(options: &Options, is_installed: impl Fn(&str) -> bool) -> Vec<String> {
    RIGHT
        .iter()
        .copied()
        .filter(|id| options.keep_notifications || *id != NOTIFICATIONS)
        .filter(|id| is_installed(id))
        .map(str::to_owned)
        .collect()
}

/// `true` if `<id>.desktop` exists in any XDG applications directory,
/// which is exactly how cosmic-panel discovers applets.
#[must_use]
pub fn applet_installed(id: &str) -> bool {
    use freedesktop_desktop_entry as fde;
    fde::Iter::new(fde::default_paths()).any(|path| path.file_stem().is_some_and(|stem| stem == id))
}

/// Compute every change needed for the panel config.
pub fn panel_changes(panel: &Config, options: &Options, right: Vec<String>) -> Vec<Change> {
    let c = PANEL_COMPONENT;
    let left: Vec<String> = LEFT.iter().map(|s| (*s).to_owned()).collect();
    [
        diff(panel, c, "anchor", PanelAnchor::Top),
        diff(panel, c, "anchor_gap", false),
        diff(panel, c, "expand_to_edges", true),
        diff(panel, c, "border_radius", 0_u32),
        diff(panel, c, "margin", 0_u16),
        diff(panel, c, "padding", 0_u32),
        diff(panel, c, "spacing", 2_u32),
        diff(panel, c, "size", PanelSize::XS),
        diff(
            panel,
            c,
            "size_wings",
            None::<(Option<PanelSize>, Option<PanelSize>)>,
        ),
        diff(panel, c, "size_center", None::<PanelSize>),
        diff(panel, c, "background", CosmicPanelBackground::ThemeDefault),
        diff(panel, c, "opacity", options.opacity.clamp(0.0, 1.0)),
        diff(panel, c, "exclusive_zone", true),
        diff(panel, c, "autohide", AutoHide::Never),
        diff(panel, c, "keep_style_on_maximize", true),
        diff(panel, c, "plugins_wings", Some((left, right))),
        diff(panel, c, "plugins_center", None::<Vec<String>>),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Compute the change for the focused-app applet config.
pub fn active_app_changes(active_app: &Config, options: &Options) -> Vec<Change> {
    options
        .global_menu
        .and_then(|enabled| diff(active_app, ACTIVE_APP_COMPONENT, "global_menu", enabled))
        .into_iter()
        .collect()
}

/// Compute every change needed for the clock applet config.
pub fn time_changes(time: &Config, options: &Options) -> Vec<Change> {
    let c = TIME_COMPONENT;
    [
        diff(time, c, "show_date_in_top_panel", true),
        diff(time, c, "show_weekday", options.clock_weekday),
        diff(time, c, "show_seconds", false),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_applets_are_skipped_in_order() {
        let options = Options::default();
        let got = right_applets(&options, |id| {
            !id.ends_with("A11y") && !id.ends_with("Tiling")
        });
        assert_eq!(
            got.first().map(String::as_str),
            Some("com.system76.CosmicAppletStatusArea")
        );
        assert_eq!(
            got.last().map(String::as_str),
            Some("com.system76.CosmicAppletTime")
        );
        assert!(!got.iter().any(|id| id == NOTIFICATIONS));
        assert_eq!(
            got[got.len() - 2],
            macos_common::CONTROL_CENTER_APP_ID,
            "Control Center sits next to the clock"
        );
    }

    #[test]
    fn notifications_can_be_kept() {
        let options = Options {
            keep_notifications: true,
            ..Options::default()
        };
        assert!(
            right_applets(&options, |_| true)
                .iter()
                .any(|id| id == NOTIFICATIONS)
        );
    }

    #[test]
    fn clock_is_rightmost_and_menu_leftmost() {
        assert_eq!(*RIGHT.last().unwrap(), "com.system76.CosmicAppletTime");
        assert_eq!(LEFT[0], macos_common::MENU_APP_ID);
    }

    #[test]
    fn diff_detects_changes_and_applies_them() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests in this module run in one process; nothing else reads it concurrently.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path()) };
        let config = Config::new("io.github.jayuda.CosmicMacosTest", 1).unwrap();

        let options = Options::default();
        let changes = panel_changes(
            &config,
            &options,
            vec!["com.system76.CosmicAppletTime".into()],
        );
        assert!(changes.iter().any(|c| c.key == "plugins_wings"));
        for change in &changes {
            change.apply().unwrap();
        }
        let again = panel_changes(
            &config,
            &options,
            vec!["com.system76.CosmicAppletTime".into()],
        );
        let keys: Vec<_> = again.iter().map(|c| c.key).collect();
        assert!(
            again.is_empty(),
            "profile must be idempotent, still differs: {keys:?}"
        );

        // Each change writes to the config it was computed from.
        let applet = Config::new("io.github.jayuda.CosmicMacosTestApplet", 1).unwrap();
        assert!(
            active_app_changes(&applet, &Options::default()).is_empty(),
            "without a flag the setting is left alone"
        );
        let enable = Options {
            global_menu: Some(true),
            ..Options::default()
        };
        let changes = active_app_changes(&applet, &enable);
        assert_eq!(changes.len(), 1);
        changes[0].apply().unwrap();
        assert!(applet.get::<bool>("global_menu").unwrap());
        assert!(
            config.get::<bool>("global_menu").is_err(),
            "must not leak into another component"
        );
        assert!(active_app_changes(&applet, &enable).is_empty());
        assert!(
            active_app_changes(&applet, &Options::default()).is_empty(),
            "re-running apply without the flag keeps it enabled"
        );
    }
}
