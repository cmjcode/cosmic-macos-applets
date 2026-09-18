// SPDX-License-Identifier: GPL-3.0-only
//! macOS Control Center look built from COSMIC theme colors, so light/dark
//! themes and custom accent colors are respected.

use cosmic::{
    iced::{Background, Border, Color},
    theme,
    widget::{button, container},
};
use macos_common::config::ThemePreset;

const TILE_RADIUS: f32 = 16.0;

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = alpha;
    color
}

/// Rounded card behind a group of controls, styled by `preset`.
pub fn tile_preset(preset: ThemePreset) -> theme::Container<'static> {
    theme::Container::custom(move |theme| {
        let cosmic = theme.cosmic();
        let base_bg: Color = cosmic.primary(false).component.base.into();
        let (bg, border_color) = match preset {
            ThemePreset::LiquidGlass => (
                Background::Color(with_alpha(base_bg, 0.45)),
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.22,
                },
            ),
            ThemePreset::Classic => (
                Background::Color(base_bg),
                with_alpha(cosmic.primary(false).component.divider.into(), 0.5),
            ),
        };

        container::Style {
            background: Some(bg),
            text_color: Some(cosmic.primary(false).component.on.into()),
            border: Border {
                radius: TILE_RADIUS.into(),
                width: 1.0,
                color: border_color,
            },
            ..Default::default()
        }
    })
}

/// Rounded card behind a group of controls (Classic preset).
#[allow(dead_code)]
pub fn tile() -> theme::Container<'static> {
    tile_preset(ThemePreset::Classic)
}

fn button_style(background: Color, foreground: Color, radius: f32) -> button::Style {
    let mut style = button::Style::new();
    style.background = Some(Background::Color(background));
    style.icon_color = Some(foreground);
    style.text_color = Some(foreground);
    style.border_radius = radius.into();
    style
}

/// Circular toggle styled by `preset`.
pub fn round_toggle_preset(on: bool, preset: ThemePreset) -> theme::Button {
    let paint = move |theme: &cosmic::Theme, state: u8| {
        let cosmic = theme.cosmic();
        if on {
            let accent: Color = match preset {
                ThemePreset::LiquidGlass => Color {
                    r: 0.0,
                    g: 0.48,
                    b: 1.0,
                    a: 0.95,
                },
                ThemePreset::Classic => cosmic.accent_color().into(),
            };
            let shade = match state {
                1 => 0.9,
                2 => 0.8,
                _ => 1.0,
            };
            let bg = Color {
                r: accent.r * shade,
                g: accent.g * shade,
                b: accent.b * shade,
                a: accent.a,
            };
            button_style(bg, cosmic.on_accent_color().into(), 999.0)
        } else {
            let component = &cosmic.button;
            let bg: Color = match state {
                1 => component.hover,
                2 => component.pressed,
                _ => component.base,
            }
            .into();
            let alpha = match preset {
                ThemePreset::LiquidGlass => 0.20,
                ThemePreset::Classic => bg.a.max(0.35),
            };
            button_style(with_alpha(bg, alpha), component.on.into(), 999.0)
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_, t| paint(t, 0)),
        disabled: Box::new(move |t| {
            let mut s = paint(t, 0);
            s.icon_color = s.icon_color.map(|c| with_alpha(c, 0.4));
            s.text_color = s.text_color.map(|c| with_alpha(c, 0.4));
            s
        }),
        hovered: Box::new(move |_, t| paint(t, 1)),
        pressed: Box::new(move |_, t| paint(t, 2)),
    }
}

/// Circular toggle (Classic preset).
#[allow(dead_code)]
pub fn round_toggle(on: bool) -> theme::Button {
    round_toggle_preset(on, ThemePreset::Classic)
}

/// Filled accent circle for a selected list row, styled by `preset`.
pub fn selected_circle_preset(preset: ThemePreset) -> theme::Container<'static> {
    theme::Container::custom(move |theme| {
        let cosmic = theme.cosmic();
        let bg_color = match preset {
            ThemePreset::LiquidGlass => Color {
                r: 0.0,
                g: 0.48,
                b: 1.0,
                a: 1.0,
            },
            ThemePreset::Classic => cosmic.accent_color().into(),
        };
        container::Style {
            background: Some(Background::Color(bg_color)),
            icon_color: Some(cosmic.on_accent_color().into()),
            border: Border {
                radius: 999.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
}

/// Filled accent circle (Classic preset).
#[allow(dead_code)]
pub fn selected_circle() -> theme::Container<'static> {
    selected_circle_preset(ThemePreset::Classic)
}

/// Invisible button inside a tile, styled by `preset`.
pub fn flat_preset(preset: ThemePreset) -> theme::Button {
    let paint = move |theme: &cosmic::Theme, alpha: f32| {
        let cosmic = theme.cosmic();
        let hover_alpha = match preset {
            ThemePreset::LiquidGlass => alpha.max(0.25),
            ThemePreset::Classic => alpha,
        };
        button_style(
            with_alpha(cosmic.primary(false).component.hover.into(), hover_alpha),
            cosmic.primary(false).component.on.into(),
            12.0,
        )
    };
    theme::Button::Custom {
        active: Box::new(move |_, t| paint(t, 0.0)),
        disabled: Box::new(move |t| paint(t, 0.0)),
        hovered: Box::new(move |_, t| paint(t, 0.6)),
        pressed: Box::new(move |_, t| paint(t, 1.0)),
    }
}

/// Invisible button inside a tile (Classic preset).
#[allow(dead_code)]
pub fn flat() -> theme::Button {
    flat_preset(ThemePreset::Classic)
}
