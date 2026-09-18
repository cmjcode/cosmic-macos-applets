// SPDX-License-Identifier: GPL-3.0-only

use std::{sync::Arc, time::Duration};

use cosmic::{
    Element, Task, app,
    applet::token::subscription::{TokenRequest, TokenUpdate, activation_token_subscription},
    cctk::sctk::reexports::calloop,
    cosmic_config::{Config, ConfigGet, ConfigSet, CosmicConfigEntry},
    cosmic_theme::{THEME_MODE_ID, ThemeMode},
    iced::{
        Alignment, Length, Subscription,
        widget::{column, row},
        window,
    },
    widget::{button, container, divider, icon, scrollable, slider, space, text, toggler},
};
use cosmic_notifications_config::NotificationsConfig;
use cosmic_settings_daemon_subscription as settings_daemon;
use cosmic_settings_upower_subscription::device::{DeviceDbusEvent, device_subscription};
use macos_common::{
    CONTROL_CENTER_APP_ID,
    config::{ControlCenterConfig, Section},
    session::PowerAction,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    brightness, fl,
    services::{
        audio::{self, AudioState},
        bluetooth::{self, BluetoothState},
        media::{self, NowPlaying},
        network::{self, WifiNetwork, WifiState},
    },
    style,
};

const PANEL_ICON: &[u8] = include_bytes!(
    "../data/icons/scalable/apps/io.github.jayuda.CosmicMacosControlCenter-symbolic.svg"
);
const POPUP_WIDTH: f32 = 344.0;
const GAP: u16 = 8;
const TILE_ROW_HEIGHT: f32 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Main,
    Wifi,
    Bluetooth,
    Sound,
}

/// Brightness state with drag throttling.
#[derive(Debug, Default)]
struct Brightness {
    tx: Option<UnboundedSender<settings_daemon::Request>>,
    max: i32,
    value: i32,
    sent: i32,
    flushing: bool,
}

#[derive(Default)]
pub struct ControlCenter {
    core: app::Core,
    config: ControlCenterConfig,
    popup: Option<window::Id>,
    page: Page,
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,

    wifi: Option<Arc<WifiState>>,
    wifi_tx: Option<UnboundedSender<network::Request>>,
    bluetooth: Option<Arc<BluetoothState>>,
    bluetooth_tx: Option<UnboundedSender<bluetooth::Request>>,
    audio: Option<Arc<AudioState>>,
    audio_tx: Option<UnboundedSender<audio::Request>>,
    /// Volume shown while dragging, before the daemon confirms it.
    volume_preview: Option<u32>,
    opacity: f32,
    opacity_preview: Option<f32>,
    media: Option<Box<NowPlaying>>,
    session_bus: Option<zbus::Connection>,
    brightness: Brightness,
    battery: Option<(u8, bool)>,
    do_not_disturb: bool,
    dark_mode: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(window::Id),
    ShowPage(Page),

    Network(network::Event),
    Bluetooth(bluetooth::Event),
    Audio(audio::Event),
    Media(media::Event),
    SessionBus(Option<zbus::Connection>),
    SettingsDaemon(settings_daemon::Event),
    Battery(DeviceDbusEvent),
    Notifications(NotificationsConfig),
    ThemeMode(ThemeMode),
    Config(ControlCenterConfig),
    Token(TokenUpdate),

    SetWifi(bool),
    ConnectWifi(WifiNetwork),
    DisconnectWifi,
    SetBluetooth(bool),
    ConnectDevice(bluer::Address),
    DisconnectDevice(bluer::Address),
    SetVolume(u32),
    VolumeReleased,
    SetOpacity(u32),
    OpacityReleased,
    ToggleMute,
    SetSink(u32),
    SetBrightness(i32),
    FlushBrightness,
    SetDoNotDisturb(bool),
    SetDarkMode(bool),
    MediaControl(media::Request),
    Screenshot,
    StartScreenshot,
    Lock,
    OpenSettings(&'static str),
}

fn send<T: std::fmt::Debug>(tx: Option<&UnboundedSender<T>>, request: T) {
    match tx {
        Some(tx) => {
            if let Err(error) = tx.send(request) {
                tracing::warn!(?error, "service channel closed");
            }
        }
        None => tracing::debug!(?request, "service not ready"),
    }
}

fn write_config<T: serde::Serialize>(id: &str, version: u64, key: &str, value: T) {
    match Config::new(id, version) {
        Ok(config) => {
            if let Err(error) = config.set(key, value) {
                tracing::error!(%error, id, key, "failed to write config");
            }
        }
        Err(error) => tracing::error!(%error, id, "failed to open config"),
    }
}

fn spawn(program: &str, args: &[&str], token: Option<&str>) {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(token) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", token);
        cmd.env("DESKTOP_STARTUP_ID", token);
    }
    tokio::spawn(cosmic::process::spawn(cmd));
}

