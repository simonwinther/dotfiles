//! Clipboard offers are received on Wayland and streamed to private files.
use crate::{
    backend::Backend,
    config::private_directory,
    store::{Capture, supported_mime},
};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::OpenOptionsExt, net::UnixStream},
    },
    sync::{
        Arc, Mutex,
        atomic::Ordering,
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::ObjectId,
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
    ext_data_control_offer_v1 as ext_offer,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as wlr_device, zwlr_data_control_manager_v1 as wlr_manager,
    zwlr_data_control_offer_v1 as wlr_offer,
};

struct Transfer {
    reader: OwnedFd,
    mime: String,
    position: i64,
    created: Instant,
    generation: u64,
}
struct Transfers {
    sender: SyncSender<Transfer>,
    wake: Mutex<UnixStream>,
}
struct Collector {
    backend: Arc<Backend>,
    offers: HashMap<ObjectId, Vec<String>>,
    seats: HashMap<u32, wl_seat::WlSeat>,
    ext: Option<ext_manager::ExtDataControlManagerV1>,
    wlr: Option<wlr_manager::ZwlrDataControlManagerV1>,
    transfers: Arc<Transfers>,
    ready: bool,
    finished: bool,
}

pub fn start(backend: Arc<Backend>, mut wake: UnixStream) -> io::Result<()> {
    let transfers = pipeline(backend.clone())?;
    std::thread::Builder::new()
        .name("search-wayland".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            loop {
                if let Err(error) = run(backend.clone(), transfers.clone(), &mut wake) {
                    eprintln!("desktop-search: clipboard connection unavailable: {error}");
                    let mut fd = libc::pollfd {
                        fd: wake.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    unsafe {
                        libc::poll(&mut fd, 1, 3000);
                    }
                    let _ = wake.read(&mut [0; 256]);
                }
            }
        })?;
    Ok(())
}

fn pipeline(backend: Arc<Backend>) -> io::Result<Arc<Transfers>> {
    let (sender, receiver) = mpsc::sync_channel(128);
    let (writer, reader) = UnixStream::pair()?;
    writer.set_nonblocking(true)?;
    reader.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("search-clipboard".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            if let Err(error) = receive_all(&backend, receiver, reader) {
                eprintln!("desktop-search: clipboard transfers stopped: {error}");
            }
        })?;
    Ok(Arc::new(Transfers {
        sender,
        wake: Mutex::new(writer),
    }))
}

impl Transfers {
    fn send(&self, transfer: Transfer) {
        if self.sender.try_send(transfer).is_ok() {
            let _ = self.wake.lock().unwrap().write(&[1]);
        } else {
            eprintln!("desktop-search: clipboard capture queue is full");
        }
    }
}

