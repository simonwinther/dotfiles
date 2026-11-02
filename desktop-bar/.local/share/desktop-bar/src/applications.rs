//! Warm the application catalog independently of interactive desktop queries.
use crate::{
    modules::Update,
    shell::{Entry, Reply},
};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::{
    process::Command,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    entries: Option<(Instant, Vec<Entry>)>,
    requested: Option<u64>,
    loading: bool,
}
pub struct Catalog {
    state: Arc<Mutex<State>>,
    jobs: mpsc::Sender<()>,
    replies: Sender<Update>,
}
impl Catalog {
    pub fn start(replies: Sender<Update>) -> Self {
        Self::with_loader(replies, || {
            let load = || {
                if crate::theme::runtime()
                    .join("desktop-search/search.sock")
                    .exists()
                    && let Ok(output) = crate::process::output(
                        Command::new("desktop-search").args(["catalog", "apps"]),
                        Duration::from_secs(3),
                        8 * 1024 * 1024,
                        || false,
                    )
                    && output.status.success()
                {
                    return serde_json::from_slice::<Vec<Entry>>(&output.stdout)
                        .map_err(|error| error.to_string());
                }
                crate::process::output(
                    Command::new("desktopctl").args(["bar", "catalog", "apps"]),
                    Duration::from_secs(8),
                    8 * 1024 * 1024,
                    || false,
                )
                .map_err(|error| error.to_string())
                .and_then(|output| {
                    if !output.status.success() {
                        return Err("Application catalog unavailable".into());
                    }
                    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
                })
            };
            load().map(|mut entries| {
                entries.extend(crate::search::launcher_entries());
                entries
            })
        })
    }
    fn with_loader(
        replies: Sender<Update>,
        mut load: impl FnMut() -> Result<Vec<Entry>, String> + Send + 'static,
    ) -> Self {
        let state = Arc::new(Mutex::new(State {
            loading: true,
            ..Default::default()
        }));
        let shared = state.clone();
        let sender = replies.clone();
        let (jobs, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("bar-applications".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    let result = load();
                    let requested = {
                        let mut state = shared.lock().unwrap();
                        if let Ok(entries) = &result {
                            state.entries = Some((Instant::now(), entries.clone()));
                        }
                        state.loading = false;
                        state.requested.take()
                    };
                    if let Some(id) = requested
                        && sender
                            .send(Update::Shell(Reply::Catalog(id, result)))
                            .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("application catalog worker");
        let _ = jobs.send(());
        Self {
            state,
            jobs,
            replies,
        }
    }
    pub fn request(&self, id: u64) {
        let mut state = self.state.lock().unwrap();
        if let Some((loaded, entries)) = &state.entries
            && loaded.elapsed() < Duration::from_secs(60)
        {
            let _ = self
                .replies
                .send(Update::Shell(Reply::Catalog(id, Ok(entries.clone()))));
        } else {
            state.requested = Some(id);
            if !state.loading {
                state.loading = true;
                let _ = self.jobs.send(());
            }
        }
    }
    pub fn cached(&self) -> Option<Vec<Entry>> {
        self.state
            .lock()
            .unwrap()
            .entries
            .as_ref()
            .map(|(_, entries)| entries.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warmup_shares_inflight_work_and_expired_results_survive_failed_refreshes() {
        let (sender, _receiver) = smithay_client_toolkit::reexports::calloop::channel::channel();
        let (started, starts) = mpsc::channel();
        let (finish, finishes) = mpsc::channel();
        let catalog = Catalog::with_loader(sender, move || {
            started.send(()).unwrap();
            finishes.recv().unwrap()
        });
        starts.recv_timeout(Duration::from_secs(1)).unwrap();
        catalog.request(1);
        catalog.request(2);
        assert_eq!(catalog.state.lock().unwrap().requested, Some(2));
        assert!(starts.try_recv().is_err());
        finish
            .send(Ok(vec![Entry {
                id: "original".into(),
                ..Default::default()
            }]))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while catalog.state.lock().unwrap().loading {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        catalog.request(3);
        assert!(starts.try_recv().is_err());
        catalog.state.lock().unwrap().entries.as_mut().unwrap().0 -= Duration::from_secs(61);
        catalog.request(4);
        starts.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(catalog.cached().unwrap()[0].id, "original");
        finish.send(Err("Offline".into())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while catalog.state.lock().unwrap().loading {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(catalog.cached().unwrap()[0].id, "original");
    }
}
