// SPDX-License-Identifier: GPL-3.0-only
//! Active App: the focused application's label and the global menu.

use cosmic::Element;
use cosmic::app::Task;
use cosmic::iced::Length;
use cosmic::widget::{self, settings};
use cosmic_config::Config;
use macos_common::config::ActiveAppConfig;

use super::{SectionDef, load_entry, log_write, open_config};
use crate::{app, fl};

const MIN_CHARS: u16 = 8;
const MAX_CHARS: u16 = 80;

#[derive(Debug, Clone)]
pub enum Message {
    Bold(bool),
    MaxChars(u16),
    EmptyLabelInput(String),
    EmptyLabelEditing(bool),
    FollowOutput(bool),
    GlobalMenu(bool),
}

pub struct Page {
    config: Option<Config>,
    entry: ActiveAppConfig,
    empty_label: String,
    empty_label_editing: bool,
}

impl Default for Page {
    fn default() -> Self {
        let config = open_config::<ActiveAppConfig>(macos_common::ACTIVE_APP_APP_ID);
        let entry: ActiveAppConfig = load_entry(config.as_ref());
        Self {
            empty_label: entry.empty_label.clone(),
            config,
            entry,
            empty_label_editing: false,
        }
    }
}

impl Page {
    pub fn on_enter(&mut self) -> Task<app::Message> {
        self.entry = load_entry(self.config.as_ref());
        self.empty_label = self.entry.empty_label.clone();
        Task::none()
    }

    pub fn update(&mut self, message: Message) -> Task<app::Message> {
        let Some(config) = self.config.as_ref() else {
            return Task::none();
        };
        match message {
            Message::Bold(v) => log_write("bold", self.entry.set_bold(config, v)),
            Message::MaxChars(v) => log_write(
                "max_chars",
                self.entry
                    .set_max_chars(config, v.clamp(MIN_CHARS, MAX_CHARS)),
            ),
            Message::EmptyLabelInput(value) => self.empty_label = value,
            Message::EmptyLabelEditing(editing) => {
                self.empty_label_editing = editing;
                if !editing {
                    log_write(
                        "empty_label",
                        self.entry.set_empty_label(config, self.empty_label.clone()),
                    );
                }
            }
            Message::FollowOutput(v) => log_write(
                "follow_panel_output",
                self.entry.set_follow_panel_output(config, v),
            ),
            Message::GlobalMenu(v) => {
                log_write("global_menu", self.entry.set_global_menu(config, v));
            }
        }
        Task::none()
    }

    pub fn sections(&self) -> Vec<SectionDef> {
        vec![
            SectionDef::new(
                fl!("app-label"),
                [fl!("bold"), fl!("max-chars"), fl!("empty-label")],
            ),
            SectionDef::new(fl!("monitors"), [fl!("follow-output")]),
            SectionDef::new(fl!("global-menu"), [fl!("global-menu-toggle")]),
        ]
    }

    pub fn section_view(&self, index: usize) -> Element<'_, super::Message> {
        let element: Element<'_, Message> = match index {
            0 => {
                let chars = widget::spin_button(
                    self.entry.max_chars.to_string(),
                    self.entry.max_chars,
                    1,
                    MIN_CHARS,
                    MAX_CHARS,
                    Message::MaxChars,
                );
                let empty = widget::editable_input(
                    fl!("desktop"),
                    &self.empty_label,
                    self.empty_label_editing,
                    Message::EmptyLabelEditing,
                )
                .on_input(Message::EmptyLabelInput)
                .on_submit(|_| Message::EmptyLabelEditing(false))
                .width(Length::Fixed(200.0));
                settings::section()
                    .title(fl!("app-label"))
                    .add(
                        settings::item::builder(fl!("bold"))
                            .toggler(self.entry.bold, Message::Bold),
                    )
                    .add(
                        settings::item::builder(fl!("max-chars"))
                            .description(fl!("max-chars-description"))
                            .control(chars),
                    )
                    .add(
                        settings::item::builder(fl!("empty-label"))
                            .description(fl!("empty-label-description"))
                            .control(empty),
                    )
                    .into()
            }
            1 => settings::section()
                .title(fl!("monitors"))
                .add(
                    settings::item::builder(fl!("follow-output"))
                        .description(fl!("follow-output-description"))
                        .toggler(self.entry.follow_panel_output, Message::FollowOutput),
                )
                .into(),
            _ => settings::section()
                .title(fl!("global-menu"))
                .add(
                    settings::item::builder(fl!("global-menu-toggle"))
                        .description(fl!("global-menu-description"))
                        .toggler(self.entry.global_menu, Message::GlobalMenu),
                )
                .into(),
        };
        element.map(super::Message::ActiveApp)
    }
}
