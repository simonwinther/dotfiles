use crate::core::Event;
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BluetoothState {
    pub adapters: Vec<Adapter>,
    pub devices: Vec<Device>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adapter {
    pub path: String,
    pub label: String,
    pub powered: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub path: String,
    pub address: String,
    pub label: String,
    pub connected: bool,
    pub audio: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BluetoothAction {
    Power { path: String, on: bool },
    Connect { path: String },
    Disconnect { path: String },
}
fn string(p: &HashMap<String, OwnedValue>, k: &str) -> String {
    p.get(k)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| String::try_from(v).ok())
        .unwrap_or_default()
}
fn boolean(p: &HashMap<String, OwnedValue>, k: &str) -> bool {
    p.get(k)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}
pub async fn discover(connection: &Connection) -> Result<BluetoothState> {
    let bus = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?;
    let available: bool = bus.call("NameHasOwner", &("org.bluez",)).await?;
    if !available {
        return Ok(BluetoothState {
            error: Some("No active Bluetooth adapter / BlueZ service".into()),
            ..Default::default()
        });
    }

    let proxy = zbus::fdo::ObjectManagerProxy::builder(connection)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await?;
    let objects = tokio::time::timeout(Duration::from_secs(8), proxy.get_managed_objects())
        .await
        .context("BlueZ discovery timed out")??;
    let mut state = BluetoothState::default();
    for (path, interfaces) in objects {
        if let Some(p) = interfaces.get("org.bluez.Adapter1") {
            state.adapters.push(Adapter {
                path: path.to_string(),
                label: string(p, "Alias"),
                powered: boolean(p, "Powered"),
            });
        }
        if let Some(p) = interfaces.get("org.bluez.Device1") {
            if !boolean(p, "Paired") {
                continue;
            }
            let icon = string(p, "Icon");
            state.devices.push(Device {
                path: path.to_string(),
                address: string(p, "Address"),
                label: string(p, "Alias"),
                connected: boolean(p, "Connected"),
                audio: icon.contains("audio") || icon.contains("headset"),
            });
        }
    }
    state.adapters.sort_by(|a, b| a.path.cmp(&b.path));
    state.devices.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(state)
}
pub async fn act(
    connection: &Connection,
    action: &BluetoothAction,
    state: &BluetoothState,
) -> Result<()> {
    match action {
        BluetoothAction::Power { path, on } => {
            ensure!(
                state.adapters.iter().any(|a| a.path == *path),
                "Bluetooth adapter disappeared"
            );
            let proxy = Proxy::new(
                connection,
                "org.bluez",
                OwnedObjectPath::try_from(path.clone())?,
                "org.bluez.Adapter1",
            )
            .await?;
            proxy.set_property("Powered", on).await?;
        }
        BluetoothAction::Connect { path } | BluetoothAction::Disconnect { path } => {
            ensure!(
                state.devices.iter().any(|d| d.path == *path),
                "paired Bluetooth device disappeared"
            );
            let proxy = Proxy::new(
                connection,
                "org.bluez",
                OwnedObjectPath::try_from(path.clone())?,
                "org.bluez.Device1",
            )
            .await?;
            let method = if matches!(action, BluetoothAction::Connect { .. }) {
                "Connect"
            } else {
                "Disconnect"
            };
            tokio::time::timeout(Duration::from_secs(35), proxy.call::<_, _, ()>(method, &()))
                .await
                .context("Bluetooth operation timed out")??;
        }
    }
    Ok(())
}
pub async fn watch(tx: mpsc::Sender<Event>) {
    let mut backoff = 1;
    loop {
        let result: Result<()> = async {
            let connection = Connection::system().await?;
            let bluez = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.bluez")?
                .build();
            let owner = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")?
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .add_arg("org.bluez")?
                .build();
            let mut signals = MessageStream::for_match_rule(bluez, &connection, Some(64)).await?;
            let mut owners = MessageStream::for_match_rule(owner, &connection, Some(8)).await?;
            tx.send(Event::Bluetooth).await?;
            loop {
                tokio::select! {
                    msg=signals.next()=>{msg.context("BlueZ signal stream closed")??;},
                    msg=owners.next()=>{msg.context("D-Bus owner stream closed")??;},
                }
                tx.send(Event::Bluetooth).await?;
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if tx.is_closed() {
            return;
        }
        eprintln!("bluetooth: {result:?}; reconnect in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
pub async fn resume_watch(tx: mpsc::Sender<Event>) {
    let mut backoff = 1;
    loop {
        let result: Result<()> = async {
            let connection = Connection::system().await?;
            let proxy = Proxy::new(
                &connection,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .await?;
            let mut signals = proxy.receive_signal("PrepareForSleep").await?;
            while let Some(signal) = signals.next().await {
                let (sleeping,): (bool,) = signal.body().deserialize()?;
                if !sleeping {
                    tx.send(Event::Resume).await?;
                }
            }
            anyhow::bail!("logind signal stream closed")
        }
        .await;
        if tx.is_closed() {
            return;
        }
        eprintln!("resume: {result:?}; reconnect in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
