//! On-demand native desktop panels and their actions.
use crate::shell::{Entry, Page};
use std::{collections::HashMap, process::Command};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

type Properties = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;
fn text(values: &Properties, key: &str) -> String {
    values
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or_default()
        .into()
}
fn flag(values: &Properties, key: &str) -> bool {
    values
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}
fn entry(id: String, name: String, detail: String, icon: &str, args: Vec<String>) -> Entry {
    Entry {
        id,
        name,
        detail,
        icon: icon.into(),
        command: args,
        ..Default::default()
    }
}
fn action_entry(id: &str, name: &str, detail: &str, icon: &str, args: &[&str]) -> Entry {
    entry(
        id.into(),
        name.into(),
        detail.into(),
        icon,
        std::iter::once("native")
            .chain(args.iter().copied())
            .map(String::from)
            .collect(),
    )
}
pub fn load(provider: &str, cancelled: impl Fn() -> bool) -> Option<Result<Vec<Entry>, String>> {
    if let Some(group) = provider.strip_prefix("menu/")
        && !matches!(
            group,
            "font" | "power-profile" | "config" | "hardware" | "toggle"
        )
    {
        return Some(crate::menus::load(group));
    }
    match provider {
        "keybindings" => return Some(keybindings(&cancelled).map_err(|e| e.to_string())),
        "audio" => {
            return Some(crate::modules::audio::entries(&cancelled).map_err(|e| e.to_string()));
        }
        "notifications" | "bluetooth" | "wifi" => {}
        _ => return None,
    }
    let read = async {
        match provider {
            "notifications" => notifications().await,
            "bluetooth" => bluetooth().await,
            _ => wifi().await,
        }
        .map_err(|error| error.to_string())
    };
    let guard = async {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if cancelled() {
                return Err("Cancelled".into());
            }
            if std::time::Instant::now() >= deadline {
                return Err("Desktop service did not respond".into());
            }
            async_io::Timer::after(std::time::Duration::from_millis(40)).await;
        }
    };
    let result = async_io::block_on(futures_lite::future::race(read, guard));
    if result.is_err() {
        // A stalled or broken connection must not serve the next poll.
        release_connections();
    }
    Some(result)
}

thread_local! {
    /// Session and system connections reused by pages that poll every second.
    static BUSES: std::cell::RefCell<[Option<zbus::Connection>; 2]> = Default::default();
}

async fn bus(system: bool) -> zbus::Result<zbus::Connection> {
    if let Some(connection) = BUSES.with_borrow(|buses| buses[usize::from(system)].clone()) {
        return Ok(connection);
    }
    let connection = if system {
        zbus::Connection::system().await?
    } else {
        zbus::Connection::session().await?
    };
    BUSES.with_borrow_mut(|buses| buses[usize::from(system)] = Some(connection.clone()));
    Ok(connection)
}

