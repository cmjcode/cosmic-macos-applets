// SPDX-License-Identifier: GPL-3.0-only
//! Bluetooth through BlueZ (`bluer`, as used by `cosmic-applet-bluetooth`).
//!
//! Like macOS Control Center, only paired devices are listed; pairing new
//! devices happens in COSMIC Settings. That avoids discovery and polling:
//! everything is driven by BlueZ property-change signals.

use std::{pin::Pin, sync::Arc, time::Duration};

use bluer::{Adapter, AdapterEvent, Address, Session};
use cosmic::iced::{
    Subscription,
    futures::{FutureExt, SinkExt, Stream, StreamExt, channel::mpsc, stream::SelectAll},
    stream,
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

const DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub address: Address,
    pub name: String,
    pub icon: String,
    pub connected: bool,
    pub battery: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BluetoothState {
    pub present: bool,
    pub powered: bool,
    /// Paired devices: connected first, then by name.
    pub devices: Vec<Device>,
}

impl BluetoothState {
    #[must_use]
    pub fn connected_names(&self) -> Vec<&str> {
        self.devices
            .iter()
            .filter(|d| d.connected)
            .map(|d| d.name.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Request {
    SetPowered(bool),
    Connect(Address),
    Disconnect(Address),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(UnboundedSender<Request>),
    State(Arc<BluetoothState>),
    Unavailable,
}

/// Append the address tail to devices that share a name (e.g. a mouse paired
/// over both Bluetooth Classic and LE), so the list stays unambiguous.
pub fn disambiguate(devices: &mut [Device]) {
    let names: Vec<String> = devices.iter().map(|d| d.name.clone()).collect();
    for device in devices.iter_mut() {
        if names.iter().filter(|n| **n == device.name).count() > 1 {
            let address = device.address.to_string();
            let tail = address
                .get(address.len().saturating_sub(5)..)
                .unwrap_or(&address);
            device.name = format!("{} ({tail})", device.name);
        }
    }
}

pub fn sort_devices(devices: &mut [Device]) {
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-cc-bluetooth", |_| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let mut backoff = Duration::from_secs(2);
            loop {
                if let Err(error) = run(&mut output).await {
                    tracing::info!(%error, "bluetooth unavailable, retrying in {backoff:?}");
                    let _ = output.send(Event::Unavailable).await;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        })
    })
}

type Signals = SelectAll<Pin<Box<dyn Stream<Item = ()> + Send>>>;

async fn watch_devices(adapter: &Adapter) -> bluer::Result<Signals> {
    let mut signals: Signals = SelectAll::new();
    for address in adapter.device_addresses().await? {
        if let Ok(device) = adapter.device(address)
            && let Ok(events) = device.events().await
        {
            signals.push(Box::pin(events.map(|_| ())));
        }
    }
    Ok(signals)
}

async fn read_state(adapter: &Adapter) -> bluer::Result<BluetoothState> {
    let powered = adapter.is_powered().await?;
    let mut devices = Vec::new();
    for address in adapter.device_addresses().await? {
        let Ok(device) = adapter.device(address) else {
            continue;
        };
        if !device.is_paired().await.unwrap_or(false) {
            continue;
        }
        let (alias, name, icon, connected, battery) = cosmic::iced::futures::join!(
            device.alias(),
            device.name(),
            device.icon(),
            device.is_connected(),
            device.battery_percentage(),
        );
        devices.push(Device {
            address,
            name: alias
                .ok()
                .or(name.ok().flatten())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| address.to_string()),
            icon: icon.ok().flatten().unwrap_or_else(|| "bluetooth".into()),
            connected: connected.unwrap_or(false),
            battery: battery.ok().flatten(),
        });
    }
    disambiguate(&mut devices);
    sort_devices(&mut devices);
    Ok(BluetoothState {
        present: true,
        powered,
        devices,
    })
}

async fn run(output: &mut mpsc::Sender<Event>) -> bluer::Result<()> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    let mut adapter_events = adapter.events().await?;
    let mut device_signals = watch_devices(&adapter).await?;

    let (tx, mut rx) = unbounded_channel();
    let _ = output.send(Event::Ready(tx)).await;

    let mut last: Option<BluetoothState> = None;
    let mut dirty = true;
    loop {
        if dirty {
            dirty = false;
            let state = read_state(&adapter).await?;
            if last.as_ref() != Some(&state) {
                last = Some(state.clone());
                if output.send(Event::State(Arc::new(state))).await.is_err() {
                    return Ok(());
                }
            }
        }

        tokio::select! {
            event = adapter_events.next() => {
                let Some(event) = event else {
                    return Err(bluer::Error { kind: bluer::ErrorKind::NotFound, message: "adapter removed".into() });
                };
                if matches!(event, AdapterEvent::DeviceAdded(_) | AdapterEvent::DeviceRemoved(_)) {
                    device_signals = watch_devices(&adapter).await?;
                }
                dirty = true;
            }
            Some(()) = device_signals.next(), if !device_signals.is_empty() => {
                tokio::time::sleep(DEBOUNCE).await;
                while let Some(Some(())) = device_signals.next().now_or_never() {}
                dirty = true;
            }
            request = rx.recv() => {
                let Some(request) = request else { return Ok(()) };
                let adapter = adapter.clone();
                // Connecting can take seconds; never block the event loop on it.
                tokio::spawn(async move {
                    let result = match request {
                        Request::SetPowered(on) => adapter.set_powered(on).await,
                        Request::Connect(address) => match adapter.device(address) {
                            Ok(device) => device.connect().await,
                            Err(error) => Err(error),
                        },
                        Request::Disconnect(address) => match adapter.device(address) {
                            Ok(device) => device.disconnect().await,
                            Err(error) => Err(error),
                        },
                    };
                    if let Err(error) = result {
                        tracing::warn!(%error, ?request, "bluetooth request failed");
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, connected: bool) -> Device {
        Device {
            address: Address::any(),
            name: name.into(),
            icon: "audio-headphones".into(),
            connected,
            battery: None,
        }
    }

    #[test]
    fn connected_devices_come_first_then_alphabetical() {
        let mut devices = vec![
            dev("mouse", false),
            dev("Keyboard", false),
            dev("AirPods", true),
        ];
        sort_devices(&mut devices);
        let names: Vec<_> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["AirPods", "Keyboard", "mouse"]);
        let state = BluetoothState {
            present: true,
            powered: true,
            devices,
        };
        assert_eq!(state.connected_names(), ["AirPods"]);
    }

    #[test]
    fn duplicate_names_get_address_suffix() {
        let mut a = dev("Logi M650", false);
        a.address = Address::new([0xAA, 0xBB, 0xCC, 0xDD, 0x12, 0x34]);
        let mut b = dev("Logi M650", false);
        b.address = Address::new([0xAA, 0xBB, 0xCC, 0xDD, 0x56, 0x78]);
        let mut devices = vec![a, b, dev("Keyboard", false)];
        disambiguate(&mut devices);
        let names: Vec<_> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Logi M650 (12:34)", "Logi M650 (56:78)", "Keyboard"]
        );
    }
}