struct Incoming {
    transfer: Transfer,
    file: File,
    size: u64,
}
impl Incoming {
    fn new(backend: &Backend, transfer: Transfer) -> Result<Self> {
        let directory = backend.paths.state.join("incoming");
        private_directory(&directory)?;
        let path = directory.join(format!("{}-{}", std::process::id(), transfer.position));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        // Transfer files are unlinked immediately, including on process failure.
        std::fs::remove_file(path)?;
        let fd = transfer.reader.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
            "Unable to configure clipboard pipe"
        );
        Ok(Self {
            transfer,
            file,
            size: 0,
        })
    }
    fn read(&mut self, backend: &Backend) -> Result<bool> {
        ensure!(
            self.transfer.created.elapsed() < Duration::from_secs(8),
            "Clipboard transfer timed out"
        );
        let mut buffer = [0; 16 * 1024];
        // Give every ready source a turn even while another offers a large file.
        for _ in 0..4 {
            let count = unsafe {
                libc::read(
                    self.transfer.reader.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if count == 0 {
                return Ok(true);
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(false);
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            self.size += count as u64;
            ensure!(
                self.size <= backend.config.max_payload_mib * 1024 * 1024,
                "Clipboard payload exceeds the configured limit"
            );
            self.file.write_all(&buffer[..count as usize])?;
        }
        Ok(false)
    }
    fn save(mut self, backend: &Backend) -> Result<()> {
        if self.size == 0 {
            return Ok(());
        }
        self.file.rewind()?;
        let mut store = backend.store.lock().unwrap();
        if backend.paused.load(Ordering::Acquire)
            || self.transfer.generation != backend.capture_generation.load(Ordering::Acquire)
        {
            return Ok(());
        }
        store.capture(Capture {
            mime: &self.transfer.mime,
            reader: &mut self.file,
            identifier: None,
            pinned: false,
            label: None,
            import_position: Some(self.transfer.position),
        })?;
        drop(store);
        backend.events.notify("clipboard");
        Ok(())
    }
}

fn receive_all(
    backend: &Backend,
    receiver: Receiver<Transfer>,
    mut wake: UnixStream,
) -> Result<()> {
    let mut active: Vec<Incoming> = vec![];
    loop {
        for transfer in receiver.try_iter() {
            if backend.paused.load(Ordering::Acquire)
                || transfer.generation != backend.capture_generation.load(Ordering::Acquire)
            {
                continue;
            }
            if active.len() == 128 {
                active.remove(0);
            }
            match Incoming::new(backend, transfer) {
                Ok(incoming) => active.push(incoming),
                Err(error) => eprintln!("desktop-search: clipboard transfer rejected: {error}"),
            }
        }
        let timeout = active
            .iter()
            .map(|incoming| {
                (incoming.transfer.created + Duration::from_secs(8))
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(8000) as i32
            })
            .min()
            .unwrap_or(-1);
        let mut fds: Vec<_> = std::iter::once(libc::pollfd {
            fd: wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .chain(active.iter().map(|incoming| libc::pollfd {
            fd: incoming.transfer.reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }))
        .collect();
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error().into());
        }
        if fds[0].revents != 0 {
            loop {
                match wake.read(&mut [0; 256]) {
                    Ok(0) => return Ok(()),
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.into()),
                }
            }
        }
        for index in (0..active.len()).rev() {
            if backend.paused.load(Ordering::Acquire)
                || active[index].transfer.generation
                    != backend.capture_generation.load(Ordering::Acquire)
            {
                active.swap_remove(index);
                continue;
            }
            if fds[index + 1].revents == 0
                && active[index].transfer.created.elapsed() < Duration::from_secs(8)
            {
                continue;
            }
            match active[index].read(backend) {
                Ok(false) => {}
                Ok(true) => {
                    if let Err(error) = active.swap_remove(index).save(backend) {
                        eprintln!("desktop-search: clipboard capture rejected: {error}");
                    }
                }
                Err(error) => {
                    active.swap_remove(index);
                    eprintln!("desktop-search: clipboard transfer rejected: {error}");
                }
            }
        }
    }
}

fn run(backend: Arc<Backend>, transfers: Arc<Transfers>, wake: &mut UnixStream) -> Result<()> {
    let connection = Connection::connect_to_env()?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let _registry = connection.display().get_registry(&qh, ());
    let mut state = Collector {
        backend,
        offers: HashMap::new(),
        seats: HashMap::new(),
        ext: None,
        wlr: None,
        transfers,
        ready: false,
        finished: false,
    };
    queue.roundtrip(&mut state)?;
    ensure!(
        state.ext.is_some() || state.wlr.is_some(),
        "Compositor does not support clipboard data control"
    );
    state.ready = true;
    for seat in state.seats.values() {
        state.device(seat, &qh);
    }
    loop {
        queue.dispatch_pending(&mut state)?;
        ensure!(!state.finished, "Clipboard data device ended");
        connection.flush()?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fds = [
            libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error().into());
        }
        if fds[1].revents != 0 {
            drop(guard);
            while wake.read(&mut [0; 256]).is_ok_and(|n| n > 0) {}
            return Ok(());
        }
        guard.read()?;
    }
}

impl Collector {
    fn device(&self, seat: &wl_seat::WlSeat, qh: &QueueHandle<Self>) {
        if let Some(manager) = &self.ext {
            manager.get_data_device(seat, qh, ());
        } else if let Some(manager) = &self.wlr {
            manager.get_data_device(seat, qh, ());
        }
    }
    fn transfer(&mut self, id: ObjectId, receive: impl FnOnce(String, &OwnedFd)) {
        let Some(mimes) = self.offers.remove(&id) else {
            return;
        };
        if self.backend.paused.load(Ordering::Acquire) {
            return;
        }
        let Some(mime) = choose_mime(&mimes) else {
            return;
        };
        let mut fds = [-1; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
            return;
        }
        let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        let position = self.backend.position.fetch_add(1, Ordering::Relaxed);
        receive(mime.clone(), &writer);
        drop(writer);
        self.transfers.send(Transfer {
            reader,
            mime,
            position,
            created: Instant::now(),
            generation: self.backend.capture_generation.load(Ordering::Acquire),
        });
    }
}

