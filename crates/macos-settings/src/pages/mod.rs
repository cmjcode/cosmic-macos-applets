// SPDX-License-Identifier: GPL-3.0-only
//! Settings pages and the helpers they share.

pub mod active_app;
pub mod control_center;
pub mod menu;
pub mod top_bar;
pub mod windows;

use cosmic::app::Task;
use cosmic_config::{Config, CosmicConfigEntry};

use crate::app;
use crate::fl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageId {
    TopBar,
    Menu,
    ActiveApp,
    ControlCenter,
    Windows,
}

impl PageId {
    pub const ALL: [Self; 5] = [
        Self::TopBar,
        Self::Menu,
        Self::ActiveApp,
        Self::ControlCenter,
        Self::Windows,
    ];

    pub fn title(self) -> String {
        match self {
            Self::TopBar => fl!("page-top-bar"),
            Self::Menu => fl!("page-menu"),
            Self::ActiveApp => fl!("page-active-app"),
            Self::ControlCenter => fl!("page-control-center"),
            Self::Windows => fl!("page-windows"),
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            Self::TopBar => "preferences-panel-symbolic",
            Self::Menu => "open-menu-symbolic",
            Self::ActiveApp => "focus-windows-symbolic",
            Self::ControlCenter => "preferences-system-symbolic",
            Self::Windows => "preferences-window-management-symbolic",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TopBar(top_bar::Message),
    Menu(menu::Message),
    ActiveApp(active_app::Message),
    ControlCenter(control_center::Message),
    Windows(windows::Message),
}

/// The searchable text of one section: its title and every item label.
pub struct SectionDef {
    pub title: String,
    pub labels: Vec<String>,
}

impl SectionDef {
    pub fn new(title: String, labels: impl IntoIterator<Item = String>) -> Self {
        Self {
            title,
            labels: labels.into_iter().collect(),
        }
    }

    /// Case-insensitive match of every word in `query`, anywhere in the section.
    pub fn matches(&self, query: &str) -> bool {
        let haystack = std::iter::once(&self.title)
            .chain(&self.labels)
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        let mut words = query.split_whitespace().peekable();
        words.peek().is_some() && words.all(|word| haystack.contains(&word.to_lowercase()))
    }
}

/// Run blocking work (systemctl, gsettings, backups) off the UI thread.
pub fn blocking<T, Work, Done>(work: Work, done: Done) -> Task<app::Message>
where
    T: Send + 'static,
    Work: FnOnce() -> T + Send + 'static,
    Done: FnOnce(T) -> Message + Send + 'static,
{
    cosmic::Task::perform(
        tokio::task::spawn_blocking(work),
        move |result| match result {
            Ok(value) => cosmic::Action::App(app::Message::Page(done(value))),
            Err(error) => {
                tracing::error!(%error, "background task failed");
                cosmic::Action::None
            }
        },
    )
}

/// Open an applet's config, logging instead of failing.
pub fn open_config<T: CosmicConfigEntry>(id: &str) -> Option<Config> {
    Config::new(id, T::VERSION)
        .inspect_err(|error| tracing::error!(%error, id, "cannot open config"))
        .ok()
}

/// Read an applet's config, keeping defaults for missing keys.
pub fn load_entry<T: CosmicConfigEntry + Default>(config: Option<&Config>) -> T {
    let Some(config) = config else {
        return T::default();
    };
    match T::get_entry(config) {
        Ok(entry) => entry,
        Err((errors, entry)) => {
            for error in errors.iter().filter(|e| e.is_err()) {
                tracing::warn!(%error, "config key unreadable, using default");
            }
            entry
        }
    }
}

/// Log a failed config write; the UI keeps the value so the user sees intent.
pub fn log_write(key: &str, result: Result<bool, cosmic_config::Error>) {
    if let Err(error) = result {
        tracing::error!(%error, key, "cannot write config");
    }
}

/// An error chain on one line, for showing in the window.
pub fn error_text(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_all_words_in_any_label() {
        let section = SectionDef::new("Window controls".into(), ["Buttons on the left".to_owned()]);
        assert!(section.matches("window"));
        assert!(section.matches("LEFT buttons"));
        assert!(!section.matches("left touchpad"));
        assert!(!section.matches("   "));
    }
}
