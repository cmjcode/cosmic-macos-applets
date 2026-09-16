// SPDX-License-Identifier: GPL-3.0-only
//! Now Playing through MPRIS.
//!
//! Adapted from `cosmic-applet-audio/src/mpris_subscription.rs`
//! (Copyright 2023 System76 <info@system76.com>, GPL-3.0-only).

use std::time::Duration;

use cosmic::iced::{
    Subscription,
    futures::{self, SinkExt, StreamExt, channel::mpsc, future::OptionFuture},
    stream,
};
use mpris2_zbus::{
    enumerator,
    player::{PlaybackStatus, Player},
};
use zbus::{Connection, names::OwnedBusName};

#[derive(Debug, Clone)]
pub struct NowPlaying {
    pub player: Player,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_go_previous: bool,
    pub can_go_next: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Request {
    PlayPause,
    Next,
    Previous,
}

#[derive(Debug, Clone)]
pub enum Event {
    Player(Box<NowPlaying>),
    NoPlayer,
}

/// Perform a control request on `player` in the background.
pub fn request(player: &Player, request: Request) {
    let player = player.clone();
    tokio::spawn(async move {
        let result = match request {
            Request::PlayPause => player.play_pause().await,
            Request::Next => player.next().await,
            Request::Previous => player.previous().await,
        };
        if let Err(error) = result {
            tracing::debug!(%error, ?request, "media request failed");
        }
    });
}

/// Ranking used to pick the player to show: playing beats paused beats stopped.
#[must_use]
pub fn rank(status: Option<PlaybackStatus>, has_metadata: bool) -> i32 {
    let base = match status {
        Some(PlaybackStatus::Playing) => 100,
        Some(PlaybackStatus::Paused) => 10,
        _ => return 0,
    };
    base + i32::from(has_metadata)
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-cc-media", |_| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            loop {
                if let Err(error) = run(&mut output).await {
                    tracing::debug!(%error, "MPRIS monitor stopped, restarting");
                }
                let _ = output.send(Event::NoPlayer).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        })
    })
}

struct State {
    conn: Connection,
    players: Vec<Player>,
    active: Option<Player>,
}

impl State {
    async fn add(&mut self, name: OwnedBusName) {
        match Player::new(&self.conn, name).await {
            Ok(player) => {
                self.players.push(player);
                // Firefox exposes duplicates when plasma-browser-integration is present.
                if self.players.iter().any(|p| {
                    p.inner().destination() == "org.mpris.MediaPlayer2.plasma-browser-integration"
                }) {
                    self.players.retain(|p| {
                        !p.inner()
                            .destination()
                            .starts_with("org.mpris.MediaPlayer2.firefox.")
                    });
                }
                self.players
                    .sort_by(|a, b| a.inner().destination().cmp(b.inner().destination()));
            }
            Err(error) => tracing::debug!(%error, "cannot add MPRIS player"),
        }
    }

    async fn pick_active(&mut self) -> bool {
        let mut best: (i32, Option<&Player>) = (0, None);
        for player in &self.players {
            let score = rank(
                player.playback_status().await.ok(),
                player.metadata().await.is_ok(),
            );
            if score > best.0 {
                best = (score, Some(player));
            }
        }
        let new = best.1.cloned();
        let changed = new.as_ref().map(|p| p.inner().destination().to_owned())
            != self
                .active
                .as_ref()
                .map(|p| p.inner().destination().to_owned());
        self.active = new;
        changed
    }
}

async fn describe(player: &Player) -> Option<NowPlaying> {
    let metadata = player.metadata().await.ok()?;
    let (status, can_play, can_pause, can_go_previous, can_go_next) = tokio::join!(
        player.playback_status(),
        player.can_play(),
        player.can_pause(),
        player.can_go_previous(),
        player.can_go_next(),
    );
    Some(NowPlaying {
        player: player.clone(),
        title: metadata.title().unwrap_or_default(),
        artist: metadata.artists().unwrap_or_default().join(", "),
        playing: matches!(status, Ok(PlaybackStatus::Playing)),
        can_play_pause: can_play.unwrap_or(false) || can_pause.unwrap_or(false),
        can_go_previous: can_go_previous.unwrap_or(false),
        can_go_next: can_go_next.unwrap_or(false),
    })
}

async fn run(output: &mut mpsc::Sender<Event>) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let enumerator = enumerator::Enumerator::new(&conn).await?;
    let mut changes = enumerator.receive_changes().await?;
    let mut state = State {
        conn,
        players: Vec::new(),
        active: None,
    };
    for name in enumerator.players().await? {
        state.add(name).await;
    }
    state.pick_active().await;

    loop {
        match &state.active {
            Some(player) => match describe(player).await {
                Some(now) => {
                    if output.send(Event::Player(Box::new(now))).await.is_err() {
                        return Ok(());
                    }
                }
                None => {
                    let _ = output.send(Event::NoPlayer).await;
                }
            },
            None => {
                let _ = output.send(Event::NoPlayer).await;
            }
        }

        // Signals: metadata/capabilities of the active player, and playback
        // status of every player (a different one may start playing).
        let mut active_changes = match &state.active {
            Some(p) => Some(futures::stream::select_all([
                p.receive_metadata_changed().await.map(|_| ()).boxed(),
                p.receive_can_go_next_changed().await.map(|_| ()).boxed(),
                p.receive_can_go_previous_changed()
                    .await
                    .map(|_| ())
                    .boxed(),
            ])),
            None => None,
        };
        let mut statuses = Vec::with_capacity(state.players.len());
        for p in &state.players {
            statuses.push(
                p.receive_playback_status_changed()
                    .await
                    .map(|_| ())
                    .boxed(),
            );
        }
        let mut any_status = futures::stream::select_all(statuses);

        tokio::select! {
            _ = OptionFuture::from(active_changes.as_mut().map(|s| s.next())), if active_changes.is_some() => {}
            _ = any_status.next(), if !state.players.is_empty() => {
                state.pick_active().await;
            }
            change = changes.next() => {
                match change {
                    Some(Ok(enumerator::Event::Add(name))) => state.add(name).await,
                    Some(Ok(enumerator::Event::Remove(name))) => {
                        state.players.retain(|p| p.inner().destination() != &name);
                    }
                    Some(Err(error)) => return Err(error),
                    None => return Ok(()),
                }
                state.pick_active().await;
            }
        }
        // Coalesce bursts (track changes emit several signals at once).
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_players_outrank_paused_and_stopped() {
        assert!(
            rank(Some(PlaybackStatus::Playing), false) > rank(Some(PlaybackStatus::Paused), true)
        );
        assert!(
            rank(Some(PlaybackStatus::Paused), false) > rank(Some(PlaybackStatus::Stopped), true)
        );
        assert_eq!(rank(None, true), 0);
        assert!(
            rank(Some(PlaybackStatus::Playing), true) > rank(Some(PlaybackStatus::Playing), false)
        );
    }
}
