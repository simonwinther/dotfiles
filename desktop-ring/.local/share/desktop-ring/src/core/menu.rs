use super::{Action, Snapshot};
use crate::{audio::AudioAction, bluetooth::BluetoothAction, monitors::Position};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Menu {
    pub id: String,
    pub label: String,
    pub entries: Vec<Entry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub icon: String,
    pub active: bool,
    pub enabled: bool,
    pub submenu: Option<Menu>,
    pub action: Option<Action>,
    pub close: bool,
}
impl Entry {
    pub fn info(id: &str, label: &str, detail: &str, icon: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: detail.into(),
            icon: icon.into(),
            active: false,
            enabled: false,
            submenu: None,
            action: None,
            close: false,
        }
    }
    pub fn action(id: &str, label: &str, detail: &str, icon: &str, action: Action) -> Self {
        Self {
            enabled: true,
            action: Some(action),
            ..Self::info(id, label, detail, icon)
        }
    }
    pub fn branch(menu: Menu, icon: &str) -> Self {
        Self {
            enabled: true,
            submenu: Some(menu.clone()),
            ..Self::info(&menu.id, &menu.label, "", icon)
        }
    }
}
fn menu(id: &str, label: &str, entries: Vec<Entry>) -> Menu {
    Menu {
        id: id.into(),
        label: label.into(),
        entries,
    }
}
pub fn build(s: &Snapshot) -> Menu {
    let mut displays = vec![];
    if s.displays.switchable {
        let mut positions = vec![];
        for position in Position::ALL {
            let label = match position {
                Position::Left => "Left",
                Position::Right => "Right",
                Position::Below => "Below",
            };
            let mut e = Entry::action(
                label,
                label,
                "Laptop position",
                "󰌢",
                Action::Layout { position },
            );
            e.active = s.displays.position == Some(position);
            e.close = true;
            positions.push(e);
        }
        displays.push(Entry::branch(
            menu("laptop-position", "Laptop Position", positions),
            "󰌢",
        ));
    }
    for m in &s.displays.monitors {
        let mut e = Entry::info(
            &m.name,
            &m.model,
            &format!(
                "{} × {} · {:.0} Hz · {}×",
                m.width, m.height, m.refresh, m.scale
            ),
            "󰍹",
        );
        e.active = !m.disabled;
        displays.push(e);
    }
    if let Some(error) = &s.displays.diagnostic {
        displays.push(Entry::info("display-error", "Display status", error, "!"));
    }
    let mut roots = vec![Entry::branch(menu("displays", "Displays", displays), "󰍹")];
    for (input, devices) in [(false, &s.audio.outputs), (true, &s.audio.inputs)] {
        let mut entries = vec![];
        for d in devices {
            let status = if !d.available {
                "Unavailable"
            } else if d.active {
                "Current default"
            } else {
                "Select default"
            };
            let mut e = Entry::action(
                &d.identity,
                if d.short_label.is_empty() {
                    &d.label
                } else {
                    &d.short_label
                },
                &format!("{}\n{}", d.label, status),
                if input { "󰍬" } else { "󰕾" },
                Action::Audio {
                    action: AudioAction::Select {
                        input,
                        name: d.name.clone(),
                    },
                },
            );
            e.active = d.active;
            e.enabled = d.available;
            e.close = true;
            entries.push(e);
        }
        if entries.is_empty() {
            entries.push(Entry::info(
                "no-devices",
                "No devices",
                s.audio.error.as_deref().unwrap_or("No connected devices"),
                "—",
            ));
        }
        roots.push(Entry::branch(
            menu(
                if input { "audio-input" } else { "audio-output" },
                if input { "Microphone" } else { "Audio Output" },
                entries,
            ),
            if input { "󰍬" } else { "󰕾" },
        ));
    }
    let mut bluetooth = vec![];
    for a in &s.bluetooth.adapters {
        let mut e = Entry::action(
            &a.path,
            &a.label,
            if a.powered {
                "Turn Bluetooth off"
            } else {
                "Turn Bluetooth on"
            },
            "󰂯",
            Action::Bluetooth {
                action: BluetoothAction::Power {
                    path: a.path.clone(),
                    on: !a.powered,
                },
            },
        );
        e.active = a.powered;
        bluetooth.push(e);
    }
    for d in &s.bluetooth.devices {
        let busy = s.busy.as_deref() == Some(&d.path);
        let mut e = Entry::action(
            &d.path,
            &d.label,
            if busy {
                "Connecting…"
            } else if d.connected {
                if d.audio {
                    "Connected · choose it in Audio Output"
                } else {
                    "Connected · click to disconnect"
                }
            } else {
                "Paired · click to connect"
            },
            "󰂯",
            Action::Bluetooth {
                action: if d.connected {
                    BluetoothAction::Disconnect {
                        path: d.path.clone(),
                    }
                } else {
                    BluetoothAction::Connect {
                        path: d.path.clone(),
                    }
                },
            },
        );
        e.active = d.connected;
        e.enabled = !busy && s.busy.is_none();
        bluetooth.push(e);
    }
    if bluetooth.is_empty() {
        bluetooth.push(Entry::info(
            "no-bluetooth",
            "Bluetooth unavailable",
            s.bluetooth
                .error
                .as_deref()
                .unwrap_or("No Bluetooth adapter"),
            "󰂲",
        ));
    }
    roots.push(Entry::branch(
        menu("bluetooth", "Bluetooth", bluetooth),
        "󰂯",
    ));
    let mut volumes = vec![];
    for (input, devices) in [(false, &s.audio.outputs), (true, &s.audio.inputs)] {
        let current = devices.iter().find(|d| d.active);
        let detail = current
            .map(|d| format!("{}%{}", d.volume, if d.muted { " · muted" } else { "" }))
            .unwrap_or_else(|| "Unavailable".into());
        let mut controls = vec![];
        for (id, label, icon, action) in [
            (
                "lower",
                "Quieter",
                "−",
                AudioAction::Volume { input, delta: -5 },
            ),
            ("mute", "Mute / Unmute", "󰖁", AudioAction::Mute { input }),
            (
                "raise",
                "Louder",
                "+",
                AudioAction::Volume { input, delta: 5 },
            ),
        ] {
            let mut e = Entry::action(id, label, &detail, icon, Action::Audio { action });
            e.enabled = current.is_some();
            e.active = id == "mute" && current.is_some_and(|d| d.muted);
            controls.push(e);
        }
        volumes.push(Entry::branch(
            menu(
                if input {
                    "input-volume"
                } else {
                    "output-volume"
                },
                if input {
                    "Microphone level"
                } else {
                    "Output volume"
                },
                controls,
            ),
            if input { "󰍬" } else { "󰕾" },
        ));
    }
    roots.push(Entry::branch(
        menu("volume", "Volume Controls", volumes),
        "󰕾",
    ));
    if s.power.available || s.power.error.is_some() {
        let mut sleep = Entry::action(
            "sleep",
            "Sleep",
            s.power
                .error
                .as_deref()
                .unwrap_or("Lock the screen, then suspend to RAM"),
            "󰒲",
            Action::Suspend,
        );
        sleep.enabled = s.power.can_suspend && s.busy.is_none();
        sleep.close = true;
        roots.push(Entry::branch(menu("system", "System", vec![sleep]), "󰒲"));
    }
    menu("root", "Desktop", roots)
}
// Menu pages keep every device reachable while preserving large usable sectors.
pub fn paged(m: &Menu) -> Menu {
    if m.entries.len() <= 8 {
        return m.clone();
    }
    let mut entries = m.entries[..7].to_vec();
    entries.push(Entry::branch(
        paged(&menu(
            &format!("{}-more", m.id),
            "More",
            m.entries[7..].to_vec(),
        )),
        "…",
    ));
    menu(&m.id, &m.label, entries)
}
