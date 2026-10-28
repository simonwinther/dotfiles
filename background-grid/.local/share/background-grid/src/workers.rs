use background_grid::catalog::{Folder, catalog_with_cancel};
use background_grid::config::Paths;
use background_grid::thumbnail::thumbnail;
use background_grid::wallpaper::set_wallpaper;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

const PREVIEW_WORKERS: usize = 2;
const MAX_PENDING_PREVIEWS: usize = 32;
type Outcome<T> = std::result::Result<T, String>;
type Preview = (usize, PathBuf);

pub struct Collection {
    pub folder: Folder,
    pub active: PathBuf,
    pub count: usize,
}

pub enum Event {
    Catalog(Outcome<Collection>),
    Preview(usize, Outcome<PathBuf>),
    Applied(PathBuf, bool, Outcome<()>),
}

#[derive(Default)]
struct Pending {
    previews: VecDeque<Preview>,
    decoding: HashSet<usize>,
    ready: VecDeque<(usize, Outcome<PathBuf>)>,
    apply: Option<(PathBuf, bool)>,
    applying: bool,
    control: Option<Event>,
}

#[derive(Default)]
struct Queue {
    pending: Mutex<Pending>,
    changed: Condvar,
    stopped: AtomicBool,
}

impl Queue {
    fn previews(&self, previews: impl IntoIterator<Item = Preview>) {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        // Replace obsolete viewport requests, retaining only a bounded batch.
        // Completed jobs remain excluded until GTK consumes their results.
        let mut seen = pending.decoding.clone();
        seen.extend(pending.ready.iter().map(|(id, _)| *id));
        pending.previews = previews
            .into_iter()
            .filter(|(id, _)| seen.insert(*id))
            .take(MAX_PENDING_PREVIEWS)
            .collect();
        self.changed.notify_all();
    }

    fn next_preview(&self) -> Option<Preview> {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        loop {
            if self.stopped.load(Ordering::Acquire) {
                return None;
            }
            if let Some(preview) = pending.previews.pop_front() {
                pending.decoding.insert(preview.0);
                return Some(preview);
            }
            pending = self.changed.wait(pending).expect("worker queue poisoned");
        }
    }

    fn preview_ready(&self, id: usize, result: Outcome<PathBuf>) {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        // Backpressure also bounds completed results when GTK is busy.
        while pending.ready.len() >= PREVIEW_WORKERS && !self.stopped.load(Ordering::Acquire) {
            pending = self.changed.wait(pending).expect("worker queue poisoned");
        }
        pending.decoding.remove(&id);
        if !self.stopped.load(Ordering::Acquire) {
            pending.ready.push_back((id, result));
        }
    }

    fn apply(&self, path: PathBuf, keep_open: bool) -> bool {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        if self.stopped.load(Ordering::Acquire)
            || pending.applying
            || pending.apply.is_some()
            || pending.control.is_some()
        {
            return false;
        }
        pending.apply = Some((path, keep_open));
        self.changed.notify_all();
        true
    }

    fn next_apply(&self) -> Option<(PathBuf, bool)> {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        loop {
            // An accepted wallpaper change must finish, even when closing.
            if let Some(apply) = pending.apply.take() {
                pending.applying = true;
                return Some(apply);
            }
            if self.stopped.load(Ordering::Acquire) {
                return None;
            }
            pending = self.changed.wait(pending).expect("worker queue poisoned");
        }
    }

    fn events(&self) -> Vec<Event> {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        // Wallpaper and scan results cannot wait behind thumbnail results.
        let mut events: Vec<_> = pending.control.take().into_iter().collect();
        events.extend(
            pending
                .ready
                .drain(..)
                .map(|(id, result)| Event::Preview(id, result)),
        );
        self.changed.notify_all();
        events
    }

    fn stop(&self) {
        let mut pending = self.pending.lock().expect("worker queue poisoned");
        self.stopped.store(true, Ordering::Release);
        pending.previews.clear();
        self.changed.notify_all();
    }
}

pub struct Workers {
    queue: Arc<Queue>,
    handles: Vec<JoinHandle<()>>,
}

