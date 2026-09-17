// SPDX-License-Identifier: GPL-3.0-only
//! System Menu: entries, confirmation and the panel icon.

use cosmic::Element;
use cosmic::app::Task;
use cosmic::iced::Length;
use cosmic::widget::{self, button, row, settings};
use cosmic_config::Config;
use macos_common::config::MenuConfig;

use super::{SectionDef, load_entry, log_write, open_config};
use crate::{app, fl};

#[derive(Debug, Clone)]
pub enum Message {
    ShowAbout(bool),
    ShowAppStore(bool),
    ConfirmPower(bool),
    IconInput(String),
    IconEditing(bool),
    IconReset,
}

pub struct Page {
    config: Option<Config>,
    entry: MenuConfig,
    icon_text: String,
    icon_editing: bool,
}

impl Default for Page {
    fn default() -> Self {
        let config = open_config::<MenuConfig>(macos_common::MENU_APP_ID);
        let entry: MenuConfig = load_entry(config.as_ref());
        Self {
            icon_text: entry.icon_name.clone(),
            config,
            entry,
            icon_editing: false,
        }
    }
}

impl Page {
    pub fn on_enter(&mut self) -> Task<app::Message> {
        self.entry = load_entry(self.config.as_ref());
        self.icon_text = self.entry.icon_name.clone();
        Task::none()
    }

    pub fn update(&mut self, message: Message) -> Task<app::Message> {
        let Some(config) = self.config.as_ref() else {
            return Task::none();
        };
        match message {
            Message::ShowAbout(v) => log_write("show_about", self.entry.set_show_about(config, v)),
            Message::ShowAppStore(v) => {
                log_write("show_app_store", self.entry.set_show_app_store(config, v));
            }
            Message::ConfirmPower(v) => log_write(
                "confirm_power_actions",
                self.entry.set_confirm_power_actions(config, v),
            ),
            Message::IconInput(value) => self.icon_text = value,
            Message::IconEditing(editing) => {
                self.icon_editing = editing;
                let name = self.icon_text.trim().to_owned();
                if !editing && !name.is_empty() {
                    log_write("icon_name", self.entry.set_icon_name(config, name));
                }
            }
            Message::IconReset => {
                let default = MenuConfig::default().icon_name;
                self.icon_text.clone_from(&default);
                self.icon_editing = false;
                log_write("icon_name", self.entry.set_icon_name(config, default));
            }
        }
        Task::none()
    }

    pub fn sections(&self) -> Vec<SectionDef> {
        vec![
            SectionDef::new(
                fl!("menu-entries"),
                [fl!("show-about"), fl!("show-app-store")],
            ),
            SectionDef::new(fl!("power"), [fl!("confirm-power")]),
            SectionDef::new(fl!("menu-icon"), [fl!("icon-name")]),
        ]
    }

    pub fn section_view(&self, index: usize) -> Element<'_, super::Message> {
        let element: Element<'_, Message> = match index {
            0 => settings::section()
                .title(fl!("menu-entries"))
                .add(
                    settings::item::builder(fl!("show-about"))
                        .toggler(self.entry.show_about, Message::ShowAbout),
                )
                .add(
                    settings::item::builder(fl!("show-app-store"))
                        .description(fl!("show-app-store-description"))
                        .toggler(self.entry.show_app_store, Message::ShowAppStore),
                )
                .into(),
            1 => settings::section()
                .title(fl!("power"))
                .add(
                    settings::item::builder(fl!("confirm-power"))
                        .description(fl!("confirm-power-description"))
                        .toggler(self.entry.confirm_power_actions, Message::ConfirmPower),
                )
                .into(),
            _ => {
                let input = widget::editable_input(
                    "",
                    &self.icon_text,
                    self.icon_editing,
                    Message::IconEditing,
                )
                .on_input(Message::IconInput)
                .on_submit(|_| Message::IconEditing(false))
                .width(Length::Fixed(260.0));
                let controls = row::with_capacity(2)
                    .push(input)
                    .push(button::standard(fl!("reset")).on_press(Message::IconReset))
                    .spacing(cosmic::theme::spacing().space_xs);
                settings::section()
                    .title(fl!("menu-icon"))
                    .add(
                        settings::item::builder(fl!("icon-name"))
                            .description(fl!("icon-name-description"))
                            .control(controls),
                    )
                    .into()
            }
        };
        element.map(super::Message::Menu)
    }
}