fn wifi_icon(state: Option<&WifiState>) -> &'static str {
    match state {
        Some(s) if s.enabled => match s.connected.as_ref().map(|n| n.strength) {
            Some(80..) => "network-wireless-signal-excellent-symbolic",
            Some(55..) => "network-wireless-signal-good-symbolic",
            Some(30..) => "network-wireless-signal-ok-symbolic",
            Some(_) => "network-wireless-signal-weak-symbolic",
            None => "network-wireless-signal-none-symbolic",
        },
        _ => "network-wireless-disconnected-symbolic",
    }
}

fn volume_icon(volume: u32, muted: bool) -> &'static str {
    match (muted, volume) {
        (true, _) | (_, 0) => "audio-volume-muted-symbolic",
        (_, 1..=33) => "audio-volume-low-symbolic",
        (_, 34..=66) => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

fn sym(name: &str, size: u16) -> cosmic::widget::Icon {
    icon::from_name(name).size(size).symbolic(true).icon()
}

fn round_button<'a>(
    icon_name: &'a str,
    on: bool,
    preset: macos_common::config::ThemePreset,
    message: Option<Message>,
) -> Element<'a, Message> {
    button::custom(container(sym(icon_name, 16)).center(Length::Fill))
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .class(style::round_toggle_preset(on, preset))
        .on_press_maybe(message)
        .into()
}

fn caption_or_empty(label: Option<String>) -> Element<'static, Message> {
    match label {
        Some(label) => text::caption(label)
            .wrapping(cosmic::iced::core::text::Wrapping::None)
            .into(),
        None => space::vertical().height(Length::Shrink).into(),
    }
}

impl ControlCenter {
    fn close_popup(&mut self) -> app::Task<Message> {
        self.leave_page();
        self.popup.take().map_or_else(Task::none, |id| {
            cosmic::surface::surface_task(cosmic::surface::action::destroy_popup(id))
        })
    }

    fn leave_page(&mut self) {
        if self.page == Page::Wifi {
            send(self.wifi_tx.as_ref(), network::Request::Watch(false));
        }
        self.page = Page::Main;
    }

    fn launch_settings(&self, page: &'static str) {
        let exec = if page.is_empty() {
            "cosmic-settings".to_owned()
        } else {
            format!("cosmic-settings {page}")
        };
        match &self.token_tx {
            Some(tx)
                if tx
                    .send(TokenRequest {
                        app_id: CONTROL_CENTER_APP_ID.to_owned(),
                        exec: exec.clone(),
                    })
                    .is_ok() => {}
            _ => {
                let args: Vec<&str> = exec.split_whitespace().skip(1).collect();
                spawn("cosmic-settings", &args, None);
            }
        }
    }

    // ---- views --------------------------------------------------------

    fn connectivity_card(&self) -> Element<'_, Message> {
        let wifi = self.wifi.as_deref();
        let wifi_on = wifi.is_some_and(|w| w.enabled);
        let wifi_caption = match wifi {
            None => Some(fl!("unavailable")),
            Some(w) if !w.present => Some(fl!("no-device")),
            Some(w) if w.hardware_blocked => Some(fl!("airplane-mode")),
            Some(w) if !w.enabled => Some(fl!("off")),
            Some(w) => Some(
                w.connected
                    .as_ref()
                    .map_or_else(|| fl!("not-connected"), |n| n.ssid.clone()),
            ),
        };

        let bt = self.bluetooth.as_deref();
        let bt_on = bt.is_some_and(|b| b.powered);
        let bt_caption = match bt {
            None => Some(fl!("unavailable")),
            Some(b) if !b.powered => Some(fl!("off")),
            Some(b) => Some(match b.connected_names().as_slice() {
                [] => fl!("on"),
                [one] => (*one).to_owned(),
                many => fl!("devices-connected", count = many.len()),
            }),
        };

