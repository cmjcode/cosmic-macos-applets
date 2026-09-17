// SPDX-License-Identifier: GPL-3.0-only
//! Control Center: which blocks the popup shows, their order, and limits.

use cosmic::Element;
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, icon, row, settings, text};
use cosmic_config::Config;
use macos_common::config::{ControlCenterConfig, Section};

use super::{SectionDef, load_entry, log_write, open_config};
use crate::{app, fl};

/// Canonical order, used to place a block that is switched back on.
const ALL: [Section; 5] = [
    Section::Connectivity,
    Section::Toggles,
    Section::Display,
    Section::Sound,
    Section::Shortcuts,
];

fn rank(section: Section) -> usize {
    ALL.iter().position(|s| *s == section).unwrap_or(ALL.len())
}

/// Show or hide `section`. A shown block goes before the first visible block
/// that comes after it in the default order.
#[must_use]
pub fn set_shown(sections: &[Section], section: Section, shown: bool) -> Vec<Section> {
    let mut out: Vec<Section> = sections.iter().copied().filter(|s| *s != section).collect();
    if shown {
        let at = out
            .iter()
            .position(|s| rank(*s) > rank(section))
            .unwrap_or(out.len());
        out.insert(at, section);
    }
    out
}

/// Move a visible block one step up (`-1`) or down (`1`).
#[must_use]
pub fn move_by(sections: &[Section], section: Section, delta: isize) -> Vec<Section> {
    let mut out = sections.to_vec();
    if let Some(from) = out.iter().position(|s| *s == section) {
        let to = from.saturating_add_signed(delta);
        if to < out.len() {
            out.swap(from, to);
        }
    }
    out
}

