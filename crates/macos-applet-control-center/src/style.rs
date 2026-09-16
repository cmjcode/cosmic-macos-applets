// SPDX-License-Identifier: GPL-3.0-only
//! macOS Control Center look built from COSMIC theme colors, so light/dark
//! themes and custom accent colors are respected.

use cosmic::{
    iced::{Background, Border, Color},
    theme,
    widget::{button, container},
};

const TILE_RADIUS: f32 = 16.0;

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = alpha;
    color
}

/// Rounded translucent card behind a group of controls.
pub fn tile() -> theme::Container<'static> {
    theme::Container::custom(|theme| {
        let cosmic = theme.cosmic();
        container::Style {
            background: Some(Background::Color(
                cosmic.primary(false).component.base.into(),
            )),
            text_color: Some(cosmic.primary(false).component.on.into()),
            border: Border {
                radius: TILE_RADIUS.into(),
                width: 1.0,
                color: with_alpha(cosmic.primary(false).component.divider.into(), 0.5),
            },
            ..Default::default()
        }
    })
}

fn button_style(background: Color, foreground: Color, radius: f32) -> button::Style {
    let mut style = button::Style::new();
    style.background = Some(Background::Color(background));
    style.icon_color = Some(foreground);
    style.text_color = Some(foreground);
    style.border_radius = radius.into();
    style
}

/// Circular toggle: vivid accent when `on` (like macOS' blue), neutral otherwise.
pub fn round_toggle(on: bool) -> theme::Button {
    let paint = move |theme: &cosmic::Theme, state: u8| {
        let cosmic = theme.cosmic();
        if on {
            let accent: Color = cosmic.accent_color().into();
            // Hover/press darken slightly instead of switching palette entries.
            let shade = match state {
                1 => 0.9,
                2 => 0.8,
                _ => 1.0,
            };
            let bg = Color {
                r: accent.r * shade,
                g: accent.g * shade,
                b: accent.b * shade,
                a: 1.0,
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
            button_style(with_alpha(bg, bg.a.max(0.35)), component.on.into(), 999.0)
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

/// Filled accent circle for a selected list row.
pub fn selected_circle() -> theme::Container<'static> {
    theme::Container::custom(|theme| {
        let cosmic = theme.cosmic();
        container::Style {
            background: Some(Background::Color(cosmic.accent_color().into())),
            icon_color: Some(cosmic.on_accent_color().into()),
            border: Border {
                radius: 999.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
}

/// Invisible button inside a tile (label areas that open a detail page).
pub fn flat() -> theme::Button {
    let paint = |theme: &cosmic::Theme, alpha: f32| {
        let cosmic = theme.cosmic();
        button_style(
            with_alpha(cosmic.primary(false).component.hover.into(), alpha),
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
