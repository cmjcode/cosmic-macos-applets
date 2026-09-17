// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    Element, Task, app,
    applet::{
        menu_button, padded_control,
        token::subscription::{TokenRequest, TokenUpdate, activation_token_subscription},
    },
    cctk::sctk::reexports::calloop,
    cosmic_theme::Spacing,
    iced::{
        Alignment, Length, Subscription,
        widget::{column, row},
        window,
    },
    theme,
    widget::{divider, icon, space, text},
};
use macos_common::{MENU_APP_ID, config::MenuConfig, program_in_path, session::PowerAction};

use crate::fl;

const DEFAULT_ICON_SVG: &[u8] =
    include_bytes!("../data/icons/scalable/apps/io.github.jayuda.CosmicMacosMenu-symbolic.svg");

/// Programs the menu can launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    About,
    Settings,
    TopBarSettings,
    AppStore,
}

impl Launch {
    const fn command(self) -> &'static [&'static str] {
        match self {
            Self::About => &["cosmic-settings", "about"],
            Self::Settings => &["cosmic-settings"],
            Self::TopBarSettings => &[TOP_BAR_SETTINGS],
            Self::AppStore => &["cosmic-store"],
        }
    }

    fn exec(self) -> String {
        if self == Self::TopBarSettings {
            // The panel's PATH may lack ~/.local/bin, so prefer the link
            // installed next to this binary.
            let exe = std::env::current_exe().ok();
            return sibling_program(exe.as_deref(), TOP_BAR_SETTINGS, |p| p.exists());
        }
        self.command().join(" ")
    }
}

const TOP_BAR_SETTINGS: &str = "cosmic-macos-settings";

/// `<dir of exe>/<program>` when it exists, otherwise the bare program name.
fn sibling_program(
    exe: Option<&std::path::Path>,
    program: &str,
    exists: impl Fn(&std::path::Path) -> bool,
) -> String {
    exe.and_then(std::path::Path::parent)
        .map(|dir| dir.join(program))
        .filter(|path| exists(path) && !path.to_string_lossy().contains(char::is_whitespace))
        .map_or_else(
            || program.to_owned(),
            |path| path.to_string_lossy().into_owned(),
        )
}

pub struct MenuApplet {
    core: app::Core,
    popup: Option<window::Id>,
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,
    config: MenuConfig,
    has_app_store: bool,
    has_osd: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(window::Id),
    Launch(Launch),
    Power(PowerAction),
    PowerFinished(PowerAction, Result<(), String>),
    Token(TokenUpdate),
    Config(MenuConfig),
}

impl MenuApplet {
    fn close_popup(&mut self) -> Task<cosmic::Action<Message>> {
        self.popup.take().map_or_else(Task::none, |id| {
            cosmic::surface::surface_task(cosmic::surface::action::destroy_popup(id))
        })
    }

    fn launch(&self, launch: Launch) {
        let exec = launch.exec();
        match &self.token_tx {
            // Ask the compositor for an activation token so the new window gets focus.
            Some(tx)
                if tx
                    .send(TokenRequest {
                        app_id: MENU_APP_ID.to_owned(),
                        exec: exec.clone(),
                    })
                    .is_ok() => {}
            // No token channel (e.g. the token thread died): launch without focus hint.
            _ => spawn_exec(&exec, None),
        }
    }

    fn power(&self, action: PowerAction) -> Task<cosmic::Action<Message>> {
        if self.config.confirm_power_actions
            && self.has_osd
            && let Some(arg) = action.osd_confirmation_arg()
        {
            match tokio::process::Command::new("cosmic-osd").arg(arg).spawn() {
                Ok(mut child) => {
                    // Reap the dialog process so it never lingers as a zombie.
                    tokio::spawn(async move {
                        let _ = child.wait().await;
                    });
                    return Task::none();
                }
                Err(error) => {
                    tracing::error!(%error, "cosmic-osd failed; performing action directly");
                }
            }
        }
        Task::perform(action.perform(), move |result| {
            cosmic::action::app(Message::PowerFinished(
                action,
                result.map_err(|e| e.to_string()),
            ))
        })
    }

    fn icon(&self) -> icon::Handle {
        if self.config.icon_name == MenuConfig::default().icon_name {
            // Embedded so the applet looks right even before the icon cache is refreshed.
            let mut handle = icon::from_svg_bytes(DEFAULT_ICON_SVG);
            handle.symbolic = true;
            handle
        } else {
            icon::from_name(self.config.icon_name.as_str())
                .symbolic(true)
                .size(self.core.applet.suggested_size(true).0)
                .into()
        }
    }
}

fn spawn_exec(exec: &str, token: Option<&str>) {
    let mut parts = exec.split_whitespace();
    let Some(program) = parts.next() else {
        return;
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(parts);
    if let Some(token) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", token);
        cmd.env("DESKTOP_STARTUP_ID", token);
    }
    tokio::spawn(cosmic::process::spawn(cmd));
}

