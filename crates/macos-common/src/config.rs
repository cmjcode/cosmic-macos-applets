// SPDX-License-Identifier: GPL-3.0-only
//! Per-applet configuration stored through `cosmic-config`.
//!
//! Every field has a default so that missing or partially written config
//! never prevents an applet from starting.

use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// Configuration of the system menu applet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
#[serde(default)]
pub struct MenuConfig {
    /// Icon theme name shown in the panel.
    pub icon_name: String,
    /// Show "About This Computer".
    pub show_about: bool,
    /// Show the "App Store" entry (hidden automatically if `cosmic-store` is missing).
    pub show_app_store: bool,
    /// Ask for confirmation (via `cosmic-osd`) before restart, shut down and log out.
    pub confirm_power_actions: bool,
}

impl Default for MenuConfig {
    fn default() -> Self {
        Self {
            icon_name: "io.github.jayuda.CosmicMacosMenu-symbolic".into(),
            show_about: true,
            show_app_store: true,
            confirm_power_actions: true,
        }
    }
}

/// Configuration of the focused-application applet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
#[serde(default)]
pub struct ActiveAppConfig {
    /// Longest label, in characters, before it is truncated with an ellipsis.
    pub max_chars: u16,
    /// Render the label in a bold font, like macOS.
    pub bold: bool,
    /// Label shown when no window is focused. Empty means the localized
    /// default ("Desktop"); a single space hides the label.
    pub empty_label: String,
    /// Only consider windows on the output this panel instance lives on.
    pub follow_panel_output: bool,
    /// Experimental global application menu (File, Edit, View…).
    ///
    /// Hosts `com.canonical.AppMenu.Registrar`. Apps that support it (Qt, and
    /// X11 apps such as Electron with `--ozone-platform=x11` or JetBrains IDEs)
    /// then move their menu bar into the panel. Apps only notice the registrar
    /// when they start, so restart them after enabling.
    pub global_menu: bool,
}

impl Default for ActiveAppConfig {
    fn default() -> Self {
        Self {
            max_chars: 32,
            bold: true,
            empty_label: String::new(),
            follow_panel_output: true,
            global_menu: false,
        }
    }
}

/// A block of the Control Center popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Section {
    /// Wi-Fi and Bluetooth card, next to Now Playing.
    Connectivity,
    /// Do Not Disturb, dark mode and screenshot.
    Toggles,
    /// Display brightness slider (hidden without a backlight).
    Display,
    /// Output volume slider.
    Sound,
    /// Lock screen, settings and battery.
    Shortcuts,
}

/// OS and applet visual theme preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePreset {
    /// Classic macOS look using default COSMIC theme colors.
    #[default]
    Classic,
    /// Liquid Glass aesthetic with glassmorphism translucency, glowing liquid accent, and frosted glass borders.
    LiquidGlass,
}

impl ThemePreset {
    pub const CHOICES: [Self; 2] = [Self::Classic, Self::LiquidGlass];
}

/// Configuration of the Control Center applet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
#[serde(default)]
pub struct ControlCenterConfig {
    /// Theme preset used by applets and OS setup.
    pub theme_preset: ThemePreset,
    /// Blocks shown in the popup, top to bottom. Unknown or duplicate entries are ignored.
    pub sections: Vec<Section>,
    /// Show the Now Playing card beside the connectivity card.
    pub show_now_playing: bool,
    /// Highest volume the slider allows, in percent (100 – 150).
    pub max_volume: u32,
}

impl Default for ControlCenterConfig {
    fn default() -> Self {
        Self {
            theme_preset: ThemePreset::Classic,
            sections: vec![
                Section::Connectivity,
                Section::Toggles,
                Section::Display,
                Section::Sound,
                Section::Shortcuts,
            ],
            show_now_playing: true,
            max_volume: 100,
        }
    }
}

impl ControlCenterConfig {
    /// Sections in display order with duplicates removed.
    #[must_use]
    pub fn unique_sections(&self) -> Vec<Section> {
        let mut out = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            if !out.contains(section) {
                out.push(*section);
            }
        }
        out
    }

    /// `max_volume` clamped to a safe range.
    #[must_use]
    pub fn volume_limit(&self) -> u32 {
        self.max_volume.clamp(100, 150)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let menu = MenuConfig::default();
        assert!(
            menu.confirm_power_actions,
            "power actions must confirm by default"
        );
        assert!(!menu.icon_name.is_empty());

        let active = ActiveAppConfig::default();
        assert!(active.max_chars >= 8);
        assert!(active.follow_panel_output);
        assert!(!active.global_menu, "global menu is opt-in");
    }

    #[test]
    fn control_center_sections_are_deduplicated_and_volume_clamped() {
        let config = ControlCenterConfig {
            theme_preset: ThemePreset::LiquidGlass,
            sections: vec![Section::Sound, Section::Display, Section::Sound],
            show_now_playing: false,
            max_volume: 500,
        };
        assert_eq!(config.theme_preset, ThemePreset::LiquidGlass);
        assert_eq!(
            config.unique_sections(),
            vec![Section::Sound, Section::Display]
        );
        assert_eq!(config.volume_limit(), 150);
        assert_eq!(
            ControlCenterConfig {
                max_volume: 0,
                ..config
            }
            .volume_limit(),
            100
        );
    }
}