        let line = |icon_name: &'static str,
                    on: bool,
                    toggle: Option<Message>,
                    title: String,
                    caption: Option<String>,
                    page: Page| {
            row![
                round_button(icon_name, on, self.config.theme_preset, toggle),
                button::custom(
                    column![
                        text::body(title).font(cosmic::font::semibold()),
                        caption_or_empty(caption)
                    ]
                    .width(Length::Fill),
                )
                .class(style::flat_preset(self.config.theme_preset))
                .padding([2, 6])
                .width(Length::Fill)
                .on_press(Message::ShowPage(page)),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        };

        container(
            column![
                line(
                    wifi_icon(wifi),
                    wifi_on,
                    wifi.filter(|w| w.present && !w.hardware_blocked)
                        .map(|_| Message::SetWifi(!wifi_on)),
                    fl!("wifi"),
                    wifi_caption,
                    Page::Wifi,
                ),
                line(
                    if bt_on {
                        "bluetooth-active-symbolic"
                    } else {
                        "bluetooth-disabled-symbolic"
                    },
                    bt_on,
                    bt.map(|_| Message::SetBluetooth(!bt_on)),
                    fl!("bluetooth"),
                    bt_caption,
                    Page::Bluetooth,
                ),
            ]
            .spacing(GAP),
        )
        .padding(10)
        .width(Length::Fill)
        .class(style::tile_preset(self.config.theme_preset))
        .into()
    }

