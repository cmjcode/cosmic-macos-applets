// SPDX-License-Identifier: GPL-3.0-only
//! Background thread that tracks toplevel windows through
//! `ext-foreign-toplevel-list` + `cosmic-toplevel-info` and forwards compact
//! snapshots to the UI. Only changed snapshots are sent, so title churn in
//! other windows does not wake the applet needlessly.

use std::{
    os::{
        fd::{FromRawFd, RawFd},
        unix::net::UnixStream,
    },
    sync::Arc,
};

use cosmic::{
    cctk::{
        cosmic_protocols::{
            toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State,
            toplevel_management::v1::client::zcosmic_toplevel_manager_v1,
        },
        sctk::{
            self,
            output::{OutputHandler, OutputState},
            reexports::{calloop, calloop_wayland_source::WaylandSource},
            registry::{ProvidesRegistryState, RegistryState},
        },
        toplevel_info::{ToplevelInfo, ToplevelInfoHandler, ToplevelInfoState},
        toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
        wayland_client::{
            Connection, Proxy, QueueHandle, WEnum, globals::registry_queue_init,
            protocol::wl_output,
        },
        wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    },
    iced::{
        Subscription,
        futures::{SinkExt, StreamExt, channel::mpsc},
        stream,
    },
};

use crate::model::{Toplevel, ToplevelId};

/// Messages from the Wayland thread to the applet.
#[derive(Debug, Clone)]
pub enum Event {
    /// The thread is running; requests can be sent through this channel.
    Ready(calloop::channel::Sender<Request>),
    /// Full list of current toplevels.
    Toplevels(Arc<[Toplevel]>),
    /// The compositor does not provide the required protocols, or the
    /// connection failed. The applet keeps running with an empty label.
    Unavailable(String),
}

/// Requests from the applet to the Wayland thread.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    Close(ToplevelId),
    Minimize(ToplevelId),
}

/// Subscription that owns the Wayland thread for the lifetime of the applet.
pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-active-app-wayland", |_| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let (event_tx, mut event_rx) = mpsc::unbounded();
            let (request_tx, request_rx) = calloop::channel::channel();

            let spawned = std::thread::Builder::new()
                .name("macos-active-app-wayland".into())
                .spawn(move || {
                    if let Err(error) = run(&event_tx, request_rx) {
                        let _ = event_tx.unbounded_send(Event::Unavailable(error));
                    }
                });

            match spawned {
                Ok(_) => {
                    let _ = output.send(Event::Ready(request_tx)).await;
                    while let Some(event) = event_rx.next().await {
                        if output.send(event).await.is_err() {
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = output
                        .send(Event::Unavailable(format!("cannot spawn thread: {error}")))
                        .await;
                }
            }
            // Keep the subscription alive without busy looping.
            std::future::pending::<()>().await;
        })
    })
}

struct State_ {
    registry_state: RegistryState,
    output_state: OutputState,
    toplevel_info_state: ToplevelInfoState,
    toplevel_manager_state: Option<ToplevelManagerState>,
    tx: mpsc::UnboundedSender<Event>,
    dirty: bool,
    last_sent: Option<Arc<[Toplevel]>>,
    exit: bool,
}

fn connect() -> Result<Connection, String> {
    // The panel hands privileged applets a socket that may use restricted
    // protocols such as toplevel info (desktop file: X-HostWaylandDisplay).
    if let Some(fd) = std::env::var("X_PRIVILEGED_WAYLAND_SOCKET")
        .ok()
        .and_then(|fd| fd.parse::<RawFd>().ok())
    {
        // SAFETY: the panel passes an open, inherited socket fd that nothing
        // else in this process owns; ownership is transferred exactly once.
        let stream = unsafe { UnixStream::from_raw_fd(fd) };
        return Connection::from_socket(stream).map_err(|e| format!("privileged socket: {e}"));
    }
    Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))
}

