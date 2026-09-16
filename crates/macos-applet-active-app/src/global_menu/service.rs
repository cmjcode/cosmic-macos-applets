// SPDX-License-Identifier: GPL-3.0-only
//! Global menu runtime: hosts the registrar, follows focus, loads and
//! forwards menu interactions. Runs entirely off the UI thread.
//!
//! Every call into another application is bounded by [`CALL_TIMEOUT`]; a hung
//! app must never freeze the panel.

use std::{collections::HashMap, future::Future, path::Path, pin::Pin, sync::Arc, time::Duration};

use cosmic::iced::{
    Subscription,
    futures::{SinkExt, Stream, StreamExt, channel::mpsc, future::OptionFuture, stream::SelectAll},
    stream as iced_stream,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection,
    fdo::{DBusProxy, RequestNameFlags},
    names::BusName,
    zvariant::{OwnedObjectPath, Value},
};

use super::{
    dbusmenu::DBusMenuProxy,
    matcher::{self, Candidate, Focus},
    model::{self, Menu},
    procinfo::{self, ProcInfo},
    registrar::{BUS_NAME, OBJECT_PATH, Registrar, RegistrarClientProxy},
    x11,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(2);
/// Coalesce bursts of registrar or layout signals.
const DEBOUNCE: Duration = Duration::from_millis(80);
/// At most this many matching bus connections are introspected per lookup.
const MAX_DISCOVERY_CANDIDATES: usize = 8;

#[derive(Debug, Clone)]
pub enum Request {
    /// The focused application changed.
    Focus(Focus),
    /// A (sub)menu is about to be shown.
    Open(i32),
    /// A menu entry was clicked.
    Activate(i32),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(UnboundedSender<Request>),
    /// Menu of the focused app, or `None` if it exports none.
    Menu(Option<Arc<Menu>>),
}

type Signal = Pin<Box<dyn Stream<Item = ()> + Send>>;

async fn bounded<T, E: std::fmt::Display>(
    what: &str,
    call: impl Future<Output = Result<T, E>>,
) -> Option<T> {
    match tokio::time::timeout(CALL_TIMEOUT, call).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            tracing::debug!(%error, "{what} failed");
            None
        }
        Err(_) => {
            tracing::warn!("{what} timed out");
            None
        }
    }
}

/// The subscription used by the applet.
pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("macos-global-menu", |_| {
        iced_stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                let buses = async {
                    Ok::<_, zbus::Error>((
                        Connection::session().await?,
                        Connection::session().await?,
                    ))
                };
                match buses.await {
                    Ok((client, server)) => {
                        if let Err(error) = run(client, Some(server), &mut output).await {
                            tracing::warn!(%error, "global menu stopped, restarting in {backoff:?}");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "session bus unavailable"),
                }
                let _ = output.send(Event::Menu(None)).await;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        })
    })
}

/// Expose the registrar on `server`. The name is queued, not forced: an
/// existing registrar (another panel instance, KDE's) keeps serving, and this
/// one takes over automatically if it goes away.
async fn host_registrar(server: &Connection) -> zbus::Result<Registrar> {
    let registrar = Registrar::default();
    server
        .object_server()
        .at(OBJECT_PATH, registrar.clone())
        .await?;
    let reply = server
        .request_name_with_flags(BUS_NAME, RequestNameFlags::AllowReplacement.into())
        .await?;
    tracing::info!(?reply, "requested {BUS_NAME}");
    Ok(registrar)
}

struct Engine {
    client: Connection,
    dbus: DBusProxy<'static>,
    registrar: RegistrarClientProxy<'static>,
    registrations: Vec<(u32, String, OwnedObjectPath)>,
    seq: HashMap<(u32, String), u64>,
    next_seq: u64,
    procs: HashMap<String, ProcInfo>,
    /// Identity of every unique bus name seen during discovery.
    bus_procs: HashMap<String, ProcInfo>,
    /// Our own connections: never introspect them (a call to oneself would
    /// only wait for the timeout).
    own_names: Vec<String>,
    focus: Focus,
    attached: Option<(String, OwnedObjectPath)>,
    proxy: Option<DBusMenuProxy<'static>>,
    menu: Option<Arc<Menu>>,
}