/// Closes the polling connections, for example when their panel closes.
pub fn release_connections() {
    BUSES.with_borrow_mut(|buses| *buses = Default::default());
}
pub struct Service {
    sender: smithay_client_toolkit::reexports::calloop::channel::Sender<crate::modules::Update>,
    bluetooth: Option<Connection>,
    wireless: Option<Connection>,
}
impl Service {
    pub fn new(
        sender: smithay_client_toolkit::reexports::calloop::channel::Sender<crate::modules::Update>,
    ) -> Self {
        Self {
            sender,
            bluetooth: None,
            wireless: None,
        }
    }
    pub fn execute(&mut self, args: &[String]) -> Result<(), String> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = match args.as_slice() {
            ["idle"] => {
                self.bluetooth = None;
                self.wireless = None;
                return Ok(());
            }
            ["media", bus, method] => return crate::modules::media::action(bus, method),
            ["bluetooth", path, method] => {
                if self.bluetooth.is_none() {
                    self.bluetooth = Some(self.connect_agent(false).map_err(|e| e.to_string())?);
                }
                bluetooth_action(self.bluetooth.as_ref().unwrap(), path, method)
            }
            ["wifi", path, method] => {
                if self.wireless.is_none() {
                    self.wireless = Some(self.connect_agent(true).map_err(|e| e.to_string())?);
                }
                wifi_action(self.wireless.as_ref().unwrap(), path, method)
            }
            ["notifications", method, id] => notification_action(method, id),
            ["audio", operation, target, value] => {
                return crate::modules::audio::action(operation, target, value);
            }
            _ => return Err("Unsupported desktop action".into()),
        };
        result.map_err(|e| e.to_string())
    }
    fn connect_agent(&self, wireless: bool) -> zbus::Result<Connection> {
        let agent = crate::prompt::Agent {
            sender: self.sender.clone(),
        };
        let path = zbus::zvariant::ObjectPath::try_from("/org/desktop/Bar/Agent")?;
        let builder = zbus::blocking::connection::Builder::system()?
            .method_timeout(std::time::Duration::from_secs(120));
        let connection = if wireless {
            builder
                .serve_at(path.clone(), crate::prompt::WirelessAgent(agent))?
                .build()?
        } else {
            builder.serve_at(path.clone(), agent)?.build()?
        };
        let (name, root, interface) = if wireless {
            (
                "net.connman.iwd",
                "/net/connman/iwd",
                "net.connman.iwd.AgentManager",
            )
        } else {
            ("org.bluez", "/org/bluez", "org.bluez.AgentManager1")
        };
        let manager = Proxy::new(&connection, name, root, interface)?;
        if wireless {
            manager.call::<_, _, ()>("RegisterAgent", &(path,))?;
        } else {
            let _ = manager.call::<_, _, ()>("RegisterAgent", &(path, "KeyboardDisplay"));
        }
        Ok(connection)
    }
}
pub fn refresh(page: Page) -> bool {
    matches!(
        page,
        Page::Notifications | Page::Bluetooth | Page::Audio | Page::Wifi
    )
}
fn keybindings(cancelled: &impl Fn() -> bool) -> anyhow::Result<Vec<Entry>> {
    let output = crate::process::output(
        Command::new("hyprctl").args(["-j", "binds"]),
        std::time::Duration::from_secs(3),
        8 * 1024 * 1024,
        cancelled,
    )?;
    anyhow::ensure!(
        output.status.success(),
        "Could not read compositor keybindings"
    );
    let bindings: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;
    let readable = if bindings.iter().any(|binding| {
        binding["description"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            && binding["key"].as_str().unwrap_or_default().is_empty()
            && binding["keycode"].as_u64().unwrap_or(0) == 0
    }) {
        let result = crate::process::output(
            Command::new("hyprctl").arg("binds"),
            std::time::Duration::from_secs(3),
            8 * 1024 * 1024,
            cancelled,
        )?;
        let mut records: Vec<HashMap<String, String>> = Vec::new();
        for line in String::from_utf8_lossy(&result.stdout).lines() {
            if line.starts_with("bind") {
                records.push(HashMap::new());
            } else if let Some(line) = line.strip_prefix('\t')
                && let Some((key, value)) = line.split_once(": ")
                && let Some(record) = records.last_mut()
            {
                record.insert(key.into(), value.into());
            }
        }
        records
    } else {
        vec![]
    };
    let mut entries = Vec::new();
    for binding in bindings {
        let description = binding["description"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if description.is_empty() {
            continue;
        }
        let mut keys = Vec::new();
        let mask = binding["modmask"].as_u64().unwrap_or(0);
        for (bit, name) in [(64, "Super"), (4, "Ctrl"), (8, "Alt"), (1, "Shift")] {
            if mask & bit != 0 {
                keys.push(name.to_owned());
            }
        }
        let keycode = binding["keycode"].as_u64().unwrap_or(0);
        let mut key = if keycode > 0 {
            format!("code:{keycode}")
        } else {
            binding["key"].as_str().unwrap_or_default().into()
        };
        if key.is_empty() {
            let matching: std::collections::HashSet<_> = readable
                .iter()
                .filter(|record| {
                    ["modmask", "submap", "description", "dispatcher", "arg"]
                        .iter()
                        .all(|field| {
                            let expected = binding[*field]
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| {
                                    if binding[*field].is_null() {
                                        String::new()
                                    } else {
                                        binding[*field].to_string()
                                    }
                                });
                            record.get(*field).map(String::as_str).unwrap_or_default() == expected
                        })
                })
                .filter_map(|record| record.get("key"))
                .collect();
            if matching.len() == 1 {
                key = (*matching.into_iter().next().unwrap()).clone();
            }
        }
        if key.is_empty() && binding["catch_all"] == true {
            key = "catchall".into();
        }
        if key.is_empty() {
            continue;
        }
        keys.push(key);
        let mut shortcut = keys.join(" + ");
        if let Some(submap) = binding["submap"].as_str().filter(|value| !value.is_empty()) {
            shortcut = format!("[{submap}] {shortcut}");
        }
        for (field, label) in [("release", "release"), ("longPress", "long press")] {
            if binding[field] == true {
                shortcut.push_str(&format!(" ({label})"));
            }
        }
        entries.push(entry(shortcut.clone(), shortcut, description, "󰌌", vec![]));
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.detail.cmp(&b.detail)));
    entries.dedup_by(|a, b| a.name == b.name && a.detail == b.detail);
    Ok(entries)
}
async fn notifications() -> anyhow::Result<Vec<Entry>> {
    let connection = bus(false).await?;
    let mako = zbus::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/fr/emersion/Mako",
        "fr.emersion.Mako",
    )
    .await?;
    let current: Vec<Properties> = mako.call("ListNotifications", &()).await?;
    let history: Vec<Properties> = mako.call("ListHistory", &()).await?;
    let mut entries = Vec::new();
    for (active, notification) in current
        .into_iter()
        .map(|n| (true, n))
        .chain(history.into_iter().map(|n| (false, n)))
    {
        let Some(id) = notification.get("id").and_then(|v| u32::try_from(v).ok()) else {
            continue;
        };
        let title = text(&notification, "summary");
        let body = plain_text(&text(&notification, "body"));
        let app = text(&notification, "app-name");
        let mut item = action_entry(
            &format!("notification/{id}"),
            &title,
            &format!("{app} · {body}"),
            "󰂚",
            &["notifications", "invoke", &id.to_string()],
        );
        if !active {
            item.command = vec!["notification-detail".into(), title.clone(), body.clone()];
        }
        item.keywords = if active { "active" } else { "history" }.into();
        entries.push(item);
    }
    if !entries.is_empty() {
        entries.push(action_entry(
            "dismiss-all",
            "Dismiss all",
            "Move active notifications to history",
            "󰂛",
            &["notifications", "dismiss-all", "0"],
        ));
    }
    Ok(entries)
}
fn plain_text(value: &str) -> String {
    let mut inside = false;
    value
        .chars()
        .filter(|c| {
            if *c == '<' {
                inside = true;
                false
            } else if *c == '>' {
                inside = false;
                false
            } else {
                !inside && (!c.is_control() || *c == '\n')
            }
        })
        .collect::<String>()
        .replace('\n', " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
fn notification_action(method: &str, id: &str) -> zbus::Result<()> {
    let connection = Connection::session()?;
    let mako = Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/fr/emersion/Mako",
        "fr.emersion.Mako",
    )?;
    match method {
        "invoke" => mako.call("InvokeAction", &(id.parse::<u32>().unwrap_or(0), "default")),
        "dismiss" | "dismiss-all" => {
            let mut values: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
            if method == "dismiss" {
                values.insert("id", id.parse::<u32>().unwrap_or(0).into());
            } else {
                values.insert("all", true.into());
            }
            mako.call("DismissNotifications", &(values,))
        }
        _ => Ok(()),
    }
}
async fn bluetooth() -> anyhow::Result<Vec<Entry>> {
    let connection = bus(true).await?;
    let manager = zbus::Proxy::new(
        &connection,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .await?;
    let objects: Objects = manager.call("GetManagedObjects", &()).await?;
    let mut adapters: Vec<_> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Adapter1")
                .map(|properties| (path, properties))
        })
        .collect();
    adapters.sort_by_key(|(path, _)| path.as_str());
    let mut entries = Vec::new();
    for (path, adapter) in adapters {
        let powered = flag(adapter, "Powered");
        entries.push(action_entry(
            &format!("{path}/power"),
            if powered {
                "Bluetooth on"
            } else {
                "Bluetooth off"
            },
            &text(adapter, "Alias"),
            "󰂯",
            &[
                "bluetooth",
                path.as_str(),
                if powered { "power-off" } else { "power-on" },
            ],
        ));
        if powered {
            entries.push(action_entry(
                &format!("{path}/scan"),
                if flag(adapter, "Discovering") {
                    "Stop scanning"
                } else {
                    "Scan for devices"
                },
                "Paired and nearby devices",
                "󰑐",
                &[
                    "bluetooth",
                    path.as_str(),
                    if flag(adapter, "Discovering") {
                        "scan-stop"
                    } else {
                        "scan-start"
                    },
                ],
            ));
        }
    }
    let mut devices: Vec<_> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Device1")
                .map(|properties| (path, properties))
        })
        .collect();
    devices.sort_by_key(|(_, d)| {
        (
            !flag(d, "Connected"),
            !flag(d, "Paired"),
            text(d, "Alias").to_lowercase(),
        )
    });
    for (path, device) in devices {
        let connected = flag(device, "Connected");
        let paired = flag(device, "Paired");
        let detail = if connected {
            "Connected · Enter to disconnect"
        } else if paired {
            "Paired · Enter to connect"
        } else {
            "Nearby · Enter to pair"
        };
        entries.push(action_entry(
            path.as_str(),
            &text(device, "Alias"),
            detail,
            if text(device, "Icon").contains("audio") {
                "󰋋"
            } else {
                "󰂯"
            },
            &[
                "bluetooth",
                path.as_str(),
                if connected {
                    "disconnect"
                } else if paired {
                    "connect"
                } else {
                    "pair"
                },
            ],
        ));
    }
    Ok(entries)
}
fn bluetooth_action(connection: &Connection, path: &str, method: &str) -> zbus::Result<()> {
    if !path.starts_with("/org/bluez/") {
        return Ok(());
    }
    let adapter = ["power-on", "power-off", "scan-start", "scan-stop"].contains(&method);
    let proxy = Proxy::new(
        connection,
        "org.bluez",
        path,
        if adapter {
            "org.bluez.Adapter1"
        } else {
            "org.bluez.Device1"
        },
    )?;
    match method {
        "power-on" | "power-off" => proxy
            .set_property("Powered", method == "power-on")
            .map_err(Into::into),
        "scan-start" => proxy.call("StartDiscovery", &()),
        "scan-stop" => proxy.call("StopDiscovery", &()),
        "connect" => proxy.call("Connect", &()),
        "disconnect" => proxy.call("Disconnect", &()),
        "pair" => {
            proxy.call::<_, _, ()>("Pair", &())?;
            proxy.set_property("Trusted", true)?;
            proxy.call("Connect", &())
        }
        _ => Ok(()),
    }
}

