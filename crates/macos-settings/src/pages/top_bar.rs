// SPDX-License-Identifier: GPL-3.0-only
//! Top Bar: apply or restore the profile, tune the panel's look and place
//! notification popups.

use cosmic::Element;
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, row, settings, text};
use cosmic_config::{Config, ConfigSet};
use macos_setup::{
    Options, backup,
    notifications::{self, Position, Support},
    panel_profile, profile,
};

use super::{SectionDef, blocking, error_text};
use crate::{app, fl};

/// What the page shows, read off the UI thread.
#[derive(Debug, Clone)]
pub struct Snapshot {
    applied: bool,
    differences: Result<usize, String>,
    missing_applets: Vec<&'static str>,
    options: Options,
    backups: usize,
    /// Whether the session's notification daemon reads the position.
    support: Support,
}

fn load() -> Snapshot {
    let options = profile::current_options();
    Snapshot {
        applied: profile::profile_applied(),
        differences: profile::collect_changes(&options)
            .map(|changes| changes.len())
            .map_err(|e| error_text(&e)),
        missing_applets: profile::missing_applets(),
        backups: backup::backups_dir()
            .and_then(|dir| backup::list(&dir))
            .map_or(0, |list| list.len()),
        support: notifications::support(),
        options,
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Applied,
    NothingToDo,
    Restored(usize),
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Snapshot>),
    ThemePreset(usize),
    Opacity(u8),
    OpacityReleased,
    Weekday(bool),
    KeepNotifications(bool),
    /// Index into `Position::CHOICES`.
    Position(usize),
    CopyNotificationCommands,
    Apply,
    UndoLast,
    ConfirmRestoreOriginal(bool),
    RestoreOriginal,
    Done(Result<Outcome, String>),
}

impl From<Message> for super::Message {
    fn from(message: Message) -> Self {
        Self::TopBar(message)
    }
}

#[derive(Default)]
pub struct Page {
    snapshot: Option<Snapshot>,
    busy: bool,
    confirm_restore: bool,
    result: Option<Result<Outcome, String>>,
}

impl Page {
    pub fn on_enter(&mut self) -> Task<app::Message> {
        blocking(load, |s| Message::Loaded(Box::new(s)).into())
    }

    fn run(
        &mut self,
        work: impl FnOnce() -> anyhow::Result<Outcome> + Send + 'static,
    ) -> Task<app::Message> {
        self.busy = true;
        self.result = None;
        blocking(
            move || work().map_err(|e| error_text(&e)),
            |result| Message::Done(result).into(),
        )
    }

    fn apply_profile(&mut self) -> Task<app::Message> {
        let Some(options) = self.snapshot.as_ref().map(|s| s.options.clone()) else {
            return Task::none();
        };
        self.run(move || {
            let plan = profile::plan(&options, false)?;
            Ok(match profile::execute(&plan)? {
                Some(_) => Outcome::Applied,
                None => Outcome::NothingToDo,
            })
        })
    }

    pub fn update(&mut self, message: Message) -> Task<app::Message> {
        match message {
            Message::Loaded(snapshot) => {
                self.snapshot = Some(*snapshot);
            }
            Message::ThemePreset(index) => {
                let Some(preset) = macos_common::config::ThemePreset::CHOICES.get(index).copied() else {
                    return Task::none();
                };
                if let Some(s) = self.snapshot.as_mut() {
                    s.options.theme_preset = preset;
                    let opacity = match preset {
                        macos_common::config::ThemePreset::LiquidGlass => 0.55,
                        macos_common::config::ThemePreset::Classic => 0.80,
                    };
                    s.options.opacity = opacity;
                    let _ = macos_setup::theme::apply_system_theme(preset, opacity);
                }
            }
            Message::Opacity(percent) => {
                if let Some(s) = self.snapshot.as_mut() {
                    let opacity = f32::from(percent) / 100.0;
                    s.options.opacity = opacity;
                    write(panel_profile::PANEL_COMPONENT, "opacity", opacity);
                    let _ = macos_setup::theme::apply_system_theme(s.options.theme_preset, opacity);
                }
            }
            Message::OpacityReleased => {
                if let Some(s) = self.snapshot.as_ref() {
                    write(panel_profile::PANEL_COMPONENT, "opacity", s.options.opacity);
                    let _ = macos_setup::theme::apply_system_theme(s.options.theme_preset, s.options.opacity);
                }
            }
            Message::Weekday(show) => {
                if let Some(s) = self.snapshot.as_mut() {
                    s.options.clock_weekday = show;
                    if s.applied {
                        write(panel_profile::TIME_COMPONENT, "show_weekday", show);
                    }
                }
            }
            Message::KeepNotifications(keep) => {
                let Some(s) = self.snapshot.as_mut() else {
                    return Task::none();
                };
                s.options.keep_notifications = keep;
                if s.applied {
                    // Changes the applet list, so it goes through a backup.
                    return self.apply_profile();
                }
            }
            Message::Position(index) => {
                let Some(position) = Position::CHOICES.get(index).copied() else {
                    return Task::none();
                };
                if let Some(s) = self.snapshot.as_mut() {
                    s.options.notification_position = Some(position);
                }
                // Not part of the panel layout: the daemon watches this key
                // and moves the popups at once, like COSMIC Settings would.
                write(notifications::COMPONENT, notifications::KEY, position);
            }
            Message::CopyNotificationCommands => {
                return cosmic::iced::clipboard::write(notifications::INSTALL_COMMANDS.to_owned());
            }
            Message::Apply => return self.apply_profile(),
            Message::UndoLast => {
                return self.run(|| {
                    profile::restore(profile::RestoreTarget::Latest)
                        .map(|r| Outcome::Restored(r.keys + r.services.len()))
                });
            }
            Message::ConfirmRestoreOriginal(show) => self.confirm_restore = show,
            Message::RestoreOriginal => {
                self.confirm_restore = false;
                return self.run(|| {
                    profile::restore(profile::RestoreTarget::First)
                        .map(|r| Outcome::Restored(r.keys + r.services.len()))
                });
            }
            Message::Done(result) => {
                self.busy = false;
                self.result = Some(result);
                return self.on_enter();
            }
        }
        Task::none()
    }

