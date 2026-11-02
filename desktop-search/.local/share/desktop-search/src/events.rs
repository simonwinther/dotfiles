use std::{
    collections::HashMap,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Default)]
pub struct Events {
    next: AtomicU64,
    listeners: Mutex<HashMap<u64, Weak<Listener>>>,
}

pub struct Listener {
    pub provider: String,
    reader: Mutex<UnixStream>,
    writer: Mutex<UnixStream>,
    value: Mutex<Option<String>>,
}

impl Events {
    pub fn listen(&self, provider: String) -> io::Result<Arc<Listener>> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        let listener = Arc::new(Listener {
            provider,
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            value: Mutex::new(None),
        });
        let mut listeners = self.listeners.lock().unwrap();
        listeners.retain(|_, listener| listener.strong_count() > 0);
        listeners.insert(
            self.next.fetch_add(1, Ordering::Relaxed),
            Arc::downgrade(&listener),
        );
        Ok(listener)
    }

    pub fn notify(&self, provider: &str) {
        let base = provider.split(':').next().unwrap_or(provider);
        self.listeners.lock().unwrap().retain(|_, listener| {
            if let Some(listener) = listener.upgrade() {
                if listener.provider == provider || listener.provider == base {
                    *listener.value.lock().unwrap() = Some(provider.into());
                    let _ = listener.writer.lock().unwrap().write(&[1]);
                }
                true
            } else {
                false
            }
        });
    }
}

impl Listener {
    pub fn fd(&self) -> RawFd {
        self.reader.lock().unwrap().as_raw_fd()
    }
    pub fn take(&self) -> Option<String> {
        let mut reader = self.reader.lock().unwrap();
        let mut bytes = [0; 256];
        while reader.read(&mut bytes).is_ok_and(|size| size > 0) {}
        self.value.lock().unwrap().take()
    }
}