impl Workers {
    pub fn new(paths: &Paths) -> Self {
        let (scan_paths, preview_paths, apply_paths) =
            (paths.clone(), paths.clone(), paths.clone());
        Self::start(
            move |cancelled| {
                let folder = scan_paths
                    .folders()
                    .and_then(|roots| {
                        catalog_with_cancel(&roots, || cancelled.load(Ordering::Acquire))
                    })
                    .map_err(|error| error.to_string())?;
                Ok(Collection {
                    count: folder.all_images().len(),
                    folder,
                    active: scan_paths.current().canonicalize().unwrap_or_default(),
                })
            },
            move |path| thumbnail(path, &preview_paths.cache).map_err(|error| error.to_string()),
            move |path| set_wallpaper(&apply_paths, path, None).map_err(|error| error.to_string()),
        )
    }

    pub(super) fn start(
        scan: impl FnOnce(&AtomicBool) -> Outcome<Collection> + Send + 'static,
        preview: impl Fn(&Path) -> Outcome<PathBuf> + Send + Sync + 'static,
        apply: impl Fn(&Path) -> Outcome<()> + Send + 'static,
    ) -> Self {
        let queue = Arc::new(Queue::default());
        let mut handles = Vec::new();
        let preview = Arc::new(preview);
        for _ in 0..PREVIEW_WORKERS {
            let (queue, preview) = (queue.clone(), preview.clone());
            handles.push(thread::spawn(move || {
                while let Some((id, path)) = queue.next_preview() {
                    queue.preview_ready(id, preview(&path));
                }
            }));
        }
        let control = queue.clone();
        handles.push(thread::spawn(move || {
            let result = scan(&control.stopped);
            control
                .pending
                .lock()
                .expect("worker queue poisoned")
                .control = Some(Event::Catalog(result));
            // The catalog runs once, before any image can be selected. This
            // dedicated worker then handles applications without decoding work.
            while let Some((path, keep_open)) = control.next_apply() {
                let result = apply(&path);
                let mut pending = control.pending.lock().expect("worker queue poisoned");
                pending.control = Some(Event::Applied(path, keep_open, result));
                pending.applying = false;
            }
        }));
        Self { queue, handles }
    }

    pub fn previews(&self, previews: impl IntoIterator<Item = Preview>) {
        self.queue.previews(previews);
    }

    pub fn apply(&self, path: PathBuf, keep_open: bool) -> bool {
        self.queue.apply(path, keep_open)
    }