fn label(section: Section) -> String {
    match section {
        Section::Connectivity => fl!("cc-connectivity"),
        Section::Toggles => fl!("cc-toggles"),
        Section::Display => fl!("cc-display"),
        Section::Sound => fl!("cc-sound"),
        Section::Shortcuts => fl!("cc-shortcuts"),
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Shown(Section, bool),
    Move(Section, isize),
    NowPlaying(bool),
    MaxVolume(u32),
    MaxVolumeReleased,
    ResetLayout,
}

pub struct Page {
    config: Option<Config>,
    entry: ControlCenterConfig,
    max_volume: u32,
}

impl Default for Page {
    fn default() -> Self {
        let config = open_config::<ControlCenterConfig>(macos_common::CONTROL_CENTER_APP_ID);
        let entry: ControlCenterConfig = load_entry(config.as_ref());
        Self {
            max_volume: entry.volume_limit(),
            config,
            entry,
        }
    }
}

impl Page {
    pub fn on_enter(&mut self) -> Task<app::Message> {
        self.entry = load_entry(self.config.as_ref());
        self.entry.sections = self.entry.unique_sections();
        self.max_volume = self.entry.volume_limit();
        Task::none()
    }

    fn save_sections(&mut self, sections: Vec<Section>) {
        if let Some(config) = self.config.as_ref() {
            log_write("sections", self.entry.set_sections(config, sections));
        }
    }

    pub fn update(&mut self, message: Message) -> Task<app::Message> {
        match message {
            Message::Shown(section, shown) => {
                self.save_sections(set_shown(&self.entry.sections, section, shown));
            }
            Message::Move(section, delta) => {
                self.save_sections(move_by(&self.entry.sections, section, delta));
            }
            Message::ResetLayout => self.save_sections(ALL.to_vec()),
            Message::NowPlaying(v) => {
                if let Some(config) = self.config.as_ref() {
                    log_write(
                        "show_now_playing",
                        self.entry.set_show_now_playing(config, v),
                    );
                }
            }
            Message::MaxVolume(v) => self.max_volume = v,
            Message::MaxVolumeReleased => {
                if let Some(config) = self.config.as_ref() {
                    log_write(
                        "max_volume",
                        self.entry.set_max_volume(config, self.max_volume),
                    );
                }
            }
        }
        Task::none()
    }

    pub fn sections(&self) -> Vec<SectionDef> {
        vec![
            SectionDef::new(
                fl!("cc-layout"),
                ALL.into_iter().map(label).chain([fl!("reset")]),
            ),
            SectionDef::new(fl!("cc-media"), [fl!("now-playing"), fl!("max-volume")]),
        ]
    }

    fn layout_row(&self, section: Section) -> Element<'_, Message> {
        let position = self.entry.sections.iter().position(|s| *s == section);
        let shown = position.is_some();
        let last = self.entry.sections.len().saturating_sub(1);
        let arrow = |name: &'static str, enabled: bool, delta: isize| {
            button::icon(icon::from_name(name))
                .on_press_maybe(enabled.then_some(Message::Move(section, delta)))
        };
        let controls = row::with_capacity(3)
            .push(arrow("go-up-symbolic", position.is_some_and(|p| p > 0), -1))
            .push(arrow(
                "go-down-symbolic",
                position.is_some_and(|p| p < last),
                1,
            ))
            .push(widget::toggler(shown).on_toggle(move |v| Message::Shown(section, v)))
            .spacing(cosmic::theme::spacing().space_xxs)
            .align_y(Alignment::Center);
        settings::item(label(section), controls).into()
    }

    pub fn section_view(&self, index: usize) -> Element<'_, super::Message> {
        let element: Element<'_, Message> = if index == 0 {
            // Visible blocks in popup order, then hidden ones in default order.
            let hidden = ALL.into_iter().filter(|s| !self.entry.sections.contains(s));
            let mut section = settings::section().title(fl!("cc-layout"));
            for block in self.entry.sections.iter().copied().chain(hidden) {
                section = section.add(self.layout_row(block));
            }
            section
                .add(
                    settings::item::builder(fl!("reset"))
                        .description(fl!("cc-reset-description"))
                        .control(button::standard(fl!("reset")).on_press(Message::ResetLayout)),
                )
                .into()
        } else {
            let volume = row::with_capacity(2)
                .push(
                    widget::slider(100..=150, self.max_volume, Message::MaxVolume)
                        .step(5u32)
                        .on_release(Message::MaxVolumeReleased)
                        .width(Length::Fixed(220.0)),
                )
                .push(text::body(format!("{}%", self.max_volume)).width(Length::Fixed(48.0)))
                .spacing(cosmic::theme::spacing().space_s)
                .align_y(Alignment::Center);
            settings::section()
                .title(fl!("cc-media"))
                .add(
                    settings::item::builder(fl!("now-playing"))
                        .toggler(self.entry.show_now_playing, Message::NowPlaying),
                )
                .add(
                    settings::item::builder(fl!("max-volume"))
                        .description(fl!("max-volume-description"))
                        .control(volume),
                )
                .into()
        };
        element.map(super::Message::ControlCenter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Section::*;

    #[test]
    fn shown_blocks_return_to_their_default_place() {
        let current = [Sound, Connectivity];
        // Display comes before Sound in the default order, so it lands before it.
        assert_eq!(
            set_shown(&current, Display, true),
            vec![Display, Sound, Connectivity]
        );
        assert_eq!(
            set_shown(&current, Shortcuts, true),
            vec![Sound, Connectivity, Shortcuts]
        );
        assert_eq!(set_shown(&current, Sound, false), vec![Connectivity]);
        assert_eq!(
            set_shown(&current, Sound, true),
            vec![Connectivity, Sound],
            "no duplicates"
        );
    }

    #[test]
    fn moving_stays_in_bounds() {
        let current = [Connectivity, Sound, Shortcuts];
        assert_eq!(
            move_by(&current, Sound, -1),
            vec![Sound, Connectivity, Shortcuts]
        );
        assert_eq!(
            move_by(&current, Sound, 1),
            vec![Connectivity, Shortcuts, Sound]
        );
        assert_eq!(move_by(&current, Connectivity, -1), current.to_vec());
        assert_eq!(move_by(&current, Shortcuts, 1), current.to_vec());
        assert_eq!(
            move_by(&current, Display, 1),
            current.to_vec(),
            "hidden blocks do not move"
        );
    }
}