    pub fn sections(&self) -> Vec<SectionDef> {
        vec![
            SectionDef::new(fl!("profile"), [fl!("profile-name"), fl!("apply")]),
            SectionDef::new(
                fl!("appearance"),
                [
                    fl!("theme-preset"),
                    fl!("opacity"),
                    fl!("show-weekday"),
                    fl!("keep-notifications"),
                ],
            ),
            SectionDef::new(
                fl!("notifications"),
                [fl!("notification-position"), fl!("notifications-daemon")],
            ),
            SectionDef::new(fl!("backups"), [fl!("undo-last"), fl!("restore-original")]),
        ]
    }

    pub fn section_view(&self, index: usize) -> Element<'_, super::Message> {
        let Some(snapshot) = &self.snapshot else {
            return settings::section()
                .add(settings::item_row(vec![text::body(fl!("loading")).into()]))
                .into();
        };
        let element: Element<'_, Message> = match index {
            0 => self.profile_section(snapshot),
            1 => self.appearance_section(snapshot),
            2 => self.notifications_section(snapshot),
            _ => self.backups_section(snapshot),
        };
        element.map(super::Message::TopBar)
    }

    fn profile_section<'a>(&'a self, s: &'a Snapshot) -> Element<'a, Message> {
        let (status, needs_apply) = match (&s.differences, s.applied) {
            (Err(error), _) => (error.clone(), false),
            (Ok(0), _) => (fl!("profile-applied"), false),
            (Ok(_), false) => (fl!("profile-not-applied"), true),
            (Ok(n), true) => (fl!("profile-differs", count = n), true),
        };
        let apply = button::suggested(fl!("apply"))
            .on_press_maybe((needs_apply && !self.busy).then_some(Message::Apply));

        let mut section = settings::section().title(fl!("profile")).add(
            settings::item::builder(fl!("profile-name"))
                .description(status)
                .control(apply),
        );
        if !s.missing_applets.is_empty() {
            section = section.add(warning(fl!(
                "applets-missing",
                applets = s.missing_applets.join(", ")
            )));
        }
        if let Some(result) = &self.result {
            section = section.add(match result {
                Ok(outcome) => settings::item_row(vec![text::body(outcome_text(outcome)).into()]),
                Err(error) => warning(error.clone()),
            });
        }
        section.into()
    }

    fn appearance_section<'a>(&'a self, s: &'a Snapshot) -> Element<'a, Message> {
        let percent = (s.options.opacity * 100.0).round().clamp(0.0, 100.0) as u8;
        let opacity = row::with_capacity(2)
            .push(
                widget::slider(0..=100, percent, Message::Opacity)
                    .on_release(Message::OpacityReleased)
                    .width(Length::Fixed(220.0)),
            )
            .push(text::body(format!("{percent}%")).width(Length::Fixed(44.0)))
            .spacing(cosmic::theme::spacing().space_s)
            .align_y(Alignment::Center);

        let theme_labels: Vec<String> = macos_common::config::ThemePreset::CHOICES
            .iter()
            .map(|p| match p {
                macos_common::config::ThemePreset::Classic => fl!("theme-classic"),
                macos_common::config::ThemePreset::LiquidGlass => fl!("theme-liquid-glass"),
            })
            .collect();
        let selected_theme = macos_common::config::ThemePreset::CHOICES
            .iter()
            .position(|c| *c == s.options.theme_preset);

        settings::section()
            .title(fl!("appearance"))
            .add(
                settings::item::builder(fl!("theme-preset"))
                    .control(widget::dropdown(theme_labels, selected_theme, Message::ThemePreset)),
            )
            .add(settings::item(fl!("opacity"), opacity))
            .add(
                settings::item::builder(fl!("show-weekday"))
                    .toggler(s.options.clock_weekday, Message::Weekday),
            )
            .add(
                settings::item::builder(fl!("keep-notifications"))
                    .description(fl!("keep-notifications-description"))
                    .toggler_maybe(
                        s.options.keep_notifications,
                        (!self.busy).then_some(Message::KeepNotifications),
                    ),
            )
            .into()
    }

    fn notifications_section<'a>(&'a self, s: &'a Snapshot) -> Element<'a, Message> {
        let labels: Vec<String> = Position::CHOICES
            .iter()
            .map(|p| position_label(*p))
            .collect();
        let selected = s
            .options
            .notification_position
            .and_then(|p| Position::CHOICES.iter().position(|c| *c == p));
        // With the applet in the bar the daemon puts popups next to it.
        let description = if s.options.keep_notifications {
            fl!("notification-position-applet")
        } else {
            fl!("notification-position-description")
        };
        let mut section = settings::section().title(fl!("notifications")).add(
            settings::item::builder(fl!("notification-position"))
                .description(description)
                .control(widget::dropdown(labels, selected, Message::Position)),
        );
        section = match s.support {
            Support::Active => section.add(
                settings::item::builder(fl!("notifications-daemon"))
                    .description(fl!("notifications-daemon-active"))
                    .control(widget::icon::from_name("emblem-ok-symbolic").size(16)),
            ),
            Support::InstalledNotRunning => section.add(
                settings::item::builder(fl!("notifications-daemon"))
                    .description(fl!("notifications-daemon-installed"))
                    .control(widget::icon::from_name("dialog-warning-symbolic").size(16)),
            ),
            Support::Missing => {
                let spacing = cosmic::theme::spacing();
                let help = column::with_capacity(3)
                    .push(text::body(fl!("notifications-daemon-missing")))
                    .push(
                        widget::container(text::monotext(notifications::INSTALL_COMMANDS))
                            .padding(spacing.space_xs)
                            .class(cosmic::theme::Container::Card)
                            .width(Length::Fill),
                    )
                    .push(
                        button::standard(fl!("copy-commands"))
                            .on_press(Message::CopyNotificationCommands),
                    )
                    .spacing(spacing.space_xs);
                section
                    .add(settings::item_row(vec![
                        text::body(fl!("notifications-daemon")).into(),
                    ]))
                    .add(settings::item_row(vec![help.into()]))
            }
        };
        section.into()
    }

    fn backups_section<'a>(&'a self, s: &'a Snapshot) -> Element<'a, Message> {
        let can_restore = s.backups > 0 && !self.busy;
        settings::section()
            .title(fl!("backups"))
            .add(
                settings::item::builder(fl!("undo-last"))
                    .description(fl!("undo-last-description", count = s.backups))
                    .control(
                        button::standard(fl!("undo"))
                            .on_press_maybe(can_restore.then_some(Message::UndoLast)),
                    ),
            )
            .add(
                settings::item::builder(fl!("restore-original"))
                    .description(fl!("restore-original-description"))
                    .control(button::destructive(fl!("restore")).on_press_maybe(
                        can_restore.then_some(Message::ConfirmRestoreOriginal(true)),
                    )),
            )
            .into()
    }

    pub fn dialog(&self) -> Option<Element<'_, super::Message>> {
        if !self.confirm_restore {
            return None;
        }
        let dialog: Element<'_, Message> = widget::dialog()
            .title(fl!("restore-original"))
            .body(fl!("restore-original-confirm"))
            .primary_action(button::destructive(fl!("restore")).on_press(Message::RestoreOriginal))
            .secondary_action(
                button::standard(fl!("cancel")).on_press(Message::ConfirmRestoreOriginal(false)),
            )
            .into();
        Some(dialog.map(super::Message::TopBar))
    }
}

