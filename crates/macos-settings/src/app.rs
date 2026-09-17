// SPDX-License-Identifier: GPL-3.0-only
//! Window, navigation and search.

use cosmic::app::{Core, Task};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, column, container, icon, nav_bar, row, scrollable, settings, text};
use cosmic::{ApplicationExt, Apply, Element};

use crate::fl;
use crate::pages::{self, PageId, SectionDef, active_app, control_center, menu, top_bar, windows};

pub struct App {
    core: Core,
    nav: nav_bar::Model,
    active: PageId,
    top_bar: top_bar::Page,
    menu: menu::Page,
    active_app: active_app::Page,
    control_center: control_center::Page,
    windows: windows::Page,
    search_active: bool,
    search: String,
    search_id: widget::Id,
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(pages::Message),
    Navigate(PageId),
    SearchActivate,
    SearchChanged(String),
    SearchClear,
}

impl From<pages::Message> for Message {
    fn from(message: pages::Message) -> Self {
        Self::Page(message)
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = macos_common::SETTINGS_APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let mut nav = nav_bar::Model::default();
        for page in PageId::ALL {
            nav.insert()
                .text(page.title())
                .icon(icon::from_name(page.icon()))
                .data(page);
        }
        nav.activate_position(0);

        let mut app = Self {
            core,
            nav,
            active: PageId::TopBar,
            top_bar: top_bar::Page::default(),
            menu: menu::Page::default(),
            active_app: active_app::Page::default(),
            control_center: control_center::Page::default(),
            windows: windows::Page::default(),
            search_active: false,
            search: String::new(),
            search_id: widget::Id::unique(),
        };
        let title = app.set_title();
        let enter = app.activate(PageId::TopBar);
        (app, Task::batch([title, enter]))
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        match self.nav.data::<PageId>(id).copied() {
            Some(page) => self.activate(page),
            None => Task::none(),
        }
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        let search = if self.search_active {
            widget::text_input::search_input(fl!("search"), &self.search)
                .width(Length::Fixed(240.0))
                .id(self.search_id.clone())
                .on_input(Message::SearchChanged)
                .on_clear(Message::SearchClear)
                .into()
        } else {
            icon::from_name("system-search-symbolic")
                .apply(widget::button::icon)
                .padding(8)
                .on_press(Message::SearchActivate)
                .into()
        };
        vec![search]
    }

    fn on_escape(&mut self) -> Task<Message> {
        self.search_active = false;
        self.search.clear();
        Task::none()
    }

    fn dialog(&self) -> Option<Element<'_, Message>> {
        match self.active {
            PageId::TopBar => self.top_bar.dialog().map(|e| e.map(Message::Page)),
            _ => None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Page(message) => self.update_page(message),
            Message::Navigate(page) => {
                self.search_active = false;
                self.search.clear();
                let nav_id = self
                    .nav
                    .iter()
                    .find(|&id| self.nav.data::<PageId>(id) == Some(&page));
                if let Some(id) = nav_id {
                    self.nav.activate(id);
                }
                self.activate(page)
            }
            Message::SearchActivate => {
                self.search_active = true;
                widget::text_input::focus(self.search_id.clone())
            }
            Message::SearchChanged(phrase) => {
                self.search = phrase;
                Task::none()
            }
            Message::SearchClear => {
                self.search.clear();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        if self.search_active && !self.search.trim().is_empty() {
            return self.search_view();
        }
        let header = text::title3(self.active.title());
        let sections = (0..self.sections(self.active).len())
            .map(|index| self.section_view(self.active, index))
            .collect();
        column::with_capacity(2)
            .push(page_container(&self.core, header))
            .push(
                page_container(&self.core, settings::view_column(sections))
                    .apply(scrollable)
                    .height(Length::Fill),
            )
            .height(Length::Fill)
            .into()
    }
}

impl App {
    fn set_title(&mut self) -> Task<Message> {
        match self.core.main_window_id() {
            Some(id) => self.set_window_title(fl!("app-title"), id),
            None => Task::none(),
        }
    }

    fn activate(&mut self, page: PageId) -> Task<Message> {
        self.active = page;
        match page {
            PageId::TopBar => self.top_bar.on_enter(),
            PageId::Menu => self.menu.on_enter(),
            PageId::ActiveApp => self.active_app.on_enter(),
            PageId::ControlCenter => self.control_center.on_enter(),
            PageId::Windows => self.windows.on_enter(),
        }
    }

    fn update_page(&mut self, message: pages::Message) -> Task<Message> {
        match message {
            pages::Message::TopBar(m) => self.top_bar.update(m),
            pages::Message::Menu(m) => self.menu.update(m),
            pages::Message::ActiveApp(m) => self.active_app.update(m),
            pages::Message::ControlCenter(m) => self.control_center.update(m),
            pages::Message::Windows(m) => self.windows.update(m),
        }
    }

    fn sections(&self, page: PageId) -> Vec<SectionDef> {
        match page {
            PageId::TopBar => self.top_bar.sections(),
            PageId::Menu => self.menu.sections(),
            PageId::ActiveApp => self.active_app.sections(),
            PageId::ControlCenter => self.control_center.sections(),
            PageId::Windows => self.windows.sections(),
        }
    }

    fn section_view(&self, page: PageId, index: usize) -> Element<'_, Message> {
        let element = match page {
            PageId::TopBar => self.top_bar.section_view(index),
            PageId::Menu => self.menu.section_view(index),
            PageId::ActiveApp => self.active_app.section_view(index),
            PageId::ControlCenter => self.control_center.section_view(index),
            PageId::Windows => self.windows.section_view(index),
        };
        element.map(Message::Page)
    }

    /// Every matching section, grouped under a link to its page.
    fn search_view(&self) -> Element<'_, Message> {
        let mut children: Vec<Element<'_, Message>> = Vec::new();
        for page in PageId::ALL {
            let matching: Vec<usize> = self
                .sections(page)
                .iter()
                .enumerate()
                .filter(|(_, section)| section.matches(&self.search))
                .map(|(index, _)| index)
                .collect();
            if matching.is_empty() {
                continue;
            }
            children.push(
                widget::button::link(page.title())
                    .on_press(Message::Navigate(page))
                    .into(),
            );
            children.extend(
                matching
                    .into_iter()
                    .map(|index| self.section_view(page, index)),
            );
        }
        if children.is_empty() {
            children.push(
                row::with_capacity(1)
                    .push(text::body(fl!("search-no-results")))
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        page_container(&self.core, settings::view_column(children))
            .apply(scrollable)
            .height(Length::Fill)
            .into()
    }
}

/// Same centered, width-limited column as COSMIC Settings.
fn page_container<'a, Message: 'static>(
    core: &Core,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let padding = if core.is_condensed() {
        spacing.space_s
    } else {
        spacing.space_l
    };
    container(content.into())
        .max_width(800)
        .width(Length::Fill)
        .apply(container)
        .center_x(Length::Fill)
        .padding([0, padding, spacing.space_m, padding])
        .into()
}
