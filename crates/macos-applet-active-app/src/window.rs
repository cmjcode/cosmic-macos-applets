// SPDX-License-Identifier: GPL-3.0-only

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Duration,
};

use cosmic::{
    Element, Task, app,
    applet::{menu_button, padded_control},
    cctk::sctk::reexports::calloop,
    cosmic_theme::Spacing,
    iced::{
        Alignment, Length, Rectangle, Subscription,
        core::text::Wrapping,
        widget::{column, row},
        window,
    },
    theme,
    widget::{
        Id, autosize, divider,
        rectangle_tracker::{RectangleTracker, RectangleUpdate, rectangle_tracker_subscription},
        scrollable, space, text,
    },
};
use macos_common::{
    ACTIVE_APP_APP_ID,
    config::ActiveAppConfig,
    desktop_index::{DesktopEntry, DesktopIndex, Resolution, humanize_app_id},
};

use crate::{
    fl,
    global_menu::{
        matcher::Focus,
        model::{Item, Menu, Toggle},
        service as menu_service,
    },
    model::{FocusHistory, Toplevel, ToplevelId, ellipsize},
    wayland::{self, Request},
};

/// Rectangle tracker key of the application name; menu titles use `index + 1`.
const APP_NAME_KEY: u32 = 0;

static AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("macos-active-app-autosize"));

/// What the panel currently shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Focused {
    id: Option<ToplevelId>,
    app_id: String,
    name: String,
    /// Program from the desktop entry, used to match global menus.
    program: Option<String>,
}