fn position_label(position: Position) -> String {
    match position {
        Position::Top => fl!("position-top"),
        Position::Bottom => fl!("position-bottom"),
        Position::Left => fl!("position-left"),
        Position::Right => fl!("position-right"),
        Position::TopLeft => fl!("position-top-left"),
        Position::TopRight => fl!("position-top-right"),
        Position::BottomLeft => fl!("position-bottom-left"),
        Position::BottomRight => fl!("position-bottom-right"),
    }
}

fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Applied => fl!("result-applied"),
        Outcome::NothingToDo => fl!("result-nothing"),
        Outcome::Restored(count) => fl!("result-restored", count = count),
    }
}

/// A caption row with a warning icon, for errors and missing pieces.
pub fn warning<'a, M: 'static>(message: String) -> cosmic::widget::Row<'a, M, cosmic::Theme> {
    settings::item_row(vec![
        widget::icon::from_name("dialog-warning-symbolic")
            .size(16)
            .icon()
            .into(),
        text::body(message).width(Length::Fill).into(),
    ])
    .align_y(Alignment::Center)
}

fn write<T: serde::Serialize>(component: &str, key: &str, value: T) {
    let result = Config::new(component, 1).and_then(|config| config.set(key, value));
    if let Err(error) = result {
        tracing::error!(%error, component, key, "cannot write panel setting");
    }
}
