// SPDX-License-Identifier: GPL-3.0-only
//! System-wide Liquid Glass theme manager for COSMIC Desktop & GTK applications.

use anyhow::{Context, Result};
use cosmic_config::{Config, ConfigSet};
use macos_common::config::ThemePreset;
use std::fs;
use std::path::{Path, PathBuf};

pub const THEME_COMPONENT: &str = "io.github.jayuda.CosmicMacosControlCenter";

pub const GTK_CSS_MARKER_START: &str = "/* === COSMIC MACOS LIQUID GLASS THEME START === */";
pub const GTK_CSS_MARKER_END: &str = "/* === COSMIC MACOS LIQUID GLASS THEME END === */";

/// Apply or remove the system-wide Liquid Glass theme across GTK and COSMIC config.
pub fn apply_system_theme(preset: ThemePreset, opacity: f32) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("cannot find HOME directory")?;

    let opacity = opacity.clamp(0.05, 1.0);

    update_gtk_css_in_home(&home, preset, opacity)?;

    if let Ok(config) = Config::new(THEME_COMPONENT, 1) {
        let _ = config.set("theme_preset", preset);
    }

    let is_frosted = matches!(preset, ThemePreset::LiquidGlass);
    if let Ok(dark_theme) = Config::new("com.system76.CosmicTheme.Dark", 1) {
        let _ = dark_theme.set("is_frosted", is_frosted);
    }
    if let Ok(light_theme) = Config::new("com.system76.CosmicTheme.Light", 1) {
        let _ = light_theme.set("is_frosted", is_frosted);
    }

    if let Ok(panel_config) = Config::new("com.system76.CosmicPanel.Panel", 1) {
        let _ = panel_config.set("opacity", opacity);
    }
    if let Ok(dock_config) = Config::new("com.system76.CosmicPanel.Dock", 1) {
        let _ = dock_config.set("opacity", opacity);
    }

    update_cosmic_theme_container_opacities(&home, preset, opacity)?;

    Ok(())
}

fn update_gtk_css_in_home(home: &Path, preset: ThemePreset, opacity: f32) -> Result<()> {
    let gtk_css = format!(
        "/* === COSMIC MACOS LIQUID GLASS THEME START === */\n\
window, .background, headerbar, .titlebar, .navigation-bar {{\n\
    background-color: rgba(22, 22, 32, {:.2}) !important;\n\
    border-color: rgba(255, 255, 255, 0.18) !important;\n\
}}\n\
.card, .frame, box.linked > button, button.flat {{\n\
    background-color: rgba(255, 255, 255, 0.08) !important;\n\
    border-radius: 12px !important;\n\
    border: 1px solid rgba(255, 255, 255, 0.15) !important;\n\
}}\n\
button:hover, .button:hover {{\n\
    background-color: rgba(255, 255, 255, 0.16) !important;\n\
}}\n\
/* === COSMIC MACOS LIQUID GLASS THEME END === */\n",
        opacity
    );

    for gtk_ver in ["gtk-3.0", "gtk-4.0"] {
        let dir = home.join(".config").join(gtk_ver);
        let css_file = dir.join("gtk.css");
        let mut content = if css_file.exists() {
            fs::read_to_string(&css_file).unwrap_or_default()
        } else {
            String::new()
        };

        // Strip existing marker block if present
        if let Some(start_idx) = content.find(GTK_CSS_MARKER_START)
            && let Some(end_idx) = content.find(GTK_CSS_MARKER_END)
        {
            let end_pos = end_idx + GTK_CSS_MARKER_END.len();
            let mut stripped = String::new();
            stripped.push_str(&content[..start_idx]);
            stripped.push_str(&content[end_pos..]);
            content = stripped;
        }

        if preset == ThemePreset::LiquidGlass {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&gtk_css);
        }

        let trimmed = content.trim();
        if trimmed.is_empty() {
            if css_file.exists() {
                let _ = fs::remove_file(&css_file);
            }
        } else {
            fs::create_dir_all(&dir)?;
            fs::write(&css_file, content.trim_start())
                .with_context(|| format!("cannot write GTK CSS to {}", css_file.display()))?;
        }
    }

    Ok(())
}

fn update_cosmic_theme_container_opacities(home: &Path, preset: ThemePreset, opacity: f32) -> Result<()> {
    let cosmic_dir = home.join(".config").join("cosmic");
    for theme_name in ["com.system76.CosmicTheme.Dark", "com.system76.CosmicTheme.Light"] {
        let v1_dir = cosmic_dir.join(theme_name).join("v1");
        if !v1_dir.exists() {
            continue;
        }
        for file_name in ["background", "primary"] {
            let file_path = v1_dir.join(file_name);
            if !file_path.exists() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&file_path) {
                let target_alpha = match preset {
                    ThemePreset::LiquidGlass => opacity.clamp(0.15, 0.95),
                    ThemePreset::Classic => 1.0,
                };
                let new_content = update_ron_alpha(&content, target_alpha);
                if new_content != content {
                    let _ = fs::write(&file_path, new_content);
                }
            }
        }
    }
    Ok(())
}

fn update_ron_alpha(content: &str, alpha: f32) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_base_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("base:") {
            in_base_block = true;
        }
        if in_base_block && trimmed.starts_with("alpha:") {
            let indent = line.len() - trimmed.len();
            result.push_str(&format!("{:indent$}alpha: {alpha:.4},\n", ""));
            in_base_block = false;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    if content.ends_with('\n') {
        result
    } else {
        result.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn gtk_css_injection_and_removal_works() {
        let tmp = tempdir().unwrap();
        let home = tmp.path();

        // Initially apply LiquidGlass with 0.55 opacity
        update_gtk_css_in_home(home, ThemePreset::LiquidGlass, 0.55).unwrap();

        let gtk4_css = home.join(".config/gtk-4.0/gtk.css");
        assert!(gtk4_css.exists());
        let content = fs::read_to_string(&gtk4_css).unwrap();
        assert!(content.contains(GTK_CSS_MARKER_START));
        assert!(content.contains("rgba(22, 22, 32, 0.55)"));

        // Switch back to Classic
        update_gtk_css_in_home(home, ThemePreset::Classic, 0.80).unwrap();
        assert!(!gtk4_css.exists(), "empty css file should be removed");
    }

    #[test]
    fn ron_alpha_update_works() {
        let sample_ron = r#"(
    base: (
        red: 0.10588235,
        green: 0.10588235,
        blue: 0.10588235,
        alpha: 1.0,
    ),
)"#;
        let updated = update_ron_alpha(sample_ron, 0.55);
        assert!(updated.contains("alpha: 0.5500,"));
    }
}