async fn wifi() -> anyhow::Result<Vec<Entry>> {
    let connection = bus(true).await?;
    let manager = zbus::Proxy::new(
        &connection,
        "net.connman.iwd",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .await?;
    let objects: Objects = manager.call("GetManagedObjects", &()).await?;
    let mut entries = Vec::new();
    for (path, interfaces) in &objects {
        let Some(station) = interfaces.get("net.connman.iwd.Station") else {
            continue;
        };
        let device = interfaces.get("net.connman.iwd.Device");
        let powered = device.is_some_and(|d| flag(d, "Powered"));
        entries.push(action_entry(
            &format!("{path}/power"),
            if powered { "Wi-Fi on" } else { "Wi-Fi off" },
            "Toggle wireless radio",
            "󰖩",
            &[
                "wifi",
                path.as_str(),
                if powered { "power-off" } else { "power-on" },
            ],
        ));
        if !powered {
            continue;
        }
        entries.push(action_entry(
            &format!("{path}/scan"),
            if flag(station, "Scanning") {
                "Scanning…"
            } else {
                "Scan for networks"
            },
            "Nearby wireless networks",
            "󰑐",
            &["wifi", path.as_str(), "scan"],
        ));
        let proxy = zbus::Proxy::new(
            &connection,
            "net.connman.iwd",
            path.as_str(),
            "net.connman.iwd.Station",
        )
        .await?;
        let networks: Vec<(OwnedObjectPath, i16)> = proxy.call("GetOrderedNetworks", &()).await?;
        for (network, strength) in networks {
            let Some(properties) = objects
                .get(&network)
                .and_then(|i| i.get("net.connman.iwd.Network"))
            else {
                continue;
            };
            let connected = flag(properties, "Connected");
            let security = text(properties, "Type");
            entries.push(action_entry(
                network.as_str(),
                &text(properties, "Name"),
                &format!(
                    "{} · {} dBm · {}",
                    if connected { "Connected" } else { "Available" },
                    strength / 100,
                    security
                ),
                "󰖩",
                &[
                    "wifi",
                    network.as_str(),
                    if connected { "disconnect" } else { "connect" },
                ],
            ));
        }
    }
    Ok(entries)
}
fn wifi_action(connection: &Connection, path: &str, method: &str) -> zbus::Result<()> {
    if !path.starts_with("/net/connman/iwd/") {
        return Ok(());
    }
    let interface = match method {
        "power-on" | "power-off" => "net.connman.iwd.Device",
        "connect" => "net.connman.iwd.Network",
        _ => "net.connman.iwd.Station",
    };
    let station = if method == "disconnect" {
        path.rsplit_once('/')
            .map(|(station, _)| station)
            .unwrap_or(path)
    } else {
        path
    };
    let proxy = Proxy::new(connection, "net.connman.iwd", station, interface)?;
    match method {
        "power-on" | "power-off" => proxy
            .set_property("Powered", method == "power-on")
            .map_err(Into::into),
        "scan" => proxy.call("Scan", &()),
        "connect" => proxy.call("Connect", &()),
        "disconnect" => proxy.call("Disconnect", &()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_markup_is_plain_and_preserves_labels() {
        assert_eq!(plain_text("<b>Title</b>\nA &amp; B"), "Title A & B");
    }
}