impl cosmic::Application for MenuApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = MENU_APP_ID;

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        let applet = Self {
            core,
            popup: None,
            token_tx: None,
            config: MenuConfig::default(),
            has_app_store: program_in_path("cosmic-store"),
            has_osd: program_in_path("cosmic-osd"),
        };
        (applet, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            activation_token_subscription(0).map(Message::Token),
            self.core
                .watch_config::<MenuConfig>(MENU_APP_ID)
                .map(|update| {
                    for error in update.errors {
                        tracing::warn!(?error, "menu applet config");
                    }
                    Message::Config(update.config)
                }),
        ])
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::TogglePopup => {
                if self.popup.is_some() {
                    return self.close_popup();
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
                        // macOS menus are narrower than COSMIC's default 360px popups.
                        settings.positioner.size_limits = settings
                            .positioner
                            .size_limits
                            .min_width(240.0)
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
            Message::Launch(launch) => {
                self.launch(launch);
                return self.close_popup();
            }
            Message::Power(action) => {
                let close = self.close_popup();
                return close.chain(self.power(action));
            }
            Message::PowerFinished(action, result) => {
                if let Err(error) = result {
                    tracing::error!(action = action.verb(), %error, "power action failed");
                    let body = fl!("action-failed", action = action.verb(), error = error);
                    let mut cmd = std::process::Command::new("notify-send");
                    cmd.args(["--app-name=COSMIC", "--icon=dialog-error-symbolic", &body]);
                    tokio::spawn(cosmic::process::spawn(cmd));
                }
            }
            Message::Token(update) => match update {
                TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                TokenUpdate::Finished => self.token_tx = None,
                TokenUpdate::ActivationToken { token, exec } => {
                    spawn_exec(&exec, token.as_deref());
                }
            },
            Message::Config(config) => self.config = config,
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.core
            .applet
            .icon_button_from_handle(self.icon())
            .on_press_down(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.popup != Some(id) {
            return space::horizontal().width(Length::Shrink).into();
        }
        let Spacing {
            space_xxs, space_s, ..
        } = theme::active().cosmic().spacing;

        let separator =
            || padded_control(divider::horizontal::default()).padding([space_xxs, space_s]);
        let item =
            |label: String, message: Message| menu_button(text::body(label)).on_press(message);
        let item_with_shortcut = |label: String, shortcut: String, message: Message| {
            menu_button(
                row![
                    text::body(label),
                    space::horizontal().width(Length::Fill),
                    text::caption(shortcut),
                ]
                .align_y(Alignment::Center)
                .spacing(space_xxs),
            )
            .on_press(message)
        };

        let mut content = column![].padding([space_xxs, 0]);

        let mut top = column![];
        if self.config.show_about {
            top = top.push(item(fl!("about"), Message::Launch(Launch::About)));
        }
        if self.config.show_about {
            content = content.push(top).push(separator());
            top = column![];
        }
        top = top.push(item(
            fl!("system-settings"),
            Message::Launch(Launch::Settings),
        ));
        top = top.push(item(
            fl!("top-bar-settings"),
            Message::Launch(Launch::TopBarSettings),
        ));
        if self.config.show_app_store && self.has_app_store {
            top = top.push(item(fl!("app-store"), Message::Launch(Launch::AppStore)));
        }

        content = content
            .push(top)
            .push(separator())
            .push(item(fl!("sleep"), Message::Power(PowerAction::Sleep)))
            .push(item(fl!("restart"), Message::Power(PowerAction::Restart)))
            .push(item(
                fl!("shut-down"),
                Message::Power(PowerAction::ShutDown),
            ))
            .push(separator())
            .push(item_with_shortcut(
                fl!("lock-screen"),
                fl!("lock-screen-shortcut"),
                Message::Power(PowerAction::Lock),
            ))
            .push(item_with_shortcut(
                fl!("log-out"),
                fl!("log-out-shortcut"),
                Message::Power(PowerAction::LogOut),
            ));

        self.core.applet.popup_container(content).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn top_bar_settings_prefers_the_installed_link() {
        let exe = Path::new("/home/u/.local/bin/cosmic-macos-applets");
        assert_eq!(
            sibling_program(Some(exe), TOP_BAR_SETTINGS, |_| true),
            "/home/u/.local/bin/cosmic-macos-settings"
        );
        assert_eq!(
            sibling_program(Some(exe), TOP_BAR_SETTINGS, |_| false),
            TOP_BAR_SETTINGS
        );
        assert_eq!(
            sibling_program(Some(Path::new("/my apps/bin/x")), TOP_BAR_SETTINGS, |_| {
                true
            }),
            TOP_BAR_SETTINGS,
            "exec strings are split on whitespace"
        );
        assert_eq!(
            sibling_program(None, TOP_BAR_SETTINGS, |_| true),
            TOP_BAR_SETTINGS
        );
    }
}
