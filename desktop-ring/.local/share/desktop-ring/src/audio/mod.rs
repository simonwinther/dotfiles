use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::watch;

mod native;
pub use native::Client;

// Let related server/sink/source notifications settle before taking one snapshot.
// The upper bound keeps a continuously changing server from starving refreshes.
pub(crate) async fn settle(changes: &mut watch::Receiver<bool>) {
    let quiet = tokio::time::sleep(Duration::from_millis(50));
    let limit = tokio::time::sleep(Duration::from_millis(250));
    tokio::pin!(quiet, limit);
    changes.borrow_and_update();
    loop {
        tokio::select! {
            biased;
            _ = &mut limit => break,
            _ = &mut quiet => break,
            Ok(()) = changes.changed() => {
                quiet.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(50));
            }
        }
    }
    // Only consume events preceding discovery. Changes during discovery must
    // remain pending so a hotplug or external default change cannot be lost.
    changes.borrow_and_update();
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioState {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub identity: String,
    pub label: String,
    #[serde(default)]
    pub short_label: String,
    pub available: bool,
    pub active: bool,
    pub muted: bool,
    pub volume: u32,
    #[serde(skip)]
    channels: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioAction {
    Select { input: bool, name: String },
    Volume { input: bool, delta: i32 },
    Mute { input: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn refresh_waits_for_quiet_but_bounds_continuous_changes() {
        let (tx, mut rx) = watch::channel(true);
        let start = Instant::now();
        let events = tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                tx.send(true).unwrap();
            }
        });
        settle(&mut rx).await;
        assert_eq!(start.elapsed(), Duration::from_millis(250));
        events.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_coalesces_bursts_and_leaves_later_changes_pending() {
        let (tx, mut rx) = watch::channel(true);
        let start = Instant::now();
        let events = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            for _ in 0..100 {
                events.send(true).unwrap();
            }
        });
        settle(&mut rx).await;
        assert_eq!(start.elapsed(), Duration::from_millis(80));
        assert!(!rx.has_changed().unwrap());
        tx.send(true).unwrap();
        assert!(rx.has_changed().unwrap());
    }
}
