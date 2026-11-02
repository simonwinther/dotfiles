use crate::{
    backend::Backend,
    config::private_directory,
    protocol::{self, Activation, MenuRequest, Query, QueryResponse, StateRequest, Subscription},
    providers,
};
use anyhow::{Result, ensure};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Client {
    id: u64,
    writer: Mutex<UnixStream>,
    generation: AtomicU64,
    closed: AtomicBool,
    legacy: bool,
}
struct Job {
    client: Arc<Client>,
    query: Query,
    format: u8,
    generation: u64,
    qid: i32,
}
#[derive(Default)]
struct Pending {
    order: VecDeque<u64>,
    jobs: HashMap<u64, Job>,
}
struct Pool {
    pending: Mutex<Pending>,
    ready: Condvar,
}

impl Pool {
    fn new(backend: Arc<Backend>) -> io::Result<Arc<Self>> {
        let pool = Arc::new(Self {
            pending: Mutex::new(Pending::default()),
            ready: Condvar::new(),
        });
        for index in 0..4 {
            let pool = pool.clone();
            let backend = backend.clone();
            std::thread::Builder::new()
                .name(format!("search-query-{index}"))
                .stack_size(1024 * 1024)
                .spawn(move || {
                    loop {
                        let job = {
                            let mut pending = pool.pending.lock().unwrap();
                            while pending.order.is_empty() {
                                pending = pool.ready.wait(pending).unwrap();
                            }
                            let id = pending.order.pop_front().unwrap();
                            pending.jobs.remove(&id).unwrap()
                        };
                        let cancelled = || {
                            job.client.closed.load(Ordering::Acquire)
                                || job.client.generation.load(Ordering::Acquire) != job.generation
                        };
                        if cancelled() {
                            continue;
                        }
                        let result = providers::query(&backend, &job.query, &cancelled);
                        if cancelled() {
                            continue;
                        }
                        if let Err(error) = send_results(&job, result, &cancelled) {
                            job.client.closed.store(true, Ordering::Release);
                            let _ = job
                                .client
                                .writer
                                .lock()
                                .unwrap()
                                .shutdown(std::net::Shutdown::Both);
                            eprintln!("desktop-search: query response failed: {error}");
                        }
                    }
                })?;
        }
        Ok(pool)
    }
    fn submit(&self, job: Job) {
        let mut pending = self.pending.lock().unwrap();
        if !pending.jobs.contains_key(&job.client.id) {
            pending.order.push_back(job.client.id);
        }
        pending.jobs.insert(job.client.id, job);
        self.ready.notify_one();
    }
}

fn send_results(
    job: &Job,
    result: Result<Vec<protocol::Item>>,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let items = match result {
        Ok(items) => items,
        Err(error) => {
            eprintln!("desktop-search: provider query failed: {error}");
            vec![]
        }
    };
    let mut writer = job.client.writer.lock().unwrap();
    if cancelled() {
        return Ok(());
    }
    if items.is_empty() {
        protocol::write_frame(&mut *writer, protocol::EMPTY, &[])?;
    }
    for mut item in items {
        if cancelled() {
            return Ok(());
        }
        if job.client.legacy && item.provider == "clipboard" && item.preview_type == "text" {
            item.preview =
                String::from_utf8_lossy(&std::fs::read(&item.payload_path)?).into_owned();
            if item.mimetype == "text/uri-list" {
                item.preview_type = "file".into();
            }
        }
        let response = QueryResponse {
            query: job.query.query.clone(),
            item: Some(item),
            qid: job.qid,
        };
        protocol::write_frame(
            &mut *writer,
            protocol::ITEM,
            &protocol::encode(job.format, &response)?,
        )?;
    }
    if !cancelled() {
        protocol::write_frame(&mut *writer, protocol::DONE, &[])?;
    }
    Ok(())
}

