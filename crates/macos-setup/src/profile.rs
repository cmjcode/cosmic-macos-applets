// SPDX-License-Identifier: GPL-3.0-only
//! Plan, apply and restore the whole profile: panel config plus session services.

use anyhow::{Context, Result, bail};
use cosmic_config::{Config, ConfigGet};
use std::path::{Path, PathBuf};

use crate::{
    backup, notifications,
    panel_profile::{self, Change, Options},
    three_finger_drag, window_controls,
};

fn open(component: &str) -> Result<Config> {
    Config::new(component, 1).with_context(|| format!("open config {component}"))
}

/// A session tweak that lives in a user service rather than in cosmic-config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    WindowControlsLeft,
    ThreeFingerDrag,
}

impl Service {
    pub const ALL: [Self; 2] = [Self::WindowControlsLeft, Self::ThreeFingerDrag];

    /// Stable name used in backups.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::WindowControlsLeft => "window-controls-left",
            Self::ThreeFingerDrag => "three-finger-drag",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    fn requested(self, options: &Options) -> Option<bool> {
        match self {
            Self::WindowControlsLeft => options.window_controls_left,
            Self::ThreeFingerDrag => options.three_finger_drag,
        }
    }

    /// Current state. Runs `systemctl`, so call it off the UI thread.
    #[must_use]
    pub fn enabled(self) -> bool {
        match self {
            Self::WindowControlsLeft => window_controls::enabled(),
            Self::ThreeFingerDrag => three_finger_drag::enabled(),
        }
    }

    fn set_enabled(self, enable: bool) -> Result<()> {
        match self {
            Self::WindowControlsLeft => window_controls::set_enabled(enable),
            Self::ThreeFingerDrag => three_finger_drag::set_enabled(enable),
        }
        .with_context(|| format!("switch {} {}", self.name(), on_off(enable)))
    }

    /// Fail before anything is written when switching on cannot work.
    fn check_can_enable(self) -> Result<()> {
        match self {
            Self::WindowControlsLeft => Ok(()),
            Self::ThreeFingerDrag => three_finger_drag::check_can_enable(),
        }
    }
}

#[must_use]
pub fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

/// Services whose requested state differs from `current`, in a stable order.
pub fn service_changes(
    options: &Options,
    current: impl Fn(Service) -> bool,
) -> Vec<(Service, bool)> {
    Service::ALL
        .into_iter()
        .filter_map(|s| s.requested(options).map(|want| (s, want)))
        .filter(|&(s, want)| current(s) != want)
        .collect()
}

fn service_states() -> Vec<(&'static str, bool)> {
    Service::ALL
        .into_iter()
        .map(|s| (s.name(), s.enabled()))
        .collect()
}

/// Our applets that have no desktop entry, so the panel cannot load them.
#[must_use]
pub fn missing_applets() -> Vec<&'static str> {
    panel_profile::OWN_APPLETS
        .iter()
        .copied()
        .filter(|id| !panel_profile::applet_installed(id))
        .collect()
}

/// Every config key that differs from the profile for `options`.
pub fn collect_changes(options: &Options) -> Result<Vec<Change>> {
    let entries: Vec<String> = open(panel_profile::PANEL_LIST_COMPONENT)?
        .get("entries")
        .unwrap_or_default();
    if !entries.iter().any(|e| e == "Panel") {
        bail!(
            "no COSMIC panel named \"Panel\" is configured (found: {entries:?}).\n\
             Enable the top panel in Settings › Desktop › Panel first."
        );
    }

    let panel = open(panel_profile::PANEL_COMPONENT)?;
    let time = open(panel_profile::TIME_COMPONENT)?;
    let active_app = open(panel_profile::ACTIVE_APP_COMPONENT)?;
    let right = panel_profile::right_applets(options, panel_profile::applet_installed);
    let mut changes = panel_profile::panel_changes(&panel, options, right);
    changes.extend(panel_profile::time_changes(&time, options));
    changes.extend(panel_profile::active_app_changes(&active_app, options));
    let popups = open(notifications::COMPONENT)?;
    changes.extend(notifications::changes(&popups, options));
    Ok(changes)
}

type Wings = Option<(Vec<String>, Vec<String>)>;

fn wings() -> Option<(Vec<String>, Vec<String>)> {
    open(panel_profile::PANEL_COMPONENT)
        .ok()
        .and_then(|panel| panel.get::<Wings>("plugins_wings").ok().flatten())
}

/// `true` when the panel's left side holds this project's applets.
#[must_use]
pub fn profile_applied() -> bool {
    wings().is_some_and(|(left, _)| left == panel_profile::LEFT)
}

