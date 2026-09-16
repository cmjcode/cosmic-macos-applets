// SPDX-License-Identifier: GPL-3.0-only

use std::sync::{Arc, LazyLock};

use cosmic::{
    Element, Task, app,
    applet::{menu_button, padded_control},
    cctk::sctk::reexports::calloop,
    cosmic_theme::Spacing,
    iced::{Length, Subscription, core::text::Wrapping, widget::column, window},
    theme,
    widget::{Id, autosize, divider, space, text},
};
use macos_common::{
    ACTIVE_APP_APP_ID,
    config::ActiveAppConfig,
    desktop_index::{DesktopEntry, DesktopIndex, Resolution, humanize_app_id},
};

use crate::{
    fl,
    model::{FocusHistory, Toplevel, ToplevelId, ellipsize},
    wayland::{self, Request},
};

static AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("macos-active-app-autosize"));

/// What the panel currently shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Focused {
    id: Option<ToplevelId>,
    app_id: String,
    name: String,
}

pub struct ActiveAppApplet {
    core: app::Core,
    config: ActiveAppConfig,
    toplevels: Arc<[Toplevel]>,
    history: FocusHistory,
    index: DesktopIndex,
    focused: Focused,
    requests: Option<calloop::channel::Sender<Request>>,
    popup: Option<window::Id>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Wayland(wayland::Event),
    DesktopEntries(Arc<Vec<DesktopEntry>>),
    Config(ActiveAppConfig),
    TogglePopup,
    PopupClosed(window::Id),
    Minimize,
    Quit,
}

impl ActiveAppApplet {
    fn scan_desktop_entries(&mut self) -> app::Task<Message> {
        if !self.index.begin_scan() {
            return Task::none();
        }
        let locales = self.index.locales().to_vec();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || DesktopIndex::scan(&locales))
                    .await
                    .unwrap_or_else(|error| {
                        tracing::error!(%error, "desktop entry scan panicked");
                        Vec::new()
                    })
            },
            |entries| cosmic::action::app(Message::DesktopEntries(Arc::new(entries))),
        )
    }

    /// Recompute the label; returns a task if a desktop entry rescan is needed.
    fn refresh(&mut self) -> app::Task<Message> {
        let output = self.core.applet.output_name.as_str();
        let selected = self
            .history
            .select(
                &self.toplevels,
                Some(output),
                self.config.follow_panel_output,
            )
            .cloned();

        let Some(window) = selected else {
            self.focused = Focused::default();
            return Task::none();
        };

        let mut task = Task::none();
        let name = match self.index.resolve(&window.app_id) {
            Resolution::Hit(meta) => meta.name,
            Resolution::Miss { rescan } => {
                if rescan {
                    task = self.scan_desktop_entries();
                }
                Some(humanize_app_id(&window.app_id))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| window.title.clone())
            }
        };

        self.focused = Focused {
            id: Some(window.id),
            app_id: window.app_id,
            name,
        };
        task
    }

    fn label(&self) -> String {
        if self.focused.id.is_some() {
            return ellipsize(&self.focused.name, usize::from(self.config.max_chars));
        }
        match self.config.empty_label.as_str() {
            "" => fl!("desktop"),
            custom => custom.trim().to_owned(),
        }
    }

    fn send(&self, request: Request) {
        match &self.requests {
            Some(tx) if tx.send(request).is_ok() => {}
            _ => tracing::warn!(?request, "wayland thread unavailable"),
        }
    }

    fn close_popup(&mut self) -> app::Task<Message> {
        self.popup.take().map_or_else(Task::none, |id| {
            cosmic::surface::surface_task(cosmic::surface::action::destroy_popup(id))
        })
    }
}

impl cosmic::Application for ActiveAppApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ACTIVE_APP_APP_ID;

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        let mut applet = Self {
            core,
            config: ActiveAppConfig::default(),
            toplevels: Arc::from([]),
            history: FocusHistory::default(),
            index: DesktopIndex::new(),
            focused: Focused::default(),
            requests: None,
            popup: None,
        };
        let scan = applet.scan_desktop_entries();
        (applet, scan)
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            wayland::subscription().map(Message::Wayland),
            self.core
                .watch_config::<ActiveAppConfig>(ACTIVE_APP_APP_ID)
                .map(|update| {
                    for error in update.errors {
                        tracing::warn!(?error, "active app applet config");
                    }
                    Message::Config(update.config)
                }),
        ])
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Wayland(wayland::Event::Ready(tx)) => self.requests = Some(tx),
            Message::Wayland(wayland::Event::Toplevels(toplevels)) => {
                self.history.observe(&toplevels);
                self.toplevels = toplevels;
                let refresh = self.refresh();
                // Close a stale popup if the window it described went away.
                if self.popup.is_some() && self.focused.id.is_none() {
                    return Task::batch([refresh, self.close_popup()]);
                }
                return refresh;
            }
            Message::Wayland(wayland::Event::Unavailable(reason)) => {
                tracing::error!(%reason, "window tracking unavailable");
                self.requests = None;
                self.toplevels = Arc::from([]);
                self.focused = Focused::default();
            }
            Message::DesktopEntries(entries) => {
                let entries = Arc::try_unwrap(entries).unwrap_or_else(|arc| (*arc).clone());
                self.index.install(entries);
                return self.refresh();
            }
            Message::Config(config) => {
                self.config = config;
                return self.refresh();
            }
            Message::TogglePopup => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                if self.focused.id.is_none() {
                    return Task::none();
                }
                return cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                    |_| Default::default(),
                    |app: &mut Self| {
                        let id = window::Id::unique();
                        app.popup = Some(id);
                        let mut settings = app.core.applet.get_popup_settings(
                            app.core.main_window_id().unwrap_or(window::Id::RESERVED),
                            id,
                            None,
                            None,
                            None,
                        );
                        settings.positioner.size_limits = settings
                            .positioner
                            .size_limits
                            .min_width(220.0)
                            .max_width(280.0);
                        settings
                    },
                    None,
                ));
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::Minimize => {
                if let Some(id) = self.focused.id {
                    self.send(Request::Minimize(id));
                }
                return self.close_popup();
            }
            Message::Quit => {
                // Close every window of the focused application, like "Quit" on macOS.
                if !self.focused.app_id.is_empty() {
                    for window in self
                        .toplevels
                        .iter()
                        .filter(|t| t.app_id == self.focused.app_id)
                    {
                        self.send(Request::Close(window.id));
                    }
                } else if let Some(id) = self.focused.id {
                    self.send(Request::Close(id));
                }
                return self.close_popup();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // A single line: long names are ellipsized instead of wrapping.
        let mut label = self.core.applet.text(self.label()).wrapping(Wrapping::None);
        if self.config.bold && self.focused.id.is_some() {
            label = label.font(cosmic::font::bold());
        }
        let button = self.core.applet.text_button(label, Message::TogglePopup);
        // Let the applet surface grow and shrink with the label width.
        autosize::autosize(button, AUTOSIZE_ID.clone()).into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.popup != Some(id) {
            return space::horizontal().width(Length::Shrink).into();
        }
        let Spacing {
            space_xxs, space_s, ..
        } = theme::active().cosmic().spacing;
        let name = self.focused.name.clone();

        let content = column![
            padded_control(text::heading(name.clone())),
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            menu_button(text::body(fl!("hide-app", name = name.clone())))
                .on_press(Message::Minimize),
            menu_button(text::body(fl!("quit-app", name = name))).on_press(Message::Quit),
        ]
        .padding([space_xxs, 0]);

        self.core.applet.popup_container(content).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