    pub fn events(&self) -> Vec<Event> {
        self.queue.events()
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.queue.stop();
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn collection() -> Outcome<Collection> {
        Ok(Collection {
            folder: Folder::default(),
            active: PathBuf::new(),
            count: 0,
        })
    }

    fn wait_event(workers: &Workers, matches: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(event) = workers.events().into_iter().find(&matches) {
                return event;
            }
            assert!(Instant::now() < deadline, "worker event timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[derive(Default)]
    struct Gate(Mutex<bool>, Condvar);

    impl Gate {
        fn wait(&self) {
            let guard = self.0.lock().unwrap();
            let (guard, _) = self
                .1
                .wait_timeout_while(guard, Duration::from_secs(5), |open| !*open)
                .unwrap();
            assert!(*guard, "test gate timed out");
        }

        fn open(&self) {
            *self.0.lock().unwrap() = true;
            self.1.notify_all();
        }
    }

    struct Release(Arc<Gate>);

    impl Drop for Release {
        fn drop(&mut self) {
            self.0.open();
        }
    }

    #[test]
    fn scanning_is_asynchronous_and_receives_shutdown_cancellation() {
        let (started, scanning) = mpsc::channel();
        let workers = Workers::start(
            move |cancelled| {
                started.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !cancelled.load(Ordering::Acquire) {
                    assert!(Instant::now() < deadline, "scan was not cancelled");
                    thread::yield_now();
                }
                Err("cancelled".into())
            },
            |_| unreachable!(),
            |_| unreachable!(),
        );
        scanning.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(workers.events().is_empty());
        let queue = workers.queue.clone();
        drop(workers);
        assert!(queue.stopped.load(Ordering::Acquire));
        assert!(
            matches!(queue.events().as_slice(), [Event::Catalog(Err(error))] if error == "cancelled")
        );
    }

    #[test]
    fn viewport_requests_replace_old_work_and_remain_bounded_and_unique() {
        let queue = Queue::default();
        queue.previews((0..10_000).map(|id| (id, PathBuf::from(format!("{id}.png")))));
        assert_eq!(
            queue.pending.lock().unwrap().previews.len(),
            MAX_PENDING_PREVIEWS
        );
        let first = queue.next_preview().unwrap();
        let second = queue.next_preview().unwrap();
        queue.preview_ready(first.0, Ok(first.1.clone()));
        queue.previews([
            first,
            second,
            (999, PathBuf::from("focused.png")),
            (900, PathBuf::from("visible.png")),
            (999, PathBuf::from("duplicate.png")),
        ]);
        let next = queue.next_preview().unwrap();
        assert_eq!(next, (999, PathBuf::from("focused.png")));
        assert_eq!(queue.pending.lock().unwrap().previews.len(), 1);
        queue.previews([]);
        assert!(queue.pending.lock().unwrap().previews.is_empty());
        queue.stop();
        assert!(queue.next_preview().is_none());
    }

    #[test]
    fn apply_and_its_result_bypass_two_blocked_decoders_and_preview_backlog() {
        let gate = Arc::new(Gate::default());
        let decoder_gate = gate.clone();
        let (started, decoding) = mpsc::channel();
        let (applied, applications) = mpsc::channel();
        let workers = Workers::start(
            |_| collection(),
            move |path| {
                started.send(()).unwrap();
                decoder_gate.wait();
                Ok(path.to_path_buf())
            },
            move |path| {
                applied.send(path.to_path_buf()).unwrap();
                Err("renderer refused image".into())
            },
        );
        let _release = Release(gate);
        wait_event(&workers, |event| matches!(event, Event::Catalog(Ok(_))));
        workers.previews((0..100).map(|id| (id, PathBuf::from(format!("{id}.png")))));
        for _ in 0..PREVIEW_WORKERS {
            decoding.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let selected = PathBuf::from("selected.png");
        assert!(workers.apply(selected.clone(), true));
        assert!(!workers.apply(PathBuf::from("second.png"), false));
        assert_eq!(
            applications.recv_timeout(Duration::from_secs(5)).unwrap(),
            selected
        );
        let event = wait_event(&workers, |event| matches!(event, Event::Applied(..)));
        assert!(
            matches!(event, Event::Applied(path, true, Err(error)) if path == selected && error == "renderer refused image")
        );
        assert!(workers.queue.pending.lock().unwrap().ready.is_empty());
    }

    #[test]
    fn completed_previews_have_backpressure_and_shutdown_wakes_blocked_producers() {
        let queue = Arc::new(Queue::default());
        for id in 0..PREVIEW_WORKERS {
            queue.preview_ready(id, Ok(PathBuf::from("preview.png")));
        }
        let producer_queue = queue.clone();
        let (started, producing) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let producer = thread::spawn(move || {
            started.send(()).unwrap();
            producer_queue.preview_ready(42, Ok(PathBuf::from("extra.png")));
            done.send(()).unwrap();
        });
        producing.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(queue.pending.lock().unwrap().ready.len(), PREVIEW_WORKERS);
        queue.stop();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        producer.join().unwrap();
        assert_eq!(queue.pending.lock().unwrap().ready.len(), PREVIEW_WORKERS);
    }

    #[test]
    fn shutdown_preserves_an_accepted_apply_and_discards_pending_previews() {
        let queue = Queue::default();
        queue.previews([(1, PathBuf::from("preview.png"))]);
        assert!(queue.apply(PathBuf::from("selected.png"), false));
        queue.stop();
        assert_eq!(
            queue.next_apply(),
            Some((PathBuf::from("selected.png"), false))
        );
        assert!(queue.next_apply().is_none());
        assert!(queue.next_preview().is_none());
        assert!(!queue.apply(PathBuf::from("late.png"), true));
    }
}