/// The profile options that reproduce the current setup, so re-applying
/// keeps what the user tuned. Defaults while the profile is not applied.
#[must_use]
pub fn current_options() -> Options {
    // The position is independent of the panel layout, so it is read even
    // while the profile is not applied.
    let mut options = Options {
        notification_position: notifications::stored(),
        ..Options::default()
    };
    let Some((left, right)) = wings() else {
        return options;
    };
    if left != panel_profile::LEFT {
        return options;
    }
    if let Ok(opacity) = open(panel_profile::PANEL_COMPONENT)
        .and_then(|p| p.get::<f32>("opacity").context("read opacity"))
    {
        options.opacity = opacity.clamp(0.0, 1.0);
    }
    if let Ok(time) = open(panel_profile::TIME_COMPONENT) {
        options.clock_weekday = time.get("show_weekday").unwrap_or(options.clock_weekday);
    }
    options.keep_notifications = right.iter().any(|id| id == panel_profile::NOTIFICATIONS);
    options
}

/// Everything `execute` would change.
pub struct Plan {
    pub changes: Vec<Change>,
    pub services: Vec<(Service, bool)>,
}

impl Plan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.services.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len() + self.services.len()
    }
}

fn check_services(services: &[(Service, bool)]) -> Result<()> {
    services
        .iter()
        .filter(|&&(_, want)| want)
        .try_for_each(|&(service, _)| service.check_can_enable())
}

/// Work out the changes for `options`, checking that they can be applied.
pub fn plan(options: &Options, force: bool) -> Result<Plan> {
    let missing = missing_applets();
    if !missing.is_empty() && !force {
        bail!(
            "these applets are not installed: {missing:?}\n\
             Run `just install` first, or pass --force."
        );
    }
    let changes = collect_changes(options)?;
    let services = service_changes(options, Service::enabled);
    check_services(&services)?;
    Ok(Plan { changes, services })
}

/// A plan that only switches one service, leaving the panel config alone.
pub fn plan_service(service: Service, enable: bool) -> Result<Plan> {
    let services = if service.enabled() == enable {
        Vec::new()
    } else {
        vec![(service, enable)]
    };
    check_services(&services)?;
    Ok(Plan {
        changes: Vec::new(),
        services,
    })
}

/// Back up, then apply `plan`, rolling everything back on failure.
/// Returns the backup directory, or `None` when there was nothing to do.
pub fn execute(plan: &Plan) -> Result<Option<PathBuf>> {
    if plan.is_empty() {
        return Ok(None);
    }
    let backup_dir = backup::create(
        &backup::cosmic_config_dir()?,
        &backup::backups_dir()?,
        &service_states(),
    )
    .context("backup failed; nothing was changed")?;

    let result = plan
        .changes
        .iter()
        .try_for_each(Change::apply)
        .and_then(|()| {
            plan.services
                .iter()
                .try_for_each(|&(service, want)| service.set_enabled(want))
        });
    if let Err(error) = result {
        if let Err(rollback) = restore_from(&backup_dir) {
            return Err(error.context(format!(
                "rollback failed too ({rollback:#}); restore manually with \
                 `cosmic-macos-setup restore`"
            )));
        }
        return Err(error.context("apply failed and the previous configuration was restored"));
    }
    Ok(Some(backup_dir))
}

#[derive(Debug, PartialEq, Eq)]
pub enum RestoreTarget {
    Latest,
    First,
    Dir(PathBuf),
}

/// What a restore changed.
#[derive(Debug)]
pub struct Restored {
    pub backup: PathBuf,
    pub keys: usize,
    pub services: Vec<(Service, bool)>,
}

/// Put config and services back to the state saved in `backup_dir`.
pub fn restore_from(backup_dir: &Path) -> Result<Restored> {
    let saved = backup::services(backup_dir)?;
    let keys = backup::restore(&backup::cosmic_config_dir()?, backup_dir)?;
    let mut services = Vec::new();
    for (name, want) in saved {
        let Some(service) = Service::from_name(&name) else {
            continue;
        };
        if service.enabled() != want {
            service.set_enabled(want)?;
            services.push((service, want));
        }
    }
    Ok(Restored {
        backup: backup_dir.to_owned(),
        keys,
        services,
    })
}

pub fn restore(target: RestoreTarget) -> Result<Restored> {
    let dir = match target {
        RestoreTarget::Dir(dir) => dir,
        RestoreTarget::Latest => backup::list(&backup::backups_dir()?)?
            .pop()
            .context("no backups found")?,
        RestoreTarget::First => backup::list(&backup::backups_dir()?)?
            .into_iter()
            .next()
            .context("no backups found")?,
    };
    restore_from(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_services_that_differ_change() {
        let options = Options {
            window_controls_left: Some(true),
            three_finger_drag: Some(false),
            ..Options::default()
        };
        // Window controls already left, drag currently on: only drag changes.
        let got = service_changes(&options, |_| true);
        assert_eq!(got, vec![(Service::ThreeFingerDrag, false)]);
        // Unrequested services never change.
        assert!(service_changes(&Options::default(), |_| false).is_empty());
    }

    #[test]
    fn service_names_round_trip() {
        for service in Service::ALL {
            assert_eq!(Service::from_name(service.name()), Some(service));
        }
        assert_eq!(Service::from_name("bogus"), None);
    }

    #[test]
    fn empty_plan_does_nothing() {
        let plan = Plan {
            changes: Vec::new(),
            services: Vec::new(),
        };
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
        assert!(execute(&plan).unwrap().is_none());
    }
}