/// What the open popup shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PopupKind {
    /// Hide / Quit for the focused app.
    App,
    /// A global menu, drilled down along these entry ids (title first).
    Menu(Vec<i32>),
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
    popup_kind: PopupKind,
    menu: Option<Arc<Menu>>,
    menu_requests: Option<tokio::sync::mpsc::UnboundedSender<menu_service::Request>>,
    last_focus_sent: Option<Focus>,
    rectangle_tracker: Option<RectangleTracker<u32>>,
    rectangles: HashMap<u32, Rectangle>,
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
    GlobalMenu(menu_service::Event),
    Rectangle(RectangleUpdate<u32>),
    /// Open or close the menu under this title id.
    ToggleMenu(i32),
    /// An entry in an open menu was clicked.
    MenuEntry(i32),
    MenuBack,
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
            self.send_focus();
            return Task::none();
        };

        let mut task = Task::none();
        let (name, program) = match self.index.resolve(&window.app_id) {
            Resolution::Hit(meta) => (meta.name, meta.program),
            Resolution::Miss { rescan } => {
                if rescan {
                    task = self.scan_desktop_entries();
                }
                let name = Some(humanize_app_id(&window.app_id))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| window.title.clone());
                (name, None)
            }
        };

        self.focused = Focused {
            id: Some(window.id),
            app_id: window.app_id,
            name,
            program,
        };
        self.send_focus();
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

    /// Tell the global menu service which app is focused (only on change).
    fn send_focus(&mut self) {
        let Some(tx) = &self.menu_requests else {
            return;
        };
        // Position of the focused window among its app's windows, oldest
        // first (toplevel ids grow in creation order).
        let mut siblings: Vec<ToplevelId> = self
            .toplevels
            .iter()
            .filter(|t| !self.focused.app_id.is_empty() && t.app_id == self.focused.app_id)
            .map(|t| t.id)
            .collect();
        siblings.sort_unstable();
        let focus = Focus {
            app_id: self.focused.app_id.clone(),
            program: self.focused.program.clone(),
            program_path: None,
            window_index: self
                .focused
                .id
                .and_then(|id| siblings.iter().position(|s| *s == id))
                .unwrap_or(0),
            window_count: siblings.len(),
        };
        if self.last_focus_sent.as_ref() != Some(&focus)
            && tx.send(menu_service::Request::Focus(focus.clone())).is_ok()
        {
            self.last_focus_sent = Some(focus);
        }
    }

    fn open_popup(&mut self, kind: PopupKind, anchor_key: u32) -> app::Task<Message> {
        let close = self.close_popup();
        self.popup_kind = kind;
        let anchor = self.rectangles.get(&anchor_key).copied();
        let open = cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            |_| Default::default(),
            move |app: &mut Self| {
                let id = window::Id::unique();
                app.popup = Some(id);
                let mut settings = app.core.applet.get_popup_settings(
                    app.core.main_window_id().unwrap_or(window::Id::RESERVED),
                    id,
                    None,
                    None,
                    None,
                );
                // Drop down right under the clicked title, like macOS.
                if let Some(r) = anchor {
                    settings.positioner.anchor_rect = Rectangle::<i32> {
                        x: r.x.max(0.0) as i32,
                        y: r.y.max(0.0) as i32,
                        width: r.width.max(1.0) as i32,
                        height: r.height.max(1.0) as i32,
                    };
                }
                settings.positioner.size_limits = settings
                    .positioner
                    .size_limits
                    .min_width(220.0)
                    .max_width(340.0);
                settings
            },
            None,
        ));
        close.chain(open)
    }

    fn send_menu(&self, request: menu_service::Request) {
        if let Some(tx) = &self.menu_requests {
            let _ = tx.send(request);
        }
    }

    /// Keep an open menu consistent after the layout changed.
    fn prune_open_menu(&mut self) -> app::Task<Message> {
        let PopupKind::Menu(stack) = &mut self.popup_kind else {
            return Task::none();
        };
        let Some(menu) = self.menu.clone() else {
            return self.close_popup();
        };
        if let Some(valid) = stack.iter().position(|id| menu.find(*id).is_none()) {
            stack.truncate(valid);
        }
        if stack.is_empty() {
            return self.close_popup();
        }
        Task::none()
    }

    fn menu_page(&self, stack: &[i32]) -> Element<'_, Message> {
        let Spacing {
            space_xxs, space_s, ..
        } = theme::active().cosmic().spacing;
        let entry = self
            .menu
            .as_deref()
            .and_then(|m| stack.last().and_then(|id| m.find(*id)));
        let Some(entry) = entry else {
            return space::horizontal().width(Length::Shrink).into();
        };

        let mut list = column![];
        if stack.len() > 1 {
            list = list
                .push(
                    menu_button(
                        row![
                            text::body("‹"),
                            text::body(entry.label.clone()).font(cosmic::font::semibold())
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .on_press(Message::MenuBack),
                )
                .push(padded_control(divider::horizontal::default()).padding([space_xxs, space_s]));
        }
        for item in &entry.children {
            list = match item {
                Item::Separator => list.push(
                    padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
                ),
                Item::Entry(child) => {
                    let mark = match child.toggle {
                        Toggle::Check(true) => "✓",
                        Toggle::Radio(true) => "•",
                        _ => "",
                    };
                    let trailing = if child.submenu {
                        Some("›".to_owned())
                    } else {
                        child.shortcut.clone()
                    };
                    let label = if child.enabled {
                        text::body(child.label.clone())
                    } else {
                        text::body(child.label.clone()).class(cosmic::style::Text::Custom(
                            |theme| {
                                let mut color: cosmic::iced::Color =
                                    theme.cosmic().on_bg_color().into();
                                color.a = 0.45;
                                cosmic::iced::widget::text::Style {
                                    color: Some(color),
                                    ..Default::default()
                                }
                            },
                        ))
                    };
                    let content = row![
                        text::body(mark).width(Length::Fixed(14.0)),
                        label.wrapping(Wrapping::None),
                        space::horizontal().width(Length::Fill),
                        text::caption(trailing.unwrap_or_default()),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center);
                    let button = menu_button(content);
                    list.push(if child.enabled {
                        button.on_press(Message::MenuEntry(child.id))
                    } else {
                        button
                    })
                }
            };
        }

        self.core
            .applet
            .popup_container(
                cosmic::widget::container(scrollable(list.padding([space_xxs, 0])))
                    .max_height(640.0),
            )
            .into()
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
            popup_kind: PopupKind::App,
            menu: None,
            menu_requests: None,
            last_focus_sent: None,
            rectangle_tracker: None,
            rectangles: HashMap::new(),
        };
        let scan = applet.scan_desktop_entries();
        // Development aid: `COSMIC_MACOS_MENU_OPEN=<title index>` opens a
        // global menu shortly after start, for screenshots.
        let open = match std::env::var("COSMIC_MACOS_MENU_OPEN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            Some(index) => Task::perform(tokio::time::sleep(Duration::from_secs(6)), move |()| {
                cosmic::action::app(Message::ToggleMenu(-1 - index as i32))
            }),
            None => Task::none(),
        };
        (applet, Task::batch([scan, open]))
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        let global_menu = if self.config.global_menu {
            menu_service::subscription().map(Message::GlobalMenu)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            global_menu,
            rectangle_tracker_subscription(0).map(|update| Message::Rectangle(update.1)),
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
                let previous_app = self.focused.app_id.clone();
                let refresh = self.refresh();
                // Close a stale popup if its window went away, and a global
                // menu whenever another app takes focus.
                let stale = self.focused.id.is_none()
                    || (matches!(self.popup_kind, PopupKind::Menu(_))
                        && previous_app != self.focused.app_id);
                if self.popup.is_some() && stale {
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
                if !config.global_menu {
                    // The subscription (and the registrar name) is dropped with it.
                    self.menu = None;
                    self.menu_requests = None;
                    self.last_focus_sent = None;
                }
                self.config = config;
                let refresh = self.refresh();
                return Task::batch([refresh, self.prune_open_menu()]);
            }
            Message::TogglePopup => {
                if self.popup.is_some() && self.popup_kind == PopupKind::App {
                    return self.close_popup();
                }
                if self.focused.id.is_none() {
                    return self.close_popup();
                }
                return self.open_popup(PopupKind::App, APP_NAME_KEY);
            }
            Message::GlobalMenu(event) => match event {
                menu_service::Event::Ready(tx) => {
                    self.menu_requests = Some(tx);
                    self.last_focus_sent = None;
                    self.send_focus();
                }
                menu_service::Event::Menu(menu) => {
                    self.menu = menu;
                    return self.prune_open_menu();
                }
            },
            Message::Rectangle(update) => match update {
                RectangleUpdate::Rectangle((key, rect)) => {
                    self.rectangles.insert(key, rect);
                }
                RectangleUpdate::Init(tracker) => self.rectangle_tracker = Some(tracker),
            },
            Message::ToggleMenu(id) => {
                let Some(menu) = self.menu.clone() else {
                    return Task::none();
                };
                // Negative ids come from the development hook: `-1 - index`.
                let id = if id < 0 {
                    match menu.titles.get((-1 - id) as usize) {
                        Some(title) => title.id,
                        None => return Task::none(),
                    }
                } else {
                    id
                };
                if matches!(&self.popup_kind, PopupKind::Menu(stack) if stack.first() == Some(&id))
                    && self.popup.is_some()
                {
                    return self.close_popup();
                }
                let Some(index) = menu.titles.iter().position(|t| t.id == id) else {
                    return Task::none();
                };
                self.send_menu(menu_service::Request::Open(id));
                return self.open_popup(PopupKind::Menu(vec![id]), index as u32 + 1);
            }
            Message::MenuEntry(id) => {
                let Some(entry) = self.menu.as_deref().and_then(|m| m.find(id)) else {
                    return Task::none();
                };
                if entry.submenu {
                    if let PopupKind::Menu(stack) = &mut self.popup_kind {
                        stack.push(id);
                    }
                    self.send_menu(menu_service::Request::Open(id));
                    return Task::none();
                }
                self.send_menu(menu_service::Request::Activate(id));
                return self.close_popup();
            }
            Message::MenuBack => {
                if let PopupKind::Menu(stack) = &mut self.popup_kind
                    && stack.len() > 1
                {
                    stack.pop();
                }
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
        let track = |key: u32, element: Element<'static, Message>| -> Element<'static, Message> {
            match &self.rectangle_tracker {
                Some(tracker) => tracker.container(key, element).ignore_bounds(true).into(),
                None => element,
            }
        };

        // A single line: long names are ellipsized instead of wrapping.
        let mut label = self.core.applet.text(self.label()).wrapping(Wrapping::None);
        if self.config.bold && self.focused.id.is_some() {
            label = label.font(cosmic::font::bold());
        }
        let mut bar = row![track(
            APP_NAME_KEY,
            self.core
                .applet
                .text_button(label, Message::TogglePopup)
                .into()
        )]
        .align_y(Alignment::Center);

        if self.config.global_menu
            && let Some(menu) = self.menu.as_deref()
        {
            for (index, title) in menu.titles.iter().enumerate() {
                let label = self
                    .core
                    .applet
                    .text(title.label.clone())
                    .wrapping(Wrapping::None);
                bar = bar.push(track(
                    index as u32 + 1,
                    self.core
                        .applet
                        .text_button(label, Message::ToggleMenu(title.id))
                        .into(),
                ));
            }
        }
        // Let the applet surface grow and shrink with its content.
        autosize::autosize(bar, AUTOSIZE_ID.clone()).into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.popup != Some(id) {
            return space::horizontal().width(Length::Shrink).into();
        }
        if let PopupKind::Menu(stack) = &self.popup_kind {
            return self.menu_page(stack);
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
