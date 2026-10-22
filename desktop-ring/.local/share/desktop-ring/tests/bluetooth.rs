use desktop_ring::bluetooth::{self, BluetoothAction};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};
struct Bus(Child, std::path::PathBuf);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_file(&self.1);
    }
}
struct Adapter {
    powered: bool,
}
#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Test adapter"
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.powered
    }
    #[zbus(property)]
    fn set_powered(&mut self, value: bool) {
        self.powered = value;
    }
}
struct Device {
    connected: bool,
}
#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Paired headphones"
    }
    #[zbus(property)]
    fn address(&self) -> &str {
        "00:11:22:33:44:55"
    }
    #[zbus(property)]
    fn icon(&self) -> &str {
        "audio-headset"
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.connected
    }
    fn connect(&mut self) {
        self.connected = true;
    }
    fn disconnect(&mut self) {
        self.connected = false;
    }
}
#[tokio::test]
async fn paired_devices_power_connect_disconnect_and_stale_actions_use_bluez_dbus() {
    let config = std::env::temp_dir().join(format!("ring-bus-{}.conf", std::process::id()));
    std::fs::write(&config,r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>"#).unwrap();
    let mut child = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon test dependency");
    let mut address = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let _bus = Bus(child, config);
    let address = address.trim();
    let adapter = "/org/bluez/hci0";
    let device = "/org/bluez/hci0/dev_00_11_22_33_44_55";
    let _server = zbus::connection::Builder::address(address)
        .unwrap()
        .name("org.bluez")
        .unwrap()
        .serve_at("/", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at(adapter, Adapter { powered: false })
        .unwrap()
        .serve_at(device, Device { connected: false })
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let state = bluetooth::discover(&client).await.unwrap();
    assert_eq!(state.adapters.len(), 1);
    assert_eq!(state.devices.len(), 1);
    assert!(state.devices[0].audio);
    assert!(!state.devices[0].connected);
    bluetooth::act(
        &client,
        &BluetoothAction::Power {
            path: adapter.into(),
            on: true,
        },
        &state,
    )
    .await
    .unwrap();
    let state = bluetooth::discover(&client).await.unwrap();
    assert!(state.adapters[0].powered);
    bluetooth::act(
        &client,
        &BluetoothAction::Connect {
            path: device.into(),
        },
        &state,
    )
    .await
    .unwrap();
    let state = bluetooth::discover(&client).await.unwrap();
    assert!(state.devices[0].connected);
    bluetooth::act(
        &client,
        &BluetoothAction::Disconnect {
            path: device.into(),
        },
        &state,
    )
    .await
    .unwrap();
    assert!(!bluetooth::discover(&client).await.unwrap().devices[0].connected);
    assert!(
        bluetooth::act(
            &client,
            &BluetoothAction::Connect {
                path: "/org/bluez/gone".into()
            },
            &state
        )
        .await
        .is_err()
    );
}