fn run(
    tx: &mpsc::UnboundedSender<Event>,
    requests: calloop::channel::Channel<Request>,
) -> Result<(), String> {
    let conn = connect()?;
    let (globals, event_queue) =
        registry_queue_init::<State_>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = event_queue.handle();

    let mut event_loop =
        calloop::EventLoop::<State_>::try_new().map_err(|e| format!("event loop: {e}"))?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .map_err(|e| format!("wayland source: {e}"))?;
    handle
        .insert_source(requests, |event, (), state| match event {
            calloop::channel::Event::Msg(request) => state.handle(request),
            calloop::channel::Event::Closed => state.exit = true,
        })
        .map_err(|e| format!("request source: {e}"))?;

    let registry_state = RegistryState::new(&globals);
    let toplevel_info_state = ToplevelInfoState::try_new(&registry_state, &qh)
        .ok_or_else(|| "compositor lacks ext-foreign-toplevel-list-v1".to_owned())?;
    let mut state = State_ {
        output_state: OutputState::new(&globals, &qh),
        toplevel_manager_state: ToplevelManagerState::try_new(&registry_state, &qh),
        toplevel_info_state,
        registry_state,
        tx: tx.clone(),
        dirty: true,
        last_sent: None,
        exit: false,
    };

    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| format!("dispatch: {e}"))?;
        state.flush();
    }
    Ok(())
}

impl State_ {
    fn snapshot(&self, info: &ToplevelInfo) -> Toplevel {
        let mut outputs: Vec<String> = info
            .output
            .iter()
            .filter_map(|o| self.output_state.info(o).and_then(|i| i.name))
            .collect();
        outputs.sort_unstable();
        Toplevel {
            id: info.foreign_toplevel.id().protocol_id(),
            app_id: info.app_id.clone(),
            title: info.title.clone(),
            activated: info.state.contains(&State::Activated),
            minimized: info.state.contains(&State::Minimized),
            outputs,
        }
    }

    /// Send a snapshot if something changed since the last one.
    fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let mut toplevels: Vec<Toplevel> = self
            .toplevel_info_state
            .toplevels()
            .map(|info| self.snapshot(info))
            .collect();
        toplevels.sort_unstable_by_key(|t| t.id);
        if self.last_sent.as_deref() == Some(toplevels.as_slice()) {
            return;
        }
        let snapshot: Arc<[Toplevel]> = toplevels.into();
        self.last_sent = Some(snapshot.clone());
        if self.tx.unbounded_send(Event::Toplevels(snapshot)).is_err() {
            // The applet is gone.
            self.exit = true;
        }
    }

    fn handle(&self, request: Request) {
        let Some(manager) = &self.toplevel_manager_state else {
            tracing::warn!("compositor lacks cosmic-toplevel-management; ignoring {request:?}");
            return;
        };
        let (Request::Close(id) | Request::Minimize(id)) = request;
        let cosmic_handle = self
            .toplevel_info_state
            .toplevels()
            .find(|info| info.foreign_toplevel.id().protocol_id() == id)
            .and_then(|info| info.cosmic_toplevel.as_ref());
        match (cosmic_handle, request) {
            (Some(handle), Request::Close(_)) => manager.manager.close(handle),
            (Some(handle), Request::Minimize(_)) => manager.manager.set_minimized(handle),
            (None, _) => tracing::debug!(id, "window vanished before {request:?}"),
        }
    }
}

impl ProvidesRegistryState for State_ {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    sctk::registry_handlers!(OutputState);
}

impl OutputHandler for State_ {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.dirty = true;
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.dirty = true;
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.dirty = true;
    }
}

impl ToplevelInfoHandler for State_ {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }
    fn new_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.dirty = true;
    }
    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.dirty = true;
    }
    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.dirty = true;
    }
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>) {
        let _ = self.tx.unbounded_send(Event::Unavailable(
            "toplevel list finished by compositor".into(),
        ));
        self.exit = true;
    }
}

impl ToplevelManagerHandler for State_ {
    fn toplevel_manager_state(&mut self) -> &mut ToplevelManagerState {
        self.toplevel_manager_state
            .as_mut()
            .expect("manager events only arrive when the global was bound")
    }
    fn capabilities(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: Vec<WEnum<zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1>>,
    ) {
    }
}

sctk::delegate_output!(State_);
sctk::delegate_registry!(State_);
cosmic::cctk::delegate_toplevel_info!(State_);
cosmic::cctk::delegate_toplevel_manager!(State_);
