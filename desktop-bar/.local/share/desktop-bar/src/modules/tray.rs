//! SNI watcher/host and dbusmenu client. All blocking bus work stays off the UI thread.
use crate::theme::Color;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedValue, Value},
};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
type MenuNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[derive(Clone, Debug)]
pub struct Item {
    pub address: String,
    pub bus: String,
    pub path: String,
    pub owner: String,
    pub title: String,
    pub icon: Option<Arc<tiny_skia::Pixmap>>,
    pub symbolic: bool,
    pub menu: Option<String>,
    pub attention: bool,
    pub passive: bool,
    pub is_menu: bool,
}
#[derive(Clone, Debug, Default)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: bool,
    pub toggle: Option<bool>,
}
#[derive(Clone, Debug)]
pub struct MenuReply {
    pub address: String,
    pub parent: i32,
    pub entries: Vec<MenuEntry>,
}
pub enum Action {
    Foreground(Color),
    Activate(String, i32, i32),
    Secondary(String, i32, i32),
    Context(String, i32, i32),
    Scroll(String, i32),
    Menu(String, i32),
    CloseMenu,
    Click(String, i32, u32),
}
enum Event {
    Register(String),
    Signal(zbus::Message),
    Action(Action),
    Disconnected,
}
#[derive(Clone)]
pub struct Client {
    tx: mpsc::Sender<Event>,
    pub menus: Arc<Mutex<Vec<MenuReply>>>,
}
impl Client {
    pub fn new(foreground: Color, send: impl Fn(Vec<Item>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let menus = Arc::new(Mutex::new(Vec::new()));
        let client = Self {
            tx: tx.clone(),
            menus: menus.clone(),
        };
        std::thread::Builder::new()
            .name("bar-tray".into())
            .spawn(move || {
                let mut foreground = foreground;
                loop {
                    if let Err(error) = run(tx.clone(), &rx, menus.clone(), &mut foreground, &send)
                    {
                        eprintln!("desktop-bar: tray unavailable: {error}");
                    }
                    menus.lock().unwrap().clear();
                    send(vec![]);
                    let retry = std::time::Instant::now() + Duration::from_secs(5);
                    loop {
                        let remaining = retry.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(remaining) {
                            Ok(Event::Action(Action::Foreground(color))) => foreground = color,
                            Ok(_) => {}
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
            })
            .expect("tray worker");
        client
    }
    pub fn send(&self, action: Action) {
        let _ = self.tx.send(Event::Action(action));
    }
}

struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
    tx: mpsc::Sender<Event>,
}
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let address = if service.starts_with('/') {
            format!(
                "{}{}",
                header
                    .sender()
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Missing sender".into()))?,
                service
            )
        } else {
            format!("{service}/StatusNotifierItem")
        };
        {
            let mut items = self.items.lock().unwrap();
            if items.contains(&address) {
                // A repeated registration retries an item whose first read failed.
                let _ = self.tx.send(Event::Register(address));
                return Ok(());
            }
            if items.len() >= 64 {
                return Err(zbus::fdo::Error::LimitsExceeded(
                    "Too many tray items".into(),
                ));
            }
            items.push(address.clone());
        }
        let _ = self.tx.send(Event::Register(address.clone()));
        Self::status_notifier_item_registered(&emitter, &address).await?;
        Ok(())
    }
    async fn register_status_notifier_host(
        &self,
        _service: &str,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        Self::status_notifier_host_registered(&emitter).await?;
        Ok(())
    }
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

fn run(
    tx: mpsc::Sender<Event>,
    rx: &mpsc::Receiver<Event>,
    menus: Arc<Mutex<Vec<MenuReply>>>,
    foreground: &mut Color,
    send: impl Fn(Vec<Item>),
) -> zbus::Result<()> {
    let registered = Arc::new(Mutex::new(Vec::new()));
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(Duration::from_secs(2))
        .serve_at(
            WATCHER_PATH,
            Watcher {
                items: registered.clone(),
                tx: tx.clone(),
            },
        )?
        .build()?;
    let own_watcher = connection.request_name(WATCHER).is_ok();
    let host = format!(
        "org.kde.StatusNotifierHost.desktop_bar_{}",
        std::process::id()
    );
    connection.request_name(host.as_str())?;
    let watcher = Proxy::new(&connection, WATCHER, WATCHER_PATH, WATCHER)?;
    let _: Result<(), _> = watcher.call("RegisterStatusNotifierHost", &(host.as_str(),));
    let list: Vec<String> = watcher
        .get_property("RegisteredStatusNotifierItems")
        .unwrap_or_default();
    for address in list {
        let _ = tx.send(Event::Register(address));
    }
    let mut signals = crate::bus::Signals::new(
        &connection,
        &[
            "type='signal',interface='org.kde.StatusNotifierItem'",
            "type='signal',interface='org.kde.StatusNotifierWatcher'",
            "type='signal',interface='com.canonical.dbusmenu'",
            "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.kde.StatusNotifierItem'",
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'",
        ],
    )?;
    let events = tx.clone();
    std::thread::Builder::new()
        .name("bar-tray-signals".into())
        .spawn(move || {
            for message in &mut signals {
                let Ok(message) = message else {
                    break;
                };
                let h = message.header();
                if h.interface().is_some_and(|i| {
                    [
                        ITEM,
                        WATCHER,
                        MENU,
                        "org.freedesktop.DBus.Properties",
                        "org.freedesktop.DBus",
                    ]
                    .contains(&i.as_str())
                }) && events.send(Event::Signal(message)).is_err()
                {
                    break;
                }
            }
            let _ = events.send(Event::Disconnected);
        })
        .expect("tray signals");
    let mut items: Vec<Item> = vec![];
    let mut icon_keys: HashMap<String, u64> = HashMap::new();
    let mut open_menu: Option<(String, i32)> = None;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Disconnected => {
                return Err(zbus::Error::Failure("Session bus disconnected".into()));
            }
            Event::Register(address) => {
                if items.len() < 64
                    && !items.iter().any(|i| i.address == address)
                    && let Some((item, key)) = read_item(&connection, &address, *foreground, None)
                {
                    icon_keys.insert(address, key);
                    items.push(item);
                    send(items.clone());
                }
            }
            Event::Signal(message) => {
                let header = message.header();
                let member = header.member().map(|v| v.as_str()).unwrap_or("");
                if header.interface().is_some_and(|i| i.as_str() == WATCHER)
                    && member == "StatusNotifierItemRegistered"
                {
                    if let Ok(address) = message.body().deserialize::<String>() {
                        let _ = tx.send(Event::Register(address));
                    }
                } else if header
                    .interface()
                    .is_some_and(|i| i.as_str() == "org.freedesktop.DBus")
                    && member == "NameOwnerChanged"
                {
                    if let Ok((name, _, new)) =
                        message.body().deserialize::<(String, String, String)>()
                    {
                        if new.is_empty() {
                            let removed: Vec<_> = items
                                .iter()
                                .filter(|i| i.bus == name || i.owner == name)
                                .map(|i| i.address.clone())
                                .collect();
                            items.retain(|i| !removed.contains(&i.address));
                            icon_keys.retain(|address, _| !removed.contains(address));
                            // Also forget registrations that were never read
                            // successfully, so they cannot fill the watcher.
                            registered.lock().unwrap().retain(|i| {
                                !removed.contains(i)
                                    && i.split_once('/').is_none_or(|(bus, _)| bus != name)
                            });
                            for address in &removed {
                                if own_watcher {
                                    let _ = connection.emit_signal(
                                        None::<&str>,
                                        WATCHER_PATH,
                                        WATCHER,
                                        "StatusNotifierItemUnregistered",
                                        &(address,),
                                    );
                                }
                            }
                            if !removed.is_empty() {
                                send(items.clone());
                            }
                        } else if name == WATCHER && !own_watcher {
                            // A restarted watcher needs this host registered again.
                            let _: Result<(), _> =
                                watcher.call("RegisterStatusNotifierHost", &(host.as_str(),));
                        }
                    }
                } else if header.interface().is_some_and(|i| i.as_str() == MENU) {
                    if let Some((address, parent)) = &open_menu
                        && let Some(item) = items.iter().find(|i| {
                            &i.address == address
                                && header.sender().is_some_and(|s| s.as_str() == i.owner)
                                && header
                                    .path()
                                    .is_some_and(|p| Some(p.as_str()) == i.menu.as_deref())
                        })
                    {
                        let entries = read_menu(&connection, item, *parent, false);
                        let mut replies = menus.lock().unwrap();
                        if replies.len() >= 8 {
                            replies.clear();
                        }
                        replies.push(MenuReply {
                            address: address.clone(),
                            parent: *parent,
                            entries,
                        });
                        send(items.clone());
                    }
                } else if header.interface().is_some_and(|i| {
                    [ITEM, "org.freedesktop.DBus.Properties"].contains(&i.as_str())
                }) && member != "NewToolTip"
                {
                    // Tooltips are not displayed; hover text comes from the title.
                    let sender = header.sender().map(|v| v.as_str()).unwrap_or("");
                    let mut changed = false;
                    for old in items.iter_mut().filter(|i| {
                        i.owner == sender && header.path().is_some_and(|p| p.as_str() == i.path)
                    }) {
                        let previous = icon_keys.get(&old.address).map(|key| (&*old, *key));
                        if let Some((new, key)) =
                            read_item(&connection, &old.address, *foreground, previous)
                        {
                            changed |= !same(old, &new);
                            icon_keys.insert(new.address.clone(), key);
                            *old = new;
                        }
                    }
                    if changed {
                        send(items.clone());
                    }
                }
            }
            Event::Action(action) => {
                let method = match &action {
                    Action::Secondary(..) => "SecondaryActivate",
                    Action::Context(..) => "ContextMenu",
                    _ => "Activate",
                };
                match action {
                    Action::Foreground(color) => {
                        if *foreground != color {
                            *foreground = color;
                            for item in &mut items {
                                if item.symbolic
                                    && let Some(icon) = &mut item.icon
                                {
                                    tint_symbolic(Arc::make_mut(icon), *foreground);
                                }
                            }
                            send(items.clone());
                        }
                    }
                    Action::Menu(address, parent) => {
                        open_menu = Some((address.clone(), parent));
                        if let Some(item) = items.iter().find(|i| i.address == address) {
                            let entries = read_menu(&connection, item, parent, true);
                            let mut replies = menus.lock().unwrap();
                            if replies.len() >= 8 {
                                replies.clear();
                            }
                            replies.push(MenuReply {
                                address,
                                parent,
                                entries,
                            });
                            send(items.clone());
                        }
                    }
                    Action::CloseMenu => open_menu = None,
                    Action::Click(address, id, time) => {
                        if let Some(item) = items.iter().find(|i| i.address == address)
                            && let Some(path) = &item.menu
                            && let Ok(proxy) =
                                Proxy::new(&connection, item.bus.as_str(), path.as_str(), MENU)
                        {
                            let _: Result<(), _> =
                                proxy.call("Event", &(id, "clicked", Value::from(0_i32), time));
                        }
                    }
                    Action::Activate(address, x, y)
                    | Action::Secondary(address, x, y)
                    | Action::Context(address, x, y) => {
                        if let Some(item) = items.iter().find(|i| i.address == address)
                            && let Ok(proxy) =
                                Proxy::new(&connection, item.bus.as_str(), item.path.as_str(), ITEM)
                        {
                            let _: Result<(), _> = proxy.call(method, &(x, y));
                        }
                    }
                    Action::Scroll(address, delta) => {
                        if let Some(item) = items.iter().find(|i| i.address == address)
                            && let Ok(proxy) =
                                Proxy::new(&connection, item.bus.as_str(), item.path.as_str(), ITEM)
                        {
                            let _: Result<(), _> = proxy.call("Scroll", &(delta, "vertical"));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Whether two snapshots of an item would display and behave identically.
fn same(a: &Item, b: &Item) -> bool {
    a.address == b.address
        && a.bus == b.bus
        && a.path == b.path
        && a.owner == b.owner
        && a.title == b.title
        && match (&a.icon, &b.icon) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
        && a.symbolic == b.symbolic
        && a.menu == b.menu
        && a.attention == b.attention
        && a.passive == b.passive
        && a.is_menu == b.is_menu
}

/// Reads an item. `previous` is the item's last snapshot and icon key, whose
/// icon and owner are reused when the signal cannot have changed them.
fn read_item(
    c: &Connection,
    address: &str,
    foreground: Color,
    previous: Option<(&Item, u64)>,
) -> Option<(Item, u64)> {
    let (bus, path) = address.split_once('/')?;
    let path = format!("/{path}");
    // Address Properties.GetAll directly to the advertised name. Electron can
    // reject requests addressed to its unique owner, leaving proxy caches empty.
    let reply = c
        .call_method(
            Some(bus),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(ITEM,),
        )
        .ok()?;
    let properties: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
    let string = |name: &str| {
        properties
            .get(name)
            .and_then(|v| v.downcast_ref::<&str>().ok())
            .unwrap_or("")
            .to_owned()
    };
    let status = string("Status");
    let attention = status == "NeedsAttention";
    let icon_name = string(if attention {
        "AttentionIconName"
    } else {
        "IconName"
    });
    let pixmaps = properties
        .get(if attention {
            "AttentionIconPixmap"
        } else {
            "IconPixmap"
        })
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<(i32, i32, Vec<u8>)>::try_from(v).ok());
    let theme_path = string("IconThemePath");
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (&icon_name, &theme_path, &pixmaps).hash(&mut hash);
        hash.finish()
    };
    let symbolic = Path::new(&icon_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("-symbolic"));
    let icon = match previous {
        // Reused icons were tinted with the current foreground already.
        Some((item, previous_key)) if previous_key == key && item.icon.is_some() => {
            item.icon.clone()
        }
        _ => {
            let mut icon = pixmaps
                .and_then(decode_pixmaps)
                .or_else(|| lookup_icon(&icon_name, &theme_path));
            if symbolic && let Some(icon) = &mut icon {
                tint_symbolic(icon, foreground);
            }
            icon.map(Arc::new)
        }
    };
    let menu = properties
        .get("Menu")
        .and_then(|v| v.downcast_ref::<&zbus::zvariant::ObjectPath<'_>>().ok())
        .map(|v| v.as_str().to_owned());
    // Item signals are matched by their sender, which is the current owner.
    let owner = match previous {
        Some((item, _)) => item.owner.clone(),
        None => Proxy::new(
            c,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .ok()?
        .call("GetNameOwner", &(bus,))
        .ok()?,
    };
    let item = Item {
        address: address.into(),
        bus: bus.into(),
        path,
        owner,
        title: string("Title"),
        icon,
        symbolic,
        menu,
        attention,
        passive: status == "Passive",
        is_menu: properties
            .get("ItemIsMenu")
            .and_then(|v| v.downcast_ref::<bool>().ok())
            .unwrap_or(false),
    };
    Some((item, key))
}

fn tint_symbolic(icon: &mut tiny_skia::Pixmap, foreground: Color) {
    for pixel in icon.data_mut().as_chunks_mut::<4>().0 {
        let alpha = f32::from(pixel[3]);
        for (channel, color) in
            pixel[..3]
                .iter_mut()
                .zip([foreground.0, foreground.1, foreground.2])
        {
            *channel = (color * alpha).round() as u8;
        }
    }
}

pub fn decode_pixmaps(mut images: Vec<(i32, i32, Vec<u8>)>) -> Option<tiny_skia::Pixmap> {
    images.retain(|(w, h, b)| {
        *w > 0 && *h > 0 && *w <= 256 && *h <= 256 && b.len() == (*w as usize) * (*h as usize) * 4
    });
    images.sort_by_key(|(w, _, _)| (*w - 24).abs());
    let (w, h, bytes) = images.into_iter().next()?;
    let mut p = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    for (out, argb) in p
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(bytes.as_chunks::<4>().0.iter())
    {
        let a = u16::from(argb[0]);
        for (dst, c) in out.iter_mut().zip([argb[1], argb[2], argb[3]]) {
            *dst = ((u16::from(c) * a + 127) / 255) as u8;
        }
        out[3] = argb[0];
    }
    Some(p)
}

fn load_icon(path: &Path) -> Option<tiny_skia::Pixmap> {
    if std::fs::metadata(path).ok()?.len() > 2 * 1024 * 1024 {
        return None;
    }
    if path.extension().is_some_and(|extension| extension == "svg") {
        let tree = resvg::usvg::Tree::from_data(
            &std::fs::read(path).ok()?,
            &resvg::usvg::Options::default(),
        )
        .ok()?;
        // Rasterize once on the tray worker, retaining only a small bitmap.
        // A square canvas preserves non-square icons' aspect ratios in the bar.
        let size = tree.size();
        let scale = 64.0 / size.width().max(size.height());
        let mut p = tiny_skia::Pixmap::new(64, 64)?;
        let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(
            (64.0 - size.width() * scale) / 2.0,
            (64.0 - size.height() * scale) / 2.0,
        );
        resvg::render(&tree, transform, &mut p.as_mut());
        return Some(p);
    }
    let p = tiny_skia::Pixmap::load_png(path).ok()?;
    (p.width() <= 512 && p.height() <= 512).then_some(p)
}

fn lookup_icon(name: &str, theme_path: &str) -> Option<tiny_skia::Pixmap> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::theme::home().join(".local/share"));
    let mut roots = vec![
        PathBuf::from(theme_path),
        crate::theme::home().join(".icons"),
        data_home.join("icons"),
        data_home.join("pixmaps"),
    ];
    roots.extend(
        std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/run/current-system/sw/share:/usr/local/share:/usr/share".into())
            .split(':')
            .filter(|path| !path.is_empty())
            .flat_map(|path| {
                [
                    PathBuf::from(path).join("icons"),
                    PathBuf::from(path).join("pixmaps"),
                ]
            }),
    );
    lookup_icon_in_roots(name, &roots)
}

fn lookup_icon_in_roots(name: &str, roots: &[PathBuf]) -> Option<tiny_skia::Pixmap> {
    if name.is_empty() {
        return None;
    }
    if Path::new(name).is_absolute() {
        return load_icon(Path::new(name));
    }
    if name.contains('/') || name.contains("..") {
        return None;
    }
    let names = if name.ends_with(".png") || name.ends_with(".svg") {
        vec![name.to_owned()]
    } else {
        vec![format!("{name}.png"), format!("{name}.svg")]
    };
    for root in roots {
        if root.as_os_str().is_empty() {
            continue;
        }
        for name in &names {
            if let Some(icon) = load_icon(&root.join(name)) {
                return Some(icon);
            }
        }
        for theme in ["", "Papirus", "hicolor", "Adwaita"] {
            for size in [
                "16x16", "22x22", "24x24", "32x32", "48x48", "64x64", "128x128", "256x256",
                "scalable", "symbolic",
            ] {
                for category in ["status", "apps", "devices", "actions"] {
                    for name in &names {
                        if let Some(icon) =
                            load_icon(&root.join(theme).join(size).join(category).join(name))
                        {
                            return Some(icon);
                        }
                    }
                }
            }
        }
    }
    None
}

fn read_menu(c: &Connection, item: &Item, parent: i32, showing: bool) -> Vec<MenuEntry> {
    let Some(path) = &item.menu else {
        return vec![];
    };
    let Ok(proxy) = Proxy::new(c, item.bus.as_str(), path.as_str(), MENU) else {
        return vec![];
    };
    // A layout signal can be emitted by AboutToShow itself. Refreshing an open
    // menu must not call it again and start a feedback loop.
    if showing {
        let _: Result<bool, _> = proxy.call("AboutToShow", &(parent,));
    }
    let Ok((_, (_, _, children))) =
        proxy.call::<_, _, (u32, MenuNode)>("GetLayout", &(parent, 1_i32, Vec::<&str>::new()))
    else {
        return vec![];
    };
    children
        .into_iter()
        .take(128)
        .filter_map(|value| {
            let structure = value
                .downcast_ref::<&zbus::zvariant::Structure<'_>>()
                .ok()?;
            let f = structure.fields();
            let id = f.first()?.downcast_ref::<i32>().ok()?;
            let properties = f
                .get(1)?
                .try_clone()
                .ok()?
                .downcast::<HashMap<String, OwnedValue>>()
                .ok()?;
            let text = |key: &str| {
                properties
                    .get(key)
                    .and_then(|v| v.downcast_ref::<&str>().ok())
                    .unwrap_or("")
                    .to_owned()
            };
            let flag = |key: &str, default| {
                properties
                    .get(key)
                    .and_then(|v| v.downcast_ref::<bool>().ok())
                    .unwrap_or(default)
            };
            if !flag("visible", true) {
                return None;
            }
            Some(MenuEntry {
                id,
                label: text("label").replace('_', ""),
                enabled: flag("enabled", true),
                separator: text("type") == "separator",
                submenu: text("children-display") == "submenu",
                toggle: properties
                    .get("toggle-state")
                    .and_then(|v| v.downcast_ref::<i32>().ok())
                    .map(|v| v == 1),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture {
        symbolic: bool,
    }
    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl Fixture {
        #[zbus(property)]
        fn title(&self) -> &str {
            "Fixture"
        }
        #[zbus(property)]
        fn status(&self) -> &str {
            "Active"
        }
        #[zbus(property)]
        fn icon_name(&self) -> &str {
            if self.symbolic {
                "fixture-symbolic"
            } else {
                ""
            }
        }
        #[zbus(property)]
        fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
            vec![(1, 1, vec![255, 10, 20, 30])]
        }
        #[zbus(property)]
        fn menu(&self) -> zbus::zvariant::ObjectPath<'static> {
            zbus::zvariant::ObjectPath::try_from("/Menu").unwrap()
        }
        #[zbus(property)]
        fn item_is_menu(&self) -> bool {
            true
        }
    }

    struct FixtureMenu {
        shows: Arc<AtomicUsize>,
        clicks: mpsc::Sender<(i32, u32)>,
    }
    #[zbus::interface(name = "com.canonical.dbusmenu")]
    impl FixtureMenu {
        fn about_to_show(&self, _id: i32) -> bool {
            self.shows.fetch_add(1, Ordering::Relaxed);
            false
        }
        fn event(&self, id: i32, event: &str, _data: Value<'_>, time: u32) {
            if event == "clicked" {
                self.clicks.send((id, time)).unwrap();
            }
        }
        fn get_layout(
            &self,
            _parent: i32,
            _depth: i32,
            _properties: Vec<String>,
        ) -> (u32, MenuNode) {
            let mut properties = HashMap::new();
            properties.insert(
                "label".to_owned(),
                OwnedValue::try_from(Value::from("_Connect")).unwrap(),
            );
            properties.insert("enabled".to_owned(), OwnedValue::from(true));
            properties.insert("toggle-state".to_owned(), OwnedValue::from(1_i32));
            let child =
                OwnedValue::try_from(Value::from((7_i32, properties, Vec::<OwnedValue>::new())))
                    .unwrap();
            (1, (0, HashMap::new(), vec![child]))
        }
    }

    #[test]
    fn watcher_registers_icons_menus_and_removes_disconnected_owners() {
        const TEST: &str =
            "modules::tray::tests::watcher_registers_icons_menus_and_removes_disconnected_owners";
        if std::env::var_os("DESKTOP_BAR_PRIVATE_BUS_TEST").is_none() {
            let directory = std::env::temp_dir().join(format!(
                "desktop-bar-bus-{}-{}",
                std::process::id(),
                super::super::clock::now()
            ));
            std::fs::create_dir(&directory).unwrap();
            let config = directory.join("session.conf");
            // Never include host bus policy, service activation, or listeners.
            std::fs::write(
                &config,
                r#"<busconfig>
                <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
                <policy context="default"><allow send_destination="*"/>
                <allow receive_sender="*"/><allow own="*"/></policy>
                </busconfig>"#,
            )
            .unwrap();
            let status = std::process::Command::new("dbus-run-session")
                .arg("--config-file")
                .arg(&config)
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env("DESKTOP_BAR_PRIVATE_BUS_TEST", "1")
                .status()
                .expect("dbus-run-session is required for tray tests");
            std::fs::remove_file(config).unwrap();
            std::fs::remove_dir(directory).unwrap();
            assert!(status.success());
            return;
        }
        let (tx, rx) = mpsc::channel();
        let client = Client::new(Color::hex("#f4f4f5"), move |items| {
            let _ = tx.send(items);
        });
        let shows = Arc::new(AtomicUsize::new(0));
        let (clicks, clicked) = mpsc::channel();
        let fixture = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.example.DesktopBarFixture")
            .unwrap()
            .serve_at("/StatusNotifierItem", Fixture { symbolic: false })
            .unwrap()
            .serve_at(
                "/Menu",
                FixtureMenu {
                    shows: shows.clone(),
                    clicks,
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let watcher = Proxy::new(&fixture, WATCHER, WATCHER_PATH, WATCHER).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if watcher
                .call::<_, _, ()>(
                    "RegisterStatusNotifierItem",
                    &("org.example.DesktopBarFixture",),
                )
                .is_ok()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "watcher did not start"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let items = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Fixture");
        assert_eq!(items[0].icon.as_ref().unwrap().data(), &[10, 20, 30, 255]);
        assert_eq!(items[0].bus, "org.example.DesktopBarFixture");
        let address = items[0].address.clone();
        client.send(Action::Menu(address.clone(), 0));
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let replies = client.menus.lock().unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].entries.len(), 1);
        assert_eq!(replies[0].entries[0].label, "Connect");
        assert_eq!(replies[0].entries[0].toggle, Some(true));
        drop(replies);
        fixture
            .emit_signal(
                None::<&str>,
                "/Menu",
                MENU,
                "LayoutUpdated",
                &(2_u32, 0_i32),
            )
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(shows.load(Ordering::Relaxed), 1);
        client.send(Action::Click(address, 7, 123_456));
        assert_eq!(
            clicked.recv_timeout(Duration::from_secs(5)).unwrap(),
            (7, 123_456)
        );
        client.send(Action::CloseMenu);
        // Synchronize with the action worker before emitting another signal.
        client.send(Action::Click(items[0].address.clone(), 7, 123_457));
        clicked.recv_timeout(Duration::from_secs(5)).unwrap();
        fixture
            .emit_signal(
                None::<&str>,
                "/Menu",
                MENU,
                "LayoutUpdated",
                &(3_u32, 0_i32),
            )
            .unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        fixture
            .object_server()
            .at("/SymbolicItem", Fixture { symbolic: true })
            .unwrap();
        watcher
            .call::<_, _, ()>("RegisterStatusNotifierItem", &("/SymbolicItem",))
            .unwrap();
        let icons = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(icons.len(), 2);
        let symbolic = icons.iter().find(|item| item.symbolic).unwrap();
        assert_eq!(
            symbolic.icon.as_ref().unwrap().data(),
            &[244, 244, 245, 255]
        );
        client.send(Action::Foreground(Color::hex("#4c4f69")));
        let icons = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let symbolic = icons.iter().find(|item| item.symbolic).unwrap();
        let regular = icons.iter().find(|item| !item.symbolic).unwrap();
        assert_eq!(symbolic.icon.as_ref().unwrap().data(), &[76, 79, 105, 255]);
        assert_eq!(regular.icon.as_ref().unwrap().data(), &[10, 20, 30, 255]);
        drop(watcher);
        drop(fixture);
        // The advertised bus name and its unique owner can disappear separately.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !rx
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap()
            .is_empty()
        {}
    }
    #[test]
    fn network_argb_is_premultiplied_and_malformed_icons_are_rejected() {
        let p = decode_pixmaps(vec![(1, 1, vec![128, 255, 0, 64])]).unwrap();
        assert_eq!(p.data(), &[128, 0, 32, 128]);
        assert!(decode_pixmaps(vec![(i32::MAX, 1, vec![])]).is_none());
        assert!(decode_pixmaps(vec![(2, 2, vec![0; 3])]).is_none());
    }

    #[test]
    fn symbolic_tints_preserve_transparency_and_premultiplied_edges() {
        let mut icon = decode_pixmaps(vec![(
            3,
            1,
            vec![255, 46, 52, 54, 128, 46, 52, 54, 0, 46, 52, 54],
        )])
        .unwrap();
        tint_symbolic(&mut icon, Color::hex("#ffffff"));
        assert_eq!(
            icon.data(),
            &[255, 255, 255, 255, 128, 128, 128, 128, 0, 0, 0, 0]
        );
        tint_symbolic(&mut icon, Color::hex("#4c4f69"));
        assert_eq!(
            icon.data(),
            &[76, 79, 105, 255, 38, 40, 53, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn named_icons_load_scalable_svg_and_existing_png_files() {
        let root = std::env::temp_dir().join(format!("desktop-bar-icons-{}", std::process::id()));
        let scalable = root.join("hicolor/scalable/apps");
        std::fs::create_dir_all(&scalable).unwrap();
        std::fs::write(
            scalable.join("fixture-tray.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10"><path fill="#ffffff" d="M0 0H20V10H0Z"/></svg>"##,
        )
        .unwrap();
        let roots = [root.clone()];
        let icon = lookup_icon_in_roots("fixture-tray", &roots).unwrap();
        assert_eq!((icon.width(), icon.height()), (64, 64));
        assert_eq!(icon.pixel(32, 32).unwrap().alpha(), 255);
        assert_eq!(icon.pixel(32, 0).unwrap().alpha(), 0);
        assert!(lookup_icon_in_roots("fixture-tray.svg", &roots).is_some());
        assert!(
            lookup_icon_in_roots(scalable.join("fixture-tray.svg").to_str().unwrap(), &[])
                .is_some()
        );
        let mut png = tiny_skia::Pixmap::new(16, 16).unwrap();
        png.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        png.save_png(root.join("fixture-png.png")).unwrap();
        let icon = lookup_icon_in_roots("fixture-png", &roots).unwrap();
        assert_eq!((icon.width(), icon.height()), (16, 16));
        assert_eq!(icon.pixel(0, 0).unwrap().red(), 10);
        assert!(lookup_icon_in_roots("../fixture-png", &roots).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