impl Engine {
    async fn refresh_registrations(&mut self) {
        let list = bounded("GetMenus", self.registrar.get_menus())
            .await
            .unwrap_or_default();
        // Registrars do not expose order; remember first sighting as a proxy.
        for (window, service, _) in &list {
            let key = (*window, service.clone());
            if !self.seq.contains_key(&key) {
                self.next_seq += 1;
                self.seq.insert(key, self.next_seq);
            }
        }
        self.seq
            .retain(|(w, s), _| list.iter().any(|(lw, ls, _)| lw == w && ls == s));
        self.procs
            .retain(|service, _| list.iter().any(|(_, s, _)| s == service));
        for (_, service, _) in &list {
            if self.procs.contains_key(service) {
                continue;
            }
            let Ok(name) = BusName::try_from(service.as_str()) else {
                continue;
            };
            if let Some(pid) = bounded(
                "GetConnectionUnixProcessID",
                self.dbus.get_connection_unix_process_id(name),
            )
            .await
            {
                self.procs
                    .insert(service.clone(), ProcInfo::read(Path::new("/proc"), pid));
            }
        }
        tracing::debug!(registrations = ?list, procs = ?self.procs, "registrations refreshed");
        self.registrations = list;
    }

    async fn select(&mut self) -> Option<(String, OwnedObjectPath)> {
        if self.focus.app_id.is_empty() {
            return None;
        }
        if !self.registrations.is_empty() {
            let active_x11 = x11::active_window().await;
            tracing::debug!(focus = ?self.focus, ?active_x11, "selecting registered menu");
            let candidates: Vec<Candidate<'_>> = self
                .registrations
                .iter()
                .map(|(window_id, service, path)| Candidate {
                    window_id: *window_id,
                    service,
                    path: path.as_str(),
                    seq: self
                        .seq
                        .get(&(*window_id, service.clone()))
                        .copied()
                        .unwrap_or(0),
                    proc: self.procs.get(service),
                })
                .collect();
            if let Some(c) = matcher::select(&self.focus, active_x11.as_ref(), &candidates) {
                return Some((
                    c.service.to_owned(),
                    OwnedObjectPath::try_from(c.path).expect("registrar only stores valid paths"),
                ));
            }
        }
        self.discover().await
    }

