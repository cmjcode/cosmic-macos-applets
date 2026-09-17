// SPDX-License-Identifier: GPL-3.0-only
//! Windows & Touchpad: window controls on the left and three-finger drag.

use cosmic::Element;
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, settings, text};
use macos_setup::{Service, profile, three_finger_drag};

use super::{SectionDef, blocking, error_text, top_bar::warning};
use crate::{app, fl};

#[derive(Debug, Clone)]
pub struct Snapshot {
    controls_left: bool,
    drag: bool,
    drag_missing: Vec<&'static str>,
}

fn load() -> Snapshot {
    Snapshot {
        controls_left: Service::WindowControlsLeft.enabled(),
        drag: Service::ThreeFingerDrag.enabled(),
        drag_missing: three_finger_drag::missing_now(),
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Snapshot>),
    Toggle(Service, bool),
    Done(Result<(), String>),
    CopyCommands,
    OpenProject,
}

#[derive(Default)]
pub struct Page {
    snapshot: Option<Snapshot>,
    busy: bool,
    error: Option<String>,
}

impl Page {
    pub fn on_enter(&mut self) -> Task<app::Message> {
        blocking(load, |s| {
            super::Message::Windows(Message::Loaded(Box::new(s)))
        })
    }

    pub fn update(&mut self, message: Message) -> Task<app::Message> {
        match message {
            Message::Loaded(snapshot) => self.snapshot = Some(*snapshot),
            Message::Toggle(service, enable) => {
                self.busy = true;
                self.error = None;
                return blocking(
                    move || {
                        profile::plan_service(service, enable)
                            .and_then(|plan| profile::execute(&plan))
                            .map(drop)
                            .map_err(|e| error_text(&e))
                    },
                    |result| super::Message::Windows(Message::Done(result)),
                );
            }
            Message::Done(result) => {
                self.busy = false;
                self.error = result.err();
                return self.on_enter();
            }
            Message::OpenProject => {
                let mut command = std::process::Command::new("xdg-open");
                command.arg(three_finger_drag::PROJECT_URL);
                tokio::spawn(cosmic::process::spawn(command));
            }
            Message::CopyCommands => {
                return cosmic::iced::clipboard::write(
                    three_finger_drag::INSTALL_COMMANDS.to_owned(),
                );
            }
        }
        Task::none()
    }

    pub fn sections(&self) -> Vec<SectionDef> {
        vec![
            SectionDef::new(fl!("window-controls"), [fl!("controls-left")]),
            SectionDef::new(fl!("touchpad"), [fl!("three-finger-drag")]),
        ]
    }

    pub fn section_view(&self, index: usize) -> Element<'_, super::Message> {
        let Some(s) = &self.snapshot else {
            return settings::section()
                .add(settings::item_row(vec![text::body(fl!("loading")).into()]))
                .into();
        };
        let toggle = |service: Service, allowed: bool| {
            (allowed && !self.busy).then_some(move |v| Message::Toggle(service, v))
        };
        let element: Element<'_, Message> = if index == 0 {
            let mut section = settings::section().title(fl!("window-controls")).add(
                settings::item::builder(fl!("controls-left"))
                    .description(fl!("controls-left-description"))
                    .toggler_maybe(s.controls_left, toggle(Service::WindowControlsLeft, true)),
            );
            if let Some(error) = &self.error {
                section = section.add(warning(error.clone()));
            }
            section.into()
        } else {
            // Switching off always works; switching on needs the prerequisites.
            let ready = s.drag_missing.is_empty();
            let mut section = settings::section().title(fl!("touchpad")).add(
                settings::item::builder(fl!("three-finger-drag"))
                    .description(fl!("three-finger-drag-description"))
                    .toggler_maybe(s.drag, toggle(Service::ThreeFingerDrag, ready || s.drag)),
            );
            if !ready {
                let missing = s
                    .drag_missing
                    .iter()
                    .fold(column::with_capacity(s.drag_missing.len()), |col, item| {
                        col.push(text::caption(format!("• {item}")))
                    });
                let help = column::with_capacity(4)
                    .push(text::body(fl!("three-finger-drag-setup")))
                    .push(missing)
                    .push(
                        widget::container(text::monotext(three_finger_drag::INSTALL_COMMANDS))
                            .padding(cosmic::theme::spacing().space_xs)
                            .class(cosmic::theme::Container::Card)
                            .width(Length::Fill),
                    )
                    .push(
                        widget::row::with_capacity(2)
                            .push(
                                button::standard(fl!("copy-commands"))
                                    .on_press(Message::CopyCommands),
                            )
                            .push(button::link(fl!("project-page")).on_press(Message::OpenProject))
                            .spacing(cosmic::theme::spacing().space_s)
                            .align_y(Alignment::Center),
                    )
                    .spacing(cosmic::theme::spacing().space_xs);
                section = section.add(settings::item_row(vec![help.into()]));
            }
            if let Some(error) = &self.error {
                section = section.add(warning(error.clone()));
            }
            section.into()
        };
        element.map(super::Message::Windows)
    }
}