struct BoundSocket {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl BoundSocket {
    fn bind(path: &Path) -> Result<Self> {
        private_directory(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("Socket has no parent"))?,
        )?;
        if let Ok(metadata) = path.symlink_metadata() {
            ensure!(
                metadata.file_type().is_socket(),
                "Socket path is occupied by another file"
            );
            match UnixStream::connect(path) {
                Ok(_) => anyhow::bail!("Search socket is already in use"),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    std::fs::remove_file(path)?
                }
                Err(error) => return Err(error.into()),
            }
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let metadata = path.symlink_metadata()?;
        Ok(Self {
            listener,
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}
impl Drop for BoundSocket {
    fn drop(&mut self) {
        if self
            .path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn serve(
    backend: Arc<Backend>,
    compatibility: bool,
    collector: Option<UnixStream>,
) -> Result<()> {
    let canonical = BoundSocket::bind(&backend.paths.socket)?;
    let pool = Pool::new(backend.clone())?;
    let clients = Arc::new(AtomicUsize::new(0));
    if let Some(wake) = collector {
        crate::collector::start(backend.clone(), wake)?;
    }
    if compatibility {
        let backend = backend.clone();
        let pool = pool.clone();
        let clients = clients.clone();
        std::thread::Builder::new()
            .name("search-compatibility".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                loop {
                    match BoundSocket::bind(&backend.paths.compatibility_socket) {
                        Ok(socket) => {
                            let _ = accept(socket, backend.clone(), pool.clone(), clients.clone());
                        }
                        Err(_) => std::thread::sleep(Duration::from_secs(1)),
                    }
                }
            })?;
    }
    accept(canonical, backend, pool, clients)
}

fn accept(
    socket: BoundSocket,
    backend: Arc<Backend>,
    pool: Arc<Pool>,
    clients: Arc<AtomicUsize>,
) -> Result<()> {
    static IDS: AtomicU64 = AtomicU64::new(1);
    for stream in socket.listener.incoming() {
        let stream = stream?;
        if clients.load(Ordering::Relaxed) >= 32 {
            continue;
        }
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        ensure!(
            unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut credentials as *mut libc::ucred).cast(),
                    &mut size,
                )
            } == 0,
            "Unable to verify client credentials"
        );
        if credentials.uid != unsafe { libc::geteuid() } {
            continue;
        }
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let client = Arc::new(Client {
            id: IDS.fetch_add(1, Ordering::Relaxed),
            writer: Mutex::new(stream.try_clone()?),
            generation: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            legacy: socket.path == backend.paths.compatibility_socket,
        });
        let backend = backend.clone();
        let pool = pool.clone();
        let clients = clients.clone();
        clients.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("search-client".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let result = handle(stream, client.clone(), backend, pool);
                client.closed.store(true, Ordering::Release);
                let _ = client
                    .writer
                    .lock()
                    .unwrap()
                    .shutdown(std::net::Shutdown::Both);
                clients.fetch_sub(1, Ordering::Relaxed);
                if let Err(error) = result {
                    if !error.downcast_ref::<io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::BrokenPipe
                                | io::ErrorKind::ConnectionReset
                        )
                    }) {
                        eprintln!("desktop-search: client request rejected: {error}");
                    }
                }
            })?;
    }
    Ok(())
}

