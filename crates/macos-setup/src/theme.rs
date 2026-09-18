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
    let is_frosted = matches!(preset, ThemePreset::LiquidGlass);
    let target_alpha = match preset {
        ThemePreset::LiquidGlass => opacity.clamp(0.05, 1.0),
        ThemePreset::Classic => 1.0,
    };
    let alpha_byte = (target_alpha * 255.0).round().clamp(5.0, 255.0) as u8;
    let alpha_hex = format!("{:02X}", alpha_byte);

    let theme_names = [
        "com.system76.CosmicTheme.Dark",
        "com.system76.CosmicTheme.Light",
        "com.system76.CosmicTheme.Dark.Builder",
        "com.system76.CosmicTheme.Light.Builder",
    ];

    for theme_name in theme_names {
        let theme_dir = cosmic_dir.join(theme_name);
        if !theme_dir.exists() {
            continue;
        }

        // --- Update v1 ---
        let v1_dir = theme_dir.join("v1");
        if v1_dir.exists() {
            for file_name in ["background", "primary", "secondary"] {
                let file_path = v1_dir.join(file_name);
                if let Ok(content) = fs::read_to_string(&file_path) {
                    let new_content = update_ron_alpha(&content, target_alpha);
                    if new_content != content {
                        let _ = fs::write(&file_path, new_content);
                    }
                }
            }
        }

        // --- Update v2 ---
        let v2_dir = theme_dir.join("v2");
        if v2_dir.exists() {
            let frosted_val = if is_frosted { "true" } else { "false" };
            for file_name in [
                "frosted_windows",
                "frosted_panel",
                "frosted_applets",
                "frosted_system_interface",
            ] {
                let file_path = v2_dir.join(file_name);
                let _ = fs::write(&file_path, frosted_val);
            }

            let frosted_level = if is_frosted { "Medium" } else { "None" };
            let _ = fs::write(v2_dir.join("frosted"), frosted_level);

            for file_name in [
                "background",
                "transparent_background",
                "primary",
                "transparent_primary",
                "secondary",
                "transparent_secondary",
            ] {
                let file_path = v2_dir.join(file_name);
                if let Ok(content) = fs::read_to_string(&file_path) {
                    let new_content = update_v2_hex_alpha(&content, &alpha_hex);
                    if new_content != content {
                        let _ = fs::write(&file_path, new_content);
                    }
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

fn update_v2_hex_alpha(content: &str, alpha_hex: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("base:") || trimmed.starts_with("base :"))
            && let Some(hash_idx) = line.find('#')
            && line.len() >= hash_idx + 9
        {
            let hex_part = &line[hash_idx..hash_idx + 9];
            if hex_part.starts_with('#') && hex_part.chars().skip(1).all(|c| c.is_ascii_hexdigit()) {
                let mut new_line = String::new();
                new_line.push_str(&line[..hash_idx + 7]);
                new_line.push_str(alpha_hex);
                new_line.push_str(&line[hash_idx + 9..]);
                result.push_str(&new_line);
                result.push('\n');
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
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

    #[test]
    fn v2_hex_alpha_update_works() {
        let sample = r##"(
    base: "#1B1B1BFF",
    component: (
        base: "#2E2E2EFF",
    ),
)"##;
        let updated = update_v2_hex_alpha(sample, "8C");
        println!("UPDATED CONTENT:\n{updated}");
        assert!(updated.contains("base: \"#1B1B1B8C\","));
        assert!(updated.contains("base: \"#2E2E2E8C\","));
    }
}


