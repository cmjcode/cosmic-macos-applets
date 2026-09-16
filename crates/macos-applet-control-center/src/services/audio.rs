// SPDX-License-Identifier: GPL-3.0-only
//! Output volume through cosmic-settings-daemon's varlink audio service,
//! the same backend `cosmic-applet-audio` uses.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use cosmic::iced::{
    Subscription,
    futures::{SinkExt, StreamExt, channel::mpsc},
    stream,
};
use cosmic_settings_audio_client::{self as audio_client, CosmicAudioProxy};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

/// An audio output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
}

/// Snapshot of the output state the UI needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioState {
    pub volume: u32,
    pub muted: bool,
    pub sinks: Vec<Sink>,
    pub default_sink: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub enum Request {
    SetVolume(u32),
    ToggleMute,
    SetDefault(u32),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(UnboundedSender<Request>),
    State(Arc<AudioState>),
    Unavailable,
}

#[derive(Debug, Default)]
struct NodeState {
    name: String,
    is_sink: bool,
    volume: u32,
    muted: bool,
}

/// Folds daemon events into [`AudioState`]. Pure, so it is unit tested.
#[derive(Debug, Default)]
pub struct Model {
    nodes: BTreeMap<u32, NodeState>,
    default_sink: Option<u32>,
}

impl Model {
    pub fn apply(&mut self, event: audio_client::Event) {
        use audio_client::Event as E;
        match event {
            E::Node(id, info) => {
                let node = self.nodes.entry(id).or_default();
                node.name = if info.description.is_empty() {
                    info.name
                } else {
                    info.description
                };
                node.is_sink = info.is_sink;
            }
            E::NodeVolume(id, volume, _) => self.nodes.entry(id).or_default().volume = volume,
            E::NodeMute(id, muted) => self.nodes.entry(id).or_default().muted = muted,
            E::DefaultSink(id) => self.default_sink = Some(id),
            E::RemoveNode(id) => {
                self.nodes.remove(&id);
                if self.default_sink == Some(id) {
                    self.default_sink = None;
                }
            }
            _ => {}
        }
    }

    #[must_use]
    pub fn state(&self) -> AudioState {
        let default = self.default_sink.and_then(|id| self.nodes.get(&id));
        AudioState {
            volume: default.map_or(0, |n| n.volume),
            muted: default.is_some_and(|n| n.muted),
            sinks: self
                .nodes
                .iter()
                .filter(|(_, n)| n.is_sink && !n.name.is_empty())
                .map(|(id, n)| Sink {
                    id: *id,
                    name: n.name.clone(),
                })
                .collect(),
            default_sink: self.default_sink,
        }
    }
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-cc-audio", |_| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match run(&mut output).await {
                    Ok(()) => backoff = Duration::from_secs(1),
                    Err(error) => {
                        tracing::warn!(%error, "audio service unavailable, retrying in {backoff:?}");
                        let _ = output.send(Event::Unavailable).await;
                    }
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        })
    })
}

async fn run(output: &mut mpsc::Sender<Event>) -> Result<(), String> {
    let mut client = audio_client::connect().await.map_err(|e| e.to_string())?;
    let mut events = client
        .recv_events()
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:?}"))?;

    let (tx, mut rx) = unbounded_channel();
    let _ = output.send(Event::Ready(tx)).await;

    let mut model = Model::default();
    let mut last = AudioState::default();
    loop {
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(event)) => model.apply(event),
                    Some(Err(error)) => tracing::debug!(?error, "undecodable audio event"),
                    None => return Err("audio event stream closed".into()),
                }
            }
            request = rx.recv() => {
                let Some(mut request) = request else { return Ok(()) };
                // Coalesce slider drags: only the newest volume matters.
                while let Ok(next) = rx.try_recv() {
                    match (request, next) {
                        (Request::SetVolume(_), Request::SetVolume(_)) => request = next,
                        _ => { perform(&mut client, request).await; request = next; }
                    }
                }
                perform(&mut client, request).await;
            }
        }
        let state = model.state();
        if state != last {
            last = state.clone();
            if output.send(Event::State(Arc::new(state))).await.is_err() {
                return Ok(());
            }
        }
    }
}

async fn perform(client: &mut audio_client::Client, request: Request) {
    let result = match request {
        Request::SetVolume(volume) => client
            .conn
            .set_sink_volume(volume)
            .await
            .map(|r| r.map(drop)),
        Request::ToggleMute => client.conn.sink_mute_toggle().await.map(|r| r.map(drop)),
        Request::SetDefault(id) => client.conn.set_default(id, true).await,
    };
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(?error, ?request, "audio request rejected"),
        Err(error) => tracing::warn!(%error, ?request, "audio request failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_client::{Event as E, NodeInfo};

    fn node(name: &str, description: &str, is_sink: bool) -> NodeInfo {
        NodeInfo {
            name: name.into(),
            description: description.into(),
            device_profile_description: String::new(),
            device_id: None,
            card_profile_device: None,
            is_sink,
        }
    }

    #[test]
    fn tracks_default_sink_volume_and_mute() {
        let mut m = Model::default();
        m.apply(E::Node(1, node("alsa.speaker", "Speakers", true)));
        m.apply(E::Node(2, node("alsa.mic", "Microphone", false)));
        m.apply(E::Node(3, node("bt.headset", "", true)));
        m.apply(E::NodeVolume(1, 40, None));
        m.apply(E::NodeVolume(3, 70, None));
        m.apply(E::DefaultSink(1));
        m.apply(E::NodeMute(1, true));

        let s = m.state();
        assert_eq!((s.volume, s.muted, s.default_sink), (40, true, Some(1)));
        let names: Vec<_> = s.sinks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Speakers", "bt.headset"], "sources are excluded");

        m.apply(E::DefaultSink(3));
        assert_eq!((m.state().volume, m.state().muted), (70, false));
    }

    #[test]
    fn removing_default_sink_clears_it() {
        let mut m = Model::default();
        m.apply(E::Node(1, node("a", "A", true)));
        m.apply(E::DefaultSink(1));
        m.apply(E::RemoveNode(1));
        assert_eq!(m.state(), AudioState::default());
    }
}
