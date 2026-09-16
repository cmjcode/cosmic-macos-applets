// SPDX-License-Identifier: GPL-3.0-only
//! Wi-Fi state through NetworkManager (`nmrs`, as used by `cosmic-applet-network`).
//!
//! The full network list is only fetched while the Wi-Fi page is open, so a
//! closed popup costs one cheap refresh per NetworkManager event.

use std::{sync::Arc, time::Duration};

use cosmic::iced::{
    Subscription,
    futures::{SinkExt, StreamExt, channel::mpsc},
    stream,
};
use nmrs::{NetworkEvent, NetworkManager, WifiSecurity};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

/// Collapse bursts of NetworkManager signals (scans emit many) into one refresh.
const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub strength: u8,
    pub secured: bool,
    pub known: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiState {
    /// A Wi-Fi device exists.
    pub present: bool,
    pub enabled: bool,
    /// Blocked by a hardware switch / rfkill.
    pub hardware_blocked: bool,
    pub connected: Option<WifiNetwork>,
    /// Visible networks, strongest first. Empty unless the Wi-Fi page is open.
    pub networks: Vec<WifiNetwork>,
}

#[derive(Debug, Clone)]
pub enum Request {
    SetEnabled(bool),
    /// Fetch and keep refreshing the network list (Wi-Fi page open/closed).
    Watch(bool),
    Connect(WifiNetwork),
    Disconnect,
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(UnboundedSender<Request>),
    State(Arc<WifiState>),
    /// A connection needs credentials this popup does not ask for.
    NeedsSettings,
    Unavailable,
}

impl From<nmrs::Network> for WifiNetwork {
    fn from(n: nmrs::Network) -> Self {
        Self {
            ssid: n.ssid,
            strength: n.strength.unwrap_or(0),
            secured: n.secured,
            known: n.known,
            active: n.is_active,
        }
    }
}

/// Hidden networks have no usable SSID; nmrs labels them `<Hidden Network>`.
fn is_hidden(ssid: &str) -> bool {
    let ssid = ssid.trim();
    ssid.is_empty() || (ssid.starts_with('<') && ssid.ends_with('>'))
}

/// Deduplicate by SSID (keeping the strongest), drop hidden networks, and
/// order: connected first, then known, then by signal strength.
#[must_use]
pub fn sort_networks(networks: impl IntoIterator<Item = WifiNetwork>) -> Vec<WifiNetwork> {
    let mut out: Vec<WifiNetwork> = Vec::new();
    for n in networks.into_iter().filter(|n| !is_hidden(&n.ssid)) {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(existing) => {
                existing.active |= n.active;
                existing.known |= n.known;
                existing.strength = existing.strength.max(n.strength);
            }
            None => out.push(n),
        }
    }
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.known.cmp(&a.known))
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.ssid.cmp(&b.ssid))
    });
    out
}

/// Credentials to use for `network`, or `None` if the user must enter a password.
#[must_use]
pub fn credentials_for(network: &WifiNetwork) -> Option<WifiSecurity> {
    match (network.known, network.secured) {
        // Empty PSK tells nmrs to reuse the secret stored in the saved profile.
        (true, true) => Some(WifiSecurity::WpaPsk { psk: String::new() }),
        (_, false) => Some(WifiSecurity::Open),
        (false, true) => None,
    }
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-cc-network", |_| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                if let Err(error) = run(&mut output).await {
                    tracing::warn!(%error, "NetworkManager unavailable, retrying in {backoff:?}");
                    let _ = output.send(Event::Unavailable).await;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        })
    })
}

async fn read_state(nm: &NetworkManager, with_list: bool) -> nmrs::Result<WifiState> {
    let radio = nm.wifi_state().await?;
    let mut state = WifiState {
        present: radio.present,
        enabled: radio.enabled,
        hardware_blocked: !radio.hardware_enabled,
        connected: None,
        networks: Vec::new(),
    };
    if radio.present && radio.enabled {
        state.connected = nm.current_network().await?.map(WifiNetwork::from);
        if with_list {
            state.networks =
                sort_networks(nm.list_networks(None).await?.into_iter().map(Into::into));
        }
    }
    Ok(state)
}

