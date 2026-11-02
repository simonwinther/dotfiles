//! Preserve user action order without starting a thread for every wheel event.
use std::{
    process::Command,
    sync::{OnceLock, mpsc},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Set { microphone: bool, percent: u32 },
    Adjust { microphone: bool, percent: i32 },
    Mute { microphone: bool },
}
impl Action {
    fn replaces(self, previous: Self) -> bool {
        matches!((self, previous), (Self::Set { microphone: a, .. }, Self::Set { microphone: b, .. }) if a == b)
    }
    fn arguments(self) -> [String; 3] {
        let (microphone, operation, value) = match self {
            Self::Set {
                microphone,
                percent,
            } => (microphone, "set-volume", format!("{percent}%")),
            Self::Adjust {
                microphone,
                percent,
            } => (
                microphone,
                "set-volume",
                format!(
                    "{}%{}",
                    percent.unsigned_abs(),
                    if percent < 0 { "-" } else { "+" }
                ),
            ),
            Self::Mute { microphone } => (microphone, "set-mute", "toggle".into()),
        };
        [
            operation.into(),
            if microphone {
                "@DEFAULT_SOURCE@"
            } else {
                "@DEFAULT_SINK@"
            }
            .into(),
            value,
        ]
    }
}

pub fn send(action: Action) {
    static WORKER: OnceLock<mpsc::Sender<Action>> = OnceLock::new();
    let sender = WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Action>();
        std::thread::Builder::new()
            .name("bar-audio-actions".into())
            .spawn(move || {
                crate::action_queue::run(
                    receiver,
                    |previous, newer| newer.replaces(*previous),
                    |action| {
                        let result = crate::process::output(
                            Command::new("wpctl").args(action.arguments()),
                            Duration::from_secs(3),
                            4096,
                            || false,
                        );
                        if let Err(error) = result.and_then(|output| {
                            if output.status.success() {
                                Ok(())
                            } else {
                                Err(std::io::Error::other("Audio command failed"))
                            }
                        }) {
                            eprintln!("desktop-bar: {error}");
                        }
                        true
                    },
                );
            })
            .expect("audio action worker");
        sender
    });
    let _ = sender.send(action);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slider_bursts_preserve_device_changes_wheel_steps_and_mute_order() {
        let (sender, receiver) = mpsc::channel();
        let set = |microphone, percent| Action::Set {
            microphone,
            percent,
        };
        let up = Action::Adjust {
            microphone: false,
            percent: 10,
        };
        let down = Action::Adjust {
            microphone: false,
            percent: -10,
        };
        let mute = Action::Mute { microphone: false };
        for action in [
            set(false, 10),
            set(false, 20),
            up,
            down,
            mute,
            set(true, 10),
            set(true, 20),
            set(false, 30),
        ] {
            sender.send(action).unwrap();
        }
        drop(sender);
        let mut applied = Vec::new();
        crate::action_queue::run(
            receiver,
            |previous, newer| newer.replaces(*previous),
            |action| {
                applied.push(action);
                true
            },
        );
        assert_eq!(
            applied,
            [
                set(false, 20),
                up,
                down,
                mute,
                set(true, 20),
                set(false, 30)
            ]
        );
        assert_eq!(up.arguments(), ["set-volume", "@DEFAULT_SINK@", "10%+"]);
        assert_eq!(down.arguments(), ["set-volume", "@DEFAULT_SINK@", "10%-"]);
    }
}