    /// Find a menu bar the focused app exports without registering it.
    ///
    /// Qt on Wayland announces menus through the KDE-only
    /// `org_kde_kwin_appmenu` protocol instead of the registrar, but still
    /// exports `/MenuBar/<n>` objects on its bus connection. Look those up on
    /// connections belonging to the focused app's process.
    async fn discover(&mut self) -> Option<(String, OwnedObjectPath)> {
        let names = bounded("ListNames", self.dbus.list_names()).await?;
        let unique: Vec<String> = names
            .iter()
            .map(ToString::to_string)
            .filter(|n| n.starts_with(':'))
            .collect();
        self.bus_procs.retain(|name, _| unique.contains(name));

        let unknown: Vec<&String> = unique
            .iter()
            .filter(|n| !self.bus_procs.contains_key(*n))
            .collect();
        let lookups = unknown.iter().map(|name| async {
            let pid = match BusName::try_from(name.as_str()) {
                Ok(bus_name) => {
                    bounded(
                        "GetConnectionUnixProcessID",
                        self.dbus.get_connection_unix_process_id(bus_name),
                    )
                    .await
                }
                Err(_) => None,
            };
            // Cache failures too, so the bus daemon itself is not asked again.
            let info = pid.map_or_else(ProcInfo::default, |pid| {
                ProcInfo::read(Path::new("/proc"), pid)
            });
            ((*name).clone(), info)
        });
        let found = cosmic::iced::futures::future::join_all(lookups).await;
        self.bus_procs.extend(found);

        let mut matching: Vec<&String> = self
            .bus_procs
            .iter()
            .filter(|(name, info)| {
                info.pid != 0
                    && !self.own_names.contains(name)
                    && matcher::identity_matches(info, &self.focus)
            })
            .map(|(name, _)| name)
            .collect();
        matching.sort_unstable();
        matching.truncate(MAX_DISCOVERY_CANDIDATES);

        for service in matching {
            let introspect = async {
                zbus::fdo::IntrospectableProxy::builder(&self.client)
                    .destination(service.as_str())?
                    .path("/MenuBar")?
                    .cache_properties(zbus::proxy::CacheProperties::No)
                    .build()
                    .await?
                    .introspect()
                    .await
                    .map_err(zbus::Error::from)
            };
            let Some(xml) = bounded("Introspect /MenuBar", introspect).await else {
                continue;
            };
            let menubars = matcher::menubar_children(&xml);
            if let Some(id) =
                matcher::pick_menubar(&menubars, self.focus.window_index, self.focus.window_count)
            {
                tracing::debug!(%service, ?menubars, id, "discovered unregistered menu bar");
                let path = OwnedObjectPath::try_from(format!("/MenuBar/{id}")).ok()?;
                return Some((service.clone(), path));
            }
        }
        None
    }

    /// Re-evaluate which menu to show; returns new layout signals if attached
    /// to a different exporter.
    async fn reselect(
        &mut self,
        output: &mut mpsc::Sender<Event>,
    ) -> Option<Option<SelectAll<Signal>>> {
        let target = self.select().await;
        if target == self.attached {
            return None;
        }
        self.attached = target.clone();
        self.proxy = None;
        let Some((service, path)) = target else {
            self.publish(None, output).await;
            return Some(None);
        };
        let proxy = async {
            DBusMenuProxy::builder(&self.client)
                .destination(service.clone())?
                .path(path.clone())?
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
        };
        let Some(proxy) = bounded("dbusmenu proxy", proxy).await else {
            self.publish(None, output).await;
            return Some(None);
        };
        let mut signals: SelectAll<Signal> = SelectAll::new();
        if let Some(s) = bounded("LayoutUpdated", proxy.receive_layout_updated()).await {
            signals.push(Box::pin(s.map(|_| ())));
        }
        if let Some(s) = bounded(
            "ItemsPropertiesUpdated",
            proxy.receive_items_properties_updated(),
        )
        .await
        {
            signals.push(Box::pin(s.map(|_| ())));
        }
        // Some exporters only populate the root after AboutToShow.
        let _ = bounded("AboutToShow(0)", proxy.about_to_show(0)).await;
        self.proxy = Some(proxy);
        self.reload(output).await;
        Some(Some(signals))
    }

    async fn reload(&mut self, output: &mut mpsc::Sender<Event>) {
        let Some(proxy) = &self.proxy else { return };
        let menu = bounded("GetLayout", proxy.get_layout(0, -1, &[]))
            .await
            .map(|(_, root)| model::parse(&root))
            .filter(|m| !m.is_empty())
            .map(Arc::new);
        self.publish(menu, output).await;
    }

    async fn publish(&mut self, menu: Option<Arc<Menu>>, output: &mut mpsc::Sender<Event>) {
        if self.menu.as_deref() != menu.as_deref() {
            self.menu = menu.clone();
            let _ = output.send(Event::Menu(menu)).await;
        }
    }
}

async fn drain(signals: &mut (impl Stream<Item = ()> + Unpin)) {
    tokio::time::sleep(DEBOUNCE).await;
    while let Some(Some(())) = cosmic::iced::futures::FutureExt::now_or_never(signals.next()) {}
}