    fn media_card(&self) -> Option<Element<'_, Message>> {
        let media = self
            .media
            .as_deref()
            .filter(|_| self.config.show_now_playing)?;
        let title = if media.title.is_empty() {
            fl!("unknown-title")
        } else {
            media.title.clone()
        };
        let controls = row![
            round_button(
                "media-skip-backward-symbolic",
                false,
                self.config.theme_preset,
                media
                    .can_go_previous
                    .then_some(Message::MediaControl(media::Request::Previous)),
            ),
            round_button(
                if media.playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                },
                false,
                self.config.theme_preset,
                media
                    .can_play_pause
                    .then_some(Message::MediaControl(media::Request::PlayPause)),
            ),
            round_button(
                "media-skip-forward-symbolic",
                false,
                self.config.theme_preset,
                media
                    .can_go_next
                    .then_some(Message::MediaControl(media::Request::Next)),
            ),
        ]
        .spacing(4);

        Some(
            container(
                column![
                    text::body(crate::ellipsize(&title, 22)).font(cosmic::font::semibold()),
                    caption_or_empty(
                        (!media.artist.is_empty()).then(|| crate::ellipsize(&media.artist, 26))
                    ),
                    space::vertical().height(Length::Fill),
                    controls,
                ]
                .spacing(2),
            )
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(style::tile_preset(self.config.theme_preset))
            .into(),
        )
    }

    fn toggles_row(&self) -> Element<'_, Message> {
        let preset = self.config.theme_preset;
        let focus = container(
            row![
                round_button(
                    "notification-disabled-symbolic",
                    self.do_not_disturb,
                    preset,
                    Some(Message::SetDoNotDisturb(!self.do_not_disturb))
                ),
                column![
                    text::body(fl!("focus")).font(cosmic::font::semibold()),
                    text::caption(if self.do_not_disturb {
                        fl!("do-not-disturb")
                    } else {
                        fl!("off")
                    }),
                ],
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(10)
        .width(Length::FillPortion(2))
        .center_y(Length::Fill)
        .class(style::tile_preset(preset));

        let square = |icon_name: &'static str, on: bool, message: Message| {
            container(round_button(icon_name, on, preset, Some(message)))
                .center_x(Length::FillPortion(1))
                .center_y(Length::Fill)
                .class(style::tile_preset(preset))
        };

        row![
            focus,
            square(
                "weather-clear-night-symbolic",
                self.dark_mode,
                Message::SetDarkMode(!self.dark_mode)
            ),
            square(
                "accessories-screenshot-symbolic",
                false,
                Message::Screenshot
            ),
        ]
        .spacing(GAP)
        .height(Length::Fixed(TILE_ROW_HEIGHT))
        .into()
    }

    fn display_card(&self) -> Option<Element<'_, Message>> {
        let b = &self.brightness;
        if b.max <= 0 {
            return None;
        }
        Some(
            container(
                column![
                    text::body(fl!("display")).font(cosmic::font::semibold()),
                    row![
                        sym("display-brightness-symbolic", 16),
                        slider(
                            brightness::floor(b.max)..=b.max,
                            b.value,
                            Message::SetBrightness
                        )
                        .step(1)
                        .width(Length::Fill),
                        text::caption(format!("{}%", brightness::percent(b.value, b.max)))
                            .width(Length::Fixed(36.0)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(6),
            )
            .padding(10)
            .width(Length::Fill)
            .class(style::tile_preset(self.config.theme_preset))
            .into(),
        )
    }

    fn sound_card(&self) -> Element<'_, Message> {
        let state = self.audio.as_deref();
        let volume = self.volume_preview.or(state.map(|s| s.volume)).unwrap_or(0);
        let muted = state.is_some_and(|s| s.muted);
        let limit = self.config.volume_limit();
        let preset = self.config.theme_preset;

        let body: Element<'_, Message> = if state.is_some_and(|s| s.default_sink.is_some()) {
            row![
                round_button(volume_icon(volume, muted), false, preset, Some(Message::ToggleMute)),
                slider(0..=limit, volume.min(limit), Message::SetVolume)
                    .on_release(Message::VolumeReleased)
                    .width(Length::Fill),
                button::custom(sym("go-next-symbolic", 16))
                    .class(style::flat_preset(preset))
                    .padding(6)
                    .on_press(Message::ShowPage(Page::Sound)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        } else {
            text::caption(fl!("unavailable")).into()
        };

        container(
            column![
                text::body(fl!("sound")).font(cosmic::font::semibold()),
                body
            ]
            .spacing(6),
        )
        .padding(10)
        .width(Length::Fill)
        .class(style::tile_preset(preset))
        .into()
    }

    fn opacity_card(&self) -> Element<'_, Message> {
        let opacity = self.opacity_preview.unwrap_or(self.opacity).clamp(0.05, 1.0);
        let percent = (opacity * 100.0).round() as u32;
        let preset = self.config.theme_preset;

        container(
            column![
                text::body(fl!("opacity")).font(cosmic::font::semibold()),
                row![
                    sym("color-select-symbolic", 16),
                    slider(5..=100, percent, Message::SetOpacity)
                        .on_release(Message::OpacityReleased)
                        .width(Length::Fill),
                    text::caption(format!("{percent}%")).width(Length::Fixed(36.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(6),
        )
        .padding(10)
        .width(Length::Fill)
        .class(style::tile_preset(preset))
        .into()
    }

    fn shortcuts_row(&self) -> Element<'_, Message> {
        let preset = self.config.theme_preset;
        let battery: Element<'_, Message> = match self.battery {
            Some((percent, on_battery)) => container(
                row![
                    sym(
                        if on_battery {
                            "battery-symbolic"
                        } else {
                            "ac-adapter-symbolic"
                        },
                        16
                    ),
                    text::body(format!("{percent}%")),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .center_x(Length::FillPortion(2))
            .center_y(Length::Fill)
            .class(style::tile_preset(preset))
            .into(),
            None => space::horizontal().width(Length::FillPortion(2)).into(),
        };
        let square = |icon_name: &'static str, message: Message| {
            container(round_button(icon_name, false, preset, Some(message)))
                .center_x(Length::FillPortion(1))
                .center_y(Length::Fill)
                .class(style::tile_preset(preset))
        };
        row![
            square("system-lock-screen-symbolic", Message::Lock),
            square("video-display-symbolic", Message::OpenSettings("displays")),
            square("emblem-system-symbolic", Message::OpenSettings("")),
            battery,
        ]
        .spacing(GAP)
        .height(Length::Fixed(TILE_ROW_HEIGHT))
        .into()
    }

    fn main_page(&self) -> Element<'_, Message> {
        let mut content = column![].spacing(GAP);
        for section in self.config.unique_sections() {
            content = match section {
                Section::Connectivity => {
                    let card = self.connectivity_card();
                    match self.media_card() {
                        Some(media) => {
                            content.push(row![card, media].spacing(GAP).height(Length::Shrink))
                        }
                        None => content.push(card),
                    }
                }
                Section::Toggles => content.push(self.toggles_row()),
                Section::Display => match self.display_card() {
                    Some(card) => content.push(card),
                    None => content,
                },
                Section::Sound => {
                    content = content.push(self.sound_card());
                    content.push(self.opacity_card())
                }
                Section::Shortcuts => content.push(self.shortcuts_row()),
            };
        }
        content.into()
    }

    fn detail_page<'a>(
        &'a self,
        title: String,
        header_toggle: Option<Element<'a, Message>>,
        items: Vec<Element<'a, Message>>,
        empty: String,
        settings_label: String,
        settings_page: &'static str,
    ) -> Element<'a, Message> {
        let preset = self.config.theme_preset;
        let mut header = row![
            button::custom(sym("go-previous-symbolic", 16))
                .class(style::flat_preset(preset))
                .padding(6)
                .on_press(Message::ShowPage(Page::Main)),
            text::heading(title).width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if let Some(toggle) = header_toggle {
            header = header.push(toggle);
        }

        let list: Element<'a, Message> = if items.is_empty() {
            container(text::caption(empty)).padding([12, 8]).into()
        } else {
            scrollable(column(items).spacing(2))
                .height(Length::Shrink)
                .into()
        };

        container(
            column![
                header,
                divider::horizontal::default(),
                container(list).max_height(360.0),
                divider::horizontal::default(),
                button::custom(text::body(settings_label))
                    .class(style::flat_preset(preset))
                    .padding([6, 8])
                    .width(Length::Fill)
                    .on_press(Message::OpenSettings(settings_page)),
            ]
            .spacing(6),
        )
        .padding(10)
        .class(style::tile_preset(preset))
        .into()
    }

    fn list_row<'a>(
        preset: macos_common::config::ThemePreset,
        icon_name: String,
        label: String,
        detail: Option<String>,
        selected: bool,
        message: Message,
    ) -> Element<'a, Message> {
        button::custom(
            row![
                round_button_owned(icon_name, selected, preset),
                column![text::body(label), caption_or_empty(detail)].width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .class(style::flat_preset(preset))
        .padding([4, 6])
        .width(Length::Fill)
        .on_press(message)
        .into()
    }

    fn wifi_page(&self) -> Element<'_, Message> {
        let state = self.wifi.as_deref();
        let on = state.is_some_and(|s| s.enabled);
        let preset = self.config.theme_preset;
        let items = state
            .map(|s| {
                s.networks
                    .iter()
                    .map(|n| {
                        let icon_name = match n.strength {
                            80.. => "network-wireless-signal-excellent-symbolic",
                            55.. => "network-wireless-signal-good-symbolic",
                            30.. => "network-wireless-signal-ok-symbolic",
                            _ => "network-wireless-signal-weak-symbolic",
                        };
                        let detail = if n.active {
                            Some(fl!("connected"))
                        } else if n.known {
                            Some(fl!("saved"))
                        } else if n.secured {
                            Some(fl!("secured"))
                        } else {
                            None
                        };
                        let message = if n.active {
                            Message::DisconnectWifi
                        } else {
                            Message::ConnectWifi(n.clone())
                        };
                        Self::list_row(
                            preset,
                            icon_name.to_owned(),
                            n.ssid.clone(),
                            detail,
                            n.active,
                            message,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.detail_page(
            fl!("wifi"),
            state
                .filter(|s| s.present && !s.hardware_blocked)
                .map(|_| toggler(on).on_toggle(Message::SetWifi).into()),
            items,
            if on { fl!("searching") } else { fl!("off") },
            fl!("wifi-settings"),
            "wireless",
        )
    }

    fn bluetooth_page(&self) -> Element<'_, Message> {
        let state = self.bluetooth.as_deref();
        let on = state.is_some_and(|s| s.powered);
        let preset = self.config.theme_preset;
        let items = state
            .filter(|s| s.powered)
            .map(|s| {
                s.devices
                    .iter()
                    .map(|d| {
                        let detail = match (d.connected, d.battery) {
                            (true, Some(b)) => Some(format!("{} · {b}%", fl!("connected"))),
                            (true, None) => Some(fl!("connected")),
                            _ => None,
                        };
                        let message = if d.connected {
                            Message::DisconnectDevice(d.address)
                        } else {
                            Message::ConnectDevice(d.address)
                        };
                        Self::list_row(
                            preset,
                            format!("{}-symbolic", d.icon),
                            d.name.clone(),
                            detail,
                            d.connected,
                            message,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.detail_page(
            fl!("bluetooth"),
            state.map(|_| toggler(on).on_toggle(Message::SetBluetooth).into()),
            items,
            if on {
                fl!("no-paired-devices")
            } else {
                fl!("off")
            },
            fl!("bluetooth-settings"),
            "bluetooth",
        )
    }

    fn sound_page(&self) -> Element<'_, Message> {
        let preset = self.config.theme_preset;
        let items = self
            .audio
            .as_deref()
            .map(|s| {
                s.sinks
                    .iter()
                    .map(|sink| {
                        let selected = s.default_sink == Some(sink.id);
                        let icon_name = if sink.name.to_lowercase().contains("headphone") {
                            "audio-headphones-symbolic"
                        } else {
                            "audio-speakers-symbolic"
                        };
                        Self::list_row(
                            preset,
                            icon_name.to_owned(),
                            sink.name.clone(),
                            None,
                            selected,
                            Message::SetSink(sink.id),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.detail_page(
            fl!("sound-output"),
            None,
            items,
            fl!("unavailable"),
            fl!("sound-settings"),
            "sound",
        )
    }
}

fn round_button_owned<'a>(
    icon_name: String,
    on: bool,
    preset: macos_common::config::ThemePreset,
) -> Element<'a, Message> {
    container(icon::from_name(icon_name).size(16).symbolic(true).icon())
        .center(Length::Fixed(32.0))
        .class(if on {
            style::selected_circle_preset(preset)
        } else {
            cosmic::theme::Container::Transparent
        })
        .into()
}

impl cosmic::Application for ControlCenter {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = CONTROL_CENTER_APP_ID;

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        let config = Config::new(CONTROL_CENTER_APP_ID, 1)
            .ok()
            .and_then(|c| ControlCenterConfig::get_entry(&c).ok())
            .unwrap_or_default();
        let opacity = Config::new("com.system76.CosmicPanel.Panel", 1)
            .ok()
            .and_then(|c| c.get::<f32>("opacity").ok())
            .unwrap_or(0.80);
        let applet = Self {
            core,
            config,
            opacity,
            dark_mode: ThemeMode::config()
                .ok()
                .and_then(|c| ThemeMode::get_entry(&c).ok())
                .is_none_or(|m| m.is_dark),
            ..Default::default()
        };
        let bus = Task::perform(zbus::Connection::session(), |conn| {
            cosmic::action::app(Message::SessionBus(
                conn.inspect_err(|error| tracing::error!(%error, "session bus"))
                    .ok(),
            ))
        });
        // Development aid: open the popup (optionally on a detail page) shortly
        // after start, e.g. `COSMIC_MACOS_CC_OPEN_PAGE=wifi`, for screenshots.
        let open = match std::env::var("COSMIC_MACOS_CC_OPEN_PAGE").ok().as_deref() {
            Some(page) => {
                let page = match page {
                    "wifi" => Page::Wifi,
                    "bluetooth" => Page::Bluetooth,
                    "sound" => Page::Sound,
                    _ => Page::Main,
                };
                Task::perform(tokio::time::sleep(Duration::from_secs(3)), |()| {
                    cosmic::action::app(Message::TogglePopup)
                })
                .chain(Task::perform(
                    tokio::time::sleep(Duration::from_millis(500)),
                    move |()| cosmic::action::app(Message::ShowPage(page)),
                ))
            }
            None => Task::none(),
        };
        (applet, Task::batch([bus, open]))
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            activation_token_subscription(0).map(Message::Token),
            network::subscription().map(Message::Network),
            bluetooth::subscription().map(Message::Bluetooth),
            audio::subscription().map(Message::Audio),
            device_subscription("macos-cc-battery").map(Message::Battery),
            self.core
                .watch_config::<ControlCenterConfig>(CONTROL_CENTER_APP_ID)
                .map(|u| Message::Config(u.config)),
            self.core
                .watch_config::<NotificationsConfig>(cosmic_notifications_config::ID)
                .map(|u| Message::Notifications(u.config)),
            self.core
                .watch_config::<ThemeMode>(THEME_MODE_ID)
                .map(|u| Message::ThemeMode(u.config)),
        ];
        if self.config.show_now_playing {
            subscriptions.push(media::subscription().map(Message::Media));
        }
        if let Some(bus) = &self.session_bus {
            subscriptions
                .push(settings_daemon::subscription(bus.clone()).map(Message::SettingsDaemon));
        }
        Subscription::batch(subscriptions)
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::TogglePopup => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                self.page = Page::Main;
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
                            .min_width(POPUP_WIDTH)
                            .max_width(POPUP_WIDTH);
                        settings
                    },
                    None,
                ));
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                    self.leave_page();
                }
            }
            Message::ShowPage(page) => {
                self.leave_page();
                if page == Page::Wifi {
                    send(self.wifi_tx.as_ref(), network::Request::Watch(true));
                }
                self.page = page;
            }

            Message::Network(event) => match event {
                network::Event::Ready(tx) => {
                    if self.page == Page::Wifi {
                        let _ = tx.send(network::Request::Watch(true));
                    }
                    self.wifi_tx = Some(tx);
                }
                network::Event::State(state) => self.wifi = Some(state),
                network::Event::NeedsSettings => {
                    self.launch_settings("wireless");
                    return self.close_popup();
                }
                network::Event::Unavailable => {
                    self.wifi = None;
                    self.wifi_tx = None;
                }
            },
            Message::Bluetooth(event) => match event {
                bluetooth::Event::Ready(tx) => self.bluetooth_tx = Some(tx),
                bluetooth::Event::State(state) => self.bluetooth = Some(state),
                bluetooth::Event::Unavailable => {
                    self.bluetooth = None;
                    self.bluetooth_tx = None;
                }
            },
            Message::Audio(event) => match event {
                audio::Event::Ready(tx) => self.audio_tx = Some(tx),
                audio::Event::State(state) => self.audio = Some(state),
                audio::Event::Unavailable => {
                    self.audio = None;
                    self.audio_tx = None;
                }
            },
            Message::Media(event) => {
                self.media = match event {
                    media::Event::Player(now) => Some(now),
                    media::Event::NoPlayer => None,
                };
            }
            Message::SessionBus(bus) => self.session_bus = bus,
            Message::SettingsDaemon(event) => match event {
                settings_daemon::Event::Sender(tx) => self.brightness.tx = Some(tx),
                settings_daemon::Event::MaxDisplayBrightness(max) => self.brightness.max = max,
                settings_daemon::Event::DisplayBrightness(value) => {
                    if !self.brightness.flushing {
                        self.brightness.value = value;
                        self.brightness.sent = value;
                    }
                }
            },
            Message::Battery(event) => {
                self.battery = match event {
                    DeviceDbusEvent::NoBattery => None,
                    DeviceDbusEvent::Update {
                        on_battery,
                        percent,
                        ..
                    } => Some((percent.round().clamp(0.0, 100.0) as u8, on_battery)),
                };
            }
            Message::Notifications(config) => self.do_not_disturb = config.do_not_disturb,
            Message::ThemeMode(mode) => self.dark_mode = mode.is_dark,
            Message::Config(config) => self.config = config,
            Message::Token(update) => match update {
                TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                TokenUpdate::Finished => self.token_tx = None,
                TokenUpdate::ActivationToken { token, exec } => {
                    let mut parts = exec.split_whitespace();
                    if let Some(program) = parts.next() {
                        let args: Vec<&str> = parts.collect();
                        spawn(program, &args, token.as_deref());
                    }
                }
            },

            Message::SetWifi(on) => send(self.wifi_tx.as_ref(), network::Request::SetEnabled(on)),
            Message::ConnectWifi(network) => {
                send(self.wifi_tx.as_ref(), network::Request::Connect(network))
            }
            Message::DisconnectWifi => send(self.wifi_tx.as_ref(), network::Request::Disconnect),
            Message::SetBluetooth(on) => send(
                self.bluetooth_tx.as_ref(),
                bluetooth::Request::SetPowered(on),
            ),
            Message::ConnectDevice(address) => {
                send(
                    self.bluetooth_tx.as_ref(),
                    bluetooth::Request::Connect(address),
                );
            }
            Message::DisconnectDevice(address) => {
                send(
                    self.bluetooth_tx.as_ref(),
                    bluetooth::Request::Disconnect(address),
                );
            }
            Message::SetVolume(volume) => {
                self.volume_preview = Some(volume);
                send(self.audio_tx.as_ref(), audio::Request::SetVolume(volume));
            }
            Message::VolumeReleased => self.volume_preview = None,
            Message::SetOpacity(percent) => {
                let opacity = (percent as f32 / 100.0).clamp(0.05, 1.0);
                self.opacity_preview = Some(opacity);
            }
            Message::OpacityReleased => {
                if let Some(opacity) = self.opacity_preview.take() {
                    self.opacity = opacity;
                    write_config("com.system76.CosmicPanel.Panel", 1, "opacity", opacity);
                    let _ = macos_setup::theme::apply_system_theme(self.config.theme_preset, opacity);
                }
            }
            Message::ToggleMute => send(self.audio_tx.as_ref(), audio::Request::ToggleMute),
            Message::SetSink(id) => send(self.audio_tx.as_ref(), audio::Request::SetDefault(id)),
            Message::SetBrightness(raw) => {
                self.brightness.value = brightness::snap(raw, self.brightness.max);
                if !self.brightness.flushing {
                    self.brightness.flushing = true;
                    return self.update(Message::FlushBrightness);
                }
            }
            Message::FlushBrightness => {
                // Throttle: at most one D-Bus write per 50 ms while dragging.
                let b = &mut self.brightness;
                if b.value == b.sent {
                    b.flushing = false;
                    return Task::none();
                }
                b.sent = b.value;
                send(
                    b.tx.as_ref(),
                    settings_daemon::Request::SetDisplayBrightness(b.value),
                );
                return Task::perform(tokio::time::sleep(Duration::from_millis(50)), |()| {
                    cosmic::action::app(Message::FlushBrightness)
                });
            }
            Message::SetDoNotDisturb(on) => {
                self.do_not_disturb = on;
                write_config(
                    cosmic_notifications_config::ID,
                    NotificationsConfig::VERSION,
                    "do_not_disturb",
                    on,
                );
            }
            Message::SetDarkMode(dark) => {
                self.dark_mode = dark;
                // Turn off automatic switching, otherwise the choice is undone at sunset/sunrise.
                write_config(THEME_MODE_ID, ThemeMode::VERSION, "auto_switch", false);
                write_config(THEME_MODE_ID, ThemeMode::VERSION, "is_dark", dark);
            }
            Message::MediaControl(request) => {
                if let Some(media) = &self.media {
                    media::request(&media.player, request);
                }
            }
            Message::Screenshot => {
                // Let the popup disappear before the screenshot UI grabs the screen.
                let close = self.close_popup();
                let later = Task::perform(tokio::time::sleep(Duration::from_millis(300)), |()| {
                    cosmic::action::app(Message::StartScreenshot)
                });
                return close.chain(later);
            }
            Message::StartScreenshot => spawn("cosmic-screenshot", &[], None),
            Message::Lock => {
                let close = self.close_popup();
                let lock = Task::perform(PowerAction::Lock.perform(), |result| {
                    if let Err(error) = result {
                        tracing::error!(%error, "lock screen failed");
                    }
                    cosmic::action::none()
                });
                return close.chain(lock);
            }
            Message::OpenSettings(page) => {
                self.launch_settings(page);
                return self.close_popup();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut handle = icon::from_svg_bytes(PANEL_ICON);
        handle.symbolic = true;
        self.core
            .applet
            .icon_button_from_handle(handle)
            .on_press_down(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.popup != Some(id) {
            return space::horizontal().width(Length::Shrink).into();
        }
        let page = match self.page {
            Page::Main => self.main_page(),
            Page::Wifi => self.wifi_page(),
            Page::Bluetooth => self.bluetooth_page(),
            Page::Sound => self.sound_page(),
        };
        self.core
            .applet
            .popup_container(
                container(page)
                    .padding(GAP)
                    .width(Length::Fixed(POPUP_WIDTH)),
            )
            .into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