async fn run(output: &mut mpsc::Sender<Event>) -> Result<(), String> {
    let nm = NetworkManager::new().await.map_err(|e| e.to_string())?;
    let mut events = nm.network_events().await.map_err(|e| e.to_string())?;
    let (tx, mut rx) = unbounded_channel();
    let _ = output.send(Event::Ready(tx)).await;

    let mut watching = false;
    let mut last: Option<WifiState> = None;
    let mut dirty = true;

    loop {
        if dirty {
            dirty = false;
            let state = read_state(&nm, watching).await.map_err(|e| e.to_string())?;
            if last.as_ref() != Some(&state) {
                last = Some(state.clone());
                if output.send(Event::State(Arc::new(state))).await.is_err() {
                    return Ok(());
                }
            }
        }

        tokio::select! {
            event = events.next() => match event {
                Some(Ok(NetworkEvent::NetworkManagerRestarted)) => {
                    return Err("NetworkManager restarted".into());
                }
                Some(Ok(_)) => {
                    tokio::time::sleep(DEBOUNCE).await;
                    // Drain whatever arrived during the debounce window.
                    while let Some(Some(_)) = events.next().now_or_never() {}
                    dirty = true;
                }
                Some(Err(error)) => tracing::debug!(%error, "network event error"),
                None => return Err("network event stream closed".into()),
            },
            request = rx.recv() => {
                let Some(request) = request else { return Ok(()) };
                dirty = true;
                match request {
                    Request::SetEnabled(on) => {
                        if let Err(error) = nm.set_wireless_enabled(on).await {
                            tracing::warn!(%error, "toggle Wi-Fi");
                        }
                    }
                    Request::Watch(on) => {
                        watching = on;
                        if on && let Err(error) = nm.scan_networks(None).await {
                            tracing::debug!(%error, "Wi-Fi scan request");
                        }
                    }
                    Request::Connect(network) => match credentials_for(&network) {
                        Some(creds) => {
                            if let Err(error) = nm.connect(&network.ssid, None, creds).await {
                                tracing::warn!(%error, ssid = %network.ssid, "Wi-Fi connect");
                                let _ = output.send(Event::NeedsSettings).await;
                            }
                        }
                        None => { let _ = output.send(Event::NeedsSettings).await; }
                    },
                    Request::Disconnect => {
                        if let Err(error) = nm.disconnect(None).await {
                            tracing::warn!(%error, "Wi-Fi disconnect");
                        }
                    }
                }
            }
        }
    }
}

use cosmic::iced::futures::FutureExt as _;

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, strength: u8, known: bool, active: bool) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.into(),
            strength,
            secured: true,
            known,
            active,
        }
    }

    #[test]
    fn networks_are_deduplicated_and_ordered() {
        let sorted = sort_networks([
            net("Cafe", 90, false, false),
            net("Home", 40, true, false),
            net("", 99, false, false),
            net("<Hidden Network>", 98, false, false),
            net("Office", 60, true, true),
            net("Cafe", 95, false, false),
        ]);
        let order: Vec<_> = sorted.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(order, ["Office", "Home", "Cafe"]);
        assert_eq!(sorted[2].strength, 95);
    }

    #[test]
    fn only_passwordless_connections_are_attempted() {
        let mut n = net("x", 50, true, false);
        assert!(
            matches!(credentials_for(&n), Some(WifiSecurity::WpaPsk { ref psk }) if psk.is_empty())
        );
        n.known = false;
        assert!(credentials_for(&n).is_none());
        n.secured = false;
        assert!(matches!(credentials_for(&n), Some(WifiSecurity::Open)));
    }
}