/// Run the global menu on the given connections until an unrecoverable
/// bus error. `server` hosts the registrar when provided.
pub async fn run(
    client: Connection,
    server: Option<Connection>,
    output: &mut mpsc::Sender<Event>,
) -> zbus::Result<()> {
    let (tx, rx) = unbounded_channel();
    let _ = output.send(Event::Ready(tx)).await;
    run_with_requests(client, server, rx, output).await
}

async fn run_with_requests(
    client: Connection,
    server: Option<Connection>,
    mut requests: UnboundedReceiver<Request>,
    output: &mut mpsc::Sender<Event>,
) -> zbus::Result<()> {
    // Registrar hosting is best-effort; the client side works with any registrar.
    let hosted = match &server {
        Some(server) => match host_registrar(server).await {
            Ok(registrar) => Some((registrar, server)),
            Err(error) => {
                tracing::warn!(%error, "cannot host the registrar");
                None
            }
        },
        None => None,
    };
    // Clients leaving the bus: drop their registrations.
    let mut vanished = match &hosted {
        Some((_, server)) => Some(
            DBusProxy::new(server)
                .await?
                .receive_name_owner_changed()
                .await?,
        ),
        None => None,
    };

    let dbus = DBusProxy::new(&client).await?;
    let registrar = RegistrarClientProxy::new(&client).await?;
    let mut registrar_signals: SelectAll<Signal> = SelectAll::new();
    registrar_signals.push(Box::pin(
        registrar.receive_window_registered().await?.map(|_| ()),
    ));
    registrar_signals.push(Box::pin(
        registrar.receive_window_unregistered().await?.map(|_| ()),
    ));
    registrar_signals.push(Box::pin(
        dbus.receive_name_owner_changed_with_args(&[(0, BUS_NAME)])
            .await?
            .map(|_| ()),
    ));

    let own_names = [Some(&client), server.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|conn| conn.unique_name().map(ToString::to_string))
        .collect();
    let mut engine = Engine {
        own_names,
        client,
        dbus,
        registrar,
        registrations: Vec::new(),
        seq: HashMap::new(),
        next_seq: 0,
        procs: HashMap::new(),
        bus_procs: HashMap::new(),
        focus: Focus::default(),
        attached: None,
        proxy: None,
        menu: None,
    };
    engine.refresh_registrations().await;
    // Any client leaving the bus: forget it, and drop its menu if shown.
    let mut owner_changes = engine.dbus.receive_name_owner_changed().await?;
    let mut menu_signals: Option<SelectAll<Signal>> = None;
    let mut reselect = true;

    loop {
        if reselect {
            reselect = false;
            if let Some(signals) = engine.reselect(output).await {
                menu_signals = signals;
            }
        }

        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return Ok(()) };
                match request {
                    Request::Focus(mut focus) => {
                        focus.program_path = focus.program.as_deref().and_then(|program| {
                            procinfo::resolve_program(program, std::env::var_os("PATH").as_deref())
                        });
                        if focus != engine.focus {
                            engine.focus = focus;
                            reselect = true;
                        }
                    }
                    Request::Open(id) => {
                        if let Some(proxy) = &engine.proxy {
                            let needs_update = bounded("AboutToShow", proxy.about_to_show(id)).await;
                            let _ = bounded("Event(opened)", proxy.event(id, "opened", &Value::I32(0), 0)).await;
                            if needs_update == Some(true) {
                                engine.reload(output).await;
                            }
                        }
                    }
                    Request::Activate(id) => {
                        if let Some(proxy) = &engine.proxy {
                            let _ = bounded("Event(clicked)", proxy.event(id, "clicked", &Value::I32(0), 0)).await;
                        }
                    }
                }
            }
            Some(()) = registrar_signals.next() => {
                drain(&mut registrar_signals).await;
                engine.refresh_registrations().await;
                // Re-selection detaches if the exporter we show went away.
                reselect = true;
            }
            Some(Some(())) = OptionFuture::from(menu_signals.as_mut().map(StreamExt::next)) => {
                if let Some(signals) = menu_signals.as_mut() {
                    drain(signals).await;
                }
                engine.reload(output).await;
            }
            Some(change) = owner_changes.next() => {
                if let Ok(args) = change.args()
                    && args.new_owner().is_none()
                {
                    let name = args.name().to_string();
                    engine.bus_procs.remove(&name);
                    if engine.attached.as_ref().is_some_and(|(service, _)| *service == name) {
                        reselect = true;
                    }
                }
            }
            Some(Some(change)) = OptionFuture::from(vanished.as_mut().map(StreamExt::next)) => {
                tracing::debug!(args = ?change.args().ok(), "NameOwnerChanged");
                if let (Some((registrar, server)), Ok(args)) = (&hosted, change.args())
                    && args.new_owner().is_none()
                    && args.name().starts_with(':')
                {
                    let emitter = zbus::object_server::SignalEmitter::new(server, OBJECT_PATH)?;
                    registrar.client_vanished(&emitter, args.name()).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod bus_tests {
    //! End-to-end tests on a private `dbus-daemon`: registrar, fake exporter,
    //! focus matching, layout loading and click forwarding.

    use super::*;
    use crate::global_menu::{dbusmenu::LayoutItem, model::tests::sample};
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::Mutex,
    };

    struct Bus {
        child: Child,
        address: String,
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn private_bus() -> Option<Bus> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut line)
            .ok()?;
        Some(Bus {
            child,
            address: line.trim().to_owned(),
        })
    }

    async fn connect(bus: &Bus) -> Connection {
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }

    #[derive(Clone, Default)]
    struct FakeMenu {
        events: Arc<Mutex<Vec<(i32, String)>>>,
    }

    #[zbus::interface(name = "com.canonical.dbusmenu")]
    impl FakeMenu {
        async fn get_layout(
            &self,
            _parent: i32,
            _depth: i32,
            _props: Vec<String>,
        ) -> (u32, LayoutItem) {
            (1, sample())
        }
        async fn event(
            &self,
            id: i32,
            event_id: String,
            _data: zbus::zvariant::OwnedValue,
            _ts: u32,
        ) {
            self.events.lock().unwrap().push((id, event_id));
        }
        async fn about_to_show(&self, _id: i32) -> bool {
            false
        }
    }

    #[zbus::proxy(
        interface = "com.canonical.AppMenu.Registrar",
        default_service = "com.canonical.AppMenu.Registrar",
        default_path = "/com/canonical/AppMenu/Registrar",
        gen_blocking = false
    )]
    trait Register {
        fn register_window(
            &self,
            window_id: u32,
            menu_object_path: OwnedObjectPath,
        ) -> zbus::Result<()>;
    }

    async fn next_menu(events: &mut mpsc::Receiver<Event>) -> Option<Arc<Menu>> {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), events.next())
                .await
                .expect("timed out waiting for a menu event")
                .expect("service ended");
            if let Event::Menu(menu) = event {
                return menu;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registers_matches_loads_and_clicks() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(std::env::var("TEST_LOG").unwrap_or_else(|_| "warn".into()))
            .with_test_writer()
            .try_init();
        let Some(bus) = private_bus() else {
            eprintln!("dbus-daemon not available; skipping");
            return;
        };
        let (client, server, app) = (
            connect(&bus).await,
            connect(&bus).await,
            connect(&bus).await,
        );

        let (mut out_tx, mut out_rx) = mpsc::channel(16);
        let (req_tx, req_rx) = unbounded_channel();
        tokio::spawn(async move {
            let _ = run_with_requests(client, Some(server), req_rx, &mut out_tx).await;
        });

        // The fake app lives in this very process, so match on our own exe name.
        let me = ProcInfo::read(Path::new("/proc"), std::process::id());
        let focus = Focus {
            app_id: "org.example.Fake".into(),
            program: me.exe.clone(),
            ..Focus::default()
        };

        let fake = FakeMenu::default();
        app.object_server()
            .at("/MenuBar/1", fake.clone())
            .await
            .unwrap();
        // Wait until our registrar owns the name.
        let dbus = DBusProxy::new(&app).await.unwrap();
        for _ in 0..50 {
            if dbus
                .name_has_owner(BUS_NAME.try_into().unwrap())
                .await
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        RegisterProxy::new(&app)
            .await
            .unwrap()
            .register_window(42, "/MenuBar/1".try_into().unwrap())
            .await
            .unwrap();

        req_tx.send(Request::Focus(focus)).unwrap();
        let menu = next_menu(&mut out_rx).await.expect("menu for focused app");
        let titles: Vec<_> = menu.titles.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(titles, ["File", "Help"]);

        req_tx.send(Request::Open(1)).unwrap();
        req_tx.send(Request::Activate(11)).unwrap();
        let mut seen = Vec::new();
        for _ in 0..100 {
            seen = fake.events.lock().unwrap().clone();
            if seen.iter().any(|(_, e)| e == "clicked") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(seen.contains(&(1, "opened".into())), "{seen:?}");
        assert!(seen.contains(&(11, "clicked".into())), "{seen:?}");

        // Focus another app: menu goes away.
        req_tx
            .send(Request::Focus(Focus {
                app_id: "firefox".into(),
                ..Focus::default()
            }))
            .unwrap();
        assert!(next_menu(&mut out_rx).await.is_none());

        // Back to the app, then the app quits: its registration is dropped.
        req_tx
            .send(Request::Focus(Focus {
                app_id: "org.example.Fake".into(),
                program: me.exe,
                ..Focus::default()
            }))
            .unwrap();
        assert!(next_menu(&mut out_rx).await.is_some());
        // Every handle must go for the connection to actually close.
        drop(dbus);
        app.close().await.unwrap();
        assert!(next_menu(&mut out_rx).await.is_none(), "exporter vanished");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn discovers_unregistered_menubars_of_the_focused_window() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(std::env::var("TEST_LOG").unwrap_or_else(|_| "warn".into()))
            .with_test_writer()
            .try_init();
        let Some(bus) = private_bus() else {
            eprintln!("dbus-daemon not available; skipping");
            return;
        };
        let (client, server, app) = (
            connect(&bus).await,
            connect(&bus).await,
            connect(&bus).await,
        );
        let (mut out_tx, mut out_rx) = mpsc::channel(16);
        let (req_tx, req_rx) = unbounded_channel();
        tokio::spawn(async move {
            let _ = run_with_requests(client, Some(server), req_rx, &mut out_tx).await;
        });

        // Like Qt on Wayland: one /MenuBar/<n> per window, never registered.
        let (first, second) = (FakeMenu::default(), FakeMenu::default());
        app.object_server()
            .at("/MenuBar/1", first.clone())
            .await
            .unwrap();
        app.object_server()
            .at("/MenuBar/2", second.clone())
            .await
            .unwrap();

        let me = ProcInfo::read(Path::new("/proc"), std::process::id());
        let focus = |window_index| Focus {
            app_id: "org.example.Office".into(),
            program: me.exe.clone(),
            window_index,
            window_count: 2,
            ..Focus::default()
        };

        req_tx.send(Request::Focus(focus(1))).unwrap();
        assert!(
            next_menu(&mut out_rx).await.is_some(),
            "menu discovered without registration"
        );
        req_tx.send(Request::Activate(11)).unwrap();
        for _ in 0..100 {
            if !second.events.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            second.events.lock().unwrap().as_slice(),
            [(11, "clicked".to_owned())]
        );
        assert!(
            first.events.lock().unwrap().is_empty(),
            "the other window's menu is untouched"
        );

        // The exporting process quits: the menu disappears.
        app.close().await.unwrap();
        assert!(next_menu(&mut out_rx).await.is_none());
    }
}
