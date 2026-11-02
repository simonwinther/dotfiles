use crate::{
    applications::Catalog,
    config::{Config, Paths},
    events::Events,
    store::Store,
};
use anyhow::Result;
use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
};

pub struct Backend {
    pub config: Config,
    pub paths: Paths,
    pub store: Mutex<Store>,
    pub applications: Mutex<Catalog>,
    pub events: Events,
    pub paused: AtomicBool,
    pub position: AtomicI64,
    pub capture_generation: AtomicU64,
    pub wake: Mutex<UnixStream>,
}

impl Backend {
    pub fn new(paths: Paths, config: Config) -> Result<(Arc<Self>, UnixStream)> {
        let store = Store::open(
            &paths.state,
            config.max_items,
            config.max_payload_mib * 1024 * 1024,
        )?;
        let paused = store.setting("paused")? == "true";
        let position = store.db.query_row(
            "SELECT MAX(COALESCE(MAX(position),0),0)+1 FROM clips",
            [],
            |row| row.get(0),
        )?;
        let (writer, reader) = UnixStream::pair()?;
        writer.set_nonblocking(true)?;
        reader.set_nonblocking(true)?;
        Ok((
            Arc::new(Self {
                config,
                paths,
                store: Mutex::new(store),
                applications: Mutex::new(Catalog::default()),
                events: Events::default(),
                paused: AtomicBool::new(paused),
                position: AtomicI64::new(position),
                capture_generation: AtomicU64::new(0),
                wake: Mutex::new(writer),
            }),
            reader,
        ))
    }

    pub fn pause(&self, paused: bool) -> Result<()> {
        let store = self.store.lock().unwrap();
        store.set_setting("paused", if paused { "true" } else { "false" })?;
        self.paused.store(paused, Ordering::Release);
        self.capture_generation.fetch_add(1, Ordering::AcqRel);
        drop(store);
        let _ = self.wake.lock().unwrap().write(&[1]);
        self.events.notify("clipboard");
        Ok(())
    }
}