fn choose_mime(mimes: &[String]) -> Option<String> {
    if mimes.iter().any(|mime| {
        mime == "x-kde-passwordManagerHint"
            || mime.contains("keepass")
            || mime.contains("password-manager")
    }) {
        return None;
    }
    [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/tiff",
        "text/uri-list",
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "text/plain",
    ]
    .iter()
    .find_map(|wanted| mimes.iter().find(|mime| mime.as_str() == *wanted).cloned())
    .or_else(|| mimes.iter().find(|mime| supported_mime(mime)).cloned())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Collector {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "ext_data_control_manager_v1" => {
                    state.ext = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "zwlr_data_control_manager_v1" => {
                    state.wlr = Some(registry.bind(name, version.min(2), qh, ()))
                }
                "wl_seat" => {
                    let seat = registry.bind(name, version.min(8), qh, ());
                    if state.ready {
                        state.device(&seat, qh);
                    }
                    state.seats.insert(name, seat);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(seat) = state.seats.remove(&name) {
                    if seat.version() >= 5 {
                        seat.release();
                    }
                }
            }
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(Collector: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(Collector: ext_manager::ExtDataControlManagerV1);
wayland_client::delegate_noop!(Collector: wlr_manager::ZwlrDataControlManagerV1);

impl Dispatch<ext_offer::ExtDataControlOfferV1, ()> for Collector {
    fn event(
        state: &mut Self,
        offer: &ext_offer::ExtDataControlOfferV1,
        event: ext_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_offer::Event::Offer { mime_type } = event {
            state.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}
impl Dispatch<wlr_offer::ZwlrDataControlOfferV1, ()> for Collector {
    fn event(
        state: &mut Self,
        offer: &wlr_offer::ZwlrDataControlOfferV1,
        event: wlr_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wlr_offer::Event::Offer { mime_type } = event {
            state.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}
impl Dispatch<ext_device::ExtDataControlDeviceV1, ()> for Collector {
    fn event(
        state: &mut Self,
        device: &ext_device::ExtDataControlDeviceV1,
        event: ext_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_device::Event::Selection { id: Some(offer) } => {
                state.transfer(offer.id(), |mime, fd| offer.receive(mime, fd.as_fd()));
                offer.destroy();
            }
            ext_device::Event::PrimarySelection { id: Some(offer) } => {
                state.offers.remove(&offer.id());
                offer.destroy();
            }
            ext_device::Event::Finished => {
                device.destroy();
                state.finished = true;
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(Collector,ext_device::ExtDataControlDeviceV1,[0=>(ext_offer::ExtDataControlOfferV1,())]);
}
impl Dispatch<wlr_device::ZwlrDataControlDeviceV1, ()> for Collector {
    fn event(
        state: &mut Self,
        device: &wlr_device::ZwlrDataControlDeviceV1,
        event: wlr_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wlr_device::Event::Selection { id: Some(offer) } => {
                state.transfer(offer.id(), |mime, fd| offer.receive(mime, fd.as_fd()));
                offer.destroy();
            }
            wlr_device::Event::PrimarySelection { id: Some(offer) } => {
                state.offers.remove(&offer.id());
                offer.destroy();
            }
            wlr_device::Event::Finished => {
                device.destroy();
                state.finished = true;
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(Collector,wlr_device::ZwlrDataControlDeviceV1,[0=>(wlr_offer::ZwlrDataControlOfferV1,())]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_prefers_images_and_ignores_password_offers() {
        let mimes = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            choose_mime(&mimes(&["text/plain", "image/png"])),
            Some("image/png".into())
        );
        assert_eq!(
            choose_mime(&mimes(&["text/plain", "text/uri-list"])),
            Some("text/uri-list".into())
        );
        assert!(choose_mime(&mimes(&["x-kde-passwordManagerHint", "text/plain"])).is_none());
    }

    #[test]
    fn stalled_sources_do_not_block_new_copies_or_cross_pause_boundaries() {
        use crate::config::{Config, Paths};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_path_buf();
        let paths = Paths {
            config: path.clone(),
            state: path.join("state"),
            socket: path.join("search.sock"),
            compatibility_socket: path.join("legacy.sock"),
            home: path.clone(),
            data: path.clone(),
            cache: path,
        };
        let (backend, _wake) = Backend::new(paths, Config::default()).unwrap();
        let transfers = pipeline(backend.clone()).unwrap();
        let pipe = |position, generation| {
            let mut fds = [-1; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
            let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
            let writer = unsafe { File::from_raw_fd(fds[1]) };
            transfers.send(Transfer {
                reader,
                mime: "text/plain".into(),
                position,
                created: Instant::now(),
                generation,
            });
            writer
        };
        let first = pipe(1, 0);
        let second = pipe(2, 0);
        let mut quick = pipe(3, 0);
        quick.write_all(b"quick copy").unwrap();
        drop(quick);
        let deadline = Instant::now() + Duration::from_secs(1);
        while backend
            .store
            .lock()
            .unwrap()
            .list("quick", 10, "")
            .unwrap()
            .is_empty()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            backend
                .store
                .lock()
                .unwrap()
                .list("quick", 10, "")
                .unwrap()
                .len(),
            1
        );
        let mut stale = pipe(4, 0);
        backend.pause(true).unwrap();
        backend.pause(false).unwrap();
        let _ = stale.write_all(b"must not enter history");
        drop(stale);
        let mut current = pipe(5, backend.capture_generation.load(Ordering::Acquire));
        current.write_all(b"after resume").unwrap();
        drop(current);
        while backend
            .store
            .lock()
            .unwrap()
            .list("resume", 10, "")
            .unwrap()
            .is_empty()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            backend
                .store
                .lock()
                .unwrap()
                .list("resume", 10, "")
                .unwrap()
                .len(),
            1
        );
        assert!(
            backend
                .store
                .lock()
                .unwrap()
                .list("must", 10, "")
                .unwrap()
                .is_empty()
        );
        drop(first);
        drop(second);
        drop(transfers);
        while Arc::strong_count(&backend) > 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(Arc::strong_count(&backend), 1);
    }
}
