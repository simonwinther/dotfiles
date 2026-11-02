use std::{collections::HashMap, time::Duration};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

type Properties = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;
const IWD: &str = "net.connman.iwd";
const STATION: &str = "net.connman.iwd.Station";
const DEVICE: &str = "net.connman.iwd.Device";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    pub available: bool,
    pub powered: bool,
    pub state: String,
    pub name: String,
    pub interface: String,
    pub rssi: Option<i16>,
}
impl Network {
    pub fn connected(&self) -> bool {
        self.available && self.powered && matches!(self.state.as_str(), "connected" | "roaming")
    }
    pub fn bars(&self) -> usize {
        match self.rssi {
            Some(rssi) if rssi < -80 => 1,
            Some(rssi) if rssi < -70 => 2,
            Some(rssi) if rssi < -60 => 3,
            _ => 4,
        }
    }
    pub fn icon(&self) -> &'static str {
        if !self.available || !self.powered {
            "󰤮"
        } else if !self.connected() {
            "󰤭"
        } else {
            ["󰤟", "󰤢", "󰤥", "󰤨"][self.bars() - 1]
        }
    }
    pub fn status(&self) -> &str {
        if !self.available {
            "Wi-Fi service unavailable"
        } else if !self.powered {
            "Wi-Fi off"
        } else {
            match self.state.as_str() {
                "connected" => "Connected",
                "roaming" => "Roaming",
                "connecting" => "Connecting…",
                "disconnecting" => "Disconnecting…",
                _ => "Disconnected",
            }
        }
    }
}

pub fn signals() -> HashMap<String, i16> {
    std::fs::read_to_string("/proc/net/wireless")
        .ok()
        .into_iter()
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| {
                    let (interface, values) = line.split_once(':')?;
                    let rssi = values
                        .split_whitespace()
                        .nth(2)?
                        .trim_end_matches('.')
                        .parse::<i16>()
                        .ok()?;
                    (-120..=0)
                        .contains(&rssi)
                        .then(|| (interface.trim().to_owned(), rssi))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn read(connection: &zbus::Connection) -> zbus::Result<Network> {
    let manager =
        zbus::Proxy::new(connection, IWD, "/", "org.freedesktop.DBus.ObjectManager").await?;
    let objects: Objects = manager.call("GetManagedObjects", &()).await?;
    let mut devices: Vec<_> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let station = interfaces.get(STATION)?;
            let device = interfaces.get(DEVICE)?;
            Some((path, station, device))
        })
        .collect();
    devices.sort_by_key(|(path, station, _)| {
        let state = station
            .get("State")
            .and_then(|value| <&str>::try_from(value).ok())
            .unwrap_or("");
        (!matches!(state, "connected" | "roaming"), path.as_str())
    });
    let Some((_, station, device)) = devices.first() else {
        return Ok(Network::default());
    };
    let text = |values: &Properties, key: &str| {
        values
            .get(key)
            .and_then(|value| <&str>::try_from(value).ok())
            .unwrap_or("")
            .to_owned()
    };
    let interface = text(device, "Name");
    let name = station
        .get("ConnectedNetwork")
        .and_then(|value| <&zbus::zvariant::ObjectPath<'_>>::try_from(value).ok())
        .and_then(|path| objects.get(path))
        .and_then(|interfaces| interfaces.get("net.connman.iwd.Network"))
        .map(|network| text(network, "Name"))
        .unwrap_or_default();
    let rssi = signals().get(&interface).copied();
    Ok(Network {
        available: true,
        powered: device
            .get("Powered")
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(true),
        state: text(station, "State"),
        name,
        interface,
        rssi,
    })
}

fn iwd_present(connection: &zbus::blocking::Connection) -> bool {
    zbus::blocking::fdo::DBusProxy::new(connection)
        .and_then(|bus| Ok(bus.name_has_owner(IWD.try_into()?)?))
        .unwrap_or(true)
}

pub fn listen(send: impl Fn(Network) + Send + 'static) {
    std::thread::Builder::new()
        .name("bar-network".into())
        .spawn(move || {
            loop {
                let run = || -> zbus::Result<()> {
                    let connection = zbus::blocking::connection::Builder::system()?
                        .method_timeout(Duration::from_secs(2)).build()?;
                    let mut messages = crate::bus::Signals::new(&connection, &[
                        "type='signal',sender='net.connman.iwd',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
                        "type='signal',sender='net.connman.iwd',interface='org.freedesktop.DBus.ObjectManager'",
                        "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='net.connman.iwd'",
                    ])?;
                    let mut previous = crate::bus::query(read(connection.inner())).unwrap_or_default();
                    send(previous.clone());
                    for message in &mut messages {
                        let message = message?;
                        let header = message.header();
                        if header
                            .member()
                            .is_some_and(|member| member == "PropertiesChanged")
                        {
                            let (interface, changed, invalidated): (
                                String,
                                Properties,
                                Vec<String>,
                            ) = message.body().deserialize()?;
                            if ![STATION, DEVICE].contains(&interface.as_str())
                                || !["State", "ConnectedNetwork", "Powered"].iter().any(|key| {
                                    changed.contains_key(*key)
                                        || invalidated.iter().any(|value| value == key)
                                })
                            {
                                continue;
                            }
                        } else if header.member().is_some_and(|member| member == "InterfacesAdded") {
                            // Scans add networks and access points constantly.
                            if let Ok((_, interfaces)) = message.body().deserialize::<(
                                zbus::zvariant::OwnedObjectPath,
                                HashMap<String, Properties>,
                            )>() && ![STATION, DEVICE].iter().any(|i| interfaces.contains_key(*i))
                            {
                                continue;
                            }
                        } else if header.member().is_some_and(|member| member == "InterfacesRemoved")
                            && let Ok((_, interfaces)) = message
                                .body()
                                .deserialize::<(zbus::zvariant::OwnedObjectPath, Vec<String>)>()
                            && ![STATION, DEVICE].iter().any(|i| interfaces.iter().any(|n| n == i))
                        {
                            continue;
                        }
                        let next = match crate::bus::query(read(connection.inner())) {
                            Ok(next) => next,
                            // A slow reply is not an outage. Keep the last state
                            // unless the bus confirms that iwd has gone away.
                            Err(_) if iwd_present(&connection) => continue,
                            Err(_) => Network::default(),
                        };
                        if next != previous {
                            send(next.clone());
                            previous = next;
                        }
                    }
                    Ok(())
                };
                let _ = run();
                send(Network::default());
                std::thread::sleep(Duration::from_secs(5));
            }
        })
        .expect("network worker");
}

#[cfg(test)]
mod tests {
    use super::Network;

    #[test]
    fn stale_station_state_does_not_show_a_connection_when_wifi_is_off() {
        let mut network = Network {
            available: true,
            powered: true,
            state: "connected".into(),
            ..Default::default()
        };
        for state in ["connected", "roaming"] {
            network.state = state.into();
            assert!(network.connected());
            network.powered = false;
            assert!(!network.connected());
            network.powered = true;
            network.available = false;
            assert!(!network.connected());
            network.available = true;
        }
        network.state = "disconnected".into();
        assert!(!network.connected());
    }
}