fn handle(
    mut stream: UnixStream,
    client: Arc<Client>,
    backend: Arc<Backend>,
    pool: Arc<Pool>,
) -> Result<()> {
    static QUERY_IDS: AtomicU64 = AtomicU64::new(1);
    while let Some((kind, format, body)) = protocol::read_request(&mut stream)? {
        match kind {
            protocol::QUERY => {
                let query: Query = protocol::decode(format, &body)?;
                ensure!(
                    query.query.len() <= 4096 && query.providers.len() <= 32,
                    "Query exceeds its limit"
                );
                let generation = client.generation.fetch_add(1, Ordering::AcqRel) + 1;
                pool.submit(Job {
                    client: client.clone(),
                    query,
                    format,
                    generation,
                    qid: QUERY_IDS.fetch_add(1, Ordering::Relaxed) as i32,
                });
            }
            protocol::ACTIVATE => {
                let action: Activation = protocol::decode(format, &body)?;
                if action.provider == "desktop-search" && action.action == "import-elephant" {
                    let result =
                        crate::migration::import(&backend, &backend.paths.compatibility_socket);
                    let value = match result {
                        Ok(imported) => serde_json::json!({"imported":imported}),
                        Err(error) => serde_json::json!({"error":error.to_string()}),
                    };
                    protocol::write_frame(
                        &mut *client.writer.lock().unwrap(),
                        protocol::ACTIVATED,
                        &serde_json::to_vec(&value)?,
                    )?;
                    continue;
                }
                let result = providers::activate(&backend, &action);
                let mut writer = client.writer.lock().unwrap();
                if let Err(error) = result {
                    eprintln!("desktop-search: provider action failed: {error}");
                    // Existing clients accept an empty activation frame; native
                    // clients can additionally report a structured error.
                    let body = if format == 1 {
                        serde_json::to_vec(&serde_json::json!({"error":error.to_string()}))?
                    } else {
                        vec![]
                    };
                    protocol::write_frame(&mut *writer, protocol::ACTIVATED, &body)?;
                } else {
                    protocol::write_frame(&mut *writer, protocol::ACTIVATED, &[])?;
                }
            }
            protocol::STATE => {
                let request: StateRequest = protocol::decode(format, &body)?;
                let state = providers::state(&backend, &request.provider)?;
                let mut writer = client.writer.lock().unwrap();
                protocol::write_frame(
                    &mut *writer,
                    protocol::PROVIDER_STATE,
                    &protocol::encode(format, &state)?,
                )?;
                protocol::write_frame(&mut *writer, protocol::STATUS_DONE, &[])?;
            }
            protocol::SUBSCRIBE => {
                return subscribe(stream, &backend, protocol::decode(format, &body)?, format);
            }
            protocol::MENU => {
                let menu: MenuRequest = protocol::decode(format, &body)?;
                backend.events.notify(&format!("menus:{}", menu.menu));
            }
            _ => anyhow::bail!("Unsupported request"),
        }
    }
    Ok(())
}

fn subscribe(
    mut stream: UnixStream,
    backend: &Backend,
    subscription: Subscription,
    format: u8,
) -> Result<()> {
    ensure!(
        subscription.provider.len() <= 256 && subscription.query.len() <= 4096,
        "Subscription exceeds its limit"
    );
    let listener = backend.events.listen(subscription.provider.clone())?;
    let interval = (subscription.interval > 0)
        .then(|| Duration::from_millis(subscription.interval.max(250) as u64));
    let mut next = Instant::now() + interval.unwrap_or(Duration::from_secs(86400));
    let mut fingerprint = None;
    loop {
        let timeout = interval
            .map(|_| {
                next.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i32::MAX as u128) as i32
            })
            .unwrap_or(-1);
        let mut fds = [
            libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN | libc::POLLRDHUP,
                revents: 0,
            },
            libc::pollfd {
                fd: listener.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error().into());
        }
        if fds[0].revents != 0 {
            let mut byte = [0];
            if stream.read(&mut byte)? == 0 {
                return Ok(());
            }
            anyhow::bail!("A subscription must use its own connection");
        }
        let mut value = listener.take();
        if let Some(interval) = interval
            && Instant::now() >= next
        {
            let entries = providers::query(
                backend,
                &Query {
                    providers: vec![subscription.provider.clone()],
                    query: subscription.query.clone(),
                    maxresults: 256,
                    ..Default::default()
                },
                &|| false,
            )?;
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut hash = DefaultHasher::new();
            serde_json::to_vec(&entries)?.hash(&mut hash);
            let current = hash.finish();
            if fingerprint.is_some_and(|previous| previous != current) {
                value = Some(String::new());
            }
            fingerprint = Some(current);
            next = Instant::now() + interval;
        }
        if let Some(value) = value {
            protocol::write_frame(
                &mut stream,
                0,
                &protocol::encode(format, &protocol::Notification { value })?,
            )?;
        }
    }
}
